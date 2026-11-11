//! Setup for an incoming native call. The caller must supply a fingerprint
//! authenticated independently of the signaling server. Audio is opened only
//! on explicit activation and this module is not yet wired to the interface.
#![allow(dead_code)]
use crate::{api::Session, live::IncomingCall, voice_transport::VoiceTransport};

pub struct PreparedCall {
    pub transport: VoiceTransport,
    pub answer_sdp: String,
    pub local_fingerprint: String,
}

/// An outgoing offer has not authenticated its recipient yet and cannot open
/// audio. Its answer must be checked against an independently verified key.
pub struct OutgoingOffer {
    transport: VoiceTransport,
    pub offer_sdp: String,
    pub local_fingerprint: String,
}

impl OutgoingOffer {
    pub async fn accept_answer(self, answer_sdp: String, verified_fingerprint: &str, candidates: Vec<rtc::peer_connection::transport::RTCIceCandidateInit>) -> Result<PreparedCall, String> {
        let result = async {
            if candidates.len() > 128 || candidates.iter().any(|candidate| !crate::ice_candidate::valid_remote_candidate(candidate)) { return Err("Invalid network candidates".into()); }
            self.transport.set_remote(answer_sdp, verified_fingerprint, false).await?;
            for candidate in candidates { self.transport.add_remote_candidate(candidate).await?; }
            Ok(())
        }.await;
        if let Err(error) = result { self.transport.close().await; return Err(error); }
        Ok(PreparedCall { transport: self.transport, answer_sdp: String::new(), local_fingerprint: self.local_fingerprint })
    }
    pub async fn cancel(self) { self.transport.close().await; }
    /// A local network route found since the last call, to send while the call rings.
    pub async fn next_local_candidate(&mut self) -> Option<rtc::peer_connection::transport::RTCIceCandidateInit> { self.transport.next_local_candidate().await }
}

/// The offer is ready at once so the other person starts ringing straight away;
/// network routes follow through `OutgoingOffer::next_local_candidate`.
pub async fn prepare_outgoing(session: &Session, relay_only: bool, bind: Vec<String>) -> Result<OutgoingOffer, String> {
    let (servers, relay) = session.voice_configuration().await?.into_servers(relay_only)?;
    let mut transport = VoiceTransport::new(servers, relay, bind).await?;
    let prepared = async {
        let offer_sdp = transport.offer_now().await?;
        let local_fingerprint = crate::voice_transport::fingerprint(&offer_sdp).map_err(str::to_owned)?;
        Ok::<_, String>((offer_sdp, local_fingerprint))
    }.await;
    match prepared {
        Ok((offer_sdp, local_fingerprint)) => Ok(OutgoingOffer { transport, offer_sdp, local_fingerprint }),
        Err(error) => { transport.close().await; Err(error) }
    }
}

pub struct ActiveCall {
    transport: VoiceTransport,
    audio: crate::voice_audio::VoiceAudio,
    muted: bool,
    deafened: bool,
    disconnected_since: Option<std::time::Instant>,
}

impl PreparedCall {
    /// Call only after the authenticated fingerprint was sent to the peer and
    /// the server accepted our answer. Do not capture until WebRTC is connected.
    pub async fn activate(mut self, input: Option<String>, output: Option<String>) -> Result<ActiveCall, String> {
        if let Err(error) = self.transport.wait_connected().await {
            self.transport.close().await;
            return Err(error);
        }
        let audio = tokio::task::spawn_blocking(move || crate::voice_audio::VoiceAudio::open(input, output)).await;
        match audio {
            Ok(Ok(audio)) => Ok(ActiveCall { transport: self.transport, audio, muted: false, deafened: false, disconnected_since: None }),
            Ok(Err(error)) => { self.transport.close().await; Err(error) }
            Err(_) => { self.transport.close().await; Err("Could not start your audio devices.".into()) }
        }
    }
    pub async fn cancel(self) { self.transport.close().await; }
    /// Opens the audio devices once the transport is already connected.
    pub async fn start_audio(self, input: Option<String>, output: Option<String>) -> Result<ActiveCall, String> {
        let audio = tokio::task::spawn_blocking(move || crate::voice_audio::VoiceAudio::open(input, output)).await;
        match audio {
            Ok(Ok(audio)) => Ok(ActiveCall { transport: self.transport, audio, muted: false, deafened: false, disconnected_since: None }),
            Ok(Err(error)) => { self.transport.close().await; Err(error) }
            Err(_) => { self.transport.close().await; Err("Could not start your audio devices.".into()) }
        }
    }
}

impl ActiveCall {
    pub async fn add_remote_candidate(&mut self, candidate: rtc::peer_connection::transport::RTCIceCandidateInit) -> Result<(), String> {
        self.transport.add_remote_candidate(candidate).await
    }
    pub fn set_muted(&mut self, value: bool) {
        self.muted = value;
        self.audio.set_muted(self.muted || self.deafened);
    }
    pub fn set_deafened(&mut self, value: bool) {
        self.deafened = value;
        // Deafen silences both directions, while preserving the user's mute
        // choice when they later turn deafen off.
        if value { self.audio.set_muted(true); self.audio.set_deafened(true); }
        else { self.audio.set_deafened(false); self.audio.set_muted(self.muted); }
    }
    /// The owner drives this while the call is live. A failure consumes the
    /// session and releases both the device streams and network connection.
    pub async fn pump_once(mut self) -> Result<Self, String> {
        if self.transport.disconnected() {
            let since = self.disconnected_since.get_or_insert_with(std::time::Instant::now);
            if since.elapsed() >= std::time::Duration::from_secs(10) {
                self.close().await;
                return Err("The voice connection was interrupted".into());
            }
        } else { self.disconnected_since = None; }
        if let Err(error) = self.transport.pump(&self.audio).await {
            drop(self.audio);
            self.transport.close().await;
            return Err(error);
        }
        Ok(self)
    }
    pub async fn close(self) {
        drop(self.audio);
        self.transport.close().await;
    }
}

pub async fn prepare_incoming(session: &Session, call: IncomingCall, verified_fingerprint: &str, relay_only: bool, bind: Vec<String>) -> Result<PreparedCall, String> {
    let (servers, relay) = session.voice_configuration().await?.into_servers(relay_only)?;
    let mut transport = VoiceTransport::new(servers, relay, bind).await?;
    // Answer at once; our network routes follow through `transport.next_local_candidate`.
    let prepared = async {
        if call.candidates.len() > 128 || call.candidates.iter().any(|candidate| !crate::ice_candidate::valid_remote_candidate(candidate)) { return Err("Invalid network candidates".to_string()); }
        let answer_sdp = transport.answer_now(call.sdp, verified_fingerprint).await?;
        for candidate in call.candidates { transport.add_remote_candidate(candidate).await?; }
        let local_fingerprint = crate::voice_transport::fingerprint(&answer_sdp).map_err(str::to_owned)?;
        Ok((answer_sdp, local_fingerprint))
    }.await;
    match prepared {
        Ok((answer_sdp, local_fingerprint)) => Ok(PreparedCall { transport, answer_sdp, local_fingerprint }),
        Err(error) => { transport.close().await; Err(error) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn outgoing_rejects_an_unverified_answer_and_accepts_a_matching_one() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0; 4096]; let n = stream.read(&mut buf).unwrap();
                assert!(String::from_utf8_lossy(&buf[..n]).starts_with("GET /api/voice/config "));
                let body = r#"{"iceServers":[],"turnConfigured":false,"relayOnly":false,"expiresAt":null}"#;
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        let session = crate::api::synthetic_session_at(&format!("http://{addr}"));
        for valid in [false, true] {
            let offer = prepare_outgoing(&session, false, vec!["127.0.0.1:0".into()]).await.unwrap();
            let mut peer = VoiceTransport::new(vec![], false, vec!["127.0.0.1:0".into()]).await.unwrap();
            let (answer, fingerprint) = peer.answer(offer.offer_sdp.clone(), &offer.local_fingerprint).await.unwrap();
            let expected = if valid { fingerprint } else { format!("{}AA", "AA:".repeat(31)) };
            let result = offer.accept_answer(answer, &expected, vec![]).await;
            if valid {
                let prepared = result.ok().expect("matching fingerprint");
                prepared.cancel().await;
            } else { assert!(result.is_err()); }
            peer.close().await;
        }
        server.join().unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn incoming_uses_server_configuration_and_requires_matching_fingerprint() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0; 4096]; let n = stream.read(&mut buf).unwrap();
                let request = String::from_utf8_lossy(&buf[..n]);
                assert!(request.starts_with("GET /api/voice/config "));
                assert!(request.to_ascii_lowercase().contains("cookie: jwt=synthetic"));
                let body = r#"{"iceServers":[],"turnConfigured":false,"relayOnly":false,"expiresAt":null}"#;
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        let session = crate::api::synthetic_session_at(&format!("http://{addr}"));
        let mut caller = VoiceTransport::new(vec![], false, vec!["127.0.0.1:0".into()]).await.unwrap();
        let (sdp, fingerprint) = caller.offer().await.unwrap();
        let call = IncomingCall { from: "0123456789abcdef01234567".into(), id: "6bfa8180-b4fb-44f8-8922-6ad700b06b68".into(), sdp, candidates: Vec::new() };
        assert!(prepare_incoming(&session, call.clone(), &format!("{}AA", "AA:".repeat(31)), false, vec!["127.0.0.1:0".into()]).await.is_err());
        let prepared = prepare_incoming(&session, call, &fingerprint, false, vec!["127.0.0.1:0".into()]).await.unwrap();
        caller.set_remote(prepared.answer_sdp.clone(), &prepared.local_fingerprint, false).await.unwrap();
        caller.wait_connected().await.unwrap();
        // A deliberately unavailable selection fails without opening a real mic;
        // activation must release the already-established network transport.
        assert!(prepared.activate(Some("sonar-test-nonexistent-microphone".into()), None).await.is_err());
        caller.close().await;
        server.join().unwrap();
    }
}
