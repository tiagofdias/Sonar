// Compatibility with Sonar's existing v1 envelopes. This is not a new protocol.
// Key storage, trust, recovery, forward secrecy and device linking are not implemented.
#![allow(dead_code)]
use aes_gcm::{aead::{Aead, Payload}, Aes256Gcm, KeyInit, Nonce};
use base64::{engine::general_purpose::STANDARD, Engine};
use p256::{ecdh::diffie_hellman, PublicKey, SecretKey};
use rand_core::{OsRng, RngCore};

pub fn fingerprint(encoded: &str) -> Result<String, &'static str> {
    use sha2::{Digest, Sha256};
    let bytes = STANDARD.decode(encoded).map_err(|_| "invalid public key")?;
    if bytes.len() != 65 || bytes[0] != 4 || PublicKey::from_sec1_bytes(&bytes).is_err() {
        return Err("invalid public key");
    }
    let digest = Sha256::digest(&bytes);
    Ok(digest.chunks(2).map(|part| format!("{:02x}{:02x}", part[0], part[1])).collect::<Vec<_>>().join(" "))
}
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Envelope { pub iv: String, pub ciphertext: String }
fn cipher(private: &SecretKey, peer_public: &str) -> Result<Aes256Gcm, &'static str> {
    let bytes = STANDARD.decode(peer_public).map_err(|_| "invalid public key")?;
    let public = PublicKey::from_sec1_bytes(&bytes).map_err(|_| "invalid public key")?;
    let shared = diffie_hellman(private.to_nonzero_scalar(), public.as_affine());
    Aes256Gcm::new_from_slice(shared.raw_secret_bytes()).map_err(|_| "invalid shared key")
}
const FILE_AAD: &[u8] = b"sonar-file-v1";

/// Encrypts a file with a new random key. Returns (ciphertext, key, iv) in the
/// same format the web app uses.
pub fn encrypt_file(plain: &[u8]) -> Result<(Vec<u8>, String, String), &'static str> {
    use rand_core::RngCore;
    let (mut key, mut iv) = ([0u8; 32], [0u8; 12]);
    rand_core::OsRng.fill_bytes(&mut key);
    rand_core::OsRng.fill_bytes(&mut iv);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| "invalid key")?;
    let sealed = cipher.encrypt(Nonce::from_slice(&iv), Payload { msg: plain, aad: FILE_AAD }).map_err(|_| "encryption failed")?;
    Ok((sealed, STANDARD.encode(key), STANDARD.encode(iv)))
}

pub fn decrypt_file(sealed: &[u8], key: &str, iv: &str) -> Result<Vec<u8>, &'static str> {
    let key = STANDARD.decode(key).map_err(|_| "invalid key")?;
    let iv = STANDARD.decode(iv).map_err(|_| "invalid iv")?;
    if iv.len() != 12 { return Err("invalid iv"); }
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| "invalid key")?;
    cipher.decrypt(Nonce::from_slice(&iv), Payload { msg: sealed, aad: FILE_AAD }).map_err(|_| "This file could not be decrypted")
}

pub fn context(sender: &str, receiver: &str, channel: Option<&str>) -> Result<String, &'static str> {
    fn valid(value: &str) -> bool { value.len() == 24 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) }
    if !valid(sender) || !valid(receiver) || channel.is_some_and(|value| !valid(value)) {
        return Err("invalid message scope");
    }
    Ok(match channel {
        Some(id) => format!("sonar:v1:{sender}:{receiver}:channel:{id}"),
        None => format!("sonar:v1:{sender}:{receiver}"),
    })
}
pub fn encrypt(private: &SecretKey, peer_public: &str, plaintext: &str, scope: &str) -> Result<Envelope, &'static str> {
    let mut iv = [0u8; 12];
    OsRng.try_fill_bytes(&mut iv).map_err(|_| "random source unavailable")?;
    let ciphertext = cipher(private, peer_public)?.encrypt(&Nonce::from(iv), Payload { msg: plaintext.as_bytes(), aad: scope.as_bytes() }).map_err(|_| "encryption failed")?;
    Ok(Envelope { iv: STANDARD.encode(iv), ciphertext: STANDARD.encode(ciphertext) })
}
pub fn decrypt(private: &SecretKey, peer_public: &str, envelope: &Envelope, context: &str) -> Result<String, &'static str> {
    let iv = STANDARD.decode(&envelope.iv).map_err(|_| "invalid iv")?;
    if iv.len() != 12 { return Err("invalid iv"); }
    let ciphertext = STANDARD.decode(&envelope.ciphertext).map_err(|_| "invalid ciphertext")?;
    let nonce_bytes: [u8; 12] = iv.try_into().map_err(|_| "invalid iv")?;
    let nonce = Nonce::from(nonce_bytes);
    let data = cipher(private, peer_public)?.decrypt(&nonce, Payload { msg: &ciphertext, aad: context.as_bytes() }).map_err(|_| "decryption failed")?;
    String::from_utf8(data).map_err(|_| "invalid plaintext")
}
#[cfg(test)]
mod tests {
    use super::*;
    use p256::pkcs8::DecodePrivateKey;
    #[test]
    fn fingerprint_matches_webcrypto_fixture_and_rejects_invalid_keys() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../tests/webcrypto-fixture.json")).unwrap();
        let key = fixture["alicePublic"].as_str().unwrap();
        let expected = fixture["aliceFingerprint"].as_str().unwrap();
        assert_eq!(fingerprint(key).unwrap(), expected);
        assert!(fingerprint("invalid").is_err());
        assert!(fingerprint(&STANDARD.encode([4u8; 65])).is_err());
    }
    #[test]
    fn message_context_checks_identifiers_and_binds_channels() {
        let sender = "0123456789abcdef01234567";
        let receiver = "abcdef0123456789abcdef01";
        let channel = "abcdef0123456789abcdef02";
        assert_eq!(context(sender, receiver, None).unwrap(), format!("sonar:v1:{sender}:{receiver}"));
        assert_eq!(context(sender, receiver, Some(channel)).unwrap(), format!("sonar:v1:{sender}:{receiver}:channel:{channel}"));
        assert!(context(sender, "invalid", None).is_err());
        assert!(context(sender, receiver, Some("" )).is_err());
    }
    #[test]
    fn decrypts_webcrypto_and_rejects_tampering() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../tests/webcrypto-fixture.json")).unwrap();
        let string = |name: &str| fixture[name].as_str().unwrap();
        let alice = SecretKey::from_pkcs8_der(&STANDARD.decode(string("alicePrivate")).unwrap()).unwrap();
        let bob = SecretKey::from_pkcs8_der(&STANDARD.decode(string("bobPrivate")).unwrap()).unwrap();
        let envelope = Envelope { iv: string("iv").into(), ciphertext: string("ciphertext").into() };
        let nonce_bytes: [u8; 12] = STANDARD.decode(string("iv")).unwrap().try_into().unwrap();
        let rust_ciphertext = cipher(&alice, string("bobPublic")).unwrap().encrypt(&Nonce::from(nonce_bytes), Payload { msg: string("text").as_bytes(), aad: string("context").as_bytes() }).unwrap();
        assert_eq!(STANDARD.encode(rust_ciphertext), string("ciphertext"));
        assert_eq!(decrypt(&alice, string("bobPublic"), &envelope, string("context")).unwrap(), string("text"));
        assert_eq!(decrypt(&bob, string("alicePublic"), &envelope, string("context")).unwrap(), string("text"));
        assert!(decrypt(&bob, string("alicePublic"), &envelope, "sonar:v1:wrong").is_err());
        let first = encrypt(&alice, string("bobPublic"), string("text"), string("context")).unwrap();
        let second = encrypt(&alice, string("bobPublic"), string("text"), string("context")).unwrap();
        assert_ne!(first.iv, second.iv);
        assert_eq!(decrypt(&bob, string("alicePublic"), &first, string("context")).unwrap(), string("text"));
        let mut bytes = STANDARD.decode(&envelope.ciphertext).unwrap(); bytes[0] ^= 1;
        assert!(decrypt(&bob, string("alicePublic"), &Envelope { iv: envelope.iv.clone(), ciphertext: STANDARD.encode(bytes) }, string("context")).is_err());
    }
}
