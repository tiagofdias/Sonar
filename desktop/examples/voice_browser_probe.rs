//! Test helper: exchanges synthetic Opus with a browser via stdin/stdout SDP.
//! It never opens audio devices or uses the application database.
#[path = "../src/voice_audio.rs"] mod voice_audio;
#[path = "../src/ice_candidate.rs"] mod ice_candidate;
#[path = "../src/voice_codec.rs"] mod voice_codec;
#[path = "../src/voice_transport.rs"] mod voice_transport;
use std::{io::{self, BufRead, Write}, time::Duration};
use serde_json::json;
use voice_codec::{VoiceEncoder, VoiceDecoder, FRAME_SAMPLES, SAMPLE_RATE};
use voice_transport::VoiceTransport;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut peer = VoiceTransport::new(vec![], false, vec!["127.0.0.1:0".into()]).await?;
    let (sdp, fingerprint) = peer.offer().await?;
    println!("{}", json!({"type":"offer", "sdp":sdp, "fingerprint":fingerprint}));
    io::stdout().flush()?;
    let line = io::stdin().lock().lines().next().ok_or("no answer")??;
    if line.len() > 40000 { return Err("answer too large".into()); }
    let answer: serde_json::Value = serde_json::from_str(&line)?;
    peer.set_remote(answer["sdp"].as_str().ok_or("missing sdp")?.to_owned(), answer["fingerprint"].as_str().ok_or("missing fingerprint")?, false).await?;
    let candidates: Vec<rtc::peer_connection::transport::RTCIceCandidateInit> = serde_json::from_value(answer["candidates"].clone())?;
    if candidates.is_empty() || candidates.len() > 128 { return Err("invalid candidates".into()); }
    for candidate in candidates { peer.add_remote_candidate(candidate).await?; }
    peer.wait_connected().await?;
    let mut encoder = VoiceEncoder::new()?; let mut decoder = VoiceDecoder::new()?;
    let mut received = 0usize; let mut received_peak = 0.0f32;
    for frame in 0..250 {
        let samples: Vec<f32> = (0..FRAME_SAMPLES).map(|index| (((frame * FRAME_SAMPLES + index) as f32 / SAMPLE_RATE as f32) * 440.0 * std::f32::consts::TAU).sin() * 0.2).collect();
        peer.send_packet(&encoder.encode(&samples)?).await?;
        while let Some(packet) = peer.try_receive() {
            if let Ok(samples) = decoder.decode(&packet) {
                received += 1;
                received_peak = samples.iter().fold(received_peak, |peak, value| peak.max(value.abs()));
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    println!("{}", json!({"type":"result", "receivedPackets":received, "peak":received_peak}));
    io::stdout().flush()?;
    peer.close().await;
    Ok(())
}
