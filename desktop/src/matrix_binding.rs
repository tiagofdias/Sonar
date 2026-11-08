//! Narrow proof of possession for an existing Matrix device. This proves key
//! possession only and does not establish device approval or user trust.
#![allow(dead_code)]
use matrix_sdk_crypto::OlmMachine;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(serde::Serialize)]
struct EnrollmentKeys<'a> {
    user_id: &'a str,
    device_id: &'a str,
    algorithms: &'a serde_json::Value,
    keys: BTreeMap<String, String>,
    signatures: BTreeMap<String, BTreeMap<String, String>>,
}

pub async fn sign_enrollment(machine: &OlmMachine, user: &str, device: &str, keys: &serde_json::Value, token: &str, message: &str, expires_unix_ms: u64) -> Result<String, String> {
    let invalid = || "Desafio de registo invalido.".to_string();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| invalid())?.as_millis();
    if machine.user_id().as_str() != user || machine.device_id().as_str() != device || token.len() != 43 || !token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') || u128::from(expires_unix_ms) <= now || u128::from(expires_unix_ms) > now + 180_000 || keys["user_id"] != user || keys["device_id"] != device { return Err(invalid()); }
    let ed = format!("ed25519:{device}"); let curve = format!("curve25519:{device}");
    let algorithms = keys["algorithms"].as_array().ok_or_else(invalid)?;
    if algorithms.len() != 2 || !algorithms.contains(&serde_json::json!("m.olm.v1.curve25519-aes-sha2")) || !algorithms.contains(&serde_json::json!("m.megolm.v1.aes-sha2")) { return Err(invalid()); }
    let own = machine.identity_keys();
    let ed_value = keys["keys"][&ed].as_str().ok_or_else(invalid)?;
    let curve_value = keys["keys"][&curve].as_str().ok_or_else(invalid)?;
    if ed_value != own.ed25519.to_base64() || curve_value != own.curve25519.to_base64() { return Err(invalid()); }
    let signature = keys["signatures"][user][&ed].as_str().ok_or_else(invalid)?;
    let normalized = EnrollmentKeys { user_id: user, device_id: device, algorithms: &keys["algorithms"], keys: BTreeMap::from([(curve, curve_value.to_owned()), (ed.clone(), ed_value.to_owned())]), signatures: BTreeMap::from([(user.to_owned(), BTreeMap::from([(ed, signature.to_owned())]))]) };
    let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&normalized).map_err(|_| invalid())?));
    let parts: Vec<_> = message.split('\n').collect();
    if parts.len() != 6 || parts[0] != "sonar-matrix-device-enrollment-v1" || parts[1] != user || parts[2] != device || parts[3].len() != 64 || !parts[3].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) || parts[4] != digest || parts[5] != token { return Err(invalid()); }
    let signatures = serde_json::to_value(machine.sign(message).await.map_err(|_| invalid())?).map_err(|_| invalid())?;
    let result = signatures.get(user).and_then(|s| s.get(format!("ed25519:{device}"))).and_then(|s| s.as_str()).ok_or_else(invalid)?;
    if result.len() != 86 || !result.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/') { return Err(invalid()); }
    Ok(result.to_owned())
}

pub async fn sign_binding(machine: &OlmMachine, user: &str, device: &str, token: &str, message: &str, expires_unix_ms: u64) -> Result<String, String> {
    let invalid = || "Desafio de dispositivo invalido.".to_string();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| invalid())?.as_millis();
    if machine.user_id().as_str() != user || machine.device_id().as_str() != device || token.len() != 43 || !token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') || u128::from(expires_unix_ms) <= now || u128::from(expires_unix_ms) > now + 180_000 { return Err(invalid()); }
    let parts: Vec<_> = message.split('\n').collect();
    if parts.len() != 5 || parts[0] != "sonar-matrix-device-binding-v1" || parts[1] != user || parts[2] != device || parts[3].len() != 64 || !parts[3].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) || parts[4] != token { return Err(invalid()); }
    let signatures = serde_json::to_value(machine.sign(message).await.map_err(|_| "Nao foi possivel assinar o desafio.")?).map_err(|_| invalid())?;
    let signature = signatures.get(user).and_then(|s| s.get(format!("ed25519:{device}"))).and_then(|s| s.as_str()).ok_or_else(invalid)?;
    if signature.len() != 86 || !signature.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/') { return Err(invalid()); }
    Ok(signature.to_owned())
}
