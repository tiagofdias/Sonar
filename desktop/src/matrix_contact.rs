//! Experimental room-scoped SAS verification between different accounts.
#![allow(dead_code)]
use crate::{api::{MatrixRoomSnapshot, Session}, matrix_store::MatrixStore};
use matrix_sdk_crypto::{Sas, Verification, VerificationRequest, types::requests::{AnyOutgoingRequest, OutgoingVerificationRequest}};

pub struct ContactVerification { store: MatrixStore, session: Session, room: MatrixRoomSnapshot, user: String, device: String, curve: String, ed25519: String, request: VerificationRequest, sas: Option<Sas> }
pub struct ContactListener { store: MatrixStore, session: Session, room: MatrixRoomSnapshot, user: String, device: String, curve: String }
impl ContactListener {
    pub async fn open(mut store: MatrixStore, session: Session, peer: &str, device: &str) -> Result<Self, String> {
        let (room, user, curve, _) = ContactVerification::prepare(&mut store, &session, peer, device).await?;
        Ok(Self { store, session, room, user, device:device.into(), curve })
    }
    pub async fn requests(&mut self) -> Result<Vec<String>, String> {
        self.store.receive_contact_verification(&self.session, &self.room, &self.user, &self.device, &self.curve).await?;
        let user = ruma::UserId::parse(&self.user).map_err(|_| "Contacto invalido")?;
        let requests = self.store.machine.get_verification_requests(&user);
        if requests.len() > 100 { return Err("Demasiados pedidos".into()); }
        Ok(requests.into_iter().filter(|request| !request.we_started() && !request.is_self_verification() && !request.is_done() && !request.is_cancelled() && !request.timed_out() && request.other_device_id().as_ref().is_some_and(|id| id.as_str() == self.device)).map(|request| request.flow_id().as_str().to_owned()).collect())
    }
    pub async fn accept(self, flow: &str) -> Result<ContactVerification, String> {
        let peer = ruma::UserId::parse(&self.user).map_err(|_| "Contacto invalido")?;
        ContactVerification::accept(self.store, self.session, peer.localpart(), &self.device, flow).await
    }
}
impl ContactVerification {
    async fn prepare(store: &mut MatrixStore, session: &Session, peer: &str, device: &str) -> Result<(MatrixRoomSnapshot, String, String, String), String> {
        let room = store.prepare_direct_room(session, peer).await?;
        let user = room.members.iter().find(|id| id.as_str() != store.machine.user_id().as_str()).ok_or("Contacto indisponivel")?.clone();
        let devices = session.matrix_room_devices(&store.machine, &room, false).await?;
        let target = devices.iter().find(|item| item.user_id == user && item.device_id == device).ok_or("Dispositivo indisponivel")?;
        let curve = target.curve25519.clone();
        let ed25519 = target.ed25519.clone();
        session.claim_matrix_contact_key(&store.machine, &room, &user, device).await?;
        Ok((room, user, curve, ed25519))
    }
    pub async fn start(mut store: MatrixStore, session: Session, peer: &str, device: &str) -> Result<Self, String> {
        let (room, user, curve, ed25519) = Self::prepare(&mut store, &session, peer, device).await?;
        let user_id = ruma::UserId::parse(&user).map_err(|_| "Contacto invalido")?;
        let target = store.machine.get_device(&user_id, device.into(), None).await.map_err(|_| "Dispositivo indisponivel")?.ok_or("Dispositivo indisponivel")?;
        let (request, outgoing) = target.request_verification();
        send(&mut store, &session, &room, &user, device, outgoing).await?;
        Ok(Self { store, session, room, user, device:device.into(), curve, ed25519, request, sas:None })
    }
    pub async fn accept(mut store: MatrixStore, session: Session, peer: &str, device: &str, flow: &str) -> Result<Self, String> {
        let (room, user, curve, ed25519) = Self::prepare(&mut store, &session, peer, device).await?;
        store.receive_contact_verification(&session, &room, &user, device, &curve).await?;
        let user_id = ruma::UserId::parse(&user).map_err(|_| "Contacto invalido")?;
        let request = store.machine.get_verification_request(&user_id, flow).ok_or("Pedido indisponivel")?;
        if request.we_started() || request.is_self_verification() || request.is_cancelled() || request.timed_out() || request.other_device_id().as_ref().map(|id| id.as_str()) != Some(device) { return Err("Pedido invalido".into()); }
        send(&mut store, &session, &room, &user, device, request.accept().ok_or("Pedido indisponivel")?).await?;
        Ok(Self { store, session, room, user, device:device.into(), curve, ed25519, request, sas:None })
    }
    pub fn flow_id(&self) -> &str { self.request.flow_id().as_str() }
    pub fn codes(&self) -> Option<(u16,u16,u16)> { self.sas.as_ref().and_then(Sas::decimals) }
    pub fn done(&self) -> bool { self.sas.as_ref().is_some_and(Sas::is_done) }
    pub async fn poll(&mut self) -> Result<(), String> {
        let devices = self.session.matrix_room_devices(&self.store.machine, &self.room, false).await?;
        if !devices.iter().any(|item| item.user_id == self.user && item.device_id == self.device && item.curve25519 == self.curve && item.ed25519 == self.ed25519) { return Err("Identidade do contacto mudou".into()); }
        self.store.receive_contact_verification(&self.session, &self.room, &self.user, &self.device, &self.curve).await?;
        if self.request.is_cancelled() || self.request.timed_out() { return Err("Verificacao cancelada ou expirada".into()); }
        if self.sas.is_none() && self.request.we_started() && self.request.is_ready() {
            let (sas, outgoing) = self.request.start_sas().await.map_err(|_| "Verificacao indisponivel")?.ok_or("Verificacao indisponivel")?;
            self.sas = Some(sas); send(&mut self.store, &self.session, &self.room, &self.user, &self.device, outgoing).await?;
        }
        if self.sas.is_none() {
            let user = ruma::UserId::parse(&self.user).map_err(|_| "Contacto invalido")?;
            if let Some(Verification::SasV1(sas)) = self.store.machine.get_verification(&user, self.flow_id()) {
                if sas.is_self_verification() || sas.other_device_id().as_str() != self.device { return Err("Identidade do contacto mudou".into()); }
                if let Some(outgoing) = sas.accept() { send(&mut self.store, &self.session, &self.room, &self.user, &self.device, outgoing).await?; }
                self.sas = Some(*sas);
            }
        }
        for request in self.store.machine.outgoing_requests().await.map_err(|_| "Verificacao indisponivel")? {
            if let AnyOutgoingRequest::ToDeviceRequest(body) = request.request() {
                let messages = serde_json::to_value(&body.messages).map_err(|_| "Verificacao invalida")?;
                let flow = messages.get(&self.user).and_then(|v| v.get(&self.device)).and_then(|v| v.get("transaction_id")).and_then(|v| v.as_str());
                if body.event_type.to_string().starts_with("m.key.verification.") && flow == Some(self.flow_id()) { self.store.send_contact_verification(&self.session, &self.room, &self.user, &self.device, body).await?; }
            }
        }
        Ok(())
    }
    pub async fn confirm(&mut self) -> Result<(), String> {
        let sas = self.sas.as_ref().ok_or("Codigos indisponiveis")?;
        if sas.decimals().is_none() || sas.is_cancelled() || sas.timed_out() { return Err("Codigos indisponiveis".into()); }
        for request in sas.confirm().await.map_err(|_| "Nao foi possivel confirmar")?.0 { send(&mut self.store, &self.session, &self.room, &self.user, &self.device, request).await?; }
        Ok(())
    }
    pub async fn cancel(&mut self) -> Result<(), String> {
        let request = match &self.sas { Some(sas) => sas.cancel(), None => self.request.cancel() };
        if let Some(request) = request { send(&mut self.store, &self.session, &self.room, &self.user, &self.device, request).await?; }
        Ok(())
    }
    pub fn into_store(self) -> MatrixStore { self.store }
}
async fn send(store: &mut MatrixStore, session: &Session, room: &MatrixRoomSnapshot, user: &str, target: &str, request: OutgoingVerificationRequest) -> Result<(), String> {
    let OutgoingVerificationRequest::ToDevice(request) = request else { return Err("Verificacao invalida".into()); };
    store.send_contact_verification(session, room, user, target, &request).await
}
