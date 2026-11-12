//! Device/Opus bridge for native voice. No audio is written to disk.
//! This module is not yet connected to signaling or enabled in the UI.
#![allow(dead_code)]
use crate::voice_codec::{VoiceDecoder, VoiceEncoder, FRAME_SAMPLES, SAMPLE_RATE};
use cpal::{traits::{DeviceTrait, StreamTrait}, SampleFormat, StreamConfig, SupportedStreamConfig};
use std::{collections::{HashMap, VecDeque}, sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}, mpsc::{self, Receiver, SyncSender}}, thread::{self, JoinHandle}, time::Duration};
use zeroize::{Zeroize, Zeroizing};

type Frame = (u64, Zeroizing<Vec<f32>>);
/// Diagnostics: loudest captured sample, in millionths of full scale.
pub static CAPTURE_PEAK: AtomicU64 = AtomicU64::new(0);
/// Loudness of the latest captured frame (RMS, millionths of full scale);
/// drives the "your mic is picking up sound" indicator.
pub static MIC_LEVEL: AtomicU64 = AtomicU64::new(0);
/// Mirrors the Voice setting, so changing it applies during a call.
pub static NOISE_SUPPRESSION: AtomicBool = AtomicBool::new(true);

/// DeepFilterNet3. The web app runs this same engine compiled to WebAssembly.
pub struct Denoiser { engine: deep_filter_rt::tract::DfTract, input: ndarray::Array2<f32>, output: ndarray::Array2<f32> }
impl Denoiser {
    pub fn new() -> Self { Self { engine: deep_filter_rt::tract::DfTract::default(), input: ndarray::Array2::zeros((1, 480)), output: ndarray::Array2::zeros((1, 480)) } }
    /// Cleans samples in place. Each 480-sample block is one 10 ms model frame.
    pub fn process(&mut self, samples: &mut [f32]) {
        for block in samples.chunks_exact_mut(480) {
            self.input.row_mut(0).iter_mut().zip(block.iter()).for_each(|(to, from)| *to = *from);
            if self.engine.process(self.input.view(), self.output.view_mut()).is_err() { continue; }
            block.iter_mut().zip(self.output.row(0).iter()).for_each(|(to, from)| *to = from.clamp(-1.0, 1.0));
        }
    }
}
impl Default for Denoiser { fn default() -> Self { Self::new() } }
type Packet = (u64, Zeroizing<Vec<u8>>);

pub struct VoiceAudio {
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
    deafened: Arc<AtomicBool>,
    capture_epoch: Arc<AtomicU64>,
    playback_epoch: Arc<AtomicU64>,
    // Behind a mutex so the audio bridge can be shared across await points.
    packets: Mutex<Receiver<Packet>>,
    mixer: Arc<Mixer>,
    // One decoder per remote speaker (a direct call uses source 0).
    decoders: Mutex<HashMap<u64, VoiceDecoder>>,
    worker: Option<JoinHandle<()>>,
}

impl VoiceAudio {
    /// Opens devices only when called; selected names must resolve uniquely.
    pub fn start(input: Option<String>, output: Option<String>) -> Result<Self, String> {
        let decoder = VoiceDecoder::new().map_err(str::to_owned)?;
        let encoder = VoiceEncoder::new().map_err(str::to_owned)?;
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let muted = Arc::new(AtomicBool::new(false));
        let deafened = Arc::new(AtomicBool::new(false));
        let capture_epoch = Arc::new(AtomicU64::new(0));
        let playback_epoch = Arc::new(AtomicU64::new(0));
        let (capture_tx, capture_rx) = mpsc::sync_channel::<Frame>(5);
        let (packet_tx, packets) = mpsc::sync_channel::<Packet>(5);
        let mixer = Arc::new(Mixer::default());
        let worker_mixer = mixer.clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker_stop = stop.clone(); let worker_failed = failed.clone();
        let worker_muted = muted.clone(); let worker_deafened = deafened.clone();
        let worker_capture_epoch = capture_epoch.clone(); let worker_playback_epoch = playback_epoch.clone();
        let worker = thread::spawn(move || {
            let streams = open_streams(input, output, capture_tx, worker_mixer, worker_failed.clone(), worker_muted.clone(), worker_deafened, worker_capture_epoch.clone(), worker_playback_epoch);
            let (input_stream, output_stream) = match streams {
                Ok(streams) => streams,
                Err(error) => { let _ = ready_tx.send(Err(error)); return; }
            };
            if input_stream.play().is_err() || output_stream.play().is_err() { let _ = ready_tx.send(Err("Could not start your audio devices.".into())); return; }
            if ready_tx.send(Ok(())).is_err() { return; }
            let mut encoder = encoder;
            let mut denoiser = Denoiser::new();
            while !worker_stop.load(Ordering::Acquire) && !worker_failed.load(Ordering::Acquire) {
                match capture_rx.recv_timeout(Duration::from_millis(50)) {
                    Ok((epoch, mut frame)) if !worker_muted.load(Ordering::Acquire) && epoch == worker_capture_epoch.load(Ordering::Acquire) => {
                        if NOISE_SUPPRESSION.load(Ordering::Relaxed) { denoiser.process(&mut frame); }
                        match encoder.encode(&frame) {
                            Ok(packet) => { let _ = packet_tx.try_send((epoch, packet)); }
                            Err(_) => { worker_failed.store(true, Ordering::Release); }
                        }
                    }
                    Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            drop(input_stream); drop(output_stream);
        });
        match ready_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(Self { stop, failed, muted, deafened, capture_epoch, playback_epoch, packets: Mutex::new(packets), mixer, decoders: Mutex::new(HashMap::from([(0, decoder)])), worker: Some(worker) }),
            Ok(Err(error)) => { let _ = worker.join(); Err(error) }
            Err(_) => { stop.store(true, Ordering::Release); Err("Your audio devices did not respond.".into()) }
        }
    }
    /// Development builds only: an audio bridge without devices, for automated
    /// connectivity checks that must not open the user's microphone.
    #[cfg(debug_assertions)]
    pub fn silent() -> Result<Self, String> {
        let (_, packets) = mpsc::sync_channel::<Packet>(1);
        Ok(Self { stop: Arc::new(AtomicBool::new(false)), failed: Arc::new(AtomicBool::new(false)), muted: Arc::new(AtomicBool::new(false)), deafened: Arc::new(AtomicBool::new(false)),
            capture_epoch: Arc::new(AtomicU64::new(0)), playback_epoch: Arc::new(AtomicU64::new(0)), packets: Mutex::new(packets), mixer: Arc::new(Mixer::default()),
            decoders: Mutex::new(HashMap::from([(0, VoiceDecoder::new().map_err(str::to_owned)?)])), worker: None })
    }
    /// Opens real devices, or none in development checks (SONAR_DEV_SILENT_AUDIO).
    pub fn open(input: Option<String>, output: Option<String>) -> Result<Self, String> {
        #[cfg(debug_assertions)]
        if std::env::var_os("SONAR_DEV_SILENT_AUDIO").is_some() { return Self::silent(); }
        Self::start(input, output)
    }
    pub fn set_muted(&self, muted: bool) {
        if muted { self.muted.store(true, Ordering::Release); }
        self.capture_epoch.fetch_add(1, Ordering::AcqRel);
        if !muted { self.muted.store(false, Ordering::Release); }
    }
    pub fn set_deafened(&self, deafened: bool) {
        if deafened { self.deafened.store(true, Ordering::Release); }
        self.playback_epoch.fetch_add(1, Ordering::AcqRel);
        if !deafened { self.deafened.store(false, Ordering::Release); }
    }
    pub fn failed(&self) -> bool { self.failed.load(Ordering::Acquire) }
    /// Invalidate audio queued before a transport interruption without changing
    /// the user's mute or deafen choices.
    pub fn discard_pending(&self) {
        self.capture_epoch.fetch_add(1, Ordering::AcqRel);
        self.playback_epoch.fetch_add(1, Ordering::AcqRel);
        if let Ok(packets) = self.packets.lock() { while packets.try_recv().is_ok() {} }
    }
    /// Called by the transport; queued frames from before mute are discarded.
    pub fn next_packet(&self) -> Option<Zeroizing<Vec<u8>>> {
        let packets = self.packets.lock().ok()?;
        while let Ok((epoch, packet)) = packets.try_recv() {
            if !self.muted.load(Ordering::Acquire) && epoch == self.capture_epoch.load(Ordering::Acquire) { return Some(packet); }
        }
        None
    }
    /// Drops audio instead of growing memory or blocking the network task.
    pub fn play_packet(&self, packet: &[u8]) -> Result<(), &'static str> { self.play_from(0, packet) }
    /// Decodes audio from one remote speaker; speakers are mixed on output.
    pub fn play_from(&self, source: u64, packet: &[u8]) -> Result<(), &'static str> {
        if self.deafened.load(Ordering::Acquire) { return Ok(()); }
        let epoch = self.playback_epoch.load(Ordering::Acquire);
        let frame = {
            let mut decoders = self.decoders.lock().map_err(|_| "decoder unavailable")?;
            if !decoders.contains_key(&source) {
                if decoders.len() >= 8 { return Err("too many speakers"); }
                decoders.insert(source, VoiceDecoder::new()?);
            }
            decoders.get_mut(&source).ok_or("decoder unavailable")?.decode(packet)?
        };
        self.mixer.push(source, epoch, &frame);
        Ok(())
    }
    /// Forgets a speaker who left, releasing its decoder and queued audio.
    pub fn remove_source(&self, source: u64) {
        if let Ok(mut decoders) = self.decoders.lock() { decoders.remove(&source); }
        self.mixer.remove(source);
    }
}
impl Drop for VoiceAudio {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        MIC_LEVEL.store(0, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

/// Output-only playback of the sound in a shared screen.
pub struct ScreenPlayback { mixer: Arc<Mixer>, deafened: Arc<AtomicBool>, stop: Arc<AtomicBool>, volumes: Mutex<HashMap<u64, f32>> }
impl ScreenPlayback {
    pub fn open(output: Option<String>) -> Result<Self, String> {
        let (mixer, deafened, stop) = (Arc::new(Mixer::default()), Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
        #[cfg(debug_assertions)]
        if std::env::var_os("SONAR_DEV_SILENT_AUDIO").is_some() { return Ok(Self { mixer, deafened, stop, volumes: Mutex::default() }); }
        let (thread_mixer, thread_deafened, thread_stop) = (mixer.clone(), deafened.clone(), stop.clone());
        thread::spawn(move || {
            let Ok(device) = select_device(false, output) else { return };
            let Ok(supported) = select_config(&device, false) else { return };
            let format = supported.sample_format();
            let config: StreamConfig = supported.into();
            let channels = usize::from(config.channels);
            let mut renderer = PlaybackRenderer::new(thread_mixer, thread_deafened, Arc::new(AtomicU64::new(0)));
            let stream = match format {
                SampleFormat::F32 => device.build_output_stream(&config, move |data: &mut [f32], _| renderer.fill(data, channels, |v| v), |_| {}, None),
                SampleFormat::I16 => device.build_output_stream(&config, move |data: &mut [i16], _| renderer.fill(data, channels, |v| (v * i16::MAX as f32) as i16), |_| {}, None),
                SampleFormat::U16 => device.build_output_stream(&config, move |data: &mut [u16], _| renderer.fill(data, channels, |v| ((v * 0.5 + 0.5) * u16::MAX as f32) as u16), |_| {}, None),
                _ => return,
            };
            let Ok(stream) = stream else { return };
            if stream.play().is_err() { return; }
            while !thread_stop.load(Ordering::Acquire) { thread::sleep(Duration::from_millis(100)); }
        });
        Ok(Self { mixer, deafened, stop, volumes: Mutex::default() })
    }
    pub fn play(&self, source: u64, frame: &[f32]) {
        let volume = self.volumes.lock().ok().and_then(|v| v.get(&source).copied()).unwrap_or(1.0);
        if volume >= 1.0 { self.mixer.push(source, 0, frame); return; }
        let quieter: Vec<f32> = frame.iter().map(|sample| sample * volume).collect();
        self.mixer.push(source, 0, &quieter);
    }
    /// One viewer's volume for one stream; nobody else is affected.
    pub fn set_volume(&self, source: u64, volume: f32) { if let Ok(mut volumes) = self.volumes.lock() { volumes.insert(source, volume.clamp(0.0, 1.0)); } }
    pub fn remove(&self, source: u64) { self.mixer.remove(source); }
    /// Samples of this stream still waiting to be played.
    pub fn queued(&self, source: u64) -> usize { self.mixer.sources.lock().ok().and_then(|s| s.get(&source).map(|(_, queue)| queue.len())).unwrap_or(0) }
    pub fn set_deafened(&self, value: bool) { self.deafened.store(value, Ordering::Release); }
}
impl Drop for ScreenPlayback { fn drop(&mut self) { self.stop.store(true, Ordering::Release); } }

fn select_device(input: bool, selected: Option<String>) -> Result<cpal::Device, String> { pick_device(input, selected) }
/// The microphone or speakers chosen in settings, found by name. When that device is
/// unplugged, renamed by Windows (wireless headsets come back as "3- …"), or several
/// share the name, the Windows default is used instead of refusing to join a call.
pub(crate) fn pick_device(input: bool, selected: Option<String>) -> Result<cpal::Device, String> {
    use cpal::traits::HostTrait;
    let host = cpal::default_host();
    if let Some(name) = selected {
        // Tests need a selection that always fails, to check a failed call start cleans up.
        #[cfg(test)]
        if name == "sonar-test-nonexistent-microphone" { return Err("Test microphone".into()); }
        if let Ok(devices) = if input { host.input_devices() } else { host.output_devices() } {
            let matching: Vec<cpal::Device> = devices.filter(|device| device.name().ok().as_deref() == Some(name.as_str())).collect();
            if matching.len() == 1 { if let Some(device) = matching.into_iter().next() { return Ok(device); } }
        }
    }
    if input { host.default_input_device().ok_or_else(|| "No microphone was found. Plug one in or check Windows sound settings.".into()) }
    else { host.default_output_device().ok_or_else(|| "No speakers were found. Plug some in or check Windows sound settings.".into()) }
}
fn select_config(device: &cpal::Device, input: bool) -> Result<SupportedStreamConfig, String> {
    let ranges: Vec<_> = if input { device.supported_input_configs().map(|r| r.collect()) } else { device.supported_output_configs().map(|r| r.collect()) }.map_err(|_| "Could not read the audio formats of this device.")?;
    ranges.into_iter().filter(|range| range.min_sample_rate().0 <= SAMPLE_RATE && range.max_sample_rate().0 >= SAMPLE_RATE && range.channels() > 0 && range.channels() <= 8 && matches!(range.sample_format(), SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16))
        .min_by_key(|range| (usize::from(range.channels().abs_diff(if input { 1 } else { 2 })), if range.sample_format() == SampleFormat::F32 { 0 } else { 1 }))
        .map(|range| range.with_sample_rate(cpal::SampleRate(SAMPLE_RATE)))
        .ok_or_else(|| "This device does not support 48 kHz audio.".into())
}

#[allow(clippy::too_many_arguments)]
fn open_streams(input: Option<String>, output: Option<String>, capture: SyncSender<Frame>, mixer: Arc<Mixer>, failed: Arc<AtomicBool>, muted: Arc<AtomicBool>, deafened: Arc<AtomicBool>, capture_epoch: Arc<AtomicU64>, playback_epoch: Arc<AtomicU64>) -> Result<(cpal::Stream, cpal::Stream), String> {
    let microphone = select_device(true, input)?; let speakers = select_device(false, output)?;
    let input_supported = select_config(&microphone, true)?; let output_supported = select_config(&speakers, false)?;
    let input_format = input_supported.sample_format(); let output_format = output_supported.sample_format();
    let input_config: StreamConfig = input_supported.into(); let output_config: StreamConfig = output_supported.into();
    let input_channels = usize::from(input_config.channels); let output_channels = usize::from(output_config.channels);
    let mut accumulator = CaptureAccumulator::new(input_channels, capture, muted, capture_epoch);
    let input_failed = failed.clone();
    let on_input_error = move |_| { input_failed.store(true, Ordering::Release); };
    let input_stream = match input_format {
        SampleFormat::F32 => microphone.build_input_stream(&input_config, move |data: &[f32], _| accumulator.push(data.iter().copied()), on_input_error, None),
        SampleFormat::I16 => microphone.build_input_stream(&input_config, move |data: &[i16], _| accumulator.push(data.iter().map(|v| *v as f32 / 32768.0)), on_input_error, None),
        SampleFormat::U16 => microphone.build_input_stream(&input_config, move |data: &[u16], _| accumulator.push(data.iter().map(|v| (*v as f32 - 32768.0) / 32768.0)), on_input_error, None),
        _ => unreachable!(),
    }.map_err(|_| "Could not open the microphone. Check the Windows microphone permissions.")?;
    let mut renderer = PlaybackRenderer::new(mixer, deafened, playback_epoch);
    let on_output_error = move |_| { failed.store(true, Ordering::Release); };
    let output_stream = match output_format {
        SampleFormat::F32 => speakers.build_output_stream(&output_config, move |data: &mut [f32], _| renderer.fill(data, output_channels, |v| v), on_output_error, None),
        SampleFormat::I16 => speakers.build_output_stream(&output_config, move |data: &mut [i16], _| renderer.fill(data, output_channels, |v| (v * i16::MAX as f32) as i16), on_output_error, None),
        SampleFormat::U16 => speakers.build_output_stream(&output_config, move |data: &mut [u16], _| renderer.fill(data, output_channels, |v| ((v * 0.5 + 0.5) * u16::MAX as f32) as u16), on_output_error, None),
        _ => unreachable!(),
    }.map_err(|_| "Could not open your speakers.")?;
    Ok((input_stream, output_stream))
}

struct CaptureAccumulator { channels: usize, sum: f32, count: usize, frame: Zeroizing<Vec<f32>>, epoch: u64, tx: SyncSender<Frame>, muted: Arc<AtomicBool>, current: Arc<AtomicU64> }
impl CaptureAccumulator {
    fn new(channels: usize, tx: SyncSender<Frame>, muted: Arc<AtomicBool>, current: Arc<AtomicU64>) -> Self { Self { channels, sum: 0.0, count: 0, frame: Zeroizing::new(Vec::with_capacity(FRAME_SAMPLES)), epoch: current.load(Ordering::Acquire), tx, muted, current } }
    fn push(&mut self, samples: impl Iterator<Item = f32>) {
        for sample in samples {
            let epoch = self.current.load(Ordering::Acquire);
            if epoch != self.epoch || self.muted.load(Ordering::Acquire) { self.frame.zeroize(); self.sum = 0.0; self.count = 0; self.epoch = epoch; if self.muted.load(Ordering::Acquire) { MIC_LEVEL.store(0, Ordering::Relaxed); continue; } }
            let value = if sample.is_finite() { sample.clamp(-1.0, 1.0) } else { 0.0 };
            CAPTURE_PEAK.fetch_max((value.abs() * 1_000_000.0) as u64, Ordering::Relaxed);
            self.sum += value;
            self.count += 1;
            if self.count == self.channels { self.frame.push((self.sum / self.channels as f32).clamp(-1.0, 1.0)); self.sum = 0.0; self.count = 0; }
            if self.frame.len() == FRAME_SAMPLES {
                let energy: f32 = self.frame.iter().map(|v| v * v).sum::<f32>() / FRAME_SAMPLES as f32;
                MIC_LEVEL.store((energy.sqrt() * 1_000_000.0) as u64, Ordering::Relaxed);
                let frame = std::mem::replace(&mut self.frame, Zeroizing::new(Vec::with_capacity(FRAME_SAMPLES)));
                let _ = self.tx.try_send((self.epoch, frame));
            }
        }
    }
}
/// Per-speaker queues of decoded mono samples. At most 250 ms is kept per
/// speaker so a stalled output never grows memory or latency.
#[derive(Default)]
pub struct Mixer { sources: Mutex<HashMap<u64, (u64, VecDeque<f32>)>> }
const MAX_QUEUED: usize = (SAMPLE_RATE as usize) / 4;
impl Mixer {
    fn push(&self, source: u64, epoch: u64, frame: &[f32]) {
        let Ok(mut sources) = self.sources.lock() else { return };
        let entry = sources.entry(source).or_insert_with(|| (epoch, VecDeque::with_capacity(FRAME_SAMPLES * 4)));
        if entry.0 != epoch { entry.0 = epoch; entry.1.clear(); }
        entry.1.extend(frame.iter().map(|v| if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 }));
        let excess = entry.1.len().saturating_sub(MAX_QUEUED);
        entry.1.drain(..excess);
    }
    fn remove(&self, source: u64) { if let Ok(mut sources) = self.sources.lock() { sources.remove(&source); } }
}
struct PlaybackRenderer { mixer: Arc<Mixer>, deafened: Arc<AtomicBool>, current: Arc<AtomicU64> }
impl PlaybackRenderer {
    fn new(mixer: Arc<Mixer>, deafened: Arc<AtomicBool>, current: Arc<AtomicU64>) -> Self { Self { mixer, deafened, current } }
    fn fill<T: Copy>(&mut self, data: &mut [T], channels: usize, convert: impl Fn(f32) -> T) {
        let Ok(mut sources) = self.mixer.sources.try_lock() else { for frame in data.chunks_mut(channels) { frame.fill(convert(0.0)); } return };
        if self.deafened.load(Ordering::Acquire) { sources.values_mut().for_each(|(_, queue)| queue.clear()); }
        let epoch = self.current.load(Ordering::Acquire);
        sources.retain(|_, (source_epoch, _)| *source_epoch == epoch);
        for frame in data.chunks_mut(channels) {
            let mixed: f32 = sources.values_mut().filter_map(|(_, queue)| queue.pop_front()).sum();
            frame.fill(convert(mixed.clamp(-1.0, 1.0)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_downmixes_and_mute_discards_old_frames() {
        let (tx, rx) = mpsc::sync_channel(1);
        let muted = Arc::new(AtomicBool::new(false)); let epoch = Arc::new(AtomicU64::new(0));
        let mut capture = CaptureAccumulator::new(2, tx, muted.clone(), epoch.clone());
        capture.push(std::iter::repeat_n(0.5, FRAME_SAMPLES * 2));
        let (generation, frame) = rx.try_recv().unwrap(); assert_eq!(generation, 0); assert!(frame.iter().all(|v| *v == 0.5));
        assert!(MIC_LEVEL.load(Ordering::Relaxed) > 400_000, "a loud frame registers as sound");
        capture.push(std::iter::repeat_n(0.8, 100)); muted.store(true, Ordering::Release); epoch.fetch_add(1, Ordering::AcqRel);
        capture.push(std::iter::repeat_n(1.0, FRAME_SAMPLES * 2)); assert!(rx.try_recv().is_err());
        assert_eq!(MIC_LEVEL.load(Ordering::Relaxed), 0, "a muted mic shows no sound");
        muted.store(false, Ordering::Release); epoch.fetch_add(1, Ordering::AcqRel);
        capture.push(std::iter::repeat_n(0.2, FRAME_SAMPLES * 2));
        let (generation, frame) = rx.try_recv().unwrap(); assert_eq!(generation, 2); assert!(frame.iter().all(|v| *v == 0.2));
    }
    #[test]
    fn playback_mixes_speakers_and_discards_stale_frames_after_deafen() {
        let mixer = Arc::new(Mixer::default()); let deafened = Arc::new(AtomicBool::new(false)); let epoch = Arc::new(AtomicU64::new(0));
        let mut renderer = PlaybackRenderer::new(mixer.clone(), deafened.clone(), epoch.clone());
        mixer.push(1, 0, &[0.25; 3]); mixer.push(2, 0, &[0.5; 2]);
        let mut out = [0.0; 3]; renderer.fill(&mut out, 1, |v| v); assert_eq!(out, [0.75, 0.75, 0.25]);
        mixer.push(1, 0, &[0.9; 4]); mixer.push(2, 0, &[0.9; 4]);
        let mut out = [0.0; 1]; renderer.fill(&mut out, 1, |v| v); assert_eq!(out, [1.0], "mix is clamped");
        deafened.store(true, Ordering::Release); epoch.fetch_add(1, Ordering::AcqRel);
        let mut out = [1.0; 2]; renderer.fill(&mut out, 1, |v| v); assert_eq!(out, [0.0, 0.0]);
        deafened.store(false, Ordering::Release); epoch.fetch_add(1, Ordering::AcqRel);
        mixer.push(1, 2, &[0.1]); let mut stereo = [0.0; 2]; renderer.fill(&mut stereo, 2, |v| v); assert_eq!(stereo, [0.1, 0.1]);
        mixer.push(1, 2, &vec![0.1; MAX_QUEUED * 2]); assert_eq!(mixer.sources.lock().unwrap()[&1].1.len(), MAX_QUEUED);
        mixer.remove(1); assert!(mixer.sources.lock().unwrap().is_empty());
    }
}

#[cfg(test)]
mod denoiser_tests {
    use super::*;
    #[test]
    fn steady_noise_is_removed() {
        let mut denoiser = Denoiser::new();
        let mut seed = 99u32;
        let mut frame = |quiet: &mut f32, loud: &mut f32, measure: bool| {
            let mut samples: Vec<f32> = (0..960).map(|_| { seed = seed.wrapping_mul(1664525).wrapping_add(1013904223); ((seed >> 8) as f32 / 8388608.0 - 1.0) * 0.05 }).collect();
            if measure { *loud += samples.iter().map(|v| v * v).sum::<f32>(); }
            denoiser.process(&mut samples);
            if measure { *quiet += samples.iter().map(|v| v * v).sum::<f32>(); }
        };
        let (mut quiet, mut loud) = (0f32, 0f32);
        for n in 0..200 { frame(&mut quiet, &mut loud, n >= 100); }
        assert!(quiet < loud * 0.01, "noise energy {quiet} of {loud}");
    }
}
