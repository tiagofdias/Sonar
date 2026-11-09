//! REST calls used by the desktop interface. Errors carry the server's own
//! message where it provides one, so the desktop shows exactly what the web
//! client shows. Plaintext is never sent: messages leave as AES-GCM envelopes.
use crate::api::{Account, Community, CreatedInvite, InvitePreview, MessageEnvelope, MessagePage, Passkey, Person, Session, Sessions};
use crate::key_backup::Backup;
use reqwest::{header, redirect::Policy, Client, Method, Response};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use std::time::Duration;
use zeroize::Zeroizing;

const UNREACHABLE: &str = "Could not reach Sonar. Check your connection and try again.";

fn http() -> Result<Client, String> {
    Client::builder().redirect(Policy::none()).timeout(Duration::from_secs(20)).build().map_err(|_| "Could not prepare a secure connection.".into())
}
fn valid_id(value: &str) -> bool { value.len() == 24 && value.bytes().all(|c| c.is_ascii_hexdigit()) }
fn check_id(value: &str) -> Result<&str, String> { if valid_id(value) { Ok(value) } else { Err("That item is no longer available.".into()) } }

async fn failure(response: Response, fallback: &str) -> String {
    #[derive(Deserialize)] struct Body { message: Option<String> }
    let status = response.status();
    let message = response.bytes().await.ok().filter(|bytes| bytes.len() < 4096).and_then(|bytes| serde_json::from_slice::<Body>(&bytes).ok()).and_then(|body| body.message);
    match message {
        Some(message) if !message.is_empty() && message.len() < 300 => message,
        _ if status.as_u16() == 401 => "Your session has ended. Sign in again.".into(),
        _ if status.as_u16() == 429 => "Too many attempts. Try again in a few minutes.".into(),
        _ => fallback.into(),
    }
}

fn session_cookie(response: &Response) -> Option<String> {
    response.headers().get_all(header::SET_COOKIE).iter()
        .filter_map(|value| value.to_str().ok()).filter_map(|value| value.split(';').next())
        .find(|value| value.starts_with("jwt=") && value.len() > 4).map(str::to_owned)
}

async fn authenticate(server: &str, path: &str, body: Value, fallback: &str) -> Result<Session, String> {
    let base = crate::api::server_url(server).map_err(|_| "Enter a valid server address (https://, or http:// for this computer only).".to_string())?;
    let client = http()?;
    let response = client.post(base.join(path).map_err(|_| "Invalid server address.")?).header(header::ORIGIN, base.origin().ascii_serialization()).json(&body).send().await.map_err(|_| UNREACHABLE)?;
    if !response.status().is_success() { return Err(failure(response, fallback).await); }
    let cookie = session_cookie(&response).ok_or("Sonar did not return a valid session.")?;
    let account = response.json::<Account>().await.map_err(|_| "Unexpected response from Sonar.")?;
    if !account.id.as_deref().is_some_and(valid_id) { return Err("Unexpected response from Sonar.".into()); }
    Ok(Session { account, cookie, base, client })
}

pub async fn login(server: String, email: String, password: Zeroizing<String>) -> Result<Session, String> {
    authenticate(&server, "api/auth/login", json!({ "email": email.trim(), "password": &*password }), "Invalid credentials").await
}

pub async fn signup(server: String, name: String, username: String, email: String, password: Zeroizing<String>) -> Result<Session, String> {
    if password.chars().count() < 10 || password.len() > 72 { return Err("Password must be at least 10 characters and at most 72 UTF-8 bytes".into()); }
    authenticate(&server, "api/auth/signup", json!({ "fullName": name.trim(), "username": username.trim(), "email": email.trim(), "password": &*password }), "Could not create your account").await
}

/// Forgotten password: proves the recovery key and sets a new password.
pub async fn recover(server: String, email: String, proof: String, new_password: Zeroizing<String>) -> Result<Session, String> {
    if new_password.chars().count() < 10 || new_password.len() > 72 { return Err("Password must be at least 10 characters and at most 72 UTF-8 bytes".into()); }
    authenticate(&server, "api/auth/recover", json!({ "email": email.trim(), "proof": proof, "newPassword": &*new_password }), "That email and recovery key do not match.").await
}

/// Rebuilds a session from a stored cookie and confirms it is still valid.
pub async fn restore(server: String, cookie: String) -> Result<Session, String> {
    let base = crate::api::server_url(&server)?;
    let session = Session { account: Account::default(), cookie, base, client: http()? };
    let account = session.check().await?;
    Ok(Session { account, ..session })
}

impl Session {
    pub fn origin(&self) -> String { self.base.origin().ascii_serialization() }
    pub fn me(&self) -> &str { self.account.id.as_deref().unwrap_or_default() }

    async fn send(&self, method: Method, path: &str, body: Option<Value>, fallback: &str) -> Result<Response, String> {
        let url = self.base.join(path).map_err(|_| "Invalid server address.")?;
        // The server's CSRF guard accepts its own origin for state changes.
        let mut request = self.client.request(method, url).header(header::COOKIE, &self.cookie).header(header::ORIGIN, self.origin());
        if let Some(body) = body { request = request.json(&body); }
        let response = request.send().await.map_err(|_| UNREACHABLE)?;
        if !response.status().is_success() { return Err(failure(response, fallback).await); }
        Ok(response)
    }
    async fn call<T: DeserializeOwned>(&self, method: Method, path: &str, body: Option<Value>, fallback: &str) -> Result<T, String> {
        let response = self.send(method, path, body, fallback).await?;
        let bytes = response.bytes().await.map_err(|_| "Unexpected response from Sonar.")?;
        if bytes.len() > 8 * 1024 * 1024 { return Err("Unexpected response from Sonar.".into()); }
        serde_json::from_slice(&bytes).map_err(|_| "Unexpected response from Sonar.".into())
    }

    pub async fn check(&self) -> Result<Account, String> { self.call(Method::GET, "api/auth/check", None, "Your session has ended. Sign in again.").await }
    pub async fn sign_out_everywhere(self) -> Result<(), String> { self.send(Method::POST, "api/auth/logout", None, "Could not sign out").await.map(|_| ()) }

    // Encryption key and its password-protected backup.
    pub async fn key_backup(&self) -> Result<Option<Backup>, String> {
        #[derive(Deserialize)] struct Body { backup: Option<Backup> }
        Ok(self.call::<Body>(Method::GET, "api/auth/key-backup", None, "Could not load your encrypted key").await?.backup)
    }
    pub async fn publish_key(&self, public_key: &str, backup: Option<&Backup>, verifier: Option<&str>) -> Result<Account, String> {
        let mut body = json!({ "publicKey": public_key });
        if let Some(backup) = backup { body["backup"] = serde_json::to_value(backup).map_err(|_| "Invalid key")?; }
        if let Some(verifier) = verifier { body["recoveryVerifier"] = json!(verifier); }
        self.call(Method::PUT, "api/auth/encryption-key", Some(body), "Could not save the encryption key").await
    }
    pub async fn rotate_key(&self, password: &str, previous: Option<&str>, public_key: &str, backup: &Backup, verifier: &str) -> Result<Account, String> {
        self.call(Method::POST, "api/auth/encryption-key/rotate", Some(json!({ "currentPassword": password, "previousKey": previous, "publicKey": public_key, "backup": backup, "recoveryVerifier": verifier })), "Could not replace the encryption key").await
    }
    /// Sets up or replaces the recovery key (the server checks the password).
    pub async fn set_recovery(&self, password: &str, backup: &Backup, verifier: &str) -> Result<(), String> {
        self.send(Method::PUT, "api/auth/recovery", Some(json!({ "currentPassword": password, "backup": backup, "recoveryVerifier": verifier })), "Could not save the recovery key").await.map(|_| ())
    }
    pub async fn set_password(&self, current: &str, next: &str, key_backup: Option<&Backup>) -> Result<(), String> {
        let mut body = json!({ "currentPassword": current, "newPassword": next });
        if let Some(backup) = key_backup { body["keyBackup"] = serde_json::to_value(backup).map_err(|_| "Invalid key")?; }
        self.send(Method::PUT, "api/auth/password", Some(body), "Could not change password").await.map(|_| ())
    }
    /// Stores already-encrypted bytes; `target` is `to=<id>` or `community=<id>`.
    pub async fn upload_file(&self, target: &str, ciphertext: Vec<u8>) -> Result<String, String> {
        #[derive(Deserialize)] struct Body { id: String }
        let url = self.base.join(&format!("api/attachments?{target}")).map_err(|_| "Invalid server address.")?;
        let response = self.client.post(url).header(header::COOKIE, &self.cookie).header(header::ORIGIN, self.origin()).header(header::CONTENT_TYPE, "application/octet-stream").body(ciphertext).send().await.map_err(|_| UNREACHABLE)?;
        if !response.status().is_success() { return Err(failure(response, "Could not send the file").await); }
        Ok(response.json::<Body>().await.map_err(|_| "Unexpected response from Sonar.")?.id)
    }
    pub async fn download_file(&self, id: &str) -> Result<Vec<u8>, String> {
        let response = self.send(Method::GET, &format!("api/attachments/{}", check_id(id)?), None, "This file is no longer available").await?;
        let bytes = response.bytes().await.map_err(|_| "Could not download the file.")?;
        if bytes.len() > crate::message_content::MAX_FILE_BYTES + 16 { return Err("Unexpected response from Sonar.".into()); }
        Ok(bytes.to_vec())
    }
    pub async fn delete_file(&self, id: &str) -> Result<(), String> { self.send(Method::DELETE, &format!("api/attachments/{}", check_id(id)?), None, "Could not delete the file").await.map(|_| ()) }
    pub async fn friend_requests(&self) -> Result<crate::api::FriendRequests, String> { self.call(Method::GET, "api/friends", None, "Could not load friend requests").await }
    pub async fn send_friend_request(&self, username: &str) -> Result<(), String> {
        self.send(Method::POST, "api/friends/request", Some(json!({ "username": username })), "Could not send the friend request").await.map(|_| ())
    }
    pub async fn answer_friend_request(&self, id: &str, accept: bool) -> Result<(), String> {
        let id = check_id(id)?;
        if accept { self.send(Method::POST, &format!("api/friends/{id}/accept"), None, "Could not accept the request").await.map(|_| ()) }
        else { self.send(Method::DELETE, &format!("api/friends/{id}"), None, "Could not remove the request").await.map(|_| ()) }
    }
    pub async fn set_presence(&self, presence: &str) -> Result<(), String> {
        self.send(Method::PUT, "api/auth/presence", Some(json!({ "presence": presence })), "Could not change your status").await.map(|_| ())
    }
    pub async fn update_profile_details(&self, name: &str, username: &str) -> Result<Account, String> {
        self.call(Method::PUT, "api/auth/update-profile", Some(json!({ "fullName": name.trim(), "username": username })), "Could not update profile").await
    }
    pub async fn sessions_list(&self) -> Result<Sessions, String> { self.call(Method::GET, "api/auth/sessions", None, "Could not load sessions").await }
    pub async fn end_session(&self, id: &str) -> Result<bool, String> {
        #[derive(Deserialize)] struct Body { current: bool }
        if id.len() != 64 || !id.bytes().all(|c| c.is_ascii_hexdigit()) { return Err("Session unavailable".into()); }
        Ok(self.call::<Body>(Method::DELETE, &format!("api/auth/sessions/{id}"), None, "Could not sign out that session").await?.current)
    }
    pub async fn passkey_list(&self) -> Result<Vec<Passkey>, String> { self.call(Method::GET, "api/auth/passkeys", None, "Could not load passkeys").await }
    pub async fn delete_passkey(&self, id: &str, password: &str) -> Result<(), String> {
        self.send(Method::DELETE, &format!("api/auth/passkeys/{}", check_id(id)?), Some(json!({ "currentPassword": password })), "Could not remove passkey").await.map(|_| ())
    }

    // Direct messages.
    pub async fn people(&self) -> Result<Vec<Person>, String> { self.call(Method::GET, "api/messages/users", None, "Could not load people").await }
    pub async fn direct_page(&self, peer: &str, before: Option<&str>, pinned: bool) -> Result<MessagePage, String> {
        let mut path = format!("api/messages/{}?page=true&pinned={pinned}", check_id(peer)?);
        if let Some(cursor) = before { path.push_str(&format!("&before={}", url::form_urlencoded::byte_serialize(cursor.as_bytes()).collect::<String>())); }
        self.call(Method::GET, &path, None, "Could not load messages").await
    }
    pub async fn send_direct(&self, peer: &str, envelope: crate::crypto::Envelope) -> Result<MessageEnvelope, String> {
        self.call(Method::POST, &format!("api/messages/send/{}", check_id(peer)?), Some(json!({ "version": 1, "iv": envelope.iv, "ciphertext": envelope.ciphertext })), "Could not send message").await
    }
    pub async fn pin_direct(&self, id: &str, pinned: bool) -> Result<MessageEnvelope, String> {
        self.call(Method::PATCH, &format!("api/messages/{}/pin", check_id(id)?), Some(json!({ "pinned": pinned })), "Could not update the pin").await
    }
    pub async fn delete_direct(&self, id: &str) -> Result<(), String> {
        self.send(Method::DELETE, &format!("api/messages/{}", check_id(id)?), None, "Could not delete message").await.map(|_| ())
    }

    // Communities.
    pub async fn communities(&self) -> Result<Vec<Community>, String> { self.call(Method::GET, "api/communities", None, "Could not load communities").await }
    pub async fn community(&self, id: &str) -> Result<Community, String> { self.call(Method::GET, &format!("api/communities/{}", check_id(id)?), None, "Community unavailable").await }
    pub async fn new_community(&self, name: &str) -> Result<Community, String> { self.call(Method::POST, "api/communities", Some(json!({ "name": name.trim() })), "Could not create community").await }
    pub async fn invite_preview(&self, token: &str) -> Result<InvitePreview, String> { self.call(Method::POST, "api/communities/invite-preview", Some(json!({ "token": token })), "Could not load this invite").await }
    pub async fn join(&self, token: &str) -> Result<Community, String> { self.call(Method::POST, "api/communities/join", Some(json!({ "token": token })), "Could not join community").await }
    pub async fn new_channel(&self, community: &str, name: &str, kind: &str) -> Result<Community, String> {
        self.call(Method::POST, &format!("api/communities/{}/channels", check_id(community)?), Some(json!({ "name": name.trim(), "type": kind })), "Could not create channel").await
    }
    pub async fn rename(&self, community: &str, channel: Option<&str>, name: &str) -> Result<(), String> {
        let path = match channel { Some(channel) => format!("api/communities/{}/channels/{}", check_id(community)?, check_id(channel)?), None => format!("api/communities/{}", check_id(community)?) };
        self.send(Method::PATCH, &path, Some(json!({ "name": name.trim() })), "Could not rename").await.map(|_| ())
    }
    pub async fn delete_channel(&self, community: &str, channel: &str) -> Result<(), String> {
        self.send(Method::DELETE, &format!("api/communities/{}/channels/{}", check_id(community)?, check_id(channel)?), None, "Could not delete channel").await.map(|_| ())
    }
    pub async fn delete_community(&self, community: &str) -> Result<(), String> {
        self.send(Method::DELETE, &format!("api/communities/{}", check_id(community)?), None, "Could not delete community").await.map(|_| ())
    }
    pub async fn invite(&self, community: &str) -> Result<CreatedInvite, String> {
        self.call(Method::POST, &format!("api/communities/{}/invite", check_id(community)?), None, "Could not create invite").await
    }
    pub async fn member_action(&self, community: &str, member: &str, action: &str) -> Result<(), String> {
        let (community, member) = (check_id(community)?, check_id(member)?);
        let base = format!("api/communities/{community}");
        let (method, path, body) = match action {
            "leave" => (Method::POST, format!("{base}/leave"), None),
            "remove" => (Method::DELETE, format!("{base}/members/{member}"), None),
            "ban" => (Method::POST, format!("{base}/members/{member}/ban"), None),
            "unban" => (Method::DELETE, format!("{base}/bans/{member}"), None),
            "transfer" => (Method::POST, format!("{base}/transfer"), Some(json!({ "memberId": member }))),
            "promote" | "demote" => (Method::PUT, format!("{base}/members/{member}/moderator"), Some(json!({ "moderator": action == "promote" }))),
            _ => return Err("Unknown action".into()),
        };
        self.send(method, &path, body, "Could not update membership").await.map(|_| ())
    }
    pub async fn list_bans(&self, community: &str) -> Result<Vec<Person>, String> { self.call(Method::GET, &format!("api/communities/{}/bans", check_id(community)?), None, "Could not load bans").await }
    pub async fn channel_page(&self, community: &str, channel: &str, before: Option<&str>, pinned: bool) -> Result<MessagePage, String> {
        let mut path = format!("api/communities/{}/channels/{}/messages?page=true&pinned={pinned}", check_id(community)?, check_id(channel)?);
        if let Some(cursor) = before { path.push_str(&format!("&before={}", url::form_urlencoded::byte_serialize(cursor.as_bytes()).collect::<String>())); }
        self.call(Method::GET, &path, None, "Could not load channel").await
    }
    pub async fn send_channel(&self, community: &str, channel: &str, revision: u64, envelopes: Vec<Value>) -> Result<MessageEnvelope, String> {
        self.call(Method::POST, &format!("api/communities/{}/channels/{}/messages", check_id(community)?, check_id(channel)?), Some(json!({ "revision": revision, "envelopes": envelopes })), "Could not send message").await
    }
    pub async fn pin_channel(&self, community: &str, channel: &str, id: &str, pinned: bool) -> Result<MessageEnvelope, String> {
        self.call(Method::PATCH, &format!("api/communities/{}/channels/{}/messages/{}/pin", check_id(community)?, check_id(channel)?, check_id(id)?), Some(json!({ "pinned": pinned })), "Could not update the pin").await
    }
    pub async fn delete_channel_message(&self, community: &str, channel: &str, id: &str) -> Result<(), String> {
        self.send(Method::DELETE, &format!("api/communities/{}/channels/{}/messages/{}", check_id(community)?, check_id(channel)?, check_id(id)?), None, "Could not delete message").await.map(|_| ())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnreadCandidate { #[serde(default)] pub sender_id: Option<String>, #[serde(default)] pub channel_id: Option<String>, pub message_id: String, pub created_at: String }

impl Session {
    /// Latest live message per sender (direct) and per channel, for unread dots.
    pub async fn unread_candidates(&self, communities: Vec<String>) -> Result<Vec<(String, (i64, String))>, String> {
        #[derive(Deserialize)] struct Page { items: Vec<UnreadCandidate>, next: Option<String> }
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        for _ in 0..50 {
            let path = match &after { Some(cursor) => format!("api/messages/unread-candidates?after={}", check_id(cursor)?), None => "api/messages/unread-candidates".into() };
            let page: Page = self.call(Method::GET, &path, None, "Could not load unread indicators").await?;
            for item in page.items {
                if let (Some(sender), Some(time)) = (item.sender_id, crate::chat::timestamp(&item.created_at)) { out.push((sender, (time, item.message_id))); }
            }
            match page.next { Some(next) if Some(&next) != after.as_ref() => after = Some(next), _ => break }
        }
        for community in communities {
            let Ok(items) = self.call::<Vec<UnreadCandidate>>(Method::GET, &format!("api/communities/{}/unread-candidates", check_id(&community)?), None, "").await else { continue };
            for item in items {
                if let (Some(channel), Some(time)) = (item.channel_id, crate::chat::timestamp(&item.created_at)) { out.push((format!("{community}:{channel}"), (time, item.message_id))); }
            }
        }
        Ok(out)
    }
}

impl Session {
    pub async fn set_voice_lock(&self, community: &str, channel: &str, locked: bool) -> Result<(), String> {
        self.send(Method::PATCH, &format!("api/communities/{}/channels/{}/voice-lock", check_id(community)?, check_id(channel)?), Some(json!({ "locked": locked })), "Could not update the voice room").await.map(|_| ())
    }
}
