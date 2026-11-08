//! Synthetic browser/native contact verification integration helper.
#![allow(dead_code)]
#[path = "../src/api.rs"] mod api;
#[path = "../src/passkey.rs"] mod passkey;
#[path = "../src/live.rs"] mod live;
#[path = "../src/ice_candidate.rs"] mod ice_candidate;
#[path = "../src/crypto.rs"] mod crypto;
#[path = "../src/key_store.rs"] mod key_store;
#[path = "../src/matrix_store.rs"] mod matrix_store;
#[path = "../src/matrix_binding.rs"] mod matrix_binding;
#[path = "../src/matrix_contact.rs"] mod matrix_contact;
#[path = "../src/matrix_history.rs"] mod matrix_history;
use std::io::{self, BufRead, Write};
fn emit(value: serde_json::Value) -> io::Result<()> { println!("{value}"); io::stdout().flush() }
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::var("SONAR_CONTACT_TEST_BASE")?;
    let root = std::path::PathBuf::from(std::env::var_os("SONAR_CONTACT_TEST_ROOT").ok_or("test root missing")?);
    let peer = std::env::var("SONAR_CONTACT_TEST_PEER")?;
    let session = api::login(base.clone(), "native-contact-test@example.invalid".into(), "synthetic-password".into()).await?;
    let account = session.account.id.as_deref().ok_or("account missing")?;
    let user = format!("@{account}:sonar.invalid");
    let (_, store) = matrix_store::create(&root, &base, account, &user, "NATIVECONTACT").await?;
    session.register_matrix_device(&store.machine).await?;
    session.publish_matrix_one_time_keys(&store.machine).await?;
    emit(serde_json::json!({"type":"registered"}))?;
    let mut lines = io::stdin().lock().lines();
    let command = lines.next().ok_or("missing start")??;
    let mut flow = if command == "start" {
        matrix_contact::ContactVerification::start(store, session.clone(), &peer, "BROWSER").await?
    } else if command == "listen" {
        let mut listener = matrix_contact::ContactListener::open(store, session.clone(), &peer, "BROWSER").await?;
        emit(serde_json::json!({"type":"listening"}))?;
        if lines.next().ok_or("missing request check")?? != "check" { return Err("invalid request check".into()); }
        let requests = listener.requests().await?;
        let id = requests.first().ok_or("incoming request missing")?;
        listener.accept(id).await?
    } else { return Err("invalid start".into()); };
    emit(serde_json::json!({"type":"started", "flow":flow.flow_id()}))?;
    while let Some(line) = lines.next() {
        match line?.as_str() {
            "poll" => { flow.poll().await?; emit(serde_json::json!({"type":"state", "codes":flow.codes(), "done":flow.done()}))?; }
            "confirm" => { flow.confirm().await?; emit(serde_json::json!({"type":"confirmed"}))?; }
            "send" => {
                if !flow.done() { return Err("verification incomplete".into()); }
                let mut store = flow.into_store();
                let room = store.prepare_direct_room(&session, &peer).await?;
                store.queue_room_text_with_reply(&session, &room, "synthetic verified native message", Some("0123456789abcdef01234567")).await?;
                let id = store.deliver_pending_room_text(&session, &room).await?;
                emit(serde_json::json!({"type":"sent", "id":id}))?;
                if lines.next().ok_or("missing reaction command")?? != "react" { return Err("invalid reaction command".into()); }
                store.queue_room_reaction(&session, &room, &id, "👍").await?;
                let reaction_id = store.deliver_pending_room_text(&session, &room).await?;
                emit(serde_json::json!({"type":"reacted", "id":reaction_id}))?;
                if lines.next().ok_or("missing unreact command")?? != "unreact" { return Err("invalid unreact command".into()); }
                session.delete_matrix_message(&store.machine, &room, &reaction_id).await?;
                emit(serde_json::json!({"type":"unreacted"}))?;
                if lines.next().ok_or("missing read command")?? != "read" { return Err("invalid read command".into()); }
                let trusted = session.trusted_matrix_room_devices(&store.machine, &room).await?;
                store.receive_room_keys(&session, &room, &trusted).await?;
                let page = session.matrix_room_history(&store.machine, &room, None, false).await?;
                let mut found = false;
                for row in &page.messages { if matrix_history::decrypt_text(&store.machine, row, &trusted).await.as_deref() == Some("synthetic verified browser message") { found = true; } }
                if !found { return Err("browser message not decrypted".into()); }
                emit(serde_json::json!({"type":"read"}))?;
                if lines.next().ok_or("missing rotated read command")?? != "read-rotated" { return Err("invalid rotated read command".into()); }
                let rotated_room = store.prepare_direct_room(&session, &peer).await?;
                if rotated_room.revision <= room.revision { return Err("room revision did not advance".into()); }
                let trusted = session.trusted_matrix_room_devices(&store.machine, &rotated_room).await?;
                store.receive_room_keys(&session, &rotated_room, &trusted).await?;
                let page = session.matrix_room_history(&store.machine, &rotated_room, None, false).await?;
                let mut found = false;
                for row in &page.messages { if matrix_history::decrypt_text(&store.machine, row, &trusted).await.as_deref() == Some("browser message after revocation") { found = true; } }
                if !found { return Err("rotated browser message not decrypted".into()); }
                emit(serde_json::json!({"type":"read-rotated"}))?;
                store.close().await?;
                return Ok(());
            }
            "close" => break,
            _ => return Err("invalid command".into()),
        }
    }
    flow.into_store().close().await?;
    Ok(())
}
