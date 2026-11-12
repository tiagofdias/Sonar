//! Screen sharing inside a call or voice room. Signaling matches the web
//! client's useScreenShare.js. One capture and one encoder serve every viewer,
//! each over its own DTLS-SRTP peer connection, separate from voice.
use crate::api::Session;
use crate::live::CallHandle;
use crate::screen_audio::{self, Sound};
use crate::screen_decode::{nv12_to_rgba, Decoder, Depacketizer};
use crate::screen_media::{self, Codec, Ended, Quality, Source};
use crate::voice_audio::ScreenPlayback;
use crate::voice_codec::{VoiceDecoder, VoiceEncoder};
use crate::voice_transport::fingerprint;
use iced::futures::{SinkExt, Stream};
use rtc::media::Sample;
use rtc::media_stream::MediaStreamTrack;
use rtc::peer_connection::configuration::media_engine::{MIME_TYPE_H264, MIME_TYPE_OPUS};
use rtc::peer_connection::configuration::{RTCConfigurationBuilder, RTCIceServer, RTCIceTransportPolicy};
use rtc::peer_connection::sdp::RTCSessionDescription;
use rtc::peer_connection::transport::RTCIceCandidateInit;
use rtc::rtp_transceiver::rtp_sender::{RTCRtpCodec, RTCRtpCodecParameters, RTCRtpCodingParameters, RTCRtpEncodingParameters, RTCPFeedback, RtpCodecKind};
use rand_core::RngCore;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use webrtc::media_stream::track_local::{static_sample::TrackLocalStaticSample, TrackLocal};
use webrtc::media_stream::track_remote::{TrackRemote, TrackRemoteEvent};
use webrtc::media_stream::Track;
use webrtc::peer_connection::{MediaEngine, PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCIceGatheringState, RTCPeerConnectionState};
use webrtc::rtp_transceiver::RtpSender;

const H264_FMTP: [&str; 2] = ["level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f", "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42001f"];
/// TV channels are H.264 High profile; offered first on TV links so browsers accept the original.
const H264_HIGH: &str = "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=640032";

#[derive(Clone, Debug)]
pub enum Scope { Call { peer: String, id: String }, Room { community: String, channel: String, session: String } }

#[derive(Clone, Debug, PartialEq)]
pub struct Target { pub user: String, pub session: String }

#[derive(Clone)]
pub struct Spec { pub key: String, pub scope: Scope, pub session: Session, pub socket: CallHandle, pub output: Option<String>, pub relay_only: bool }
impl Hash for Spec { fn hash<H: Hasher>(&self, state: &mut H) { self.key.hash(state); } }

#[derive(Clone, Debug)]
pub struct Choice { pub source: Source, pub sound: Sound, pub height: u32 }

/// A TV channel to play (and share, when `share` is set). The address may hold the
/// person's login: it stays in this process and is never sent anywhere but the provider.
pub struct TvChoice { pub address: zeroize::Zeroizing<String>, pub label: String, pub share: bool }
impl std::fmt::Debug for TvChoice { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_struct("TvChoice").field("label", &self.label).field("share", &self.share).finish_non_exhaustive() } }

#[derive(Debug)]
/// `JoinTv` / `LeaveTv`: start or stop watching someone's TV share (it waits until you join).
pub enum Command { Start(Choice), StartTv(TvChoice), Stop, Signal { from: String, signal: Value }, Targets(Vec<Target>), Deafen(bool), Volume { from: String, volume: f32 }, JoinTv(String), LeaveTv(String) }

#[derive(Clone, Debug)]
/// `TvAvailable`: someone shares TV and it is waiting for us to join (None: no longer).
pub enum Update { Ready(Controls), Sharing(Option<String>), Notice(String), Pictures, ViewerGone(String), SoundFailed, Label(String, Option<String>), TvAvailable(String, Option<String>) }

pub struct Picture { pub width: u32, pub height: u32, pub rgba: Vec<u8> }
/// Only the newest picture per sharer is kept, so a slow screen never grows memory.
pub type Mailbox = Arc<Mutex<HashMap<String, Picture>>>;

#[derive(Clone)]
pub struct Controls { tx: mpsc::UnboundedSender<Command>, pub pictures: Mailbox }
impl std::fmt::Debug for Controls { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("ScreenControls") } }
impl Controls { pub fn send(&self, command: Command) { let _ = self.tx.send(command); } }

struct Handler { gathered: mpsc::Sender<()>, connected: Arc<AtomicBool>, failed: Arc<AtomicBool>, rtp: Option<mpsc::Sender<Rtp>> }
enum Rtp { Video(Vec<u8>, bool), Audio(Vec<u8>) }

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete { let _ = self.gathered.try_send(()); }
    }
    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        self.connected.store(state == RTCPeerConnectionState::Connected, Ordering::Release);
        if matches!(state, RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed) { self.failed.store(true, Ordering::Release); }
    }
    async fn on_track(&self, track: Arc<dyn TrackRemote>) {
        let Some(rtp) = self.rtp.clone() else { return };
        let video = track.kind().await == RtpCodecKind::Video;
        tokio::spawn(async move {
            while let Some(event) = track.poll().await {
                if let TrackRemoteEvent::OnRtpPacket(packet) = event {
                    if packet.payload.is_empty() || packet.payload.len() > 4096 { continue; }
                    let item = if video { Rtp::Video(packet.payload.to_vec(), packet.header.marker) } else { Rtp::Audio(packet.payload.to_vec()) };
                    // Video must not lose packets to a brief stall; audio may.
                    if video { if rtp.send(item).await.is_err() { break; } } else { let _ = rtp.try_send(item); }
                }
            }
        });
    }
}

fn engine(tv: bool) -> Result<MediaEngine, String> {
    let mut media = MediaEngine::default();
    let feedback = || vec![RTCPFeedback { typ: "nack".into(), parameter: String::new() }, RTCPFeedback { typ: "nack".into(), parameter: "pli".into() }];
    if tv { media.register_codec(RTCRtpCodecParameters { rtp_codec: RTCRtpCodec { mime_type: MIME_TYPE_H264.into(), clock_rate: 90000, channels: 0, sdp_fmtp_line: H264_HIGH.into(), rtcp_feedback: feedback() }, payload_type: 100 }, RtpCodecKind::Video).map_err(|_| "Video codec unavailable")?; }
    for (index, fmtp) in H264_FMTP.iter().enumerate() {
        media.register_codec(RTCRtpCodecParameters { rtp_codec: RTCRtpCodec { mime_type: MIME_TYPE_H264.into(), clock_rate: 90000, channels: 0, sdp_fmtp_line: (*fmtp).into(), rtcp_feedback: feedback() }, payload_type: 102 + index as u8 * 2 }, RtpCodecKind::Video).map_err(|_| "Video codec unavailable")?;
    }
    media.register_codec(RTCRtpCodecParameters { rtp_codec: RTCRtpCodec { mime_type: MIME_TYPE_OPUS.into(), clock_rate: 48000, channels: 2, sdp_fmtp_line: "minptime=10;useinbandfec=1".into(), rtcp_feedback: vec![] }, payload_type: 111 }, RtpCodecKind::Audio).map_err(|_| "Audio codec unavailable")?;
    Ok(media)
}

struct Link { connection: Arc<dyn PeerConnection>, gathered: mpsc::Receiver<()>, connected: Arc<AtomicBool>, failed: Arc<AtomicBool>, remote_set: bool, pending: Vec<RTCIceCandidateInit> }

async fn link(servers: Vec<RTCIceServer>, relay: bool, rtp: Option<mpsc::Sender<Rtp>>, tv: bool) -> Result<Link, String> {
    let config = RTCConfigurationBuilder::new().with_ice_servers(servers).with_ice_transport_policy(if relay { RTCIceTransportPolicy::Relay } else { RTCIceTransportPolicy::All }).build();
    let (tx, gathered) = mpsc::channel(1);
    let (connected, failed) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let handler = Handler { gathered: tx, connected: connected.clone(), failed: failed.clone(), rtp };
    let connection: Arc<dyn PeerConnection> = Arc::new(PeerConnectionBuilder::new().with_configuration(config).with_media_engine(engine(tv)?).with_udp_addrs(vec!["0.0.0.0:0".to_string()]).with_handler(Arc::new(handler)).build().await.map_err(|_| "Could not start the screen connection")?);
    Ok(Link { connection, gathered, connected, failed, remote_set: false, pending: Vec::new() })
}

impl Link {
    async fn local_sdp(&mut self) -> Result<String, String> {
        tokio::time::timeout(Duration::from_secs(15), self.gathered.recv()).await.map_err(|_| "Network setup timed out")?.ok_or("Network setup was interrupted")?;
        Ok(self.connection.local_description().await.ok_or("No local description")?.sdp)
    }
    async fn set_remote(&mut self, description: RTCSessionDescription) -> Result<(), String> {
        self.connection.set_remote_description(description).await.map_err(|_| "Could not apply the screen description")?;
        self.remote_set = true;
        for candidate in std::mem::take(&mut self.pending) { let _ = self.connection.add_ice_candidate(candidate).await; }
        Ok(())
    }
    async fn add_candidate(&mut self, value: &Value) {
        let Ok(candidate) = serde_json::from_value::<RTCIceCandidateInit>(value.clone()) else { return };
        if !crate::ice_candidate::valid_remote_candidate(&candidate) { return; }
        if self.remote_set { let _ = self.connection.add_ice_candidate(candidate).await; } else if self.pending.len() < 128 { self.pending.push(candidate); }
    }
    fn close(self) { tokio::spawn(async move { let _ = self.connection.close().await; }); }
}

struct OutTrack { track: Arc<TrackLocalStaticSample>, sender: Arc<dyn RtpSender>, mime: &'static str, ready: Option<(u32, u8)> }
impl OutTrack {
    async fn create(connection: &Arc<dyn PeerConnection>, id: &str, kind: RtpCodecKind, codec: RTCRtpCodec, mime: &'static str) -> Result<Self, String> {
        let ssrc = rand_core::OsRng.next_u32();
        let track = Arc::new(TrackLocalStaticSample::new(Instant::now(), MediaStreamTrack::new("sonar-screen".into(), id.into(), id.into(), kind,
            vec![RTCRtpEncodingParameters { rtp_coding_parameters: RTCRtpCodingParameters { ssrc: Some(ssrc), ..Default::default() }, codec, ..Default::default() }])).map_err(|_| "Could not create the screen track")?);
        let sender = connection.add_track(track.clone() as Arc<dyn TrackLocal>).await.map_err(|_| "Could not add the screen track")?;
        Ok(Self { track, sender, mime, ready: None })
    }
    async fn write(&mut self, data: &[u8], duration: Duration) -> Result<(), String> {
        if self.ready.is_none() {
            let parameters = self.sender.get_parameters().await.map_err(|_| "No parameters")?;
            let payload = parameters.rtp_parameters.codecs.iter().find(|c| c.rtp_codec.mime_type.eq_ignore_ascii_case(self.mime)).ok_or("Codec not agreed")?.payload_type;
            let ssrc = *self.track.ssrcs().await.first().ok_or("No track")?;
            self.ready = Some((ssrc, payload));
        }
        let (ssrc, payload) = self.ready.ok_or("No track")?;
        self.track.sample_writer(ssrc, payload).write_sample(&Sample { data: bytes::Bytes::copy_from_slice(data), duration, ..Sample::new(Instant::now()) }).await.map_err(|_| "Could not send".to_string())
    }
}

/// `desktop`: the viewer is the Sonar app, which plays a TV channel's original stream even with B-frames.
struct Outgoing { link: Link, video: OutTrack, audio: OutTrack, joined: bool, offered_to: Target, desktop: bool, tv: bool }
struct Incoming { link: Link, stop: Arc<AtomicBool> }

enum Media { Video(Vec<u8>), Audio(Vec<u8>) }
struct Sharing { stop: Arc<AtomicBool>, force_key: Arc<AtomicBool> }

fn start_capture(choice: Choice, media: mpsc::Sender<Media>, updates: mpsc::UnboundedSender<Update>, restarts: mpsc::UnboundedSender<u32>) -> Sharing {
    let stop = Arc::new(AtomicBool::new(false));
    let force_key = Arc::new(AtomicBool::new(false));
    let (video_stop, key) = (stop.clone(), force_key.clone());
    let (video_media, video_updates) = (media.clone(), updates.clone());
    let height = choice.height;
    std::thread::spawn(move || {
        let flag = video_stop.clone();
        let mut keep = move || !flag.load(Ordering::Acquire);
        let report = video_updates.clone();
        let mut ready = |quality: Quality, codec: Codec, name: &str| { let _ = report.send(Update::Sharing(Some(format!("{}p {codec:?} via {name}", quality.height)))); };
        let mut packet = |data: &[u8], _key: bool, _: Duration| { let _ = video_media.try_send(Media::Video(data.to_vec())); };
        match screen_media::run(choice.source, Quality::for_height(height), &key, &mut keep, &mut ready, &mut packet) {
            Ok(Ended::TooSlow) if height > 720 => { let lower = match height { 2160 => 1440, 1440 => 1080, _ => 720 }; let _ = restarts.send(lower); }
            Ok(_) => {}
            Err(error) => { let _ = video_updates.send(Update::Notice(format!("Screen sharing stopped: {error}"))); let _ = video_updates.send(Update::Sharing(None)); }
        }
    });
    let audio_stop = stop.clone();
    let sound = choice.sound;
    std::thread::spawn(move || {
        #[cfg(debug_assertions)]
        if std::env::var_os("SONAR_DEV_SILENT_AUDIO").is_some() { return; }
        let Ok(mut encoder) = VoiceEncoder::new() else { return };
        let mut frame = |samples: &[f32]| { if let Ok(packet) = encoder.encode(samples) { let _ = media.try_send(Media::Audio(packet.to_vec())); } };
        if screen_audio::run(sound, &audio_stop, &mut frame).is_err() { let _ = updates.send(Update::SoundFailed); }
    });
    Sharing { stop, force_key }
}

fn start_viewer(from: String, mut rtp: mpsc::Receiver<Rtp>, pictures: Mailbox, playback: Arc<ScreenPlayback>, source: u64, updates: mpsc::UnboundedSender<Update>, stop: Arc<AtomicBool>) {
    let (units, unit_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(6);
    let (name, thread_stop) = (from.clone(), stop.clone());
    let notify = updates.clone();
    std::thread::spawn(move || {
        let Ok(mut decoder) = Decoder::new() else { return };
        let mut rgba = Vec::new();
        while let Ok(unit) = unit_rx.recv() {
            if thread_stop.load(Ordering::Acquire) { break; }
            let mut shown = false;
            let result = decoder.decode(&unit, &mut |y, uv, stride, width, height| {
                nv12_to_rgba(y, uv, stride, width, height, &mut rgba);
                if let Ok(mut slot) = pictures.lock() {
                    let spare = slot.remove(&name).map(|old| old.rgba);
                    let next = spare.unwrap_or_default();
                    slot.insert(name.clone(), Picture { width: width as u32, height: height as u32, rgba: std::mem::replace(&mut rgba, next) });
                }
                shown = true;
            });
            if result.is_err() { continue; }
            if shown { let _ = notify.send(Update::Pictures); }
        }
    });
    tokio::spawn(async move {
        let mut depacketizer = Depacketizer::default();
        let mut decoder = VoiceDecoder::new().ok();
        while let Some(item) = rtp.recv().await {
            if stop.load(Ordering::Acquire) { break; }
            match item {
                Rtp::Video(payload, marker) => if let Some(unit) = depacketizer.push(&payload, marker) { let _ = units.try_send(unit); },
                Rtp::Audio(packet) => if let Some(frame) = decoder.as_mut().and_then(|d| d.decode(&packet).ok()) { playback.play(source, &frame); },
            }
        }
        playback.remove(source);
    });
}

fn stable_source(id: &str) -> u64 { id.bytes().fold(0xcbf29ce484222325u64, |hash, byte| (hash ^ byte as u64).wrapping_mul(0x100000001b3)) }

async fn send(spec: &Spec, target: &Target, mut signal: Value) -> Result<(), String> {
    match &spec.scope {
        Scope::Call { peer, id } => {
            signal["callId"] = json!(id);
            spec.socket.request("call:signal", json!({ "to": peer, "signal": signal })).await.map(|_| ())
        }
        Scope::Room { community, channel, session } => spec.socket.request("voice:room-signal", json!({ "communityId": community, "channelId": channel, "sessionId": session, "to": target.user, "toSession": target.session, "signal": signal })).await.map(|_| ()),
    }
}

/// `tv`: the channel's name, for a TV share (High-profile video, labelled for viewers).
async fn offer(spec: &Spec, target: &Target, tv: Option<&str>) -> Result<Outgoing, String> {
    let (servers, relay) = spec.session.voice_configuration().await?.into_servers(spec.relay_only)?;
    let mut link = link(servers, relay, None, tv.is_some()).await?;
    let fmtp = if tv.is_some() { H264_HIGH } else { H264_FMTP[0] };
    let video = OutTrack::create(&link.connection, "screen-video", RtpCodecKind::Video, RTCRtpCodec { mime_type: MIME_TYPE_H264.into(), clock_rate: 90000, channels: 0, sdp_fmtp_line: fmtp.into(), rtcp_feedback: vec![] }, MIME_TYPE_H264).await?;
    let audio = OutTrack::create(&link.connection, "screen-audio", RtpCodecKind::Audio, RTCRtpCodec { mime_type: MIME_TYPE_OPUS.into(), clock_rate: 48000, channels: 2, sdp_fmtp_line: String::new(), rtcp_feedback: vec![] }, MIME_TYPE_OPUS).await?;
    let description = link.connection.create_offer(None).await.map_err(|_| "Could not create the screen offer")?;
    link.connection.set_local_description(description).await.map_err(|_| "Could not prepare the screen offer")?;
    let sdp = link.local_sdp().await?;
    if sdp.len() > 30000 { return Err("Screen description too large".into()); }
    let mut signal = json!({ "type": "screen-offer", "sdp": { "type": "offer", "sdp": sdp } });
    if let Some(label) = tv { signal["tv"] = json!(true); signal["label"] = json!(label.chars().take(80).collect::<String>()); }
    send(spec, target, signal).await?;
    Ok(Outgoing { link, video, audio, joined: false, offered_to: target.clone(), desktop: false, tv: tv.is_some() })
}

pub fn run(spec: &Spec) -> impl Stream<Item = Update> + 'static {
    let spec = spec.clone();
    iced::stream::channel(32, move |mut output: iced::futures::channel::mpsc::Sender<Update>| async move {
        let (tx, mut commands) = mpsc::unbounded_channel();
        // Joining a waiting TV share runs the stored offer through the usual path.
        let rejoin = tx.clone();
        // TV offers waiting until we choose to join: sharer -> their offer.
        let mut offers: HashMap<String, Value> = HashMap::new();
        let pictures: Mailbox = Arc::new(Mutex::new(HashMap::new()));
        let _ = output.send(Update::Ready(Controls { tx, pictures: pictures.clone() })).await;
        let (update_tx, mut updates) = mpsc::unbounded_channel::<Update>();
        let (restart_tx, mut restarts) = mpsc::unbounded_channel::<u32>();
        let (media_tx, mut media) = mpsc::channel::<Media>(16);
        let (mut targets, mut sharing, mut choice): (Vec<Target>, Option<Sharing>, Option<Choice>) = (Vec::new(), None, None);
        let mut outgoing: HashMap<String, Outgoing> = HashMap::new();
        let mut incoming: HashMap<String, Incoming> = HashMap::new();
        let mut playback: Option<Arc<ScreenPlayback>> = None;
        let mut deafened = false;
        let mut volumes: HashMap<String, f32> = HashMap::new();
        // A TV share: its player, the channel name, whether it is shared, and its output.
        let mut tv: Option<(Arc<crate::tv::player::Shared>, String, bool)> = None;
        let mut tv_out: Option<mpsc::Receiver<crate::tv::player::Out>> = None;
        let stop_tv = |tv: &mut Option<(Arc<crate::tv::player::Shared>, String, bool)>| { if let Some((shared, ..)) = tv.take() { shared.stop.store(true, Ordering::Release); } };
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        let frame = Duration::from_millis(33);
        loop {
            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else { break };
                    match command {
                        Command::StartTv(choice_tv) => {
                            // Another channel while sharing TV: the same connections carry it, so nobody has to join again.
                            let switching = choice_tv.share && tv.as_ref().is_some_and(|(_, _, shared)| *shared);
                            if let Some(old) = sharing.take() { old.stop.store(true, Ordering::Release); }
                            choice = None;
                            stop_tv(&mut tv);
                            if switching {
                                for out in outgoing.values_mut() { out.joined = false; let _ = send(&spec, &out.offered_to, json!({ "type": "screen-label", "label": choice_tv.label.chars().take(80).collect::<String>() })).await; }
                            } else {
                                for (_, out) in outgoing.drain() { let _ = send(&spec, &out.offered_to, json!({ "type": "screen-stop" })).await; out.link.close(); }
                            }
                            if playback.is_none() { if let Ok(p) = ScreenPlayback::open(spec.output.clone()) { p.set_deafened(deafened); playback = Some(Arc::new(p)); } }
                            let shared = Arc::new(crate::tv::player::Shared::default());
                            let (tx, rx) = mpsc::channel(64);
                            let notify = update_tx.clone();
                            crate::tv::player::start(choice_tv.address, shared.clone(), tx, crate::tv::player::Local { pictures: pictures.clone(), key: spec.session.me().to_owned(), playback: playback.clone(), source: stable_source(spec.session.me()), notify: Box::new(move || { let _ = notify.send(Update::Pictures); }) });
                            tv_out = Some(rx);
                            tv = Some((shared, choice_tv.label.clone(), choice_tv.share));
                            let _ = output.send(Update::Sharing(Some(format!("TV · {}", choice_tv.label)))).await;
                            if choice_tv.share && !switching { for target in targets.clone() { if let Ok(out) = offer(&spec, &target, Some(&choice_tv.label)).await { outgoing.insert(target.user.clone(), out); } } }
                        }
                        Command::Start(next) => {
                            stop_tv(&mut tv);
                            tv_out = None;
                            if let Some(old) = sharing.take() { old.stop.store(true, Ordering::Release); }
                            choice = Some(next.clone());
                            sharing = Some(start_capture(next, media_tx.clone(), update_tx.clone(), restart_tx.clone()));
                            for target in targets.clone() { if let Ok(out) = offer(&spec, &target, None).await { if let Some(old) = outgoing.insert(target.user.clone(), out) { old.link.close(); } } }
                        }
                        Command::Stop => {
                            stop_tv(&mut tv);
                            tv_out = None;
                            if let Some(old) = sharing.take() { old.stop.store(true, Ordering::Release); }
                            choice = None;
                            for (_, out) in outgoing.drain() { let _ = send(&spec, &out.offered_to, json!({ "type": "screen-stop" })).await; out.link.close(); }
                            let _ = output.send(Update::Sharing(None)).await;
                        }
                        Command::Targets(next) => {
                            let gone: Vec<String> = outgoing.keys().filter(|id| !next.iter().any(|t| &t.user == *id)).cloned().collect();
                            for id in gone { if let Some(out) = outgoing.remove(&id) { out.link.close(); } }
                            let left: Vec<String> = incoming.keys().filter(|id| !next.iter().any(|t| &t.user == *id)).cloned().collect();
                            for id in left { if let Some(view) = incoming.remove(&id) { view.stop.store(true, Ordering::Release); view.link.close(); if let Ok(mut slot) = pictures.lock() { slot.remove(&id); } let _ = output.send(Update::ViewerGone(id)).await; } }
                            let waiting: Vec<String> = offers.keys().filter(|id| !next.iter().any(|t| &t.user == *id)).cloned().collect();
                            for id in waiting { offers.remove(&id); let _ = output.send(Update::TvAvailable(id, None)).await; }
                            let label = tv.as_ref().filter(|(_, _, share)| *share).map(|(_, label, _)| label.clone());
                            if sharing.is_some() || label.is_some() { for target in &next { if !outgoing.contains_key(&target.user) { if let Ok(out) = offer(&spec, target, label.as_deref()).await { outgoing.insert(target.user.clone(), out); } } } }
                            targets = next;
                        }
                        Command::Volume { from, volume } => {
                            if let Some(p) = &playback { p.set_volume(stable_source(&from), volume); }
                            volumes.insert(from, volume);
                        }
                        Command::JoinTv(from) => {
                            if let Some(mut signal) = offers.remove(&from) {
                                let _ = output.send(Update::TvAvailable(from.clone(), None)).await;
                                signal["sonarJoin"] = json!(true);
                                let _ = rejoin.send(Command::Signal { from, signal });
                            }
                        }
                        Command::LeaveTv(from) => {
                            if let Some(view) = incoming.remove(&from) { view.stop.store(true, Ordering::Release); view.link.close(); }
                            if let Ok(mut slot) = pictures.lock() { slot.remove(&from); }
                            let _ = output.send(Update::ViewerGone(from.clone())).await;
                            // The sharer offers it again, so it can be joined later.
                            if let Some(target) = targets.iter().find(|t| t.user == from).cloned() { let _ = send(&spec, &target, json!({ "type": "screen-leave" })).await; }
                        }
                        Command::Deafen(value) => { deafened = value; if let Some(p) = &playback { p.set_deafened(value); } }
                        Command::Signal { from, signal } => {
                            let kind = signal.get("type").and_then(Value::as_str).unwrap_or_default().to_owned();
                            match kind.as_str() {
                                "screen-leave" => {
                                    // Someone stopped watching our TV: offer it again so they can come back.
                                    if let Some(out) = outgoing.remove(&from) { out.link.close(); }
                                    let label = tv.as_ref().filter(|(_, _, share)| *share).map(|(_, label, _)| label.clone());
                                    if let (Some(target), true) = (targets.iter().find(|t| t.user == from).cloned(), sharing.is_some() || label.is_some()) {
                                        if let Ok(out) = offer(&spec, &target, label.as_deref()).await { outgoing.insert(target.user.clone(), out); }
                                    }
                                }
                                "screen-label" => {
                                    let label: String = signal.get("label").and_then(Value::as_str).unwrap_or("TV").chars().filter(|c| !c.is_control()).take(80).collect();
                                    if let Some(waiting) = offers.get_mut(&from) { waiting["label"] = json!(label.clone()); let _ = output.send(Update::TvAvailable(from.clone(), Some(label.clone()))).await; }
                                    if incoming.contains_key(&from) { let _ = output.send(Update::Label(from.clone(), Some(label))).await; }
                                }
                                "screen-stop" => { if offers.remove(&from).is_some() { let _ = output.send(Update::TvAvailable(from.clone(), None)).await; } if let Some(view) = incoming.remove(&from) { view.stop.store(true, Ordering::Release); view.link.close(); } if let Ok(mut slot) = pictures.lock() { slot.remove(&from); } let _ = output.send(Update::ViewerGone(from)).await; }
                                "screen-answer" => {
                                    let sdp = signal.get("sdp").and_then(|v| v.get("sdp")).and_then(Value::as_str).unwrap_or_default().to_owned();
                                    if let Some(out) = outgoing.get_mut(&from) {
                                        out.desktop = signal.get("client").and_then(Value::as_str) == Some("desktop");
                                        if fingerprint(&sdp).is_ok() { if let Ok(answer) = RTCSessionDescription::answer(sdp) { let _ = out.link.set_remote(answer).await; } }
                                    }
                                }
                                "screen-ice" => {
                                    let candidate = signal.get("candidate").cloned().unwrap_or_default();
                                    if signal.get("role").and_then(Value::as_str) == Some("sharer") { if let Some(view) = incoming.get_mut(&from) { view.link.add_candidate(&candidate).await; } }
                                    else if let Some(out) = outgoing.get_mut(&from) { out.link.add_candidate(&candidate).await; }
                                }
                                "screen-offer" => {
                                    let Some(target) = targets.iter().find(|t| t.user == from).cloned() else { continue };
                                    let sdp = signal.get("sdp").and_then(|v| v.get("sdp")).and_then(Value::as_str).unwrap_or_default().to_owned();
                                    if sdp.len() > 32000 || fingerprint(&sdp).is_err() { continue; }
                                    if let Some(old) = incoming.remove(&from) { old.stop.store(true, Ordering::Release); old.link.close(); }
                                    // TV waits until we choose to join; screens open straight away.
                                    let is_tv = signal.get("tv").and_then(Value::as_bool) == Some(true);
                                    if is_tv && signal.get("sonarJoin").is_none() {
                                        let label: String = signal.get("label").and_then(Value::as_str).unwrap_or("TV").chars().filter(|c| !c.is_control()).take(80).collect();
                                        offers.insert(from.clone(), signal);
                                        let _ = output.send(Update::TvAvailable(from, Some(label))).await;
                                        continue;
                                    }
                                    let label = (signal.get("tv").and_then(Value::as_bool) == Some(true)).then(|| signal.get("label").and_then(Value::as_str).unwrap_or("TV").chars().filter(|c| !c.is_control()).take(80).collect::<String>());
                                    let _ = output.send(Update::Label(from.clone(), label)).await;
                                    if playback.is_none() { if let Ok(p) = ScreenPlayback::open(spec.output.clone()) { p.set_deafened(deafened); playback = Some(Arc::new(p)); } }
                                    let Some(player) = playback.clone() else { continue };
                                    if let Some(volume) = volumes.get(&from) { player.set_volume(stable_source(&from), *volume); }
                                    let Ok(configuration) = spec.session.voice_configuration().await else { continue };
                                    let Ok((servers, relay)) = configuration.into_servers(spec.relay_only) else { continue };
                                    let (rtp_tx, rtp_rx) = mpsc::channel(512);
                                    let Ok(mut view) = link(servers, relay, Some(rtp_tx), signal.get("tv").and_then(Value::as_bool) == Some(true)).await else { continue };
                                    let Ok(description) = RTCSessionDescription::offer(sdp) else { continue };
                                    if view.set_remote(description).await.is_err() { view.close(); continue; }
                                    let Ok(answer) = view.connection.create_answer(None).await else { view.close(); continue };
                                    if view.connection.set_local_description(answer).await.is_err() { view.close(); continue; }
                                    let Ok(local) = view.local_sdp().await else { view.close(); continue };
                                    // "desktop": this app plays a TV channel's original stream, B-frames included.
                                    if send(&spec, &target, json!({ "type": "screen-answer", "client": "desktop", "sdp": { "type": "answer", "sdp": local } })).await.is_err() { view.close(); continue; }
                                    let stop = Arc::new(AtomicBool::new(false));
                                    start_viewer(from.clone(), rtp_rx, pictures.clone(), player, stable_source(&from), update_tx.clone(), stop.clone());
                                    incoming.insert(from, Incoming { link: view, stop });
                                }
                                _ => {}
                            }
                        }
                    }
                }
                item = media.recv() => {
                    let Some(item) = item else { continue };
                    for out in outgoing.values_mut() {
                        if !out.link.connected.load(Ordering::Acquire) { continue; }
                        match &item {
                            Media::Video(data) => { let _ = out.video.write(data, frame).await; }
                            Media::Audio(data) => { let _ = out.audio.write(data, Duration::from_millis(20)).await; }
                        }
                    }
                }
                item = async { match tv_out.as_mut() { Some(rx) => rx.recv().await, None => std::future::pending().await } } => {
                    use crate::tv::player::Out;
                    let Some(item) = item else { tv_out = None; continue };
                    let Some((shared, label, _)) = tv.clone() else { continue };
                    let (native, web_native) = (shared.native.load(Ordering::Acquire), shared.web_native.load(Ordering::Acquire));
                    let takes_original = |out: &Outgoing| native && (out.desktop || web_native);
                    match item {
                        Out::Native(data, duration) => for out in outgoing.values_mut() { if out.tv && out.link.connected.load(Ordering::Acquire) && takes_original(out) { let _ = out.video.write(&data, duration).await; } },
                        Out::Converted(data, duration) => for out in outgoing.values_mut() { if out.tv && out.link.connected.load(Ordering::Acquire) && !takes_original(out) { let _ = out.video.write(&data, duration).await; } },
                        Out::Audio(packet) => for out in outgoing.values_mut() { if out.tv && out.link.connected.load(Ordering::Acquire) { let _ = out.audio.write(&packet, Duration::from_millis(20)).await; } },
                        Out::Info(text) => { let _ = output.send(Update::Sharing(Some(format!("TV · {label} · {text}")))).await; }
                        Out::Failed(message) => {
                            stop_tv(&mut tv); tv_out = None;
                            for (_, out) in outgoing.drain() { let _ = send(&spec, &out.offered_to, json!({ "type": "screen-stop" })).await; out.link.close(); }
                            let _ = output.send(Update::Notice(message)).await;
                            let _ = output.send(Update::Sharing(None)).await;
                        }
                        Out::Ended => {
                            stop_tv(&mut tv); tv_out = None;
                            for (_, out) in outgoing.drain() { let _ = send(&spec, &out.offered_to, json!({ "type": "screen-stop" })).await; out.link.close(); }
                            let _ = output.send(Update::Notice("The channel ended.".into())).await;
                            let _ = output.send(Update::Sharing(None)).await;
                        }
                    }
                }
                update = updates.recv() => { if let Some(update) = update { if matches!(update, Update::Pictures) { let _ = output.try_send(update); } else { let _ = output.send(update).await; } } }
                lower = restarts.recv() => {
                    if let (Some(height), Some(mut next)) = (lower, choice.clone()) {
                        next.height = height;
                        if let Some(old) = sharing.take() { old.stop.store(true, Ordering::Release); }
                        choice = Some(next.clone());
                        sharing = Some(start_capture(next, media_tx.clone(), update_tx.clone(), restart_tx.clone()));
                        let _ = output.send(Update::Notice(format!("Your PC could not keep up, so sharing dropped to {height}p."))).await;
                    }
                }
                _ = tick.tick() => {
                    let tv_flags = tv.as_ref().map(|(shared, ..)| (shared.clone(), shared.native.load(Ordering::Acquire), shared.web_native.load(Ordering::Acquire)));
                    let mut need_converted = false;
                    for out in outgoing.values_mut() {
                        let original = tv_flags.as_ref().is_some_and(|(_, native, web)| *native && (out.desktop || *web));
                        if out.tv && out.link.connected.load(Ordering::Acquire) && !original { need_converted = true; }
                        if !out.joined && out.link.connected.load(Ordering::Acquire) {
                            out.joined = true;
                            if let Some(active) = &sharing { active.force_key.store(true, Ordering::Release); }
                            if let Some((shared, ..)) = &tv_flags { if !original { shared.force_key.store(true, Ordering::Release); } }
                        }
                    }
                    // The converted copy costs the graphics card some work, so it runs only while someone needs it.
                    if let Some((shared, ..)) = &tv_flags { shared.want_converted.store(need_converted, Ordering::Release); }
                    let failed: Vec<String> = incoming.iter().filter(|(_, v)| v.link.failed.load(Ordering::Acquire)).map(|(id, _)| id.clone()).collect();
                    for id in failed { if let Some(view) = incoming.remove(&id) { view.stop.store(true, Ordering::Release); view.link.close(); if let Ok(mut slot) = pictures.lock() { slot.remove(&id); } let _ = output.send(Update::ViewerGone(id)).await; } }
                }
            }
        }
        if let Some(old) = sharing.take() { old.stop.store(true, Ordering::Release); }
        stop_tv(&mut tv);
        for (_, view) in incoming.drain() { view.stop.store(true, Ordering::Release); view.link.close(); }
        for (_, out) in outgoing.drain() { out.link.close(); }
        std::future::pending::<()>().await;
    })
}

/// Everything that can be shared right now, for the picker.
#[derive(Clone, Debug)]
pub struct Shareable { pub label: String, pub source: Source, pub sound: Sound, pub width: u32, pub height: u32, pub monitor: bool }

pub fn list_shareable() -> Vec<Shareable> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let mut list: Vec<Shareable> = Vec::new();
    unsafe extern "system" fn monitor(handle: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        unsafe {
            let list = &mut *(data as *mut Vec<Shareable>);
            let mut info: MONITORINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(handle, &mut info) != 0 {
                let (width, height) = ((info.rcMonitor.right - info.rcMonitor.left) as u32, (info.rcMonitor.bottom - info.rcMonitor.top) as u32);
                let number = list.iter().filter(|s| s.monitor).count() + 1;
                list.push(Shareable { label: format!("Screen {number} ({width}x{height})"), source: Source::Monitor(handle as isize), sound: Sound::AllExceptProcess(GetCurrentProcessId()), width, height, monitor: true });
            }
        }
        1
    }
    unsafe extern "system" fn window(handle: HWND, data: LPARAM) -> BOOL {
        unsafe {
            let list = &mut *(data as *mut Vec<Shareable>);
            let mut process = 0;
            GetWindowThreadProcessId(handle, &mut process);
            let mut cloaked = 0u32;
            DwmGetWindowAttribute(handle, DWMWA_CLOAKED as u32, &mut cloaked as *mut _ as *mut _, 4);
            let style = GetWindowLongW(handle, GWL_EXSTYLE) as u32;
            if process == GetCurrentProcessId() || IsWindowVisible(handle) == 0 || IsIconic(handle) != 0 || cloaked != 0 || style & WS_EX_TOOLWINDOW != 0 || !GetWindow(handle, GW_OWNER).is_null() { return 1; }
            let mut title = [0u16; 200];
            let length = GetWindowTextW(handle, title.as_mut_ptr(), 200);
            let mut rect: RECT = std::mem::zeroed();
            GetWindowRect(handle, &mut rect);
            let (width, height) = ((rect.right - rect.left) as u32, (rect.bottom - rect.top) as u32);
            if length > 0 && width > 100 && height > 100 {
                list.push(Shareable { label: String::from_utf16_lossy(&title[..length as usize]), source: Source::Window(handle as isize), sound: Sound::OnlyProcess(process), width, height, monitor: false });
            }
        }
        1
    }
    unsafe {
        EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(monitor), &mut list as *mut _ as LPARAM);
        EnumWindows(Some(window), &mut list as *mut _ as LPARAM);
    }
    list
}

/// Development checks share Sonar's own window instead of the real screen.
#[cfg(debug_assertions)]
pub fn own_window() -> Option<isize> {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible};
    unsafe extern "system" fn visit(window: HWND, data: LPARAM) -> BOOL {
        unsafe {
            let mut owner = 0;
            GetWindowThreadProcessId(window, &mut owner);
            if owner == GetCurrentProcessId() && IsWindowVisible(window) != 0 { *(data as *mut isize) = window as isize; return 0; }
        }
        1
    }
    let mut found = 0isize;
    unsafe { EnumWindows(Some(visit), &mut found as *mut _ as LPARAM); }
    (found != 0).then_some(found)
}

#[cfg(test)]
mod tv_tests {
    use super::*;
    use crate::tv::player::{self, Out, Shared};
    use std::io::{BufRead, Read, Write};

    /// A desktop viewer gets the channel's original stream, B-frames and all: two of this
    /// module's TV links (sharer and viewer) carry the untouched access units, and the
    /// viewer's normal receive path (depacketizer and decoder) shows every frame in order.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn tv_share_reaches_a_desktop_viewer_unchanged() {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/tv/box_bframes.ts")).unwrap();
        let mut demuxer = crate::tv::demux::Demuxer::new();
        let mut units = Vec::new();
        demuxer.push(&bytes, &mut units);
        demuxer.finish(&mut units);
        let frames: Vec<Vec<u8>> = units.into_iter().filter_map(|u| if let crate::tv::demux::Unit::Video { data, .. } = u { Some(data) } else { None }).collect();
        let mut sharer = link(vec![], false, None, true).await.unwrap();
        let mut video = OutTrack::create(&sharer.connection, "screen-video", RtpCodecKind::Video, RTCRtpCodec { mime_type: MIME_TYPE_H264.into(), clock_rate: 90000, channels: 0, sdp_fmtp_line: H264_HIGH.into(), rtcp_feedback: vec![] }, MIME_TYPE_H264).await.unwrap();
        let description = sharer.connection.create_offer(None).await.unwrap();
        sharer.connection.set_local_description(description).await.unwrap();
        let offer = sharer.local_sdp().await.unwrap();
        let (rtp_tx, mut rtp_rx) = mpsc::channel(4096);
        let mut viewer = link(vec![], false, Some(rtp_tx), true).await.unwrap();
        viewer.set_remote(RTCSessionDescription::offer(offer).unwrap()).await.unwrap();
        let answer = viewer.connection.create_answer(None).await.unwrap();
        viewer.connection.set_local_description(answer).await.unwrap();
        let answer = viewer.local_sdp().await.unwrap();
        sharer.set_remote(RTCSessionDescription::answer(answer).unwrap()).await.unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !(sharer.connected.load(Ordering::Acquire) && viewer.connected.load(Ordering::Acquire)) { assert!(Instant::now() < deadline, "connected"); tokio::time::sleep(Duration::from_millis(20)).await; }
        for frame in &frames { video.write(frame, Duration::from_millis(40)).await.unwrap(); tokio::time::sleep(Duration::from_millis(4)).await; }
        let mut depacketizer = crate::screen_decode::Depacketizer::default();
        let mut received = Vec::new();
        while let Ok(Some(item)) = tokio::time::timeout(Duration::from_secs(2), rtp_rx.recv()).await {
            if let Rtp::Video(payload, marker) = item { if let Some(unit) = depacketizer.push(&payload, marker) { received.push(unit); } }
        }
        let mut decoder = Decoder::new().unwrap();
        let mut xs = Vec::new();
        let mut read = |y: &[u8], _: &[u8], stride: usize, width: usize, _: usize| xs.push(y[80 * stride + 8..80 * stride + width].iter().position(|&v| v > 128).map_or(-1, |x| x as i64 + 8));
        for unit in &received { decoder.decode(unit, &mut read).unwrap(); }
        decoder.drain(&mut read).unwrap();
        let xs: Vec<i64> = xs.into_iter().filter(|&x| x >= 0).collect();
        sharer.close(); viewer.close();
        assert_eq!(received.len(), frames.len(), "every original frame arrived");
        assert_eq!(xs.len(), 50, "every frame was shown");
        assert_eq!(xs.windows(2).filter(|w| w[1] < w[0] && w[0] - w[1] < 200).count(), 0, "in display order: {xs:?}");
    }

    /// End to end with a real browser: a 1080p50 channel goes through the TV player,
    /// this module's own High-profile link and tracks, and the same rule the share loop
    /// uses to pick the original or the converted copy, to Edge (as the web app plays
    /// it). Without B-frames the browser must get the original; with them, the
    /// converted copy; either way every frame in order, full size, with sound.
    /// Needs ffmpeg, Node with Playwright (backend/node_modules) and Edge:
    /// cargo test tv_share_reaches_a_browser -- --ignored --nocapture
    /// This process's memory in MB (working set), to see what a TV share costs.
    fn memory_mb() -> u64 {
        use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb); }
        (counters.WorkingSetSize / (1024 * 1024)) as u64
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore]
    async fn tv_share_reaches_a_browser() {
        let folder = std::env::temp_dir().join(format!("sonar-tv-e2e-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let only = std::env::var("SONAR_TV_E2E").ok();
        for (name, b_frames, expect_original) in [("plain", 0, true), ("bframes", 3, false)] {
            if only.as_deref().is_some_and(|o| o != name) { continue; }
            let file = folder.join(format!("{name}.ts"));
            let made = std::process::Command::new("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i",
                "color=black:s=1920x1080:r=50:d=4[bg];color=white:s=60x60:d=4[b];[bg][b]overlay=x='mod(n*8\\,1800)':y=510",
                "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=4", "-c:v", "libx264", "-preset", "veryfast", "-profile:v", "high", "-pix_fmt", "yuv420p",
                "-g", "50", "-bf", &b_frames.to_string(), "-b:v", "6M", "-c:a", "aac", "-ac", "2", "-b:a", "128k", "-f", "mpegts"]).arg(&file).status().expect("ffmpeg");
            assert!(made.success());
            let bytes = std::fs::read(&file).unwrap();
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = format!("http://{}/live/sonar-test-user/sonar-secret-9f3/1.ts", listener.local_addr().unwrap());
            std::thread::spawn(move || for mut stream in listener.incoming().flatten() {
                let mut request = [0u8; 2048];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len()).as_bytes());
                let _ = stream.write_all(&bytes);
            });

            // The viewer: Edge, through the same High-profile link a share uses.
            let mut link = link(vec![], false, None, true).await.unwrap();
            let mut video = OutTrack::create(&link.connection, "screen-video", RtpCodecKind::Video, RTCRtpCodec { mime_type: MIME_TYPE_H264.into(), clock_rate: 90000, channels: 0, sdp_fmtp_line: H264_HIGH.into(), rtcp_feedback: vec![] }, MIME_TYPE_H264).await.unwrap();
            let mut audio = OutTrack::create(&link.connection, "screen-audio", RtpCodecKind::Audio, RTCRtpCodec { mime_type: MIME_TYPE_OPUS.into(), clock_rate: 48000, channels: 2, sdp_fmtp_line: String::new(), rtcp_feedback: vec![] }, MIME_TYPE_OPUS).await.unwrap();
            let description = link.connection.create_offer(None).await.unwrap();
            link.connection.set_local_description(description).await.unwrap();
            let sdp = link.local_sdp().await.unwrap();
            eprintln!("{name}: offer ready");
            let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../backend/scripts/test-tv-browser.mjs");
            let mut node = std::process::Command::new("node").arg(script).current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../backend"))
                .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().expect("node");
            let mut to_node = node.stdin.take().unwrap();
            let mut from_node = std::io::BufReader::new(node.stdout.take().unwrap()).lines();
            writeln!(to_node, "{}", json!({ "type": "offer", "sdp": sdp })).unwrap();
            let answer: Value = serde_json::from_str(&from_node.next().unwrap().unwrap()).unwrap();
            link.set_remote(RTCSessionDescription::answer(answer["sdp"].as_str().unwrap().to_owned()).unwrap()).await.unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while !link.connected.load(Ordering::Acquire) { assert!(Instant::now() < deadline, "connected"); tokio::time::sleep(Duration::from_millis(20)).await; }
            eprintln!("{name}: connected");

            // Privacy: nothing that goes to the viewer or the server carries the address or login.
            assert!(!sdp.contains("sonar-secret-9f3") && !sdp.contains("sonar-test-user") && !sdp.contains("/live/"), "{name}: the offer holds no address or login");
            let before = memory_mb();
            let mut peak = before;
            // The share loop's rule for a browser viewer (not "desktop").
            let shared = Arc::new(Shared::default());
            let (tx, mut rx) = mpsc::channel(64);
            player::start(zeroize::Zeroizing::new(address), shared.clone(), tx, player::Local { pictures: Arc::new(Mutex::new(HashMap::new())), key: "me".into(), playback: None, source: 1, notify: Box::new(|| {}) });
            let (mut originals, mut converted) = (0, 0);
            while let Some(item) = tokio::time::timeout(Duration::from_secs(20), rx.recv()).await.expect("player output") {
                peak = peak.max(memory_mb());
                if let Out::Info(text) | Out::Failed(text) = &item { assert!(!text.contains("sonar-secret-9f3") && !text.contains("127.0.0.1"), "{name}: status text holds no address: {text}"); }
                let original = shared.native.load(Ordering::Acquire) && shared.web_native.load(Ordering::Acquire);
                shared.want_converted.store(!original, Ordering::Release);
                match item {
                    Out::Native(data, duration) if original => { video.write(&data, duration).await.unwrap(); originals += 1; }
                    Out::Converted(data, duration) if !original => { video.write(&data, duration).await.unwrap(); converted += 1; if converted % 50 == 0 { eprintln!("{name}: {converted} converted"); } }
                    Out::Audio(packet) => { audio.write(&packet, Duration::from_millis(20)).await.unwrap(); }
                    Out::Failed(message) => panic!("{message}"),
                    Out::Ended => { eprintln!("{name}: ended"); break }
                    _ => {}
                }
            }
            writeln!(to_node, "measure").unwrap();
            let seen: Value = serde_json::from_str(&from_node.next().unwrap().unwrap()).unwrap();
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let after = memory_mb();
            println!("{name}: sent {originals} original and {converted} converted frames; Edge saw {seen}");
            println!("{name}: memory {before} MB before, {peak} MB at most while sharing 1080p50 (+{} MB), {after} MB after stopping", peak.saturating_sub(before));
            drop(to_node);
            let _ = node.wait();
            link.close();
            assert_eq!(seen["size"], "1920x1080", "{name}: full size");
            assert!(seen["framesDecoded"].as_u64().unwrap() >= 150, "{name}: most of the 200 frames decoded: {seen}");
            assert_eq!(seen["boxBackwards"], 0, "{name}: every frame in display order: {seen}");
            assert!(seen["soundPeak"].as_f64().unwrap() > 0.05, "{name}: the tone is heard: {seen}");
            if expect_original { assert!(originals >= 140 && converted < 60, "{name}: the browser got the original stream"); }
            else { assert!(originals == 0 && converted >= 150, "{name}: B-frames, so the browser got the converted copy"); }
        }
        let _ = std::fs::remove_dir_all(folder);
    }
}
