//! High-level device configuration loading.
//!
//! This mirrors `bumble.device.DeviceConfiguration`, including its special
//! address/IRK, advertising-data, and legacy advertising-interval handling.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use bumble::{Address, AddressType};
use serde::Deserialize;
use serde_json::Value;

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
