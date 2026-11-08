#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Sonar desktop: a native Rust client (Iced, software rendering, no WebView)
//! that mirrors the web client's design and speaks the same encrypted protocol.
#[allow(dead_code)]
mod api;
#[allow(dead_code)]
mod audio;
mod call;
mod chat;
mod client;
#[allow(dead_code)]
mod crypto;
mod ice_candidate;
mod identity;
#[allow(dead_code)]
mod key_backup;
#[cfg(windows)]
#[allow(dead_code)]
mod key_store;
#[allow(dead_code)]
mod live;
#[allow(dead_code)]
mod matrix_binding;
#[allow(dead_code)]
mod message_content;
#[cfg(windows)]
#[allow(dead_code)]
mod passkey;
#[cfg(windows)]
#[allow(dead_code)]
mod native_file;
mod room;
mod store;
mod theme;
mod ui;
mod wheel;
#[allow(dead_code)]
mod voice_audio;
#[allow(dead_code)]
mod voice_codec;
#[allow(dead_code)]
mod voice_session;
#[allow(dead_code)]
mod voice_transport;

use api::{Community, CreatedInvite, Passkey, Person, Session, Sessions};
use iced::widget::text_editor;
use iced::{Subscription, Task};
use p256::SecretKey;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use zeroize::{Zeroize, Zeroizing};

fn main() -> iced::Result {
    iced::application(App::boot, App::update, ui::view)
        .title("Sonar")
        .theme(|_: &App| iced::Theme::Dark)
        .style(|_, _| iced::theme::Style { background_color: theme::PAGE, text_color: theme::TEXT })
        .font(include_bytes!("../assets/fonts/Poppins-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Poppins-SemiBold.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Poppins-Bold.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Poppins-ExtraBold.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Poppins-BoldItalic.ttf").as_slice())
        .default_font(theme::REGULAR)
        .window(iced::window::Settings {
            size: iced::Size::new(1280.0, 820.0),
            min_size: Some(iced::Size::new(980.0, 620.0)),
            // Development builds can run scripted visual checks off-screen.
            position: if cfg!(debug_assertions) && std::env::var_os("SONAR_DEV_OFFSCREEN").is_some() { iced::window::Position::Specific(iced::Point::new(-4000.0, -4000.0)) } else { iced::window::Position::Default },
            ..Default::default()
        })
        .subscription(App::subscription)
        .run()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen { Boot, Auth, Unlock, Main }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnlockReason { Recovery, Password, Missing }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode { Direct, Community }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel { People, Privacy }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab { Profile, Security, Privacy, Voice }
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dialog { Start, Create, Join, Channel, Invite, Bans, RenameCommunity, RenameChannel, DeleteChannel }
#[derive(Clone, Debug)]
pub struct Confirm { pub action: String, pub member: Option<Person>, pub channel: Option<api::Channel> }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind { Success, Error, Plain }
#[derive(Clone, Debug)]
pub struct Toast { pub id: u64, pub text: String, pub kind: ToastKind, pub until: std::time::Instant }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallStatus { Calling, Connecting, Connected }
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Participant { pub user_id: String, #[serde(default)] pub name: String, pub session_id: String, #[serde(default)] pub muted: bool, #[serde(default)] pub deafened: bool }
#[derive(Clone)]
pub struct RoomState {
    pub spec: Option<room::Spec>,
    pub controls: Option<room::Controls>,
    pub community: String,
    pub channel: String,
    pub name: String,
    pub community_name: String,
    pub participants: Vec<Participant>,
    pub connections: usize,
    pub muted: bool,
    pub deafened: bool,
    pub early: Vec<(String, String, serde_json::Value)>,
}
#[derive(Clone)]
pub struct CallState {
    pub spec: call::Spec,
    pub controls: Option<call::Controls>,
    pub status: CallStatus,
    pub muted: bool,
    pub deafened: bool,
    pub started: Option<std::time::Instant>,
}

/// The active conversation: a direct contact or a community channel.
#[derive(Default)]
pub struct Thread {
    pub messages: Vec<chat::Message>,
    pub cursor: Option<String>,
    pub has_more: bool,
    pub loading: bool,
    pub loading_older: bool,
    pub pins: bool,
    pub deleted: HashSet<String>,
}

#[derive(Default)]
pub struct AuthForm { pub recover: bool, pub recovery_key: String, pub confirm: String, pub signup: bool, pub name: String, pub email: String, pub password: String, pub server: String, pub server_open: bool, pub busy: bool }

pub struct App {
    pub screen: Screen,
    pub settings: store::Settings,
    pub auth: AuthForm,
    pub unlock_reason: UnlockReason,
    pub unlock_password: String,
    pub unlock_confirm: bool,
    pub unlock_reset: bool,
    /// A recovery key just created on this computer, shown once until saved.
    pub new_recovery: Option<String>,
    pub recovery_saved: bool,
    pub pending: Option<Session>,
    pub session: Option<Session>,
    pub key: Option<Arc<SecretKey>>,
    pub trust: store::Trust,
    pub reads: store::ReadPositions,
    pub generation: u64,
    pub now: i64,
    // Live connection.
    pub live_generation: u64,
    pub live_connected: bool,
    pub socket: Option<live::CallHandle>,
    pub online: HashSet<String>,
    // Navigation.
    pub mode: Mode,
    pub people: Vec<Person>,
    pub people_loaded: bool,
    pub query: String,
    pub peer: Option<String>,
    pub communities: Vec<Community>,
    pub community: Option<Community>,
    pub channel: Option<String>,
    pub members_open: bool,
    pub thread: Thread,
    pub unread_direct: HashSet<String>,
    pub unread_channels: HashSet<String>,
    // Composer.
    pub composer: text_editor::Content,
    pub drafts: HashMap<String, String>,
    pub reply_to: Option<String>,
    pub sending: bool,
    pub picker: Option<String>,
    pub confirm_delete: Option<String>,
    // Overlays.
    pub panel: Option<Panel>,
    pub dialog: Option<Dialog>,
    pub dialog_value: String,
    pub dialog_voice: bool,
    pub dialog_channel: Option<String>,
    pub invite: Option<CreatedInvite>,
    pub bans: Option<Vec<Person>>,
    pub confirm: Option<Confirm>,
    pub busy: bool,
    // Account settings.
    pub settings_open: bool,
    pub tab: Tab,
    pub profile_name: String,
    pub passwords: [String; 3],
    pub show_passwords: bool,
    pub sessions: Option<Sessions>,
    pub passkeys: Option<Vec<Passkey>>,
    pub passkey_adding: bool,
    pub passkey_name: String,
    pub passkey_password: String,
    pub passkey_removing: Option<String>,
    /// "recovery", "password" (legacy), "none", or None while loading.
    pub backup_kind: Option<&'static str>,
    pub sync_password: String,
    pub audio_devices: Option<audio::Devices>,
    pub mic_level: Option<u32>,
    pub testing: bool,
    // Calls.
    pub call: Option<CallState>,
    pub incoming: Option<live::IncomingCall>,
    pub early_candidates: Vec<live::CallCandidate>,
    pub room: Option<RoomState>,
    /// Voice room occupancy by "community:channel", from server snapshots.
    pub rooms: HashMap<String, Vec<Participant>>,
    // Toasts.
    pub toasts: Vec<Toast>,
    pub toast_id: u64,
    /// Development builds: scripted UI steps for automated visual checks.
    pub script: std::collections::VecDeque<String>,
    pub probe: Option<(std::time::Instant, Vec<f32>)>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Tick,
    // Authentication.
    Restored(Result<Session, String>),
    AuthField(u8, String),
    ToggleSignup,
    ToggleServer,
    Submit,
    #[cfg(windows)]
    PasskeySignIn,
    Authenticated(Result<Session, String>),
    Identity(u64, Result<(api::Account, Arc<SecretKey>, Option<String>), String>, Option<UnlockReason>),
    ToggleRecover,
    Recover,
    Recovered(u64, Result<(Session, api::Account, Arc<SecretKey>, Option<String>), String>),
    ToggleUnlockReset,
    RecoverySaved(bool),
    CopyRecovery,
    #[cfg(windows)]
    SaveRecoveryFile,
    DismissRecovery,
    RecoveryMade(Result<String, String>),
    UnlockPassword(String),
    Unlock,
    SignOut,
    SignedOut,
    // Live events.
    Live(u64, live::Event),
    Reconnect,
    // Directory.
    Refresh,
    People(u64, Result<Vec<Person>, String>),
    Communities(u64, Result<Vec<Community>, String>),
    Unread(Result<Vec<(String, (i64, String))>, String>),
    Query(String),
    SetMode(Mode),
    OpenPeer(Option<String>),
    OpenPanel(Option<Panel>),
    AcceptKey,
    // Threads.
    Page(u64, Result<(Vec<chat::Message>, Option<String>, bool), String>),
    LoadOlder,
    TogglePins,
    Edit(text_editor::Action),
    Send,
    Sent(u64, Result<chat::Message, String>),
    React(String, String, bool),
    Picker(Option<String>),
    Reply(Option<String>),
    Pin(String),
    Pinned(u64, Result<chat::Message, String>),
    AskDelete(Option<String>),
    Delete(String),
    Deleted(u64, String, Result<(), String>),
    // Communities.
    OpenCommunity(Option<String>),
    CommunityLoaded(u64, Result<Community, String>),
    OpenChannel(String),
    ToggleMembers,
    OpenDialog(Option<Dialog>),
    DialogValue(String),
    DialogVoice(bool),
    NewChannel(bool),
    DialogChannel(String),
    SubmitDialog,
    DialogDone(Result<Option<String>, String>),
    InviteReady(Result<CreatedInvite, String>),
    CopyInvite,
    BansLoaded(Result<Vec<Person>, String>),
    AskConfirm(Option<Confirm>),
    ApplyConfirm,
    Confirmed(String, Result<(), String>),
    // Settings.
    OpenSettings(bool),
    SetTab(Tab),
    ProfileName(String),
    SaveProfile,
    ProfileSaved(Result<api::Account, String>),
    PasswordField(usize, String),
    ShowPasswords(bool),
    ChangePassword,
    PasswordChanged(Result<(), String>),
    SessionsLoaded(Result<Sessions, String>),
    EndSession(String),
    SessionEnded(Result<bool, String>),
    PasskeysLoaded(Result<Vec<Passkey>, String>),
    PasskeyAdding(bool),
    PasskeyName(String),
    PasskeyPassword(String),
    #[cfg(windows)]
    AddPasskey,
    PasskeyAdded(Result<Passkey, String>),
    PasskeyRemove(Option<String>),
    RemovePasskey,
    PasskeyRemoved(Result<(), String>),
    BackupStatus(Result<&'static str, String>),
    SyncPassword(String),
    EnableSync,
    Synced(Result<(api::Account, Arc<SecretKey>, Option<String>), String>),
    CopyFingerprint,
    AudioLoaded(Result<audio::Devices, String>),
    SelectInput(String),
    SelectOutput(String),
    RelayOnly(bool),
    TestMic,
    MicResult(Result<u32, String>),
    TestSpeakers,
    SpeakersResult(Result<(), String>),
    // Calls.
    StartCall,
    AcceptCall,
    DeclineCall,
    CallUpdate(String, call::Update),
    Mute,
    Deafen,
    Hangup,
    JoinRoom(String),
    RoomJoined(Result<serde_json::Value, String>),
    RoomUpdate(String, room::Update),
    RoomMute,
    RoomDeafen,
    LeaveRoom,
    RoomLock(String, bool),
    RoomKick(String, Participant),
    RoomsLoaded(Result<serde_json::Value, String>),
    Done(Result<(), String>, &'static str),
    // Misc.
    Script,
    ProbeFrame(std::time::Instant),
    Screenshot(String, iced::window::Screenshot),
    DismissToast(u64),
    Noop,
}

impl App {
    fn boot() -> (Self, Task<Msg>) {
        let settings = store::load_settings();
        let server = if settings.server.is_empty() { "http://127.0.0.1:5000".to_string() } else { settings.server.clone() };
        let saved = store::load_session();
        let app = App {
            screen: if saved.is_some() { Screen::Boot } else { Screen::Auth },
            auth: AuthForm { server, ..Default::default() },
            settings,
            unlock_reason: UnlockReason::Recovery, unlock_password: String::new(), unlock_confirm: false, unlock_reset: false, new_recovery: None, recovery_saved: false,
            pending: None, session: None, key: None, trust: store::Trust::default(), reads: store::ReadPositions::default(), generation: 0, now: chat::now(),
            live_generation: 0, live_connected: false, socket: None, online: HashSet::new(),
            mode: Mode::Direct, people: Vec::new(), people_loaded: false, query: String::new(), peer: None,
            communities: Vec::new(), community: None, channel: None, members_open: false, thread: Thread::default(),
            unread_direct: HashSet::new(), unread_channels: HashSet::new(),
            composer: text_editor::Content::new(), drafts: HashMap::new(), reply_to: None, sending: false, picker: None, confirm_delete: None,
            panel: None, dialog: None, dialog_value: String::new(), dialog_voice: false, dialog_channel: None,
            invite: None, bans: None, confirm: None, busy: false,
            settings_open: false, tab: Tab::Profile, profile_name: String::new(), passwords: Default::default(), show_passwords: false,
            sessions: None, passkeys: None, passkey_adding: false, passkey_name: String::new(), passkey_password: String::new(), passkey_removing: None,
            backup_kind: None, sync_password: String::new(), audio_devices: None, mic_level: None, testing: false,
            call: None, incoming: None, early_candidates: Vec::new(), room: None, rooms: HashMap::new(), toasts: Vec::new(), toast_id: 0, script: Default::default(), probe: None,
        };
        #[cfg_attr(not(debug_assertions), allow(unused_mut))]
        let mut app = app;
        // Development builds only: sign in from the environment for automated UI checks.
        #[cfg(debug_assertions)]
        if let Ok(script) = std::env::var("SONAR_DEV_SCRIPT") {
            app.screen = Screen::Auth;
            if let Ok(server) = std::env::var("SONAR_DEV_SERVER") { app.auth.server = server; }
            app.script = script.split(';').filter(|s| !s.is_empty()).map(str::to_owned).collect();
            let mut tasks = vec![Task::done(Msg::Script)];
            if let (Ok(email), Ok(password)) = (std::env::var("SONAR_DEV_EMAIL"), std::env::var("SONAR_DEV_PASSWORD")) {
                app.auth.email = email;
                app.auth.password = password;
                if let Ok(name) = std::env::var("SONAR_DEV_NAME") { app.auth.signup = true; app.auth.name = name; }
                tasks.push(Task::done(Msg::Submit));
            } else if let Some((server, cookie)) = saved {
                app.screen = Screen::Boot;
                tasks.push(Task::perform(client::restore(server, cookie), Msg::Restored));
            }
            return (app, Task::batch(tasks));
        }
        let task = match saved {
            Some((server, cookie)) => Task::perform(client::restore(server, cookie), Msg::Restored),
            None => Task::none(),
        };
        (app, task)
    }

    fn subscription(&self) -> Subscription<Msg> {
        let mut list = vec![iced::time::every(std::time::Duration::from_millis(1000)).map(|_| Msg::Tick)];
        if let (Some(session), Screen::Main) = (&self.session, self.screen) {
            list.push(Subscription::run_with(session.live_auth(self.live_generation), live::events).map(|(generation, event)| Msg::Live(generation, event)));
            list.push(iced::time::every(std::time::Duration::from_secs(30)).map(|_| Msg::Refresh));
        }
        if self.probe.is_some() { list.push(iced::window::frames().map(Msg::ProbeFrame)); }
        if let Some(spec) = self.room.as_ref().and_then(|r| r.spec.clone()) {
            let id = spec.session_id.clone();
            list.push(Subscription::run_with(spec, room::run).with(id).map(|(id, update)| Msg::RoomUpdate(id, update)));
        }
        if let Some(call) = &self.call {
            list.push(Subscription::run_with(call.spec.clone(), call::run).with(call.spec.id.clone()).map(|(id, update)| Msg::CallUpdate(id, update)));
        }
        Subscription::batch(list)
    }

    pub fn me(&self) -> &str { self.session.as_ref().map(|s| s.me()).unwrap_or_default() }
    pub fn person(&self, id: &str) -> Option<&Person> { self.people.iter().find(|p| p.id == id) }
    pub fn selected_peer(&self) -> Option<&Person> { self.peer.as_deref().and_then(|id| self.person(id)) }
    pub fn channel_info(&self) -> Option<&api::Channel> { self.community.as_ref()?.channels.iter().find(|c| Some(&c.id) == self.channel.as_ref()) }
    pub fn member(&self, id: &str) -> Option<&Person> {
        let community = self.community.as_ref()?;
        community.member_details.iter().chain(community.history_authors.iter()).find(|p| p.id == id)
    }
    pub fn trust_changed(&self, peer: &Person) -> bool {
        matches!((self.trust.saved(&peer.id), &peer.encryption_key), (Some(saved), Some(key)) if saved != key)
    }
    pub fn owner(&self) -> bool { self.community.as_ref().is_some_and(|c| c.owner_id.as_deref() == Some(self.me())) }
    pub fn moderator(&self) -> bool { self.community.as_ref().is_some_and(|c| c.moderators.iter().any(|m| m == self.me())) }
    fn draft_key(&self) -> Option<String> {
        match self.mode {
            Mode::Direct => self.peer.as_ref().map(|p| format!("dm:{p}")),
            Mode::Community => Some(format!("channel:{}:{}", self.community.as_ref()?.id, self.channel.as_ref()?)),
        }
    }

    fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toast_id += 1;
        let text = text.into();
        self.toasts.retain(|toast| toast.text != text);
        self.toasts.push(Toast { id: self.toast_id, text, kind, until: std::time::Instant::now() + std::time::Duration::from_secs(if kind == ToastKind::Error { 5 } else { 3 }) });
        if self.toasts.len() > 3 { self.toasts.remove(0); }
    }
    fn fail(&mut self, error: String) { self.toast(ToastKind::Error, error); }

    fn save_draft(&mut self) {
        if let Some(key) = self.draft_key() {
            let text = self.composer.text();
            let text = text.trim_end_matches('\n');
            if text.is_empty() { self.drafts.remove(&key); } else if self.drafts.len() < 100 || self.drafts.contains_key(&key) { self.drafts.insert(key, text.to_owned()); }
        }
    }
    fn restore_draft(&mut self) {
        let text = self.draft_key().and_then(|key| self.drafts.get(&key).cloned()).unwrap_or_default();
        self.composer = text_editor::Content::with_text(&text);
    }

    fn clear_session(&mut self) {
        self.generation += 1;
        self.live_generation += 1;
        self.session = None; self.pending = None; self.key = None; self.trust = store::Trust::default(); self.reads = store::ReadPositions::default();
        self.live_connected = false; self.socket = None; self.online.clear();
        self.people.clear(); self.people_loaded = false; self.peer = None; self.communities.clear(); self.community = None; self.channel = None;
        self.thread = Thread::default(); self.unread_direct.clear(); self.unread_channels.clear();
        self.composer = text_editor::Content::new(); self.drafts.clear(); self.reply_to = None; self.sending = false; self.picker = None; self.confirm_delete = None;
        self.panel = None; self.dialog = None; self.confirm = None; self.invite = None; self.bans = None; self.busy = false;
        self.settings_open = false; self.sessions = None; self.passkeys = None; self.backup_kind = None; self.new_recovery = None; self.recovery_saved = false; self.unlock_reset = false;
        self.passwords.iter_mut().for_each(Zeroize::zeroize); self.sync_password.zeroize(); self.passkey_password.zeroize(); self.unlock_password.zeroize(); self.auth.password.zeroize();
        self.call = None; self.incoming = None; self.early_candidates.clear(); self.room = None; self.rooms.clear();
        self.mode = Mode::Direct; self.screen = Screen::Auth; self.auth.busy = false;
    }

    /// After authentication: make sure the key is on this device.
    fn pair(&mut self, session: Session, password: Option<Zeroizing<String>>, recovery: Option<Zeroizing<String>>) -> Task<Msg> {
        self.generation += 1;
        let generation = self.generation;
        self.pending = Some(session.clone());
        Task::perform(async move {
            match identity::ensure(session, password, recovery).await {
                Ok(paired) => (Ok((paired.account, Arc::new(paired.key), paired.recovery.map(|r| r.to_string()))), None),
                Err(identity::Problem::Locked(identity::Needs::Recovery)) => (Err(String::new()), Some(UnlockReason::Recovery)),
                Err(identity::Problem::Locked(identity::Needs::Password)) => (Err(String::new()), Some(UnlockReason::Password)),
                Err(identity::Problem::Missing) => (Err(String::new()), Some(UnlockReason::Missing)),
                Err(identity::Problem::Failed(error)) => (Err(error), None),
            }
        }, move |(result, reason)| Msg::Identity(generation, result, reason))
    }

    fn enter(&mut self, account: api::Account, key: Arc<SecretKey>, recovery: Option<String>) -> Task<Msg> {
        let Some(mut session) = self.pending.take() else { return Task::none() };
        if recovery.is_some() { self.new_recovery = recovery; self.recovery_saved = false; }
        session.account = account;
        let server = session.base.as_str().trim_end_matches('/').to_owned();
        store::save_session(&server, &session.cookie);
        self.settings.server = server;
        store::save_settings(&self.settings);
        self.trust = store::Trust::open(&session.origin(), session.me());
        self.reads = store::ReadPositions::open(&session.origin(), session.me());
        self.profile_name = session.account.full_name.clone();
        self.session = Some(session);
        self.key = Some(key);
        self.screen = Screen::Main;
        self.unlock_password.zeroize(); self.auth.password.zeroize(); self.auth.busy = false; self.unlock_confirm = false; self.unlock_reset = false;
        self.live_generation += 1;
        self.refresh()
    }

    fn refresh(&mut self) -> Task<Msg> {
        let Some(session) = self.session.clone() else { return Task::none() };
        let generation = self.generation;
        let other = session.clone();
        Task::batch([
            Task::perform(async move { session.people().await }, move |result| Msg::People(generation, result)),
            Task::perform(async move { other.communities().await }, move |result| Msg::Communities(generation, result)),
        ])
    }

    fn load_thread(&mut self, before: Option<String>) -> Task<Msg> {
        let (Some(session), Some(key)) = (self.session.clone(), self.key.clone()) else { return Task::none() };
        let generation = self.generation;
        let pins = self.thread.pins;
        if before.is_some() { self.thread.loading_older = true; } else { self.thread.loading = true; }
        match self.mode {
            Mode::Direct => {
                let Some(peer) = self.selected_peer().cloned() else { return Task::none() };
                let trusted = !self.trust_changed(&peer);
                if trusted { if let Some(k) = &peer.encryption_key { self.trust.remember(&peer.id, k); } }
                Task::perform(async move {
                    let page = session.direct_page(&peer.id, before.as_deref(), pins).await?;
                    let messages = page.messages.iter().map(|m| chat::open(&key, peer.encryption_key.as_deref(), m, trusted)).collect();
                    Ok((messages, page.next_cursor, page.has_more))
                }, move |result| Msg::Page(generation, result))
            }
            Mode::Community => {
                let (Some(community), Some(channel)) = (self.community.clone(), self.channel.clone()) else { return Task::none() };
                let senders: HashMap<String, Option<String>> = community.member_details.iter().chain(community.history_authors.iter()).map(|p| (p.id.clone(), p.encryption_key.clone())).collect();
                Task::perform(async move {
                    let page = session.channel_page(&community.id, &channel, before.as_deref(), pins).await?;
                    let messages = page.messages.iter().map(|m| chat::open(&key, senders.get(&m.sender_id).cloned().flatten().as_deref(), m, true)).collect();
                    Ok((messages, page.next_cursor, page.has_more))
                }, move |result| Msg::Page(generation, result))
            }
        }
    }

    fn reset_thread(&mut self) {
        self.generation += 1;
        self.thread = Thread::default();
        self.reply_to = None; self.picker = None; self.confirm_delete = None;
    }

    fn open_peer(&mut self, peer: Option<String>) -> Task<Msg> {
        self.save_draft();
        self.reset_thread();
        if let Some(id) = &peer { self.unread_direct.remove(id); }
        self.peer = peer;
        self.restore_draft();
        if self.peer.is_some() { self.load_thread(None) } else { Task::none() }
    }

    fn open_community(&mut self, id: Option<String>) -> Task<Msg> {
        self.save_draft();
        self.reset_thread();
        self.community = None; self.channel = None; self.members_open = false;
        self.restore_draft();
        let (Some(id), Some(session)) = (id, self.session.clone()) else { return Task::none() };
        let generation = self.generation;
        self.thread.loading = true;
        Task::perform(async move { session.community(&id).await }, move |result| Msg::CommunityLoaded(generation, result))
    }

    fn refresh_community(&mut self) -> Task<Msg> {
        let (Some(session), Some(community)) = (self.session.clone(), self.community.as_ref()) else { return Task::none() };
        let id = community.id.clone();
        let generation = self.generation;
        Task::perform(async move { session.community(&id).await }, move |result| Msg::CommunityLoaded(generation, result))
    }

    /// The read-position key of the open conversation: a contact id, or
    /// "community:channel" for a channel.
    fn active_key(&self) -> Option<String> {
        match self.mode {
            Mode::Direct => self.peer.clone(),
            Mode::Community => Some(format!("{}:{}", self.community.as_ref()?.id, self.channel.as_ref()?)),
        }
    }
    /// Remembers the newest message from others in the open conversation.
    fn mark_read(&mut self) {
        if self.thread.pins { return; }
        let Some(key) = self.active_key() else { return };
        let me = self.me().to_owned();
        let latest = self.thread.messages.iter()
            .filter(|m| !m.unreadable && m.sender != me && (self.mode == Mode::Community || self.peer.as_deref() == Some(m.sender.as_str())))
            .map(|m| (m.created, m.id.clone())).max();
        if let Some(position) = latest { self.reads.remember(&key, position); }
    }

    fn visible(&self, id: &str) -> bool { !self.thread.deleted.contains(id) }

    fn receive_direct(&mut self, value: serde_json::Value, updated: bool) {
        let Ok(envelope) = serde_json::from_value::<api::MessageEnvelope>(value) else { return };
        let me = self.me().to_owned();
        let peer_id = if envelope.sender_id == me { envelope.receiver_id.clone() } else { envelope.sender_id.clone() };
        let active = self.mode == Mode::Direct && self.peer.as_deref() == Some(&peer_id);
        if !updated && envelope.sender_id != me && !active { self.unread_direct.insert(peer_id.clone()); }
        if !active || !self.visible(&envelope.id) { return; }
        let (Some(key), Some(peer)) = (self.key.clone(), self.person(&peer_id).cloned()) else { return };
        let message = chat::open(&key, peer.encryption_key.as_deref(), &envelope, !self.trust_changed(&peer));
        if self.thread.pins && !message.pinned { self.thread.messages.retain(|m| m.id != message.id); return; }
        chat::merge(&mut self.thread.messages, [message]);
        self.mark_read();
    }

    fn receive_channel(&mut self, value: serde_json::Value, new: bool) -> Task<Msg> {
        let Ok(envelope) = serde_json::from_value::<api::MessageEnvelope>(value) else { return Task::none() };
        let (Some(community_id), Some(channel_id)) = (envelope.community_id.clone(), envelope.channel_id.clone()) else { return Task::none() };
        let active = self.mode == Mode::Community && self.community.as_ref().is_some_and(|c| c.id == community_id) && self.channel.as_deref() == Some(&channel_id);
        if new && envelope.sender_id != self.me() && !active { self.unread_channels.insert(format!("{community_id}:{channel_id}")); }
        if !active || !self.visible(&envelope.id) { return Task::none(); }
        let Some(key) = self.key.clone() else { return Task::none() };
        let sender_key = self.member(&envelope.sender_id).and_then(|p| p.encryption_key.clone());
        let unknown = self.member(&envelope.sender_id).is_none();
        let message = chat::open(&key, sender_key.as_deref(), &envelope, true);
        if self.thread.pins && !message.pinned { self.thread.messages.retain(|m| m.id != message.id); }
        else { chat::merge(&mut self.thread.messages, [message]); }
        self.mark_read();
        if unknown { self.refresh_community() } else { Task::none() }
    }

    fn update(&mut self, message: Msg) -> Task<Msg> {
        match message {
            Msg::Tick => {
                self.now = chat::now();
                let now = std::time::Instant::now();
                self.toasts.retain(|toast| toast.until > now);
                let current = self.now;
                self.thread.messages.retain(|m| m.live(current));
            }
            Msg::DismissToast(id) => self.toasts.retain(|toast| toast.id != id),
            Msg::Script => return self.script_step(),
            Msg::ProbeFrame(at) => {
                let Some((last, frames)) = self.probe.as_mut() else { return Task::none() };
                frames.push(at.duration_since(*last).as_secs_f32() * 1000.0);
                *last = at;
                if frames.len() >= 240 {
                    let mut sorted = std::mem::take(frames);
                    self.probe = None;
                    sorted.sort_by(f32::total_cmp);
                    let started = std::time::Instant::now();
                    drop(ui::view(self));
                    println!("PROBE avg {:.1} ms, p50 {:.1}, p95 {:.1}, max {:.1}; view build {:.2} ms", sorted.iter().sum::<f32>() / sorted.len() as f32, sorted[120], sorted[228], sorted[239], started.elapsed().as_secs_f32() * 1000.0);
                    return Task::done(Msg::Script);
                }
                let down = (frames.len() / 60) % 2 == 0;
                return iced::widget::operation::scroll_by("thread", iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: if down { -40.0 } else { 40.0 } });
            }
            Msg::Screenshot(path, shot) => {
                let file = std::fs::File::create(&path).map(std::io::BufWriter::new);
                if let Ok(file) = file {
                    let mut encoder = png::Encoder::new(file, shot.size.width, shot.size.height);
                    encoder.set_color(png::ColorType::Rgba);
                    encoder.set_depth(png::BitDepth::Eight);
                    if let Ok(mut writer) = encoder.write_header() { let _ = writer.write_image_data(&shot.rgba); }
                }
                return Task::done(Msg::Script);
            }
            Msg::Noop => {}

            // ----- Authentication -------------------------------------------------
            Msg::Restored(result) => match result {
                Ok(session) => return self.pair(session, None, None),
                Err(_) => { store::forget_session(); self.screen = Screen::Auth; }
            },
            Msg::AuthField(field, value) => match field {
                0 => self.auth.name = value,
                1 => self.auth.email = value,
                2 => self.auth.password = value,
                4 => self.auth.recovery_key = value,
                5 => self.auth.confirm = value,
                _ => self.auth.server = value,
            },
            Msg::ToggleSignup => { if self.auth.recover { self.auth.recover = false; } else { self.auth.signup = !self.auth.signup; } self.auth.password.zeroize(); self.auth.confirm.zeroize(); self.auth.recovery_key.zeroize(); }
            Msg::ToggleRecover => { self.auth.recover = !self.auth.recover; self.auth.signup = false; self.auth.password.zeroize(); self.auth.confirm.zeroize(); self.auth.recovery_key.zeroize(); }
            Msg::Recover if !self.auth.busy => {
                if self.auth.email.trim().is_empty() || self.auth.recovery_key.trim().is_empty() || self.auth.password.is_empty() { self.fail("All fields are required".into()); return Task::none(); }
                if self.auth.password != self.auth.confirm { self.fail("Your new passwords do not match".into()); return Task::none(); }
                self.auth.busy = true;
                self.generation += 1;
                let generation = self.generation;
                let (server, email) = (self.auth.server.clone(), self.auth.email.clone());
                let (recovery, password) = (Zeroizing::new(std::mem::take(&mut self.auth.recovery_key)), Zeroizing::new(self.auth.password.clone()));
                return Task::perform(identity::recover(server, email, recovery, password), move |result| Msg::Recovered(generation, result.map(|(session, paired)| (session, paired.account, Arc::new(paired.key), paired.recovery.map(|r| r.to_string())))));
            }
            Msg::Recovered(generation, result) if generation == self.generation => {
                self.auth.busy = false;
                match result {
                    Ok((session, account, key, recovery)) => {
                        self.pending = Some(session);
                        self.auth.recover = false; self.auth.confirm.zeroize();
                        self.toast(ToastKind::Success, "Password reset. You are signed in.");
                        return self.enter(account, key, recovery);
                    }
                    Err(error) => self.fail(error),
                }
            }
            Msg::ToggleServer => self.auth.server_open = !self.auth.server_open,
            Msg::Submit if !self.auth.busy => {
                if self.auth.email.trim().is_empty() || self.auth.password.is_empty() || (self.auth.signup && self.auth.name.trim().is_empty()) {
                    self.fail("All fields are required".into());
                    return Task::none();
                }
                self.auth.busy = true;
                let password = Zeroizing::new(self.auth.password.clone());
                let (server, email, name) = (self.auth.server.clone(), self.auth.email.clone(), self.auth.name.clone());
                let signup = self.auth.signup;
                // Kept only until the key is unlocked or created, then wiped.
                self.unlock_password = (*password).clone();
                return Task::perform(async move {
                    if signup { client::signup(server, name, email, password).await } else { client::login(server, email, password).await }
                }, Msg::Authenticated);
            }
            #[cfg(windows)]
            Msg::PasskeySignIn if !self.auth.busy => {
                let window = passkey::active_window();
                if window == 0 { self.fail("Bring Sonar to the front and try again.".into()); return Task::none(); }
                self.auth.busy = true;
                self.unlock_password.zeroize();
                return Task::perform(api::passkey_login(self.auth.server.clone(), window), Msg::Authenticated);
            }
            Msg::Authenticated(result) => match result {
                Ok(session) => {
                    let password = (!self.unlock_password.is_empty()).then(|| Zeroizing::new(std::mem::take(&mut self.unlock_password)));
                    self.toast(ToastKind::Success, if self.auth.signup { "Account created successfully" } else if password.is_some() { "Logged in successfully" } else { "Signed in with your passkey" });
                    return self.pair(session, password, None);
                }
                Err(error) => { self.auth.busy = false; self.unlock_password.zeroize(); self.fail(error); }
            },
            Msg::Identity(generation, result, reason) if generation == self.generation => {
                self.auth.busy = false;
                self.busy = false;
                match (result, reason) {
                    (Ok((account, key, recovery)), _) => {
                        let welcome = self.screen == Screen::Unlock;
                        let task = self.enter(account, key, recovery);
                        if welcome { self.toast(ToastKind::Success, "Your messages are unlocked on this device"); }
                        return task;
                    }
                    (Err(_), Some(reason)) => { self.unlock_reason = reason; self.unlock_confirm = false; self.unlock_reset = reason == UnlockReason::Missing; self.screen = Screen::Unlock; }
                    (Err(error), None) => {
                        if self.screen == Screen::Boot { self.screen = Screen::Auth; }
                        self.fail(error);
                    }
                }
            }
            Msg::UnlockPassword(value) => self.unlock_password = value,
            Msg::Unlock if !self.busy && !self.unlock_password.is_empty() => {
                let Some(session) = self.pending.clone() else { return Task::none() };
                let secret = Zeroizing::new(std::mem::take(&mut self.unlock_password));
                if self.unlock_reset {
                    if !self.unlock_confirm { self.unlock_confirm = true; self.unlock_password = secret.to_string(); return Task::none(); }
                    self.busy = true;
                    self.generation += 1;
                    let generation = self.generation;
                    return Task::perform(identity::rotate(session, secret), move |result| Msg::Identity(generation, result.map(|p| (p.account, Arc::new(p.key), p.recovery.map(|r| r.to_string()))), None));
                }
                self.busy = true;
                return match self.unlock_reason {
                    UnlockReason::Recovery => self.pair(session, None, Some(secret)),
                    _ => self.pair(session, Some(secret), None),
                };
            }
            Msg::ToggleUnlockReset => { if !self.busy { self.unlock_reset = !self.unlock_reset; self.unlock_confirm = false; self.unlock_password.zeroize(); } }
            Msg::RecoverySaved(value) => self.recovery_saved = value,
            Msg::CopyRecovery => if let Some(key) = &self.new_recovery { let key = key.clone(); self.toast(ToastKind::Success, "Recovery key copied"); return iced::clipboard::write(key); },
            #[cfg(windows)]
            Msg::SaveRecoveryFile => if let Some(key) = self.new_recovery.clone() {
                return Task::perform(async move {
                    let path = tokio::task::spawn_blocking(|| native_file::save("sonar-recovery-key.txt")).await.map_err(|_| "Could not open the save dialog".to_string())??;
                    let Some(path) = path else { return Ok(false) };
                    std::fs::write(path, format!("Sonar recovery key\r\n\r\n{key}\r\n\r\nUse it to open your messages on a new device and to reset a forgotten password.\r\nKeep it private. Anyone with this key and your email can reset your password.\r\n")).map_err(|_| "Could not save the file".to_string())?;
                    Ok(true)
                }, |result: Result<bool, String>| match result { Ok(true) => Msg::RecoverySaved(true), Ok(false) => Msg::Noop, Err(error) => Msg::RecoveryMade(Err(error)) });
            },
            Msg::DismissRecovery if self.recovery_saved => { if let Some(mut key) = self.new_recovery.take() { key.zeroize(); } self.recovery_saved = false; }
            Msg::RecoveryMade(result) => {
                self.busy = false;
                match result {
                    Ok(key) => { self.new_recovery = Some(key); self.recovery_saved = false; self.backup_kind = Some("recovery"); }
                    Err(error) => self.fail(error),
                }
            }
            Msg::SignOut => {
                let session = self.session.clone().or_else(|| self.pending.clone());
                store::forget_session();
                self.clear_session();
                self.toast(ToastKind::Success, "Logged out successfully");
                if let Some(session) = session { return Task::perform(async move { let _ = session.sign_out_everywhere().await; }, |_| Msg::SignedOut); }
            }
            Msg::SignedOut => {}

            // ----- Live events -----------------------------------------------------
            Msg::Live(generation, event) if generation == self.live_generation && self.session.is_some() => match event {
                live::Event::Connected(handle) => { self.live_connected = true; self.socket = Some(handle); }
                live::Event::Disconnected => {
                    self.live_connected = false; self.socket = None;
                    if self.call.is_some() { self.call = None; self.fail("Connection lost. The call ended.".into()); }
                    if self.room.is_some() { self.room = None; self.fail("Connection lost. You left the voice room.".into()); }
                    self.incoming = None; self.rooms.clear();
                    // Reconnect shortly; the subscription restarts with a new generation.
                    return Task::perform(tokio::time::sleep(std::time::Duration::from_secs(3)), |_| Msg::Reconnect);
                }
                live::Event::Revoked => { store::forget_session(); self.clear_session(); self.fail("You were signed out.".into()); }
                live::Event::Online(ids) => self.online = ids.into_iter().collect(),
                live::Event::Incoming(call) => {
                    if self.call.is_some() || self.incoming.is_some() { return Task::none(); }
                    let mut call = call;
                    call.candidates.extend(self.early_candidates.drain(..).filter(|c| c.id == call.id).map(|c| c.candidate));
                    self.incoming = Some(call);
                }
                live::Event::Answer(answer) => {
                    if let Some(state) = self.call.as_ref().filter(|c| c.spec.id == answer.id && c.spec.peer == answer.from) {
                        if let Some(controls) = &state.controls { controls.send(call::Command::Answer(answer.sdp)); }
                    }
                }
                live::Event::Candidate(candidate) => {
                    if let Some(state) = self.call.as_ref().filter(|c| c.spec.id == candidate.id && c.spec.peer == candidate.from) {
                        if let Some(controls) = &state.controls { controls.send(call::Command::Candidate(candidate.candidate)); }
                    } else if let Some(incoming) = self.incoming.as_mut().filter(|c| c.id == candidate.id && c.from == candidate.from) {
                        if incoming.candidates.len() < 128 { incoming.candidates.push(candidate.candidate); }
                    } else if self.early_candidates.len() < 64 { self.early_candidates.push(candidate); }
                }
                live::Event::CallEnded(id) => {
                    if self.incoming.as_ref().is_some_and(|c| c.id == id) { self.incoming = None; self.toast(ToastKind::Plain, "Call ended"); }
                    if self.call.as_ref().is_some_and(|c| c.spec.id == id) { self.call = None; self.toast(ToastKind::Plain, "Call ended"); }
                }
                live::Event::MatrixChanged => {}
                live::Event::Data(name, value) => match name {
                    "newMessage" => self.receive_direct(value, false),
                    "messageUpdated" => self.receive_direct(value, true),
                    "messageDeleted" | "channelMessageDeleted" => {
                        if let Some(id) = value.get("id").and_then(|v| v.as_str()) {
                            self.thread.deleted.insert(id.to_owned());
                            self.thread.messages.retain(|m| m.id != id);
                        }
                    }
                    "voice:room-state" => {
                        let (Some(community), Some(channel)) = (value.get("communityId").and_then(|v| v.as_str()), value.get("channelId").and_then(|v| v.as_str())) else { return Task::none() };
                        let participants: Vec<Participant> = value.get("participants").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
                        if let Some(room) = self.room.as_mut().filter(|r| r.community == community && r.channel == channel) {
                            room.participants = participants.clone();
                            if let Some(controls) = &room.controls { controls.send(room::Command::Participants(participants.iter().map(|p| (p.user_id.clone(), p.session_id.clone())).collect())); }
                        }
                        self.rooms.insert(format!("{community}:{channel}"), participants);
                    }
                    "voice:room-signal" => {
                        let Some(room) = self.room.as_mut() else { return Task::none() };
                        let field = |name: &str| value.get(name).and_then(|v| v.as_str()).unwrap_or_default().to_owned();
                        if field("communityId") != room.community || field("channelId") != room.channel { return Task::none(); }
                        let (from, session, signal) = (field("from"), field("sessionId"), value.get("signal").cloned().unwrap_or_default());
                        match (&room.spec, &room.controls) {
                            (Some(spec), Some(controls)) if field("toSession") == spec.session_id => controls.send(room::Command::Signal { from, session, signal }),
                            (Some(spec), _) if field("toSession") != spec.session_id => {}
                            _ if room.early.len() < 128 => room.early.push((from, session, signal)),
                            _ => {}
                        }
                    }
                    "voice:room-left" => {
                        let field = |name: &str| value.get(name).and_then(|v| v.as_str()).unwrap_or_default().to_owned();
                        if self.room.as_ref().is_some_and(|r| r.community == field("communityId") && r.channel == field("channelId") && r.spec.as_ref().is_none_or(|s| s.session_id == field("sessionId"))) {
                            if let Some(c) = self.room.take().and_then(|r| r.controls) { c.send(room::Command::Leave); }
                            match field("reason").as_str() {
                                "removed" => self.toast(ToastKind::Plain, "You no longer have access to that voice room."),
                                "channelDeleted" => self.toast(ToastKind::Plain, "This voice room was deleted."),
                                "kicked" => self.toast(ToastKind::Plain, "A moderator disconnected you from the voice room."),
                                _ => {}
                            }
                        }
                    }
                    "channelMessageNew" => return self.receive_channel(value, true),
                    "channelMessage" => return self.receive_channel(value, false),
                    "communityUpdated" => {
                        let changed = value.get("communityId").and_then(|v| v.as_str()).map(str::to_owned);
                        let task = self.refresh();
                        if changed.is_some() && self.community.as_ref().map(|c| &c.id) == changed.as_ref() { return Task::batch([task, self.refresh_community()]); }
                        return task;
                    }
                    "communityRemoved" => {
                        if let Some(id) = value.get("communityId").and_then(|v| v.as_str()).map(str::to_owned) {
                            self.communities.retain(|c| c.id != id);
                            self.unread_channels.retain(|key| !key.starts_with(&format!("{id}:")));
                            self.drafts.retain(|key, _| !key.starts_with(&format!("channel:{id}:")));
                            if self.community.as_ref().is_some_and(|c| c.id == id) { self.toast(ToastKind::Plain, "You no longer have access to that community"); return self.open_community(None); }
                        }
                    }
                    _ => {}
                },
            },
            Msg::Reconnect => { if self.session.is_some() && !self.live_connected { self.live_generation += 1; } }

            // ----- Directory -------------------------------------------------------
            Msg::Refresh => return self.refresh(),
            Msg::People(_, result) if self.session.is_some() => match result {
                Ok(people) => {
                    self.people = people;
                    self.people_loaded = true;
                    if let Some(peer) = &self.peer { if !self.people.iter().any(|p| &p.id == peer) { return self.open_peer(None); } }
                }
                Err(error) => self.fail(error),
            },
            Msg::Communities(_, result) if self.session.is_some() => match result {
                Ok(list) => {
                    self.communities = list;
                    if let Some(current) = &self.community { if !self.communities.iter().any(|c| c.id == current.id) { return self.open_community(None); } }
                    if let Some(session) = self.session.clone() {
                        let ids = self.communities.iter().map(|c| c.id.clone()).collect();
                        return Task::perform(async move { session.unread_candidates(ids).await }, Msg::Unread);
                    }
                }
                Err(error) => self.fail(error),
            },
            Msg::Unread(result) => {
                if let Ok(candidates) = result {
                    let active = self.active_key();
                    for (key, position) in candidates {
                        if Some(&key) == active.as_ref() || !self.reads.newer(&key, &position) { continue; }
                        if key.contains(':') { self.unread_channels.insert(key); } else { self.unread_direct.insert(key); }
                    }
                }
            }
            Msg::Query(value) => self.query = value,
            Msg::SetMode(mode) => {
                if self.mode != mode {
                    self.save_draft();
                    self.reset_thread();
                    self.mode = mode;
                    self.restore_draft();
                    return match mode { Mode::Direct if self.peer.is_some() => self.load_thread(None), Mode::Community if self.channel.is_some() => self.load_thread(None), _ => Task::none() };
                }
            }
            Msg::OpenPeer(peer) => {
                self.panel = None;
                if self.mode != Mode::Direct { self.save_draft(); self.mode = Mode::Direct; }
                return self.open_peer(peer);
            }
            Msg::OpenPanel(panel) => self.panel = panel,
            Msg::AcceptKey => {
                if let Some(peer) = self.selected_peer().cloned() {
                    if let Some(key) = &peer.encryption_key {
                        self.trust.accept(&peer.id, key);
                        self.toast(ToastKind::Success, "New key trusted on this device.");
                        self.reset_thread();
                        return self.load_thread(None);
                    }
                }
            }

            // ----- Threads ---------------------------------------------------------
            Msg::Page(generation, result) if generation == self.generation => {
                self.thread.loading = false; self.thread.loading_older = false;
                match result {
                    Ok((messages, cursor, has_more)) => {
                        let deleted = self.thread.deleted.clone();
                        let pins = self.thread.pins;
                        chat::merge(&mut self.thread.messages, messages.into_iter().filter(|m| !deleted.contains(&m.id) && (!pins || m.pinned)));
                        self.thread.cursor = cursor;
                        self.thread.has_more = has_more;
                        self.mark_read();
                    }
                    Err(error) => self.fail(error),
                }
            }
            Msg::LoadOlder if self.thread.has_more && !self.thread.loading_older => { let cursor = self.thread.cursor.clone(); return self.load_thread(cursor); }
            Msg::TogglePins => {
                let pins = !self.thread.pins;
                self.reset_thread();
                self.thread.pins = pins;
                return self.load_thread(None);
            }
            Msg::Edit(action) => {
                if !action.is_edit() || self.composer.text().chars().count() < 4000 { self.composer.perform(action); }
            }
            Msg::Send => return self.send(None),
            Msg::React(target, emoji, selected) => {
                self.picker = None;
                return self.send(Some(chat::Reaction { target, emoji, action: if selected { "remove".into() } else { "add".into() } }));
            }
            Msg::Sent(generation, result) => {
                self.sending = false;
                match result {
                    Ok(message) if generation == self.generation => {
                        if message.reaction.is_none() {
                            self.composer = text_editor::Content::new();
                            if let Some(key) = self.draft_key() { self.drafts.remove(&key); }
                            if self.reply_to.as_ref() == message.reply_to.as_ref() { self.reply_to = None; }
                        }
                        if !self.thread.pins { chat::merge(&mut self.thread.messages, [message]); }
                    }
                    Ok(_) => {}
                    Err(error) => self.fail(error),
                }
            }
            Msg::Picker(target) => self.picker = if self.picker == target { None } else { target },
            Msg::Reply(target) => self.reply_to = target,
            Msg::Pin(id) => {
                let (Some(session), Some(message)) = (self.session.clone(), self.thread.messages.iter().find(|m| m.id == id).cloned()) else { return Task::none() };
                let generation = self.generation;
                let place = (self.community.as_ref().map(|c| c.id.clone()), self.channel.clone());
                let mode = self.mode;
                return Task::perform(async move {
                    let envelope = match (mode, place) {
                        (Mode::Community, (Some(community), Some(channel))) => session.pin_channel(&community, &channel, &message.id, !message.pinned).await?,
                        _ => session.pin_direct(&message.id, !message.pinned).await?,
                    };
                    let mut updated = message.clone();
                    updated.pinned = envelope.pinned;
                    updated.expires = envelope.expires_at.as_deref().and_then(chat::timestamp);
                    Ok(updated)
                }, move |result| Msg::Pinned(generation, result));
            }
            Msg::Pinned(generation, result) if generation == self.generation => match result {
                Ok(message) => {
                    if self.thread.pins && !message.pinned { self.thread.messages.retain(|m| m.id != message.id); }
                    else if self.visible(&message.id) { chat::merge(&mut self.thread.messages, [message]); }
                }
                Err(error) => self.fail(error),
            },
            Msg::AskDelete(id) => self.confirm_delete = id,
            Msg::Delete(id) => {
                self.confirm_delete = None;
                let Some(session) = self.session.clone() else { return Task::none() };
                let generation = self.generation;
                let place = (self.community.as_ref().map(|c| c.id.clone()), self.channel.clone());
                let mode = self.mode;
                let target = id.clone();
                return Task::perform(async move {
                    match (mode, place) {
                        (Mode::Community, (Some(community), Some(channel))) => session.delete_channel_message(&community, &channel, &target).await,
                        _ => session.delete_direct(&target).await,
                    }
                }, move |result| Msg::Deleted(generation, id.clone(), result));
            }
            Msg::Deleted(generation, id, result) => match result {
                Ok(()) if generation == self.generation => { self.thread.deleted.insert(id.clone()); self.thread.messages.retain(|m| m.id != id); }
                Ok(()) => {}
                Err(error) => self.fail(error),
            },

            // ----- Communities -----------------------------------------------------
            Msg::OpenCommunity(id) => {
                if self.mode != Mode::Community { self.save_draft(); self.mode = Mode::Community; }
                return self.open_community(id);
            }
            Msg::CommunityLoaded(generation, result) if generation == self.generation => match result {
                Ok(community) => {
                    let first_load = self.community.as_ref().map(|c| c.id.clone()) != Some(community.id.clone());
                    let channel_exists = self.channel.as_ref().is_some_and(|id| community.channels.iter().any(|c| &c.id == id && c.kind != "voice"));
                    self.community = Some(community);
                    let rooms = match (first_load, self.socket.clone(), self.community.as_ref()) {
                        (true, Some(socket), Some(c)) => { let id = c.id.clone(); Task::perform(async move { socket.request("voice:rooms", serde_json::json!({ "communityId": id })).await }, Msg::RoomsLoaded) }
                        _ => Task::none(),
                    };
                    if first_load || !channel_exists {
                        let first = self.community.as_ref().and_then(|c| c.channels.iter().find(|c| c.kind != "voice").map(|c| c.id.clone()));
                        self.save_draft();
                        self.reset_thread();
                        self.channel = first;
                        if let (Some(c), Some(ch)) = (self.community.as_ref(), &self.channel) { self.unread_channels.remove(&format!("{}:{ch}", c.id)); }
                        self.restore_draft();
                        return Task::batch([rooms, self.load_thread(None)]);
                    }
                    self.thread.loading = false;
                    return rooms;
                }
                Err(error) => { self.thread.loading = false; self.fail(error); }
            },
            Msg::OpenChannel(id) => {
                self.save_draft();
                self.reset_thread();
                if let Some(c) = &self.community { self.unread_channels.remove(&format!("{}:{id}", c.id)); }
                self.channel = Some(id);
                self.restore_draft();
                return self.load_thread(None);
            }
            Msg::ToggleMembers => self.members_open = !self.members_open,
            Msg::OpenDialog(dialog) => {
                if self.busy { return Task::none(); }
                self.dialog_value.clear(); self.invite = None; self.bans = None;
                if !matches!(dialog, Some(Dialog::Channel)) { self.dialog_voice = false; }
                let first = self.community.as_ref().and_then(|c| c.channels.first()).cloned();
                self.dialog_channel = first.as_ref().map(|c| c.id.clone());
                if dialog == Some(Dialog::RenameCommunity) { self.dialog_value = self.community.as_ref().map(|c| c.name.clone()).unwrap_or_default(); }
                if dialog == Some(Dialog::RenameChannel) { self.dialog_value = first.map(|c| c.name).unwrap_or_default(); }
                self.dialog = dialog.clone();
                let (Some(session), Some(community)) = (self.session.clone(), self.community.as_ref().map(|c| c.id.clone())) else { return Task::none() };
                match dialog {
                    Some(Dialog::Invite) => { self.busy = true; return Task::perform(async move { session.invite(&community).await }, Msg::InviteReady); }
                    Some(Dialog::Bans) => { self.busy = true; return Task::perform(async move { session.list_bans(&community).await }, Msg::BansLoaded); }
                    _ => {}
                }
            }
            Msg::DialogValue(value) => self.dialog_value = value,
            Msg::DialogVoice(voice) => self.dialog_voice = voice,
            Msg::NewChannel(voice) => { self.dialog_voice = voice; return self.update(Msg::OpenDialog(Some(Dialog::Channel))); }
            Msg::DialogChannel(id) => {
                if self.dialog == Some(Dialog::RenameChannel) { self.dialog_value = self.community.as_ref().and_then(|c| c.channels.iter().find(|ch| ch.id == id)).map(|c| c.name.clone()).unwrap_or_default(); }
                self.dialog_channel = Some(id);
            }
            Msg::SubmitDialog if !self.busy => {
                let Some(session) = self.session.clone() else { return Task::none() };
                let value = self.dialog_value.trim().to_owned();
                if value.is_empty() { return Task::none(); }
                let community = self.community.as_ref().map(|c| c.id.clone()).unwrap_or_default();
                let channel = self.dialog_channel.clone();
                let kind = if self.dialog_voice { "voice" } else { "text" };
                let dialog = self.dialog.clone();
                if dialog == Some(Dialog::Join) {
                    let lower = value.to_ascii_lowercase();
                    let token = lower.as_bytes().windows(48).find(|w| w.iter().all(u8::is_ascii_hexdigit)).map(|w| String::from_utf8_lossy(w).into_owned());
                    let Some(token) = token else { self.fail("Paste a valid invite link or code".into()); return Task::none() };
                    self.busy = true;
                    return Task::perform(async move { session.join(&token).await.map(|c| Some(c.id)) }, Msg::DialogDone);
                }
                self.busy = true;
                return Task::perform(async move {
                    match dialog {
                        Some(Dialog::Create) => session.new_community(&value).await.map(|c| Some(c.id)),
                        Some(Dialog::Channel) => session.new_channel(&community, &value, kind).await.map(|_| None),
                        Some(Dialog::RenameCommunity) => session.rename(&community, None, &value).await.map(|_| None),
                        Some(Dialog::RenameChannel) => session.rename(&community, channel.as_deref(), &value).await.map(|_| None),
                        _ => Ok(None),
                    }
                }, Msg::DialogDone);
            }
            Msg::DialogDone(result) => {
                self.busy = false;
                match result {
                    Ok(open) => {
                        self.dialog = None;
                        let refresh = self.refresh();
                        if let Some(id) = open { self.mode = Mode::Community; return Task::batch([refresh, self.open_community(Some(id))]); }
                        return Task::batch([refresh, self.refresh_community()]);
                    }
                    Err(error) => self.fail(error),
                }
            }
            Msg::InviteReady(result) => { self.busy = false; match result { Ok(invite) => self.invite = Some(invite), Err(error) => self.fail(error) } }
            Msg::CopyInvite => {
                if let (Some(invite), Some(session)) = (&self.invite, &self.session) {
                    let link = format!("{}/?invite={}", session.origin(), invite.token);
                    self.toast(ToastKind::Success, "Invite link copied");
                    return iced::clipboard::write(link);
                }
            }
            Msg::BansLoaded(result) => { self.busy = false; match result { Ok(list) => self.bans = Some(list), Err(error) => self.fail(error) } }
            Msg::AskConfirm(confirm) => { if !self.busy { self.confirm = confirm; if self.confirm.is_some() { self.dialog = None; } } }
            Msg::ApplyConfirm if !self.busy => {
                let (Some(confirm), Some(session), Some(community)) = (self.confirm.clone(), self.session.clone(), self.community.as_ref().map(|c| c.id.clone())) else { return Task::none() };
                self.busy = true;
                let action = confirm.action.clone();
                return Task::perform(async move {
                    match confirm.action.as_str() {
                        "delete-community" => session.delete_community(&community).await,
                        "delete-channel" => session.delete_channel(&community, &confirm.channel.map(|c| c.id).unwrap_or_default()).await,
                        "leave" => session.member_action(&community, session.me(), "leave").await,
                        action => session.member_action(&community, &confirm.member.map(|m| m.id).unwrap_or_default(), action).await,
                    }
                }, move |r| Msg::Confirmed(action.clone(), r));
            }
            Msg::Confirmed(action, result) => {
                self.busy = false;
                match result {
                    Ok(()) => {
                        self.confirm = None;
                        self.toast(ToastKind::Success, match action.as_str() {
                            "ban" => "Member banned. Previous invite revoked.", "unban" => "Ban removed. Create a new invite to let them rejoin.",
                            "promote" => "Moderator added", "demote" => "Moderator removed", "delete-community" => "Community deleted",
                            "delete-channel" => "Channel deleted", "leave" => "You left the community", "remove" => "Member removed. Previous invite revoked.",
                            _ => "Ownership transferred. Previous invite revoked.",
                        });
                        let refresh = self.refresh();
                        if matches!(action.as_str(), "leave" | "delete-community") { return Task::batch([refresh, self.open_community(None)]); }
                        return Task::batch([refresh, self.refresh_community()]);
                    }
                    Err(error) => self.fail(error),
                }
            }

            // ----- Account settings ------------------------------------------------
            Msg::OpenSettings(open) => {
                if !open && self.busy { return Task::none(); }
                self.settings_open = open;
                self.passwords.iter_mut().for_each(Zeroize::zeroize);
                if open {
                    self.tab = Tab::Profile;
                    self.profile_name = self.session.as_ref().map(|s| s.account.full_name.clone()).unwrap_or_default();
                }
            }
            Msg::SetTab(tab) => {
                if self.busy { return Task::none(); }
                self.tab = tab;
                self.passwords.iter_mut().for_each(Zeroize::zeroize);
                self.show_passwords = false; self.passkey_adding = false; self.passkey_removing = None; self.passkey_password.zeroize();
                let Some(session) = self.session.clone() else { return Task::none() };
                match tab {
                    Tab::Security => {
                        let other = session.clone();
                        return Task::batch([
                            Task::perform(async move { session.sessions_list().await }, Msg::SessionsLoaded),
                            Task::perform(async move { other.passkey_list().await }, Msg::PasskeysLoaded),
                        ]);
                    }
                    Tab::Privacy => {
                        self.backup_kind = None;
                        let key = session.account.encryption_key.clone();
                        return Task::perform(async move {
                            Ok(match session.key_backup().await?.filter(|b| Some(&b.public_key) == key.as_ref()) {
                                Some(b) if b.version == key_backup::RECOVERY_VERSION => "recovery",
                                Some(_) => "password",
                                None => "none",
                            })
                        }, Msg::BackupStatus);
                    }
                    Tab::Voice => return Task::perform(async { tokio::task::spawn_blocking(audio::enumerate).await.map_err(|_| "Audio devices unavailable".to_string())? }, Msg::AudioLoaded),
                    Tab::Profile => {}
                }
            }
            Msg::ProfileName(value) => self.profile_name = value,
            Msg::SaveProfile if !self.busy => {
                let Some(session) = self.session.clone() else { return Task::none() };
                let name = self.profile_name.trim().to_owned();
                if name.is_empty() || name.chars().count() > 80 { return Task::none(); }
                self.busy = true;
                return Task::perform(async move { session.update_name(&name).await }, Msg::ProfileSaved);
            }
            Msg::ProfileSaved(result) => {
                self.busy = false;
                match result {
                    Ok(account) => { if let Some(s) = self.session.as_mut() { s.account.full_name = account.full_name; } self.toast(ToastKind::Success, "Profile updated successfully"); }
                    Err(error) => self.fail(error),
                }
            }
            Msg::PasswordField(index, value) => { if let Some(slot) = self.passwords.get_mut(index) { *slot = value; } }
            Msg::ShowPasswords(value) => self.show_passwords = value,
            Msg::ChangePassword if !self.busy => {
                let [current, next, confirmation] = &self.passwords;
                if next != confirmation { self.fail("Your new passwords do not match".into()); return Task::none(); }
                if current == next { self.fail("Choose a different password".into()); return Task::none(); }
                if next.chars().count() < 10 || next.len() > 72 { self.fail("Use a new password of at least 10 characters and at most 72 UTF-8 bytes".into()); return Task::none(); }
                let Some(session) = self.session.clone() else { return Task::none() };
                self.busy = true;
                let (current, next) = (Zeroizing::new(current.clone()), Zeroizing::new(next.clone()));
                return Task::perform(identity::change_password(session, current, next), Msg::PasswordChanged);
            }
            Msg::PasswordChanged(result) => {
                self.busy = false;
                match result {
                    Ok(()) => { store::forget_session(); self.clear_session(); self.toast(ToastKind::Success, "Password updated. Sign in with your new password."); }
                    Err(error) => self.fail(error),
                }
            }
            Msg::SessionsLoaded(result) => match result { Ok(list) => self.sessions = Some(list), Err(error) => self.fail(error) },
            Msg::EndSession(id) if !self.busy => {
                let Some(session) = self.session.clone() else { return Task::none() };
                self.busy = true;
                return Task::perform(async move { session.end_session(&id).await }, Msg::SessionEnded);
            }
            Msg::SessionEnded(result) => {
                self.busy = false;
                match result {
                    Ok(true) => { store::forget_session(); self.clear_session(); self.toast(ToastKind::Success, "Session signed out"); }
                    Ok(false) => { self.toast(ToastKind::Success, "Session signed out"); return self.update(Msg::SetTab(Tab::Security)); }
                    Err(error) => self.fail(error),
                }
            }
            Msg::PasskeysLoaded(result) => match result { Ok(list) => self.passkeys = Some(list), Err(error) => self.fail(error) },
            Msg::PasskeyAdding(value) => { self.passkey_adding = value; self.passkey_removing = None; self.passkey_name.clear(); self.passkey_password.zeroize(); }
            Msg::PasskeyName(value) => self.passkey_name = value,
            Msg::PasskeyPassword(value) => self.passkey_password = value,
            #[cfg(windows)]
            Msg::AddPasskey if !self.busy => {
                let Some(session) = self.session.clone() else { return Task::none() };
                let window = passkey::active_window();
                if window == 0 { self.fail("Bring Sonar to the front and try again.".into()); return Task::none(); }
                self.busy = true;
                let (name, password) = (self.passkey_name.clone(), std::mem::take(&mut self.passkey_password));
                return Task::perform(session.register_passkey(name, password, window), Msg::PasskeyAdded);
            }
            Msg::PasskeyAdded(result) => {
                self.busy = false;
                match result {
                    Ok(_) => { self.passkey_adding = false; self.toast(ToastKind::Success, "Passkey added"); return self.update(Msg::SetTab(Tab::Security)); }
                    Err(error) => self.fail(error),
                }
            }
            Msg::PasskeyRemove(id) => { self.passkey_removing = id; self.passkey_adding = false; self.passkey_password.zeroize(); }
            Msg::RemovePasskey if !self.busy => {
                let (Some(session), Some(id)) = (self.session.clone(), self.passkey_removing.clone()) else { return Task::none() };
                self.busy = true;
                let password = Zeroizing::new(std::mem::take(&mut self.passkey_password));
                return Task::perform(async move { session.delete_passkey(&id, &password).await }, Msg::PasskeyRemoved);
            }
            Msg::PasskeyRemoved(result) => {
                self.busy = false;
                match result {
                    Ok(()) => { store::forget_session(); self.clear_session(); self.toast(ToastKind::Success, "Passkey removed. Sign in again."); }
                    Err(error) => self.fail(error),
                }
            }
            Msg::BackupStatus(result) => self.backup_kind = result.ok(),
            Msg::SyncPassword(value) => self.sync_password = value,
            Msg::EnableSync if !self.busy && !self.sync_password.is_empty() => {
                let Some(session) = self.session.clone() else { return Task::none() };
                self.busy = true;
                let password = Zeroizing::new(std::mem::take(&mut self.sync_password));
                if self.backup_kind != Some("none") {
                    let Some(key) = self.key.clone() else { self.busy = false; return Task::none() };
                    return Task::perform(identity::new_recovery(session, key, password), |r| Msg::RecoveryMade(r.map(|k| k.to_string())));
                }
                return Task::perform(identity::rotate(session, password), |r| Msg::Synced(r.map(|p| (p.account, Arc::new(p.key), p.recovery.map(|k| k.to_string())))));
            }
            Msg::Synced(result) => {
                self.busy = false;
                match result {
                    Ok((account, key, recovery)) => {
                        if let Some(s) = self.session.as_mut() { s.account = account; }
                        self.key = Some(key);
                        self.backup_kind = Some("recovery");
                        self.new_recovery = recovery; self.recovery_saved = false;
                        self.toast(ToastKind::Success, "Multi-device access is on. Sign in on the web or another computer.");
                        self.reset_thread();
                    }
                    Err(error) => self.fail(error),
                }
            }
            Msg::CopyFingerprint => {
                if let Some(print) = self.session.as_ref().and_then(|s| s.account.encryption_key.as_deref()).and_then(|k| crypto::fingerprint(k).ok()) {
                    self.toast(ToastKind::Success, "Fingerprint copied");
                    return iced::clipboard::write(print);
                }
            }
            Msg::AudioLoaded(result) => match result { Ok(devices) => self.audio_devices = Some(devices), Err(error) => self.fail(error) },
            Msg::SelectInput(value) => { self.settings.input = Some(value).filter(|v| v != ui::SYSTEM_DEFAULT); store::save_settings(&self.settings); }
            Msg::SelectOutput(value) => { self.settings.output = Some(value).filter(|v| v != ui::SYSTEM_DEFAULT); store::save_settings(&self.settings); }
            Msg::RelayOnly(value) => { self.settings.relay_only = value; store::save_settings(&self.settings); }
            Msg::TestMic if !self.testing => {
                self.testing = true; self.mic_level = None;
                let input = self.settings.input.clone();
                return Task::perform(async move { tokio::task::spawn_blocking(move || audio::test_input(input)).await.map_err(|_| "Microphone test failed".to_string())? }, Msg::MicResult);
            }
            Msg::MicResult(result) => { self.testing = false; match result { Ok(level) => self.mic_level = Some(level), Err(error) => self.fail(error) } }
            Msg::TestSpeakers if !self.testing => {
                self.testing = true;
                let output = self.settings.output.clone();
                return Task::perform(async move { tokio::task::spawn_blocking(move || audio::test_output(output)).await.map_err(|_| "Speaker test failed".to_string())? }, Msg::SpeakersResult);
            }
            Msg::SpeakersResult(result) => { self.testing = false; if let Err(error) = result { self.fail(error); } }

            // ----- Calls ------------------------------------------------------------
            Msg::RoomsLoaded(result) => {
                if let Ok(value) = result {
                    for item in value.get("rooms").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
                        let (Some(community), Some(channel)) = (item.get("communityId").and_then(|v| v.as_str()), item.get("channelId").and_then(|v| v.as_str())) else { continue };
                        let participants = item.get("participants").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
                        self.rooms.insert(format!("{community}:{channel}"), participants);
                    }
                }
            }
            Msg::JoinRoom(channel) => {
                if self.call.is_some() || self.incoming.is_some() { self.toast(ToastKind::Plain, "End your direct call before joining a voice room."); return Task::none(); }
                if self.room.is_some() { self.toast(ToastKind::Plain, "Leave your current voice room first."); return Task::none(); }
                let (Some(socket), Some(community)) = (self.socket.clone(), self.community.clone()) else { self.fail("Reconnect before joining voice.".into()); return Task::none() };
                let name = community.channels.iter().find(|c| c.id == channel).map(|c| c.name.clone()).unwrap_or_default();
                self.room = Some(RoomState { spec: None, controls: None, community: community.id.clone(), channel: channel.clone(), name, community_name: community.name.clone(), participants: Vec::new(), connections: 0, muted: false, deafened: false, early: Vec::new() });
                return Task::perform(async move { socket.request("voice:room-join", serde_json::json!({ "communityId": community.id, "channelId": channel })).await }, Msg::RoomJoined);
            }
            Msg::RoomJoined(result) => {
                let (Some(session), Some(socket)) = (self.session.clone(), self.socket.clone()) else { return Task::none() };
                let Some(room) = self.room.as_mut() else { return Task::none() };
                match result {
                    Ok(value) => {
                        let Some(session_id) = value.get("sessionId").and_then(|v| v.as_str()).map(str::to_owned) else { self.room = None; self.fail("Voice room unavailable".into()); return Task::none() };
                        let participants: Vec<Participant> = value.get("participants").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
                        room.participants = participants.clone();
                        let me = session.me().to_owned();
                        room.spec = Some(room::Spec {
                            community: room.community.clone(), channel: room.channel.clone(), session_id, me,
                            participants: participants.iter().map(|p| (p.user_id.clone(), p.session_id.clone())).collect(),
                            session, socket, input: self.settings.input.clone(), output: self.settings.output.clone(), relay_only: self.settings.relay_only,
                        });
                        let key = format!("{}:{}", room.community, room.channel);
                        self.rooms.insert(key, participants);
                    }
                    Err(error) => { self.room = None; self.fail(error); }
                }
            }
            Msg::RoomUpdate(id, update) => {
                let Some(room) = self.room.as_mut().filter(|r| r.spec.as_ref().is_some_and(|s| s.session_id == id)) else { return Task::none() };
                match update {
                    room::Update::Ready(controls) => {
                        for (from, session, signal) in room.early.drain(..) { controls.send(room::Command::Signal { from, session, signal }); }
                        if room.muted { controls.send(room::Command::Mute(true)); }
                        if room.deafened { controls.send(room::Command::Deafen(true)); }
                        room.controls = Some(controls);
                    }
                    room::Update::Connections(count) => room.connections = count,
                    room::Update::Ended(error) => {
                        let left = self.room.take();
                        if let Some(error) = error { self.fail(error); }
                        if let (Some(socket), Some(spec)) = (self.socket.clone(), left.and_then(|r| r.spec)) {
                            return Task::perform(async move { let _ = socket.request("voice:room-leave", serde_json::json!({ "sessionId": spec.session_id })).await; }, |_| Msg::Noop);
                        }
                    }
                }
            }
            Msg::RoomMute | Msg::RoomDeafen => {
                let mute = matches!(message, Msg::RoomMute);
                let Some(room) = self.room.as_mut() else { return Task::none() };
                if mute { room.muted = !room.muted; } else { room.deafened = !room.deafened; }
                if let Some(c) = &room.controls { c.send(room::Command::Mute(room.muted)); c.send(room::Command::Deafen(room.deafened)); }
                if let (Some(socket), Some(spec)) = (self.socket.clone(), room.spec.clone()) {
                    let (muted, deafened) = (room.muted, room.deafened);
                    return Task::perform(async move { let _ = socket.request("voice:room-status", serde_json::json!({ "sessionId": spec.session_id, "muted": muted, "deafened": deafened })).await; }, |_| Msg::Noop);
                }
            }
            Msg::LeaveRoom => {
                let Some(room) = self.room.take() else { return Task::none() };
                if let Some(c) = &room.controls { c.send(room::Command::Leave); }
                if let (Some(socket), Some(spec)) = (self.socket.clone(), room.spec) {
                    return Task::perform(async move { let _ = socket.request("voice:room-leave", serde_json::json!({ "sessionId": spec.session_id })).await; }, |_| Msg::Noop);
                }
            }
            Msg::RoomLock(channel, locked) => {
                let (Some(session), Some(community)) = (self.session.clone(), self.community.as_ref().map(|c| c.id.clone())) else { return Task::none() };
                return Task::perform(async move { session.set_voice_lock(&community, &channel, locked).await }, move |r| Msg::Done(r, if locked { "Voice room locked to new participants." } else { "Voice room unlocked." }));
            }
            Msg::RoomKick(channel, person) => {
                let (Some(socket), Some(community)) = (self.socket.clone(), self.community.as_ref().map(|c| c.id.clone())) else { return Task::none() };
                return Task::perform(async move { socket.request("voice:room-kick", serde_json::json!({ "communityId": community, "channelId": channel, "to": person.user_id, "toSession": person.session_id })).await.map(|_| ()) }, |r| Msg::Done(r, "Participant disconnected. They can rejoin unless the room is locked."));
            }
            Msg::Done(result, success) => match result {
                Ok(()) => { self.toast(ToastKind::Plain, success); return self.refresh_community(); }
                Err(error) => self.fail(error),
            },
            Msg::StartCall if self.call.is_none() && self.room.is_none() => {
                let (Some(session), Some(socket), Some(peer)) = (self.session.clone(), self.socket.clone(), self.peer.clone()) else { return Task::none() };
                if !self.online.contains(&peer) { return Task::none(); }
                self.call = Some(CallState {
                    spec: call::Spec { id: call::new_id(), peer, role: call::Role::Outgoing, session, socket, input: self.settings.input.clone(), output: self.settings.output.clone(), relay_only: self.settings.relay_only },
                    controls: None, status: CallStatus::Calling, muted: false, deafened: false, started: None,
                });
            }
            Msg::AcceptCall if self.call.is_none() && self.room.is_none() => {
                let (Some(session), Some(socket), Some(incoming)) = (self.session.clone(), self.socket.clone(), self.incoming.take()) else { return Task::none() };
                self.call = Some(CallState {
                    spec: call::Spec { id: incoming.id.clone(), peer: incoming.from.clone(), role: call::Role::Incoming(incoming), session, socket, input: self.settings.input.clone(), output: self.settings.output.clone(), relay_only: self.settings.relay_only },
                    controls: None, status: CallStatus::Connecting, muted: false, deafened: false, started: None,
                });
            }
            Msg::DeclineCall => {
                if let (Some(incoming), Some(socket)) = (self.incoming.take(), self.socket.clone()) {
                    return Task::perform(async move { let _ = socket.decline(incoming).await; }, |_| Msg::Noop);
                }
            }
            Msg::CallUpdate(id, update) => {
                let Some(state) = self.call.as_mut().filter(|c| c.spec.id == id) else { return Task::none() };
                match update {
                    call::Update::Ready(controls) => {
                        if state.muted { controls.send(call::Command::Mute(true)); }
                        if state.deafened { controls.send(call::Command::Deafen(true)); }
                        state.controls = Some(controls);
                    }
                    call::Update::Connecting => state.status = CallStatus::Connecting,
                    call::Update::Connected => { state.status = CallStatus::Connected; state.started = Some(std::time::Instant::now()); }
                    call::Update::Ended(error) => { self.call = None; if let Some(error) = error { self.fail(error); } }
                }
            }
            Msg::Mute => if let Some(state) = self.call.as_mut() {
                state.muted = !state.muted;
                if let Some(c) = &state.controls { c.send(call::Command::Mute(state.muted)); }
            },
            Msg::Deafen => if let Some(state) = self.call.as_mut() {
                state.deafened = !state.deafened;
                if let Some(c) = &state.controls { c.send(call::Command::Deafen(state.deafened)); }
            },
            Msg::Hangup => if let Some(state) = self.call.as_ref() {
                match &state.controls { Some(c) => c.send(call::Command::Hangup), None => self.call = None }
            },
            _ => {}
        }
        Task::none()
    }

    /// Runs the next scripted step (development builds only; empty otherwise).
    fn script_step(&mut self) -> Task<Msg> {
        let Some(step) = self.script.pop_front() else { return Task::none() };
        let next = || Task::perform(tokio::time::sleep(std::time::Duration::from_millis(400)), |_| Msg::Script);
        let (kind, value) = step.split_once(':').unwrap_or((step.as_str(), ""));
        let value = value.to_owned();
        let message = match kind {
            "wait" => return Task::perform(tokio::time::sleep(std::time::Duration::from_millis(value.parse().unwrap_or(1000))), |_| Msg::Script),
            "shot" => return iced::window::latest().and_then(iced::window::screenshot).map(move |shot| Msg::Screenshot(value.clone(), shot)),
            "peer" => self.people.iter().find(|p| p.name == value).map(|p| Msg::OpenPeer(Some(p.id.clone()))),
            "community" => self.communities.iter().find(|c| c.name == value).map(|c| Msg::OpenCommunity(Some(c.id.clone()))),
            "mode" => Some(Msg::SetMode(if value == "community" { Mode::Community } else { Mode::Direct })),
            "settings" => Some(Msg::OpenSettings(value != "close")),
            "tab" => Some(Msg::SetTab(match value.as_str() { "security" => Tab::Security, "privacy" => Tab::Privacy, "voice" => Tab::Voice, _ => Tab::Profile })),
            "panel" => Some(Msg::OpenPanel(match value.as_str() { "people" => Some(Panel::People), "privacy" => Some(Panel::Privacy), _ => None })),
            "dialog" => Some(Msg::OpenDialog(match value.as_str() { "create" => Some(Dialog::Create), "invite" => Some(Dialog::Invite), "start" => Some(Dialog::Start), _ => None })),
            "members" => Some(Msg::ToggleMembers),
            "type" => { self.composer = text_editor::Content::with_text(&value); None }
            "send" => Some(Msg::Send),
            "react" => self.thread.messages.iter().rev().find(|m| m.reaction.is_none()).map(|m| Msg::React(m.id.clone(), value.clone(), false)),
            "reply" => self.thread.messages.iter().rev().find(|m| m.reaction.is_none()).map(|m| Msg::Reply(Some(m.id.clone()))),
            "pin" => self.thread.messages.iter().rev().find(|m| m.reaction.is_none()).map(|m| Msg::Pin(m.id.clone())),
            "call" => Some(Msg::StartCall),
            "accept" => Some(Msg::AcceptCall),
            "hangup" => Some(Msg::Hangup),
            "room" => self.community.as_ref().and_then(|c| c.channels.iter().find(|ch| ch.name == value && ch.kind == "voice")).map(|ch| Msg::JoinRoom(ch.id.clone())),
            "leave" => Some(Msg::LeaveRoom),
            #[cfg(windows)]
            "addpasskey" => {
                let (name, password) = value.split_once('|').unwrap_or((value.as_str(), ""));
                self.passkey_adding = true; self.passkey_name = name.into(); self.passkey_password = password.into();
                Some(Msg::AddPasskey)
            }
            #[cfg(windows)]
            "passkeylogin" => Some(Msg::PasskeySignIn),
            "focus" => return Task::batch([iced::window::latest().and_then(iced::window::gain_focus), next()]),
            "signout" => Some(Msg::SignOut),
            "probe" => { self.probe = Some((std::time::Instant::now(), Vec::new())); return Task::none(); }
            "testmic" => Some(Msg::TestMic),
            "testspk" => Some(Msg::TestSpeakers),
            "saved" => { self.recovery_saved = true; Some(Msg::DismissRecovery) }
            "unlock" => { self.unlock_password = value.clone(); Some(Msg::Unlock) }
            "resetkey" => Some(Msg::ToggleUnlockReset),
            "forgot" => {
                let parts: Vec<&str> = value.split('|').collect();
                if parts.len() == 3 { self.auth.recover = true; self.auth.email = parts[0].into(); self.auth.recovery_key = parts[1].into(); self.auth.password = parts[2].into(); self.auth.confirm = parts[2].into(); }
                Some(Msg::Recover)
            }
            "syncpw" => { self.sync_password = value.clone(); Some(Msg::EnableSync) }
            "print" => { println!("SCRIPT {value} {} {}", chrono::Utc::now().timestamp_millis() % 100000, self.debug_state()); None }
            _ => None,
        };
        match message { Some(message) => Task::batch([self.update(message), next()]), None => next() }
    }
    fn debug_state(&self) -> String {
        let texts: Vec<&str> = self.thread.messages.iter().filter(|m| m.reaction.is_none()).map(|m| m.text.as_str()).collect();
        let reactions = chat::reactions(&self.thread.messages, self.now);
        format!("{}", serde_json::json!({ "screen": format!("{:?}", self.screen), "unlock": format!("{:?}", self.unlock_reason), "recoveryKey": self.new_recovery.clone(), "backup": self.backup_kind, "micLevel": self.mic_level, "peer": self.peer.clone(), "online": self.online.len(), "live": self.live_connected, "people": self.people.len(), "passkeys": self.passkeys.as_ref().map(|p| p.len()), "sent": voice_transport::PACKETS_SENT.load(std::sync::atomic::Ordering::Relaxed), "received": voice_transport::PACKETS_RECEIVED.load(std::sync::atomic::Ordering::Relaxed), "capturePeak": voice_audio::CAPTURE_PEAK.load(std::sync::atomic::Ordering::Relaxed), "texts": texts, "reactions": reactions.len(), "call": self.call.as_ref().map(|c| format!("{:?}", c.status)), "room": self.room.as_ref().map(|r| format!("{} people, {} connected", r.participants.len(), r.connections)), "incoming": self.incoming.is_some(), "unread": self.unread_direct.len() + self.unread_channels.len(), "toasts": self.toasts.iter().map(|t| t.text.clone()).collect::<Vec<_>>() }))
    }

    fn send(&mut self, reaction: Option<chat::Reaction>) -> Task<Msg> {
        if self.sending { return Task::none(); }
        let (Some(session), Some(key)) = (self.session.clone(), self.key.clone()) else { return Task::none() };
        let text = self.composer.text().trim().to_owned();
        if reaction.is_none() && text.is_empty() { return Task::none(); }
        let reply = if reaction.is_none() { self.reply_to.clone() } else { None };
        let plain = match &reaction {
            Some(r) => message_content::encode_reaction(&r.target, &r.emoji, &r.action),
            None => message_content::encode_text(&text, reply.as_deref()),
        };
        let plain = match plain { Ok(p) => Zeroizing::new(p), Err(_) => { self.fail("Messages can be up to 4000 characters.".into()); return Task::none(); } };
        let generation = self.generation;
        let me = self.me().to_owned();
        match self.mode {
            Mode::Direct => {
                let Some(peer) = self.selected_peer().cloned() else { return Task::none() };
                let Some(peer_key) = peer.encryption_key.clone() else { self.fail("This person has not set up encryption yet.".into()); return Task::none() };
                if !self.trust.remember(&peer.id, &peer_key) { self.fail("Contact key changed. Sending is blocked. Verify their identity first.".into()); return Task::none(); }
                self.sending = true;
                Task::perform(async move {
                    let context = crypto::context(&me, &peer.id, None).map_err(|e| e.to_string())?;
                    let envelope = crypto::encrypt(&key, &peer_key, &plain, &context).map_err(|e| e.to_string())?;
                    let stored = session.send_direct(&peer.id, envelope).await?;
                    Ok(local(stored, text, reply, reaction))
                }, move |r| Msg::Sent(generation, r))
            }
            Mode::Community => {
                let (Some(community_id), Some(channel)) = (self.community.as_ref().map(|c| c.id.clone()), self.channel.clone()) else { return Task::none() };
                self.sending = true;
                Task::perform(async move {
                    // Refresh membership first so every current member gets an envelope.
                    let community = session.community(&community_id).await?;
                    let mut envelopes = Vec::with_capacity(community.member_details.len());
                    for member in &community.member_details {
                        let member_key = member.encryption_key.as_deref().ok_or("A member has not set up encryption yet.")?;
                        let context = crypto::context(&me, &member.id, Some(&channel)).map_err(|e| e.to_string())?;
                        let envelope = crypto::encrypt(&key, member_key, &plain, &context).map_err(|e| e.to_string())?;
                        envelopes.push(serde_json::json!({ "recipientId": member.id, "version": 1, "iv": envelope.iv, "ciphertext": envelope.ciphertext }));
                    }
                    let stored = session.send_channel(&community_id, &channel, community.revision, envelopes).await?;
                    Ok(local(stored, text, reply, reaction))
                }, move |r| Msg::Sent(generation, r))
            }
        }
    }
}

/// The sender's view of a message it just encrypted and stored.
fn local(stored: api::MessageEnvelope, text: String, reply: Option<String>, reaction: Option<chat::Reaction>) -> chat::Message {
    chat::Message {
        id: stored.id, sender: stored.sender_id, receiver: stored.receiver_id, channel: stored.channel_id,
        created: chat::timestamp(&stored.created_at).unwrap_or_else(chat::now), created_at: stored.created_at,
        expires: stored.expires_at.as_deref().and_then(chat::timestamp), pinned: stored.pinned,
        text: if reaction.is_some() { String::new() } else { text }, reply_to: reply, reaction, unreadable: false,
    }
}
