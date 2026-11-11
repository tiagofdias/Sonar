//! Watch Together on desktop: the video plays in the person's own browser (no
//! embedded browser, so the app's memory stays as it is). The app asks the server
//! for a one-time link and hands it to the default browser.

/// Checks that a link is exactly a Sonar watch link on this server, so nothing
/// else can ever be passed to the system's "open" handler.
pub fn watch_url(origin: &str, path: &str) -> Option<String> {
    let token = path.strip_prefix("/watch#t=")?;
    if token.len() != 43 || !token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') { return None; }
    let base = url::Url::parse(origin).ok()?;
    let local = matches!(base.host_str(), Some("localhost") | Some("127.0.0.1") | Some("[::1]"));
    if !(base.scheme() == "https" || (base.scheme() == "http" && local)) || base.path() != "/" || base.query().is_some() || !base.username().is_empty() { return None; }
    Some(format!("{}{path}", base.origin().ascii_serialization()))
}

/// Opens a checked watch link in the default browser.
#[cfg(windows)]
pub fn open_in_browser(url: &str) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |value: &str| value.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (verb, target) = (wide("open"), wide(url));
    // ShellExecuteW reports success with a value above 32.
    let result = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
    if result as isize > 32 { Ok(()) } else { Err("Could not open your browser.".into()) }
}
#[cfg(not(windows))]
pub fn open_in_browser(_url: &str) -> Result<(), String> { Err("Opening a browser is only supported on Windows.".into()) }

#[cfg(test)]
mod tests {
    use super::watch_url;
    const TOKEN: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJ0123-_6";
    #[test]
    fn only_watch_links_on_this_server_are_opened() {
        assert_eq!(watch_url("https://sonar.example", &format!("/watch#t={TOKEN}")).unwrap(), format!("https://sonar.example/watch#t={TOKEN}"));
        assert_eq!(watch_url("http://127.0.0.1:5000", &format!("/watch#t={TOKEN}")).unwrap(), format!("http://127.0.0.1:5000/watch#t={TOKEN}"));
        assert!(watch_url("http://sonar.example", &format!("/watch#t={TOKEN}")).is_none(), "plain http only for this computer");
        assert!(watch_url("https://sonar.example", "/watch#t=short").is_none());
        assert!(watch_url("https://sonar.example", &format!("/settings#t={TOKEN}")).is_none());
        assert!(watch_url("https://sonar.example", &format!("/watch#t={}\" & calc", &TOKEN[..40])).is_none());
        assert!(watch_url("file:///C:/Windows", &format!("/watch#t={TOKEN}")).is_none());
    }
}
