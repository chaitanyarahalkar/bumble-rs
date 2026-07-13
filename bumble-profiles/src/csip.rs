//! Coordinated Set Identification Service (CSIS).

use crate::{discover_profile, find_characteristic, require_characteristic, uuid, Error, Result};
use bumble::{advertising_data::Type as AdvertisingType, AdvertisingData};
use bumble_crypto::{aes_cmac, e};
use bumble_gatt::{
    permissions, properties, AccessContext, AttTransport, CharacteristicDefinition,
    CharacteristicProxy, DynamicValue, GattClient, GattServer, ServiceDefinition, ServiceProxy,
};
use rand_core::{OsRng, RngCore};
use std::sync::Arc;

pub const COORDINATED_SET_IDENTIFICATION_SERVICE: u16 = 0x1846;
pub const SET_IDENTITY_RESOLVING_KEY_CHARACTERISTIC: u16 = 0x2B84;
pub const COORDINATED_SET_SIZE_CHARACTERISTIC: u16 = 0x2B85;
pub const SET_MEMBER_LOCK_CHARACTERISTIC: u16 = 0x2B86;
pub const SET_MEMBER_RANK_CHARACTERISTIC: u16 = 0x2B87;
pub const SET_IDENTITY_RESOLVING_KEY_LENGTH: usize = 16;

const RESOLVABLE_SET_IDENTIFIER_AD_TYPE: u8 = 0x2E;
const UNLIKELY_ERROR: u8 = 0x0E;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SirkType {
    Encrypted = 0x00,
    Plaintext = 0x01,
}

impl TryFrom<u8> for SirkType {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0x00 => Ok(Self::Encrypted),
            0x01 => Ok(Self::Plaintext),
            _ => Err(Error::InvalidValue(format!(
                "unknown Set Identity Resolving Key type 0x{value:02X}"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MemberLock {
    Unlocked = 0x01,
    Locked = 0x02,
}

fn reversed(value: &[u8]) -> Vec<u8> {
    value.iter().rev().copied().collect()
}

/// CSIS salt generation function, Core Specification Supplement section 4.3.
pub fn s1(message: &[u8]) -> [u8; 16] {
    let mut result = aes_cmac(&reversed(message), &[0; 16]);
    result.reverse();
    result
}

/// CSIS key derivation function, Core Specification Supplement section 4.4.
pub fn k1(n: &[u8], salt: &[u8], p: &[u8]) -> Result<[u8; 16]> {
    require_length("k1 salt", salt, 16)?;
    let t = aes_cmac(&reversed(n), &reversed(salt));
    let mut result = aes_cmac(&reversed(p), &t);
    result.reverse();
    Ok(result)
}

/// Encrypts or decrypts a SIRK, since CSIS `sef` and `sdf` are identical.
pub fn sef(key: &[u8], value: &[u8]) -> Result<[u8; 16]> {
    require_length("SIRK encryption key", key, 16)?;
    require_length("SIRK", value, 16)?;
    let salt = s1(&reversed(b"SIRKenc"));
    let derived = k1(key, &salt, &reversed(b"csis"))?;
    let mut output = [0; 16];
    for (output, (left, right)) in output.iter_mut().zip(derived.iter().zip(value)) {
        *output = left ^ right;
    }
    Ok(output)
}

/// Computes the three-byte Resolvable Set Identifier hash.
pub fn sih(key: &[u8], prand: &[u8]) -> Result<[u8; 3]> {
    require_length("SIRK", key, 16)?;
    require_length("RSI prand", prand, 3)?;
    let mut input = [0; 16];
    input[..3].copy_from_slice(prand);
    let encrypted = e(key, &input);
    Ok([encrypted[0], encrypted[1], encrypted[2]])
}

/// Builds an RSI from a deterministic three-byte random part.
pub fn rsi_with_prand(sirk: &[u8], prand: [u8; 3]) -> Result<[u8; 6]> {
    let hash = sih(sirk, &prand)?;
    Ok([hash[0], hash[1], hash[2], prand[0], prand[1], prand[2]])
}

/// Generates a Resolvable Set Identifier for advertising.
pub fn generate_rsi(sirk: &[u8]) -> Result<[u8; 6]> {
    let mut prand = [0; 3];
    OsRng.fill_bytes(&mut prand);
    // The two most significant bits are 0b01 (Vol 6, Part E, Table 1.2).
    prand[2] = (prand[2] & 0x7F) | 0x40;
    rsi_with_prand(sirk, prand)
}

fn require_length(name: &str, value: &[u8], expected: usize) -> Result<()> {
    if value.len() != expected {
        return Err(Error::InvalidValue(format!(
            "{name} has length {}, expected {expected}",
            value.len()
        )));
    }
    Ok(())
}

type EncryptionKeyReader = dyn Fn(AccessContext) -> Option<[u8; 16]> + Send + Sync + 'static;

#[derive(Clone)]
pub struct CoordinatedSetIdentificationService {
    sirk: [u8; 16],
    sirk_type: SirkType,
    coordinated_set_size: Option<u8>,
    set_member_lock: Option<MemberLock>,
    set_member_rank: Option<u8>,
    encryption_key: Option<Arc<EncryptionKeyReader>>,
}

impl core::fmt::Debug for CoordinatedSetIdentificationService {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("CoordinatedSetIdentificationService")
            .field("sirk_type", &self.sirk_type)
            .field("coordinated_set_size", &self.coordinated_set_size)
            .field("set_member_lock", &self.set_member_lock)
            .field("set_member_rank", &self.set_member_rank)
            .field("has_encryption_key_reader", &self.encryption_key.is_some())
            .finish_non_exhaustive()
    }
}

impl CoordinatedSetIdentificationService {
    pub fn new(sirk: &[u8], sirk_type: SirkType) -> Result<Self> {
        require_length("SIRK", sirk, SET_IDENTITY_RESOLVING_KEY_LENGTH)?;
        let mut key = [0; 16];
        key.copy_from_slice(sirk);
        Ok(Self {
            sirk: key,
            sirk_type,
            coordinated_set_size: None,
            set_member_lock: None,
            set_member_rank: None,
            encryption_key: None,
        })
    }

    pub fn coordinated_set_size(mut self, size: u8) -> Self {
        self.coordinated_set_size = Some(size);
        self
    }

    pub fn set_member_lock(mut self, lock: MemberLock) -> Self {
        self.set_member_lock = Some(lock);
        self
    }

    pub fn set_member_rank(mut self, rank: u8) -> Self {
        self.set_member_rank = Some(rank);
        self
    }

    pub fn encryption_key(
        mut self,
        reader: impl Fn(AccessContext) -> Option<[u8; 16]> + Send + Sync + 'static,
    ) -> Self {
        self.encryption_key = Some(Arc::new(reader));
        self
    }

    pub fn definition(&self) -> ServiceDefinition {
        let encrypted_read = permissions::READABLE | permissions::READ_REQUIRES_ENCRYPTION;
        let mut characteristics = vec![CharacteristicDefinition {
            uuid: uuid(SET_IDENTITY_RESOLVING_KEY_CHARACTERISTIC),
            properties: properties::READ | properties::NOTIFY,
            permissions: encrypted_read,
            value: vec![self.sirk_type as u8; 17],
            descriptors: Vec::new(),
        }];
        if let Some(size) = self.coordinated_set_size {
            characteristics.push(optional_characteristic(
                COORDINATED_SET_SIZE_CHARACTERISTIC,
                properties::READ | properties::NOTIFY,
                encrypted_read,
                size,
            ));
        }
        if let Some(lock) = self.set_member_lock {
            characteristics.push(optional_characteristic(
                SET_MEMBER_LOCK_CHARACTERISTIC,
                properties::READ | properties::NOTIFY | properties::WRITE,
                encrypted_read | permissions::WRITEABLE,
                lock as u8,
            ));
        }
        if let Some(rank) = self.set_member_rank {
            characteristics.push(optional_characteristic(
                SET_MEMBER_RANK_CHARACTERISTIC,
                properties::READ | properties::NOTIFY,
                encrypted_read,
                rank,
            ));
        }
        ServiceDefinition {
            uuid: uuid(COORDINATED_SET_IDENTIFICATION_SERVICE),
            primary: true,
            included_services: Vec::new(),
            characteristics,
        }
    }

    pub fn bind(&self, server: &mut GattServer) -> Result<u16> {
        let handle = server
            .handles_by_uuid(&uuid(SET_IDENTITY_RESOLVING_KEY_CHARACTERISTIC))
            .into_iter()
            .next()
            .ok_or_else(|| Error::InvalidValue("missing CSIS SIRK characteristic".into()))?;
        let sirk = self.sirk;
        let sirk_type = self.sirk_type;
        let encryption_key = self.encryption_key.clone();
        server.set_dynamic_value(
            handle,
            DynamicValue::read_only(move |context| {
                let value = match sirk_type {
                    SirkType::Plaintext => sirk,
                    SirkType::Encrypted => {
                        let key = encryption_key
                            .as_ref()
                            .and_then(|reader| reader(context))
                            .ok_or(UNLIKELY_ERROR)?;
                        sef(&key, &sirk).map_err(|_| UNLIKELY_ERROR)?
                    }
                };
                let mut encoded = Vec::with_capacity(17);
                encoded.push(sirk_type as u8);
                encoded.extend_from_slice(&value);
                Ok(encoded)
            }),
        )?;
        Ok(handle)
    }

    pub fn advertising_data(&self) -> Result<Vec<u8>> {
        Ok(AdvertisingData {
            ad_structures: vec![(
                AdvertisingType(RESOLVABLE_SET_IDENTIFIER_AD_TYPE),
                generate_rsi(&self.sirk)?.to_vec(),
            )],
        }
        .to_bytes())
    }
}

fn optional_characteristic(
    characteristic_uuid: u16,
    characteristic_properties: u8,
    characteristic_permissions: u8,
    value: u8,
) -> CharacteristicDefinition {
    CharacteristicDefinition {
        uuid: uuid(characteristic_uuid),
        properties: characteristic_properties,
        permissions: characteristic_permissions,
        value: vec![value],
        descriptors: Vec::new(),
    }
}

#[derive(Clone, Debug)]
pub struct CoordinatedSetIdentificationProxy {
    pub service: ServiceProxy,
    pub set_identity_resolving_key: CharacteristicProxy,
    pub coordinated_set_size: Option<CharacteristicProxy>,
    pub set_member_lock: Option<CharacteristicProxy>,
    pub set_member_rank: Option<CharacteristicProxy>,
}

impl CoordinatedSetIdentificationProxy {
    pub fn from_parts(
        service: ServiceProxy,
        characteristics: &[CharacteristicProxy],
    ) -> Result<Self> {
        Ok(Self {
            service,
            set_identity_resolving_key: require_characteristic(
                characteristics,
                SET_IDENTITY_RESOLVING_KEY_CHARACTERISTIC,
            )?,
            coordinated_set_size: find_characteristic(
                characteristics,
                COORDINATED_SET_SIZE_CHARACTERISTIC,
            ),
            set_member_lock: find_characteristic(characteristics, SET_MEMBER_LOCK_CHARACTERISTIC),
            set_member_rank: find_characteristic(characteristics, SET_MEMBER_RANK_CHARACTERISTIC),
        })
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let Some((service, characteristics)) =
            discover_profile(client, transport, COORDINATED_SET_IDENTIFICATION_SERVICE)?
        else {
            return Ok(None);
        };
        Self::from_parts(service, &characteristics).map(Some)
    }

    pub fn read_set_identity_resolving_key(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
        encryption_key: Option<[u8; 16]>,
    ) -> Result<(SirkType, [u8; 16])> {
        let value = client.read_value(transport, self.set_identity_resolving_key.handle, false)?;
        if value.len() != SET_IDENTITY_RESOLVING_KEY_LENGTH + 1 {
            return Err(Error::InvalidValue(format!(
                "SIRK value has length {}, expected 17",
                value.len()
            )));
        }
        let sirk_type = SirkType::try_from(value[0])?;
        let mut sirk = [0; 16];
        sirk.copy_from_slice(&value[1..]);
        if sirk_type == SirkType::Encrypted {
            let key = encryption_key.ok_or_else(|| {
                Error::InvalidValue("LTK or LinkKey is not present for encrypted SIRK".into())
            })?;
            sirk = sef(&key, &sirk)?;
        }
        Ok((sirk_type, sirk))
    }
}
