//! A community voice room: a mesh of up to five WebRTC peers sharing one
//! microphone, with each remote speaker decoded separately and mixed on
//! output. Signaling matches the web client's useVoiceRoom.js: the newcomer
//! offers to everyone already in the room over `voice:room-signal`. Audio is
//! DTLS-SRTP encrypted in transit and never recorded.
use crate::api::Session;
use crate::live::CallHandle;
use crate::voice_transport::VoiceTransport;
use iced::futures::{SinkExt, Stream};
use rtc::peer_connection::transport::RTCIceCandidateInit;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum Command { Signal { from: String, session: String, signal: Value }, Participants(Vec<(String, String)>), Mute(bool), Deafen(bool), Leave }

#[derive(Clone, Debug)]
pub enum Update { Ready(Controls), Connections(usize), Ended(Option<String>) }

#[derive(Clone)]
pub struct Controls(mpsc::UnboundedSender<Command>);
impl std::fmt::Debug for Controls { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("RoomControls") } }
impl Controls { pub fn send(&self, command: Command) { let _ = self.0.send(command); } }

#[derive(Clone)]
pub struct Spec {
    pub community: String,
    pub channel: String,
    pub session_id: String,
    pub me: String,
    /// (account, room session) of everyone already in the room.
    pub participants: Vec<(String, String)>,
    pub session: Session,
    pub socket: CallHandle,
    pub input: Option<String>,
    pub output: Option<String>,
    pub relay_only: bool,
}
impl Hash for Spec { fn hash<H: Hasher>(&self, state: &mut H) { self.session_id.hash(state); } }

struct Peer { session: String, transport: VoiceTransport, source: u64, remote: bool, pending: Vec<RTCIceCandidateInit> }

pub fn run(spec: &Spec) -> impl Stream<Item = Update> + 'static {
    let spec = spec.clone();
    iced::stream::channel(16, move |mut output: iced::futures::channel::mpsc::Sender<Update>| async move {
        let (sender, mut commands) = mpsc::unbounded_channel();
        let _ = output.send(Update::Ready(Controls(sender))).await;
        let result = drive(&spec, &mut commands, &mut output).await;
        let _ = output.send(Update::Ended(result.err())).await;
    })
}

async fn signal(spec: &Spec, to: &str, to_session: &str, signal: Value) -> Result<(), String> {
    spec.socket.request("voice:room-signal", json!({ "communityId": spec.community, "channelId": spec.channel, "sessionId": spec.session_id, "to": to, "toSession": to_session, "signal": signal })).await.map(|_| ())
}

async fn drive(spec: &Spec, commands: &mut mpsc::UnboundedReceiver<Command>, output: &mut iced::futures::channel::mpsc::Sender<Update>) -> Result<(), String> {
    let (servers, relay) = spec.session.voice_configuration().await?.into_servers(spec.relay_only)?;
    let (input, out) = (spec.input.clone(), spec.output.clone());
    let audio = tokio::task::spawn_blocking(move || crate::voice_audio::VoiceAudio::open(input, out)).await.map_err(|_| "Could not start your audio devices.")??;
    let bind = || vec!["0.0.0.0:0".to_string()];
    let mut peers: HashMap<String, Peer> = HashMap::new();
    let mut next_source = 1u64;
    let (mut muted, mut deafened) = (false, false);
    // Newcomers offer to everyone already present (like the web client).
    for (user, session) in spec.participants.iter().filter(|(user, _)| user != &spec.me) {
        let mut transport = VoiceTransport::new(servers.clone(), relay, bind()).await?;
        let (sdp, _) = transport.offer().await?;
        if signal(spec, user, session, json!({ "type": "offer", "sdp": { "type": "offer", "sdp": sdp } })).await.is_ok() {
            peers.insert(user.clone(), Peer { session: session.clone(), transport, source: next_source, remote: false, pending: Vec::new() });
            next_source += 1;
        } else { transport.close().await; }
    }
    let mut reported = usize::MAX;
    loop {
        while let Ok(command) = commands.try_recv() {
            match command {
                Command::Leave => { for (_, peer) in peers.drain() { peer.transport.close().await; } return Ok(()); }
                Command::Mute(value) => { muted = value; audio.set_muted(muted || deafened); }
                Command::Deafen(value) => { deafened = value; audio.set_deafened(deafened); audio.set_muted(muted || deafened); }
                Command::Participants(list) => {
                    let gone: Vec<String> = peers.iter().filter(|(user, peer)| !list.iter().any(|(u, s)| u == *user && s == &peer.session)).map(|(user, _)| user.clone()).collect();
                    for user in gone { if let Some(peer) = peers.remove(&user) { audio.remove_source(peer.source); peer.transport.close().await; } }
                }
                Command::Signal { from, session, signal: data } => {
                    let kind = data.get("type").and_then(Value::as_str).unwrap_or_default();
                    let sdp = data.get("sdp").and_then(|v| v.get("sdp")).and_then(Value::as_str).map(str::to_owned);
                    match (kind, sdp) {
                        ("offer", Some(sdp)) if sdp.len() < 32000 => {
                            // Someone joined after us; replace any stale connection.
                            if let Some(old) = peers.remove(&from) { audio.remove_source(old.source); old.transport.close().await; }
                            let Ok(remote) = crate::voice_transport::fingerprint(&sdp) else { continue };
                            let Ok(mut transport) = VoiceTransport::new(servers.clone(), relay, bind()).await else { continue };
                            match transport.answer_with_candidates(sdp, &remote, Vec::new()).await {
                                Ok((answer, _)) if signal(spec, &from, &session, json!({ "type": "answer", "sdp": { "type": "answer", "sdp": answer } })).await.is_ok() => {
                                    peers.insert(from, Peer { session, transport, source: next_source, remote: true, pending: Vec::new() });
                                    next_source += 1;
                                }
                                _ => transport.close().await,
                            }
                        }
                        ("answer", Some(sdp)) => {
                            let Some(peer) = peers.get_mut(&from).filter(|p| p.session == session && !p.remote) else { continue };
                            let applied = match crate::voice_transport::fingerprint(&sdp) { Ok(remote) => peer.transport.set_remote(sdp, &remote, false).await.is_ok(), Err(_) => false };
                            if applied {
                                peer.remote = true;
                                for candidate in std::mem::take(&mut peer.pending) { let _ = peer.transport.add_remote_candidate(candidate).await; }
                            } else if let Some(peer) = peers.remove(&from) { audio.remove_source(peer.source); peer.transport.close().await; }
                        }
                        ("ice", _) => {
                            let Some(peer) = peers.get_mut(&from).filter(|p| p.session == session) else { continue };
                            let Ok(candidate) = serde_json::from_value::<RTCIceCandidateInit>(data.get("candidate").cloned().unwrap_or_default()) else { continue };
                            if !crate::ice_candidate::valid_remote_candidate(&candidate) { continue; }
                            if peer.remote { let _ = peer.transport.add_remote_candidate(candidate).await; } else if peer.pending.len() < 128 { peer.pending.push(candidate); }
                        }
                        _ => {}
                    }
                }
            }
        }
        if commands.is_closed() { for (_, peer) in peers.drain() { peer.transport.close().await; } return Ok(()); }
        if audio.failed() { for (_, peer) in peers.drain() { peer.transport.close().await; } return Err("Your audio device stopped working".into()); }
        // One captured packet goes to every connected peer.
        while let Some(packet) = audio.next_packet() {
            for peer in peers.values() { if peer.transport.connected() { let _ = peer.transport.send_packet(&packet).await; } }
        }
        let mut failed = Vec::new();
        for (user, peer) in peers.iter_mut() {
            while let Some(packet) = peer.transport.try_receive() { let _ = audio.play_from(peer.source, &packet); }
            if peer.transport.failed() { failed.push(user.clone()); }
        }
        for user in failed { if let Some(peer) = peers.remove(&user) { audio.remove_source(peer.source); peer.transport.close().await; } }
        let connected = peers.values().filter(|p| p.transport.connected()).count();
        if connected != reported { reported = connected; let _ = output.send(Update::Connections(connected)).await; }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
