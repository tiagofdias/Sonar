//! Local desktop state under %LOCALAPPDATA%\Sonar. The identity key and the
//! session cookie are protected with Windows DPAPI for the current user; the
//! contact-key trust list and preferences are not secret.
use p256::SecretKey;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub fn root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Sonar")
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub server: String,
    pub input: Option<String>,
    pub output: Option<String>,
    pub relay_only: bool,
    /// Stored inverted so that it defaults to on for existing settings files.
    pub noise_suppression_off: bool,
}
pub fn load_settings() -> Settings {
    std::fs::read(root().join("settings.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}
pub fn save_settings(settings: &Settings) {
    let _ = std::fs::create_dir_all(root());
    if let Ok(bytes) = serde_json::to_vec_pretty(settings) { let _ = std::fs::write(root().join("settings.json"), bytes); }
}

// ----- Identity key -----------------------------------------------------

#[cfg(windows)]
pub fn load_key(origin: &str, account: &str, public_key: &str) -> Option<SecretKey> {
    crate::key_store::load_matching(&root().join("keys"), origin, account, public_key).ok()
}
#[cfg(windows)]
pub fn save_key(origin: &str, account: &str, key: &SecretKey) -> Result<(), String> {
    crate::key_store::save_replace(&root().join("keys"), origin, account, key).map_err(|_| "Could not protect the encryption key on this computer.".into())
}
#[cfg(not(windows))]
pub fn load_key(_: &str, _: &str, _: &str) -> Option<SecretKey> { None }
#[cfg(not(windows))]
pub fn save_key(_: &str, _: &str, _: &SecretKey) -> Result<(), String> { Ok(()) }

// ----- Remembered session -------------------------------------------------

#[derive(Serialize, Deserialize)]
struct SavedSession { server: String, cookie: String }
const SESSION_SCOPE: &str = "sonar-desktop-session-v1";

#[cfg(windows)]
pub fn save_session(server: &str, cookie: &str) {
    let Ok(bytes) = serde_json::to_vec(&SavedSession { server: server.into(), cookie: cookie.into() }) else { return };
    let bytes = zeroize::Zeroizing::new(bytes);
    if let Ok(protected) = crate::key_store::crypt(&bytes, SESSION_SCOPE, true) {
        let _ = std::fs::create_dir_all(root());
        let _ = std::fs::write(root().join("session.bin"), &protected[..]);
    }
}
#[cfg(windows)]
pub fn load_session() -> Option<(String, String)> {
    let protected = std::fs::read(root().join("session.bin")).ok()?;
    let plain = crate::key_store::crypt(&protected, SESSION_SCOPE, false).ok()?;
    let saved: SavedSession = serde_json::from_slice(&plain).ok()?;
    Some((saved.server, saved.cookie))
}
#[cfg(not(windows))]
pub fn save_session(_: &str, _: &str) {}
#[cfg(not(windows))]
pub fn load_session() -> Option<(String, String)> { None }
pub fn forget_session() { let _ = std::fs::remove_file(root().join("session.bin")); }

// ----- Contact key trust (trust on first use, like the web client) --------

fn trust_path(origin: &str, account: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    root().join("trust").join(format!("{:x}.json", Sha256::digest(format!("sonar-trust-v1\0{origin}\0{account}").as_bytes())))
}
#[derive(Default)]
pub struct Trust { path: PathBuf, keys: HashMap<String, String> }
impl Trust {
    pub fn open(origin: &str, account: &str) -> Self {
        let path = trust_path(origin, account);
        let keys = std::fs::read(&path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        Self { path, keys }
    }
    fn save(&self) {
        if let Some(parent) = self.path.parent() { let _ = std::fs::create_dir_all(parent); }
        if let Ok(bytes) = serde_json::to_vec(&self.keys) { let _ = std::fs::write(&self.path, bytes); }
    }
    /// Records the first key seen for a contact; returns false if it changed.
    pub fn remember(&mut self, peer: &str, key: &str) -> bool {
        match self.keys.get(peer) {
            Some(saved) => saved == key,
            None => { self.keys.insert(peer.into(), key.into()); self.save(); true }
        }
    }
    pub fn saved(&self, peer: &str) -> Option<&str> { self.keys.get(peer).map(String::as_str) }
    pub fn accept(&mut self, peer: &str, key: &str) { self.keys.insert(peer.into(), key.into()); self.save(); }
}

// ----- Read positions (offline unread indicators, like the web client) -----

/// Latest message seen per conversation: (createdAt in ms, message id). No
/// message content is stored.
pub struct ReadPositions { path: PathBuf, positions: HashMap<String, (i64, String)> }
impl Default for ReadPositions { fn default() -> Self { Self { path: PathBuf::new(), positions: HashMap::new() } } }
impl ReadPositions {
    pub fn open(origin: &str, account: &str) -> Self {
        use sha2::{Digest, Sha256};
        let path = root().join("unread").join(format!("{:x}.json", Sha256::digest(format!("sonar-unread-v1\0{origin}\0{account}").as_bytes())));
        let positions = std::fs::read(&path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        Self { path, positions }
    }
    pub fn newer(&self, key: &str, position: &(i64, String)) -> bool {
        match self.positions.get(key) { Some(seen) => position > seen, None => true }
    }
    pub fn remember(&mut self, key: &str, position: (i64, String)) {
        if self.path.as_os_str().is_empty() || !self.newer(key, &position) { return; }
        self.positions.insert(key.into(), position);
        if let Some(parent) = self.path.parent() { let _ = std::fs::create_dir_all(parent); }
        if let Ok(bytes) = serde_json::to_vec(&self.positions) { let _ = std::fs::write(&self.path, bytes); }
    }
}
