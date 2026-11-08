use reqwest::{header, redirect::Policy, Client, Url};
use serde::Deserialize;
use std::time::Duration;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Account {
    #[serde(rename = "_id", default)]
    pub id: Option<String>,
    #[serde(rename = "fullName")]
    pub full_name: String,
    #[serde(default)]
    pub email: String,
    #[serde(rename = "encryptionKey", default)]
    pub encryption_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Person {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "fullName")]
    pub name: String,
    #[serde(rename = "encryptionKey", default)]
    pub encryption_key: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Community {
    #[serde(rename = "_id")]
    pub id: String,
    pub name: String,
    #[serde(rename = "ownerId", default)]
    pub owner_id: Option<String>,
    #[serde(default)]
    pub moderators: Vec<String>,
    #[serde(rename = "memberDetails", default)]
    pub member_details: Vec<Person>,
    #[serde(default)]
    pub channels: Vec<Channel>,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub revision: u64,
    #[serde(rename = "historyAuthors", default)]
    pub history_authors: Vec<Person>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Channel {
    #[serde(rename = "_id")]
    pub id: String,
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(rename = "voiceLocked", default)]
    pub voice_locked: bool,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedInvite { pub token: String, pub expires_at: String }
#[derive(Clone)]
pub struct Directory {
    pub people: Vec<Person>,
    pub communities: Vec<Community>,
}

// Encrypted server records; plaintext is never requested from the service.
#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageEnvelope {
    #[serde(rename = "_id")]
    pub id: String,
    pub sender_id: String,
    pub receiver_id: String,
    pub ciphertext: String,
    pub iv: String,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub community_id: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    pub created_at: String,
    pub expires_at: Option<String>,
}
#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage { pub messages: Vec<MessageEnvelope>, pub has_more: bool, pub next_cursor: Option<String> }
#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedMessage { pub id: String, pub sender_id: String, pub receiver_id: String }

#[derive(Clone)]
pub struct Session {
    pub account: Account,
    pub(crate) cookie: String,
    pub(crate) base: Url,
    pub(crate) client: Client,
}
// The cookie is a credential: keep it out of diagnostics.
impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_struct("Session").field("account", &self.account.id).field("base", &self.base.as_str()).finish_non_exhaustive() }
}
#[cfg(test)]
pub fn synthetic_session() -> Session {
    Session { account: Account { id: Some("0123456789abcdef01234567".into()), full_name: "Synthetic user".into(), ..Default::default() }, cookie: "jwt=synthetic".into(), base: Url::parse("http://127.0.0.1:5000/").unwrap(), client: Client::new() }
}
#[allow(dead_code)]
pub fn synthetic_session_at(base: &str) -> Session {
    Session { account: Account { id: Some("0123456789abcdef01234567".into()), full_name: "Synthetic user".into(), ..Default::default() }, cookie: "jwt=synthetic".into(), base: Url::parse(base).unwrap(), client: Client::new() }
}

pub fn server_url(value: &str) -> Result<Url, String> {
    let mut url = Url::parse(value.trim()).map_err(|_| "Enter a valid server address.".to_string())?;
    let local = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
    if (url.scheme() != "https" && !(url.scheme() == "http" && local))
        || !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some()
        || !matches!(url.path(), "" | "/") {
        return Err("Use https:// (or http:// for this computer only), without a path or credentials.".into());
    }
    url.set_path("/");
    Ok(url)
}

fn matrix_challenge(value: &serde_json::Value) -> Result<(&str, &str, u64), String> {
    let invalid = || "Desafio Matrix invalido.".to_string();
    let token = value["token"].as_str().ok_or_else(invalid)?;
    let message = value["message"].as_str().ok_or_else(invalid)?;
    let expiry = value["expiresAt"].as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).and_then(|date| u64::try_from(date.timestamp_millis()).ok()).ok_or_else(invalid)?;
    Ok((token, message, expiry))
}

pub async fn login(server: String, email: String, password: String) -> Result<Session, String> {
    let base = server_url(&server)?;
    let client = Client::builder().redirect(Policy::none()).timeout(Duration::from_secs(15)).build()
        .map_err(|_| "Não foi possível preparar a ligação segura.".to_string())?;
    let response = client.post(base.join("api/auth/login").map_err(|_| "Servidor inválido")?)
        .json(&serde_json::json!({ "email": email.trim(), "password": password }))
        .send().await.map_err(|_| "Não foi possível contactar o servidor.".to_string())?;
    if !response.status().is_success() { return Err("Não foi possível iniciar sessão. Verifica os dados e tenta novamente.".into()); }
    let cookie = response.headers().get_all(header::SET_COOKIE).iter()
        .filter_map(|v| v.to_str().ok()).filter_map(|v| v.split(';').next())
        .find(|v| v.starts_with("jwt=") && v.len() > 4)
        .ok_or("O servidor não devolveu uma sessão válida.")?.to_string();
    let account = response.json::<Account>().await.map_err(|_| "Resposta do servidor inválida.")?;
    Ok(Session { account, cookie, base, client })
}

#[cfg(windows)]
pub async fn passkey_login(server: String, window: isize) -> Result<Session, String> {
    passkey_login_with(server, window, crate::passkey::authenticate).await
}
#[cfg(windows)]
async fn passkey_login_with<F>(server: String, window: isize, authenticate: F) -> Result<Session, String>
where F: FnOnce(isize, String, Vec<u8>) -> Result<serde_json::Value, String> + Send + 'static {
    let base = server_url(&server)?;
    let client = Client::builder().redirect(Policy::none()).timeout(Duration::from_secs(15)).build().map_err(|_| "Could not prepare a secure connection.")?;
    let response = client.post(base.join("api/auth/passkeys/login/options").map_err(|_| "Invalid server address")?)
        .json(&serde_json::json!({})).send().await.map_err(|_| "Could not start passkey sign-in.")?;
    if !response.status().is_success() { return Err("Could not start passkey sign-in.".into()); }
    let challenge_cookie = response.headers().get_all(header::SET_COOKIE).iter().filter_map(|value| value.to_str().ok()).filter_map(|value| value.split(';').next())
        .find(|value| value.starts_with("sonar-passkey-authentication=") && value.len() > "sonar-passkey-authentication=".len())
        .ok_or("Sonar did not return a valid passkey challenge.")?.to_owned();
    let body: serde_json::Value = response.json().await.map_err(|_| "Invalid passkey response.")?;
    let origin = body["origin"].as_str().ok_or("Invalid passkey origin.")?;
    let (rp_id, client_data) = crate::passkey::validate_request(&base, origin, &body["options"])?;
    let assertion = tokio::task::spawn_blocking(move || authenticate(window, rp_id, client_data)).await
        .map_err(|_| "Passkey sign-in was interrupted.")??;
    let response = client.post(base.join("api/auth/passkeys/login/verify").map_err(|_| "Invalid server address")?)
        .header(header::COOKIE, challenge_cookie).json(&serde_json::json!({ "response": assertion })).send().await
        .map_err(|_| "Could not confirm the passkey.")?;
    if !response.status().is_success() { return Err("Could not sign in with this passkey.".into()); }
    let cookie = response.headers().get_all(header::SET_COOKIE).iter().filter_map(|value| value.to_str().ok()).filter_map(|value| value.split(';').next())
        .find(|value| value.starts_with("jwt=") && value.len() > 4).ok_or("Sonar did not return a valid session.")?.to_owned();
    let account = response.json::<Account>().await.map_err(|_| "Unexpected response from Sonar.")?;
    Ok(Session { account, cookie, base, client })
}

#[derive(Clone, Copy)]
pub enum MemberAction { Remove, Ban, Promote, Demote }
fn valid_id(value: &str) -> bool { value.len() == 24 && value.bytes().all(|c| c.is_ascii_hexdigit()) }
fn invite_token(value: &str) -> Result<String, String> {
    let value = value.trim();
    let token = if value.len() == 48 && value.bytes().all(|c| c.is_ascii_hexdigit()) {
        value.to_string()
    } else {
        let url = Url::parse(value).map_err(|_| "Convite invalido.".to_string())?;
        if !matches!(url.scheme(), "https" | "http") || !url.username().is_empty() || url.password().is_some() {
            return Err("Convite invalido.".into());
        }
        let values: Vec<String> = url.query_pairs().filter(|(key, _)| key == "invite").map(|(_, value)| value.into_owned()).collect();
        if values.len() != 1 { return Err("Convite invalido.".into()); }
        values[0].clone()
    };
    if token.len() != 48 || !token.bytes().all(|c| c.is_ascii_hexdigit()) { return Err("Convite invalido.".into()); }
    Ok(token.to_ascii_lowercase())
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Passkey { #[serde(rename = "_id")] pub id: String, pub name: String, #[serde(default)] pub created_at: String, #[serde(default)] pub last_used_at: Option<String> }
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sessions { pub sessions: Vec<SessionEntry>, #[serde(default)] pub has_more: bool, #[serde(default)] pub legacy_current: bool }
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntry { pub id: String, pub current: bool, #[serde(default)] pub method: String, #[serde(default)] pub created_at: String, #[serde(default)] pub expires_at: String }

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatrixRoomSnapshot { pub room_id: String, pub revision: u64, pub members: Vec<String>, pub kind: String }
pub struct TrustedMatrixDevice { pub user_id: String, pub device_id: String, pub ed25519: String, pub curve25519: String }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatrixHistoryEvent { #[serde(rename = "_id")] pub id: String, pub room_id: String, pub sender: String, pub sender_device: String, pub content: serde_json::Value, pub pinned: bool, pub created_at: String, pub expires_at: Option<String> }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatrixHistoryPage { pub messages: Vec<MatrixHistoryEvent>, pub has_more: bool, pub next_cursor: Option<String> }

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatrixDeviceEntry { pub device_id: String, pub server_authorization: String, pub ed25519: String, pub current: bool }
#[derive(Deserialize)]
struct MatrixDeviceList { devices: Vec<MatrixDeviceEntry> }

fn expected_matrix_direct_room(user: &ruma::UserId, peer_account: &str) -> Result<(String, Vec<String>), String> {
    use sha2::{Digest, Sha256};
    if peer_account.len() != 24 || !peer_account.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) || peer_account == user.localpart() { return Err("Conversa invalida".into()); }
    let peer = format!("@{}:{}", peer_account, user.server_name());
    let mut members = vec![user.to_string(), peer];
    members.sort();
    let material = serde_json::to_vec(&["sonar-direct-v1", members[0].as_str(), members[1].as_str()]).map_err(|_| "Conversa invalida")?;
    Ok((format!("!{:x}:{}", Sha256::digest(material), user.server_name()), members))
}

#[cfg(test)]
mod direct_room_identity_tests {
    use super::*;
    #[test]
    fn direct_room_matches_browser_digest_and_is_symmetric() {
        let first = ruma::UserId::parse("@0123456789abcdef01234567:sonar.invalid").unwrap();
        let second = ruma::UserId::parse("@abcdef0123456789abcdef01:sonar.invalid").unwrap();
        let a = expected_matrix_direct_room(&first, second.localpart()).unwrap();
        let b = expected_matrix_direct_room(&second, first.localpart()).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.0, "!34d9733e173f04aadc177004f613d68ca5ab13cae6b41173e7e466db4b27c0de:sonar.invalid");
        assert!(expected_matrix_direct_room(&first, first.localpart()).is_err());
        assert!(expected_matrix_direct_room(&first, "invalid").is_err());
    }
}

// Credentials are ephemeral and deliberately do not implement Debug.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceConfiguration {
    ice_servers: Vec<VoiceIceServer>,
    turn_configured: bool,
    relay_only: bool,
    pub expires_at: Option<String>,
}
#[derive(Deserialize)]
struct VoiceIceServer { urls: Vec<String>, #[serde(default)] username: String, #[serde(default)] credential: String }
impl VoiceConfiguration {
    pub fn into_servers(self, prefer_relay: bool) -> Result<(Vec<rtc::peer_connection::configuration::RTCIceServer>, bool), String> {
        let invalid = || "The voice configuration is invalid or expired.".to_string();
        if self.ice_servers.len() > 12 || self.expires_at.as_ref().is_some_and(|s| s.len() > 64) { return Err(invalid()); }
        let mut has_turn = false;
        let mut total = 0;
        let mut servers = Vec::new();
        for server in self.ice_servers {
            if server.urls.is_empty() || server.username.len() > 256 || server.credential.len() > 256 { return Err(invalid()); }
            total += server.urls.len();
            if total > 24 { return Err(invalid()); }
            let mut turn = false;
            for url in &server.urls {
                if url.len() > 2048 || url.bytes().any(|b| b.is_ascii_whitespace() || b.is_ascii_control()) { return Err(invalid()); }
                if let Some(host) = url.strip_prefix("turn:").or_else(|| url.strip_prefix("turns:")) {
                    if host.is_empty() { return Err(invalid()); }
                    turn = true;
                } else if url.strip_prefix("stun:").or_else(|| url.strip_prefix("stuns:")).is_none_or(str::is_empty) { return Err(invalid()); }
            }
            if turn {
                let expiry = server.username.split(':').next().and_then(|s| s.parse::<u64>().ok()).ok_or_else(invalid)?;
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| invalid())?.as_secs();
                if expiry <= now + 10 || server.credential.is_empty() { return Err(invalid()); }
                has_turn = true;
            } else if !server.username.is_empty() || !server.credential.is_empty() { return Err(invalid()); }
            servers.push(rtc::peer_connection::configuration::RTCIceServer { urls: server.urls, username: server.username, credential: server.credential, ..Default::default() });
        }
        if has_turn != self.turn_configured || (has_turn && self.expires_at.is_none()) || ((prefer_relay || self.relay_only) && !has_turn) { return Err(invalid()); }
        Ok((servers, prefer_relay || self.relay_only))
    }
}
impl Session {
    pub async fn matrix_identity(&self) -> Result<String, String> {
        let failure = || "Servico Matrix experimental indisponivel.".to_string();
        let account = self.account.id.as_deref().filter(|id| valid_id(id)).ok_or_else(failure)?;
        let response = self.client.get(self.base.join("api/matrix/device/identity").map_err(|_| failure())?).header(header::COOKIE, &self.cookie).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let bytes = response.bytes().await.map_err(|_| failure())?;
        if bytes.len() > 4096 { return Err(failure()); }
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| failure())?;
        let user = ruma::UserId::parse(value["userId"].as_str().ok_or_else(failure)?).map_err(|_| failure())?;
        if user.localpart() != account { return Err(failure()); }
        Ok(user.to_string())
    }
    pub fn matrix_origin(&self) -> String { self.base.origin().ascii_serialization() }
    pub fn live_auth(&self, generation: u64) -> crate::live::Auth {
        crate::live::Auth { generation, base: self.base.to_string(), cookie: self.cookie.clone() }
    }
    /// Server authorization is not cryptographic verification. Confirm the
    /// local protected key before presenting this list for device management.
    pub async fn matrix_devices(&self, machine: &matrix_sdk_crypto::OlmMachine) -> Result<Vec<MatrixDeviceEntry>, String> {
        let failure = || "Nao foi possivel confirmar os dispositivos Matrix.".to_string();
        if self.account.id.as_deref() != Some(machine.user_id().localpart()) { return Err(failure()); }
        let identity = self.client.get(self.base.join("api/matrix/device/identity").map_err(|_| failure())?).header(header::COOKIE, &self.cookie).send().await.map_err(|_| failure())?;
        if !identity.status().is_success() { return Err(failure()); }
        let identity: serde_json::Value = identity.json().await.map_err(|_| failure())?;
        if identity["userId"] != machine.user_id().as_str() { return Err(failure()); }
        let response = self.client.get(self.base.join("api/matrix/device/list").map_err(|_| failure())?).header(header::COOKIE, &self.cookie).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let bytes = response.bytes().await.map_err(|_| failure())?;
        if bytes.len() > 65536 { return Err(failure()); }
        let list: MatrixDeviceList = serde_json::from_slice(&bytes).map_err(|_| failure())?;
        let mut ids = std::collections::HashSet::new();
        if list.devices.len() > 100 || list.devices.iter().any(|d| d.device_id.is_empty() || d.device_id.len() > 32 || !d.device_id.bytes().all(|b| b.is_ascii_alphanumeric()) || !ids.insert(&d.device_id) || !["pending", "authorized"].contains(&d.server_authorization.as_str()) || d.ed25519.len() != 43 || !d.ed25519.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')) { return Err(failure()); }
        let current: Vec<_> = list.devices.iter().filter(|d| d.current).collect();
        if current.len() != 1 || current[0].device_id != machine.device_id().as_str() || current[0].ed25519 != machine.identity_keys().ed25519.to_base64() { return Err(failure()); }
        Ok(list.devices)
    }
    pub async fn revoke_matrix_device(&self, machine: &matrix_sdk_crypto::OlmMachine, target: &MatrixDeviceEntry) -> Result<(), String> {
        let failure = || "O dispositivo mudou ou nao foi possivel remove-lo.".to_string();
        if target.current || target.device_id == machine.device_id().as_str() { return Err(failure()); }
        let devices = self.matrix_devices(machine).await?;
        if !devices.iter().any(|d| d.current && d.server_authorization == "authorized") || !devices.iter().any(|d| !d.current && d.device_id == target.device_id && d.ed25519 == target.ed25519) { return Err(failure()); }
        let result = self.matrix_post("revoke", serde_json::json!({"deviceId":machine.device_id().as_str(), "targetDeviceId":target.device_id, "expectedEd25519":target.ed25519})).await?;
        if result["revoked"] != true || result["deviceId"] != target.device_id { return Err(failure()); }
        Ok(())
    }
    pub async fn authorize_matrix_device(&self, machine: &matrix_sdk_crypto::OlmMachine, target: &str, ed25519: &str) -> Result<(), String> {
        let failure = || "Nao foi possivel aprovar o dispositivo verificado.".to_string();
        let devices = self.matrix_devices(machine).await?;
        if !devices.iter().any(|d| d.current && d.server_authorization == "authorized") || !devices.iter().any(|d| !d.current && d.device_id == target && d.ed25519 == ed25519 && d.server_authorization == "pending") { return Err(failure()); }
        let result = self.matrix_post("authorize", serde_json::json!({"deviceId":machine.device_id().as_str(), "targetDeviceId":target, "expectedEd25519":ed25519})).await?;
        if result["authorized"] != true || result["deviceId"] != target { return Err(failure()); }
        Ok(())
    }
    /// Refresh same-account public device keys in the SDK. This is key
    /// discovery only and must never be treated as device verification.
    pub async fn refresh_matrix_own_keys(&self, machine: &matrix_sdk_crypto::OlmMachine) -> Result<(), String> {
        let failure = || "Nao foi possivel consultar as chaves dos dispositivos.".to_string();
        if self.matrix_identity().await? != machine.user_id().as_str() { return Err(failure()); }
        machine.update_tracked_users([machine.user_id()]).await.map_err(|_| failure())?;
        let (transaction, _) = machine.query_keys_for_users([machine.user_id()]);
        let response = self.client.get(self.base.join("api/matrix/device/keys/own").map_err(|_| failure())?).header(header::COOKIE, &self.cookie)
            .query(&[("deviceId", machine.device_id().as_str())]).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let bytes = response.bytes().await.map_err(|_| failure())?;
        if bytes.len() > 262144 { return Err(failure()); }
        let raw: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| failure())?;
        let root = raw.as_object().ok_or_else(failure)?;
        let keys = root.get("device_keys").and_then(|v| v.as_object()).ok_or_else(failure)?;
        if root.keys().any(|k| k != "device_keys" && k != "failures") || keys.len() != 1 || !keys.get(machine.user_id().as_str()).is_some_and(|v| v.is_object()) || !root.get("failures").and_then(|v| v.as_object()).is_some_and(|v| v.is_empty()) { return Err(failure()); }
        let mut parsed = ruma::api::client::keys::get_keys::v3::Response::new();
        parsed.device_keys = serde_json::from_value(raw["device_keys"].clone()).map_err(|_| failure())?;
        machine.mark_request_as_sent(&transaction, &parsed).await.map_err(|_| failure())?;
        Ok(())
    }
    pub async fn publish_matrix_one_time_keys(&self, machine: &matrix_sdk_crypto::OlmMachine) -> Result<(), String> {
        use matrix_sdk_crypto::types::requests::AnyOutgoingRequest;
        let failure = || "Nao foi possivel publicar chaves de verificacao.".to_string();
        if self.matrix_identity().await? != machine.user_id().as_str() { return Err(failure()); }
        let requests = machine.outgoing_requests().await.map_err(|_| failure())?;
        let Some(request) = requests.iter().find(|request| matches!(request.request(), AnyOutgoingRequest::KeysUpload(_))) else { return Ok(()); };
        let AnyOutgoingRequest::KeysUpload(body) = request.request() else { unreachable!() };
        let result = self.matrix_post("keys/upload", serde_json::json!({"deviceId":machine.device_id().as_str(), "oneTimeKeys":body.one_time_keys})).await?;
        let counts = result.get("one_time_key_counts").cloned().ok_or_else(failure)?;
        if counts.get("signed_curve25519").and_then(|n| n.as_u64()).is_none() { return Err(failure()); }
        let response = ruma::api::client::keys::upload_keys::v3::Response::new(serde_json::from_value(counts).map_err(|_| failure())?);
        machine.mark_request_as_sent(request.request_id(), &response).await.map_err(|_| failure())?;
        Ok(())
    }
    /// Establish an Olm session for one known device. This does not mark the
    /// device trusted; SAS verification is still required.
    pub async fn claim_matrix_own_key(&self, machine: &matrix_sdk_crypto::OlmMachine, target: &str) -> Result<bool, String> {
        let failure = || "Nao foi possivel iniciar a verificacao do dispositivo.".to_string();
        if target.is_empty() || target.len() > 32 || !target.bytes().all(|b| b.is_ascii_alphanumeric()) || target == machine.device_id().as_str() { return Err(failure()); }
        if self.matrix_identity().await? != machine.user_id().as_str() { return Err(failure()); }
        if machine.get_device(machine.user_id(), target.into(), None).await.map_err(|_| failure())?.is_none() { return Err(failure()); }
        let Some((transaction, request)) = machine.get_missing_sessions(std::iter::once(machine.user_id())).await.map_err(|_| failure())? else { return Ok(false); };
        let requests = serde_json::to_value(&request.one_time_keys).map_err(|_| failure())?;
        let Some(algorithm) = requests.get(machine.user_id().as_str()).and_then(|v| v.get(target)).and_then(|v| v.as_str()) else { return Ok(false); };
        if algorithm != "signed_curve25519" { return Err(failure()); }
        let result = self.matrix_post("keys/claim-own", serde_json::json!({"deviceId":machine.device_id().as_str(), "targetDeviceId":target, "algorithm":algorithm})).await?;
        let keys = result.get("one_time_keys").and_then(|v| v.as_object()).ok_or_else(failure)?;
        let devices = keys.get(machine.user_id().as_str()).and_then(|v| v.as_object()).ok_or_else(failure)?;
        let claimed = devices.get(target).and_then(|v| v.as_object()).ok_or_else(failure)?;
        if keys.len() != 1 || devices.len() != 1 || claimed.len() != 1 || !result.get("failures").and_then(|v| v.as_object()).is_some_and(|v| v.is_empty()) { return Err(failure()); }
        let response = ruma::api::client::keys::claim_keys::v3::Response::new(serde_json::from_value(result["one_time_keys"].clone()).map_err(|_| failure())?);
        machine.mark_request_as_sent(&transaction, &response).await.map_err(|_| failure())?;
        Ok(true)
    }
    /// Consume encrypted same-account verification events only. Successful
    /// decryption alone never grants trust or approves a device.
    pub async fn receive_matrix_verification(&self, machine: &matrix_sdk_crypto::OlmMachine) -> Result<usize, String> {
        use matrix_sdk_crypto::{DecryptionSettings, EncryptionSyncChanges, TrustRequirement};
        let failure = || "Nao foi possivel receber a verificacao do dispositivo.".to_string();
        if self.matrix_identity().await? != machine.user_id().as_str() { return Err(failure()); }
        let response = self.client.get(self.base.join("api/matrix/device/verification/inbox").map_err(|_| failure())?).header(header::COOKIE, &self.cookie)
            .query(&[("deviceId", machine.device_id().as_str())]).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let bytes = response.bytes().await.map_err(|_| failure())?;
        if bytes.len() > 2_000_000 { return Err(failure()); }
        let raw: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| failure())?;
        let rows = raw.get("events").and_then(|v| v.as_array()).ok_or_else(failure)?;
        if rows.len() > 100 { return Err(failure()); }
        let mut ids = Vec::new(); let mut unique = std::collections::HashSet::new(); let mut events = Vec::new();
        for row in rows {
            let id = row.get("id").and_then(|v| v.as_str()).filter(|id| id.len() == 24 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))).ok_or_else(failure)?;
            let event = row.get("event").ok_or_else(failure)?;
            if !unique.insert(id) || event["type"] != "m.room.encrypted" || event["sender"] != machine.user_id().as_str() || !event["content"].is_object() { return Err(failure()); }
            ids.push(id.to_owned());
            events.push(serde_json::from_value(event.clone()).map_err(|_| failure())?);
        }
        if ids.is_empty() { return Ok(0); }
        let changes = EncryptionSyncChanges { to_device_events: events, changed_devices: &Default::default(), one_time_keys_counts: &Default::default(), unused_fallback_keys: None, next_batch_token: None };
        let settings = DecryptionSettings { sender_device_trust_requirement: TrustRequirement::Untrusted };
        machine.receive_sync_changes(changes, &settings).await.map_err(|_| failure())?;
        let ack = self.matrix_post("verification/ack", serde_json::json!({"deviceId":machine.device_id().as_str(), "ids":ids})).await?;
        if ack["acknowledged"] != true { return Err(failure()); }
        Ok(rows.len())
    }
    /// Send already-persisted ciphertext verbatim. Callers must persist it
    /// before invoking this method and retain it until acceptance is confirmed.
    pub async fn deliver_matrix_verification(&self, machine: &matrix_sdk_crypto::OlmMachine, transaction: &str, target: &str, ciphertext: serde_json::Value) -> Result<(), String> {
        let failure = || "Nao foi possivel enviar a verificacao.".to_string();
        if transaction.is_empty() || transaction.len() > 128 || !transaction.bytes().all(|b| b.is_ascii_alphanumeric() || b"._~-".contains(&b)) || target.is_empty() || target.len() > 32 || !target.bytes().all(|b| b.is_ascii_alphanumeric()) || target == machine.device_id().as_str() || !ciphertext.is_object() { return Err(failure()); }
        if self.matrix_identity().await? != machine.user_id().as_str() { return Err(failure()); }
        let mut devices = serde_json::Map::new(); devices.insert(target.into(), ciphertext);
        let mut users = serde_json::Map::new(); users.insert(machine.user_id().to_string(), devices.into());
        let result = self.matrix_post("verification/send", serde_json::json!({"deviceId":machine.device_id().as_str(), "transactionId":transaction, "eventType":"m.room.encrypted", "messages":users})).await?;
        if result["accepted"] != true { return Err(failure()); }
        Ok(())
    }
    pub async fn deliver_matrix_contact_verification(&self, machine: &matrix_sdk_crypto::OlmMachine, room: &MatrixRoomSnapshot, transaction: &str, user: &str, target: &str, ciphertext: serde_json::Value) -> Result<(), String> {
        let failure = || "Nao foi possivel enviar a verificacao do contacto.".to_string();
        if room.kind != "direct" || room.members.len() != 2 || !room.members.iter().any(|id| id == user) || user == machine.user_id().as_str() || transaction.is_empty() || transaction.len() > 128 || !transaction.bytes().all(|b| b.is_ascii_alphanumeric() || b"._~-".contains(&b)) || target.is_empty() || target.len() > 32 || !target.bytes().all(|b| b.is_ascii_alphanumeric()) || !ciphertext.is_object() { return Err(failure()); }
        let mut devices = serde_json::Map::new(); devices.insert(target.into(), ciphertext);
        let mut users = serde_json::Map::new(); users.insert(user.into(), devices.into());
        let response = self.client.post(self.base.join("api/matrix/messages/verification/send").map_err(|_| failure())?).header(header::COOKIE, &self.cookie)
            .json(&serde_json::json!({"deviceId":machine.device_id().as_str(),"roomId":room.room_id,"revision":room.revision,"transactionId":transaction,"eventType":"m.room.encrypted","messages":users})).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let value: serde_json::Value = response.json().await.map_err(|_| failure())?;
        if value["accepted"] != true { return Err(failure()); }
        Ok(())
    }
    pub async fn matrix_contact_verification_inbox(&self, machine: &matrix_sdk_crypto::OlmMachine, room: &MatrixRoomSnapshot) -> Result<Vec<serde_json::Value>, String> {
        let failure = || "Verificacao do contacto indisponivel.".to_string();
        let response = self.client.get(self.base.join("api/matrix/messages/verification/inbox").map_err(|_| failure())?).header(header::COOKIE, &self.cookie)
            .query(&[("deviceId", machine.device_id().as_str()), ("roomId", room.room_id.as_str())]).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let bytes = response.bytes().await.map_err(|_| failure())?;
        if bytes.len() > 2_000_000 { return Err(failure()); }
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| failure())?;
        let rows = value.get("events").and_then(|v| v.as_array()).ok_or_else(failure)?;
        if rows.len() > 100 { return Err(failure()); }
        Ok(rows.clone())
    }
    pub async fn acknowledge_matrix_contact_verification(&self, machine: &matrix_sdk_crypto::OlmMachine, room: &MatrixRoomSnapshot, ids: &[String]) -> Result<(), String> {
        let failure = || "Nao foi possivel confirmar os eventos de verificacao.".to_string();
        let response = self.client.post(self.base.join("api/matrix/messages/verification/ack").map_err(|_| failure())?).header(header::COOKIE, &self.cookie)
            .json(&serde_json::json!({"deviceId":machine.device_id().as_str(),"roomId":room.room_id,"ids":ids})).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let value: serde_json::Value = response.json().await.map_err(|_| failure())?;
        if value["acknowledged"] != true { return Err(failure()); }
        Ok(())
    }
    /// Resolve the same direct room as the browser and reject a server response
    /// that changes its identity or membership before any keys are shared.
    pub async fn matrix_direct_room(&self, machine: &matrix_sdk_crypto::OlmMachine, peer_account: &str) -> Result<MatrixRoomSnapshot, String> {
        let failure = || "Nao foi possivel confirmar a conversa Matrix.".to_string();
        if self.account.id.as_deref() != Some(machine.user_id().localpart()) { return Err(failure()); }
        let (expected_id, expected_members) = expected_matrix_direct_room(machine.user_id(), peer_account)?;
        let identity = self.client.get(self.base.join("api/matrix/device/identity").map_err(|_| failure())?).header(header::COOKIE, &self.cookie).send().await.map_err(|_| failure())?;
        if !identity.status().is_success() { return Err(failure()); }
        let identity: serde_json::Value = identity.json().await.map_err(|_| failure())?;
        if identity["userId"] != machine.user_id().as_str() { return Err(failure()); }
        let response = self.client.post(self.base.join("api/matrix/messages/rooms/direct").map_err(|_| failure())?).header(header::COOKIE, &self.cookie)
            .json(&serde_json::json!({"deviceId": machine.device_id().as_str(), "peerAccountId": peer_account})).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let bytes = response.bytes().await.map_err(|_| failure())?;
        if bytes.len() > 16384 { return Err(failure()); }
        let snapshot: MatrixRoomSnapshot = serde_json::from_slice(&bytes).map_err(|_| failure())?;
        let mut members = snapshot.members.clone(); members.sort();
        if snapshot.room_id != expected_id || snapshot.kind != "direct" || snapshot.revision > 9_007_199_254_740_991 || members != expected_members { return Err(failure()); }
        Ok(snapshot)
    }
    /// Query every current room device and require SDK trust before any room
    /// key is shared or plaintext from a remote device is shown.
    pub async fn trusted_matrix_room_devices(&self, machine: &matrix_sdk_crypto::OlmMachine, room: &MatrixRoomSnapshot) -> Result<Vec<TrustedMatrixDevice>, String> {
        self.matrix_room_devices(machine, room, true).await
    }
    /// Device discovery for starting a SAS comparison; returned devices are
    /// not trusted until the user completes verification.
    pub async fn matrix_room_devices(&self, machine: &matrix_sdk_crypto::OlmMachine, room: &MatrixRoomSnapshot, require_trust: bool) -> Result<Vec<TrustedMatrixDevice>, String> {
        let failure = || "Um dispositivo da conversa nao esta verificado.".to_string();
        if self.matrix_identity().await? != machine.user_id().as_str() || room.members.len() < 2 || room.members.len() > 32 || !room.members.iter().any(|u| u == machine.user_id().as_str()) { return Err(failure()); }
        let users: Vec<_> = room.members.iter().map(|user| ruma::UserId::parse(user).map_err(|_| failure())).collect::<Result<_, _>>()?;
        let unique: std::collections::HashSet<_> = room.members.iter().collect();
        if unique.len() != users.len() { return Err(failure()); }
        let user_refs: Vec<&ruma::UserId> = users.iter().map(|user| user.as_ref()).collect();
        machine.update_tracked_users(user_refs.clone()).await.map_err(|_| failure())?;
        let (transaction, _) = machine.query_keys_for_users(user_refs);
        let response = self.client.post(self.base.join("api/matrix/messages/keys/query").map_err(|_| failure())?).header(header::COOKIE, &self.cookie)
            .json(&serde_json::json!({"deviceId":machine.device_id().as_str(),"roomId":room.room_id,"revision":room.revision,"users":room.members})).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let bytes = response.bytes().await.map_err(|_| failure())?;
        if bytes.len() > 1_000_000 { return Err(failure()); }
        let raw: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| failure())?;
        let keys = raw.get("device_keys").and_then(|v| v.as_object()).ok_or_else(failure)?;
        if keys.len() != users.len() || keys.keys().any(|user| !room.members.contains(user)) || !raw.get("failures").and_then(|v| v.as_object()).is_some_and(|v| v.is_empty()) { return Err(failure()); }
        let mut parsed = ruma::api::client::keys::get_keys::v3::Response::new();
        parsed.device_keys = serde_json::from_value(raw["device_keys"].clone()).map_err(|_| failure())?;
        machine.mark_request_as_sent(&transaction, &parsed).await.map_err(|_| failure())?;
        let mut trusted = Vec::new();
        for user in &users {
            let devices = keys.get(user.as_str()).and_then(|v| v.as_object()).ok_or_else(failure)?;
            if devices.is_empty() || devices.len() > 100 { return Err(failure()); }
            for (id, value) in devices {
                if id.is_empty() || id.len() > 32 || !id.bytes().all(|b| b.is_ascii_alphanumeric()) { return Err(failure()); }
                let device = machine.get_device(user, id.as_str().into(), None).await.map_err(|_| failure())?.ok_or_else(failure)?;
                let ed = device.ed25519_key().ok_or_else(failure)?.to_base64();
                let curve = device.curve25519_key().ok_or_else(failure)?.to_base64();
                let local = user.as_str() == machine.user_id().as_str() && id == machine.device_id().as_str();
                if device.is_deleted() || device.is_blacklisted() || (require_trust && !local && !device.is_verified()) || (local && ed != machine.identity_keys().ed25519.to_base64()) || value["keys"][format!("ed25519:{id}")] != ed || value["keys"][format!("curve25519:{id}")] != curve { return Err(failure()); }
                trusted.push(TrustedMatrixDevice { user_id: user.to_string(), device_id: id.clone(), ed25519: ed, curve25519: curve });
            }
        }
        Ok(trusted)
    }
    /// Claim keys only for devices in the freshly verified room snapshot.
    pub async fn establish_matrix_room_sessions(&self, machine: &matrix_sdk_crypto::OlmMachine, room: &MatrixRoomSnapshot, trusted: &[TrustedMatrixDevice]) -> Result<usize, String> {
        let failure = || "Nao foi possivel iniciar sessoes seguras para a conversa.".to_string();
        let users: Vec<_> = room.members.iter().map(|user| ruma::UserId::parse(user).map_err(|_| failure())).collect::<Result<_, _>>()?;
        let refs: Vec<&ruma::UserId> = users.iter().map(|user| user.as_ref()).collect();
        let Some((transaction, request)) = machine.get_missing_sessions(refs.into_iter()).await.map_err(|_| failure())? else { return Ok(0); };
        let requested = serde_json::to_value(&request.one_time_keys).map_err(|_| failure())?;
        let users = requested.as_object().ok_or_else(failure)?;
        let mut collected = serde_json::Map::new(); let mut count = 0;
        for (user, targets) in users {
            let targets = targets.as_object().ok_or_else(failure)?;
            for (device, algorithm) in targets {
                if algorithm != "signed_curve25519" || (user == machine.user_id().as_str() && device == machine.device_id().as_str()) || !trusted.iter().any(|item| &item.user_id == user && &item.device_id == device) { return Err(failure()); }
                let response = self.client.post(self.base.join("api/matrix/messages/keys/claim").map_err(|_| failure())?).header(header::COOKIE, &self.cookie)
                    .json(&serde_json::json!({"deviceId":machine.device_id().as_str(),"roomId":room.room_id,"revision":room.revision,"targetUser":user,"targetDevice":device,"algorithm":"signed_curve25519"})).send().await.map_err(|_| failure())?;
                if !response.status().is_success() { return Err(failure()); }
                let bytes = response.bytes().await.map_err(|_| failure())?;
                if bytes.len() > 16384 { return Err(failure()); }
                let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| failure())?;
                let all = value.get("one_time_keys").and_then(|v| v.as_object()).ok_or_else(failure)?;
                let devices = all.get(user).and_then(|v| v.as_object()).ok_or_else(failure)?;
                let keyset = devices.get(device).and_then(|v| v.as_object()).ok_or_else(failure)?;
                if all.len() != 1 || devices.len() != 1 || keyset.len() != 1 || !keyset.keys().all(|key| key.starts_with("signed_curve25519:")) || !value.get("failures").and_then(|v| v.as_object()).is_some_and(|v| v.is_empty()) { return Err(failure()); }
                collected.entry(user.clone()).or_insert_with(|| serde_json::json!({})).as_object_mut().ok_or_else(failure)?.insert(device.clone(), serde_json::Value::Object(keyset.clone()));
                count += 1;
            }
        }
        let response = ruma::api::client::keys::claim_keys::v3::Response::new(serde_json::from_value(serde_json::Value::Object(collected)).map_err(|_| failure())?);
        machine.mark_request_as_sent(&transaction, &response).await.map_err(|_| failure())?;
        Ok(count)
    }
    pub async fn claim_matrix_contact_key(&self, machine: &matrix_sdk_crypto::OlmMachine, room: &MatrixRoomSnapshot, user: &str, device: &str) -> Result<bool, String> {
        let failure = || "Nao foi possivel iniciar a verificacao do contacto.".to_string();
        if room.kind != "direct" || room.members.len() != 2 || !room.members.iter().any(|item| item == user) || user == machine.user_id().as_str() || device.is_empty() || device.len() > 32 || !device.bytes().all(|b| b.is_ascii_alphanumeric()) { return Err(failure()); }
        let user_id = ruma::UserId::parse(user).map_err(|_| failure())?;
        if machine.get_device(&user_id, device.into(), None).await.map_err(|_| failure())?.is_none() { return Err(failure()); }
        let Some((transaction, request)) = machine.get_missing_sessions(std::iter::once(user_id.as_ref())).await.map_err(|_| failure())? else { return Ok(false); };
        let requested = serde_json::to_value(&request.one_time_keys).map_err(|_| failure())?;
        let Some(algorithm) = requested.get(user).and_then(|v| v.get(device)).and_then(|v| v.as_str()) else { return Ok(false); };
        if algorithm != "signed_curve25519" { return Err(failure()); }
        let response = self.client.post(self.base.join("api/matrix/messages/keys/claim").map_err(|_| failure())?).header(header::COOKIE, &self.cookie)
            .json(&serde_json::json!({"deviceId":machine.device_id().as_str(),"roomId":room.room_id,"revision":room.revision,"targetUser":user,"targetDevice":device,"algorithm":algorithm})).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let bytes = response.bytes().await.map_err(|_| failure())?;
        if bytes.len() > 16384 { return Err(failure()); }
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| failure())?;
        let all = value.get("one_time_keys").and_then(|v| v.as_object()).ok_or_else(failure)?;
        let devices = all.get(user).and_then(|v| v.as_object()).ok_or_else(failure)?;
        let keys = devices.get(device).and_then(|v| v.as_object()).ok_or_else(failure)?;
        if all.len() != 1 || devices.len() != 1 || keys.len() != 1 || !value.get("failures").and_then(|v| v.as_object()).is_some_and(|v| v.is_empty()) { return Err(failure()); }
        let response = ruma::api::client::keys::claim_keys::v3::Response::new(serde_json::from_value(value["one_time_keys"].clone()).map_err(|_| failure())?);
        machine.mark_request_as_sent(&transaction, &response).await.map_err(|_| failure())?;
        Ok(true)
    }
    /// Returns encrypted events only. Callers must independently validate the
    /// sender's verified SDK identity before exposing decrypted contents.
    pub async fn matrix_room_history(&self, machine: &matrix_sdk_crypto::OlmMachine, room: &MatrixRoomSnapshot, before: Option<String>, pinned: bool) -> Result<MatrixHistoryPage, String> {
        let failure = || "Historico cifrado indisponivel.".to_string();
        if before.as_ref().is_some_and(|s| s.is_empty() || s.len() > 256 || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')) { return Err(failure()); }
        if self.matrix_identity().await? != machine.user_id().as_str() { return Err(failure()); }
        let mut query = vec![("deviceId", machine.device_id().to_string()), ("roomId", room.room_id.clone()), ("limit", "50".to_owned())];
        if let Some(before) = before { query.push(("before", before)); }
        if pinned { query.push(("pinned", "true".to_owned())); }
        let response = self.client.get(self.base.join("api/matrix/messages/events/history").map_err(|_| failure())?).header(header::COOKIE, &self.cookie).query(&query).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let bytes = response.bytes().await.map_err(|_| failure())?;
        if bytes.len() > 4_000_000 { return Err(failure()); }
        let mut page: MatrixHistoryPage = serde_json::from_slice(&bytes).map_err(|_| failure())?;
        if page.messages.len() > 50 || page.has_more != page.next_cursor.is_some() || page.next_cursor.as_ref().is_some_and(|s| s.is_empty() || s.len() > 256 || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')) { return Err(failure()); }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| failure())?.as_millis() as i64;
        for row in &page.messages {
            let expires = row.expires_at.as_ref().map(|value| chrono::DateTime::parse_from_rfc3339(value)).transpose().map_err(|_| failure())?;
            if row.id.len() != 24 || !row.id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) || row.room_id != room.room_id || !room.members.contains(&row.sender) || row.sender_device.is_empty() || row.sender_device.len() > 32 || !row.sender_device.bytes().all(|b| b.is_ascii_alphanumeric()) || !row.content.is_object() || chrono::DateTime::parse_from_rfc3339(&row.created_at).is_err() || row.pinned == expires.is_some() { return Err(failure()); }
        }
        page.messages.retain(|row| row.pinned || row.expires_at.as_ref().and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok()).is_some_and(|value| value.timestamp_millis() > now));
        Ok(page)
    }
    pub async fn matrix_room_key_inbox(&self, machine: &matrix_sdk_crypto::OlmMachine, room: &MatrixRoomSnapshot) -> Result<Vec<serde_json::Value>, String> {
        let failure = || "Chaves da conversa indisponiveis.".to_string();
        let response = self.client.get(self.base.join("api/matrix/messages/shares/inbox").map_err(|_| failure())?).header(header::COOKIE, &self.cookie)
            .query(&[("deviceId", machine.device_id().as_str()), ("roomId", room.room_id.as_str())]).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let bytes = response.bytes().await.map_err(|_| failure())?;
        if bytes.len() > 2_000_000 { return Err(failure()); }
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| failure())?;
        let rows = value.get("events").and_then(|v| v.as_array()).ok_or_else(failure)?;
        if rows.len() > 100 { return Err(failure()); }
        Ok(rows.clone())
    }
    pub async fn acknowledge_matrix_room_keys(&self, machine: &matrix_sdk_crypto::OlmMachine, room: &MatrixRoomSnapshot, ids: &[String]) -> Result<(), String> {
        let failure = || "Confirmacao das chaves indisponivel.".to_string();
        let response = self.client.post(self.base.join("api/matrix/messages/shares/ack").map_err(|_| failure())?).header(header::COOKIE, &self.cookie)
            .json(&serde_json::json!({"deviceId":machine.device_id().as_str(), "roomId":room.room_id, "ids":ids})).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let value: serde_json::Value = response.json().await.map_err(|_| failure())?;
        if value["acknowledged"] != true { return Err(failure()); }
        Ok(())
    }
    pub async fn deliver_matrix_room_share(&self, machine: &matrix_sdk_crypto::OlmMachine, room: &MatrixRoomSnapshot, transaction: &str, messages: &serde_json::Value) -> Result<(), String> {
        let failure = || "Nao foi possivel enviar as chaves da conversa.".to_string();
        if transaction.is_empty() || transaction.len() > 128 || !transaction.bytes().all(|b| b.is_ascii_alphanumeric() || b"._~-".contains(&b)) || !messages.is_object() { return Err(failure()); }
        let response = self.client.post(self.base.join("api/matrix/messages/shares/send").map_err(|_| failure())?).header(header::COOKIE, &self.cookie)
            .json(&serde_json::json!({"deviceId":machine.device_id().as_str(), "roomId":room.room_id, "revision":room.revision, "transactionId":transaction, "eventType":"m.room.encrypted", "messages":messages})).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let result: serde_json::Value = response.json().await.map_err(|_| failure())?;
        if result["accepted"] != true { return Err(failure()); }
        Ok(())
    }
    pub async fn issue_matrix_message_token(&self, machine: &matrix_sdk_crypto::OlmMachine, room: &MatrixRoomSnapshot) -> Result<(String, String), String> {
        let failure = || "Nao foi possivel preparar a mensagem cifrada.".to_string();
        let response = self.client.post(self.base.join("api/matrix/messages/events/transaction").map_err(|_| failure())?).header(header::COOKIE, &self.cookie)
            .json(&serde_json::json!({"deviceId":machine.device_id().as_str(),"roomId":room.room_id,"revision":room.revision})).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let bytes = response.bytes().await.map_err(|_| failure())?;
        if bytes.len() > 4096 { return Err(failure()); }
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| failure())?;
        let token = value["token"].as_str().filter(|v| v.len() == 43 && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')).ok_or_else(failure)?;
        let expires = value["expiresAt"].as_str().ok_or_else(failure)?;
        let deadline = chrono::DateTime::parse_from_rfc3339(expires).map_err(|_| failure())?;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| failure())?.as_millis() as i64;
        if deadline.timestamp_millis() <= now { return Err(failure()); }
        Ok((token.to_owned(), expires.to_owned()))
    }
    pub async fn deliver_matrix_message(&self, machine: &matrix_sdk_crypto::OlmMachine, room: &MatrixRoomSnapshot, token: &str, content: &serde_json::Value) -> Result<String, String> {
        let failure = || "Envio nao confirmado. Tenta novamente a mesma mensagem.".to_string();
        let response = self.client.post(self.base.join("api/matrix/messages/events/send").map_err(|_| failure())?).header(header::COOKIE, &self.cookie)
            .json(&serde_json::json!({"deviceId":machine.device_id().as_str(),"roomId":room.room_id,"revision":room.revision,"token":token,"content":content})).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let bytes = response.bytes().await.map_err(|_| failure())?;
        if bytes.len() > 131072 { return Err(failure()); }
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| failure())?;
        let id = value["_id"].as_str().filter(|v| v.len() == 24 && v.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))).ok_or_else(failure)?;
        if value["unavailable"] != true && value.get("content") != Some(content) { return Err(failure()); }
        Ok(id.to_owned())
    }
    pub async fn deliver_matrix_attachment(&self, machine: &matrix_sdk_crypto::OlmMachine, room: &MatrixRoomSnapshot, token: &str, content: &serde_json::Value, attachment_id: &str, ciphertext: Vec<u8>) -> Result<String, String> {
        let failure = || "Envio do anexo nao confirmado. Tenta novamente.".to_string();
        if attachment_id.len() != 32 || !attachment_id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) || ciphertext.is_empty() || ciphertext.len() > 10 * 1024 * 1024 { return Err(failure()); }
        let header = serde_json::to_vec(&serde_json::json!({"deviceId":machine.device_id().as_str(),"roomId":room.room_id,"revision":room.revision,"token":token,"content":content,"attachmentId":attachment_id})).map_err(|_| failure())?;
        if header.len() > 65536 { return Err(failure()); }
        let mut frame = Vec::with_capacity(4 + header.len() + ciphertext.len());
        frame.extend_from_slice(&(header.len() as u32).to_be_bytes()); frame.extend_from_slice(&header); frame.extend_from_slice(&ciphertext);
        let response = self.client.post(self.base.join("api/matrix/messages/events/send-attachment").map_err(|_| failure())?).header(header::COOKIE, &self.cookie).header(header::CONTENT_TYPE, "application/octet-stream").body(frame).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let bytes = response.bytes().await.map_err(|_| failure())?;
        if bytes.len() > 131072 { return Err(failure()); }
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| failure())?;
        let id = value["_id"].as_str().filter(|value| valid_id(value)).ok_or_else(failure)?;
        if value["unavailable"] != true && (value.get("content") != Some(content) || value["attachmentId"] != attachment_id) { return Err(failure()); }
        Ok(id.to_owned())
    }
    pub async fn download_matrix_attachment(&self, machine: &matrix_sdk_crypto::OlmMachine, room: &MatrixRoomSnapshot, attachment_id: &str, expected_size: usize) -> Result<Vec<u8>, String> {
        let failure = || "Nao foi possivel obter o anexo cifrado.".to_string();
        if attachment_id.len() != 32 || !attachment_id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) || expected_size == 0 || expected_size > 10 * 1024 * 1024 { return Err(failure()); }
        let response = self.client.get(self.base.join("api/matrix/messages/events/attachment").map_err(|_| failure())?).header(header::COOKIE, &self.cookie)
            .query(&[("deviceId", machine.device_id().as_str()), ("roomId", room.room_id.as_str()), ("attachmentId", attachment_id)]).send().await.map_err(|_| failure())?;
        if !response.status().is_success() || response.content_length() != Some(expected_size as u64) { return Err(failure()); }
        let bytes = response.bytes().await.map_err(|_| failure())?;
        if bytes.len() != expected_size { return Err(failure()); }
        Ok(bytes.to_vec())
    }
    pub async fn pin_matrix_message(&self, machine: &matrix_sdk_crypto::OlmMachine, room: &MatrixRoomSnapshot, id: &str, pinned: bool) -> Result<(), String> {
        let failure = || "Nao foi possivel atualizar a mensagem fixada.".to_string();
        if !valid_id(id) { return Err(failure()); }
        let response = self.client.post(self.base.join("api/matrix/messages/events/pin").map_err(|_| failure())?).header(header::COOKIE, &self.cookie)
            .json(&serde_json::json!({"deviceId":machine.device_id().as_str(),"roomId":room.room_id,"eventId":id,"pinned":pinned})).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let bytes = response.bytes().await.map_err(|_| failure())?;
        if bytes.len() > 131072 { return Err(failure()); }
        let result: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| failure())?;
        if result["_id"] != id || result["roomId"] != room.room_id || result["pinned"] != pinned || if pinned { !result["expiresAt"].is_null() } else { result["expiresAt"].as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).is_none() } { return Err(failure()); }
        Ok(())
    }
    pub async fn delete_matrix_message(&self, machine: &matrix_sdk_crypto::OlmMachine, room: &MatrixRoomSnapshot, id: &str) -> Result<(), String> {
        let failure = || "Nao foi possivel apagar a mensagem.".to_string();
        if !valid_id(id) { return Err(failure()); }
        let response = self.client.post(self.base.join("api/matrix/messages/events/delete").map_err(|_| failure())?).header(header::COOKIE, &self.cookie)
            .json(&serde_json::json!({"deviceId":machine.device_id().as_str(),"roomId":room.room_id,"eventId":id})).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let result: serde_json::Value = response.json().await.map_err(|_| failure())?;
        if result["id"] != id { return Err(failure()); }
        Ok(())
    }
    // Registration only; the server leaves the device pending until separately
    // approved. Never creates or replaces a local Matrix store.
    pub async fn register_matrix_device(&self, machine: &matrix_sdk_crypto::OlmMachine) -> Result<(), String> {
        use matrix_sdk_crypto::types::requests::AnyOutgoingRequest;
        let failure = || "Nao foi possivel confirmar o dispositivo Matrix. As chaves locais foram mantidas.".to_string();
        let account = self.account.id.as_deref().filter(|id| valid_id(id)).ok_or_else(failure)?;
        let response = self.client.get(self.base.join("api/matrix/device/identity").map_err(|_| failure())?).header(header::COOKIE, &self.cookie).send().await.map_err(|_| failure())?;
        if !response.status().is_success() { return Err(failure()); }
        let identity: serde_json::Value = response.json().await.map_err(|_| failure())?;
        let user = identity["userId"].as_str().ok_or_else(failure)?;
        if user != machine.user_id().as_str() || machine.user_id().localpart() != account { return Err("A identidade Matrix do servidor mudou.".into()); }
        let device = machine.device_id().as_str();
        let requests = machine.outgoing_requests().await.map_err(|_| failure())?;
        if let Some(upload) = requests.iter().find(|request| matches!(request.request(), AnyOutgoingRequest::KeysUpload(_))) {
            let AnyOutgoingRequest::KeysUpload(body) = upload.request() else { unreachable!() };
            if body.device_keys.is_some() {
                let keys = serde_json::to_value(&body.device_keys).map_err(|_| failure())?;
                let challenge = self.matrix_post("enroll/challenge", serde_json::json!({ "deviceId": device, "deviceKeys": keys })).await;
                if let Ok(challenge) = challenge {
                    let (token, message, expiry) = matrix_challenge(&challenge)?;
                    let signature = crate::matrix_binding::sign_enrollment(machine, user, device, &keys, token, message, expiry).await?;
                    // If the response is lost, the binding proof below can still recover.
                    let _ = self.matrix_post("enroll/complete", serde_json::json!({ "deviceId": device, "token": token, "signature": signature })).await;
                }
            }
        }
        let binding = self.matrix_post("bind/challenge", serde_json::json!({ "deviceId": device })).await.map_err(|_| failure())?;
        let (token, message, expiry) = matrix_challenge(&binding)?;
        let signature = crate::matrix_binding::sign_binding(machine, user, device, token, message, expiry).await?;
        let result = self.matrix_post("bind/complete", serde_json::json!({ "deviceId": device, "token": token, "signature": signature })).await.map_err(|_| failure())?;
        if result["userId"] != user || result["deviceId"] != device { return Err(failure()); }
        Ok(())
    }
    pub async fn matrix_room_snapshot(&self, machine: &matrix_sdk_crypto::OlmMachine, room_id: &str) -> Result<MatrixRoomSnapshot, String> {
        let invalid = || "Dados da sala Matrix invalidos.".to_string();
        if room_id.len() > 302 || ruma::RoomId::parse(room_id).is_err() || self.account.id.as_deref() != Some(machine.user_id().localpart()) { return Err(invalid()); }
        let identity = self.client.get(self.base.join("api/matrix/device/identity").map_err(|_| invalid())?).header(header::COOKIE, &self.cookie).send().await.map_err(|_| invalid())?;
        if !identity.status().is_success() { return Err(invalid()); }
        let identity: serde_json::Value = identity.json().await.map_err(|_| invalid())?;
        if identity["userId"] != machine.user_id().as_str() { return Err("A identidade Matrix do servidor mudou.".into()); }
        let response = self.client.get(self.base.join("api/matrix/device/room").map_err(|_| invalid())?).header(header::COOKIE, &self.cookie)
            .query(&[("roomId", room_id), ("deviceId", machine.device_id().as_str())]).send().await.map_err(|_| invalid())?;
        if !response.status().is_success() { return Err(invalid()); }
        let bytes = response.bytes().await.map_err(|_| invalid())?;
        if bytes.len() > 16384 { return Err(invalid()); }
        let snapshot: MatrixRoomSnapshot = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        let unique: std::collections::HashSet<_> = snapshot.members.iter().collect();
        if snapshot.room_id != room_id || snapshot.revision > 9_007_199_254_740_991 || !["direct", "community"].contains(&snapshot.kind.as_str()) || snapshot.members.len() < 2 || snapshot.members.len() > 32 || unique.len() != snapshot.members.len() || !snapshot.members.iter().any(|id| id == machine.user_id().as_str()) || snapshot.members.iter().any(|id| id.len() > 226 || ruma::UserId::parse(id).is_err()) { return Err(invalid()); }
        Ok(snapshot)
    }
    async fn matrix_post(&self, path: &str, body: serde_json::Value) -> Result<serde_json::Value, String> {
        let response = self.client.post(self.base.join(&format!("api/matrix/device/{path}")).map_err(|_| "Invalid server address")?).header(header::COOKIE, &self.cookie).json(&body).send().await.map_err(|_| "Servico Matrix indisponivel")?;
        if !response.status().is_success() { return Err(format!("Operacao Matrix recusada ({})", response.status().as_u16())); }
        let bytes = response.bytes().await.map_err(|_| "Resposta Matrix invalida")?;
        if bytes.len() > 16384 { return Err("Resposta Matrix invalida".into()); }
        serde_json::from_slice(&bytes).map_err(|_| "Resposta Matrix invalida".into())
    }
    pub async fn voice_configuration(&self) -> Result<VoiceConfiguration, String> {
        let response = self.client.get(self.base.join("api/voice/config").map_err(|_| "Invalid server address")?)
            .header(header::COOKIE, &self.cookie).send().await.map_err(|_| "Could not load the voice configuration.")?;
        if !response.status().is_success() { return Err("Voice configuration unavailable.".into()); }
        let bytes = response.bytes().await.map_err(|_| "Unexpected voice configuration response.")?;
        if bytes.len() > 32768 { return Err("Unexpected voice configuration response.".into()); }
        serde_json::from_slice(&bytes).map_err(|_| "Unexpected voice configuration response.".into())
    }
    #[cfg(windows)]
    pub async fn register_passkey(self, name: String, password: String, window: isize) -> Result<Passkey, String> {
        self.register_passkey_with(name, password, window, crate::passkey::register).await
    }
    #[cfg(windows)]
    async fn register_passkey_with<F>(self, name: String, password: String, window: isize, register: F) -> Result<Passkey, String>
    where F: FnOnce(isize, crate::passkey::Registration) -> Result<serde_json::Value, String> + Send + 'static {
        use zeroize::Zeroizing;
        let password = Zeroizing::new(password);
        let name = name.trim();
        if name.is_empty() || name.encode_utf16().count() > 60 || password.is_empty() || password.len() > 72 { return Err("Enter a name and your current password.".into()); }
        let account_id = self.account.id.as_deref().filter(|id| valid_id(id)).ok_or("Invalid account.")?;
        let response = self.client.post(self.base.join("api/auth/passkeys/register/options").map_err(|_| "Invalid server address")?)
            .header(header::COOKIE, &self.cookie).json(&serde_json::json!({ "name": name, "currentPassword": &*password }))
            .send().await.map_err(|_| "Could not start passkey sign-in.")?;
        drop(password);
        if !response.status().is_success() { return Err("Could not start. Check your password and the passkey limit.".into()); }
        let challenge_cookie = response.headers().get_all(header::SET_COOKIE).iter().filter_map(|value| value.to_str().ok()).filter_map(|value| value.split(';').next())
            .find(|value| value.starts_with("sonar-passkey-registration=") && value.len() > "sonar-passkey-registration=".len())
            .ok_or("Sonar did not return a valid passkey challenge.")?.to_owned();
        let body: serde_json::Value = response.json().await.map_err(|_| "Invalid passkey response.")?;
        let origin = body["origin"].as_str().ok_or("Invalid passkey origin.")?;
        let request = crate::passkey::validate_registration(&self.base, origin, &body["options"], account_id)?;
        let credential = tokio::task::spawn_blocking(move || register(window, request)).await.map_err(|_| "Passkey creation was interrupted.")??;
        let cookies = format!("{}; {}", self.cookie, challenge_cookie);
        let response = self.client.post(self.base.join("api/auth/passkeys/register/verify").map_err(|_| "Invalid server address")?)
            .header(header::COOKIE, cookies).json(&serde_json::json!({ "response": credential })).send().await
            .map_err(|_| "Could not confirm the passkey. Refresh the list before trying again.")?;
        if !response.status().is_success() { return Err("Could not confirm the passkey. Refresh the list before trying again.".into()); }
        response.json().await.map_err(|_| "Unexpected response. Refresh the list before trying again.".into())
    }
    pub async fn leave_community(self, id: String) -> Result<(), String> {
        if !valid_id(&id) { return Err("Comunidade invalida.".into()); }
        let response = self.client.post(self.base.join(&format!("api/communities/{id}/leave")).map_err(|_| "Invalid server address")?)
            .header(header::COOKIE, &self.cookie).send().await.map_err(|_| "Nao foi possivel confirmar a saida. Atualiza a lista.")?;
        if !response.status().is_success() { return Err("Nao foi possivel sair. O dono deve transferir a propriedade primeiro.".into()); }
        Ok(())
    }
    pub async fn rename_community(self, community_id: String, name: String) -> Result<Community, String> {
        if !valid_id(&community_id) { return Err("Comunidade invalida.".into()); }
        let name = name.trim();
        if name.is_empty() || name.encode_utf16().count() > 60 { return Err("Usa um nome entre 1 e 60 caracteres.".into()); }
        self.rename_request(format!("api/communities/{community_id}"), name).await
    }
    pub async fn rename_channel(self, community_id: String, channel_id: String, name: String) -> Result<Community, String> {
        if !valid_id(&community_id) || !valid_id(&channel_id) { return Err("Comunidade ou canal invalido.".into()); }
        let normalized = name.split_whitespace().collect::<Vec<_>>().join("-").to_ascii_lowercase();
        if normalized.is_empty() || normalized.len() > 40 || !normalized.bytes().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) || !normalized.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-') {
            return Err("Usa 1 a 40 letras sem acentos, numeros ou hifens.".into());
        }
        self.rename_request(format!("api/communities/{community_id}/channels/{channel_id}"), &normalized).await
    }
    async fn rename_request(self, path: String, name: &str) -> Result<Community, String> {
        let response = self.client.patch(self.base.join(&path).map_err(|_| "Invalid server address")?)
            .header(header::COOKIE, &self.cookie).json(&serde_json::json!({"name": name}))
            .send().await.map_err(|_| "Nao foi possivel confirmar a alteracao. Atualiza a lista.")?;
        if !response.status().is_success() { return Err("Nao foi possivel alterar o nome. Confirma as permissoes e o nome.".into()); }
        response.json().await.map_err(|_| "Resposta invalida. Atualiza a lista.".into())
    }
    #[allow(dead_code)]
    pub async fn pin_direct_message(self, id: String, pinned: bool) -> Result<MessageEnvelope, String> {
        if !valid_id(&id) { return Err("Mensagem invalida.".into()); }
        let url = self.base.join(&format!("api/messages/{id}/pin")).map_err(|_| "Invalid server address")?;
        let response = self.client.patch(url).header(header::COOKIE, &self.cookie).json(&serde_json::json!({ "pinned": pinned }))
            .send().await.map_err(|_| "Nao foi possivel confirmar a alteracao. Atualiza a conversa.")?;
        if !response.status().is_success() { return Err("Mensagem indisponivel ou sem permissao.".into()); }
        response.json().await.map_err(|_| "Resposta invalida. Atualiza a conversa.".into())
    }
    #[allow(dead_code)]
    pub async fn delete_direct_message(self, id: String) -> Result<DeletedMessage, String> {
        if !valid_id(&id) { return Err("Mensagem invalida.".into()); }
        let url = self.base.join(&format!("api/messages/{id}")).map_err(|_| "Invalid server address")?;
        let response = self.client.delete(url).header(header::COOKIE, &self.cookie).send().await
            .map_err(|_| "Nao foi possivel confirmar a eliminacao. Atualiza a conversa.")?;
        if !response.status().is_success() { return Err("Mensagem indisponivel ou sem permissao.".into()); }
        response.json().await.map_err(|_| "Resposta invalida. Atualiza a conversa.".into())
    }
    #[allow(dead_code)]
    pub async fn send_direct_envelope(self, peer: String, envelope: crate::crypto::Envelope) -> Result<MessageEnvelope, String> {
        use base64::{engine::general_purpose::STANDARD, Engine};
        if !valid_id(&peer) { return Err("Pessoa invalida.".into()); }
        let iv = STANDARD.decode(&envelope.iv).map_err(|_| "Mensagem cifrada invalida.")?;
        let ciphertext = STANDARD.decode(&envelope.ciphertext).map_err(|_| "Mensagem cifrada invalida.")?;
        if iv.len() != 12 || ciphertext.len() < 16 || envelope.ciphertext.len() < 24 || envelope.ciphertext.len() > 24000 {
            return Err("Mensagem cifrada invalida.".into());
        }
        let url = self.base.join(&format!("api/messages/send/{peer}")).map_err(|_| "Invalid server address")?;
        let response = self.client.post(url).header(header::COOKIE, &self.cookie)
            .json(&serde_json::json!({ "version": 1, "iv": envelope.iv, "ciphertext": envelope.ciphertext }))
            .send().await.map_err(|_| "Nao foi possivel confirmar o envio. Atualiza a conversa antes de tentar novamente.")?;
        if !response.status().is_success() { return Err("Nao foi possivel enviar a mensagem cifrada.".into()); }
        response.json().await.map_err(|_| "Envio confirmado, mas a resposta do servidor e invalida. Atualiza a conversa.".into())
    }
    #[allow(dead_code)]
    pub async fn direct_history(self, peer: String, before: Option<String>) -> Result<MessagePage, String> {
        if !valid_id(&peer) { return Err("Pessoa invalida.".into()); }
        if before.as_ref().is_some_and(|cursor| cursor.is_empty() || cursor.len() > 256 || !cursor.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')) {
            return Err("Cursor de historico invalido.".into());
        }
        let mut url = self.base.join(&format!("api/messages/{peer}")).map_err(|_| "Invalid server address")?;
        { let mut query = url.query_pairs_mut(); query.append_pair("page", "true"); query.append_pair("limit", "50"); if let Some(cursor) = before { query.append_pair("before", &cursor); } }
        let response = self.client.get(url).header(header::COOKIE, &self.cookie).send().await.map_err(|_| "Nao foi possivel carregar o historico.")?;
        if !response.status().is_success() { return Err("Historico indisponivel ou sessao terminada.".into()); }
        response.json().await.map_err(|_| "Resposta do historico invalida.".into())
    }
    pub async fn directory(self) -> Result<Directory, String> {
        async fn fetch<T: serde::de::DeserializeOwned>(session: &Session, path: &str) -> Result<T, String> {
            let response = session.client.get(session.base.join(path).map_err(|_| "Servidor inv\u{e1}lido")?)
                .header(header::COOKIE, &session.cookie).send().await
                .map_err(|_| "N\u{e3}o foi poss\u{ed}vel contactar o servidor.")?;
            if !response.status().is_success() { return Err("N\u{e3}o foi poss\u{ed}vel carregar os teus espa\u{e7}os. Confirma que a sess\u{e3}o est\u{e1} ativa.".into()); }
            response.json().await.map_err(|_| "Resposta do servidor inv\u{e1}lida.".into())
        }
        let people = fetch(&self, "api/messages/users").await?;
        let communities = fetch(&self, "api/communities").await?;
        Ok(Directory { people, communities })
    }
    pub async fn community_details(self, community_id: String) -> Result<Community, String> {
        if !valid_id(&community_id) { return Err("Comunidade invalida.".into()); }
        let response = self.client.get(self.base.join(&format!("api/communities/{community_id}")).map_err(|_| "Invalid server address")?)
            .header(header::COOKIE, &self.cookie).send().await.map_err(|_| "Nao foi possivel carregar a comunidade.")?;
        if !response.status().is_success() { return Err("Comunidade indisponivel ou acesso removido.".into()); }
        response.json().await.map_err(|_| "Unexpected response from Sonar.".into())
    }
    pub async fn manage_member(self, community_id: String, member_id: String, action: MemberAction) -> Result<(), String> {
        if !valid_id(&community_id) || !valid_id(&member_id) { return Err("Membro ou comunidade invalida.".into()); }
        let path = format!("api/communities/{community_id}/members/{member_id}");
        let request = match action {
            MemberAction::Remove => self.client.delete(self.base.join(&path).map_err(|_| "Invalid server address")?),
            MemberAction::Ban => self.client.post(self.base.join(&format!("{path}/ban")).map_err(|_| "Invalid server address")?),
            MemberAction::Promote | MemberAction::Demote => self.client.put(self.base.join(&format!("{path}/moderator")).map_err(|_| "Invalid server address")?)
                .json(&serde_json::json!({ "moderator": matches!(action, MemberAction::Promote) })),
        };
        let response = request.header(header::COOKIE, &self.cookie).send().await.map_err(|_| "Nao foi possivel confirmar a operacao. Atualiza os membros antes de tentar novamente.")?;
        if !response.status().is_success() { return Err("Nao foi possivel alterar o membro. Confirma as permissoes e atualiza a lista.".into()); }
        Ok(())
    }
    pub async fn bans(self, community_id: String) -> Result<Vec<Person>, String> {
        if !valid_id(&community_id) { return Err("Comunidade invalida.".into()); }
        let response = self.client.get(self.base.join(&format!("api/communities/{community_id}/bans")).map_err(|_| "Invalid server address")?)
            .header(header::COOKIE, &self.cookie).send().await.map_err(|_| "Nao foi possivel carregar os bans.")?;
        if !response.status().is_success() { return Err("Nao foi possivel carregar os bans. Apenas o dono pode consulta-los.".into()); }
        response.json().await.map_err(|_| "Unexpected response from Sonar.".into())
    }
    pub async fn unban(self, community_id: String, member_id: String) -> Result<(), String> {
        if !valid_id(&community_id) || !valid_id(&member_id) { return Err("Comunidade ou conta invalida.".into()); }
        let response = self.client.delete(self.base.join(&format!("api/communities/{community_id}/bans/{member_id}")).map_err(|_| "Invalid server address")?)
            .header(header::COOKIE, &self.cookie).send().await.map_err(|_| "Nao foi possivel confirmar a operacao. Atualiza a lista de bans.")?;
        if !response.status().is_success() { return Err("Nao foi possivel remover o ban. Confirma as permissoes e atualiza a lista.".into()); }
        Ok(())
    }
    pub async fn create_community(self, name: String) -> Result<Community, String> {
        let name = name.trim();
        if name.is_empty() || name.encode_utf16().count() > 60 { return Err("Usa um nome entre 1 e 60 caracteres.".into()); }
        self.community_request("api/communities", serde_json::json!({ "name": name })).await
    }
    pub async fn join_community(self, invite: String) -> Result<Community, String> {
        use zeroize::Zeroizing;
        let invite = Zeroizing::new(invite);
        let token = Zeroizing::new(invite_token(&invite)?);
        self.community_request("api/communities/join", serde_json::json!({ "token": &*token })).await
    }
    async fn community_request(&self, path: &str, body: serde_json::Value) -> Result<Community, String> {
        let response = self.client.post(self.base.join(path).map_err(|_| "Invalid server address")?)
            .header(header::COOKIE, &self.cookie).json(&body).send().await
            .map_err(|_| "Nao foi possivel contactar o servidor. Atualiza a lista antes de tentar novamente.")?;
        if !response.status().is_success() { return Err("Nao foi possivel concluir a operacao. Confirma o convite ou a identidade da conta.".into()); }
        response.json().await.map_err(|_| "Operacao concluida, mas resposta invalida. Atualiza a lista.".into())
    }
    pub async fn create_channel(self, community_id: String, name: String, kind: &str) -> Result<Community, String> {
        if community_id.len() != 24 || !community_id.bytes().all(|c| c.is_ascii_hexdigit()) { return Err("Comunidade invalida.".into()); }
        if !["text", "voice"].contains(&kind) { return Err("Tipo de canal invalido.".into()); }
        let name = name.trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join("-");
        if name.is_empty() || name.len() > 40 || !name.bytes().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) ||
            !name.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-') {
            return Err("Usa 1 a 40 letras sem acentos, numeros ou hifens.".into());
        }
        self.community_request(&format!("api/communities/{community_id}/channels"), serde_json::json!({ "name": name, "type": kind })).await
    }
    pub async fn create_invite(self, community_id: String) -> Result<CreatedInvite, String> {
        if community_id.len() != 24 || !community_id.bytes().all(|c| c.is_ascii_hexdigit()) { return Err("Comunidade invalida.".into()); }
        let response = self.client.post(self.base.join(&format!("api/communities/{community_id}/invite")).map_err(|_| "Invalid server address")?)
            .header(header::COOKIE, &self.cookie).send().await
            .map_err(|_| "Nao foi possivel confirmar a criacao do convite. O convite anterior pode ter sido revogado.")?;
        if !response.status().is_success() { return Err("Nao foi possivel criar o convite. Confirma as permissoes e tenta novamente.".into()); }
        let invite: CreatedInvite = response.json().await.map_err(|_| "Resposta invalida. O convite anterior pode ter sido revogado.")?;
        if invite.token.len() != 48 || !invite.token.bytes().all(|c| c.is_ascii_hexdigit()) { return Err("Convite invalido recebido do servidor.".into()); }
        Ok(invite)
    }
    pub async fn identity_fingerprint(self) -> Result<Option<String>, String> {
        #[derive(Deserialize)] struct Identity { #[serde(rename = "encryptionKey")] encryption_key: Option<String> }
        let response = self.client.get(self.base.join("api/auth/check").map_err(|_| "Invalid server address")?)
            .header(header::COOKIE, &self.cookie).send().await.map_err(|_| "Nao foi possivel consultar a identidade.")?;
        if !response.status().is_success() { return Err("Nao foi possivel consultar a identidade.".into()); }
        let identity = response.json::<Identity>().await.map_err(|_| "Unexpected response from Sonar.")?;
        identity.encryption_key.map(|key| crate::crypto::fingerprint(&key).map_err(|_| "Chave publica invalida.".into())).transpose()
    }
    pub async fn update_profile(self, name: String) -> Result<Account, String> {
        let name = name.trim();
        if name.is_empty() || name.encode_utf16().count() > 80 {
            return Err("Usa um nome entre 1 e 80 caracteres.".into());
        }
        let response = self.client.put(self.base.join("api/auth/update-profile").map_err(|_| "Invalid server address")?)
            .header(header::COOKIE, &self.cookie).json(&serde_json::json!({ "fullName": name }))
            .send().await.map_err(|_| "Nao foi possivel contactar o servidor.")?;
        if !response.status().is_success() { return Err("Nao foi possivel atualizar o perfil.".into()); }
        response.json().await.map_err(|_| "Unexpected response from Sonar.".into())
    }
    pub async fn change_password(self, current: String, next: String) -> Result<(), String> {
        use zeroize::Zeroizing;
        let current = Zeroizing::new(current);
        let next = Zeroizing::new(next);
        if current.is_empty() || current.len() > 72 || next.encode_utf16().count() < 10 || next.len() > 72 || *current == *next {
            return Err("Verifica a palavra-passe atual e usa uma nova com 10 caracteres e ate 72 bytes.".into());
        }
        let response = self.client.put(self.base.join("api/auth/password").map_err(|_| "Invalid server address")?)
            .header(header::COOKIE, &self.cookie).json(&serde_json::json!({ "currentPassword": &*current, "newPassword": &*next }))
            .send().await.map_err(|_| "Nao foi possivel confirmar a alteracao. Tenta iniciar sessao com a nova palavra-passe.")?;
        if !response.status().is_success() { return Err("Nao foi possivel alterar a palavra-passe. Verifica os dados ou tenta mais tarde.".into()); }
        Ok(())
    }
    pub async fn passkeys(self) -> Result<Vec<Passkey>, String> {
        let response = self.client.get(self.base.join("api/auth/passkeys").map_err(|_| "Invalid server address")?)
            .header(header::COOKIE, &self.cookie).send().await.map_err(|_| "Nao foi possivel carregar as passkeys.")?;
        if !response.status().is_success() { return Err("Nao foi possivel carregar as passkeys.".into()); }
        response.json().await.map_err(|_| "Unexpected response from Sonar.".into())
    }
    pub async fn remove_passkey(self, id: String, password: String) -> Result<(), String> {
        use zeroize::Zeroizing;
        let password = Zeroizing::new(password);
        if id.len() != 24 || !id.bytes().all(|c| c.is_ascii_hexdigit()) || password.is_empty() || password.len() > 72 {
            return Err("Confirma a passkey e a palavra-passe atual.".into());
        }
        let response = self.client.delete(self.base.join(&format!("api/auth/passkeys/{id}")).map_err(|_| "Invalid server address")?)
            .header(header::COOKIE, &self.cookie).json(&serde_json::json!({ "currentPassword": &*password })).send().await
            .map_err(|_| "Nao foi possivel confirmar a remocao. A sessao pode ter terminado; inicia sessao para verificar.")?;
        if !response.status().is_success() { return Err("Nao foi possivel remover a passkey. Verifica a palavra-passe e tenta novamente.".into()); }
        Ok(())
    }
    pub async fn sessions(self) -> Result<Sessions, String> {
        let response = self.client.get(self.base.join("api/auth/sessions").map_err(|_| "Invalid server address")?)
            .header(header::COOKIE, &self.cookie).send().await.map_err(|_| "Nao foi possivel carregar as sessoes.")?;
        if !response.status().is_success() { return Err("Nao foi possivel carregar as sessoes.".into()); }
        response.json().await.map_err(|_| "Unexpected response from Sonar.".into())
    }
    pub async fn revoke_session(self, id: String) -> Result<bool, String> {
        #[derive(Deserialize)] struct ResultBody { current: bool }
        if id.len() != 64 || !id.bytes().all(|c| c.is_ascii_hexdigit()) { return Err("Identificador de sessao invalido.".into()); }
        let response = self.client.delete(self.base.join(&format!("api/auth/sessions/{id}")).map_err(|_| "Invalid server address")?)
            .header(header::COOKIE, &self.cookie).send().await.map_err(|_| "Nao foi possivel terminar a sessao.")?;
        if !response.status().is_success() { return Err("Nao foi possivel terminar a sessao.".into()); }
        Ok(response.json::<ResultBody>().await.map_err(|_| "Unexpected response from Sonar.")?.current)
    }
    pub async fn logout(self) -> Result<(), String> {
        let response = self.client.get(self.base.join("api/auth/sessions").map_err(|_| "Servidor inválido")?)
            .header(header::COOKIE, &self.cookie).send().await.map_err(|_| "Não foi possível terminar a sessão.")?;
        if !response.status().is_success() { return Err("Não foi possível terminar a sessão no servidor.".into()); }
        let sessions: Sessions = response.json().await.map_err(|_| "Resposta do servidor inválida.")?;
        let current = sessions.sessions.iter().find(|entry| entry.current).ok_or("Sessão atual não encontrada.")?;
        if current.id.len() != 64 || !current.id.bytes().all(|c| c.is_ascii_hexdigit()) { return Err("Identificador de sessão inválido.".into()); }
        let url = self.base.join(&format!("api/auth/sessions/{}", current.id)).map_err(|_| "Servidor inválido")?;
        let response = self.client.delete(url).header(header::COOKIE, &self.cookie).send().await.map_err(|_| "Não foi possível terminar a sessão.")?;
        if !response.status().is_success() { return Err("Não foi possível terminar a sessão no servidor.".into()); }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::server_url;
    #[tokio::test]
    async fn voice_configuration_is_authenticated_and_relay_fails_closed() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let mut bytes = [0; 4096]; let n = socket.read(&mut bytes).unwrap();
            let request = String::from_utf8_lossy(&bytes[..n]);
            assert!(request.starts_with("GET /api/voice/config "));
            assert!(request.to_ascii_lowercase().contains("cookie: jwt=test"));
            let body = r#"{"iceServers":[{"urls":["turn:relay.example.invalid:3478"],"username":"4102444800:synthetic","credential":"test-only"}],"turnConfigured":true,"relayOnly":true,"expiresAt":"2100-01-01T00:00:00Z"}"#;
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });
        let session = super::Session { account: super::Account { id: None, full_name: "Test".into(), ..Default::default() }, cookie: "jwt=test".into(), base: server_url(&format!("http://{address}")).unwrap(), client: reqwest::Client::new() };
        let (servers, relay) = session.voice_configuration().await.unwrap().into_servers(false).unwrap();
        assert!(relay);
        assert_eq!(servers[0].urls, vec!["turn:relay.example.invalid:3478"]);
        server.join().unwrap();
        for bad in [
            r#"{"iceServers":[],"turnConfigured":false,"relayOnly":true,"expiresAt":null}"#,
            r#"{"iceServers":[{"urls":["turn:example.invalid"],"username":"1:expired","credential":"x"}],"turnConfigured":true,"relayOnly":true,"expiresAt":"old"}"#,
            r#"{"iceServers":[{"urls":["https://example.invalid"]}],"turnConfigured":false,"relayOnly":false,"expiresAt":null}"#,
        ] {
            let config: super::VoiceConfiguration = serde_json::from_str(bad).unwrap();
            assert!(config.into_servers(false).is_err());
        }
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn passkey_registration_is_account_scoped_and_uses_challenge_cookie() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
            for step in 0..2 {
                let (mut socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                let mut request = Vec::new(); let mut bytes = [0; 4096];
                let end = loop {
                    let n = socket.read(&mut bytes).unwrap(); assert!(n > 0); request.extend_from_slice(&bytes[..n]);
                    if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]);
                        let length: usize = headers.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length: ").and_then(|s| s.parse().ok())).unwrap();
                        if request.len() >= end + 4 + length { break end; }
                    }
                };
                let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                assert!(headers.contains("cookie: jwt=test-session"));
                let body: serde_json::Value = serde_json::from_slice(&request[end + 4..]).unwrap();
                let (cookie, response) = if step == 0 {
                    assert!(headers.starts_with("post /api/auth/passkeys/register/options "));
                    assert_eq!(body["name"], "Test key"); assert_eq!(body["currentPassword"], "synthetic-password");
                    ("Set-Cookie: sonar-passkey-registration=challenge-token; HttpOnly\r\n", serde_json::json!({ "origin": "http://localhost:5173", "options": { "rp": { "id":"localhost", "name":"Sonar" }, "user": { "id": URL_SAFE_NO_PAD.encode("0123456789abcdef01234567"), "name":"user@example.invalid", "displayName":"Test" }, "challenge": URL_SAFE_NO_PAD.encode([1u8;32]), "authenticatorSelection": { "residentKey":"required", "userVerification":"required" }, "attestation":"none", "pubKeyCredParams":[{"type":"public-key", "alg":-7}], "excludeCredentials":[] } }).to_string())
                } else {
                    assert!(headers.starts_with("post /api/auth/passkeys/register/verify "));
                    assert!(headers.contains("sonar-passkey-registration=challenge-token"));
                    assert_eq!(body["response"]["id"], "synthetic-id");
                    ("", r#"{"_id":"abcdef0123456789abcdef01","name":"Test key","createdAt":"2030-01-01T00:00:00.000Z"}"#.to_owned())
                };
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n{}", response.len(), cookie, response).unwrap();
            }
        });
        let session = super::Session { account: super::Account { id: Some("0123456789abcdef01234567".into()), full_name: "Test".into(), ..Default::default() }, cookie: "jwt=test-session".into(), base: server_url(&format!("http://{address}")).unwrap(), client: reqwest::Client::new() };
        assert!(session.clone().register_passkey_with("".into(), "password".into(), 1, |_, _| unreachable!()).await.is_err());
        let result = session.register_passkey_with(" Test key ".into(), "synthetic-password".into(), 1, |window, _| { assert_eq!(window, 1); Ok(serde_json::json!({"id":"synthetic-id"})) }).await.unwrap();
        assert_eq!(result.name, "Test key");
        server.join().unwrap();
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn passkey_login_exchanges_challenge_and_session_cookies() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for step in 0..2 {
                let (mut socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                let mut request = Vec::new(); let mut bytes = [0; 4096];
                let end = loop {
                    let n = socket.read(&mut bytes).unwrap(); assert!(n > 0); request.extend_from_slice(&bytes[..n]);
                    if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]);
                        let length: usize = headers.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length: ").and_then(|s| s.parse().ok())).unwrap();
                        if request.len() >= end + 4 + length { break end; }
                    }
                };
                let headers = String::from_utf8_lossy(&request[..end]);
                let body: serde_json::Value = serde_json::from_slice(&request[end + 4..]).unwrap();
                let (cookie, response) = if step == 0 {
                    assert!(headers.starts_with("POST /api/auth/passkeys/login/options "));
                    ("Set-Cookie: sonar-passkey-authentication=challenge-token; HttpOnly\r\n", serde_json::json!({ "origin": "http://localhost:5173", "options": { "rpId": "localhost", "challenge": "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE", "userVerification": "required", "allowCredentials": [] } }).to_string())
                } else {
                    assert!(headers.starts_with("POST /api/auth/passkeys/login/verify "));
                    assert!(headers.to_ascii_lowercase().contains("cookie: sonar-passkey-authentication=challenge-token"));
                    assert_eq!(body["response"]["id"], "synthetic-id");
                    ("Set-Cookie: jwt=test-session; HttpOnly\r\n", r#"{"_id":"0123456789abcdef01234567","fullName":"Synthetic user"}"#.to_owned())
                };
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n{}", response.len(), cookie, response).unwrap();
            }
        });
        let session = super::passkey_login_with(format!("http://{address}"), 1, |window, rp, client_data| {
            assert_eq!(window, 1); assert_eq!(rp, "localhost");
            assert_eq!(serde_json::from_slice::<serde_json::Value>(&client_data).unwrap()["origin"], "http://localhost:5173");
            Ok(serde_json::json!({ "id": "synthetic-id" }))
        }).await.unwrap();
        assert_eq!(session.account.full_name, "Synthetic user");
        assert_eq!(session.cookie, "jwt=test-session");
        server.join().unwrap();
    }
    #[tokio::test]
    async fn leave_community_is_authenticated_and_rejects_invalid_ids() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let mut bytes = [0; 4096]; let n = socket.read(&mut bytes).unwrap();
            let request = String::from_utf8_lossy(&bytes[..n]);
            assert!(request.starts_with("POST /api/communities/0123456789abcdef01234567/leave "));
            assert!(request.to_ascii_lowercase().contains("cookie: jwt=test"));
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").unwrap();
        });
        let session = super::Session { account: super::Account { id: None, full_name: "Test".into(), ..Default::default() }, cookie: "jwt=test".into(), base: server_url(&format!("http://{address}")).unwrap(), client: reqwest::Client::new() };
        assert!(session.clone().leave_community("../bad".into()).await.is_err());
        session.leave_community("0123456789abcdef01234567".into()).await.unwrap();
        server.join().unwrap();
    }
    #[tokio::test]
    async fn rename_uses_scoped_endpoints_and_validates_names() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for expected in ["PATCH /api/communities/0123456789abcdef01234567 ", "PATCH /api/communities/0123456789abcdef01234567/channels/abcdef0123456789abcdef01 "] {
                let (mut socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                let mut request = Vec::new(); let mut bytes = [0; 4096];
                loop {
                    let n = socket.read(&mut bytes).unwrap(); assert!(n > 0); request.extend_from_slice(&bytes[..n]);
                    if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]);
                        let length: usize = headers.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length: ").and_then(|s| s.parse().ok())).unwrap();
                        if request.len() >= end + 4 + length { break; }
                    }
                }
                let end = request.windows(4).position(|part| part == b"\r\n\r\n").unwrap();
                let headers = String::from_utf8_lossy(&request[..end]);
                assert!(headers.starts_with(expected));
                assert!(headers.to_ascii_lowercase().contains("cookie: jwt=test"));
                let sent: serde_json::Value = serde_json::from_slice(&request[end + 4..]).unwrap();
                assert_eq!(sent["name"], if expected.contains("/channels/") { "new-room" } else { "New community" });
                let body = r#"{"_id":"0123456789abcdef01234567","name":"New community","channels":[]}"#;
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        let session = super::Session { account: super::Account { id: None, full_name: "Test".into(), ..Default::default() }, cookie: "jwt=test".into(), base: server_url(&format!("http://{address}")).unwrap(), client: reqwest::Client::new() };
        let id = "0123456789abcdef01234567"; let channel = "abcdef0123456789abcdef01";
        assert!(session.clone().rename_community("../bad".into(), "name".into()).await.is_err());
        assert!(session.clone().rename_channel(id.into(), channel.into(), "../bad".into()).await.is_err());
        assert_eq!(session.clone().rename_community(id.into(), " New community ".into()).await.unwrap().name, "New community");
        session.rename_channel(id.into(), channel.into(), " New Room ".into()).await.unwrap();
        server.join().unwrap();
    }
    #[test]
    fn contact_key_is_optional_and_can_be_fingerprinted() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../tests/webcrypto-fixture.json")).unwrap();
        let person: super::Person = serde_json::from_value(serde_json::json!({ "_id": "0123456789abcdef01234567", "fullName": "Alice", "encryptionKey": fixture["alicePublic"] })).unwrap();
        assert_eq!(crate::crypto::fingerprint(person.encryption_key.as_deref().unwrap()).unwrap(), fixture["aliceFingerprint"].as_str().unwrap());
        let without_key: super::Person = serde_json::from_str(r#"{"_id":"0123456789abcdef01234567","fullName":"Alice"}"#).unwrap();
        assert!(without_key.encryption_key.is_none());
    }
    #[tokio::test]
    async fn direct_message_management_is_scoped_and_uses_correct_methods() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for (expected, body) in [
                ("PATCH /api/messages/0123456789abcdef01234567/pin ", r#"{"_id":"0123456789abcdef01234567","senderId":"0123456789abcdef01234568","receiverId":"abcdef0123456789abcdef01","ciphertext":"AAAAAAAAAAAAAAAAAAAAAA==","iv":"AAAAAAAAAAAAAAAA","version":1,"pinned":true,"createdAt":"2030-01-01T00:00:00.000Z","expiresAt":null}"#),
                ("DELETE /api/messages/0123456789abcdef01234567 ", r#"{"id":"0123456789abcdef01234567","senderId":"0123456789abcdef01234568","receiverId":"abcdef0123456789abcdef01"}"#),
            ] {
                let (mut socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                let mut bytes = [0; 4096];
                let n = socket.read(&mut bytes).unwrap();
                let request = String::from_utf8_lossy(&bytes[..n]);
                assert!(request.starts_with(expected));
                assert!(request.to_ascii_lowercase().contains("cookie: jwt=test"));
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        let session = super::Session { account: super::Account { id: None, full_name: "Test".into(), ..Default::default() }, cookie: "jwt=test".into(), base: server_url(&format!("http://{address}")).unwrap(), client: reqwest::Client::new() };
        assert!(session.clone().pin_direct_message("../invalid".into(), true).await.is_err());
        assert!(session.clone().delete_direct_message("../invalid".into()).await.is_err());
        let id = "0123456789abcdef01234567";
        assert!(session.clone().pin_direct_message(id.into(), true).await.unwrap().pinned);
        assert_eq!(session.delete_direct_message(id.into()).await.unwrap().id, id);
        server.join().unwrap();
    }
    #[tokio::test]
    async fn send_direct_sends_only_ciphertext_and_rejects_bad_envelopes() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let mut request = Vec::new();
            let mut bytes = [0; 4096];
            loop {
                let n = socket.read(&mut bytes).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&bytes[..n]);
                if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let length: usize = headers.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length: ").and_then(|s| s.parse().ok())).unwrap();
                    if request.len() >= end + 4 + length { break; }
                }
            }
            let end = request.windows(4).position(|part| part == b"\r\n\r\n").unwrap();
            let headers = String::from_utf8_lossy(&request[..end]);
            assert!(headers.starts_with("POST /api/messages/send/abcdef0123456789abcdef01 "));
            assert!(headers.to_ascii_lowercase().contains("cookie: jwt=test"));
            let body: serde_json::Value = serde_json::from_slice(&request[end + 4..]).unwrap();
            assert_eq!(body, serde_json::json!({ "version": 1, "iv": "AAAAAAAAAAAAAAAA", "ciphertext": "AAAAAAAAAAAAAAAAAAAAAA==" }));
            let response = r#"{"_id":"0123456789abcdef01234567","senderId":"0123456789abcdef01234568","receiverId":"abcdef0123456789abcdef01","ciphertext":"AAAAAAAAAAAAAAAAAAAAAA==","iv":"AAAAAAAAAAAAAAAA","version":1,"createdAt":"2030-01-01T00:00:00.000Z","expiresAt":null}"#;
            write!(socket, "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
        });
        let session = super::Session { account: super::Account { id: None, full_name: "Test".into(), ..Default::default() }, cookie: "jwt=test".into(), base: server_url(&format!("http://{address}")).unwrap(), client: reqwest::Client::new() };
        let envelope = || crate::crypto::Envelope { iv: "AAAAAAAAAAAAAAAA".into(), ciphertext: "AAAAAAAAAAAAAAAAAAAAAA==".into() };
        assert!(session.clone().send_direct_envelope("../invalid".into(), envelope()).await.is_err());
        assert!(session.clone().send_direct_envelope("abcdef0123456789abcdef01".into(), crate::crypto::Envelope { iv: "bad".into(), ciphertext: "bad".into() }).await.is_err());
        let result = session.send_direct_envelope("abcdef0123456789abcdef01".into(), envelope()).await.unwrap();
        assert_eq!(result.receiver_id, "abcdef0123456789abcdef01");
        server.join().unwrap();
    }
    #[tokio::test]
    async fn direct_history_requests_only_valid_scoped_envelopes() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let mut bytes = [0; 4096];
            let count = socket.read(&mut bytes).unwrap();
            let request = String::from_utf8_lossy(&bytes[..count]);
            assert!(request.starts_with("GET /api/messages/abcdef0123456789abcdef01?page=true&limit=50&before=abc_123-xyz "));
            assert!(request.to_lowercase().contains("cookie: jwt=test"));
            let body = r#"{"messages":[{"_id":"0123456789abcdef01234567","senderId":"abcdef0123456789abcdef01","receiverId":"0123456789abcdef01234568","ciphertext":"ciphertext","iv":"nonce","version":1,"pinned":false,"createdAt":"2030-01-01T00:00:00.000Z","expiresAt":null}],"hasMore":false,"nextCursor":null}"#;
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });
        let session = super::Session { account: super::Account { id: None, full_name: "Test".into(), ..Default::default() }, cookie: "jwt=test".into(), base: server_url(&format!("http://{address}")).unwrap(), client: reqwest::Client::new() };
        assert!(session.clone().direct_history("../invalid".into(), None).await.is_err());
        assert!(session.clone().direct_history("abcdef0123456789abcdef01".into(), Some("bad&cursor".into())).await.is_err());
        let page = session.direct_history("abcdef0123456789abcdef01".into(), Some("abc_123-xyz".into())).await.unwrap();
        assert_eq!(page.messages[0].ciphertext, "ciphertext");
        assert!(!page.has_more);
        server.join().unwrap();
    }
    #[tokio::test]
    async fn login_and_revoke_only_current_session() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for expected in ["POST /api/auth/login", "GET /api/messages/users", "GET /api/communities", "GET /api/auth/check", "POST /api/communities", "POST /api/communities/join", "POST /api/communities/0123456789abcdef01234567/channels", "POST /api/communities/0123456789abcdef01234567/channels", "POST /api/communities/0123456789abcdef01234567/invite", "GET /api/communities/0123456789abcdef01234567", "PUT /api/communities/0123456789abcdef01234567/members/abcdef0123456789abcdef01/moderator", "DELETE /api/communities/0123456789abcdef01234567/members/abcdef0123456789abcdef01", "POST /api/communities/0123456789abcdef01234567/members/abcdef0123456789abcdef01/ban", "GET /api/communities/0123456789abcdef01234567/bans", "DELETE /api/communities/0123456789abcdef01234567/bans/abcdef0123456789abcdef01", "GET /api/auth/passkeys", "DELETE /api/auth/passkeys/0123456789abcdef01234567", "PUT /api/auth/update-profile", "PUT /api/auth/password", "GET /api/auth/sessions", "DELETE /api/auth/sessions/", "GET /api/auth/sessions", "DELETE /api/auth/sessions/"] {
                let (mut socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                let mut request = Vec::new();
                let mut bytes = [0; 2048];
                loop {
                    let count = socket.read(&mut bytes).unwrap();
                    if count == 0 { break; }
                    request.extend_from_slice(&bytes[..count]);
                    if request.windows(4).any(|part| part == b"\r\n\r\n") { break; }
                }
                let request = String::from_utf8_lossy(&request);
                assert!(request.starts_with(expected), "{request}");
                let (cookie, body) = if expected.starts_with("POST /api/auth/login") {
                    ("Set-Cookie: jwt=example-session; HttpOnly\r\n", r#"{"fullName":"Test user"}"#.to_string())
                } else {
                    assert!(request.to_lowercase().contains("cookie: jwt=example-session"));
                    if expected.ends_with("/bans") {
                        ("", r#"[{"_id":"abcdef0123456789abcdef01","fullName":"Alice"}]"#.to_string())
                    } else if expected.contains("/bans/") {
                        ("", "{}".to_string())
                    } else if expected.contains("/members/") {
                        ("", "{}".to_string())
                    } else if expected == "GET /api/communities/0123456789abcdef01234567" {
                        ("", r#"{"_id":"0123456789abcdef01234567","name":"Friends","ownerId":"0123456789abcdef01234568","moderators":["abcdef0123456789abcdef01"],"memberDetails":[{"_id":"abcdef0123456789abcdef01","fullName":"Alice"}],"channels":[]}"#.to_string())
                    } else if expected.ends_with("/invite") {
                        ("", format!(r#"{{"token":"{}","expiresAt":"2030-01-01T00:00:00.000Z"}}"#, "ab".repeat(24)))
                    } else if expected.starts_with("POST /api/communities") {
                        ("", r#"{"_id":"community-new","name":"Friends","channels":[]}"#.to_string())
                    } else if expected.contains("messages/users") {
                        ("", r#"[{"_id":"user-1","fullName":"Alice"}]"#.to_string())
                    } else if expected.contains("communities") {
                        ("", r#"[{"_id":"community-1","name":"Friends","channels":[{"_id":"channel-1","name":"general","type":"text"}]}]"#.to_string())
                    } else if expected.contains("auth/check") {
                        let fixture: serde_json::Value = serde_json::from_str(include_str!("../tests/webcrypto-fixture.json")).unwrap();
                        ("", format!(r#"{{"encryptionKey":"{}"}}"#, fixture["alicePublic"].as_str().unwrap()))
                    } else if expected == "GET /api/auth/passkeys" {
                        ("", r#"[{"_id":"0123456789abcdef01234567","name":"My key","createdAt":"2030-01-01T00:00:00.000Z","lastUsedAt":null}]"#.to_string())
                    } else if expected.starts_with("PUT") {
                        ("", r#"{"fullName":"Novo nome"}"#.to_string())
                    } else if expected.starts_with("GET") {
                        ("", format!(r#"{{"sessions":[{{"id":"{}","current":true}}]}}"#, "a".repeat(64)))
                    } else { ("", r#"{"current":false}"#.to_string()) }
                };
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n{}", body.len(), cookie, body).unwrap();
            }
        });
        let session = super::login(format!("http://{address}"), "test@example.invalid".into(), "test-password".into()).await.unwrap();
        assert_eq!(session.account.full_name, "Test user");
        let directory = session.clone().directory().await.unwrap();
        assert_eq!(directory.people[0].name, "Alice");
        assert_eq!(directory.communities[0].channels[0].name, "general");
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../tests/webcrypto-fixture.json")).unwrap();
        assert_eq!(session.clone().identity_fingerprint().await.unwrap().unwrap(), fixture["aliceFingerprint"].as_str().unwrap());
        assert!(session.clone().create_community("  ".into()).await.is_err());
        assert!(session.clone().join_community("invalid".into()).await.is_err());
        assert_eq!(session.clone().create_community("Friends".into()).await.unwrap().id, "community-new");
        assert_eq!(session.clone().join_community("ab".repeat(24)).await.unwrap().id, "community-new");
        let id = "0123456789abcdef01234567";
        assert!(session.clone().create_channel("../bad".into(), "test".into(), "text").await.is_err());
        assert!(session.clone().create_channel(id.into(), "../bad".into(), "text").await.is_err());
        assert!(session.clone().create_channel(id.into(), "test".into(), "other").await.is_err());
        assert_eq!(session.clone().create_channel(id.into(), "Some Plans".into(), "text").await.unwrap().id, "community-new");
        assert_eq!(session.clone().create_channel(id.into(), "Lounge".into(), "voice").await.unwrap().id, "community-new");
        assert!(session.clone().create_invite("../bad".into()).await.is_err());
        let invite = session.clone().create_invite(id.into()).await.unwrap();
        assert_eq!(invite.token, "ab".repeat(24));
        assert_eq!(invite.expires_at, "2030-01-01T00:00:00.000Z");
        let details = session.clone().community_details(id.into()).await.unwrap();
        assert_eq!(details.member_details[0].name, "Alice");
        assert_eq!(details.moderators.len(), 1);
        let member = "abcdef0123456789abcdef01";
        assert!(session.clone().manage_member(id.into(), "../bad".into(), super::MemberAction::Ban).await.is_err());
        session.clone().manage_member(id.into(), member.into(), super::MemberAction::Promote).await.unwrap();
        session.clone().manage_member(id.into(), member.into(), super::MemberAction::Remove).await.unwrap();
        session.clone().manage_member(id.into(), member.into(), super::MemberAction::Ban).await.unwrap();
        let bans = session.clone().bans(id.into()).await.unwrap();
        assert_eq!(bans[0].name, "Alice");
        assert!(session.clone().unban(id.into(), "../bad".into()).await.is_err());
        session.clone().unban(id.into(), member.into()).await.unwrap();
        let keys = session.clone().passkeys().await.unwrap();
        assert_eq!(keys[0].name, "My key");
        assert!(session.clone().remove_passkey("../bad".into(), "password".into()).await.is_err());
        assert!(session.clone().remove_passkey(id.into(), "".into()).await.is_err());
        session.clone().remove_passkey(id.into(), "test-password".into()).await.unwrap();
        assert!(session.clone().update_profile("  ".into()).await.is_err());
        assert!(session.clone().update_profile("x".repeat(81)).await.is_err());
        assert_eq!(session.clone().update_profile("  Novo nome  ".into()).await.unwrap().full_name, "Novo nome");
        assert!(session.clone().change_password("old-password".into(), "short".into()).await.is_err());
        assert!(session.clone().change_password("same-password".into(), "same-password".into()).await.is_err());
        session.clone().change_password("old-password".into(), "new-password".into()).await.unwrap();
        let sessions = session.clone().sessions().await.unwrap();
        assert!(sessions.sessions[0].current);
        assert!(session.clone().revoke_session("../../invalid".into()).await.is_err());
        assert!(!session.clone().revoke_session("b".repeat(64)).await.unwrap());
        session.logout().await.unwrap();
        server.join().unwrap();
    }
    #[test]
    fn invite_parser_accepts_codes_and_rejects_ambiguous_links() {
        let token = "ab".repeat(24);
        assert_eq!(super::invite_token(&token).unwrap(), token);
        assert_eq!(super::invite_token(&format!("https://sonar.invalid/?invite={token}")).unwrap(), token);
        assert!(super::invite_token(&format!("https://sonar.invalid/?invite={token}&invite={token}")).is_err());
        assert!(super::invite_token("https://sonar.invalid/?invite=bad").is_err());
        assert!(super::invite_token(&format!("javascript://host/?invite={token}")).is_err());
    }
    #[test]
    fn server_origin_requires_https_except_loopback() {
        assert!(server_url("https://sonar.example.org").is_ok());
        assert!(server_url("http://127.0.0.1:5000").is_ok());
        for bad in ["http://example.org", "http://localhost.evil.example", "https://user:password@example.org", "https://example.org/path", "https://example.org/?a=b"] {
            assert!(server_url(bad).is_err(), "{bad}");
        }
    }
}
