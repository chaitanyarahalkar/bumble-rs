//! High-level device configuration loading.
//!
//! This mirrors `bumble.device.DeviceConfiguration`, including its special
//! address/IRK, advertising-data, and legacy advertising-interval handling.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use bumble::{Address, AddressType, Uuid};
use bumble_gatt::{
    permissions, properties, CharacteristicDefinition, DescriptorDefinition, GattServer,
    ServiceDefinition,
};
use bumble_profiles::gap::GenericAccessService;
use bumble_profiles::gatt_service::{GenericAttributeProfileService, EATT_SUPPORTED};
use bumble_smp::{
    AcceptAllDelegate, IdentityAddressType, IoCapability, PairingConfig, PairingManager,
};
use serde::Deserialize;
use serde_json::{Map, Value};

pub const DEVICE_DEFAULT_NAME: &str = "Bumble";
pub const DEVICE_DEFAULT_ADDRESS: &str = "00:00:00:00:00:00";
pub const DEVICE_DEFAULT_ADVERTISING_INTERVAL: f64 = 1_000.0;
pub const DEVICE_DEFAULT_CLASS_OF_DEVICE: u32 = 0;
pub const DEVICE_DEFAULT_LE_RPA_TIMEOUT: u64 = 15 * 60;

/// Errors produced while loading a [`DeviceConfiguration`].
#[derive(Debug)]
pub enum DeviceConfigurationError {
    Io(std::io::Error),
    Json(serde_json::Error),
    InvalidField {
        field: &'static str,
        message: String,
    },
}

impl fmt::Display for DeviceConfigurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "device configuration I/O error: {error}"),
            Self::Json(error) => write!(f, "invalid device configuration JSON: {error}"),
            Self::InvalidField { field, message } => {
                write!(f, "invalid device configuration field {field:?}: {message}")
            }
        }
    }
}

impl std::error::Error for DeviceConfigurationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::InvalidField { .. } => None,
        }
    }
}

impl From<std::io::Error> for DeviceConfigurationError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for DeviceConfigurationError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Upstream-compatible reusable device configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceConfiguration {
    pub name: String,
    pub address: Address,
    pub class_of_device: u32,
    pub scan_response_data: Vec<u8>,
    pub advertising_interval_min: f64,
    pub advertising_interval_max: f64,
    pub le_enabled: bool,
    pub le_simultaneous_enabled: bool,
    pub le_privacy_enabled: bool,
    pub le_rpa_timeout: u64,
    pub le_subrate_enabled: bool,
    pub le_shorter_connection_intervals_enabled: bool,
    pub classic_enabled: bool,
    pub classic_sc_enabled: bool,
    pub classic_ssp_enabled: bool,
    pub classic_smp_enabled: bool,
    pub classic_accept_any: bool,
    pub classic_interlaced_scan_enabled: bool,
    pub connectable: bool,
    pub discoverable: bool,
    pub advertising_data: Vec<u8>,
    pub irk: Vec<u8>,
    pub keystore: Option<String>,
    pub address_resolution_offload: bool,
    pub address_generation_offload: bool,
    pub cis_enabled: bool,
    pub channel_sounding_enabled: bool,
    pub identity_address_type: Option<u8>,
    pub io_capability: u8,
    pub gap_service_enabled: bool,
    pub gatt_service_enabled: bool,
    pub enhanced_retransmission_supported: bool,
    pub l2cap_extended_features: Vec<u16>,
    pub eatt_enabled: bool,
    pub gatt_services: Vec<Value>,
    pub smp_debug_mode: bool,
    /// Unknown Python dictionary keys that would become dynamic attributes.
    pub extra: BTreeMap<String, Value>,
}

impl Default for DeviceConfiguration {
    fn default() -> Self {
        Self {
            name: DEVICE_DEFAULT_NAME.into(),
            address: Address::parse(DEVICE_DEFAULT_ADDRESS, AddressType::RANDOM_DEVICE)
                .expect("the built-in default address is valid"),
            class_of_device: DEVICE_DEFAULT_CLASS_OF_DEVICE,
            scan_response_data: Vec::new(),
            advertising_interval_min: DEVICE_DEFAULT_ADVERTISING_INTERVAL,
            advertising_interval_max: DEVICE_DEFAULT_ADVERTISING_INTERVAL,
            le_enabled: true,
            le_simultaneous_enabled: false,
            le_privacy_enabled: false,
            le_rpa_timeout: DEVICE_DEFAULT_LE_RPA_TIMEOUT,
            le_subrate_enabled: false,
            le_shorter_connection_intervals_enabled: false,
            classic_enabled: false,
            classic_sc_enabled: true,
            classic_ssp_enabled: true,
            classic_smp_enabled: true,
            classic_accept_any: true,
            classic_interlaced_scan_enabled: true,
            connectable: true,
            discoverable: true,
            advertising_data: complete_local_name(DEVICE_DEFAULT_NAME)
                .expect("the built-in default name fits in advertising data"),
            irk: vec![0; 16],
            keystore: None,
            address_resolution_offload: false,
            address_generation_offload: false,
            cis_enabled: false,
            channel_sounding_enabled: false,
            identity_address_type: None,
            io_capability: 0x03,
            gap_service_enabled: true,
            gatt_service_enabled: true,
            enhanced_retransmission_supported: false,
            l2cap_extended_features: vec![0x0080, 0x0020, 0x0008],
            eatt_enabled: false,
            gatt_services: Vec::new(),
            smp_debug_mode: false,
            extra: BTreeMap::new(),
        }
    }
}

impl DeviceConfiguration {
    pub(crate) fn build_pairing_manager(&self) -> Result<PairingManager, DeviceConfigurationError> {
        let io_capability = IoCapability::try_from(self.io_capability).map_err(|error| {
            DeviceConfigurationError::InvalidField {
                field: "io_capability",
                message: error.to_string(),
            }
        })?;
        let identity_address_type = match self.identity_address_type {
            None => None,
            Some(0) => Some(IdentityAddressType::Public),
            Some(1) => Some(IdentityAddressType::Random),
            Some(value) => {
                return Err(DeviceConfigurationError::InvalidField {
                    field: "identity_address_type",
                    message: format!("expected 0 (public) or 1 (random), got {value}"),
                });
            }
        };
        let mut pairing_config = PairingConfig::default();
        pairing_config.capabilities.io_capability = io_capability;
        pairing_config.identity_address_type = identity_address_type;
        let mut manager =
            PairingManager::new(pairing_config, Box::new(|_, _| Box::new(AcceptAllDelegate)));
        manager.set_debug_mode(self.smp_debug_mode);
        Ok(manager)
    }

    pub(crate) fn build_gatt_server(&self) -> Result<GattServer, DeviceConfigurationError> {
        let mut definitions = self
            .gatt_services
            .iter()
            .enumerate()
            .map(|(index, service)| parse_gatt_service(index, service))
            .collect::<Result<Vec<_>, _>>()?;

        if self.gap_service_enabled {
            definitions.push(
                GenericAccessService::from_packed_appearance(self.name.clone(), 0).definition(),
            );
        }

        let gatt_service = self
            .gatt_service_enabled
            .then_some(GenericAttributeProfileService {
                server_supported_features: self.eatt_enabled.then_some(EATT_SUPPORTED),
                ..GenericAttributeProfileService::default()
            });
        if let Some(service) = gatt_service {
            definitions.push(service.definition());
        }

        let mut server = GattServer::from_definitions(definitions)
            .map_err(|error| invalid_gatt_configuration("database", error.to_string()))?;
        if let Some(service) = gatt_service {
            service
                .bind_database_hash(&mut server)
                .map_err(|error| invalid_gatt_configuration("database_hash", error.to_string()))?;
        }
        Ok(server)
    }

    pub fn load_from_json_value(&mut self, value: &Value) -> Result<(), DeviceConfigurationError> {
        let clear_keystore = value.get("keystore").is_some_and(Value::is_null);
        let clear_identity_address_type = value
            .get("identity_address_type")
            .is_some_and(Value::is_null);
        let raw: RawDeviceConfiguration = serde_json::from_value(value.clone())?;

        if let Some(address) = raw.address.as_deref().filter(|address| !address.is_empty()) {
            self.address =
                Address::parse(address, AddressType::RANDOM_DEVICE).map_err(|error| {
                    DeviceConfigurationError::InvalidField {
                        field: "address",
                        message: error.to_string(),
                    }
                })?;
        }

        if let Some(irk) = raw.irk.as_deref().filter(|irk| !irk.is_empty()) {
            self.irk = decode_hex_field("irk", irk)?;
        } else if self.address
            != Address::parse(DEVICE_DEFAULT_ADDRESS, AddressType::RANDOM_DEVICE)
                .expect("the built-in default address is valid")
        {
            let bytes = self.address.address_bytes();
            self.irk = bytes.iter().copied().cycle().take(16).collect::<Vec<_>>();
        } else {
            self.irk = bumble_crypto::random_128().to_vec();
        }

        let name_was_set = raw.name.is_some();
        if let Some(name) = raw.name {
            self.name = name;
        }

        if let Some(data) = raw
            .advertising_data
            .as_deref()
            .filter(|data| !data.is_empty())
        {
            self.advertising_data = decode_hex_field("advertising_data", data)?;
        } else if name_was_set {
            self.advertising_data = complete_local_name(&self.name)?;
        }

        if let Some(data) = raw
            .scan_response_data
            .as_deref()
            .filter(|data| !data.is_empty())
        {
            self.scan_response_data = decode_hex_field("scan_response_data", data)?;
        }

        if let Some(interval) = raw.advertising_interval.filter(|interval| *interval != 0.0) {
            self.advertising_interval_min = interval;
            self.advertising_interval_max = interval;
        }

        macro_rules! assign {
            ($($field:ident),+ $(,)?) => {
                $(if let Some(value) = raw.$field {
                    self.$field = value;
                })+
            };
        }

        assign!(
            class_of_device,
            advertising_interval_min,
            advertising_interval_max,
            le_enabled,
            le_simultaneous_enabled,
            le_privacy_enabled,
            le_rpa_timeout,
            le_subrate_enabled,
            le_shorter_connection_intervals_enabled,
            classic_enabled,
            classic_sc_enabled,
            classic_ssp_enabled,
            classic_smp_enabled,
            classic_accept_any,
            classic_interlaced_scan_enabled,
            connectable,
            discoverable,
            keystore,
            address_resolution_offload,
            address_generation_offload,
            cis_enabled,
            channel_sounding_enabled,
            identity_address_type,
            io_capability,
            gap_service_enabled,
            gatt_service_enabled,
            enhanced_retransmission_supported,
            l2cap_extended_features,
            eatt_enabled,
            gatt_services,
            smp_debug_mode,
        );
        if clear_keystore {
            self.keystore = None;
        }
        if clear_identity_address_type {
            self.identity_address_type = None;
        }
        self.extra.extend(raw.extra);

        Ok(())
    }

    pub fn from_json_value(value: &Value) -> Result<Self, DeviceConfigurationError> {
        let mut config = Self::default();
        config.load_from_json_value(value)?;
        Ok(config)
    }

    pub fn load_from_json_str(&mut self, json: &str) -> Result<(), DeviceConfigurationError> {
        let value = serde_json::from_str(json)?;
        self.load_from_json_value(&value)
    }

    pub fn from_json_str(json: &str) -> Result<Self, DeviceConfigurationError> {
        let mut config = Self::default();
        config.load_from_json_str(json)?;
        Ok(config)
    }

    pub fn load_from_file(
        &mut self,
        filename: impl AsRef<Path>,
    ) -> Result<(), DeviceConfigurationError> {
        let json = std::fs::read_to_string(filename)?;
        self.load_from_json_str(&json)
    }

    pub fn from_file(filename: impl AsRef<Path>) -> Result<Self, DeviceConfigurationError> {
        let mut config = Self::default();
        config.load_from_file(filename)?;
        Ok(config)
    }
}

fn parse_gatt_service(
    service_index: usize,
    value: &Value,
) -> Result<ServiceDefinition, DeviceConfigurationError> {
    let path = format!("[{service_index}]");
    let service = gatt_object(value, &path)?;
    let uuid = gatt_uuid(service, "uuid", &path)?;
    let characteristics = gatt_array(service, "characteristics", &path)?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            parse_gatt_characteristic(value, &format!("{path}.characteristics[{index}]"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ServiceDefinition {
        uuid,
        primary: true,
        included_services: Vec::new(),
        characteristics,
    })
}

fn parse_gatt_characteristic(
    value: &Value,
    path: &str,
) -> Result<CharacteristicDefinition, DeviceConfigurationError> {
    let characteristic = gatt_object(value, path)?;
    let descriptors = gatt_array(characteristic, "descriptors", path)?
        .iter()
        .enumerate()
        .map(|(index, value)| parse_gatt_descriptor(value, &format!("{path}.descriptors[{index}]")))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CharacteristicDefinition {
        uuid: gatt_uuid(characteristic, "uuid", path)?,
        properties: gatt_named_bits(
            characteristic.get("properties"),
            "properties",
            path,
            &[
                ("BROADCAST", properties::BROADCAST),
                ("READ", properties::READ),
                ("WRITE_WITHOUT_RESPONSE", properties::WRITE_WITHOUT_RESPONSE),
                ("WRITE", properties::WRITE),
                ("NOTIFY", properties::NOTIFY),
                ("INDICATE", properties::INDICATE),
                (
                    "AUTHENTICATED_SIGNED_WRITES",
                    properties::AUTHENTICATED_SIGNED_WRITES,
                ),
                ("EXTENDED_PROPERTIES", properties::EXTENDED_PROPERTIES),
            ],
            false,
        )?,
        permissions: gatt_named_bits(
            characteristic.get("permissions"),
            "permissions",
            path,
            &permission_names(),
            true,
        )?,
        value: Vec::new(),
        descriptors,
    })
}

fn parse_gatt_descriptor(
    value: &Value,
    path: &str,
) -> Result<DescriptorDefinition, DeviceConfigurationError> {
    let descriptor = gatt_object(value, path)?;
    if descriptor.get("permission").is_some_and(json_truthy) {
        return Err(invalid_gatt_configuration(
            path,
            "the key 'permission' must be renamed to 'permissions'",
        ));
    }
    Ok(DescriptorDefinition {
        uuid: gatt_uuid(descriptor, "descriptor_type", path)?,
        permissions: gatt_named_bits(
            descriptor.get("permissions"),
            "permissions",
            path,
            &permission_names(),
            true,
        )?,
        value: Vec::new(),
    })
}

fn permission_names() -> [(&'static str, u8); 8] {
    [
        ("READABLE", permissions::READABLE),
        ("WRITEABLE", permissions::WRITEABLE),
        (
            "READ_REQUIRES_ENCRYPTION",
            permissions::READ_REQUIRES_ENCRYPTION,
        ),
        (
            "WRITE_REQUIRES_ENCRYPTION",
            permissions::WRITE_REQUIRES_ENCRYPTION,
        ),
        (
            "READ_REQUIRES_AUTHENTICATION",
            permissions::READ_REQUIRES_AUTHENTICATION,
        ),
        (
            "WRITE_REQUIRES_AUTHENTICATION",
            permissions::WRITE_REQUIRES_AUTHENTICATION,
        ),
        (
            "READ_REQUIRES_AUTHORIZATION",
            permissions::READ_REQUIRES_AUTHORIZATION,
        ),
        (
            "WRITE_REQUIRES_AUTHORIZATION",
            permissions::WRITE_REQUIRES_AUTHORIZATION,
        ),
    ]
}

fn gatt_object<'a>(
    value: &'a Value,
    path: &str,
) -> Result<&'a Map<String, Value>, DeviceConfigurationError> {
    value
        .as_object()
        .ok_or_else(|| invalid_gatt_configuration(path, "expected an object"))
}

fn gatt_array<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    path: &str,
) -> Result<&'a [Value], DeviceConfigurationError> {
    match object.get(field) {
        None => Ok(&[]),
        Some(Value::Array(values)) => Ok(values),
        Some(_) => Err(invalid_gatt_configuration(
            &format!("{path}.{field}"),
            "expected an array",
        )),
    }
}

fn gatt_uuid(
    object: &Map<String, Value>,
    field: &str,
    path: &str,
) -> Result<Uuid, DeviceConfigurationError> {
    let field_path = format!("{path}.{field}");
    let value = object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_gatt_configuration(&field_path, "expected a UUID string"))?;
    Uuid::parse(value).map_err(|error| invalid_gatt_configuration(&field_path, error.to_string()))
}

fn gatt_named_bits(
    value: Option<&Value>,
    field: &str,
    path: &str,
    names: &[(&str, u8)],
    allow_number: bool,
) -> Result<u8, DeviceConfigurationError> {
    let field_path = format!("{path}.{field}");
    let value = value.ok_or_else(|| invalid_gatt_configuration(&field_path, "missing field"))?;
    if allow_number {
        if let Some(value) = value.as_u64() {
            return u8::try_from(value).map_err(|_| {
                invalid_gatt_configuration(&field_path, "numeric bits must fit in one byte")
            });
        }
    }
    let value = value
        .as_str()
        .ok_or_else(|| invalid_gatt_configuration(&field_path, "expected a flag-name string"))?;
    value
        .replace('|', ",")
        .split(',')
        .try_fold(0, |bits, name| {
            names
                .iter()
                .find_map(|(candidate, value)| (*candidate == name).then_some(*value))
                .map(|value| bits | value)
                .ok_or_else(|| {
                    invalid_gatt_configuration(&field_path, format!("unknown flag {name:?}"))
                })
        })
}

fn invalid_gatt_configuration(path: &str, message: impl Into<String>) -> DeviceConfigurationError {
    DeviceConfigurationError::InvalidField {
        field: "gatt_services",
        message: format!("{path}: {}", message.into()),
    }
}

fn json_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

fn decode_hex_field(field: &'static str, value: &str) -> Result<Vec<u8>, DeviceConfigurationError> {
    let bytes = value.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return Err(DeviceConfigurationError::InvalidField {
            field,
            message: "hex value must contain an even number of digits".into(),
        });
    }
    bytes
        .chunks_exact(2)
        .map(|pair| {
            let high =
                hex_nibble(pair[0]).ok_or_else(|| DeviceConfigurationError::InvalidField {
                    field,
                    message: format!("invalid hex digit {:?}", pair[0] as char),
                })?;
            let low =
                hex_nibble(pair[1]).ok_or_else(|| DeviceConfigurationError::InvalidField {
                    field,
                    message: format!("invalid hex digit {:?}", pair[1] as char),
                })?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn complete_local_name(name: &str) -> Result<Vec<u8>, DeviceConfigurationError> {
    let name = name.as_bytes();
    let length = name
        .len()
        .checked_add(1)
        .filter(|length| *length <= 255)
        .ok_or(DeviceConfigurationError::InvalidField {
            field: "name",
            message: "UTF-8 name is too long for one advertising-data structure".into(),
        })?;
    let mut data = Vec::with_capacity(length + 1);
    data.push(length as u8);
    data.push(0x09);
    data.extend_from_slice(name);
    Ok(data)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawDeviceConfiguration {
    name: Option<String>,
    address: Option<String>,
    class_of_device: Option<u32>,
    scan_response_data: Option<String>,
    advertising_interval: Option<f64>,
    advertising_interval_min: Option<f64>,
    advertising_interval_max: Option<f64>,
    le_enabled: Option<bool>,
    le_simultaneous_enabled: Option<bool>,
    le_privacy_enabled: Option<bool>,
    le_rpa_timeout: Option<u64>,
    le_subrate_enabled: Option<bool>,
    le_shorter_connection_intervals_enabled: Option<bool>,
    classic_enabled: Option<bool>,
    classic_sc_enabled: Option<bool>,
    classic_ssp_enabled: Option<bool>,
    classic_smp_enabled: Option<bool>,
    classic_accept_any: Option<bool>,
    classic_interlaced_scan_enabled: Option<bool>,
    connectable: Option<bool>,
    discoverable: Option<bool>,
    advertising_data: Option<String>,
    irk: Option<String>,
    keystore: Option<Option<String>>,
    address_resolution_offload: Option<bool>,
    address_generation_offload: Option<bool>,
    cis_enabled: Option<bool>,
    channel_sounding_enabled: Option<bool>,
    identity_address_type: Option<Option<u8>>,
    io_capability: Option<u8>,
    gap_service_enabled: Option<bool>,
    gatt_service_enabled: Option<bool>,
    enhanced_retransmission_supported: Option<bool>,
    l2cap_extended_features: Option<Vec<u16>>,
    eatt_enabled: Option<bool>,
    gatt_services: Option<Vec<Value>>,
    smp_debug_mode: Option<bool>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}
