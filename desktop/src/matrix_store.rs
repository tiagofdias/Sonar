//! Experimental Matrix SDK store. No existing Sonar identity is migrated or
//! replaced. Account/device approval and recovery are not implemented here.
#![allow(dead_code)]
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine};
use matrix_sdk_crypto::{OlmMachine, OlmMachineBuilder, store::CryptoStore};
use matrix_sdk_sqlite::{SqliteCryptoStore, SqliteStoreConfig};
use rand_core::{OsRng, RngCore};
use ruma::{DeviceId, UserId};
use sha2::{Digest, Sha256};
use std::{fs::{self, File, OpenOptions}, io::{Read, Write}, os::windows::fs::OpenOptionsExt, path::{Path, PathBuf}, sync::Arc};
use zeroize::Zeroizing;

const DB: &str = "matrix-sdk-crypto.sqlite3";
const MANIFEST: &str = "identity.dpapi";

#[derive(serde::Serialize, serde::Deserialize)]
pub struct LocalDevice { pub user: String, pub device: String, pub name: String }

fn locator_scope(origin: &str, account: &str) -> Result<String, String> {
    let url = crate::api::server_url(origin).map_err(|_| "Origem invalida")?;
    if account.len() != 24 || !account.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) { return Err("Conta invalida".into()); }
    Ok(format!("sonar-matrix-locator-v1-{:x}", Sha256::digest(format!("{}\0{}", url.origin().ascii_serialization(), account).as_bytes())))
}

/// Explicitly records an existing store; never replaces a prior locator.
pub fn save_locator(root: &Path, origin: &str, account: &str, descriptor: &LocalDevice) -> Result<(), String> {
    validate(origin, account, &descriptor.user, &descriptor.device, &descriptor.name)?;
    let scope = locator_scope(origin, account)?;
    let data = Zeroizing::new(serde_json::to_vec(descriptor).map_err(|_| "Identidade invalida")?);
    let encrypted = crate::key_store::crypt(&data, &scope, true).map_err(str::to_owned)?;
    fs::create_dir_all(root).map_err(|_| "Pasta Matrix indisponivel")?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(root.join(format!("{scope}.dpapi"))).map_err(|_| "Ja existe uma identidade local ou nao foi possivel guarda-la")?;
    file.write_all(&encrypted).and_then(|_| file.sync_all()).map_err(|_| "Nao foi possivel guardar a identidade local".into())
}

pub fn load_locator(root: &Path, origin: &str, account: &str) -> Result<LocalDevice, String> {
    let scope = locator_scope(origin, account)?;
    let file = File::open(root.join(format!("{scope}.dpapi"))).map_err(|_| "Identidade local indisponivel")?;
    let mut encrypted = Vec::new(); file.take(4097).read_to_end(&mut encrypted).map_err(|_| "Identidade local ilegivel")?;
    let data = crate::key_store::crypt(&encrypted, &scope, false).map_err(str::to_owned)?;
    let descriptor: LocalDevice = serde_json::from_slice(&data).map_err(|_| "Identidade local invalida")?;
    validate(origin, account, &descriptor.user, &descriptor.device, &descriptor.name)?;
    Ok(descriptor)
}
pub async fn open_local(root: &Path, session: &crate::api::Session) -> Result<MatrixStore, String> {
    let account = session.account.id.as_deref().ok_or("Conta invalida")?;
    let origin = session.matrix_origin();
    let descriptor = load_locator(root, &origin, account)?;
    if session.matrix_identity().await? != descriptor.user { return Err("A identidade Matrix do servidor mudou".into()); }
    let store = open(root, &origin, account, &descriptor.user, &descriptor.device, &descriptor.name).await?;
    session.register_matrix_device(&store.machine).await?;
    session.publish_matrix_one_time_keys(&store.machine).await?;
    Ok(store)
}

/// Explicit setup or retry. A per-account Windows lock prevents two processes
/// creating competing identities; failures never replace an existing locator.
pub async fn setup_device(root: &Path, session: &crate::api::Session) -> Result<String, String> {
    let account = session.account.id.as_deref().ok_or("Conta invalida")?;
    let origin = session.matrix_origin();
    let scope = locator_scope(&origin, account)?;
    fs::create_dir_all(root).map_err(|_| "Pasta Matrix indisponivel")?;
    let _setup_lock = OpenOptions::new().read(true).write(true).create(true).share_mode(0).open(root.join(format!("{scope}.lock"))).map_err(|_| "Configuracao Matrix ja em curso")?;
    let user = session.matrix_identity().await?;
    let locator = root.join(format!("{scope}.dpapi"));
    let store = if locator.try_exists().map_err(|_| "Identidade local indisponivel")? {
        let descriptor = load_locator(root, &origin, account)?;
        if descriptor.user != user { return Err("A identidade Matrix do servidor mudou".into()); }
        open(root, &origin, account, &user, &descriptor.device, &descriptor.name).await?
    } else {
        let mut random = [0u8; 12]; OsRng.fill_bytes(&mut random);
        let device = random.iter().map(|byte| format!("{byte:02X}")).collect::<String>();
        let (name, store) = create(root, &origin, account, &user, &device).await?;
        let descriptor = LocalDevice { user: user.clone(), device, name };
        if let Err(error) = save_locator(root, &origin, account, &descriptor) { let _ = store.close().await; return Err(error); }
        store
    };
    let result = async {
        session.register_matrix_device(&store.machine).await?;
        session.publish_matrix_one_time_keys(&store.machine).await?;
        let devices = session.matrix_devices(&store.machine).await?;
        Ok(devices.into_iter().find(|device| device.current).ok_or("Dispositivo local ausente")?.server_authorization)
    }.await;
    let closed = store.close().await;
    result.and_then(|status| closed.map(|()| status))
}

pub struct MatrixStore {
    pub machine: OlmMachine,
    store: Arc<SqliteCryptoStore>,
    // Windows denies other opens for as long as this handle is alive.
    _lock: File,
}
const VERIFICATION_OUTBOX: &str = "sonar-verification-outbox-v1";
#[derive(serde::Serialize, serde::Deserialize)]
struct VerificationOutbox { transaction: String, target: String, digest: String, ciphertext: serde_json::Value }
#[derive(serde::Serialize, serde::Deserialize)]
struct ContactVerificationOutbox { transaction: String, user: String, device: String, room: String, revision: u64, digest: String, ciphertext: serde_json::Value }
#[derive(serde::Serialize, serde::Deserialize)]
struct RoomShareOutbox { revision: u64, shares: Vec<RoomShare> }
#[derive(serde::Serialize, serde::Deserialize)]
struct RoomShare { transaction: String, messages: serde_json::Value }
#[derive(serde::Serialize, serde::Deserialize)]
struct PendingRoomMessage { room_id: String, revision: u64, token: String, expires_at: String, content: serde_json::Value, #[serde(default, skip_serializing_if = "Option::is_none")] attachment: Option<PendingAttachment> }
#[derive(serde::Serialize, serde::Deserialize)]
struct PendingAttachment { id: String, ciphertext: String }
#[path = "matrix_attachment.rs"] mod attachment_crypto;
impl MatrixStore {
    pub async fn receive_contact_verification(&mut self, session: &crate::api::Session, room: &crate::api::MatrixRoomSnapshot, user: &str, target: &str, expected_curve: &str) -> Result<usize, String> {
        use matrix_sdk_common::deserialized_responses::{AlgorithmInfo, ProcessedToDeviceEvent};
        use matrix_sdk_crypto::{DecryptionSettings, EncryptionSyncChanges, TrustRequirement};
        let failure = || "Nao foi possivel validar a verificacao do contacto.".to_string();
        if room.kind != "direct" || room.members.len() != 2 || user == self.machine.user_id().as_str() || !room.members.iter().any(|id| id == user) { return Err(failure()); }
        let rows = session.matrix_contact_verification_inbox(&self.machine, room).await?;
        let mut ids = Vec::new(); let mut receipts = Vec::new(); let mut seen = std::collections::HashSet::new();
        for row in rows {
            let id = row.get("id").and_then(|v| v.as_str()).filter(|id| id.len() == 24 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))).ok_or_else(failure)?;
            let event = row.get("event").ok_or_else(failure)?;
            if !seen.insert(id.to_owned()) || event["type"] != "m.room.encrypted" || event["sender"] != user || event["content"]["sender_key"] != expected_curve { return Err(failure()); }
            let key = format!("sonar-contact-receipt-v1:{id}");
            let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(event).map_err(|_| failure())?));
            match self.store.get_custom_value(&key).await.map_err(|_| failure())? {
                Some(saved) if saved == digest.as_bytes() => {},
                Some(_) => return Err(failure()),
                None => {
                    self.store.set_custom_value(&key, b"processing".to_vec()).await.map_err(|_| failure())?;
                    let raw = serde_json::from_value(event.clone()).map_err(|_| failure())?;
                    let changes = EncryptionSyncChanges { to_device_events: vec![raw], changed_devices: &Default::default(), one_time_keys_counts: &Default::default(), unused_fallback_keys: None, next_batch_token: None };
                    let settings = DecryptionSettings { sender_device_trust_requirement: TrustRequirement::Untrusted };
                    let (events, _) = self.machine.receive_sync_changes(changes, &settings).await.map_err(|_| failure())?;
                    let [ProcessedToDeviceEvent::Decrypted { raw, encryption_info }] = events.as_slice() else { return Err(failure()); };
                    let clear: serde_json::Value = serde_json::from_str(raw.json().get()).map_err(|_| failure())?;
                    let AlgorithmInfo::OlmV1Curve25519AesSha2 { curve25519_public_key_base64 } = &encryption_info.algorithm_info else { return Err(failure()); };
                    let kind = clear["type"].as_str().ok_or_else(failure)?;
                    if !["m.key.verification.request", "m.key.verification.ready", "m.key.verification.start", "m.key.verification.accept", "m.key.verification.key", "m.key.verification.mac", "m.key.verification.done", "m.key.verification.cancel"].contains(&kind) || encryption_info.sender.as_str() != user || encryption_info.sender_device.as_ref().map(|id| id.as_str()) != Some(target) || curve25519_public_key_base64 != expected_curve { return Err(failure()); }
                    self.store.set_custom_value(&key, digest.into_bytes()).await.map_err(|_| failure())?;
                }
            }
            ids.push(id.to_owned()); receipts.push(key);
        }
        if !ids.is_empty() {
            session.acknowledge_matrix_contact_verification(&self.machine, room, &ids).await?;
            for key in receipts { self.store.remove_custom_value(&key).await.map_err(|_| failure())?; }
        }
        Ok(ids.len())
    }
    pub async fn send_contact_verification(&mut self, session: &crate::api::Session, room: &crate::api::MatrixRoomSnapshot, user: &str, target: &str, request: &matrix_sdk_crypto::types::requests::ToDeviceRequest) -> Result<(), String> {
        let failure = || "Nao foi possivel enviar a verificacao do contacto.".to_string();
        let kind = request.event_type.to_string();
        if !["m.key.verification.request", "m.key.verification.ready", "m.key.verification.start", "m.key.verification.accept", "m.key.verification.key", "m.key.verification.mac", "m.key.verification.done", "m.key.verification.cancel"].contains(&kind.as_str()) || room.kind != "direct" || room.members.len() != 2 || user == self.machine.user_id().as_str() || !room.members.iter().any(|id| id == user) { return Err(failure()); }
        let snapshot = session.matrix_room_snapshot(&self.machine, &room.room_id).await?;
        let mut members = snapshot.members; members.sort(); let mut expected = room.members.clone(); expected.sort();
        if snapshot.revision != room.revision || snapshot.kind != room.kind || members != expected { return Err(failure()); }
        let devices = session.matrix_room_devices(&self.machine, room, false).await?;
        if !devices.iter().any(|device| device.user_id == user && device.device_id == target) { return Err(failure()); }
        let messages = serde_json::to_value(&request.messages).map_err(|_| failure())?;
        let users = messages.as_object().ok_or_else(failure)?;
        let targets = users.get(user).and_then(|v| v.as_object()).ok_or_else(failure)?;
        if users.len() != 1 || targets.len() != 1 { return Err(failure()); }
        let content = targets.get(target).ok_or_else(failure)?;
        let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&(kind.as_str(), &messages)).map_err(|_| failure())?));
        let transaction = request.txn_id.to_string();
        let key = format!("sonar-contact-verification-v1:{:x}", Sha256::digest(room.room_id.as_bytes()));
        let record: ContactVerificationOutbox = match self.store.get_custom_value(&key).await.map_err(|_| failure())? {
            Some(bytes) => {
                let record: ContactVerificationOutbox = serde_json::from_slice(&bytes).map_err(|_| failure())?;
                if record.transaction != transaction || record.user != user || record.device != target || record.room != room.room_id || record.revision != room.revision || record.digest != digest { return Err(failure()); }
                record
            }
            None => {
                let user_id = ruma::UserId::parse(user).map_err(|_| failure())?;
                let device = self.machine.get_device(&user_id, target.into(), None).await.map_err(|_| failure())?.ok_or_else(failure)?;
                self.store.set_custom_value(&key, b"preparing".to_vec()).await.map_err(|_| failure())?;
                let encrypted = device.encrypt_event_raw(&kind, content, matrix_sdk_crypto::CollectStrategy::AllDevices).await.map_err(|_| failure())?;
                let record = ContactVerificationOutbox { transaction: transaction.clone(), user:user.into(), device:target.into(), room:room.room_id.clone(), revision:room.revision, digest, ciphertext:serde_json::to_value(encrypted).map_err(|_| failure())? };
                self.store.set_custom_value(&key, serde_json::to_vec(&record).map_err(|_| failure())?).await.map_err(|_| failure())?;
                record
            }
        };
        session.deliver_matrix_contact_verification(&self.machine, room, &record.transaction, user, target, record.ciphertext).await?;
        self.machine.mark_request_as_sent(&request.txn_id, &ruma::api::client::to_device::send_event_to_device::v3::Response::new()).await.map_err(|_| failure())?;
        self.store.remove_custom_value(&key).await.map_err(|_| failure())?;
        Ok(())
    }
    fn message_outbox_key(room: &str) -> String { format!("sonar-room-message-v1:{:x}", Sha256::digest(room.as_bytes())) }
    pub async fn queue_room_text(&mut self, session: &crate::api::Session, room: &crate::api::MatrixRoomSnapshot, text: &str) -> Result<(), String> {
        self.queue_room_text_with_reply(session, room, text, None).await
    }
    pub async fn queue_room_text_with_reply(&mut self, session: &crate::api::Session, room: &crate::api::MatrixRoomSnapshot, text: &str, reply: Option<&str>) -> Result<(), String> {
        let failure = || "Nao foi possivel preparar a mensagem cifrada.".to_string();
        if text.trim().is_empty() || text.len() > 32000 || reply.is_some_and(|id| id.len() != 24 || !id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))) { return Err(failure()); }
        let mut plain = serde_json::json!({"msgtype":"m.text", "body":text});
        if let Some(id) = reply { plain["m.relates_to"] = serde_json::json!({"m.in_reply_to":{"event_id":format!("${id}")}}); }
        self.queue_room_event(session, room, "m.room.message", plain, None).await
    }
    pub async fn queue_room_reaction(&mut self, session: &crate::api::Session, room: &crate::api::MatrixRoomSnapshot, target: &str, emoji: &str) -> Result<(), String> {
        if target.len() != 24 || !target.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) || !["👍", "❤️", "😂", "🎉"].contains(&emoji) { return Err("Reacao invalida.".into()); }
        self.queue_room_event(session, room, "m.reaction", serde_json::json!({"m.relates_to":{"rel_type":"m.annotation","event_id":format!("${target}"),"key":emoji}}), None).await
    }
    pub async fn queue_room_attachment(&mut self, session: &crate::api::Session, room: &crate::api::MatrixRoomSnapshot, name: &str, data: &[u8]) -> Result<(), String> {
        use base64::{engine::general_purpose::STANDARD, Engine};
        if name.is_empty() || name.len() > 255 || name.chars().any(|c| c.is_control() || c == '/' || c == '\\') { return Err("Nome de anexo invalido".into()); }
        let (ciphertext, metadata) = attachment_crypto::encrypt(data)?;
        let mut random = [0u8; 16]; OsRng.fill_bytes(&mut random);
        let id: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let mut file: serde_json::Value = serde_json::from_str(&metadata).map_err(|_| "Metadados invalidos")?;
        file["url"] = format!("mxc://{}/{id}", self.machine.user_id().server_name()).into();
        let event = serde_json::json!({"msgtype":"m.file","body":name,"info":{"size":data.len(),"mimetype":"application/octet-stream"},"file":file});
        self.queue_room_event(session, room, "m.room.message", event, Some(PendingAttachment { id, ciphertext: STANDARD.encode(ciphertext) })).await
    }
    async fn queue_room_event(&mut self, session: &crate::api::Session, room: &crate::api::MatrixRoomSnapshot, event_type: &str, plain: serde_json::Value, attachment: Option<PendingAttachment>) -> Result<(), String> {
        let failure = || "Nao foi possivel preparar a mensagem cifrada.".to_string();
        let key = Self::message_outbox_key(&room.room_id);
        if self.store.get_custom_value(&key).await.map_err(|_| failure())?.is_some() { return Err("Existe uma mensagem pendente; tenta o envio anterior.".into()); }
        self.share_room_keys(session, room).await?;
        let snapshot = session.matrix_room_snapshot(&self.machine, &room.room_id).await?;
        let mut members = snapshot.members; members.sort(); let mut expected = room.members.clone(); expected.sort();
        if snapshot.revision != room.revision || snapshot.kind != room.kind || members != expected { return Err(failure()); }
        session.trusted_matrix_room_devices(&self.machine, room).await?;
        let (token, expires_at) = session.issue_matrix_message_token(&self.machine, room).await?;
        self.store.set_custom_value(&key, b"preparing".to_vec()).await.map_err(|_| failure())?;
        let room_id = ruma::RoomId::parse(&room.room_id).map_err(|_| failure())?;
        let raw = ruma::serde::Raw::new(&plain).map_err(|_| failure())?.cast_unchecked();
        let encrypted = self.machine.encrypt_room_event_raw(&room_id, event_type, &raw).await.map_err(|_| failure())?;
        let mut content = serde_json::to_value(encrypted.content).map_err(|_| failure())?;
        content.as_object_mut().ok_or_else(failure)?.remove("m.relates_to");
        let record = PendingRoomMessage { room_id: room.room_id.clone(), revision: room.revision, token, expires_at, content, attachment };
        self.store.set_custom_value(&key, serde_json::to_vec(&record).map_err(|_| failure())?).await.map_err(|_| failure())?;
        Ok(())
    }
    pub async fn deliver_pending_room_text(&mut self, session: &crate::api::Session, room: &crate::api::MatrixRoomSnapshot) -> Result<String, String> {
        let failure = || "Mensagem pendente indisponivel ou expirada.".to_string();
        let key = Self::message_outbox_key(&room.room_id);
        let bytes = self.store.get_custom_value(&key).await.map_err(|_| failure())?.ok_or_else(failure)?;
        let pending: PendingRoomMessage = serde_json::from_slice(&bytes).map_err(|_| failure())?;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| failure())?.as_millis() as i64;
        if pending.room_id != room.room_id || pending.revision != room.revision || chrono::DateTime::parse_from_rfc3339(&pending.expires_at).map_err(|_| failure())?.timestamp_millis() <= now { return Err(failure()); }
        let snapshot = session.matrix_room_snapshot(&self.machine, &room.room_id).await?;
        let mut members = snapshot.members; members.sort(); let mut expected = room.members.clone(); expected.sort();
        if snapshot.revision != room.revision || snapshot.kind != room.kind || members != expected { return Err(failure()); }
        session.trusted_matrix_room_devices(&self.machine, room).await?;
        let id = match pending.attachment {
            Some(attachment) => {
                use base64::{engine::general_purpose::STANDARD, Engine};
                let bytes = STANDARD.decode(&attachment.ciphertext).map_err(|_| failure())?;
                session.deliver_matrix_attachment(&self.machine, room, &pending.token, &pending.content, &attachment.id, bytes).await?
            },
            None => session.deliver_matrix_message(&self.machine, room, &pending.token, &pending.content).await?,
        };
        self.store.remove_custom_value(&key).await.map_err(|_| failure())?;
        Ok(id)
    }
    /// Explicitly abandon the local retry, including an interrupted prepare.
    /// The server may already have accepted the event despite a lost response.
    pub async fn discard_pending_room_text(&mut self, session: &crate::api::Session, room: &crate::api::MatrixRoomSnapshot) -> Result<(), String> {
        let failure = || "Nao foi possivel descartar o envio pendente.".to_string();
        let key = Self::message_outbox_key(&room.room_id);
        if self.store.get_custom_value(&key).await.map_err(|_| failure())?.is_none() { return Err("Nao existe envio pendente nesta conversa.".into()); }
        let snapshot = session.matrix_room_snapshot(&self.machine, &room.room_id).await?;
        let mut actual = snapshot.members; actual.sort(); let mut expected = room.members.clone(); expected.sort();
        if snapshot.revision != room.revision || snapshot.kind != room.kind || actual != expected { return Err(failure()); }
        let id = ruma::RoomId::parse(&room.room_id).map_err(|_| failure())?;
        self.machine.discard_room_key(&id).await.map_err(|_| failure())?;
        self.store.remove_custom_value(&key).await.map_err(|_| failure())?;
        Ok(())
    }
    pub async fn share_room_keys(&mut self, session: &crate::api::Session, room: &crate::api::MatrixRoomSnapshot) -> Result<(), String> {
        use matrix_sdk_crypto::{CollectStrategy, EncryptionSettings};
        let failure = || "Nao foi possivel partilhar as chaves da conversa.".to_string();
        let key = format!("sonar-room-share-v1:{:x}", Sha256::digest(room.room_id.as_bytes()));
        let room_id = ruma::RoomId::parse(&room.room_id).map_err(|_| failure())?;
        let snapshot = session.matrix_room_snapshot(&self.machine, &room.room_id).await?;
        let mut current = snapshot.members.clone(); current.sort(); let mut expected = room.members.clone(); expected.sort();
        if snapshot.revision != room.revision || snapshot.kind != room.kind || current != expected { return Err(failure()); }
        let trusted = session.trusted_matrix_room_devices(&self.machine, room).await?;
        let saved = self.store.get_custom_value(&key).await.map_err(|_| failure())?;
        let mut pending = match saved {
            Some(bytes) if bytes == b"preparing" => { self.machine.discard_room_key(&room_id).await.map_err(|_| failure())?; self.store.remove_custom_value(&key).await.map_err(|_| failure())?; None }
            Some(bytes) => {
                let pending: RoomShareOutbox = serde_json::from_slice(&bytes).map_err(|_| failure())?;
                if pending.revision > room.revision { return Err(failure()); }
                if pending.revision < room.revision { self.machine.discard_room_key(&room_id).await.map_err(|_| failure())?; self.store.remove_custom_value(&key).await.map_err(|_| failure())?; None } else { Some(pending) }
            }
            None => None,
        };
        if pending.is_none() {
            self.ensure_room_revision(&room.room_id, room.revision).await?;
            session.establish_matrix_room_sessions(&self.machine, room, &trusted).await?;
            self.store.set_custom_value(&key, b"preparing".to_vec()).await.map_err(|_| failure())?;
            let users: Vec<_> = room.members.iter().map(|user| ruma::UserId::parse(user).map_err(|_| failure())).collect::<Result<_, _>>()?;
            let settings = EncryptionSettings { sharing_strategy: CollectStrategy::OnlyTrustedDevices, ..Default::default() };
            let shares = self.machine.share_room_key(&room_id, users.iter().map(|user| user.as_ref()), settings).await.map_err(|_| failure())?;
            if shares.len() > 100 { return Err(failure()); }
            let mut records = Vec::new();
            for share in shares { if share.event_type.to_string() != "m.room.encrypted" { return Err(failure()); } records.push(RoomShare { transaction: share.txn_id.to_string(), messages: serde_json::to_value(&share.messages).map_err(|_| failure())? }); }
            let record = RoomShareOutbox { revision: room.revision, shares: records };
            self.store.set_custom_value(&key, serde_json::to_vec(&record).map_err(|_| failure())?).await.map_err(|_| failure())?;
            pending = Some(record);
        }
        let mut pending = pending.ok_or_else(failure)?;
        while let Some(share) = pending.shares.first() {
            let snapshot = session.matrix_room_snapshot(&self.machine, &room.room_id).await?;
            let mut members = snapshot.members; members.sort();
            if snapshot.revision != room.revision || snapshot.kind != room.kind || members != expected { return Err(failure()); }
            let fresh = session.trusted_matrix_room_devices(&self.machine, room).await?;
            let users = share.messages.as_object().ok_or_else(failure)?; let mut recipients = 0;
            for (user, devices) in users { for (device, content) in devices.as_object().ok_or_else(failure)? {
                let target = fresh.iter().find(|d| &d.user_id == user && &d.device_id == device).ok_or_else(failure)?;
                if content["algorithm"] != "m.olm.v1.curve25519-aes-sha2" || content["sender_key"] != self.machine.identity_keys().curve25519.to_base64() || !content["ciphertext"].as_object().is_some_and(|v| v.len() == 1 && v.contains_key(&target.curve25519)) { return Err(failure()); }
                recipients += 1;
            } }
            if recipients == 0 || recipients > 100 { return Err(failure()); }
            session.deliver_matrix_room_share(&self.machine, room, &share.transaction, &share.messages).await?;
            self.machine.mark_request_as_sent(share.transaction.as_str().into(), &ruma::api::client::to_device::send_event_to_device::v3::Response::new()).await.map_err(|_| failure())?;
            pending.shares.remove(0);
            self.store.set_custom_value(&key, serde_json::to_vec(&pending).map_err(|_| failure())?).await.map_err(|_| failure())?;
        }
        self.store.remove_custom_value(&key).await.map_err(|_| failure())?;
        Ok(())
    }
    pub async fn receive_room_keys(&mut self, session: &crate::api::Session, room: &crate::api::MatrixRoomSnapshot, trusted: &[crate::api::TrustedMatrixDevice]) -> Result<usize, String> {
        use matrix_sdk_common::deserialized_responses::{AlgorithmInfo, ProcessedToDeviceEvent};
        use matrix_sdk_crypto::{DecryptionSettings, EncryptionSyncChanges, TrustRequirement};
        let failure = || "Nao foi possivel validar as chaves da conversa.".to_string();
        let rows = session.matrix_room_key_inbox(&self.machine, room).await?;
        let mut ids = Vec::new(); let mut receipts = Vec::new(); let mut seen = std::collections::HashSet::new();
        for row in rows {
            let id = row.get("id").and_then(|v| v.as_str()).filter(|id| id.len() == 24 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))).ok_or_else(failure)?;
            if !seen.insert(id.to_owned()) { return Err(failure()); }
            let event = row.get("event").ok_or_else(failure)?;
            let sender = event.get("sender").and_then(|v| v.as_str()).ok_or_else(failure)?;
            let curve = event.get("content").and_then(|v| v.get("sender_key")).and_then(|v| v.as_str()).ok_or_else(failure)?;
            if event["type"] != "m.room.encrypted" || !trusted.iter().any(|d| d.user_id == sender && d.curve25519 == curve) { return Err(failure()); }
            let key = format!("sonar-room-key-receipt-v1:{id}");
            let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(event).map_err(|_| failure())?));
            match self.store.get_custom_value(&key).await.map_err(|_| failure())? {
                Some(saved) if saved == digest.as_bytes() => {},
                Some(_) => return Err(failure()),
                None => {
                    self.store.set_custom_value(&key, b"processing".to_vec()).await.map_err(|_| failure())?;
                    let raw = serde_json::from_value(event.clone()).map_err(|_| failure())?;
                    let changes = EncryptionSyncChanges { to_device_events: vec![raw], changed_devices: &Default::default(), one_time_keys_counts: &Default::default(), unused_fallback_keys: None, next_batch_token: None };
                    let settings = DecryptionSettings { sender_device_trust_requirement: TrustRequirement::Untrusted };
                    let (events, _) = self.machine.receive_sync_changes(changes, &settings).await.map_err(|_| failure())?;
                    let [ProcessedToDeviceEvent::Decrypted { raw, encryption_info }] = events.as_slice() else { return Err(failure()); };
                    let clear: serde_json::Value = serde_json::from_str(raw.json().get()).map_err(|_| failure())?;
                    let AlgorithmInfo::OlmV1Curve25519AesSha2 { curve25519_public_key_base64 } = &encryption_info.algorithm_info else { return Err(failure()); };
                    if clear["type"] != "m.room_key" || clear["content"]["room_id"] != room.room_id || encryption_info.sender.as_str() != sender || curve25519_public_key_base64 != curve { return Err(failure()); }
                    self.store.set_custom_value(&key, digest.into_bytes()).await.map_err(|_| failure())?;
                }
            }
            ids.push(id.to_owned()); receipts.push(key);
        }
        if !ids.is_empty() {
            session.acknowledge_matrix_room_keys(&self.machine, room, &ids).await?;
            for key in receipts { self.store.remove_custom_value(&key).await.map_err(|_| failure())?; }
        }
        Ok(ids.len())
    }
    /// The SDK advances its Olm ratchet while encrypting. Record a fail-closed
    /// marker first, and retain the exact ciphertext until server acceptance.
    pub async fn send_verification(&mut self, session: &crate::api::Session, target: &str, request: &matrix_sdk_crypto::types::requests::ToDeviceRequest) -> Result<(), String> {
        let failure = || "Nao foi possivel enviar a verificacao; inicia novamente se estiver interrompida.".to_string();
        let event_type = request.event_type.to_string();
        if !["m.key.verification.request", "m.key.verification.ready", "m.key.verification.start", "m.key.verification.accept", "m.key.verification.key", "m.key.verification.mac", "m.key.verification.done", "m.key.verification.cancel"].contains(&event_type.as_str()) { return Err(failure()); }
        let messages = serde_json::to_value(&request.messages).map_err(|_| failure())?;
        let users = messages.as_object().ok_or_else(failure)?;
        let devices = users.get(self.machine.user_id().as_str()).and_then(|v| v.as_object()).ok_or_else(failure)?;
        if users.len() != 1 || devices.len() != 1 || target == self.machine.device_id().as_str() { return Err(failure()); }
        let content = devices.get(target).ok_or_else(failure)?;
        let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&(event_type.as_str(), &messages)).map_err(|_| failure())?));
        let transaction = request.txn_id.to_string();
        let existing = self.store.get_custom_value(VERIFICATION_OUTBOX).await.map_err(|_| failure())?;
        let prepared: VerificationOutbox = if let Some(data) = existing {
            let record: VerificationOutbox = serde_json::from_slice(&data).map_err(|_| failure())?;
            if record.transaction != transaction || record.target != target || record.digest != digest { return Err(failure()); }
            record
        } else {
            let device = self.machine.get_device(self.machine.user_id(), target.into(), None).await.map_err(|_| failure())?.ok_or_else(failure)?;
            self.store.set_custom_value(VERIFICATION_OUTBOX, b"preparing".to_vec()).await.map_err(|_| failure())?;
            let encrypted = device.encrypt_event_raw(&event_type, content, matrix_sdk_crypto::CollectStrategy::AllDevices).await.map_err(|_| failure())?;
            let record = VerificationOutbox { transaction: transaction.clone(), target: target.into(), digest, ciphertext: serde_json::to_value(encrypted).map_err(|_| failure())? };
            self.store.set_custom_value(VERIFICATION_OUTBOX, serde_json::to_vec(&record).map_err(|_| failure())?).await.map_err(|_| failure())?;
            record
        };
        session.deliver_matrix_verification(&self.machine, &prepared.transaction, &prepared.target, prepared.ciphertext).await?;
        self.machine.mark_request_as_sent(&request.txn_id, &ruma::api::client::to_device::send_event_to_device::v3::Response::new()).await.map_err(|_| failure())?;
        self.store.remove_custom_value(VERIFICATION_OUTBOX).await.map_err(|_| failure())?;
        Ok(())
    }
    pub async fn close(self) -> Result<(), String> {
        self.store.close().await.map_err(|_| "Nao foi possivel fechar o armazenamento Matrix".into())
    }
    /// The exclusive store lock and mutable borrow serialize SDK rotation and
    /// the persistent revision. Invalidate before saving to make crash retry safe.
    pub async fn ensure_room_revision(&mut self, room_id: &str, revision: u64) -> Result<bool, String> {
        if revision > 9_007_199_254_740_991 { return Err("Versao da sala invalida".into()); }
        let room = ruma::RoomId::parse(room_id).map_err(|_| "Sala invalida")?;
        let key = format!("sonar-room-revision-v1:{:x}", Sha256::digest(room_id.as_bytes()));
        let previous = self.store.get_custom_value(&key).await.map_err(|_| "Versao da sala indisponivel")?;
        let old = match previous {
            Some(bytes) => Some(u64::from_be_bytes(bytes.as_slice().try_into().map_err(|_| "Versao da sala corrompida")?)),
            None => None,
        };
        if old.is_some_and(|value| value > revision || value > 9_007_199_254_740_991) { return Err("A versao da sala recuou. Envio bloqueado".into()); }
        if old == Some(revision) { return Ok(false); }
        self.machine.discard_room_key(&room).await.map_err(|_| "Nao foi possivel renovar a chave da sala")?;
        self.store.set_custom_value(&key, revision.to_be_bytes().to_vec()).await.map_err(|_| "Nao foi possivel guardar a versao da sala")?;
        Ok(true)
    }
    pub async fn sync_room(&mut self, session: &crate::api::Session, room_id: &str) -> Result<crate::api::MatrixRoomSnapshot, String> {
        let snapshot = session.matrix_room_snapshot(&self.machine, room_id).await?;
        self.ensure_room_revision(room_id, snapshot.revision).await?;
        Ok(snapshot)
    }
    pub async fn prepare_direct_room(&mut self, session: &crate::api::Session, peer_account: &str) -> Result<crate::api::MatrixRoomSnapshot, String> {
        let snapshot = session.matrix_direct_room(&self.machine, peer_account).await?;
        self.ensure_room_revision(&snapshot.room_id, snapshot.revision).await?;
        Ok(snapshot)
    }
}
fn validate(origin: &str, account: &str, user: &str, device: &str, name: &str) -> Result<String, String> {
    let url = crate::api::server_url(origin).map_err(|_| "Origem invalida")?;
    if account.len() != 24 || !account.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) { return Err("Conta invalida".into()); }
    let parsed = UserId::parse(user).map_err(|_| "Identidade Matrix invalida")?;
    if parsed.localpart() != account || device.is_empty() || device.len() > 32 || !device.bytes().all(|b| b.is_ascii_alphanumeric()) || name.len() != 42 || !name.starts_with("matrix-v2-") || !name[10..].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) { return Err("Dispositivo Matrix invalido".into()); }
    let scope = format!("sonar-matrix-v2\0{}\0{}\0{}\0{}\0{}", url.origin().ascii_serialization(), account, user, device, name);
    Ok(format!("{:x}", Sha256::digest(scope.as_bytes())))
}
fn exclusive(path: &Path) -> Result<File, String> {
    OpenOptions::new().read(true).write(true).create(true).share_mode(0).open(path.join("store.lock")).map_err(|_| "O armazenamento Matrix ja esta aberto ou indisponivel".into())
}
async fn machine(path: &Path, key: &[u8], user: &str, device: &str) -> Result<(OlmMachine, Arc<SqliteCryptoStore>), String> {
    let config = SqliteStoreConfig::with_low_memory_config(path).key(Some(key));
    let store = Arc::new(SqliteCryptoStore::open_with_config(&config).await.map_err(|_| "Nao foi possivel abrir as chaves Matrix")?);
    let user = UserId::parse(user).map_err(|_| "Identidade Matrix invalida")?;
    let device: &DeviceId = device.into();
    let machine = OlmMachineBuilder::new(&user, device).with_crypto_store(store.clone()).build().await.map_err(|_| "Nao foi possivel carregar a identidade Matrix")?;
    Ok((machine, store))
}
fn public_key(machine: &OlmMachine) -> Result<Vec<u8>, String> {
    let key = STANDARD_NO_PAD.decode(machine.identity_keys().ed25519.to_base64()).map_err(|_| "Chave publica invalida")?;
    if key.len() != 32 { return Err("Chave publica invalida".into()); }
    Ok(key)
}
/// Explicitly creates a new isolated store with a random name. On any failure,
/// the incomplete directory is left untouched and cannot be opened as valid.
pub async fn create(root: &Path, origin: &str, account: &str, user: &str, device: &str) -> Result<(String, MatrixStore), String> {
    let mut random = [0u8; 16]; OsRng.fill_bytes(&mut random);
    let name = format!("matrix-v2-{}", random.iter().map(|b| format!("{b:02x}")).collect::<String>());
    let scope = validate(origin, account, user, device, &name)?;
    fs::create_dir_all(root).map_err(|_| "Pasta Matrix indisponivel")?;
    let path = root.join(&name);
    fs::create_dir(&path).map_err(|_| "Nao foi possivel criar o armazenamento Matrix")?;
    let lock = exclusive(&path)?;
    let mut key = Zeroizing::new([0u8; 32]); OsRng.fill_bytes(&mut *key);
    let (machine, store) = machine(&path, &key[..], user, device).await?;
    let mut manifest = Zeroizing::new(Vec::with_capacity(64));
    manifest.extend_from_slice(&key[..]); manifest.extend_from_slice(&public_key(&machine)?);
    let encrypted = crate::key_store::crypt(&manifest, &scope, true).map_err(str::to_owned)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path.join(MANIFEST)).map_err(|_| "Nao foi possivel guardar a identidade Matrix")?;
    file.write_all(&encrypted).and_then(|_| file.sync_all()).map_err(|_| "Nao foi possivel guardar a identidade Matrix")?;
    Ok((name, MatrixStore { machine, store, _lock: lock }))
}
/// Checks the previously protected public identity before returning the SDK.
pub async fn open(root: &Path, origin: &str, account: &str, user: &str, device: &str, name: &str) -> Result<MatrixStore, String> {
    let scope = validate(origin, account, user, device, name)?;
    let path: PathBuf = root.join(name);
    if !path.join(DB).is_file() { return Err("Dados Matrix locais ausentes".into()); }
    let lock = exclusive(&path)?;
    let file = File::open(path.join(MANIFEST)).map_err(|_| "Identidade Matrix ausente")?;
    let mut encrypted = Vec::new(); file.take(4097).read_to_end(&mut encrypted).map_err(|_| "Identidade Matrix ilegivel")?;
    let manifest = crate::key_store::crypt(&encrypted, &scope, false).map_err(str::to_owned)?;
    if manifest.len() != 64 { return Err("Identidade Matrix invalida".into()); }
    let (machine, store) = machine(&path, &manifest[..32], user, device).await?;
    if public_key(&machine)? != manifest[32..] { let _ = store.close().await; return Err("A identidade Matrix mudou ou os dados foram perdidos".into()); }
    Ok(MatrixStore { machine, store, _lock: lock })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn protected_matrix_store_reopens_and_blocks_replacement() {
        let mut random = [0u8; 16]; OsRng.fill_bytes(&mut random);
        let root = std::env::temp_dir().join(format!("sonar-matrix-test-{:x}", u128::from_le_bytes(random)));
        let origin = "http://127.0.0.1:5000"; let account = "0123456789abcdef01234567";
        let user = "@0123456789abcdef01234567:sonar.invalid"; let device = "DEVICE1";
        let (name, mut store) = create(&root, origin, account, user, device).await.unwrap();
        let descriptor = LocalDevice { user: user.into(), device: device.into(), name: name.clone() };
        save_locator(&root, origin, account, &descriptor).unwrap();
        assert!(save_locator(&root, origin, account, &descriptor).is_err());
        let loaded = load_locator(&root, origin, account).unwrap();
        assert_eq!(loaded.name, name); assert_eq!(loaded.device, device);
        assert!(load_locator(&root, "http://127.0.0.1:5001", account).is_err());
        let original = public_key(&store.machine).unwrap();
        assert!(store.ensure_room_revision("!test:sonar.invalid", 1).await.unwrap());
        assert!(!store.ensure_room_revision("!test:sonar.invalid", 1).await.unwrap());
        let room = ruma::RoomId::parse("!test:sonar.invalid").unwrap();
        let settings = matrix_sdk_crypto::EncryptionSettings { sharing_strategy: matrix_sdk_crypto::CollectStrategy::AllDevices, ..Default::default() };
        store.machine.share_room_key(&room, std::iter::empty(), settings.clone()).await.unwrap();
        let first = store.machine.encrypt_room_event(&room, ruma::events::room::message::RoomMessageEventContent::text_plain("synthetic one")).await.unwrap();
        assert!(open(&root, origin, account, user, device, &name).await.is_err());
        store.close().await.unwrap();
        let mut reopened = open(&root, origin, account, user, device, &name).await.unwrap();
        assert_eq!(public_key(&reopened.machine).unwrap(), original);
        assert!(!reopened.ensure_room_revision("!test:sonar.invalid", 1).await.unwrap());
        assert!(reopened.ensure_room_revision("!test:sonar.invalid", 0).await.is_err());
        assert!(reopened.ensure_room_revision("!test:sonar.invalid", 2).await.unwrap());
        reopened.machine.share_room_key(&room, std::iter::empty(), settings).await.unwrap();
        let second = reopened.machine.encrypt_room_event(&room, ruma::events::room::message::RoomMessageEventContent::text_plain("synthetic two")).await.unwrap();
        let first = serde_json::to_value(first.content).unwrap(); let second = serde_json::to_value(second.content).unwrap();
        assert_ne!(first["session_id"], second["session_id"]);
        reopened.close().await.unwrap();
        assert!(open(&root, "http://127.0.0.1:5001", account, user, device, &name).await.is_err());
        assert!(open(&root, origin, account, user, "OTHER", &name).await.is_err());
        fs::remove_file(root.join(&name).join(DB)).unwrap();
        assert!(open(&root, origin, account, user, device, &name).await.is_err());
        fs::remove_dir_all(&root).unwrap();
    }
}
