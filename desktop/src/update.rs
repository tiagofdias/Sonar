//! Free self-update: fetches a signed version note from the site that hosts the
//! download, stages the new program, and swaps it in when the person chooses.
use base64::Engine;
use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use p256::pkcs8::DecodePublicKey;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

const PUBLIC_KEY: &str = include_str!("../assets/update-key.pub");
const HOST: &str = "https://sonar-73yj.onrender.com";
const PROGRAM: &str = "sonar-desktop.exe";
const MAX_DOWNLOAD: u64 = 120 * 1024 * 1024;

#[derive(Deserialize)]
struct Manifest { version: String, sha256: String, signature: String }

fn base() -> Option<String> {
    #[cfg(debug_assertions)]
    return std::env::var("SONAR_DEV_UPDATE_URL").ok();
    #[cfg(not(debug_assertions))]
    Some(HOST.to_owned())
}

fn parts(version: &str) -> Option<Vec<u32>> {
    let list = version.split('.').map(|part| part.parse().ok()).collect::<Option<Vec<u32>>>()?;
    (list.len() == 3).then_some(list)
}

fn newer(candidate: &str, current: &str) -> bool {
    matches!((parts(candidate), parts(current)), (Some(a), Some(b)) if a > b)
}

fn signed(key: &VerifyingKey, manifest: &Manifest) -> bool {
    let engine = base64::engine::general_purpose::STANDARD;
    let Some(signature) = engine.decode(manifest.signature.trim()).ok().and_then(|bytes| Signature::from_slice(&bytes).ok()) else { return false };
    key.verify(format!("sonar-update\n{}\n{}", manifest.version, manifest.sha256).as_bytes(), &signature).is_ok()
}

fn release_key() -> Option<VerifyingKey> {
    let der = base64::engine::general_purpose::STANDARD.decode(PUBLIC_KEY.trim()).ok()?;
    VerifyingKey::from_public_key_der(&der).ok()
}

fn folder() -> PathBuf { crate::store::root().join("update") }
fn staged(version: &str) -> PathBuf { folder().join(format!("sonar-{version}.exe")) }

fn old_program() -> Option<PathBuf> { Some(std::env::current_exe().ok()?.with_extension("old")) }

fn unpack(archive: &Path, target: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut entry = zip.by_name(PROGRAM).map_err(|e| e.to_string())?;
    let partial = target.with_extension("part");
    let mut output = std::fs::File::create(&partial).map_err(|e| e.to_string())?;
    std::io::copy(&mut entry, &mut output).map_err(|e| e.to_string())?;
    output.flush().map_err(|e| e.to_string())?;
    drop(output);
    std::fs::rename(&partial, target).map_err(|e| e.to_string())
}

/// Returns the version that is ready to install, if there is one.
pub async fn check() -> Result<Option<String>, String> {
    if let Some(old) = old_program() { let _ = std::fs::remove_file(old); }
    let Some(base) = base() else { return Ok(None) };
    let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(std::time::Duration::from_secs(600)).build().map_err(|e| e.to_string())?;
    let manifest: Manifest = client.get(format!("{base}/downloads/latest.json")).send().await.map_err(|e| e.to_string())?
        .error_for_status().map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;
    if !newer(&manifest.version, env!("CARGO_PKG_VERSION")) { return Ok(None); }
    let key = release_key().ok_or("The update key is damaged.")?;
    if !signed(&key, &manifest) { return Err("The update signature did not match.".into()); }
    let target = staged(&manifest.version);
    if target.exists() { return Ok(Some(manifest.version)); }

    let _ = std::fs::remove_dir_all(folder());
    std::fs::create_dir_all(folder()).map_err(|e| e.to_string())?;
    let archive = folder().join("download.zip");
    let mut response = client.get(format!("{base}/downloads/Sonar-Desktop-windows-x64.zip")).send().await.map_err(|e| e.to_string())?
        .error_for_status().map_err(|e| e.to_string())?;
    let mut file = std::fs::File::create(&archive).map_err(|e| e.to_string())?;
    let (mut hash, mut size) = (Sha256::new(), 0u64);
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        size += chunk.len() as u64;
        if size > MAX_DOWNLOAD { return Err("The update is too large.".into()); }
        hash.update(&chunk);
        file.write_all(&chunk).map_err(|e| e.to_string())?;
    }
    drop(file);
    let digest: String = hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    if digest != manifest.sha256.to_lowercase() {
        let _ = std::fs::remove_file(&archive);
        return Err("The update did not match its signature.".into());
    }
    let (from, to) = (archive.clone(), target);
    tokio::task::spawn_blocking(move || unpack(&from, &to)).await.map_err(|e| e.to_string())??;
    let _ = std::fs::remove_file(archive);
    Ok(Some(manifest.version))
}

/// Swaps the staged program in and starts it. The caller then closes this one.
pub fn restart(version: &str) -> Result<(), String> {
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let old = current.with_extension("old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&current, &old).map_err(|e| e.to_string())?;
    if let Err(error) = std::fs::copy(staged(version), &current) {
        let _ = std::fs::remove_file(&current);
        let _ = std::fs::rename(&old, &current);
        return Err(error.to_string());
    }
    std::process::Command::new(&current).spawn().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(folder());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::elliptic_curve::Generate;
    use p256::ecdsa::{signature::Signer, SigningKey};

    fn manifest(key: &SigningKey, version: &str, sha256: &str) -> Manifest {
        let signature: Signature = key.sign(format!("sonar-update\n{version}\n{sha256}").as_bytes());
        Manifest { version: version.into(), sha256: sha256.into(), signature: base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()) }
    }

    #[test]
    fn only_higher_versions_count() {
        assert!(newer("0.2.0", "0.1.9"));
        assert!(newer("0.10.0", "0.9.0"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.0.9", "0.1.0"));
        assert!(!newer("../../evil", "0.1.0"));
        assert!(!newer("1.0", "0.1.0"));
    }

    #[test]
    fn signatures_cover_version_and_hash() {
        let key = SigningKey::generate();
        let public = *key.verifying_key();
        let good = manifest(&key, "1.0.0", "abc");
        assert!(signed(&public, &good));
        assert!(!signed(&public, &Manifest { version: "2.0.0".into(), sha256: good.sha256.clone(), signature: good.signature.clone() }));
        assert!(!signed(&public, &Manifest { version: good.version.clone(), sha256: "def".into(), signature: good.signature.clone() }));
        let other = SigningKey::generate();
        assert!(!signed(other.verifying_key(), &good));
    }

    #[test]
    fn the_built_in_key_is_readable() { assert!(release_key().is_some()); }

    #[test]
    fn unpacking_takes_only_the_program_from_the_archive() {
        let folder = std::env::temp_dir().join(format!("sonar-update-test-{}", std::process::id()));
        let inner = folder.join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        let target = inner.join("sonar-test.exe");
        unpack(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/update-fixture.zip")), &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"SONAR-TEST-PROGRAM
".repeat(200));
        // The archive also holds a "../evil.exe" entry; only the named program is ever read.
        assert!(!folder.join("evil.exe").exists());
        assert_eq!(std::fs::read_dir(&inner).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(folder);
    }
}
