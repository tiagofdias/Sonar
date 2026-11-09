// Enumerating devices does not create an audio stream or capture microphone samples.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[derive(Clone, Debug)]
pub struct Devices { pub inputs: Vec<String>, pub outputs: Vec<String>, pub default_input: Option<String>, pub default_output: Option<String> }
pub fn enumerate() -> Result<Devices, String> {
    let host = cpal::default_host();
    let default_input = host.default_input_device().and_then(|device| device.name().ok());
    let default_output = host.default_output_device().and_then(|device| device.name().ok());
    let inputs = host.input_devices().map_err(|_| "Could not list microphones.")?
        .map(|device| device.name().unwrap_or_else(|_| "Unnamed microphone".into())).collect();
    let outputs = host.output_devices().map_err(|_| "Could not list speakers.")?
        .map(|device| device.name().unwrap_or_else(|_| "Unnamed speakers".into())).collect();
    Ok(Devices { inputs, outputs, default_input, default_output })
}
// Capture only after an explicit request, keeping a peak level rather than audio.
pub fn test_input(selected: Option<String>) -> Result<u32, String> {
    use cpal::{SampleFormat, StreamConfig};
    use std::sync::{Arc, atomic::{AtomicBool, AtomicU32, Ordering}};
    let host = cpal::default_host();
    let device = if let Some(name) = selected {
        let mut matching = host.input_devices().map_err(|_| "Could not list microphones.")?
            .filter(|device| device.name().ok().as_deref() == Some(name.as_str()));
        let device = matching.next().ok_or("The selected microphone is no longer available.")?;
        if matching.next().is_some() { return Err("Several microphones share that name. Use the Windows default instead.".into()); }
        device
    } else { host.default_input_device().ok_or("No default microphone was found.")? };
    let supported = device.default_input_config().map_err(|_| "Could not set up the microphone.")?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let peak = Arc::new(AtomicU32::new(0));
    let received = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));
    let error_flag = failed.clone();
    let on_error = move |_| { error_flag.store(true, Ordering::Relaxed); };
    let result_peak = peak.clone();
    let result_received = received.clone();
    let update = move |value: f32| {
        received.store(true, Ordering::Relaxed);
        if value.is_finite() { peak.fetch_max((value.abs().min(1.0) * 100.0).round() as u32, Ordering::Relaxed); }
    };
    let stream = match format {
        SampleFormat::F32 => device.build_input_stream(&config, move |data: &[f32], _| { for &sample in data { update(sample); } }, on_error, None),
        SampleFormat::I16 => device.build_input_stream(&config, move |data: &[i16], _| { for &sample in data { update(sample as f32 / 32768.0); } }, on_error, None),
        SampleFormat::U16 => device.build_input_stream(&config, move |data: &[u16], _| { for &sample in data { update((sample as f32 - 32768.0) / 32768.0); } }, on_error, None),
        _ => return Err("This microphone format is not supported yet.".into()),
    }.map_err(|_| "Could not open the microphone. Check the Windows microphone permissions.")?;
    stream.play().map_err(|_| "Could not start the microphone.")?;
    std::thread::sleep(std::time::Duration::from_secs(3));
    drop(stream);
    if failed.load(Ordering::Relaxed) { return Err("The microphone test failed.".into()); }
    if !result_received.load(Ordering::Relaxed) { return Err("The microphone sent no sound during the test.".into()); }
    Ok(result_peak.load(Ordering::Relaxed))
}
fn output_device(selected: Option<String>) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    if let Some(name) = selected {
        let mut matching = host.output_devices().map_err(|_| "Could not list speakers.")?
            .filter(|device| device.name().ok().as_deref() == Some(name.as_str()));
        let device = matching.next().ok_or("The selected device is no longer available.")?;
        if matching.next().is_some() { return Err("Several devices share that name. Use the Windows default instead.")?; }
        Ok(device)
    } else {
        Ok(host.default_output_device().ok_or("No default speakers were found.")?)
    }
}

fn fill<T>(data: &mut [T], channels: usize, mut next: impl FnMut() -> f32, convert: impl Fn(f32) -> T) where T: Copy {
    for frame in data.chunks_mut(channels) { let value = convert(next()); frame.fill(value); }
}

/// The same file the web app plays: 24 kHz mono 16-bit PCM after a 44-byte header.
/// It is read in place, so ringing allocates no audio buffer.
static RINGTONE: &[u8] = include_bytes!("../../frontend/public/sounds/ringtone.wav");
const RINGTONE_RATE: f64 = 24_000.0;

fn ringtone_sample(index: usize) -> f32 {
    let at = 44 + index * 2;
    f32::from(i16::from_le_bytes([RINGTONE[at], RINGTONE[at + 1]])) / 32768.0
}

/// Loops the ringtone at any device rate.
struct RingtonePlayer { position: f64, step: f64, length: usize, level: f32 }
impl RingtonePlayer {
    fn new(device_rate: f64, level: f32) -> Self { Self { position: 0.0, step: RINGTONE_RATE / device_rate, length: (RINGTONE.len() - 44) / 2, level } }
    fn next_sample(&mut self) -> f32 {
        let index = self.position as usize;
        let blend = (self.position - index as f64) as f32;
        let value = ringtone_sample(index) * (1.0 - blend) + ringtone_sample((index + 1) % self.length) * blend;
        self.position += self.step;
        if self.position >= self.length as f64 { self.position -= self.length as f64; }
        value * self.level
    }
}

/// Rings until dropped. A speaker problem must never disturb the incoming call,
/// so failures just leave it silent; the call screen still shows.
pub struct Ringtone { _stop: Option<std::sync::mpsc::Sender<()>> }
impl Ringtone {
    pub fn silent() -> Self { Self { _stop: None } }
    pub fn start(selected: Option<String>, level: f32) -> Self {
        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        // The stream lives on its own thread because it cannot move between threads.
        std::thread::spawn(move || {
            let Ok(stream) = open_ringtone(selected, level) else { return };
            if stream.play().is_ok() { let _ = stopped.recv(); }
        });
        Self { _stop: Some(stop) }
    }
}

fn open_ringtone(selected: Option<String>, level: f32) -> Result<cpal::Stream, String> {
    use cpal::{SampleFormat, StreamConfig};
    let device = output_device(selected)?;
    let supported = device.default_output_config().map_err(|_| "Could not set up your speakers.")?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let channels = usize::from(config.channels);
    if config.sample_rate.0 == 0 || channels == 0 { return Err("Invalid audio configuration.".into()); }
    let mut player = RingtonePlayer::new(f64::from(config.sample_rate.0), level);
    let on_error = |_error| {};
    match format {
        SampleFormat::F32 => device.build_output_stream(&config, move |data: &mut [f32], _| fill(data, channels, || player.next_sample(), |v| v), on_error, None),
        SampleFormat::I16 => device.build_output_stream(&config, move |data: &mut [i16], _| fill(data, channels, || player.next_sample(), |v| (v * i16::MAX as f32) as i16), on_error, None),
        SampleFormat::U16 => device.build_output_stream(&config, move |data: &mut [u16], _| fill(data, channels, || player.next_sample(), |v| ((v * 0.5 + 0.5) * u16::MAX as f32) as u16), on_error, None),
        _ => return Err("This speaker format is not supported yet.".into()),
    }.map_err(|_| "Could not open your speakers.".to_string())
}

#[cfg(test)]
mod ringtone_tests {
    use super::*;
    #[test]
    fn ringtone_is_the_expected_format_and_loops_cleanly() {
        assert_eq!(&RINGTONE[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(RINGTONE[24..28].try_into().unwrap()), 24_000);
        assert_eq!(u16::from_le_bytes(RINGTONE[22..24].try_into().unwrap()), 1, "mono");
        assert_eq!(u16::from_le_bytes(RINGTONE[34..36].try_into().unwrap()), 16, "16-bit");
        let mut player = RingtonePlayer::new(48_000.0, 1.0);
        let mut peak = 0f32;
        // Three times through at a typical device rate, crossing the seam twice.
        for _ in 0..(player.length * 6) { peak = peak.max(player.next_sample().abs()); }
        assert!(peak > 0.3 && peak <= 0.51, "peak {peak}");
        assert!((ringtone_sample(0) - ringtone_sample(player.length - 1)).abs() < 0.05, "no click at the loop point");
    }
}

// Play a short, quiet locally generated tone on the system default output.
// No input stream is created and the stream is dropped after playback.
pub fn test_output(selected: Option<String>) -> Result<(), String> {
    use cpal::{SampleFormat, StreamConfig};
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    let device = output_device(selected)?;
    let supported = device.default_output_config().map_err(|_| "Could not set up your speakers.")?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let sample_rate = config.sample_rate.0 as f32;
    let channels = usize::from(config.channels);
    if sample_rate <= 0.0 || channels == 0 { return Err("Invalid audio configuration.".into()); }
    let failed = Arc::new(AtomicBool::new(false));
    let error_flag = failed.clone();
    let on_error = move |_error| { error_flag.store(true, Ordering::Relaxed); };
    let played = Arc::new(AtomicBool::new(false));
    let played_flag = played.clone();
    let mut frame: u64 = 0;
    let next = move || {
        played_flag.store(true, Ordering::Relaxed);
        let seconds = frame as f32 / sample_rate;
        frame += 1;
        let fade = (seconds * 20.0).min(1.0).min(((1.0 - seconds) * 20.0).max(0.0));
        (seconds * 440.0 * std::f32::consts::TAU).sin() * 0.08 * fade
    };
    let stream = match format {
        SampleFormat::F32 => { let mut next = next; device.build_output_stream(&config, move |data: &mut [f32], _| fill(data, channels, &mut next, |v| v), on_error, None) }
        SampleFormat::I16 => { let mut next = next; device.build_output_stream(&config, move |data: &mut [i16], _| fill(data, channels, &mut next, |v| (v * i16::MAX as f32) as i16), on_error, None) }
        SampleFormat::U16 => { let mut next = next; device.build_output_stream(&config, move |data: &mut [u16], _| fill(data, channels, &mut next, |v| ((v * 0.5 + 0.5) * u16::MAX as f32) as u16), on_error, None) }
        _ => return Err("This speaker format is not supported yet.".into()),
    }.map_err(|_| "Could not open your speakers.")?;
    stream.play().map_err(|_| "Could not play the test sound.")?;
    std::thread::sleep(std::time::Duration::from_millis(1100));
    drop(stream);
    if failed.load(Ordering::Relaxed) { return Err("The speaker test failed.".into()); }
    if !played.load(Ordering::Relaxed) { return Err("The device did not play the test sound.".into()); }
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "Requires an actual Windows audio host"]
    fn system_device_enumeration_does_not_open_audio_streams() {
        let devices = super::enumerate().expect("Windows audio enumeration failed");
        assert!(devices.inputs.len() < 10000 && devices.outputs.len() < 10000);
    }
}
