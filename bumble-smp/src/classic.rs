//! SMP-over-BR/EDR Cross-Transport Key Derivation session.

use std::collections::VecDeque;

use bumble::keys::{Key, KeyStore, KeyStoreResult, PairingKeys};
use bumble::Address;

use crate::{
    derive_ltk, AuthReq, Error, IoCapability, KeyDistribution, KeyDistributionConfig,
    KeyDistributionSession, KeyDistributionState, LocalKeyMaterial, PairingConfig,
    PairingFailureReason, PairingFeatures, PairingMethod, PairingRole, Result, SmpPdu,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassicCtkdState {
    Idle,
    WaitPairingResponse,
    KeyDistribution,
    Complete,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassicCtkdOutcome {
    pub ltk: [u8; 16],
    pub link_key: [u8; 16],
    pub method: PairingMethod,
    pub authenticated: bool,
    pub bonding: bool,
    pub ct2: bool,
    pub maximum_encryption_key_size: u8,
    pub initiator_key_distribution: KeyDistribution,
    pub responder_key_distribution: KeyDistribution,
}

pub struct ClassicCtkdSession {
    role: PairingRole,
    config: PairingConfig,
    initiator_address: Address,
    responder_address: Address,
    link_key: [u8; 16],
    authenticated: bool,
    state: ClassicCtkdState,
    outbound: VecDeque<SmpPdu>,
    request_features: Option<PairingFeatures>,
    bonding: bool,
    ct2: bool,
    maximum_encryption_key_size: u8,
    initiator_key_distribution: KeyDistribution,
    responder_key_distribution: KeyDistribution,
    local_keys: LocalKeyMaterial,
    distribution: Option<KeyDistributionSession>,
    outcome: Option<ClassicCtkdOutcome>,
    failure: Option<PairingFailureReason>,
}

impl ClassicCtkdSession {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        role: PairingRole,
        config: PairingConfig,
        initiator_address: Address,
        responder_address: Address,
        link_key: [u8; 16],
        authenticated: bool,
        encrypted: bool,
    ) -> Result<Self> {
        config.validate()?;
        if !config.secure_connections {
            return Err(Error::InvalidPacket(
                "CTKD over BR/EDR requires Secure Connections support".into(),
            ));
        }
        if !encrypted {
            return Err(Error::InvalidPacket(
                "CTKD over BR/EDR requires an encrypted Classic ACL".into(),
            ));
        }
        let identity_address = match role {
            PairingRole::Initiator => initiator_address.clone(),
            PairingRole::Responder => responder_address.clone(),
        };
        Ok(Self {
            role,
            bonding: config.bonding,
            ct2: config.ct2,
            maximum_encryption_key_size: config.capabilities.maximum_encryption_key_size,
            initiator_key_distribution: config.capabilities.local_initiator_key_distribution,
            responder_key_distribution: config.capabilities.local_responder_key_distribution,
            local_keys: LocalKeyMaterial::generate(identity_address),
            config,
            initiator_address,
            responder_address,
            link_key,
            authenticated,
            state: ClassicCtkdState::Idle,
            outbound: VecDeque::new(),
            request_features: None,
            distribution: None,
            outcome: None,
            failure: None,
        })
    }

    pub fn start(&mut self) -> Result<()> {
        if self.role != PairingRole::Initiator || self.state != ClassicCtkdState::Idle {
            return Err(Error::InvalidPacket(
                "only an idle Classic initiator can start CTKD".into(),
            ));
        }
        let features = self.local_features();
        self.request_features = Some(features);
        self.outbound.push_back(SmpPdu::PairingRequest(features));
        self.state = ClassicCtkdState::WaitPairingResponse;
        Ok(())
    }

    pub fn process(&mut self, pdu: SmpPdu) -> Result<()> {
        if self.state == ClassicCtkdState::Failed {
            return Ok(());
        }
        if let SmpPdu::PairingFailed { reason } = pdu {
            self.failure = crate::session::pairing_failure_from_u8(reason);
            self.state = ClassicCtkdState::Failed;
            return Ok(());
        }
        if self.state == ClassicCtkdState::KeyDistribution {
            self.distribution
                .as_mut()
                .expect("distribution exists")
                .process(pdu);
            self.sync_distribution();
            return Ok(());
        }
        match (self.role, self.state, pdu) {
            (PairingRole::Responder, ClassicCtkdState::Idle, SmpPdu::PairingRequest(features)) => {
                self.on_pairing_request(features)
            }
            (
                PairingRole::Initiator,
                ClassicCtkdState::WaitPairingResponse,
                SmpPdu::PairingResponse(features),
            ) => self.on_pairing_response(features),
            _ => {
                self.fail(PairingFailureReason::InvalidParameters);
                Ok(())
            }
        }
    }

    pub fn drain_outbound(&mut self) -> Vec<SmpPdu> {
        self.outbound.drain(..).collect()
    }

    pub fn state(&self) -> ClassicCtkdState {
        self.state
    }

    pub fn outcome(&self) -> Option<&ClassicCtkdOutcome> {
        self.outcome.as_ref()
    }

    pub fn failure(&self) -> Option<PairingFailureReason> {
        self.failure
    }

    pub fn set_local_key_material(&mut self, keys: LocalKeyMaterial) -> Result<()> {
        if self.state != ClassicCtkdState::Idle {
            return Err(Error::InvalidPacket(
                "local keys can only be changed before CTKD".into(),
            ));
        }
        self.local_keys = keys;
        Ok(())
    }

    pub fn pairing_keys(&self) -> Option<PairingKeys> {
        let mut keys = self.distribution.as_ref()?.pairing_keys()?;
        keys.link_key = Some(Key {
            value: self.link_key.to_vec(),
            authenticated: self.authenticated,
            ediv: None,
            rand: None,
            sign_counter: None,
        });
        Some(keys)
    }

    pub fn store_bond(&self, store: &mut dyn KeyStore) -> KeyStoreResult<bool> {
        if !self.bonding {
            return Ok(false);
        }
        let Some(keys) = self.pairing_keys() else {
            return Ok(false);
        };
        let peer = self
            .distribution
            .as_ref()
            .expect("pairing keys require distribution")
            .peer_address();
        store.update(&peer.to_string(false), keys)?;
        Ok(true)
    }

    fn on_pairing_request(&mut self, features: PairingFeatures) -> Result<()> {
        let response = self.negotiate(features)?;
        self.outbound.push_back(SmpPdu::PairingResponse(response));
        self.finish_features();
        Ok(())
    }

    fn on_pairing_response(&mut self, features: PairingFeatures) -> Result<()> {
        let request = self.request_features.expect("initiator saved request");
        if features.initiator_key_distribution & !request.initiator_key_distribution != 0
            || features.responder_key_distribution & !request.responder_key_distribution != 0
        {
            self.fail(PairingFailureReason::InvalidParameters);
            return Ok(());
        }
        self.apply_peer(features)?;
        self.finish_features();
        Ok(())
    }

    fn negotiate(&mut self, peer: PairingFeatures) -> Result<PairingFeatures> {
        self.apply_peer(peer)?;
        let local = self.local_features();
        Ok(PairingFeatures {
            maximum_encryption_key_size: self.maximum_encryption_key_size,
            initiator_key_distribution: self.initiator_key_distribution.0,
            responder_key_distribution: self.responder_key_distribution.0,
            ..local
        })
    }

    fn apply_peer(&mut self, peer: PairingFeatures) -> Result<()> {
        if IoCapability::try_from(peer.io_capability).is_err()
            || peer.oob_data_flag > 1
            || peer.maximum_encryption_key_size > 16
            || peer.initiator_key_distribution & !KeyDistribution::ALL.0 != 0
            || peer.responder_key_distribution & !KeyDistribution::ALL.0 != 0
        {
            self.fail(PairingFailureReason::InvalidParameters);
            return Err(Error::InvalidPacket(
                "invalid Classic pairing features".into(),
            ));
        }
        if !AuthReq(peer.auth_req).contains(AuthReq::SECURE_CONNECTIONS) {
            self.fail(PairingFailureReason::CrossTransportKeyDerivationNotAllowed);
            return Err(Error::InvalidPacket("peer does not permit CTKD".into()));
        }
        self.maximum_encryption_key_size = self
            .maximum_encryption_key_size
            .min(peer.maximum_encryption_key_size);
        if self.maximum_encryption_key_size < 7 {
            self.fail(PairingFailureReason::EncryptionKeySize);
            return Err(Error::InvalidPacket("encryption key size too small".into()));
        }
        self.bonding &= AuthReq(peer.auth_req).contains(AuthReq::BONDING);
        self.ct2 &= AuthReq(peer.auth_req).contains(AuthReq::CT2);
        self.initiator_key_distribution = self
            .initiator_key_distribution
            .intersection(KeyDistribution(peer.initiator_key_distribution));
        self.responder_key_distribution = self
            .responder_key_distribution
            .intersection(KeyDistribution(peer.responder_key_distribution));
        Ok(())
    }

    fn finish_features(&mut self) {
        let mut ltk = derive_ltk(&self.link_key, self.ct2);
        ltk[usize::from(self.maximum_encryption_key_size)..].fill(0);
        self.outcome = Some(ClassicCtkdOutcome {
            ltk,
            link_key: self.link_key,
            method: PairingMethod::CtkdOverClassic,
            authenticated: self.authenticated,
            bonding: self.bonding,
            ct2: self.ct2,
            maximum_encryption_key_size: self.maximum_encryption_key_size,
            initiator_key_distribution: self.initiator_key_distribution,
            responder_key_distribution: self.responder_key_distribution,
        });
        let peer_address = match self.role {
            PairingRole::Initiator => self.responder_address.clone(),
            PairingRole::Responder => self.initiator_address.clone(),
        };
        let mut distribution = KeyDistributionSession::new(KeyDistributionConfig {
            role: self.role,
            secure_connections: true,
            ct2: self.ct2,
            authenticated: self.authenticated,
            maximum_encryption_key_size: self.maximum_encryption_key_size,
            pairing_ltk: ltk,
            initiator_keys: self.initiator_key_distribution,
            responder_keys: self.responder_key_distribution,
            local_keys: self.local_keys.clone(),
            peer_address,
        });
        distribution.mark_encrypted();
        self.distribution = Some(distribution);
        self.sync_distribution();
    }

    fn sync_distribution(&mut self) {
        let distribution = self
            .distribution
            .as_mut()
            .expect("distribution session exists");
        self.outbound.extend(distribution.drain_outbound());
        match distribution.state() {
            KeyDistributionState::Complete => self.state = ClassicCtkdState::Complete,
            KeyDistributionState::Failed => {
                self.failure = distribution.failure();
                self.state = ClassicCtkdState::Failed;
            }
            KeyDistributionState::WaitEncryption | KeyDistributionState::Distributing => {
                self.state = ClassicCtkdState::KeyDistribution;
            }
        }
    }

    fn local_features(&self) -> PairingFeatures {
        PairingFeatures {
            io_capability: self.config.capabilities.io_capability as u8,
            oob_data_flag: 0,
            auth_req: AuthReq::from_booleans(
                self.config.bonding,
                true,
                self.authenticated,
                false,
                self.config.ct2,
            )
            .0,
            maximum_encryption_key_size: self.config.capabilities.maximum_encryption_key_size,
            initiator_key_distribution: self.config.capabilities.local_initiator_key_distribution.0,
            responder_key_distribution: self.config.capabilities.local_responder_key_distribution.0,
        }
    }

    fn fail(&mut self, reason: PairingFailureReason) {
        if self.state != ClassicCtkdState::Failed {
            self.outbound.push_back(SmpPdu::PairingFailed {
                reason: reason as u8,
            });
        }
        self.failure = Some(reason);
        self.state = ClassicCtkdState::Failed;
    }
}
