//! Synthetic integration helper; never uses a real account or store.
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
#[path = "../src/matrix_history.rs"] mod matrix_history;
#[path = "../src/matrix_attachment.rs"] mod matrix_attachment;
#[path = "../src/matrix_contact.rs"] mod matrix_contact;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::thread::Builder::new().stack_size(16 * 1024 * 1024).spawn(|| {
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap().block_on(run()).map_err(|error| error.to_string())
    })?.join().map_err(|_| "test thread failed")?.map_err(Into::into)
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::var("SONAR_MATRIX_TEST_BASE")?;
    let root = std::path::PathBuf::from(std::env::var_os("SONAR_MATRIX_TEST_ROOT").ok_or("missing test root")?);
    let session = api::login(base.clone(), "native-registration@example.invalid".into(), "synthetic-password".into()).await?;
    let account = session.account.id.as_deref().ok_or("missing account")?;
    let user = format!("@{account}:sonar.invalid");
    let (name, store) = matrix_store::create(&root, &base, account, &user, "NATIVE").await?;
    let key = store.machine.identity_keys().ed25519.to_base64();
    session.register_matrix_device(&store.machine).await?;
    session.publish_matrix_one_time_keys(&store.machine).await?;
    let pending = session.matrix_devices(&store.machine).await?;
    if pending.len() != 1 || pending[0].server_authorization != "pending" { return Err("pending device missing".into()); }
    let test_http = reqwest::Client::new();
    if !test_http.post(format!("{base}/synthetic/setup")).send().await?.status().is_success() { return Err("test setup failed".into()); }
    let authorized = session.matrix_devices(&store.machine).await?;
    if authorized.len() != 1 || authorized[0].server_authorization != "authorized" { return Err("authorized device missing".into()); }
    let second_session = api::login(base.clone(), "native-registration@example.invalid".into(), "synthetic-password".into()).await?;
    let (_second_name, second_store) = matrix_store::create(&root, &base, account, &user, "SECOND").await?;
    second_session.register_matrix_device(&second_store.machine).await?;
    second_session.publish_matrix_one_time_keys(&second_store.machine).await?;
    let devices = session.matrix_devices(&store.machine).await?;
    let target = devices.iter().find(|device| device.device_id == "SECOND").ok_or("second device missing")?;
    session.refresh_matrix_own_keys(&store.machine).await?;
    if store.machine.get_device(store.machine.user_id(), "SECOND".into(), None).await?.is_none() { return Err("second device keys missing from SDK".into()); }
    if !session.claim_matrix_own_key(&store.machine, "SECOND").await? { return Err("Olm session was not created".into()); }
    second_session.refresh_matrix_own_keys(&second_store.machine).await?;
    let mut flow_a = matrix_verification::DeviceVerification::start(store, session.clone(), "SECOND".into()).await?;
    let mut listener = matrix_verification::DeviceListener::new(second_store, second_session.clone());
    let requests = listener.requests().await?;
    if !requests.iter().any(|(flow, device)| flow == flow_a.flow_id() && device == "NATIVE") { return Err("incoming device verification request missing".into()); }
    let mut flow_b = listener.accept(flow_a.flow_id()).await?;
    if flow_a.approve().await.is_ok() { return Err("unverified device was approved".into()); }
    for _ in 0..8 {
        flow_a.poll().await?; flow_b.poll().await?;
        if flow_a.codes().is_some() && flow_b.codes().is_some() { break; }
    }
    if flow_a.codes().is_none() || flow_a.codes() != flow_b.codes() { return Err("workflow codes differ".into()); }
    flow_a.confirm().await?; flow_b.confirm().await?;
    for _ in 0..8 { flow_a.poll().await?; flow_b.poll().await?; if flow_a.done() && flow_b.done() { break; } }
    if !flow_a.done() || !flow_b.done() { return Err("workflow verification incomplete".into()); }
    if flow_b.approve().await.is_ok() { return Err("pending device approved another device".into()); }
    flow_a.approve().await?;
    let mut store = flow_a.into_store();
    let second_store = flow_b.into_store();
    let third_session = api::login(base.clone(), "native-registration@example.invalid".into(), "synthetic-password".into()).await?;
    let (_, third_store) = matrix_store::create(&root, &base, account, &user, "THIRD").await?;
    third_session.register_matrix_device(&third_store.machine).await?;
    third_session.publish_matrix_one_time_keys(&third_store.machine).await?;
    if !test_http.post(format!("{base}/synthetic/authorize-third")).send().await?.status().is_success() { return Err("third device setup failed".into()); }
    session.refresh_matrix_own_keys(&store.machine).await?;
    let third = store.machine.get_device(store.machine.user_id(), "THIRD".into(), None).await?.ok_or("third device missing")?;
    let trusted = vec![api::TrustedMatrixDevice { user_id:user.clone(), device_id:"THIRD".into(), ed25519:third.ed25519_key().ok_or("missing key")?.to_base64(), curve25519:third.curve25519_key().ok_or("missing key")?.to_base64() }];
    let room_snapshot = session.matrix_room_snapshot(&store.machine, "!native-synthetic:sonar.invalid").await?;
    if session.establish_matrix_room_sessions(&store.machine, &room_snapshot, &[]).await.is_ok() { return Err("untrusted key claim accepted".into()); }
    if session.establish_matrix_room_sessions(&store.machine, &room_snapshot, &trusted).await? != 1 { return Err("trusted room key claim failed".into()); }
    if !test_http.post(format!("{base}/synthetic/reset-third")).send().await?.status().is_success() { return Err("third device reset failed".into()); }
    session.revoke_matrix_device(&store.machine, &session.matrix_devices(&store.machine).await?.into_iter().find(|device| device.device_id == "THIRD").ok_or("third device missing")?).await?;
    third_store.close().await?;
    let mut changed = target.clone(); changed.ed25519 = "A".repeat(43);
    if session.revoke_matrix_device(&store.machine, &changed).await.is_ok() { return Err("changed device key accepted".into()); }
    session.revoke_matrix_device(&store.machine, target).await?;
    if session.matrix_devices(&store.machine).await?.len() != 1 { return Err("device was not removed".into()); }
    second_store.close().await?;
    let peer = std::env::var("SONAR_MATRIX_TEST_PEER")?;
    let peer_session = api::login(base.clone(), "native-peer@example.invalid".into(), "synthetic-password".into()).await?;
    let peer_user = format!("@{peer}:sonar.invalid");
    let (_, peer_store) = matrix_store::create(&root, &base, &peer, &peer_user, "PEER").await?;
    peer_session.register_matrix_device(&peer_store.machine).await?;
    peer_session.publish_matrix_one_time_keys(&peer_store.machine).await?;
    if !test_http.post(format!("{base}/synthetic/bootstrap-peer")).send().await?.status().is_success() { return Err("peer bootstrap failed".into()); }
    let direct = store.prepare_direct_room(&session, &peer).await?;
    if direct.kind != "direct" || direct.members.len() != 2 { return Err("invalid direct room".into()); }
    if session.trusted_matrix_room_devices(&store.machine, &direct).await.is_ok() { return Err("unverified peer was trusted".into()); }
    let discovered = session.matrix_room_devices(&store.machine, &direct, false).await?;
    if !discovered.iter().any(|device| device.user_id == peer_user && device.device_id == "PEER") { return Err("contact device missing".into()); }
    if !session.claim_matrix_contact_key(&store.machine, &direct, &peer_user, "PEER").await? { return Err("contact verification session missing".into()); }
    if store.share_room_keys(&session, &direct).await.is_ok() { return Err("keys shared with unverified peer".into()); }
    let mut contact_a = Box::pin(matrix_contact::ContactVerification::start(store, session.clone(), &peer, "PEER")).await?;
    let mut listener = Box::pin(matrix_contact::ContactListener::open(peer_store, peer_session.clone(), account, "NATIVE")).await?;
    if !listener.requests().await?.contains(&contact_a.flow_id().to_owned()) { return Err("incoming contact request missing".into()); }
    let mut contact_b = Box::pin(listener.accept(contact_a.flow_id())).await?;
    for _ in 0..8 { contact_a.poll().await?; contact_b.poll().await?; if contact_a.codes().is_some() && contact_b.codes().is_some() { break; } }
    if contact_a.codes().is_none() || contact_a.codes() != contact_b.codes() { return Err("contact codes differ".into()); }
    contact_a.confirm().await?; contact_b.confirm().await?;
    for _ in 0..8 { contact_a.poll().await?; contact_b.poll().await?; if contact_a.done() && contact_b.done() { break; } }
    if !contact_a.done() || !contact_b.done() { return Err("contact verification incomplete".into()); }
    let mut store = contact_a.into_store();
    let mut peer_store = contact_b.into_store();
    if store.share_room_keys(&session, &direct).await.is_ok() { return Err("failed delivery was accepted".into()); }
    store.share_room_keys(&session, &direct).await?;
    let peer_trusted = peer_session.trusted_matrix_room_devices(&peer_store.machine, &direct).await?;
    if peer_store.receive_room_keys(&peer_session, &direct, &peer_trusted).await? != 1 { return Err("room share missing".into()); }
    store.queue_room_text(&session, &direct, "synthetic native message").await?;
    if store.queue_room_text(&session, &direct, "must not replace pending message").await.is_ok() { return Err("pending message replaced".into()); }
    if store.deliver_pending_room_text(&session, &direct).await.is_ok() { return Err("failed message delivery was accepted".into()); }
    let id = store.deliver_pending_room_text(&session, &direct).await?;
    let history = peer_session.matrix_room_history(&peer_store.machine, &direct, None, false).await?;
    let row = history.messages.iter().find(|row| row.id == id).ok_or("message missing")?;
    if matrix_history::decrypt_text(&peer_store.machine, row, &peer_trusted).await.as_deref() != Some("synthetic native message") { return Err("message decryption failed".into()); }
    session.pin_matrix_message(&store.machine, &direct, &id, true).await?;
    let pinned = peer_session.matrix_room_history(&peer_store.machine, &direct, None, true).await?;
    if !pinned.messages.iter().any(|row| row.id == id && row.pinned && row.expires_at.is_none()) { return Err("pinned message missing".into()); }
    session.pin_matrix_message(&store.machine, &direct, &id, false).await?;
    let unpinned = peer_session.matrix_room_history(&peer_store.machine, &direct, None, false).await?;
    if !unpinned.messages.iter().any(|row| row.id == id && !row.pinned && row.expires_at.is_some()) { return Err("unpin expiry missing".into()); }
    if peer_session.delete_matrix_message(&peer_store.machine, &direct, &id).await.is_ok() { return Err("peer deleted another sender message".into()); }
    session.delete_matrix_message(&store.machine, &direct, &id).await?;
    if peer_session.matrix_room_history(&peer_store.machine, &direct, None, false).await?.messages.iter().any(|row| row.id == id) { return Err("deleted message remains".into()); }
    store.queue_room_attachment(&session, &direct, "synthetic.txt", b"synthetic private file").await?;
    let file_id = store.deliver_pending_room_text(&session, &direct).await?;
    let file_history = peer_session.matrix_room_history(&peer_store.machine, &direct, None, false).await?;
    let file_event = file_history.messages.iter().find(|row| row.id == file_id).ok_or("attachment message missing")?;
    let file = match matrix_history::decrypt_content(&peer_store.machine, file_event, &peer_trusted).await { Some(matrix_history::MatrixContent::Attachment(file)) => file, _ => return Err("attachment metadata missing".into()) };
    let encrypted_file = peer_session.download_matrix_attachment(&peer_store.machine, &direct, &file.id, file.size).await?;
    if matrix_attachment::decrypt(&encrypted_file, &file.metadata)? != b"synthetic private file" { return Err("attachment content mismatch".into()); }
    session.delete_matrix_message(&store.machine, &direct, &file_id).await?;
    if peer_session.download_matrix_attachment(&peer_store.machine, &direct, &file.id, file.size).await.is_ok() { return Err("deleted attachment remained available".into()); }
    store.queue_room_text(&session, &direct, "expired synthetic message").await?;
    let expired = store.deliver_pending_room_text(&session, &direct).await?;
    if !test_http.post(format!("{base}/synthetic/expire-event")).json(&serde_json::json!({"eventId": &expired})).send().await?.status().is_success() { return Err("test expiry failed".into()); }
    if peer_session.matrix_room_history(&peer_store.machine, &direct, None, false).await?.messages.iter().any(|row| row.id == expired) { return Err("expired message visible".into()); }
    if session.pin_matrix_message(&store.machine, &direct, &expired, true).await.is_ok() { return Err("expired message could be pinned".into()); }
    store.queue_room_text(&session, &direct, "abandoned pending message").await?;
    if !test_http.post(format!("{base}/synthetic/advance-direct")).json(&serde_json::json!({"roomId": &direct.room_id})).send().await?.status().is_success() { return Err("direct revision advance failed".into()); }
    let updated = store.prepare_direct_room(&session, &peer).await?;
    if updated.revision <= direct.revision { return Err("direct revision did not advance".into()); }
    if store.deliver_pending_room_text(&session, &updated).await.is_ok() { return Err("stale pending message was delivered".into()); }
    store.discard_pending_room_text(&session, &updated).await?;
    if store.deliver_pending_room_text(&session, &updated).await.is_ok() { return Err("discarded pending message was delivered".into()); }
    store.queue_room_text(&session, &updated, "fresh message after discard").await?;
    store.deliver_pending_room_text(&session, &updated).await?;
    // A verified device receives the old room key; after revocation it must
    // neither get a new share nor decrypt messages encrypted with a new key.
    let fourth_session = api::login(base.clone(), "native-registration@example.invalid".into(), "synthetic-password".into()).await?;
    let (_, fourth_store) = matrix_store::create(&root, &base, account, &user, "FOURTH").await?;
    fourth_session.register_matrix_device(&fourth_store.machine).await?;
    fourth_session.publish_matrix_one_time_keys(&fourth_store.machine).await?;
    let mut verify_a = matrix_verification::DeviceVerification::start(store, session.clone(), "FOURTH".into()).await?;
    let mut listener = matrix_verification::DeviceListener::new(fourth_store, fourth_session.clone());
    if !listener.requests().await?.iter().any(|(flow, _)| flow == verify_a.flow_id()) { return Err("fourth verification request missing".into()); }
    let mut verify_b = listener.accept(verify_a.flow_id()).await?;
    for _ in 0..8 { verify_a.poll().await?; verify_b.poll().await?; if verify_a.codes().is_some() && verify_b.codes().is_some() { break; } }
    if verify_a.codes().is_none() || verify_a.codes() != verify_b.codes() { return Err("fourth verification codes differ".into()); }
    verify_a.confirm().await?; verify_b.confirm().await?;
    for _ in 0..8 { verify_a.poll().await?; verify_b.poll().await?; if verify_a.done() && verify_b.done() { break; } }
    if !verify_a.done() || !verify_b.done() { return Err("fourth verification incomplete".into()); }
    verify_a.approve().await?;
    let mut store = verify_a.into_store(); let mut fourth_store = verify_b.into_store();
    let before = store.prepare_direct_room(&session, &peer).await?;
    store.queue_room_text(&session, &before, "message before device revocation").await?;
    let before_id = store.deliver_pending_room_text(&session, &before).await?;
    let keys = store.machine.identity_keys();
    let sender = vec![api::TrustedMatrixDevice { user_id: user.clone(), device_id: "NATIVE".into(), ed25519: keys.ed25519.to_base64(), curve25519: keys.curve25519.to_base64() }];
    if fourth_store.receive_room_keys(&fourth_session, &before, &sender).await? == 0 { return Err("fourth device did not receive the old key".into()); }
    let before_history = fourth_session.matrix_room_history(&fourth_store.machine, &before, None, false).await?;
    let before_event = before_history.messages.iter().find(|row| row.id == before_id).ok_or("pre-revocation event missing")?;
    if matrix_history::decrypt_text(&fourth_store.machine, before_event, &sender).await.as_deref() != Some("message before device revocation") { return Err("fourth device could not decrypt old message".into()); }
    let target = session.matrix_devices(&store.machine).await?.into_iter().find(|device| device.device_id == "FOURTH").ok_or("fourth device missing")?;
    session.revoke_matrix_device(&store.machine, &target).await?;
    if fourth_session.matrix_room_history(&fourth_store.machine, &before, None, false).await.is_ok() { return Err("revoked device retained server access".into()); }
    let after = store.prepare_direct_room(&session, &peer).await?;
    if after.revision <= before.revision { return Err("device revocation did not rotate room revision".into()); }
    store.queue_room_text(&session, &after, "message after device revocation").await?;
    let after_id = store.deliver_pending_room_text(&session, &after).await?;
    let after_history = session.matrix_room_history(&store.machine, &after, None, false).await?;
    let after_event = after_history.messages.iter().find(|row| row.id == after_id).ok_or("post-revocation event missing")?;
    if before_event.content["session_id"] == after_event.content["session_id"] { return Err("room key was reused after revocation".into()); }
    if matrix_history::decrypt_text(&fourth_store.machine, after_event, &sender).await.is_some() { return Err("revoked device decrypted a future message".into()); }
    fourth_store.close().await?;
    peer_store.close().await?;
    if store.receive_room_keys(&session, &direct, &[]).await? != 0 { return Err("unexpected room key".into()); }
    let room = "!native-synthetic:sonar.invalid";
    let initial_revision = store.sync_room(&session, room).await?.revision;
    if initial_revision < 1 { return Err("wrong initial revision".into()); }
    if !test_http.post(format!("{base}/synthetic/advance")).send().await?.status().is_success() { return Err("test advance failed".into()); }
    if store.sync_room(&session, room).await?.revision != initial_revision + 1 { return Err("wrong newer revision".into()); }
    store.close().await?;
    let mut reopened = matrix_store::open(&root, &base, account, &user, "NATIVE", &name).await?;
    if reopened.machine.identity_keys().ed25519.to_base64() != key { return Err("identity changed".into()); }
    if reopened.sync_room(&session, room).await?.revision != initial_revision + 1 { return Err("room revision lost".into()); }
    if !test_http.post(format!("{base}/synthetic/rollback")).send().await?.status().is_success() { return Err("test rollback failed".into()); }
    if reopened.sync_room(&session, room).await.is_ok() { return Err("rollback accepted".into()); }
    let setup_root = root.join("explicit-setup");
    let setup_session = api::login(base.clone(), "native-registration@example.invalid".into(), "synthetic-password".into()).await?;
    if matrix_store::setup_device(&setup_root, &setup_session).await.map_err(|e| format!("first setup: {e}"))? != "pending" { return Err("setup did not create pending device".into()); }
    let descriptor = matrix_store::load_locator(&setup_root, &base, account)?;
    let again = matrix_store::load_locator(&setup_root, &base, account)?;
    if descriptor.name != again.name || descriptor.device != again.device { return Err("setup replaced identity".into()); }
    reopened.close().await?;
    println!("SONAR_MATRIX_REGISTER_OK");
    Ok(())
}
