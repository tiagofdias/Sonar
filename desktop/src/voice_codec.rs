//! Opus voice frames for the native WebRTC path. No device or file is opened.
//! The transport must provide authenticated encryption (DTLS-SRTP).
#![allow(dead_code)]
use opus::{Application, Bitrate, Channels, Decoder, Encoder};
use zeroize::Zeroizing;

pub const SAMPLE_RATE: u32 = 48_000;
pub const FRAME_SAMPLES: usize = 960; // 20 ms, mono
const MAX_DECODED_SAMPLES: usize = 5760; // 120 ms, per RFC 6716
const MAX_ENCODED_BYTES: usize = 1275;
const MAX_INCOMING_BYTES: usize = 8192;

pub struct VoiceEncoder(Encoder);
impl VoiceEncoder {
    pub fn new() -> Result<Self, &'static str> {
        let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Voip).map_err(|_| "opus encoder unavailable")?;
        encoder.set_bitrate(Bitrate::Bits(32_000)).map_err(|_| "opus configuration failed")?;
        Ok(Self(encoder))
    }
    pub fn encode(&mut self, frame: &[f32]) -> Result<Zeroizing<Vec<u8>>, &'static str> {
        if frame.len() != FRAME_SAMPLES || frame.iter().any(|value| !value.is_finite() || value.abs() > 1.0) { return Err("invalid audio frame"); }
        let mut packet = Zeroizing::new(vec![0u8; MAX_ENCODED_BYTES]);
        let length = self.0.encode_float(frame, &mut packet).map_err(|_| "opus encoding failed")?;
        packet.truncate(length);
        Ok(packet)
    }
}
pub struct VoiceDecoder(Decoder);
impl VoiceDecoder {
    pub fn new() -> Result<Self, &'static str> {
        Ok(Self(Decoder::new(SAMPLE_RATE, Channels::Mono).map_err(|_| "opus decoder unavailable")?))
    }
    pub fn decode(&mut self, packet: &[u8]) -> Result<Zeroizing<Vec<f32>>, &'static str> {
        if packet.is_empty() || packet.len() > MAX_INCOMING_BYTES { return Err("invalid opus packet"); }
        let mut samples = Zeroizing::new(vec![0.0f32; MAX_DECODED_SAMPLES]);
        let length = self.0.decode_float(packet, &mut samples, false).map_err(|_| "opus decoding failed")?;
        samples.truncate(length);
        Ok(samples)
    }
    pub fn conceal_lost_frame(&mut self) -> Result<Zeroizing<Vec<f32>>, &'static str> {
        let mut samples = Zeroizing::new(vec![0.0f32; FRAME_SAMPLES]);
        let length = self.0.decode_float(&[], &mut samples, false).map_err(|_| "opus concealment failed")?;
        samples.truncate(length);
        Ok(samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synthetic_voice_roundtrips_and_packet_loss_is_bounded() {
        let mut encoder = VoiceEncoder::new().unwrap();
        let mut decoder = VoiceDecoder::new().unwrap();
        let mut peak = 0.0f32;
        for frame_index in 0..30 {
            let samples: Vec<f32> = (0..FRAME_SAMPLES).map(|index| {
                let t = (frame_index * FRAME_SAMPLES + index) as f32 / SAMPLE_RATE as f32;
                (t * 440.0 * std::f32::consts::TAU).sin() * 0.2
            }).collect();
            let packet = encoder.encode(&samples).unwrap();
            assert!(!packet.is_empty() && packet.len() <= MAX_ENCODED_BYTES);
            let output = decoder.decode(&packet).unwrap();
            assert_eq!(output.len(), FRAME_SAMPLES);
            peak = peak.max(output.iter().fold(0.0f32, |max, value| max.max(value.abs())));
        }
        assert!(peak > 0.05 && peak < 1.0);
        assert_eq!(decoder.conceal_lost_frame().unwrap().len(), FRAME_SAMPLES);
        assert!(encoder.encode(&[0.0; 20]).is_err());
        assert!(encoder.encode(&[f32::NAN; FRAME_SAMPLES]).is_err());
        assert!(decoder.decode(&[]).is_err());
        assert!(decoder.decode(&vec![0; MAX_INCOMING_BYTES + 1]).is_err());
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn synthetic_opus_travels_between_native_webrtc_peers() {
        use std::{sync::Arc, time::{Duration, Instant}};
        use rtc::media::Sample;
        use rtc::media_stream::MediaStreamTrack;
        use rtc::peer_connection::configuration::media_engine::MIME_TYPE_OPUS;
        use rtc::rtp_transceiver::rtp_sender::{RTCRtpCodec, RTCRtpCodingParameters, RTCRtpEncodingParameters, RtpCodecKind};
        use webrtc::media_stream::Track;
        use webrtc::media_stream::track_local::{TrackLocal, static_sample::TrackLocalStaticSample};
        use webrtc::media_stream::track_remote::{TrackRemote, TrackRemoteEvent};
        use webrtc::peer_connection::{MediaEngine, PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCIceGatheringState, RTCPeerConnectionState};
        use tokio::sync::mpsc;
        struct Handler { gathered: mpsc::Sender<()>, connected: mpsc::Sender<()>, packets: mpsc::Sender<Vec<u8>> }
        #[async_trait::async_trait]
        impl PeerConnectionEventHandler for Handler {
            async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
                if state == RTCIceGatheringState::Complete { let _ = self.gathered.try_send(()); }
            }
            async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
                if state == RTCPeerConnectionState::Connected { let _ = self.connected.try_send(()); }
            }
            async fn on_track(&self, track: Arc<dyn TrackRemote>) {
                let packets = self.packets.clone();
                tokio::spawn(async move {
                    while let Some(event) = track.poll().await {
                        if let TrackRemoteEvent::OnRtpPacket(packet) = event { let _ = packets.try_send(packet.payload.to_vec()); }
                    }
                });
            }
        }
        let mut media = MediaEngine::default(); media.register_default_codecs().unwrap();
        let (gather_a_tx, mut gather_a) = mpsc::channel(1); let (connected_a_tx, mut connected_a) = mpsc::channel(1);
        let (gather_b_tx, mut gather_b) = mpsc::channel(1); let (connected_b_tx, mut connected_b) = mpsc::channel(1);
        let (packets_tx, mut packets) = mpsc::channel(32);
        let a = PeerConnectionBuilder::new().with_media_engine(media.clone()).with_udp_addrs(vec!["127.0.0.1:0".to_owned()])
            .with_handler(Arc::new(Handler { gathered: gather_a_tx, connected: connected_a_tx, packets: packets_tx.clone() })).build().await.unwrap();
        let b = PeerConnectionBuilder::new().with_media_engine(media).with_udp_addrs(vec!["127.0.0.1:0".to_owned()])
            .with_handler(Arc::new(Handler { gathered: gather_b_tx, connected: connected_b_tx, packets: packets_tx })).build().await.unwrap();
        let track = Arc::new(TrackLocalStaticSample::new(Instant::now(), MediaStreamTrack::new(
            "sonar-test".into(), "audio".into(), "audio".into(), RtpCodecKind::Audio,
            vec![RTCRtpEncodingParameters { rtp_coding_parameters: RTCRtpCodingParameters { ssrc: Some(1234567), ..Default::default() }, codec: RTCRtpCodec { mime_type: MIME_TYPE_OPUS.into(), clock_rate: 48000, channels: 2, sdp_fmtp_line: String::new(), rtcp_feedback: vec![] }, ..Default::default() }],
        )).unwrap());
        let sender = a.add_track(track.clone() as Arc<dyn TrackLocal>).await.unwrap();
        a.set_local_description(a.create_offer(None).await.unwrap()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), gather_a.recv()).await.unwrap().unwrap();
        b.set_remote_description(a.local_description().await.unwrap()).await.unwrap();
        b.set_local_description(b.create_answer(None).await.unwrap()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), gather_b.recv()).await.unwrap().unwrap();
        a.set_remote_description(b.local_description().await.unwrap()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), connected_a.recv()).await.unwrap().unwrap();
        tokio::time::timeout(Duration::from_secs(10), connected_b.recv()).await.unwrap().unwrap();
        let payload_type = sender.get_parameters().await.unwrap().rtp_parameters.codecs.first().unwrap().payload_type;
        let ssrc = *track.ssrcs().await.first().unwrap();
        let mut encoder = VoiceEncoder::new().unwrap(); let mut decoder = VoiceDecoder::new().unwrap();
        for frame in 0..12 {
            let samples: Vec<f32> = (0..FRAME_SAMPLES).map(|index| (((frame * FRAME_SAMPLES + index) as f32 / SAMPLE_RATE as f32) * 440.0 * std::f32::consts::TAU).sin() * 0.2).collect();
            let packet = encoder.encode(&samples).unwrap();
            track.sample_writer(ssrc, payload_type).write_sample(&Sample { data: bytes::Bytes::copy_from_slice(&packet), duration: Duration::from_millis(20), ..Sample::new(Instant::now()) }).await.unwrap();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let payload = tokio::time::timeout(Duration::from_secs(5), packets.recv()).await.unwrap().unwrap();
        assert_eq!(decoder.decode(&payload).unwrap().len(), FRAME_SAMPLES);
        a.close().await.unwrap(); b.close().await.unwrap();
    }
    #[test]
    #[ignore = "requires ffmpeg with libopus; uses synthetic samples only"]
    fn synthetic_opus_interoperates_with_ffmpeg() {
        use ogg::{PacketReader, PacketWriteEndInfo, PacketWriter};
        use std::io::{Cursor, Write};
        use std::process::{Command, Stdio};
        #[cfg(windows)] use std::os::windows::process::CommandExt;
        let mut writer = PacketWriter::new(Cursor::new(Vec::new()));
        let mut head = b"OpusHead".to_vec();
        head.extend([1, 1, 0, 0]); // version, mono, pre-skip
        head.extend(SAMPLE_RATE.to_le_bytes());
        head.extend([0, 0, 0]); // gain, mapping
        writer.write_packet(head, 17, PacketWriteEndInfo::EndPage, 0).unwrap();
        let mut tags = b"OpusTags".to_vec();
        tags.extend(5u32.to_le_bytes()); tags.extend(b"Sonar"); tags.extend(0u32.to_le_bytes());
        writer.write_packet(tags, 17, PacketWriteEndInfo::EndPage, 0).unwrap();
        let mut encoder = VoiceEncoder::new().unwrap();
        for frame in 0..30 {
            let samples: Vec<f32> = (0..FRAME_SAMPLES).map(|index| (((frame * FRAME_SAMPLES + index) as f32 / SAMPLE_RATE as f32) * 440.0 * std::f32::consts::TAU).sin() * 0.2).collect();
            let packet = encoder.encode(&samples).unwrap();
            let end = if frame == 29 { PacketWriteEndInfo::EndStream } else { PacketWriteEndInfo::NormalPacket };
            writer.write_packet(packet.to_vec(), 17, end, ((frame + 1) * FRAME_SAMPLES) as u64).unwrap();
        }
        let encoded = writer.into_inner().into_inner();
        let mut command = Command::new("ffmpeg");
        command.args(["-v", "error", "-c:a", "opus", "-f", "ogg", "-i", "pipe:0", "-f", "f32le", "-ac", "1", "-ar", "48000", "pipe:1"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)] command.creation_flags(0x08000000);
        let mut child = command.spawn().expect("ffmpeg is required");
        child.stdin.take().unwrap().write_all(&encoded).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "ffmpeg rejected native packets: {}", String::from_utf8_lossy(&output.stderr));
        assert!(output.stdout.len() >= 20 * FRAME_SAMPLES * 4);
        let peak = output.stdout.chunks_exact(4).map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()).abs()).fold(0.0f32, f32::max);
        assert!(peak > 0.05 && peak < 1.0);

        let mut command = Command::new("ffmpeg");
        command.args(["-v", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=0.6:sample_rate=48000", "-c:a", "libopus", "-frame_duration", "20", "-ac", "1", "-f", "ogg", "pipe:1"]).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)] command.creation_flags(0x08000000);
        let output = command.output().expect("ffmpeg is required");
        assert!(output.status.success(), "ffmpeg could not encode: {}", String::from_utf8_lossy(&output.stderr));
        let mut reader = PacketReader::new(Cursor::new(output.stdout));
        let mut decoder = VoiceDecoder::new().unwrap();
        let mut count = 0;
        while let Some(packet) = reader.read_packet().unwrap() {
            if packet.data.starts_with(b"OpusHead") || packet.data.starts_with(b"OpusTags") { continue; }
            assert!(!decoder.decode(&packet.data).unwrap().is_empty());
            count += 1;
        }
        assert!(count >= 20);
    }
}
