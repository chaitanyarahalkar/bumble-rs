//! Generic Attribute Profile service.

use crate::{discover_profile, find_characteristic, uuid, Error, Result};
use bumble_gatt::{
    permissions, properties, AttTransport, CharacteristicDefinition, CharacteristicProxy,
    GattClient, GattServer, ServiceDefinition, ServiceProxy,
};

pub const GENERIC_ATTRIBUTE_SERVICE: u16 = 0x1801;
pub const SERVICE_CHANGED_CHARACTERISTIC: u16 = 0x2A05;
pub const CLIENT_SUPPORTED_FEATURES_CHARACTERISTIC: u16 = 0x2B29;
pub const DATABASE_HASH_CHARACTERISTIC: u16 = 0x2B2A;
pub const SERVER_SUPPORTED_FEATURES_CHARACTERISTIC: u16 = 0x2B3A;
pub const EATT_SUPPORTED: u8 = 1 << 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenericAttributeProfileService {
    pub server_supported_features: Option<u8>,
    pub database_hash_enabled: bool,
    pub service_change_enabled: bool,
}

impl Default for GenericAttributeProfileService {
    fn default() -> Self {
        Self {
            server_supported_features: None,
            database_hash_enabled: true,
            service_change_enabled: true,
        }
    }
}

impl GenericAttributeProfileService {
    pub fn definition(&self) -> ServiceDefinition {
        let mut characteristics = Vec::new();
        if self.service_change_enabled {
            characteristics.push(CharacteristicDefinition {
                uuid: uuid(SERVICE_CHANGED_CHARACTERISTIC),
                properties: properties::INDICATE,
                permissions: 0,
                value: Vec::new(),
                descriptors: Vec::new(),
            });
        }
        if (self.database_hash_enabled && self.service_change_enabled)
            || self
                .server_supported_features
                .is_some_and(|features| features & EATT_SUPPORTED != 0)
        {
            characteristics.push(CharacteristicDefinition {
                uuid: uuid(CLIENT_SUPPORTED_FEATURES_CHARACTERISTIC),
                properties: properties::READ | properties::WRITE,
                permissions: permissions::READABLE | permissions::WRITEABLE,
                value: vec![0],
                descriptors: Vec::new(),
            });
        }
        if self.database_hash_enabled {
            characteristics.push(CharacteristicDefinition {
                uuid: uuid(DATABASE_HASH_CHARACTERISTIC),
                properties: properties::READ,
                permissions: permissions::READABLE,
                value: vec![0; 16],
                descriptors: Vec::new(),
            });
        }
        if let Some(features) = self.server_supported_features {
            characteristics.push(CharacteristicDefinition {
                uuid: uuid(SERVER_SUPPORTED_FEATURES_CHARACTERISTIC),
                properties: properties::READ,
                permissions: permissions::READABLE,
                value: vec![features],
                descriptors: Vec::new(),
            });
        }
        ServiceDefinition {
            uuid: uuid(GENERIC_ATTRIBUTE_SERVICE),
            primary: true,
            included_services: Vec::new(),
            characteristics,
        }
    }

    pub fn bind_database_hash(&self, server: &mut GattServer) -> Result<Option<u16>> {
        if !self.database_hash_enabled {
            return Ok(None);
        }
        let handle = server
            .handles_by_uuid(&uuid(DATABASE_HASH_CHARACTERISTIC))
            .into_iter()
            .next()
            .ok_or(Error::MissingCharacteristic(DATABASE_HASH_CHARACTERISTIC))?;
        let hash = server.database_hash();
        server.set_attribute_value(handle, hash.to_vec())?;
        Ok(Some(handle))
    }
}

#[derive(Clone, Debug)]
pub struct GenericAttributeProfileServiceProxy {
    pub service: ServiceProxy,
    pub client_supported_features_characteristic: Option<CharacteristicProxy>,
    pub server_supported_features_characteristic: Option<CharacteristicProxy>,
    pub database_hash_characteristic: Option<CharacteristicProxy>,
    pub service_changed_characteristic: Option<CharacteristicProxy>,
}

impl GenericAttributeProfileServiceProxy {
    pub fn from_parts(service: ServiceProxy, characteristics: &[CharacteristicProxy]) -> Self {
        Self {
            service,
            client_supported_features_characteristic: find_characteristic(
                characteristics,
                CLIENT_SUPPORTED_FEATURES_CHARACTERISTIC,
            ),
            server_supported_features_characteristic: find_characteristic(
                characteristics,
                SERVER_SUPPORTED_FEATURES_CHARACTERISTIC,
            ),
            database_hash_characteristic: find_characteristic(
                characteristics,
                DATABASE_HASH_CHARACTERISTIC,
            ),
            service_changed_characteristic: find_characteristic(
                characteristics,
                SERVICE_CHANGED_CHARACTERISTIC,
            ),
        }
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let Some((service, characteristics)) =
            discover_profile(client, transport, GENERIC_ATTRIBUTE_SERVICE)?
        else {
            return Ok(None);
        };
        Ok(Some(Self::from_parts(service, &characteristics)))
    }
}
