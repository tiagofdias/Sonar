mod api;
mod audio;
mod ice_candidate;
mod crypto;
mod key_backup;
mod message_content;
mod live;
mod voice_codec;
mod voice_audio;
mod voice_transport;
mod voice_session;
mod matrix_attachment;
#[cfg(windows)] mod native_file;
mod matrix_store;
mod matrix_binding;
mod matrix_verification;
mod matrix_history;
mod matrix_contact;
#[cfg(windows)]
mod passkey;
#[cfg(windows)]
mod key_store;
use iced::widget::{button, column, container, row, text, text_input, scrollable, Space};
use iced::{Color, Element, Length, Subscription, Task, Theme};

fn main() -> iced::Result {
    iced::application(Sonar::default, Sonar::update, Sonar::view)
        .title("Sonar - Native Desktop Preview")
        .theme(Theme::Dark)
        .subscription(Sonar::subscription)
        .run()
}

struct Sonar {
    server: String,
    email: String,
    password: String,
    session: Option<api::Session>,
    busy: bool,
    notice: String,
    directory: Option<api::Directory>,
    loading_directory: bool,
    selected: Selection,
    generation: u64,
    profile_name: String,
    sessions: Option<api::Sessions>,
    pending_revoke: Option<String>,
    current_password: String,
    new_password: String,
    confirm_password: String,
    fingerprint: Option<Option<String>>,
    community_name: String,
    invite: String,
    channel_name: String,
    channel_voice: bool,
    created_invite: Option<api::CreatedInvite>,
    invite_confirmation: bool,
    passkeys: Option<Vec<api::Passkey>>,
    passkey_target: Option<String>,
    passkey_password: String,
    passkey_name: String,
    passkey_registering: bool,
    member_details: Option<api::Community>,
    member_confirmation: Option<(String, String, api::MemberAction)>,
    bans: Option<Vec<api::Person>>,
    ban_confirmation: Option<(String, String)>,
    audio_devices: Option<audio::Devices>,
    selected_output: Option<String>,
    selected_input: Option<String>,
    leave_confirmation: Option<String>,
    live_generation: u64,
    live_connected: bool,
    online_users: Vec<String>,
    call_handle: Option<live::CallHandle>,
    incoming_call: Option<live::IncomingCall>,
    call_declining: bool,
    matrix_target: String,
    matrix_devices: Option<Vec<api::MatrixDeviceEntry>>,
    matrix_revoke_target: Option<String>,
    matrix_listener: Option<std::sync::Arc<tokio::sync::Mutex<Option<matrix_verification::DeviceListener>>>>,
    matrix_requests: Vec<(String, String)>,
    matrix_flow: Option<std::sync::Arc<tokio::sync::Mutex<matrix_verification::DeviceVerification>>>,
    matrix_codes: Option<(u16, u16, u16)>,
    matrix_done: bool,
    matrix_history: Option<MatrixHistoryView>,
    matrix_refresh_pending: bool,
    matrix_loading_peer: Option<String>,
    matrix_draft: String,
    matrix_reply: Option<String>,
    matrix_delete_confirmation: Option<String>,
    matrix_discard_confirmation: bool,
    contact_device: String,
    contact_flow: Option<std::sync::Arc<tokio::sync::Mutex<matrix_contact::ContactVerification>>>,
    contact_codes: Option<(u16,u16,u16)>,
    contact_done: bool,
    contact_listener: Option<std::sync::Arc<tokio::sync::Mutex<Option<matrix_contact::ContactListener>>>>,
    contact_requests: Vec<String>,
    contact_devices: Vec<(String, String)>,
}
#[derive(Clone)]
struct MatrixHistoryItem { id: String, text: Option<String>, reply_to: Option<String>, reaction: Option<(String, String)>, attachment: Option<matrix_history::MatrixAttachment>, sender: String, pinned: bool, expires_at: Option<String> }
#[derive(Clone)]
struct MatrixHistoryView { peer: String, items: Vec<MatrixHistoryItem>, next_cursor: Option<String>, pinned_only: bool }
async fn load_matrix_history(session: api::Session, peer: String, before: Option<String>, pinned_only: bool) -> Result<(Vec<MatrixHistoryItem>, Option<String>), String> {
    let root = std::env::var_os("LOCALAPPDATA").ok_or("Pasta local indisponivel")?;
    let mut store = matrix_store::open_local(&std::path::PathBuf::from(root).join("Sonar").join("matrix"), &session).await?;
    let room = store.prepare_direct_room(&session, &peer).await?;
    let trusted = session.trusted_matrix_room_devices(&store.machine, &room).await?;
    store.receive_room_keys(&session, &room, &trusted).await?;
    let page = session.matrix_room_history(&store.machine, &room, before, pinned_only).await?;
    let mut items = Vec::new();
    for row in page.messages { let message = matrix_history::decrypt_content(&store.machine, &row, &trusted).await; let (text, reply_to, reaction, attachment) = match message { Some(matrix_history::MatrixContent::Text(text, reply)) => (Some(text), reply, None, None), Some(matrix_history::MatrixContent::Reaction(target, emoji)) => (None, None, Some((target, emoji)), None), Some(matrix_history::MatrixContent::Attachment(file)) => (None, None, None, Some(file)), None => (None, None, None, None) }; items.push(MatrixHistoryItem { id: row.id, text, reply_to, reaction, attachment, sender: row.sender, pinned: row.pinned, expires_at: row.expires_at }); }
    store.close().await?;
    Ok((items, page.next_cursor))
}
async fn send_matrix_reaction(session: api::Session, peer: String, target: String, emoji: String) -> Result<String, String> {
    let root = std::env::var_os("LOCALAPPDATA").ok_or("Pasta local indisponivel")?;
    let mut store = matrix_store::open_local(&std::path::PathBuf::from(root).join("Sonar").join("matrix"), &session).await?;
    let room = store.prepare_direct_room(&session, &peer).await?;
    store.queue_room_reaction(&session, &room, &target, &emoji).await?;
    let result = store.deliver_pending_room_text(&session, &room).await;
    let closed = store.close().await;
    result.and_then(|id| closed.map(|()| id))
}
async fn manage_matrix_message(session: api::Session, peer: String, id: String, pin: Option<bool>) -> Result<(), String> {
    let root = std::env::var_os("LOCALAPPDATA").ok_or("Pasta local indisponivel")?;
    let mut store = matrix_store::open_local(&std::path::PathBuf::from(root).join("Sonar").join("matrix"), &session).await?;
    let room = store.prepare_direct_room(&session, &peer).await?;
    session.trusted_matrix_room_devices(&store.machine, &room).await?;
    match pin { Some(value) => session.pin_matrix_message(&store.machine, &room, &id, value).await?, None => session.delete_matrix_message(&store.machine, &room, &id).await? }
    store.close().await
}
async fn discard_matrix_pending(session: api::Session, peer: String) -> Result<(), String> {
    let root = std::env::var_os("LOCALAPPDATA").ok_or("Pasta local indisponivel")?;
    let mut store = matrix_store::open_local(&std::path::PathBuf::from(root).join("Sonar").join("matrix"), &session).await?;
    let room = store.prepare_direct_room(&session, &peer).await?;
    store.discard_pending_room_text(&session, &room).await?;
    store.close().await
}
#[cfg(windows)]
async fn send_matrix_file(session: api::Session, peer: String) -> (bool, Result<bool, String>) {
    let mut queued = false;
    let result = async {
        let selected = tokio::task::spawn_blocking(|| -> Result<Option<(String, zeroize::Zeroizing<Vec<u8>>)>, String> {
            use std::io::Read;
            let Some(path) = native_file::open()? else { return Ok(None); };
            let name = path.file_name().and_then(|name| name.to_str()).ok_or("Nome de ficheiro invalido")?.to_owned();
            let file = std::fs::File::open(&path).map_err(|_| "Nao foi possivel abrir o ficheiro")?;
            let mut bytes = zeroize::Zeroizing::new(Vec::new());
            file.take((matrix_attachment::MAX_BYTES + 1) as u64).read_to_end(&mut bytes).map_err(|_| "Nao foi possivel ler o ficheiro")?;
            if bytes.is_empty() || bytes.len() > matrix_attachment::MAX_BYTES { return Err("O ficheiro deve ter entre 1 byte e 10 MiB".into()); }
            Ok(Some((name, bytes)))
        }).await.map_err(|_| "Nao foi possivel selecionar o ficheiro")??;
        let Some((name, bytes)) = selected else { return Ok(false); };
        let root = std::env::var_os("LOCALAPPDATA").ok_or("Pasta local indisponivel")?;
        let mut store = matrix_store::open_local(&std::path::PathBuf::from(root).join("Sonar").join("matrix"), &session).await?;
        let room = store.prepare_direct_room(&session, &peer).await?;
        store.queue_room_attachment(&session, &room, &name, &bytes).await?; queued = true;
        store.deliver_pending_room_text(&session, &room).await?;
        store.close().await?; Ok(true)
    }.await;
    (queued, result)
}
#[cfg(windows)]
async fn save_matrix_file(session: api::Session, peer: String, file: matrix_history::MatrixAttachment) -> Result<bool, String> {
    let name = file.name.clone();
    let path = tokio::task::spawn_blocking(move || native_file::save(&name)).await.map_err(|_| "Nao foi possivel selecionar o destino")??;
    let Some(path) = path else { return Ok(false); };
    let root = std::env::var_os("LOCALAPPDATA").ok_or("Pasta local indisponivel")?;
    let mut store = matrix_store::open_local(&std::path::PathBuf::from(root).join("Sonar").join("matrix"), &session).await?;
    let room = store.prepare_direct_room(&session, &peer).await?;
    session.trusted_matrix_room_devices(&store.machine, &room).await?;
    let ciphertext = session.download_matrix_attachment(&store.machine, &room, &file.id, file.size).await?;
    let plaintext = zeroize::Zeroizing::new(matrix_attachment::decrypt(&ciphertext, &file.metadata)?);
    store.close().await?;
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        use std::io::Write;
        let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|_| "Nao foi possivel criar o ficheiro; escolhe um nome que ainda nao exista")?;
        output.write_all(&plaintext).map_err(|_| "Nao foi possivel guardar o ficheiro")?;
        Ok(())
    }).await.map_err(|_| "Nao foi possivel guardar o ficheiro")??;
    Ok(true)
}
async fn send_matrix_text(session: api::Session, peer: String, text: Option<String>, reply: Option<String>) -> (bool, Result<String, String>) {
    use zeroize::Zeroize;
    let mut queued = false;
    let result = async {
        let root = std::env::var_os("LOCALAPPDATA").ok_or("Pasta local indisponivel")?;
        let mut store = matrix_store::open_local(&std::path::PathBuf::from(root).join("Sonar").join("matrix"), &session).await?;
        let room = store.prepare_direct_room(&session, &peer).await?;
        if let Some(mut text) = text {
            let result = store.queue_room_text_with_reply(&session, &room, &text, reply.as_deref()).await;
            text.zeroize(); result?; queued = true;
        }
        let id = store.deliver_pending_room_text(&session, &room).await?;
        store.close().await?;
        Ok(id)
    }.await;
    (queued, result)
}
impl Default for Sonar {
    fn default() -> Self {
        Self { server: "http://127.0.0.1:5000".into(), email: String::new(), password: String::new(), session: None, busy: false, notice: String::new(), directory: None, loading_directory: false, selected: Selection::Home, generation: 0, profile_name: String::new(), sessions: None, pending_revoke: None, current_password: String::new(), new_password: String::new(), confirm_password: String::new(), fingerprint: None, community_name: String::new(), invite: String::new(), channel_name: String::new(), channel_voice: false, created_invite: None, invite_confirmation: false, passkeys: None, passkey_target: None, passkey_password: String::new(), passkey_name: String::new(), passkey_registering: false, member_details: None, member_confirmation: None, bans: None, ban_confirmation: None, audio_devices: None, selected_output: None, selected_input: None, leave_confirmation: None, live_generation: 0, live_connected: false, online_users: Vec::new(), call_handle: None, incoming_call: None, call_declining: false, matrix_target: String::new(), matrix_devices: None, matrix_revoke_target: None, matrix_listener: None, matrix_requests: Vec::new(), matrix_flow: None, matrix_codes: None, matrix_done: false, matrix_history: None, matrix_refresh_pending: false, matrix_loading_peer: None, matrix_draft: String::new(), matrix_reply: None, matrix_delete_confirmation: None, matrix_discard_confirmation: false, contact_device: String::new(), contact_flow: None, contact_codes: None, contact_done: false, contact_listener: None, contact_requests: Vec::new(), contact_devices: Vec::new() }
    }
}
#[derive(Clone, Default)]
enum Selection { #[default] Home, Person(String), Community(String), Channel(String, String), Profile, Sessions, Password, Privacy, CreateCommunity, JoinCommunity, CreateChannel(String), RenameCommunity(String), RenameChannel(String, String), InviteManagement(String), Passkeys, Members(String), Bans(String), Audio }
#[derive(Clone)]
enum Message {
    Server(String), Email(String), Password(String), Login,
    #[cfg(windows)]
    PasskeyLogin,
    LoginResult(Result<api::Session, String>), Logout, LogoutResult(Result<(), String>),
    LiveEvent(u64, live::Event), ReconnectLive,
    DeclineCall, DeclineCallResult(u64, String, Result<(), String>),
    SelectOutput(Option<String>), SelectInput(Option<String>), TestMicrophone, MicrophoneResult(u64, Result<u32, String>), TestSpeakers, SpeakersResult(u64, Result<(), String>),
    LoadAudio, AudioResult(u64, Result<audio::Devices, String>),
    ConfirmLeave(String), CancelLeave, ApplyLeave, LeaveResult(u64, String, Result<(), String>),
    LoadBans(String), BansResult(u64, String, Result<Vec<api::Person>, String>), ConfirmUnban(String, String), CancelUnban, ApplyUnban, UnbanResult(u64, String, Result<(), String>),
    LoadMembers(String), MembersResult(u64, String, Result<api::Community, String>), ConfirmMember(String, String, api::MemberAction), CancelMember, ApplyMember, MemberResult(u64, String, Result<(), String>),
    LoadPasskeys, PasskeysResult(u64, Result<Vec<api::Passkey>, String>), SelectPasskey(String), CancelPasskey, PasskeyPassword(String), RemovePasskey, RemovePasskeyResult(u64, Result<(), String>),
    #[cfg(windows)]
    ShowPasskeyRegistration, PasskeyName(String), CancelPasskeyRegistration, RegisterPasskey, PasskeyRegistrationResult(u64, Result<api::Passkey, String>),
    AskInvite, CancelInvite, GenerateInvite(String), InviteResult(u64, String, Result<api::CreatedInvite, String>), CopyInvite,
    ChannelName(String), ChannelType(bool), CreateChannel(String), RenameCommunity(String), RenameChannel(String, String),
    CommunityName(String), Invite(String), CreateCommunity, JoinCommunity, CommunityResult(u64, Result<api::Community, String>),
    LoadFingerprint, FingerprintResult(u64, Result<Option<String>, String>),
    SetupMatrix, SetupMatrixResult(u64, Result<String, String>),
    LoadMatrixDevices, MatrixDevicesResult(u64, Result<Vec<api::MatrixDeviceEntry>, String>),
    SelectMatrixDevice(String), CancelMatrixDeviceRemoval, RemoveMatrixDevice, MatrixDeviceRemoved(u64, Result<(), String>),
    MatrixTarget(String), StartMatrixVerification, MatrixStarted(u64, Result<std::sync::Arc<tokio::sync::Mutex<matrix_verification::DeviceVerification>>, String>),
    ListenMatrixDevices, MatrixRequests(u64, Result<(std::sync::Arc<tokio::sync::Mutex<Option<matrix_verification::DeviceListener>>>, Vec<(String, String)>), String>), AcceptMatrixRequest(String), StopMatrixListener,
    PollMatrixVerification, ConfirmMatrixVerification, CancelMatrixVerification, MatrixCancelled(u64, Result<(), String>), MatrixProgress(u64, Result<(Option<(u16,u16,u16)>, bool), String>), CloseMatrixVerification, ApproveMatrixDevice, MatrixApproved(u64, Result<(), String>),
    LoadMatrixHistory(String, bool, bool), MatrixHistoryResult(u64, String, bool, bool, Result<(Vec<MatrixHistoryItem>, Option<String>), String>),
    PruneMatrixHistory,
    MatrixDraft(String), SendMatrixText(String, bool), MatrixTextResult(u64, String, bool, Result<String, String>),
    #[cfg(windows)] SendMatrixFile(String),
    #[cfg(windows)] MatrixFileSent(u64, String, bool, Result<bool, String>),
    #[cfg(windows)] SaveMatrixFile(String, String),
    #[cfg(windows)] MatrixFileSaved(u64, String, Result<bool, String>),
    MatrixReply(String), CancelMatrixReply,
    MatrixReact(String, String, String), MatrixReactionResult(u64, String, Result<String, String>),
    PinMatrixMessage(String, String, bool), ConfirmMatrixDelete(String), CancelMatrixDelete, DeleteMatrixMessage(String), MatrixManageResult(u64, String, Result<(), String>),
    AskDiscardMatrix, CancelDiscardMatrix, DiscardMatrix(String), DiscardMatrixResult(u64, String, Result<(), String>),
    ContactDevice(String), StartContact(String), ContactStarted(u64, String, Result<std::sync::Arc<tokio::sync::Mutex<matrix_contact::ContactVerification>>, String>),
    ContactPoll, ContactConfirm, ContactCancel, ContactProgress(u64, Result<(Option<(u16,u16,u16)>, bool), String>), ContactCancelled(u64, Result<(), String>), ContactClose,
    ListenContact(String), ContactRequests(u64, String, Result<(std::sync::Arc<tokio::sync::Mutex<Option<matrix_contact::ContactListener>>>, Vec<String>), String>), AcceptContact(String, String),
    ListContactDevices(String), ContactDevicesResult(u64, String, Result<Vec<(String, String)>, String>),
    CurrentPassword(String), NewPassword(String), ConfirmPassword(String), ChangePassword, PasswordResult(u64, Result<(), String>),
    Refresh, DirectoryResult(u64, Result<api::Directory, String>), Select(Selection), ProfileName(String), SaveProfile, ProfileResult(u64, Result<api::Account, String>),
    LoadSessions, SessionsResult(u64, Result<api::Sessions, String>), ConfirmRevoke(String), CancelRevoke, Revoke, RevokeResult(u64, Result<bool, String>),
}
impl Sonar {
    fn subscription(&self) -> Subscription<Message> {
        let live = match &self.session {
            Some(session) => Subscription::run_with(session.live_auth(self.live_generation), live::events).map(|(generation, event)| Message::LiveEvent(generation, event)),
            None => Subscription::none(),
        };
        let expiry = if self.session.is_some() && self.matrix_history.is_some() { iced::time::every(std::time::Duration::from_secs(15)).map(|_| Message::PruneMatrixHistory) } else { Subscription::none() };
        Subscription::batch([live, expiry])
    }
    #[cfg(windows)]
    fn passkey_login_button(&self) -> Element<'_, Message> {
        if self.busy { button("Entrar com passkey").into() }
        else { button("Entrar com passkey").on_press(Message::PasskeyLogin).into() }
    }
    #[cfg(not(windows))]
    fn passkey_login_button(&self) -> Element<'_, Message> {
        text("Passkeys nativas indisponiveis nesta plataforma.").size(12).into()
    }
    #[cfg(windows)]
    fn passkey_registration_view(&self, muted: Color) -> Element<'_, Message> {
        if !self.passkey_registering {
            return if self.busy { button("Adicionar passkey").into() } else { button("Adicionar passkey").on_press(Message::ShowPasskeyRegistration).into() };
        }
        column![
            text("Adicionar passkey").size(16),
            text("Confirma a palavra-passe atual e conclui o pedido do Windows.").size(12).color(muted),
            text_input("Nome da passkey", &self.passkey_name).on_input(Message::PasskeyName).padding(12),
            text_input("Palavra-passe atual", &self.passkey_password).secure(true).on_input(Message::PasskeyPassword).padding(12),
            row![if self.busy || self.passkey_name.trim().is_empty() || self.passkey_password.is_empty() { button("Criar passkey") } else { button("Criar passkey").on_press(Message::RegisterPasskey) }, if self.busy { button("Cancelar") } else { button("Cancelar").on_press(Message::CancelPasskeyRegistration) }].spacing(10)
        ].spacing(10).into()
    }
    #[cfg(not(windows))]
    fn passkey_registration_view(&self, muted: Color) -> Element<'_, Message> {
        text("Criacao de passkeys indisponivel nesta plataforma.").size(12).color(muted).into()
    }
    fn clear_session(&mut self) {
        use zeroize::Zeroize;
        self.generation += 1; self.live_generation += 1; self.live_connected = false; self.online_users.clear(); self.call_handle = None; self.incoming_call = None; self.call_declining = false;
        self.session = None; self.directory = None; self.sessions = None; self.pending_revoke = None;
        self.matrix_flow = None; self.matrix_listener = None; self.matrix_requests.clear(); self.matrix_devices = None; self.matrix_revoke_target = None; self.matrix_target.clear(); self.matrix_codes = None; self.matrix_done = false; self.matrix_history = None; self.matrix_delete_confirmation = None; self.matrix_discard_confirmation = false; self.matrix_refresh_pending = false; self.matrix_loading_peer = None; self.matrix_draft.zeroize(); self.matrix_reply = None; self.contact_device.clear(); self.contact_flow = None; self.contact_codes = None; self.contact_done = false; self.contact_listener = None; self.contact_requests.clear(); self.contact_devices.clear();
        self.selected_output = None; self.selected_input = None; self.leave_confirmation = None; self.passkey_name.clear(); self.passkey_registering = false;
        self.profile_name.clear(); self.fingerprint = None; self.selected = Selection::Home; self.loading_directory = false;
        self.current_password.zeroize(); self.new_password.zeroize(); self.confirm_password.zeroize(); self.invite.zeroize(); if let Some(invite) = &mut self.created_invite { invite.token.zeroize(); } self.created_invite = None; self.invite_confirmation = false; self.member_details = None; self.member_confirmation = None; self.audio_devices = None; self.bans = None; self.ban_confirmation = None; self.passkeys = None; self.passkey_target = None; self.passkey_password.zeroize(); self.community_name.clear(); self.channel_name.clear(); self.channel_voice = false;
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.update_inner(message);
        if self.matrix_refresh_pending && !self.busy && self.matrix_flow.is_none() && self.matrix_listener.is_none() && self.contact_flow.is_none() && self.contact_listener.is_none() {
            if let (Selection::Person(peer), Some(view)) = (&self.selected, &self.matrix_history) {
                if &view.peer == peer {
                    let peer = peer.clone(); let pinned = view.pinned_only; self.matrix_refresh_pending = false;
                    return Task::batch([task, self.update_inner(Message::LoadMatrixHistory(peer, pinned, false))]);
                }
            }
        }
        task
    }
    fn update_inner(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Server(value) => self.server = value,
            Message::Email(value) => self.email = value,
            Message::Password(value) => self.password = value,
            #[cfg(windows)]
            Message::PasskeyLogin if !self.busy => {
                let window = passkey::active_window();
                if window == 0 { self.notice = "Ativa a janela e tenta novamente.".into(); }
                else {
                    use zeroize::Zeroize;
                    self.password.zeroize(); self.busy = true; self.notice.clear();
                    return Task::perform(api::passkey_login(self.server.clone(), window), Message::LoginResult);
                }
            }
            Message::Login if !self.busy && !self.email.trim().is_empty() && !self.password.is_empty() => {
                self.busy = true;
                self.notice.clear();
                let password = std::mem::take(&mut self.password);
                return Task::perform(api::login(self.server.clone(), self.email.clone(), password), Message::LoginResult);
            }
            Message::LoginResult(result) => {
                self.busy = false;
                match result {
                    Ok(session) => { self.profile_name = session.account.full_name.clone(); self.session = Some(session.clone()); self.generation += 1; self.live_generation += 1; self.live_connected = false; self.online_users.clear(); self.loading_directory = true;
                        let generation = self.generation;
                        return Task::perform(session.directory(), move |result| Message::DirectoryResult(generation, result)); }
                    Err(error) => self.notice = error,
                }
            }
            Message::Logout if !self.busy => {
                if let Some(session) = self.session.clone() {
                    self.busy = true;
                    return Task::perform(session.logout(), Message::LogoutResult);
                }
            }
            Message::LogoutResult(result) => {
                self.busy = false;
                match result { Ok(()) => { self.clear_session(); self.notice = "Sessao terminada.".into(); }, Err(error) => self.notice = error }
            }
            Message::SetupMatrix if !self.busy => {
                if let Some(session) = self.session.clone() {
                    self.busy = true; self.notice.clear(); let generation = self.generation;
                    return Task::perform(async move {
                        let root = std::env::var_os("LOCALAPPDATA").ok_or("Pasta local do Windows indisponivel")?;
                        matrix_store::setup_device(&std::path::PathBuf::from(root).join("Sonar").join("matrix"), &session).await
                    }, move |result| Message::SetupMatrixResult(generation, result));
                }
            }
            Message::SetupMatrixResult(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                match result {
                    Ok(status) => {
                        self.notice = if status == "authorized" { "Dispositivo Matrix autorizado pelo servidor.".into() } else { "Dispositivo Matrix registado; aguarda aprovacao e verificacao noutro dispositivo.".into() };
                        return self.update(Message::LoadMatrixDevices);
                    },
                    Err(error) => self.notice = error,
                }
            }
            Message::LoadMatrixDevices if !self.busy && self.matrix_flow.is_none() && self.matrix_listener.is_none() && self.contact_flow.is_none() && self.contact_listener.is_none() => {
                if let Some(session) = self.session.clone() {
                    let generation = self.generation; self.busy = true;
                    return Task::perform(async move {
                        let root = std::env::var_os("LOCALAPPDATA").ok_or("Pasta local indisponivel")?;
                        let store = matrix_store::open_local(&std::path::PathBuf::from(root).join("Sonar").join("matrix"), &session).await?;
                        let result = session.matrix_devices(&store.machine).await;
                        store.close().await?; result
                    }, move |result| Message::MatrixDevicesResult(generation, result));
                }
            }
            Message::MatrixDevicesResult(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                match result { Ok(devices) => { self.matrix_devices = Some(devices); self.matrix_revoke_target = None; }, Err(error) => { self.matrix_devices = None; self.matrix_revoke_target = None; self.notice = error; } }
            }
            Message::SelectMatrixDevice(id) if !self.busy => self.matrix_revoke_target = Some(id),
            Message::CancelMatrixDeviceRemoval if !self.busy => self.matrix_revoke_target = None,
            Message::RemoveMatrixDevice if !self.busy && self.matrix_flow.is_none() && self.matrix_listener.is_none() && self.contact_flow.is_none() && self.contact_listener.is_none() => {
                if let (Some(session), Some(target)) = (self.session.clone(), self.matrix_revoke_target.as_ref().and_then(|id| self.matrix_devices.as_ref()?.iter().find(|device| &device.device_id == id && !device.current)).cloned()) {
                    let generation = self.generation; self.busy = true; self.notice.clear();
                    return Task::perform(async move {
                        let root = std::env::var_os("LOCALAPPDATA").ok_or("Pasta local indisponivel")?;
                        let store = matrix_store::open_local(&std::path::PathBuf::from(root).join("Sonar").join("matrix"), &session).await?;
                        let result = session.revoke_matrix_device(&store.machine, &target).await;
                        store.close().await?; result
                    }, move |result| Message::MatrixDeviceRemoved(generation, result));
                }
            }
            Message::MatrixDeviceRemoved(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false; self.matrix_devices = None; self.matrix_revoke_target = None;
                match result { Ok(()) => { self.notice = "Acesso do dispositivo removido. Copias ja guardadas nesse dispositivo nao podem ser apagadas remotamente.".into(); return self.update(Message::LoadMatrixDevices); }, Err(error) => self.notice = error }
            }
            Message::MatrixTarget(value) => self.matrix_target = value,
            Message::ListenMatrixDevices if !self.busy && self.matrix_flow.is_none() && self.contact_flow.is_none() && self.contact_listener.is_none() => {
                if let Some(session) = self.session.clone() {
                    let existing = self.matrix_listener.clone(); let generation = self.generation; self.busy = true; self.notice.clear();
                    return Task::perform(async move {
                        let listener = if let Some(listener) = existing { listener } else {
                            let root = std::env::var_os("LOCALAPPDATA").ok_or("Pasta local indisponivel")?;
                            let store = matrix_store::open_local(&std::path::PathBuf::from(root).join("Sonar").join("matrix"), &session).await?;
                            std::sync::Arc::new(tokio::sync::Mutex::new(Some(matrix_verification::DeviceListener::new(store, session))))
                        };
                        let requests = listener.lock().await.as_mut().ok_or("Pedido indisponivel")?.requests().await?;
                        Ok((listener, requests))
                    }, move |result| Message::MatrixRequests(generation, result));
                }
            }
            Message::MatrixRequests(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                if !matches!(self.selected, Selection::Privacy) { self.matrix_listener = None; self.matrix_requests.clear(); return Task::none(); }
                match result { Ok((listener, requests)) => { self.matrix_listener = Some(listener); self.matrix_requests = requests; self.notice = if self.matrix_requests.is_empty() { "Nenhum pedido encontrado.".into() } else { "Pedido de verificacao encontrado.".into() }; }, Err(error) => { self.matrix_listener = None; self.matrix_requests.clear(); self.notice = error; } }
            }
            Message::AcceptMatrixRequest(flow) if !self.busy && self.matrix_flow.is_none() && self.matrix_requests.iter().any(|(id, _)| id == &flow) => {
                if let Some(listener) = self.matrix_listener.take() {
                    let generation = self.generation; self.busy = true; self.matrix_requests.clear();
                    return Task::perform(async move {
                        let listener = listener.lock().await.take().ok_or("Pedido indisponivel")?;
                        let verification = listener.accept(&flow).await?;
                        Ok(std::sync::Arc::new(tokio::sync::Mutex::new(verification)))
                    }, move |result| Message::MatrixStarted(generation, result));
                }
            }
            Message::StopMatrixListener if !self.busy => { self.matrix_listener = None; self.matrix_requests.clear(); }
            Message::StartMatrixVerification if !self.busy && self.matrix_flow.is_none() && self.matrix_listener.is_none() && self.contact_flow.is_none() && self.contact_listener.is_none() => {
                if let Some(session) = self.session.clone() {
                    let target = self.matrix_target.trim().to_owned(); let generation = self.generation;
                    self.busy = true; self.notice.clear();
                    return Task::perform(async move {
                        let root = std::env::var_os("LOCALAPPDATA").ok_or("Pasta local indisponivel")?;
                        let store = matrix_store::open_local(&std::path::PathBuf::from(root).join("Sonar").join("matrix"), &session).await?;
                        let flow = matrix_verification::DeviceVerification::start(store, session, target).await?;
                        Ok(std::sync::Arc::new(tokio::sync::Mutex::new(flow)))
                    }, move |result| Message::MatrixStarted(generation, result));
                }
            }
            Message::MatrixStarted(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                match result { Ok(flow) => { self.matrix_flow = Some(flow); self.matrix_codes = None; self.matrix_done = false; self.notice = "Verificacao iniciada. Atualiza o estado nos dois dispositivos e compara os codigos.".into(); }, Err(error) => self.notice = error }
            }
            Message::PollMatrixVerification | Message::ConfirmMatrixVerification if !self.busy => {
                let confirm = matches!(message, Message::ConfirmMatrixVerification);
                if let Some(flow) = self.matrix_flow.clone() {
                    let generation = self.generation; self.busy = true;
                    return Task::perform(async move {
                        let mut flow = flow.lock().await;
                        if confirm { flow.confirm().await?; } else { flow.poll().await?; }
                        Ok((flow.codes(), flow.done()))
                    }, move |result| Message::MatrixProgress(generation, result));
                }
            }
            Message::MatrixProgress(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                match result { Ok((codes, done)) => { self.matrix_codes = codes; self.matrix_done = done; if done { self.notice = "Codigos verificados. A aprovacao no servidor ainda tem de ser concluida no dispositivo autorizado.".into(); } }, Err(error) => self.notice = error }
            }
            Message::CancelMatrixVerification if !self.busy => {
                if let Some(flow) = self.matrix_flow.clone() {
                    let generation = self.generation; self.busy = true;
                    return Task::perform(async move { flow.lock().await.cancel().await }, move |result| Message::MatrixCancelled(generation, result));
                }
            }
            Message::MatrixCancelled(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                match result { Ok(()) => { self.matrix_flow = None; self.matrix_codes = None; self.matrix_done = false; self.notice = "Verificacao cancelada.".into(); }, Err(error) => self.notice = error }
            }
            Message::CloseMatrixVerification if !self.busy => { self.matrix_flow = None; self.matrix_codes = None; self.matrix_done = false; }
            Message::ApproveMatrixDevice if !self.busy && self.matrix_done => {
                if let Some(flow) = self.matrix_flow.clone() {
                    let generation = self.generation; self.busy = true;
                    return Task::perform(async move { flow.lock().await.approve().await }, move |result| Message::MatrixApproved(generation, result));
                }
            }
            Message::MatrixApproved(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                self.notice = match result { Ok(()) => "Dispositivo verificado e autorizado no servidor.".into(), Err(error) => error };
                self.matrix_devices = None;
            }
            Message::LoadMatrixHistory(peer, pinned_only, older) if !self.busy && self.matrix_flow.is_none() && self.matrix_listener.is_none() && self.contact_flow.is_none() && self.contact_listener.is_none() => {
                if let Some(session) = self.session.clone() {
                    let before = if older { self.matrix_history.as_ref().filter(|view| view.peer == peer && view.pinned_only == pinned_only).and_then(|view| view.next_cursor.clone()) } else { None };
                    if older && before.is_none() { return Task::none(); }
                    self.busy = true; self.notice.clear(); if !older { self.matrix_history = None; self.matrix_refresh_pending = false; } self.matrix_loading_peer = Some(peer.clone()); let generation = self.generation; let selected = peer.clone();
                    return Task::perform(load_matrix_history(session, peer, before, pinned_only), move |result| Message::MatrixHistoryResult(generation, selected, pinned_only, older, result));
                }
            }
            Message::MatrixHistoryResult(generation, peer, pinned_only, older, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false; self.matrix_loading_peer = None;
                if matches!(&self.selected, Selection::Person(id) if id == &peer) { match result {
                    Ok((mut items, next_cursor)) => {
                        if older { if let Some(view) = &mut self.matrix_history { if view.peer == peer && view.pinned_only == pinned_only { items.append(&mut view.items); view.items = items; view.next_cursor = next_cursor; } } }
                        else { self.matrix_history = Some(MatrixHistoryView { peer, items, next_cursor, pinned_only }); }
                    }, Err(error) => self.notice = error
                } }
            }
            Message::MatrixDraft(value) => self.matrix_draft = value,
            #[cfg(windows)]
            Message::SendMatrixFile(peer) if !self.busy && self.matrix_flow.is_none() && self.matrix_listener.is_none() && self.contact_flow.is_none() && self.contact_listener.is_none() => {
                if let Some(session) = self.session.clone() { let generation = self.generation; let selected = peer.clone(); self.busy = true; self.notice.clear(); return Task::perform(send_matrix_file(session, peer), move |(queued, result)| Message::MatrixFileSent(generation, selected, queued, result)); }
            }
            #[cfg(windows)]
            Message::MatrixFileSent(generation, peer, queued, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                if matches!(&self.selected, Selection::Person(id) if id == &peer) { match result { Ok(true) => { self.notice = "Anexo cifrado enviado.".into(); return self.update(Message::LoadMatrixHistory(peer, false, false)); }, Ok(false) => {}, Err(error) => self.notice = if queued { format!("{error} Tenta o envio pendente ou descarta-o.") } else { error } } }
            }
            #[cfg(windows)]
            Message::SaveMatrixFile(peer, id) if !self.busy && self.matrix_flow.is_none() && self.matrix_listener.is_none() && self.contact_flow.is_none() && self.contact_listener.is_none() => {
                if let (Some(session), Some(file)) = (self.session.clone(), self.matrix_history.as_ref().filter(|view| view.peer == peer).and_then(|view| view.items.iter().find(|item| item.id == id)).and_then(|item| item.attachment.clone())) {
                    let generation = self.generation; let selected = peer.clone(); self.busy = true; self.notice.clear();
                    return Task::perform(save_matrix_file(session, peer, file), move |result| Message::MatrixFileSaved(generation, selected, result));
                }
            }
            #[cfg(windows)]
            Message::MatrixFileSaved(generation, peer, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                if matches!(&self.selected, Selection::Person(id) if id == &peer) { self.notice = match result { Ok(true) => "Anexo guardado.".into(), Ok(false) => String::new(), Err(error) => error }; }
            }
            Message::AskDiscardMatrix if !self.busy => self.matrix_discard_confirmation = true,
            Message::CancelDiscardMatrix if !self.busy => self.matrix_discard_confirmation = false,
            Message::DiscardMatrix(peer) if !self.busy && self.matrix_flow.is_none() && self.matrix_listener.is_none() && self.contact_flow.is_none() && self.contact_listener.is_none() && self.matrix_discard_confirmation => {
                if let Some(session) = self.session.clone() { let generation = self.generation; let selected = peer.clone(); self.busy = true; return Task::perform(discard_matrix_pending(session, peer), move |result| Message::DiscardMatrixResult(generation, selected, result)); }
            }
            Message::DiscardMatrixResult(generation, peer, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                if matches!(&self.selected, Selection::Person(id) if id == &peer) { match result { Ok(()) => { self.matrix_discard_confirmation = false; self.notice = "Envio local descartado. A mensagem pode ja ter sido entregue.".into(); }, Err(error) => self.notice = error } }
            }
            Message::PruneMatrixHistory => {
                use zeroize::Zeroize;
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|v| v.as_millis() as i64).unwrap_or(i64::MAX);
                if let Some(view) = &mut self.matrix_history {
                    view.items.retain_mut(|item| {
                        let keep = item.pinned || item.expires_at.as_ref().and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok()).is_some_and(|v| v.timestamp_millis() > now);
                        if !keep { if let Some(text) = &mut item.text { text.zeroize(); } }
                        keep
                    });
                    if self.matrix_reply.as_ref().is_some_and(|id| !view.items.iter().any(|item| &item.id == id)) { self.matrix_reply = None; }
                    if self.matrix_delete_confirmation.as_ref().is_some_and(|id| !view.items.iter().any(|item| &item.id == id)) { self.matrix_delete_confirmation = None; }
                }
            }
            Message::MatrixReply(id) if !self.busy => self.matrix_reply = Some(id),
            Message::CancelMatrixReply if !self.busy => self.matrix_reply = None,
            Message::MatrixReact(peer, target, emoji) if !self.busy && self.matrix_flow.is_none() && self.matrix_listener.is_none() && self.contact_flow.is_none() && self.contact_listener.is_none() => {
                if let Some(session) = self.session.clone() {
                    let account = session.account.id.clone().unwrap_or_default();
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|v| v.as_millis() as i64).unwrap_or(i64::MAX);
                    let own = self.matrix_history.as_ref().and_then(|view| view.items.iter().rev().find(|item| (item.pinned || item.expires_at.as_ref().and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok()).is_some_and(|v| v.timestamp_millis() > now)) && item.sender.starts_with(&format!("@{account}:")) && item.reaction.as_ref().is_some_and(|(id, value)| id == &target && value == &emoji))).map(|item| item.id.clone());
                    let generation = self.generation; let selected = peer.clone(); self.busy = true;
                    return Task::perform(async move { if let Some(id) = own { manage_matrix_message(session, peer, id.clone(), None).await.map(|()| id) } else { send_matrix_reaction(session, peer, target, emoji).await } }, move |result| Message::MatrixReactionResult(generation, selected, result));
                }
            }
            Message::MatrixReactionResult(generation, peer, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                if matches!(&self.selected, Selection::Person(id) if id == &peer) { match result { Ok(_) => return self.update(Message::LoadMatrixHistory(peer, false, false)), Err(error) => self.notice = format!("{error} Se o envio ficou pendente, usa 'Tentar envio pendente'.") } }
            }
            Message::ConfirmMatrixDelete(id) if !self.busy => self.matrix_delete_confirmation = Some(id),
            Message::CancelMatrixDelete if !self.busy => self.matrix_delete_confirmation = None,
            Message::PinMatrixMessage(peer, id, pinned) if !self.busy && self.matrix_flow.is_none() && self.matrix_listener.is_none() && self.contact_flow.is_none() && self.contact_listener.is_none() => {
                if let Some(session) = self.session.clone() { let generation = self.generation; let selected = peer.clone(); self.busy = true; return Task::perform(manage_matrix_message(session, peer, id, Some(pinned)), move |result| Message::MatrixManageResult(generation, selected, result)); }
            }
            Message::DeleteMatrixMessage(peer) if !self.busy && self.matrix_flow.is_none() && self.matrix_listener.is_none() && self.contact_flow.is_none() && self.contact_listener.is_none() => {
                if let (Some(session), Some(id)) = (self.session.clone(), self.matrix_delete_confirmation.clone()) { let generation = self.generation; let selected = peer.clone(); self.busy = true; return Task::perform(manage_matrix_message(session, peer, id, None), move |result| Message::MatrixManageResult(generation, selected, result)); }
            }
            Message::MatrixManageResult(generation, peer, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                if matches!(&self.selected, Selection::Person(id) if id == &peer) { match result { Ok(()) => { self.matrix_delete_confirmation = None; let pinned = self.matrix_history.as_ref().is_some_and(|view| view.pinned_only); return self.update(Message::LoadMatrixHistory(peer, pinned, false)); }, Err(error) => self.notice = error } }
            }
            Message::SendMatrixText(peer, retry) if !self.busy && self.matrix_flow.is_none() && self.matrix_listener.is_none() && self.contact_flow.is_none() && self.contact_listener.is_none() => {
                if let Some(session) = self.session.clone() {
                    if !retry && (self.matrix_draft.trim().is_empty() || self.matrix_draft.len() > 32000) { self.notice = "Escreve uma mensagem ate 32000 bytes.".into(); }
                    else {
                        let text = if retry { None } else { Some(self.matrix_draft.clone()) };
                        let reply = if retry { None } else { self.matrix_reply.clone() };
                        let generation = self.generation; let selected = peer.clone(); self.busy = true; self.notice.clear();
                        return Task::perform(send_matrix_text(session, peer, text, reply), move |(queued, result)| Message::MatrixTextResult(generation, selected, queued, result));
                    }
                }
            }
            Message::MatrixTextResult(generation, peer, queued, result) if generation == self.generation && self.session.is_some() => {
                use zeroize::Zeroize;
                self.busy = false;
                if matches!(&self.selected, Selection::Person(id) if id == &peer) {
                    if queued { self.matrix_draft.zeroize(); self.matrix_reply = None; }
                    match result { Ok(_) => { self.matrix_draft.zeroize(); return self.update(Message::LoadMatrixHistory(peer, false, false)); }, Err(error) => self.notice = if queued { format!("{error} Usa 'Tentar envio pendente' para reenviar a mesma mensagem cifrada.") } else { error } }
                }
            }
            Message::ContactDevice(value) if !self.busy => { self.contact_device = value; self.contact_listener = None; self.contact_requests.clear(); }
            Message::ListContactDevices(peer) if !self.busy && self.matrix_flow.is_none() && self.matrix_listener.is_none() && self.contact_flow.is_none() && self.contact_listener.is_none() => {
                if let Some(session) = self.session.clone() {
                    let generation = self.generation; let selected = peer.clone(); self.busy = true; self.notice.clear();
                    return Task::perform(async move {
                        let root = std::env::var_os("LOCALAPPDATA").ok_or("Pasta local indisponivel")?;
                        let mut store = matrix_store::open_local(&std::path::PathBuf::from(root).join("Sonar").join("matrix"), &session).await?;
                        let room = store.prepare_direct_room(&session, &peer).await?;
                        let contact_user = room.members.iter().find(|user| user.as_str() != store.machine.user_id().as_str()).ok_or("Contacto indisponivel")?.clone();
                        let devices = session.matrix_room_devices(&store.machine, &room, false).await?;
                        let results = devices.into_iter().filter(|device| device.user_id == contact_user).map(|device| (device.device_id, device.ed25519)).collect();
                        store.close().await?; Ok(results)
                    }, move |result| Message::ContactDevicesResult(generation, selected, result));
                }
            }
            Message::ContactDevicesResult(generation, peer, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                if matches!(&self.selected, Selection::Person(id) if id == &peer) { match result { Ok(devices) => { self.contact_devices = devices; if self.contact_devices.is_empty() { self.notice = "O contacto ainda nao tem dispositivos Matrix disponiveis.".into(); } }, Err(error) => self.notice = error } }
            }
            Message::ListenContact(peer) if !self.busy && self.matrix_flow.is_none() && self.matrix_listener.is_none() && self.contact_flow.is_none() => {
                if let Some(session) = self.session.clone() {
                    let target = self.contact_device.trim().to_owned(); let existing = self.contact_listener.clone(); let generation = self.generation; let selected = peer.clone(); self.busy = true; self.notice.clear();
                    return Task::perform(async move {
                        let listener = if let Some(listener) = existing { listener } else {
                            let root = std::env::var_os("LOCALAPPDATA").ok_or("Pasta local indisponivel")?;
                            let store = matrix_store::open_local(&std::path::PathBuf::from(root).join("Sonar").join("matrix"), &session).await?;
                            let listener = matrix_contact::ContactListener::open(store, session, &peer, &target).await?;
                            std::sync::Arc::new(tokio::sync::Mutex::new(Some(listener)))
                        };
                        let requests = listener.lock().await.as_mut().ok_or("Verificacao indisponivel")?.requests().await?;
                        Ok((listener, requests))
                    }, move |result| Message::ContactRequests(generation, selected, result));
                }
            }
            Message::ContactRequests(generation, peer, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                if matches!(&self.selected, Selection::Person(id) if id == &peer) { match result { Ok((listener, requests)) => { self.contact_listener = Some(listener); self.contact_requests = requests; self.notice = if self.contact_requests.is_empty() { "Nenhum pedido encontrado.".into() } else { "Pedido de verificacao encontrado.".into() }; }, Err(error) => self.notice = error } }
            }
            Message::AcceptContact(peer, flow_id) if !self.busy && self.contact_flow.is_none() => {
                if let Some(listener) = self.contact_listener.clone() {
                    let generation = self.generation; let selected = peer.clone(); self.busy = true;
                    return Task::perform(async move {
                        let listener = listener.lock().await.take().ok_or("Pedido indisponivel")?;
                        let flow = listener.accept(&flow_id).await?;
                        Ok(std::sync::Arc::new(tokio::sync::Mutex::new(flow)))
                    }, move |result| Message::ContactStarted(generation, selected, result));
                }
            }
            Message::StartContact(peer) if !self.busy && self.matrix_flow.is_none() && self.matrix_listener.is_none() && self.contact_flow.is_none() && self.contact_listener.is_none() => {
                if let Some(session) = self.session.clone() {
                    let target = self.contact_device.trim().to_owned(); let generation = self.generation; let selected = peer.clone(); self.busy = true; self.notice.clear();
                    return Task::perform(async move {
                        let root = std::env::var_os("LOCALAPPDATA").ok_or("Pasta local indisponivel")?;
                        let store = matrix_store::open_local(&std::path::PathBuf::from(root).join("Sonar").join("matrix"), &session).await?;
                        let flow = matrix_contact::ContactVerification::start(store, session, &peer, &target).await?;
                        Ok(std::sync::Arc::new(tokio::sync::Mutex::new(flow)))
                    }, move |result| Message::ContactStarted(generation, selected, result));
                }
            }
            Message::ContactStarted(generation, peer, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                if matches!(&self.selected, Selection::Person(id) if id == &peer) { self.contact_listener = None; self.contact_requests.clear(); match result { Ok(flow) => { self.contact_flow = Some(flow); self.contact_codes = None; self.contact_done = false; self.notice = "Verificacao iniciada. Continua no outro dispositivo e atualiza o estado.".into(); }, Err(error) => self.notice = error } }
            }
            Message::ContactPoll | Message::ContactConfirm | Message::ContactCancel if !self.busy => {
                let confirm = matches!(message, Message::ContactConfirm); let cancel = matches!(message, Message::ContactCancel);
                if let Some(flow) = self.contact_flow.clone() {
                    let generation = self.generation; self.busy = true;
                    if cancel { return Task::perform(async move { flow.lock().await.cancel().await }, move |result| Message::ContactCancelled(generation, result)); }
                    return Task::perform(async move { let mut flow = flow.lock().await; if confirm { flow.confirm().await?; } else { flow.poll().await?; } Ok((flow.codes(), flow.done())) }, move |result| Message::ContactProgress(generation, result));
                }
            }
            Message::ContactProgress(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                match result { Ok((codes, done)) => { self.contact_codes = codes; self.contact_done = done; if done { self.notice = "Contacto verificado neste dispositivo.".into(); } }, Err(error) => self.notice = error }
            }
            Message::ContactCancelled(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                match result { Ok(()) => { self.contact_flow = None; self.contact_codes = None; self.contact_done = false; self.notice = "Verificacao cancelada.".into(); }, Err(error) => self.notice = error }
            }
            Message::ContactClose if !self.busy => { self.contact_flow = None; self.contact_listener = None; self.contact_requests.clear(); self.contact_codes = None; self.contact_done = false; }
            Message::LiveEvent(generation, event) if generation == self.live_generation && self.session.is_some() => match event {
                live::Event::Connected(handle) => { self.live_connected = true; self.call_handle = Some(handle); }
                live::Event::Disconnected => { self.live_connected = false; self.online_users.clear(); self.call_handle = None; self.incoming_call = None; self.call_declining = false; }
                live::Event::Online(ids) => self.online_users = ids,
                live::Event::MatrixChanged => {
                    if let Selection::Person(peer) = &self.selected { if self.matrix_loading_peer.as_ref() == Some(peer) || self.matrix_history.as_ref().is_some_and(|view| &view.peer == peer) { self.matrix_refresh_pending = true; } }
                }
                live::Event::Revoked => { self.clear_session(); self.busy = false; self.notice = "Sessao terminada pelo servidor. Inicia sessao novamente.".into(); }
                live::Event::Incoming(call) => { if self.incoming_call.is_none() { self.incoming_call = Some(call); } }
                live::Event::Answer(_) => {} // Outgoing calls are not yet exposed by the preview.
                live::Event::Candidate(candidate) => {
                    if let Some(call) = &mut self.incoming_call {
                        if call.id == candidate.id && call.from == candidate.from && call.candidates.len() < 128 { call.candidates.push(candidate.candidate); }
                    }
                }
                live::Event::CallEnded(id) => { if self.incoming_call.as_ref().is_some_and(|call| call.id == id) { self.incoming_call = None; self.call_declining = false; } }
            },
            Message::ReconnectLive if self.session.is_some() && !self.live_connected => { self.live_generation += 1; self.online_users.clear(); }
            Message::DeclineCall if !self.call_declining => {
                if let (Some(handle), Some(call)) = (self.call_handle.clone(), self.incoming_call.clone()) {
                    self.call_declining = true; let generation = self.live_generation; let id = call.id.clone();
                    return Task::perform(handle.decline(call), move |result| Message::DeclineCallResult(generation, id, result));
                }
            }
            Message::DeclineCallResult(generation, id, result) if generation == self.live_generation && self.session.is_some() => {
                self.call_declining = false;
                if self.incoming_call.as_ref().is_some_and(|call| call.id == id) { self.incoming_call = None; }
                if let Err(error) = result { self.notice = error; }
            }
            Message::SelectOutput(output) if !self.busy => { self.selected_output = output; self.notice.clear(); }
            Message::ConfirmLeave(id) if !self.busy => self.leave_confirmation = Some(id),
            Message::CancelLeave if !self.busy => self.leave_confirmation = None,
            Message::ApplyLeave if !self.busy => {
                if let (Some(session), Some(id)) = (self.session.clone(), self.leave_confirmation.clone()) {
                    self.busy = true; self.notice.clear(); let generation = self.generation; let target = id.clone();
                    return Task::perform(session.leave_community(id), move |result| Message::LeaveResult(generation, target, result));
                }
            }
            Message::LeaveResult(generation, id, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false; self.leave_confirmation = None;
                match result {
                    Ok(()) => { if let Some(directory) = &mut self.directory { directory.communities.retain(|community| community.id != id); } self.selected = Selection::Home; self.notice = "Saida confirmada.".into(); }
                    Err(error) => self.notice = error,
                }
            }
            Message::SelectInput(input) if !self.busy => { self.selected_input = input; self.notice.clear(); }
            Message::TestMicrophone if !self.busy => {
                self.busy = true; self.notice = "A ouvir durante 3 segundos...".into(); let generation = self.generation;
                let input = self.selected_input.clone();
                return Task::perform(async move { tokio::task::spawn_blocking(move || audio::test_input(input)).await.map_err(|_| "Teste do microfone interrompido.".to_string()).and_then(|result| result) }, move |result| Message::MicrophoneResult(generation, result));
            }
            Message::MicrophoneResult(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false; self.notice = match result { Ok(level) => format!("Teste concluido. Nivel maximo: {level}% ."), Err(error) => error };
            }
            Message::TestSpeakers if !self.busy => {
                self.busy = true; self.notice.clear(); let generation = self.generation;
                let output = self.selected_output.clone();
                return Task::perform(async move { tokio::task::spawn_blocking(move || audio::test_output(output)).await.map_err(|_| "Teste de som interrompido.".to_string()).and_then(|result| result) }, move |result| Message::SpeakersResult(generation, result));
            }
            Message::SpeakersResult(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false; self.notice = match result { Ok(()) => "Teste de altifalantes concluido.".into(), Err(error) => error };
            }
            Message::LoadAudio if !self.busy => {
                self.busy = true; self.notice.clear(); let generation = self.generation;
                return Task::perform(async { audio::enumerate() }, move |result| Message::AudioResult(generation, result));
            }
            Message::AudioResult(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false; match result { Ok(devices) => self.audio_devices = Some(devices), Err(error) => self.notice = error }
            }
            Message::LoadBans(id) if !self.busy => {
                if let Some(session) = self.session.clone() {
                    self.selected = Selection::Bans(id.clone()); self.bans = None; self.ban_confirmation = None; self.busy = true; self.notice.clear();
                    let generation = self.generation; let target = id.clone();
                    return Task::perform(session.bans(id), move |result| Message::BansResult(generation, target, result));
                }
            }
            Message::BansResult(generation, id, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                if matches!(&self.selected, Selection::Bans(selected) if selected == &id) {
                    match result { Ok(bans) => self.bans = Some(bans), Err(error) => self.notice = error }
                }
            }
            Message::ConfirmUnban(community, member) if !self.busy => self.ban_confirmation = Some((community, member)),
            Message::CancelUnban if !self.busy => self.ban_confirmation = None,
            Message::ApplyUnban if !self.busy => {
                if let (Some(session), Some((community, member))) = (self.session.clone(), self.ban_confirmation.clone()) {
                    self.busy = true; self.notice.clear(); let generation = self.generation; let target = community.clone();
                    return Task::perform(session.unban(community, member), move |result| Message::UnbanResult(generation, target, result));
                }
            }
            Message::UnbanResult(generation, id, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false; self.ban_confirmation = None;
                match result {
                    Ok(()) => {
                        self.notice = "Ban removido. Cria um convite novo para permitir a entrada.".into();
                        if matches!(&self.selected, Selection::Bans(selected) if selected == &id) {
                            if let Some(session) = self.session.clone() { self.busy = true; let target = id.clone(); return Task::perform(session.bans(id), move |result| Message::BansResult(generation, target, result)); }
                        }
                    }
                    Err(error) => { self.bans = None; self.notice = error; }
                }
            }
            Message::LoadMembers(id) if !self.busy => {
                if let Some(session) = self.session.clone() {
                    self.selected = Selection::Members(id.clone()); self.member_details = None; self.member_confirmation = None; self.busy = true; self.notice.clear();
                    let generation = self.generation; let target = id.clone();
                    return Task::perform(session.community_details(id), move |result| Message::MembersResult(generation, target, result));
                }
            }
            Message::MembersResult(generation, id, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                if matches!(&self.selected, Selection::Members(selected) if selected == &id) {
                    match result { Ok(details) if details.id == id => self.member_details = Some(details), Ok(_) => self.notice = "Resposta de comunidade invalida.".into(), Err(error) => self.notice = error }
                }
            }
            Message::ConfirmMember(community, member, action) if !self.busy => self.member_confirmation = Some((community, member, action)),
            Message::CancelMember if !self.busy => self.member_confirmation = None,
            Message::ApplyMember if !self.busy => {
                if let (Some(session), Some((community, member, action))) = (self.session.clone(), self.member_confirmation.clone()) {
                    self.busy = true; self.notice.clear(); let generation = self.generation; let target = community.clone();
                    return Task::perform(session.manage_member(community, member, action), move |result| Message::MemberResult(generation, target, result));
                }
            }
            Message::MemberResult(generation, id, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false; self.member_confirmation = None;
                match result {
                    Ok(()) => {
                        self.notice = "Membro atualizado.".into();
                        if matches!(&self.selected, Selection::Members(selected) if selected == &id) {
                            if let Some(session) = self.session.clone() {
                                self.busy = true; let target = id.clone();
                                return Task::perform(session.community_details(id), move |result| Message::MembersResult(generation, target, result));
                            }
                        }
                    }
                    Err(error) => { self.member_details = None; self.notice = error; }
                }
            }
            Message::LoadPasskeys if !self.busy => {
                if let Some(session) = self.session.clone() { self.busy = true; self.notice.clear(); let generation = self.generation;
                    return Task::perform(session.passkeys(), move |result| Message::PasskeysResult(generation, result)); }
            }
            Message::PasskeysResult(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false; match result { Ok(keys) => self.passkeys = Some(keys), Err(error) => self.notice = error }
            }
            #[cfg(windows)]
            Message::ShowPasskeyRegistration if !self.busy => {
                use zeroize::Zeroize;
                self.passkey_password.zeroize(); self.passkey_target = None; self.passkey_name = "Sonar Windows".into(); self.passkey_registering = true; self.notice.clear();
            }
            #[cfg(windows)]
            Message::PasskeyName(value) => self.passkey_name = value,
            #[cfg(windows)]
            Message::CancelPasskeyRegistration if !self.busy => { use zeroize::Zeroize; self.passkey_password.zeroize(); self.passkey_name.clear(); self.passkey_registering = false; }
            #[cfg(windows)]
            Message::RegisterPasskey if !self.busy && self.passkey_registering && !self.passkey_name.trim().is_empty() && !self.passkey_password.is_empty() => {
                if let Some(session) = self.session.clone() {
                    let window = passkey::active_window();
                    if window == 0 { self.notice = "Ativa a janela e tenta novamente.".into(); }
                    else {
                        self.busy = true; self.notice.clear(); let generation = self.generation;
                        let password = std::mem::take(&mut self.passkey_password);
                        return Task::perform(session.register_passkey(self.passkey_name.clone(), password, window), move |result| Message::PasskeyRegistrationResult(generation, result));
                    }
                }
            }
            #[cfg(windows)]
            Message::PasskeyRegistrationResult(generation, result) if generation == self.generation && self.session.is_some() => {
                use zeroize::Zeroize;
                self.passkey_password.zeroize(); self.busy = false;
                match result {
                    Ok(_) => {
                        self.passkey_registering = false; self.passkey_name.clear(); self.notice = "Passkey adicionada. A atualizar a lista.".into();
                        if let Some(session) = self.session.clone() { self.busy = true; return Task::perform(session.passkeys(), move |result| Message::PasskeysResult(generation, result)); }
                    }
                    Err(error) => self.notice = error,
                }
            }
            Message::SelectPasskey(id) if !self.busy => { use zeroize::Zeroize; self.passkey_password.zeroize(); self.passkey_registering = false; self.passkey_name.clear(); self.passkey_target = Some(id); }
            Message::CancelPasskey if !self.busy => { use zeroize::Zeroize; self.passkey_password.zeroize(); self.passkey_target = None; }
            Message::PasskeyPassword(value) => self.passkey_password = value,
            Message::RemovePasskey if !self.busy && !self.passkey_password.is_empty() => {
                if let (Some(session), Some(id)) = (self.session.clone(), self.passkey_target.clone()) {
                    self.busy = true; self.notice.clear(); let generation = self.generation;
                    let password = std::mem::take(&mut self.passkey_password);
                    return Task::perform(session.remove_passkey(id, password), move |result| Message::RemovePasskeyResult(generation, result));
                }
            }
            Message::RemovePasskeyResult(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false; match result { Ok(()) => { self.clear_session(); self.notice = "Passkey removida. Inicia sessao novamente.".into(); }, Err(error) => self.notice = error }
            }
            Message::AskInvite if !self.busy => self.invite_confirmation = true,
            Message::CancelInvite if !self.busy => self.invite_confirmation = false,
            Message::GenerateInvite(id) if !self.busy && self.invite_confirmation => {
                if let Some(session) = self.session.clone() {
                    use zeroize::Zeroize;
                    if let Some(invite) = &mut self.created_invite { invite.token.zeroize(); }
                    self.created_invite = None; self.invite_confirmation = false; self.busy = true; self.notice.clear();
                    let generation = self.generation; let target = id.clone();
                    return Task::perform(session.create_invite(id), move |result| Message::InviteResult(generation, target, result));
                }
            }
            Message::InviteResult(generation, id, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                match result {
                    Ok(mut invite) => {
                        if matches!(&self.selected, Selection::InviteManagement(current) if current == &id) { self.created_invite = Some(invite); }
                        else { use zeroize::Zeroize; invite.token.zeroize(); }
                    }
                    Err(error) => self.notice = error,
                }
            }
            Message::CopyInvite => {
                if let Some(invite) = &self.created_invite { self.notice = "Codigo copiado. Qualquer pessoa com ele pode entrar ate expirar.".into(); return iced::clipboard::write(invite.token.clone()); }
            }
            Message::ChannelName(value) => self.channel_name = value,
            Message::ChannelType(voice) => self.channel_voice = voice,
            Message::RenameCommunity(id) if !self.busy && !self.community_name.trim().is_empty() => {
                if let Some(session) = self.session.clone() {
                    self.busy = true; self.notice.clear(); let generation = self.generation;
                    return Task::perform(session.rename_community(id, self.community_name.clone()), move |result| Message::CommunityResult(generation, result));
                }
            }
            Message::RenameChannel(id, channel) if !self.busy && !self.channel_name.trim().is_empty() => {
                if let Some(session) = self.session.clone() {
                    self.busy = true; self.notice.clear(); let generation = self.generation;
                    return Task::perform(session.rename_channel(id, channel, self.channel_name.clone()), move |result| Message::CommunityResult(generation, result));
                }
            }
            Message::CreateChannel(id) if !self.busy && !self.channel_name.trim().is_empty() => {
                if let Some(session) = self.session.clone() {
                    self.busy = true; self.notice.clear(); let generation = self.generation;
                    let kind = if self.channel_voice { "voice" } else { "text" };
                    return Task::perform(session.create_channel(id, self.channel_name.clone(), kind), move |result| Message::CommunityResult(generation, result));
                }
            }
            Message::CommunityName(value) => self.community_name = value,
            Message::Invite(value) => self.invite = value,
            Message::CreateCommunity if !self.busy && !self.community_name.trim().is_empty() => {
                if let Some(session) = self.session.clone() {
                    self.busy = true; self.notice.clear(); let generation = self.generation;
                    return Task::perform(session.create_community(self.community_name.clone()), move |result| Message::CommunityResult(generation, result));
                }
            }
            Message::JoinCommunity if !self.busy && !self.invite.trim().is_empty() => {
                if let Some(session) = self.session.clone() {
                    self.busy = true; self.notice.clear(); let generation = self.generation;
                    let invite = std::mem::take(&mut self.invite);
                    return Task::perform(session.join_community(invite), move |result| Message::CommunityResult(generation, result));
                }
            }
            Message::CommunityResult(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                match result {
                    Ok(community) => {
                        self.community_name.clear(); self.channel_name.clear(); self.channel_voice = false;
                        self.selected = Selection::Community(community.id.clone());
                        if let Some(directory) = &mut self.directory {
                            directory.communities.retain(|item| item.id != community.id);
                            directory.communities.push(community);
                        } else { self.selected = Selection::Home; }
                        self.notice = "Comunidade atualizada.".into();
                        self.generation += 1;
                        if let Some(session) = self.session.clone() {
                            self.loading_directory = true;
                            let generation = self.generation;
                            return Task::perform(session.directory(), move |result| Message::DirectoryResult(generation, result));
                        }
                    }
                    Err(error) => self.notice = error,
                }
            }
            Message::LoadFingerprint if !self.busy => {
                if let Some(session) = self.session.clone() {
                    self.busy = true; self.notice.clear(); let generation = self.generation;
                    return Task::perform(session.identity_fingerprint(), move |result| Message::FingerprintResult(generation, result));
                }
            }
            Message::FingerprintResult(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false; match result { Ok(value) => self.fingerprint = Some(value), Err(error) => self.notice = error }
            }
            Message::CurrentPassword(value) => self.current_password = value,
            Message::NewPassword(value) => self.new_password = value,
            Message::ConfirmPassword(value) => self.confirm_password = value,
            Message::ChangePassword if !self.busy => {
                use zeroize::Zeroize;
                if self.new_password != self.confirm_password { self.notice = "As palavras-passe nao coincidem.".into(); return Task::none(); }
                if self.current_password.is_empty() || self.current_password.len() > 72 || self.new_password.encode_utf16().count() < 10 || self.new_password.len() > 72 || self.current_password == self.new_password {
                    self.notice = "Usa uma nova palavra-passe com pelo menos 10 caracteres e ate 72 bytes.".into(); return Task::none();
                }
                if let Some(session) = self.session.clone() {
                    self.busy = true; self.notice.clear(); self.confirm_password.zeroize();
                    let current = std::mem::take(&mut self.current_password); let next = std::mem::take(&mut self.new_password); let generation = self.generation;
                    return Task::perform(session.change_password(current, next), move |result| Message::PasswordResult(generation, result));
                }
            }
            Message::PasswordResult(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                match result { Ok(()) => { self.clear_session(); self.notice = "Palavra-passe atualizada. Inicia sessao novamente.".into(); }, Err(error) => self.notice = error }
            }
            Message::Refresh if !self.loading_directory => {
                if let Some(session) = self.session.clone() {
                    self.loading_directory = true; self.notice.clear();
                    let generation = self.generation;
                    return Task::perform(session.directory(), move |result| Message::DirectoryResult(generation, result));
                }
            }
            Message::DirectoryResult(generation, result) if generation == self.generation && self.session.is_some() => {
                self.loading_directory = false;
                match result { Ok(directory) => self.directory = Some(directory), Err(error) => self.notice = error }
            }
            Message::ProfileName(value) => self.profile_name = value,
            Message::SaveProfile if !self.busy => {
                if let Some(session) = self.session.clone() {
                    self.busy = true; self.notice.clear(); let generation = self.generation;
                    return Task::perform(session.update_profile(self.profile_name.clone()), move |result| Message::ProfileResult(generation, result));
                }
            }
            Message::ProfileResult(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                match result { Ok(account) => { self.profile_name = account.full_name.clone(); if let Some(session) = &mut self.session { session.account = account; } self.notice = "Perfil atualizado.".into(); }, Err(error) => self.notice = error }
            }
            Message::LoadSessions if !self.busy => {
                if let Some(session) = self.session.clone() {
                    self.busy = true; self.notice.clear(); let generation = self.generation;
                    return Task::perform(session.sessions(), move |result| Message::SessionsResult(generation, result));
                }
            }
            Message::SessionsResult(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false; match result { Ok(sessions) => self.sessions = Some(sessions), Err(error) => self.notice = error }
            }
            Message::ConfirmRevoke(id) if !self.busy => self.pending_revoke = Some(id),
            Message::CancelRevoke if !self.busy => self.pending_revoke = None,
            Message::Revoke if !self.busy => {
                if let (Some(session), Some(id)) = (self.session.clone(), self.pending_revoke.clone()) {
                    self.busy = true; self.notice.clear(); let generation = self.generation;
                    return Task::perform(session.revoke_session(id), move |result| Message::RevokeResult(generation, result));
                }
            }
            Message::RevokeResult(generation, result) if generation == self.generation && self.session.is_some() => {
                self.busy = false;
                match result {
                    Ok(true) => { self.clear_session(); self.notice = "Sessao terminada.".into(); }
                    Ok(false) => { let id = self.pending_revoke.take(); if let Some(sessions) = &mut self.sessions { sessions.sessions.retain(|entry| Some(&entry.id) != id.as_ref()); } self.notice = "Sessao terminada.".into(); }
                    Err(error) => self.notice = error,
                }
            }
            Message::Select(selection) => {
                use zeroize::Zeroize;
                self.matrix_listener = None; self.matrix_requests.clear();
                self.matrix_history = None; self.matrix_delete_confirmation = None; self.matrix_discard_confirmation = false; self.matrix_refresh_pending = false; self.matrix_loading_peer = None; self.matrix_draft.zeroize(); self.matrix_reply = None; self.contact_flow = None; self.contact_listener = None; self.contact_requests.clear(); self.contact_codes = None; self.contact_done = false; self.contact_device.clear(); self.contact_devices.clear();
                self.leave_confirmation = None;
                self.passkey_name.clear(); self.passkey_registering = false;
                self.current_password.zeroize(); self.new_password.zeroize(); self.confirm_password.zeroize(); self.invite.zeroize(); self.passkey_password.zeroize(); self.passkey_target = None; self.member_confirmation = None; self.member_details = None; self.bans = None; self.ban_confirmation = None;
                if let Some(invite) = &mut self.created_invite { invite.token.zeroize(); } self.created_invite = None; self.invite_confirmation = false;
                match (&selection, &self.directory) {
                    (Selection::RenameCommunity(id), Some(directory)) => self.community_name = directory.communities.iter().find(|community| &community.id == id).map(|community| community.name.clone()).unwrap_or_default(),
                    (Selection::RenameChannel(id, channel_id), Some(directory)) => self.channel_name = directory.communities.iter().find(|community| &community.id == id).and_then(|community| community.channels.iter().find(|channel| &channel.id == channel_id)).map(|channel| channel.name.clone()).unwrap_or_default(),
                    _ => {}
                }
                self.selected = selection;
            },
            _ => {}
        }
        Task::none()
    }
    fn view(&self) -> Element<'_, Message> {
        let accent = Color::from_rgb8(190, 160, 232);
        let muted = Color::from_rgb8(152, 142, 166);
        if let Some(session) = &self.session { return self.workspace(session, accent, muted); }
        let left = container(column![
            text("sonar").size(30).color(accent),
            Space::new().height(Length::Fill),
            text("MENOS RUÍDO. MAIS LIGAÇÃO.").size(11).color(accent),
            text("As tuas pessoas.\nAs tuas conversas.\nO teu espaço.").size(30),
            text("Uma aplicação desktop nativa, criada em Rust.").size(14).color(muted),
            Space::new().height(Length::Fill),
            text("Grátis. Sem subscrições. Sem anúncios.").size(11).color(muted),
        ].spacing(22)).padding(40).width(Length::FillPortion(1)).height(Length::Fill)
            .style(|_| container::Style::default().background(Color::from_rgb8(24, 23, 30)));
        let content: Element<'_, Message> = if let Some(session) = &self.session {
            column![
                text("SESSÃO INICIADA").size(10).color(accent),
                text(format!("Olá, {}.", session.account.full_name)).size(28),
                text("Esta é uma prévia nativa. As mensagens e chamadas ainda não estão disponíveis no desktop.").size(14).color(muted),
                text("O histórico encriptado precisa de ligação segura entre dispositivos, ainda em desenvolvimento.").size(13).color(muted),
                if self.busy { button("A terminar...") } else { button("Terminar esta sessão").on_press(Message::Logout) },
                text(&self.notice).size(12).color(accent),
            ].spacing(18).max_width(450).into()
        } else {
            column![
                text("O TEU ESPAÇO").size(10).color(accent),
                text("Bem-vindo de volta.").size(28),
                text("Inicia sessão no Sonar.").size(14).color(muted),
                text("Servidor").size(12).color(muted),
                text_input("https://servidor", &self.server).on_input(Message::Server).padding(12),
                text("Email").size(12).color(muted),
                text_input("O teu email", &self.email).on_input(Message::Email).padding(12),
                text("Palavra-passe").size(12).color(muted),
                text_input("Palavra-passe", &self.password).secure(true).on_input(Message::Password).on_submit(Message::Login).padding(12),
                if self.busy { button("A entrar...") } else { button("Entrar no Sonar").on_press(Message::Login) },
                self.passkey_login_button(),
                text(&self.notice).size(12).color(accent),
                text("Previa: mensagens e voz ainda nao estao disponiveis. A passkey nao transfere chaves de mensagens.").size(12).color(muted),
            ].spacing(10).max_width(400).into()
        };
        let right = container(content).padding(40).width(Length::FillPortion(1)).height(Length::Fill)
            .center_x(Length::Fill).center_y(Length::Fill)
            .style(|_| container::Style::default().background(Color::from_rgb8(31, 30, 37)));
        row![left, right].height(Length::Fill).into()
    }
    fn workspace<'a>(&'a self, session: &'a api::Session, accent: Color, muted: Color) -> Element<'a, Message> {
        let mut nav = column![text("sonar").size(26).color(accent), text("O TEU ESPA?O").size(10).color(muted),
            button("Mensagens diretas").on_press(Message::Select(Selection::Home)),
            button("O teu perfil").on_press(Message::Select(Selection::Profile)),
            button("Sessoes").on_press(Message::Select(Selection::Sessions)),
            button("Palavra-passe").on_press(Message::Select(Selection::Password)),
            button("Passkeys").on_press(Message::Select(Selection::Passkeys)),
            button("Privacidade").on_press(Message::Select(Selection::Privacy)),
            button("Voz e audio").on_press(Message::Select(Selection::Audio)),
            button("Criar comunidade").on_press(Message::Select(Selection::CreateCommunity)),
            button("Entrar com convite").on_press(Message::Select(Selection::JoinCommunity)), text("PESSOAS").size(10).color(muted)].spacing(12);
        nav = if self.live_connected { nav.push(text("Ligacao em tempo real ativa").size(11).color(muted)) }
            else { nav.push(button("Ligar eventos em tempo real").on_press(Message::ReconnectLive)) };
        if let Some(directory) = &self.directory {
            if directory.people.is_empty() { nav = nav.push(text("Ainda n?o h? pessoas.").size(12).color(muted)); }
            for person in &directory.people {
                let label = if self.live_connected && self.online_users.iter().any(|id| id == &person.id) { format!("{}  - online", person.name) } else { person.name.clone() };
                nav = nav.push(button(text(label).size(14)).on_press(Message::Select(Selection::Person(person.id.clone()))));
            }
            nav = nav.push(text("COMUNIDADES").size(10).color(muted));
            if directory.communities.is_empty() { nav = nav.push(text("Ainda n?o h? comunidades.").size(12).color(muted)); }
            for community in &directory.communities { nav = nav.push(button(text(&community.name).size(14)).on_press(Message::Select(Selection::Community(community.id.clone())))); }
        } else if self.loading_directory { nav = nav.push(text("A carregar...").size(12).color(muted)); }
        let sidebar = container(scrollable(nav)).padding(20).width(Length::Fixed(275.0)).height(Length::Fill)
            .style(|_| container::Style::default().background(Color::from_rgb8(26, 25, 32)));
        let mut content = column![text("SONAR DESKTOP \u{b7} PR\u{c9}VIA").size(10).color(accent)].spacing(16);
        if let Some(call) = &self.incoming_call {
            let name = self.directory.as_ref().and_then(|directory| directory.people.iter().find(|person| person.id == call.from)).map(|person| person.name.as_str()).unwrap_or("Um contacto");
            content = content.push(text(format!("Chamada recebida de {name}")).size(18))
                .push(text("Ainda nao e possivel atender chamadas nesta previa. Podes atende-la na web ou recusa-la aqui.").size(12).color(muted))
                .push(if self.call_declining { button("A recusar...") } else { button("Recusar chamada").on_press(Message::DeclineCall) });
        }
        match (&self.selected, &self.directory) {
            (Selection::Audio, _) => {
                content = content.push(text("Voz e audio").size(28))
                    .push(text("Consulta os dispositivos detetados pelo Windows. Esta verificacao nao grava nem transmite audio.").size(13).color(muted))
                    .push(if self.busy { button("A procurar...") } else { button("Procurar dispositivos").on_press(Message::LoadAudio) });
                if let Some(devices) = &self.audio_devices {
                    content = content.push(text("Microfones").size(16));
                    if devices.inputs.is_empty() { content = content.push(text("Nenhum microfone encontrado.").size(12).color(muted)); }
                    content = content.push(if self.busy { button("Usar microfone predefinido") } else { button("Usar microfone predefinido").on_press(Message::SelectInput(None)) });
                    for device in &devices.inputs {
                        let unique = devices.inputs.iter().filter(|item| *item == device).count() == 1;
                        let label = format!("{}{}{}", device, if devices.default_input.as_ref() == Some(device) && unique { " - predefinido" } else { "" }, if !unique { " - nome duplicado" } else { "" });
                        content = content.push(if self.busy || !unique { button(text(label).size(13)) } else { button(text(label).size(13)).on_press(Message::SelectInput(Some(device.clone()))) });
                    }
                    content = content.push(text(format!("Microfone para teste: {}", self.selected_input.as_deref().unwrap_or("predefinido do Windows"))).size(13))
                        .push(if self.busy { button("A testar...") } else { button("Testar microfone durante 3 segundos").on_press(Message::TestMicrophone) })
                        .push(text("O teste acede ao microfone durante 3 segundos e mostra o nivel maximo. O som nao e guardado nem transmitido.").size(12).color(muted));
                    content = content.push(text("Altifalantes e auscultadores").size(16));
                    if devices.outputs.is_empty() { content = content.push(text("Nenhum dispositivo de saida encontrado.").size(12).color(muted)); }
                    content = content.push(if self.busy { button("Usar predefinido do Windows") } else { button("Usar predefinido do Windows").on_press(Message::SelectOutput(None)) });
                    for device in &devices.outputs {
                        let unique = devices.outputs.iter().filter(|item| *item == device).count() == 1;
                        let label = format!("{}{}{}", device, if devices.default_output.as_ref() == Some(device) && unique { " - predefinido" } else { "" }, if !unique { " - nome duplicado" } else { "" });
                        content = content.push(if self.busy || !unique { button(text(label).size(13)) } else { button(text(label).size(13)).on_press(Message::SelectOutput(Some(device.clone()))) });
                    }
                }
                content = content.push(text(format!("Saida para teste: {}", self.selected_output.as_deref().unwrap_or("predefinido do Windows"))).size(13))
                    .push(if self.busy { button("A testar...") } else { button("Testar saida selecionada").on_press(Message::TestSpeakers) })
                    .push(text("O teste reproduz um tom curto localmente e nao usa o microfone. As escolhas aplicam-se apenas aos testes; as chamadas ainda nao estao disponiveis no desktop.").size(12).color(muted));
            }
            (Selection::Bans(id), _) => {
                content = content.push(text("Contas banidas").size(28))
                    .push(text("Remover um ban nao restaura a entrada. O convite atual e revogado e sera necessario criar outro.").size(13).color(muted))
                    .push(if self.busy { button("A carregar...") } else { button("Atualizar bans").on_press(Message::LoadBans(id.clone())) });
                if let Some(bans) = &self.bans {
                    if bans.is_empty() { content = content.push(text("Nao ha contas banidas.").size(13).color(muted)); }
                    for person in bans {
                        content = content.push(row![text(&person.name).size(13), if self.busy { button("Remover ban") } else { button("Remover ban").on_press(Message::ConfirmUnban(id.clone(), person.id.clone())) }].spacing(10));
                    }
                    if let Some((community_id, member_id)) = &self.ban_confirmation {
                        if community_id == id {
                            if let Some(person) = bans.iter().find(|person| &person.id == member_id) {
                                content = content.push(text(format!("Remover o ban de {}?", person.name)).size(14))
                                    .push(row![if self.busy { button("A atualizar...") } else { button("Confirmar").on_press(Message::ApplyUnban) }, if self.busy { button("Cancelar") } else { button("Cancelar").on_press(Message::CancelUnban) }].spacing(10));
                            }
                        }
                    }
                }
            }
            (Selection::Members(id), _) => {
                content = content.push(text("Membros").size(28))
                    .push(if self.busy { button("A carregar...") } else { button("Atualizar membros").on_press(Message::LoadMembers(id.clone())) });
                if let Some(community) = self.member_details.as_ref().filter(|item| &item.id == id) {
                    let me = session.account.id.as_deref().unwrap_or("");
                    let owner = community.owner_id.as_deref() == Some(me);
                    let moderator = community.moderators.iter().any(|item| item == me);
                    for member in &community.member_details {
                        let is_owner = community.owner_id.as_deref() == Some(member.id.as_str());
                        let is_moderator = community.moderators.contains(&member.id);
                        let role = if is_owner { "Dono" } else if is_moderator { "Moderador" } else { "Membro" };
                        let mut line = row![text(format!("{} - {role}", member.name)).size(13)].spacing(8);
                        if member.id != me && !is_owner && (owner || (moderator && !is_moderator)) {
                            let action = |kind| Message::ConfirmMember(community.id.clone(), member.id.clone(), kind);
                            line = line.push(button("Remover").on_press(action(api::MemberAction::Remove)))
                                .push(button("Banir").on_press(action(api::MemberAction::Ban)));
                            if owner { line = line.push(button(if is_moderator { "Retirar moderador" } else { "Tornar moderador" }).on_press(action(if is_moderator { api::MemberAction::Demote } else { api::MemberAction::Promote }))); }
                        }
                        content = content.push(line);
                    }
                    if let Some((community_id, member_id, action)) = &self.member_confirmation {
                        if community_id == id {
                            if let Some(member) = community.member_details.iter().find(|item| &item.id == member_id) {
                                let description = match action { api::MemberAction::Remove => "Remover este membro revoga o convite atual.", api::MemberAction::Ban => "Banir impede esta conta de voltar a entrar e revoga o convite atual.", api::MemberAction::Promote => "O moderador pode remover ou banir membros normais.", api::MemberAction::Demote => "O membro deixa de poder moderar." };
                                content = content.push(text(format!("Confirmar para {}?", member.name)).size(14)).push(text(description).size(12).color(muted))
                                    .push(row![if self.busy { button("A atualizar...") } else { button("Confirmar").on_press(Message::ApplyMember) }, if self.busy { button("Cancelar") } else { button("Cancelar").on_press(Message::CancelMember) }].spacing(10));
                            }
                        }
                    }
                }
            }
            (Selection::InviteManagement(id), Some(directory)) => {
                if let Some(community) = directory.communities.iter().find(|item| &item.id == id) {
                    content = content.push(text(format!("Convite para {}", community.name)).size(26))
                        .push(text("O convite dura 24 horas. Gerar um novo revoga o anterior; qualquer pessoa com o codigo pode entrar.").size(13).color(muted));
                    if self.invite_confirmation {
                        content = content.push(text("Confirmas a criacao e a revogacao do convite anterior?").size(13))
                            .push(row![if self.busy { button("A criar...") } else { button("Confirmar").on_press(Message::GenerateInvite(community.id.clone())) },
                                if self.busy { button("Cancelar") } else { button("Cancelar").on_press(Message::CancelInvite) }].spacing(10));
                    } else { content = content.push(if self.busy { button("A criar...") } else { button("Gerar convite").on_press(Message::AskInvite) }); }
                    if let Some(invite) = &self.created_invite {
                        content = content.push(text(&invite.token).size(13)).push(text(format!("Expira: {}", invite.expires_at)).size(12).color(muted))
                            .push(button("Copiar codigo").on_press(Message::CopyInvite))
                            .push(text("O codigo copiado fica na area de transferencia do sistema.").size(12).color(muted));
                    }
                }
            }
            (Selection::CreateChannel(id), Some(directory)) => {
                if let Some(community) = directory.communities.iter().find(|item| &item.id == id) {
                    content = content.push(text(format!("Criar canal em {}", community.name)).size(26))
                        .push(text("Apenas o dono pode criar canais. O desktop ainda nao permite mensagens ou chamadas.").size(13).color(muted))
                        .push(row![button("Texto").on_press(Message::ChannelType(false)), button("Voz").on_press(Message::ChannelType(true)),
                            text(if self.channel_voice { "Selecionado: voz" } else { "Selecionado: texto" }).size(12)].spacing(10))
                        .push(text_input("Nome do canal", &self.channel_name).on_input(Message::ChannelName).padding(12))
                        .push(if self.busy || self.channel_name.trim().is_empty() { button("Criar canal") } else { button("Criar canal").on_press(Message::CreateChannel(community.id.clone())) });
                }
            }
            (Selection::RenameCommunity(id), Some(directory)) => {
                if let Some(community) = directory.communities.iter().find(|item| &item.id == id && session.account.id.as_ref().is_some_and(|account| item.owner_id.as_ref() == Some(account))) {
                    content = content.push(text("Renomear comunidade").size(26))
                        .push(text_input("Nome da comunidade", &self.community_name).on_input(Message::CommunityName).padding(12))
                        .push(if self.busy || self.community_name.trim().is_empty() { button("Guardar nome") } else { button("Guardar nome").on_press(Message::RenameCommunity(community.id.clone())) });
                }
            }
            (Selection::RenameChannel(id, channel_id), Some(directory)) => {
                if let Some(community) = directory.communities.iter().find(|item| &item.id == id && session.account.id.as_ref().is_some_and(|account| item.owner_id.as_ref() == Some(account))) {
                    if let Some(channel) = community.channels.iter().find(|item| &item.id == channel_id) {
                        content = content.push(text(format!("Renomear {}", channel.name)).size(26))
                            .push(text("Letras sem acentos, numeros e hifens; espacos sao convertidos em hifens.").size(12).color(muted))
                            .push(text_input("Nome do canal", &self.channel_name).on_input(Message::ChannelName).padding(12))
                            .push(if self.busy || self.channel_name.trim().is_empty() { button("Guardar nome") } else { button("Guardar nome").on_press(Message::RenameChannel(community.id.clone(), channel.id.clone())) });
                    }
                }
            }
            (Selection::CreateCommunity, _) => {
                content = content.push(text("Criar comunidade").size(28))
                    .push(text("A comunidade inicia com um canal de texto geral. O desktop ainda nao permite ler ou enviar mensagens.").size(13).color(muted))
                    .push(text_input("Nome da comunidade", &self.community_name).on_input(Message::CommunityName).padding(12))
                    .push(if self.busy || self.community_name.trim().is_empty() { button("Criar") } else { button("Criar").on_press(Message::CreateCommunity) });
            }
            (Selection::JoinCommunity, _) => {
                content = content.push(text("Entrar numa comunidade").size(28))
                    .push(text("Cola um codigo ou ligacao de convite. O desktop ainda nao permite ler ou enviar mensagens.").size(13).color(muted))
                    .push(text_input("Convite", &self.invite).on_input(Message::Invite).padding(12))
                    .push(if self.busy || self.invite.trim().is_empty() { button("Entrar") } else { button("Entrar").on_press(Message::JoinCommunity) });
            }
            (Selection::Passkeys, _) => {
                content = content.push(text("Passkeys").size(28))
                    .push(text("As passkeys iniciam sessao, mas nao transferem chaves nem historico de mensagens.").size(13).color(muted))
                    .push(if self.busy { button("A carregar...") } else { button("Carregar passkeys").on_press(Message::LoadPasskeys) });
                content = content.push(self.passkey_registration_view(muted));
                if let Some(keys) = &self.passkeys {
                    if keys.is_empty() { content = content.push(text("Ainda nao tens passkeys registadas.").size(13).color(muted)); }
                    for key in keys {
                        let date = key.last_used_at.as_ref().unwrap_or(&key.created_at);
                        content = content.push(row![text(format!("{} | {}", key.name, date)).size(13),
                            if self.busy { button("Remover") } else { button("Remover").on_press(Message::SelectPasskey(key.id.clone())) }].spacing(10));
                    }
                }
                if let Some(target) = &self.passkey_target {
                    if let Some(key) = self.passkeys.as_ref().and_then(|keys| keys.iter().find(|key| &key.id == target)) {
                        content = content.push(text(format!("Remover {}?", key.name)).size(16))
                            .push(text("Confirma a palavra-passe atual. Todas as sessoes serao terminadas.").size(13).color(muted))
                            .push(text_input("Palavra-passe atual", &self.passkey_password).secure(true).on_input(Message::PasskeyPassword).padding(12))
                            .push(row![if self.busy || self.passkey_password.is_empty() { button("Confirmar remocao") } else { button("Confirmar remocao").on_press(Message::RemovePasskey) },
                                if self.busy { button("Cancelar") } else { button("Cancelar").on_press(Message::CancelPasskey) }].spacing(10));
                    }
                }
            }
            (Selection::Privacy, _) => {
                content = content.push(text("Identidade de encriptacao").size(28))
                    .push(text("Consulta a impressao digital da chave publica associada a tua conta. Compara-a fora do Sonar com a pessoa com quem queres verificar a identidade.").size(13).color(muted))
                    .push(if self.busy { button("A consultar...") } else { button("Consultar impressao digital").on_press(Message::LoadFingerprint) });
                match &self.fingerprint {
                    Some(Some(value)) => content = content.push(text(value).size(16).color(accent)),
                    Some(None) => content = content.push(text("A conta ainda nao tem uma chave publica associada.").size(13).color(muted)),
                    None => {}
                }
                content = content.push(text("Esta consulta nao transfere a chave privada nem ativa mensagens neste dispositivo. A chave apresentada pelo servidor deve ser verificada de forma independente.").size(13).color(muted));
                content = content.push(text("Dispositivo Matrix experimental").size(18))
                    .push(text("Regista este dispositivo sem alterar a identidade existente. Compara os codigos e aprova-o num dispositivo autorizado.").size(13).color(muted))
                    .push(if self.busy || self.matrix_listener.is_some() || self.matrix_flow.is_some() || self.contact_flow.is_some() || self.contact_listener.is_some() { button("Configurar dispositivo experimental") } else { button("Configurar dispositivo experimental").on_press(Message::SetupMatrix) });
                content = content.push(if self.busy || self.matrix_listener.is_some() || self.matrix_flow.is_some() || self.contact_flow.is_some() || self.contact_listener.is_some() { button("Mostrar dispositivos") } else { button("Mostrar dispositivos").on_press(Message::LoadMatrixDevices) });
                if let Some(devices) = &self.matrix_devices {
                    content = content.push(text("A autorizacao no servidor e separada da verificacao criptografica.").size(12).color(muted));
                    let can_remove = devices.iter().any(|device| device.current && device.server_authorization == "authorized");
                    for device in devices {
                        content = content.push(text(format!("{}{} — {}", device.device_id, if device.current { " (este dispositivo)" } else { "" }, device.server_authorization)).size(14))
                            .push(text(&device.ed25519).size(11).color(muted));
                        if !device.current && can_remove {
                            content = content.push(if self.busy { button("Remover dispositivo") } else { button("Remover dispositivo").on_press(Message::SelectMatrixDevice(device.device_id.clone())) });
                        }
                    }
                    if let Some(target) = &self.matrix_revoke_target {
                        content = content.push(text(format!("Remover {target}? O acesso termina, mas copias ja guardadas nao podem ser apagadas remotamente.")).size(12).color(muted))
                            .push(row![if self.busy { button("Confirmar remocao") } else { button("Confirmar remocao").on_press(Message::RemoveMatrixDevice) }, if self.busy { button("Cancelar") } else { button("Cancelar").on_press(Message::CancelMatrixDeviceRemoval) }].spacing(8));
                    }
                }
                if self.matrix_flow.is_none() {
                    content = content.push(text("Verificar outro dispositivo da mesma conta").size(16))
                        .push(text("Introduz o ID mostrado no outro dispositivo e aceita o pedido nesse dispositivo.").size(12).color(muted))
                        .push(text_input("ID do outro dispositivo", &self.matrix_target).on_input(Message::MatrixTarget).padding(12))
                        .push(if self.busy || self.matrix_listener.is_some() || self.matrix_target.trim().is_empty() { button("Iniciar verificacao") } else { button("Iniciar verificacao").on_press(Message::StartMatrixVerification) })
                        .push(if self.busy { button("Procurar pedidos") } else { button("Procurar pedidos").on_press(Message::ListenMatrixDevices) });
                    if self.matrix_listener.is_some() {
                        for (flow, device) in &self.matrix_requests {
                            content = content.push(row![text(format!("Pedido do dispositivo {device}")).size(13), if self.busy { button("Aceitar") } else { button("Aceitar").on_press(Message::AcceptMatrixRequest(flow.clone())) }].spacing(8));
                        }
                        content = content.push(if self.busy { button("Parar procura") } else { button("Parar procura").on_press(Message::StopMatrixListener) });
                    }
                } else {
                    content = content.push(text("Compara os tres numeros nos dois dispositivos antes de confirmar.").size(13).color(muted));
                    if let Some((a,b,c)) = self.matrix_codes { content = content.push(text(format!("{a}  {b}  {c}")).size(22).color(accent)); }
                    content = content.push(row![if self.busy { button("Atualizar") } else { button("Atualizar").on_press(Message::PollMatrixVerification) },
                        if self.busy || self.matrix_codes.is_none() || self.matrix_done { button("Os codigos coincidem") } else { button("Os codigos coincidem").on_press(Message::ConfirmMatrixVerification) },
                        if self.busy || self.matrix_done { button("Os codigos nao coincidem") } else { button("Os codigos nao coincidem").on_press(Message::CancelMatrixVerification) },
                        if self.busy { button("Fechar") } else { button("Fechar").on_press(Message::CloseMatrixVerification) }].spacing(10));
                    if self.matrix_done { content = content.push(if self.busy { button("Autorizar dispositivo verificado") } else { button("Autorizar dispositivo verificado").on_press(Message::ApproveMatrixDevice) }); }
                }
            }
            (Selection::Password, _) => {
                content = content.push(text("Alterar palavra-passe").size(28))
                    .push(text("Esta alteracao termina todas as sessoes, incluindo esta.").size(13).color(muted))
                    .push(text("Palavra-passe atual").size(12))
                    .push(text_input("Palavra-passe atual", &self.current_password).secure(true).on_input(Message::CurrentPassword).padding(12))
                    .push(text("Nova palavra-passe").size(12))
                    .push(text_input("Nova palavra-passe", &self.new_password).secure(true).on_input(Message::NewPassword).padding(12))
                    .push(text("Confirmar nova palavra-passe").size(12))
                    .push(text_input("Confirmar", &self.confirm_password).secure(true).on_input(Message::ConfirmPassword).padding(12))
                    .push(text("Minimo 10 caracteres; maximo 72 bytes UTF-8.").size(12).color(muted))
                    .push(if self.busy { button("A atualizar...") } else { button("Alterar e terminar sessoes").on_press(Message::ChangePassword) });
            }
            (Selection::Sessions, _) => {
                content = content.push(text("Sessoes ativas").size(28))
                    .push(text("Lista de inicios de sessao. Nao sao apresentados enderecos IP ou nomes de dispositivos.").size(13).color(muted))
                    .push(if self.busy { button("A carregar...") } else { button("Carregar sessoes").on_press(Message::LoadSessions) });
                if let Some(sessions) = &self.sessions {
                    if sessions.legacy_current { content = content.push(text("A sessao antiga nao aparece na lista.").size(12).color(muted)); }
                    if sessions.has_more { content = content.push(text("A mostrar as 100 sessoes mais recentes.").size(12).color(muted)); }
                    for item in &sessions.sessions {
                        let label = if item.method == "passkey" { "Passkey" } else { "Palavra-passe" };
                        let title = format!("{label}{} | {} | {}", if item.current { " - esta sessao" } else { "" }, item.created_at, item.expires_at);
                        content = content.push(row![text(title).size(12), button("Terminar").on_press(Message::ConfirmRevoke(item.id.clone()))].spacing(10));
                    }
                }
                if self.pending_revoke.is_some() {
                    content = content.push(text("Confirmas que queres terminar esta sessao?").size(13))
                        .push(row![if self.busy { button("A terminar...") } else { button("Confirmar").on_press(Message::Revoke) }, if self.busy { button("Cancelar") } else { button("Cancelar").on_press(Message::CancelRevoke) }].spacing(10));
                }
            }
            (Selection::Profile, _) => {
                content = content.push(text("O teu perfil").size(28))
                    .push(text("Nome apresentado").size(12).color(muted))
                    .push(text_input("Nome", &self.profile_name).on_input(Message::ProfileName).padding(12))
                    .push(if self.busy || self.profile_name.trim().is_empty() || self.profile_name.trim() == session.account.full_name {
                        button("Guardar nome")
                    } else { button("Guardar nome").on_press(Message::SaveProfile) });
            }
            (Selection::Person(id), Some(directory)) => {
                if let Some(person) = directory.people.iter().find(|item| &item.id == id) {
                    content = content.push(text(&person.name).size(28))
                        .push(text("Historico Matrix experimental; todos os dispositivos da conversa devem estar verificados.").size(14).color(muted))
                        .push(row![if self.busy || self.matrix_flow.is_some() || self.contact_flow.is_some() || self.contact_listener.is_some() { button("Consultar historico") } else { button("Consultar historico").on_press(Message::LoadMatrixHistory(id.clone(), false, false)) }, if self.busy || self.matrix_flow.is_some() || self.contact_flow.is_some() || self.contact_listener.is_some() { button("Ver fixadas") } else { button("Ver fixadas").on_press(Message::LoadMatrixHistory(id.clone(), true, false)) }].spacing(10));
                    if let Some(view) = &self.matrix_history {
                        if &view.peer == id {
                            if view.items.is_empty() { content = content.push(text("Sem mensagens neste historico.").size(13).color(muted)); }
                            if view.next_cursor.is_some() { content = content.push(if self.busy { button("A carregar...") } else { button("Carregar mensagens anteriores").on_press(Message::LoadMatrixHistory(id.clone(), view.pinned_only, true)) }); }
                            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|v| v.as_millis() as i64).unwrap_or(i64::MAX);
                            for item in &view.items {
                                if !item.pinned && !item.expires_at.as_ref().and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok()).is_some_and(|v| v.timestamp_millis() > now) { continue; }
                                if item.reaction.is_some() { continue; }
                                if let Some(reply) = &item.reply_to { content = content.push(text(format!("Em resposta a {reply}")).size(11).color(muted)); }
                                content = content.push(text(format!("{}: {}", item.sender, item.attachment.as_ref().map(|file| file.name.as_str()).or(item.text.as_deref()).unwrap_or("Mensagem cifrada indisponivel"))).size(14));
                                #[cfg(windows)]
                                if let Some(file) = &item.attachment { content = content.push(if self.busy { button(text(format!("Guardar anexo ({} bytes)", file.size))) } else { button(text(format!("Guardar anexo ({} bytes)", file.size))).on_press(Message::SaveMatrixFile(id.clone(), item.id.clone())) }); }
                                if item.text.is_some() || item.attachment.is_some() {
                                    let pin_label = if item.pinned { "Desafixar" } else { "Fixar" };
                                    let mut actions = row![if self.busy { button(pin_label) } else { button(pin_label).on_press(Message::PinMatrixMessage(id.clone(), item.id.clone(), !item.pinned)) }, if self.busy { button("Responder") } else { button("Responder").on_press(Message::MatrixReply(item.id.clone())) }].spacing(8);
                                    if session.account.id.as_ref().is_some_and(|account| item.sender.starts_with(&format!("@{account}:"))) { actions = actions.push(if self.busy { button("Apagar") } else { button("Apagar").on_press(Message::ConfirmMatrixDelete(item.id.clone())) }); }
                                    if !view.pinned_only { for emoji in ["👍", "❤️", "😂", "🎉"] {
                                        let mut senders = std::collections::HashSet::new();
                                        for reaction in &view.items { if (reaction.pinned || reaction.expires_at.as_ref().and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok()).is_some_and(|v| v.timestamp_millis() > now)) && reaction.reaction.as_ref().is_some_and(|(target, value)| target == &item.id && value == emoji) { senders.insert(reaction.sender.as_str()); } }
                                        let own = session.account.id.as_ref().is_some_and(|account| senders.iter().any(|sender| sender.starts_with(&format!("@{account}:"))));
                                        let label = if senders.is_empty() { emoji.to_owned() } else { format!("{emoji} {}{}", senders.len(), if own { " ✓" } else { "" }) };
                                        actions = actions.push(if self.busy { button(text(label)) } else { button(text(label)).on_press(Message::MatrixReact(id.clone(), item.id.clone(), emoji.into())) });
                                    } }
                                    content = content.push(actions);
                                }
                            }
                            if self.matrix_delete_confirmation.is_some() { content = content.push(text("Apagar esta mensagem?").size(13)).push(row![if self.busy { button("Confirmar") } else { button("Confirmar").on_press(Message::DeleteMatrixMessage(id.clone())) }, if self.busy { button("Cancelar") } else { button("Cancelar").on_press(Message::CancelMatrixDelete) }].spacing(8)); }
                        }
                    }
                    content = content
                        .push(text("Enviar mensagem Matrix experimental").size(16));
                    if let Some(reply) = &self.matrix_reply { content = content.push(row![text(format!("A responder a {reply}")).size(12), if self.busy { button("Cancelar resposta") } else { button("Cancelar resposta").on_press(Message::CancelMatrixReply) }].spacing(8)); }
                    content = content
                        .push(text_input("Mensagem", &self.matrix_draft).on_input(Message::MatrixDraft).padding(12))
                        .push(row![if self.busy || self.matrix_flow.is_some() || self.contact_flow.is_some() || self.contact_listener.is_some() || self.matrix_draft.trim().is_empty() { button("Enviar") } else { button("Enviar").on_press(Message::SendMatrixText(id.clone(), false)) }, if self.busy || self.matrix_flow.is_some() || self.contact_flow.is_some() || self.contact_listener.is_some() { button("Tentar envio pendente") } else { button("Tentar envio pendente").on_press(Message::SendMatrixText(id.clone(), true)) }].spacing(10))
                        .push(text("Verificar dispositivo do contacto").size(16));
                    #[cfg(windows)]
                    { content = content.push(if self.busy || self.matrix_flow.is_some() || self.matrix_listener.is_some() || self.contact_flow.is_some() || self.contact_listener.is_some() { button("Enviar anexo cifrado") } else { button("Enviar anexo cifrado").on_press(Message::SendMatrixFile(id.clone())) }); }
                    if !self.matrix_discard_confirmation { content = content.push(if self.busy { button("Descartar envio pendente") } else { button("Descartar envio pendente").on_press(Message::AskDiscardMatrix) }); }
                    else { content = content.push(text("Descartar a tentativa local? A mensagem pode ja ter sido entregue.").size(12).color(muted)).push(row![if self.busy { button("Confirmar descarte") } else { button("Confirmar descarte").on_press(Message::DiscardMatrix(id.clone())) }, if self.busy { button("Cancelar") } else { button("Cancelar").on_press(Message::CancelDiscardMatrix) }].spacing(8)); }
                    if self.contact_flow.is_none() {
                        content = content.push(text("Introduz o ID Matrix apresentado pelo contacto; compara os numeros por outro canal.").size(12).color(muted))
                            .push(if self.busy || self.matrix_flow.is_some() || self.contact_listener.is_some() { button("Listar dispositivos do contacto") } else { button("Listar dispositivos do contacto").on_press(Message::ListContactDevices(id.clone())) })
                            .push(text_input("ID do dispositivo do contacto", &self.contact_device).on_input(Message::ContactDevice).padding(12))
                            .push(row![if self.busy || self.matrix_flow.is_some() || self.contact_listener.is_some() || self.contact_device.trim().is_empty() { button("Iniciar verificacao do contacto") } else { button("Iniciar verificacao do contacto").on_press(Message::StartContact(id.clone())) }, if self.busy || self.matrix_flow.is_some() || self.contact_device.trim().is_empty() { button("Procurar pedidos") } else { button("Procurar pedidos").on_press(Message::ListenContact(id.clone())) }].spacing(10));
                        if !self.contact_devices.is_empty() { content = content.push(text("Dispositivos apresentados pelo servidor; ainda nao estao verificados.").size(12).color(muted)); }
                        for (device, key) in &self.contact_devices { content = content.push(row![if self.busy { button(text(device)) } else { button(text(device)).on_press(Message::ContactDevice(device.clone())) }, text(key).size(11).color(muted)].spacing(8)); }
                        for request in &self.contact_requests { content = content.push(if self.busy { button("Aceitar pedido") } else { button("Aceitar pedido").on_press(Message::AcceptContact(id.clone(), request.clone())) }); }
                        if self.contact_listener.is_some() { content = content.push(if self.busy { button("Fechar procura") } else { button("Fechar procura").on_press(Message::ContactClose) }); }
                    } else {
                        if let Some((a,b,c)) = self.contact_codes { content = content.push(text(format!("{a}  {b}  {c}")).size(22).color(accent)); }
                        content = content.push(text("Confirma apenas se os tres numeros forem iguais nos dois dispositivos.").size(12).color(muted))
                            .push(row![if self.busy { button("Atualizar") } else { button("Atualizar").on_press(Message::ContactPoll) }, if self.busy || self.contact_codes.is_none() || self.contact_done { button("Os codigos coincidem") } else { button("Os codigos coincidem").on_press(Message::ContactConfirm) }, if self.busy || self.contact_done { button("Nao coincidem") } else { button("Nao coincidem").on_press(Message::ContactCancel) }, if self.busy { button("Fechar") } else { button("Fechar").on_press(Message::ContactClose) }].spacing(10));
                    }
                    content = content
                        .push(text("Impress\u{e3}o digital da chave p\u{fa}blica").size(16));
                    match &person.encryption_key {
                        Some(key) => match crypto::fingerprint(key) {
                            Ok(value) => content = content.push(text(value).size(14).color(accent)),
                            Err(_) => content = content.push(text("A chave apresentada pelo servidor \u{e9} inv\u{e1}lida.").size(13).color(muted)),
                        },
                        None => content = content.push(text("Esta pessoa ainda n\u{e3}o tem uma chave p\u{fa}blica dispon\u{ed}vel.").size(13).color(muted)),
                    }
                    content = content.push(text("Compara esta impress\u{e3}o com a pessoa por um canal independente. O servidor pode apresentar uma chave incorreta; esta consulta n\u{e3}o verifica a identidade automaticamente. Atualiza os espa\u{e7}os para consultar altera\u{e7}\u{f5}es.").size(12).color(muted));
                }
            }
            (Selection::Community(id), Some(directory)) | (Selection::Channel(id, _), Some(directory)) => {
                if let Some(community) = directory.communities.iter().find(|item| &item.id == id) {
                    content = content.push(text(&community.name).size(28));
                    content = content.push(button("Ver membros").on_press(Message::LoadMembers(community.id.clone())));
                    if session.account.id.as_ref().is_some_and(|id| community.owner_id.as_ref() == Some(id)) {
                        content = content.push(button("Renomear comunidade").on_press(Message::Select(Selection::RenameCommunity(community.id.clone()))));
                        content = content.push(button("Criar canal ou sala de voz").on_press(Message::Select(Selection::CreateChannel(community.id.clone()))));
                        content = content.push(button("Gerir convite").on_press(Message::Select(Selection::InviteManagement(community.id.clone()))));
                        content = content.push(button("Gerir bans").on_press(Message::LoadBans(community.id.clone())));
                    } else if session.account.id.is_some() && community.owner_id.is_some() {
                        if self.leave_confirmation.as_ref() == Some(&community.id) {
                            content = content.push(text("Sair revoga o convite atual. Para regressar precisas de um novo convite.").size(12).color(muted))
                                .push(row![if self.busy { button("A sair...") } else { button("Confirmar saida").on_press(Message::ApplyLeave) }, if self.busy { button("Cancelar") } else { button("Cancelar").on_press(Message::CancelLeave) }].spacing(10));
                        } else {
                            content = content.push(if self.busy { button("Sair da comunidade") } else { button("Sair da comunidade").on_press(Message::ConfirmLeave(community.id.clone())) });
                        }
                    }
                    for channel in &community.channels {
                        let icon = if channel.kind == "voice" { "Voz" } else { "#" };
                        content = content.push(button(text(format!("{icon} {}", channel.name))).on_press(Message::Select(Selection::Channel(community.id.clone(), channel.id.clone()))));
                    }
                    if let Selection::Channel(_, channel_id) = &self.selected {
                        if let Some(channel) = community.channels.iter().find(|item| &item.id == channel_id) {
                            content = content.push(text(format!("{}: ainda n\u{e3}o dispon\u{ed}vel no desktop.", channel.name)).size(14).color(muted));
                            if session.account.id.as_ref().is_some_and(|id| community.owner_id.as_ref() == Some(id)) {
                                content = content.push(button("Renomear canal").on_press(Message::Select(Selection::RenameChannel(community.id.clone(), channel.id.clone()))));
                            }
                        }
                    }
                }
            }
            _ => content = content.push(text(format!("Ol?, {}.", session.account.full_name)).size(28))
                .push(text("Escolhe uma pessoa ou comunidade na barra lateral.").size(14).color(muted)),
        }
        content = content.push(text("Mensagens Matrix estao em teste; chamadas ainda nao estao disponiveis no desktop.").size(13).color(muted))
            .push(if self.loading_directory { button("A atualizar...") } else { button("Atualizar lista").on_press(Message::Refresh) })
            .push(if self.busy { button("A terminar...") } else { button("Terminar esta sess\u{e3}o").on_press(Message::Logout) })
            .push(text(&self.notice).size(12).color(accent));
        let main = container(scrollable(content)).padding(38).width(Length::Fill).height(Length::Fill)
            .style(|_| container::Style::default().background(Color::from_rgb8(31, 30, 37)));
        row![sidebar, main].height(Length::Fill).into()
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expiry_prunes_unpinned_history_but_keeps_pinned_messages() {
        let mut app = Sonar::default();
        app.matrix_history = Some(MatrixHistoryView { peer: "peer".into(), next_cursor: None, pinned_only: false, items: vec![
            MatrixHistoryItem { id:"old".into(), text:Some("expired".into()), reply_to:None, reaction:None, attachment:None, sender:"a".into(), pinned:false, expires_at:Some("2000-01-01T00:00:00Z".into()) },
            MatrixHistoryItem { id:"pinned".into(), text:Some("keep".into()), reply_to:None, reaction:None, attachment:None, sender:"a".into(), pinned:true, expires_at:None },
        ] });
        app.matrix_reply = Some("old".into());
        let _ = app.update(Message::PruneMatrixHistory);
        assert_eq!(app.matrix_history.unwrap().items.len(), 1);
        assert!(app.matrix_reply.is_none());
    }
    #[test]
    fn message_notification_during_busy_operation_triggers_deferred_refresh() {
        let mut app = Sonar::default();
        let _ = app.update(Message::LoginResult(Ok(api::synthetic_session())));
        let peer = "abcdef0123456789abcdef01".to_string();
        app.selected = Selection::Person(peer.clone());
        app.matrix_history = Some(MatrixHistoryView { peer: peer.clone(), items: vec![], next_cursor: None, pinned_only: false });
        app.busy = true;
        let _ = app.update(Message::LiveEvent(app.live_generation, live::Event::MatrixChanged));
        assert!(app.matrix_refresh_pending);
        let _ = app.update(Message::MatrixProgress(app.generation, Ok((None, false))));
        assert!(app.busy);
        assert!(!app.matrix_refresh_pending);
        assert_eq!(app.matrix_loading_peer.as_deref(), Some(peer.as_str()));
    }
    #[test]
    fn contact_device_results_are_scoped_to_current_conversation() {
        let mut app = Sonar::default();
        let _ = app.update(Message::LoginResult(Ok(api::synthetic_session())));
        let generation = app.generation;
        app.selected = Selection::Person("first".into());
        let _ = app.update(Message::ContactDevicesResult(generation, "second".into(), Ok(vec![("OTHER".into(), "key".into())])));
        assert!(app.contact_devices.is_empty());
        let _ = app.update(Message::ContactDevicesResult(generation, "first".into(), Ok(vec![("DEVICE".into(), "key".into())])));
        assert_eq!(app.contact_devices[0].0, "DEVICE");
        let _ = app.update(Message::Select(Selection::Home));
        assert!(app.contact_devices.is_empty());
    }
    #[test]
    fn incoming_calls_are_scoped_to_the_live_session_and_clear_when_ended() {
        let mut app = Sonar::default();
        let _ = app.update(Message::LoginResult(Ok(api::synthetic_session())));
        let generation = app.live_generation;
        let call = live::IncomingCall { from: "abcdef0123456789abcdef01".into(), id: "6bfa8180-b4fb-44f8-8922-6ad700b06b68".into(), sdp: "synthetic".into(), candidates: Vec::new() };
        let _ = app.update(Message::LiveEvent(generation - 1, live::Event::Incoming(call.clone())));
        assert!(app.incoming_call.is_none());
        let _ = app.update(Message::LiveEvent(generation, live::Event::Incoming(call.clone())));
        assert_eq!(app.incoming_call.as_ref(), Some(&call));
        let ice = rtc::peer_connection::transport::RTCIceCandidateInit { candidate: "candidate:synthetic".into(), sdp_mid: Some("0".into()), ..Default::default() };
        let wrong = live::CallCandidate { from: "000000000000000000000000".into(), id: call.id.clone(), candidate: ice.clone() };
        let _ = app.update(Message::LiveEvent(generation, live::Event::Candidate(wrong)));
        assert!(app.incoming_call.as_ref().unwrap().candidates.is_empty());
        let valid = live::CallCandidate { from: call.from.clone(), id: call.id.clone(), candidate: ice };
        for _ in 0..130 { let _ = app.update(Message::LiveEvent(generation, live::Event::Candidate(valid.clone()))); }
        assert_eq!(app.incoming_call.as_ref().unwrap().candidates.len(), 128);
        let _ = app.update(Message::LiveEvent(generation, live::Event::CallEnded("other".into())));
        assert_eq!(app.incoming_call.as_ref().map(|active| &active.id), Some(&call.id));
        let _ = app.update(Message::LiveEvent(generation, live::Event::CallEnded(call.id.clone())));
        assert!(app.incoming_call.is_none());
    }
    #[test]
    fn current_session_revocation_clears_account_state_and_ignores_stale_events() {
        let mut app = Sonar::default();
        let _ = app.update(Message::LoginResult(Ok(api::synthetic_session())));
        let generation = app.live_generation;
        app.passkey_password = "synthetic-secret".into();
        let _ = app.update(Message::LiveEvent(generation - 1, live::Event::Revoked));
        assert!(app.session.is_some());
        let _ = app.update(Message::LiveEvent(generation, live::Event::Revoked));
        assert!(app.session.is_none());
        assert!(app.passkey_password.is_empty());
        assert!(!app.live_connected);
        let _ = app.update(Message::LiveEvent(generation, live::Event::Online(vec!["0123456789abcdef01234567".into()])));
        assert!(app.online_users.is_empty());
    }
    #[test]
    fn logout_discards_previous_account_state() {
        let mut app = Sonar::default();
        app.directory = Some(api::Directory { people: vec![], communities: vec![] });
        app.profile_name = "private name".into();
        app.current_password = "password".into();
        app.passkey_password = "secret".into();
        app.created_invite = Some(api::CreatedInvite { token: "secret-invite".into(), expires_at: "later".into() });
        app.loading_directory = true;
        let old_generation = app.generation;
        let _ = app.update(Message::LogoutResult(Ok(())));
        assert!(app.directory.is_none());
        assert!(app.profile_name.is_empty());
        assert!(app.current_password.is_empty());
        assert!(app.passkey_password.is_empty());
        assert!(app.created_invite.is_none());
        assert!(!app.loading_directory);
        assert!(app.generation > old_generation);
        let _ = app.update(Message::DirectoryResult(old_generation, Ok(api::Directory { people: vec![], communities: vec![] })));
        assert!(app.directory.is_none());
    }
}
