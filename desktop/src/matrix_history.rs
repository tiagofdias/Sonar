//! Decrypt room history only after matching the SDK's sender identity and keys
//! against the freshly verified room device list.
#![allow(dead_code)]
use crate::api::{MatrixHistoryEvent, TrustedMatrixDevice};
use matrix_sdk_crypto::{DecryptionSettings, OlmMachine, TrustRequirement};

pub async fn decrypt_text(machine: &OlmMachine, row: &MatrixHistoryEvent, trusted: &[TrustedMatrixDevice]) -> Option<String> {
    decrypt_message(machine, row, trusted).await.map(|message| message.0)
}
pub async fn decrypt_message(machine: &OlmMachine, row: &MatrixHistoryEvent, trusted: &[TrustedMatrixDevice]) -> Option<(String, Option<String>)> {
    match decrypt_content(machine, row, trusted).await? { MatrixContent::Text(text, reply) => Some((text, reply)), MatrixContent::Reaction(_, _) | MatrixContent::Attachment(_) => None }
}
#[derive(Clone)]
pub struct MatrixAttachment { pub id: String, pub name: String, pub size: usize, pub metadata: String }
impl Drop for MatrixAttachment { fn drop(&mut self) { use zeroize::Zeroize; self.metadata.zeroize(); } }
pub enum MatrixContent { Text(String, Option<String>), Reaction(String, String), Attachment(MatrixAttachment) }
pub async fn decrypt_content(machine: &OlmMachine, row: &MatrixHistoryEvent, trusted: &[TrustedMatrixDevice]) -> Option<MatrixContent> {
    let expected = trusted.iter().find(|device| device.user_id == row.sender && device.device_id == row.sender_device)?;
    let room = ruma::RoomId::parse(&row.room_id).ok()?;
    let created = chrono::DateTime::parse_from_rfc3339(&row.created_at).ok()?.timestamp_millis();
    let raw = serde_json::from_value(serde_json::json!({"type":"m.room.encrypted", "event_id":format!("${}", row.id), "sender":row.sender, "origin_server_ts":created, "content":row.content})).ok()?;
    let settings = DecryptionSettings { sender_device_trust_requirement: TrustRequirement::Untrusted };
    let decrypted = machine.decrypt_room_event(&raw, &room, &settings).await.ok()?;
    let info = &decrypted.encryption_info;
    if info.sender.as_str() != row.sender || info.sender_device.as_ref()?.as_str() != row.sender_device || info.forwarder.is_some() { return None; }
    let algorithm = serde_json::to_value(&info.algorithm_info).ok()?;
    let megolm = algorithm.get("MegolmV1AesSha2")?;
    if megolm.get("curve25519_key")?.as_str()? != expected.curve25519 || megolm.get("sender_claimed_keys")?.get("ed25519")?.as_str()? != expected.ed25519 { return None; }
    let event: serde_json::Value = serde_json::from_str(decrypted.event.json().get()).ok()?;
    if event["type"] == "m.reaction" {
        let relation = event["content"].get("m.relates_to")?;
        let id = relation.get("event_id")?.as_str()?.strip_prefix('$')?;
        let emoji = relation.get("key")?.as_str()?;
        if relation["rel_type"] != "m.annotation" || id.len() != 24 || !id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) || !["👍", "❤️", "😂", "🎉"].contains(&emoji) { return None; }
        return Some(MatrixContent::Reaction(id.to_owned(), emoji.to_owned()));
    }
    if event["type"] != "m.room.message" { return None; }
    if event["content"]["msgtype"] == "m.file" {
        let name = event["content"]["body"].as_str()?;
        if name.is_empty() || name.len() > 255 || name.chars().any(|c| c.is_control() || c == '/' || c == '\\') { return None; }
        let size = usize::try_from(event["content"]["info"]["size"].as_u64()?).ok()?;
        if size == 0 || size > 10 * 1024 * 1024 { return None; }
        let mut file = event["content"]["file"].as_object()?.clone();
        let url = file.remove("url")?.as_str()?.to_owned();
        let prefix = format!("mxc://{}/", machine.user_id().server_name());
        let id = url.strip_prefix(&prefix)?;
        if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) { return None; }
        let metadata = serde_json::to_string(&file).ok()?;
        if metadata.len() > 4096 || serde_json::from_str::<matrix_sdk_crypto::MediaEncryptionInfo>(&metadata).is_err() { return None; }
        return Some(MatrixContent::Attachment(MatrixAttachment { id: id.into(), name: name.into(), size, metadata }));
    }
    if event["content"]["msgtype"] != "m.text" { return None; }
    let text = event["content"]["body"].as_str()?;
    if text.len() > 32000 { return None; }
    let reply = match event["content"].get("m.relates_to").and_then(|value| value.get("m.in_reply_to")).and_then(|value| value.get("event_id")) {
        None => None,
        Some(value) => {
            let id = value.as_str()?.strip_prefix('$')?;
            if id.len() != 24 || !id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) { return None; }
            Some(id.to_owned())
        }
    };
    Some(MatrixContent::Text(text.to_owned(), reply))
}
