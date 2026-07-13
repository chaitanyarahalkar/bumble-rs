//! Battery Service.

use crate::{discover_profile, require_characteristic, uuid, Error, Result};
use bumble_gatt::{
    permissions, properties, AccessContext, AdapterError, AttTransport, CharacteristicDefinition,
    CharacteristicProxyAdapter, DelegatedCodec, DynamicValue, GattClient, GattServer,
    ServiceDefinition, ServiceProxy,
};
use std::sync::Arc;

pub const BATTERY_SERVICE: u16 = 0x180F;
pub const BATTERY_LEVEL_CHARACTERISTIC: u16 = 0x2A19;

type BatteryReader = dyn Fn(AccessContext) -> u8 + Send + Sync + 'static;

#[derive(Clone)]
pub struct BatteryService {
    read_battery_level: Arc<BatteryReader>,
}

impl core::fmt::Debug for BatteryService {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("BatteryService { read_battery_level: <callback> }")
    }
}

impl BatteryService {
    pub fn new(read_battery_level: impl Fn(AccessContext) -> u8 + Send + Sync + 'static) -> Self {
        Self {
            read_battery_level: Arc::new(read_battery_level),
        }
    }

    pub fn with_level(level: u8) -> Self {
        Self::new(move |_| level)
    }

    pub fn definition(&self) -> ServiceDefinition {
        ServiceDefinition {
            uuid: uuid(BATTERY_SERVICE),
            primary: true,
            included_services: Vec::new(),
            characteristics: vec![CharacteristicDefinition {
                uuid: uuid(BATTERY_LEVEL_CHARACTERISTIC),
                properties: properties::READ | properties::NOTIFY,
                permissions: permissions::READABLE,
                value: vec![0],
                descriptors: Vec::new(),
            }],
        }
    }

    pub fn bind(&self, server: &mut GattServer) -> Result<u16> {
        let handle = server
            .handles_by_uuid(&uuid(BATTERY_LEVEL_CHARACTERISTIC))
            .into_iter()
            .next()
            .ok_or(Error::MissingCharacteristic(BATTERY_LEVEL_CHARACTERISTIC))?;
        let reader = Arc::clone(&self.read_battery_level);
        server.set_dynamic_value(
            handle,
            DynamicValue::read_only(move |context| Ok(vec![reader(context)])),
        )?;
        Ok(handle)
    }
}

pub type BatteryLevelProxy = CharacteristicProxyAdapter<DelegatedCodec<u8>>;

#[derive(Clone, Debug)]
pub struct BatteryServiceProxy {
    pub service: ServiceProxy,
    pub battery_level: BatteryLevelProxy,
}

impl BatteryServiceProxy {
    pub fn from_parts(
        service: ServiceProxy,
        characteristics: &[bumble_gatt::CharacteristicProxy],
    ) -> Result<Self> {
        let proxy = require_characteristic(characteristics, BATTERY_LEVEL_CHARACTERISTIC)?;
        let codec = DelegatedCodec::new(
            |value: &u8| Ok(vec![*value]),
            |bytes| match bytes {
                [value] => Ok(*value),
                _ => Err(AdapterError::InvalidValue(format!(
                    "battery level needs 1 byte, got {}",
                    bytes.len()
                ))),
            },
        );
        Ok(Self {
            service,
            battery_level: BatteryLevelProxy::new(proxy, codec),
        })
    }

    pub fn discover(
        client: &mut GattClient,
        transport: &mut impl AttTransport,
    ) -> Result<Option<Self>> {
        let Some((service, characteristics)) =
            discover_profile(client, transport, BATTERY_SERVICE)?
        else {
            return Ok(None);
        };
        Self::from_parts(service, &characteristics).map(Some)
    }
}
