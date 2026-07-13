//! Sans-I/O LE Secure Connections JustWorks/Numeric Comparison session.

use std::collections::VecDeque;

use bumble::Address;
use bumble_crypto::EccKey;

use crate::{
    sc, select_pairing_method, AuthReq, Error, IoCapability, KeyDistribution, PairingConfig,
    PairingDelegate, PairingFailureReason, PairingFeatures, PairingMethod, PairingRole, Result,
    SmpPdu,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScPairingState {
    Idle,
    WaitPairingResponse,
    WaitPublicKey,
    WaitPairingConfirm,
    WaitPairingRandom,
    WaitDhKeyCheck,
    WaitEncryption,
    Complete,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScPairingOutcome {
    pub mac_key: [u8; 16],
    pub ltk: [u8; 16],
    pub numeric_check: u32,
    pub method: PairingMethod,
    pub authenticated: bool,
    pub bonding: bool,
    pub maximum_encryption_key_size: u8,
    pub initiator_key_distribution: KeyDistribution,
    pub responder_key_distribution: KeyDistribution,
}

pub struct ScPairingSession {
    role: PairingRole,
    config: PairingConfig,
    delegate: Box<dyn PairingDelegate>,
    initiator_address: Address,
    responder_address: Address,
    ecc_key: EccKey,
    local_nonce: [u8; 16],
    state: ScPairingState,
    outbound: VecDeque<SmpPdu>,
    preq: Option<Vec<u8>>,
    pres: Option<Vec<u8>>,
    method: Option<PairingMethod>,
    peer_public_x: Option<[u8; 32]>,
    peer_public_y: Option<[u8; 32]>,
    dh_key: Option<[u8; 32]>,
    peer_confirm: Option<[u8; 16]>,
    peer_nonce: Option<[u8; 16]>,
    keys: Option<sc::ScKeys>,
    user_confirmed: bool,
    outcome: Option<ScPairingOutcome>,
    failure: Option<PairingFailureReason>,
    bonding: bool,
    maximum_encryption_key_size: u8,
    initiator_key_distribution: KeyDistribution,
    responder_key_distribution: KeyDistribution,
}

impl ScPairingSession {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        role: PairingRole,
        config: PairingConfig,
        delegate: Box<dyn PairingDelegate>,
        initiator_address: Address,
        responder_address: Address,
        ecc_key: EccKey,
        local_nonce: [u8; 16],
    ) -> Result<Self> {
        config.capabilities.validate()?;
        if !config.secure_connections {
            return Err(Error::InvalidPacket(
                "SC session requires Secure Connections policy".into(),
            ));
        }
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
            ecc_key,
            local_nonce,
            state: ScPairingState::Idle,
            outbound: VecDeque::new(),
            preq: None,
            pres: None,
            method: None,
            peer_public_x: None,
            peer_public_y: None,
            dh_key: None,
            peer_confirm: None,
            peer_nonce: None,
            keys: None,
            user_confirmed: false,
            outcome: None,
            failure: None,
        })
    }

    pub fn start(&mut self) -> Result<()> {
        if self.role != PairingRole::Initiator || self.state != ScPairingState::Idle {
            return Err(Error::InvalidPacket(
                "only an idle initiator can start SC pairing".into(),
            ));
        }
        let request = SmpPdu::PairingRequest(self.local_features());
        self.preq = Some(request.to_bytes());
        self.outbound.push_back(request);
        self.state = ScPairingState::WaitPairingResponse;
        Ok(())
    }

    pub fn process(&mut self, pdu: SmpPdu) -> Result<()> {
        if let SmpPdu::PairingFailed { reason } = pdu {
            self.failure = failure_from_u8(reason);
            self.state = ScPairingState::Failed;
            return Ok(());
        }
        match (self.role, self.state, pdu) {
            (PairingRole::Responder, ScPairingState::Idle, SmpPdu::PairingRequest(features)) => {
                self.on_pairing_request(features)
            }
            (
                PairingRole::Initiator,
                ScPairingState::WaitPairingResponse,
                SmpPdu::PairingResponse(features),
            ) => self.on_pairing_response(features),
            (
                _,
                ScPairingState::WaitPublicKey,
                SmpPdu::PairingPublicKey {
                    public_key_x,
                    public_key_y,
                },
            ) => self.on_public_key(public_key_x, public_key_y),
            (
                PairingRole::Initiator,
                ScPairingState::WaitPairingConfirm,
                SmpPdu::PairingConfirm { confirm_value },
            ) => self.on_pairing_confirm(confirm_value),
            (_, ScPairingState::WaitPairingRandom, SmpPdu::PairingRandom { random_value }) => {
                self.on_pairing_random(random_value)
            }
            (_, ScPairingState::WaitDhKeyCheck, SmpPdu::PairingDhKeyCheck { dhkey_check }) => {
                self.on_dhkey_check(dhkey_check)
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

    pub fn state(&self) -> ScPairingState {
        self.state
    }

    pub fn method(&self) -> Option<PairingMethod> {
        self.method
    }

    pub fn ltk(&self) -> Option<[u8; 16]> {
        self.outcome.as_ref().map(|outcome| outcome.ltk)
    }

    pub fn outcome(&self) -> Option<&ScPairingOutcome> {
        self.outcome.as_ref()
    }

    pub fn failure(&self) -> Option<PairingFailureReason> {
        self.failure
    }

    pub fn mark_encrypted(&mut self) -> Result<()> {
        if self.state != ScPairingState::WaitEncryption || self.outcome.is_none() {
            return Err(Error::InvalidPacket(
                "SC pairing is not waiting for encryption".into(),
            ));
        }
        self.state = ScPairingState::Complete;
        Ok(())
    }

    fn on_pairing_request(&mut self, features: PairingFeatures) -> Result<()> {
        if !self.delegate.accept() {
            self.fail(PairingFailureReason::PairingNotSupported);
            return Ok(());
        }
        self.preq = Some(SmpPdu::PairingRequest(features).to_bytes());
        let response = match self.negotiate_response(features) {
            Ok(response) => response,
            Err(_) => return Ok(()),
        };
        let pdu = SmpPdu::PairingResponse(response);
        self.pres = Some(pdu.to_bytes());
        if !self.select_method(features, response)? {
            return Ok(());
        }
        self.outbound.push_back(pdu);
        self.state = ScPairingState::WaitPublicKey;
        Ok(())
    }

    fn on_pairing_response(&mut self, features: PairingFeatures) -> Result<()> {
        if !valid_sc_features(features) {
            self.fail(PairingFailureReason::InvalidParameters);
            return Ok(());
        }
        if !AuthReq(features.auth_req).contains(AuthReq::SECURE_CONNECTIONS) {
            self.fail(PairingFailureReason::AuthenticationRequirements);
            return Ok(());
        }
        let request = self.request_features()?;
        self.apply_negotiated(features)?;
        self.pres = Some(SmpPdu::PairingResponse(features).to_bytes());
        if !self.select_method(request, features)? {
            return Ok(());
        }
        self.outbound.push_back(self.public_key_pdu());
        self.state = ScPairingState::WaitPublicKey;
        Ok(())
    }

    fn on_public_key(&mut self, x_le: [u8; 32], y_le: [u8; 32]) -> Result<()> {
        if x_le == self.public_x_le() && y_le == self.public_y_le() {
            self.fail(PairingFailureReason::InvalidParameters);
            return Ok(());
        }
        let mut x_be = x_le;
        let mut y_be = y_le;
        x_be.reverse();
        y_be.reverse();
        let mut dh_key = match self.ecc_key.dh(&x_be, &y_be) {
            Ok(key) => key,
            Err(_) => {
                self.fail(PairingFailureReason::InvalidParameters);
                return Ok(());
            }
        };
        dh_key.reverse();
        self.peer_public_x = Some(x_le);
        self.peer_public_y = Some(y_le);
        self.dh_key = Some(dh_key);

        match self.role {
            PairingRole::Initiator => {
                self.state = ScPairingState::WaitPairingConfirm;
            }
            PairingRole::Responder => {
                self.outbound.push_back(self.public_key_pdu());
                let confirm = sc::confirm_value(&self.public_x_le(), &x_le, &self.local_nonce);
                self.outbound.push_back(SmpPdu::PairingConfirm {
                    confirm_value: confirm,
                });
                self.state = ScPairingState::WaitPairingRandom;
            }
        }
        Ok(())
    }

    fn on_pairing_confirm(&mut self, confirm_value: [u8; 16]) -> Result<()> {
        self.peer_confirm = Some(confirm_value);
        self.outbound.push_back(SmpPdu::PairingRandom {
            random_value: self.local_nonce,
        });
        self.state = ScPairingState::WaitPairingRandom;
        Ok(())
    }

    fn on_pairing_random(&mut self, random_value: [u8; 16]) -> Result<()> {
        self.peer_nonce = Some(random_value);
        match self.role {
            PairingRole::Initiator => {
                let peer_x = self.peer_public_x.expect("peer key received");
                let expected = sc::confirm_value(&peer_x, &self.public_x_le(), &random_value);
                if self.peer_confirm != Some(expected) {
                    self.fail(PairingFailureReason::ConfirmValueFailed);
                    return Ok(());
                }
                self.derive_keys()?;
                if !self.confirm_user() {
                    return Ok(());
                }
                let ea = self.keys.as_ref().expect("keys derived").ea;
                self.outbound
                    .push_back(SmpPdu::PairingDhKeyCheck { dhkey_check: ea });
                self.state = ScPairingState::WaitDhKeyCheck;
            }
            PairingRole::Responder => {
                self.outbound.push_back(SmpPdu::PairingRandom {
                    random_value: self.local_nonce,
                });
                self.derive_keys()?;
                if !self.confirm_user() {
                    return Ok(());
                }
                self.state = ScPairingState::WaitDhKeyCheck;
            }
        }
        Ok(())
    }

    fn on_dhkey_check(&mut self, received: [u8; 16]) -> Result<()> {
        let keys = self.keys.as_ref().expect("keys derived");
        let expected = match self.role {
            PairingRole::Initiator => keys.eb,
            PairingRole::Responder => keys.ea,
        };
        if received != expected {
            self.fail(PairingFailureReason::DhKeyCheckFailed);
            return Ok(());
        }
        if self.role == PairingRole::Responder {
            self.outbound.push_back(SmpPdu::PairingDhKeyCheck {
                dhkey_check: keys.eb,
            });
        }
        self.finish();
        Ok(())
    }

    fn derive_keys(&mut self) -> Result<()> {
        let (na, nb, pka, pkb) = match self.role {
            PairingRole::Initiator => (
                self.local_nonce,
                self.peer_nonce.expect("peer nonce received"),
                self.public_x_le(),
                self.peer_public_x.expect("peer key received"),
            ),
            PairingRole::Responder => (
                self.peer_nonce.expect("peer nonce received"),
                self.local_nonce,
                self.peer_public_x.expect("peer key received"),
                self.public_x_le(),
            ),
        };
        let preq = self
            .preq
            .as_deref()
            .ok_or_else(|| Error::InvalidPacket("missing Pairing Request".into()))?;
        let pres = self
            .pres
            .as_deref()
            .ok_or_else(|| Error::InvalidPacket("missing Pairing Response".into()))?;
        self.keys = Some(sc::just_works_keys(
            &self.dh_key.expect("DH key derived"),
            &na,
            &nb,
            self.initiator_address.address_bytes(),
            u8::from(self.initiator_address.is_random()),
            self.responder_address.address_bytes(),
            u8::from(self.responder_address.is_random()),
            &sc::io_cap(preq).expect("pairing request shape"),
            &sc::io_cap(pres).expect("pairing response shape"),
            &pka,
            &pkb,
        ));
        Ok(())
    }

    fn confirm_user(&mut self) -> bool {
        let keys = self.keys.as_ref().expect("keys derived");
        let accepted = match self.method.expect("method selected") {
            PairingMethod::JustWorks => self.delegate.confirm(true),
            PairingMethod::NumericComparison => {
                self.delegate.compare_numbers(keys.numeric_check, 6)
            }
            _ => false,
        };
        if !accepted {
            self.fail(PairingFailureReason::ConfirmValueFailed);
            return false;
        }
        self.user_confirmed = true;
        true
    }

    fn finish(&mut self) {
        let keys = self.keys.as_ref().expect("keys derived");
        let mut ltk = keys.ltk;
        ltk[usize::from(self.maximum_encryption_key_size)..].fill(0);
        let method = self.method.expect("method selected");
        self.outcome = Some(ScPairingOutcome {
            mac_key: keys.mac_key,
            ltk,
            numeric_check: keys.numeric_check,
            method,
            authenticated: method != PairingMethod::JustWorks,
            bonding: self.bonding,
            maximum_encryption_key_size: self.maximum_encryption_key_size,
            initiator_key_distribution: self.initiator_key_distribution,
            responder_key_distribution: self.responder_key_distribution,
        });
        self.state = ScPairingState::WaitEncryption;
    }

    fn negotiate_response(&mut self, peer: PairingFeatures) -> Result<PairingFeatures> {
        if !valid_sc_features(peer) {
            self.fail(PairingFailureReason::InvalidParameters);
            return Err(Error::InvalidPacket("invalid SC pairing features".into()));
        }
        if !AuthReq(peer.auth_req).contains(AuthReq::SECURE_CONNECTIONS) {
            self.fail(PairingFailureReason::AuthenticationRequirements);
            return Err(Error::InvalidPacket("peer does not support SC".into()));
        }
        self.apply_negotiated(peer)?;
        let local = self.local_features();
        Ok(PairingFeatures {
            maximum_encryption_key_size: self.maximum_encryption_key_size,
            initiator_key_distribution: self.initiator_key_distribution.0,
            responder_key_distribution: self.responder_key_distribution.0,
            ..local
        })
    }

    fn apply_negotiated(&mut self, peer: PairingFeatures) -> Result<()> {
        self.maximum_encryption_key_size = self
            .maximum_encryption_key_size
            .min(peer.maximum_encryption_key_size);
        if self.maximum_encryption_key_size < 7 {
            self.fail(PairingFailureReason::EncryptionKeySize);
            return Err(Error::InvalidPacket("encryption key size too small".into()));
        }
        self.bonding &= AuthReq(peer.auth_req).contains(AuthReq::BONDING);
        self.initiator_key_distribution = self
            .initiator_key_distribution
            .intersection(KeyDistribution(peer.initiator_key_distribution));
        self.responder_key_distribution = self
            .responder_key_distribution
            .intersection(KeyDistribution(peer.responder_key_distribution));
        Ok(())
    }

    fn select_method(
        &mut self,
        request: PairingFeatures,
        response: PairingFeatures,
    ) -> Result<bool> {
        let selection = select_pairing_method(
            true,
            self.config.mitm,
            AuthReq(if self.role == PairingRole::Initiator {
                response.auth_req
            } else {
                request.auth_req
            }),
            IoCapability::try_from(request.io_capability)?,
            IoCapability::try_from(response.io_capability)?,
        );
        if !matches!(
            selection.method,
            PairingMethod::JustWorks | PairingMethod::NumericComparison
        ) {
            self.fail(PairingFailureReason::AuthenticationRequirements);
            return Ok(false);
        }
        self.method = Some(selection.method);
        Ok(true)
    }

    fn local_features(&self) -> PairingFeatures {
        PairingFeatures {
            io_capability: self.config.capabilities.io_capability as u8,
            oob_data_flag: 0,
            auth_req: AuthReq::from_booleans(
                self.config.bonding,
                true,
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

    fn request_features(&self) -> Result<PairingFeatures> {
        match SmpPdu::from_bytes(
            self.preq
                .as_deref()
                .ok_or_else(|| Error::InvalidPacket("missing Pairing Request".into()))?,
        )? {
            SmpPdu::PairingRequest(features) => Ok(features),
            _ => unreachable!("preq is a Pairing Request"),
        }
    }

    fn public_x_le(&self) -> [u8; 32] {
        let mut x = self.ecc_key.public_x();
        x.reverse();
        x
    }

    fn public_y_le(&self) -> [u8; 32] {
        let mut y = self.ecc_key.public_y();
        y.reverse();
        y
    }

    fn public_key_pdu(&self) -> SmpPdu {
        SmpPdu::PairingPublicKey {
            public_key_x: self.public_x_le(),
            public_key_y: self.public_y_le(),
        }
    }

    fn fail(&mut self, reason: PairingFailureReason) {
        if self.state != ScPairingState::Failed {
            self.outbound.push_back(SmpPdu::PairingFailed {
                reason: reason as u8,
            });
        }
        self.failure = Some(reason);
        self.state = ScPairingState::Failed;
    }
}

fn valid_sc_features(features: PairingFeatures) -> bool {
    IoCapability::try_from(features.io_capability).is_ok()
        && features.oob_data_flag <= 1
        && features.maximum_encryption_key_size <= 16
        && features.initiator_key_distribution & !KeyDistribution::ALL.0 == 0
        && features.responder_key_distribution & !KeyDistribution::ALL.0 == 0
}

fn failure_from_u8(reason: u8) -> Option<PairingFailureReason> {
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
