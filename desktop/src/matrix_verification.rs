//! Experimental same-account SAS flow. The owner keeps the store locked while
//! comparing codes; no server authorization is granted by this module.
#![allow(dead_code)]
use crate::{api::Session, matrix_store::MatrixStore};
use matrix_sdk_crypto::{Sas, Verification, VerificationRequest, types::requests::{AnyOutgoingRequest, OutgoingVerificationRequest}};

pub struct DeviceListener { store: MatrixStore, session: Session }
impl DeviceListener {
    pub fn new(store: MatrixStore, session: Session) -> Self { Self { store, session } }
    pub async fn requests(&mut self) -> Result<Vec<(String, String)>, String> {
        self.session.refresh_matrix_own_keys(&self.store.machine).await?;
        self.session.receive_matrix_verification(&self.store.machine).await?;
        let requests = self.store.machine.get_verification_requests(self.store.machine.user_id());
        if requests.len() > 100 { return Err("Demasiados pedidos de verificacao".into()); }
        Ok(requests.into_iter().filter_map(|request| {
            let device = request.other_device_id()?;
            if request.we_started() || !request.is_self_verification() || request.is_done() || request.is_cancelled() || request.timed_out() || device == self.store.machine.device_id() { return None; }
            Some((request.flow_id().as_str().to_owned(), device.to_string()))
        }).collect())
    }
    pub async fn accept(self, flow: &str) -> Result<DeviceVerification, String> {
        DeviceVerification::accept(self.store, self.session, flow).await
    }
}

pub struct DeviceVerification {
    store: MatrixStore,
    session: Session,
    target: String,
    request: VerificationRequest,
    sas: Option<Sas>,
}
impl DeviceVerification {
    pub async fn start(mut store: MatrixStore, session: Session, target: String) -> Result<Self, String> {
        session.refresh_matrix_own_keys(&store.machine).await?;
        session.claim_matrix_own_key(&store.machine, &target).await?;
        let device = store.machine.get_device(store.machine.user_id(), target.as_str().into(), None).await.map_err(|_| "Dispositivo indisponivel")?.ok_or("Dispositivo indisponivel")?;
        let (request, outgoing) = device.request_verification();
        send(&mut store, &session, &target, outgoing).await?;
        Ok(Self { store, session, target, request, sas: None })
    }
    pub async fn accept(mut store: MatrixStore, session: Session, flow: &str) -> Result<Self, String> {
        session.refresh_matrix_own_keys(&store.machine).await?;
        session.receive_matrix_verification(&store.machine).await?;
        let request = store.machine.get_verification_request(store.machine.user_id(), flow).ok_or("Pedido de verificacao indisponivel")?;
        let target = request.other_device_id().ok_or("Dispositivo indisponivel")?.to_string();
        if request.we_started() || !request.is_self_verification() || request.is_cancelled() || request.timed_out() || target == store.machine.device_id().as_str() { return Err("Pedido de verificacao invalido".into()); }
        let outgoing = request.accept().ok_or("Nao foi possivel aceitar a verificacao")?;
        send(&mut store, &session, &target, outgoing).await?;
        Ok(Self { store, session, target, request, sas: None })
    }
    pub fn flow_id(&self) -> &str { self.request.flow_id().as_str() }
    pub fn target(&self) -> &str { &self.target }
    pub fn codes(&self) -> Option<(u16, u16, u16)> { self.sas.as_ref().and_then(Sas::decimals) }
    pub fn done(&self) -> bool { self.sas.as_ref().is_some_and(Sas::is_done) }
    pub async fn poll(&mut self) -> Result<(), String> {
        self.session.receive_matrix_verification(&self.store.machine).await?;
        if self.request.is_cancelled() || self.request.timed_out() { return Err("Verificacao cancelada ou expirada".into()); }
        if self.sas.is_none() && self.request.we_started() && self.request.is_ready() {
            let (sas, outgoing) = self.request.start_sas().await.map_err(|_| "Verificacao indisponivel")?.ok_or("Verificacao indisponivel")?;
            self.sas = Some(sas);
            send(&mut self.store, &self.session, &self.target, outgoing).await?;
        }
        if self.sas.is_none() {
            if let Some(Verification::SasV1(sas)) = self.store.machine.get_verification(self.store.machine.user_id(), self.request.flow_id().as_str()) {
                if !sas.is_self_verification() || sas.other_device_id().as_str() != self.target { return Err("Dispositivo de verificacao mudou".into()); }
                if let Some(outgoing) = sas.accept() { send(&mut self.store, &self.session, &self.target, outgoing).await?; }
                self.sas = Some(*sas);
            }
        }
        for request in self.store.machine.outgoing_requests().await.map_err(|_| "Verificacao indisponivel")? {
            if let AnyOutgoingRequest::ToDeviceRequest(body) = request.request() {
                let messages = serde_json::to_value(&body.messages).map_err(|_| "Verificacao invalida")?;
                let flow = messages.get(self.store.machine.user_id().as_str()).and_then(|value| value.get(&self.target)).and_then(|value| value.get("transaction_id")).and_then(|value| value.as_str());
                if body.event_type.to_string().starts_with("m.key.verification.") && flow == Some(self.flow_id()) { self.store.send_verification(&self.session, &self.target, body).await?; }
            }
        }
        Ok(())
    }
    /// Call only after the user compared all three values on both devices.
    pub async fn confirm(&mut self) -> Result<(), String> {
        let sas = self.sas.as_ref().ok_or("Codigos ainda indisponiveis")?;
        if sas.decimals().is_none() || sas.is_cancelled() || sas.timed_out() { return Err("Codigos ainda indisponiveis".into()); }
        for request in sas.confirm().await.map_err(|_| "Nao foi possivel confirmar os codigos")?.0 { send(&mut self.store, &self.session, &self.target, request).await?; }
        Ok(())
    }
    /// Server access is granted only to the exact device verified in this flow.
    pub async fn approve(&self) -> Result<(), String> {
        let failure = || "Conclui primeiro a verificacao dos codigos.".to_string();
        let sas = self.sas.as_ref().ok_or_else(failure)?;
        if !sas.is_self_verification() || !sas.is_done() || !sas.have_we_confirmed() || sas.is_cancelled() || sas.timed_out() || sas.other_device_id().as_str() != self.target { return Err(failure()); }
        self.session.refresh_matrix_own_keys(&self.store.machine).await?;
        let device = self.store.machine.get_device(self.store.machine.user_id(), self.target.as_str().into(), None).await.map_err(|_| failure())?.ok_or_else(failure)?;
        if device.is_deleted() || device.is_blacklisted() || !device.is_verified() { return Err(failure()); }
        let key = device.ed25519_key().ok_or_else(failure)?.to_base64();
        self.session.authorize_matrix_device(&self.store.machine, &self.target, &key).await
    }
    pub async fn cancel(&mut self) -> Result<(), String> {
        let outgoing = match &self.sas { Some(sas) => sas.cancel(), None => self.request.cancel() };
        if let Some(request) = outgoing { send(&mut self.store, &self.session, &self.target, request).await?; }
        Ok(())
    }
    pub async fn close(self) -> Result<(), String> { self.store.close().await }
    pub fn into_store(self) -> MatrixStore { self.store }
}
async fn send(store: &mut MatrixStore, session: &Session, target: &str, request: OutgoingVerificationRequest) -> Result<(), String> {
    let OutgoingVerificationRequest::ToDevice(request) = request else { return Err("Verificacao invalida".into()); };
    store.send_verification(session, target, &request).await
}
