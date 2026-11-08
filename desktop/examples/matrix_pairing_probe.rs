//! Integration-only bridge for a synthetic browser/native verification test.
#![allow(dead_code)]
#[path = "../src/api.rs"] mod api;
#[path = "../src/passkey.rs"] mod passkey;
#[path = "../src/live.rs"] mod live;
#[path = "../src/ice_candidate.rs"] mod ice_candidate;
#[path = "../src/crypto.rs"] mod crypto;
#[path = "../src/key_store.rs"] mod key_store;
#[path = "../src/matrix_store.rs"] mod matrix_store;
#[path = "../src/matrix_binding.rs"] mod matrix_binding;
#[path = "../src/matrix_verification.rs"] mod matrix_verification;
use std::io::{self, BufRead, Write};

fn emit(value: serde_json::Value) -> io::Result<()> { println!("{value}"); io::stdout().flush() }

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::var("SONAR_PAIR_TEST_BASE")?;
    let root = std::path::PathBuf::from(std::env::var_os("SONAR_PAIR_TEST_ROOT").ok_or("test root missing")?);
    let target = std::env::var("SONAR_PAIR_TEST_TARGET")?;
    let session = api::login(base.clone(), "pairing-test@example.invalid".into(), "synthetic-password".into()).await?;
    let account = session.account.id.as_deref().ok_or("account missing")?;
    let user = format!("@{account}:sonar.invalid");
    let (_, store) = matrix_store::create(&root, &base, account, &user, "NATIVEPAIR").await?;
    session.register_matrix_device(&store.machine).await?;
    session.publish_matrix_one_time_keys(&store.machine).await?;
    let reverse = std::env::var_os("SONAR_PAIR_TEST_REVERSE").is_some();
    let mut flow = if reverse {
        let mut listener = matrix_verification::DeviceListener::new(store, session);
        emit(serde_json::json!({"type":"listening", "device":"NATIVEPAIR"}))?;
        let mut lines = io::stdin().lock().lines();
        if lines.next().ok_or("missing listen command")?? != "accept" { return Err("invalid test command".into()); }
        let requests = listener.requests().await?;
        let (flow, _) = requests.into_iter().find(|(_, device)| device == &target).ok_or("request missing")?;
        listener.accept(&flow).await?
    } else { matrix_verification::DeviceVerification::start(store, session, target).await? };
    emit(serde_json::json!({"type":"started", "device":"NATIVEPAIR", "flow":flow.flow_id()}))?;
    for line in io::stdin().lock().lines() {
        match line?.as_str() {
            "poll" => { flow.poll().await?; emit(serde_json::json!({"type":"state", "codes":flow.codes(), "done":flow.done()}))?; }
            "confirm" => { flow.confirm().await?; emit(serde_json::json!({"type":"confirmed"}))?; }
            "approve" => { flow.approve().await?; emit(serde_json::json!({"type":"approved"}))?; }
            "cancel" => { flow.cancel().await?; emit(serde_json::json!({"type":"cancelled"}))?; break; }
            "close" => break,
            _ => return Err("invalid test command".into()),
        }
    }
    flow.close().await?;
    Ok(())
}
