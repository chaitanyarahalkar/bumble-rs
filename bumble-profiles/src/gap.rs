//! Generic Access Profile service.

use crate::{discover_profile, find_characteristic, uuid, Result};
use bumble::Appearance;
use bumble_gatt::{
    permissions, properties, AdapterError, AttTransport, CharacteristicDefinition,
    CharacteristicProxyAdapter, DelegatedCodec, GattClient, ServiceDefinition, ServiceProxy,
    Utf8CharacteristicProxyAdapter, Utf8Codec,
};

pub const GENERIC_ACCESS_SERVICE: u16 = 0x1800;
pub const DEVICE_NAME_CHARACTERISTIC: u16 = 0x2A00;
pub const APPEARANCE_CHARACTERISTIC: u16 = 0x2A01;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenericAccessService {
    device_name: String,
    appearance: Appearance,
}

impl GenericAccessService {
    pub fn new(device_name: impl Into<String>, appearance: Appearance) -> Self {
        Self {
            device_name: device_name.into(),
            appearance,
        }
    }

    pub fn from_packed_appearance(device_name: impl Into<String>, appearance: u16) -> Self {
        Self::new(device_name, Appearance::from_int(appearance))
    }

    pub fn definition(&self) -> ServiceDefinition {
        let mut device_name = self.device_name.as_bytes().to_vec();
        device_name.truncate(248);
        ServiceDefinition {
            uuid: uuid(GENERIC_ACCESS_SERVICE),
            primary: true,
            included_services: Vec::new(),
            characteristics: vec![
                CharacteristicDefinition {
                    uuid: uuid(DEVICE_NAME_CHARACTERISTIC),
                    properties: properties::READ,
                    permissions: permissions::READABLE,
                    value: device_name,
                    descriptors: Vec::new(),
                },
                CharacteristicDefinition {
                    uuid: uuid(APPEARANCE_CHARACTERISTIC),
                    properties: properties::READ,
                    permissions: permissions::READABLE,
                    value: self.appearance.to_bytes().to_vec(),
                    descriptors: Vec::new(),
                },
            ],
        }
    }
}

impl Default for GenericAccessService {
    fn default() -> Self {
        Self::from_packed_appearance("", 0)
    }
}

pub type AppearanceProxy = CharacteristicProxyAdapter<DelegatedCodec<Appearance>>;

#[derive(Clone, Debug)]
pub struct GenericAccessServiceProxy {
    pub service: ServiceProxy,
    pub device_name: Option<Utf8CharacteristicProxyAdapter>,
    pub appearance: Option<AppearanceProxy>,
}

impl GenericAccessServiceProxy {
    pub fn from_parts(
        service: ServiceProxy,
        characteristics: &[bumble_gatt::CharacteristicProxy],
    ) -> Self {
        let device_name = find_characteristic(characteristics, DEVICE_NAME_CHARACTERISTIC)
            .map(|proxy| Utf8CharacteristicProxyAdapter::new(proxy, Utf8Codec));
        let appearance =
            find_characteristic(characteristics, APPEARANCE_CHARACTERISTIC).map(|proxy| {
                AppearanceProxy::new(
                    proxy,
                    DelegatedCodec::decoder(|bytes| {
                        let bytes: [u8; 2] = bytes.try_into().map_err(|_| {
                            AdapterError::InvalidValue(format!(
                                "appearance needs 2 bytes, got {}",
                                bytes.len()
                            ))
                        })?;
                        Ok(Appearance::from_int(u16::from_le_bytes(bytes)))
                    }),
                )
            });
        Self {
            service,
            device_name,
            appearance,
        }
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let Some((service, characteristics)) =
            discover_profile(client, transport, GENERIC_ACCESS_SERVICE)?
        else {
            return Ok(None);
        };
        Ok(Some(Self::from_parts(service, &characteristics)))
    }
}
