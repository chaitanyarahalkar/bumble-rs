use crate::proto::delete_bond_request;
use crate::proto::is_bonded_request;
use crate::proto::pairing_event;
use crate::proto::pairing_event_answer;
use crate::proto::secure_request;
use crate::proto::secure_response;
use crate::proto::security_server::Security;
use crate::proto::security_storage_server::SecurityStorage;
use crate::proto::wait_security_request;
use crate::proto::wait_security_response;
use crate::proto::{
    DeleteBondRequest, IsBondedRequest, LeSecurityLevel, PairingEvent, PairingEventAnswer,
    SecureRequest, SecureResponse, SecurityLevel, WaitSecurityRequest, WaitSecurityResponse,
};
use crate::runtime::{
    address, cookie, handle, ConnectionSecurity, PandoraRuntime, RuntimeState, POLL_INTERVAL,
    PROCEDURE_TIMEOUT,
};
use bumble::keys::{KeyStoreError, PairingKeys};
use bumble::{Address, AddressType};
use bumble_smp::{
    IdentityAddressType, IoCapability, KeyDistribution, ManagedPairingState, PairingCapabilities,
    PairingConfig, PairingDelegate, PairingRole, ScPairingState,
};
use bumble_transport::{ClassicPairingSession, LePairingSession};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;
use tonic::{Request, Response, Status, Streaming};

type ResponseStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

#[derive(Clone)]
struct PairingIo {
    id: u64,
    events: std_mpsc::SyncSender<PairingEvent>,
    answers: Arc<Mutex<std_mpsc::Receiver<PairingEventAnswer>>>,
}

#[derive(Default)]
struct PairingBridge {
    next_id: AtomicU64,
    active: Mutex<Option<PairingIo>>,
}

impl PairingBridge {
    fn install(
        &self,
    ) -> Result<
        (
            PairingIo,
            std_mpsc::Receiver<PairingEvent>,
            std_mpsc::Sender<PairingEventAnswer>,
        ),
        Status,
    > {
        let mut active = self
            .active
            .lock()
            .map_err(|_| Status::internal("pairing stream lock poisoned"))?;
        if active.is_some() {
            return Err(Status::aborted("already streaming pairing events"));
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (event_sender, event_receiver) = std_mpsc::sync_channel(16);
        let (answer_sender, answer_receiver) = std_mpsc::channel();
        let io = PairingIo {
            id,
            events: event_sender,
            answers: Arc::new(Mutex::new(answer_receiver)),
        };
        *active = Some(io.clone());
        Ok((io, event_receiver, answer_sender))
    }

    fn clear(&self, id: u64) {
        if let Ok(mut active) = self.active.lock() {
            if active.as_ref().is_some_and(|active| active.id == id) {
                *active = None;
            }
        }
    }

    fn active_io(&self) -> Option<PairingIo> {
        self.active.lock().ok()?.clone()
    }

    fn answer(&self, event: PairingEvent) -> Option<PairingEventAnswer> {
        let io = self.active_io()?;
        io.events.send(event.clone()).ok()?;
        let answer = io
            .answers
            .lock()
            .ok()?
            .recv_timeout(PROCEDURE_TIMEOUT)
            .ok()?;
        (answer.event.as_ref() == Some(&event)).then_some(answer)
    }

    fn notify(&self, event: PairingEvent) {
        if let Some(io) = self.active_io() {
            let _ = io.events.send(event);
        }
    }
}

struct PandoraPairingDelegate {
    bridge: Arc<PairingBridge>,
    connection_handle: u16,
}

impl PandoraPairingDelegate {
    fn event(&self, method: pairing_event::Method) -> PairingEvent {
        PairingEvent {
            remote: Some(pairing_event::Remote::Connection(cookie(
                self.connection_handle,
            ))),
            method: Some(method),
        }
    }
}

impl PairingDelegate for PandoraPairingDelegate {
    fn confirm(&mut self, _auto: bool) -> bool {
        if self.bridge.active_io().is_none() {
            return true;
        }
        let event = self.event(pairing_event::Method::JustWorks(()));
        self.bridge
            .answer(event)
            .and_then(|answer| answer.answer)
            .is_some_and(|answer| matches!(answer, pairing_event_answer::Answer::Confirm(true)))
    }

    fn compare_numbers(&mut self, number: u32, _digits: u8) -> bool {
        let event = self.event(pairing_event::Method::NumericComparison(number));
        self.bridge
            .answer(event)
            .and_then(|answer| answer.answer)
            .is_some_and(|answer| matches!(answer, pairing_event_answer::Answer::Confirm(true)))
    }

    fn get_number(&mut self) -> Option<u32> {
        let io = self.bridge.active_io()?;
        let event = self.event(pairing_event::Method::PasskeyEntryRequest(()));
        io.events.send(event.clone()).ok()?;
        let answer = io
            .answers
            .lock()
            .ok()?
            .recv_timeout(PROCEDURE_TIMEOUT)
            .ok()?;
        if answer.event.as_ref() != Some(&event) {
            return None;
        }
        match answer.answer {
            Some(pairing_event_answer::Answer::Passkey(passkey)) if passkey <= 999_999 => {
                Some(passkey)
            }
            _ => None,
        }
    }

    fn get_string(&mut self, max_length: usize) -> Option<String> {
        let io = self.bridge.active_io()?;
        let event = self.event(pairing_event::Method::PinCodeRequest(()));
        io.events.send(event.clone()).ok()?;
        let answer = io
            .answers
            .lock()
            .ok()?
            .recv_timeout(PROCEDURE_TIMEOUT)
            .ok()?;
        if answer.event.as_ref() != Some(&event) {
            return None;
        }
        let Some(pairing_event_answer::Answer::Pin(pin)) = answer.answer else {
            return None;
        };
        let pin = String::from_utf8(pin).ok()?;
        (!pin.is_empty() && pin.len() <= max_length).then_some(pin)
    }

    fn display_number(&mut self, number: u32, _digits: u8) {
        self.bridge
            .notify(self.event(pairing_event::Method::PasskeyEntryNotification(number)));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RequestedSecurity {
    Classic(SecurityLevel),
    Le(LeSecurityLevel),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcedureResult {
    Success,
    NotReached,
    ConnectionDied,
    PairingFailure,
    AuthenticationFailure,
    EncryptionFailure,
}

#[derive(Clone)]
pub struct SecurityService {
    runtime: PandoraRuntime,
    bridge: Arc<PairingBridge>,
}

impl SecurityService {
    pub fn new(runtime: PandoraRuntime) -> Self {
        Self {
            runtime,
            bridge: Arc::new(PairingBridge::default()),
        }
    }
}

#[derive(Clone)]
pub struct SecurityStorageService {
    runtime: PandoraRuntime,
}

impl SecurityStorageService {
    pub fn new(runtime: PandoraRuntime) -> Self {
        Self { runtime }
    }
}

fn io_capability(name: &str) -> Result<IoCapability, Status> {
    match name.to_ascii_lowercase().as_str() {
        "no_output_no_input" | "no_input_no_output" => Ok(IoCapability::NoInputNoOutput),
        "keyboard_input_only" | "keyboard_only" => Ok(IoCapability::KeyboardOnly),
        "display_output_only" | "display_only" => Ok(IoCapability::DisplayOnly),
        "display_output_and_yes_no_input" | "display_yes_no" => Ok(IoCapability::DisplayYesNo),
        "display_output_and_keyboard_input" | "keyboard_display" => {
            Ok(IoCapability::KeyboardDisplay)
        }
        _ => Err(Status::invalid_argument(format!(
            "unknown pairing I/O capability {name:?}"
        ))),
    }
}

fn pairing_config(state: &RuntimeState) -> Result<PairingConfig, Status> {
    let server = &state.config.server;
    let identity_address_type = match server.identity_address_type.to_ascii_lowercase().as_str() {
        "public" => Some(IdentityAddressType::Public),
        "random" => Some(IdentityAddressType::Random),
        value => {
            return Err(Status::invalid_argument(format!(
                "unknown pairing identity address type {value:?}"
            )))
        }
    };
    Ok(PairingConfig {
        secure_connections: server.pairing_sc_enable,
        ct2: false,
        mitm: server.pairing_mitm_enable,
        bonding: server.pairing_bonding_enable,
        capabilities: PairingCapabilities {
            io_capability: io_capability(&server.io_capability)?,
            local_initiator_key_distribution: KeyDistribution(
                server.smp_local_initiator_key_distribution,
            ),
            local_responder_key_distribution: KeyDistribution(
                server.smp_local_responder_key_distribution,
            ),
            maximum_encryption_key_size: 16,
        },
        identity_address_type,
        oob: None,
    })
}

fn secure_level(level: Option<secure_request::Level>) -> Result<RequestedSecurity, Status> {
    match level {
        Some(secure_request::Level::Classic(level)) => SecurityLevel::try_from(level)
            .map(RequestedSecurity::Classic)
            .map_err(|_| Status::invalid_argument("invalid Classic security level")),
        Some(secure_request::Level::Le(level)) => LeSecurityLevel::try_from(level)
            .map(RequestedSecurity::Le)
            .map_err(|_| Status::invalid_argument("invalid LE security level")),
        None => Err(Status::invalid_argument("security level is required")),
    }
}

fn wait_level(level: Option<wait_security_request::Level>) -> Result<RequestedSecurity, Status> {
    match level {
        Some(wait_security_request::Level::Classic(level)) => SecurityLevel::try_from(level)
            .map(RequestedSecurity::Classic)
            .map_err(|_| Status::invalid_argument("invalid Classic security level")),
        Some(wait_security_request::Level::Le(level)) => LeSecurityLevel::try_from(level)
            .map(RequestedSecurity::Le)
            .map_err(|_| Status::invalid_argument("invalid LE security level")),
        None => Err(Status::invalid_argument("security level is required")),
    }
}

fn validate_connection(
    state: &RuntimeState,
    connection_handle: u16,
    level: RequestedSecurity,
) -> Result<(), Status> {
    let valid = match level {
        RequestedSecurity::Classic(_) => {
            state.device.classic_connection(connection_handle).is_some()
        }
        RequestedSecurity::Le(_) => state.device.is_connected_on_handle(connection_handle),
    };
    valid.then_some(()).ok_or_else(|| {
        Status::invalid_argument("connection does not match the requested security transport")
    })
}

fn reached(state: &RuntimeState, connection_handle: u16, level: RequestedSecurity) -> bool {
    let security = state
        .connection_security
        .get(&connection_handle)
        .copied()
        .unwrap_or_default();
    match level {
        RequestedSecurity::Classic(SecurityLevel::Level0) => true,
        RequestedSecurity::Classic(SecurityLevel::Level1) => {
            !state
                .device
                .is_classic_encrypted_on_handle(connection_handle)
                || security.authenticated
        }
        RequestedSecurity::Classic(SecurityLevel::Level2) => {
            state
                .device
                .is_classic_encrypted_on_handle(connection_handle)
                && security.authenticated
        }
        RequestedSecurity::Classic(SecurityLevel::Level3) => {
            state
                .device
                .is_classic_encrypted_on_handle(connection_handle)
                && security.authenticated
                && matches!(security.link_key_type, Some(0x05 | 0x08))
        }
        RequestedSecurity::Classic(SecurityLevel::Level4) => {
            state
                .device
                .is_classic_encrypted_on_handle(connection_handle)
                && security.authenticated
                && security.secure_connections
                && security.link_key_type == Some(0x08)
        }
        RequestedSecurity::Le(LeSecurityLevel::LeLevel1) => true,
        RequestedSecurity::Le(LeSecurityLevel::LeLevel2) => {
            state.device.is_encrypted_on_handle(connection_handle)
        }
        RequestedSecurity::Le(LeSecurityLevel::LeLevel3) => {
            state.device.is_encrypted_on_handle(connection_handle) && security.authenticated
        }
        RequestedSecurity::Le(LeSecurityLevel::LeLevel4) => {
            state.device.is_encrypted_on_handle(connection_handle)
                && security.authenticated
                && security.secure_connections
        }
    }
}

fn key_authenticated(keys: &PairingKeys) -> bool {
    [
        keys.ltk.as_ref(),
        keys.ltk_central.as_ref(),
        keys.ltk_peripheral.as_ref(),
    ]
    .into_iter()
    .flatten()
    .any(|key| key.authenticated)
}

fn pair_le(
    state: &mut RuntimeState,
    connection_handle: u16,
    bridge: Arc<PairingBridge>,
    initiate: bool,
) -> Result<(), String> {
    let config = pairing_config(state).map_err(|status| status.message().to_owned())?;
    let bonding = config.bonding;
    let factory_bridge = Arc::clone(&bridge);
    let mut session = LePairingSession::new(
        &state.device,
        connection_handle,
        state.random_address.clone(),
        config,
        Box::new(move |handle, _role: PairingRole| {
            Box::new(PandoraPairingDelegate {
                bridge: Arc::clone(&factory_bridge),
                connection_handle: handle,
            })
        }),
    )
    .map_err(|error| error.to_string())?;
    let keys = if initiate {
        session
            .pair(&mut state.host, &mut state.device, PROCEDURE_TIMEOUT)
            .map_err(|error| error.to_string())?
    } else {
        session
            .listen(&state.device)
            .map_err(|error| error.to_string())?;
        session
            .run_to_completion(&mut state.host, &mut state.device, PROCEDURE_TIMEOUT)
            .map_err(|error| error.to_string())?
    };
    let secure_connections = matches!(
        session.state(),
        Some(ManagedPairingState::SecureConnections(
            ScPairingState::Complete
        ))
    );
    if bonding {
        session
            .store_bond(state.key_store.as_mut())
            .map_err(|error| error.to_string())?;
    }
    state.connection_security.insert(
        connection_handle,
        ConnectionSecurity {
            authenticated: key_authenticated(&keys),
            secure_connections,
            link_key_type: keys.link_key_type,
        },
    );
    Ok(())
}

fn pair_classic(
    state: &mut RuntimeState,
    connection_handle: u16,
    bridge: Arc<PairingBridge>,
    initiate: bool,
) -> Result<(), String> {
    let config = pairing_config(state).map_err(|status| status.message().to_owned())?;
    let bonding = config.bonding;
    let peer = state
        .device
        .classic_connection(connection_handle)
        .ok_or_else(|| "Classic connection is not active".to_string())?
        .peer_address
        .clone();
    let stored = state
        .key_store
        .get(&peer.to_string(false))
        .map_err(|error| error.to_string())?;
    let mut session = ClassicPairingSession::new(
        &state.device,
        connection_handle,
        config,
        Box::new(PandoraPairingDelegate {
            bridge,
            connection_handle,
        }),
        stored,
    )
    .map_err(|error| error.to_string())?;
    let keys = if initiate {
        session
            .pair(&mut state.host, &mut state.device, PROCEDURE_TIMEOUT)
            .map_err(|error| error.to_string())?
    } else {
        session
            .listen(&state.device)
            .map_err(|error| error.to_string())?;
        session
            .run_to_completion(&mut state.host, &mut state.device, PROCEDURE_TIMEOUT)
            .map_err(|error| error.to_string())?
    };
    if bonding {
        session
            .store_bond(state.key_store.as_mut())
            .map_err(|error| error.to_string())?;
    }
    let link_key_type = keys.link_key_type;
    state.connection_security.insert(
        connection_handle,
        ConnectionSecurity {
            authenticated: true,
            secure_connections: matches!(link_key_type, Some(0x07 | 0x08)),
            link_key_type,
        },
    );
    Ok(())
}

fn connection_alive(
    state: &RuntimeState,
    connection_handle: u16,
    level: RequestedSecurity,
) -> bool {
    match level {
        RequestedSecurity::Classic(_) => {
            state.device.classic_connection(connection_handle).is_some()
        }
        RequestedSecurity::Le(_) => state.device.is_connected_on_handle(connection_handle),
    }
}

fn secure_connection(
    state: &mut RuntimeState,
    connection_handle: u16,
    level: RequestedSecurity,
    bridge: Arc<PairingBridge>,
) -> ProcedureResult {
    if validate_connection(state, connection_handle, level).is_err() {
        return ProcedureResult::ConnectionDied;
    }
    if reached(state, connection_handle, level) {
        return ProcedureResult::Success;
    }
    let pairing = match level {
        RequestedSecurity::Classic(level) if level >= SecurityLevel::Level2 => {
            pair_classic(state, connection_handle, bridge, true)
        }
        RequestedSecurity::Le(level) if level >= LeSecurityLevel::LeLevel2 => {
            pair_le(state, connection_handle, bridge, true)
        }
        _ => Ok(()),
    };
    if pairing.is_err() {
        return if connection_alive(state, connection_handle, level) {
            match level {
                RequestedSecurity::Classic(_) => ProcedureResult::AuthenticationFailure,
                RequestedSecurity::Le(_) => ProcedureResult::PairingFailure,
            }
        } else {
            ProcedureResult::ConnectionDied
        };
    }
    if let RequestedSecurity::Classic(level) = level {
        if level >= SecurityLevel::Level2
            && !state
                .device
                .is_classic_encrypted_on_handle(connection_handle)
        {
            if !state.device.set_classic_encryption_on_handle(
                &mut state.host,
                connection_handle,
                true,
            ) {
                return ProcedureResult::EncryptionFailure;
            }
            let deadline = Instant::now() + PROCEDURE_TIMEOUT;
            while !state
                .device
                .is_classic_encrypted_on_handle(connection_handle)
            {
                if !connection_alive(state, connection_handle, level.into()) {
                    return ProcedureResult::ConnectionDied;
                }
                if Instant::now() >= deadline || state.poll(POLL_INTERVAL).is_err() {
                    return ProcedureResult::EncryptionFailure;
                }
            }
        }
    }
    if reached(state, connection_handle, level) {
        ProcedureResult::Success
    } else {
        ProcedureResult::NotReached
    }
}

impl From<SecurityLevel> for RequestedSecurity {
    fn from(level: SecurityLevel) -> Self {
        Self::Classic(level)
    }
}

fn wait_for_security(
    state: &mut RuntimeState,
    connection_handle: u16,
    level: RequestedSecurity,
    bridge: Arc<PairingBridge>,
) -> ProcedureResult {
    if validate_connection(state, connection_handle, level).is_err() {
        return ProcedureResult::ConnectionDied;
    }
    if reached(state, connection_handle, level) {
        return ProcedureResult::Success;
    }
    let pairing = match level {
        RequestedSecurity::Classic(level) if level >= SecurityLevel::Level2 => {
            pair_classic(state, connection_handle, bridge, false)
        }
        RequestedSecurity::Le(level) if level >= LeSecurityLevel::LeLevel2 => {
            pair_le(state, connection_handle, bridge, false)
        }
        _ => Ok(()),
    };
    if pairing.is_err() {
        return if connection_alive(state, connection_handle, level) {
            match level {
                RequestedSecurity::Classic(_) => ProcedureResult::AuthenticationFailure,
                RequestedSecurity::Le(_) => ProcedureResult::PairingFailure,
            }
        } else {
            ProcedureResult::ConnectionDied
        };
    }
    let deadline = Instant::now() + PROCEDURE_TIMEOUT;
    while !reached(state, connection_handle, level) {
        if !connection_alive(state, connection_handle, level) {
            return ProcedureResult::ConnectionDied;
        }
        if Instant::now() >= deadline {
            return ProcedureResult::PairingFailure;
        }
        if state.poll(POLL_INTERVAL).is_err() {
            return ProcedureResult::ConnectionDied;
        }
    }
    ProcedureResult::Success
}

fn secure_response(result: ProcedureResult) -> SecureResponse {
    let result = match result {
        ProcedureResult::Success => secure_response::Result::Success(()),
        ProcedureResult::NotReached => secure_response::Result::NotReached(()),
        ProcedureResult::ConnectionDied => secure_response::Result::ConnectionDied(()),
        ProcedureResult::PairingFailure => secure_response::Result::PairingFailure(()),
        ProcedureResult::AuthenticationFailure => {
            secure_response::Result::AuthenticationFailure(())
        }
        ProcedureResult::EncryptionFailure => secure_response::Result::EncryptionFailure(()),
    };
    SecureResponse {
        result: Some(result),
    }
}

fn wait_response(result: ProcedureResult) -> WaitSecurityResponse {
    let result = match result {
        ProcedureResult::Success => wait_security_response::Result::Success(()),
        ProcedureResult::ConnectionDied => wait_security_response::Result::ConnectionDied(()),
        ProcedureResult::AuthenticationFailure => {
            wait_security_response::Result::AuthenticationFailure(())
        }
        ProcedureResult::EncryptionFailure => wait_security_response::Result::EncryptionFailure(()),
        ProcedureResult::NotReached | ProcedureResult::PairingFailure => {
            wait_security_response::Result::PairingFailure(())
        }
    };
    WaitSecurityResponse {
        result: Some(result),
    }
}

fn request_address(
    address_request: Option<(Vec<u8>, AddressType)>,
) -> Result<Option<Address>, Status> {
    address_request
        .map(|(bytes, address_type)| address(bytes, address_type))
        .transpose()
}

#[tonic::async_trait]
impl Security for SecurityService {
    type OnPairingStream = ResponseStream<PairingEvent>;

    async fn on_pairing(
        &self,
        request: Request<Streaming<PairingEventAnswer>>,
    ) -> Result<Response<Self::OnPairingStream>, Status> {
        let has_connections = self
            .runtime
            .blocking(|state| {
                Ok(state.device.le_connections().next().is_some()
                    || state.device.classic_connections().next().is_some())
            })
            .await?;
        if has_connections {
            return Err(Status::aborted(
                "OnPairing must be initiated before establishing connections",
            ));
        }
        let (io, event_receiver, answer_sender) = self.bridge.install()?;
        let id = io.id;
        let bridge = Arc::clone(&self.bridge);
        let (sender, receiver) = mpsc::channel(16);
        let event_sender = sender.clone();
        tokio::task::spawn_blocking(move || loop {
            if event_sender.is_closed() {
                bridge.clear(id);
                return;
            }
            match event_receiver.recv_timeout(POLL_INTERVAL) {
                Ok(event) => {
                    if event_sender.blocking_send(Ok(event)).is_err() {
                        bridge.clear(id);
                        return;
                    }
                }
                Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                Err(std_mpsc::RecvTimeoutError::Disconnected) => return,
            }
        });
        let bridge = Arc::clone(&self.bridge);
        let mut incoming = request.into_inner();
        tokio::spawn(async move {
            loop {
                match incoming.message().await {
                    Ok(Some(answer)) => {
                        if answer_sender.send(answer).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let _ = sender.send(Err(Status::unknown(error.to_string()))).await;
                        break;
                    }
                }
            }
            bridge.clear(id);
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }

    async fn secure(
        &self,
        request: Request<SecureRequest>,
    ) -> Result<Response<SecureResponse>, Status> {
        let request = request.into_inner();
        let connection_handle = handle(request.connection)?;
        let level = secure_level(request.level)?;
        let bridge = Arc::clone(&self.bridge);
        let response = self
            .runtime
            .blocking(move |state| {
                validate_connection(state, connection_handle, level)?;
                Ok(secure_response(secure_connection(
                    state,
                    connection_handle,
                    level,
                    bridge,
                )))
            })
            .await?;
        Ok(Response::new(response))
    }

    async fn wait_security(
        &self,
        request: Request<WaitSecurityRequest>,
    ) -> Result<Response<WaitSecurityResponse>, Status> {
        let request = request.into_inner();
        let connection_handle = handle(request.connection)?;
        let level = wait_level(request.level)?;
        let bridge = Arc::clone(&self.bridge);
        let response = self
            .runtime
            .blocking(move |state| {
                validate_connection(state, connection_handle, level)?;
                Ok(wait_response(wait_for_security(
                    state,
                    connection_handle,
                    level,
                    bridge,
                )))
            })
            .await?;
        Ok(Response::new(response))
    }
}

#[tonic::async_trait]
impl SecurityStorage for SecurityStorageService {
    async fn is_bonded(&self, request: Request<IsBondedRequest>) -> Result<Response<bool>, Status> {
        let request = request.into_inner();
        let requested = match request.address {
            Some(is_bonded_request::Address::Public(bytes)) => {
                Some((bytes, AddressType::PUBLIC_DEVICE))
            }
            Some(is_bonded_request::Address::Random(bytes)) => {
                Some((bytes, AddressType::RANDOM_DEVICE))
            }
            None => None,
        };
        let address = request_address(requested)?;
        let bonded = self
            .runtime
            .blocking(move |state| {
                let Some(address) = address else {
                    return Ok(false);
                };
                state
                    .key_store
                    .get(&address.to_string(false))
                    .map(|keys| keys.is_some())
                    .map_err(|error| Status::internal(error.to_string()))
            })
            .await?;
        Ok(Response::new(bonded))
    }

    async fn delete_bond(
        &self,
        request: Request<DeleteBondRequest>,
    ) -> Result<Response<()>, Status> {
        let request = request.into_inner();
        let requested = match request.address {
            Some(delete_bond_request::Address::Public(bytes)) => {
                Some((bytes, AddressType::PUBLIC_DEVICE))
            }
            Some(delete_bond_request::Address::Random(bytes)) => {
                Some((bytes, AddressType::RANDOM_DEVICE))
            }
            None => None,
        };
        let address = request_address(requested)?;
        self.runtime
            .blocking(move |state| {
                let Some(address) = address else {
                    return Ok(());
                };
                match state.key_store.delete(&address.to_string(false)) {
                    Ok(()) | Err(KeyStoreError::NotFound(_)) => Ok(()),
                    Err(error) => Err(Status::internal(error.to_string())),
                }
            })
            .await?;
        Ok(Response::new(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bumble::keys::MemoryKeyStore;
    use bumble_hci::{
        Command, Event, HciPacket, ReturnParameters, HCI_AUTHENTICATION_REQUESTED_COMMAND,
        HCI_IO_CAPABILITY_REQUEST_REPLY_COMMAND, HCI_LINK_KEY_REQUEST_NEGATIVE_REPLY_COMMAND,
        HCI_USER_CONFIRMATION_REQUEST_REPLY_COMMAND,
    };
    use bumble_transport::{
        ExternalHost, ExternalHostActivity, PacketSink, PacketSource, Result as TransportResult,
        SplitOpenedTransport,
    };
    use std::collections::{BTreeMap, BTreeSet};
    use std::time::Duration;

    struct ChannelSource(std_mpsc::Receiver<HciPacket>);

    impl PacketSource for ChannelSource {
        fn read_packet(&mut self) -> TransportResult<Option<HciPacket>> {
            Ok(self.0.recv().ok())
        }
    }

    #[derive(Clone)]
    struct ClassicSecuritySink {
        incoming: std_mpsc::Sender<HciPacket>,
        packets: Arc<Mutex<Vec<HciPacket>>>,
        peer: Address,
        connection_handle: u16,
    }

    impl PacketSink for ClassicSecuritySink {
        fn write_packet(&mut self, packet: &HciPacket) -> TransportResult<()> {
            self.packets.lock().unwrap().push(packet.clone());
            match packet {
                HciPacket::Command(Command::AuthenticationRequested { connection_handle })
                    if *connection_handle == self.connection_handle =>
                {
                    for event in [
                        Event::CommandStatus {
                            status: 0,
                            num_hci_command_packets: 1,
                            command_opcode: HCI_AUTHENTICATION_REQUESTED_COMMAND,
                        },
                        Event::IoCapabilityResponse {
                            bd_addr: self.peer.clone(),
                            io_capability: IoCapability::NoInputNoOutput as u8,
                            oob_data_present: 0,
                            authentication_requirements: 0x05,
                        },
                        Event::IoCapabilityRequest {
                            bd_addr: self.peer.clone(),
                        },
                        Event::CommandComplete {
                            num_hci_command_packets: 1,
                            command_opcode: HCI_IO_CAPABILITY_REQUEST_REPLY_COMMAND,
                            return_parameters: ReturnParameters::Status { status: 0 },
                        },
                        Event::UserConfirmationRequest {
                            bd_addr: self.peer.clone(),
                            numeric_value: 123_456,
                        },
                        Event::CommandComplete {
                            num_hci_command_packets: 1,
                            command_opcode: HCI_USER_CONFIRMATION_REQUEST_REPLY_COMMAND,
                            return_parameters: ReturnParameters::Status { status: 0 },
                        },
                        Event::LinkKeyRequest {
                            bd_addr: self.peer.clone(),
                        },
                        Event::CommandComplete {
                            num_hci_command_packets: 1,
                            command_opcode: HCI_LINK_KEY_REQUEST_NEGATIVE_REPLY_COMMAND,
                            return_parameters: ReturnParameters::Status { status: 0 },
                        },
                        Event::LinkKeyNotification {
                            bd_addr: self.peer.clone(),
                            link_key: [0xA5; 16],
                            key_type: 0x08,
                        },
                        Event::SimplePairingComplete {
                            status: 0,
                            bd_addr: self.peer.clone(),
                        },
                        Event::AuthenticationComplete {
                            status: 0,
                            connection_handle: self.connection_handle,
                        },
                    ] {
                        self.incoming.send(HciPacket::Event(event)).unwrap();
                    }
                }
                HciPacket::Command(Command::SetConnectionEncryption {
                    connection_handle,
                    encryption_enable: 1,
                }) if *connection_handle == self.connection_handle => {
                    self.incoming
                        .send(HciPacket::Event(Event::EncryptionChange {
                            status: 0,
                            connection_handle: self.connection_handle,
                            encryption_enabled: 1,
                        }))
                        .unwrap();
                }
                _ => {}
            }
            Ok(())
        }
    }

    fn classic_runtime() -> (PandoraRuntime, Arc<Mutex<Vec<HciPacket>>>, Address) {
        let peer = Address::parse("11:22:33:44:55:66/P", AddressType::PUBLIC_DEVICE).unwrap();
        let public_address =
            Address::parse("AA:BB:CC:DD:EE:FF/P", AddressType::PUBLIC_DEVICE).unwrap();
        let random_address =
            Address::parse("C4:F2:17:1A:1D:AA", AddressType::RANDOM_DEVICE).unwrap();
        let connection_handle = 0x0234;
        let (incoming, receiver) = std_mpsc::channel();
        let packets = Arc::new(Mutex::new(Vec::new()));
        let sink = ClassicSecuritySink {
            incoming: incoming.clone(),
            packets: Arc::clone(&packets),
            peer: peer.clone(),
            connection_handle,
        };
        let mut host = ExternalHost::new(SplitOpenedTransport {
            source: Box::new(ChannelSource(receiver)),
            sink: Box::new(sink),
            metadata: BTreeMap::new(),
        });
        let mut device = bumble_host::Device::new(0);
        incoming
            .send(HciPacket::Event(Event::ConnectionComplete {
                status: 0,
                connection_handle,
                bd_addr: peer.clone(),
                link_type: 1,
                encryption_enabled: 0,
            }))
            .unwrap();
        assert_eq!(
            host.wait_for_activity(Duration::from_secs(1)).unwrap(),
            ExternalHostActivity::Packet
        );
        assert!(device.poll(&mut host));
        let state = RuntimeState {
            host,
            device,
            config: crate::config::PandoraConfig::default(),
            public_address,
            random_address,
            key_store: Box::new(MemoryKeyStore::new()),
            connection_security: BTreeMap::new(),
            waited_classic_connections: BTreeSet::new(),
            classic_discoverable: true,
            classic_connectable: true,
            l2cap_channels: BTreeMap::new(),
            pending_l2cap_channels: Vec::new(),
            l2cap_classic_servers: BTreeSet::new(),
            l2cap_le_servers: BTreeSet::new(),
        };
        (
            PandoraRuntime {
                state: Arc::new(Mutex::new(state)),
            },
            packets,
            peer,
        )
    }

    #[test]
    fn io_capability_names_match_upstream_configuration() {
        assert_eq!(
            io_capability("display_output_and_yes_no_input").unwrap(),
            IoCapability::DisplayYesNo
        );
        assert_eq!(
            io_capability("no_output_no_input").unwrap(),
            IoCapability::NoInputNoOutput
        );
        assert!(io_capability("magic_keyboard").is_err());
    }

    #[test]
    fn pairing_delegate_uses_connection_cookie_and_validates_answers() {
        let bridge = Arc::new(PairingBridge::default());
        let (_io, events, answers) = bridge.install().unwrap();
        let bridge_for_answer = Arc::clone(&bridge);
        std::thread::spawn(move || {
            let event = events.recv().unwrap();
            assert_eq!(
                event.remote,
                Some(pairing_event::Remote::Connection(cookie(0x1234)))
            );
            answers
                .send(PairingEventAnswer {
                    event: Some(event),
                    answer: Some(pairing_event_answer::Answer::Confirm(true)),
                })
                .unwrap();
            drop(bridge_for_answer);
        });
        let mut delegate = PandoraPairingDelegate {
            bridge,
            connection_handle: 0x1234,
        };
        assert!(delegate.confirm(false));
    }

    #[test]
    fn pairing_delegate_only_auto_accepts_just_works_without_a_stream() {
        let mut delegate = PandoraPairingDelegate {
            bridge: Arc::new(PairingBridge::default()),
            connection_handle: 0x1234,
        };
        assert!(delegate.confirm(true));
        assert!(!delegate.compare_numbers(123_456, 6));
        assert_eq!(delegate.get_number(), None);
        assert_eq!(delegate.get_string(16), None);
    }

    #[test]
    fn security_response_variants_are_canonical() {
        assert!(matches!(
            secure_response(ProcedureResult::NotReached).result,
            Some(secure_response::Result::NotReached(()))
        ));
        assert!(matches!(
            wait_response(ProcedureResult::EncryptionFailure).result,
            Some(wait_security_response::Result::EncryptionFailure(()))
        ));
        assert!(matches!(
            secure_response(ProcedureResult::AuthenticationFailure).result,
            Some(secure_response::Result::AuthenticationFailure(()))
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn classic_secure_rpc_pairs_encrypts_and_stores_bond() {
        let (runtime, packets, peer) = classic_runtime();
        let service = SecurityService::new(runtime.clone());
        let response = service
            .secure(Request::new(SecureRequest {
                connection: Some(cookie(0x0234)),
                level: Some(secure_request::Level::Classic(SecurityLevel::Level2 as i32)),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(
            matches!(response.result, Some(secure_response::Result::Success(()))),
            "unexpected secure response: {response:?}; packets: {:?}",
            packets.lock().unwrap()
        );

        let state = runtime.state.lock().unwrap();
        assert!(state.device.is_classic_encrypted_on_handle(0x0234));
        assert!(state
            .key_store
            .get(&peer.to_string(false))
            .unwrap()
            .is_some());
        let security = state.connection_security.get(&0x0234).unwrap();
        assert!(security.authenticated);
        assert!(security.secure_connections);
        assert_eq!(security.link_key_type, Some(0x08));
        drop(state);

        let packets = packets.lock().unwrap();
        assert!(packets.iter().any(|packet| matches!(
            packet,
            HciPacket::Command(Command::AuthenticationRequested {
                connection_handle: 0x0234
            })
        )));
        assert!(packets.iter().any(|packet| matches!(
            packet,
            HciPacket::Command(Command::UserConfirmationRequestReply { bd_addr })
                if bd_addr == &peer
        )));
        assert!(packets.iter().any(|packet| matches!(
            packet,
            HciPacket::Command(Command::SetConnectionEncryption {
                connection_handle: 0x0234,
                encryption_enable: 1,
            })
        )));
    }
}
