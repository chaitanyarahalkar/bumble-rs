//! Telephony and Media Audio Profile (TMAP) role service.

use crate::{discover_profile, require_characteristic, uuid, Error, Result};
use bumble_gatt::{
    permissions, properties, AttTransport, CharacteristicDefinition, CharacteristicProxy,
    GattClient, ServiceDefinition, ServiceProxy,
};
use std::ops::{BitOr, BitOrAssign};

pub const TELEPHONY_AND_MEDIA_AUDIO_SERVICE: u16 = 0x1855;
pub const TMAP_ROLE_CHARACTERISTIC: u16 = 0x2B51;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Role(pub u16);

impl Role {
    pub const CALL_GATEWAY: Self = Self(1 << 0);
    pub const CALL_TERMINAL: Self = Self(1 << 1);
    pub const UNICAST_MEDIA_SENDER: Self = Self(1 << 2);
    pub const UNICAST_MEDIA_RECEIVER: Self = Self(1 << 3);
    pub const BROADCAST_MEDIA_SENDER: Self = Self(1 << 4);
    pub const BROADCAST_MEDIA_RECEIVER: Self = Self(1 << 5);
}

impl BitOr for Role {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Role {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelephonyAndMediaAudioService {
    pub role: Role,
}

impl TelephonyAndMediaAudioService {
    pub fn new(role: Role) -> Self {
        Self { role }
    }

    pub fn definition(self) -> ServiceDefinition {
        ServiceDefinition {
            uuid: uuid(TELEPHONY_AND_MEDIA_AUDIO_SERVICE),
            primary: true,
            included_services: Vec::new(),
            characteristics: vec![CharacteristicDefinition {
                uuid: uuid(TMAP_ROLE_CHARACTERISTIC),
                properties: properties::READ,
                permissions: permissions::READABLE,
                value: self.role.0.to_le_bytes().to_vec(),
                descriptors: Vec::new(),
            }],
        }
    }
}

#[derive(Clone, Debug)]
pub struct TelephonyAndMediaAudioServiceProxy {
    pub service: ServiceProxy,
    pub role: CharacteristicProxy,
}

impl TelephonyAndMediaAudioServiceProxy {
    pub fn from_parts(
        service: ServiceProxy,
        characteristics: &[CharacteristicProxy],
    ) -> Result<Self> {
        Ok(Self {
            service,
            role: require_characteristic(characteristics, TMAP_ROLE_CHARACTERISTIC)?,
        })
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let Some((service, characteristics)) =
            discover_profile(client, transport, TELEPHONY_AND_MEDIA_AUDIO_SERVICE)?
        else {
            return Ok(None);
        };
        Self::from_parts(service, &characteristics).map(Some)
    }

    pub fn read_role(
        &self,
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Role> {
        let value = client.read_value(transport, self.role.handle, false)?;
        let bytes: [u8; 2] = value.try_into().map_err(|value: Vec<u8>| {
            Error::InvalidValue(format!("TMAP role has length {}, expected 2", value.len()))
        })?;
        Ok(Role(u16::from_le_bytes(bytes)))
    }
}
