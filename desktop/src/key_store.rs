// Experimental, explicit key storage. Never creates or replaces account identities automatically.
#![allow(dead_code)]
use p256::SecretKey;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};
use std::{fs::{self, OpenOptions}, io::{Read, Write}, path::{Path, PathBuf}, ptr};
use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN}};
use zeroize::{Zeroize, Zeroizing};

fn scope_name(origin: &str, account: &str) -> Result<String, &'static str> {
    let url = crate::api::server_url(origin).map_err(|_| "invalid origin")?;
    if account.len() != 24 || !account.bytes().all(|byte| byte.is_ascii_hexdigit()) { return Err("invalid account"); }
    let input = format!("sonar-desktop-key-v1\0{}\0{}", url.origin().ascii_serialization(), account.to_ascii_lowercase());
    Ok(format!("{:x}.key", Sha256::digest(input.as_bytes())))
}
pub(crate) fn crypt(input: &[u8], scope: &str, protect: bool) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    if input.is_empty() || input.len() > 4096 { return Err("invalid key data"); }
    let blob = CRYPT_INTEGER_BLOB { cbData: input.len() as u32, pbData: input.as_ptr() as *mut u8 };
    let entropy = CRYPT_INTEGER_BLOB { cbData: scope.len() as u32, pbData: scope.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: ptr::null_mut() };
    // DPAPI allocates output; always clear and release it before returning.
    let ok = unsafe {
        if protect { CryptProtectData(&blob, ptr::null(), &entropy, ptr::null(), ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output) }
        else { CryptUnprotectData(&blob, ptr::null_mut(), &entropy, ptr::null(), ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output) }
    };
    if ok == 0 { return Err("key protection failed"); }
    if output.pbData.is_null() || output.cbData == 0 {
        if !output.pbData.is_null() { unsafe { LocalFree(output.pbData.cast()); } }
        return Err("key protection returned invalid data");
    }
    let bytes = unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
    let result = Zeroizing::new(bytes.to_vec());
    bytes.zeroize();
    unsafe { LocalFree(output.pbData.cast()); }
    Ok(result)
}
fn key_path(root: &Path, origin: &str, account: &str) -> Result<PathBuf, &'static str> {
    Ok(root.join(scope_name(origin, account)?))
}
pub fn save_new(root: &Path, origin: &str, account: &str, key: &SecretKey) -> Result<(), &'static str> {
    let path = key_path(root, origin, account)?;
    let secret = Zeroizing::new(key.to_bytes());
    let scope = scope_name(origin, account)?;
    let encrypted = crypt(&secret[..], &scope, true)?;
    fs::create_dir_all(root).map_err(|_| "key directory unavailable")?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path).map_err(|_| "key already exists or cannot be saved")?;
    file.write_all(&encrypted).and_then(|_| file.sync_all()).map_err(|_| "key could not be saved")
}
/// Stores a key unlocked from the encrypted backup or newly rotated, replacing
/// an older one atomically (write to a temporary file, then rename).
pub fn save_replace(root: &Path, origin: &str, account: &str, key: &SecretKey) -> Result<(), &'static str> {
    let path = key_path(root, origin, account)?;
    let secret = Zeroizing::new(key.to_bytes());
    let scope = scope_name(origin, account)?;
    let encrypted = crypt(&secret[..], &scope, true)?;
    fs::create_dir_all(root).map_err(|_| "key directory unavailable")?;
    let temporary = path.with_extension("tmp");
    let mut file = OpenOptions::new().write(true).create(true).truncate(true).open(&temporary).map_err(|_| "key could not be saved")?;
    file.write_all(&encrypted).and_then(|_| file.sync_all()).map_err(|_| "key could not be saved")?;
    drop(file);
    fs::rename(&temporary, &path).map_err(|_| "key could not be saved")
}
pub fn load(root: &Path, origin: &str, account: &str) -> Result<SecretKey, &'static str> {
    let path = key_path(root, origin, account)?;
    let file = fs::File::open(path).map_err(|_| "key unavailable")?;
    let mut encrypted = Vec::new();
    file.take(4097).read_to_end(&mut encrypted).map_err(|_| "key could not be read")?;
    let scope = scope_name(origin, account)?;
    let secret = crypt(&encrypted, &scope, false)?;
    if secret.len() != 32 { return Err("invalid key data"); }
    SecretKey::from_slice(&secret).map_err(|_| "invalid key data")
}
// The server's public key is only an identity consistency check, not proof of
// authenticity. Callers must separately verify identities and device trust.
pub fn load_matching(root: &Path, origin: &str, account: &str, expected_public: &str) -> Result<SecretKey, &'static str> {
    let key = load(root, origin, account)?;
    let registered = STANDARD.decode(expected_public).map_err(|_| "invalid registered public key")?;
    let actual = key.public_key().to_encoded_point(false);
    if registered.as_slice() != actual.as_bytes() { return Err("local key does not match account identity"); }
    Ok(key)
}
#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::{OsRng, RngCore};
    use p256::pkcs8::DecodePrivateKey;
    #[test]
    fn webcrypto_fixture_matches_locally_protected_identity() {
        let mut random = [0u8; 16]; OsRng.fill_bytes(&mut random);
        let root = std::env::temp_dir().join(format!("sonar-key-fixture-{:x}", u128::from_le_bytes(random)));
        let account = "0123456789abcdef01234567";
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../tests/webcrypto-fixture.json")).unwrap();
        let private = STANDARD.decode(fixture["alicePrivate"].as_str().unwrap()).unwrap();
        let key = SecretKey::from_pkcs8_der(&private).unwrap();
        save_new(&root, "http://127.0.0.1:5000", account, &key).unwrap();
        let loaded = load_matching(&root, "http://127.0.0.1:5000", account, fixture["alicePublic"].as_str().unwrap()).unwrap();
        assert_eq!(loaded.to_bytes(), key.to_bytes());
        assert!(load_matching(&root, "http://127.0.0.1:5000", account, fixture["bobPublic"].as_str().unwrap()).is_err());
        fs::remove_file(key_path(&root, "http://127.0.0.1:5000", account).unwrap()).unwrap();
        fs::remove_dir(root).unwrap();
    }
    #[test]
    fn user_protected_key_round_trips_and_cannot_be_overwritten() {
        let mut random = [0u8; 16]; OsRng.fill_bytes(&mut random);
        let root = std::env::temp_dir().join(format!("sonar-key-test-{:x}", u128::from_le_bytes(random)));
        let account = "0123456789abcdef01234567";
        let key = SecretKey::random(&mut OsRng);
        save_new(&root, "http://127.0.0.1:5000", account, &key).unwrap();
        let path = key_path(&root, "http://127.0.0.1:5000", account).unwrap();
        assert_ne!(fs::read(&path).unwrap(), key.to_bytes().to_vec());
        assert_eq!(load(&root, "http://127.0.0.1:5000", account).unwrap().to_bytes(), key.to_bytes());
        let expected = STANDARD.encode(key.public_key().to_encoded_point(false).as_bytes());
        assert_eq!(load_matching(&root, "http://127.0.0.1:5000", account, &expected).unwrap().to_bytes(), key.to_bytes());
        let other = SecretKey::random(&mut OsRng);
        let other_public = STANDARD.encode(other.public_key().to_encoded_point(false).as_bytes());
        assert!(load_matching(&root, "http://127.0.0.1:5000", account, &other_public).is_err());
        assert!(load_matching(&root, "http://127.0.0.1:5000", account, "invalid").is_err());
        assert!(save_new(&root, "http://127.0.0.1:5000", account, &SecretKey::random(&mut OsRng)).is_err());
        let alternate = key_path(&root, "http://127.0.0.1:5001", account).unwrap();
        fs::copy(&path, &alternate).unwrap();
        assert!(load(&root, "http://127.0.0.1:5001", account).is_err());
        fs::remove_file(alternate).unwrap();
        fs::write(&path, b"not protected data").unwrap();
        assert!(load(&root, "http://127.0.0.1:5000", account).is_err());
        fs::remove_file(path).unwrap(); fs::remove_dir(root).unwrap();
    }
}
