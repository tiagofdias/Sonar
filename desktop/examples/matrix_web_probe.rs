//! Synthetic Matrix interoperability helper. No real accounts or network.
#![allow(dead_code)]
#[path = "../src/api.rs"] mod api;
#[path = "../src/passkey.rs"] mod passkey;
#[path = "../src/live.rs"] mod live;
#[path = "../src/ice_candidate.rs"] mod ice_candidate;
#[path = "../src/crypto.rs"] mod crypto;
#[path = "../src/key_store.rs"] mod key_store;
#[path = "../src/matrix_store.rs"] mod matrix_store;
#[path = "../src/matrix_binding.rs"] mod matrix_binding;
#[path = "../src/matrix_history.rs"] mod matrix_history;
use std::io::{self, BufRead, Write};
use matrix_sdk_crypto::{CollectStrategy, DecryptionSettings, EncryptionSettings, TrustRequirement, types::requests::AnyOutgoingRequest};
use ruma::{api::client::{keys::{get_keys, upload_keys}, to_device::send_event_to_device}, events::room::message::RoomMessageEventContent, RoomId, UserId};
use serde_json::{json, Value};

fn receive() -> Result<Value, Box<dyn std::error::Error>> {
    let line = io::stdin().lock().lines().next().ok_or("missing input")??;
    if line.len() > 300_000 { return Err("input too large".into()); }
    Ok(serde_json::from_str(&line)?)
}
fn send(value: Value) -> io::Result<()> { println!("{value}"); io::stdout().flush() }

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::PathBuf::from(std::env::var_os("SONAR_MATRIX_PROBE_ROOT").ok_or("test root missing")?);
    let origin = "http://127.0.0.1:5000";
    let account = "abcdef0123456789abcdef01";
    let user = "@abcdef0123456789abcdef01:sonar.invalid";
    let browser = UserId::parse("@0123456789abcdef01234567:sonar.invalid")?;
    if let Some(name) = std::env::args().nth(1) {
        let store = matrix_store::open(&root, origin, account, user, "NATIVE", &name).await?;
        let machine = &store.machine;
        let room = RoomId::parse("!synthetic-room:sonar.invalid")?;
        send(json!({"type":"reopened"}))?;
        let input = receive()?;
        let event = serde_json::from_value(input["event"].clone())?;
        let settings = DecryptionSettings { sender_device_trust_requirement: TrustRequirement::Untrusted };
        let decrypted = machine.decrypt_room_event(&event, &room, &settings).await?;
        send(json!({"type":"history", "event":decrypted.event}))?;
        let outgoing_settings = EncryptionSettings { sharing_strategy: CollectStrategy::AllDevices, ..Default::default() };
        let shares = machine.share_room_key(&room, [browser.as_ref()].into_iter(), outgoing_settings).await?;
        if shares.len() != 1 { return Err("missing return room key share".into()); }
        let share = &shares[0];
        send(json!({"type":"share", "txnId":share.txn_id, "eventType":share.event_type, "messages":share.messages}))?;
        if receive()?["type"] != "ack" { return Err("missing share acknowledgement".into()); }
        machine.mark_request_as_sent(&share.txn_id, &send_event_to_device::v3::Response::new()).await?;
        let encrypted = machine.encrypt_room_event(&room, RoomMessageEventContent::text_plain("synthetic native to web message")).await?;
        send(json!({"type":"encrypted", "content":encrypted.content}))?;
        store.close().await?;
        return Ok(());
    }
    let (name, mut store) = matrix_store::create(&root, origin, account, user, "NATIVE").await?;
    let machine = &store.machine;
    let requests = machine.outgoing_requests().await?;
    let upload = requests.iter().find(|request| matches!(request.request(), AnyOutgoingRequest::KeysUpload(_))).ok_or("missing upload")?;
    let AnyOutgoingRequest::KeysUpload(body) = upload.request() else { unreachable!() };
    send(json!({"type":"upload", "body": {"device_keys":body.device_keys, "one_time_keys":body.one_time_keys}}))?;
    let enrollment = receive()?;
    let enrollment_token = enrollment["token"].as_str().ok_or("missing enrollment token")?;
    let enrollment_message = enrollment["message"].as_str().ok_or("missing enrollment message")?;
    let enrollment_expiry = enrollment["expiresUnixMs"].as_u64().ok_or("missing enrollment expiry")?;
    let device_keys = serde_json::to_value(&body.device_keys)?;
    if matrix_binding::sign_enrollment(machine, user, "NATIVE", &device_keys, enrollment_token, &format!("{enrollment_message}\nextra"), enrollment_expiry).await.is_ok() { return Err("invalid enrollment accepted".into()); }
    let mut substituted = device_keys.clone(); substituted["keys"]["curve25519:NATIVE"] = json!("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    if matrix_binding::sign_enrollment(machine, user, "NATIVE", &substituted, enrollment_token, enrollment_message, enrollment_expiry).await.is_ok() { return Err("substituted key accepted".into()); }
    let enrollment_proof = matrix_binding::sign_enrollment(machine, user, "NATIVE", &device_keys, enrollment_token, enrollment_message, enrollment_expiry).await?;
    send(json!({"type":"enrollment", "signature":enrollment_proof}))?;
    let binding = receive()?;
    let token = binding["token"].as_str().ok_or("missing token")?;
    let message = binding["message"].as_str().ok_or("missing message")?;
    let expiry = binding["expiresUnixMs"].as_u64().ok_or("missing expiry")?;
    if matrix_binding::sign_binding(machine, user, "NATIVE", token, &format!("{message}\nextra"), expiry).await.is_ok() || matrix_binding::sign_binding(machine, user, "OTHER", token, message, expiry).await.is_ok() || matrix_binding::sign_binding(machine, user, "NATIVE", token, message, 0).await.is_ok() { return Err("invalid challenge accepted".into()); }
    let proof = matrix_binding::sign_binding(machine, user, "NATIVE", token, message, expiry).await?;
    send(json!({"type":"binding", "signature":proof}))?;
    let setup = receive()?;
    let upload_response = upload_keys::v3::Response::new(serde_json::from_value(setup["upload"]["one_time_key_counts"].clone())?);
    machine.mark_request_as_sent(upload.request_id(), &upload_response).await?;
    machine.update_tracked_users([browser.as_ref()]).await?;
    let requests = machine.outgoing_requests().await?;
    let query = requests.iter().find(|request| matches!(request.request(), AnyOutgoingRequest::KeysQuery(_))).ok_or("missing query")?;
    let mut query_response = get_keys::v3::Response::new();
    query_response.device_keys = serde_json::from_value(setup["query"]["device_keys"].clone())?;
    machine.mark_request_as_sent(query.request_id(), &query_response).await?;
    send(json!({"type":"ready"}))?;
    let input = receive()?;
    // Untrusted is used only for isolated interoperability; production must
    // require verified identities and verified devices.
    let settings = DecryptionSettings { sender_device_trust_requirement: TrustRequirement::Untrusted };
    let web_device = machine.get_device(&browser, "WEB".into(), None).await?.ok_or("web device missing")?;
    let trusted = vec![api::TrustedMatrixDevice { user_id: browser.to_string(), device_id: "WEB".into(), ed25519: web_device.ed25519_key().ok_or("signing key missing")?.to_base64(), curve25519: web_device.curve25519_key().ok_or("curve key missing")?.to_base64() }];
    let snapshot = api::MatrixRoomSnapshot { room_id: "!synthetic-room:sonar.invalid".into(), revision: 0, members: vec![user.into(), browser.to_string()], kind: "direct".into() };
    let test_session = api::synthetic_session_at(&std::env::var("SONAR_MATRIX_INBOX_TEST_BASE")?);
    if store.receive_room_keys(&test_session, &snapshot, &[]).await.is_ok() { return Err("untrusted room-key sender accepted".into()); }
    if store.receive_room_keys(&test_session, &snapshot, &trusted).await.is_ok() { return Err("failed acknowledgement was accepted".into()); }
    if store.receive_room_keys(&test_session, &snapshot, &trusted).await? != 1 { return Err("room key retry failed".into()); }
    let machine = &store.machine;
    let event = serde_json::from_value(input["event"].clone())?;
    let room = RoomId::parse("!synthetic-room:sonar.invalid")?;
    let decrypted = machine.decrypt_room_event(&event, &room, &settings).await?;
    let row = api::MatrixHistoryEvent { id: "0123456789abcdef01234567".into(), room_id: room.to_string(), sender: browser.to_string(), sender_device: "WEB".into(), content: input["event"]["content"].clone(), pinned: false, created_at: "2023-11-14T22:13:20.000Z".into(), expires_at: Some("2099-01-01T00:00:00.000Z".into()) };
    if matrix_history::decrypt_text(machine, &row, &trusted).await.as_deref() != Some("synthetic web to native message") { return Err("trusted history decryption failed".into()); }
    if matrix_history::decrypt_text(machine, &row, &[]).await.is_some() { return Err("untrusted history was exposed".into()); }
    send(json!({"type":"result", "event":decrypted.event}))?;
    store.close().await?;
    send(json!({"type":"closed", "name":name}))?;
    Ok(())
}
