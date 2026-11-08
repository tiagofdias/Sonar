//! Native Windows file picker. The user chooses both input and output paths.
use std::path::PathBuf;

#[cfg(windows)]
fn dialog(save: bool, suggested: &str) -> Result<Option<PathBuf>, String> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, GetSaveFileNameW, OPENFILENAMEW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST, OFN_NOCHANGEDIR};
    let mut buffer = vec![0u16; 32768];
    let proposed: Vec<u16> = suggested.encode_utf16().collect();
    if proposed.len() >= buffer.len() { return Err("Nome de ficheiro invalido".into()); }
    buffer[..proposed.len()].copy_from_slice(&proposed);
    let filter: Vec<u16> = "Todos os ficheiros\0*.*\0\0".encode_utf16().collect();
    let mut options = OPENFILENAMEW::default();
    options.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    options.lpstrFile = buffer.as_mut_ptr(); options.nMaxFile = buffer.len() as u32;
    options.lpstrFilter = filter.as_ptr(); options.nFilterIndex = 1;
    options.Flags = OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | if save { 0 } else { OFN_FILEMUSTEXIST };
    let accepted = unsafe { if save { GetSaveFileNameW(&mut options) } else { GetOpenFileNameW(&mut options) } };
    if accepted == 0 { return Ok(None); }
    let end = buffer.iter().position(|v| *v == 0).ok_or("Caminho invalido")?;
    Ok(Some(PathBuf::from(std::ffi::OsString::from_wide(&buffer[..end]))))
}
#[cfg(windows)] pub fn open() -> Result<Option<PathBuf>, String> { dialog(false, "") }
#[cfg(windows)] pub fn save(name: &str) -> Result<Option<PathBuf>, String> { dialog(true, name) }
