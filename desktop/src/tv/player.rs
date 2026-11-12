//! Plays one channel for a TV share. On its own thread it fetches the channel,
//! keeps everything on the stream's own clock, and hands out:
//! - the provider's original H.264 access units, untouched, for viewers that can take them;
//! - a converted H.264 copy (no B-frames) only while some viewer needs one;
//! - Opus sound for the call, plus the picture and sound for the person sharing.
use super::audio::TvAudio;
use super::demux::{Unit, VideoCodec};
use super::fetch::{self, Event};
use crate::screen::{Mailbox, Picture};
use crate::screen_decode::{nv12_to_rgba, Decoder};
use crate::screen_media::FrameEncoder;
use crate::voice_audio::ScreenPlayback;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use zeroize::Zeroizing;

/// Shared between the player thread and the screen-share loop.
#[derive(Default)]
pub struct Shared {
    pub stop: Arc<AtomicBool>,
    /// Set by the share loop while at least one viewer needs the converted copy.
    pub want_converted: AtomicBool,
    /// A viewer of the converted copy just joined and needs a full picture.
    pub force_key: AtomicBool,
    /// Browsers can take the original: H.264 with no B-frames (decided after the first frames).
    pub web_native: AtomicBool,
    /// The original can be forwarded at all (H.264; H.265 is converted for everyone for now).
    pub native: AtomicBool,
}

#[derive(Debug)]
pub enum Out {
    Native(Vec<u8>, Duration),
    Converted(Vec<u8>, Duration),
    Audio(Vec<u8>),
    /// For the call bar, such as "1080p50 · H.264".
    Info(String),
    Failed(String),
    Ended,
}

/// Where the person sharing sees and hears the channel.
pub struct Local { pub pictures: Mailbox, pub key: String, pub playback: Option<Arc<ScreenPlayback>>, pub source: u64, pub notify: Box<dyn Fn() + Send> }

const TICKS: u64 = 90_000;
/// The stream clock runs a little behind arrival so bursts of segments play smoothly.
const LEAD: Duration = Duration::from_millis(700);

pub fn start(address: Zeroizing<String>, shared: Arc<Shared>, out: mpsc::Sender<Out>, local: Local) {
    let (events_tx, events) = mpsc::channel::<Event>(4);
    tokio::spawn(fetch::run(address, events_tx, shared.stop.clone()));
    std::thread::Builder::new().name("sonar-tv".into()).spawn(move || Worker::new(shared, out, local).run(events)).ok();
}

struct Clock { wall: Instant, ts: u64 }

struct Worker {
    shared: Arc<Shared>, out: mpsc::Sender<Out>, local: Local,
    clock: Option<Clock>, last_ts: Option<u64>, frame_ticks: u64,
    codec: Option<VideoCodec>, decoder: Option<Decoder>, converter: Option<FrameEncoder>, converter_failed: bool,
    audio: Option<TvAudio>, audio_failed: bool,
    frames: u64, reordered: bool, announced: bool, converted_time: i64, rgba: Vec<u8>, mono: Vec<f32>,
}

impl Worker {
    fn new(shared: Arc<Shared>, out: mpsc::Sender<Out>, local: Local) -> Self {
        Self { shared, out, local, clock: None, last_ts: None, frame_ticks: 3600, codec: None, decoder: None, converter: None, converter_failed: false,
            audio: None, audio_failed: false, frames: 0, reordered: false, announced: false, converted_time: 0, rgba: Vec::new(), mono: Vec::new() }
    }
    fn stopped(&self) -> bool { self.shared.stop.load(Ordering::Acquire) }
    fn send(&self, item: Out) -> bool { !self.stopped() && self.out.blocking_send(item).is_ok() }

    fn run(mut self, mut events: mpsc::Receiver<Event>) {
        while !self.stopped() {
            let Some(event) = events.blocking_recv() else { break };
            match event {
                Event::Units(units) => for unit in units { if self.stopped() { break; } self.unit(unit); },
                Event::Ended => { self.drain(); self.send(Out::Ended); break; }
                Event::Failed(message) => { self.send(Out::Failed(message)); break; }
            }
        }
        self.shared.stop.store(true, Ordering::Release);
        if let Some(playback) = &self.local.playback { playback.remove(self.local.source); }
        if let Ok(mut pictures) = self.local.pictures.lock() { pictures.remove(&self.local.key); }
        (self.local.notify)();
    }

    /// Waits until a unit's time on the stream clock; re-anchors after jumps and long stalls.
    fn pace(&mut self, ts: u64) {
        let now = Instant::now();
        let clock = self.clock.get_or_insert(Clock { wall: now + LEAD, ts });
        let behind = |c: &Clock| ts < c.ts || ts - c.ts > TICKS * 3600;
        if behind(clock) { *clock = Clock { wall: now + LEAD, ts }; }
        let due = clock.wall + Duration::from_micros((ts - clock.ts) * 1_000_000 / TICKS);
        if due + Duration::from_secs(2) < now { *clock = Clock { wall: now, ts }; return; }
        while let Some(wait) = due.checked_duration_since(Instant::now()) {
            if self.stopped() { return; }
            std::thread::sleep(wait.min(Duration::from_millis(50)));
        }
    }

    fn unit(&mut self, unit: Unit) {
        match unit {
            Unit::Video { codec, pts, dts, data, .. } => {
                let Some(ts) = dts.or(pts) else { return };
                self.pace(ts);
                if let Some(last) = self.last_ts { let delta = ts.wrapping_sub(last); if (300..=9000).contains(&delta) { self.frame_ticks = delta; } }
                self.last_ts = Some(ts);
                if self.codec != Some(codec) { self.codec = Some(codec); self.decoder = None; self.converter = None; self.frames = 0; self.reordered = false; self.announced = false; }
                self.frames += 1;
                if pts.is_some() && dts.is_some() && pts != dts { self.reordered = true; }
                self.shared.native.store(codec == VideoCodec::H264, Ordering::Release);
                // Browsers show B-frames out of order, so they get the original only without them.
                self.shared.web_native.store(codec == VideoCodec::H264 && !self.reordered && self.frames >= 50, Ordering::Release);
                let duration = Duration::from_micros(self.frame_ticks * 1_000_000 / TICKS);
                if codec == VideoCodec::H264 && !self.send(Out::Native(data.clone(), duration)) { return; }
                self.picture(codec, &data, duration);
            }
            Unit::Audio { pts, sample_rate, channels, data, .. } => {
                if let Some(ts) = pts { if self.last_ts.is_none() { self.pace(ts); } }
                self.sound(sample_rate, channels, &data);
            }
        }
    }

    /// Decodes for the person sharing (and, when needed, for the converted copy).
    fn picture(&mut self, codec: VideoCodec, data: &[u8], duration: Duration) {
        if self.decoder.is_none() {
            match if codec == VideoCodec::H265 { Decoder::hevc() } else { Decoder::reordering() } {
                Ok(decoder) => self.decoder = Some(decoder),
                Err(message) => { self.send(Out::Failed(message)); self.shared.stop.store(true, Ordering::Release); return; }
            }
        }
        let mut decoder = self.decoder.take();
        if let Some(d) = decoder.as_mut() { let _ = d.decode(data, &mut |y, uv, stride, width, height| self.show(y, uv, stride, width, height, duration)); }
        self.decoder = decoder;
    }

    fn drain(&mut self) {
        let duration = Duration::from_micros(self.frame_ticks * 1_000_000 / TICKS);
        let mut decoder = self.decoder.take();
        if let Some(d) = decoder.as_mut() { let _ = d.drain(&mut |y, uv, stride, width, height| self.show(y, uv, stride, width, height, duration)); }
    }

    fn show(&mut self, y: &[u8], uv: &[u8], stride: usize, width: usize, height: usize, duration: Duration) {
        let fps = (TICKS as f64 / self.frame_ticks as f64).round() as u32;
        if !self.announced {
            self.announced = true;
            let codec = if self.codec == Some(VideoCodec::H265) { "H.265" } else { "H.264" };
            self.send(Out::Info(format!("{height}p{fps} · {codec}")));
        }
        nv12_to_rgba(y, uv, stride, width, height, &mut self.rgba);
        if let Ok(mut pictures) = self.local.pictures.lock() {
            let spare = pictures.remove(&self.local.key).map(|p| p.rgba).unwrap_or_default();
            pictures.insert(self.local.key.clone(), Picture { width: width as u32, height: height as u32, rgba: std::mem::replace(&mut self.rgba, spare) });
        }
        (self.local.notify)();
        if !self.shared.want_converted.load(Ordering::Acquire) || self.converter_failed { return; }
        if self.converter.is_none() {
            match FrameEncoder::new(width as u32, height as u32, fps) {
                Ok(encoder) => self.converter = Some(encoder),
                Err(message) => { self.converter_failed = true; self.send(Out::Info(format!("Browser viewers cannot watch this channel: {message}"))); return; }
            }
        }
        let Some(mut converter) = self.converter.take() else { return };
        if self.shared.force_key.swap(false, Ordering::AcqRel) { converter.request_key(); }
        let mut packets = Vec::new();
        let time = self.converted_time;
        self.converted_time += duration.as_nanos() as i64 / 100;
        let _ = converter.encode(y, uv, stride, time, &mut |packet, _| packets.push(packet.to_vec()));
        self.converter = Some(converter);
        for packet in packets { if !self.send(Out::Converted(packet, duration)) { return; } }
    }

    fn sound(&mut self, sample_rate: u32, channels: u8, data: &[u8]) {
        if self.audio_failed { return; }
        if self.audio.is_none() {
            match TvAudio::new(sample_rate, channels) {
                Ok(audio) => self.audio = Some(audio),
                Err(_) => { self.audio_failed = true; self.send(Out::Info("This channel's sound format is not supported; sharing the picture only.".into())); return; }
            }
        }
        let Some(mut audio) = self.audio.take() else { return };
        let (mut packets, mut mono) = (Vec::new(), std::mem::take(&mut self.mono));
        mono.clear();
        let _ = audio.push(data, &mut |stereo| mono.extend(stereo.chunks_exact(2).map(|s| (s[0] + s[1]) * 0.5)), &mut |packet| packets.push(packet));
        self.audio = Some(audio);
        if let Some(playback) = &self.local.playback { if !mono.is_empty() { playback.play(self.local.source, &mono); } }
        self.mono = mono;
        for packet in packets { if !self.send(Out::Audio(packet)) { return; } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::Mutex;

    /// A local "provider" serving the 1 s test channel; the player must hand out every
    /// original frame unchanged, Opus sound, a picture for the sharer, and the converted
    /// copy once asked for, at the stream's own pace.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plays_a_channel_natively_with_sound_picture_and_a_converted_copy() {
        let ts = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/tv/h264_high_50fps.ts")).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}/live/ana/pw/1.ts", listener.local_addr().unwrap());
        std::thread::spawn(move || for mut stream in listener.incoming().flatten() {
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", ts.len()).as_bytes());
            let _ = stream.write_all(&ts);
        });
        let shared = Arc::new(Shared::default());
        shared.want_converted.store(true, Ordering::Release);
        let pictures: Mailbox = Arc::new(Mutex::new(Default::default()));
        let shown = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = shown.clone();
        let (tx, mut rx) = mpsc::channel(64);
        let started = Instant::now();
        start(Zeroizing::new(address), shared.clone(), tx, Local { pictures: pictures.clone(), key: "me".into(), playback: None, source: 1, notify: Box::new(move || { counter.fetch_add(1, Ordering::Relaxed); }) });
        let (mut native, mut converted, mut audio, mut info, mut ended) = (Vec::new(), 0, 0, None, false);
        while let Ok(Some(item)) = tokio::time::timeout(Duration::from_secs(15), rx.recv()).await {
            match item {
                // The first frame's length is a guess; from the second on it is measured.
                Out::Native(data, duration) => { if !native.is_empty() { assert_eq!(duration, Duration::from_millis(20), "50 fps"); } native.push(data); }
                Out::Converted(..) => converted += 1,
                Out::Audio(packet) => { assert!(packet.len() > 50); audio += 1; }
                Out::Info(text) => info = Some(text),
                Out::Failed(message) => panic!("{message}"),
                Out::Ended => { ended = true; break; }
            }
        }
        assert!(ended);
        let elapsed = started.elapsed();
        assert!(elapsed >= Duration::from_millis(900), "paced at the stream's speed, not as fast as it downloads: {elapsed:?}");
        let joined: Vec<u8> = native.concat();
        assert_eq!(joined, std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/tv/h264_high_50fps.h264")).unwrap(), "the original frames, unchanged");
        assert!(converted >= 40, "the converted copy was made: {converted}");
        assert!(audio >= 40, "about 50 Opus packets for a second of sound: {audio}");
        assert_eq!(info.as_deref(), Some("180p50 · H.264"));
        assert!(shown.load(Ordering::Relaxed) >= 40, "the sharer sees the picture");
        assert!(!shared.web_native.load(Ordering::Acquire), "this channel has B-frames, so browsers get the converted copy");
        assert!(shared.native.load(Ordering::Acquire));
        assert!(pictures.lock().unwrap().is_empty(), "the picture is cleared when playback ends");
    }
}
