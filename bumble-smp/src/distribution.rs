//! Encrypted SMP key distribution and bond material assembly.

use std::collections::VecDeque;

use bumble::keys::{Key, KeyStore, KeyStoreResult, PairingKeys};
use bumble::{Address, AddressType};
use bumble_crypto::random_128;

use crate::{derive_link_key, KeyDistribution, PairingFailureReason, PairingRole, SmpPdu};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalKeyMaterial {
    pub ltk: [u8; 16],
    pub ediv: u16,
    pub rand: [u8; 8],
    pub irk: [u8; 16],
    pub identity_address: Address,
    pub csrk: [u8; 16],
}

impl LocalKeyMaterial {
    pub fn generate(identity_address: Address) -> Self {
        let ltk = random_128();
        let irk = random_128();
        let csrk = random_128();
        let metadata = random_128();
        Self {
            ltk,
            ediv: u16::from_le_bytes([metadata[0], metadata[1]]),
            rand: metadata[2..10].try_into().expect("eight-byte slice"),
            irk,
            identity_address,
            csrk,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyDistributionState {
    WaitEncryption,
    Distributing,
    Complete,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DistributionKind {
    EncryptionInformation,
    MasterIdentification,
    IdentityInformation,
    IdentityAddressInformation,
    SigningInformation,
}

#[derive(Clone, Debug)]
pub struct KeyDistributionConfig {
    pub role: PairingRole,
    pub secure_connections: bool,
    pub ct2: bool,
    pub authenticated: bool,
    pub maximum_encryption_key_size: u8,
    pub pairing_ltk: [u8; 16],
    pub initiator_keys: KeyDistribution,
    pub responder_keys: KeyDistribution,
    pub local_keys: LocalKeyMaterial,
    pub peer_address: Address,
}

/// Implements Vol 3, Part H phase 3, including Bumble's responder-first rule.
pub struct KeyDistributionSession {
    config: KeyDistributionConfig,
    state: KeyDistributionState,
    outbound: VecDeque<SmpPdu>,
    expected: Vec<DistributionKind>,
    local_sent: bool,
    peer_ltk: Option<[u8; 16]>,
    peer_ediv: Option<u16>,
    peer_rand: Option<[u8; 8]>,
    peer_irk: Option<[u8; 16]>,
    peer_identity_address: Option<Address>,
    peer_csrk: Option<[u8; 16]>,
    failure: Option<PairingFailureReason>,
}

impl KeyDistributionSession {
    pub fn new(config: KeyDistributionConfig) -> Self {
        let peer_flags = match config.role {
            PairingRole::Initiator => config.responder_keys,
            PairingRole::Responder => config.initiator_keys,
        };
        let expected = expected_distributions(config.secure_connections, peer_flags);
        Self {
            config,
            state: KeyDistributionState::WaitEncryption,
            outbound: VecDeque::new(),
            expected,
            local_sent: false,
            peer_ltk: None,
            peer_ediv: None,
            peer_rand: None,
            peer_irk: None,
            peer_identity_address: None,
            peer_csrk: None,
            failure: None,
        }
    }

    pub fn mark_encrypted(&mut self) {
        if self.state != KeyDistributionState::WaitEncryption {
            return;
        }
        self.state = KeyDistributionState::Distributing;
        if self.config.role == PairingRole::Responder {
            self.distribute_local_keys();
        }
        if self.expected.is_empty() {
            self.peer_distribution_complete();
        }
    }

    pub fn process(&mut self, pdu: SmpPdu) {
        if self.state == KeyDistributionState::Failed {
            return;
        }
        if self.state != KeyDistributionState::Distributing {
            self.fail(PairingFailureReason::UnspecifiedReason);
            return;
        }

        let kind = match &pdu {
            SmpPdu::EncryptionInformation { .. } => DistributionKind::EncryptionInformation,
            SmpPdu::MasterIdentification { .. } => DistributionKind::MasterIdentification,
            SmpPdu::IdentityInformation { .. } => DistributionKind::IdentityInformation,
            SmpPdu::IdentityAddressInformation { .. } => {
                DistributionKind::IdentityAddressInformation
            }
            SmpPdu::SigningInformation { .. } => DistributionKind::SigningInformation,
            SmpPdu::PairingFailed { reason } => {
                self.failure = super::session::pairing_failure_from_u8(*reason);
                self.state = KeyDistributionState::Failed;
                return;
            }
            _ => {
                self.fail(PairingFailureReason::UnspecifiedReason);
                return;
            }
        };
        let Some(index) = self.expected.iter().position(|expected| *expected == kind) else {
            self.fail(PairingFailureReason::UnspecifiedReason);
            return;
        };

        match pdu {
            SmpPdu::EncryptionInformation { long_term_key } => self.peer_ltk = Some(long_term_key),
            SmpPdu::MasterIdentification { ediv, rand } => {
                self.peer_ediv = Some(ediv);
                self.peer_rand = Some(rand);
            }
            SmpPdu::IdentityInformation {
                identity_resolving_key,
            } => self.peer_irk = Some(identity_resolving_key),
            SmpPdu::IdentityAddressInformation { addr_type, bd_addr } => {
                self.peer_identity_address =
                    Some(Address::from_bytes(bd_addr, AddressType(addr_type)))
            }
            SmpPdu::SigningInformation { signature_key } => self.peer_csrk = Some(signature_key),
            _ => unreachable!("distribution kind was matched above"),
        }
        self.expected.remove(index);
        if self.expected.is_empty() {
            self.peer_distribution_complete();
        }
    }

    pub fn poll_outbound(&mut self) -> Option<SmpPdu> {
        self.outbound.pop_front()
    }

    pub fn drain_outbound(&mut self) -> Vec<SmpPdu> {
        self.outbound.drain(..).collect()
    }

    pub fn state(&self) -> KeyDistributionState {
        self.state
    }

    pub fn failure(&self) -> Option<PairingFailureReason> {
        self.failure
    }

    pub fn peer_address(&self) -> &Address {
        self.peer_identity_address
            .as_ref()
            .unwrap_or(&self.config.peer_address)
    }

    pub fn pairing_keys(&self) -> Option<PairingKeys> {
        if self.state != KeyDistributionState::Complete {
            return None;
        }
        let key = |value: [u8; 16]| Key {
            value: value.to_vec(),
            authenticated: self.config.authenticated,
            ediv: None,
            rand: None,
            sign_counter: None,
        };
        let legacy_key = |value: [u8; 16], ediv: u16, rand: [u8; 8]| Key {
            value: value.to_vec(),
            authenticated: self.config.authenticated,
            ediv: Some(ediv),
            rand: Some(rand.to_vec()),
            sign_counter: None,
        };

        let mut keys = PairingKeys {
            address_type: Some(self.peer_address().address_type()),
            irk: self.peer_irk.map(key),
            csrk: self.peer_csrk.map(key),
            ..PairingKeys::default()
        };
        let local_flags = match self.config.role {
            PairingRole::Initiator => self.config.initiator_keys,
            PairingRole::Responder => self.config.responder_keys,
        };
        if local_flags.contains(KeyDistribution::SIGNING_KEY) {
            let mut local_csrk = key(self.config.local_keys.csrk);
            local_csrk.sign_counter = Some(0);
            keys.local_csrk = Some(local_csrk);
        }
        if self.config.secure_connections {
            keys.ltk = Some(key(self.config.pairing_ltk));
        } else {
            if local_flags.contains(KeyDistribution::ENCRYPTION_KEY) {
                let ours = legacy_key(
                    self.local_ltk(),
                    self.config.local_keys.ediv,
                    self.config.local_keys.rand,
                );
                match self.config.role {
                    PairingRole::Initiator => keys.ltk_peripheral = Some(ours),
                    PairingRole::Responder => keys.ltk_central = Some(ours),
                }
            }
            if let (Some(peer_ltk), Some(peer_ediv), Some(peer_rand)) =
                (self.peer_ltk, self.peer_ediv, self.peer_rand)
            {
                let peer = legacy_key(peer_ltk, peer_ediv, peer_rand);
                match self.config.role {
                    PairingRole::Initiator => keys.ltk_central = Some(peer),
                    PairingRole::Responder => keys.ltk_peripheral = Some(peer),
                }
            }
        }
        if self.config.secure_connections && local_flags.contains(KeyDistribution::LINK_KEY) {
            keys.link_key = Some(key(derive_link_key(
                &self.config.pairing_ltk,
                self.config.ct2,
            )));
        }
        Some(keys)
    }

    pub fn store_bond(&self, store: &mut dyn KeyStore) -> KeyStoreResult<bool> {
        let Some(keys) = self.pairing_keys() else {
            return Ok(false);
        };
        store.update(&self.peer_address().to_string(false), keys)?;
        Ok(true)
    }

    fn peer_distribution_complete(&mut self) {
        if self.config.role == PairingRole::Initiator && !self.local_sent {
            self.distribute_local_keys();
        }
        self.state = KeyDistributionState::Complete;
    }

    fn distribute_local_keys(&mut self) {
        if self.local_sent {
            return;
        }
        let flags = match self.config.role {
            PairingRole::Initiator => self.config.initiator_keys,
            PairingRole::Responder => self.config.responder_keys,
        };
        if !self.config.secure_connections && flags.contains(KeyDistribution::ENCRYPTION_KEY) {
            self.outbound.push_back(SmpPdu::EncryptionInformation {
                long_term_key: self.local_ltk(),
            });
            self.outbound.push_back(SmpPdu::MasterIdentification {
                ediv: self.config.local_keys.ediv,
                rand: self.config.local_keys.rand,
            });
        }
        if flags.contains(KeyDistribution::IDENTITY_KEY) {
            self.outbound.push_back(SmpPdu::IdentityInformation {
                identity_resolving_key: self.config.local_keys.irk,
            });
            self.outbound.push_back(SmpPdu::IdentityAddressInformation {
                addr_type: self.config.local_keys.identity_address.address_type().0,
                bd_addr: *self.config.local_keys.identity_address.address_bytes(),
            });
        }
        if flags.contains(KeyDistribution::SIGNING_KEY) {
            self.outbound.push_back(SmpPdu::SigningInformation {
                signature_key: self.config.local_keys.csrk,
            });
        }
        self.local_sent = true;
    }

    fn fail(&mut self, reason: PairingFailureReason) {
        self.failure = Some(reason);
        self.state = KeyDistributionState::Failed;
        self.outbound.push_back(SmpPdu::PairingFailed {
            reason: reason as u8,
        });
    }

    fn local_ltk(&self) -> [u8; 16] {
        let mut ltk = self.config.local_keys.ltk;
        ltk[usize::from(self.config.maximum_encryption_key_size).min(16)..].fill(0);
        ltk
    }
}

fn expected_distributions(
    secure_connections: bool,
    flags: KeyDistribution,
) -> Vec<DistributionKind> {
    let mut expected = Vec::new();
    if !secure_connections && flags.contains(KeyDistribution::ENCRYPTION_KEY) {
        expected.push(DistributionKind::EncryptionInformation);
        expected.push(DistributionKind::MasterIdentification);
    }
    if flags.contains(KeyDistribution::IDENTITY_KEY) {
        expected.push(DistributionKind::IdentityInformation);
        expected.push(DistributionKind::IdentityAddressInformation);
    }
    if flags.contains(KeyDistribution::SIGNING_KEY) {
        expected.push(DistributionKind::SigningInformation);
    }
    expected
}

pub(crate) fn is_key_distribution_pdu(pdu: &SmpPdu) -> bool {
    matches!(
        pdu,
        SmpPdu::EncryptionInformation { .. }
            | SmpPdu::MasterIdentification { .. }
            | SmpPdu::IdentityInformation { .. }
            | SmpPdu::IdentityAddressInformation { .. }
            | SmpPdu::SigningInformation { .. }
    )
}
