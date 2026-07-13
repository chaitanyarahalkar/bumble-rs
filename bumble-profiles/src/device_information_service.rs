//! Device Information Service.

use crate::{discover_profile, find_characteristic, uuid, Error, Result};
use bumble_gatt::{
    permissions, properties, AdapterError, AttTransport, CharacteristicDefinition,
    CharacteristicProxy, CharacteristicProxyAdapter, DelegatedCodec, GattClient, ServiceDefinition,
    ServiceProxy, Utf8CharacteristicProxyAdapter, Utf8Codec,
};

pub const DEVICE_INFORMATION_SERVICE: u16 = 0x180A;
pub const SYSTEM_ID_CHARACTERISTIC: u16 = 0x2A23;
pub const MODEL_NUMBER_STRING_CHARACTERISTIC: u16 = 0x2A24;
pub const SERIAL_NUMBER_STRING_CHARACTERISTIC: u16 = 0x2A25;
pub const FIRMWARE_REVISION_STRING_CHARACTERISTIC: u16 = 0x2A26;
pub const HARDWARE_REVISION_STRING_CHARACTERISTIC: u16 = 0x2A27;
pub const SOFTWARE_REVISION_STRING_CHARACTERISTIC: u16 = 0x2A28;
pub const MANUFACTURER_NAME_STRING_CHARACTERISTIC: u16 = 0x2A29;
pub const REGULATORY_CERTIFICATION_DATA_LIST_CHARACTERISTIC: u16 = 0x2A2A;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceInformationService {
    pub manufacturer_name: Option<String>,
    pub model_number: Option<String>,
    pub serial_number: Option<String>,
    pub hardware_revision: Option<String>,
    pub firmware_revision: Option<String>,
    pub software_revision: Option<String>,
    /// `(OUI, manufacturer ID)` encoded into the 64-bit System ID field.
    pub system_id: Option<(u32, u64)>,
    pub ieee_regulatory_certification_data_list: Option<Vec<u8>>,
}

impl DeviceInformationService {
    pub fn pack_system_id(oui: u32, manufacturer_id: u64) -> Result<[u8; 8]> {
        if oui > 0xFF_FFFF {
            return Err(Error::InvalidValue("system ID OUI exceeds 24 bits".into()));
        }
        if manufacturer_id > 0xFF_FFFF_FFFF {
            return Err(Error::InvalidValue(
                "system ID manufacturer ID exceeds 40 bits".into(),
            ));
        }
        Ok(((u64::from(oui) << 40) | manufacturer_id).to_le_bytes())
    }

    pub fn unpack_system_id(bytes: &[u8]) -> Result<(u32, u64)> {
        let bytes: [u8; 8] = bytes.try_into().map_err(|_| {
            Error::InvalidValue(format!("system ID needs 8 bytes, got {}", bytes.len()))
        })?;
        let value = u64::from_le_bytes(bytes);
        Ok(((value >> 40) as u32, value & 0xFF_FFFF_FFFF))
    }

    pub fn definition(&self) -> Result<ServiceDefinition> {
        let mut characteristics = Vec::new();
        for (value, characteristic_uuid) in [
            (
                self.manufacturer_name.as_ref(),
                MANUFACTURER_NAME_STRING_CHARACTERISTIC,
            ),
            (
                self.model_number.as_ref(),
                MODEL_NUMBER_STRING_CHARACTERISTIC,
            ),
            (
                self.serial_number.as_ref(),
                SERIAL_NUMBER_STRING_CHARACTERISTIC,
            ),
            (
                self.hardware_revision.as_ref(),
                HARDWARE_REVISION_STRING_CHARACTERISTIC,
            ),
            (
                self.firmware_revision.as_ref(),
                FIRMWARE_REVISION_STRING_CHARACTERISTIC,
            ),
            (
                self.software_revision.as_ref(),
                SOFTWARE_REVISION_STRING_CHARACTERISTIC,
            ),
        ] {
            if let Some(value) = value {
                characteristics.push(read_characteristic(
                    characteristic_uuid,
                    value.as_bytes().to_vec(),
                ));
            }
        }
        if let Some((oui, manufacturer_id)) = self.system_id {
            characteristics.push(read_characteristic(
                SYSTEM_ID_CHARACTERISTIC,
                Self::pack_system_id(oui, manufacturer_id)?.to_vec(),
            ));
        }
        if let Some(value) = &self.ieee_regulatory_certification_data_list {
            characteristics.push(read_characteristic(
                REGULATORY_CERTIFICATION_DATA_LIST_CHARACTERISTIC,
                value.clone(),
            ));
        }
        Ok(ServiceDefinition {
            uuid: uuid(DEVICE_INFORMATION_SERVICE),
            primary: true,
            included_services: Vec::new(),
            characteristics,
        })
    }
}

fn read_characteristic(characteristic_uuid: u16, value: Vec<u8>) -> CharacteristicDefinition {
    CharacteristicDefinition {
        uuid: uuid(characteristic_uuid),
        properties: properties::READ,
        permissions: permissions::READABLE,
        value,
        descriptors: Vec::new(),
    }
}

pub type SystemIdProxy = CharacteristicProxyAdapter<DelegatedCodec<(u32, u64)>>;

#[derive(Clone, Debug)]
pub struct DeviceInformationServiceProxy {
    pub service: ServiceProxy,
    pub manufacturer_name: Option<Utf8CharacteristicProxyAdapter>,
    pub model_number: Option<Utf8CharacteristicProxyAdapter>,
    pub serial_number: Option<Utf8CharacteristicProxyAdapter>,
    pub hardware_revision: Option<Utf8CharacteristicProxyAdapter>,
    pub firmware_revision: Option<Utf8CharacteristicProxyAdapter>,
    pub software_revision: Option<Utf8CharacteristicProxyAdapter>,
    pub system_id: Option<SystemIdProxy>,
    pub ieee_regulatory_certification_data_list: Option<CharacteristicProxy>,
}

impl DeviceInformationServiceProxy {
    pub fn from_parts(service: ServiceProxy, characteristics: &[CharacteristicProxy]) -> Self {
        let utf8 = |characteristic_uuid| {
            find_characteristic(characteristics, characteristic_uuid)
                .map(|proxy| Utf8CharacteristicProxyAdapter::new(proxy, Utf8Codec))
        };
        let system_id =
            find_characteristic(characteristics, SYSTEM_ID_CHARACTERISTIC).map(|proxy| {
                SystemIdProxy::new(
                    proxy,
                    DelegatedCodec::new(
                        |value: &(u32, u64)| {
                            DeviceInformationService::pack_system_id(value.0, value.1)
                                .map(Vec::from)
                                .map_err(|error| AdapterError::Codec(error.to_string()))
                        },
                        |bytes| {
                            DeviceInformationService::unpack_system_id(bytes)
                                .map_err(|error| AdapterError::Codec(error.to_string()))
                        },
                    ),
                )
            });
        Self {
            service,
            manufacturer_name: utf8(MANUFACTURER_NAME_STRING_CHARACTERISTIC),
            model_number: utf8(MODEL_NUMBER_STRING_CHARACTERISTIC),
            serial_number: utf8(SERIAL_NUMBER_STRING_CHARACTERISTIC),
            hardware_revision: utf8(HARDWARE_REVISION_STRING_CHARACTERISTIC),
            firmware_revision: utf8(FIRMWARE_REVISION_STRING_CHARACTERISTIC),
            software_revision: utf8(SOFTWARE_REVISION_STRING_CHARACTERISTIC),
            system_id,
            ieee_regulatory_certification_data_list: find_characteristic(
                characteristics,
                REGULATORY_CERTIFICATION_DATA_LIST_CHARACTERISTIC,
            ),
        }
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let Some((service, characteristics)) =
            discover_profile(client, transport, DEVICE_INFORMATION_SERVICE)?
        else {
            return Ok(None);
        };
        Ok(Some(Self::from_parts(service, &characteristics)))
    }
}
