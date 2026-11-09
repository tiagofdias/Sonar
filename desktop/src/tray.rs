//! The icon beside the clock, and starting with Windows.
//! The icon gets its own small thread and hidden window, so the app stays light.
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::OnceLock;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, Debug)]
pub enum Action { Open, Quit }

const CALLBACK: u32 = WM_APP + 1;
const MENU_OPEN: i32 = 1;
const MENU_QUIT: i32 = 2;
static SENDER: OnceLock<UnboundedSender<Action>> = OnceLock::new();
static WINDOW: AtomicIsize = AtomicIsize::new(0);

fn wide(value: &str) -> Vec<u16> { value.encode_utf16().chain(std::iter::once(0)).collect() }

fn data(window: HWND) -> NOTIFYICONDATAW {
    let mut item: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    item.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    item.hWnd = window;
    item.uID = 1;
    item
}

fn write_tip(item: &mut NOTIFYICONDATAW, tip: &str) {
    let text = wide(tip);
    let length = text.len().min(item.szTip.len());
    item.szTip[..length].copy_from_slice(&text[..length]);
    item.szTip[item.szTip.len() - 1] = 0;
}

fn send(action: Action) { if let Some(sender) = SENDER.get() { let _ = sender.send(action); } }

unsafe fn show_menu(window: HWND) {
    unsafe {
        let menu = CreatePopupMenu();
        AppendMenuW(menu, MF_STRING, MENU_OPEN as usize, wide("Open Sonar").as_ptr());
        AppendMenuW(menu, MF_STRING, MENU_QUIT as usize, wide("Quit Sonar").as_ptr());
        let mut point = std::mem::zeroed();
        GetCursorPos(&mut point);
        // Without this, the menu would stay open after clicking elsewhere.
        SetForegroundWindow(window);
        match TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY, point.x, point.y, 0, window, std::ptr::null()) {
            MENU_OPEN => send(Action::Open),
            MENU_QUIT => send(Action::Quit),
            _ => {}
        }
        DestroyMenu(menu);
    }
}

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == CALLBACK {
        match (lparam & 0xffff) as u32 {
            WM_LBUTTONUP => send(Action::Open),
            WM_RBUTTONUP => unsafe { show_menu(window) },
            _ => {}
        }
        return 0;
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn run() {
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide("SonarTray");
        let mut description: WNDCLASSW = std::mem::zeroed();
        description.lpfnWndProc = Some(procedure);
        description.hInstance = instance;
        description.lpszClassName = class.as_ptr();
        RegisterClassW(&description);
        let window = CreateWindowExW(WS_EX_TOOLWINDOW, class.as_ptr(), class.as_ptr(), WS_POPUP, 0, 0, 0, 0, std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null());
        if window.is_null() { return; }
        WINDOW.store(window as isize, Ordering::Relaxed);
        let mut item = data(window);
        item.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        item.uCallbackMessage = CALLBACK;
        // Resource 1 is the icon from assets/sonar.rc.
        item.hIcon = LoadImageW(instance, 1 as _, IMAGE_ICON, GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON), LR_DEFAULTCOLOR) as _;
        write_tip(&mut item, "Sonar");
        Shell_NotifyIconW(NIM_ADD, &item);
        let mut message = std::mem::zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// Starts the icon the first time and reports clicks on it.
pub fn events() -> impl iced::futures::Stream<Item = Action> {
    iced::stream::channel(8, async |mut output: iced::futures::channel::mpsc::Sender<Action>| {
        use iced::futures::SinkExt;
        let (sender, mut receiver) = unbounded_channel();
        if SENDER.set(sender).is_ok() { std::thread::spawn(run); }
        while let Some(action) = receiver.recv().await { if output.send(action).await.is_err() { break; } }
    })
}

pub fn set_unread(count: u32) {
    let window = WINDOW.load(Ordering::Relaxed) as HWND;
    if window.is_null() { return; }
    let mut item = data(window);
    item.uFlags = NIF_TIP;
    write_tip(&mut item, &if count == 0 { "Sonar".to_owned() } else { format!("Sonar: {count} unread") });
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &item); }
}

/// Windows leaves a dead icon behind unless it is removed before quitting.
pub fn remove() {
    let window = WINDOW.swap(0, Ordering::Relaxed) as HWND;
    if !window.is_null() { unsafe { Shell_NotifyIconW(NIM_DELETE, &data(window)); } }
}

/// Adds or removes Sonar from the programs Windows opens at sign-in.
pub fn set_start_with_windows(enabled: bool) -> bool {
    use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Run");
    let name = wide("Sonar");
    unsafe {
        if !enabled {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr());
            return true;
        }
        let Ok(program) = std::env::current_exe() else { return false };
        let value = wide(&format!("\"{}\"", program.display()));
        RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_SZ, value.as_ptr().cast(), (value.len() * 2) as u32) == 0
    }
}
