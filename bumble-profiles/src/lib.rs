//! Standard Bluetooth GATT profiles ported from `bumble.profiles`.

use bumble::Uuid;
use bumble_gatt::{
    AdapterError, AttTransport, CharacteristicProxy, DatabaseError, GattClient, GattError,
    ServiceProxy,
};
use core::fmt;

pub mod aics;
pub mod ascs;
pub mod asha;
pub mod bap;
pub mod battery_service;
pub mod csip;
pub mod device_information_service;
pub mod gap;
pub mod gatt_service;
pub mod gmap;
pub mod heart_rate_service;
pub mod le_audio;
pub mod mcp;
pub mod pacs;
pub mod pbp;
pub mod tmap;
pub mod vcs;
pub mod vocs;

#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    Gatt(GattError),
    Adapter(AdapterError),
    Database(DatabaseError),
    MissingCharacteristic(u16),
    InvalidValue(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gatt(error) => write!(formatter, "{error}"),
            Self::Adapter(error) => write!(formatter, "{error}"),
            Self::Database(error) => write!(formatter, "{error}"),
            Self::MissingCharacteristic(uuid) => {
                write!(
                    formatter,
                    "required GATT characteristic 0x{uuid:04X} is missing"
                )
            }
            Self::InvalidValue(message) => write!(formatter, "invalid profile value: {message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<GattError> for Error {
    fn from(error: GattError) -> Self {
        Self::Gatt(error)
    }
}

impl From<AdapterError> for Error {
    fn from(error: AdapterError) -> Self {
        Self::Adapter(error)
    }
}

impl From<DatabaseError> for Error {
    fn from(error: DatabaseError) -> Self {
        Self::Database(error)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

pub(crate) fn uuid(value: u16) -> Uuid {
    Uuid::from_16_bits(value)
}

pub(crate) fn find_characteristic(
    characteristics: &[CharacteristicProxy],
    characteristic_uuid: u16,
) -> Option<CharacteristicProxy> {
    let expected = uuid(characteristic_uuid);
    characteristics
        .iter()
        .find(|characteristic| characteristic.uuid == expected)
        .cloned()
}

pub(crate) fn require_characteristic(
    characteristics: &[CharacteristicProxy],
    characteristic_uuid: u16,
) -> Result<CharacteristicProxy> {
    find_characteristic(characteristics, characteristic_uuid)
        .ok_or(Error::MissingCharacteristic(characteristic_uuid))
}

pub(crate) fn discover_profile(
    client: &mut GattClient,
    transport: &mut impl AttTransport,
    service_uuid: u16,
) -> Result<Option<(ServiceProxy, Vec<CharacteristicProxy>)>> {
    let mut services = client.discover_service_by_uuid(transport, &uuid(service_uuid))?;
    let Some(service) = services.drain(..).next() else {
        return Ok(None);
    };
    let characteristics = client.discover_characteristics(transport, &service)?;
    Ok(Some((service, characteristics)))
}

pub(crate) fn discover_secondary_profile(
    client: &mut GattClient,
    transport: &mut impl AttTransport,
    service_uuid: u16,
) -> Result<Option<(ServiceProxy, Vec<CharacteristicProxy>)>> {
    let mut services = client.discover_secondary_service_by_uuid(transport, &uuid(service_uuid))?;
    let Some(service) = services.drain(..).next() else {
        return Ok(None);
    };
    let characteristics = client.discover_characteristics(transport, &service)?;
    Ok(Some((service, characteristics)))
}
