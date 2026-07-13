//! Pairing policy, OOB data, method selection, and cross-transport key derivation.

use bumble::advertising_data::Type as AdvertisingType;
use bumble::{Address, AddressType, AdvertisingData, LeRole};
use bumble_crypto::{f4, h6, h7, random_128, EccKey};

use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum IoCapability {
    DisplayOnly = 0x00,
    DisplayYesNo = 0x01,
    KeyboardOnly = 0x02,
    NoInputNoOutput = 0x03,
    KeyboardDisplay = 0x04,
}

impl TryFrom<u8> for IoCapability {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::DisplayOnly),
            1 => Ok(Self::DisplayYesNo),
            2 => Ok(Self::KeyboardOnly),
            3 => Ok(Self::NoInputNoOutput),
            4 => Ok(Self::KeyboardDisplay),
            _ => Err(Error::InvalidPacket("invalid SMP I/O capability".into())),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyDistribution(pub u8);

impl KeyDistribution {
    pub const ENCRYPTION_KEY: Self = Self(0b0001);
    pub const IDENTITY_KEY: Self = Self(0b0010);
    pub const SIGNING_KEY: Self = Self(0b0100);
    pub const LINK_KEY: Self = Self(0b1000);
    pub const ALL: Self = Self(0b1111);
    pub const DEFAULT: Self = Self(Self::ENCRYPTION_KEY.0 | Self::IDENTITY_KEY.0);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0 & Self::ALL.0)
    }
}

impl core::ops::BitOr for KeyDistribution {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self((self.0 | rhs.0) & Self::ALL.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AuthReq(pub u8);

impl AuthReq {
    pub const BONDING: Self = Self(0b0000_0001);
    pub const MITM: Self = Self(0b0000_0100);
    pub const SECURE_CONNECTIONS: Self = Self(0b0000_1000);
    pub const KEYPRESS: Self = Self(0b0001_0000);
    pub const CT2: Self = Self(0b0010_0000);

    pub fn from_booleans(
        bonding: bool,
        secure_connections: bool,
        mitm: bool,
        keypress: bool,
        ct2: bool,
    ) -> Self {
        let mut value = 0;
        for (enabled, flag) in [
            (bonding, Self::BONDING),
            (secure_connections, Self::SECURE_CONNECTIONS),
            (mitm, Self::MITM),
            (keypress, Self::KEYPRESS),
            (ct2, Self::CT2),
        ] {
            if enabled {
                value |= flag.0;
            }
        }
        Self(value)
    }

    pub fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairingMethod {
    JustWorks,
    NumericComparison,
    Passkey,
    Oob,
    CtkdOverClassic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairingMethodSelection {
    pub method: PairingMethod,
    pub initiator_displays: bool,
    pub responder_displays: bool,
}

impl PairingMethodSelection {
    const fn simple(method: PairingMethod) -> Self {
        Self {
            method,
            initiator_displays: false,
            responder_displays: false,
        }
    }

    const fn passkey(initiator_displays: bool, responder_displays: bool) -> Self {
        Self {
            method: PairingMethod::Passkey,
            initiator_displays,
            responder_displays,
        }
    }
}

/// Apply Vol 3, Part H, Table 2.8 exactly as Bumble's `Session` does.
pub fn select_pairing_method(
    secure_connections: bool,
    local_mitm: bool,
    peer_auth_req: AuthReq,
    initiator: IoCapability,
    responder: IoCapability,
) -> PairingMethodSelection {
    if !local_mitm && !peer_auth_req.contains(AuthReq::MITM) {
        return PairingMethodSelection::simple(PairingMethod::JustWorks);
    }
    use IoCapability::{
        DisplayOnly as D, DisplayYesNo as Y, KeyboardDisplay as B, KeyboardOnly as K,
        NoInputNoOutput as N,
    };
    match (initiator, responder) {
        (D, K | B) | (Y, K) => PairingMethodSelection::passkey(true, false),
        (Y, B) if !secure_connections => PairingMethodSelection::passkey(true, false),
        (K, D | Y | B) => PairingMethodSelection::passkey(false, true),
        (K, K) => PairingMethodSelection::passkey(false, false),
        (B, D) => PairingMethodSelection::passkey(false, true),
        (B, Y) if !secure_connections => PairingMethodSelection::passkey(false, true),
        (B, K) => PairingMethodSelection::passkey(true, false),
        (B, B) if !secure_connections => PairingMethodSelection::passkey(true, false),
        (Y, Y | B) | (B, Y | B) if secure_connections => {
            PairingMethodSelection::simple(PairingMethod::NumericComparison)
        }
        (N, _) | (_, N) | (D, D | Y) | (Y, D) => {
            PairingMethodSelection::simple(PairingMethod::JustWorks)
        }
        _ => PairingMethodSelection::simple(PairingMethod::JustWorks),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn select_pairing_method_with_oob(
    secure_connections: bool,
    local_has_oob: bool,
    peer_has_oob: bool,
    local_mitm: bool,
    peer_auth_req: AuthReq,
    initiator: IoCapability,
    responder: IoCapability,
) -> PairingMethodSelection {
    if (local_has_oob && peer_has_oob) || (secure_connections && (local_has_oob || peer_has_oob)) {
        PairingMethodSelection::simple(PairingMethod::Oob)
    } else {
        select_pairing_method(
            secure_connections,
            local_mitm,
            peer_auth_req,
            initiator,
            responder,
        )
    }
}

pub struct OobContext {
    pub ecc_key: EccKey,
    pub r: [u8; 16],
}

impl OobContext {
    pub fn new(ecc_key: Option<EccKey>, r: Option<[u8; 16]>) -> Self {
        Self {
            ecc_key: ecc_key.unwrap_or_else(EccKey::generate),
            r: r.unwrap_or_else(random_128),
        }
    }

    pub fn share(&self) -> OobSharedData {
        let mut pkx = self.ecc_key.public_x();
        pkx.reverse();
        OobSharedData {
            c: f4(&pkx, &pkx, &self.r, 0),
            r: self.r.to_vec(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OobLegacyContext {
    pub tk: Vec<u8>,
}

impl OobLegacyContext {
    pub fn new(tk: Option<Vec<u8>>) -> Self {
        Self {
            tk: tk.unwrap_or_else(|| random_128().to_vec()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OobSharedData {
    pub c: Vec<u8>,
    pub r: Vec<u8>,
}

impl OobSharedData {
    pub fn to_ad(&self) -> AdvertisingData {
        AdvertisingData {
            ad_structures: vec![
                (
                    AdvertisingType::LE_SECURE_CONNECTIONS_CONFIRMATION_VALUE,
                    self.c.clone(),
                ),
                (
                    AdvertisingType::LE_SECURE_CONNECTIONS_RANDOM_VALUE,
                    self.r.clone(),
                ),
            ],
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OobData {
    pub address: Option<Address>,
    pub role: Option<LeRole>,
    pub shared_data: Option<OobSharedData>,
    pub legacy_context: Option<OobLegacyContext>,
}

impl OobData {
    pub fn from_ad(ad: &AdvertisingData) -> Self {
        let address = ad
            .get(AdvertisingType::LE_BLUETOOTH_DEVICE_ADDRESS)
            .and_then(|data| {
                (data.len() == 7).then(|| {
                    Address::from_bytes(
                        data[1..].try_into().expect("checked six address bytes"),
                        AddressType(data[0]),
                    )
                })
            });
        let role = ad
            .get(AdvertisingType::LE_ROLE)
            .and_then(|data| data.first().copied())
            .map(LeRole);
        let c = ad.get(AdvertisingType::LE_SECURE_CONNECTIONS_CONFIRMATION_VALUE);
        let r = ad.get(AdvertisingType::LE_SECURE_CONNECTIONS_RANDOM_VALUE);
        Self {
            address,
            role,
            shared_data: c.zip(r).map(|(c, r)| OobSharedData { c, r }),
            legacy_context: ad
                .get(AdvertisingType::SECURITY_MANAGER_TK_VALUE)
                .map(|tk| OobLegacyContext { tk }),
        }
    }

    pub fn to_ad(&self) -> AdvertisingData {
        let mut structures = Vec::new();
        if let Some(address) = &self.address {
            let mut value = vec![address.address_type().0];
            value.extend_from_slice(address.address_bytes());
            structures.push((AdvertisingType::LE_BLUETOOTH_DEVICE_ADDRESS, value));
        }
        if let Some(role) = self.role {
            structures.push((AdvertisingType::LE_ROLE, vec![role.0]));
        }
        if let Some(shared) = &self.shared_data {
            structures.extend(shared.to_ad().ad_structures);
        }
        if let Some(legacy) = &self.legacy_context {
            structures.push((
                AdvertisingType::SECURITY_MANAGER_TK_VALUE,
                legacy.tk.clone(),
            ));
        }
        AdvertisingData {
            ad_structures: structures,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairingCapabilities {
    pub io_capability: IoCapability,
    pub local_initiator_key_distribution: KeyDistribution,
    pub local_responder_key_distribution: KeyDistribution,
    pub maximum_encryption_key_size: u8,
}

impl Default for PairingCapabilities {
    fn default() -> Self {
        Self {
            io_capability: IoCapability::NoInputNoOutput,
            local_initiator_key_distribution: KeyDistribution::DEFAULT,
            local_responder_key_distribution: KeyDistribution::DEFAULT,
            maximum_encryption_key_size: 16,
        }
    }
}

impl PairingCapabilities {
    pub fn negotiate_key_distribution(
        self,
        peer_initiator: KeyDistribution,
        peer_responder: KeyDistribution,
    ) -> (KeyDistribution, KeyDistribution) {
        (
            peer_initiator.intersection(self.local_initiator_key_distribution),
            peer_responder.intersection(self.local_responder_key_distribution),
        )
    }

    pub fn validate(self) -> Result<Self> {
        if !(7..=16).contains(&self.maximum_encryption_key_size) {
            return Err(Error::InvalidPacket(
                "maximum encryption key size must be between 7 and 16".into(),
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityAddressType {
    Public,
    Random,
}

pub struct PairingConfig {
    pub secure_connections: bool,
    pub mitm: bool,
    pub bonding: bool,
    pub capabilities: PairingCapabilities,
    pub identity_address_type: Option<IdentityAddressType>,
    pub oob: Option<OobConfig>,
}

pub struct OobConfig {
    pub our_context: Option<OobContext>,
    pub peer_data: Option<OobSharedData>,
    pub legacy_context: Option<OobLegacyContext>,
}

impl Default for PairingConfig {
    fn default() -> Self {
        Self {
            secure_connections: true,
            mitm: true,
            bonding: true,
            capabilities: PairingCapabilities::default(),
            identity_address_type: None,
            oob: None,
        }
    }
}

impl PairingConfig {
    pub fn validate(&self) -> Result<()> {
        self.capabilities.validate()?;
        if let Some(oob) = &self.oob {
            if self.secure_connections && oob.our_context.is_none() {
                return Err(Error::InvalidPacket(
                    "SC OOB pairing requires a local OOB context".into(),
                ));
            }
            if !self.secure_connections && oob.legacy_context.is_none() {
                return Err(Error::InvalidPacket(
                    "Legacy OOB pairing requires a TK context".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Synchronous counterpart of Bumble's async pairing delegate. Stateful test
/// or UI adapters can implement this trait and drive the sans-I/O session.
pub trait PairingDelegate {
    fn accept(&mut self) -> bool {
        true
    }

    fn confirm(&mut self, _auto: bool) -> bool {
        true
    }

    fn compare_numbers(&mut self, _number: u32, _digits: u8) -> bool {
        true
    }

    fn get_number(&mut self) -> Option<u32> {
        Some(0)
    }

    fn display_number(&mut self, _number: u32, _digits: u8) {}

    fn generate_passkey(&mut self) -> u32 {
        let value = random_128();
        u32::from_le_bytes(value[..4].try_into().expect("four random bytes")) % 1_000_000
    }
}

#[derive(Default)]
pub struct AcceptAllDelegate;

impl PairingDelegate for AcceptAllDelegate {}

pub fn derive_ltk(link_key: &[u8; 16], ct2: bool) -> [u8; 16] {
    const SALT: [u8; 16] = *b"\0\0\0\0\0\0\0\0\0\0\0\0tmp2";
    let ilk = if ct2 {
        h7(&SALT, link_key)
    } else {
        h6(link_key, b"tmp2")
    };
    to_16(&h6(&ilk, b"brle"))
}

pub fn derive_link_key(ltk: &[u8; 16], ct2: bool) -> [u8; 16] {
    const SALT: [u8; 16] = *b"\0\0\0\0\0\0\0\0\0\0\0\0tmp1";
    let ilk = if ct2 {
        h7(&SALT, ltk)
    } else {
        h6(ltk, b"tmp1")
    };
    to_16(&h6(&ilk, b"lebr"))
}

fn to_16(value: &[u8]) -> [u8; 16] {
    value.try_into().expect("SMP KDF returns 16 bytes")
}
