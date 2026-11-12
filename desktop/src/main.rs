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
mod taskbar;
mod theme;
#[cfg(windows)]
mod tray;
mod tv;
mod watch;
mod ui;
mod update;
mod wheel;
mod screen;
mod screen_audio;
mod screen_decode;
mod screen_media;
#[allow(dead_code)]
mod voice_audio;
#[allow(dead_code)]
mod voice_codec;
#[allow(dead_code)]
mod voice_session;
#[allow(dead_code)]
mod voice_transport;

use api::{Community, CreatedInvite, InvitePreview, Passkey, Person, Session, Sessions};
use iced::widget::text_editor;
use iced::{Subscription, Task};
use p256::SecretKey;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use zeroize::{Zeroize, Zeroizing};

/// Windows uses the large icon on the taskbar and Iced only sets the small one,
/// so hand it the .exe's own icon once the window exists.
#[cfg(windows)]
const FRIEND_NOTE_SCOPE: &str = "000000000000000000000001";

#[cfg(windows)]
fn show_window() -> Task<Msg> {
    iced::window::latest().and_then(|id| Task::batch([iced::window::set_mode(id, iced::window::Mode::Windowed), iced::window::gain_focus(id)]))
}

fn t_animal(value: &str) -> Option<&'static str> { theme::ANIMALS.iter().find(|(name, _)| *name == value).map(|(name, _)| *name) }

fn first_update_check() -> Task<Msg> {
    Task::perform(tokio::time::sleep(std::time::Duration::from_secs(10)), |_| Msg::UpdateCheck)
}

fn set_taskbar_icon() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetSystemMetrics, GetWindowThreadProcessId, IsWindowVisible, LoadImageW, SendMessageW, ICON_BIG, IMAGE_ICON, LR_DEFAULTCOLOR, SM_CXICON, WM_SETICON};
    static DONE: AtomicBool = AtomicBool::new(false);
    unsafe extern "system" fn visit(window: HWND, _: LPARAM) -> BOOL {
        let mut owner = 0;
        unsafe {
            GetWindowThreadProcessId(window, &mut owner);
            if owner != GetCurrentProcessId() || IsWindowVisible(window) == 0 { return 1; }
            let size = GetSystemMetrics(SM_CXICON);
            // Resource 1 is the icon from assets/sonar.rc.
            let icon = LoadImageW(GetModuleHandleW(std::ptr::null()), 1 as _, IMAGE_ICON, size, size, LR_DEFAULTCOLOR);
            if !icon.is_null() { SendMessageW(window, WM_SETICON, ICON_BIG as _, icon as _); DONE.store(true, Ordering::Relaxed); }
        }
        1
    }
    if !DONE.load(Ordering::Relaxed) { unsafe { EnumWindows(Some(visit), 0); } }
}

/// The window and taskbar icon. The .exe carries the same artwork for Explorer.
fn app_icon() -> Option<iced::window::Icon> {
    let decoder = png::Decoder::new(&include_bytes!("../assets/sonar-icon.png")[..]);
    let mut reader = decoder.read_info().ok()?;
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight { return None; }
    pixels.truncate(info.buffer_size());
    iced::window::icon::from_rgba(pixels, info.width, info.height).ok()
}

fn main() -> iced::Result {
    // Software drawing took 282 ms per frame at 2154x1343; the GPU takes 6 ms.
    // DX12 used the least memory of the GPU backends. Iced falls back to
    // software drawing by itself on a PC where this cannot start.
    if std::env::var_os("WGPU_BACKEND").is_none() { std::env::set_var("WGPU_BACKEND", "dx12"); }
    iced::application(App::boot, App::step, ui::view)
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
            size: if cfg!(debug_assertions) { std::env::var("SONAR_DEV_SIZE").ok().and_then(|v| { let (w, h) = v.split_once('x')?; Some(iced::Size::new(w.parse().ok()?, h.parse().ok()?)) }).unwrap_or(iced::Size::new(1280.0, 820.0)) } else { iced::Size::new(1280.0, 820.0) },
            min_size: Some(iced::Size::new(980.0, 620.0)),
            exit_on_close_request: false,
            icon: app_icon(),
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
pub enum Panel { People, Privacy, Search, Group, Tv }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab { Profile, Security, Privacy, Voice, Alerts }
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dialog { Start, Create, Join, Channel, Invite, Bans, RenameCommunity, RenameChannel, DeleteChannel, Activity, Permissions }
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
#[derive(Clone, Debug)]
pub struct ScreenPicker { pub sources: Vec<screen::Shareable>, pub selected: usize, pub height: u32, pub hardware: bool }
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
    pub started: Option<std::time::Instant>, pub rang: bool,
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
pub struct AuthForm { pub username: String, pub recover: bool, pub recovery_key: String, pub confirm: String, pub signup: bool, pub name: String, pub email: String, pub password: String, pub server: String, pub server_open: bool, pub busy: bool }

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
    unread_counts: HashMap<String, u32>,
    shown_badge: u32,
    shown_tip: u32,
    deferred_preferences: bool,
    hidden: bool,
    pub preferences: api::Preferences,
    pub editing: Option<String>,
    pub search_query: String,
    pub search_results: Option<Vec<chat::Message>>,
    pub search_busy: bool,
    typing: HashMap<String, std::time::Instant>,
    last_typing: Option<std::time::Instant>,
    pub friend_note: String,
    pub friend_notes: HashMap<String, String>,
    pub keyword_input: String,
    pub group_name: String,
    pub group_picks: Vec<String>,
    pub folder_name: String,
    pub activity: Option<Vec<api::ActivityRow>>,
    pub permissions_for: Option<Person>,
    pub denied: Vec<String>,
    pub update_ready: Option<String>,
    last_chime: Option<std::time::Instant>,
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
    /// Invite cards by token; None while loading, Err when the lookup failed.
    pub invite_previews: HashMap<String, Option<Result<InvitePreview, String>>>,
    invite_scan: (usize, String),
    pub waiting: Option<live::IncomingCall>,
    pub emoji_open: bool,
    pub manage_open: bool,
    pub uploading: bool,
    pending_file: Option<message_content::FileMeta>,
    ringtone_level: f32,
    pub presence: HashMap<String, String>,
    pub status_open: bool,
    /// The person whose "⋯" actions are open in the People or members panel.
    pub person_menu: Option<String>,
    /// A Watch Together session running in this call or voice room: (video title, people watching).
    pub watch_note: Option<(Option<String>, u64)>,
    /// Sonar TV: sources saved on this PC (encrypted), the open one's channels, and the add form.
    pub tv_sources: Vec<tv::sources::Source>,
    pub tv_source: Option<String>,
    pub tv_channels: Vec<tv::sources::Channel>,
    pub tv_loading: bool,
    pub tv_query: String,
    pub tv_error: Option<String>,
    pub tv_xtream: bool,
    pub tv_adding: bool,
    /// Showing only the favorite channels of the chosen source.
    pub tv_favorites_only: bool,
    pub apps_open: bool,
    pub tv_name: String,
    pub tv_url: String,
    pub tv_server: String,
    pub tv_user: String,
    pub tv_pass: zeroize::Zeroizing<String>,
    /// What each share is: a TV channel's name (including our own), or none for a screen.
    pub screen_labels: HashMap<String, String>,
    /// TV shares waiting for us to join: sharer -> channel.
    pub tv_available: HashMap<String, String>,
    /// Our TV is shared with the call (not only watched here).
    pub tv_shared: bool,
    pub screen_controls: Option<screen::Controls>,
    pub screen_sharing: Option<String>,
    pub screen_views: HashMap<String, (iced::widget::image::Handle, u32, u32)>,
    pub screen_expanded: Option<String>,
    pub screen_volumes: HashMap<String, f32>,
    /// Keeps each shown frame's GPU upload alive; a frame only replaces the shown one once uploaded, so video never blinks black.
    pub screen_allocations: HashMap<String, iced_runtime::image::Allocation>,
    pub screen_uploading: HashMap<String, std::time::Instant>,
    pub screen_ended: std::collections::HashSet<String>,
    pub screen_picker: Option<ScreenPicker>,
    screen_targets: Vec<screen::Target>,
    screen_deafened: bool,
    ringtone: Option<audio::Ringtone>,
    pub bans: Option<Vec<Person>>,
    pub confirm: Option<Confirm>,
    pub busy: bool,
    // Account settings.
    pub settings_open: bool,
    pub tab: Tab,
    pub profile_name: String,
    pub profile_username: String,
    pub people_query: String,
    pub friend_requests: api::FriendRequests,
    pub mic_active: bool,
    pub mic_until: Option<std::time::Instant>,
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
    UpdateCheck,
    UpdateChecked(Result<Option<String>, String>),
    UpdateRestart,
    PreferencesLoaded(Result<api::Preferences, String>),
    SavePreferences(serde_json::Value),
    ToggleMute(String),
    TogglePinPerson(String),
    MoveToFolder(String, String),
    FolderName(String),
    Unblock(String),
    SetAnimal(&'static str),
    AnimalSaved(Result<api::Account, String>),
    StartEdit(String),
    CancelEdit,
    Edited(u64, Result<chat::Message, String>),
    Disappear(u32),
    Finished(Result<(), String>),
    SearchQuery(String),
    SearchRun,
    SearchDone(Result<Vec<chat::Message>, String>),
    AlertVolume(f32),
    ConfirmCalls(bool),
    AskCall,
    TypingHidden(bool),
    KeywordInput(String),
    AddKeyword,
    RemoveKeyword(String),
    FriendNote(String),
    GroupName(String),
    GroupPick(String, bool),
    CreateGroup,
    GroupCreated(Result<api::Community, String>),
    OpenGroup(String),
    ActivityLoaded(Result<Vec<api::ActivityRow>, String>),
    OpenPermissions(Person),
    Deny(&'static str, bool),
    SavePermissions,
    PermissionsSaved(Result<(), String>),
    InsertMention(String),
    MutedAlert(Box<api::MessageEnvelope>, Result<api::Community, String>),
    NoteOpened(String, String),
    CloseRequested(iced::window::Id),
    #[cfg(windows)]
    Tray(tray::Action),
    KeepInTray(bool),
    StartWithWindows(bool),
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
    PeopleQuery(String),
    ProfileUsername(String),
    MicPoll,
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
    TogglePersonMenu(String),
    FriendRemoved(Result<(), String>),
    OpenDialog(Option<Dialog>),
    DialogValue(String),
    DialogVoice(bool),
    NewChannel(bool),
    DialogChannel(String),
    SubmitDialog,
    DialogDone(Result<Option<String>, String>),
    InviteReady(Result<CreatedInvite, String>),
    InvitePreviewReady(String, Result<InvitePreview, String>),
    ScreenUpdate(String, screen::Update),
    ToggleStatusMenu,
    SwapCall,
    ToggleEmoji,
    Pulse,
    ToggleManage,
    PickEmoji(&'static str),
    FriendRequests(Result<api::FriendRequests, String>),
    SendFriendRequest,
    FriendRequestSent(Result<(), String>),
    AnswerFriendRequest(String, bool),
    FriendsChanged(Result<(), String>),
    AttachFile,
    FileReady(u64, Result<Option<message_content::FileMeta>, String>),
    DownloadFile(message_content::FileMeta),
    DeclineWaiting,
    SetStatus(&'static str),
    StatusSaved(String, Result<(), String>),
    ScreenOpenPicker,
    OpenWatch,
    OpenTv,
    ToggleApps,
    TvAdding(bool),
    TvSelect(String),
    TvChannels(String, Result<Vec<tv::sources::Channel>, String>),
    TvQuery(String),
    TvKind(bool),
    TvName(String),
    TvUrl(String),
    TvServer(String),
    TvUser(String),
    TvPass(String),
    TvAdd,
    TvRemove(String),
    TvFavorite(String),
    TvFavoritesOnly(bool),
    TvPlay(usize, bool),
    /// The next (1) or previous (-1) channel, on the same share.
    TvStep(i64),
    TvJoin(String),
    TvLeave(String),
    WatchOpened(Result<(), String>),
    ScreenSources(Vec<screen::Shareable>, bool),
    ScreenClosePicker,
    ScreenSelect(usize),
    ScreenHeight(u32),
    ScreenStart,
    ScreenStop,
    ScreenExpand(Option<String>),
    ScreenVolume(String, f32),
    ScreenUploaded(String, u32, u32, Result<iced_runtime::image::Allocation, iced_runtime::image::Error>),
    NoiseSuppression(bool),
    MissedCallSent(String, Result<chat::Message, String>),
    JoinInvite(String),
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
        voice_audio::NOISE_SUPPRESSION.store(!settings.noise_suppression_off, std::sync::atomic::Ordering::Relaxed);
        let server = if settings.server.is_empty() { "https://sonar-73yj.onrender.com".to_string() } else { settings.server.clone() };
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
            unread_counts: HashMap::new(), shown_badge: 0, update_ready: None, shown_tip: 0, deferred_preferences: false, hidden: false, preferences: Default::default(), editing: None, search_query: String::new(), search_results: None, search_busy: false, typing: HashMap::new(), last_typing: None, friend_note: String::new(), friend_notes: HashMap::new(), keyword_input: String::new(), group_name: String::new(), group_picks: Vec::new(), folder_name: String::new(), activity: None, permissions_for: None, denied: Vec::new(), last_chime: None, unread_direct: HashSet::new(), unread_channels: HashSet::new(),
            composer: text_editor::Content::new(), drafts: HashMap::new(), reply_to: None, sending: false, picker: None, confirm_delete: None,
            panel: None, dialog: None, dialog_value: String::new(), dialog_voice: false, dialog_channel: None,
            invite: None, invite_previews: HashMap::new(), invite_scan: Default::default(), waiting: None, emoji_open: false, manage_open: false, uploading: false, pending_file: None, ringtone_level: 0.0, presence: HashMap::new(), status_open: false, person_menu: None, watch_note: None, tv_sources: Vec::new(), tv_source: None, tv_channels: Vec::new(), tv_loading: false, tv_query: String::new(), tv_error: None, tv_xtream: false, tv_adding: false, tv_favorites_only: false, apps_open: false, tv_name: String::new(), tv_url: String::new(), tv_server: String::new(), tv_user: String::new(), tv_pass: Default::default(), screen_labels: HashMap::new(), tv_available: HashMap::new(), tv_shared: false, screen_controls: None, screen_sharing: None, screen_views: HashMap::new(), screen_expanded: None, screen_volumes: HashMap::new(), screen_allocations: HashMap::new(), screen_uploading: Default::default(), screen_ended: Default::default(), screen_picker: None, screen_targets: Vec::new(), screen_deafened: false, ringtone: None, bans: None, confirm: None, busy: false,
            settings_open: false, tab: Tab::Profile, profile_name: String::new(), profile_username: String::new(), people_query: String::new(), friend_requests: Default::default(), mic_active: false, mic_until: None, passwords: Default::default(), show_passwords: false,
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
                if let Ok(name) = std::env::var("SONAR_DEV_NAME") { app.auth.signup = true; app.auth.username = name.to_lowercase().replace(' ', ""); app.auth.name = name; }
                tasks.push(Task::done(Msg::Submit));
            } else if let Some((server, cookie)) = saved {
                app.screen = Screen::Boot;
                tasks.push(Task::perform(client::restore(server, cookie), Msg::Restored));
            }
            tasks.push(first_update_check());
            return (app, Task::batch(tasks));
        }
        let task = match saved {
            Some((server, cookie)) => Task::perform(client::restore(server, cookie), Msg::Restored),
            None => Task::none(),
        };
        (app, Task::batch([task, first_update_check()]))
    }

    fn subscription(&self) -> Subscription<Msg> {
        let mut list = vec![iced::time::every(std::time::Duration::from_millis(1000)).map(|_| Msg::Tick), iced::time::every(std::time::Duration::from_secs(6 * 60 * 60)).map(|_| Msg::UpdateCheck), iced::window::close_requests().map(Msg::CloseRequested)];
        #[cfg(windows)]
        if !(cfg!(debug_assertions) && std::env::var_os("SONAR_DEV_SCRIPT").is_some()) { list.push(Subscription::run(tray::events).map(Msg::Tray)); }
        if let (Some(session), Screen::Main) = (&self.session, self.screen) {
            list.push(Subscription::run_with(session.live_auth(self.live_generation), live::events).map(|(generation, event)| Msg::Live(generation, event)));
            list.push(iced::time::every(std::time::Duration::from_secs(30)).map(|_| Msg::Refresh));
        }
        list.push(iced::event::listen_with(|event, _, _| matches!(event, iced::Event::Window(iced::window::Event::Focused)).then_some(Msg::Noop)));
        if self.probe.is_some() { list.push(iced::window::frames().map(Msg::ProbeFrame)); }
        let speaking_possible = self.call.as_ref().is_some_and(|c| c.status == CallStatus::Connected && !c.muted && !c.deafened)
            || self.room.as_ref().is_some_and(|r| r.controls.is_some() && !r.muted && !r.deafened);
        if speaking_possible { list.push(iced::time::every(std::time::Duration::from_millis(100)).map(|_| Msg::MicPoll)); }
        if let Some(spec) = self.room.as_ref().and_then(|r| r.spec.clone()) {
            let id = spec.session_id.clone();
            list.push(Subscription::run_with(spec, room::run).with(id).map(|(id, update)| Msg::RoomUpdate(id, update)));
        }
        if let Some(call) = &self.call {
            list.push(Subscription::run_with(call.spec.clone(), call::run).with(call.spec.id.clone()).map(|(id, update)| Msg::CallUpdate(id, update)));
        }
        // The sonar pulse only redraws while a welcome screen is showing.
        let welcome = matches!(self.screen, Screen::Main) && match self.mode { Mode::Direct => self.peer.is_none(), Mode::Community => self.community.is_none() };
        if welcome { list.push(iced::time::every(std::time::Duration::from_millis(33)).map(|_| Msg::Pulse)); }
        if let Some(spec) = self.screen_spec() {
            let key = spec.key.clone();
            list.push(Subscription::run_with(spec, screen::run).with(key).map(|(key, update)| Msg::ScreenUpdate(key, update)));
        }
        Subscription::batch(list)
    }

    /// Screen sharing exists only inside a connected call or a joined room.
    /// Drops someone's shared picture, including a frame still being uploaded for them.
    fn forget_screen(&mut self, id: &str) {
        self.screen_views.remove(id);
        self.screen_allocations.remove(id);
        if self.screen_uploading.remove(id).is_some() { self.screen_ended.insert(id.to_owned()); }
    }
    fn screen_spec(&self) -> Option<screen::Spec> {
        let session = self.session.clone()?;
        let socket = self.socket.clone()?;
        if let Some(call) = self.call.as_ref().filter(|c| c.status == CallStatus::Connected) {
            return Some(screen::Spec { key: call.spec.id.clone(), scope: screen::Scope::Call { peer: call.spec.peer.clone(), id: call.spec.id.clone() }, session, socket, output: self.settings.output.clone(), relay_only: self.settings.relay_only });
        }
        let room = self.room.as_ref()?;
        let spec = room.spec.as_ref()?;
        Some(screen::Spec { key: spec.session_id.clone(), scope: screen::Scope::Room { community: spec.community.clone(), channel: spec.channel.clone(), session: spec.session_id.clone() }, session, socket, output: self.settings.output.clone(), relay_only: self.settings.relay_only })
    }

    fn sync_screen(&mut self) {
        if self.screen_spec().is_none() {
            self.tv_available.clear(); self.screen_controls = None; self.screen_sharing = None; self.screen_views.clear(); self.screen_allocations.clear(); self.screen_ended.extend(self.screen_uploading.drain().map(|(id, _)| id)); self.screen_expanded = None; self.screen_picker = None; self.screen_targets.clear();
            return;
        }
        let Some(controls) = self.screen_controls.clone() else { return };
        let targets: Vec<screen::Target> = if let Some(call) = self.call.as_ref().filter(|c| c.status == CallStatus::Connected) {
            vec![screen::Target { user: call.spec.peer.clone(), session: String::new() }]
        } else if let Some(room) = self.room.as_ref() {
            let mine = room.spec.as_ref().map(|s| s.session_id.clone()).unwrap_or_default();
            room.participants.iter().filter(|p| p.session_id != mine).map(|p| screen::Target { user: p.user_id.clone(), session: p.session_id.clone() }).collect()
        } else { Vec::new() };
        if targets != self.screen_targets { self.screen_targets = targets.clone(); controls.send(screen::Command::Targets(targets)); }
        let deafened = self.call.as_ref().is_some_and(|c| c.deafened) || self.room.as_ref().is_some_and(|r| r.deafened);
        if deafened != self.screen_deafened { self.screen_deafened = deafened; controls.send(screen::Command::Deafen(deafened)); }
    }

    /// The word shown for someone, or `here` when they are simply online.
    pub fn status_text<'a>(&'a self, id: &str, here: &'a str) -> &'a str {
        if !self.online.contains(id) { return "Offline"; }
        match self.presence.get(id).map(String::as_str) { Some("away") => "Away", Some("busy") => "Busy", Some("room") => "In a voice room", _ => here }
    }
    pub fn avatar_dot(&self, id: &str) -> Option<iced::Color> { self.online.contains(id).then(|| self.status_color(id)) }
    pub fn status_color(&self, id: &str) -> iced::Color { theme::presence_color(self.presence.get(id).map_or("online", String::as_str)) }

    pub fn screen_name(&self, id: &str) -> String {
        self.room.as_ref().and_then(|r| r.participants.iter().find(|p| p.user_id == id)).map(|p| p.name.clone()).filter(|n| !n.is_empty())
            .or_else(|| self.person(id).map(|p| p.name.clone())).unwrap_or_else(|| "Member".into())
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
        self.call = None; self.incoming = None; self.waiting = None; self.early_candidates.clear(); self.room = None; self.rooms.clear();
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
        self.typing.clear();
        self.deferred_preferences = true;
        self.unlock_password.zeroize(); self.auth.password.zeroize(); self.auth.busy = false; self.unlock_confirm = false; self.unlock_reset = false;
        self.live_generation += 1;
        self.refresh()
    }

    fn refresh(&mut self) -> Task<Msg> {
        let Some(session) = self.session.clone() else { return Task::none() };
        let generation = self.generation;
        let other = session.clone();
        Task::batch([
            self.refresh_friends(),
            Task::perform(async move { session.people().await }, move |result| Msg::People(generation, result)),
            Task::perform(async move { other.communities().await }, move |result| Msg::Communities(generation, result)),
        ])
    }

    fn refresh_friends(&self) -> Task<Msg> {
        let Some(session) = self.session.clone() else { return Task::none() };
        Task::perform(async move { session.friend_requests().await }, Msg::FriendRequests)
    }
    fn load_preferences(&self) -> Task<Msg> {
        let Some(session) = self.session.clone() else { return Task::none() };
        Task::perform(async move { session.preferences().await }, Msg::PreferencesLoaded)
    }

    pub fn blocked(&self, id: &str) -> bool { self.preferences.blocked.iter().any(|p| p.id == id) }
    pub fn muted(&self, id: &str) -> bool { self.preferences.muted_communities.iter().any(|c| c == id) }
    pub fn group(&self) -> bool { self.community.as_ref().is_some_and(|c| c.kind == "group") }
    pub fn may(&self, permission: &str) -> bool {
        let me = self.me();
        self.owner() || (self.moderator() && !self.community.as_ref().is_some_and(|c| c.moderator_limits.iter().any(|l| l.user_id == me && l.denied.iter().any(|d| d == permission))))
    }
    pub fn typing_line(&self) -> Option<String> {
        let names: Vec<String> = self.typing.keys().map(|id| {
            let full = if self.mode == Mode::Direct { self.person(id).map(|p| p.name.clone()) } else { self.member(id).map(|p| p.name.clone()) };
            full.and_then(|n| n.split_whitespace().next().map(str::to_owned)).unwrap_or_else(|| "Someone".into())
        }).collect();
        match names.as_slice() {
            [] => None,
            [one] => Some(format!("{one} is typing...")),
            [one, two] => Some(format!("{one} and {two} are typing...")),
            _ => Some("Several people are typing...".into()),
        }
    }
    pub fn typing_query(&self) -> Option<String> {
        let text = self.composer.text();
        let text = text.trim_end_matches('\n');
        let at = text.rfind('@')?;
        if at > 0 && !text[..at].ends_with(char::is_whitespace) { return None; }
        let query = &text[at + 1..];
        (!query.contains(char::is_whitespace)).then(|| query.to_owned())
    }
    fn named_people(&self, text: &str) -> Vec<String> {
        if self.mode != Mode::Community { return Vec::new(); }
        let value = text.to_lowercase();
        let mut found: Vec<String> = self.community.iter().flat_map(|c| c.member_details.iter()).filter(|m| value.contains(&format!("@{}", m.name.to_lowercase()))).map(|m| m.id.clone()).collect();
        if value.split_whitespace().any(|word| word.starts_with("@everyone")) { found.push("everyone".into()); }
        found.truncate(32);
        found
    }
    fn alert_worthy(&self, message: &chat::Message, can_everyone: bool) -> bool {
        let text = message.text.to_lowercase();
        message.mentions.iter().any(|m| m == self.me()) || (can_everyone && message.mentions.iter().any(|m| m == "everyone"))
            || self.settings.keywords.iter().any(|word| !word.is_empty() && text.contains(&word.to_lowercase()))
    }
    fn channel_alert(&mut self, community_id: &str, channel_id: &str) {
        let unread = format!("{community_id}:{channel_id}");
        *self.unread_counts.entry(unread.clone()).or_default() += 1;
        self.unread_channels.insert(unread);
        let place = self.communities.iter().find(|c| c.id == community_id).map_or_else(|| "a server".to_owned(), |c| c.name.clone());
        self.alert_message(format!("New message in {place}"));
    }
    fn send_edit(&mut self, id: String) -> Task<Msg> {
        let (Some(session), Some(key)) = (self.session.clone(), self.key.clone()) else { return Task::none() };
        let Some(original) = self.thread.messages.iter().find(|m| m.id == id).cloned() else { return Task::none() };
        let text = self.composer.text().trim().to_owned();
        if text.is_empty() { return Task::none(); }
        let named = self.named_people(&text);
        let Ok(plain) = message_content::encode_text_with_mentions(&text, original.reply_to.as_deref(), &named) else { self.fail("Messages can be up to 4000 characters.".into()); return Task::none() };
        let plain = Zeroizing::new(plain);
        let (generation, me) = (self.generation, self.me().to_owned());
        self.sending = true;
        let finish = move |stored: api::MessageEnvelope, text: String, named: Vec<String>| { let mut message = local(stored, text, original.reply_to.clone(), None); message.mentions = named; message };
        match self.mode {
            Mode::Direct => {
                let Some(peer) = self.selected_peer().cloned() else { self.sending = false; return Task::none() };
                let Some(peer_key) = peer.encryption_key.clone() else { self.sending = false; return Task::none() };
                Task::perform(async move {
                    let context = crypto::context(&me, &peer.id, None).map_err(|e| e.to_string())?;
                    let envelope = crypto::encrypt(&key, &peer_key, &plain, &context).map_err(|e| e.to_string())?;
                    Ok(finish(session.edit_direct(&id, envelope).await?, text, named))
                }, move |r| Msg::Edited(generation, r))
            }
            Mode::Community => {
                let (Some(community_id), Some(channel)) = (self.community.as_ref().map(|c| c.id.clone()), self.channel.clone()) else { self.sending = false; return Task::none() };
                Task::perform(async move {
                    let community = session.community(&community_id).await?;
                    let mut envelopes = Vec::with_capacity(community.member_details.len());
                    for member in &community.member_details {
                        let member_key = member.encryption_key.as_deref().ok_or("A member has not set up encryption yet.")?;
                        let context = crypto::context(&me, &member.id, Some(&channel)).map_err(|e| e.to_string())?;
                        let envelope = crypto::encrypt(&key, member_key, &plain, &context).map_err(|e| e.to_string())?;
                        envelopes.push(serde_json::json!({ "recipientId": member.id, "version": 1, "iv": envelope.iv, "ciphertext": envelope.ciphertext }));
                    }
                    Ok(finish(session.edit_channel(&community_id, &channel, &id, community.revision, envelopes).await?, text, named))
                }, move |r| Msg::Edited(generation, r))
            }
        }
    }
    fn open_notes(&self) -> Task<Msg> {
        let Some(key) = self.key.clone() else { return Task::none() };
        let me = self.me().to_owned();
        let mut tasks = Vec::new();
        for person in &self.friend_requests.incoming {
            if self.friend_notes.contains_key(&person.id) { continue; }
            let (Some(note), Some(public)) = (&person.note, &person.encryption_key) else { continue };
            let Ok(context) = crypto::context(&person.id, &me, Some(FRIEND_NOTE_SCOPE)) else { continue };
            if let Ok(text) = crypto::decrypt(&key, public, note, &context) { tasks.push(Task::done(Msg::NoteOpened(person.id.clone(), text.to_string()))); }
        }
        Task::batch(tasks)
    }

    fn refresh_people(&self) -> Task<Msg> {
        let Some(session) = self.session.clone() else { return Task::none() };
        let generation = self.generation;
        Task::batch([self.refresh_friends(), Task::perform(async move { session.people().await }, move |result| Msg::People(generation, result))])
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

    fn typing_event(&mut self, value: &serde_json::Value) {
        if self.preferences.typing_hidden { return; }
        let Some(from) = value.get("from").and_then(|v| v.as_str()) else { return };
        if self.blocked(from) { return; }
        let community = value.get("communityId").and_then(|v| v.as_str());
        let channel = value.get("channelId").and_then(|v| v.as_str());
        let here = match self.mode {
            Mode::Direct => community.is_none() && self.peer.as_deref() == Some(from),
            Mode::Community => community == self.community.as_ref().map(|c| c.id.as_str()) && community.is_some() && channel == self.channel.as_deref(),
        };
        if here { self.typing.insert(from.to_owned(), std::time::Instant::now() + std::time::Duration::from_secs(4)); }
    }

    fn receive_direct(&mut self, value: serde_json::Value, updated: bool) {
        let Ok(envelope) = serde_json::from_value::<api::MessageEnvelope>(value) else { return };
        let me = self.me().to_owned();
        let peer_id = if envelope.sender_id == me { envelope.receiver_id.clone() } else { envelope.sender_id.clone() };
        let active = self.mode == Mode::Direct && self.peer.as_deref() == Some(&peer_id);
        if !updated && envelope.sender_id != me && !(active && taskbar::focused()) {
            let opened = self.key.clone().zip(self.person(&peer_id).cloned()).map(|(key, peer)| chat::open(&key, peer.encryption_key.as_deref(), &envelope, true));
            // Reactions are messages too, but nobody needs an alert for one.
            if !opened.is_some_and(|m| m.reaction.is_some()) {
                self.unread_direct.insert(peer_id.clone());
                *self.unread_counts.entry(peer_id.clone()).or_default() += 1;
                let name = self.person(&peer_id).map_or_else(|| "someone".to_owned(), |p| p.name.clone());
                self.alert_message(format!("New message from {name}"));
            }
        }
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
        if new && envelope.sender_id != self.me() && !self.blocked(&envelope.sender_id) && !(active && taskbar::focused()) {
            let sender_key = self.member(&envelope.sender_id).and_then(|p| p.encryption_key.clone());
            let opened = self.key.clone().filter(|_| sender_key.is_some()).map(|key| chat::open(&key, sender_key.as_deref(), &envelope, true));
            if !opened.as_ref().is_some_and(|m| m.reaction.is_some()) {
                if self.muted(&community_id) {
                    // A muted server stays silent unless you were named or a keyword of yours appears.
                    match &opened {
                        Some(message) => {
                            let can_everyone = self.community.as_ref().is_some_and(|c| c.owner_id.as_deref() == Some(envelope.sender_id.as_str()) || c.moderators.contains(&envelope.sender_id));
                            if self.alert_worthy(message, can_everyone) { self.channel_alert(&community_id, &channel_id); }
                        }
                        None => if let Some(session) = self.session.clone() {
                            let (id, boxed) = (community_id.clone(), Box::new(envelope.clone()));
                            return Task::perform(async move { session.community(&id).await }, move |result| Msg::MutedAlert(boxed.clone(), result));
                        },
                    }
                } else { self.channel_alert(&community_id, &channel_id); }
            }
        }
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

    fn step(&mut self, message: Msg) -> Task<Msg> {
        // A call that rang but was never answered becomes a message in the chat.
        let unanswered = self.call.as_ref().filter(|c| c.rang && c.status == CallStatus::Calling).map(|c| (c.spec.id.clone(), c.spec.peer.clone()));
        #[cfg(windows)]
        set_taskbar_icon();
        let mut task = self.update(message);
        if std::mem::take(&mut self.deferred_preferences) { task = Task::batch([task, self.load_preferences()]); }
        #[cfg(windows)]
        if self.hidden && self.incoming.is_some() { self.hidden = false; task = Task::batch([task, show_window()]); }
        self.sync_ringtone();
        self.sync_unread();
        self.sync_screen();
        if let Some((id, peer)) = unanswered {
            if self.call.as_ref().map(|c| &c.spec.id) != Some(&id) { return Task::batch([task, self.send_missed_call(peer), self.load_invite_previews()]); }
        }
        Task::batch([task, self.load_invite_previews()])
    }

    /// Best effort and silent: a failure here must not disturb the end of a call.
    fn send_missed_call(&mut self, peer_id: String) -> Task<Msg> {
        let (Some(session), Some(key)) = (self.session.clone(), self.key.clone()) else { return Task::none() };
        let Some(peer_key) = self.person(&peer_id).and_then(|p| p.encryption_key.clone()) else { return Task::none() };
        if !self.trust.remember(&peer_id, &peer_key) { return Task::none(); }
        let me = self.me().to_owned();
        let target = peer_id.clone();
        Task::perform(async move {
            let context = crypto::context(&me, &target, None).map_err(|e| e.to_string())?;
            let plain = Zeroizing::new(message_content::encode_missed_call());
            let envelope = crypto::encrypt(&key, &peer_key, &plain, &context).map_err(|e| e.to_string())?;
            let stored = session.send_direct(&target, envelope).await?;
            let mut message = local(stored, message_content::MISSED_CALL_TEXT.into(), None, None);
            message.missed_call = true;
            Ok::<_, String>(message)
        }, move |result| Msg::MissedCallSent(peer_id.clone(), result))
    }

    /// Rings exactly while a call is waiting to be answered, however it ended.
    /// Sound, a notice and a flashing taskbar button. Busy means no sound.
    fn alert_message(&mut self, text: String) {
        let focused = taskbar::focused();
        if focused { self.toast(ToastKind::Plain, text); } else { taskbar::flash(); }
        let busy = self.session.as_ref().is_some_and(|s| s.account.presence == "busy");
        let due = self.last_chime.is_none_or(|at| at.elapsed() >= std::time::Duration::from_millis(1200));
        if busy || !due || (cfg!(debug_assertions) && std::env::var_os("SONAR_DEV_SILENT_AUDIO").is_some()) { return; }
        self.last_chime = Some(std::time::Instant::now());
        let volume = self.settings.alert_volume.unwrap_or(0.8);
        audio::play_message(self.settings.output.clone(), if self.call.is_some() || self.room.is_some() { volume * 0.375 } else { volume });
    }

    /// Keeps the taskbar number equal to what is still unread.
    fn sync_unread(&mut self) {
        if taskbar::focused() {
            match self.mode {
                Mode::Direct => if let Some(peer) = &self.peer { self.unread_direct.remove(peer); },
                Mode::Community => if let (Some(community), Some(channel)) = (&self.community, &self.channel) { self.unread_channels.remove(&format!("{}:{channel}", community.id)); },
            }
        }
        let (direct, channels) = (&self.unread_direct, &self.unread_channels);
        self.unread_counts.retain(|key, _| direct.contains(key) || channels.contains(key));
        let total = direct.iter().chain(channels.iter()).map(|key| self.unread_counts.get(key).copied().unwrap_or(1)).sum();
        if total != self.shown_badge && taskbar::set_badge(total) { self.shown_badge = total; }
        #[cfg(windows)]
        if total != self.shown_tip { tray::set_unread(total); self.shown_tip = total; }
    }

    fn sync_ringtone(&mut self) {
        // When the current call ends, a waiting call becomes a normal incoming one.
        if self.call.is_none() && self.incoming.is_none() { self.incoming = self.waiting.take(); }
        // A second call rings softly so it never drowns out the conversation.
        let level = if self.incoming.is_some() && self.room.is_none() { 1.0 } else if self.waiting.is_some() || self.incoming.is_some() { 0.35 } else { 0.0 };
        let level = level * (self.settings.alert_volume.unwrap_or(0.8) / 0.8).min(1.25);
        if level == self.ringtone_level { return; }
        self.ringtone_level = level;
        self.ringtone = (level > 0.0).then(|| {
            if cfg!(debug_assertions) && std::env::var_os("SONAR_DEV_SILENT_AUDIO").is_some() { audio::Ringtone::silent() }
            else { audio::Ringtone::start(self.settings.output.clone(), level) }
        });
    }

    /// Looks up the invite cards for any invite links newly in the conversation.
    fn load_invite_previews(&mut self) -> Task<Msg> {
        let scan = (self.thread.messages.len(), self.thread.messages.last().map(|m| m.id.clone()).unwrap_or_default());
        if scan == self.invite_scan { return Task::none(); }
        self.invite_scan = scan;
        let Some(session) = self.session.clone() else { return Task::none() };
        let mut tasks = Vec::new();
        for message in &self.thread.messages {
            let Some((token, _)) = api::find_invite(&message.text) else { continue };
            if self.invite_previews.contains_key(&token) { continue; }
            self.invite_previews.insert(token.clone(), None);
            let session = session.clone();
            tasks.push(Task::perform(async move { let result = session.invite_preview(&token).await; (token, result) }, |(token, result)| Msg::InvitePreviewReady(token, result)));
        }
        Task::batch(tasks)
    }

    fn update(&mut self, message: Msg) -> Task<Msg> {
        // The community menu closes once anything in it, or anywhere you go next, is chosen.
        if matches!(message, Msg::OpenDialog(_) | Msg::AskConfirm(_) | Msg::ToggleMute(_) | Msg::RoomLock(..) | Msg::OpenChannel(_) | Msg::SetMode(_) | Msg::OpenCommunity(_) | Msg::OpenGroup(_) | Msg::ToggleMembers) { self.manage_open = false; }
        match message {
            Msg::Tick => {
                self.now = chat::now();
                let now = std::time::Instant::now();
                self.toasts.retain(|toast| toast.until > now);
                let current = self.now;
                self.typing.retain(|_, until| *until > now);
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
            Msg::PreferencesLoaded(result) => if let Ok(preferences) = result { self.preferences = preferences; if self.preferences.typing_hidden { self.typing.clear(); } },
            Msg::SavePreferences(body) => {
                let Some(session) = self.session.clone() else { return Task::none() };
                return Task::perform(async move { session.save_preferences(body).await }, Msg::PreferencesLoaded);
            }
            Msg::ToggleMute(id) => {
                let mut list = self.preferences.muted_communities.clone();
                if list.contains(&id) { list.retain(|item| item != &id); } else { list.push(id); }
                self.preferences.muted_communities = list.clone();
                return self.update(Msg::SavePreferences(serde_json::json!({ "mutedCommunities": list })));
            }
            Msg::TogglePinPerson(id) => {
                let mut list = self.preferences.pinned_people.clone();
                if list.contains(&id) { list.retain(|item| item != &id); } else { list.push(id); }
                self.preferences.pinned_people = list.clone();
                return self.update(Msg::SavePreferences(serde_json::json!({ "pinnedPeople": list })));
            }
            Msg::FolderName(value) => self.folder_name = value.chars().take(30).collect(),
            Msg::MoveToFolder(person, name) => {
                let mut folders = self.preferences.folders.clone();
                for folder in &mut folders { folder.people.retain(|id| id != &person); }
                let name = if name == crate::ui::NEW_FOLDER { self.folder_name.trim().to_owned() } else if name == crate::ui::NO_FOLDER { String::new() } else { name };
                if !name.is_empty() {
                    if let Some(folder) = folders.iter_mut().find(|f| f.name == name) { folder.people.push(person); }
                    else { folders.push(api::Folder { name, people: vec![person] }); }
                }
                folders.retain(|f| !f.people.is_empty());
                self.folder_name.clear();
                self.preferences.folders = folders.clone();
                let body: Vec<_> = folders.iter().map(|f| serde_json::json!({ "name": f.name, "people": f.people })).collect();
                return self.update(Msg::SavePreferences(serde_json::json!({ "folders": body })));
            }
            Msg::Unblock(id) => {
                let Some(session) = self.session.clone() else { return Task::none() };
                return Task::perform(async move { session.unblock(&id).await }, Msg::PreferencesLoaded);
            }
            Msg::SetAnimal(animal) => {
                let Some(session) = self.session.clone() else { return Task::none() };
                let name = session.account.full_name.clone();
                return Task::perform(async move { session.set_animal(&name, animal).await }, Msg::AnimalSaved);
            }
            Msg::AnimalSaved(result) => match result {
                Ok(account) => if let Some(session) = &mut self.session { session.account.animal = account.animal; },
                Err(error) => self.fail(error),
            },
            Msg::StartEdit(id) => {
                if let Some(message) = self.thread.messages.iter().find(|m| m.id == id) {
                    self.composer = text_editor::Content::with_text(&message.text);
                    self.editing = Some(id);
                    self.reply_to = None;
                }
            }
            Msg::CancelEdit => { self.editing = None; self.composer = text_editor::Content::new(); }
            Msg::Edited(generation, result) => {
                self.sending = false;
                match result {
                    Ok(message) if generation == self.generation => {
                        self.editing = None;
                        self.composer = text_editor::Content::new();
                        chat::merge(&mut self.thread.messages, [message]);
                    }
                    Ok(_) => {}
                    Err(error) => self.fail(error),
                }
            }
            Msg::Disappear(seconds) => {
                let Some(session) = self.session.clone() else { return Task::none() };
                match self.mode {
                    Mode::Direct => {
                        let Some(peer) = self.peer.clone() else { return Task::none() };
                        return Task::perform(async move { session.disappear_direct(&peer, seconds).await }, Msg::Finished);
                    }
                    Mode::Community => {
                        let (Some(community), Some(channel)) = (self.community.as_ref().map(|c| c.id.clone()), self.channel.clone()) else { return Task::none() };
                        return Task::perform(async move { session.disappear_channel(&community, &channel, seconds).await }, Msg::Finished);
                    }
                }
            }
            Msg::Finished(result) => if let Err(error) = result { self.fail(error); },
            Msg::SearchQuery(value) => self.search_query = value.chars().take(200).collect(),
            Msg::SearchRun if !self.search_busy => {
                let (Some(session), Some(key)) = (self.session.clone(), self.key.clone()) else { return Task::none() };
                let needle = self.search_query.trim().to_lowercase();
                if needle.is_empty() { return Task::none(); }
                self.search_busy = true;
                self.search_results = None;
                let peer = self.selected_peer().cloned();
                let place = (self.community.clone(), self.channel.clone());
                let mode = self.mode;
                // Runs on this device: pages of history are decrypted and looked through in memory.
                return Task::perform(async move {
                    let (mut found, mut before): (Vec<chat::Message>, Option<String>) = (Vec::new(), None);
                    for _ in 0..40 {
                        let page = match (mode, &place, &peer) {
                            (Mode::Community, (Some(community), Some(channel)), _) => session.channel_page(&community.id, channel, before.as_deref(), false).await?,
                            (_, _, Some(peer)) => session.direct_page(&peer.id, before.as_deref(), false).await?,
                            _ => return Ok(found),
                        };
                        for envelope in &page.messages {
                            let public = match mode {
                                Mode::Community => place.0.as_ref().and_then(|c| c.member_details.iter().chain(c.history_authors.iter()).find(|p| p.id == envelope.sender_id)).and_then(|p| p.encryption_key.as_deref()),
                                Mode::Direct => peer.as_ref().and_then(|p| p.encryption_key.as_deref()),
                            };
                            let message = chat::open(&key, public, envelope, true);
                            if message.reaction.is_none() && !message.unreadable && message.text.to_lowercase().contains(&needle) { found.push(message); }
                        }
                        if !page.has_more { break; }
                        before = page.next_cursor;
                    }
                    Ok(found)
                }, Msg::SearchDone);
            }
            Msg::SearchDone(result) => {
                self.search_busy = false;
                match result { Ok(found) => self.search_results = Some(found), Err(error) => self.fail(error) }
            }
            Msg::AlertVolume(value) => { self.settings.alert_volume = Some(value); store::save_settings(&self.settings); }
            Msg::ConfirmCalls(value) => { self.settings.confirm_calls = value; store::save_settings(&self.settings); }
            Msg::AskCall => {
                if self.settings.confirm_calls { self.confirm = Some(Confirm { action: "call".into(), member: self.selected_peer().cloned(), channel: None }); }
                else { return self.update(Msg::StartCall); }
            }
            Msg::TypingHidden(value) => { self.preferences.typing_hidden = value; if value { self.typing.clear(); } return self.update(Msg::SavePreferences(serde_json::json!({ "typingHidden": value }))); }
            Msg::KeywordInput(value) => self.keyword_input = value.chars().take(40).collect(),
            Msg::AddKeyword => {
                let word = self.keyword_input.trim().to_owned();
                if !word.is_empty() && !self.settings.keywords.contains(&word) && self.settings.keywords.len() < 30 { self.settings.keywords.push(word); store::save_settings(&self.settings); }
                self.keyword_input.clear();
            }
            Msg::RemoveKeyword(word) => { self.settings.keywords.retain(|item| item != &word); store::save_settings(&self.settings); }
            Msg::FriendNote(value) => self.friend_note = value.chars().take(200).collect(),
            Msg::NoteOpened(id, text) => { self.friend_notes.insert(id, text); }
            Msg::GroupName(value) => self.group_name = value.chars().take(60).collect(),
            Msg::GroupPick(id, on) => { if on && self.group_picks.len() < 9 { self.group_picks.push(id); } else { self.group_picks.retain(|item| item != &id); } }
            Msg::CreateGroup => {
                let Some(session) = self.session.clone() else { return Task::none() };
                let (picks, name) = (self.group_picks.clone(), self.group_name.trim().to_owned());
                return Task::perform(async move { session.create_group(picks, name).await }, Msg::GroupCreated);
            }
            Msg::GroupCreated(result) => match result {
                Ok(group) => { self.panel = None; let id = group.id.clone(); self.communities.push(group); return self.update(Msg::OpenGroup(id)); }
                Err(error) => self.fail(error),
            },
            Msg::OpenGroup(id) => { let first = self.update(Msg::SetMode(Mode::Community)); return Task::batch([first, Task::done(Msg::OpenCommunity(Some(id)))]); }
            Msg::ActivityLoaded(result) => match result { Ok(rows) => self.activity = Some(rows), Err(error) => self.fail(error) },
            Msg::OpenPermissions(member) => {
                self.person_menu = None;
                self.denied = self.community.as_ref().and_then(|c| c.moderator_limits.iter().find(|l| l.user_id == member.id)).map(|l| l.denied.clone()).unwrap_or_default();
                self.permissions_for = Some(member);
                self.dialog = Some(Dialog::Permissions);
            }
            Msg::Deny(permission, allowed) => { if allowed { self.denied.retain(|d| d != permission); } else if !self.denied.iter().any(|d| d == permission) { self.denied.push(permission.to_owned()); } }
            Msg::SavePermissions => {
                let (Some(session), Some(community), Some(member)) = (self.session.clone(), self.community.as_ref().map(|c| c.id.clone()), self.permissions_for.clone()) else { return Task::none() };
                let denied = self.denied.clone();
                return Task::perform(async move { session.set_moderator_limits(&community, &member.id, denied).await }, Msg::PermissionsSaved);
            }
            Msg::PermissionsSaved(result) => match result { Ok(()) => { self.dialog = None; return self.refresh_community(); } Err(error) => self.fail(error) },
            Msg::InsertMention(name) => {
                let text = self.composer.text();
                let text = text.trim_end_matches('\n');
                if let Some(at) = text.rfind('@') { self.composer = text_editor::Content::with_text(&format!("{}@{name} ", &text[..at])); self.composer.perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd)); }
            }
            Msg::MutedAlert(envelope, result) => if let (Ok(community), Some(key)) = (result, self.key.clone()) {
                let sender = community.member_details.iter().find(|p| p.id == envelope.sender_id);
                let message = chat::open(&key, sender.and_then(|p| p.encryption_key.as_deref()), &envelope, true);
                let can_everyone = community.owner_id.as_deref() == Some(envelope.sender_id.as_str()) || community.moderators.contains(&envelope.sender_id);
                if !message.unreadable && message.reaction.is_none() && self.alert_worthy(&message, can_everyone) {
                    if let (Some(a), Some(b)) = (envelope.community_id.clone(), envelope.channel_id.clone()) { self.channel_alert(&a, &b); }
                }
            },
            Msg::CloseRequested(id) => {
                #[cfg(windows)]
                if self.settings.keep_in_tray { self.hidden = true; return iced::window::set_mode(id, iced::window::Mode::Hidden); }
                let _ = id;
                #[cfg(windows)]
                tray::remove();
                return iced::exit();
            }
            #[cfg(windows)]
            Msg::Tray(tray::Action::Open) => { self.hidden = false; return show_window(); }
            #[cfg(windows)]
            Msg::Tray(tray::Action::Quit) => { tray::remove(); return iced::exit(); }
            Msg::KeepInTray(value) => { self.settings.keep_in_tray = value; store::save_settings(&self.settings); }
            Msg::StartWithWindows(value) => {
                #[cfg(windows)]
                if tray::set_start_with_windows(value) { self.settings.start_with_windows = value; store::save_settings(&self.settings); }
            }
            Msg::UpdateCheck => return Task::perform(update::check(), Msg::UpdateChecked),
            Msg::UpdateChecked(result) => if let Ok(Some(version)) = result { self.update_ready = Some(version); },
            Msg::UpdateRestart => {
                if self.call.is_none() && self.room.is_none() {
                    if let Some(version) = self.update_ready.clone() {
                        match update::restart(&version) {
                            Ok(()) => { #[cfg(windows)] tray::remove(); return iced::exit(); }
                            Err(_) => self.toast(ToastKind::Error, "Sonar could not update itself. Download the new version from the website."),
                        }
                    }
                }
            }

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
                6 => self.auth.username = value.trim_start_matches('@').to_lowercase(),
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
                if self.auth.email.trim().is_empty() || self.auth.password.is_empty() || (self.auth.signup && (self.auth.name.trim().is_empty() || self.auth.username.trim().is_empty())) {
                    self.fail("All fields are required".into());
                    return Task::none();
                }
                self.auth.busy = true;
                let password = Zeroizing::new(self.auth.password.clone());
                let (server, email, name, username) = (self.auth.server.clone(), self.auth.email.clone(), self.auth.name.clone(), self.auth.username.clone());
                let signup = self.auth.signup;
                // Kept only until the key is unlocked or created, then wiped.
                self.unlock_password = (*password).clone();
                return Task::perform(async move {
                    if signup { client::signup(server, name, username, email, password).await } else { client::login(server, email, password).await }
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
                    self.incoming = None; self.waiting = None; self.rooms.clear();
                    // Reconnect shortly; the subscription restarts with a new generation.
                    return Task::perform(tokio::time::sleep(std::time::Duration::from_secs(3)), |_| Msg::Reconnect);
                }
                live::Event::Revoked => { store::forget_session(); self.clear_session(); self.fail("You were signed out.".into()); }
                live::Event::Online(ids) => self.online = ids.into_iter().collect(),
                live::Event::Incoming(call) => {
                    let can_wait = self.call.as_ref().is_some_and(|c| c.status == CallStatus::Connected) && self.waiting.is_none();
                    if self.incoming.is_some() || (self.call.is_some() && !can_wait) { return Task::none(); }
                    let mut call = call;
                    call.candidates.extend(self.early_candidates.drain(..).filter(|c| c.id == call.id).map(|c| c.candidate));
                    if can_wait { self.waiting = Some(call); } else { self.incoming = Some(call); }
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
                    } else if let Some(waiting) = self.waiting.as_mut().filter(|c| c.id == candidate.id && c.from == candidate.from) {
                        if waiting.candidates.len() < 128 { waiting.candidates.push(candidate.candidate); }
                    } else if self.early_candidates.len() < 64 { self.early_candidates.push(candidate); }
                }
                live::Event::Screen { from, id, signal } => {
                    if self.call.as_ref().is_some_and(|c| c.spec.id == id && c.spec.peer == from) {
                        if let Some(controls) = &self.screen_controls { controls.send(screen::Command::Signal { from, signal }); }
                    }
                }
                live::Event::CallEnded(id) => {
                    if self.waiting.as_ref().is_some_and(|c| c.id == id) { self.waiting = None; }
                    if self.incoming.as_ref().is_some_and(|c| c.id == id) { self.incoming = None; self.toast(ToastKind::Plain, "Call ended"); }
                    if self.call.as_ref().is_some_and(|c| c.spec.id == id) { self.call = None; self.toast(ToastKind::Plain, "Call ended"); }
                }
                live::Event::MatrixChanged => {}
                live::Event::Data(name, value) => match name {
                    "typing" => self.typing_event(&value),
                    "watch:available" => {
                        self.watch_note = (value.get("active").and_then(|v| v.as_bool()) == Some(true)).then(|| (
                            value.get("title").and_then(|v| v.as_str()).map(|title| title.chars().take(120).collect()),
                            value.get("viewers").and_then(|v| v.as_u64()).unwrap_or(0).min(99),
                        ));
                    }
                    "preferencesChanged" => return self.load_preferences(),
                    "profileChanged" => {
                        let (Some(user), Some(animal)) = (value.get("userId").and_then(|v| v.as_str()).map(str::to_owned), value.get("animal").and_then(|v| v.as_str()).map(str::to_owned)) else { return Task::none() };
                        for person in self.people.iter_mut().filter(|p| p.id == user) { person.animal = Some(animal.clone()); }
                        if let Some(community) = &mut self.community { for person in community.member_details.iter_mut().filter(|p| p.id == user) { person.animal = Some(animal.clone()); } }
                        if let Some(session) = self.session.as_mut().filter(|s| s.account.id.as_deref() == Some(user.as_str())) { session.account.animal = Some(animal); }
                    }
                    "friendsChanged" => return self.refresh_people(),
                    "friendRequest" => { let who = value.get("fullName").and_then(|v| v.as_str()).unwrap_or("Someone").to_owned(); self.toast(ToastKind::Success, format!("{who} sent you a friend request")); }
                    "newMessage" => self.receive_direct(value, false),
                    "messageUpdated" => self.receive_direct(value, true),
                    "messageDeleted" | "channelMessageDeleted" => {
                        if let Some(id) = value.get("id").and_then(|v| v.as_str()) {
                            self.thread.deleted.insert(id.to_owned());
                            self.thread.messages.retain(|m| m.id != id);
                        }
                    }
                    "getPresence" => {
                        self.presence = value.as_object().map(|all| all.iter().filter_map(|(id, status)| Some((id.clone(), status.as_str()?.to_owned()))).collect()).unwrap_or_default();
                    }
                    "ownPresence" => {
                        if let (Some(session), Some(status)) = (self.session.as_mut(), value.get("presence").and_then(|v| v.as_str())) { session.account.presence = status.to_owned(); }
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
                        if signal.get("type").and_then(|t| t.as_str()).is_some_and(|t| t.starts_with("screen-")) {
                            let mine = room.spec.as_ref().is_some_and(|s| s.session_id == field("toSession"));
                            if mine && room.participants.iter().any(|p| p.user_id == from && p.session_id == session) {
                                if let Some(controls) = &self.screen_controls { controls.send(screen::Command::Signal { from, signal }); }
                            }
                            return Task::none();
                        }
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
                                "movedElsewhere" => self.toast(ToastKind::Plain, "You joined this voice room on another device."),
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
                    if !self.people_loaded && people.is_empty() { self.panel = Some(Panel::People); }
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
            Msg::PeopleQuery(value) => self.people_query = value,
            Msg::FriendRequests(result) => { if let Ok(requests) = result { self.friend_requests = requests; return self.open_notes(); } }
            Msg::SendFriendRequest => {
                let username = self.people_query.trim().to_owned();
                let Some(session) = self.session.clone().filter(|_| !username.is_empty()) else { return Task::none() };
                let (note, key, me) = (self.friend_note.trim().to_owned(), self.key.clone(), self.me().to_owned());
                return Task::perform(async move {
                    let mut sealed = None;
                    if !note.is_empty() {
                        let peer = session.friend_key(&username).await?;
                        let public = peer.encryption_key.ok_or("They need to sign in to Sonar once before you can add a note")?;
                        let key = key.ok_or("Your key is not unlocked")?;
                        let context = crypto::context(&me, &peer.id, Some(FRIEND_NOTE_SCOPE)).map_err(|e| e.to_string())?;
                        sealed = Some(crypto::encrypt(&key, &public, &note, &context).map_err(|e| e.to_string())?);
                    }
                    session.send_friend_request(&username, sealed).await
                }, Msg::FriendRequestSent);
            }
            Msg::FriendRequestSent(result) => match result {
                Ok(()) => { self.people_query.clear(); self.friend_note.clear(); self.toast(ToastKind::Success, "Friend request sent"); return self.refresh_people(); }
                Err(error) => self.fail(error),
            },
            Msg::AnswerFriendRequest(id, accept) => {
                let Some(session) = self.session.clone() else { return Task::none() };
                return Task::perform(async move { session.answer_friend_request(&id, accept).await }, Msg::FriendsChanged);
            }
            Msg::FriendsChanged(result) => { if let Err(error) = result { self.fail(error); } return self.refresh_people(); }
            Msg::ProfileUsername(value) => self.profile_username = value.trim_start_matches('@').to_lowercase(),
            Msg::MicPoll => {
                let now = std::time::Instant::now();
                if voice_audio::MIC_LEVEL.load(std::sync::atomic::Ordering::Relaxed) > 10_000 { self.mic_until = Some(now + std::time::Duration::from_millis(300)); }
                self.mic_active = self.mic_until.is_some_and(|until| now < until);
            }
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
            Msg::OpenPanel(panel) => { self.panel = panel; self.people_query.clear(); self.search_results = None; self.search_query.clear(); if panel == Some(Panel::Group) { self.group_picks.clear(); self.group_name.clear(); } }
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
                let typing = action.is_edit();
                if !action.is_edit() || self.composer.text().chars().count() < 4000 { self.composer.perform(action); }
                let now = std::time::Instant::now();
                if typing && !self.preferences.typing_hidden && self.composer.text().trim().len() > 0 && self.last_typing.is_none_or(|at| now.duration_since(at).as_millis() >= 2500) {
                    let target = match self.mode {
                        Mode::Direct => self.peer.clone().map(|to| serde_json::json!({ "to": to })),
                        Mode::Community => self.community.as_ref().map(|c| c.id.clone()).zip(self.channel.clone()).map(|(community, channel)| serde_json::json!({ "communityId": community, "channelId": channel })),
                    };
                    if let (Some(socket), Some(target)) = (self.socket.clone(), target) {
                        self.last_typing = Some(now);
                        return Task::perform(async move { socket.notify("typing", target).await }, |_| Msg::Noop);
                    }
                }
            }
            Msg::Send => {
                if let Some(id) = self.editing.clone() { return self.send_edit(id); }
                return self.send(None);
            }
            Msg::React(target, emoji, selected) => {
                self.picker = None;
                return self.send(Some(chat::Reaction { target, emoji, action: if selected { "remove".into() } else { "add".into() } }));
            }
            Msg::Sent(generation, result) => {
                self.sending = false;
                match result {
                    Ok(message) if generation == self.generation => {
                        if message.reaction.is_none() && message.file.is_none() {
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
                let attachment = self.thread.messages.iter().find(|m| m.id == id).and_then(|m| m.file.as_ref().map(|f| f.id.clone()));
                return Task::perform(async move {
                    if let Some(file) = attachment { let _ = session.delete_file(&file).await; }
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
            Msg::ToggleMembers => { self.members_open = !self.members_open; self.person_menu = None; }
            Msg::TogglePersonMenu(id) => self.person_menu = if self.person_menu.as_ref() == Some(&id) { None } else { Some(id) },
            Msg::FriendRemoved(result) => match result {
                Ok(()) => { self.toast(ToastKind::Success, "Removed from your friends"); return self.refresh(); }
                Err(error) => self.fail(error),
            },
            Msg::OpenDialog(dialog) => {
                if self.busy { return Task::none(); }
                self.dialog_value.clear(); self.invite = None; self.bans = None;
                if !matches!(dialog, Some(Dialog::Channel)) { self.dialog_voice = false; }
                let first = self.community.as_ref().and_then(|c| c.channels.first()).cloned();
                self.dialog_channel = first.as_ref().map(|c| c.id.clone());
                if dialog == Some(Dialog::Activity) {
                    self.activity = None;
                    if let (Some(session), Some(community)) = (self.session.clone(), self.community.as_ref().map(|c| c.id.clone())) {
                        self.dialog = dialog;
                        return Task::perform(async move { session.activity(&community).await }, Msg::ActivityLoaded);
                    }
                }
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
            Msg::MissedCallSent(peer, Ok(message)) => {
                if self.mode == Mode::Direct && self.peer.as_ref() == Some(&peer) && !self.thread.pins { chat::merge(&mut self.thread.messages, [message]); }
            }
            Msg::MissedCallSent(_, Err(_)) => {}
            Msg::ScreenUpdate(key, update) => {
                if self.screen_spec().is_none_or(|s| s.key != key) { return Task::none(); }
                match update {
                    screen::Update::Ready(controls) => { self.screen_targets.clear(); self.screen_controls = Some(controls); }
                    screen::Update::Sharing(info) => {
                        // When our TV (or screen) stops, its picture here goes too.
                        if info.is_none() { let me = self.me().to_owned(); self.forget_screen(&me); self.screen_labels.remove(&me); if self.screen_expanded.as_ref() == Some(&me) { self.screen_expanded = None; } }
                        self.screen_sharing = info;
                    }
                    screen::Update::TvAvailable(id, label) => match label {
                        Some(label) => {
                            if self.tv_available.insert(id.clone(), label.clone()).is_none() {
                                let name = self.screen_name(&id);
                                self.toast(ToastKind::Plain, format!("{name} is sharing TV: {label}. Press Join TV to watch."));
                            }
                        }
                        None => { self.tv_available.remove(&id); }
                    },
                    screen::Update::Label(id, label) => match label { Some(label) => { self.screen_labels.insert(id, label); } None => { self.screen_labels.remove(&id); } },
                    screen::Update::Notice(text) => self.toast(ToastKind::Plain, text),
                    screen::Update::SoundFailed => self.toast(ToastKind::Plain, "Sharing without sound: Windows could not capture that app's audio."),
                    screen::Update::ViewerGone(id) => { self.forget_screen(&id); self.screen_labels.remove(&id); if self.screen_expanded.as_ref() == Some(&id) { self.screen_expanded = None; } }
                    screen::Update::Pictures => if let Some(controls) = &self.screen_controls {
                        // Upload each new frame first and show it only once it is on the GPU; until then the last frame stays.
                        // A frame still uploading holds back newer ones for that person (they stay in the mailbox, newest wins).
                        let mut uploads = Vec::new();
                        if let Ok(mut slot) = controls.pictures.lock() {
                            // An upload iced never answers (no window drawing) is given up after a second.
                            let ready: Vec<String> = slot.keys().filter(|id| self.screen_uploading.get(*id).is_none_or(|since| since.elapsed() > std::time::Duration::from_secs(1))).cloned().collect();
                            for id in ready {
                                let Some(picture) = slot.remove(&id) else { continue };
                                let (width, height) = (picture.width, picture.height);
                                let handle = iced::widget::image::Handle::from_rgba(width, height, bytes::Bytes::from(picture.rgba));
                                self.screen_uploading.insert(id.clone(), std::time::Instant::now());
                                uploads.push(iced_runtime::image::allocate(handle).map(move |result| Msg::ScreenUploaded(id.clone(), width, height, result)));
                            }
                        }
                        return Task::batch(uploads);
                    },
                }
            }
            Msg::ToggleStatusMenu => self.status_open = !self.status_open,
            Msg::ToggleEmoji => self.emoji_open = !self.emoji_open,
            Msg::ToggleManage => self.manage_open = !self.manage_open,
            Msg::Pulse => { theme::PULSE.store(STARTED.get_or_init(std::time::Instant::now).elapsed().as_millis() as u32, std::sync::atomic::Ordering::Relaxed); }
            Msg::PickEmoji(emoji) => self.composer.perform(text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(emoji.to_owned())))),
            Msg::AttachFile if !self.uploading => {
                let Some(target) = self.upload_target() else { return Task::none() };
                let Some(session) = self.session.clone() else { return Task::none() };
                self.uploading = true;
                let generation = self.generation;
                return Task::perform(async move {
                    let path = tokio::task::spawn_blocking(native_file::open).await.map_err(|_| "Could not open the file picker".to_string())??;
                    let Some(path) = path else { return Ok(None) };
                    upload(session, target, path).await.map(Some)
                }, move |result| Msg::FileReady(generation, result));
            }
            Msg::FileReady(generation, result) => {
                self.uploading = false;
                match result {
                    Ok(Some(file)) if generation == self.generation => { self.pending_file = Some(file); return self.send(None); }
                    Ok(_) => {}
                    Err(error) => self.fail(error),
                }
            }
            Msg::DownloadFile(file) => {
                let Some(session) = self.session.clone() else { return Task::none() };
                return Task::perform(async move {
                    let suggested = safe_file_name(&file.name);
                    let path = tokio::task::spawn_blocking(move || native_file::save(&suggested)).await.map_err(|_| "Could not open the save dialog".to_string())??;
                    let Some(path) = path else { return Ok(()) };
                    save_file(session, file, path).await
                }, |result| Msg::Done(result, "File saved"));
            }
            Msg::SwapCall => {
                // Swapping from a voice room leaves the room; the call is already ringing.
                if self.room.is_some() && self.incoming.is_some() {
                    let left = self.update(Msg::LeaveRoom);
                    return Task::batch([left, self.update(Msg::AcceptCall)]);
                }
                let Some(next) = self.waiting.take() else { return Task::none() };
                if let Some(state) = self.call.take() { if let Some(controls) = &state.controls { controls.send(call::Command::Hangup); } }
                self.incoming = Some(next);
                return self.update(Msg::AcceptCall);
            }
            Msg::DeclineWaiting if self.waiting.is_none() => return self.update(Msg::DeclineCall),
            Msg::DeclineWaiting => {
                if let (Some(next), Some(socket)) = (self.waiting.take(), self.socket.clone()) {
                    return Task::perform(async move { let _ = socket.decline(next).await; }, |_| Msg::Noop);
                }
            }
            Msg::SetStatus(status) => {
                self.status_open = false;
                let Some(session) = self.session.as_mut() else { return Task::none() };
                let previous = std::mem::replace(&mut session.account.presence, status.to_owned());
                let session = session.clone();
                return Task::perform(async move { session.set_presence(status).await }, move |result| Msg::StatusSaved(previous.clone(), result));
            }
            Msg::StatusSaved(previous, result) => if let Err(error) = result {
                if let Some(session) = self.session.as_mut() { session.account.presence = previous; }
                self.fail(error);
            },
            Msg::ToggleApps => self.apps_open = !self.apps_open,
            Msg::TvAdding(on) => { self.tv_adding = on; self.tv_error = None; }
            Msg::OpenTv => {
                self.apps_open = false;
                if self.tv_sources.is_empty() { self.tv_sources = tv::sources::load(); }
                self.tv_adding = self.tv_sources.is_empty();
                self.panel = Some(Panel::Tv);
                self.tv_error = None;
                // Opening the panel shows the favorites first, when the source has any.
                self.tv_query.clear();
                self.tv_favorites_only = self.tv_source.as_ref().and_then(|id| self.tv_sources.iter().find(|s| &s.id == id)).is_some_and(|s| !s.favorites.is_empty());
                if self.tv_source.is_none() { if let Some(first) = self.tv_sources.first() { return self.update(Msg::TvSelect(first.id.clone())); } }
            }
            Msg::TvSelect(id) => {
                let Some(source) = self.tv_sources.iter().find(|s| s.id == id).cloned() else { return Task::none() };
                self.tv_source = Some(id.clone());
                self.tv_favorites_only = !source.favorites.is_empty();
                self.tv_channels.clear();
                self.tv_loading = true;
                self.tv_error = None;
                return Task::perform(async move { tv::sources::load_channels(&source).await }, move |result| Msg::TvChannels(id.clone(), result));
            }
            Msg::TvChannels(id, result) => if self.tv_source.as_ref() == Some(&id) {
                self.tv_loading = false;
                match result { Ok(list) => self.tv_channels = list, Err(error) => self.tv_error = Some(error) }
            },
            Msg::TvQuery(value) => self.tv_query = value,
            Msg::TvKind(xtream) => { self.tv_xtream = xtream; self.tv_error = None; }
            Msg::TvName(value) => self.tv_name = value,
            Msg::TvUrl(value) => self.tv_url = value,
            Msg::TvServer(value) => self.tv_server = value,
            Msg::TvUser(value) => self.tv_user = value,
            Msg::TvPass(value) => self.tv_pass = zeroize::Zeroizing::new(value),
            Msg::TvAdd => {
                let name: String = self.tv_name.trim().chars().take(40).collect();
                let kind = if self.tv_xtream {
                    if let Err(error) = tv::sources::xtream_api(&self.tv_server, self.tv_user.trim(), &self.tv_pass, "get_live_streams") { self.tv_error = Some(error.into()); return Task::none(); }
                    tv::sources::SourceKind::Xtream { server: self.tv_server.trim().to_owned(), username: self.tv_user.trim().to_owned(), password: self.tv_pass.to_string() }
                } else {
                    if url::Url::parse(self.tv_url.trim()).ok().filter(|u| matches!(u.scheme(), "http" | "https")).is_none() { self.tv_error = Some("Paste the playlist address from your provider (http:// or https://).".into()); return Task::none(); }
                    tv::sources::SourceKind::M3u { url: self.tv_url.trim().to_owned() }
                };
                let id = format!("{:x}", rand_core::RngCore::next_u64(&mut rand_core::OsRng));
                self.tv_sources.push(tv::sources::Source { id: id.clone(), name: if name.is_empty() { format!("Source {}", self.tv_sources.len() + 1) } else { name }, kind, favorites: Vec::new() });
                if let Err(error) = tv::sources::save(&self.tv_sources) { self.tv_sources.pop(); self.tv_error = Some(error); return Task::none(); }
                self.tv_name.clear(); self.tv_url.clear(); self.tv_server.clear(); self.tv_user.clear(); self.tv_pass = Default::default();
                self.tv_adding = false;
                return self.update(Msg::TvSelect(id));
            }
            Msg::TvRemove(id) => {
                self.tv_sources.retain(|s| s.id != id);
                let _ = tv::sources::save(&self.tv_sources);
                if self.tv_source.as_ref() == Some(&id) { self.tv_source = None; self.tv_channels.clear(); }
                if self.tv_sources.is_empty() { self.tv_adding = true; } else if self.tv_source.is_none() { return self.update(Msg::TvSelect(self.tv_sources[0].id.clone())); }
            }
            Msg::TvFavorite(name) => {
                let Some(source) = self.tv_source.as_ref().and_then(|id| self.tv_sources.iter_mut().find(|s| &s.id == id)) else { return Task::none() };
                let before = source.favorites.clone();
                if let Some(at) = source.favorites.iter().position(|n| *n == name) { source.favorites.remove(at); }
                else if source.favorites.len() < 500 { source.favorites.push(name); }
                let empty = source.favorites.is_empty();
                if let Err(error) = tv::sources::save(&self.tv_sources) {
                    if let Some(source) = self.tv_source.as_ref().and_then(|id| self.tv_sources.iter_mut().find(|s| &s.id == id)) { source.favorites = before; }
                    self.toast(ToastKind::Plain, error);
                } else if empty { self.tv_favorites_only = false; }
            }
            Msg::TvFavoritesOnly(on) => { self.tv_favorites_only = on; if on { self.tv_query.clear(); } }
            Msg::TvJoin(id) => {
                self.tv_available.remove(&id);
                if let Some(controls) = &self.screen_controls { controls.send(screen::Command::JoinTv(id)); }
            }
            Msg::TvLeave(id) => if let Some(controls) = &self.screen_controls { controls.send(screen::Command::LeaveTv(id)); },
            Msg::TvStep(step) => {
                let me = self.me().to_owned();
                let Some(current) = self.screen_labels.get(&me).cloned() else { return Task::none() };
                let favorites = self.tv_source.as_ref().and_then(|id| self.tv_sources.iter().find(|s| &s.id == id)).map(|s| s.favorites.clone()).unwrap_or_default();
                let order = tv::sources::play_order(&self.tv_channels, &favorites);
                if order.is_empty() { return Task::none(); }
                let next = match order.iter().position(|&i| self.tv_channels[i].name == current) {
                    Some(at) => order[(at as i64 + step).rem_euclid(order.len() as i64) as usize],
                    None => order[0],
                };
                return self.update(Msg::TvPlay(next, self.tv_shared));
            }
            Msg::TvPlay(index, share) => {
                let Some(channel) = self.tv_channels.get(index).cloned() else { return Task::none() };
                let Some(controls) = &self.screen_controls else { self.toast(ToastKind::Plain, "Join a call or voice room to watch TV here."); return Task::none(); };
                controls.send(screen::Command::StartTv(screen::TvChoice { address: zeroize::Zeroizing::new(channel.url().to_owned()), label: channel.name.clone(), share }));
                self.tv_shared = share;
                let me = self.me().to_owned();
                self.screen_labels.insert(me, channel.name.clone());
                self.panel = None;
            }
            Msg::OpenWatch => {
                self.apps_open = false;
                let Some(session) = self.session.clone() else { return Task::none() };
                return Task::perform(async move {
                    let path = session.watch_link().await?;
                    let url = watch::watch_url(&session.origin(), &path).ok_or("Sonar sent an unexpected watch link.")?;
                    tokio::task::spawn_blocking(move || watch::open_in_browser(&url)).await.map_err(|_| "Could not open your browser.".to_string())?
                }, Msg::WatchOpened);
            }
            Msg::WatchOpened(result) => match result {
                Ok(()) => self.toast(ToastKind::Success, "Watch together opened in your browser"),
                Err(error) => self.fail(error),
            },
            Msg::ScreenOpenPicker => return Task::perform(async { tokio::task::spawn_blocking(|| (screen::list_shareable(), screen_media::has_hardware_encoder())).await.unwrap_or_default() }, |(list, hardware)| Msg::ScreenSources(list, hardware)),
            Msg::ScreenSources(sources, hardware) => {
                if sources.is_empty() { self.toast(ToastKind::Plain, "Nothing to share was found."); return Task::none(); }
                self.screen_picker = Some(ScreenPicker { sources, selected: 0, height: 1080, hardware });
            }
            Msg::ScreenClosePicker => self.screen_picker = None,
            Msg::ScreenSelect(index) => if let Some(p) = &mut self.screen_picker { p.selected = index; p.height = 1080; },
            Msg::ScreenHeight(height) => if let Some(p) = &mut self.screen_picker { p.height = height; },
            Msg::ScreenStart => if let (Some(p), Some(controls)) = (self.screen_picker.take(), &self.screen_controls) {
                if let Some(source) = p.sources.get(p.selected) { controls.send(screen::Command::Start(screen::Choice { source: source.source, sound: source.sound, height: p.height })); }
            },
            Msg::ScreenStop => { self.apps_open = false; if let Some(controls) = &self.screen_controls { controls.send(screen::Command::Stop); } }
            Msg::ScreenExpand(id) => self.screen_expanded = id,
            Msg::ScreenUploaded(id, width, height, result) => {
                self.screen_uploading.remove(&id);
                // A share that ended while its frame was uploading stays gone.
                let ended = self.screen_ended.remove(&id);
                if let (Ok(allocation), false) = (result, ended || self.screen_controls.is_none()) {
                    self.screen_views.insert(id.clone(), (allocation.handle().clone(), width, height));
                    self.screen_allocations.insert(id.clone(), allocation);
                }
                if let Some(controls) = &self.screen_controls { if controls.pictures.lock().is_ok_and(|slot| slot.contains_key(&id)) { if let Some(key) = self.screen_spec().map(|s| s.key.clone()) { return self.update(Msg::ScreenUpdate(key, screen::Update::Pictures)); } } }
            }
            Msg::ScreenVolume(id, volume) => {
                if let Some(controls) = &self.screen_controls { controls.send(screen::Command::Volume { from: id.clone(), volume }); }
                self.screen_volumes.insert(id, volume);
            }
            Msg::InvitePreviewReady(token, result) => { self.invite_previews.insert(token, Some(result)); }
            Msg::JoinInvite(token) if !self.busy => {
                let Some(session) = self.session.clone() else { return Task::none() };
                self.busy = true;
                return Task::perform(async move { session.join(&token).await.map(|c| Some(c.id)) }, Msg::DialogDone);
            }
            Msg::DialogDone(result) => {
                self.busy = false;
                match result {
                    Ok(open) => {
                        self.dialog = None;
                        self.invite_previews.clear();
                        self.invite_scan = Default::default();
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
            Msg::ApplyConfirm if self.confirm.as_ref().is_some_and(|c| c.action == "call") => { self.confirm = None; return self.update(Msg::StartCall); }
            Msg::ApplyConfirm if self.confirm.as_ref().is_some_and(|c| c.action == "block") => {
                let (Some(session), Some(member)) = (self.session.clone(), self.confirm.take().and_then(|c| c.member)) else { return Task::none() };
                return Task::perform(async move { session.block(&member.id).await }, Msg::PreferencesLoaded);
            }
            Msg::ApplyConfirm if self.confirm.as_ref().is_some_and(|c| c.action == "unfriend") => {
                let (Some(session), Some(member)) = (self.session.clone(), self.confirm.take().and_then(|c| c.member)) else { return Task::none() };
                return Task::perform(async move { session.remove_friend(&member.id).await }, Msg::FriendRemoved);
            }
            Msg::AskConfirm(confirm) => { self.person_menu = None; if !self.busy { self.confirm = confirm; if self.confirm.is_some() { self.dialog = None; } } }
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
                    self.profile_username = self.session.as_ref().map(|s| s.account.username.clone()).unwrap_or_default();
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
                    Tab::Profile | Tab::Alerts => {}
                }
            }
            Msg::ProfileName(value) => self.profile_name = value,
            Msg::SaveProfile if !self.busy => {
                let Some(session) = self.session.clone() else { return Task::none() };
                let name = self.profile_name.trim().to_owned();
                if name.is_empty() || name.chars().count() > 80 || self.profile_username.is_empty() { return Task::none(); }
                let username = self.profile_username.clone();
                self.busy = true;
                return Task::perform(async move { session.update_profile_details(&name, &username).await }, Msg::ProfileSaved);
            }
            Msg::ProfileSaved(result) => {
                self.busy = false;
                match result {
                    Ok(account) => { if let Some(s) = self.session.as_mut() { s.account.full_name = account.full_name; s.account.username = account.username; } self.toast(ToastKind::Success, "Profile updated successfully"); }
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
            Msg::NoiseSuppression(value) => { self.settings.noise_suppression_off = !value; voice_audio::NOISE_SUPPRESSION.store(value, std::sync::atomic::Ordering::Relaxed); store::save_settings(&self.settings); }
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
                        self.watch_note = None;
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
                self.watch_note = None;
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
                    controls: None, status: CallStatus::Calling, muted: false, deafened: false, started: None, rang: false,
                });
            }
            Msg::AcceptCall if self.call.is_none() && self.room.is_none() => {
                let (Some(session), Some(socket), Some(incoming)) = (self.session.clone(), self.socket.clone(), self.incoming.take()) else { return Task::none() };
                self.call = Some(CallState {
                    spec: call::Spec { id: incoming.id.clone(), peer: incoming.from.clone(), role: call::Role::Incoming(incoming), session, socket, input: self.settings.input.clone(), output: self.settings.output.clone(), relay_only: self.settings.relay_only },
                    controls: None, status: CallStatus::Connecting, muted: false, deafened: false, started: None, rang: false,
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
                    call::Update::Ringing => state.rang = true,
                    call::Update::Connecting => state.status = CallStatus::Connecting,
                    call::Update::Connected => { state.status = CallStatus::Connected; state.started = Some(std::time::Instant::now()); }
                    call::Update::Ended(error) => { self.call = None; self.watch_note = None; if let Some(error) = error { self.fail(error); } }
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
            "joininvite" => self.invite_previews.keys().next().cloned().map(Msg::JoinInvite),
            "wait" => return Task::perform(tokio::time::sleep(std::time::Duration::from_millis(value.parse().unwrap_or(1000))), |_| Msg::Script),
            "shot" => return iced::window::latest().and_then(iced::window::screenshot).map(move |shot| Msg::Screenshot(value.clone(), shot)),
            "peer" => self.people.iter().find(|p| p.name == value).map(|p| Msg::OpenPeer(Some(p.id.clone()))),
            "community" => self.communities.iter().find(|c| c.name == value).map(|c| Msg::OpenCommunity(Some(c.id.clone()))),
            "mode" => Some(Msg::SetMode(if value == "community" { Mode::Community } else { Mode::Direct })),
            "settings" => Some(Msg::OpenSettings(value != "close")),
            "tab" => Some(Msg::SetTab(match value.as_str() { "security" => Tab::Security, "privacy" => Tab::Privacy, "voice" => Tab::Voice, "alerts" => Tab::Alerts, _ => Tab::Profile })),
            "panel" => Some(Msg::OpenPanel(match value.as_str() { "people" => Some(Panel::People), "privacy" => Some(Panel::Privacy), _ => None })),
            "dialog" => Some(Msg::OpenDialog(match value.as_str() { "create" => Some(Dialog::Create), "invite" => Some(Dialog::Invite), "start" => Some(Dialog::Start), _ => None })),
            "members" => Some(Msg::ToggleMembers),
            "tv" => Some(Msg::OpenTv),
            "apps" => Some(Msg::ToggleApps),
            "tvadding" => Some(Msg::TvAdding(value == "on")),
            "tvxtream" => Some(Msg::TvKind(value == "on")),
            "tvadd" => { self.tv_xtream = false; self.tv_name = "Test playlist".into(); self.tv_url = value.clone(); Some(Msg::TvAdd) }
            "tvquery" => Some(Msg::TvQuery(value.clone())),
            "tvfav" => value.parse::<usize>().ok().and_then(|i| self.tv_channels.get(i)).map(|c| Msg::TvFavorite(c.name.clone())),
            "tvnext" => Some(Msg::TvStep(value.parse().unwrap_or(1))),
            "tvjoin" => self.tv_available.keys().next().cloned().map(Msg::TvJoin),
            "tvleave" => self.screen_labels.keys().find(|id| id.as_str() != self.me()).cloned().map(Msg::TvLeave),
            "tvplay" => value.split_once(',').and_then(|(index, share)| Some(Msg::TvPlay(index.parse().ok()?, share == "share"))),
            "dismissrecovery" => { self.recovery_saved = true; Some(Msg::DismissRecovery) }
            "personmenu" => self.people.iter().chain(self.community.iter().flat_map(|c| c.member_details.iter())).find(|p| p.name == value).map(|p| Msg::TogglePersonMenu(p.id.clone())),
            "menu" => Some(Msg::ToggleManage),
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
            #[cfg(debug_assertions)]
            "wheel" => { wheel::test_notches(value.parse().unwrap_or(-3.0)); return next(); }
            #[cfg(debug_assertions)]
            "sharewindow" => {
                if let (Some(window), Some(controls)) = (screen::own_window(), &self.screen_controls) {
                    controls.send(screen::Command::Start(screen::Choice { source: screen_media::Source::Window(window), sound: screen_audio::Sound::OnlyProcess(std::process::id()), height: value.parse().unwrap_or(1080) }));
                }
                None
            }
            "sharestop" => Some(Msg::ScreenStop),
            "swap" => Some(Msg::SwapCall),
            "declinewaiting" => Some(Msg::DeclineWaiting),
            #[cfg(debug_assertions)]
            "sendfile" => {
                let (Some(session), Some(target)) = (self.session.clone(), self.upload_target()) else { return Task::none() };
                let generation = self.generation;
                let path = std::path::PathBuf::from(value.clone());
                return Task::batch([Task::perform(async move { upload(session, target, path).await.map(Some) }, move |result| Msg::FileReady(generation, result)), next()]);
            }
            #[cfg(debug_assertions)]
            "getfile" => {
                let (Some(session), Some(file)) = (self.session.clone(), self.thread.messages.iter().rev().find_map(|m| m.file.clone())) else { return next() };
                let path = std::path::PathBuf::from(value.clone());
                return Task::batch([Task::perform(async move { save_file(session, file, path).await }, |result| Msg::Done(result, "File saved")), next()]);
            }
            "updaterestart" => Some(Msg::UpdateRestart),
            "friendnote" => Some(Msg::FriendNote(value.clone())),
            "panelsearch" => Some(Msg::OpenPanel(Some(Panel::Search))),
            "search" => { self.search_query = value.clone(); Some(Msg::SearchRun) }
            "editlast" => { let me = self.me().to_owned(); self.thread.messages.iter().rev().find(|m| m.sender == me && m.reaction.is_none()).map(|m| Msg::StartEdit(m.id.clone())) }
            "animal" => t_animal(&value).map(Msg::SetAnimal),
            "disappear" => value.parse().ok().map(Msg::Disappear),
            "mute" => self.community.as_ref().map(|c| Msg::ToggleMute(c.id.clone())),
            "friendadd" => { self.people_query = value.clone(); Some(Msg::SendFriendRequest) }
            "friendaccept" => self.friend_requests.incoming.first().map(|p| Msg::AnswerFriendRequest(p.id.clone(), true)),
            "emoji" => Some(Msg::ToggleEmoji),
            "statusmenu" => Some(Msg::ToggleStatusMenu),
            "status" => Some(Msg::SetStatus(match value.as_str() { "away" => "away", "busy" => "busy", "offline" => "offline", _ => "online" })),
            #[cfg(debug_assertions)]
            "screenvolume" => self.screen_views.keys().next().cloned().map(|id| Msg::ScreenVolume(id, value.parse().unwrap_or(0.4))),
            #[cfg(debug_assertions)]
            "pickertest" => {
                let item = |label: &str, monitor: bool, height: u32| screen::Shareable { label: label.into(), source: screen_media::Source::Window(0), sound: screen_audio::Sound::OnlyProcess(0), width: height * 16 / 9, height, monitor };
                Some(Msg::ScreenSources(vec![item("Screen 1 (2560x1440)", true, 1440), item("Screen 2 (1920x1080)", true, 1080), item("Example browser window", false, 900), item("Example game", false, 1440)], true))
            }
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
        format!("{}", serde_json::json!({ "searchResults": self.search_results.as_ref().map(|r| r.iter().map(|m| m.text.clone()).collect::<Vec<_>>()), "typing": self.typing_line(), "animal": self.session.as_ref().and_then(|s| s.account.animal.clone()), "threadText": self.thread.messages.iter().filter(|m| m.reaction.is_none()).map(|m| format!("{}{}", m.text, if m.edited { " (edited)" } else { "" })).collect::<Vec<_>>(), "updateReady": self.update_ready, "shownBadge": self.shown_badge, "waiting": self.waiting.as_ref().map(|c| c.from.clone()), "callPeer": self.call.as_ref().map(|c| c.spec.peer.clone()), "presence": self.presence, "myStatus": self.session.as_ref().map(|s| s.account.presence.clone()), "screenViews": self.screen_views.iter().map(|(k, v)| (k.clone(), format!("{}x{}", v.1, v.2))).collect::<HashMap<_, _>>(), "screenSharing": self.screen_sharing, "ringing": self.ringtone.is_some(), "screen": format!("{:?}", self.screen), "unlock": format!("{:?}", self.unlock_reason), "recoveryKey": self.new_recovery.clone(), "backup": self.backup_kind, "micLevel": self.mic_level, "peer": self.peer.clone(), "online": self.online.len(), "live": self.live_connected, "people": self.people.len(), "passkeys": self.passkeys.as_ref().map(|p| p.len()), "sent": voice_transport::PACKETS_SENT.load(std::sync::atomic::Ordering::Relaxed), "received": voice_transport::PACKETS_RECEIVED.load(std::sync::atomic::Ordering::Relaxed), "capturePeak": voice_audio::CAPTURE_PEAK.load(std::sync::atomic::Ordering::Relaxed), "texts": texts, "reactions": reactions.len(), "call": self.call.as_ref().map(|c| format!("{:?}", c.status)), "room": self.room.as_ref().map(|r| format!("{} people, {} connected", r.participants.len(), r.connections)), "incoming": self.incoming.is_some(), "unread": self.unread_direct.len() + self.unread_channels.len(), "toasts": self.toasts.iter().map(|t| t.text.clone()).collect::<Vec<_>>() }))
    }

    /// Where an attachment is being sent, as the upload's query string.
    fn upload_target(&self) -> Option<String> {
        match self.mode {
            Mode::Direct => self.selected_peer().filter(|p| p.encryption_key.is_some()).map(|p| format!("to={}", p.id)),
            Mode::Community => self.community.as_ref().map(|c| match &self.channel { Some(channel) => format!("community={}&channel={channel}", c.id), None => format!("community={}", c.id) }),
        }
    }

    fn send(&mut self, reaction: Option<chat::Reaction>) -> Task<Msg> {
        if self.sending { return Task::none(); }
        let (Some(session), Some(key)) = (self.session.clone(), self.key.clone()) else { return Task::none() };
        let file = self.pending_file.take();
        let text = file.as_ref().map_or_else(|| self.composer.text().trim().to_owned(), |f| f.label());
        if reaction.is_none() && text.is_empty() { return Task::none(); }
        let reply = if reaction.is_none() && file.is_none() { self.reply_to.clone() } else { None };
        let plain = match (&file, &reaction) {
            (Some(file), _) => message_content::encode_file(file).map_err(|_| "invalid file"),
            (None, Some(r)) => message_content::encode_reaction(&r.target, &r.emoji, &r.action),
            (None, None) => message_content::encode_text_with_mentions(&text, reply.as_deref(), &self.named_people(&text)),
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
                    let mut message = local(stored, text, reply, reaction);
                    message.file = file;
                    Ok(message)
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
                    let mut message = local(stored, text, reply, reaction);
                    message.file = file;
                    Ok(message)
                }, move |r| Msg::Sent(generation, r))
            }
        }
    }
}

static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Windows would reject these characters in a suggested name.
fn safe_file_name(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
    let cleaned = cleaned.trim_matches(['.', ' ']).to_owned();
    if cleaned.is_empty() { "file".into() } else { cleaned }
}

async fn upload(session: Session, target: String, path: std::path::PathBuf) -> Result<message_content::FileMeta, String> {
    let size = std::fs::metadata(&path).map_err(|_| "Could not read that file")?.len();
    if size == 0 { return Err("That file is empty".into()); }
    if size > message_content::MAX_FILE_BYTES as u64 { return Err("Files can be up to 8 MB".into()); }
    let plain = Zeroizing::new(std::fs::read(&path).map_err(|_| "Could not read that file")?);
    let (sealed, key, iv) = crypto::encrypt_file(&plain).map_err(str::to_owned)?;
    let id = session.upload_file(&target, sealed).await?;
    let name: String = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into()).chars().take(200).collect();
    Ok(message_content::FileMeta { id, key, iv, name, kind: "application/octet-stream".into(), size })
}

async fn save_file(session: Session, file: message_content::FileMeta, path: std::path::PathBuf) -> Result<(), String> {
    let sealed = session.download_file(&file.id).await?;
    let plain = Zeroizing::new(crypto::decrypt_file(&sealed, &file.key, &file.iv).map_err(str::to_owned)?);
    std::fs::write(path, plain.as_slice()).map_err(|_| "Could not save the file".to_string())
}

/// The sender's view of a message it just encrypted and stored.
fn local(stored: api::MessageEnvelope, text: String, reply: Option<String>, reaction: Option<chat::Reaction>) -> chat::Message {
    chat::Message {
        id: stored.id, sender: stored.sender_id, receiver: stored.receiver_id, channel: stored.channel_id,
        created: chat::timestamp(&stored.created_at).unwrap_or_else(chat::now), created_at: stored.created_at,
        expires: stored.expires_at.as_deref().and_then(chat::timestamp), pinned: stored.pinned,
        text: if reaction.is_some() { String::new() } else { text }, reply_to: reply, reaction, missed_call: false, file: None, unreadable: false, edited: stored.edited_at.is_some(), mentions: Vec::new(),
    }
}
