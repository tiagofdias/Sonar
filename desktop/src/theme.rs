//! Visual tokens copied from frontend/src/index.css so the native client and
//! the web client render the same design. Sizes are CSS pixels.
use iced::border::Radius;
use iced::font::Weight;
use iced::gradient::Linear;
use iced::widget::{button, container, pick_list, scrollable, svg, text, text_editor, text_input};
use iced::{Background, Border, Color, Degrees, Element, Font, Gradient, Length, Shadow, Theme};

pub const fn rgb(hex: u32) -> Color {
    Color::from_rgb8(((hex >> 16) & 0xff) as u8, ((hex >> 8) & 0xff) as u8, (hex & 0xff) as u8)
}
/// `#rrggbbaa` as written in the stylesheet.
pub fn rgba(hex: u32) -> Color {
    let base = rgb(hex >> 8);
    Color { a: (hex & 0xff) as f32 / 255.0, ..base }
}

pub const PAGE: Color = rgb(0x111417);
pub const TEXT: Color = rgb(0xdfe2e4);
pub const RAIL: Color = rgb(0x0e1114);
pub const SIDEBAR: Color = rgb(0x161a1e);
pub const CONVERSATION: Color = rgb(0x191d22);
pub const FOOTER_BG: Color = rgb(0x12161a);
pub const LINE: Color = rgb(0x22282e);
pub const LINE_SIDEBAR: Color = rgb(0x232a31);
pub const LINE_CHAT: Color = rgb(0x272e36);
pub const ACCENT: Color = rgb(0x75ebfb);
pub const MUTED: Color = rgb(0x8a949c);
pub const EYEBROW: Color = rgb(0xd0d7dc);
pub const GREEN: Color = rgb(0x8fc3a3);
pub const OFFLINE: Color = rgb(0x75818b);
pub const UNREAD: Color = rgb(0x70eeff);
pub const DANGER: Color = rgb(0xa2485d);

pub fn font(weight: Weight) -> Font { Font { weight, ..REGULAR } }
pub const REGULAR: Font = Font::with_name("Poppins");
pub fn semibold() -> Font { font(Weight::Semibold) }
pub fn bold() -> Font { font(Weight::Bold) }
pub fn heavy() -> Font { font(Weight::ExtraBold) }

pub fn radius(value: f32) -> Radius { value.into() }
pub fn border(color: Color, width: f32, r: f32) -> Border { Border { color, width, radius: radius(r) } }

/// CSS letter-spacing is not available in the renderer; thin spaces between
/// letters reproduce the wide-tracked uppercase labels.
pub fn tracked(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 4);
    for (index, ch) in value.chars().enumerate() {
        if index > 0 { out.push('\u{2009}'); }
        out.push(ch);
    }
    out
}

pub fn eyebrow<'a>(value: &str, size: f32, color: Color) -> iced::widget::Text<'a> {
    text(tracked(value)).size(size).color(color).font(bold())
}

// ----- Containers -------------------------------------------------------

pub fn fill(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: Some(color.into()), text_color: Some(TEXT), ..Default::default() }
}
pub fn panel(background: Color, line: Color, r: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: Some(background.into()), border: border(line, 1.0, r), text_color: Some(TEXT), ..Default::default() }
}
pub fn gradient_panel(angle: f32, from: Color, to: Color, line: Color, r: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Gradient(Gradient::Linear(Linear::new(Degrees(angle)).add_stop(0.0, from).add_stop(1.0, to)))),
        border: border(line, 1.0, r),
        text_color: Some(TEXT),
        ..Default::default()
    }
}
pub fn modal_card(background: Color, line: Color, r: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(background.into()),
        border: border(line, 1.0, r),
        // No soft shadow: large blurs render as black blocks in partial software redraws.
        text_color: Some(TEXT),
        ..Default::default()
    }
}
/// A 1px rule drawn as a filled container (CSS borders on one side only).
pub fn rule<'a, M: 'a>(color: Color, horizontal: bool) -> Element<'a, M> {
    let bar = container(iced::widget::space()).style(fill(color));
    if horizontal { bar.width(Length::Fill).height(1).into() } else { bar.width(1).height(Length::Fill).into() }
}
pub fn dot<'a, M: 'a>(color: Color, size: f32) -> Element<'a, M> {
    container(iced::widget::space()).width(size).height(size)
        .style(move |_| container::Style { background: Some(color.into()), border: border(Color::TRANSPARENT, 0.0, size / 2.0), ..Default::default() })
        .into()
}

// ----- Buttons ----------------------------------------------------------

type ButtonStyle = Box<dyn Fn(&Theme, button::Status) -> button::Style>;
fn btn(background: Option<Color>, hover: Option<Color>, color: Color, hover_color: Color, line: Option<Color>, r: f32) -> ButtonStyle {
    Box::new(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let disabled = matches!(status, button::Status::Disabled);
        let mut style = button::Style {
            background: (if hovered { hover.or(background) } else { background }).map(Background::Color),
            text_color: if hovered { hover_color } else { color },
            border: line.map(|line| border(line, 1.0, r)).unwrap_or_else(|| border(Color::TRANSPARENT, 0.0, r)),
            ..Default::default()
        };
        if disabled {
            style.text_color.a *= 0.4;
            if let Some(Background::Color(c)) = style.background.as_mut() { c.a *= 0.4; }
            style.border.color.a *= 0.4;
        }
        style
    })
}
/// The main action color: a current running from aqua into deep blue.
fn current(r: f32) -> ButtonStyle {
    Box::new(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let alpha = if matches!(status, button::Status::Disabled) { 0.4 } else { 1.0 };
        let (from, to) = if hovered { (rgb(0x8ff7f8), rgb(0x62b4ff)) } else { (rgb(0x6ff3f5), rgb(0x3f9dff)) };
        button::Style {
            background: Some(Background::Gradient(Gradient::Linear(Linear::new(Degrees(120.0)).add_stop(0.0, Color { a: alpha, ..from }).add_stop(1.0, Color { a: alpha, ..to })))),
            text_color: Color { a: alpha, ..rgb(0x15191d) },
            border: border(Color::TRANSPARENT, 0.0, r),
            ..Default::default()
        }
    })
}
pub fn primary() -> ButtonStyle { current(7.0) }
pub fn secondary() -> ButtonStyle { btn(Some(rgb(0x22282e)), Some(rgb(0x2a313a)), rgb(0xd0d7dc), rgb(0xd0d7dc), Some(rgb(0x38424d)), 7.0) }
pub fn danger() -> ButtonStyle { btn(Some(DANGER), Some(rgb(0xb05269)), Color::WHITE, Color::WHITE, Some(rgb(0xc1687c)), 7.0) }
pub fn icon_button(active: bool) -> ButtonStyle {
    if active { btn(None, Some(rgba(0xffffff08)), ACCENT, ACCENT, None, 7.0) } else { btn(None, Some(rgba(0xffffff08)), rgb(0x8c969d), rgb(0xd0d7dc), None, 7.0) }
}
pub fn call_control(active: bool) -> ButtonStyle {
    if active { btn(Some(rgb(0x1d5a7d)), Some(rgb(0x1d5a7d)), rgb(0x70eeff), rgb(0x70eeff), None, 7.0) } else { btn(None, Some(rgba(0xffffff14)), rgb(0xd7f3ff), rgb(0xffffff), None, 7.0) }
}
pub fn rail_button(active: bool) -> ButtonStyle {
    if active { btn(Some(rgb(0x282f37)), Some(rgb(0x282f37)), rgb(0x70eeff), rgb(0x70eeff), None, 13.0) } else { btn(None, Some(rgb(0x1c2127)), rgb(0x76828d), rgb(0xd0d7dc), None, 13.0) }
}
pub fn nav_row(selected: bool) -> ButtonStyle {
    if selected { btn(Some(rgb(0x163447)), Some(rgb(0x163447)), rgb(0xd0d7dc), rgb(0xd0d7dc), None, 7.0) } else { btn(None, Some(rgb(0x20252c)), rgb(0x98a1a7), rgb(0x98a1a7), None, 7.0) }
}
pub fn danger_row() -> ButtonStyle { btn(None, Some(rgb(0x20252c)), rgb(0xc28b98), rgb(0xc28b98), None, 7.0) }
pub fn person_row(chosen: bool) -> ButtonStyle {
    if chosen { btn(Some(rgb(0x163447)), Some(rgb(0x163447)), TEXT, TEXT, None, 8.0) } else { btn(None, Some(rgb(0x1d2329)), TEXT, TEXT, None, 8.0) }
}
pub fn plain(color: Color, hover: Color) -> ButtonStyle { btn(None, None, color, hover, None, 0.0) }
pub fn community_icon(active: bool) -> ButtonStyle {
    if active { btn(Some(rgb(0x74808b)), Some(rgb(0x74808b)), rgb(0x70eeff), rgb(0x70eeff), Some(rgb(0xd0d7dc)), 12.0) } else { btn(Some(rgb(0x2a313a)), Some(rgb(0x74808b)), rgb(0xd0d7dc), rgb(0x70eeff), Some(rgb(0x37404b)), 15.0) }
}
pub fn add_community() -> ButtonStyle { btn(Some(rgb(0x21272e)), Some(rgb(0x272e36)), rgb(0xb9bfc4), rgb(0xb9bfc4), Some(rgb(0x74808b)), 13.0) }
pub fn reaction(selected: bool) -> ButtonStyle {
    if selected { btn(Some(rgb(0x313a44)), Some(rgb(0x313a44)), rgb(0xd0d7dc), rgb(0xd0d7dc), Some(rgb(0xd0d7dc)), 8.0) } else { btn(Some(rgb(0x1f252b)), Some(rgb(0x262d34)), rgb(0xd0d7dc), rgb(0xd0d7dc), Some(rgb(0x38424d)), 8.0) }
}
pub fn load_older() -> ButtonStyle { btn(Some(rgb(0x22282e)), Some(rgb(0x2a323b)), rgb(0xd0d7dc), rgb(0xd0d7dc), Some(rgb(0x39434e)), 8.0) }
pub fn volume_slider() -> impl Fn(&Theme, iced::widget::slider::Status) -> iced::widget::slider::Style {
    use iced::widget::slider::{Handle, HandleShape, Rail, Style};
    |_, _| Style {
        rail: Rail { backgrounds: (rgb(0xd0d7dc).into(), rgb(0x353e49).into()), width: 4.0, border: border(Color::TRANSPARENT, 0.0, 2.0) },
        handle: Handle { shape: HandleShape::Circle { radius: 6.0 }, background: rgb(0xd0d7dc).into(), border_width: 0.0, border_color: Color::TRANSPARENT },
    }
}
pub fn presence_color(status: &str) -> Color {
    match status { "away" => rgb(0xe5b567), "busy" => rgb(0xe0707c), "offline" => OFFLINE, _ => rgb(0x88c7a3) }
}
pub fn mic_speaking() -> ButtonStyle { btn(Some(rgb(0x2c4634)), Some(rgb(0x2c4634)), rgb(0x7fdc9a), rgb(0x7fdc9a), Some(rgb(0x5fc784)), 7.0) }
pub fn send_button() -> ButtonStyle { current(8.0) }
pub fn end_call() -> ButtonStyle { btn(Some(rgb(0x9c4b54)), Some(rgb(0xad5560)), Color::WHITE, Color::WHITE, None, 8.0) }
pub fn community_invite() -> ButtonStyle { btn(Some(rgb(0x272e37)), Some(rgb(0x2e363f)), rgb(0xd0d7dc), rgb(0xd0d7dc), Some(rgb(0x36404b)), 7.0) }
pub fn fingerprint_button() -> ButtonStyle { btn(Some(rgb(0x272e35)), Some(rgb(0x2e363f)), rgb(0xd0d7dc), rgb(0xd0d7dc), Some(rgb(0x74808b)), 6.0) }
pub fn text_button() -> ButtonStyle { btn(None, None, rgb(0xd0d7dc), rgb(0xd0d7dc), None, 0.0) }
pub fn self_avatar() -> ButtonStyle { btn(Some(rgb(0x74808b)), Some(rgb(0x75818b)), rgb(0xf8e2c9), rgb(0xf8e2c9), None, 12.0) }
pub fn brand() -> ButtonStyle { btn(Some(rgb(0x252c34)), Some(rgb(0x2c333c)), TEXT, TEXT, None, 14.0) }

// ----- Inputs -----------------------------------------------------------

pub fn input(background: Color, line: Color, focus: Color, r: f32) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| text_input::Style {
        background: background.into(),
        border: border(if matches!(status, text_input::Status::Focused { .. }) { focus } else { line }, 1.0, r),
        icon: rgb(0x74808b),
        placeholder: rgb(0x74808b),
        value: rgb(0xedeeef),
        selection: rgba(0xd0d7dc66),
    }
}
pub fn auth_input() -> impl Fn(&Theme, text_input::Status) -> text_input::Style { input(rgb(0x1e2329), rgb(0x2f3740), rgb(0xd0d7dc), 7.0) }
pub fn dialog_input() -> impl Fn(&Theme, text_input::Status) -> text_input::Style { input(rgb(0x15191e), rgb(0x3a4450), rgb(0xd0d7dc), 7.0) }
pub fn card_input() -> impl Fn(&Theme, text_input::Status) -> text_input::Style { input(rgb(0x171b20), rgb(0x37414c), rgb(0xd0d7dc), 7.0) }
pub fn bare_input() -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    |_, _| text_input::Style { background: Color::TRANSPARENT.into(), border: border(Color::TRANSPARENT, 0.0, 0.0), icon: rgb(0x74808b), placeholder: rgb(0x74808b), value: rgb(0xdadde0), selection: rgba(0xd0d7dc66) }
}
pub fn composer_editor() -> impl Fn(&Theme, text_editor::Status) -> text_editor::Style {
    |_, _| text_editor::Style { background: Color::TRANSPARENT.into(), border: border(Color::TRANSPARENT, 0.0, 0.0), placeholder: rgb(0x78848f), value: rgb(0xd0d7dc), selection: rgba(0xd0d7dc66) }
}
pub fn select() -> impl Fn(&Theme, pick_list::Status) -> pick_list::Style {
    |_, status| pick_list::Style {
        text_color: rgb(0xd0d7dc),
        placeholder_color: rgb(0x848f97),
        handle_color: rgb(0xd0d7dc),
        background: rgb(0x171b20).into(),
        border: border(if matches!(status, pick_list::Status::Hovered | pick_list::Status::Opened { .. }) { rgb(0x74808b) } else { rgb(0x37414c) }, 1.0, 7.0),
    }
}
pub fn menu() -> impl Fn(&Theme) -> iced::overlay::menu::Style {
    |_| iced::overlay::menu::Style {
        background: rgb(0x1d2227).into(),
        border: border(rgb(0x37414c), 1.0, 7.0),
        text_color: rgb(0xd0d7dc),
        selected_text_color: rgb(0x70eeff),
        selected_background: rgb(0x313a44).into(),
        shadow: Shadow::default(),
    }
}
pub fn scroll() -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    |_, status| {
        let active = matches!(status, scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. });
        let rail = scrollable::Rail {
            background: None,
            border: border(Color::TRANSPARENT, 0.0, 4.0),
            scroller: scrollable::Scroller { background: (if active { rgb(0x3a444f) } else { rgb(0x2d343d) }).into(), border: border(Color::TRANSPARENT, 0.0, 4.0) },
        };
        scrollable::Style {
            container: container::Style::default(),
            vertical_rail: rail,
            horizontal_rail: rail,
            gap: None,
            auto_scroll: scrollable::AutoScroll { background: rgb(0x1f252b).into(), border: border(rgb(0x38424d), 1.0, 12.0), shadow: Shadow::default(), icon: rgb(0xd0d7dc) },
        }
    }
}

// ----- Icons ------------------------------------------------------------

macro_rules! icons {
    ($($name:ident => $file:literal),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[allow(dead_code)]
        pub enum Icon { $($name),* }
        fn icon_bytes(icon: Icon) -> &'static [u8] {
            match icon { $(Icon::$name => include_bytes!(concat!("../assets/icons/", $file, ".svg"))),* }
        }
    };
}
icons! {
    MessageCircle => "message-circle", Users => "users", ShieldCheck => "shield-check", ChevronDown => "chevron-down",
    ChevronLeft => "chevron-left", ArrowUpRight => "arrow-up-right", Search => "search", LockKeyhole => "lock-keyhole",
    LogOut => "log-out", Phone => "phone", PhoneOff => "phone-off", PhoneIncoming => "phone-incoming", PhoneMissed => "phone-missed", Settings => "settings", Smile => "smile", Paperclip => "paperclip", File => "file", ScreenShare => "screen-share", ScreenShareOff => "screen-share-off", Maximize => "maximize", Minimize => "minimize", Pin => "pin",
    Ellipsis => "ellipsis", Timer => "timer", Headphones => "headphones", HeadphoneOff => "headphone-off", Check => "check",
    Reply => "reply", Trash => "trash-2", Send => "send", X => "x", SmilePlus => "smile-plus", Mic => "mic",
    MicOff => "mic-off", Plus => "plus", Hash => "hash", Volume => "volume-2", KeyRound => "key-round",
    UserRound => "user-round", Copy => "copy", Download => "download", Monitor => "monitor", Lock => "lock", LockOpen => "lock-open",
    ArrowLeft => "arrow-left", AudioLines => "audio-lines", RotateCcw => "rotate-ccw", CircleCheck => "circle-check",
    CircleX => "circle-x", Server => "server", Refresh => "refresh-cw", UserMinus => "user-minus", Ban => "ban",
    Crown => "crown", Shield => "shield", Pencil => "pencil", Speaker => "speaker", Play => "play", Square => "square",
    Eye => "eye", EyeOff => "eye-off", Info => "info",
}

fn handle(icon: Icon) -> svg::Handle {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<u8, svg::Handle>>> = OnceLock::new();
    let key = icon as u8;
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    cache.entry(key).or_insert_with(|| svg::Handle::from_memory(icon_bytes(icon))).clone()
}

pub fn icon<'a, M: 'a>(icon: Icon, size: f32, color: Color) -> Element<'a, M> {
    svg(handle(icon)).width(size).height(size).style(move |_, _| svg::Style { color: Some(color) }).into()
}

pub fn sonar_mark<'a, M: 'a>(scale: f32) -> Element<'a, M> {
    static MARK: std::sync::OnceLock<svg::Handle> = std::sync::OnceLock::new();
    let handle = MARK.get_or_init(|| svg::Handle::from_memory(include_bytes!("../assets/sonar-mark.svg").as_slice())).clone();
    svg(handle).width(43.0 * scale).height(43.0 * scale).into()
}
pub fn waves<'a, M: 'a>() -> Element<'a, M> {
    static WAVES: std::sync::OnceLock<svg::Handle> = std::sync::OnceLock::new();
    svg(WAVES.get_or_init(|| svg::Handle::from_memory(include_bytes!("../assets/waves.svg").as_slice())).clone()).width(Length::Fill).into()
}
/// Milliseconds driving the welcome screen's sonar pulse.
pub static PULSE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub fn orbit<'a, M: 'a>() -> svg::Svg<'a> {
    static ORBIT: std::sync::OnceLock<svg::Handle> = std::sync::OnceLock::new();
    svg(ORBIT.get_or_init(|| svg::Handle::from_memory(include_bytes!("../assets/orbit.svg").as_slice())).clone()).width(186).height(186)
}

/// Avatar tile colors rotate like `.person:nth-child(3n+...)` on the web.
pub fn avatar_colors(index: usize) -> (Color, Color) {
    match index % 3 { 1 => (rgb(0x343d48), rgb(0xd0d7dc)), 2 => (rgb(0x2f3741), rgb(0xb8d2c2)), _ => (rgb(0x38424d), rgb(0xcfd4d7)) }
}
pub fn initials(name: &str) -> String {
    let value: String = name.split_whitespace().filter_map(|part| part.chars().next()).take(2).collect::<String>().to_uppercase();
    if value.is_empty() { "?".into() } else { value }
}
