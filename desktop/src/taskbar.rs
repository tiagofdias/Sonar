//! The Windows taskbar: the unread number on Sonar's icon, and flashing for attention.
use windows::core::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{ITaskbarList3, TaskbarList};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, HICON, ICONINFO};

const SIZE: usize = 32;

/// Sonar's own top-level window.
fn own_window() -> Option<HWND> {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND as RawWindow, LPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible};
    unsafe extern "system" fn visit(window: RawWindow, found: LPARAM) -> BOOL {
        let mut owner = 0;
        unsafe {
            GetWindowThreadProcessId(window, &mut owner);
            if owner != GetCurrentProcessId() || IsWindowVisible(window) == 0 { return 1; }
            *(found as *mut RawWindow) = window;
        }
        0
    }
    let mut found: RawWindow = std::ptr::null_mut();
    unsafe { EnumWindows(Some(visit), &mut found as *mut RawWindow as LPARAM); }
    (!found.is_null()).then_some(HWND(found))
}

/// Whether Sonar is the window the person is using right now.
pub fn focused() -> bool {
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let mut owner = 0;
    unsafe {
        GetWindowThreadProcessId(GetForegroundWindow(), &mut owner);
        owner == GetCurrentProcessId()
    }
}

/// Flashes the taskbar button until Sonar is brought forward.
pub fn flash() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FlashWindowEx, FLASHWINFO, FLASHW_TIMERNOFG, FLASHW_TRAY};
    let Some(window) = own_window() else { return };
    let info = FLASHWINFO { cbSize: size_of::<FLASHWINFO>() as u32, hwnd: window.0, dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG, uCount: 0, dwTimeout: 0 };
    unsafe { FlashWindowEx(&info); }
}

fn label(count: u32) -> String { if count > 99 { "99+".into() } else { count.to_string() } }

fn glyph(character: char) -> [u8; 5] {
    match character {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b001, 0b001, 0b001],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        _ => [0b000, 0b010, 0b111, 0b010, 0b000],
    }
}

/// A red circle holding the number, as 32x32 BGRA.
fn badge_pixels(text: &str) -> Vec<u8> {
    let mut pixels = vec![0u8; SIZE * SIZE * 4];
    let center = SIZE as f32 / 2.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let distance = ((x as f32 + 0.5 - center).powi(2) + (y as f32 + 0.5 - center).powi(2)).sqrt();
            let coverage = (center - distance).clamp(0.0, 1.0);
            let at = (y * SIZE + x) * 4;
            pixels[at..at + 4].copy_from_slice(&[0x4d, 0x48, 0xe5, (coverage * 255.0) as u8]);
        }
    }
    let count = text.chars().count();
    let scale = match count { 1 => 4, 2 => 3, _ => 2 };
    let width = count * 3 * scale + (count - 1) * scale;
    let (left, top) = ((SIZE - width) / 2, (SIZE - 5 * scale) / 2);
    for (index, character) in text.chars().enumerate() {
        let rows = glyph(character);
        let start = left + index * 4 * scale;
        for (row, bits) in rows.iter().enumerate() {
            for column in 0..3 {
                if bits & (0b100 >> column) == 0 { continue; }
                for y in top + row * scale..top + (row + 1) * scale {
                    for x in start + column * scale..start + (column + 1) * scale {
                        let at = (y * SIZE + x) * 4;
                        pixels[at..at + 4].copy_from_slice(&[255, 255, 255, 255]);
                    }
                }
            }
        }
    }
    pixels
}

fn create_icon(pixels: &[u8]) -> windows::core::Result<HICON> {
    unsafe {
        let color = CreateBitmap(SIZE as i32, SIZE as i32, 1, 32, Some(pixels.as_ptr().cast()));
        let mask = CreateBitmap(SIZE as i32, SIZE as i32, 1, 1, None);
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info);
        let _ = DeleteObject(color);
        let _ = DeleteObject(mask);
        icon
    }
}

thread_local! { static TASKBAR: std::cell::RefCell<Option<ITaskbarList3>> = const { std::cell::RefCell::new(None) }; }

fn taskbar() -> Option<ITaskbarList3> {
    TASKBAR.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let list: Option<ITaskbarList3> = unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER).ok()
            };
            if let Some(list) = &list { if unsafe { list.HrInit() }.is_err() { return None; } }
            *slot = list;
        }
        slot.clone()
    })
}

/// Shows `count` on the taskbar icon, or clears it at zero. False means try again later.
pub fn set_badge(count: u32) -> bool {
    let (Some(window), Some(list)) = (own_window(), taskbar()) else { return false };
    unsafe {
        if count == 0 { return list.SetOverlayIcon(window, HICON::default(), None).is_ok(); }
        let Ok(icon) = create_icon(&badge_pixels(&label(count))) else { return false };
        let done = list.SetOverlayIcon(window, icon, w!("Unread messages")).is_ok();
        let _ = DestroyIcon(icon);
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn white(pixels: &[u8]) -> usize { pixels.chunks(4).filter(|p| p[0] == 255 && p[1] == 255 && p[2] == 255).count() }
    #[test]
    fn labels_stop_at_99() {
        assert_eq!(label(7), "7");
        assert_eq!(label(99), "99");
        assert_eq!(label(100), "99+");
    }
    #[test]
    fn numbers_are_drawn_inside_the_circle() {
        for text in ["1", "42", "99+"] {
            let pixels = badge_pixels(text);
            assert_eq!(pixels.len(), SIZE * SIZE * 4);
            assert!(white(&pixels) > 20, "{text}");
            assert_eq!(pixels[3], 0, "corners stay transparent");
        }
        assert_ne!(white(&badge_pixels("1")), white(&badge_pixels("8")));
    }
}
