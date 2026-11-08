//! Native views mirroring the web client's pages and components:
//! AuthPage, UnpairedDevicePage, HomePage, CommunityWorkspace, AccountSettings.
use crate::theme::{self as t, Icon, rgb, rgba};
use crate::{App, CallStatus, Confirm, Dialog, Mode, Msg, Panel, Screen, Tab, ToastKind, UnlockReason};
use iced::alignment::{Horizontal, Vertical};
use iced::widget::text::LineHeight;
use iced::widget::{button, center, column, container, hover, mouse_area, opaque, pick_list, row, scrollable, space, stack, text, text_editor, text_input, Column, Row};
use iced::{Color, Element, Length, Padding};

pub const SYSTEM_DEFAULT: &str = "System default";
type E<'a> = Element<'a, Msg>;
const FILL: Length = Length::Fill;

pub fn view(app: &App) -> E<'_> {
    let page: E = if let Some(key) = &app.new_recovery { recovery_view(app, key) } else { match app.screen {
        Screen::Boot => boot(),
        Screen::Auth => auth(app),
        Screen::Unlock => unlock(app),
        Screen::Main => workspace(app),
    } };
    crate::wheel::browser_speed(stack![page, toasts(app)].into())
}

// ----- Shared pieces --------------------------------------------------------

fn h_space(width: f32) -> E<'static> { space().width(width).into() }
fn v_space(height: f32) -> E<'static> { space().height(height).into() }
fn grow() -> E<'static> { space().width(FILL).into() }

fn body<'a>(value: impl text::IntoFragment<'a>, size: f32, color: Color, line: f32) -> iced::widget::Text<'a> {
    text(value).size(size).color(color).font(t::REGULAR).line_height(LineHeight::Relative(line))
}

fn avatar<'a>(name: &str, index: usize, online: Option<Color>, size: f32) -> E<'a> {
    let (background, color) = t::avatar_colors(index);
    tile(name, background, color, None, size, if size > 40.0 { 23.0 } else { 12.0 }, if size > 40.0 { 24.0 } else { 14.0 }, online)
}
#[allow(clippy::too_many_arguments)]
fn tile<'a>(name: &str, background: Color, color: Color, line: Option<Color>, size: f32, r: f32, font: f32, online: Option<Color>) -> E<'a> {
    let face = container(text(t::initials(name)).size(font).color(color).font(t::semibold()))
        .width(size).height(size).center_x(size).center_y(size)
        .style(move |_| container::Style { background: Some(background.into()), border: t::border(line.unwrap_or(Color::TRANSPARENT), if line.is_some() { 1.0 } else { 0.0 }, r), ..Default::default() });
    match online {
        Some(ring) => {
            let badge = container(container(space()).width(9).height(9).style(move |_| container::Style { background: Some(t::rgb(0x88c7a3).into()), border: t::border(ring, 2.0, 6.0), ..Default::default() }))
                .width(size + 2.0).height(size + 2.0).align_x(Horizontal::Right).align_y(Vertical::Bottom);
            stack![container(face).padding(Padding { right: 1.0, bottom: 1.0, ..Padding::ZERO }), badge].into()
        }
        None => face.into(),
    }
}

fn icon_btn<'a>(icon: Icon, size: f32, on: Option<Msg>, active: bool) -> E<'a> {
    let color = if active { t::ACCENT } else { rgb(0x8e8e9b) };
    button(container(t::icon(icon, size, color)).center_x(30).center_y(32)).padding(0).width(30).height(32).style(t::icon_button(active)).on_press_maybe(on).into()
}

fn action<'a>(label: impl Into<String>, icon: Option<Icon>, style: Box<dyn Fn(&iced::Theme, button::Status) -> button::Style>, color: Color, on: Option<Msg>, padding: [f32; 2], size: f32) -> E<'a> {
    let label: String = label.into();
    let mut content = Row::new().spacing(if padding[1] > 15.0 { 22.0 } else { 12.0 }).align_y(Vertical::Center);
    content = content.push(text(label).size(size).color(color).font(t::semibold()));
    if let Some(icon) = icon { content = content.push(t::icon(icon, size + 5.0, color)); }
    button(content).padding(padding).style(style).on_press_maybe(on).into()
}
fn primary<'a>(label: impl Into<String>, icon: Option<Icon>, on: Option<Msg>) -> E<'a> { action(label, icon, t::primary(), rgb(0x251c31), on, [13.0, 18.0], 15.0) }
/// A primary button filling its form, like `.auth-card form .primary`.
fn primary_wide<'a>(label: impl Into<String>, icon: Icon, on: Option<Msg>) -> E<'a> {
    let label: String = label.into();
    let color = rgb(0x251c31);
    button(row![text(label).size(15).color(color).font(t::semibold()), grow(), t::icon(icon, 18.0, color)].align_y(Vertical::Center)).width(FILL).padding([13, 18]).style(t::primary()).on_press_maybe(on).into()
}
fn secondary<'a>(label: impl Into<String>, icon: Option<Icon>, on: Option<Msg>) -> E<'a> {
    let label: String = label.into();
    let mut content = Row::new().spacing(12).align_y(Vertical::Center).push(text(label).size(15).color(rgb(0xc1b0d0)).font(t::REGULAR));
    if let Some(icon) = icon { content = content.push(t::icon(icon, 16.0, rgb(0xc1b0d0))); }
    button(content).padding([12, 16]).style(t::secondary()).on_press_maybe(on).into()
}
fn danger<'a>(label: impl Into<String>, on: Option<Msg>) -> E<'a> {
    let label: String = label.into();
    button(text(label).size(15).color(Color::WHITE).font(t::semibold())).padding([13, 18]).style(t::danger()).on_press_maybe(on).into()
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
    container(row![t::icon(icon, 16.0, rgb(0xb59dcf)), body(note, 13.0, rgb(0xa69aaa), 1.8)].spacing(9))
        .padding(13).width(FILL).style(t::panel(rgb(0x2c2832), rgb(0x3b3345), 7.0)).into()
}

fn scroll_area<'a>(content: impl Into<E<'a>>) -> scrollable::Scrollable<'a, Msg> {
    scrollable(content).direction(scrollable::Direction::Vertical(scrollable::Scrollbar::new().width(6).scroller_width(6).margin(1))).style(t::scroll())
}

/// A centered dialog over a dimmed backdrop (`.modal-backdrop` + card).
fn modal<'a>(content: E<'a>, width: f32, close: Option<Msg>) -> E<'a> {
    let card = container(content).max_width(width).width(FILL).style(t::modal_card(rgb(0x222127), rgb(0x44404c), 17.0));
    let backdrop = mouse_area(center(opaque(card)).padding(20).style(t::fill(rgba(0x09090eb8))));
    opaque(match close { Some(msg) => backdrop.on_press(msg), None => backdrop }).into()
}
fn modal_header<'a>(icon: Icon, size: f32, close: Msg) -> E<'a> {
    row![t::icon(icon, size, t::ACCENT), grow(), icon_btn(Icon::X, 20.0, Some(close), false)].align_y(Vertical::Top).into()
}
fn modal_title<'a>(value: impl text::IntoFragment<'a>) -> E<'a> {
    container(text(value).size(26).font(t::bold()).color(t::TEXT).line_height(LineHeight::Relative(1.35))).padding([15, 0]).into()
}

fn boot<'a>() -> E<'a> {
    center(column![t::sonar_mark(1.0), text("Opening your private space...").size(16).color(rgb(0xac9abd))].spacing(25).align_x(Horizontal::Center)).into()
}

fn toasts(app: &App) -> E<'_> {
    let list = app.toasts.iter().fold(Column::new().spacing(8).align_x(Horizontal::Center), |list, toast| {
        let (icon, color) = match toast.kind { ToastKind::Success => (Some(Icon::CircleCheck), rgb(0x61d345)), ToastKind::Error => (Some(Icon::CircleX), rgb(0xff4b4b)), ToastKind::Plain => (None, t::TEXT) };
        let mut content = Row::new().spacing(10).align_y(Vertical::Center);
        if let Some(icon) = icon { content = content.push(t::icon(icon, 20.0, color)); }
        content = content.push(body(toast.text.as_str(), 16.0, rgb(0xeeeeee), 1.3));
        list.push(mouse_area(container(content).padding([10, 14]).max_width(350).style(|_| container::Style {
            background: Some(rgb(0x24262b).into()), border: t::border(Color::TRANSPARENT, 0.0, 8.0),
            ..Default::default()
        })).on_press(Msg::DismissToast(toast.id)))
    });
    container(list).width(FILL).height(FILL).align_x(Horizontal::Center).align_y(Vertical::Bottom).padding(Padding { bottom: 16.0, ..Padding::ZERO }).into()
}

// ----- Sign in / sign up (AuthPage) -----------------------------------------

fn auth(app: &App) -> E<'_> {
    let form = &app.auth;
    let feature = |icon: Icon, label: &'static str| -> E<'static> { lead_icon(icon, label, rgb(0xb4a2c7), 17.0, 13.0, 7.0).into() };
    let story = container(column![
        row![t::sonar_mark(1.0), text("sonar").size(25).font(t::bold()).color(t::TEXT),
            container(t::eyebrow("EARLY ACCESS", 10.0, rgb(0xb8a5ce))).padding(5).style(t::panel(Color::TRANSPARENT, rgb(0x52475f), 4.0))].spacing(12).align_y(Vertical::Center),
        space().height(FILL),
        t::eyebrow("LESS NOISE. MORE CONNECTION.", 13.0, t::EYEBROW),
        column![
            text("Your people.").size(42).font(t::bold()).color(t::TEXT),
            text("Your conversations.").size(42).font(t::bold()).color(t::TEXT),
            text("Your space.").size(42).font(iced::Font { style: iced::font::Style::Italic, ..t::bold() }).color(rgb(0xc2a7ed)),
        ].spacing(2).padding([24, 0]),
        container(body("A quieter corner of the internet. Built for the conversations that matter, with privacy at the core.", 17.0, rgb(0xa39aaa), 1.9)).max_width(365),
        row![feature(Icon::LockKeyhole, "Encrypted messages"), feature(Icon::Timer, "Gone in 7 days"), feature(Icon::AudioLines, "No call recordings")].spacing(20).padding(Padding { top: 36.0, ..Padding::ZERO }),
        space().height(FILL),
        text("Free to use. No subscriptions. No ads.").size(13).color(rgb(0x8b7f98)),
    ]).padding([45, 65]).width(Length::FillPortion(105)).height(FILL)
        .style(t::gradient_panel(35.0, rgb(0x32283d), rgb(0x1d1b24), Color::TRANSPARENT, 0.0));

    let label = |value: &'static str| text(value).size(14).color(rgb(0xbcb3c6));
    let entry = |value: &str, field: u8, secure: bool| -> E<'_> {
        let input = text_input("", value).on_input(move |v| Msg::AuthField(field, v)).secure(secure).padding(12).size(16).style(t::auth_input());
        if field == 2 { input.on_submit(Msg::Submit).into() } else { input.into() }
    };
    let mut fields = Column::new().spacing(20);
    if form.signup { fields = fields.push(column![label("Display name"), entry(&form.name, 0, false)].spacing(9)); }
    fields = fields.push(column![label("Email address"), entry(&form.email, 1, false)].spacing(9));
    if form.recover {
        fields = fields.push(column![label("Recovery key"), text_input("XXXX-XXXX-XXXX-XXXX-XXXX-XXXX", &form.recovery_key).on_input(|v| Msg::AuthField(4, v)).padding(12).size(16).style(t::auth_input())].spacing(9));
        fields = fields.push(column![label("New password"), text_input("", &form.password).on_input(|v| Msg::AuthField(2, v)).secure(true).padding(12).size(16).style(t::auth_input())].spacing(9));
        fields = fields.push(column![label("Confirm new password"), text_input("", &form.confirm).on_input(|v| Msg::AuthField(5, v)).on_submit(Msg::Recover).secure(true).padding(12).size(16).style(t::auth_input())].spacing(9));
    } else {
        fields = fields.push(column![label("Password"), entry(&form.password, 2, true)].spacing(9));
    }
    if !form.signup && !form.recover {
        fields = fields.push(container(button(text("Forgot your password?").size(14).color(rgb(0xc3a5ec))).padding(0).style(t::plain(rgb(0xc3a5ec), rgb(0xdcc8f6))).on_press(Msg::ToggleRecover)).width(FILL).align_x(Horizontal::Right).padding(Padding { top: -10.0, ..Padding::ZERO }));
    }
    let submit_label = if form.busy && form.recover { "Resetting your password..." } else if form.busy { "Setting up your space..." } else if form.recover { "Reset password" } else if form.signup { "Create your account" } else { "Enter your space" };
    fields = fields.push(container(primary_wide(submit_label, Icon::ArrowUpRight, (!form.busy).then_some(if form.recover { Msg::Recover } else { Msg::Submit }))).padding(Padding { top: 5.0, ..Padding::ZERO }));

    let mut card = Column::new().width(FILL)
        .push(t::icon(Icon::ShieldCheck, 30.0, t::ACCENT))
        .push(container(text(if form.recover { "Reset your password." } else if form.signup { "Find your frequency." } else { "Welcome back." }).size(28).font(t::bold()).color(t::TEXT)).padding(Padding { top: 22.0, bottom: 10.0, ..Padding::ZERO }))
        .push(body(if form.recover { "Use the recovery key you saved when you created your account. Your messages stay encrypted and readable." } else if form.signup { "Create your own little corner of Sonar." } else { "Good conversations are waiting for you." }, 15.0, rgb(0x99919f), 1.8))
        .push(container(fields).padding(Padding { top: 30.0, bottom: 22.0, ..Padding::ZERO }));
    if !form.signup && !form.recover {
        let divider = row![t::rule(rgb(0x3d3348), true), text("or").size(13).color(rgb(0x7e6e8c)), t::rule(rgb(0x3d3348), true)].spacing(12).align_y(Vertical::Center);
        #[cfg(windows)]
        let passkey_press = (!form.busy).then_some(Msg::PasskeySignIn);
        #[cfg(not(windows))]
        let passkey_press: Option<Msg> = None;
        let passkey = button(container(lead_icon(Icon::KeyRound, "Sign in with a passkey", rgb(0xcdb3e1), 17.0, 15.0, 12.0)).center_x(FILL))
            .width(FILL).padding([12, 16]).style(t::secondary()).on_press_maybe(passkey_press);
        card = card.push(container(divider).padding(Padding { top: 24.0, bottom: 15.0, ..Padding::ZERO }))
            .push(passkey)
            .push(container(body("Use a passkey you previously added in account settings.", 12.0, rgb(0x8f7e9d), 1.8)).padding(Padding { top: 10.0, bottom: 24.0, ..Padding::ZERO }));
    }
    card = card.push(row![
        text(if form.signup || form.recover { "Remembered it?" } else { "New around here?" }).size(15).color(rgb(0x99919f)),
        button(text(if form.signup || form.recover { "Sign in" } else { "Join Sonar" }).size(15).color(rgb(0xc3a5ec))).padding(0).style(t::plain(rgb(0xc3a5ec), rgb(0xdcc8f6))).on_press(Msg::ToggleSignup),
    ].spacing(6));
    // Desktop only: which Sonar server to use (the browser uses its own address).
    let server: E = if form.server_open {
        column![
            text("Sonar server").size(14).color(rgb(0xbcb3c6)),
            text_input("https://your-sonar.example", &form.server).on_input(|v| Msg::AuthField(3, v)).padding(10).size(15).style(t::auth_input()),
        ].spacing(8).into()
    } else {
        button(lead_icon(Icon::Server, format!("Server: {}", form.server.trim_start_matches("https://").trim_start_matches("http://")), rgb(0x82758e), 13.0, 13.0, 7.0)).padding(0).style(t::plain(rgb(0x82758e), rgb(0xb49cc4))).on_press(Msg::ToggleServer).into()
    };
    card = card.push(container(column![
        t::rule(rgb(0x35313c), true),
        row![t::icon(Icon::LockKeyhole, 15.0, rgb(0x82758e)), body("Your messages are locked with a recovery key that only you keep. Sonar never has it, so it can never read your conversations, on the web or the desktop app.", 13.0, rgb(0x82758e), 1.8)].spacing(9).padding(Padding { top: 22.0, ..Padding::ZERO }),
        container(server).padding(Padding { top: 14.0, ..Padding::ZERO }),
    ]).padding(Padding { top: 34.0, ..Padding::ZERO }));
    let side = container(scroll_area(container(card.max_width(345)).center_x(FILL).padding([45, 0]))).width(Length::FillPortion(100)).height(FILL).center_y(FILL).padding([0, 45]).style(t::fill(t::SIDEBAR));
    row![story, t::rule(rgb(0x36313e), false), side].into()
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
        .push(body(note, 15.0, rgb(0x99919f), 1.8));
    if reason == UnlockReason::Missing {
        card = card.push(container(container(column![
            text("To keep your history").size(15).font(t::semibold()).color(rgb(0xd3c4ea)),
            body("Open Sonar on that browser, then go to Account settings → Privacy & device → Turn on multi-device access. Then sign in here again.", 15.0, rgb(0x9d98a8), 1.6),
        ].spacing(6)).padding([13, 14]).width(FILL).style(t::gradient_panel(120.0, rgb(0x25232b), rgb(0x202126), rgb(0x34303d), 8.0))).padding(Padding { top: 22.0, ..Padding::ZERO }));
    }
    let input = text_input(if recovery { "XXXX-XXXX-XXXX-XXXX-XXXX-XXXX" } else { "" }, &app.unlock_password).on_input(Msg::UnlockPassword).on_submit(Msg::Unlock).secure(!recovery).padding(12).size(16).style(t::auth_input());
    let mut form = Column::new().spacing(20).push(column![text(if recovery { "Recovery key" } else { "Password" }).size(14).color(rgb(0xbcb3c6)), input].spacing(9));
    if resetting && app.unlock_confirm {
        form = form.push(container(body("A new key replaces the old one. Messages encrypted for the old key stay unreadable here, and your contacts will be asked to confirm your new safety code.", 15.0, rgb(0xe3b8a8), 1.6))
            .padding([10, 12]).width(FILL).style(t::panel(rgb(0x35242a), rgb(0x4a2f37), 8.0)));
    }
    let label = if app.busy { "Working on it..." } else if !resetting { "Unlock messages" } else if app.unlock_confirm { "Yes, create a new key" } else { "Reset my encryption key" };
    let icon = if !resetting { Icon::KeyRound } else if app.unlock_confirm { Icon::RotateCcw } else { Icon::ArrowUpRight };
    form = form.push(primary_wide(label, icon, (!app.busy && !app.unlock_password.is_empty()).then_some(Msg::Unlock)));
    card = card.push(container(form).padding(Padding { top: 30.0, bottom: 14.0, ..Padding::ZERO }));
    if reason != UnlockReason::Missing {
        let toggle = if resetting { "Back to unlocking" } else if recovery { "Lost your recovery key?" } else { "Forgot your password?" };
        card = card.push(container(button(text(toggle).size(14)).padding([6, 0]).style(t::text_button()).on_press_maybe((!app.busy).then_some(Msg::ToggleUnlockReset))).padding(Padding { bottom: 12.0, ..Padding::ZERO }));
    }
    card = card.push(button(container(lead_icon(Icon::LogOut, "Sign out", rgb(0xcdb3e1), 17.0, 15.0, 12.0)).center_x(FILL)).width(FILL).padding([12, 16]).style(t::secondary()).on_press_maybe((!app.busy).then_some(Msg::SignOut)))
        .push(container(column![
            t::rule(rgb(0x35313c), true),
            row![t::icon(Icon::LockKeyhole, 15.0, rgb(0x82758e)), body("Your message key is locked with your recovery key before it is stored. If you lose the recovery key and every signed-in device, encrypted history cannot be recovered.", 13.0, rgb(0x82758e), 1.8)].spacing(9).padding(Padding { top: 22.0, ..Padding::ZERO }),
        ]).padding(Padding { top: 34.0, ..Padding::ZERO }));
    container(scroll_area(container(card.max_width(345)).center_x(FILL).padding([45, 0]))).width(FILL).height(FILL).center_y(FILL).style(t::fill(t::SIDEBAR)).into()
}

// RecoveryKeyPage: shown once whenever a recovery key is created.
fn recovery_view<'a>(app: &'a App, key: &'a str) -> E<'a> {
    let saved = app.recovery_saved;
    let check = iced::widget::checkbox(saved).label("I saved my recovery key somewhere safe").on_toggle(Msg::RecoverySaved).size(14).text_size(14).spacing(9)
        .style(move |_, _| iced::widget::checkbox::Style { background: (if saved { rgb(0xb69bdc) } else { rgb(0x1d1b22) }).into(), icon_color: rgb(0x21182e), border: t::border(if saved { rgb(0xb69bdc) } else { rgb(0x4b3d55) }, 1.0, 3.0), text_color: Some(rgb(0xb8a4c7)) });
    #[cfg(windows)]
    let save_file: E = button(container(lead_icon(Icon::Copy, "Save as file", rgb(0xc1b0d0), 15.0, 15.0, 10.0)).center_x(FILL)).width(FILL).padding([12, 16]).style(t::secondary()).on_press(Msg::SaveRecoveryFile).into();
    #[cfg(not(windows))]
    let save_file: E = space().into();
    let card = column![
        t::icon(Icon::KeyRound, 30.0, t::ACCENT),
        container(text("Save your recovery key.").size(28).font(t::bold()).color(t::TEXT)).padding(Padding { top: 22.0, bottom: 10.0, ..Padding::ZERO }),
        body("It unlocks your messages on a new device and resets your password if you forget it. Only you have it: Sonar cannot show it again or recover it for you.", 15.0, rgb(0x99919f), 1.8),
        container(container(text(key).size(16).font(t::REGULAR).color(rgb(0xefe4fb))).padding([18, 14]).width(FILL).center_x(FILL)
            .style(t::panel(rgb(0x1b191f), rgb(0x4d3e58), 10.0))).padding(Padding { top: 26.0, bottom: 14.0, ..Padding::ZERO }),
        row![
            button(container(lead_icon(Icon::Copy, "Copy", rgb(0xc1b0d0), 15.0, 15.0, 10.0)).center_x(FILL)).width(FILL).padding([12, 16]).style(t::secondary()).on_press(Msg::CopyRecovery),
            save_file,
        ].spacing(10),
        container(check).padding([18, 0]),
        primary("Continue to Sonar", Some(Icon::ArrowUpRight), saved.then_some(Msg::DismissRecovery)),
        container(column![
            t::rule(rgb(0x35313c), true),
            row![t::icon(Icon::LockKeyhole, 15.0, rgb(0x82758e)), body("Store it in a password manager or on paper. Anyone with this key and your email address can reset your password.", 13.0, rgb(0x82758e), 1.8)].spacing(9).padding(Padding { top: 22.0, ..Padding::ZERO }),
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
        container(space()).width(3).height(23).style(|_| container::Style { background: Some(rgb(0xc6a8ff).into()), border: iced::Border { radius: iced::border::Radius { top_left: 0.0, bottom_left: 0.0, top_right: 3.0, bottom_right: 3.0 }, ..Default::default() }, ..Default::default() }).into()
    } else { space().width(3).into() };
    row![marker, grow(), content, grow(), h_space(3.0)].width(76).align_y(Vertical::Center).into()
}
fn rail_button<'a>(icon: Icon, active: bool, unread: bool, on: Msg) -> E<'a> {
    let color = if active { rgb(0xc9b0ff) } else { rgb(0x7b7c88) };
    // The web places the unread dot under the icon (grid button).
    let mut inner = Column::new().align_x(Horizontal::Center).spacing(3).push(t::icon(icon, 22.0, color));
    if unread { inner = inner.push(t::dot(t::UNREAD, 8.0)); }
    rail_item(button(container(inner).center_x(46).center_y(44)).padding(0).width(46).height(44).style(t::rail_button(active)).on_press(on).into(), active)
}

fn rail(app: &App) -> E<'_> {
    let name = app.session.as_ref().map(|s| s.account.full_name.as_str()).unwrap_or_default();
    let mut top = Column::new().spacing(14).align_x(Horizontal::Center).width(76)
        .push(rail_item(button(container(t::sonar_mark(0.82)).center_x(44).center_y(42)).padding(0).width(44).height(42).style(t::brand()).on_press(if app.mode == Mode::Direct { Msg::OpenPeer(None) } else { Msg::SetMode(Mode::Direct) }).into(), false))
        .push(container(t::rule(rgb(0x2d2c33), true)).width(27).padding([6, 0]));
    match app.mode {
        Mode::Direct => {
            top = top.push(rail_button(Icon::MessageCircle, true, false, Msg::OpenPeer(None)))
                .push(rail_button(Icon::Users, false, !app.unread_channels.is_empty(), Msg::SetMode(Mode::Community)))
                .push(rail_button(Icon::ShieldCheck, false, false, Msg::OpenPanel(Some(Panel::Privacy))));
        }
        Mode::Community => {
            top = top.push(rail_button(Icon::MessageCircle, false, !app.unread_direct.is_empty(), Msg::SetMode(Mode::Direct)));
            let list = app.communities.iter().fold(Column::new().spacing(12).align_x(Horizontal::Center).padding([4, 0]), |list, item| {
                let active = app.community.as_ref().is_some_and(|c| c.id == item.id);
                let unread = app.unread_channels.iter().any(|key| key.starts_with(&format!("{}:", item.id)));
                let mut face = Row::new().align_y(Vertical::Center).push(text(t::initials(&item.name)).size(16).font(t::semibold()));
                if unread { face = face.push(h_space(3.0)).push(t::dot(t::UNREAD, 8.0)); }
                list.push(rail_item(button(container(face).center_x(43).center_y(43)).padding(0).width(43).height(43).style(t::community_icon(active)).on_press(Msg::OpenCommunity(Some(item.id.clone()))).into(), false))
            });
            top = top.push(scrollable(list).height(Length::Shrink).direction(scrollable::Direction::Vertical(scrollable::Scrollbar::new().width(0).scroller_width(0))))
                .push(rail_item(button(container(t::icon(Icon::Plus, 22.0, rgb(0xb4c9b7))).center_x(46).center_y(44)).padding(0).width(46).height(44).style(t::add_community()).on_press(Msg::OpenDialog(Some(Dialog::Start))).into(), false));
        }
    }
    let bottom = column![
        container(text(t::tracked("FREE")).size(11).font(t::bold()).color(rgb(0xb3a4ca))).padding([3, 6]).style(t::panel(Color::TRANSPARENT, rgb(0x3d384b), 4.0)),
        button(container(text(t::initials(name)).size(15).font(t::bold()).color(rgb(0xf8e2c9))).center_x(35).center_y(35)).padding(0).width(35).height(35).style(t::self_avatar()).on_press(Msg::OpenSettings(true)),
    ].spacing(21).align_x(Horizontal::Center);
    container(column![top, space().height(FILL), bottom].align_x(Horizontal::Center))
        .width(76).height(FILL).padding(Padding { top: 25.0, bottom: 15.0, left: 0.0, right: 0.0 })
        .style(|_| container::Style { background: Some(t::RAIL.into()), ..Default::default() })
        .into()
}

fn section<'a>(label: &str, extra: Option<E<'a>>) -> E<'a> {
    let mut line = Row::new().align_y(Vertical::Center).height(20).push(text(t::tracked(label)).size(12).font(t::bold()).color(rgb(0x797987))).push(grow());
    if let Some(extra) = extra { line = line.push(extra); }
    container(line).padding(Padding { top: 34.0, bottom: 9.0, left: 10.0, right: 10.0 }).into()
}
fn nav_row<'a>(icon: Option<Icon>, label: impl text::IntoFragment<'a>, trailing: Option<E<'a>>, selected: bool, on: Option<Msg>) -> E<'a> {
    let color = if selected { rgb(0xd3bdf6) } else { rgb(0x9999a6) };
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
fn person_row<'a>(name: &str, sub: &str, index: usize, online: bool, chosen: bool, trailing: Vec<E<'a>>, on: Option<Msg>) -> E<'a> {
    let mut content = Row::new().spacing(11).align_y(Vertical::Center)
        .push(avatar(name, index, online.then_some(t::SIDEBAR), 35.0))
        .push(column![text(name.to_owned()).size(15).font(t::semibold()).color(t::TEXT).wrapping(text::Wrapping::None), text(sub.to_owned()).size(13).color(rgb(0x7d7c8a))].spacing(4));
    // Like the web row: an unread dot follows the name; the lock sits at the end.
    let mut trailing = trailing;
    let last = trailing.pop();
    for item in trailing { content = content.push(item); }
    content = content.push(grow());
    if let Some(item) = last { content = content.push(item); }
    let inner = button(content).width(FILL).padding([11, 10]).style(t::person_row(chosen)).on_press_maybe(on);
    if chosen {
        stack![inner, container(space()).width(2).height(57).style(|_| container::Style { background: Some(rgb(0xb39ae4).into()), border: t::border(Color::TRANSPARENT, 0.0, 1.0), ..Default::default() })].into()
    } else { inner.into() }
}
fn sidebar_frame<'a>(title: E<'a>, header_action: Option<E<'a>>, content: E<'a>, mini: (Icon, &'a str, &'a str), app: &'a App) -> E<'a> {
    let name = app.session.as_ref().map(|s| s.account.full_name.as_str()).unwrap_or_default();
    let mut header = Row::new().align_y(Vertical::Center).push(title).push(grow());
    if let Some(action) = header_action { header = header.push(action); }
    let header = column![container(header).height(82).padding([0, 25]).center_y(82), t::rule(t::LINE, true)];
    let mini = container(container(row![t::icon(mini.0, 19.0, rgb(0xb7a2d0)), column![text(mini.1).size(14).color(rgb(0xb7aec5)), text(mini.2).size(12).color(rgb(0x84808f))].spacing(5)].spacing(10).align_y(Vertical::Center))
        .padding([13, 12]).width(FILL).style(t::gradient_panel(120.0, rgb(0x25232b), rgb(0x202126), rgb(0x34303d), 8.0))).padding(Padding { top: 10.0, bottom: 21.0, left: 18.0, right: 18.0 });
    let footer = column![t::rule(t::LINE, true), container(row![
        avatar(name, 0, Some(t::FOOTER_BG), 35.0),
        column![text(name.to_owned()).size(14).font(t::semibold()).color(t::TEXT).wrapping(text::Wrapping::None), text("Personal space").size(12).color(rgb(0x81818e))].spacing(4).width(FILL),
        icon_btn(Icon::LogOut, 17.0, Some(Msg::SignOut), false),
    ].spacing(10).align_y(Vertical::Center)).padding([15, 18]).style(t::fill(t::FOOTER_BG))];
    row![
        container(column![header, scroll_area(container(content).padding([27, 15])).height(FILL), mini, footer]).width(299).height(FILL).style(t::fill(t::SIDEBAR)),
        t::rule(t::LINE_SIDEBAR, false),
    ].into()
}

fn sidebar(app: &App) -> E<'_> {
    match app.mode {
        Mode::Direct => {
            let title = row![text("sonar").size(28).font(t::heavy()).color(t::TEXT), container(t::dot(rgb(0xb598eb), 5.0)).padding(Padding { top: 16.0, ..Padding::ZERO })].spacing(4).align_y(Vertical::Center);
            let badge = container(text(app.people.len().to_string()).size(13).color(rgb(0xdfc9ff))).padding([1, 6]).style(t::panel(rgb(0x514164), rgb(0x514164), 4.0));
            let query = app.query.to_lowercase();
            let mut list = Column::new().spacing(5);
            if !app.people_loaded { list = list.push(text("Loading people...").size(15).color(t::MUTED)); }
            for (index, person) in app.people.iter().filter(|p| p.name.to_lowercase().contains(&query)).enumerate() {
                let online = app.online.contains(&person.id);
                let mut trailing: Vec<E> = Vec::new();
                if app.unread_direct.contains(&person.id) { trailing.push(t::dot(t::UNREAD, 8.0)); }
                if person.encryption_key.is_some() { trailing.push(t::icon(Icon::LockKeyhole, 12.0, rgb(0xa395b5))); }
                list = list.push(person_row(&person.name, if online { "Around now" } else { "Offline" }, index, online, app.peer.as_deref() == Some(&person.id), trailing, Some(Msg::OpenPeer(Some(person.id.clone())))));
            }
            if app.people_loaded && app.people.is_empty() {
                list = list.push(container(body("Your space starts with a hello. Invite someone to create a Sonar account, then find them here.", 15.0, rgb(0x858390), 1.8)).padding(12));
            }
            let search = container(row![
                t::icon(Icon::Search, 15.0, rgb(0x676977)),
                text_input("Find a conversation", &app.query).on_input(Msg::Query).size(14).padding(0).style(t::bare_input()),
                text("/").size(16).color(rgb(0x72727d)),
            ].spacing(8).align_y(Vertical::Center)).padding([9, 10]).style(t::panel(rgb(0x15161a), rgb(0x2a2b32), 6.0));
            let content = column![
                container(text(t::tracked("YOUR SPACE")).size(12).font(t::bold()).color(rgb(0x70717f))).padding(Padding { left: 11.0, bottom: 13.0, ..Padding::ZERO }),
                nav_row(Some(Icon::MessageCircle), "Direct messages", Some(badge.into()), true, Some(Msg::OpenPanel(None))),
                nav_row(Some(Icon::Users), "People", Some(t::icon(Icon::ArrowUpRight, 15.0, rgb(0x9999a6))), false, Some(Msg::OpenPanel(Some(Panel::People)))),
                section("CONVERSATIONS", Some(icon_btn(Icon::Search, 15.0, Some(Msg::OpenPanel(Some(Panel::People))), false))),
                container(search).padding(Padding { left: 5.0, right: 5.0, bottom: 13.0, ..Padding::ZERO }),
                list,
            ];
            sidebar_frame(title.into(), Some(icon_btn(Icon::ChevronDown, 18.0, Some(Msg::OpenPanel(Some(Panel::Privacy))), false)), content.into(), (Icon::ShieldCheck, "A little more private.", "Messages disappear after 7 days."), app)
        }
        Mode::Community => {
            let title_text = app.community.as_ref().map(|c| c.name.clone()).unwrap_or_else(|| "Communities".into());
            let title = container(text(title_text).size(if app.community.is_some() { 17 } else { 28 }).font(t::heavy()).color(t::TEXT).wrapping(text::Wrapping::None)).max_width(185);
            let mut content = Column::new().push(nav_row(Some(Icon::ArrowLeft), "Back to messages", None, false, Some(Msg::SetMode(Mode::Direct))));
            let header_action = app.community.as_ref().map(|_| icon_btn(Icon::Users, 18.0, Some(Msg::ToggleMembers), false));
            match &app.community {
                Some(community) => {
                    let owner = app.owner();
                    let count = community.members.len();
                    content = content.push(container(column![
                        tile(&community.name, rgb(0x34303f), rgb(0xc6b5de), Some(rgb(0x494050)), 58.0, 19.0, 22.0, None),
                        text(community.name.clone()).size(17).font(t::semibold()).color(t::TEXT),
                        text(format!("{count} {} · Invite only", if count == 1 { "member" } else { "members" })).size(13).color(rgb(0x928599)),
                    ].spacing(10).align_x(Horizontal::Center)).width(FILL).center_x(FILL).padding(Padding { top: 25.0, bottom: 22.0, ..Padding::ZERO }));
                    if owner {
                        content = content.push(button(row![t::icon(Icon::Users, 15.0, rgb(0xcbb2ea)), text("Invite your people").size(14).color(rgb(0xcbb2ea)), grow(), t::icon(Icon::Plus, 14.0, rgb(0xcbb2ea))].spacing(8).align_y(Vertical::Center))
                            .width(FILL).padding(11).style(t::community_invite()).on_press(Msg::OpenDialog(Some(Dialog::Invite))))
                            .push(nav_row(None, "Rename community", None, false, Some(Msg::OpenDialog(Some(Dialog::RenameCommunity)))))
                            .push(nav_row(None, "Rename a channel or room", None, false, Some(Msg::OpenDialog(Some(Dialog::RenameChannel)))))
                            .push(danger_row(None, "Delete a channel or room", Some(Msg::OpenDialog(Some(Dialog::DeleteChannel)))))
                            .push(danger_row(None, "Delete community", Some(Msg::AskConfirm(Some(Confirm { action: "delete-community".into(), member: None, channel: None })))));
                    }
                    content = content.push(section("TEXT CHANNELS", owner.then(|| icon_btn(Icon::Plus, 16.0, Some(Msg::NewChannel(false)), false))));
                    for channel in community.channels.iter().filter(|c| c.kind != "voice") {
                        let selected = app.channel.as_deref() == Some(&channel.id);
                        let mut trailing = Row::new().spacing(8).align_y(Vertical::Center);
                        if app.unread_channels.contains(&format!("{}:{}", community.id, channel.id)) { trailing = trailing.push(t::dot(t::UNREAD, 8.0)); }
                        if selected { trailing = trailing.push(t::icon(Icon::LockKeyhole, 11.0, rgb(0xd3bdf6))); }
                        content = content.push(nav_row(Some(Icon::Hash), channel.name.clone(), Some(trailing.into()), selected, Some(Msg::OpenChannel(channel.id.clone()))));
                    }
                    content = content.push(voice_rooms(app, community));
                    content = content.push(container(row![t::icon(Icon::ShieldCheck, 14.0, rgb(0x86758f)), body("Encrypted for each member.\nUp to 32 people in this early version.", 12.0, rgb(0x86758f), 1.8)].spacing(8)).padding([24, 10]))
                        .push(danger_row(Some(Icon::LogOut), "Leave community", (!owner).then(|| Msg::AskConfirm(Some(Confirm { action: "leave".into(), member: None, channel: None })))));
                    if owner { content = content.push(container(body("To leave, transfer ownership from the member list first.", 12.0, rgb(0x8f7b99), 1.7)).padding([0, 12])); }
                }
                None => {
                    content = content.push(section("YOUR COMMUNITIES", None));
                    for (index, item) in app.communities.iter().enumerate() {
                        content = content.push(person_row(&item.name, &format!("{} members", item.members.len()), index, false, false, vec![t::icon(Icon::ArrowUpRight, 15.0, rgb(0x9999a6))], Some(Msg::OpenCommunity(Some(item.id.clone())))));
                    }
                    content = content.push(nav_row(Some(Icon::Plus), "Create a community", None, false, Some(Msg::OpenDialog(Some(Dialog::Create)))))
                        .push(nav_row(Some(Icon::Users), "Join with an invite", None, false, Some(Msg::OpenDialog(Some(Dialog::Join)))));
                }
            }
            sidebar_frame(title.into(), header_action, content.into(), (Icon::Timer, "Keep what matters.", "7-day history. Pins stay."), app)
        }
    }
}

// ----- Conversation ------------------------------------------------------------

fn encrypted_badge<'a>() -> E<'a> {
    container(lead_icon(Icon::LockKeyhole, "Encrypted", rgb(0xa1b9a9), 12.0, 12.0, 6.0)).padding([5, 8]).style(t::panel(rgb(0x25312c), rgb(0x344139), 5.0)).into()
}

fn conversation(app: &App) -> E<'_> {
    let me = app.me();
    let (identity, actions): (E, Vec<E>) = match app.mode {
        Mode::Direct => match app.selected_peer() {
            Some(peer) => {
                let online = app.online.contains(&peer.id);
                (row![
                    avatar(&peer.name, 0, online.then_some(t::CONVERSATION), 35.0),
                    column![
                        text(peer.name.clone()).size(16).font(t::semibold()).color(t::TEXT),
                        row![t::dot(if online { t::GREEN } else { t::OFFLINE }, 5.0), text(if online { "Online" } else { "Offline" }).size(13).color(rgb(0x82828f)), text("/").size(13).color(rgb(0x484850)), text("Direct message").size(13).color(rgb(0x82828f))].spacing(5).align_y(Vertical::Center),
                    ].spacing(7),
                ].spacing(12).align_y(Vertical::Center).into(),
                vec![
                    container(encrypted_badge()).padding(Padding { right: 8.0, ..Padding::ZERO }).into(),
                    icon_btn(Icon::Phone, 19.0, (online && app.call.is_none() && app.incoming.is_none() && app.live_connected).then_some(Msg::StartCall), false),
                    icon_btn(Icon::Pin, 19.0, Some(Msg::TogglePins), app.thread.pins),
                    icon_btn(Icon::Ellipsis, 21.0, Some(Msg::OpenPanel(Some(Panel::Privacy))), false),
                ])
            }
            None => (row![t::icon(Icon::MessageCircle, 22.0, rgb(0xb5a5cd)), text("Direct messages").size(16).font(t::semibold()).color(t::TEXT)].spacing(12).align_y(Vertical::Center).into(),
                vec![container(encrypted_badge()).padding(Padding { right: 8.0, ..Padding::ZERO }).into(), icon_btn(Icon::Ellipsis, 21.0, Some(Msg::OpenPanel(Some(Panel::Privacy))), false)]),
        },
        Mode::Community => {
            let channel = app.channel_info();
            let mut actions: Vec<E> = vec![container(encrypted_badge()).padding(Padding { right: 8.0, ..Padding::ZERO }).into()];
            if channel.is_some() { actions.push(icon_btn(Icon::Pin, 18.0, Some(Msg::TogglePins), app.thread.pins)); }
            if app.community.is_some() { actions.push(icon_btn(Icon::Users, 19.0, Some(Msg::ToggleMembers), app.members_open)); }
            (row![t::icon(Icon::Hash, 22.0, rgb(0xb5a5cd)), column![
                text(channel.map(|c| c.name.clone()).unwrap_or_else(|| "Your communities".into())).size(16).font(t::semibold()).color(t::TEXT),
                text(app.community.as_ref().map(|c| c.name.clone()).unwrap_or_else(|| "A place for your people".into())).size(13).color(rgb(0x82828f)),
            ].spacing(7)].spacing(12).align_y(Vertical::Center).into(), actions)
        }
    };
    let top = column![
        container(row![identity, grow(), Row::from_vec(actions).spacing(12).align_y(Vertical::Center)].align_y(Vertical::Center)).height(82).padding([0, 33]).center_y(82),
        t::rule(t::LINE_CHAT, true),
    ];
    let retention_text = if app.mode == Mode::Direct { "A conversation, not a permanent record. Messages disappear after 7 days." } else { "Messages disappear after 7 days. Pin the ones worth keeping." };
    let mut banner = Row::new().spacing(8).align_y(Vertical::Center).push(t::icon(Icon::Timer, 14.0, rgb(0xb59bdc))).push(text(retention_text).size(13).color(rgb(0x8f8a9b)));
    if app.mode == Mode::Direct {
        banner = banner.push(h_space(6.0)).push(button(row![text("How it works").size(13).color(rgb(0xb7a3d2)), t::icon(Icon::ArrowUpRight, 12.0, rgb(0xb7a3d2))].spacing(5).align_y(Vertical::Center)).padding(0).style(t::plain(rgb(0xb7a3d2), rgb(0xd3c0ec))).on_press(Msg::OpenPanel(Some(Panel::Privacy))));
    }
    let retention = column![container(banner).width(FILL).center_x(FILL).padding([12, 20]).style(t::fill(rgb(0x211f29))), t::rule(rgb(0x2c2934), true)];

    let mut main = Column::new().push(top).push(retention);
    if let Some(banner) = call_banner(app) { main = main.push(banner); }
    let thread: E = match app.mode {
        Mode::Direct => direct_thread(app, me),
        Mode::Community => community_thread(app, me),
    };
    let footer = column![t::rule(rgb(0x2b2a31), true), container(row![
        row![t::dot(t::GREEN, 5.0), text(if app.mode == Mode::Direct { "Your space. Your pace." } else { "Your space. Your people." }).size(12).color(rgb(0x76717f))].spacing(7).align_y(Vertical::Center),
        grow(),
        text(if app.live_connected { "Always free. Always ad-free." } else { "Reconnecting..." }).size(12).color(rgb(0x76717f)),
    ].align_y(Vertical::Center)).padding(Padding { top: 12.0, bottom: 15.0, left: 33.0, right: 33.0 })];
    let body_row: E = if app.mode == Mode::Community && app.members_open && app.community.is_some() {
        row![container(thread).width(FILL).height(FILL), t::rule(t::LINE_CHAT, false), members_panel(app)].height(FILL).into()
    } else { container(thread).width(FILL).height(FILL).into() };
    main = main.push(body_row).push(footer);
    container(main).width(FILL).height(FILL).style(t::fill(t::CONVERSATION)).into()
}

// VoiceRoomList: rooms, occupancy, lock controls and participants.
fn voice_rooms<'a>(app: &'a App, community: &'a crate::api::Community) -> E<'a> {
    let (owner, moderator, me) = (app.owner(), app.moderator(), app.me());
    let add = owner.then(|| icon_btn(Icon::Plus, 16.0, Some(Msg::NewChannel(true)), false));
    let mut list = Column::new().push(container(section("VOICE ROOMS", add)).padding(Padding { top: 0.0, ..Padding::ZERO }));
    let rooms: Vec<_> = community.channels.iter().filter(|c| c.kind == "voice").collect();
    if rooms.is_empty() {
        list = list.push(container(body(if owner { "Make room for a real conversation. Create your first voice room." } else { "No voice rooms yet. The owner can create one." }, 13.0, rgb(0x887694), 1.8)).padding(Padding { left: 10.0, right: 10.0, bottom: 9.0, ..Padding::ZERO }));
    }
    for channel in rooms {
        let active = app.room.as_ref().is_some_and(|r| r.community == community.id && r.channel == channel.id);
        let participants = app.rooms.get(&format!("{}:{}", community.id, channel.id)).cloned().unwrap_or_default();
        let color = if active { rgb(0xbed3bd) } else { rgb(0x9999a6) };
        let mut row_content = Row::new().spacing(11).align_y(Vertical::Center).push(t::icon(Icon::Volume, 17.0, color)).push(text(channel.name.clone()).size(15).color(color));
        if channel.voice_locked { row_content = row_content.push(t::icon(Icon::Lock, 13.0, color)); }
        row_content = row_content.push(grow()).push(text(format!("{}/6", participants.len())).size(12).color(rgb(0x8e7d9e)));
        let style: Box<dyn Fn(&iced::Theme, button::Status) -> button::Style> = if active {
            Box::new(|_, _| button::Style { background: Some(rgb(0x28332c).into()), text_color: rgb(0xbed3bd), border: t::border(Color::TRANSPARENT, 0.0, 7.0), ..Default::default() })
        } else { t::nav_row(false) };
        list = list.push(container(button(row_content).width(FILL).padding(12).style(style).on_press_maybe((!active && !channel.voice_locked).then(|| Msg::JoinRoom(channel.id.clone())))).padding(Padding { bottom: 1.0, top: 3.0, ..Padding::ZERO }));
        if owner || moderator {
            let (icon, label) = if channel.voice_locked { (Icon::LockOpen, "Unlock room") } else { (Icon::Lock, "Lock room") };
            list = list.push(button(lead_icon(icon, label, rgb(0xb7a3c9), 13.0, 13.0, 5.0)).padding([4, 10]).style(t::plain(rgb(0xb7a3c9), rgb(0xdcc8ec))).on_press(Msg::RoomLock(channel.id.clone(), !channel.voice_locked)));
        }
        let mut people = Column::new().padding(Padding { top: 3.0, bottom: 8.0, left: 29.0, right: 8.0 });
        for person in &participants {
            let mut line = Row::new().spacing(7).align_y(Vertical::Center).push(t::dot(t::GREEN, 5.0))
                .push(text(format!("{}{}", person.name, if person.user_id == me { " (you)" } else { "" })).size(13).color(rgb(0xa995b9)).width(FILL));
            let can_kick = (owner || moderator) && person.user_id != me && Some(&person.user_id) != community.owner_id.as_ref() && (owner || !community.moderators.contains(&person.user_id));
            if can_kick {
                line = line.push(button(t::icon(Icon::LogOut, 12.0, rgb(0xbc9ead))).padding(3).style(t::plain(rgb(0xbc9ead), rgb(0xe0c0cc))).on_press(Msg::RoomKick(channel.id.clone(), person.clone())));
            }
            if person.muted || person.deafened { line = line.push(t::icon(Icon::MicOff, 11.0, rgb(0xb390c6))); }
            people = people.push(container(line).padding([7, 2]));
        }
        if !participants.is_empty() { list = list.push(people); }
    }
    list = list.push(container(body("Joining shares your audio with everyone in the room. Up to 6 people. No recordings. Locking stops new joins; it does not disconnect people already in the room.", 11.0, rgb(0x796a84), 1.8)).padding([15, 10]));
    list.into()
}

fn call_banner(app: &App) -> Option<E<'_>> {
    let name = |id: &str| app.person(id).map(|p| p.name.clone()).or_else(|| app.member(id).map(|p| p.name.clone())).unwrap_or_else(|| "Someone".into());
    let style = t::panel(rgb(0x2a332b), rgb(0x495747), 9.0);
    // Mirrors VoiceCallBanner.jsx: same titles, subtitle and controls.
    let pill = |relay: bool| -> E { container(lead_icon(Icon::LockKeyhole, if relay { "Relay only" } else { "Encrypted audio" }, rgb(0xa2baa0), 11.0, 11.0, 5.0)).padding([6, 7]).style(t::panel(rgb(0x303e30), rgb(0x485946), 5.0)).into() };
    let end_call = |on: Msg| -> E { button(t::icon(Icon::PhoneOff, 18.0, Color::WHITE)).padding(10).style(t::end_call()).on_press(on).into() };
    let content: E = if let Some(incoming) = &app.incoming {
        row![
            t::icon(Icon::AudioLines, 23.0, rgb(0xbfd1bb)),
            column![text("Incoming voice call").size(15).font(t::semibold()).color(rgb(0xbfd1bb)), text(format!("{} · No recordings", name(&incoming.from))).size(13).color(rgb(0x9aaa96))].spacing(4).width(FILL),
            pill(app.settings.relay_only),
            button(text("Accept").size(15).font(t::semibold()).color(rgb(0x251c31))).padding([8, 14]).style(t::primary()).on_press_maybe((app.call.is_none() && app.room.is_none()).then_some(Msg::AcceptCall)),
            end_call(Msg::DeclineCall),
        ].spacing(14).align_y(Vertical::Center).into()
    } else if let Some(room) = &app.room {
        let joining = room.spec.is_none() || room.controls.is_none();
        return Some(container(container(row![
            t::icon(Icon::Volume, 23.0, rgb(0xbfd1bb)),
            column![
                text(if joining { "Joining voice room...".to_string() } else { format!("Voice room: {}", room.name) }).size(15).font(t::semibold()).color(rgb(0xbfd1bb)),
                text(format!("{} · {}/6 people · {} peer {}", room.community_name, room.participants.len(), room.connections, if room.connections == 1 { "connection" } else { "connections" })).size(13).color(rgb(0x9aaa96)),
            ].spacing(4).width(FILL),
            container(lead_icon(Icon::LockKeyhole, if app.settings.relay_only { "Relay only" } else { "Encrypted audio" }, rgb(0xa2baa0), 11.0, 11.0, 5.0)).padding([6, 7]).style(t::panel(rgb(0x303e30), rgb(0x485946), 5.0)),
            icon_btn(if room.muted { Icon::MicOff } else { Icon::Mic }, 18.0, Some(Msg::RoomMute), room.muted),
            icon_btn(if room.deafened { Icon::HeadphoneOff } else { Icon::Headphones }, 18.0, Some(Msg::RoomDeafen), room.deafened),
            button(t::icon(Icon::LogOut, 18.0, Color::WHITE)).padding(10).style(t::end_call()).on_press(Msg::LeaveRoom),
        ].spacing(14).align_y(Vertical::Center)).padding(14).width(FILL).style(t::panel(rgb(0x26322d), rgb(0x405a4b), 9.0)))
            .padding(Padding { top: 16.0, left: 33.0, right: 33.0, bottom: 0.0 }).into());
    } else if let Some(call) = &app.call {
        let title = match call.status { CallStatus::Calling => "Calling...", CallStatus::Connecting => "Connecting voice...", CallStatus::Connected => "Voice connected" };
        let duration = call.started.map(|s| { let secs = s.elapsed().as_secs(); format!(" · {}:{:02}", secs / 60, secs % 60) }).unwrap_or_default();
        row![
            t::icon(Icon::AudioLines, 23.0, rgb(0xbfd1bb)),
            column![text(title).size(15).font(t::semibold()).color(rgb(0xbfd1bb)), text(format!("{}{duration} · No recordings", name(&call.spec.peer))).size(13).color(rgb(0x9aaa96))].spacing(4).width(FILL),
            pill(call.spec.relay_only),
            icon_btn(if call.muted { Icon::MicOff } else { Icon::Mic }, 18.0, Some(Msg::Mute), call.muted),
            icon_btn(if call.deafened { Icon::HeadphoneOff } else { Icon::Headphones }, 18.0, Some(Msg::Deafen), call.deafened),
            end_call(Msg::Hangup),
        ].spacing(14).align_y(Vertical::Center).into()
    } else { return None };
    Some(container(container(content).padding(14).width(FILL).style(style)).padding(Padding { top: 16.0, left: 33.0, right: 33.0, bottom: 0.0 }).into())
}

fn welcome<'a>(symbol: E<'a>, eyebrow: &'a str, title: &'a str, lines: [&'a str; 2], buttons: Vec<E<'a>>, values: [(Icon, &'a str); 3]) -> E<'a> {
    let orbit = stack![t::orbit::<Msg>(), container(symbol).width(186).height(186).center_x(186).center_y(186)];
    let values = values.into_iter().fold(Row::new().spacing(28), |row, (icon, label)| row.push(lead_icon(icon, label, rgb(0x777080), 16.0, 12.0, 7.0)));
    center(column![
        container(orbit).padding(Padding { bottom: 0.0, ..Padding::ZERO }),
        container(t::eyebrow(eyebrow, 12.0, t::EYEBROW)).padding(Padding { top: 16.0, ..Padding::ZERO }),
        container(text(title).size(36).font(t::bold()).color(rgb(0xeee9f5))).padding([15, 0]),
        body(lines[0], 16.0, rgb(0x93909e), 1.9).align_x(Horizontal::Center),
        body(lines[1], 16.0, rgb(0x93909e), 1.9).align_x(Horizontal::Center),
        container(Row::from_vec(buttons).spacing(12)).padding(Padding { top: 27.0, ..Padding::ZERO }),
        container(values).padding(Padding { top: 65.0, ..Padding::ZERO }),
    ].align_x(Horizontal::Center)).padding([55, 10]).into()
}

fn date_divider<'a>(label: &str) -> E<'a> {
    container(row![t::rule(t::LINE_CHAT, true), text(t::tracked(label)).size(11).color(rgb(0x787482)), t::rule(t::LINE_CHAT, true)].spacing(15).align_y(Vertical::Center)).padding(Padding { top: 8.0, bottom: 24.0, ..Padding::ZERO }).into()
}
fn notice<'a>(value: &'a str) -> E<'a> { container(text(value).size(15).color(rgb(0x817887))).padding(24).into() }

fn message_list<'a>(app: &'a App, me: &'a str, community: bool) -> Vec<E<'a>> {
    let reactions = crate::chat::reactions(&app.thread.messages, app.now);
    let visible: Vec<&crate::chat::Message> = app.thread.messages.iter().filter(|m| m.reaction.is_none() && m.live(app.now) && (!app.thread.pins || m.pinned)).collect();
    let mut out = Vec::new();
    if app.thread.has_more {
        let label = if app.thread.loading_older { "Decrypting older messages..." } else if app.thread.pins { "Load older pinned messages" } else { "Load older messages" };
        out.push(container(button(text(label).size(13).color(rgb(0xbca0d3))).padding([10, 16]).style(t::load_older()).on_press_maybe((!app.thread.loading_older && !app.thread.loading).then_some(Msg::LoadOlder))).center_x(FILL).padding(Padding { bottom: 22.0, ..Padding::ZERO }).into());
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
        if message.sender == me { meta = meta.push(container(text(t::tracked("YOU")).size(10).color(rgb(0xb59ecb))).padding([1, 4]).style(t::panel(rgb(0x36303f), rgb(0x36303f), 3.0))); }
        meta = meta.push(text(if community { message.date_time() } else { message.time() }).size(12).color(rgb(0x777582)));
        if message.pinned { meta = meta.push(lead_icon(Icon::Pin, "Pinned", rgb(0xb9a0d8), 11.0, 12.0, 4.0)); }
        let mut content = Column::new().width(FILL).push(container(meta).padding(Padding { bottom: 7.0, ..Padding::ZERO }));
        if let Some(target) = &message.reply_to {
            let quoted = visible.iter().find(|m| &m.id == target && !m.unreadable).map(|m| m.text.clone()).unwrap_or_else(|| "Original message unavailable".into());
            content = content.push(container(row![container(space()).width(2).height(18).style(t::fill(rgb(0x9675bb))), text(format!("Replying to: {quoted}")).size(14).color(rgb(0xb8a7cb)).wrapping(text::Wrapping::None)].spacing(8).align_y(Vertical::Center)).padding(Padding { top: 3.0, bottom: 5.0, ..Padding::ZERO }));
        }
        content = content.push(body(message.text.as_str(), 16.0, if message.unreadable { rgb(0x8d8098) } else { rgb(0xb9b7c3) }, 1.8).wrapping(text::Wrapping::WordOrGlyph));
        if !app.thread.pins {
            let counts = reactions.get(&message.id);
            let open = app.picker.as_deref() == Some(&message.id);
            let mut bar = Row::new().spacing(5);
            for emoji in crate::chat::REACTION_EMOJI {
                let senders = counts.and_then(|list| list.iter().find(|(e, _)| e == emoji)).map(|(_, s)| s.as_slice()).unwrap_or_default();
                if !open && senders.is_empty() { continue; }
                let selected = senders.iter().any(|s| s == me);
                let mut label = Row::new().spacing(4).align_y(Vertical::Center).push(text(emoji).size(15));
                if !senders.is_empty() { label = label.push(text(senders.len().to_string()).size(13).color(rgb(0xe6d9f2))); }
                bar = bar.push(button(label).padding([3, 7]).style(t::reaction(selected)).on_press_maybe((!message.unreadable && !app.sending).then(|| Msg::React(message.id.clone(), emoji.to_string(), selected))));
            }
            bar = bar.push(button(t::icon(Icon::SmilePlus, 14.0, rgb(0xaaa0b4))).padding([3, 7]).style(t::reaction(false)).on_press_maybe((!message.unreadable).then(|| Msg::Picker(Some(message.id.clone())))));
            content = content.push(container(bar).padding(Padding { top: 5.0, ..Padding::ZERO }));
        }
        let row_view = container(row![avatar(&sender_name, 0, None, 35.0), content, h_space(if message.sender == me { 92.0 } else { 57.0 })].spacing(13)).padding([14, 11]).width(FILL)
            .style(if message.pinned { t::fill(rgba(0xb28ee805)) } else { t::fill(Color::TRANSPARENT) });
        let mut tools = Row::new().spacing(2);
        let tool = |icon: Icon, on: Option<Msg>| -> E { button(t::icon(icon, 14.0, rgb(0x9c8db0))).padding(4).style(t::plain(rgb(0x9c8db0), rgb(0xd9c4ee))).on_press_maybe(on).into() };
        tools = tools.push(tool(Icon::Reply, (!message.unreadable).then(|| Msg::Reply(Some(message.id.clone())))));
        if message.sender == me { tools = tools.push(tool(Icon::Trash, Some(Msg::AskDelete(Some(message.id.clone()))))); }
        tools = tools.push(tool(Icon::Pin, Some(Msg::Pin(message.id.clone()))));
        let overlay = container(tools).width(FILL).align_x(Horizontal::Right).padding([14, 11]);
        out.push(container(hover(container(row_view).style(|_| container::Style { border: t::border(Color::TRANSPARENT, 0.0, 6.0), ..Default::default() }), overlay)).padding(Padding { bottom: 4.0, left: -11.0, right: -11.0, top: 0.0 }).into());
    }
    out
}

fn composer<'a>(app: &'a App, placeholder: String, enabled: bool, hint_left: String) -> E<'a> {
    let mut area = Column::new();
    if let Some(reply) = &app.reply_to {
        let quoted = app.thread.messages.iter().find(|m| &m.id == reply).map(|m| m.text.clone()).unwrap_or_default();
        area = area.push(container(row![text(format!("Replying to: {quoted}")).size(14).color(rgb(0xb8a7cb)).wrapping(text::Wrapping::None).width(FILL), icon_btn(Icon::X, 16.0, Some(Msg::Reply(None)), false)].spacing(8).align_y(Vertical::Center))
            .padding([6, 10]).style(|_| container::Style { background: Some(rgb(0x30283e).into()), border: iced::Border { radius: iced::border::Radius { top_left: 8.0, top_right: 8.0, bottom_left: 0.0, bottom_right: 0.0 }, ..Default::default() }, ..Default::default() }));
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
    let send = button(container(t::icon(Icon::Send, 18.0, rgb(0x21182e))).center_x(32).center_y(32)).padding(0).width(32).height(32).style(t::send_button()).on_press_maybe(can_send.then_some(Msg::Send));
    let focused = false;
    area = area.push(container(row![t::icon(Icon::LockKeyhole, 18.0, rgb(0x82768f)), editor, send].spacing(12).align_y(Vertical::Center))
        .padding([13, 15]).style(move |_| container::Style {
            background: Some(rgb(0x28292f).into()), border: t::border(if focused { rgb(0x89739f) } else { rgb(0x414048) }, 1.0, 10.0), ..Default::default()
        }));
    area = area.push(container(row![
        lead_icon(Icon::LockKeyhole, hint_left, rgb(0x706b7b), 11.0, 11.0, 5.0), grow(),
        text("Enter to send · Shift + Enter for a new line").size(11).color(rgb(0x706b7b)),
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
        container(tile(&peer.name, rgb(0x40354e), rgb(0xdbc5f4), Some(rgb(0x594566)), 69.0, 23.0, 24.0, None)).padding(Padding { bottom: 18.0, ..Padding::ZERO }),
        container(text(if app.thread.pins { "The things worth keeping.".to_string() } else { format!("This is your space with {first}.") }).size(24).font(t::bold()).color(rgb(0xededf2))).padding(Padding { bottom: 10.0, ..Padding::ZERO }),
        body(if app.thread.pins { "Pinned messages stay until you unpin them." } else { "Say something. Make a plan. Stay a while." }, 15.0, rgb(0x878693), 1.7),
        container(lead_icon(Icon::LockKeyhole, if changed { "Contact key changed. Verify it in conversation details before sending." } else if peer.encryption_key.is_some() { "Encrypted on your device before sending" } else { "Waiting for this person to set up encryption" }, if changed { rgb(0xe3b8a8) } else { rgb(0xac9bbd) }, 12.0, 12.0, 6.0)).padding(Padding { top: 17.0, ..Padding::ZERO }),
    ];
    let mut list = Column::new().push(container(start).padding(Padding { top: 45.0, bottom: 27.0, ..Padding::ZERO })).push(date_divider(if app.thread.pins { "PINNED MESSAGES" } else { "YOUR CONVERSATION" }));
    for item in message_list(app, me, false) { list = list.push(item); }
    let scroll = scroll_area(container(list).padding(Padding { left: 33.0, right: 33.0, bottom: 20.0, top: 0.0 })).id("thread").anchor_bottom().height(FILL);
    let enabled = peer.encryption_key.is_some() && !changed;
    column![scroll, composer(app, format!("Message {first}..."), enabled, "Only your devices hold the message keys".into())].into()
}

fn community_thread<'a>(app: &'a App, me: &'a str) -> E<'a> {
    let Some(community) = &app.community else {
        if app.thread.loading { return center(notice("Opening your community...")).into(); }
        return scroll_area(welcome(t::icon(Icon::Users, 37.0, t::ACCENT), "GOOD COMPANY. YOUR OWN CORNER.", "Make room for your people.", ["A weekend crew. A study group. Your next big idea.", "Bring everyone together in a private community."],
            vec![
                button(row![t::icon(Icon::Plus, 17.0, rgb(0x251c31)), text("Create a community").size(15).font(t::semibold()).color(rgb(0x251c31))].spacing(22).align_y(Vertical::Center)).padding([13, 18]).style(t::primary()).on_press(Msg::OpenDialog(Some(Dialog::Create))).into(),
                secondary("Join with an invite", Some(Icon::ArrowUpRight), Some(Msg::OpenDialog(Some(Dialog::Join)))),
            ],
            [(Icon::LockKeyhole, "Encrypted channel messages"), (Icon::Timer, "7-day history"), (Icon::Users, "Invite-only spaces")])).height(FILL).into();
    };
    let channel = app.channel_info();
    let name = channel.map(|c| c.name.clone()).unwrap_or_else(|| "general".into());
    let start = column![
        container(container(t::icon(Icon::Hash, 35.0, rgb(0xbba2d4))).width(65).height(65).center_x(65).center_y(65).style(t::panel(rgb(0x352d41), rgb(0x4a3c5a), 22.0))).padding(Padding { bottom: 20.0, ..Padding::ZERO }),
        container(text(if app.thread.pins { "The things worth keeping.".to_string() } else { format!("Welcome to #{name}") }).size(24).font(t::bold()).color(rgb(0xededf2))).padding(Padding { bottom: 10.0, ..Padding::ZERO }),
        body(if app.thread.pins { "Pinned messages stay until someone unpins them." } else { "Your own corner of the conversation. Make yourself at home." }, 15.0, rgb(0x878693), 1.7),
        container(row![t::icon(Icon::LockKeyhole, 12.0, rgb(0xac9bbd)), body("Encrypted separately for each member. New members start with a clean history.", 12.0, rgb(0xac9bbd), 1.8)].spacing(6)).padding(Padding { top: 17.0, ..Padding::ZERO }),
    ];
    let mut list = Column::new().push(container(start).padding(Padding { top: 45.0, bottom: 27.0, ..Padding::ZERO })).push(date_divider(if app.thread.pins { "PINNED MESSAGES" } else { "CHANNEL CONVERSATION" }));
    for item in message_list(app, me, true) { list = list.push(item); }
    let scroll = scroll_area(container(list).padding(Padding { left: 33.0, right: 33.0, bottom: 15.0, top: 0.0 })).id("thread").anchor_bottom().height(FILL);
    if channel.is_none() { return scroll.into(); }
    let count = community.members.len();
    column![scroll, composer(app, format!("Message #{name}..."), true, format!("Encrypted for {count} {}", if count == 1 { "member" } else { "members" }))].into()
}

#[derive(Clone, PartialEq, Eq)]
struct Choice { label: &'static str, action: &'static str }
impl std::fmt::Display for Choice { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.label) } }

fn members_panel(app: &App) -> E<'_> {
    let Some(community) = &app.community else { return space().into() };
    let (owner, moderator, me) = (app.owner(), app.moderator(), app.me());
    let mut list = Column::new().push(container(row![text(t::tracked(&format!("MEMBERS — {}", community.members.len()))).size(11).font(t::bold()).color(rgb(0x797987)), grow(), icon_btn(Icon::X, 15.0, Some(Msg::ToggleMembers), false)].align_y(Vertical::Center)).padding(Padding { top: 10.0, bottom: 18.0, left: 4.0, right: 4.0 }));
    if owner { list = list.push(nav_row(None, "Manage bans", None, false, Some(Msg::OpenDialog(Some(Dialog::Bans))))); }
    for (index, member) in community.member_details.iter().enumerate() {
        let is_mod = community.moderators.contains(&member.id);
        let role = if Some(&member.id) == community.owner_id.as_ref() { "Owner" } else if is_mod { "Moderator" } else { "Member" };
        let manageable = (owner && member.id != me) || (moderator && member.id != me && Some(&member.id) != community.owner_id.as_ref() && !is_mod);
        let trailing: E = if manageable {
            let mut choices = vec![Choice { label: "Remove member", action: "remove" }, Choice { label: "Ban member", action: "ban" }];
            if owner {
                choices.push(if is_mod { Choice { label: "Remove moderator role", action: "demote" } } else { Choice { label: "Make moderator", action: "promote" } });
                choices.push(Choice { label: "Make owner", action: "transfer" });
            }
            let member = member.clone();
            pick_list(choices, None::<Choice>, move |choice| Msg::AskConfirm(Some(Confirm { action: choice.action.into(), member: Some(member.clone()), channel: None })))
                .placeholder("Manage").text_size(11).padding([5, 4]).width(84).style(t::select()).menu_style(t::menu()).into()
        } else { t::icon(Icon::LockKeyhole, 11.0, rgb(0xa395b5)) };
        list = list.push(container(row![
            avatar(&member.name, index, app.online.contains(&member.id).then_some(rgb(0x1b1c21)), 30.0),
            column![text(member.name.clone()).size(14).font(t::semibold()).color(t::TEXT).wrapping(text::Wrapping::None), text(format!("{role}{}", if member.id == me { " · You" } else { "" })).size(13).color(rgb(0x7d7c8a))].spacing(4).width(FILL),
            trailing,
        ].spacing(8).align_y(Vertical::Center)).padding([10, 4]));
    }
    container(scroll_area(list)).width(250).height(FILL).padding([12, 10]).style(t::fill(rgb(0x1b1c21))).into()
}

// ----- Overlays ---------------------------------------------------------------

fn privacy_item<'a>(icon: Icon, title: &'a str, note: &'a str) -> E<'a> {
    container(row![t::icon(icon, 19.0, rgb(0xb29cc9)), column![text(title).size(15).font(t::semibold()).color(rgb(0xddd3e7)), body(note, 14.0, rgb(0x9a909f), 1.8)].spacing(7)].spacing(14)).padding([12, 0]).into()
}

fn panel_view(app: &App, panel: Panel) -> E<'_> {
    let close = Msg::OpenPanel(None);
    let content: E = match panel {
        Panel::People => {
            let mut people = Column::new().spacing(6).padding(Padding { top: 20.0, ..Padding::ZERO });
            for (index, person) in app.people.iter().enumerate() {
                people = people.push(person_row(&person.name, if app.online.contains(&person.id) { "Around now" } else { "Offline" }, index, app.online.contains(&person.id), false, vec![t::icon(Icon::ArrowUpRight, 17.0, rgb(0x9999a6))], Some(Msg::OpenPeer(Some(person.id.clone())))));
            }
            if app.people.is_empty() { people = people.push(body("No other members yet. Share this app's address with a friend.", 15.0, rgb(0x9b91a4), 1.8)); }
            column![modal_header(Icon::Users, 28.0, close.clone()), modal_title("Your people, a little closer."),
                body("Choose someone to start an encrypted conversation. New members appear here after signing up.", 15.0, rgb(0x99919f), 1.8),
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
                    let mut card = Column::new().push(text(format!("{}’s safety code", peer.name)).size(13).font(t::semibold()).color(rgb(0xb7a5ca)))
                        .push(container(body(print, 14.0, rgb(0xb8b1c3), 1.8)).padding(Padding { top: 8.0, ..Padding::ZERO }));
                    if app.trust_changed(peer) {
                        let first = peer.name.split_whitespace().next().unwrap_or_default().to_owned();
                        card = card.push(container(body(format!("{first}’s key changed, for example because they reset it or set up a new device. Ask them to open Account settings → Privacy & device and read you their safety code in person or on a call. If it matches the code above, trust the new key. Older messages may become unreadable."), 14.0, rgb(0xb9aabc), 1.6)).padding(Padding { top: 10.0, ..Padding::ZERO }))
                            .push(container(button(text("The codes match, trust the new key").size(15).color(rgb(0xe9dcf5))).padding([8, 12]).style(t::fingerprint_button()).on_press(Msg::AcceptKey)).padding(Padding { top: 10.0, ..Padding::ZERO }));
                    }
                    content = content.push(container(card).padding(12).width(FILL).style(t::panel(rgb(0x19181d), rgb(0x39313f), 7.0))).push(v_space(20.0));
                }
            }
            content.push(honest_note(Icon::Check, "This is an early implementation, not an audited messenger. Account and connection metadata remain visible to the service. Forward secrecy is not implemented. Recipients can copy or record content."))
                .push(v_space(22.0))
                .push(primary("Got it", Some(Icon::Check), Some(close.clone())))
                .into()
        }
    };
    modal(container(scroll_area(content)).padding(34).into(), 520.0, Some(close))
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
            .padding(11).text_size(14).width(FILL).style(t::select()).menu_style(t::menu()).into(), 10.0, 14.0, rgb(0xb7a5c5))
    };
    let mut content = Column::new().push(modal_header(Icon::Users, 29.0, close.clone()));
    match dialog {
        Dialog::Start => {
            content = content.push(modal_title("Your people belong here."))
                .push(body("Start a private community, or join a space with an invite from its owner.", 15.0, rgb(0x99919f), 1.8))
                .push(container(column![
                    button(container(row![t::icon(Icon::Plus, 17.0, rgb(0x251c31)), text("Create a community").size(15).font(t::semibold()).color(rgb(0x251c31))].spacing(12).align_y(Vertical::Center)).center_x(FILL)).width(FILL).padding([13, 18]).style(t::primary()).on_press(Msg::OpenDialog(Some(Dialog::Create))),
                    button(container(lead_icon(Icon::ArrowUpRight, "Join with an invite", rgb(0xc1b0d0), 17.0, 15.0, 12.0)).center_x(FILL)).width(FILL).padding([12, 16]).style(t::secondary()).on_press(Msg::OpenDialog(Some(Dialog::Join))),
                ].spacing(12)).padding(Padding { top: 27.0, ..Padding::ZERO }));
        }
        Dialog::Invite => {
            content = content.push(modal_title("Good company starts here."))
                .push(body("This invite expires in 24 hours. Generating a new invite revokes the previous link. Anyone with it can join, up to 32 members.", 15.0, rgb(0x99919f), 1.8));
            match &app.invite {
                Some(invite) => {
                    content = content.push(container(field("Invite code", text_input("", &invite.token).padding(13).size(15).style(t::dialog_input()).into(), 10.0, 14.0, rgb(0xb7a5c5))).padding([24, 0]))
                        .push(primary("Copy invite link", Some(Icon::Copy), Some(Msg::CopyInvite)))
                        .push(v_space(24.0))
                        .push(honest_note(Icon::Timer, "New members receive future messages only. Their devices cannot decrypt older history."));
                }
                None => content = content.push(notice(if app.busy { "Creating your invite..." } else { "Unable to create an invite. Close and try again." })),
            }
        }
        Dialog::Bans => {
            content = content.push(modal_title("Community bans")).push(body("Banned accounts cannot use an invite to rejoin. Removing a ban revokes the current invite.", 15.0, rgb(0x99919f), 1.8));
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
                .push(body("Select a channel. Its stored messages, including pins, will be deleted. Every community must keep at least one text channel.", 15.0, rgb(0x99919f), 1.8))
                .push(container(channel_picker()).padding([24, 0]))
                .push(danger("Continue", target.map(|channel| Msg::AskConfirm(Some(Confirm { action: "delete-channel".into(), member: None, channel: Some(channel) })))));
        }
        Dialog::RenameCommunity | Dialog::RenameChannel => {
            let channel = *dialog == Dialog::RenameChannel;
            content = content.push(modal_title(if channel { "Rename channel or room" } else { "Rename community" }))
                .push(body("Everyone in the community will see the updated name.", 15.0, rgb(0x99919f), 1.8));
            if channel { content = content.push(container(channel_picker()).padding(Padding { top: 24.0, ..Padding::ZERO })); }
            content = content.push(container(field("New name", text_input("", &app.dialog_value).on_input(Msg::DialogValue).on_submit(Msg::SubmitDialog).padding(13).size(15).style(t::dialog_input()).into(), 10.0, 14.0, rgb(0xb7a5c5))).padding([24, 0]))
                .push(primary(if app.busy { "Saving..." } else { "Save name" }, None, (!app.busy && !app.dialog_value.trim().is_empty()).then_some(Msg::SubmitDialog)));
        }
        Dialog::Create | Dialog::Join | Dialog::Channel => {
            let (title, note, label, placeholder, submit) = match dialog {
                Dialog::Create => ("A new corner of the internet.", "Give your community a name. We will start you off with a general channel.", "Community name", "The weekend crew", "Create community"),
                Dialog::Join => ("Your people are waiting.", "Paste an invite link or code from a community owner.", "Invite link or code", "Paste your invite here", "Join community"),
                _ if app.dialog_voice => ("Give the conversation a home.", "All community members can join. Up to six people can share encrypted audio at a time.", "Room name", "lounge", "Create voice room"),
                _ => ("Give the conversation a home.", "Only the owner can create channels. All community members can read future messages.", "Channel name", "ideas-and-plans", "Create channel"),
            };
            content = content.push(modal_title(title)).push(body(note, 15.0, rgb(0x99919f), 1.8));
            if *dialog == Dialog::Channel {
                let options = vec!["Text channel".to_string(), "Voice room · up to 6 people".to_string()];
                let selected = Some(options[usize::from(app.dialog_voice)].clone());
                content = content.push(container(field("Channel type", pick_list(options, selected, |value: String| Msg::DialogVoice(value.starts_with("Voice"))).padding(11).text_size(14).width(FILL).style(t::select()).menu_style(t::menu()).into(), 10.0, 14.0, rgb(0xb7a5c5))).padding(Padding { top: 24.0, ..Padding::ZERO }));
            }
            content = content.push(container(field(label, text_input(placeholder, &app.dialog_value).on_input(Msg::DialogValue).on_submit(Msg::SubmitDialog).padding(13).size(15).style(t::dialog_input()).into(), 10.0, 14.0, rgb(0xb7a5c5))).padding(Padding { top: 24.0, bottom: 20.0, ..Padding::ZERO }))
                .push(primary(if app.busy { "One moment..." } else { submit }, Some(Icon::Check), (!app.busy && !app.dialog_value.trim().is_empty()).then_some(Msg::SubmitDialog)))
                .push(v_space(24.0))
                .push(honest_note(Icon::ShieldCheck, "Messages are encrypted on your device. Community names, channel names, and membership are visible to the service. This early encryption protocol has not been independently audited."));
        }
    }
    modal(container(scroll_area(content)).padding(34).into(), 520.0, (!app.busy).then_some(close))
}

fn confirm_view<'a>(app: &'a App, confirm: &'a Confirm) -> E<'a> {
    let community = app.community.as_ref().map(|c| c.name.clone()).unwrap_or_default();
    let member = confirm.member.as_ref().map(|m| m.name.clone()).unwrap_or_default();
    let channel = confirm.channel.as_ref().map(|c| c.name.clone()).unwrap_or_default();
    let (title, note, label) = match confirm.action.as_str() {
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
    let go = (!app.busy).then_some(Msg::ApplyConfirm);
    let confirm_button = if matches!(confirm.action.as_str(), "transfer" | "promote") {
        button(container(text(label).size(15).font(t::semibold()).color(rgb(0x251c31))).center_x(FILL)).width(FILL).padding([13, 18]).style(t::primary()).on_press_maybe(go)
    } else {
        button(container(text(label).size(15).font(t::semibold()).color(Color::WHITE)).center_x(FILL)).width(FILL).padding([13, 18]).style(t::danger()).on_press_maybe(go)
    };
    let content = column![
        modal_header(Icon::ShieldCheck, 29.0, Msg::AskConfirm(None)),
        modal_title(title),
        body(note, 15.0, rgb(0x99919f), 1.8),
        container(column![confirm_button, button(container(text("Cancel").size(15).color(rgb(0xc1b0d0))).center_x(FILL)).width(FILL).padding([12, 16]).style(t::secondary()).on_press_maybe((!app.busy).then_some(Msg::AskConfirm(None)))].spacing(12)).padding(Padding { top: 27.0, ..Padding::ZERO }),
    ];
    modal(container(content).padding(34).into(), 520.0, (!app.busy).then_some(Msg::AskConfirm(None)))
}

fn delete_view<'a>(app: &'a App, id: &'a str) -> E<'a> {
    let note = if app.mode == Mode::Direct { "Delete this message for both people? Copies already saved elsewhere cannot be removed." } else { "Delete this message for the community? Copies already saved elsewhere cannot be removed." };
    let content = column![
        modal_header(Icon::Trash, 26.0, Msg::AskDelete(None)),
        modal_title("Delete message?"),
        body(note, 15.0, rgb(0x99919f), 1.8),
        container(column![
            button(container(text("Delete message").size(15).font(t::semibold()).color(Color::WHITE)).center_x(FILL)).width(FILL).padding([13, 18]).style(t::danger()).on_press(Msg::Delete(id.to_owned())),
            button(container(text("Cancel").size(15).color(rgb(0xc1b0d0))).center_x(FILL)).width(FILL).padding([12, 16]).style(t::secondary()).on_press(Msg::AskDelete(None)),
        ].spacing(12)).padding(Padding { top: 27.0, ..Padding::ZERO }),
    ];
    modal(container(content).padding(34).into(), 420.0, Some(Msg::AskDelete(None)))
}

// ----- Account settings (AccountSettings) ----------------------------------------

fn security_card<'a>(icon: Icon, title: E<'a>, note: &'a str, content: E<'a>) -> E<'a> {
    container(column![
        row![container(t::icon(icon, 20.0, rgb(0xc9ade6))).width(39).height(39).center_x(39).center_y(39).style(t::panel(rgb(0x352b41), rgb(0x493958), 12.0)),
            column![title, body(note, 14.0, rgb(0x9d8eaa), 1.7)].spacing(5)].spacing(12),
        container(content).padding(Padding { top: 20.0, ..Padding::ZERO }),
    ]).padding(22).width(FILL).style(t::panel(rgb(0x25222b), rgb(0x3d3446), 12.0)).into()
}
fn card_title<'a>(value: &'a str) -> E<'a> { text(value).size(17).font(t::semibold()).color(rgb(0xe0d7e8)).into() }
fn small_note<'a>(value: impl text::IntoFragment<'a>) -> E<'a> {
    column![t::rule(rgb(0x39313f), true), container(body(value, 12.0, rgb(0x8e7d99), 1.8)).padding(Padding { top: 14.0, ..Padding::ZERO })].padding(Padding { top: 18.0, ..Padding::ZERO }).into()
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
                background: (if checked { rgb(0xb69bdc) } else { rgb(0x1d1b22) }).into(),
                icon_color: rgb(0x21182e),
                border: t::border(if checked || hovered { rgb(0xb69bdc) } else { rgb(0x4b3d55) }, 1.0, 3.0),
                text_color: Some(rgb(0xb8a4c7)),
            }
        }).into()
}

fn settings_view(app: &App) -> E<'_> {
    let Some(session) = &app.session else { return space().into() };
    let close = Msg::OpenSettings(false);
    let tabs = [(Tab::Profile, Icon::UserRound, "Your profile"), (Tab::Security, Icon::KeyRound, "Password & passkeys"), (Tab::Privacy, Icon::ShieldCheck, "Privacy & device"), (Tab::Voice, Icon::Headphones, "Voice & audio")];
    let nav = tabs.iter().fold(Column::new(), |nav, (tab, icon, label)| {
        let selected = app.tab == *tab;
        let color = if selected { rgb(0xd3bdf6) } else { rgb(0x9999a6) };
        nav.push(container(button(row![t::icon(*icon, 17.0, color), text(*label).size(13).color(color)].spacing(11).align_y(Vertical::Center)).width(FILL).padding([12, 10]).style(t::nav_row(selected)).on_press_maybe((!app.busy).then_some(Msg::SetTab(*tab)))).padding([3, 0]))
    });
    let navigation = container(column![
        t::eyebrow("YOUR SONAR", 11.0, t::EYEBROW),
        container(text("Make it yours.").size(23).font(t::semibold()).color(t::TEXT)).padding(Padding { top: 9.0, bottom: 24.0, ..Padding::ZERO }),
        tile(&session.account.full_name, rgb(0x4a3959), rgb(0xd9b9ee), Some(rgb(0x6a4b7d)), 65.0, 22.0, 24.0, None),
        container(text(session.account.full_name.clone()).size(16).font(t::semibold()).color(t::TEXT)).padding(Padding { top: 12.0, ..Padding::ZERO }),
        container(text(session.account.email.clone()).size(13).color(rgb(0x8c7c97))).padding(Padding { top: 7.0, bottom: 24.0, ..Padding::ZERO }),
        nav,
        container(button(row![t::icon(Icon::LogOut, 16.0, rgb(0xbb91a2)), text("Sign out everywhere").size(13).color(rgb(0xbb91a2))].spacing(11).align_y(Vertical::Center)).width(FILL).padding([12, 10]).style(t::nav_row(false)).on_press_maybe((!app.busy).then_some(Msg::SignOut))).padding(Padding { top: 30.0, ..Padding::ZERO }),
        space().height(FILL),
        text("Free to use. Your space to keep.").size(11).color(rgb(0x74667d)),
    ].align_x(Horizontal::Center)).width(270).height(FILL).padding(Padding { top: 34.0, bottom: 24.0, left: 20.0, right: 20.0 }).style(|_| container::Style { background: Some(rgb(0x1b1a20).into()), border: iced::Border { radius: iced::border::Radius { top_left: 17.0, bottom_left: 17.0, top_right: 0.0, bottom_right: 0.0 }, ..Default::default() }, ..Default::default() });

    let heading = |eyebrow: &'static str, title: &'static str, note: &'static str| -> Column<'_, Msg> {
        column![t::eyebrow(eyebrow, 11.0, t::EYEBROW), container(text(title).size(24).font(t::semibold()).color(t::TEXT)).padding([10, 0]), body(note, 15.0, rgb(0xa497ae), 1.8)]
    };
    let content: Column<Msg> = match app.tab {
        Tab::Profile => heading("A FAMILIAR FACE", "Your profile", "The name your people see in conversations and communities.")
            .push(container(field("Display name", dialog_input("", &app.profile_name, Msg::ProfileName, false), 10.0, 13.0, rgb(0xb7a5c5))).padding([22, 0]))
            .push(field("Email address", text_input("", &session.account.email).padding(13).size(15).style(t::input(rgb(0x211d26), rgb(0x4d3e58), rgb(0x4d3e58), 7.0)).into(), 10.0, 13.0, rgb(0xb7a5c5)))
            .push(container(text("Email changes are not available yet.").size(12).color(rgb(0x887691))).padding(Padding { top: 8.0, bottom: 22.0, ..Padding::ZERO }))
            .push(primary(if app.busy { "Saving..." } else { "Save changes" }, Some(Icon::Check), (!app.busy && !app.profile_name.trim().is_empty() && app.profile_name.trim() != session.account.full_name).then_some(Msg::SaveProfile)))
            .push(v_space(28.0))
            .push(honest_note(Icon::UserRound, "Your display name and email are account information visible to the service. Message content is encrypted separately on your device.")),
        Tab::Security => {
            // Passkeys.
            let mut passkeys = Column::new();
            match &app.passkeys {
                None => passkeys = passkeys.push(notice("Loading passkeys...")),
                Some(list) if list.is_empty() => passkeys = passkeys.push(container(row![t::icon(Icon::KeyRound, 18.0, rgb(0x9e83b7)), column![text("No passkeys yet").size(14).font(t::semibold()).color(rgb(0xc0accf)), body("Add one to sign in with your face, fingerprint, or device PIN.", 13.0, rgb(0x87768f), 1.7)].spacing(5)].spacing(13))
                    .padding(16).width(FILL).style(t::panel(rgb(0x201d25), rgb(0x4b3b57), 8.0))).push(v_space(17.0)),
                Some(list) => for passkey in list {
                    let removing = app.passkey_removing.as_deref() == Some(&passkey.id);
                    passkeys = passkeys.push(container(container(row![
                        t::icon(Icon::KeyRound, 16.0, rgb(0xb397ca)),
                        column![text(passkey.name.clone()).size(14).font(t::semibold()).color(rgb(0xcdb6dd)), text(format!("Added {}", day(&passkey.created_at))).size(12).color(rgb(0x8e7b9c))].spacing(5).width(FILL),
                        icon_btn(Icon::Trash, 16.0, (!app.busy).then(|| Msg::PasskeyRemove(if removing { None } else { Some(passkey.id.clone()) })), removing),
                    ].spacing(10).align_y(Vertical::Center)).padding(12).width(FILL).style(t::panel(rgb(0x211c28), rgb(0x3e3349), 7.0))).padding(Padding { top: 8.0, ..Padding::ZERO }));
                },
            }
            if app.passkey_removing.is_some() {
                passkeys = passkeys.push(container(column![
                    text("Remove this passkey?").size(15).font(t::semibold()).color(rgb(0xcebadc)),
                    body("Confirm with your current password. Removing it signs out all sessions.", 14.0, rgb(0xac90a8), 1.8),
                    container(field("Current password", card_input("", &app.passkey_password, Msg::PasskeyPassword, true), 9.0, 13.0, rgb(0xb7a5c5))).padding([17, 0]),
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
                    text("Add a passkey").size(15).font(t::semibold()).color(rgb(0xcebadc)),
                    container(field("Passkey name", card_input("My laptop", &app.passkey_name, Msg::PasskeyName, false), 9.0, 13.0, rgb(0xb7a5c5))).padding(Padding { top: 17.0, ..Padding::ZERO }),
                    container(field("Current password", card_input("", &app.passkey_password, Msg::PasskeyPassword, true), 9.0, 13.0, rgb(0xb7a5c5))).padding([17, 0]),
                    row![
                        action(if app.busy { "Waiting for Windows..." } else { "Create passkey" }, None, t::primary(), rgb(0x251c31), create, [11.0, 14.0], 14.0),
                        button(text("Cancel").size(14)).padding([10, 5]).style(t::text_button()).on_press(Msg::PasskeyAdding(false)),
                    ].spacing(14),
                ]).padding(Padding { top: 16.0, ..Padding::ZERO }));
            } else if app.passkey_removing.is_none() {
                #[cfg(windows)]
                { passkeys = passkeys.push(container(button(row![t::icon(Icon::Plus, 16.0, rgb(0xc1b0d0)), text("Add a passkey").size(14).color(rgb(0xc1b0d0))].spacing(12).align_y(Vertical::Center)).padding([10, 13]).style(t::secondary()).on_press_maybe((!app.busy).then_some(add_passkey))).padding(Padding { top: 18.0, ..Padding::ZERO })); }
            }
            passkeys = passkeys.push(small_note("Your passkey signs you in. Your messages unlock with your recovery key the first time you use a new device."));
            let passkey_title = row![card_title("Passkeys"), container(text(t::tracked("RECOMMENDED")).size(10).font(t::bold()).color(rgb(0xa7c1a7))).padding([3, 5]).style(t::panel(rgb(0x2b352d), rgb(0x4d5c4d), 4.0))].spacing(9).align_y(Vertical::Center);

            // Sessions.
            let mut sessions = Column::new();
            match &app.sessions {
                None => sessions = sessions.push(notice("Loading sessions...")),
                Some(list) => for entry in &list.sessions {
                    let method = if entry.method == "passkey" { "Passkey sign-in" } else { "Password sign-in" };
                    sessions = sessions.push(column![row![
                        column![text(format!("{method}{}", if entry.current { " · This session" } else { "" })).size(15).font(t::semibold()).color(t::TEXT), text(format!("Signed in {} · Expires {}", date(&entry.created_at), day(&entry.expires_at))).size(13).color(rgb(0xae9fbc))].spacing(4).width(FILL),
                        button(row![t::icon(Icon::LogOut, 14.0, rgb(0xc1b0d0)), text("Sign out").size(15).color(rgb(0xc1b0d0))].spacing(10).align_y(Vertical::Center)).padding([12, 16]).style(t::secondary()).on_press_maybe((!app.busy).then(|| Msg::EndSession(entry.id.clone()))),
                    ].align_y(Vertical::Center).padding([10, 0]), t::rule(rgb(0x39313f), true)]);
                },
            }
            sessions = sessions.push(small_note("Session entries don't include device names or IP addresses. Signing out does not erase encryption keys from a device."));

            // Password.
            let show = app.show_passwords;
            let pw = |index: usize, label: &'static str| -> E { field(label, card_input("", &app.passwords[index], move |v| Msg::PasswordField(index, v), !show), 9.0, 13.0, rgb(0xb7a5c5)) };
            let password = column![
                pw(0, "Current password"),
                row![container(pw(1, "New password")).width(FILL), container(pw(2, "Confirm new password")).width(FILL)].spacing(14),
                checkbox_row("Show passwords", show, Msg::ShowPasswords),
                text("Use at least 10 characters.").size(12).color(rgb(0x9c8ba8)),
                action(if app.busy { "Updating password..." } else { "Update password & sign out" }, Some(Icon::KeyRound), t::primary(), rgb(0x251c31), (!app.busy).then_some(Msg::ChangePassword), [12.0, 14.0], 14.0),
                column![t::rule(rgb(0x39313f), true), row![t::icon(Icon::LockKeyhole, 13.0, rgb(0x9680a4)), body("Your message key is re-protected with the new password on this device. Your encrypted history is kept.", 12.0, rgb(0x9680a4), 1.8)].spacing(8).padding(Padding { top: 15.0, ..Padding::ZERO })].padding(Padding { top: 2.0, ..Padding::ZERO }),
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
                .push(container(container(row![t::icon(Icon::ShieldCheck, 21.0, rgb(0xc5add8)), column![text(status).size(14).font(t::semibold()).color(rgb(0xc5add8)), text(detail).size(12).color(rgb(0x9985a6))].spacing(6).width(FILL), t::dot(dot, 5.0)].spacing(12).align_y(Vertical::Center))
                    .padding(16).width(FILL).style(t::panel(rgb(0x2b2533), rgb(0x484051), 8.0))).padding([24, 0]));
            if let Some(kind) = app.backup_kind {
                let (title, note, action) = match kind {
                    "none" => ("Turn on multi-device access", "Creates a new key protected by a recovery key. Your contacts will be asked to confirm your new key. Messages encrypted for the old key cannot be opened by the new one.", "Turn on multi-device access"),
                    "password" => ("Set up a recovery key", "Your recovery key unlocks your messages on a new device and resets a forgotten password. A new one replaces the old one.", "Create recovery key"),
                    _ => ("Create a new recovery key", "Your recovery key unlocks your messages on a new device and resets a forgotten password. A new one replaces the old one.", "Create recovery key"),
                };
                col = col.push(container(column![
                    text(title).size(16).font(t::semibold()).color(rgb(0xe2d4f6)),
                    body(note, 15.0, rgb(0x8f8a9b), 1.6),
                    field("Current password", dialog_input("", &app.sync_password, Msg::SyncPassword, true), 10.0, 13.0, rgb(0xb7a5c5)),
                    primary(if app.busy { "Securing your key..." } else { action }, Some(Icon::KeyRound), (!app.busy && !app.sync_password.is_empty()).then_some(Msg::EnableSync)),
                ].spacing(12)).padding(16).width(FILL).style(t::panel(rgb(0x221f29), rgb(0x3d324c), 10.0))).push(v_space(22.0));
            }
            col.push(container(column![
                    text("Your safety code").size(13).font(t::semibold()).color(rgb(0xb7a5ca)),
                    container(body(print, 14.0, rgb(0xb8b1c3), 1.8)).padding(Padding { top: 8.0, ..Padding::ZERO }),
                    container(button(row![t::icon(Icon::Copy, 14.0, rgb(0xc1b0d0)), text("Copy safety code").size(12).color(rgb(0xc1b0d0))].spacing(12).align_y(Vertical::Center)).padding([8, 10]).style(t::secondary()).on_press(Msg::CopyFingerprint)).padding(Padding { top: 14.0, ..Padding::ZERO }),
                ]).padding(12).width(FILL).style(t::panel(rgb(0x19181d), rgb(0x39313f), 7.0)))
                .push(container(body("Read this code to a contact in person or on a call so they can confirm it is really you. It is a code, not a fingerprint scan.", 13.0, rgb(0x9c8ba8), 1.8)).padding([12, 0]))
                .push(column![t::rule(rgb(0x39303f), true), container(column![text("A short memory, by design.").size(16).font(t::semibold()).color(t::TEXT), body("Messages expire after seven days unless pinned. Calls have no recording or storage in Sonar.", 14.0, rgb(0x9c8ba8), 1.8)].spacing(9)).padding(Padding { top: 20.0, ..Padding::ZERO })])
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
            let meter = container(container(space()).width(Length::FillPortion((level * 100.0) as u16 + 1)).height(FILL).style(t::gradient_panel(90.0, rgb(0x8baa96), rgb(0xbaa3e1), Color::TRANSPARENT, 8.0)))
                .width(FILL).height(10).style(t::panel(rgb(0x19191e), rgb(0x423a4b), 8.0));
            let meter = row![meter, space().width(Length::FillPortion(((1.0 - level) * 100.0) as u16 + 1))];
            heading("SOUND LIKE YOURSELF", "Voice & audio", "Choose your microphone and speakers. Settings stay on this computer.")
                .push(v_space(24.0))
                .push(security_card(Icon::Mic, card_title("Your microphone"), "Test your input before your next call.", column![
                    field("Microphone", pick_list(inputs, input, Msg::SelectInput).padding(11).text_size(14).width(FILL).style(t::select()).menu_style(t::menu()).into(), 9.0, 13.0, rgb(0xb7a5c5)),
                    container(meter).padding(Padding { top: 20.0, bottom: 14.0, ..Padding::ZERO }),
                    row![button(text(if app.testing { "Listening..." } else { "Test microphone" }).size(13).color(rgb(0xc1b0d0))).padding([9, 12]).style(t::secondary()).on_press_maybe((!app.testing).then_some(Msg::TestMic)),
                        body(app.mic_level.map(|l| format!("Peak level {l}%. Nothing is recorded.")).unwrap_or_else(|| "A three-second local test. Nothing is recorded or sent.".into()), 12.0, rgb(0x9e8aac), 1.7)].spacing(10).align_y(Vertical::Center),
                ].into()))
                .push(v_space(24.0))
                .push(security_card(Icon::Speaker, card_title("Your speakers"), "Choose where calls play.", column![
                    field("Speakers", pick_list(outputs, output, Msg::SelectOutput).padding(11).text_size(14).width(FILL).style(t::select()).menu_style(t::menu()).into(), 9.0, 13.0, rgb(0xb7a5c5)),
                    container(button(row![t::icon(Icon::Play, 14.0, rgb(0xc1b0d0)), text(if app.testing { "Playing..." } else { "Play test sound" }).size(13).color(rgb(0xc1b0d0))].spacing(10).align_y(Vertical::Center)).padding([10, 13]).style(t::secondary()).on_press_maybe((!app.testing).then_some(Msg::TestSpeakers))).padding(Padding { top: 18.0, ..Padding::ZERO }),
                ].into()))
                .push(v_space(24.0))
                .push(security_card(Icon::Shield, card_title("Call connectivity"), "Relay-only hides your network address from the other person, but needs a TURN server.", checkbox_row("Use relay-only connections", app.settings.relay_only, Msg::RelayOnly)))
        }
    };
    let content = container(scroll_area(container(content).padding(Padding { top: 44.0, bottom: 32.0, left: 36.0, right: 36.0 })).height(FILL)).width(FILL).height(FILL);
    let close_button = container(icon_btn(Icon::X, 20.0, (!app.busy).then_some(close.clone()), false)).width(FILL).align_x(Horizontal::Right).padding(18);
    let card = container(row![navigation, t::rule(rgb(0x37313e), false), stack![content, close_button]]).width(940).max_height(720).height(FILL)
        .style(t::modal_card(rgb(0x222127), rgb(0x44394d), 17.0));
    opaque(mouse_area(center(opaque(card)).padding(20).style(t::fill(rgba(0x09090eb8)))).on_press(if app.busy { Msg::Noop } else { close })).into()
}
