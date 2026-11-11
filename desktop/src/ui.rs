//! Native views mirroring the web client's pages and components:
//! AuthPage, UnpairedDevicePage, HomePage, CommunityWorkspace, AccountSettings.
use crate::theme::{self as t, Icon, rgb, rgba};
use crate::{App, CallStatus, Confirm, Dialog, Mode, Msg, Panel, Screen, Tab, ToastKind, UnlockReason};
use iced::alignment::{Horizontal, Vertical};
use iced::widget::text::LineHeight;
use iced::widget::{slider, tooltip, button, center, column, container, hover, mouse_area, opaque, pick_list, row, scrollable, space, stack, text, text_editor, text_input, Column, Row};
use iced::{Color, Element, Length, Padding};

pub const SYSTEM_DEFAULT: &str = "System default";
pub const NO_FOLDER: &str = "No folder";
pub const NEW_FOLDER: &str = "New folder (name it above)";

/// What sits in a person's bubble: their animal. Servers keep their letters.
#[derive(Clone)]
enum Face { Animal(Icon), Letters(String) }
fn face_of(id: &str, animal: Option<&str>) -> Face { Face::Animal(t::animal_icon(id, animal)) }
fn person_face(person: &crate::api::Person) -> Face { face_of(&person.id, person.animal.as_deref()) }
fn own_face(app: &App) -> Face { app.session.as_ref().map_or(Face::Letters(String::new()), |s| face_of(s.account.id.as_deref().unwrap_or_default(), s.account.animal.as_deref())) }

#[derive(Clone, PartialEq, Eq)]
struct Span { label: &'static str, seconds: u32 }
impl std::fmt::Display for Span { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.label) } }
const SPANS: [Span; 3] = [Span { label: "7 days", seconds: 0 }, Span { label: "1 hour", seconds: 3600 }, Span { label: "1 day", seconds: 86400 }];
type E<'a> = Element<'a, Msg>;
const FILL: Length = Length::Fill;

pub fn view(app: &App) -> E<'_> {
    let page: E = if let Some(key) = &app.new_recovery { recovery_view(app, key) } else { match app.screen {
        Screen::Boot => boot(),
        Screen::Auth => auth(app),
        Screen::Unlock => unlock(app),
        Screen::Main => workspace(app),
    } };
    let mut layers = iced::widget::Stack::new().width(FILL).height(FILL).push(page);
    if let Some(stage) = screen_stage(app) { layers = layers.push(stage); }
    if let Some(picker) = &app.screen_picker { layers = layers.push(screen_picker(picker)); }
    crate::wheel::browser_speed(layers.push(toasts(app)).into())
}

// ----- Shared pieces --------------------------------------------------------

fn h_space(width: f32) -> E<'static> { space().width(width).into() }
fn v_space(height: f32) -> E<'static> { space().height(height).into() }
fn grow() -> E<'static> { space().width(FILL).into() }

fn body<'a>(value: impl text::IntoFragment<'a>, size: f32, color: Color, line: f32) -> iced::widget::Text<'a> {
    text(value).size(size).color(color).font(t::REGULAR).line_height(LineHeight::Relative(line))
}

fn avatar<'a>(face: Face, index: usize, online: Option<Color>, size: f32) -> E<'a> { avatar_status(face, index, online, t::presence_color("online"), size) }
fn avatar_status<'a>(face: Face, index: usize, online: Option<Color>, dot: Color, size: f32) -> E<'a> {
    let (background, color) = t::avatar_colors(index);
    tile(face, background, color, None, size, if size > 40.0 { 23.0 } else { 12.0 }, if size > 40.0 { 24.0 } else { 14.0 }, online, dot)
}
#[allow(clippy::too_many_arguments)]
fn tile<'a>(who: Face, background: Color, color: Color, line: Option<Color>, size: f32, r: f32, font: f32, online: Option<Color>, dot: Color) -> E<'a> {
    // People's animals sit on the app's blue; servers keep their grey letters.
    let (background, color) = if matches!(who, Face::Animal(_)) { (rgb(0x12405f), rgb(0xd7f3ff)) } else { (background, color) };
    let inner: E<'a> = match who { Face::Animal(icon) => t::icon(icon, size * 0.74, color).into(), Face::Letters(name) => text(t::initials(&name)).size(font).color(color).font(t::semibold()).into() };
    let face: E<'a> = container(inner)
        .width(size).height(size).center_x(size).center_y(size)
        .style(move |_| container::Style { background: Some(background.into()), border: t::border(line.unwrap_or(Color::TRANSPARENT), if line.is_some() { 1.0 } else { 0.0 }, r), ..Default::default() }).into();
    match online {
        Some(ring) => {
            let badge = container(container(space()).width(9).height(9).style(move |_| container::Style { background: Some(dot.into()), border: t::border(ring, 2.0, 6.0), ..Default::default() }))
                .width(size + 2.0).height(size + 2.0).align_x(Horizontal::Right).align_y(Vertical::Bottom);
            stack![container(face).padding(Padding { right: 1.0, bottom: 1.0, ..Padding::ZERO }), badge].into()
        }
        None => face,
    }
}

fn icon_btn<'a>(icon: Icon, size: f32, on: Option<Msg>, active: bool) -> E<'a> {
    let color = if active { t::ACCENT } else { rgb(0x8c969d) };
    button(container(t::icon(icon, size, color)).center_x(30).center_y(32)).padding(0).width(30).height(32).style(t::icon_button(active)).on_press_maybe(on).into()
}

fn action<'a>(label: impl Into<String>, icon: Option<Icon>, style: Box<dyn Fn(&iced::Theme, button::Status) -> button::Style>, color: Color, on: Option<Msg>, padding: [f32; 2], size: f32) -> E<'a> {
    let label: String = label.into();
    let mut content = Row::new().spacing(if padding[1] > 15.0 { 22.0 } else { 12.0 }).align_y(Vertical::Center);
    content = content.push(text(label).size(size).color(color).font(t::semibold()));
    if let Some(icon) = icon { content = content.push(t::icon(icon, size + 5.0, color)); }
    button(content).padding(padding).style(style).on_press_maybe(on).into()
}
fn primary<'a>(label: impl Into<String>, icon: Option<Icon>, on: Option<Msg>) -> E<'a> { action(label, icon, t::primary(), rgb(0x1e2329), on, [13.0, 18.0], 15.0) }
/// A primary button filling its form, like `.auth-card form .primary`.
fn primary_wide<'a>(label: impl Into<String>, icon: Icon, on: Option<Msg>) -> E<'a> {
    let label: String = label.into();
    let color = rgb(0x1e2329);
    button(row![text(label).size(15).color(color).font(t::semibold()), grow(), t::icon(icon, 18.0, color)].align_y(Vertical::Center)).width(FILL).padding([13, 18]).style(t::primary()).on_press_maybe(on).into()
}
fn secondary<'a>(label: impl Into<String>, icon: Option<Icon>, on: Option<Msg>) -> E<'a> {
    let label: String = label.into();
    let mut content = Row::new().spacing(12).align_y(Vertical::Center).push(text(label).size(15).color(rgb(0xd0d7dc)).font(t::REGULAR));
    if let Some(icon) = icon { content = content.push(t::icon(icon, 16.0, rgb(0xd0d7dc))); }
    button(content).padding([12, 16]).style(t::secondary()).on_press_maybe(on).into()
}
fn danger<'a>(label: impl Into<String>, on: Option<Msg>) -> E<'a> {
    let label: String = label.into();
    button(text(label).size(15).color(Color::WHITE).font(t::semibold())).padding([13, 18]).style(t::danger()).on_press_maybe(on).into()
}
/// The microphone toggle; green while the mic is picking up sound.
fn call_btn<'a>(icon: Icon, size: f32, on: Option<Msg>, active: bool) -> E<'a> {
    let color = if active { rgb(0x70eeff) } else { rgb(0xd7f3ff) };
    button(container(t::icon(icon, size, color)).center_x(30).center_y(32)).padding(0).width(30).height(32).style(t::call_control(active)).on_press_maybe(on).into()
}
fn mic_btn<'a>(muted: bool, speaking: bool, on: Msg) -> E<'a> {
    if !speaking || muted { return call_btn(if muted { Icon::MicOff } else { Icon::Mic }, 18.0, Some(on), muted); }
    button(container(t::icon(Icon::Mic, 18.0, rgb(0x7fdc9a))).center_x(30).center_y(32)).padding(0).width(30).height(32).style(t::mic_speaking()).on_press(on).into()
}
fn lead_icon<'a>(icon: Icon, label: impl text::IntoFragment<'a>, color: Color, icon_size: f32, size: f32, gap: f32) -> Row<'a, Msg> {
    row![t::icon(icon, icon_size, color), text(label).size(size).color(color).font(t::REGULAR)].spacing(gap).align_y(Vertical::Center)
}

fn field<'a>(label: &'a str, input: E<'a>, gap: f32, size: f32, color: Color) -> E<'a> {
    column![text(label).size(size).color(color).font(t::REGULAR), input].spacing(gap).into()
}
fn dialog_input<'a>(placeholder: &str, value: &'a str, on: impl Fn(String) -> Msg + 'a, secure: bool) -> E<'a> {
    text_input(placeholder, value).on_input(on).secure(secure).padding(13).size(15).style(t::dialog_input()).into()
}
fn card_input<'a>(placeholder: &str, value: &'a str, on: impl Fn(String) -> Msg + 'a, secure: bool) -> E<'a> {
    text_input(placeholder, value).on_input(on).secure(secure).padding([11, 12]).size(15).style(t::card_input()).into()
}

fn honest_note<'a>(icon: Icon, note: &'a str) -> E<'a> {
    container(row![t::icon(icon, 16.0, rgb(0xd0d7dc)), body(note, 13.0, rgb(0x9ba3a9), 1.8)].spacing(9))
        .padding(13).width(FILL).style(t::panel(rgb(0x22282f), rgb(0x2d353f), 7.0)).into()
}

fn scroll_area<'a>(content: impl Into<E<'a>>) -> scrollable::Scrollable<'a, Msg> {
    scrollable(content).direction(scrollable::Direction::Vertical(scrollable::Scrollbar::new().width(6).scroller_width(6).margin(1))).style(t::scroll())
}

/// A centered dialog over a dimmed backdrop (`.modal-backdrop` + card).
/// A dialog body that scrolls with its bar beside the text, never over it.
fn modal_scroll<'a>(content: impl Into<E<'a>>) -> E<'a> {
    container(scrollable(content).direction(scrollable::Direction::Vertical(scrollable::Scrollbar::new().width(6).scroller_width(6).margin(1).spacing(14))).style(t::scroll()))
        .padding(Padding { top: 34.0, bottom: 34.0, left: 34.0, right: 12.0 }).into()
}

fn modal<'a>(content: E<'a>, width: f32, close: Option<Msg>) -> E<'a> {
    let card = container(content).max_width(width).width(FILL).style(t::modal_card(rgb(0x1b2025), rgb(0x353e49), 17.0));
    let backdrop = mouse_area(center(opaque(card)).padding(20).style(t::fill(rgba(0x090b0db8))));
    opaque(match close { Some(msg) => backdrop.on_press(msg), None => backdrop }).into()
}
fn modal_header<'a>(icon: Icon, size: f32, close: Msg) -> E<'a> {
    row![t::icon(icon, size, t::ACCENT), grow(), icon_btn(Icon::X, 20.0, Some(close), false)].align_y(Vertical::Top).into()
}
fn modal_title<'a>(value: impl text::IntoFragment<'a>) -> E<'a> {
    container(text(value).size(26).font(t::bold()).color(t::TEXT).line_height(LineHeight::Relative(1.35))).padding([15, 0]).into()
}

fn boot<'a>() -> E<'a> {
    center(column![t::sonar_mark(1.0), text("Opening your private space...").size(16).color(rgb(0xd0d7dc))].spacing(25).align_x(Horizontal::Center)).into()
}

fn screen_button<'a>(app: &'a App) -> E<'a> {
    let sharing = app.screen_sharing.is_some();
    call_btn(if sharing { Icon::ScreenShareOff } else { Icon::ScreenShare }, 18.0, app.screen_controls.is_some().then_some(if sharing { Msg::ScreenStop } else { Msg::ScreenOpenPicker }), sharing)
}

fn screen_volume<'a>(app: &'a App, id: &'a str) -> E<'a> {
    let volume = app.screen_volumes.get(id).copied().unwrap_or(1.0);
    let id = id.to_owned();
    row![t::icon(Icon::Volume, 15.0, rgb(0x929ca3)), iced::widget::slider(0.0..=1.0, volume, move |value| Msg::ScreenVolume(id.clone(), value)).step(0.01f32).width(110).style(t::volume_slider())].spacing(7).align_y(Vertical::Center).into()
}

/// Screens other people are sharing. Mirrors ScreenStage.jsx.
fn screen_stage(app: &App) -> Option<E<'_>> {
    use iced::widget::image;
    if app.screen_views.is_empty() { return None; }
    if let Some((id, (handle, _, _))) = app.screen_expanded.as_ref().and_then(|id| app.screen_views.get_key_value(id)) {
        let header = row![text(format!("{}'s screen", app.screen_name(id))).size(13).color(rgb(0xb8bec2)).width(FILL), screen_volume(app, id), icon_btn(Icon::Minimize, 16.0, Some(Msg::ScreenExpand(None)), false)].spacing(8).align_y(Vertical::Center);
        return Some(opaque(container(column![container(header).padding([6, 10]), image(handle.clone()).width(FILL).height(FILL)]).width(FILL).height(FILL).style(t::fill(rgb(0x000000)))));
    }
    let mut tiles = Column::new().spacing(10);
    for (id, (handle, width, height)) in &app.screen_views {
        let aspect = *height as f32 / (*width).max(1) as f32;
        let caption = row![text(format!("{}'s screen", app.screen_name(id))).size(13).color(rgb(0xb8bec2)).width(FILL), screen_volume(app, id), icon_btn(Icon::Maximize, 16.0, Some(Msg::ScreenExpand(Some(id.clone()))), false)].spacing(8).align_y(Vertical::Center);
        tiles = tiles.push(container(column![image(handle.clone()).width(FILL).height(520.0 * aspect), container(caption).padding([6, 10])]).width(FILL).style(t::panel(rgb(0x121518), rgb(0x2a313a), 8.0)));
    }
    Some(container(container(opaque(tiles)).width(520)).width(FILL).height(FILL).align_x(Horizontal::Right).align_y(Vertical::Bottom).padding(18).into())
}

fn screen_picker<'a>(picker: &'a crate::ScreenPicker) -> E<'a> {
    let chosen = picker.sources.get(picker.selected);
    let section = |label: &'static str| -> E<'a> { container(text(t::tracked(label)).size(11).color(rgb(0x848f96))).padding(Padding { top: 16.0, bottom: 8.0, ..Padding::ZERO }).into() };
    let mut screens = Column::new().spacing(4);
    let mut apps = Column::new().spacing(4);
    for (index, source) in picker.sources.iter().enumerate() {
        let selected = index == picker.selected;
        let line = button(row![t::icon(if source.monitor { Icon::ScreenShare } else { Icon::AudioLines }, 15.0, rgb(0xd0d7dc)), text(source.label.clone()).size(14).color(if selected { rgb(0xd0d7dc) } else { rgb(0xb8bec2) }).wrapping(text::Wrapping::None).width(FILL)].spacing(10).align_y(Vertical::Center)).width(FILL).padding([8, 10]).style(if selected { t::community_invite() } else { t::secondary() }).on_press(Msg::ScreenSelect(index));
        if source.monitor { screens = screens.push(line); } else { apps = apps.push(line); }
    }
    let list = column![section("YOUR SCREENS"), screens, section("APPS"), apps];
    let mut sizes = Row::new().spacing(8);
    for height in [1080u32, 1440, 2160] {
        let allowed = height == 1080 || (picker.hardware && chosen.is_some_and(|s| s.height >= height));
        let name = match height { 1080 => "1080p", 1440 => "1440p", _ => "4K" };
        sizes = sizes.push(button(text(name).size(14)).padding([8, 16]).style(if picker.height == height { t::primary() } else { t::secondary() }).on_press_maybe(allowed.then_some(Msg::ScreenHeight(height))));
    }
    let sound = if chosen.is_some_and(|s| s.monitor) { "Shares all PC sound except Sonar." } else { "Shares only this app's sound." };
    let encoder = if picker.hardware { "Encoded on your graphics card." } else { "No graphics-card encoder was found, so sharing is limited to 1080p." };
    let content = column![
        modal_header(Icon::ScreenShare, 24.0, Msg::ScreenClosePicker),
        modal_title("Share your screen"),
        container(scroll_area(list)).max_height(300),
        section("RESOLUTION"),
        sizes,
        container(text(format!("{sound} {encoder}")).size(13).color(rgb(0x9099a0))).padding(Padding { top: 12.0, bottom: 16.0, ..Padding::ZERO }),
        primary("Start sharing", Some(Icon::ScreenShare), chosen.map(|_| Msg::ScreenStart)),
    ];
    modal(container(content).padding(34).into(), 520.0, Some(Msg::ScreenClosePicker))
}

fn toasts(app: &App) -> E<'_> {
    let list = app.toasts.iter().fold(Column::new().spacing(8).align_x(Horizontal::Center), |list, toast| {
        let (icon, color) = match toast.kind { ToastKind::Success => (Some(Icon::CircleCheck), rgb(0x61d345)), ToastKind::Error => (Some(Icon::CircleX), rgb(0xff4b4b)), ToastKind::Plain => (None, t::TEXT) };
        let mut content = Row::new().spacing(10).align_y(Vertical::Center);
        if let Some(icon) = icon { content = content.push(t::icon(icon, 20.0, color)); }
        content = content.push(body(toast.text.as_str(), 16.0, rgb(0xedeeef), 1.3));
        list.push(mouse_area(container(content).padding([10, 14]).max_width(350).style(|_| container::Style {
            background: Some(rgb(0x1e2329).into()), border: t::border(Color::TRANSPARENT, 0.0, 8.0),
            ..Default::default()
        })).on_press(Msg::DismissToast(toast.id)))
    });
    container(list).width(FILL).height(FILL).align_x(Horizontal::Center).align_y(Vertical::Bottom).padding(Padding { bottom: 16.0, ..Padding::ZERO }).into()
}

// ----- Sign in / sign up (AuthPage) -----------------------------------------

fn auth(app: &App) -> E<'_> {
    let form = &app.auth;
    let feature = |icon: Icon, label: &'static str| -> E<'static> { lead_icon(icon, label, rgb(0xd0d7dc), 17.0, 13.0, 7.0).into() };
    let story = container(column![
        row![t::sonar_mark(1.0), text("sonar").size(25).font(t::bold()).color(t::TEXT),
            container(t::eyebrow("EARLY ACCESS", 10.0, rgb(0xd0d7dc))).padding(5).style(t::panel(Color::TRANSPARENT, rgb(0x74808b), 4.0))].spacing(12).align_y(Vertical::Center),
        space().height(FILL),
        t::eyebrow("LESS NOISE. MORE CONNECTION.", 13.0, t::EYEBROW),
        column![
            text("Your people.").size(42).font(t::bold()).color(t::TEXT),
            text("Your conversations.").size(42).font(t::bold()).color(t::TEXT),
            text("Your space.").size(42).font(t::bold()).color(rgb(0xd0d7dc)),
        ].spacing(2).padding([24, 0]),
        container(body("A quieter corner of the internet. Built for the conversations that matter, with privacy at the core.", 17.0, rgb(0x9ba3a9), 1.9)).max_width(365),
        row![feature(Icon::LockKeyhole, "Encrypted messages"), feature(Icon::Timer, "Gone in 7 days"), feature(Icon::AudioLines, "No call recordings")].spacing(20).padding(Padding { top: 36.0, ..Padding::ZERO }),
        space().height(FILL),
        text("Free to use. No subscriptions. No ads.").size(13).color(rgb(0x818c96)),
    ]).padding([45, 65]).width(FILL).height(FILL);
    let story = container(stack![container(t::waves::<Msg>()).width(FILL).height(FILL).align_y(Vertical::Bottom), story]).width(Length::FillPortion(105)).height(FILL)
        .style(t::gradient_panel(35.0, rgb(0x2b333c), rgb(0x121519), Color::TRANSPARENT, 0.0));

    let label = |value: &'static str| text(value).size(14).color(rgb(0xb7bdc2));
    let entry = |value: &str, field: u8, secure: bool| -> E<'_> {
        let input = text_input("", value).on_input(move |v| Msg::AuthField(field, v)).secure(secure).padding(12).size(16).style(t::auth_input());
        if field == 2 { input.on_submit(Msg::Submit).into() } else { input.into() }
    };
    let mut fields = Column::new().spacing(20);
    if form.signup {
        fields = fields.push(column![label("Display name"), entry(&form.name, 0, false)].spacing(9));
        fields = fields.push(column![label("Username"), entry(&form.username, 6, false)].spacing(9));
    }
    fields = fields.push(column![label("Email address"), entry(&form.email, 1, false)].spacing(9));
    if form.recover {
        fields = fields.push(column![label("Recovery key"), text_input("XXXX-XXXX-XXXX-XXXX-XXXX-XXXX", &form.recovery_key).on_input(|v| Msg::AuthField(4, v)).padding(12).size(16).style(t::auth_input())].spacing(9));
        fields = fields.push(column![label("New password"), text_input("", &form.password).on_input(|v| Msg::AuthField(2, v)).secure(true).padding(12).size(16).style(t::auth_input())].spacing(9));
        fields = fields.push(column![label("Confirm new password"), text_input("", &form.confirm).on_input(|v| Msg::AuthField(5, v)).on_submit(Msg::Recover).secure(true).padding(12).size(16).style(t::auth_input())].spacing(9));
    } else {
        fields = fields.push(column![label("Password"), entry(&form.password, 2, true)].spacing(9));
    }
    if !form.signup && !form.recover {
        fields = fields.push(container(button(text("Forgot your password?").size(14).color(rgb(0xd0d7dc))).padding(0).style(t::plain(rgb(0xd0d7dc), rgb(0xd0d7dc))).on_press(Msg::ToggleRecover)).width(FILL).align_x(Horizontal::Right).padding(Padding { top: -10.0, ..Padding::ZERO }));
    }
    let submit_label = if form.busy && form.recover { "Resetting your password..." } else if form.busy { "Setting up your space..." } else if form.recover { "Reset password" } else if form.signup { "Create your account" } else { "Enter your space" };
    fields = fields.push(container(primary_wide(submit_label, Icon::ArrowUpRight, (!form.busy).then_some(if form.recover { Msg::Recover } else { Msg::Submit }))).padding(Padding { top: 5.0, ..Padding::ZERO }));

    let mut card = Column::new().width(FILL)
        .push(t::icon(Icon::ShieldCheck, 30.0, t::ACCENT))
        .push(container(text(if form.recover { "Reset your password." } else if form.signup { "Find your frequency." } else { "Welcome back." }).size(28).font(t::bold()).color(t::TEXT)).padding(Padding { top: 22.0, bottom: 10.0, ..Padding::ZERO }))
        .push(body(if form.recover { "Use the recovery key you saved when you created your account. Your messages stay encrypted and readable." } else if form.signup { "Create your own little corner of Sonar." } else { "Good conversations are waiting for you." }, 15.0, rgb(0x9099a0), 1.8))
        .push(container(fields).padding(Padding { top: 30.0, bottom: 22.0, ..Padding::ZERO }));
    if !form.signup && !form.recover {
        let divider = row![t::rule(rgb(0x2f3842), true), text("or").size(13).color(rgb(0x74808b)), t::rule(rgb(0x2f3842), true)].spacing(12).align_y(Vertical::Center);
        #[cfg(windows)]
        let passkey_press = (!form.busy).then_some(Msg::PasskeySignIn);
        #[cfg(not(windows))]
        let passkey_press: Option<Msg> = None;
        let passkey = button(container(lead_icon(Icon::KeyRound, "Sign in with a passkey", rgb(0xd0d7dc), 17.0, 15.0, 12.0)).center_x(FILL))
            .width(FILL).padding([12, 16]).style(t::secondary()).on_press_maybe(passkey_press);
        card = card.push(container(divider).padding(Padding { top: 24.0, bottom: 15.0, ..Padding::ZERO }))
            .push(passkey)
            .push(container(body("Use a passkey you previously added in account settings.", 12.0, rgb(0x848f97), 1.8)).padding(Padding { top: 10.0, bottom: 24.0, ..Padding::ZERO }));
    }
    card = card.push(row![
        text(if form.signup || form.recover { "Remembered it?" } else { "New around here?" }).size(15).color(rgb(0x9099a0)),
        button(text(if form.signup || form.recover { "Sign in" } else { "Join Sonar" }).size(15).color(rgb(0xd0d7dc))).padding(0).style(t::plain(rgb(0xd0d7dc), rgb(0xd0d7dc))).on_press(Msg::ToggleSignup),
    ].spacing(6));
    // Desktop only: which Sonar server to use (the browser uses its own address).
    let server: E = if form.server_open {
        column![
            text("Sonar server").size(14).color(rgb(0xb7bdc2)),
            text_input("https://your-sonar.example", &form.server).on_input(|v| Msg::AuthField(3, v)).padding(10).size(15).style(t::auth_input()),
        ].spacing(8).into()
    } else {
        button(lead_icon(Icon::Server, format!("Server: {}", form.server.trim_start_matches("https://").trim_start_matches("http://")), rgb(0x76828d), 13.0, 13.0, 7.0)).padding(0).style(t::plain(rgb(0x76828d), rgb(0xd0d7dc))).on_press(Msg::ToggleServer).into()
    };
    card = card.push(container(column![
        t::rule(rgb(0x293039), true),
        row![t::icon(Icon::LockKeyhole, 15.0, rgb(0x76828d)), body("Your messages are locked with a recovery key that only you keep. Sonar never has it, so it can never read your conversations, on the web or the desktop app.", 13.0, rgb(0x76828d), 1.8)].spacing(9).padding(Padding { top: 22.0, ..Padding::ZERO }),
        container(server).padding(Padding { top: 14.0, ..Padding::ZERO }),
    ]).padding(Padding { top: 34.0, ..Padding::ZERO }));
    let side = container(scroll_area(container(card.max_width(345)).center_x(FILL).padding([45, 0]))).width(Length::FillPortion(100)).height(FILL).center_y(FILL).padding([0, 45]).style(t::fill(t::SIDEBAR));
    row![story, t::rule(rgb(0x2a313a), false), side].into()
}

// ----- Unlock / reset key (UnpairedDevicePage) --------------------------------

fn unlock(app: &App) -> E<'_> {
    let reason = app.unlock_reason;
    let resetting = app.unlock_reset;
    let recovery = reason == UnlockReason::Recovery && !resetting;
    let email = app.pending.as_ref().map(|s| s.account.email.clone()).unwrap_or_default();
    let title = if resetting { if reason == UnlockReason::Missing { "Your key lives elsewhere." } else { "Reset your encryption key." } } else { "Unlock your messages." };
    let note = if resetting {
        if reason == UnlockReason::Missing { format!("Signed in as {email}. This account's message key is only on the browser where you first used Sonar.") }
        else { format!("Signed in as {email}. Without your recovery key, Sonar can create a new message key. Messages encrypted for the old key cannot be opened with it.") }
    } else if recovery { format!("Signed in as {email}. Enter the recovery key you saved when you created your account. It unlocks your messages on this device; Sonar never sees it.") }
    else { format!("Signed in as {email}. Your password decrypts your message key on this device. You will then get a recovery key.") };
    let mut card = Column::new().width(FILL)
        .push(t::icon(Icon::LockKeyhole, 30.0, t::ACCENT))
        .push(container(text(title).size(28).font(t::bold()).color(t::TEXT)).padding(Padding { top: 22.0, bottom: 10.0, ..Padding::ZERO }))
        .push(body(note, 15.0, rgb(0x9099a0), 1.8));
    if reason == UnlockReason::Missing {
        card = card.push(container(container(column![
            text("To keep your history").size(15).font(t::semibold()).color(rgb(0xd0d7dc)),
            body("Open Sonar on that browser, then go to Account settings → Privacy & device → Turn on multi-device access. Then sign in here again.", 15.0, rgb(0x98a1a8), 1.6),
        ].spacing(6)).padding([13, 14]).width(FILL).style(t::gradient_panel(120.0, rgb(0x1d2329), rgb(0x1a1f25), rgb(0x293039), 8.0))).padding(Padding { top: 22.0, ..Padding::ZERO }));
    }
    let input = text_input(if recovery { "XXXX-XXXX-XXXX-XXXX-XXXX-XXXX" } else { "" }, &app.unlock_password).on_input(Msg::UnlockPassword).on_submit(Msg::Unlock).secure(!recovery).padding(12).size(16).style(t::auth_input());
    let mut form = Column::new().spacing(20).push(column![text(if recovery { "Recovery key" } else { "Password" }).size(14).color(rgb(0xb7bdc2)), input].spacing(9));
    if resetting && app.unlock_confirm {
        form = form.push(container(body("A new key replaces the old one. Messages encrypted for the old key stay unreadable here, and your contacts will be asked to confirm your new safety code.", 15.0, rgb(0xe3b8a8), 1.6))
            .padding([10, 12]).width(FILL).style(t::panel(rgb(0x222930), rgb(0x4a2f37), 8.0)));
    }
    let label = if app.busy { "Working on it..." } else if !resetting { "Unlock messages" } else if app.unlock_confirm { "Yes, create a new key" } else { "Reset my encryption key" };
    let icon = if !resetting { Icon::KeyRound } else if app.unlock_confirm { Icon::RotateCcw } else { Icon::ArrowUpRight };
    form = form.push(primary_wide(label, icon, (!app.busy && !app.unlock_password.is_empty()).then_some(Msg::Unlock)));
    card = card.push(container(form).padding(Padding { top: 30.0, bottom: 14.0, ..Padding::ZERO }));
    if reason != UnlockReason::Missing {
        let toggle = if resetting { "Back to unlocking" } else if recovery { "Lost your recovery key?" } else { "Forgot your password?" };
        card = card.push(container(button(text(toggle).size(14)).padding([6, 0]).style(t::text_button()).on_press_maybe((!app.busy).then_some(Msg::ToggleUnlockReset))).padding(Padding { bottom: 12.0, ..Padding::ZERO }));
    }
    card = card.push(button(container(lead_icon(Icon::LogOut, "Sign out", rgb(0xd0d7dc), 17.0, 15.0, 12.0)).center_x(FILL)).width(FILL).padding([12, 16]).style(t::secondary()).on_press_maybe((!app.busy).then_some(Msg::SignOut)))
        .push(container(column![
            t::rule(rgb(0x293039), true),
            row![t::icon(Icon::LockKeyhole, 15.0, rgb(0x76828d)), body("Your message key is locked with your recovery key before it is stored. If you lose the recovery key and every signed-in device, encrypted history cannot be recovered.", 13.0, rgb(0x76828d), 1.8)].spacing(9).padding(Padding { top: 22.0, ..Padding::ZERO }),
        ]).padding(Padding { top: 34.0, ..Padding::ZERO }));
    container(scroll_area(container(card.max_width(345)).center_x(FILL).padding([45, 0]))).width(FILL).height(FILL).center_y(FILL).style(t::fill(t::SIDEBAR)).into()
}

// RecoveryKeyPage: shown once whenever a recovery key is created.
fn recovery_view<'a>(app: &'a App, key: &'a str) -> E<'a> {
    let saved = app.recovery_saved;
    let check = iced::widget::checkbox(saved).label("I saved my recovery key somewhere safe").on_toggle(Msg::RecoverySaved).size(14).text_size(14).spacing(9)
        .style(move |_, _| iced::widget::checkbox::Style { background: (if saved { rgb(0xd0d7dc) } else { rgb(0x171b20) }).into(), icon_color: rgb(0x1b2025), border: t::border(if saved { rgb(0xd0d7dc) } else { rgb(0x37414c) }, 1.0, 3.0), text_color: Some(rgb(0xd0d7dc)) });
    #[cfg(windows)]
    let save_file: E = button(container(lead_icon(Icon::Download, "Save as file", rgb(0xd0d7dc), 15.0, 15.0, 10.0)).center_x(FILL)).width(FILL).padding([12, 16]).style(t::secondary()).on_press(Msg::SaveRecoveryFile).into();
    #[cfg(not(windows))]
    let save_file: E = space().into();
    let card = column![
        t::icon(Icon::KeyRound, 30.0, t::ACCENT),
        container(text("Save your recovery key.").size(28).font(t::bold()).color(t::TEXT)).padding(Padding { top: 22.0, bottom: 10.0, ..Padding::ZERO }),
        body("It unlocks your messages on a new device and resets your password if you forget it. Only you have it: Sonar cannot show it again or recover it for you.", 15.0, rgb(0x9099a0), 1.8),
        container(container(text(key).size(15).font(t::REGULAR).color(rgb(0xd0d7dc))).padding([18, 14]).width(FILL).center_x(FILL)
            .style(t::panel(rgb(0x15191e), rgb(0x3a4450), 10.0))).padding(Padding { top: 26.0, bottom: 14.0, ..Padding::ZERO }),
        row![
            button(container(lead_icon(Icon::Copy, "Copy", rgb(0xd0d7dc), 15.0, 15.0, 10.0)).center_x(FILL)).width(FILL).padding([12, 16]).style(t::secondary()).on_press(Msg::CopyRecovery),
            save_file,
        ].spacing(10),
        container(check).padding([18, 0]),
        primary("Continue to Sonar", Some(Icon::ArrowUpRight), saved.then_some(Msg::DismissRecovery)),
        container(column![
            t::rule(rgb(0x293039), true),
            row![t::icon(Icon::LockKeyhole, 15.0, rgb(0x76828d)), body("Store it in a password manager or on paper. Anyone with this key and your email address can reset your password.", 13.0, rgb(0x76828d), 1.8)].spacing(9).padding(Padding { top: 22.0, ..Padding::ZERO }),
        ]).padding(Padding { top: 34.0, ..Padding::ZERO }),
    ].width(FILL);
    container(scroll_area(container(card.max_width(345)).center_x(FILL).padding([45, 0]))).width(FILL).height(FILL).center_y(FILL).style(t::fill(t::SIDEBAR)).into()
}

// ----- Workspace ------------------------------------------------------------

fn workspace(app: &App) -> E<'_> {
    let base = row![rail(app), sidebar(app), conversation(app)].height(FILL);
    let mut layers: Vec<E> = vec![base.into()];
    if let Some(panel) = app.panel { layers.push(panel_view(app, panel)); }
    if app.dialog.is_some() { layers.push(dialog_view(app)); }
    if let Some(confirm) = &app.confirm { layers.push(confirm_view(app, confirm)); }
    if let Some(id) = &app.confirm_delete { layers.push(delete_view(app, id)); }
    if app.settings_open { layers.push(settings_view(app)); }
    stack(layers).into()
}

fn rail_item<'a>(content: E<'a>, active: bool) -> E<'a> {
    let marker: E = if active {
        container(space()).width(3).height(23).style(|_| container::Style { background: Some(rgb(0x70eeff).into()), border: iced::Border { radius: iced::border::Radius { top_left: 0.0, bottom_left: 0.0, top_right: 3.0, bottom_right: 3.0 }, ..Default::default() }, ..Default::default() }).into()
    } else { space().width(3).into() };
    row![marker, grow(), content, grow(), h_space(3.0)].width(76).align_y(Vertical::Center).into()
}
fn rail_button<'a>(icon: Icon, active: bool, unread: bool, on: Msg) -> E<'a> {
    let color = if active { rgb(0x70eeff) } else { rgb(0x76828d) };
    // The web places the unread dot under the icon (grid button).
    let mut inner = Column::new().align_x(Horizontal::Center).spacing(3).push(t::icon(icon, 22.0, color));
    if unread { inner = inner.push(t::dot(t::UNREAD, 8.0)); }
    rail_item(button(container(inner).center_x(46).center_y(44)).padding(0).width(46).height(44).style(t::rail_button(active)).on_press(on).into(), active)
}

fn rail(app: &App) -> E<'_> {
    let name = app.session.as_ref().map(|s| s.account.full_name.as_str()).unwrap_or_default();
    let mut top = Column::new().spacing(14).align_x(Horizontal::Center).width(76)
        .push(rail_item(button(container(t::sonar_mark(0.82)).center_x(44).center_y(42)).padding(0).width(44).height(42).style(t::brand()).on_press(if app.mode == Mode::Direct { Msg::OpenPeer(None) } else { Msg::SetMode(Mode::Direct) }).into(), false))
        .push(container(t::rule(rgb(0x242a31), true)).width(27).padding([6, 0]));
    match app.mode {
        Mode::Direct => {
            top = top.push(rail_button(Icon::MessageCircle, true, false, Msg::OpenPeer(None)))
                .push(rail_button(Icon::Users, false, !app.unread_channels.is_empty(), Msg::SetMode(Mode::Community)))
                .push(rail_button(Icon::ShieldCheck, false, false, Msg::OpenPanel(Some(Panel::Privacy))));
        }
        Mode::Community => {
            top = top.push(rail_button(Icon::MessageCircle, false, !app.unread_direct.is_empty(), Msg::SetMode(Mode::Direct)));
            let list = app.communities.iter().filter(|c| c.kind != "group").fold(Column::new().spacing(12).align_x(Horizontal::Center).padding([4, 0]), |list, item| {
                let active = app.community.as_ref().is_some_and(|c| c.id == item.id);
                let unread = app.unread_channels.iter().any(|key| key.starts_with(&format!("{}:", item.id)));
                let mut face = Row::new().align_y(Vertical::Center).push(text(t::initials(&item.name)).size(16).font(t::semibold()));
                if unread { face = face.push(h_space(3.0)).push(t::dot(t::UNREAD, 8.0)); }
                list.push(rail_item(button(container(face).center_x(43).center_y(43)).padding(0).width(43).height(43).style(t::community_icon(active)).on_press(Msg::OpenCommunity(Some(item.id.clone()))).into(), false))
            });
            top = top.push(scrollable(list).height(Length::Shrink).direction(scrollable::Direction::Vertical(scrollable::Scrollbar::new().width(0).scroller_width(0))))
                .push(rail_item(button(container(t::icon(Icon::Plus, 22.0, rgb(0xb9bfc4))).center_x(46).center_y(44)).padding(0).width(46).height(44).style(t::add_community()).on_press(Msg::OpenDialog(Some(Dialog::Start))).into(), false));
        }
    }
    let bottom = column![
        button(container(t::icon(Icon::Settings, 22.0, rgb(0x97a0a6))).center_x(46).center_y(44)).padding(0).width(46).height(44).style(t::rail_button(false)).on_press(Msg::OpenSettings(true)),
    ].spacing(21).align_x(Horizontal::Center);
    container(column![top, space().height(FILL), bottom].align_x(Horizontal::Center))
        .width(76).height(FILL).padding(Padding { top: 25.0, bottom: 15.0, left: 0.0, right: 0.0 })
        .style(t::gradient_panel(180.0, rgb(0x15181c), rgb(0x0b0c0e), Color::TRANSPARENT, 0.0))
        .into()
}

fn section<'a>(label: &str, extra: Option<E<'a>>) -> E<'a> { section_at(label, extra, 34.0) }
/// A sidebar heading; `top` is the space above it (the first heading sits closer to the header).
fn section_at<'a>(label: &str, extra: Option<E<'a>>, top: f32) -> E<'a> {
    let mut line = Row::new().align_y(Vertical::Center).height(20).push(text(t::tracked(label)).size(12).font(t::bold()).color(rgb(0x75818b))).push(grow());
    if let Some(extra) = extra { line = line.push(extra); }
    container(line).padding(Padding { top, bottom: 9.0, left: 10.0, right: 10.0 }).into()
}
fn nav_row<'a>(icon: Option<Icon>, label: impl text::IntoFragment<'a>, trailing: Option<E<'a>>, selected: bool, on: Option<Msg>) -> E<'a> {
    let color = if selected { rgb(0xd0d7dc) } else { rgb(0x98a1a7) };
    let mut content = Row::new().spacing(11).align_y(Vertical::Center);
    if let Some(icon) = icon { content = content.push(t::icon(icon, 18.0, color)); }
    content = content.push(text(label).size(15).color(color).font(t::REGULAR)).push(grow());
    if let Some(trailing) = trailing { content = content.push(trailing); }
    container(button(content).width(FILL).padding(12).style(t::nav_row(selected)).on_press_maybe(on)).padding([3, 0]).into()
}
fn danger_row<'a>(icon: Option<Icon>, label: &'a str, on: Option<Msg>) -> E<'a> {
    let color = rgb(0xc28b98);
    let mut content = Row::new().spacing(11).align_y(Vertical::Center);
    if let Some(icon) = icon { content = content.push(t::icon(icon, 16.0, color)); }
    content = content.push(text(label).size(15).color(color));
    container(button(content).width(FILL).padding(12).style(t::danger_row()).on_press_maybe(on)).padding(Padding { top: 11.0, bottom: 3.0, ..Padding::ZERO }).into()
}
fn person_row<'a>(who: Face, name: &str, sub: &str, index: usize, dot: Option<Color>, chosen: bool, trailing: Vec<E<'a>>, on: Option<Msg>) -> E<'a> {
    let mut content = Row::new().spacing(11).align_y(Vertical::Center)
        .push(avatar_status(who, index, dot.map(|_| t::SIDEBAR), dot.unwrap_or(t::OFFLINE), 35.0))
        .push(column![text(name.to_owned()).size(15).line_height(LineHeight::Relative(1.2)).font(t::semibold()).color(t::TEXT).wrapping(text::Wrapping::None), text(sub.to_owned()).size(13).line_height(LineHeight::Relative(1.2)).color(rgb(0x78848e))].spacing(2));
    // Like the web row: an unread dot follows the name; the lock sits at the end.
    let mut trailing = trailing;
    let last = trailing.pop();
    for item in trailing { content = content.push(item); }
    content = content.push(grow());
    if let Some(item) = last { content = content.push(item); }
    let inner = button(content).width(FILL).padding([11, 10]).style(t::person_row(chosen)).on_press_maybe(on);
    if chosen {
        stack![inner, container(space()).width(2).height(57).style(|_| container::Style { background: Some(rgb(0x70eeff).into()), border: t::border(Color::TRANSPARENT, 0.0, 1.0), ..Default::default() })].into()
    } else { inner.into() }
}
/// What the voice room banner says about the audio links to everyone else in the room.
/// Same wording as frontend/src/lib/roomStatus.js; failed links are dropped here, so they read as connecting.
fn room_status(joining: bool, others: usize, connected: usize) -> String {
    if joining { return "Connecting…".into(); }
    if others == 0 { return "Waiting for others to join".into(); }
    if connected >= others { return if others == 1 { "Connected".into() } else { "Connected to everyone".into() }; }
    if connected == 0 { return "Connecting…".into(); }
    format!("Connected to {connected} of {others}")
}

fn sidebar_frame<'a>(title: E<'a>, header_action: Option<E<'a>>, content: E<'a>, mini: Option<(Icon, &'a str, &'a str)>, overlay: Option<E<'a>>, app: &'a App) -> E<'a> {
    let name = app.session.as_ref().map(|s| s.account.full_name.as_str()).unwrap_or_default();
    // A community header's title fills the row itself; other headers push their action to the right.
    let mut header = Row::new().spacing(6).align_y(Vertical::Center).push(title);
    if app.mode != Mode::Community { header = header.push(grow()); }
    if let Some(action) = header_action { header = header.push(action); }
    let header = column![container(header).height(82).padding(if app.mode == Mode::Community { [0, 12] } else { [0, 25] }).center_y(82), t::rule(t::LINE, true)];
    let mini: E<'a> = match mini { Some(mini) => container(container(row![t::icon(mini.0, 19.0, rgb(0xd0d7dc)), column![text(mini.1).size(14).color(rgb(0xb4babf)), text(mini.2).size(12).color(rgb(0x7d8892))].spacing(5)].spacing(10).align_y(Vertical::Center))
        .padding([13, 12]).width(FILL).style(t::gradient_panel(120.0, rgb(0x1d2329), rgb(0x1a1f25), rgb(0x293039), 8.0))).padding(Padding { top: 10.0, bottom: 21.0, left: 18.0, right: 18.0 }).into(), None => space().height(0).into() };
    let status = app.session.as_ref().map_or("online", |s| if s.account.presence.is_empty() { "online" } else { s.account.presence.as_str() });
    let status_label = match status { "away" => "Away", "busy" => "Busy", "offline" => "Appear offline", _ => "Online" };
    let mut footer = column![t::rule(t::LINE, true)];
    if app.status_open {
        let mut menu = Column::new().spacing(2).padding(8);
        for (value, label) in [("online", "Online"), ("away", "Away"), ("busy", "Busy"), ("offline", "Appear offline")] {
            menu = menu.push(button(row![t::dot(t::presence_color(value), 9.0), text(label).size(14).color(rgb(0xc8cdd1))].spacing(10).align_y(Vertical::Center)).width(FILL).padding([8, 10]).style(t::person_row(value == status)).on_press(Msg::SetStatus(value)));
        }
        footer = footer.push(container(menu).style(t::fill(t::FOOTER_BG))).push(t::rule(t::LINE, true));
    }
    let footer = footer.push(container(row![
        button(row![
            avatar_status(own_face(app), 0, Some(t::FOOTER_BG), t::presence_color(status), 35.0),
            column![text(name.to_owned()).size(14).line_height(LineHeight::Relative(1.2)).font(t::semibold()).color(t::TEXT).wrapping(text::Wrapping::None), text(status_label).size(12).line_height(LineHeight::Relative(1.2)).color(rgb(0x7d8892))].spacing(2).width(FILL),
        ].spacing(10).align_y(Vertical::Center)).width(FILL).padding(0).style(t::plain(t::TEXT, t::TEXT)).on_press(Msg::ToggleStatusMenu),
        icon_btn(Icon::LogOut, 17.0, Some(Msg::SignOut), false),
    ].spacing(10).align_y(Vertical::Center)).padding([15, 18]).style(t::fill(t::FOOTER_BG)));
    row![
        container(column![header, match overlay {
            Some(overlay) => E::from(stack![scroll_area(container(content).padding([27, 15])).height(FILL), overlay].height(FILL)),
            None => scroll_area(container(content).padding([27, 15])).height(FILL).into(),
        }, mini, footer]).width(299).height(FILL).style(t::fill(t::SIDEBAR)),
        t::rule(t::LINE_SIDEBAR, false),
    ].into()
}

fn sidebar(app: &App) -> E<'_> {
    match app.mode {
        Mode::Direct => {
            let title = row![text("sonar").size(28).font(t::heavy()).color(t::TEXT), container(t::dot(rgb(0xd0d7dc), 5.0)).padding(Padding { top: 16.0, ..Padding::ZERO })].spacing(4).align_y(Vertical::Center);
            let badge = container(text(app.people.len().to_string()).size(13).color(rgb(0x70eeff))).padding([1, 6]).style(t::panel(rgb(0x74808b), rgb(0x74808b), 4.0));
            let query = app.query.to_lowercase();
            let mut list = Column::new().spacing(5);
            if !app.people_loaded { list = list.push(text("Loading people...").size(15).color(t::MUTED)); }
            let prefs = &app.preferences;
            let pinned = |p: &crate::api::Person| prefs.pinned_people.contains(&p.id);
            let filed = |p: &crate::api::Person| prefs.folders.iter().any(|f| f.people.contains(&p.id));
            let mut sections: Vec<(Option<String>, Vec<&crate::api::Person>)> = vec![(prefs.pinned_people.is_empty().then_some(None).unwrap_or(Some("PINNED".to_string())), app.people.iter().filter(|p| pinned(p)).collect())];
            for folder in &prefs.folders { sections.push((Some(folder.name.to_uppercase()), app.people.iter().filter(|p| folder.people.contains(&p.id) && !pinned(p)).collect())); }
            sections.push(((!prefs.pinned_people.is_empty() || !prefs.folders.is_empty()).then(|| "EVERYONE ELSE".to_string()), app.people.iter().filter(|p| !pinned(p) && !filed(p)).collect()));
            let mut index = 0;
            for (title, people) in sections {
                let shown: Vec<_> = people.into_iter().filter(|p| matches_person(p, &query)).collect();
                if shown.is_empty() { continue; }
                if let Some(title) = title { list = list.push(container(text(title).size(11).font(t::semibold()).color(rgb(0x8a959e))).padding(Padding { top: 10.0, bottom: 4.0, left: 8.0, right: 8.0 })); }
                for person in shown {
                    let mut trailing: Vec<E> = Vec::new();
                    if app.unread_direct.contains(&person.id) { trailing.push(t::dot(t::UNREAD, 8.0)); }
                    if person.encryption_key.is_some() { trailing.push(t::icon(Icon::LockKeyhole, 12.0, rgb(0xd0d7dc))); }
                    list = list.push(person_row(person_face(person), &person.name, app.status_text(&person.id, "Around now"), index, app.avatar_dot(&person.id), app.peer.as_deref() == Some(&person.id), trailing, Some(Msg::OpenPeer(Some(person.id.clone())))));
                    index += 1;
                }
            }
            let mut groups = Column::new().spacing(5);
            for item in app.communities.iter().filter(|c| c.kind == "group") {
                let trailing = if app.unread_channels.iter().any(|key| key.starts_with(&format!("{}:", item.id))) { vec![t::dot(t::UNREAD, 8.0), space().into()] } else { vec![] };
                groups = groups.push(person_row(Face::Animal(Icon::Users), &item.name, &format!("{} people", item.members.len()), 0, None, false, trailing, Some(Msg::OpenGroup(item.id.clone()))));
            }
            if app.people_loaded && app.people.is_empty() {
                list = list.push(container(body("Your space starts with a hello. Go to People and add someone by their username.", 15.0, rgb(0x7f8a94), 1.8)).padding(12));
            }
            let search = container(row![
                t::icon(Icon::Search, 15.0, rgb(0x74808b)),
                text_input("Find a conversation", &app.query).on_input(Msg::Query).size(14).padding(0).style(t::bare_input()),
                text("/").size(16).color(rgb(0x74808b)),
            ].spacing(8).align_y(Vertical::Center)).padding([9, 10]).style(t::panel(rgb(0x121518), rgb(0x222930), 6.0));
            let content = column![
                container(text(t::tracked("YOUR SPACE")).size(12).font(t::bold()).color(rgb(0x74808b))).padding(Padding { left: 11.0, bottom: 13.0, ..Padding::ZERO }),
                nav_row(Some(Icon::MessageCircle), "Direct messages", Some(badge.into()), true, Some(Msg::OpenPanel(None))),
                nav_row(Some(Icon::Users), "People", Some(if app.friend_requests.incoming.is_empty() { t::icon(Icon::ArrowUpRight, 15.0, rgb(0x98a1a7)).into() } else { E::from(container(text(app.friend_requests.incoming.len().to_string()).size(13).color(rgb(0x70eeff))).padding([1, 6]).style(t::panel(rgb(0x74808b), rgb(0x74808b), 4.0))) }), false, Some(Msg::OpenPanel(Some(Panel::People)))),
                section("CONVERSATIONS", Some(icon_btn(Icon::Search, 15.0, Some(Msg::OpenPanel(Some(Panel::People))), false))),
                container(search).padding(Padding { left: 5.0, right: 5.0, bottom: 13.0, ..Padding::ZERO }),
                list,
                section("GROUPS", Some(icon_btn(Icon::Plus, 15.0, Some(Msg::OpenPanel(Some(Panel::Group))), false))),
                groups,
            ];
            sidebar_frame(title.into(), Some(icon_btn(Icon::ChevronDown, 18.0, Some(Msg::OpenPanel(Some(Panel::Privacy))), false)), content.into(), Some((Icon::ShieldCheck, "A little more private.", "Messages disappear after 7 days.")), None, app)
        }
        Mode::Community => {
            // The name opens one menu with every community action; the list below is only channels and rooms.
            let back = icon_btn(Icon::ArrowLeft, 18.0, Some(Msg::SetMode(Mode::Direct)), false);
            let mut content = Column::new();
            let mut overlay = None;
            let (title, header_action): (E, Option<E>) = match &app.community {
                Some(community) => {
                    let count = community.members.len();
                    let online = community.members.iter().filter(|id| app.online.contains(*id)).count();
                    let name = column![
                        text(community.name.clone()).size(17).font(t::heavy()).color(t::TEXT).wrapping(text::Wrapping::None),
                        text(format!("{count} {} · {online} online", if count == 1 { "member" } else { "members" })).size(12).color(rgb(0x89939a)),
                    ].spacing(2).width(FILL).clip(true);
                    let opener = button(row![name, t::icon(Icon::ChevronDown, 16.0, rgb(0x8c969d))].spacing(6).align_y(Vertical::Center))
                        .width(FILL).padding([6, 8]).style(t::person_row(false)).on_press(Msg::ToggleManage);
                    (row![back, opener].spacing(6).align_y(Vertical::Center).width(FILL).into(), Some(icon_btn(Icon::Users, 18.0, Some(Msg::ToggleMembers), app.members_open)))
                }
                None => (row![back, text("Communities").size(28).font(t::heavy()).color(t::TEXT)].spacing(10).align_y(Vertical::Center).into(), None),
            };
            match &app.community {
                Some(community) => {
                    let owner = app.owner();
                    let moderator = app.moderator();
                    let group = community.kind == "group";
                    content = content.push(section_at("CHANNELS", (owner && !group).then(|| icon_btn(Icon::Plus, 16.0, Some(Msg::NewChannel(false)), false)), 6.0));
                    for channel in community.channels.iter().filter(|c| c.kind != "voice") {
                        let selected = app.channel.as_deref() == Some(&channel.id);
                        let mut trailing = Row::new().spacing(8).align_y(Vertical::Center);
                        if app.unread_channels.contains(&format!("{}:{}", community.id, channel.id)) { trailing = trailing.push(t::dot(t::UNREAD, 8.0)); }
                        if selected { trailing = trailing.push(t::icon(Icon::LockKeyhole, 11.0, rgb(0xd0d7dc))); }
                        content = content.push(nav_row(Some(Icon::Hash), channel.name.clone(), Some(trailing.into()), selected, Some(Msg::OpenChannel(channel.id.clone()))));
                    }
                    content = content.push(voice_rooms(app, community));
                    if app.manage_open {
                        let item = |icon: Icon, label: String, danger: bool, on: Msg| -> E {
                            let color = if danger { rgb(0xc28b98) } else { rgb(0xc8cdd1) };
                            button(row![t::icon(icon, 16.0, if danger { color } else { rgb(0x8c969d) }), text(label).size(14).color(color)].spacing(10).align_y(Vertical::Center))
                                .width(FILL).padding([9, 10]).style(t::person_row(false)).on_press(on).into()
                        };
                        let line = || -> E { container(t::rule(rgb(0x272e35), true)).padding([4, 2]).into() };
                        let muted = app.muted(&community.id);
                        let kind = if group { "group" } else { "server" };
                        let mut menu = Column::new().spacing(2);
                        if owner && !group { menu = menu.push(item(Icon::UserPlus, "Invite your people".into(), false, Msg::OpenDialog(Some(Dialog::Invite)))); }
                        menu = menu.push(item(if muted { Icon::Bell } else { Icon::BellOff }, format!("{} {kind}", if muted { "Unmute" } else { "Mute" }), false, Msg::ToggleMute(community.id.clone())));
                        if (owner || moderator) && !group {
                            menu = menu.push(line()).push(container(text(t::tracked("MANAGE")).size(11).font(t::bold()).color(rgb(0x74808b))).padding(Padding { top: 6.0, bottom: 2.0, left: 10.0, right: 10.0 }));
                            menu = menu.push(item(Icon::Users, "Who joined or left".into(), false, Msg::OpenDialog(Some(Dialog::Activity))));
                            if app.may("lock") {
                                for room in community.channels.iter().filter(|c| c.kind == "voice") {
                                    let (icon, verb) = if room.voice_locked { (Icon::LockOpen, "Unlock") } else { (Icon::Lock, "Lock") };
                                    menu = menu.push(item(icon, format!("{verb} {}", room.name), false, Msg::RoomLock(room.id.clone(), !room.voice_locked)));
                                }
                            }
                            if owner {
                                menu = menu.push(item(Icon::Pencil, "Rename community".into(), false, Msg::OpenDialog(Some(Dialog::RenameCommunity))))
                                    .push(item(Icon::Pencil, "Rename a channel or room".into(), false, Msg::OpenDialog(Some(Dialog::RenameChannel))));
                            }
                        }
                        menu = menu.push(line());
                        if owner && !group {
                            menu = menu.push(item(Icon::Trash, "Delete a channel or room".into(), true, Msg::OpenDialog(Some(Dialog::DeleteChannel))))
                                .push(item(Icon::Trash, "Delete community".into(), true, Msg::AskConfirm(Some(Confirm { action: "delete-community".into(), member: None, channel: None }))));
                        }
                        if !owner {
                            menu = menu.push(item(Icon::LogOut, if group { "Leave group" } else { "Leave community" }.into(), true, Msg::AskConfirm(Some(Confirm { action: "leave".into(), member: None, channel: None }))));
                        }
                        // Clicking anywhere else in the sidebar closes the menu.
                        let away = mouse_area(container(space()).width(FILL).height(FILL)).on_press(Msg::ToggleManage);
                        let card = container(container(menu).padding(6).width(FILL).style(|_| container::Style {
                            background: Some(rgb(0x161a1e).into()), border: t::border(rgb(0x2c343d), 1.0, 10.0),
                            shadow: iced::Shadow { color: Color { a: 0.55, ..Color::BLACK }, offset: iced::Vector::new(0.0, 12.0), blur_radius: 32.0 }, ..Default::default()
                        })).padding(Padding { top: 6.0, left: 12.0, right: 12.0, bottom: 0.0 });
                        overlay = Some(stack![away, card].width(FILL).height(FILL).into());
                    }
                }
                None => {
                    content = content.push(section_at("YOUR COMMUNITIES", None, 6.0));
                    for (index, item) in app.communities.iter().filter(|c| c.kind != "group").enumerate() {
                        content = content.push(person_row(Face::Letters(item.name.clone()), &item.name, &format!("{} members", item.members.len()), index, None, false, vec![t::icon(Icon::ArrowUpRight, 15.0, rgb(0x98a1a7))], Some(Msg::OpenCommunity(Some(item.id.clone())))));
                    }
                    content = content.push(nav_row(Some(Icon::Plus), "Create a community", None, false, Some(Msg::OpenDialog(Some(Dialog::Create)))))
                        .push(nav_row(Some(Icon::Users), "Join with an invite", None, false, Some(Msg::OpenDialog(Some(Dialog::Join)))));
                }
            }
            sidebar_frame(title, header_action, content.into(), None, overlay, app)
        }
    }
}

// ----- Conversation ------------------------------------------------------------

fn encrypted_badge<'a>() -> E<'a> {
    container(lead_icon(Icon::LockKeyhole, "Encrypted", rgb(0xa6aeb4), 12.0, 12.0, 6.0)).padding([5, 8]).style(t::panel(rgb(0x21272d), rgb(0x2d343d), 5.0)).into()
}

fn conversation(app: &App) -> E<'_> {
    let me = app.me();
    let (identity, actions): (E, Vec<E>) = match app.mode {
        Mode::Direct => match app.selected_peer() {
            Some(peer) => {
                let online = app.online.contains(&peer.id);
                (row![
                    avatar_status(person_face(peer), 0, online.then_some(t::CONVERSATION), app.status_color(&peer.id), 35.0),
                    column![
                        text(peer.name.clone()).size(16).font(t::semibold()).color(t::TEXT),
                        row![t::dot(if !online { t::OFFLINE } else if app.presence.contains_key(&peer.id) { app.status_color(&peer.id) } else { t::GREEN }, 5.0), text(app.status_text(&peer.id, "Online").to_owned()).size(13).color(rgb(0x7e8993)), text("/").size(13).color(rgb(0x3a444f)), text("Direct message").size(13).color(rgb(0x7e8993))].spacing(5).align_y(Vertical::Center),
                    ].spacing(7),
                ].spacing(12).align_y(Vertical::Center).into(),
                vec![
                    container(encrypted_badge()).padding(Padding { right: 8.0, ..Padding::ZERO }).into(),
                    {
                        let phone = icon_btn(Icon::Phone, 19.0, (online && app.call.is_none() && app.incoming.is_none() && app.live_connected).then_some(Msg::AskCall), false);
                        if online { phone } else {
                            let first = peer.name.split_whitespace().next().unwrap_or_default();
                            tooltip(phone, container(text(format!("{first} is offline. Calls need both people online.")).size(13).color(rgb(0xd0d7dc))).padding([6, 10]).style(t::panel(rgb(0x1e2329), rgb(0x353e49), 6.0)), tooltip::Position::Bottom).into()
                        }
                    },
                    icon_btn(Icon::Search, 19.0, Some(Msg::OpenPanel(Some(Panel::Search))), false),
                    icon_btn(Icon::Pin, 19.0, Some(Msg::TogglePins), app.thread.pins),
                    icon_btn(Icon::Ellipsis, 21.0, Some(Msg::OpenPanel(Some(Panel::Privacy))), false),
                ])
            }
            None => (row![t::icon(Icon::MessageCircle, 22.0, rgb(0xd0d7dc)), text("Direct messages").size(16).font(t::semibold()).color(t::TEXT)].spacing(12).align_y(Vertical::Center).into(),
                vec![container(encrypted_badge()).padding(Padding { right: 8.0, ..Padding::ZERO }).into(), icon_btn(Icon::Ellipsis, 21.0, Some(Msg::OpenPanel(Some(Panel::Privacy))), false)]),
        },
        Mode::Community => {
            let channel = app.channel_info();
            let mut actions: Vec<E> = vec![container(encrypted_badge()).padding(Padding { right: 8.0, ..Padding::ZERO }).into()];
            if channel.is_some() { actions.push(icon_btn(Icon::Search, 18.0, Some(Msg::OpenPanel(Some(Panel::Search))), false)); }
            if channel.is_some() { actions.push(icon_btn(Icon::Pin, 18.0, Some(Msg::TogglePins), app.thread.pins)); }
            if app.community.is_some() { actions.push(icon_btn(Icon::Users, 19.0, Some(Msg::ToggleMembers), app.members_open)); }
            (row![t::icon(Icon::Hash, 22.0, rgb(0xd0d7dc)), column![
                text(channel.map(|c| c.name.clone()).unwrap_or_else(|| "Your communities".into())).size(16).font(t::semibold()).color(t::TEXT),
                text(app.community.as_ref().map(|c| c.name.clone()).unwrap_or_else(|| "A place for your people".into())).size(13).color(rgb(0x7e8993)),
            ].spacing(7)].spacing(12).align_y(Vertical::Center).into(), actions)
        }
    };
    let top = column![
        container(row![identity, grow(), Row::from_vec(actions).spacing(12).align_y(Vertical::Center)].align_y(Vertical::Center)).height(82).padding([0, 33]).center_y(82),
        t::rule(t::LINE_CHAT, true),
    ];
    let disappear = if app.mode == Mode::Direct { app.selected_peer().map(|p| p.disappear_after) } else { app.channel_info().map(|c| c.disappear_after) };
    let retention_text = match disappear {
        Some(seconds) if seconds > 0 => format!("Disappearing messages are on. New messages vanish after {}.", if seconds == 3600 { "1 hour" } else { "1 day" }),
        _ if app.mode == Mode::Direct => "A conversation, not a permanent record. Messages disappear after 7 days.".to_owned(),
        _ => "Messages disappear after 7 days. Pin the ones worth keeping.".to_owned(),
    };
    let mut banner = Row::new().spacing(8).align_y(Vertical::Center).push(t::icon(Icon::Timer, 14.0, rgb(0xd0d7dc))).push(text(retention_text).size(13).color(rgb(0x8a949b)));
    if let Some(seconds) = disappear.filter(|_| app.mode == Mode::Direct || app.group() || app.owner() || app.moderator()) {
        banner = banner.push(pick_list(SPANS.to_vec(), SPANS.iter().find(|s| s.seconds == seconds).cloned(), |span: Span| Msg::Disappear(span.seconds)).text_size(12).padding([2, 6]).style(t::select()).menu_style(t::menu()));
    }
    if app.mode == Mode::Direct {
        banner = banner.push(h_space(6.0)).push(button(row![text("How it works").size(13).color(rgb(0xd0d7dc)), t::icon(Icon::ArrowUpRight, 12.0, rgb(0xd0d7dc))].spacing(5).align_y(Vertical::Center)).padding(0).style(t::plain(rgb(0xd0d7dc), rgb(0xd0d7dc))).on_press(Msg::OpenPanel(Some(Panel::Privacy))));
    }
    let retention = column![container(banner).width(FILL).center_x(FILL).padding([12, 20]).style(t::fill(rgb(0x1b2025))), t::rule(rgb(0x232a31), true)];

    let mut main = Column::new().push(top).push(retention);
    if let Some(banner) = update_banner(app) { main = main.push(banner); }
    if let Some(banner) = waiting_banner(app) { main = main.push(banner); }
    if let Some(banner) = call_banner(app) { main = main.push(banner); }
    let thread: E = match app.mode {
        Mode::Direct => direct_thread(app, me),
        Mode::Community => community_thread(app, me),
    };
    let footer = column![t::rule(rgb(0x222930), true), container(row![
        row![t::dot(t::GREEN, 5.0), text(if app.mode == Mode::Direct { "Your space. Your pace." } else { "Your space. Your people." }).size(12).color(rgb(0x74808b))].spacing(7).align_y(Vertical::Center),
        grow(),
        text(if app.live_connected { "Always free. Always ad-free." } else { "Reconnecting..." }).size(12).color(rgb(0x74808b)),
    ].align_y(Vertical::Center)).padding(Padding { top: 12.0, bottom: 15.0, left: 33.0, right: 33.0 })];
    let body_row: E = if app.mode == Mode::Community && app.members_open && app.community.is_some() {
        row![container(thread).width(FILL).height(FILL), t::rule(t::LINE_CHAT, false), members_panel(app)].height(FILL).into()
    } else { container(thread).width(FILL).height(FILL).into() };
    main = main.push(body_row).push(footer);
    container(main).width(FILL).height(FILL).style(t::gradient_panel(180.0, rgb(0x21272e), rgb(0x15191d), Color::TRANSPARENT, 0.0)).into()
}

// VoiceRoomList: rooms, occupancy, lock controls and participants.
fn voice_rooms<'a>(app: &'a App, community: &'a crate::api::Community) -> E<'a> {
    let (owner, moderator, me) = (app.owner(), app.moderator(), app.me());
    let add = owner.then(|| icon_btn(Icon::Plus, 16.0, Some(Msg::NewChannel(true)), false));
    let mut list = Column::new().push(container(section("VOICE ROOMS", add)).padding(Padding { top: 0.0, ..Padding::ZERO }));
    let rooms: Vec<_> = community.channels.iter().filter(|c| c.kind == "voice").collect();
    if rooms.is_empty() {
        list = list.push(container(body(if owner { "Make room for a real conversation. Create your first voice room." } else { "No voice rooms yet. The owner can create one." }, 13.0, rgb(0x7a8690), 1.8)).padding(Padding { left: 10.0, right: 10.0, bottom: 9.0, ..Padding::ZERO }));
    }
    for channel in rooms {
        let active = app.room.as_ref().is_some_and(|r| r.community == community.id && r.channel == channel.id);
        let participants = app.rooms.get(&format!("{}:{}", community.id, channel.id)).cloned().unwrap_or_default();
        let color = if active { rgb(0xd0d7dc) } else { rgb(0x98a1a7) };
        let mut row_content = Row::new().spacing(11).align_y(Vertical::Center).push(t::icon(Icon::Volume, 17.0, color)).push(text(channel.name.clone()).size(15).color(color));
        if channel.voice_locked { row_content = row_content.push(t::icon(Icon::Lock, 13.0, color)); }
        row_content = row_content.push(grow()).push(text(format!("{}/6", participants.len())).size(12).color(rgb(0x848f97)));
        let style: Box<dyn Fn(&iced::Theme, button::Status) -> button::Style> = if active {
            Box::new(|_, _| button::Style { background: Some(rgb(0x222930).into()), text_color: rgb(0xd0d7dc), border: t::border(Color::TRANSPARENT, 0.0, 7.0), ..Default::default() })
        } else { t::nav_row(false) };
        list = list.push(container(button(row_content).width(FILL).padding(12).style(style).on_press_maybe((!active && !channel.voice_locked).then(|| Msg::JoinRoom(channel.id.clone())))).padding(Padding { bottom: 1.0, top: 3.0, ..Padding::ZERO }));
        let mut people = Column::new().padding(Padding { top: 3.0, bottom: 8.0, left: 29.0, right: 8.0 });
        for person in &participants {
            let mut line = Row::new().spacing(7).align_y(Vertical::Center).push(t::dot(app.status_color(&person.user_id), 5.0))
                .push(text(format!("{}{}", person.name, if person.user_id == me { " (you)" } else { "" })).size(13).color(rgb(0xd0d7dc)).width(FILL));
            let can_kick = (owner || moderator) && person.user_id != me && Some(&person.user_id) != community.owner_id.as_ref() && (owner || !community.moderators.contains(&person.user_id));
            if can_kick {
                line = line.push(button(t::icon(Icon::LogOut, 12.0, rgb(0xd0d7dc))).padding(3).style(t::plain(rgb(0xd0d7dc), rgb(0xd0d7dc))).on_press(Msg::RoomKick(channel.id.clone(), person.clone())));
            }
            if person.muted || person.deafened { line = line.push(t::icon(Icon::MicOff, 11.0, rgb(0xd0d7dc))); }
            people = people.push(container(line).padding([7, 2]));
        }
        if !participants.is_empty() { list = list.push(people); }
    }
    list.into()
}

/// A second call arriving during a connected call. Mirrors VoiceCallBanner.jsx.
fn update_banner(app: &App) -> Option<E<'_>> {
    if app.call.is_some() || app.room.is_some() { return None; }
    let version = app.update_ready.as_ref()?;
    Some(container(container(row![
        t::icon(Icon::Download, 20.0, rgb(0xd7f3ff)),
        column![text("A new version of Sonar is ready").size(15).font(t::semibold()).color(rgb(0xd7f3ff)), text(format!("Version {version}")).size(13).color(rgb(0xa9d8ee))].spacing(4).width(FILL),
        button(text("Restart to update").size(15).font(t::semibold()).color(rgb(0x1e2329))).padding([8, 14]).style(t::primary()).on_press(Msg::UpdateRestart),
    ].spacing(14).align_y(Vertical::Center)).padding(14).width(FILL).style(t::panel(rgb(0x12405f), rgb(0x3f9dff), 9.0)))
        .padding(Padding { top: 16.0, left: 33.0, right: 33.0, bottom: 0.0 }).into())
}

fn waiting_banner(app: &App) -> Option<E<'_>> {
    let in_room = app.waiting.is_none();
    let waiting = app.waiting.as_ref().or(app.incoming.as_ref().filter(|_| app.room.is_some()))?;
    let name = app.person(&waiting.from).map(|p| p.name.clone()).unwrap_or_else(|| "someone".into());
    Some(container(container(row![
        t::icon(Icon::PhoneIncoming, 23.0, rgb(0xd0d7dc)),
        column![text(format!("Incoming call from {name}")).size(15).font(t::semibold()).color(rgb(0xd0d7dc)), text(if in_room { "Swapping leaves the voice room" } else { "Swapping ends your current call" }).size(13).color(rgb(0x98a1a8))].spacing(4).width(FILL),
        button(text("Swap").size(15).font(t::semibold()).color(rgb(0x1e2329))).padding([8, 14]).style(t::primary()).on_press(Msg::SwapCall),
        button(t::icon(Icon::PhoneOff, 18.0, Color::WHITE)).padding(10).style(t::end_call()).on_press(Msg::DeclineWaiting),
    ].spacing(14).align_y(Vertical::Center)).padding(14).width(FILL).style(t::panel(rgb(0x21272e), rgb(0x74808b), 9.0)))
        .padding(Padding { top: 16.0, left: 33.0, right: 33.0, bottom: 0.0 }).into())
}

fn call_banner(app: &App) -> Option<E<'_>> {
    let name = |id: &str| app.person(id).map(|p| p.name.clone()).or_else(|| app.member(id).map(|p| p.name.clone())).unwrap_or_else(|| "Someone".into());
    let style = t::panel(rgb(0x12405f), rgb(0x3f9dff), 9.0);
    // Mirrors VoiceCallBanner.jsx: same titles, subtitle and controls.
    let pill = |relay: bool| -> E { container(lead_icon(Icon::LockKeyhole, if relay { "Relay only" } else { "Encrypted audio" }, rgb(0xbfeaff), 11.0, 11.0, 5.0)).padding([6, 7]).style(t::panel(rgb(0x0f3147), rgb(0x3f9dff), 5.0)).into() };
    let end_call = |on: Msg| -> E { button(t::icon(Icon::PhoneOff, 18.0, Color::WHITE)).padding(10).style(t::end_call()).on_press(on).into() };
    let content: E = if let Some(incoming) = app.incoming.as_ref().filter(|_| app.room.is_none()) {
        row![
            t::icon(Icon::AudioLines, 23.0, rgb(0x70eeff)),
            column![text("Incoming voice call").size(15).font(t::semibold()).color(rgb(0xeafaff)), text(format!("{} · No recordings", name(&incoming.from))).size(13).color(rgb(0xa9d8ee))].spacing(4).width(FILL),
            pill(app.settings.relay_only),
            button(text("Accept").size(15).font(t::semibold()).color(rgb(0x1e2329))).padding([8, 14]).style(t::primary()).on_press_maybe((app.call.is_none() && app.room.is_none()).then_some(Msg::AcceptCall)),
            end_call(Msg::DeclineCall),
        ].spacing(14).align_y(Vertical::Center).into()
    } else if let Some(room) = &app.room {
        let joining = room.spec.is_none() || room.controls.is_none();
        return Some(container(container(row![
            t::icon(Icon::Volume, 23.0, rgb(0x70eeff)),
            column![
                text(if joining { "Joining voice room...".to_string() } else { format!("Voice room: {}", room.name) }).size(15).font(t::semibold()).color(rgb(0xeafaff)),
                text(format!("{} · {}/6 people · {}", room.community_name, room.participants.len(), {
                    let mine = room.spec.as_ref().map(|s| s.session_id.as_str()).unwrap_or_default();
                    room_status(joining, room.participants.iter().filter(|p| p.session_id != mine).count(), room.connections)
                })).size(13).color(rgb(0xa9d8ee)),
            ].spacing(4).width(FILL),
            container(lead_icon(Icon::LockKeyhole, if app.settings.relay_only { "Relay only" } else { "Encrypted audio" }, rgb(0xbfeaff), 11.0, 11.0, 5.0)).padding([6, 7]).style(t::panel(rgb(0x0f3147), rgb(0x3f9dff), 5.0)),
            mic_btn(room.muted, app.mic_active && !room.deafened, Msg::RoomMute),
            call_btn(if room.deafened { Icon::HeadphoneOff } else { Icon::Headphones }, 18.0, Some(Msg::RoomDeafen), room.deafened),
            screen_button(app),
            button(t::icon(Icon::LogOut, 18.0, Color::WHITE)).padding(10).style(t::end_call()).on_press(Msg::LeaveRoom),
        ].spacing(14).align_y(Vertical::Center)).padding(14).width(FILL).style(t::panel(rgb(0x12405f), rgb(0x3f9dff), 9.0)))
            .padding(Padding { top: 16.0, left: 33.0, right: 33.0, bottom: 0.0 }).into());
    } else if let Some(call) = &app.call {
        let title = match call.status { CallStatus::Calling => "Calling...", CallStatus::Connecting => "Connecting voice...", CallStatus::Connected => "Voice connected" };
        let duration = call.started.map(|s| { let secs = s.elapsed().as_secs(); format!(" · {}:{:02}", secs / 60, secs % 60) }).unwrap_or_default();
        row![
            t::icon(Icon::AudioLines, 23.0, rgb(0x70eeff)),
            column![text(title).size(15).font(t::semibold()).color(rgb(0xeafaff)), text(format!("{}{duration} · No recordings", name(&call.spec.peer))).size(13).color(rgb(0xa9d8ee))].spacing(4).width(FILL),
            pill(call.spec.relay_only),
            mic_btn(call.muted, app.mic_active && !call.deafened, Msg::Mute),
            call_btn(if call.deafened { Icon::HeadphoneOff } else { Icon::Headphones }, 18.0, Some(Msg::Deafen), call.deafened),
            if call.status == CallStatus::Connected { screen_button(app) } else { space().width(0).into() },
            end_call(Msg::Hangup),
        ].spacing(14).align_y(Vertical::Center).into()
    } else { return None };
    Some(container(container(content).padding(14).width(FILL).style(style)).padding(Padding { top: 16.0, left: 33.0, right: 33.0, bottom: 0.0 }).into())
}

fn welcome<'a>(symbol: E<'a>, eyebrow: &'a str, title: &'a str, lines: [&'a str; 2], buttons: Vec<E<'a>>, values: [(Icon, &'a str); 3]) -> E<'a> {
    // Two sonar rings leaving the emblem, half a cycle apart. Mirrors .orbit in index.css.
    let ms = t::PULSE.load(std::sync::atomic::Ordering::Relaxed);
    let ring = |offset: u32| -> E<'a> {
        let t_cycle = ((ms + offset) % 3000) as f32 / 3000.0;
        let eased = 1.0 - (1.0 - t_cycle) * (1.0 - t_cycle);
        let size = 114.0 + 72.0 * eased;
        container(container(space()).width(size).height(size).style(t::panel(Color::TRANSPARENT, Color { a: 0.55 * (1.0 - eased), ..rgb(0x70eeff) }, size / 2.0))).width(186).height(186).center_x(186).center_y(186).into()
    };
    let orbit = stack![t::orbit::<Msg>(), ring(0), ring(1500), container(symbol).width(186).height(186).center_x(186).center_y(186)];
    let values = values.into_iter().fold(Row::new().spacing(28), |row, (icon, label)| row.push(lead_icon(icon, label, rgb(0x74808b), 16.0, 12.0, 7.0)));
    center(column![
        container(orbit).padding(Padding { bottom: 0.0, ..Padding::ZERO }),
        container(t::eyebrow(eyebrow, 12.0, t::EYEBROW)).padding(Padding { top: 16.0, ..Padding::ZERO }),
        container(text(title).size(36).font(t::bold()).color(rgb(0xd0d7dc))).padding([15, 0]),
        body(lines[0], 16.0, rgb(0x8f989f), 1.9).align_x(Horizontal::Center),
        body(lines[1], 16.0, rgb(0x8f989f), 1.9).align_x(Horizontal::Center),
        container(Row::from_vec(buttons).spacing(12)).padding(Padding { top: 27.0, ..Padding::ZERO }),
        container(values).padding(Padding { top: 65.0, ..Padding::ZERO }),
    ].align_x(Horizontal::Center)).padding([55, 10]).into()
}

fn date_divider<'a>(label: &str) -> E<'a> {
    container(row![t::rule(t::LINE_CHAT, true), text(t::tracked(label)).size(11).color(rgb(0x74808b)), t::rule(t::LINE_CHAT, true)].spacing(15).align_y(Vertical::Center)).padding(Padding { top: 8.0, bottom: 24.0, ..Padding::ZERO }).into()
}
fn notice<'a>(value: &'a str) -> E<'a> { container(text(value).size(15).color(rgb(0x74808b))).padding(24).into() }

fn invite_card<'a>(app: &'a App, token: String) -> E<'a> {
    let (mut title, mut detail) = ("Checking invite...".to_string(), "Community invite".to_string());
    let mut join = None;
    match app.invite_previews.get(&token).and_then(Option::as_ref) {
        None => {}
        Some(Err(_)) => { title = "Could not load this invite".into(); detail = "Check your connection and reopen the conversation.".into(); }
        Some(Ok(preview)) if !preview.available => { title = "Invite expired".into(); detail = "Ask for a new invite link.".into(); }
        Some(Ok(preview)) => {
            let members = format!("{} {}", preview.member_count, if preview.member_count == 1 { "member" } else { "members" });
            title = preview.name.clone();
            detail = if preview.member { format!("{members} · You're a member") } else if preview.full { format!("{members} · Community full") } else { format!("{members} · Invite only") };
            if preview.member {
                join = Some(action("Open", None, t::secondary(), rgb(0xd0d7dc), (!app.busy).then(|| Msg::DialogDone(Ok(preview.id.clone()))), [8.0, 14.0], 14.0));
            } else if !preview.full {
                join = Some(action(if app.busy { "Joining..." } else { "Join" }, Some(Icon::ArrowUpRight), t::primary(), rgb(0x1e2329), (!app.busy).then(|| Msg::JoinInvite(token.clone())), [8.0, 14.0], 14.0));
            }
        }
    }
    let mut card = Row::new().spacing(13).align_y(Vertical::Center)
        .push(container(t::icon(Icon::Users, 19.0, rgb(0xd0d7dc))).center_x(40).center_y(40).style(t::panel(rgb(0x272e37), rgb(0x272e37), 8.0)))
        .push(column![text(title).size(16).font(t::semibold()).color(t::TEXT).wrapping(text::Wrapping::WordOrGlyph), text(detail).size(13).color(rgb(0x869098))].spacing(2).width(FILL));
    if let Some(join) = join { card = card.push(join); }
    container(container(card).padding(14).max_width(440).style(t::panel(rgb(0x1e2329), rgb(0x2a313a), 8.0))).padding(Padding { top: 4.0, ..Padding::ZERO }).into()
}

fn message_list<'a>(app: &'a App, me: &'a str, community: bool) -> Vec<E<'a>> {
    let reactions = crate::chat::reactions(&app.thread.messages, app.now);
    let visible: Vec<&crate::chat::Message> = app.thread.messages.iter().filter(|m| m.reaction.is_none() && !app.blocked(&m.sender) && m.live(app.now) && (!app.thread.pins || m.pinned)).collect();
    let mut out = Vec::new();
    if app.thread.has_more {
        let label = if app.thread.loading_older { "Decrypting older messages..." } else if app.thread.pins { "Load older pinned messages" } else { "Load older messages" };
        out.push(container(button(text(label).size(13).color(rgb(0xd0d7dc))).padding([10, 16]).style(t::load_older()).on_press_maybe((!app.thread.loading_older && !app.thread.loading).then_some(Msg::LoadOlder))).center_x(FILL).padding(Padding { bottom: 22.0, ..Padding::ZERO }).into());
    }
    if app.thread.loading {
        out.push(notice(if community { "Opening your encrypted channel..." } else { "Decrypting your conversation..." }));
        return out;
    }
    if visible.is_empty() {
        out.push(notice(match (app.thread.pins, community) { (true, _) => "No pinned messages yet.", (false, true) => "Nothing here yet. Start with a hello.", (false, false) => "A clean slate. Send the first hello." }));
    }
    for message in visible.iter().copied() {
        let sender_name = if message.sender == me { app.session.as_ref().map(|s| s.account.full_name.clone()).unwrap_or_default() }
            else if community { app.member(&message.sender).map(|p| p.name.clone()).unwrap_or_else(|| "Member".into()) }
            else { app.selected_peer().map(|p| p.name.clone()).unwrap_or_default() };
        let mut meta = Row::new().spacing(10).align_y(Vertical::Center).push(text(sender_name.clone()).size(15).font(t::semibold()).color(t::TEXT));
        if message.sender == me { meta = meta.push(container(text(t::tracked("YOU")).size(10).color(rgb(0xd0d7dc))).padding([1, 4]).style(t::panel(rgb(0x2a313a), rgb(0x2a313a), 3.0))); }
        meta = meta.push(text(if community { message.date_time() } else { message.time() }).size(12).color(rgb(0x74808b)));
        if message.edited { meta = meta.push(text("(edited)").size(11).color(rgb(0x8a959e))); }
        if message.pinned { meta = meta.push(lead_icon(Icon::Pin, "Pinned", rgb(0xd0d7dc), 11.0, 12.0, 4.0)); }
        let mut content = Column::new().width(FILL).push(container(meta).padding(Padding { bottom: 7.0, ..Padding::ZERO }));
        if let Some(target) = &message.reply_to {
            let quoted = visible.iter().find(|m| &m.id == target && !m.unreadable).map(|m| m.text.clone()).unwrap_or_else(|| "Original message unavailable".into());
            content = content.push(container(row![container(space()).width(2).height(18).style(t::fill(rgb(0xd0d7dc))), text(format!("Replying to: {quoted}")).size(14).color(rgb(0xd0d7dc)).wrapping(text::Wrapping::None)].spacing(8).align_y(Vertical::Center)).padding(Padding { top: 3.0, bottom: 5.0, ..Padding::ZERO }));
        }
        if let Some(file) = &message.file {
            let size = if file.size < 1024 * 1024 { format!("{} KB", (file.size / 1024).max(1)) } else { format!("{:.1} MB", file.size as f64 / 1_048_576.0) };
            let card = row![
                container(t::icon(Icon::File, 19.0, rgb(0xd0d7dc))).center_x(40).center_y(40).style(t::panel(rgb(0x272e37), rgb(0x272e37), 8.0)),
                column![text(file.name.clone()).size(16).font(t::semibold()).color(t::TEXT).wrapping(text::Wrapping::WordOrGlyph), text(format!("{size} · Encrypted")).size(13).color(rgb(0x869098))].spacing(2).width(FILL),
                button(container(t::icon(Icon::Download, 16.0, rgb(0xd0d7dc))).center_x(36).center_y(36)).padding(0).width(36).height(36).style(t::secondary()).on_press(Msg::DownloadFile(file.clone())),
            ].spacing(13).align_y(Vertical::Center);
            content = content.push(container(container(card).padding([12, 14]).max_width(440).style(t::panel(rgb(0x1e2329), rgb(0x2a313a), 8.0))).padding(Padding { top: 4.0, ..Padding::ZERO }));
        }
        if message.missed_call {
            let mine = message.sender == me;
            let label = if mine { "You called. No answer.".to_string() } else { format!("Missed call from {}", sender_name.split_whitespace().next().unwrap_or_default()) };
            content = content.push(container(row![t::icon(Icon::PhoneMissed, 16.0, if mine { rgb(0x929ca3) } else { rgb(0xe8909f) }), text(label).size(16).color(rgb(0xb8bec2))].spacing(9).align_y(Vertical::Center)).padding(Padding { top: 2.0, ..Padding::ZERO }));
        }
        let invite = if message.unreadable || message.missed_call || message.file.is_some() { None } else { crate::api::find_invite(&message.text) };
        if !message.missed_call && message.file.is_none() && !invite.as_ref().is_some_and(|(_, only)| *only) {
            let names: Vec<&str> = if community { app.community.iter().flat_map(|c| c.member_details.iter()).map(|m| m.name.as_str()).collect() } else { Vec::new() };
            content = content.push(message_body(message.text.as_str(), message.unreadable, &names));
        }
        if let Some((token, _)) = invite { content = content.push(invite_card(app, token)); }
        if !app.thread.pins {
            let counts = reactions.get(&message.id);
            let open = app.picker.as_deref() == Some(&message.id);
            let mut bar = Row::new().spacing(5);
            for emoji in crate::chat::REACTION_EMOJI {
                let senders = counts.and_then(|list| list.iter().find(|(e, _)| e == emoji)).map(|(_, s)| s.as_slice()).unwrap_or_default();
                if !open && senders.is_empty() { continue; }
                let selected = senders.iter().any(|s| s == me);
                let mut label = Row::new().spacing(4).align_y(Vertical::Center).push(text(emoji).size(15));
                if !senders.is_empty() { label = label.push(text(senders.len().to_string()).size(13).color(rgb(0xd0d7dc))); }
                bar = bar.push(button(label).padding([3, 7]).style(t::reaction(selected)).on_press_maybe((!message.unreadable && !app.sending).then(|| Msg::React(message.id.clone(), emoji.to_string(), selected))));
            }
            bar = bar.push(button(t::icon(Icon::SmilePlus, 14.0, rgb(0xa3abb1))).padding([3, 7]).style(t::reaction(false)).on_press_maybe((!message.unreadable).then(|| Msg::Picker(Some(message.id.clone())))));
            content = content.push(container(bar).padding(Padding { top: 5.0, ..Padding::ZERO }));
        }
        let who = if message.sender == me { own_face(app) } else if community { app.member(&message.sender).map_or_else(|| face_of(&message.sender, None), person_face) } else { app.selected_peer().map_or_else(|| face_of(&message.sender, None), person_face) };
        let row_view = container(row![avatar(who, 0, None, 35.0), content, h_space(if message.sender == me { 92.0 } else { 57.0 })].spacing(13)).padding([14, 11]).width(FILL)
            .style(if message.pinned { t::fill(rgba(0xd0d7dc05)) } else { t::fill(Color::TRANSPARENT) });
        let mut tools = Row::new().spacing(2);
        let tool = |icon: Icon, on: Option<Msg>| -> E { button(t::icon(icon, 14.0, rgb(0xd0d7dc))).padding(4).style(t::plain(rgb(0xd0d7dc), rgb(0xd0d7dc))).on_press_maybe(on).into() };
        tools = tools.push(tool(Icon::Reply, (!message.unreadable).then(|| Msg::Reply(Some(message.id.clone())))));
        if message.sender == me && message.file.is_none() && !message.missed_call && !message.unreadable { tools = tools.push(tool(Icon::Pencil, Some(Msg::StartEdit(message.id.clone())))); }
        if message.sender == me || (community && app.may("delete")) { tools = tools.push(tool(Icon::Trash, Some(Msg::AskDelete(Some(message.id.clone()))))); }
        tools = tools.push(tool(Icon::Pin, Some(Msg::Pin(message.id.clone()))));
        let overlay = container(tools).width(FILL).align_x(Horizontal::Right).padding([14, 11]);
        out.push(container(hover(container(row_view).style(|_| container::Style { border: t::border(Color::TRANSPARENT, 0.0, 6.0), ..Default::default() }), overlay)).padding(Padding { bottom: 4.0, left: -11.0, right: -11.0, top: 0.0 }).into());
    }
    out
}

fn message_body<'a>(value: &'a str, unreadable: bool, names: &[&str]) -> E<'a> {
    let color = if unreadable { rgb(0x828d96) } else { rgb(0xb8bec2) };
    let parts = mention_parts(value, names);
    if parts.len() == 1 && !parts[0].1 { return body(value, 16.0, color, 1.8).wrapping(text::Wrapping::WordOrGlyph).into(); }
    let spans: Vec<text::Span<'a, (), iced::Font>> = parts.into_iter().map(|(part, named)| {
        let span = text::Span::new(part).color(if named { rgb(0x70eeff) } else { color });
        if named { span.background(rgb(0x163447)).border(t::border(Color::TRANSPARENT, 0.0, 4.0)) } else { span }
    }).collect();
    iced::widget::text::Rich::<'a, (), Msg>::with_spans(spans).size(16).font(t::REGULAR).line_height(LineHeight::Relative(1.8)).wrapping(text::Wrapping::WordOrGlyph).into()
}

/// Splits text around "@name" and "@everyone", keeping the original spelling.
fn mention_parts(value: &str, names: &[&str]) -> Vec<(String, bool)> {
    let mut wanted: Vec<String> = names.iter().map(|name| format!("@{}", name.to_lowercase())).collect();
    wanted.push("@everyone".into());
    wanted.sort_by_key(|name| std::cmp::Reverse(name.len()));
    let (mut parts, mut plain, mut at) = (Vec::new(), String::new(), 0);
    while at < value.len() {
        let rest = &value[at..];
        let hit = rest.starts_with('@').then(|| wanted.iter().find_map(|name| rest.get(..name.len()).filter(|part| part.to_lowercase() == *name))).flatten();
        if let Some(part) = hit {
            if !plain.is_empty() { parts.push((std::mem::take(&mut plain), false)); }
            parts.push((part.to_owned(), true));
            at += part.len();
        } else {
            let next = rest.chars().next().unwrap_or_default();
            plain.push(next);
            at += next.len_utf8();
        }
    }
    if !plain.is_empty() || parts.is_empty() { parts.push((plain, false)); }
    parts
}

const EMOJI: [&str; 92] = ["😀","😃","😄","😁","😆","😅","😂","🤣","😊","🙂","😉","😍","🥰","😘","😋","😜","🤪","😎","🤩","🥳","😏","😒","😔","😢","😭","😤","😡","🤯","😳","🥺","😱","🤔","🤗","🤫","🙄","😴","🤒","🤮","🤠","👻","💀","👽","🤖","💩","👍","👎","👏","🙌","🙏","💪","🤝","👋","🤞","🤘","👌","👀","🧠","❤","🧡","💛","💚","💙","💜","🖤","💔","💯","🔥","✨","⭐","🎉","🎊","🎁","🎂","🍕","🍔","🍟","🍺","☕","🎮","🎧","🎵","⚽","🏆","🚀","💡","💻","📱","💰","⏰","✅","❌","❓"];

trait IntoGrid<'a> { fn into_grid(self, grid: Column<'a, Msg>) -> Column<'a, Msg>; }
impl<'a> IntoGrid<'a> for Row<'a, Msg> { fn into_grid(self, grid: Column<'a, Msg>) -> Column<'a, Msg> { grid.push(self) } }

fn composer<'a>(app: &'a App, placeholder: String, enabled: bool, hint_left: String) -> E<'a> {
    let mut area = Column::new();
    if let Some(reply) = &app.reply_to {
        let quoted = app.thread.messages.iter().find(|m| &m.id == reply).map(|m| m.text.clone()).unwrap_or_default();
        area = area.push(container(row![text(format!("Replying to: {quoted}")).size(14).color(rgb(0xd0d7dc)).wrapping(text::Wrapping::None).width(FILL), icon_btn(Icon::X, 16.0, Some(Msg::Reply(None)), false)].spacing(8).align_y(Vertical::Center))
            .padding([6, 10]).style(|_| container::Style { background: Some(rgb(0x272e37).into()), border: iced::Border { radius: iced::border::Radius { top_left: 8.0, top_right: 8.0, bottom_left: 0.0, bottom_right: 0.0 }, ..Default::default() }, ..Default::default() }));
    }
    if app.editing.is_some() {
        area = area.push(container(row![text("Editing your message").size(14).color(rgb(0xd0d7dc)).width(FILL), icon_btn(Icon::X, 16.0, Some(Msg::CancelEdit), false)].spacing(8).align_y(Vertical::Center))
            .padding([6, 10]).style(|_| container::Style { background: Some(rgb(0x272e37).into()), border: iced::Border { radius: iced::border::Radius { top_left: 8.0, top_right: 8.0, bottom_left: 0.0, bottom_right: 0.0 }, ..Default::default() }, ..Default::default() }));
    }
    if let (Mode::Community, Some(query), Some(community)) = (app.mode, app.typing_query(), &app.community) {
        let needle = query.to_lowercase();
        let mut picker = Column::new().spacing(2);
        if (app.owner() || app.moderator()) && "everyone".starts_with(&needle) {
            picker = picker.push(button(text("@everyone").size(14).color(rgb(0xd0d7dc))).width(FILL).padding([5, 8]).style(t::person_row(false)).on_press(Msg::InsertMention("everyone".into())));
        }
        for member in community.member_details.iter().filter(|m| m.id != app.me() && m.name.to_lowercase().contains(&needle)).take(6) {
            picker = picker.push(button(row![avatar(person_face(member), 0, None, 24.0), text(format!("@{}", member.name)).size(14).color(rgb(0xd0d7dc))].spacing(8).align_y(Vertical::Center)).width(FILL).padding([5, 8]).style(t::person_row(false)).on_press(Msg::InsertMention(member.name.clone())));
        }
        area = area.push(container(container(picker).padding(6).width(FILL).style(t::panel(rgb(0x21272e), rgb(0x323b45), 9.0))).padding(Padding { bottom: 4.0, ..Padding::ZERO }));
    }
    let mut editor = text_editor(&app.composer).placeholder(placeholder).size(15).padding(0).height(Length::Shrink).max_height(120.0)
        .line_height(LineHeight::Relative(1.6)).style(t::composer_editor())
        .key_binding(|press| {
            use iced::keyboard::{key::Named, Key};
            if press.key == Key::Named(Named::Enter) && !press.modifiers.shift() && matches!(press.status, text_editor::Status::Focused { .. }) {
                return Some(text_editor::Binding::Custom(Msg::Send));
            }
            text_editor::Binding::from_key_press(press)
        });
    if enabled { editor = editor.on_action(Msg::Edit); }
    let can_send = enabled && !app.sending && !app.composer.text().trim().is_empty();
    let send = button(container(t::icon(Icon::Send, 18.0, rgb(0x1b2025))).center_x(32).center_y(32)).padding(0).width(32).height(32).style(t::send_button()).on_press_maybe(can_send.then_some(Msg::Send));
    let focused = false;
    if app.emoji_open {
        let mut grid = Column::new().spacing(2);
        for line in EMOJI.chunks(14) {
            grid = line.iter().fold(Row::new().spacing(2), |row, emoji| row.push(button(container(text(*emoji).size(20)).center_x(34).center_y(34)).padding(0).width(34).height(34).style(t::plain(Color::WHITE, Color::WHITE)).on_press(Msg::PickEmoji(emoji)))).into_grid(grid);
        }
        area = area.push(container(container(scroll_area(grid).height(Length::Fixed(170.0))).padding(8).style(t::panel(rgb(0x1b2025), rgb(0x353e49), 8.0))).padding(Padding { bottom: 8.0, ..Padding::ZERO }));
    }
    let tool = |icon: Icon, on: Option<Msg>| -> E<'a> { button(container(t::icon(icon, 18.0, rgb(0x929ca3))).center_x(32).center_y(32)).padding(0).width(32).height(32).style(t::plain(rgb(0x929ca3), rgb(0xd0d7dc))).on_press_maybe(on).into() };
    let tools = row![tool(Icon::Smile, enabled.then_some(Msg::ToggleEmoji)), tool(Icon::Paperclip, (enabled && !app.uploading).then_some(Msg::AttachFile))].spacing(2);
    area = area.push(container(row![t::icon(Icon::LockKeyhole, 18.0, rgb(0x77838e)), editor, tools, send].spacing(12).align_y(Vertical::Center))
        .padding([13, 15]).style(move |_| container::Style {
            background: Some(rgb(0x21272d).into()), border: t::border(if focused { rgb(0x7e8a94) } else { rgb(0x333c47) }, 1.0, 10.0), ..Default::default()
        }));
    let hint: E<'a> = match app.typing_line() { Some(line) => text(line).size(11).color(rgb(0x74808b)).into(), None => lead_icon(Icon::LockKeyhole, hint_left, rgb(0x74808b), 11.0, 11.0, 5.0).into() };
    area = area.push(container(row![
        hint, grow(),
        text("Enter to send · Shift + Enter for a new line").size(11).color(rgb(0x74808b)),
    ]).padding([10, 1]));
    container(area).padding(Padding { top: 18.0, bottom: 12.0, left: 33.0, right: 33.0 }).into()
}

fn direct_thread<'a>(app: &'a App, me: &'a str) -> E<'a> {
    let Some(peer) = app.selected_peer() else {
        return scroll_area(welcome(t::sonar_mark(1.5), "TUNE INTO YOUR PEOPLE", "A space to just be.", ["Big ideas. Small talk. Late-night catch-ups.", "Your next conversation starts with a hello."],
            vec![primary("Find your people", Some(Icon::ArrowUpRight), Some(Msg::OpenPanel(Some(Panel::People))))],
            [(Icon::LockKeyhole, "Encrypted messages"), (Icon::Timer, "7-day message history"), (Icon::Headphones, "Unrecorded voice")])).height(FILL).into();
    };
    let first = peer.name.split_whitespace().next().unwrap_or_default().to_owned();
    let changed = app.trust_changed(peer);
    let start = column![
        container(tile(person_face(peer), rgb(0x333c46), rgb(0xd0d7dc), Some(rgb(0x74808b)), 69.0, 23.0, 24.0, None, t::presence_color("online"))).padding(Padding { bottom: 18.0, ..Padding::ZERO }),
        container(text(if app.thread.pins { "The things worth keeping.".to_string() } else { format!("This is your space with {first}.") }).size(24).font(t::bold()).color(rgb(0xeef0f1))).padding(Padding { bottom: 10.0, ..Padding::ZERO }),
        body(if app.thread.pins { "Pinned messages stay until you unpin them." } else { "Say something. Make a plan. Stay a while." }, 15.0, rgb(0x838e96), 1.7),
        container(lead_icon(Icon::LockKeyhole, if changed { "Contact key changed. Verify it in conversation details before sending.".to_string() } else if peer.encryption_key.is_some() { "Encrypted on your device before sending".to_string() } else { format!("{first} hasn't set up encryption yet. They need to sign in to Sonar once before you can message them.") }, if changed { rgb(0xe3b8a8) } else { rgb(0xd0d7dc) }, 12.0, 12.0, 6.0)).padding(Padding { top: 17.0, ..Padding::ZERO }),
    ];
    let mut list = Column::new().push(container(start).padding(Padding { top: 45.0, bottom: 27.0, ..Padding::ZERO })).push(date_divider(if app.thread.pins { "PINNED MESSAGES" } else { "YOUR CONVERSATION" }));
    for item in message_list(app, me, false) { list = list.push(item); }
    let scroll = scroll_area(container(list).padding(Padding { left: 33.0, right: 33.0, bottom: 20.0, top: 0.0 })).id("thread").anchor_bottom().height(FILL);
    let enabled = peer.encryption_key.is_some() && !changed;
    column![scroll, composer(app, if peer.encryption_key.is_some() { format!("Message {first}...") } else { format!("{first} needs to sign in to Sonar first") }, enabled, "Only your devices hold the message keys".into())].into()
}

fn community_thread<'a>(app: &'a App, me: &'a str) -> E<'a> {
    let Some(community) = &app.community else {
        if app.thread.loading { return center(notice("Opening your community...")).into(); }
        return scroll_area(welcome(t::icon(Icon::Users, 37.0, t::ACCENT), "GOOD COMPANY. YOUR OWN CORNER.", "Make room for your people.", ["A weekend crew. A study group. Your next big idea.", "Bring everyone together in a private community."],
            vec![
                button(row![t::icon(Icon::Plus, 17.0, rgb(0x1e2329)), text("Create a community").size(15).font(t::semibold()).color(rgb(0x1e2329))].spacing(22).align_y(Vertical::Center)).padding([13, 18]).style(t::primary()).on_press(Msg::OpenDialog(Some(Dialog::Create))).into(),
                secondary("Join with an invite", Some(Icon::ArrowUpRight), Some(Msg::OpenDialog(Some(Dialog::Join)))),
            ],
            [(Icon::LockKeyhole, "Encrypted channel messages"), (Icon::Timer, "7-day history"), (Icon::Users, "Invite-only spaces")])).height(FILL).into();
    };
    let channel = app.channel_info();
    let name = channel.map(|c| c.name.clone()).unwrap_or_else(|| "general".into());
    let start = column![
        container(container(t::icon(Icon::Hash, 35.0, rgb(0xd0d7dc))).width(65).height(65).center_x(65).center_y(65).style(t::panel(rgb(0x2a323b), rgb(0x3a4450), 22.0))).padding(Padding { bottom: 20.0, ..Padding::ZERO }),
        container(text(if app.thread.pins { "The things worth keeping.".to_string() } else { format!("Welcome to #{name}") }).size(24).font(t::bold()).color(rgb(0xeef0f1))).padding(Padding { bottom: 10.0, ..Padding::ZERO }),
        body(if app.thread.pins { "Pinned messages stay until someone unpins them." } else { "Your own corner of the conversation. Make yourself at home." }, 15.0, rgb(0x838e96), 1.7),
        container(row![t::icon(Icon::LockKeyhole, 12.0, rgb(0xd0d7dc)), body("Encrypted separately for each member. New members start with a clean history.", 12.0, rgb(0xd0d7dc), 1.8)].spacing(6)).padding(Padding { top: 17.0, ..Padding::ZERO }),
    ];
    let mut list = Column::new().push(container(start).padding(Padding { top: 45.0, bottom: 27.0, ..Padding::ZERO })).push(date_divider(if app.thread.pins { "PINNED MESSAGES" } else { "CHANNEL CONVERSATION" }));
    for item in message_list(app, me, true) { list = list.push(item); }
    let scroll = scroll_area(container(list).padding(Padding { left: 33.0, right: 33.0, bottom: 15.0, top: 0.0 })).id("thread").anchor_bottom().height(FILL);
    if channel.is_none() { return scroll.into(); }
    let count = community.members.len();
    column![scroll, composer(app, format!("Message #{name}..."), true, format!("Encrypted for {count} {}", if count == 1 { "member" } else { "members" }))].into()
}


/// One labelled action for a person: icon, label, shown in red, and what it does. `None` is a divider.
type PersonAction = Option<(Icon, &'static str, bool, Msg)>;
/// The "⋯" button for one person and, while it is open, their labelled actions to show under the row.
fn person_actions<'a>(app: &App, id: &str, actions: Vec<PersonAction>) -> (E<'a>, Option<E<'a>>) {
    let mut kept: Vec<PersonAction> = Vec::new();
    for action in actions { if action.is_some() || kept.last().is_some_and(Option::is_some) { kept.push(action); } }
    while kept.last().is_some_and(Option::is_none) { kept.pop(); }
    if kept.is_empty() { return (space().into(), None); }
    let open = app.person_menu.as_deref() == Some(id);
    let toggle = icon_btn(Icon::Ellipsis, 16.0, Some(Msg::TogglePersonMenu(id.to_owned())), open);
    if !open { return (toggle, None); }
    let mut list = Column::new().spacing(2);
    for action in kept {
        list = list.push(match action {
            None => container(t::rule(rgb(0x272e35), true)).padding([4, 2]).into(),
            Some((icon, label, danger, on)) => {
                let color = if danger { rgb(0xc28b98) } else { rgb(0xc8cdd1) };
                E::from(button(row![t::icon(icon, 15.0, if danger { color } else { rgb(0x8c969d) }), text(label).size(13).color(color)].spacing(10).align_y(Vertical::Center))
                    .width(FILL).padding([8, 10]).style(t::person_row(false)).on_press(on))
            }
        });
    }
    (toggle, Some(container(list).padding(6).width(FILL).style(t::panel(rgb(0x161a1e), rgb(0x2c343d), 10.0)).into()))
}

fn members_panel(app: &App) -> E<'_> {
    let Some(community) = &app.community else { return space().into() };
    let (owner, moderator, me) = (app.owner(), app.moderator(), app.me());
    let mut list = Column::new().push(container(row![text(t::tracked(&format!("MEMBERS — {}", community.members.len()))).size(11).font(t::bold()).color(rgb(0x75818b)), grow(), icon_btn(Icon::X, 15.0, Some(Msg::ToggleMembers), false)].align_y(Vertical::Center)).padding(Padding { top: 10.0, bottom: 18.0, left: 4.0, right: 4.0 }));
    if owner { list = list.push(nav_row(None, "Manage bans", None, false, Some(Msg::OpenDialog(Some(Dialog::Bans))))); }
    for (index, member) in community.member_details.iter().enumerate() {
        let is_mod = community.moderators.contains(&member.id);
        let role = if Some(&member.id) == community.owner_id.as_ref() { "Owner" } else if is_mod { "Moderator" } else { "Member" };
        let manageable = community.kind != "group" && ((owner && member.id != me) || (moderator && member.id != me && Some(&member.id) != community.owner_id.as_ref() && !is_mod && app.may("remove")));
        let ask = |action: &str| Msg::AskConfirm(Some(Confirm { action: action.into(), member: Some(member.clone()), channel: None }));
        let (more, actions) = person_actions(app, &member.id, vec![
            manageable.then(|| (Icon::LogOut, "Remove from community", false, ask("remove"))),
            manageable.then(|| (Icon::Ban, "Ban from community", true, ask("ban"))),
            (manageable && owner).then(|| if is_mod { (Icon::ShieldCheck, "Remove moderator role", false, ask("demote")) } else { (Icon::ShieldCheck, "Make moderator", false, ask("promote")) }),
            (manageable && owner && is_mod).then(|| (Icon::Settings, "Moderator permissions", false, Msg::OpenPermissions(member.clone()))),
            (manageable && owner).then(|| (Icon::Crown, "Make owner", false, ask("transfer"))),
            None,
            (member.id != me).then(|| (Icon::Ban, "Block", true, ask("block"))),
        ]);
        list = list.push(container(row![
            avatar_status(person_face(member), index, app.online.contains(&member.id).then_some(rgb(0x171b1f)), app.status_color(&member.id), 30.0),
            column![text(member.name.clone()).size(14).line_height(LineHeight::Relative(1.2)).font(t::semibold()).color(t::TEXT).wrapping(text::Wrapping::None), text(format!("{role}{}", if member.id == me { " · You" } else { "" })).size(13).line_height(LineHeight::Relative(1.2)).color(rgb(0x78848e))].spacing(2).width(FILL).clip(true),
            more,
        ].spacing(8).align_y(Vertical::Center)).padding([10, 4]));
        if let Some(actions) = actions { list = list.push(actions); }
    }
    container(scroll_area(list)).width(250).height(FILL).padding([12, 10]).style(t::fill(rgb(0x171b1f))).into()
}

fn matches_person(person: &crate::api::Person, query: &str) -> bool {
    let query = query.trim().trim_start_matches('@').to_lowercase();
    person.name.to_lowercase().contains(&query) || person.username.contains(&query)
}

// ----- Overlays ---------------------------------------------------------------

fn privacy_item<'a>(icon: Icon, title: &'a str, note: &'a str) -> E<'a> {
    container(row![t::icon(icon, 19.0, rgb(0xd0d7dc)), column![text(title).size(15).font(t::semibold()).color(rgb(0xd0d7dc)), body(note, 14.0, rgb(0x8f99a0), 1.8)].spacing(7)].spacing(14)).padding([12, 0]).into()
}

fn panel_view(app: &App, panel: Panel) -> E<'_> {
    let close = Msg::OpenPanel(None);
    let content: E = match panel {
        Panel::Search => {
            let mut list = Column::new().spacing(6).padding(Padding { top: 16.0, ..Padding::ZERO });
            if let Some(results) = &app.search_results {
                if results.is_empty() { list = list.push(body("Nothing found in the last 7 days or your pinned messages.", 15.0, rgb(0x929ca3), 1.8)); }
                for message in results {
                    let sender = if message.sender == app.me() { "You".to_owned() } else if app.mode == Mode::Community { app.member(&message.sender).map_or_else(|| "Member".to_owned(), |p| p.name.clone()) } else { app.selected_peer().map_or_else(String::new, |p| p.name.clone()) };
                    list = list.push(container(column![text(format!("{sender} · {}", message.date_time())).size(12).color(rgb(0x8a959e)), body(message.text.clone(), 15.0, rgb(0xb8bec2), 1.6).wrapping(text::Wrapping::WordOrGlyph)].spacing(3)).padding([8, 10]).width(FILL).style(t::panel(rgb(0x21272e), rgb(0x21272e), 8.0)));
                }
            }
            let can = !app.search_busy && !app.search_query.trim().is_empty();
            column![modal_header(Icon::Search, 28.0, close.clone()), modal_title("Find it again."),
                body("Searched on your device. The server never sees what you look for.", 15.0, rgb(0x9099a0), 1.8),
                container(row![container(text_input("Search this conversation", &app.search_query).on_input(Msg::SearchQuery).on_submit(Msg::SearchRun).size(14).padding(0).style(t::bare_input())).padding([9, 10]).width(FILL).style(t::panel(rgb(0x121518), rgb(0x222930), 6.0)), button(text(if app.search_busy { "Searching..." } else { "Search" }).size(13).font(t::semibold()).color(rgb(0x1e2329))).padding([8, 12]).style(t::primary()).on_press_maybe(can.then_some(Msg::SearchRun))].spacing(10).align_y(Vertical::Center)).padding(Padding { top: 20.0, ..Padding::ZERO }),
                list].into()
        }
        Panel::Group => {
            let mut list = Column::new().spacing(8).padding(Padding { top: 16.0, bottom: 18.0, ..Padding::ZERO });
            for person in &app.people {
                let id = person.id.clone();
                list = list.push(row![checkbox_row(&person.name, app.group_picks.contains(&person.id), move |on| Msg::GroupPick(id.clone(), on))].align_y(Vertical::Center));
            }
            if app.people.is_empty() { list = list.push(body("Add some friends first.", 15.0, rgb(0x929ca3), 1.8)); }
            column![modal_header(Icon::Users, 28.0, close.clone()), modal_title("Bring a few friends together."),
                body("Pick 2 to 9 friends. Group chats are encrypted for each person and have a voice room.", 15.0, rgb(0x9099a0), 1.8),
                container(field("Group name (optional)", dialog_input("", &app.group_name, Msg::GroupName, false), 8.0, 13.0, rgb(0xd0d7dc))).padding(Padding { top: 18.0, ..Padding::ZERO }),
                list, primary("Create group", Some(Icon::Check), (app.group_picks.len() >= 2).then_some(Msg::CreateGroup))].into()
        }
        Panel::People => {
            let mut people = Column::new().spacing(6).padding(Padding { top: 20.0, ..Padding::ZERO });
            let section = |label: &'static str| -> E<'_> { container(t::eyebrow(label, 10.0, t::EYEBROW)).padding(Padding { top: 12.0, bottom: 2.0, ..Padding::ZERO }).into() };
            let small = |label: &'static str, primary: bool, on: Msg| -> E<'_> { button(text(label).size(13).font(t::semibold()).color(if primary { rgb(0x1e2329) } else { rgb(0xd0d7dc) })).padding([6, 11]).style(if primary { t::primary() } else { t::secondary() }).on_press(on).into() };
            let request = |person: &crate::api::Person, index: usize, actions: Vec<E<'static>>| -> E<'static> {
                let mut details = column![text(person.name.clone()).size(15).font(t::semibold()).color(t::TEXT), text(format!("@{}", person.username)).size(13).color(rgb(0x78848e))].spacing(3).width(FILL);
                if let Some(note) = app.friend_notes.get(&person.id) { details = details.push(text(format!("\"{note}\"")).size(12).color(rgb(0xa9d8ee)).wrapping(text::Wrapping::WordOrGlyph)); }
                let mut line = row![avatar(person_face(person), index, None, 35.0), details].spacing(11).align_y(Vertical::Center);
                for action in actions { line = line.push(action); }
                container(line).padding([8, 10]).into()
            };
            if !app.friend_requests.incoming.is_empty() { people = people.push(section("FRIEND REQUESTS")); }
            for (index, person) in app.friend_requests.incoming.iter().enumerate() {
                people = people.push(request(person, index, vec![small("Accept", true, Msg::AnswerFriendRequest(person.id.clone(), true)), small("Decline", false, Msg::AnswerFriendRequest(person.id.clone(), false))]));
            }
            if !app.friend_requests.outgoing.is_empty() { people = people.push(section("WAITING FOR A REPLY")); }
            for (index, person) in app.friend_requests.outgoing.iter().enumerate() {
                people = people.push(request(person, index, vec![small("Cancel", false, Msg::AnswerFriendRequest(person.id.clone(), false))]));
            }
            if !app.people.is_empty() { people = people.push(section("YOUR FRIENDS")); }
            for (index, person) in app.people.iter().enumerate() {
                let mut folders: Vec<String> = vec![NO_FOLDER.to_owned()];
                folders.extend(app.preferences.folders.iter().map(|f| f.name.clone()));
                if !app.folder_name.trim().is_empty() { folders.push(NEW_FOLDER.to_owned()); }
                let current = app.preferences.folders.iter().find(|f| f.people.contains(&person.id)).map_or(NO_FOLDER.to_owned(), |f| f.name.clone());
                let (id, pin_id) = (person.id.clone(), person.id.clone());
                let ask = |action: &str| Msg::AskConfirm(Some(Confirm { action: action.into(), member: Some(person.clone()), channel: None }));
                let (more, actions) = person_actions(app, &person.id, vec![Some((Icon::UserMinus, "Remove friend", false, ask("unfriend"))), Some((Icon::Ban, "Block", true, ask("block")))]);
                people = people.push(row![
                    container(person_row(person_face(person), &person.name, &format!("@{}", person.username), index, app.avatar_dot(&person.id), false, vec![t::icon(Icon::ArrowUpRight, 17.0, rgb(0x98a1a7))], Some(Msg::OpenPeer(Some(person.id.clone()))))).width(FILL),
                    icon_btn(Icon::Pin, 15.0, Some(Msg::TogglePinPerson(pin_id)), app.preferences.pinned_people.contains(&person.id)),
                    pick_list(folders, Some(current), move |name: String| Msg::MoveToFolder(id.clone(), name)).text_size(11).padding([5, 4]).width(92).style(t::select()).menu_style(t::menu()),
                    more,
                ].spacing(4).align_y(Vertical::Center));
                if let Some(actions) = actions { people = people.push(actions); }
            }
            if app.people.is_empty() && app.friend_requests.incoming.is_empty() && app.friend_requests.outgoing.is_empty() { people = people.push(body("No friends yet. Send a request to get started.", 15.0, rgb(0x929ca3), 1.8)); }
            column![modal_header(Icon::Users, 28.0, close.clone()), modal_title("Your people, a little closer."),
                body("Add someone by their username. Once they accept, you can message and call each other.", 15.0, rgb(0x9099a0), 1.8),
                container(row![container(row![t::icon(Icon::Search, 15.0, rgb(0x74808b)), text_input("Their username", &app.people_query).on_input(Msg::PeopleQuery).on_submit(Msg::SendFriendRequest).size(14).padding(0).style(t::bare_input())].spacing(8).align_y(Vertical::Center)).padding([9, 10]).width(FILL).style(t::panel(rgb(0x121518), rgb(0x222930), 6.0)), button(text("Send request").size(13).font(t::semibold()).color(rgb(0x1e2329))).padding([8, 12]).style(t::primary()).on_press_maybe((!app.people_query.trim().is_empty()).then_some(Msg::SendFriendRequest))].spacing(10).align_y(Vertical::Center)).padding(Padding { top: 20.0, ..Padding::ZERO }),
                container(text_input("Add a short note, like where you met (optional)", &app.friend_note).on_input(Msg::FriendNote).size(14).padding([9, 12]).style(t::card_input())).padding(Padding { top: 8.0, ..Padding::ZERO }),
                container(text_input("Name a new folder, then choose it on a friend", &app.folder_name).on_input(Msg::FolderName).size(13).padding([7, 12]).style(t::card_input())).padding(Padding { top: 8.0, ..Padding::ZERO }),
                scroll_area(people).height(Length::Shrink)].into()
        }
        Panel::Privacy => {
            let mut content = Column::new()
                .push(modal_header(Icon::ShieldCheck, 30.0, close.clone()))
                .push(container(t::eyebrow("PRIVACY, IN PLAIN SIGHT", 10.0, t::EYEBROW)).padding(Padding { top: 22.0, bottom: 10.0, ..Padding::ZERO }))
                .push(modal_title("Keep the connection. Lose the noise."))
                .push(privacy_item(Icon::LockKeyhole, "Encrypted before it leaves.", "Message content is encrypted on your device. Your key is locked with your recovery key before it is stored, so only your devices can open it. Contact keys are remembered and changes block sending. Compare safety codes with a contact in person or over a call to verify them."))
                .push(privacy_item(Icon::Timer, "Seven days. Then gone.", "Unpinned messages leave history at expiry and are deleted by the database cleanup process. Pin something to keep it. Unpinning starts a fresh seven days."))
                .push(privacy_item(Icon::Headphones, "Voice is for the moment.", "Direct WebRTC audio is encrypted in transit. Sonar has no recording or media storage. Calls need a compatible network or a configured STUN/TURN service."));
            if let (Mode::Direct, Some(peer)) = (app.mode, app.selected_peer()) {
                if let Some(print) = peer.encryption_key.as_deref().and_then(|k| crate::crypto::fingerprint(k).ok()) {
                    let mut card = Column::new().push(text(format!("{}’s safety code", peer.name)).size(13).font(t::semibold()).color(rgb(0xd0d7dc)))
                        .push(container(body(print, 14.0, rgb(0xb4bbc0), 1.8)).padding(Padding { top: 8.0, ..Padding::ZERO }));
                    if app.trust_changed(peer) {
                        let first = peer.name.split_whitespace().next().unwrap_or_default().to_owned();
                        card = card.push(container(body(format!("{first}’s key changed, for example because they reset it or set up a new device. Ask them to open Account settings → Privacy & device and read you their safety code in person or on a call. If it matches the code above, trust the new key. Older messages may become unreadable."), 14.0, rgb(0xadb4b9), 1.6)).padding(Padding { top: 10.0, ..Padding::ZERO }))
                            .push(container(button(text("The codes match, trust the new key").size(15).color(rgb(0xd0d7dc))).padding([8, 12]).style(t::fingerprint_button()).on_press(Msg::AcceptKey)).padding(Padding { top: 10.0, ..Padding::ZERO }));
                    }
                    content = content.push(container(card).padding(12).width(FILL).style(t::panel(rgb(0x14181c), rgb(0x2a323b), 7.0))).push(v_space(20.0));
                }
            }
            content.push(honest_note(Icon::Check, "This is an early implementation, not an audited messenger. Account and connection metadata remain visible to the service. Forward secrecy is not implemented. Recipients can copy or record content."))
                .push(v_space(22.0))
                .push(primary("Got it", Some(Icon::Check), Some(close.clone())))
                .into()
        }
    };
    modal(modal_scroll(content), 520.0, Some(close))
}

fn dialog_view(app: &App) -> E<'_> {
    let Some(dialog) = &app.dialog else { return space().into() };
    let close = Msg::OpenDialog(None);
    let channels = app.community.as_ref().map(|c| c.channels.clone()).unwrap_or_default();
    let channel_names: Vec<String> = channels.iter().map(|c| format!("{} ({})", c.name, if c.kind == "voice" { "voice" } else { "text" })).collect();
    let selected_name = app.dialog_channel.as_ref().and_then(|id| channels.iter().position(|c| &c.id == id)).map(|i| channel_names[i].clone());
    let ids: Vec<String> = channels.iter().map(|c| c.id.clone()).collect();
    let names = channel_names.clone();
    let channel_picker = move || -> E {
        let (ids, names2) = (ids.clone(), names.clone());
        field("Channel or room", pick_list(names.clone(), selected_name.clone(), move |name: String| Msg::DialogChannel(ids[names2.iter().position(|n| n == &name).unwrap_or(0)].clone()))
            .padding(11).text_size(14).width(FILL).style(t::select()).menu_style(t::menu()).into(), 10.0, 14.0, rgb(0xd0d7dc))
    };
    let mut content = Column::new().push(modal_header(Icon::Users, 29.0, close.clone()));
    match dialog {
        Dialog::Start => {
            content = content.push(modal_title("Your people belong here."))
                .push(body("Start a private community, or join a space with an invite from its owner.", 15.0, rgb(0x9099a0), 1.8))
                .push(container(column![
                    button(container(row![t::icon(Icon::Plus, 17.0, rgb(0x1e2329)), text("Create a community").size(15).font(t::semibold()).color(rgb(0x1e2329))].spacing(12).align_y(Vertical::Center)).center_x(FILL)).width(FILL).padding([13, 18]).style(t::primary()).on_press(Msg::OpenDialog(Some(Dialog::Create))),
                    button(container(lead_icon(Icon::ArrowUpRight, "Join with an invite", rgb(0xd0d7dc), 17.0, 15.0, 12.0)).center_x(FILL)).width(FILL).padding([12, 16]).style(t::secondary()).on_press(Msg::OpenDialog(Some(Dialog::Join))),
                ].spacing(12)).padding(Padding { top: 27.0, ..Padding::ZERO }));
        }
        Dialog::Invite => {
            content = content.push(modal_title("Good company starts here."))
                .push(body("This invite expires in 24 hours. Generating a new invite revokes the previous link. Anyone with it can join, up to 32 members.", 15.0, rgb(0x9099a0), 1.8));
            match &app.invite {
                Some(invite) => {
                    content = content.push(container(field("Invite code", text_input("", &invite.token).padding(13).size(15).style(t::dialog_input()).into(), 10.0, 14.0, rgb(0xd0d7dc))).padding([24, 0]))
                        .push(primary("Copy invite link", Some(Icon::Copy), Some(Msg::CopyInvite)))
                        .push(v_space(24.0))
                        .push(honest_note(Icon::Timer, "New members receive future messages only. Their devices cannot decrypt older history."));
                }
                None => content = content.push(notice(if app.busy { "Creating your invite..." } else { "Unable to create an invite. Close and try again." })),
            }
        }
        Dialog::Activity => {
            content = content.push(modal_title("Who joined or left")).push(body("The latest 50 changes to this community's members.", 15.0, rgb(0x9099a0), 1.8));
            content = match &app.activity {
                None => content.push(notice("Loading...")),
                Some(rows) if rows.is_empty() => content.push(notice("Nothing yet.")),
                Some(rows) => rows.iter().fold(content, |content, row| content.push(container(iced::widget::row![
                    text(row.name.clone()).size(15).font(t::semibold()).color(t::TEXT),
                    text(row.action.clone()).size(15).color(rgb(0xb8bec2)).width(FILL),
                    text(day(&row.at)).size(13).color(rgb(0x78848e)),
                ].spacing(6).align_y(Vertical::Center)).padding([8, 0]))),
            };
        }
        Dialog::Permissions => {
            let name = app.permissions_for.as_ref().map_or_else(String::new, |m| m.name.clone());
            content = content.push(modal_title(format!("What {name} can do"))).push(body("Moderators can do all of this until you switch something off.", 15.0, rgb(0x9099a0), 1.8)).push(v_space(14.0));
            for (value, label) in [("pin", "Pin other people's messages"), ("delete", "Delete other people's messages"), ("remove", "Remove and ban members"), ("lock", "Lock voice rooms")] {
                content = content.push(container(checkbox_row(label, !app.denied.iter().any(|d| d == value), move |on| Msg::Deny(value, on))).padding([5, 0]));
            }
            content = content.push(v_space(14.0)).push(primary("Save permissions", None, Some(Msg::SavePermissions)));
        }
        Dialog::Bans => {
            content = content.push(modal_title("Community bans")).push(body("Banned accounts cannot use an invite to rejoin. Removing a ban revokes the current invite.", 15.0, rgb(0x9099a0), 1.8));
            content = match &app.bans {
                None if app.busy => content.push(notice("Loading bans...")),
                None => content.push(notice("Could not load bans. Close and try again.")),
                Some(list) if list.is_empty() => content.push(notice("No banned accounts.")),
                Some(list) => list.iter().fold(content, |content, member| content.push(container(row![
                    text(member.name.clone()).size(15).font(t::semibold()).color(t::TEXT).width(FILL),
                    secondary("Remove ban", None, Some(Msg::AskConfirm(Some(Confirm { action: "unban".into(), member: Some(member.clone()), channel: None })))),
                ].align_y(Vertical::Center)).padding([10, 0]))),
            };
        }
        Dialog::DeleteChannel => {
            let target = app.dialog_channel.as_ref().and_then(|id| channels.iter().find(|c| &c.id == id)).cloned();
            content = content.push(modal_title("Delete a channel or room"))
                .push(body("Select a channel. Its stored messages, including pins, will be deleted. Every community must keep at least one text channel.", 15.0, rgb(0x9099a0), 1.8))
                .push(container(channel_picker()).padding([24, 0]))
                .push(danger("Continue", target.map(|channel| Msg::AskConfirm(Some(Confirm { action: "delete-channel".into(), member: None, channel: Some(channel) })))));
        }
        Dialog::RenameCommunity | Dialog::RenameChannel => {
            let channel = *dialog == Dialog::RenameChannel;
            content = content.push(modal_title(if channel { "Rename channel or room" } else { "Rename community" }))
                .push(body("Everyone in the community will see the updated name.", 15.0, rgb(0x9099a0), 1.8));
            if channel { content = content.push(container(channel_picker()).padding(Padding { top: 24.0, ..Padding::ZERO })); }
            content = content.push(container(field("New name", text_input("", &app.dialog_value).on_input(Msg::DialogValue).on_submit(Msg::SubmitDialog).padding(13).size(15).style(t::dialog_input()).into(), 10.0, 14.0, rgb(0xd0d7dc))).padding([24, 0]))
                .push(primary(if app.busy { "Saving..." } else { "Save name" }, None, (!app.busy && !app.dialog_value.trim().is_empty()).then_some(Msg::SubmitDialog)));
        }
        Dialog::Create | Dialog::Join | Dialog::Channel => {
            let (title, note, label, placeholder, submit) = match dialog {
                Dialog::Create => ("A new corner of the internet.", "Give your community a name. We will start you off with a general channel.", "Community name", "The weekend crew", "Create community"),
                Dialog::Join => ("Your people are waiting.", "Paste an invite link or code from a community owner.", "Invite link or code", "Paste your invite here", "Join community"),
                _ if app.dialog_voice => ("Give the conversation a home.", "All community members can join. Up to six people can share encrypted audio at a time.", "Room name", "lounge", "Create voice room"),
                _ => ("Give the conversation a home.", "Only the owner can create channels. All community members can read future messages.", "Channel name", "ideas-and-plans", "Create channel"),
            };
            content = content.push(modal_title(title)).push(body(note, 15.0, rgb(0x9099a0), 1.8));
            if *dialog == Dialog::Channel {
                let options = vec!["Text channel".to_string(), "Voice room · up to 6 people".to_string()];
                let selected = Some(options[usize::from(app.dialog_voice)].clone());
                content = content.push(container(field("Channel type", pick_list(options, selected, |value: String| Msg::DialogVoice(value.starts_with("Voice"))).padding(11).text_size(14).width(FILL).style(t::select()).menu_style(t::menu()).into(), 10.0, 14.0, rgb(0xd0d7dc))).padding(Padding { top: 24.0, ..Padding::ZERO }));
            }
            content = content.push(container(field(label, text_input(placeholder, &app.dialog_value).on_input(Msg::DialogValue).on_submit(Msg::SubmitDialog).padding(13).size(15).style(t::dialog_input()).into(), 10.0, 14.0, rgb(0xd0d7dc))).padding(Padding { top: 24.0, bottom: 20.0, ..Padding::ZERO }))
                .push(primary(if app.busy { "One moment..." } else { submit }, Some(Icon::Check), (!app.busy && !app.dialog_value.trim().is_empty()).then_some(Msg::SubmitDialog)))
                .push(v_space(24.0))
                .push(honest_note(Icon::ShieldCheck, "Messages are encrypted on your device. Community names, channel names, and membership are visible to the service. This early encryption protocol has not been independently audited."));
        }
    }
    modal(modal_scroll(content), 520.0, (!app.busy).then_some(close))
}

fn confirm_view<'a>(app: &'a App, confirm: &'a Confirm) -> E<'a> {
    let community = app.community.as_ref().map(|c| c.name.clone()).unwrap_or_default();
    let member = confirm.member.as_ref().map(|m| m.name.clone()).unwrap_or_default();
    let channel = confirm.channel.as_ref().map(|c| c.name.clone()).unwrap_or_default();
    let (title, note, label) = match confirm.action.as_str() {
        "call" => (format!("Call {member}?"), "They will hear it ring right away.", "Call"),
        "unfriend" => (format!("Remove {member} from your friends?"), "You will stop seeing each other here. Either of you can send a new request later.", "Remove friend"),
        "block" => (format!("Block {member}?"), "They cannot message, call or send you requests, and their server messages are hidden completely. They are not told.", "Block"),
        "delete-community" => (format!("Delete {community}?"), "This permanently deletes the community, its channels and stored messages, including pins, for everyone. Active voice rooms close. Copies and backups may remain.", "Delete community permanently"),
        "delete-channel" => (format!("Delete {channel}?"), "This permanently deletes the channel and its stored messages, including pins. Anyone in its voice room will be disconnected. Copies and backups may remain.", "Delete channel"),
        "leave" => (format!("Leave {community}?"), "You will lose access to channels and future messages. You will need a valid invite to rejoin. Messages you sent remain subject to the community retention rules.", "Leave community"),
        "ban" => (format!("Ban {member}?"), "They will lose access and cannot rejoin while banned. The current invite will be revoked. Previously received copies cannot be erased from their device.", "Ban member"),
        "unban" => (format!("Remove the ban on {member}?"), "They will remain outside the community. The current invite will be revoked; create a new invite if you want them to rejoin.", "Remove ban"),
        "remove" => (format!("Remove {member}?"), "They will lose access to this community and future messages. The current invite will be revoked so it cannot be used to rejoin. Previously received copies cannot be erased from their device.", "Remove member"),
        "promote" => (format!("Make {member} a moderator?"), "Moderators can remove or ban ordinary members. Only the owner can manage moderators, invites, channels, and ownership.", "Make moderator"),
        "demote" => (format!("Remove {member} as moderator?"), "They will remain a member but will no longer be able to remove members.", "Remove moderator role"),
        _ => (format!("Make {member} the owner?"), "They will control invites, channels, and membership. You will remain a member and lose owner controls. The current invite will be revoked.", "Transfer ownership"),
    };
    let label = if app.busy { "Updating membership..." } else { label };
    let friendly = confirm.action == "call";
    let go = (!app.busy).then_some(Msg::ApplyConfirm);
    let confirm_button = if friendly || matches!(confirm.action.as_str(), "transfer" | "promote") {
        button(container(text(label).size(15).font(t::semibold()).color(rgb(0x1e2329))).center_x(FILL)).width(FILL).padding([13, 18]).style(t::primary()).on_press_maybe(go)
    } else {
        button(container(text(label).size(15).font(t::semibold()).color(Color::WHITE)).center_x(FILL)).width(FILL).padding([13, 18]).style(t::danger()).on_press_maybe(go)
    };
    let content = column![
        modal_header(Icon::ShieldCheck, 29.0, Msg::AskConfirm(None)),
        modal_title(title),
        body(note, 15.0, rgb(0x9099a0), 1.8),
        container(column![confirm_button, button(container(text("Cancel").size(15).color(rgb(0xd0d7dc))).center_x(FILL)).width(FILL).padding([12, 16]).style(t::secondary()).on_press_maybe((!app.busy).then_some(Msg::AskConfirm(None)))].spacing(12)).padding(Padding { top: 27.0, ..Padding::ZERO }),
    ];
    modal(container(content).padding(34).into(), 520.0, (!app.busy).then_some(Msg::AskConfirm(None)))
}

fn delete_view<'a>(app: &'a App, id: &'a str) -> E<'a> {
    let note = if app.mode == Mode::Direct { "Delete this message for both people? Copies already saved elsewhere cannot be removed." } else { "Delete this message for the community? Copies already saved elsewhere cannot be removed." };
    let content = column![
        modal_header(Icon::Trash, 26.0, Msg::AskDelete(None)),
        modal_title("Delete message?"),
        body(note, 15.0, rgb(0x9099a0), 1.8),
        container(column![
            button(container(text("Delete message").size(15).font(t::semibold()).color(Color::WHITE)).center_x(FILL)).width(FILL).padding([13, 18]).style(t::danger()).on_press(Msg::Delete(id.to_owned())),
            button(container(text("Cancel").size(15).color(rgb(0xd0d7dc))).center_x(FILL)).width(FILL).padding([12, 16]).style(t::secondary()).on_press(Msg::AskDelete(None)),
        ].spacing(12)).padding(Padding { top: 27.0, ..Padding::ZERO }),
    ];
    modal(container(content).padding(34).into(), 420.0, Some(Msg::AskDelete(None)))
}

// ----- Account settings (AccountSettings) ----------------------------------------

fn security_card<'a>(icon: Icon, title: E<'a>, note: &'a str, content: E<'a>) -> E<'a> {
    container(column![
        row![container(t::icon(icon, 20.0, rgb(0xd0d7dc))).width(39).height(39).center_x(39).center_y(39).style(t::panel(rgb(0x2a3139), rgb(0x38424e), 12.0)),
            column![title, body(note, 14.0, rgb(0x949da4), 1.7)].spacing(5)].spacing(12),
        container(content).padding(Padding { top: 20.0, ..Padding::ZERO }),
    ]).padding(22).width(FILL).style(t::panel(rgb(0x1d2329), rgb(0x2e3640), 12.0)).into()
}
fn card_title<'a>(value: &'a str) -> E<'a> { text(value).size(17).font(t::semibold()).color(rgb(0xd0d7dc)).into() }
fn small_note<'a>(value: impl text::IntoFragment<'a>) -> E<'a> {
    column![t::rule(rgb(0x2a323b), true), container(body(value, 12.0, rgb(0x818c95), 1.8)).padding(Padding { top: 14.0, ..Padding::ZERO })].padding(Padding { top: 18.0, ..Padding::ZERO }).into()
}
fn date(value: &str) -> String {
    crate::chat::timestamp(value).and_then(chrono::DateTime::from_timestamp_millis).map(|d| d.with_timezone(&chrono::Local).format("%d/%m/%Y, %H:%M:%S").to_string()).unwrap_or_default()
}
fn day(value: &str) -> String {
    crate::chat::timestamp(value).and_then(chrono::DateTime::from_timestamp_millis).map(|d| d.with_timezone(&chrono::Local).format("%d/%m/%Y").to_string()).unwrap_or_default()
}
fn checkbox_row<'a>(label: &'a str, checked: bool, on: impl Fn(bool) -> Msg + 'a) -> E<'a> {
    iced::widget::checkbox(checked).label(label).on_toggle(on).size(14).text_size(13).spacing(9)
        .style(move |_, status| {
            let hovered = matches!(status, iced::widget::checkbox::Status::Hovered { .. });
            iced::widget::checkbox::Style {
                background: (if checked { rgb(0xd0d7dc) } else { rgb(0x171b20) }).into(),
                icon_color: rgb(0x1b2025),
                border: t::border(if checked || hovered { rgb(0xd0d7dc) } else { rgb(0x37414c) }, 1.0, 3.0),
                text_color: Some(rgb(0xd0d7dc)),
            }
        }).into()
}

fn settings_view(app: &App) -> E<'_> {
    let Some(session) = &app.session else { return space().into() };
    let close = Msg::OpenSettings(false);
    let tabs = [(Tab::Profile, Icon::UserRound, "Your profile"), (Tab::Security, Icon::KeyRound, "Password & passkeys"), (Tab::Privacy, Icon::ShieldCheck, "Privacy & device"), (Tab::Voice, Icon::Headphones, "Voice & audio"), (Tab::Alerts, Icon::Bell, "Alerts & privacy")];
    let nav = tabs.iter().fold(Column::new(), |nav, (tab, icon, label)| {
        let selected = app.tab == *tab;
        let color = if selected { rgb(0xd0d7dc) } else { rgb(0x98a1a7) };
        nav.push(container(button(row![t::icon(*icon, 17.0, color), text(*label).size(13).color(color)].spacing(11).align_y(Vertical::Center)).width(FILL).padding([12, 10]).style(t::nav_row(selected)).on_press_maybe((!app.busy).then_some(Msg::SetTab(*tab)))).padding([3, 0]))
    });
    let navigation = container(column![
        t::eyebrow("YOUR SONAR", 11.0, t::EYEBROW),
        container(text("Make it yours.").size(23).font(t::semibold()).color(t::TEXT)).padding(Padding { top: 9.0, bottom: 24.0, ..Padding::ZERO }),
        tile(own_face(app), rgb(0x39434e), rgb(0xd0d7dc), Some(rgb(0x74808b)), 65.0, 22.0, 24.0, None, t::presence_color("online")),
        container(text(session.account.full_name.clone()).size(16).font(t::semibold()).color(t::TEXT)).padding(Padding { top: 12.0, ..Padding::ZERO }),
        container(text(format!("@{}", session.account.username)).size(13).color(rgb(0xd0d7dc))).padding(Padding { top: 5.0, ..Padding::ZERO }),
        container(text(session.account.email.clone()).size(13).color(rgb(0x7f8a94))).padding(Padding { top: 7.0, bottom: 24.0, ..Padding::ZERO }),
        nav,
        container(button(row![t::icon(Icon::LogOut, 16.0, rgb(0xd0d7dc)), text("Sign out everywhere").size(13).color(rgb(0xd0d7dc))].spacing(11).align_y(Vertical::Center)).width(FILL).padding([12, 10]).style(t::nav_row(false)).on_press_maybe((!app.busy).then_some(Msg::SignOut))).padding(Padding { top: 30.0, ..Padding::ZERO }),
        space().height(FILL),
        text("Free to use. Your space to keep.").size(11).color(rgb(0x74808b)),
    ].align_x(Horizontal::Center)).width(270).height(FILL).padding(Padding { top: 34.0, bottom: 24.0, left: 20.0, right: 20.0 }).style(|_| container::Style { background: Some(rgb(0x161a1e).into()), border: iced::Border { radius: iced::border::Radius { top_left: 17.0, bottom_left: 17.0, top_right: 0.0, bottom_right: 0.0 }, ..Default::default() }, ..Default::default() });

    let heading = |eyebrow: &'static str, title: &'static str, note: &'static str| -> Column<'_, Msg> {
        column![t::eyebrow(eyebrow, 11.0, t::EYEBROW), container(text(title).size(24).font(t::semibold()).color(t::TEXT)).padding([10, 0]), body(note, 15.0, rgb(0x9ba4aa), 1.8)]
    };
    let content: Column<Msg> = match app.tab {
        Tab::Alerts => {
            let volume = app.settings.alert_volume.unwrap_or(0.8);
            let mut words = Row::new().spacing(6);
            for word in &app.settings.keywords { words = words.push(button(text(format!("{word} ×")).size(13).color(rgb(0xd0d7dc))).padding([5, 9]).style(t::secondary()).on_press(Msg::RemoveKeyword(word.clone()))); }
            let mut blocked = Column::new().spacing(6);
            if app.preferences.blocked.is_empty() { blocked = blocked.push(text("Nobody is blocked.").size(13).color(rgb(0x78848f))); }
            for person in &app.preferences.blocked {
                blocked = blocked.push(row![avatar(person_face(person), 0, None, 35.0), column![text(person.name.clone()).size(15).font(t::semibold()).color(t::TEXT), text(format!("@{}", person.username)).size(13).color(rgb(0x78848e))].spacing(3).width(FILL), secondary("Unblock", None, Some(Msg::Unblock(person.id.clone())))].spacing(11).align_y(Vertical::Center));
            }
            let title = |value: &'static str| -> E<'_> { container(card_title(value)).padding(Padding { top: 24.0, bottom: 8.0, ..Padding::ZERO }).into() };
            heading("ONLY WHAT MATTERS", "Alerts & privacy", "Choose what gets your attention, and who can reach you.")
                .push(title("Alert volume"))
                .push(row![slider(0.0..=1.0, volume, Msg::AlertVolume).step(0.05_f32), text(format!("{}%", (volume * 100.0).round() as u32)).size(13).color(rgb(0xd0d7dc))].spacing(12).align_y(Vertical::Center))
                .push(small_note("Message sounds and the ringtone. Separate from call volume."))
                .push(v_space(18.0))
                .push(checkbox_row("Ask before starting a call", app.settings.confirm_calls, Msg::ConfirmCalls))
                .push(v_space(10.0))
                .push(checkbox_row("Turn off typing indicators (yours and everyone else's)", app.preferences.typing_hidden, Msg::TypingHidden))
                .push(v_space(10.0))
                .push(checkbox_row("Keep Sonar running in the tray when I close the window", app.settings.keep_in_tray, Msg::KeepInTray))
                .push(v_space(10.0))
                .push(checkbox_row("Start Sonar with Windows", app.settings.start_with_windows, Msg::StartWithWindows))
                .push(title("Keyword alerts"))
                .push(small_note("Alert me when these words appear, even in muted servers. Matched on this device, so the server never sees them."))
                .push(container(row![text_input("Add a word", &app.keyword_input).on_input(Msg::KeywordInput).on_submit(Msg::AddKeyword).size(14).padding([9, 12]).style(t::card_input()), button(text("Add").size(13).font(t::semibold()).color(rgb(0x1e2329))).padding([8, 12]).style(t::primary()).on_press_maybe((!app.keyword_input.trim().is_empty()).then_some(Msg::AddKeyword))].spacing(10).align_y(Vertical::Center)).padding([8, 0]))
                .push(words.wrap())
                .push(title("Blocked people"))
                .push(small_note("They cannot message, call or send you requests, and their server messages are hidden completely. They are not told."))
                .push(v_space(8.0))
                .push(blocked)
        }
        Tab::Profile => heading("A FAMILIAR FACE", "Your profile", "The name your people see in conversations and communities.")
            .push({
                let current = t::animal_index(session.account.id.as_deref().unwrap_or_default(), session.account.animal.as_deref());
                let mut grid = Column::new().spacing(8);
                for line in t::ANIMALS.chunks(8) {
                    let mut cells = Row::new().spacing(8);
                    for (name, icon) in line {
                        let chosen = t::ANIMALS[current].0 == *name;
                        cells = cells.push(button(container(t::icon(*icon, 30.0, if chosen { rgb(0xd7f3ff) } else { rgb(0xcfd4d7) })).center_x(44).center_y(44)).padding(0).width(44).height(44).style(if chosen { t::person_row(true) } else { t::secondary() }).on_press_maybe((!chosen && !app.busy).then_some(Msg::SetAnimal(name))));
                    }
                    grid = grid.push(cells);
                }
                container(grid).padding(Padding { top: 14.0, ..Padding::ZERO })
            })
            .push(container(field("Display name", dialog_input("", &app.profile_name, Msg::ProfileName, false), 10.0, 13.0, rgb(0xd0d7dc))).padding([22, 0]))
            .push(field("Username", dialog_input("", &app.profile_username, Msg::ProfileUsername, false), 10.0, 13.0, rgb(0xd0d7dc)))
            .push(container(text("People can find you with this. Letters, numbers, dots and underscores.").size(12).color(rgb(0x78848f))).padding(Padding { top: 8.0, bottom: 22.0, ..Padding::ZERO }))
            .push(field("Email address", text_input("", &session.account.email).padding(13).size(15).style(t::input(rgb(0x191e23), rgb(0x3a4450), rgb(0x3a4450), 7.0)).into(), 10.0, 13.0, rgb(0xd0d7dc)))
            .push(container(text("Email changes are not available yet.").size(12).color(rgb(0x78848f))).padding(Padding { top: 8.0, bottom: 22.0, ..Padding::ZERO }))
            .push(primary(if app.busy { "Saving..." } else { "Save changes" }, Some(Icon::Check), (!app.busy && !app.profile_name.trim().is_empty() && !app.profile_username.is_empty() && (app.profile_name.trim() != session.account.full_name || app.profile_username != session.account.username)).then_some(Msg::SaveProfile)))
            .push(v_space(28.0))
            .push(honest_note(Icon::UserRound, "Your display name and email are account information visible to the service. Message content is encrypted separately on your device.")),
        Tab::Security => {
            // Passkeys.
            let mut passkeys = Column::new();
            match &app.passkeys {
                None => passkeys = passkeys.push(notice("Loading passkeys...")),
                Some(list) if list.is_empty() => passkeys = passkeys.push(container(row![t::icon(Icon::KeyRound, 18.0, rgb(0xd0d7dc)), column![text("No passkeys yet").size(14).font(t::semibold()).color(rgb(0xd0d7dc)), body("Add one to sign in with your face, fingerprint, or device PIN.", 13.0, rgb(0x77838e), 1.7)].spacing(5)].spacing(13))
                    .padding(16).width(FILL).style(t::panel(rgb(0x191d22), rgb(0x38424e), 8.0))).push(v_space(17.0)),
                Some(list) => for passkey in list {
                    let removing = app.passkey_removing.as_deref() == Some(&passkey.id);
                    passkeys = passkeys.push(container(container(row![
                        t::icon(Icon::KeyRound, 16.0, rgb(0xd0d7dc)),
                        column![text(passkey.name.clone()).size(14).font(t::semibold()).color(rgb(0xd0d7dc)), text(format!("Added {}", day(&passkey.created_at))).size(12).color(rgb(0x818c96))].spacing(5).width(FILL),
                        icon_btn(Icon::Trash, 16.0, (!app.busy).then(|| Msg::PasskeyRemove(if removing { None } else { Some(passkey.id.clone()) })), removing),
                    ].spacing(10).align_y(Vertical::Center)).padding(12).width(FILL).style(t::panel(rgb(0x1a1f25), rgb(0x303842), 7.0))).padding(Padding { top: 8.0, ..Padding::ZERO }));
                },
            }
            if app.passkey_removing.is_some() {
                passkeys = passkeys.push(container(column![
                    text("Remove this passkey?").size(15).font(t::semibold()).color(rgb(0xd0d7dc)),
                    body("Confirm with your current password. Removing it signs out all sessions.", 14.0, rgb(0x969fa6), 1.8),
                    container(field("Current password", card_input("", &app.passkey_password, Msg::PasskeyPassword, true), 9.0, 13.0, rgb(0xd0d7dc))).padding([17, 0]),
                    row![
                        action(if app.busy { "Removing..." } else { "Remove passkey" }, None, t::danger(), Color::WHITE, (!app.busy && !app.passkey_password.is_empty()).then_some(Msg::RemovePasskey), [11.0, 14.0], 14.0),
                        button(text("Cancel").size(14)).padding([10, 5]).style(t::text_button()).on_press(Msg::PasskeyRemove(None)),
                    ].spacing(14),
                ]).padding(Padding { top: 16.0, ..Padding::ZERO }));
            }
            #[cfg(windows)]
            let add_passkey = Msg::PasskeyAdding(true);
            if app.passkey_adding {
                #[cfg(windows)]
                let create = (!app.busy && !app.passkey_name.trim().is_empty() && !app.passkey_password.is_empty()).then_some(Msg::AddPasskey);
                #[cfg(not(windows))]
                let create: Option<Msg> = None;
                passkeys = passkeys.push(container(column![
                    text("Add a passkey").size(15).font(t::semibold()).color(rgb(0xd0d7dc)),
                    container(field("Passkey name", card_input("My laptop", &app.passkey_name, Msg::PasskeyName, false), 9.0, 13.0, rgb(0xd0d7dc))).padding(Padding { top: 17.0, ..Padding::ZERO }),
                    container(field("Current password", card_input("", &app.passkey_password, Msg::PasskeyPassword, true), 9.0, 13.0, rgb(0xd0d7dc))).padding([17, 0]),
                    row![
                        action(if app.busy { "Waiting for Windows..." } else { "Create passkey" }, None, t::primary(), rgb(0x1e2329), create, [11.0, 14.0], 14.0),
                        button(text("Cancel").size(14)).padding([10, 5]).style(t::text_button()).on_press(Msg::PasskeyAdding(false)),
                    ].spacing(14),
                ]).padding(Padding { top: 16.0, ..Padding::ZERO }));
            } else if app.passkey_removing.is_none() {
                #[cfg(windows)]
                { passkeys = passkeys.push(container(button(row![t::icon(Icon::Plus, 16.0, rgb(0xd0d7dc)), text("Add a passkey").size(14).color(rgb(0xd0d7dc))].spacing(12).align_y(Vertical::Center)).padding([10, 13]).style(t::secondary()).on_press_maybe((!app.busy).then_some(add_passkey))).padding(Padding { top: 18.0, ..Padding::ZERO })); }
            }
            passkeys = passkeys.push(small_note("Your passkey signs you in. Your messages unlock with your recovery key the first time you use a new device."));
            let passkey_title = row![card_title("Passkeys"), container(text(t::tracked("RECOMMENDED")).size(10).font(t::bold()).color(rgb(0xd0d7dc))).padding([3, 5]).style(t::panel(rgb(0x242b32), rgb(0x74808b), 4.0))].spacing(9).align_y(Vertical::Center);

            // Sessions.
            let mut sessions = Column::new();
            match &app.sessions {
                None => sessions = sessions.push(notice("Loading sessions...")),
                Some(list) => for entry in &list.sessions {
                    let method = if entry.method == "passkey" { "Passkey sign-in" } else { "Password sign-in" };
                    sessions = sessions.push(column![row![
                        column![text(format!("{method}{}", if entry.current { " · This session" } else { "" })).size(15).font(t::semibold()).color(t::TEXT), text(format!("Signed in {} · Expires {}", date(&entry.created_at), day(&entry.expires_at))).size(13).color(rgb(0xd0d7dc))].spacing(4).width(FILL),
                        button(row![t::icon(Icon::LogOut, 14.0, rgb(0xd0d7dc)), text("Sign out").size(15).color(rgb(0xd0d7dc))].spacing(10).align_y(Vertical::Center)).padding([12, 16]).style(t::secondary()).on_press_maybe((!app.busy).then(|| Msg::EndSession(entry.id.clone()))),
                    ].align_y(Vertical::Center).padding([10, 0]), t::rule(rgb(0x2a323b), true)]);
                },
            }
            sessions = sessions.push(small_note("Session entries don't include device names or IP addresses. Signing out does not erase encryption keys from a device."));

            // Password.
            let show = app.show_passwords;
            let pw = |index: usize, label: &'static str| -> E { field(label, card_input("", &app.passwords[index], move |v| Msg::PasswordField(index, v), !show), 9.0, 13.0, rgb(0xd0d7dc)) };
            let password = column![
                pw(0, "Current password"),
                row![container(pw(1, "New password")).width(FILL), container(pw(2, "Confirm new password")).width(FILL)].spacing(14),
                checkbox_row("Show passwords", show, Msg::ShowPasswords),
                text("Use at least 10 characters.").size(12).color(rgb(0x919ba2)),
                action(if app.busy { "Updating password..." } else { "Update password & sign out" }, Some(Icon::KeyRound), t::primary(), rgb(0x1e2329), (!app.busy).then_some(Msg::ChangePassword), [12.0, 14.0], 14.0),
                column![t::rule(rgb(0x2a323b), true), row![t::icon(Icon::LockKeyhole, 13.0, rgb(0x89939b)), body("Your message key is re-protected with the new password on this device. Your encrypted history is kept.", 12.0, rgb(0x89939b), 1.8)].spacing(8).padding(Padding { top: 15.0, ..Padding::ZERO })].padding(Padding { top: 2.0, ..Padding::ZERO }),
            ].spacing(17);
            heading("KEEP YOUR SPACE YOURS", "Password & passkeys", "Choose how you sign in and keep your account secure.")
                .push(v_space(24.0))
                .push(security_card(Icon::KeyRound, passkey_title.into(), "A faster sign-in with your face, fingerprint, or device PIN.", passkeys.into()))
                .push(v_space(24.0))
                .push(security_card(Icon::Monitor, card_title("Signed-in sessions"), "Review sign-ins and sign out individual sessions.", sessions.into()))
                .push(v_space(24.0))
                .push(security_card(Icon::LockKeyhole, card_title("Password"), "Updating it signs you out on all devices.", password.into()))
        }
        Tab::Privacy => {
            let print = session.account.encryption_key.as_deref().and_then(|k| crate::crypto::fingerprint(k).ok()).unwrap_or_else(|| "Safety code unavailable".into());
            let (status, detail, dot) = match app.backup_kind {
                Some("recovery") => ("Protected by your recovery key", "Only your recovery key can unlock it · AES-256-GCM", t::GREEN),
                Some("password") => ("Protected by your password", "Set up a recovery key so the server can never unlock it", t::OFFLINE),
                Some(_) => ("Only on this device", "Other devices cannot open your messages yet", t::OFFLINE),
                None => ("Checking your key...", " ", t::OFFLINE),
            };
            let mut col = heading("PRIVACY IN PLAIN SIGHT", "Your message key", "Messages are encrypted with a key that only your devices hold. It is locked with your recovery key before it is stored, so the web app and the desktop app can both open your conversations and Sonar never can.")
                .push(container(container(row![t::icon(Icon::ShieldCheck, 21.0, rgb(0xd0d7dc)), column![text(status).size(14).font(t::semibold()).color(rgb(0xd0d7dc)), text(detail).size(12).color(rgb(0x8d979e))].spacing(6).width(FILL), t::dot(dot, 5.0)].spacing(12).align_y(Vertical::Center))
                    .padding(16).width(FILL).style(t::panel(rgb(0x21272e), rgb(0x37404b), 8.0))).padding([24, 0]));
            if let Some(kind) = app.backup_kind {
                let (title, note, action) = match kind {
                    "none" => ("Turn on multi-device access", "Creates a new key protected by a recovery key. Your contacts will be asked to confirm your new key. Messages encrypted for the old key cannot be opened by the new one.", "Turn on multi-device access"),
                    "password" => ("Set up a recovery key", "Your recovery key unlocks your messages on a new device and resets a forgotten password. A new one replaces the old one.", "Create recovery key"),
                    _ => ("Create a new recovery key", "Your recovery key unlocks your messages on a new device and resets a forgotten password. A new one replaces the old one.", "Create recovery key"),
                };
                col = col.push(container(column![
                    text(title).size(16).font(t::semibold()).color(rgb(0xd0d7dc)),
                    body(note, 15.0, rgb(0x8a949b), 1.6),
                    field("Current password", dialog_input("", &app.sync_password, Msg::SyncPassword, true), 10.0, 13.0, rgb(0xd0d7dc)),
                    primary(if app.busy { "Securing your key..." } else { action }, Some(Icon::KeyRound), (!app.busy && !app.sync_password.is_empty()).then_some(Msg::EnableSync)),
                ].spacing(12)).padding(16).width(FILL).style(t::panel(rgb(0x1b2025), rgb(0x313943), 10.0))).push(v_space(22.0));
            }
            col.push(container(column![
                    text("Your safety code").size(13).font(t::semibold()).color(rgb(0xd0d7dc)),
                    container(body(print, 14.0, rgb(0xb4bbc0), 1.8)).padding(Padding { top: 8.0, ..Padding::ZERO }),
                    container(button(row![t::icon(Icon::Copy, 14.0, rgb(0xd0d7dc)), text("Copy safety code").size(12).color(rgb(0xd0d7dc))].spacing(12).align_y(Vertical::Center)).padding([8, 10]).style(t::secondary()).on_press(Msg::CopyFingerprint)).padding(Padding { top: 14.0, ..Padding::ZERO }),
                ]).padding(12).width(FILL).style(t::panel(rgb(0x14181c), rgb(0x2a323b), 7.0)))
                .push(container(body("Read this code to a contact in person or on a call so they can confirm it is really you. It is a code, not a fingerprint scan.", 13.0, rgb(0x919ba2), 1.8)).padding([12, 0]))
                .push(column![t::rule(rgb(0x2a313a), true), container(column![text("A short memory, by design.").size(16).font(t::semibold()).color(t::TEXT), body("Messages expire after seven days unless pinned. Calls have no recording or storage in Sonar.", 14.0, rgb(0x919ba2), 1.8)].spacing(9)).padding(Padding { top: 20.0, ..Padding::ZERO })])
                .push(v_space(28.0))
                .push(honest_note(Icon::LockKeyhole, "If you lose your recovery key and every signed-in device, your encrypted history cannot be recovered. Forward secrecy is not implemented. Metadata remains visible to the service."))
        }
        Tab::Voice => {
            let devices = app.audio_devices.as_ref();
            let mut inputs = vec![SYSTEM_DEFAULT.to_string()];
            let mut outputs = vec![SYSTEM_DEFAULT.to_string()];
            if let Some(d) = devices { inputs.extend(d.inputs.iter().cloned()); outputs.extend(d.outputs.iter().cloned()); }
            let input = Some(app.settings.input.clone().unwrap_or_else(|| SYSTEM_DEFAULT.into()));
            let output = Some(app.settings.output.clone().unwrap_or_else(|| SYSTEM_DEFAULT.into()));
            let level = app.mic_level.unwrap_or(0).min(100) as f32 / 100.0;
            let meter = container(container(space()).width(Length::FillPortion((level * 100.0) as u16 + 1)).height(FILL).style(t::gradient_panel(90.0, rgb(0x929ca3), rgb(0xd0d7dc), Color::TRANSPARENT, 8.0)))
                .width(FILL).height(10).style(t::panel(rgb(0x15181c), rgb(0x323b46), 8.0));
            let meter = row![meter, space().width(Length::FillPortion(((1.0 - level) * 100.0) as u16 + 1))];
            heading("SOUND LIKE YOURSELF", "Voice & audio", "Choose your microphone and speakers. Settings stay on this computer.")
                .push(v_space(24.0))
                .push(security_card(Icon::Mic, card_title("Your microphone"), "Test your input before your next call.", column![
                    field("Microphone", pick_list(inputs, input, Msg::SelectInput).padding(11).text_size(14).width(FILL).style(t::select()).menu_style(t::menu()).into(), 9.0, 13.0, rgb(0xd0d7dc)),
                    container(meter).padding(Padding { top: 20.0, bottom: 14.0, ..Padding::ZERO }),
                    row![button(text(if app.testing { "Listening..." } else { "Test microphone" }).size(13).color(rgb(0xd0d7dc))).padding([9, 12]).style(t::secondary()).on_press_maybe((!app.testing).then_some(Msg::TestMic)),
                        body(app.mic_level.map(|l| format!("Peak level {l}%. Nothing is recorded.")).unwrap_or_else(|| "A three-second local test. Nothing is recorded or sent.".into()), 12.0, rgb(0xd0d7dc), 1.7)].spacing(10).align_y(Vertical::Center),
                ].into()))
                .push(v_space(24.0))
                .push(security_card(Icon::Speaker, card_title("Your speakers"), "Choose where calls play.", column![
                    checkbox_row("Noise suppression", !app.settings.noise_suppression_off, Msg::NoiseSuppression),
                    field("Speakers", pick_list(outputs, output, Msg::SelectOutput).padding(11).text_size(14).width(FILL).style(t::select()).menu_style(t::menu()).into(), 9.0, 13.0, rgb(0xd0d7dc)),
                    container(button(row![t::icon(Icon::Play, 14.0, rgb(0xd0d7dc)), text(if app.testing { "Playing..." } else { "Play test sound" }).size(13).color(rgb(0xd0d7dc))].spacing(10).align_y(Vertical::Center)).padding([10, 13]).style(t::secondary()).on_press_maybe((!app.testing).then_some(Msg::TestSpeakers))).padding(Padding { top: 18.0, ..Padding::ZERO }),
                ].into()))
                .push(v_space(24.0))
                .push(security_card(Icon::Shield, card_title("Call connectivity"), "Relay-only hides your network address from the other person, but needs a TURN server.", checkbox_row("Use relay-only connections", app.settings.relay_only, Msg::RelayOnly)))
        }
    };
    let content = container(scroll_area(container(content).padding(Padding { top: 44.0, bottom: 32.0, left: 36.0, right: 36.0 })).height(FILL)).width(FILL).height(FILL);
    let close_button = container(icon_btn(Icon::X, 20.0, (!app.busy).then_some(close.clone()), false)).width(FILL).align_x(Horizontal::Right).padding(18);
    let card = container(row![navigation, t::rule(rgb(0x2a313a), false), stack![content, close_button]]).width(940).max_height(720).height(FILL)
        .style(t::modal_card(rgb(0x1b2025), rgb(0x333c46), 17.0));
    opaque(mouse_area(center(opaque(card)).padding(20).style(t::fill(rgba(0x090b0db8)))).on_press(if app.busy { Msg::Noop } else { close })).into()
}

#[cfg(test)]
mod room_status_tests {
    use super::room_status;
    #[test]
    fn room_status_matches_the_web() {
        assert_eq!(room_status(true, 1, 0), "Connecting…");
        assert_eq!(room_status(false, 0, 0), "Waiting for others to join");
        assert_eq!(room_status(false, 1, 0), "Connecting…");
        assert_eq!(room_status(false, 1, 1), "Connected");
        assert_eq!(room_status(false, 3, 3), "Connected to everyone");
        assert_eq!(room_status(false, 3, 1), "Connected to 1 of 3");
    }
}
