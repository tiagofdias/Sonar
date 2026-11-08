// Development check: captures a small off-screen test window (never the real
// screen), encodes it on the GPU, and writes the raw video for inspection.
// Usage: cargo run --release --example screen_probe -- <output> <height> <seconds>
#[path = "../src/screen_media.rs"]
mod screen_media;
use screen_media::{Quality, Source};
use std::io::Write;
use std::time::{Duration, Instant};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

fn wide(text: &str) -> Vec<u16> { text.encode_utf16().chain([0]).collect() }

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (path, height, seconds): (&str, u32, u64) = (&args[0], args[1].parse().unwrap(), args[2].parse().unwrap());
    let (w, h) = (height as i32 * 16 / 9, height as i32);
    let window = unsafe { CreateWindowExW(WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW, wide("STATIC").as_ptr(), wide("sonar-capture-test").as_ptr(), WS_POPUP | WS_VISIBLE, -8000, 0, w, h, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null()) };
    assert!(!window.is_null(), "test window");
    let handle = window as isize;
    if std::env::var_os("SONAR_SOFTWARE").is_some() { screen_media::FORCE_SOFTWARE.store(true, std::sync::atomic::Ordering::Relaxed); }
    let (path_owned, stop_at) = (path.to_owned(), Instant::now() + Duration::from_secs(seconds));
    let worker = std::thread::spawn(move || {
        let mut file = std::fs::File::create(path_owned).unwrap();
        let (mut packets, mut bytes, mut keys) = (0u64, 0u64, 0u64);
        let started = Instant::now();
        let result = screen_media::run(Source::Window(handle), Quality::for_height(height), &std::sync::atomic::AtomicBool::new(false), &mut || Instant::now() < stop_at,
            &mut |q, codec, name| println!("READY {}x{} {:?} via {name}", q.width, q.height, codec),
            &mut |data, key, _| { file.write_all(data).unwrap(); packets += 1; bytes += data.len() as u64; keys += key as u64; });
        println!("RESULT {result:?} packets={packets} keyframes={keys} bytes={bytes} fps={:.1}", packets as f64 / started.elapsed().as_secs_f64());
    });
    let mut message = unsafe { std::mem::zeroed::<MSG>() };
    let mut counter = 0u32;
    while !worker.is_finished() {
        unsafe {
            while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 { TranslateMessage(&message); DispatchMessageW(&message); }
            counter += 1;
            SetWindowTextW(window, wide(&format!("Sonar capture test frame {counter} {}", "text ".repeat((counter % 40) as usize))).as_ptr());
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    worker.join().unwrap();
    let info = unsafe {
        use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb);
        c.PeakWorkingSetSize / (1024 * 1024)
    };
    println!("PEAK RAM {info} MB");
}
