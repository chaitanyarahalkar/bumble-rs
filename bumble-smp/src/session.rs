//! Sans-I/O LE Legacy pairing state machine.

use std::collections::VecDeque;

use bumble::Address;

use crate::{
    legacy_confirm, legacy_stk, select_pairing_method_with_oob, AuthReq, Error, IoCapability,
    KeyDistribution, PairingConfig, PairingDelegate, PairingFeatures, PairingMethod,
    PairingMethodSelection, Result, SmpPdu,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairingRole {
    Initiator,
    Responder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairingState {
    Idle,
    WaitPairingResponse,
    WaitPairingConfirm,
    WaitPairingRandom,
    WaitEncryption,
    Complete,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PairingFailureReason {
    PasskeyEntryFailed = 0x01,
    OobNotAvailable = 0x02,
    AuthenticationRequirements = 0x03,
    ConfirmValueFailed = 0x04,
    PairingNotSupported = 0x05,
    EncryptionKeySize = 0x06,
    CommandNotSupported = 0x07,
    UnspecifiedReason = 0x08,
    RepeatedAttempts = 0x09,
    InvalidParameters = 0x0A,
    DhKeyCheckFailed = 0x0B,
    NumericComparisonFailed = 0x0C,
    CrossTransportKeyDerivationNotAllowed = 0x0E,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyPairingOutcome {
    pub stk: [u8; 16],
    pub method: PairingMethod,
    pub authenticated: bool,
    pub bonding: bool,
    pub maximum_encryption_key_size: u8,
    pub initiator_key_distribution: KeyDistribution,
    pub responder_key_distribution: KeyDistribution,
}

/// Drives the complete feature/confirm/random phase of LE Legacy pairing.
/// Encryption is an external controller action; call [`Self::mark_encrypted`]
/// after enabling it with the derived STK.
pub struct LegacyPairingSession {
    role: PairingRole,
    config: PairingConfig,
    delegate: Box<dyn PairingDelegate>,
    initiator_address: Address,
    responder_address: Address,
    state: PairingState,
    outbound: VecDeque<SmpPdu>,
    preq: Option<Vec<u8>>,
    pres: Option<Vec<u8>>,
    peer_features: Option<PairingFeatures>,
    selection: Option<PairingMethodSelection>,
    tk: [u8; 16],
    local_random: [u8; 16],
    peer_confirm: Option<[u8; 16]>,
    peer_random: Option<[u8; 16]>,
    outcome: Option<LegacyPairingOutcome>,
    failure: Option<PairingFailureReason>,
    bonding: bool,
    maximum_encryption_key_size: u8,
    initiator_key_distribution: KeyDistribution,
    responder_key_distribution: KeyDistribution,
}

impl LegacyPairingSession {
    pub fn new(
        role: PairingRole,
        config: PairingConfig,
        delegate: Box<dyn PairingDelegate>,
        initiator_address: Address,
        responder_address: Address,
        local_random: [u8; 16],
    ) -> Result<Self> {
        config.capabilities.validate()?;
        Ok(Self {
            role,
            bonding: config.bonding,
            maximum_encryption_key_size: config.capabilities.maximum_encryption_key_size,
            initiator_key_distribution: config.capabilities.local_initiator_key_distribution,
            responder_key_distribution: config.capabilities.local_responder_key_distribution,
            config,
            delegate,
            initiator_address,
            responder_address,
            state: PairingState::Idle,
            outbound: VecDeque::new(),
            preq: None,
            pres: None,
            peer_features: None,
            selection: None,
            tk: [0; 16],
            local_random,
            peer_confirm: None,
            peer_random: None,
            outcome: None,
            failure: None,
        })
    }

    pub fn start(&mut self) -> Result<()> {
        if self.role != PairingRole::Initiator || self.state != PairingState::Idle {
            return Err(Error::InvalidPacket(
                "only an idle initiator can start pairing".into(),
            ));
        }
        let request = SmpPdu::PairingRequest(self.local_features());
        self.preq = Some(request.to_bytes());
        self.outbound.push_back(request);
        self.state = PairingState::WaitPairingResponse;
        Ok(())
    }

    pub fn process(&mut self, pdu: SmpPdu) -> Result<()> {
        if let SmpPdu::PairingFailed { reason } = pdu {
            self.failure = pairing_failure_from_u8(reason);
            self.state = PairingState::Failed;
            return Ok(());
        }
        match (self.role, self.state, pdu) {
            (PairingRole::Responder, PairingState::Idle, SmpPdu::PairingRequest(features)) => {
                self.on_pairing_request(features)
            }
            (
                PairingRole::Initiator,
                PairingState::WaitPairingResponse,
                SmpPdu::PairingResponse(features),
            ) => self.on_pairing_response(features),
            (_, PairingState::WaitPairingConfirm, SmpPdu::PairingConfirm { confirm_value }) => {
                self.on_pairing_confirm(confirm_value)
            }
            (_, PairingState::WaitPairingRandom, SmpPdu::PairingRandom { random_value }) => {
                self.on_pairing_random(random_value)
            }
            _ => {
                self.fail(PairingFailureReason::InvalidParameters);
                Ok(())
            }
        }
    }

    pub fn poll_outbound(&mut self) -> Option<SmpPdu> {
        self.outbound.pop_front()
    }

    pub fn drain_outbound(&mut self) -> Vec<SmpPdu> {
        self.outbound.drain(..).collect()
    }

    pub fn state(&self) -> PairingState {
        self.state
    }

    pub fn method(&self) -> Option<PairingMethod> {
        self.selection.map(|selection| selection.method)
    }

    pub fn stk(&self) -> Option<[u8; 16]> {
        self.outcome.as_ref().map(|outcome| outcome.stk)
    }

    pub fn outcome(&self) -> Option<&LegacyPairingOutcome> {
        self.outcome.as_ref()
    }

    pub fn failure(&self) -> Option<PairingFailureReason> {
        self.failure
    }

    pub fn mark_encrypted(&mut self) -> Result<()> {
        if self.state != PairingState::WaitEncryption || self.outcome.is_none() {
            return Err(Error::InvalidPacket(
                "pairing is not waiting for encryption".into(),
            ));
        }
        self.state = PairingState::Complete;
        Ok(())
    }

    fn on_pairing_request(&mut self, features: PairingFeatures) -> Result<()> {
        if !self.delegate.accept() {
            self.fail(PairingFailureReason::PairingNotSupported);
            return Ok(());
        }
        self.preq = Some(SmpPdu::PairingRequest(features).to_bytes());
        let response_features = match self.negotiated_response_features(features) {
            Ok(features) => features,
            Err(_) => return Ok(()),
        };
        self.peer_features = Some(features);
        let response = SmpPdu::PairingResponse(response_features);
        self.pres = Some(response.to_bytes());
        if !self.prepare_method(features, response_features)? {
            return Ok(());
        }
        self.outbound.push_back(response);
        self.state = PairingState::WaitPairingConfirm;
        Ok(())
    }

    fn on_pairing_response(&mut self, features: PairingFeatures) -> Result<()> {
        let request_features = match self.preq.as_deref() {
            Some(bytes) => match SmpPdu::from_bytes(bytes)? {
                SmpPdu::PairingRequest(features) => features,
                _ => unreachable!("preq is always a Pairing Request"),
            },
            None => {
                self.fail(PairingFailureReason::InvalidParameters);
                return Ok(());
            }
        };
        if !valid_features(features) {
            self.fail(PairingFailureReason::InvalidParameters);
            return Ok(());
        }
        self.maximum_encryption_key_size = self
            .maximum_encryption_key_size
            .min(features.maximum_encryption_key_size);
        if self.maximum_encryption_key_size < 7 {
            self.fail(PairingFailureReason::EncryptionKeySize);
            return Ok(());
        }
        self.bonding &= AuthReq(features.auth_req).contains(AuthReq::BONDING);
        self.initiator_key_distribution = self
            .initiator_key_distribution
            .intersection(KeyDistribution(features.initiator_key_distribution));
        self.responder_key_distribution = self
            .responder_key_distribution
            .intersection(KeyDistribution(features.responder_key_distribution));
        self.peer_features = Some(features);
        self.pres = Some(SmpPdu::PairingResponse(features).to_bytes());
        if !self.prepare_method(request_features, features)? {
            return Ok(());
        }
        let confirm = self.local_confirm()?;
        self.outbound.push_back(SmpPdu::PairingConfirm {
            confirm_value: confirm,
        });
        self.state = PairingState::WaitPairingConfirm;
        Ok(())
    }

    fn on_pairing_confirm(&mut self, confirm_value: [u8; 16]) -> Result<()> {
        self.peer_confirm = Some(confirm_value);
        match self.role {
            PairingRole::Initiator => {
                self.outbound.push_back(SmpPdu::PairingRandom {
                    random_value: self.local_random,
                });
            }
            PairingRole::Responder => {
                let confirm = self.local_confirm()?;
                self.outbound.push_back(SmpPdu::PairingConfirm {
                    confirm_value: confirm,
                });
            }
        }
        self.state = PairingState::WaitPairingRandom;
        Ok(())
    }

    fn on_pairing_random(&mut self, random_value: [u8; 16]) -> Result<()> {
        let expected = self.confirm_for(random_value)?;
        if self.peer_confirm != Some(expected) {
            self.fail(PairingFailureReason::ConfirmValueFailed);
            return Ok(());
        }
        self.peer_random = Some(random_value);
        if self.role == PairingRole::Responder {
            self.outbound.push_back(SmpPdu::PairingRandom {
                random_value: self.local_random,
            });
        }
        let (srand, mrand) = match self.role {
            PairingRole::Initiator => (random_value, self.local_random),
            PairingRole::Responder => (self.local_random, random_value),
        };
        let mut stk = legacy_stk(&self.tk, &srand, &mrand);
        stk[usize::from(self.maximum_encryption_key_size)..].fill(0);
        let method = self.selection.expect("method selected").method;
        self.outcome = Some(LegacyPairingOutcome {
            stk,
            method,
            authenticated: method != PairingMethod::JustWorks,
            bonding: self.bonding,
            maximum_encryption_key_size: self.maximum_encryption_key_size,
            initiator_key_distribution: self.initiator_key_distribution,
            responder_key_distribution: self.responder_key_distribution,
        });
        self.state = PairingState::WaitEncryption;
        Ok(())
    }

    fn negotiated_response_features(&mut self, peer: PairingFeatures) -> Result<PairingFeatures> {
        if !valid_features(peer) {
            self.fail(PairingFailureReason::InvalidParameters);
            return Err(Error::InvalidPacket(
                "invalid pairing request features".into(),
            ));
        }
        let local = self.local_features();
        self.maximum_encryption_key_size = local
            .maximum_encryption_key_size
            .min(peer.maximum_encryption_key_size);
        if self.maximum_encryption_key_size < 7 {
            self.fail(PairingFailureReason::EncryptionKeySize);
            return Err(Error::InvalidPacket("encryption key size too small".into()));
        }
        self.bonding &= AuthReq(peer.auth_req).contains(AuthReq::BONDING);
        self.initiator_key_distribution = KeyDistribution(peer.initiator_key_distribution)
            .intersection(self.initiator_key_distribution);
        self.responder_key_distribution = KeyDistribution(peer.responder_key_distribution)
            .intersection(self.responder_key_distribution);
        Ok(PairingFeatures {
            io_capability: local.io_capability,
            oob_data_flag: local.oob_data_flag,
            auth_req: local.auth_req,
            maximum_encryption_key_size: self.maximum_encryption_key_size,
            initiator_key_distribution: self.initiator_key_distribution.0,
            responder_key_distribution: self.responder_key_distribution.0,
        })
    }

    fn prepare_method(
        &mut self,
        request: PairingFeatures,
        response: PairingFeatures,
    ) -> Result<bool> {
        let selection = select_pairing_method_with_oob(
            false,
            self.local_has_oob(),
            self.peer_features
                .map_or(request.oob_data_flag != 0, |features| {
                    features.oob_data_flag != 0
                }),
            self.config.mitm,
            AuthReq(if self.role == PairingRole::Initiator {
                response.auth_req
            } else {
                request.auth_req
            }),
            IoCapability::try_from(request.io_capability)?,
            IoCapability::try_from(response.io_capability)?,
        );
        self.selection = Some(selection);
        match selection.method {
            PairingMethod::JustWorks => {
                if !self.delegate.confirm(true) {
                    self.fail(PairingFailureReason::ConfirmValueFailed);
                    return Ok(false);
                }
                self.tk = [0; 16];
            }
            PairingMethod::Passkey => {
                let displays = match self.role {
                    PairingRole::Initiator => selection.initiator_displays,
                    PairingRole::Responder => selection.responder_displays,
                };
                let passkey = if displays {
                    let passkey = self.delegate.generate_passkey();
                    self.delegate.display_number(passkey, 6);
                    Some(passkey)
                } else {
                    self.delegate.get_number()
                };
                let Some(passkey) = passkey.filter(|passkey| *passkey < 1_000_000) else {
                    self.fail(PairingFailureReason::PasskeyEntryFailed);
                    return Ok(false);
                };
                self.tk[..4].copy_from_slice(&passkey.to_le_bytes());
            }
            PairingMethod::Oob => {
                let Some(tk) = self
                    .config
                    .oob
                    .as_ref()
                    .and_then(|oob| oob.legacy_context.as_ref())
                    .and_then(|legacy| legacy.tk.as_slice().try_into().ok())
                else {
                    self.fail(PairingFailureReason::OobNotAvailable);
                    return Ok(false);
                };
                self.tk = tk;
            }
            _ => {
                self.fail(PairingFailureReason::AuthenticationRequirements);
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn local_features(&self) -> PairingFeatures {
        PairingFeatures {
            io_capability: self.config.capabilities.io_capability as u8,
            oob_data_flag: u8::from(self.local_has_oob()),
            auth_req: AuthReq::from_booleans(
                self.config.bonding,
                false,
                self.config.mitm,
                false,
                false,
            )
            .0,
            maximum_encryption_key_size: self.config.capabilities.maximum_encryption_key_size,
            initiator_key_distribution: self.config.capabilities.local_initiator_key_distribution.0,
            responder_key_distribution: self.config.capabilities.local_responder_key_distribution.0,
        }
    }

    fn local_has_oob(&self) -> bool {
        self.config
            .oob
            .as_ref()
            .and_then(|oob| oob.legacy_context.as_ref())
            .is_some()
    }

    fn local_confirm(&self) -> Result<[u8; 16]> {
        self.confirm_for(self.local_random)
    }

    fn confirm_for(&self, random: [u8; 16]) -> Result<[u8; 16]> {
        let preq = self
            .preq
            .as_deref()
            .ok_or_else(|| Error::InvalidPacket("missing Pairing Request".into()))?;
        let pres = self
            .pres
            .as_deref()
            .ok_or_else(|| Error::InvalidPacket("missing Pairing Response".into()))?;
        Ok(legacy_confirm(
            &self.tk,
            &random,
            preq,
            pres,
            &self.initiator_address,
            u8::from(self.initiator_address.is_random()),
            &self.responder_address,
            u8::from(self.responder_address.is_random()),
        ))
    }

    fn fail(&mut self, reason: PairingFailureReason) {
        if self.state != PairingState::Failed {
            self.outbound.push_back(SmpPdu::PairingFailed {
                reason: reason as u8,
            });
        }
        self.failure = Some(reason);
        self.state = PairingState::Failed;
    }
}

fn valid_features(features: PairingFeatures) -> bool {
    IoCapability::try_from(features.io_capability).is_ok()
        && features.oob_data_flag <= 1
        && features.maximum_encryption_key_size <= 16
        && features.initiator_key_distribution & !KeyDistribution::ALL.0 == 0
        && features.responder_key_distribution & !KeyDistribution::ALL.0 == 0
}

fn pairing_failure_from_u8(reason: u8) -> Option<PairingFailureReason> {
    Some(match reason {
        0x01 => PairingFailureReason::PasskeyEntryFailed,
        0x02 => PairingFailureReason::OobNotAvailable,
        0x03 => PairingFailureReason::AuthenticationRequirements,
        0x04 => PairingFailureReason::ConfirmValueFailed,
        0x05 => PairingFailureReason::PairingNotSupported,
        0x06 => PairingFailureReason::EncryptionKeySize,
        0x07 => PairingFailureReason::CommandNotSupported,
        0x08 => PairingFailureReason::UnspecifiedReason,
        0x09 => PairingFailureReason::RepeatedAttempts,
        0x0A => PairingFailureReason::InvalidParameters,
        0x0B => PairingFailureReason::DhKeyCheckFailed,
        0x0C => PairingFailureReason::NumericComparisonFailed,
        0x0E => PairingFailureReason::CrossTransportKeyDerivationNotAllowed,
        _ => return None,
    })
}
