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

/// Sound is handed out this much before its time, into a speaker queue kept this
/// full, so a slow video frame never leaves the speakers waiting.
const SOUND_EARLY: Duration = Duration::from_millis(100);
/// Reading stops while this much of the channel is already waiting to be played.
const READ_AHEAD: Duration = Duration::from_secs(3);

/// A unit waiting for its time. Units are read ahead of time and played from here,
/// because channels mux sound and pictures at different offsets: sound often comes
/// in chunks of a third of a second, which arrive late if reading follows the video.
struct Pending { due: Instant, order: u64, unit: Unit }
impl PartialEq for Pending { fn eq(&self, other: &Self) -> bool { (self.due, self.order) == (other.due, other.order) } }
impl Eq for Pending {}
impl PartialOrd for Pending { fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) } }
// Reversed, so the binary heap gives the earliest first.
impl Ord for Pending { fn cmp(&self, other: &Self) -> std::cmp::Ordering { (other.due, other.order).cmp(&(self.due, self.order)) } }

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
        let mut waiting = std::collections::BinaryHeap::<Pending>::new();
        let (mut order, mut latest, mut ended) = (0u64, Instant::now(), false);
        while !self.stopped() {
            // Play everything whose time has come.
            while waiting.peek().is_some_and(|p| p.due <= Instant::now()) {
                let Some(next) = waiting.pop() else { break };
                self.unit(next.unit);
                if self.stopped() { break; }
            }
            if ended && waiting.is_empty() { self.drain(); self.send(Out::Ended); break; }
            let until_next = waiting.peek().map_or(Duration::from_millis(10), |p| p.due.saturating_duration_since(Instant::now())).min(Duration::from_millis(10));
            if ended || latest > Instant::now() + READ_AHEAD { std::thread::sleep(until_next); continue; }
            match events.try_recv() {
                Ok(Event::Units(units)) => for unit in units {
                    let due = self.due_of(&unit);
                    latest = latest.max(due);
                    order += 1;
                    waiting.push(Pending { due, order, unit });
                },
                Ok(Event::Ended) => ended = true,
                Ok(Event::Failed(message)) => { self.send(Out::Failed(message)); break; }
                Err(mpsc::error::TryRecvError::Empty) => std::thread::sleep(until_next.max(Duration::from_millis(1))),
                Err(mpsc::error::TryRecvError::Disconnected) => ended = true,
            }
        }
        self.shared.stop.store(true, Ordering::Release);
        if let Some(playback) = &self.local.playback { playback.remove(self.local.source); }
        if let Ok(mut pictures) = self.local.pictures.lock() { pictures.remove(&self.local.key); }
        (self.local.notify)();
    }

    /// When a unit should be played: its time on the stream clock, which runs a little
    /// behind arrival and re-anchors after jumps and long stalls.
    fn due_of(&mut self, unit: &Unit) -> Instant {
        let (ts, early) = match unit {
            Unit::Video { pts, dts, .. } => (dts.or(*pts), Duration::ZERO),
            Unit::Audio { pts, .. } => (*pts, SOUND_EARLY),
        };
        let now = Instant::now();
        let Some(ts) = ts else { return now };
        let clock = self.clock.get_or_insert(Clock { wall: now + LEAD, ts });
        // Sound and pictures of the same moment can be muxed a second or two apart.
        let ahead = ts as i64 - clock.ts as i64;
        if ahead < -(TICKS as i64) * 3 || ahead > (TICKS as i64) * 3600 { *clock = Clock { wall: now + LEAD, ts }; }
        let ahead = ts as i64 - clock.ts as i64;
        let offset = Duration::from_micros(ahead.unsigned_abs() * 1_000_000 / TICKS);
        let due = if ahead >= 0 { clock.wall + offset } else { clock.wall.checked_sub(offset).unwrap_or(now) };
        // After a long stall, start again from now rather than rushing to catch up.
        if due + Duration::from_secs(2) < now { *clock = Clock { wall: now + LEAD, ts }; return now + LEAD; }
        due.checked_sub(early).unwrap_or(due)
    }

    fn unit(&mut self, unit: Unit) {
        match unit {
            Unit::Video { codec, pts, dts, data, .. } => {
                let Some(ts) = dts.or(pts) else { return };
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
            Unit::Audio { sample_rate, channels, data, .. } => self.sound(sample_rate, channels, &data),
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
        if let Some(playback) = &self.local.playback {
            if !mono.is_empty() {
                // Keep the speakers SOUND_EARLY ahead, so playing stays smooth.
                if playback.queued(self.local.source) == 0 { playback.play(self.local.source, &vec![0.0; (super::audio::RATE as u128 * SOUND_EARLY.as_millis() / 1000) as usize]); }
                playback.play(self.local.source, &mono);
            }
        }
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

    /// Channels often carry sound in chunks of a third of a second. It must still come
    /// out at its own pace (about 5 Opus packets per 100 ms), not in bursts the
    /// speakers' short queue would cut short.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sound_in_big_chunks_comes_out_at_its_own_pace() {
        let ts = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/tv/audio_big_chunks.ts")).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}/1.ts", listener.local_addr().unwrap());
        std::thread::spawn(move || for mut stream in listener.incoming().flatten() {
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(format!("HTTP/1.1 200 OK
Content-Length: {}
Connection: close

", ts.len()).as_bytes());
            let _ = stream.write_all(&ts);
        });
        let shared = Arc::new(Shared::default());
        let (tx, mut rx) = mpsc::channel(256);
        start(Zeroizing::new(address), shared, tx, Local { pictures: Arc::new(Mutex::new(Default::default())), key: "me".into(), playback: None, source: 1, notify: Box::new(|| {}) });
        let mut arrivals = Vec::new();
        while let Ok(Some(item)) = tokio::time::timeout(Duration::from_secs(15), rx.recv()).await {
            match item {
                Out::Audio(_) => arrivals.push(Instant::now()),
                Out::Failed(message) => panic!("{message}"),
                Out::Ended => break,
                _ => {}
            }
        }
        assert!(arrivals.len() >= 90, "about 100 packets for 2 s of sound: {}", arrivals.len());
        let busiest = arrivals.iter().map(|a| arrivals.iter().filter(|b| **b >= *a && b.duration_since(*a) < Duration::from_millis(100)).count()).max().unwrap();
        assert!(busiest <= 9, "at most about 100 ms of sound per 100 ms, got {busiest} packets");
    }
}
