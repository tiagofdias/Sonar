//! Native WebRTC voice transport. Not yet exposed to the UI or signaling.
//! The remote fingerprint must arrive over a separately authenticated channel.
#![allow(dead_code)]
use std::{sync::{Arc, atomic::{AtomicU8, AtomicU64, Ordering}}, time::{Duration, Instant}};
use rtc::{media::Sample, media_stream::MediaStreamTrack, peer_connection::{configuration::{RTCConfigurationBuilder, RTCIceServer, RTCIceTransportPolicy}, sdp::RTCSessionDescription}, rtp_transceiver::rtp_sender::{RTCRtpCodec, RTCRtpCodingParameters, RTCRtpEncodingParameters, RtpCodecKind}};
use rtc::peer_connection::configuration::media_engine::MIME_TYPE_OPUS;
use rtc::peer_connection::transport::RTCIceCandidateInit;
use webrtc::{media_stream::{Track, track_local::{TrackLocal, static_sample::TrackLocalStaticSample}, track_remote::{TrackRemote, TrackRemoteEvent}}, peer_connection::{MediaEngine, PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCIceGatheringState, RTCPeerConnectionState}, rtp_transceiver::RtpSender};
use tokio::sync::mpsc;
use zeroize::Zeroizing;

struct Handler { gathered: mpsc::Sender<()>, candidates: mpsc::Sender<RTCIceCandidateInit>, connected: mpsc::Sender<()>, packets: mpsc::Sender<(u64, Zeroizing<Vec<u8>>)>, state: Arc<AtomicU8>, generation: Arc<AtomicU64> }
#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete { let _ = self.gathered.try_send(()); }
    }
    /// Each local network route as soon as it is found, so it can be sent without waiting for the rest.
    async fn on_ice_candidate(&self, event: webrtc::peer_connection::RTCPeerConnectionIceEvent) {
        if let Ok(candidate) = event.candidate.to_json() { let _ = self.candidates.try_send(candidate); }
    }
    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        let next = match state { RTCPeerConnectionState::Connected => 1, RTCPeerConnectionState::Disconnected => 2, RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed => 3, _ => 0 };
        if self.state.swap(next, Ordering::AcqRel) != next { self.generation.fetch_add(1, Ordering::AcqRel); }
        if state == RTCPeerConnectionState::Connected { let _ = self.connected.try_send(()); }
    }
    async fn on_track(&self, track: Arc<dyn TrackRemote>) {
        let packets = self.packets.clone();
        let state = self.state.clone();
        let generation = self.generation.clone();
        tokio::spawn(async move {
            while let Some(event) = track.poll().await {
                if let TrackRemoteEvent::OnRtpPacket(packet) = event {
                    let epoch = generation.load(Ordering::Acquire);
                    if state.load(Ordering::Acquire) == 1 && !packet.payload.is_empty() && packet.payload.len() <= 8192 {
                        let _ = packets.try_send((epoch, Zeroizing::new(packet.payload.to_vec())));
                    }
                }
            }
        });
    }
}

// Enforce one SHA-256 fingerprint throughout an SDP, including media sections.
// The caller must independently authenticate the expected fingerprint.
pub fn fingerprint(sdp: &str) -> Result<String, &'static str> {
    if sdp.len() > 32000 { return Err("Call description too large"); }
    let mut found: Option<String> = None;
    for line in sdp.lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("a=fingerprint:") {
            let (algorithm, value) = value.split_once(' ').ok_or("Invalid call fingerprint")?;
            if !algorithm.eq_ignore_ascii_case("sha-256") || value.len() != 95 || !value.bytes().enumerate().all(|(i, b)| if i % 3 == 2 { b == b':' } else { b.is_ascii_hexdigit() }) { return Err("Invalid call fingerprint"); }
            let normalized = value.to_ascii_uppercase();
            if found.as_ref().is_some_and(|old| old != &normalized) { return Err("Inconsistent call fingerprints"); }
            found = Some(normalized);
        }
    }
    found.ok_or("Missing call fingerprint")
}

/// The media section name of our only (audio) section, from `a=mid:`.
fn local_mid(sdp: &str) -> String {
    sdp.lines().find_map(|line| line.trim().strip_prefix("a=mid:")).map(str::to_owned).filter(|mid| !mid.is_empty() && mid.len() <= 32).unwrap_or_else(|| "0".into())
}

fn receive_current(incoming: &mut mpsc::Receiver<(u64, Zeroizing<Vec<u8>>)>, state: &AtomicU8, generation: &AtomicU64) -> Option<Zeroizing<Vec<u8>>> {
    while let Ok((epoch, packet)) = incoming.try_recv() {
        if state.load(Ordering::Acquire) == 1 && epoch == generation.load(Ordering::Acquire) { return Some(packet); }
    }
    None
}

/// Diagnostics: packets sent and received by all voice transports.
pub static PACKETS_SENT: AtomicU64 = AtomicU64::new(0);
pub static PACKETS_RECEIVED: AtomicU64 = AtomicU64::new(0);

pub struct VoiceTransport {
    connection: Arc<dyn PeerConnection>,
    track: Arc<TrackLocalStaticSample>,
    sender: Arc<dyn RtpSender>,
    gathered: mpsc::Receiver<()>,
    candidates: mpsc::Receiver<RTCIceCandidateInit>,
    connected: mpsc::Receiver<()>,
    incoming: mpsc::Receiver<(u64, Zeroizing<Vec<u8>>)>,
    state: Arc<AtomicU8>,
    generation: Arc<AtomicU64>,
    pumped_generation: u64,
    closed: bool,
}
impl VoiceTransport {
    pub async fn new(servers: Vec<RTCIceServer>, relay_only: bool, bind: Vec<String>) -> Result<Self, String> {
        if relay_only && !servers.iter().any(|server| server.urls.iter().any(|url| url.starts_with("turn:") || url.starts_with("turns:"))) { return Err("Relay-only mode needs a TURN server".into()); }
        let config = RTCConfigurationBuilder::new().with_ice_servers(servers).with_ice_transport_policy(if relay_only { RTCIceTransportPolicy::Relay } else { RTCIceTransportPolicy::All }).build();
        let mut media = MediaEngine::default(); media.register_default_codecs().map_err(|_| "Voice codec unavailable")?;
        let (gather_tx, gathered) = mpsc::channel(1); let (candidate_tx, candidates) = mpsc::channel(64); let (connected_tx, connected) = mpsc::channel(1); let (packet_tx, incoming) = mpsc::channel(20);
        let state = Arc::new(AtomicU8::new(0));
        let generation = Arc::new(AtomicU64::new(0));
        let handler = Handler { gathered: gather_tx, candidates: candidate_tx, connected: connected_tx, packets: packet_tx, state: state.clone(), generation: generation.clone() };
        let connection: Arc<dyn PeerConnection> = Arc::new(PeerConnectionBuilder::new().with_configuration(config).with_media_engine(media).with_udp_addrs(bind).with_handler(Arc::new(handler)).build().await.map_err(|_| "Could not start the voice connection")?);
        let ssrc = rand_core::OsRng.next_u32();
        let track = Arc::new(TrackLocalStaticSample::new(Instant::now(), MediaStreamTrack::new("sonar-voice".into(), "audio".into(), "audio".into(), RtpCodecKind::Audio,
            vec![RTCRtpEncodingParameters { rtp_coding_parameters: RTCRtpCodingParameters { ssrc: Some(ssrc), ..Default::default() }, codec: RTCRtpCodec { mime_type: MIME_TYPE_OPUS.into(), clock_rate: 48000, channels: 2, sdp_fmtp_line: String::new(), rtcp_feedback: vec![] }, ..Default::default() }])).map_err(|_| "Could not create the audio track")?);
        let sender = connection.add_track(track.clone() as Arc<dyn TrackLocal>).await.map_err(|_| "Could not add audio to the call")?;
        Ok(Self { connection, track, sender, gathered, candidates, connected, incoming, state, generation, pumped_generation: 0, closed: false })
    }
    async fn local(&mut self) -> Result<(String, String), String> {
        tokio::time::timeout(Duration::from_secs(15), self.gathered.recv()).await.map_err(|_| "Network setup timed out")?.ok_or("Network setup was interrupted")?;
        let sdp = self.connection.local_description().await.ok_or("SDP local ausente")?.sdp;
        let fingerprint = fingerprint(&sdp).map_err(str::to_owned)?;
        Ok((sdp, fingerprint))
    }
    pub async fn offer(&mut self) -> Result<(String, String), String> {
        let offer = self.connection.create_offer(None).await.map_err(|_| "Could not create the call offer")?;
        self.connection.set_local_description(offer).await.map_err(|_| "Could not prepare the call offer")?;
        self.local().await
    }
    /// An offer sent right away; its network routes follow through `next_local_candidate`.
    pub async fn offer_now(&mut self) -> Result<String, String> {
        let offer = self.connection.create_offer(None).await.map_err(|_| "Could not create the call offer")?;
        self.connection.set_local_description(offer).await.map_err(|_| "Could not prepare the call offer")?;
        Ok(self.connection.local_description().await.ok_or("Could not prepare the call offer")?.sdp)
    }
    /// Answer an authenticated offer right away; network routes follow through `next_local_candidate`.
    pub async fn answer_now(&mut self, remote_sdp: String, verified_fingerprint: &str) -> Result<String, String> {
        self.set_remote(remote_sdp, verified_fingerprint, true).await?;
        let answer = self.connection.create_answer(None).await.map_err(|_| "Could not answer the call")?;
        self.connection.set_local_description(answer).await.map_err(|_| "Could not prepare the call answer")?;
        Ok(self.connection.local_description().await.ok_or("Could not prepare the call answer")?.sdp)
    }
    /// The next local network route found since the last call, labelled with our audio section so browsers accept it.
    pub async fn next_local_candidate(&mut self) -> Option<RTCIceCandidateInit> {
        let mut candidate = self.candidates.try_recv().ok()?;
        let sdp = self.connection.local_description().await.map(|d| d.sdp).unwrap_or_default();
        candidate.sdp_mid = Some(local_mid(&sdp));
        candidate.sdp_mline_index = Some(0);
        Some(candidate)
    }
    pub async fn answer(&mut self, remote_sdp: String, verified_fingerprint: &str) -> Result<(String, String), String> {
        self.answer_with_candidates(remote_sdp, verified_fingerprint, Vec::new()).await
    }
    /// Apply the authenticated offer before processing the scoped, queued ICE
    /// candidates. The expected fingerprint must come from an authenticated
    /// channel, not from the untrusted SDP or signaling server.
    pub async fn answer_with_candidates(&mut self, remote_sdp: String, verified_fingerprint: &str, candidates: Vec<RTCIceCandidateInit>) -> Result<(String, String), String> {
        if candidates.len() > 128 || candidates.iter().any(|candidate| !crate::ice_candidate::valid_remote_candidate(candidate)) { return Err("Invalid network candidates".into()); }
        self.set_remote(remote_sdp, verified_fingerprint, true).await?;
        for candidate in candidates { self.add_remote_candidate(candidate).await?; }
        let answer = self.connection.create_answer(None).await.map_err(|_| "Could not answer the call")?;
        self.connection.set_local_description(answer).await.map_err(|_| "Could not prepare the call answer")?;
        self.local().await
    }
    pub async fn set_remote(&self, sdp: String, verified_fingerprint: &str, offer: bool) -> Result<(), String> {
        if fingerprint(&sdp).map_err(str::to_owned)? != verified_fingerprint.to_ascii_uppercase() { return Err("The call identity does not match".into()); }
        let description = if offer { RTCSessionDescription::offer(sdp) } else { RTCSessionDescription::answer(sdp) }.map_err(|_| "Invalid call description")?;
        self.connection.set_remote_description(description).await.map_err(|_| "Could not apply the call description".into())
    }
    /// Add browser trickle-ICE after the remote description has been checked.
    /// The caller must scope each candidate to the active peer and call ID.
    pub async fn add_remote_candidate(&self, candidate: RTCIceCandidateInit) -> Result<(), String> {
        if !crate::ice_candidate::valid_remote_candidate(&candidate) {
            return Err("Invalid network candidate".into());
        }
        self.connection.add_ice_candidate(candidate).await.map_err(|_| "Could not apply a network candidate".into())
    }
    pub async fn wait_connected(&mut self) -> Result<(), String> {
        tokio::time::timeout(Duration::from_secs(20), self.connected.recv()).await.map_err(|_| "The voice connection timed out")?.ok_or("The voice connection was interrupted")?;
        if self.state.load(Ordering::Acquire) != 1 { return Err("The voice connection was interrupted".into()); }
        Ok(())
    }
    pub async fn send_packet(&self, packet: &[u8]) -> Result<(), String> {
        if packet.is_empty() || packet.len() > 1275 { return Err("Invalid audio packet".into()); }
        let parameters = self.sender.get_parameters().await.map_err(|_| "Audio parameters unavailable")?;
        let payload = parameters.rtp_parameters.codecs.iter().find(|codec| codec.rtp_codec.mime_type.eq_ignore_ascii_case(MIME_TYPE_OPUS)).ok_or("Voice codec was not agreed")?.payload_type;
        let ssrc = *self.track.ssrcs().await.first().ok_or("Audio track unavailable")?;
        self.track.sample_writer(ssrc, payload).write_sample(&Sample { data: bytes::Bytes::copy_from_slice(packet), duration: Duration::from_millis(20), ..Sample::new(Instant::now()) }).await.map_err(|_| "Could not send audio".to_string())?;
        PACKETS_SENT.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    pub fn try_receive(&mut self) -> Option<Zeroizing<Vec<u8>>> {
        let packet = receive_current(&mut self.incoming, &self.state, &self.generation);
        if packet.is_some() { PACKETS_RECEIVED.fetch_add(1, Ordering::Relaxed); }
        packet
    }
    pub fn disconnected(&self) -> bool { self.state.load(Ordering::Acquire) == 2 }
    pub fn connected(&self) -> bool { self.state.load(Ordering::Acquire) == 1 }
    pub fn failed(&self) -> bool { self.state.load(Ordering::Acquire) == 3 }
    /// Call regularly while connected; no device stream is started implicitly.
    pub async fn pump(&mut self, audio: &crate::voice_audio::VoiceAudio) -> Result<(), String> {
        if audio.failed() { return Err("Your audio device stopped working".into()); }
        let generation = self.generation.load(Ordering::Acquire);
        if generation != self.pumped_generation {
            audio.discard_pending();
            while self.incoming.try_recv().is_ok() {}
            self.pumped_generation = generation;
        }
        if self.state.load(Ordering::Acquire) == 3 { return Err("The voice connection was interrupted".into()); }
        if self.state.load(Ordering::Acquire) != 1 { audio.discard_pending(); while self.incoming.try_recv().is_ok() {} return Ok(()); }
        while let Some(packet) = audio.next_packet() { self.send_packet(&packet).await?; }
        while let Some(packet) = self.try_receive() { audio.play_packet(&packet).map_err(str::to_owned)?; }
        Ok(())
    }
    pub async fn close(mut self) { let _ = self.connection.close().await; self.closed = true; }
}
impl Drop for VoiceTransport {
    fn drop(&mut self) {
        if !self.closed {
            let connection = self.connection.clone();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() { runtime.spawn(async move { let _ = connection.close().await; }); }
        }
    }
}
use rand_core::RngCore;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice_codec::{VoiceEncoder, VoiceDecoder, FRAME_SAMPLES};
    #[tokio::test]
    async fn connection_state_tracks_loss_recovery_and_failure() {
        let (gathered, _) = mpsc::channel(1);
        let (candidates, _) = mpsc::channel(1);
        let (connected, mut connected_rx) = mpsc::channel(1);
        let (packets, _) = mpsc::channel(1);
        let state = Arc::new(AtomicU8::new(0));
        let generation = Arc::new(AtomicU64::new(0));
        let handler = Handler { gathered, candidates, connected, packets, state: state.clone(), generation: generation.clone() };
        handler.on_connection_state_change(RTCPeerConnectionState::Connected).await;
        assert_eq!(state.load(Ordering::Acquire), 1);
        assert!(connected_rx.try_recv().is_ok());
        handler.on_connection_state_change(RTCPeerConnectionState::Disconnected).await;
        assert_eq!(state.load(Ordering::Acquire), 2);
        handler.on_connection_state_change(RTCPeerConnectionState::Connected).await;
        assert_eq!(state.load(Ordering::Acquire), 1);
        handler.on_connection_state_change(RTCPeerConnectionState::Failed).await;
        assert_eq!(state.load(Ordering::Acquire), 3);
        assert_eq!(generation.load(Ordering::Acquire), 4);
    }
    #[test]
    fn fingerprints_reject_absent_conflicting_and_non_sha256_values() {
        let good = (0..32).map(|_| "AB").collect::<Vec<_>>().join(":");
        assert_eq!(fingerprint(&format!("v=0\r\na=fingerprint:sha-256 {good}\r\n")).unwrap(), good);
        assert!(fingerprint("v=0\r\n").is_err());
        assert!(fingerprint(&format!("a=fingerprint:sha-1 {good}")).is_err());
        assert!(fingerprint(&format!("a=fingerprint:sha-256 {good}\r\na=fingerprint:sha-256 {}", (0..32).map(|_| "CD").collect::<Vec<_>>().join(":"))).is_err());
    }
    #[test]
    fn received_audio_is_scoped_to_the_current_connection_generation() {
        let (tx, mut rx) = mpsc::channel(4);
        let state = AtomicU8::new(1);
        let generation = AtomicU64::new(2);
        tx.try_send((1, Zeroizing::new(vec![1]))).unwrap();
        tx.try_send((2, Zeroizing::new(vec![2]))).unwrap();
        assert_eq!(&*receive_current(&mut rx, &state, &generation).unwrap(), &[2]);
        state.store(2, Ordering::Release);
        tx.try_send((2, Zeroizing::new(vec![3]))).unwrap();
        assert!(receive_current(&mut rx, &state, &generation).is_none());
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn two_native_peers_exchange_opus_after_fingerprint_verification() {
        let mut a = VoiceTransport::new(vec![], false, vec!["127.0.0.1:0".into()]).await.unwrap();
        let mut b = VoiceTransport::new(vec![], false, vec!["127.0.0.1:0".into()]).await.unwrap();
        let (offer, afp) = a.offer().await.unwrap();
        assert!(b.answer(offer.clone(), &format!("{}AA", "AA:".repeat(31))).await.is_err());
        let candidates: Vec<_> = offer.lines().filter_map(|line| line.trim().strip_prefix("a=candidate:")).map(|value| RTCIceCandidateInit { candidate: format!("candidate:{value}"), sdp_mline_index: Some(0), ..Default::default() }).collect();
        assert!(!candidates.is_empty());
        let stripped = offer.lines().filter(|line| !line.trim().starts_with("a=candidate:") && line.trim() != "a=end-of-candidates").map(|line| format!("{}\r\n", line.trim_end())).collect::<String>();
        let (answer, bfp) = b.answer_with_candidates(stripped, &afp, candidates).await.unwrap();
        assert!(b.add_remote_candidate(RTCIceCandidateInit { candidate: "invalid".into(), sdp_mid: Some("0".into()), ..Default::default() }).await.is_err());
        a.set_remote(answer, &bfp, false).await.unwrap();
        let (a_connected, b_connected) = tokio::join!(a.wait_connected(), b.wait_connected());
        a_connected.unwrap(); b_connected.unwrap();
        let mut enc_a = VoiceEncoder::new().unwrap(); let mut enc_b = VoiceEncoder::new().unwrap();
        let frame = vec![0.1; FRAME_SAMPLES];
        for _ in 0..10 {
            a.send_packet(&enc_a.encode(&frame).unwrap()).await.unwrap();
            b.send_packet(&enc_b.encode(&frame).unwrap()).await.unwrap();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let received = tokio::time::timeout(Duration::from_secs(5), async {
            loop { if let (Some(a_packet), Some(b_packet)) = (a.try_receive(), b.try_receive()) { break (a_packet, b_packet); } tokio::time::sleep(Duration::from_millis(10)).await; }
        }).await.unwrap();
        assert_eq!(VoiceDecoder::new().unwrap().decode(&received.0).unwrap().len(), FRAME_SAMPLES);
        assert_eq!(VoiceDecoder::new().unwrap().decode(&received.1).unwrap().len(), FRAME_SAMPLES);
        a.close().await; b.close().await;
    }
    #[test]
    fn local_mid_reads_the_audio_section_name() {
        assert_eq!(local_mid("v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=mid:0\r\n"), "0");
        assert_eq!(local_mid("v=0\r\na=mid:audio\r\n"), "audio");
        assert_eq!(local_mid("v=0\r\n"), "0");
    }
    /// Rooms send the offer before any route is known and trickle routes afterwards, like browsers do.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn trickled_routes_connect_two_native_peers_quickly() {
        let started = Instant::now();
        let mut a = VoiceTransport::new(vec![], false, vec!["127.0.0.1:0".into()]).await.unwrap();
        let mut b = VoiceTransport::new(vec![], false, vec!["127.0.0.1:0".into()]).await.unwrap();
        let offer = a.offer_now().await.unwrap();
        let answer = b.answer_now(offer.clone(), &fingerprint(&offer).unwrap()).await.unwrap();
        a.set_remote(answer.clone(), &fingerprint(&answer).unwrap(), false).await.unwrap();
        let exchanged = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                while let Some(candidate) = a.next_local_candidate().await { assert!(crate::ice_candidate::valid_remote_candidate(&candidate)); b.add_remote_candidate(candidate).await.unwrap(); }
                while let Some(candidate) = b.next_local_candidate().await { a.add_remote_candidate(candidate).await.unwrap(); }
                if a.connected() && b.connected() { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await;
        assert!(exchanged.is_ok(), "trickled routes did not connect");
        assert!(started.elapsed() < Duration::from_secs(5));
        a.close().await; b.close().await;
    }
}
