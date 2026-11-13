//! Captures the sound of one app (with its child processes), or of everything
//! except Sonar, through Windows' per-process loopback. Sonar's own call audio
//! is therefore never sent back to the people in the call.
#![allow(dead_code)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Mutex;
use std::time::Duration;
use windows::core::{implement, IUnknown, Interface, PROPVARIANT};
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

pub const FRAME: usize = 960; // 20 ms at 48 kHz, as Opus expects.

#[derive(Clone, Copy, Debug)]
pub enum Sound { OnlyProcess(u32), AllExceptProcess(u32) }

#[implement(IActivateAudioInterfaceCompletionHandler, windows::Win32::System::Com::IAgileObject)]
struct Activated(Mutex<Option<SyncSender<()>>>);
impl windows::Win32::System::Com::IAgileObject_Impl for Activated_Impl {}
impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(&self, _operation: Option<&IActivateAudioInterfaceAsyncOperation>) -> windows::core::Result<()> {
        if let Some(done) = self.0.lock().ok().and_then(|mut value| value.take()) { let _ = done.send(()); }
        Ok(())
    }
}

/// A PROPVARIANT holding a VT_BLOB, a shape the windows crate has no constructor for.
#[repr(C)]
struct BlobVariant { kind: u16, r1: u16, r2: u16, r3: u16, size: u32, data: *mut core::ffi::c_void, _pad: usize }

fn open(sound: Sound) -> Result<IAudioClient, String> {
    unsafe {
        let (mode, process) = match sound {
            Sound::OnlyProcess(pid) => (PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, pid),
            Sound::AllExceptProcess(pid) => (PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE, pid),
        };
        let mut parameters = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 { ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS { TargetProcessId: process, ProcessLoopbackMode: mode } },
        };
        let blob = BlobVariant { kind: 65, r1: 0, r2: 0, r3: 0, size: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32, data: &mut parameters as *mut _ as *mut _, _pad: 0 };
        let (done, wait) = sync_channel(1);
        let handler: IActivateAudioInterfaceCompletionHandler = Activated(Mutex::new(Some(done))).into();
        let operation = ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&blob as *const BlobVariant as *const PROPVARIANT), &handler)
            .map_err(|e| format!("Windows could not capture that app's sound: {e}"))?;
        wait.recv_timeout(Duration::from_secs(5)).map_err(|_| "The sound capture did not start".to_string())?;
        let (mut result, mut interface): (windows::core::HRESULT, Option<IUnknown>) = (windows::core::HRESULT(0), None);
        operation.GetActivateResult(&mut result, &mut interface).map_err(|e| format!("Sound capture: {e}"))?;
        result.ok().map_err(|e| format!("Sound capture: {e}"))?;
        interface.ok_or("No sound capture")?.cast().map_err(|e| format!("Sound capture: {e}"))
    }
}

/// Delivers mono 48 kHz frames until `stop` is set.
pub fn run(sound: Sound, stop: &AtomicBool, on_frame: &mut dyn FnMut(&[f32])) -> Result<(), String> { capture(sound, false, stop, on_frame) }
/// Delivers interleaved stereo 48 kHz frames (20 ms each) until `stop` is set, for music.
pub fn run_stereo(sound: Sound, stop: &AtomicBool, on_frame: &mut dyn FnMut(&[f32])) -> Result<(), String> { capture(sound, true, stop, on_frame) }

fn capture(sound: Sound, stereo: bool, stop: &AtomicBool, on_frame: &mut dyn FnMut(&[f32])) -> Result<(), String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let client = open(sound)?;
        let format = WAVEFORMATEX { wFormatTag: 1, nChannels: 2, nSamplesPerSec: 48_000, nAvgBytesPerSec: 192_000, nBlockAlign: 4, wBitsPerSample: 16, cbSize: 0 };
        client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, 2_000_000, 0, &format, None).map_err(|e| format!("Sound capture: {e}"))?;
        let event = CreateEventW(None, false, false, None).map_err(|e| format!("Sound capture: {e}"))?;
        client.SetEventHandle(event).map_err(|e| format!("Sound capture: {e}"))?;
        let capture: IAudioCaptureClient = client.GetService().map_err(|e| format!("Sound capture: {e}"))?;
        client.Start().map_err(|e| format!("Sound capture: {e}"))?;
        let size = if stereo { FRAME * 2 } else { FRAME };
        let mut frame = Vec::with_capacity(size);
        while !stop.load(Ordering::Acquire) {
            if WaitForSingleObject(event, 100) != WAIT_OBJECT_0 { continue; }
            loop {
                let (mut data, mut count, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                if capture.GetBuffer(&mut data, &mut count, &mut flags, None, None).is_err() || count == 0 { break; }
                let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                let samples = std::slice::from_raw_parts(data as *const i16, count as usize * 2);
                for pair in samples.chunks_exact(2) {
                    if stereo { frame.extend(if silent { [0.0, 0.0] } else { [pair[0] as f32 / 32768.0, pair[1] as f32 / 32768.0] }); }
                    else { frame.push(if silent { 0.0 } else { (pair[0] as f32 + pair[1] as f32) / 65536.0 }); }
                    if frame.len() == size { on_frame(&frame); frame.clear(); }
                }
                let _ = capture.ReleaseBuffer(count);
            }
        }
        let _ = client.Stop();
        let _ = CloseHandle(event);
        Ok(())
    }
}
