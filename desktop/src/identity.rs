//! Makes sure this computer holds the account's private key. Mirrors the web
//! client's frontend/src/lib/legacyIdentity.js:
//!   - key already protected on this computer: use it
//!   - recovery backup (v2): unlock with the recovery key
//!   - legacy password backup (v1): unlock with the password, then move to a recovery key
//!   - no key yet: create one protected by a new recovery key
use crate::api::{Account, Session};
use crate::key_backup::{self, RecoveryKey};
use p256::{elliptic_curve::Generate, SecretKey};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Needs { Recovery, Password }

#[derive(Clone, Debug)]
pub enum Problem {
    /// A backup exists: this computer needs the recovery key (or, for legacy backups, the password).
    Locked(Needs),
    /// The key only exists on another device that never uploaded a backup.
    Missing,
    Failed(String),
}

pub struct Paired {
    pub account: Account,
    pub key: SecretKey,
    /// A recovery key created during pairing; it must be shown to the user once.
    pub recovery: Option<Zeroizing<String>>,
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, &'static str> + Send + 'static) -> Result<T, Problem> {
    tokio::task::spawn_blocking(work).await.map_err(|_| Problem::Failed("The key operation was interrupted.".into()))?.map_err(|error| Problem::Failed(error.into()))
}

/// Protects `key` with a brand-new recovery key on the server (password checked there).
async fn protect(session: &Session, key: &SecretKey, password: &str) -> Result<Zeroizing<String>, String> {
    let recovery = RecoveryKey::generate()?;
    let backup = key_backup::wrap_recovery(key, &recovery, session.me())?;
    session.set_recovery(password, &backup, &recovery.verifier()).await?;
    Ok(recovery.text())
}

pub async fn ensure(session: Session, password: Option<Zeroizing<String>>, recovery: Option<Zeroizing<String>>) -> Result<Paired, Problem> {
    let account_id = session.me().to_owned();
    let origin = session.origin();
    let Some(public_key) = session.account.encryption_key.clone() else {
        // New account: create the key and its recovery key on this computer.
        let key = SecretKey::try_generate().map_err(|_| Problem::Failed("Could not create your encryption key.".into()))?;
        let recovery = RecoveryKey::generate().map_err(|e| Problem::Failed(e.into()))?;
        let backup = key_backup::wrap_recovery(&key, &recovery, &account_id).map_err(|e| Problem::Failed(e.into()))?;
        let account = session.publish_key(&backup.public_key, Some(&backup), Some(&recovery.verifier())).await.map_err(Problem::Failed)?;
        crate::store::save_key(&origin, &account_id, &key).map_err(Problem::Failed)?;
        return Ok(Paired { account, key, recovery: Some(recovery.text()) });
    };
    let backup = session.key_backup().await.map_err(Problem::Failed)?.filter(|b| b.public_key == public_key);
    if let Some(key) = crate::store::load_key(&origin, &account_id, &public_key) {
        let account = session.publish_key(&public_key, None, None).await.map_err(Problem::Failed)?;
        // Move a legacy password backup to a recovery key at a password sign-in.
        let recovery = match (&backup, &password) {
            (Some(b), Some(password)) if b.version != key_backup::RECOVERY_VERSION => protect(&session, &key, password).await.ok(),
            _ => None,
        };
        return Ok(Paired { account, key, recovery });
    }
    let Some(backup) = backup else { return Err(Problem::Missing) };
    let (key, migrate) = if backup.version == key_backup::RECOVERY_VERSION {
        let Some(text) = recovery else { return Err(Problem::Locked(Needs::Recovery)) };
        let id = account_id.clone();
        (blocking(move || key_backup::unwrap_recovery(&backup, &RecoveryKey::parse(&text)?, &id)).await?, None)
    } else {
        let Some(password) = password else { return Err(Problem::Locked(Needs::Password)) };
        let (id, secret) = (account_id.clone(), password.clone());
        (blocking(move || key_backup::unwrap(&backup, &secret, &id)).await?, Some(password))
    };
    crate::store::save_key(&origin, &account_id, &key).map_err(Problem::Failed)?;
    let account = session.publish_key(&public_key, None, None).await.map_err(Problem::Failed)?;
    let recovery = match migrate { Some(password) => Some(protect(&session, &key, &password).await.map_err(Problem::Failed)?), None => None };
    Ok(Paired { account, key, recovery })
}

/// Replaces the account key with a new one protected by a new recovery key
/// (reset on a device that lacks the key, or turning on multi-device access).
pub async fn rotate(session: Session, password: Zeroizing<String>) -> Result<Paired, String> {
    let account_id = session.me().to_owned();
    let key = SecretKey::try_generate().map_err(|_| "Could not create your encryption key.".to_string())?;
    let recovery = RecoveryKey::generate()?;
    let backup = key_backup::wrap_recovery(&key, &recovery, &account_id)?;
    let account = session.rotate_key(&password, session.account.encryption_key.as_deref(), &backup.public_key, &backup, &recovery.verifier()).await?;
    crate::store::save_key(&session.origin(), &account_id, &key)?;
    Ok(Paired { account, key, recovery: Some(recovery.text()) })
}

/// Issues a new recovery key for the key on this computer (the old one stops working).
pub async fn new_recovery(session: Session, key: std::sync::Arc<SecretKey>, password: Zeroizing<String>) -> Result<Zeroizing<String>, String> {
    protect(&session, &key, &password).await
}

/// Forgotten password: reset it with the recovery key, unlock the message key,
/// then issue a fresh recovery key (the old one was shown to the server).
pub async fn recover(server: String, email: String, recovery: Zeroizing<String>, new_password: Zeroizing<String>) -> Result<(Session, Paired), String> {
    let proof = RecoveryKey::parse(&recovery)?.proof();
    let session = crate::client::recover(server, email, proof, new_password.clone()).await?;
    let mut paired = ensure(session.clone(), Some(new_password.clone()), Some(recovery)).await.map_err(|problem| match problem {
        Problem::Failed(error) => error,
        _ => "Your messages could not be unlocked with that recovery key.".to_string(),
    })?;
    if paired.recovery.is_none() {
        paired.recovery = protect(&session, &paired.key, &new_password).await.ok();
    }
    Ok((session, paired))
}

/// Changes the password. Only legacy password backups need re-protecting.
pub async fn change_password(session: Session, current: Zeroizing<String>, next: Zeroizing<String>) -> Result<(), String> {
    let backup = session.key_backup().await?.filter(|b| b.version == key_backup::VERSION);
    let rewrapped = match backup {
        Some(backup) => {
            let (current, next, id) = (current.clone(), next.clone(), session.me().to_owned());
            Some(tokio::task::spawn_blocking(move || key_backup::rewrap(&backup, &current, &next, &id)).await.map_err(|_| "The key operation was interrupted.")?
                .map_err(|error| if error == key_backup::WRONG_PASSWORD { "Current password is incorrect" } else { error })?)
        }
        None => None,
    };
    session.set_password(&current, &next, rewrapped.as_ref()).await
}
