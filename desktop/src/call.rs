//! One direct voice call, run as a task that owns the WebRTC transport and the
//! audio devices. It speaks the same signaling as the web client
//! (offer/answer/ice/hangup over `call:signal`). Audio is DTLS-SRTP encrypted
//! in transit and is never recorded or stored.
//!
//! Like the browser, the DTLS fingerprint is taken from the peer's SDP as
//! relayed by the authenticated signaling server.
use crate::api::Session;
use crate::live::{CallHandle, IncomingCall};
use iced::futures::{SinkExt, Stream};
use rtc::peer_connection::transport::RTCIceCandidateInit;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum Command { Answer(String), Candidate(RTCIceCandidateInit), Mute(bool), Deafen(bool), Hangup }

#[derive(Clone, Debug)]
pub enum Update { Ready(Controls), Ringing, Connecting, Connected, Ended(Option<String>) }

#[derive(Clone)]
pub struct Controls(mpsc::UnboundedSender<Command>);
impl std::fmt::Debug for Controls { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("Controls") } }
impl Controls { pub fn send(&self, command: Command) { let _ = self.0.send(command); } }

#[derive(Clone)]
pub enum Role { Outgoing, Incoming(IncomingCall) }

#[derive(Clone)]
pub struct Spec {
    pub id: String,
    pub peer: String,
    pub role: Role,
    pub session: Session,
    pub socket: CallHandle,
    pub input: Option<String>,
    pub output: Option<String>,
    pub relay_only: bool,
}
impl Hash for Spec { fn hash<H: Hasher>(&self, state: &mut H) { self.id.hash(state); } }

pub fn new_id() -> String {
    use rand_core::RngCore;
    let mut bytes = [0u8; 16];
    rand_core::OsRng.fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

pub fn fingerprint(sdp: &str) -> Result<String, String> {
    crate::voice_transport::fingerprint(sdp).map_err(|_| "The call could not be secured.".into())
}

pub fn run(spec: &Spec) -> impl Stream<Item = Update> + 'static {
    let spec = spec.clone();
    iced::stream::channel(16, move |mut output: iced::futures::channel::mpsc::Sender<Update>| async move {
        let (sender, mut commands) = mpsc::unbounded_channel();
        let _ = output.send(Update::Ready(Controls(sender))).await;
        let result = drive(&spec, &mut commands, &mut output).await;
        // Report first so a failure reason is shown before the server's
        // "call ended" notice arrives. The server ignores a repeated hangup.
        let (socket, peer, id) = (spec.socket.clone(), spec.peer.clone(), spec.id.clone());
        // Detached: the UI drops this stream as soon as it sees Ended.
        tokio::spawn(async move { let _ = socket.hangup(&peer, &id).await; });
        let _ = output.send(Update::Ended(result.err())).await;
    })
}

/// Runs until the local user hangs up or the call fails. A remote hangup ends
/// the call by dropping this task's subscription.
async fn drive(spec: &Spec, commands: &mut mpsc::UnboundedReceiver<Command>, output: &mut iced::futures::channel::mpsc::Sender<Update>) -> Result<(), String> {
    let bind = vec!["0.0.0.0:0".to_string()];
    let mut muted = false;
    let mut deafened = false;
    let mut pending = Vec::new();
    // Our network routes leave in order on their own task, after the offer or answer,
    // so neither the ringing nor the call waits for every route to be found.
    let (outbox, mut queue) = mpsc::unbounded_channel::<RTCIceCandidateInit>();
    let (socket, peer, id) = (spec.socket.clone(), spec.peer.clone(), spec.id.clone());
    tokio::spawn(async move { while let Some(candidate) = queue.recv().await { let _ = socket.candidate(&peer, &id, &candidate).await; } });
    let prepared = match &spec.role {
        Role::Outgoing => {
            let mut offer = crate::voice_session::prepare_outgoing(&spec.session, spec.relay_only, bind).await?;
            if let Err(error) = spec.socket.offer(&spec.peer, &spec.id, &offer.offer_sdp).await { offer.cancel().await; return Err(error); }
            let _ = output.send(Update::Ringing).await;
            // Wait up to a minute for the callee to answer (the server also times out).
            let deadline = Instant::now() + Duration::from_secs(65);
            let answer = loop {
                while let Some(candidate) = offer.next_local_candidate().await { let _ = outbox.send(candidate); }
                if Instant::now() > deadline { offer.cancel().await; return Err("No answer.".into()); }
                match tokio::time::timeout(Duration::from_millis(100), commands.recv()).await {
                    Ok(Some(Command::Answer(sdp))) => break sdp,
                    Ok(Some(Command::Candidate(candidate))) => pending.push(candidate),
                    Ok(Some(Command::Mute(value))) => muted = value,
                    Ok(Some(Command::Deafen(value))) => deafened = value,
                    Ok(Some(Command::Hangup)) | Ok(None) => { offer.cancel().await; return Ok(()); }
                    Err(_) => {}
                }
            };
            let _ = output.send(Update::Connecting).await;
            let remote = fingerprint(&answer)?;
            offer.accept_answer(answer, &remote, std::mem::take(&mut pending)).await?
        }
        Role::Incoming(call) => {
            let _ = output.send(Update::Connecting).await;
            let mut call = call.clone();
            while let Ok(command) = commands.try_recv() { if let Command::Candidate(candidate) = command { call.candidates.push(candidate); } }
            let remote = fingerprint(&call.sdp)?;
            let prepared = crate::voice_session::prepare_incoming(&spec.session, call.clone(), &remote, spec.relay_only, bind).await?;
            if let Err(error) = spec.socket.answer(&call, &prepared.answer_sdp).await { prepared.cancel().await; return Err(error); }
            prepared
        }
    };
    // Connect while still applying trickled candidates from the browser.
    let mut prepared = prepared;
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        match tokio::time::timeout(Duration::from_millis(100), prepared.transport.wait_connected()).await {
            Ok(Ok(())) => break,
            Ok(Err(error)) => { prepared.cancel().await; return Err(error); }
            Err(_) => {
                while let Some(candidate) = prepared.transport.next_local_candidate().await { let _ = outbox.send(candidate); }
                while let Ok(command) = commands.try_recv() {
                    match command {
                        Command::Candidate(candidate) => { let _ = prepared.transport.add_remote_candidate(candidate).await; }
                        Command::Mute(value) => muted = value,
                        Command::Deafen(value) => deafened = value,
                        Command::Hangup => { prepared.cancel().await; return Ok(()); }
                        Command::Answer(_) => {}
                    }
                }
                if Instant::now() > deadline { prepared.cancel().await; return Err("The call could not connect. Check your network and try again.".into()); }
            }
        }
    }
    let mut call = prepared.start_audio(spec.input.clone(), spec.output.clone()).await?;
    call.set_muted(muted);
    call.set_deafened(deafened);
    let _ = output.send(Update::Connected).await;
    loop {
        while let Ok(command) = commands.try_recv() {
            match command {
                Command::Candidate(candidate) => { let _ = call.add_remote_candidate(candidate).await; }
                Command::Mute(value) => call.set_muted(value),
                Command::Deafen(value) => call.set_deafened(value),
                Command::Hangup => { call.close().await; return Ok(()); }
                Command::Answer(_) => {}
            }
        }
        if commands.is_closed() { call.close().await; return Ok(()); }
        call = call.pump_once().await?;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
