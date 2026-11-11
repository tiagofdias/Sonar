//! Identity key backups, byte-compatible with frontend/src/lib/keyBackup.js.
//!
//! v2 (current) - recovery key. The user saves a random 120-bit recovery key;
//! the server, which sees the password at sign-in, can never unwrap it:
//!   wrapping key = HMAC-SHA256(recoveryKey, "sonar-recovery-wrap-v2:<accountId>")
//!   ciphertext   = AES-256-GCM(PKCS#8), AAD "sonar-key-backup-v2:<accountId>:<publicKey>"
//!   reset proof  = HMAC-SHA256(recoveryKey, "sonar-recovery-proof-v2"); server keeps SHA-256(proof)
//!
//! v1 (legacy) - password, unlocked once and then moved to v2:
//!   wrapping key = PBKDF2-HMAC-SHA256(password, 16-byte salt, 600000) -> AES-256
//!   ciphertext   = AES-256-GCM(PKCS#8 private key), 12-byte IV, 128-bit tag
//!   AAD          = "sonar-key-backup-v1:<accountId>:<base64 raw public key>"
//! The server only ever receives the result.
use aes_gcm::{aead::{Aead, Payload}, Aes256Gcm, KeyInit, Nonce};
use base64::{engine::general_purpose::STANDARD, Engine};
use hmac::{Hmac, Mac};
use p256::{elliptic_curve::sec1::ToSec1Point, pkcs8::{DecodePrivateKey, EncodePrivateKey}, SecretKey};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::{Zeroize, Zeroizing};

pub const VERSION: u32 = 1;
pub const RECOVERY_VERSION: u32 = 2;
pub const ITERATIONS: u32 = 600_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub version: u32,
    pub public_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub salt: Option<String>,
    pub iv: String,
    pub ciphertext: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iterations: Option<u32>,
}

pub const WRONG_PASSWORD: &str = "That password could not unlock your encrypted messages.";

pub fn public_key_text(key: &SecretKey) -> String {
    STANDARD.encode(key.public_key().to_sec1_point(false).as_bytes())
}

/// PBKDF2-HMAC-SHA256 with a single 32-byte output block (RFC 8018).
fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32) -> Zeroizing<[u8; 32]> {
    let mac = <Hmac::<Sha256> as Mac>::new_from_slice(password).expect("HMAC accepts any key length");
    let mut block = mac.clone();
    block.update(salt);
    block.update(&1u32.to_be_bytes());
    let mut u: [u8; 32] = block.finalize().into_bytes().into();
    let mut output = Zeroizing::new(u);
    for _ in 1..iterations {
        let mut next = mac.clone();
        next.update(&u);
        u = next.finalize().into_bytes().into();
        for (out, byte) in output.iter_mut().zip(u.iter()) { *out ^= byte; }
    }
    u.zeroize();
    output
}

fn cipher(password: &str, salt: &[u8], iterations: u32) -> Result<Aes256Gcm, &'static str> {
    if password.is_empty() { return Err("Enter your password."); }
    let key = pbkdf2_sha256(password.as_bytes(), salt, iterations);
    Aes256Gcm::new_from_slice(&key[..]).map_err(|_| "Could not prepare the key.")
}

fn aad(account_id: &str, public_key: &str) -> String { format!("sonar-key-backup-v1:{account_id}:{public_key}") }

fn wrap_with(key: &SecretKey, password: &str, account_id: &str, salt: [u8; 16], iv: [u8; 12]) -> Result<Backup, &'static str> {
    let public_key = public_key_text(key);
    let pkcs8 = key.to_pkcs8_der().map_err(|_| "Could not export the key.")?;
    let ciphertext = cipher(password, &salt, ITERATIONS)?
        .encrypt(&Nonce::from(iv), Payload { msg: pkcs8.as_bytes(), aad: aad(account_id, &public_key).as_bytes() })
        .map_err(|_| "Could not encrypt the key.")?;
    Ok(Backup { version: VERSION, public_key, salt: Some(STANDARD.encode(salt)), iv: STANDARD.encode(iv), ciphertext: STANDARD.encode(ciphertext), iterations: Some(ITERATIONS) })
}

/// Encrypts the private key with a fresh salt and IV. CPU-heavy: run off the UI thread.
pub fn wrap(key: &SecretKey, password: &str, account_id: &str) -> Result<Backup, &'static str> {
    let (mut salt, mut iv) = ([0u8; 16], [0u8; 12]);
    OsRng.try_fill_bytes(&mut salt).map_err(|_| "Random source unavailable.")?;
    OsRng.try_fill_bytes(&mut iv).map_err(|_| "Random source unavailable.")?;
    wrap_with(key, password, account_id, salt, iv)
}

/// Decrypts a backup and checks the key matches its published public key.
/// CPU-heavy: run off the UI thread.
pub fn unwrap(backup: &Backup, password: &str, account_id: &str) -> Result<SecretKey, &'static str> {
    if backup.version != VERSION || backup.iterations != Some(ITERATIONS) { return Err("This encrypted key needs a newer version of Sonar."); }
    let salt = STANDARD.decode(backup.salt.as_deref().unwrap_or_default()).map_err(|_| "Invalid encrypted key.")?;
    let iv: [u8; 12] = STANDARD.decode(&backup.iv).ok().and_then(|value| value.try_into().ok()).ok_or("Invalid encrypted key.")?;
    let ciphertext = STANDARD.decode(&backup.ciphertext).map_err(|_| "Invalid encrypted key.")?;
    if salt.len() != 16 { return Err("Invalid encrypted key."); }
    let pkcs8 = Zeroizing::new(cipher(password, &salt, ITERATIONS)?
        .decrypt(&Nonce::from(iv), Payload { msg: &ciphertext, aad: aad(account_id, &backup.public_key).as_bytes() })
        .map_err(|_| WRONG_PASSWORD)?);
    let key = SecretKey::from_pkcs8_der(&pkcs8).map_err(|_| "Invalid encrypted key.")?;
    if public_key_text(&key) != backup.public_key { return Err("The encrypted key does not match this account."); }
    Ok(key)
}

/// Re-protects a backup under a new password (used when changing password).
pub fn rewrap(backup: &Backup, current: &str, next: &str, account_id: &str) -> Result<Backup, &'static str> {
    let key = unwrap(backup, current, account_id)?;
    wrap(&key, next, account_id)
}

// ----- v2: recovery key -------------------------------------------------------

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
pub const WRONG_RECOVERY_KEY: &str = "That recovery key is not correct.";

/// A recovery key as shown to the user, e.g. "K7QF-M2XR-...".
pub struct RecoveryKey(Zeroizing<[u8; 15]>);
impl RecoveryKey {
    pub fn generate() -> Result<Self, &'static str> {
        let mut bytes = Zeroizing::new([0u8; 15]);
        OsRng.try_fill_bytes(&mut bytes[..]).map_err(|_| "Random source unavailable.")?;
        Ok(Self(bytes))
    }
    pub fn parse(text: &str) -> Result<Self, &'static str> {
        let clean: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b'-').map(|b| match b.to_ascii_uppercase() { b'O' => b'0', b'I' | b'L' => b'1', other => other }).collect();
        if clean.len() != 24 { return Err(WRONG_RECOVERY_KEY); }
        let mut out = Zeroizing::new([0u8; 15]);
        let (mut value, mut bits, mut index) = (0u32, 0, 0);
        for byte in clean {
            let digit = ALPHABET.iter().position(|a| *a == byte).ok_or(WRONG_RECOVERY_KEY)? as u32;
            value = (value << 5) | digit; bits += 5;
            if bits >= 8 { out[index] = (value >> (bits - 8)) as u8; index += 1; bits -= 8; value &= (1 << bits) - 1; }
        }
        Ok(Self(out))
    }
    pub fn text(&self) -> Zeroizing<String> {
        let mut chars = String::with_capacity(29);
        let (mut value, mut bits) = (0u32, 0);
        for byte in self.0.iter() {
            value = (value << 8) | u32::from(*byte); bits += 8;
            while bits >= 5 { chars.push(ALPHABET[((value >> (bits - 5)) & 31) as usize] as char); bits -= 5; }
            value &= (1 << bits) - 1;
        }
        let groups: Vec<&str> = (0..6).map(|i| &chars[i * 4..i * 4 + 4]).collect();
        Zeroizing::new(groups.join("-"))
    }
    fn hmac(&self, label: &str) -> Zeroizing<[u8; 32]> {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.0[..]).expect("HMAC accepts any key length");
        mac.update(label.as_bytes());
        Zeroizing::new(mac.finalize().into_bytes().into())
    }
    /// Sent to reset a forgotten password (base64url, no padding).
    pub fn proof(&self) -> String { base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&self.hmac("sonar-recovery-proof-v2")[..]) }
    /// Stored by the server to check a reset proof.
    pub fn verifier(&self) -> String {
        use sha2::Digest;
        Sha256::digest(&self.hmac("sonar-recovery-proof-v2")[..]).iter().map(|b| format!("{b:02x}")).collect()
    }
    fn cipher(&self, account_id: &str) -> Aes256Gcm {
        let key = self.hmac(&format!("sonar-recovery-wrap-v2:{account_id}"));
        Aes256Gcm::new_from_slice(&key[..]).expect("32-byte key")
    }
}
fn aad2(account_id: &str, public_key: &str) -> String { format!("sonar-key-backup-v2:{account_id}:{public_key}") }

fn wrap_recovery_with(key: &SecretKey, recovery: &RecoveryKey, account_id: &str, iv: [u8; 12]) -> Result<Backup, &'static str> {
    let public_key = public_key_text(key);
    let pkcs8 = key.to_pkcs8_der().map_err(|_| "Could not export the key.")?;
    let ciphertext = recovery.cipher(account_id)
        .encrypt(&Nonce::from(iv), Payload { msg: pkcs8.as_bytes(), aad: aad2(account_id, &public_key).as_bytes() })
        .map_err(|_| "Could not encrypt the key.")?;
    Ok(Backup { version: RECOVERY_VERSION, public_key, salt: None, iv: STANDARD.encode(iv), ciphertext: STANDARD.encode(ciphertext), iterations: None })
}
/// Protects the private key with the recovery key.
pub fn wrap_recovery(key: &SecretKey, recovery: &RecoveryKey, account_id: &str) -> Result<Backup, &'static str> {
    let mut iv = [0u8; 12];
    OsRng.try_fill_bytes(&mut iv).map_err(|_| "Random source unavailable.")?;
    wrap_recovery_with(key, recovery, account_id, iv)
}
pub fn unwrap_recovery(backup: &Backup, recovery: &RecoveryKey, account_id: &str) -> Result<SecretKey, &'static str> {
    if backup.version != RECOVERY_VERSION { return Err("This encrypted key needs a newer version of Sonar."); }
    let iv: [u8; 12] = STANDARD.decode(&backup.iv).ok().and_then(|value| value.try_into().ok()).ok_or("Invalid encrypted key.")?;
    let ciphertext = STANDARD.decode(&backup.ciphertext).map_err(|_| "Invalid encrypted key.")?;
    let pkcs8 = Zeroizing::new(recovery.cipher(account_id)
        .decrypt(&Nonce::from(iv), Payload { msg: &ciphertext, aad: aad2(account_id, &backup.public_key).as_bytes() })
        .map_err(|_| WRONG_RECOVERY_KEY)?);
    let key = SecretKey::from_pkcs8_der(&pkcs8).map_err(|_| "Invalid encrypted key.")?;
    if public_key_text(&key) != backup.public_key { return Err("The encrypted key does not match this account."); }
    Ok(key)
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    #[test]
    fn matches_the_web_recovery_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../tests/recovery-fixture.json")).unwrap();
        let text = fixture["recoveryKey"].as_str().unwrap();
        let account = fixture["accountId"].as_str().unwrap();
        let recovery = RecoveryKey::parse(text).unwrap();
        assert_eq!(&*recovery.text(), text);
        assert_eq!(&*RecoveryKey::parse(&text.to_lowercase().replace('-', " ")).unwrap().text(), text);
        assert_eq!(recovery.proof(), fixture["proof"].as_str().unwrap());
        assert_eq!(recovery.verifier(), fixture["verifier"].as_str().unwrap());
        let backup: Backup = serde_json::from_value(fixture["backup"].clone()).unwrap();
        let key = unwrap_recovery(&backup, &recovery, account).unwrap();
        let expected = SecretKey::from_pkcs8_der(&STANDARD.decode(fixture["pkcs8"].as_str().unwrap()).unwrap()).unwrap();
        assert_eq!(key.to_bytes(), expected.to_bytes());
        let iv: [u8; 12] = STANDARD.decode(&backup.iv).unwrap().try_into().unwrap();
        assert_eq!(wrap_recovery_with(&key, &recovery, account, iv).unwrap(), backup);
        assert_eq!(serde_json::to_value(&backup).unwrap(), fixture["backup"], "no v1 fields are sent");
    }
    #[test]
    fn rejects_wrong_keys_and_accounts() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../tests/recovery-fixture.json")).unwrap();
        let backup: Backup = serde_json::from_value(fixture["backup"].clone()).unwrap();
        let other = RecoveryKey::generate().unwrap();
        assert_eq!(unwrap_recovery(&backup, &other, fixture["accountId"].as_str().unwrap()).unwrap_err(), WRONG_RECOVERY_KEY);
        let right = RecoveryKey::parse(fixture["recoveryKey"].as_str().unwrap()).unwrap();
        assert_eq!(unwrap_recovery(&backup, &right, "abcdef0123456789abcdef01").unwrap_err(), WRONG_RECOVERY_KEY);
        assert!(RecoveryKey::parse("ABCD-EFGH").is_err());
        assert!(RecoveryKey::parse("UUUU-UUUU-UUUU-UUUU-UUUU-UUUU").is_err());
        let fresh = RecoveryKey::generate().unwrap();
        assert_eq!(RecoveryKey::parse(&fresh.text()).unwrap().verifier(), fresh.verifier());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::elliptic_curve::Generate;
    fn fixture() -> serde_json::Value { serde_json::from_str(include_str!("../tests/key-backup-fixture.json")).unwrap() }

    #[test]
    fn pbkdf2_matches_rfc_7914_vectors() {
        let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(hex(&pbkdf2_sha256(b"passwd", b"salt", 1)[..]), "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc");
        assert_eq!(hex(&pbkdf2_sha256(b"Password", b"NaCl", 80000)[..]), "4ddcd8f60b98be21830cee5ef22701f9641a4418d04c0414aeff08876b34ab56");
    }

    #[test]
    fn unwraps_web_backup_and_produces_identical_ciphertext() {
        let fixture = fixture();
        let backup: Backup = serde_json::from_value(fixture["backup"].clone()).unwrap();
        let password = fixture["password"].as_str().unwrap();
        let account = fixture["accountId"].as_str().unwrap();
        let key = unwrap(&backup, password, account).unwrap();
        let expected = SecretKey::from_pkcs8_der(&STANDARD.decode(fixture["pkcs8"].as_str().unwrap()).unwrap()).unwrap();
        assert_eq!(key.to_bytes(), expected.to_bytes());
        let salt: [u8; 16] = STANDARD.decode(backup.salt.as_deref().unwrap()).unwrap().try_into().unwrap();
        let iv: [u8; 12] = STANDARD.decode(&backup.iv).unwrap().try_into().unwrap();
        // The web export carries the same PKCS#8 encoding p256 produces, so the
        // ciphertext must match byte for byte.
        assert_eq!(wrap_with(&key, password, account, salt, iv).unwrap(), backup);
    }

    #[test]
    fn rejects_wrong_password_account_and_tampering() {
        let fixture = fixture();
        let backup: Backup = serde_json::from_value(fixture["backup"].clone()).unwrap();
        let password = fixture["password"].as_str().unwrap();
        let account = fixture["accountId"].as_str().unwrap();
        assert_eq!(unwrap(&backup, "wrong password", account).unwrap_err(), WRONG_PASSWORD);
        assert_eq!(unwrap(&backup, password, "abcdef0123456789abcdef01").unwrap_err(), WRONG_PASSWORD);
        let mut swapped = backup.clone();
        swapped.public_key = public_key_text(&SecretKey::generate());
        assert!(unwrap(&swapped, password, account).is_err());
        let mut weak = backup.clone();
        weak.iterations = Some(1000);
        assert!(unwrap(&weak, password, account).is_err());
        let rewrapped = rewrap(&backup, password, "a brand new password", account).unwrap();
        assert_ne!(rewrapped.salt, backup.salt);
        assert_eq!(unwrap(&rewrapped, password, account).unwrap_err(), WRONG_PASSWORD);
        assert_eq!(unwrap(&rewrapped, "a brand new password", account).unwrap().to_bytes(), unwrap(&backup, password, account).unwrap().to_bytes());
    }
}
