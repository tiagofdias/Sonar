//! Authenticated Socket.IO presence and session lifecycle for the native client.
//! Message payloads are intentionally not processed until device encryption is ready.
use iced::futures::{self, FutureExt, SinkExt, Stream, StreamExt};
use rust_socketio::{asynchronous::{Client, ClientBuilder}, Payload, TransportType};
use std::hash::{Hash, Hasher};

#[derive(Clone)]
pub struct Auth { pub generation: u64, pub base: String, pub cookie: String }
impl Hash for Auth {
    fn hash<H: Hasher>(&self, state: &mut H) { self.generation.hash(state); self.base.hash(state); }
}
#[derive(Clone)]
pub struct CallHandle(Client);
impl std::fmt::Debug for CallHandle { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("CallHandle") } }
#[derive(Clone, PartialEq, Eq)]
pub struct IncomingCall { pub from: String, pub id: String, pub sdp: String, pub candidates: Vec<rtc::peer_connection::transport::RTCIceCandidateInit> }
// SDP can contain network addresses; keep it out of diagnostics.
impl std::fmt::Debug for IncomingCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IncomingCall").field("from", &self.from).field("id", &self.id).field("sdp", &"[redacted]").field("candidate_count", &self.candidates.len()).finish()
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct CallCandidate { pub from: String, pub id: String, pub candidate: rtc::peer_connection::transport::RTCIceCandidateInit }
#[derive(Clone, PartialEq, Eq)]
pub struct CallAnswer { pub from: String, pub id: String, pub sdp: String }
impl std::fmt::Debug for CallAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallAnswer").field("from", &self.from).field("id", &self.id).field("sdp", &"[redacted]").finish()
    }
}
impl std::fmt::Debug for CallCandidate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallCandidate").field("from", &self.from).field("id", &self.id).field("candidate", &"[redacted]").finish()
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Connected(CallHandle), Disconnected, Revoked, Online(Vec<String>), MatrixChanged, Incoming(IncomingCall), Answer(CallAnswer), Candidate(CallCandidate), CallEnded(String),
    /// Screen-share signaling inside the active call; opaque to this layer.
    Screen { from: String, id: String, signal: serde_json::Value },
    /// Encrypted message records and membership notices. Payloads are parsed
    /// and validated by the receiver; the socket never carries plaintext.
    Data(&'static str, serde_json::Value),
}

fn valid_id(value: &str) -> bool { value.len() == 24 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) }
fn valid_call_id(value: &str) -> bool {
    value.len() == 36 && value.bytes().enumerate().all(|(index, byte)| if [8, 13, 18, 23].contains(&index) { byte == b'-' } else { byte.is_ascii_hexdigit() })
}
fn screen_signal(payload: &Payload) -> Option<(String, String, serde_json::Value)> {
    let Payload::Text(values) = payload else { return None; };
    let data = values.first()?;
    let signal = data.get("signal")?;
    if !signal.get("type")?.as_str()?.starts_with("screen-") { return None; }
    let (from, id) = (data.get("from")?.as_str()?, signal.get("callId")?.as_str()?);
    (valid_id(from) && valid_call_id(id)).then(|| (from.to_owned(), id.to_owned(), signal.clone()))
}
fn incoming(payload: Payload) -> Option<IncomingCall> {
    let Payload::Text(values) = payload else { return None; };
    let data = values.first()?;
    let from = data.get("from")?.as_str()?;
    let signal = data.get("signal")?;
    let id = signal.get("callId")?.as_str()?;
    let sdp = signal.get("sdp")?;
    let description = sdp.get("sdp")?.as_str()?;
    if !valid_id(from) || !valid_call_id(id) || signal.get("type")?.as_str()? != "offer" || sdp.get("type")?.as_str()? != "offer" || description.is_empty() || description.len() > 32000 { return None; }
    Some(IncomingCall { from: from.to_owned(), id: id.to_owned(), sdp: description.to_owned(), candidates: Vec::new() })
}
fn candidate(payload: Payload) -> Option<CallCandidate> {
    let Payload::Text(values) = payload else { return None; };
    let data = values.first()?;
    let from = data.get("from")?.as_str()?;
    let signal = data.get("signal")?;
    let id = signal.get("callId")?.as_str()?;
    if !valid_id(from) || !valid_call_id(id) || signal.get("type")?.as_str()? != "ice" { return None; }
    let value = signal.get("candidate")?;
    if value.to_string().len() > 5000 { return None; }
    let candidate: rtc::peer_connection::transport::RTCIceCandidateInit = serde_json::from_value(value.clone()).ok()?;
    if !crate::ice_candidate::valid_remote_candidate(&candidate) { return None; }
    Some(CallCandidate { from: from.to_owned(), id: id.to_owned(), candidate })
}
fn answer(payload: Payload) -> Option<CallAnswer> {
    let Payload::Text(values) = payload else { return None; };
    let data = values.first()?;
    let from = data.get("from")?.as_str()?;
    let signal = data.get("signal")?;
    let id = signal.get("callId")?.as_str()?;
    let sdp = signal.get("sdp")?;
    let description = sdp.get("sdp")?.as_str()?;
    if !valid_id(from) || !valid_call_id(id) || signal.get("type")?.as_str()? != "answer" || sdp.get("type")?.as_str()? != "answer" || description.is_empty() || description.len() > 32000 { return None; }
    Some(CallAnswer { from: from.to_owned(), id: id.to_owned(), sdp: description.to_owned() })
}
fn ended(payload: Payload) -> Option<String> {
    let Payload::Text(values) = payload else { return None; };
    let id = values.first()?.get("callId")?.as_str()?;
    valid_call_id(id).then(|| id.to_owned())
}
impl CallHandle {
    pub async fn offer(&self, peer: &str, id: &str, sdp: &str) -> Result<(), String> {
        if sdp.is_empty() || sdp.len() > 30000 { return Err("Invalid call offer.".into()); }
        self.send_signal(peer, id, serde_json::json!({ "type": "offer", "callId": id, "sdp": { "type": "offer", "sdp": sdp } })).await
    }
    pub async fn hangup(&self, peer: &str, id: &str) -> Result<(), String> {
        self.send_signal(peer, id, serde_json::json!({ "type": "hangup", "callId": id })).await
    }
    pub async fn decline(self, call: IncomingCall) -> Result<(), String> {
        self.send_signal(&call.from, &call.id, serde_json::json!({ "type": "hangup", "callId": call.id })).await
    }
    // Only call after independently verifying the peer and preparing the local SDP.
    pub async fn answer(&self, call: &IncomingCall, sdp: &str) -> Result<(), String> {
        if sdp.is_empty() || sdp.len() > 30000 { return Err("Invalid call answer.".into()); }
        self.send_signal(&call.from, &call.id, serde_json::json!({ "type": "answer", "callId": call.id, "sdp": { "type": "answer", "sdp": sdp } })).await
    }
    /// A one-way note, such as "I am typing".
    pub async fn notify(&self, event: &'static str, payload: serde_json::Value) {
        let _ = self.0.emit(event, payload).await;
    }
    /// Sends an acknowledged request (voice rooms) and returns the server's
    /// reply, or its `error` message.
    pub async fn request(&self, event: &'static str, payload: serde_json::Value) -> Result<serde_json::Value, String> {
        use std::sync::{Arc, Mutex};
        let (tx, rx) = futures::channel::oneshot::channel();
        let sender = Arc::new(Mutex::new(Some(tx)));
        let callback = move |payload: Payload, _| {
            let tx = sender.lock().ok().and_then(|mut value| value.take());
            async move {
                let value = match payload { Payload::Text(values) => values.into_iter().next().map(|value| match value { serde_json::Value::Array(mut items) if !items.is_empty() => items.remove(0), other => other }), _ => None };
                if let Some(tx) = tx { let _ = tx.send(value); }
            }.boxed()
        };
        self.0.emit_with_ack(event, payload, std::time::Duration::from_secs(5), callback).await.map_err(|_| "Voice room request failed. Try again.")?;
        match tokio::time::timeout(std::time::Duration::from_secs(6), rx).await {
            Ok(Ok(Some(value))) => match value.get("error").and_then(|e| e.as_str()) { Some(error) => Err(error.to_owned()), None => Ok(value) },
            _ => Err("Voice room request timed out".into()),
        }
    }
    async fn send_signal(&self, peer: &str, id: &str, signal: serde_json::Value) -> Result<(), String> {
        use std::sync::{Arc, Mutex};
        if !valid_id(peer) || !valid_call_id(id) { return Err("Invalid call.".into()); }
        let (tx, rx) = futures::channel::oneshot::channel();
        let sender = Arc::new(Mutex::new(Some(tx)));
        let callback = move |payload: Payload, _| {
            let tx = sender.lock().ok().and_then(|mut value| value.take());
            async move {
                let ok = match payload { Payload::Text(values) => values.first().and_then(|value| if let Some(items) = value.as_array() { items.first() } else { Some(value) }).and_then(|value| value.get("ok")).and_then(|value| value.as_bool()) == Some(true), _ => false };
                if let Some(tx) = tx { let _ = tx.send(ok); }
            }.boxed()
        };
        self.0.emit_with_ack("call:signal", serde_json::json!({ "to": peer, "signal": signal }), std::time::Duration::from_secs(5), callback).await
            .map_err(|_| "This person is unavailable or already in a call.")?;
        match tokio::time::timeout(std::time::Duration::from_secs(6), rx).await {
            Ok(Ok(true)) => Ok(()),
            _ => Err("This person is unavailable or already in a call.".into()),
        }
    }
}

fn online(payload: Payload) -> Option<Vec<String>> {
    let Payload::Text(values) = payload else { return None; };
    let entries = values.first()?.as_array()?;
    if entries.len() > 10000 { return None; }
    entries.iter().map(|entry| {
        let id = entry.as_str()?;
        if id.len() == 24 && id.bytes().all(|byte| byte.is_ascii_hexdigit()) { Some(id.to_owned()) } else { None }
    }).collect()
}
type Handler = Box<dyn FnMut(Payload, Client) -> futures::future::BoxFuture<'static, ()> + Send + Sync>;
fn data_handler(sender: futures::channel::mpsc::Sender<Event>, name: &'static str) -> Handler {
    Box::new(move |payload, _| {
        let mut sender = sender.clone();
        async move {
            if let Payload::Text(values) = payload {
                if let Some(value) = values.into_iter().next().filter(|value| value.is_object()) { let _ = sender.send(Event::Data(name, value)).await; }
            }
        }.boxed()
    })
}
struct Connection(Client);
impl Drop for Connection {
    fn drop(&mut self) {
        let client = self.0.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() { runtime.spawn(async move { let _ = client.disconnect().await; }); }
    }
}

pub fn events(auth: &Auth) -> impl Stream<Item = (u64, Event)> + 'static {
    let auth = auth.clone();
    iced::stream::channel(32, move |mut output: futures::channel::mpsc::Sender<(u64, Event)>| async move {
        let (sender, mut receiver) = futures::channel::mpsc::channel::<Event>(32);
        let online_sender = sender.clone();
        let connected_sender = sender.clone();
        let incoming_sender = sender.clone();
        let ended_sender = sender.clone();
        let error_sender = sender.clone();
        let revoked_sender = sender.clone();
        let empty_event_sender = sender.clone();
        let closed_sender = sender.clone();
        let builder = ClientBuilder::new(auth.base.clone())
            .transport_type(TransportType::Websocket)
            .opening_header("cookie", auth.cookie.clone())
            .reconnect(false)
            .reconnect_on_disconnect(false)
            .on("open", move |_, socket| {
                let mut sender = connected_sender.clone();
                async move { let _ = sender.send(Event::Connected(CallHandle(socket))).await; }.boxed()
            })
            .on("error", move |_, _| {
                let mut sender = error_sender.clone();
                async move { let _ = sender.send(Event::Disconnected).await; }.boxed()
            })
            .on("getOnlineUsers", move |payload, _| {
                let mut sender = online_sender.clone();
                async move { if let Some(ids) = online(payload) { let _ = sender.send(Event::Online(ids)).await; } }.boxed()
            })
            .on("call:signal", move |payload, _| {
                let mut sender = incoming_sender.clone();
                async move {
                    if let Some((from, id, signal)) = screen_signal(&payload) { let _ = sender.send(Event::Screen { from, id, signal }).await; }
                    else if let Some(call) = incoming(payload.clone()) { let _ = sender.send(Event::Incoming(call)).await; }
                    else if let Some(answer) = answer(payload.clone()) { let _ = sender.send(Event::Answer(answer)).await; }
                    else if let Some(candidate) = candidate(payload) { let _ = sender.send(Event::Candidate(candidate)).await; }
                }.boxed()
            })
            .on("call:ended", move |payload, _| {
                let mut sender = ended_sender.clone();
                async move { if let Some(id) = ended(payload) { let _ = sender.send(Event::CallEnded(id)).await; } }.boxed()
            })
            .on("preferencesChanged", data_handler(sender.clone(), "preferencesChanged"))
            .on("profileChanged", data_handler(sender.clone(), "profileChanged"))
            .on("typing", data_handler(sender.clone(), "typing"))
            .on("friendsChanged", data_handler(sender.clone(), "friendsChanged"))
            .on("friendRequest", data_handler(sender.clone(), "friendRequest"))
            .on("newMessage", data_handler(sender.clone(), "newMessage"))
            .on("messageUpdated", data_handler(sender.clone(), "messageUpdated"))
            .on("messageDeleted", data_handler(sender.clone(), "messageDeleted"))
            .on("channelMessageNew", data_handler(sender.clone(), "channelMessageNew"))
            .on("channelMessage", data_handler(sender.clone(), "channelMessage"))
            .on("channelMessageDeleted", data_handler(sender.clone(), "channelMessageDeleted"))
            .on("communityUpdated", data_handler(sender.clone(), "communityUpdated"))
            .on("communityRemoved", data_handler(sender.clone(), "communityRemoved"))
            .on("voice:room-state", data_handler(sender.clone(), "voice:room-state"))
            .on("voice:room-signal", data_handler(sender.clone(), "voice:room-signal"))
            .on("getPresence", data_handler(sender.clone(), "getPresence"))
            .on("ownPresence", data_handler(sender.clone(), "ownPresence"))
            .on("voice:room-left", data_handler(sender.clone(), "voice:room-left"))
            .on("sessionRevoked", move |_, _| {
                let mut sender = revoked_sender.clone();
                async move { let _ = sender.send(Event::Revoked).await; }.boxed()
            })
            // rust_socketio 0.6 decodes an event with no arguments as the
            // default message event. Sonar's server sends revocation this way.
            .on("message", move |payload, _| {
                let mut sender = empty_event_sender.clone();
                async move {
                    if let Payload::Text(values) = payload {
                        if values.len() == 1 && values[0].as_str() == Some("sessionRevoked") { let _ = sender.send(Event::Revoked).await; }
                        else if values.len() == 1 && values[0].as_str() == Some("matrixRoomChanged") { let _ = sender.send(Event::MatrixChanged).await; }
                    }
                }.boxed()
            })
            .on("close", move |_, _| {
                let mut sender = closed_sender.clone();
                async move { let _ = sender.send(Event::Disconnected).await; }.boxed()
            });
        drop(sender);
        let client = match builder.connect().await {
            Ok(client) => Connection(client),
            Err(_) => { let _ = output.send((auth.generation, Event::Disconnected)).await; return; }
        };
        while let Some(event) = receiver.next().await {
            let ended = matches!(event, Event::Revoked | Event::Disconnected);
            if output.send((auth.generation, event)).await.is_err() || ended { break; }
        }
        drop(client);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn outgoing_answer_requires_valid_scoped_signal_and_redacts_sdp() {
        let peer = "0123456789abcdef01234567";
        let id = "6bfa8180-b4fb-44f8-8922-6ad700b06b68";
        let mut value = json!({"from":peer,"signal":{"type":"answer","callId":id,"sdp":{"type":"answer","sdp":"private-network-address"}}});
        let parsed = answer(Payload::Text(vec![value.clone()])).unwrap();
        assert_eq!(parsed.from, peer); assert_eq!(parsed.id, id);
        assert!(!format!("{parsed:?}").contains("private-network-address"));
        value["signal"]["sdp"]["type"] = json!("offer");
        assert!(answer(Payload::Text(vec![value.clone()])).is_none());
        value["signal"]["sdp"]["type"] = json!("answer");
        value["signal"]["callId"] = json!("invalid");
        assert!(answer(Payload::Text(vec![value])).is_none());
    }
    #[tokio::test]
    async fn authenticated_socket_receives_presence_and_revocation() {
        use std::io::{BufRead, BufReader};
        use std::process::{Child, Command, Stdio};
        #[cfg(windows)] use std::os::windows::process::CommandExt;
        struct Fixture(Child);
        impl Drop for Fixture { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
        let mut command = Command::new("node");
        command.arg("tests/mock-live.cjs").stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)] command.creation_flags(0x08000000);
        let mut child = Fixture(command.spawn().expect("node and backend dependencies are required"));
        let mut line = String::new();
        BufReader::new(child.0.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let port: u16 = line.trim().parse().expect("mock server did not start");
        let auth = Auth { generation: 42, base: format!("http://127.0.0.1:{port}/"), cookie: "jwt=synthetic-live-session".into() };
        let rejected = Auth { generation: 41, base: auth.base.clone(), cookie: "jwt=wrong".into() };
        let rejected_stream = events(&rejected);
        futures::pin_mut!(rejected_stream);
        let rejected_event = tokio::time::timeout(std::time::Duration::from_secs(5), rejected_stream.next()).await.expect("rejection timed out");
        assert!(matches!(rejected_event, Some((41, Event::Disconnected))));
        let stream = events(&auth);
        futures::pin_mut!(stream);
        let mut got_online = false;
        let mut got_matrix_changed = false;
        let mut got_revoked = false;
        for _ in 0..5 {
            let item = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next()).await.expect("socket event timed out");
            let Some((generation, event)) = item else { break };
            assert_eq!(generation, 42);
            match event {
                Event::Online(ids) => { assert_eq!(ids, ["0123456789abcdef01234567"]); got_online = true; }
                Event::MatrixChanged => { got_matrix_changed = true; }
                Event::Revoked => { got_revoked = true; break; }
                _ => {}
            }
        }
        assert!(got_online && got_matrix_changed && got_revoked);
    }
    #[tokio::test]
    #[ignore = "run through backend/scripts/test-native-live.mjs with isolated credentials"]
    async fn sonar_server_presence_and_account_revocation() {
        use std::io::Write;
        let auth = Auth { generation: 7, base: std::env::var("SONAR_NATIVE_TEST_BASE").unwrap(), cookie: std::env::var("SONAR_NATIVE_TEST_COOKIE").unwrap() };
        let account = std::env::var("SONAR_NATIVE_TEST_USER").unwrap();
        let stream = events(&auth);
        futures::pin_mut!(stream);
        let mut ready = false;
        let mut handle = None;
        let mut declined = false;
        let outgoing_id = "c6fa8180-b4fb-44f8-8922-6ad700b06b69";
        loop {
            let event = tokio::time::timeout(std::time::Duration::from_secs(15), stream.next()).await.expect("event timed out").expect("socket ended");
            match event {
                (7, Event::Connected(value)) => handle = Some(value),
                (7, Event::Online(ids)) if ids.contains(&account) => {
                    if !ready { ready = true; println!("SONAR_NATIVE_TEST_READY"); std::io::stdout().flush().unwrap(); }
                }
                (7, Event::Incoming(call)) => {
                    assert!(ready);
                    assert_eq!(call.id, "6bfa8180-b4fb-44f8-8922-6ad700b06b68");
                    let socket = handle.clone().expect("socket handle missing");
                    assert!(socket.answer(&call, "").await.is_err());
                    socket.answer(&call, "synthetic-answer").await.expect("answer failed");
                    assert!(socket.answer(&call, "synthetic-answer").await.is_err());
                    socket.clone().decline(call.clone()).await.expect("hangup failed");
                    declined = true;
                    socket.offer(&call.from, outgoing_id, "synthetic-native-offer").await.expect("offer failed");
                }
                (7, Event::Answer(answer)) => {
                    assert!(declined);
                    assert_eq!(answer.id, outgoing_id);
                    assert_eq!(answer.sdp, "synthetic-web-answer");
                    handle.clone().unwrap().hangup(&answer.from, &answer.id).await.expect("outgoing hangup failed");
                    println!("SONAR_NATIVE_TEST_DECLINED"); std::io::stdout().flush().unwrap();
                }
                (7, Event::Revoked) => { assert!(ready && declined); break; }
                (_, Event::Disconnected) => panic!("socket closed before revocation"),
                _ => {}
            }
        }
    }
    #[test]
    fn presence_rejects_malformed_identifiers_and_payloads() {
        let id = "0123456789abcdef01234567";
        assert_eq!(online(Payload::Text(vec![json!([id])])).unwrap(), vec![id]);
        assert!(online(Payload::Text(vec![json!(["bad"])] )).is_none());
        assert!(online(Payload::Text(vec![json!("bad")])).is_none());
    }
    #[test]
    fn rejects_malformed_and_non_offer_call_events() {
        let data = json!({"from":"0123456789abcdef01234567", "signal":{"callId":"6bfa8180-b4fb-44f8-8922-6ad700b06b68", "type":"offer", "sdp":{"type":"offer", "sdp":"synthetic"}}});
        let parsed = incoming(Payload::Text(vec![data.clone()])).unwrap();
        assert_eq!(parsed.sdp, "synthetic");
        assert!(!format!("{parsed:?}").contains("synthetic"));
        let mut invalid = data.clone(); invalid["from"] = json!("invalid");
        assert!(incoming(Payload::Text(vec![invalid])).is_none());
        let mut invalid = data.clone(); invalid["signal"]["type"] = json!("ice");
        assert!(incoming(Payload::Text(vec![invalid])).is_none());
        let mut invalid = data.clone(); invalid["signal"]["callId"] = json!("invalid");
        assert!(incoming(Payload::Text(vec![invalid])).is_none());
        let mut invalid = data.clone(); invalid["signal"]["sdp"]["sdp"] = json!("");
        assert!(incoming(Payload::Text(vec![invalid])).is_none());
        let ice = json!({"from":"0123456789abcdef01234567", "signal":{"callId":"6bfa8180-b4fb-44f8-8922-6ad700b06b68", "type":"ice", "candidate":{"candidate":"candidate:synthetic", "sdpMid":"0"}}});
        let parsed = candidate(Payload::Text(vec![ice.clone()])).unwrap();
        assert_eq!(parsed.candidate.candidate, "candidate:synthetic");
        assert!(!format!("{parsed:?}").contains("synthetic"));
        let mut invalid = ice.clone(); invalid["signal"]["candidate"]["candidate"] = json!("candidate:bad\r\nline");
        assert!(candidate(Payload::Text(vec![invalid])).is_none());
        assert_eq!(ended(Payload::Text(vec![json!({"callId":"6bfa8180-b4fb-44f8-8922-6ad700b06b68"})])), Some("6bfa8180-b4fb-44f8-8922-6ad700b06b68".into()));
    }
}
