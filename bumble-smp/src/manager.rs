//! Connection-handle keyed SMP session orchestration.

use std::collections::{BTreeMap, VecDeque};

use bumble::keys::{KeyStore, KeyStoreResult, PairingKeys};
use bumble::Address;
use bumble_crypto::{random_128, EccKey};

use crate::{
    AuthReq, ClassicCtkdSession, ClassicCtkdState, Error, LegacyPairingSession, PairingConfig,
    PairingDelegate, PairingFailureReason, PairingRole, PairingState, Result, ScPairingSession,
    ScPairingState, SmpPdu,
};

pub type PairingDelegateFactory =
    Box<dyn FnMut(u16, PairingRole) -> Box<dyn PairingDelegate> + Send>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingConnection {
    pub handle: u16,
    pub role: PairingRole,
    pub local_address: Address,
    pub peer_address: Address,
    pub transport: PairingTransport,
    pub link_key: Option<[u8; 16]>,
    pub authenticated: bool,
    pub encrypted: bool,
}

impl PairingConnection {
    pub fn le(
        handle: u16,
        role: PairingRole,
        local_address: Address,
        peer_address: Address,
    ) -> Self {
        Self {
            handle,
            role,
            local_address,
            peer_address,
            transport: PairingTransport::Le,
            link_key: None,
            authenticated: false,
            encrypted: false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn br_edr(
        handle: u16,
        role: PairingRole,
        local_address: Address,
        peer_address: Address,
        link_key: [u8; 16],
        authenticated: bool,
        encrypted: bool,
    ) -> Self {
        Self {
            handle,
            role,
            local_address,
            peer_address,
            transport: PairingTransport::BrEdr,
            link_key: Some(link_key),
            authenticated,
            encrypted,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairingTransport {
    Le,
    BrEdr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagedPairingState {
    Legacy(PairingState),
    SecureConnections(ScPairingState),
    ClassicCtkd(ClassicCtkdState),
}

enum ManagedSession {
    Legacy(Box<LegacyPairingSession>),
    SecureConnections(Box<ScPairingSession>),
    ClassicCtkd(Box<ClassicCtkdSession>),
}

impl ManagedSession {
    fn process(&mut self, pdu: SmpPdu) -> Result<()> {
        match self {
            Self::Legacy(session) => session.process(pdu),
            Self::SecureConnections(session) => session.process(pdu),
            Self::ClassicCtkd(session) => session.process(pdu),
        }
    }

    fn start(&mut self) -> Result<()> {
        match self {
            Self::Legacy(session) => session.start(),
            Self::SecureConnections(session) => session.start(),
            Self::ClassicCtkd(session) => session.start(),
        }
    }

    fn mark_encrypted(&mut self) -> Result<()> {
        match self {
            Self::Legacy(session) => session.mark_encrypted(),
            Self::SecureConnections(session) => session.mark_encrypted(),
            Self::ClassicCtkd(_) => Err(Error::InvalidPacket(
                "Classic CTKD starts only on an already encrypted ACL".into(),
            )),
        }
    }

    fn drain_outbound(&mut self) -> Vec<SmpPdu> {
        match self {
            Self::Legacy(session) => session.drain_outbound(),
            Self::SecureConnections(session) => session.drain_outbound(),
            Self::ClassicCtkd(session) => session.drain_outbound(),
        }
    }

    fn state(&self) -> ManagedPairingState {
        match self {
            Self::Legacy(session) => ManagedPairingState::Legacy(session.state()),
            Self::SecureConnections(session) => {
                ManagedPairingState::SecureConnections(session.state())
            }
            Self::ClassicCtkd(session) => ManagedPairingState::ClassicCtkd(session.state()),
        }
    }

    fn failure(&self) -> Option<PairingFailureReason> {
        match self {
            Self::Legacy(session) => session.failure(),
            Self::SecureConnections(session) => session.failure(),
            Self::ClassicCtkd(session) => session.failure(),
        }
    }

    fn pairing_keys(&self) -> Option<PairingKeys> {
        match self {
            Self::Legacy(session) => session.pairing_keys(),
            Self::SecureConnections(session) => session.pairing_keys(),
            Self::ClassicCtkd(session) => session.pairing_keys(),
        }
    }

    fn encryption_key(&self) -> Option<[u8; 16]> {
        match self {
            Self::Legacy(session) => session.stk(),
            Self::SecureConnections(session) => session.ltk(),
            Self::ClassicCtkd(session) => session.outcome().map(|outcome| outcome.ltk),
        }
    }

    fn store_bond(&self, store: &mut dyn KeyStore) -> KeyStoreResult<bool> {
        match self {
            Self::Legacy(session) => session.store_bond(store),
            Self::SecureConnections(session) => session.store_bond(store),
            Self::ClassicCtkd(session) => session.store_bond(store),
        }
    }
}

pub struct PairingManager {
    config: PairingConfig,
    delegate_factory: PairingDelegateFactory,
    connections: BTreeMap<u16, PairingConnection>,
    sessions: BTreeMap<u16, ManagedSession>,
    outbound: VecDeque<(u16, SmpPdu)>,
    security_requests: VecDeque<(u16, AuthReq)>,
}

impl PairingManager {
    pub fn new(config: PairingConfig, delegate_factory: PairingDelegateFactory) -> Self {
        Self {
            config,
            delegate_factory,
            connections: BTreeMap::new(),
            sessions: BTreeMap::new(),
            outbound: VecDeque::new(),
            security_requests: VecDeque::new(),
        }
    }

    pub fn register_connection(&mut self, connection: PairingConnection) -> Result<()> {
        if self.connections.contains_key(&connection.handle) {
            return Err(Error::InvalidPacket(format!(
                "connection handle 0x{:04X} is already registered",
                connection.handle
            )));
        }
        self.connections.insert(connection.handle, connection);
        Ok(())
    }

    /// Override the protocol role before a pairing session starts. This is
    /// used when a peer is explicitly asked to initiate pairing even though
    /// the local controller owns the physical central role.
    pub fn set_connection_role(&mut self, handle: u16, role: PairingRole) -> Result<()> {
        if self.sessions.contains_key(&handle) {
            return Err(Error::InvalidPacket(
                "cannot change pairing role while a session is active".into(),
            ));
        }
        self.connection_mut(handle)?.role = role;
        Ok(())
    }

    pub fn pair(&mut self, handle: u16) -> Result<()> {
        let connection = self.connection(handle)?.clone();
        if connection.role != PairingRole::Initiator {
            return Err(Error::InvalidPacket(
                "only the initiator role can start pairing".into(),
            ));
        }
        if self.sessions.contains_key(&handle) {
            return Err(Error::InvalidPacket(
                "pairing session already active".into(),
            ));
        }
        let mut session = self.create_session(&connection)?;
        session.start()?;
        self.sessions.insert(handle, session);
        self.collect_outbound(handle);
        Ok(())
    }

    pub fn receive(&mut self, handle: u16, pdu: SmpPdu) -> Result<()> {
        if let SmpPdu::SecurityRequest { auth_req } = pdu {
            self.connection(handle)?;
            self.security_requests
                .push_back((handle, AuthReq(auth_req)));
            return Ok(());
        }
        if !self.sessions.contains_key(&handle) {
            if !matches!(pdu, SmpPdu::PairingRequest(_)) {
                return Err(Error::InvalidPacket(
                    "no pairing session for non-request PDU".into(),
                ));
            }
            let connection = self.connection(handle)?.clone();
            if connection.role != PairingRole::Responder {
                return Err(Error::InvalidPacket(
                    "initiator received peer-started Pairing Request".into(),
                ));
            }
            let session = self.create_session(&connection)?;
            self.sessions.insert(handle, session);
        }
        self.sessions
            .get_mut(&handle)
            .expect("session created above")
            .process(pdu)?;
        self.collect_outbound(handle);
        Ok(())
    }

    pub fn mark_encrypted(&mut self, handle: u16) -> Result<()> {
        self.sessions
            .get_mut(&handle)
            .ok_or_else(|| Error::InvalidPacket("no pairing session for connection".into()))?
            .mark_encrypted()?;
        self.collect_outbound(handle);
        Ok(())
    }

    pub fn poll_outbound(&mut self) -> Option<(u16, SmpPdu)> {
        self.outbound.pop_front()
    }

    pub fn drain_outbound(&mut self) -> Vec<(u16, SmpPdu)> {
        self.outbound.drain(..).collect()
    }

    pub fn poll_security_request(&mut self) -> Option<(u16, AuthReq)> {
        self.security_requests.pop_front()
    }

    pub fn state(&self, handle: u16) -> Option<ManagedPairingState> {
        self.sessions.get(&handle).map(ManagedSession::state)
    }

    pub fn failure(&self, handle: u16) -> Option<PairingFailureReason> {
        self.sessions.get(&handle).and_then(ManagedSession::failure)
    }

    pub fn pairing_keys(&self, handle: u16) -> Option<PairingKeys> {
        self.sessions
            .get(&handle)
            .and_then(ManagedSession::pairing_keys)
    }

    pub fn encryption_key(&self, handle: u16) -> Option<[u8; 16]> {
        self.sessions
            .get(&handle)
            .and_then(ManagedSession::encryption_key)
    }

    pub fn store_bond(&self, handle: u16, store: &mut dyn KeyStore) -> KeyStoreResult<bool> {
        match self.sessions.get(&handle) {
            Some(session) => session.store_bond(store),
            None => Ok(false),
        }
    }

    pub fn disconnect(&mut self, handle: u16) -> bool {
        let existed = self.connections.remove(&handle).is_some();
        self.sessions.remove(&handle);
        self.outbound
            .retain(|(queued_handle, _)| *queued_handle != handle);
        self.security_requests
            .retain(|(queued_handle, _)| *queued_handle != handle);
        existed
    }

    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    fn connection(&self, handle: u16) -> Result<&PairingConnection> {
        self.connections
            .get(&handle)
            .ok_or_else(|| Error::InvalidPacket("unknown connection handle".into()))
    }

    fn connection_mut(&mut self, handle: u16) -> Result<&mut PairingConnection> {
        self.connections.get_mut(&handle).ok_or_else(|| {
            Error::InvalidPacket(format!("unknown connection handle 0x{handle:04X}"))
        })
    }

    fn create_session(&mut self, connection: &PairingConnection) -> Result<ManagedSession> {
        let (initiator_address, responder_address) = match connection.role {
            PairingRole::Initiator => (
                connection.local_address.clone(),
                connection.peer_address.clone(),
            ),
            PairingRole::Responder => (
                connection.peer_address.clone(),
                connection.local_address.clone(),
            ),
        };
        if connection.transport == PairingTransport::BrEdr {
            return Ok(ManagedSession::ClassicCtkd(Box::new(
                ClassicCtkdSession::new(
                    connection.role,
                    self.config.clone(),
                    initiator_address,
                    responder_address,
                    connection.link_key.ok_or_else(|| {
                        Error::InvalidPacket("Classic CTKD connection has no Link Key".into())
                    })?,
                    connection.authenticated,
                    connection.encrypted,
                )?,
            )));
        }
        let delegate = (self.delegate_factory)(connection.handle, connection.role);
        if self.config.secure_connections {
            Ok(ManagedSession::SecureConnections(Box::new(
                ScPairingSession::new(
                    connection.role,
                    self.config.clone(),
                    delegate,
                    initiator_address,
                    responder_address,
                    EccKey::generate(),
                    random_128(),
                )?,
            )))
        } else {
            Ok(ManagedSession::Legacy(Box::new(LegacyPairingSession::new(
                connection.role,
                self.config.clone(),
                delegate,
                initiator_address,
                responder_address,
                random_128(),
            )?)))
        }
    }

    fn collect_outbound(&mut self, handle: u16) {
        if let Some(session) = self.sessions.get_mut(&handle) {
            self.outbound.extend(
                session
                    .drain_outbound()
                    .into_iter()
                    .map(|pdu| (handle, pdu)),
            );
        }
    }
}
