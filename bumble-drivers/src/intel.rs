//! Intel USB controller firmware and Device Data Configuration support.

use crate::{
    metadata_u16, require_success, DriverHost, Error, FirmwareProvider, HciMetadata, Result,
};
use bumble_hci::{Command, HCI_RESET_COMMAND};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const INTEL_FIRMWARE_DIR_ENV: &str = "BUMBLE_INTEL_FIRMWARE_DIR";
pub const INTEL_LINUX_FIRMWARE_DIR: &str = "/lib/firmware/intel";
pub const MAX_FRAGMENT_SIZE: usize = 252;

pub const HCI_INTEL_WRITE_DEVICE_CONFIG_COMMAND: u16 = 0xFC8B;
pub const HCI_INTEL_READ_VERSION_COMMAND: u16 = 0xFC05;
pub const HCI_INTEL_RESET_COMMAND: u16 = 0xFC01;
pub const HCI_INTEL_SECURE_SEND_COMMAND: u16 = 0xFC09;
pub const HCI_INTEL_WRITE_BOOT_PARAMS_COMMAND: u16 = 0xFC0E;

pub const INTEL_USB_PRODUCTS: &[(u16, u16)] = &[
    (0x8087, 0x0032), // AX210
    (0x8087, 0x0033), // AX211
    (0x8087, 0x0036), // BE200
];

pub const INTEL_FW_IMAGE_NAMES: &[&str] = &[
    "ibt-0040-0041",
    "ibt-0040-1020",
    "ibt-0040-1050",
    "ibt-0040-2120",
    "ibt-0040-4150",
    "ibt-0041-0041",
    "ibt-0180-0041",
    "ibt-0180-1050",
    "ibt-0180-4150",
    "ibt-0291-0291",
    "ibt-1040-0041",
    "ibt-1040-1020",
    "ibt-1040-1050",
    "ibt-1040-2120",
    "ibt-1040-4150",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValueType(pub u8);

impl ValueType {
    pub const END: Self = Self(0x00);
    pub const CNVI: Self = Self(0x10);
    pub const CNVR: Self = Self(0x11);
    pub const HARDWARE_INFO: Self = Self(0x12);
    pub const DEVICE_REVISION: Self = Self(0x16);
    pub const USB_VENDOR_ID: Self = Self(0x17);
    pub const USB_PRODUCT_ID: Self = Self(0x18);
    pub const CURRENT_MODE_OF_OPERATION: Self = Self(0x1C);
    pub const TIMESTAMP: Self = Self(0x1D);
    pub const BUILD_TYPE: Self = Self(0x1E);
    pub const BUILD_NUMBER: Self = Self(0x1F);
    pub const SECURE_BOOT: Self = Self(0x28);
    pub const OTP_LOCK: Self = Self(0x2A);
    pub const API_LOCK: Self = Self(0x2B);
    pub const DEBUG_LOCK: Self = Self(0x2C);
    pub const FIRMWARE_BUILD: Self = Self(0x2D);
    pub const SECURE_BOOT_ENGINE_TYPE: Self = Self(0x2F);
    pub const BLUETOOTH_ADDRESS: Self = Self(0x30);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HardwareInfo {
    pub platform: u8,
    pub variant: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timestamp {
    pub week: u8,
    pub year: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirmwareBuild {
    pub build_number: u8,
    pub timestamp: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    ControllerVersion(u16),
    HardwareInfo(HardwareInfo),
    U16(u16),
    U8(u8),
    Timestamp(Timestamp),
    FirmwareBuild(FirmwareBuild),
    BluetoothAddress([u8; 6]),
    Bytes(Vec<u8>),
}

impl Value {
    pub fn as_u8(&self) -> Option<u8> {
        match self {
            Self::U8(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_controller_version(&self) -> Option<u16> {
        match self {
            Self::ControllerVersion(value) => Some(*value),
            _ => None,
        }
    }
}

pub type DeviceInfo = BTreeMap<ValueType, Value>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ModeOfOperation {
    Bootloader = 0x01,
    Intermediate = 0x02,
    Operational = 0x03,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SecureBootEngineType {
    Rsa = 0x00,
    Ecdsa = 0x01,
}

impl TryFrom<u8> for SecureBootEngineType {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Rsa),
            1 => Ok(Self::Ecdsa),
            _ => Err(Error::Unsupported(format!(
                "Intel secure boot engine 0x{value:02X}"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootParams {
    pub css_header_offset: usize,
    pub css_header_size: usize,
    pub pki_offset: usize,
    pub pki_size: usize,
    pub signature_offset: usize,
    pub signature_size: usize,
    pub write_offset: usize,
}

impl SecureBootEngineType {
    pub const fn boot_params(self) -> BootParams {
        match self {
            Self::Rsa => BootParams {
                css_header_offset: 0,
                css_header_size: 128,
                pki_offset: 128,
                pki_size: 256,
                signature_offset: 388,
                signature_size: 256,
                write_offset: 964,
            },
            Self::Ecdsa => BootParams {
                css_header_offset: 644,
                css_header_size: 128,
                pki_offset: 772,
                pki_size: 96,
                signature_offset: 868,
                signature_size: 96,
                write_offset: 964,
            },
        }
    }
}

/// Decode Intel's open-ended version TLV list.
pub fn parse_tlv(mut data: &[u8]) -> Result<Vec<(ValueType, Value)>> {
    let mut result = Vec::new();
    while data.len() >= 2 {
        let value_type = ValueType(data[0]);
        let value_length = usize::from(data[1]);
        data = &data[2..];
        if value_type == ValueType::END {
            break;
        }
        if data.len() < value_length {
            return Err(Error::InvalidResponse(format!(
                "Intel TLV 0x{:02X} declares {value_length} bytes with only {} remaining",
                value_type.0,
                data.len()
            )));
        }
        let bytes = &data[..value_length];
        let value = parse_tlv_value(value_type, bytes)?;
        result.push((value_type, value));
        data = &data[value_length..];
    }
    Ok(result)
}

fn parse_tlv_value(value_type: ValueType, value: &[u8]) -> Result<Value> {
    let exact = |expected: usize| {
        if value.len() == expected {
            Ok(())
        } else {
            Err(Error::InvalidResponse(format!(
                "Intel TLV 0x{:02X} has length {}, expected {expected}",
                value_type.0,
                value.len()
            )))
        }
    };

    Ok(match value_type {
        ValueType::CNVI | ValueType::CNVR => {
            exact(4)?;
            let raw = u32::from_le_bytes(value.try_into().expect("validated four-byte TLV"));
            let mapped = ((raw & 0xF) << 12)
                | ((raw >> 4) & 0xF)
                | (((raw >> 8) & 0xF) << 4)
                | (((raw >> 24) & 0xF) << 8);
            Value::ControllerVersion(mapped as u16)
        }
        ValueType::HARDWARE_INFO => {
            exact(4)?;
            let raw = u32::from_le_bytes(value.try_into().expect("validated four-byte TLV"));
            Value::HardwareInfo(HardwareInfo {
                platform: ((raw >> 8) & 0xFF) as u8,
                variant: ((raw >> 16) & 0x3F) as u8,
            })
        }
        ValueType::USB_VENDOR_ID | ValueType::USB_PRODUCT_ID | ValueType::DEVICE_REVISION => {
            exact(2)?;
            Value::U16(u16::from_le_bytes(
                value.try_into().expect("validated two-byte TLV"),
            ))
        }
        ValueType::CURRENT_MODE_OF_OPERATION
        | ValueType::BUILD_TYPE
        | ValueType::BUILD_NUMBER
        | ValueType::SECURE_BOOT
        | ValueType::OTP_LOCK
        | ValueType::API_LOCK
        | ValueType::DEBUG_LOCK
        | ValueType::SECURE_BOOT_ENGINE_TYPE => {
            exact(1)?;
            Value::U8(value[0])
        }
        ValueType::TIMESTAMP => {
            exact(2)?;
            Value::Timestamp(Timestamp {
                week: value[0],
                year: value[1],
            })
        }
        ValueType::FIRMWARE_BUILD => {
            exact(3)?;
            Value::FirmwareBuild(FirmwareBuild {
                build_number: value[0],
                timestamp: Timestamp {
                    week: value[1],
                    year: value[2],
                },
            })
        }
        ValueType::BLUETOOTH_ADDRESS => {
            exact(6)?;
            Value::BluetoothAddress(value.try_into().expect("validated address TLV"))
        }
        _ => Value::Bytes(value.to_vec()),
    })
}

pub fn device_info_from_tlv(data: &[u8]) -> Result<DeviceInfo> {
    Ok(parse_tlv(data)?.into_iter().collect())
}

/// Intel driver options embedded in `driver=intel/...` metadata.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DriverOptions {
    pub ddc_addon: Option<Vec<u8>>,
    pub ddc_override: Option<Vec<u8>>,
}

impl DriverOptions {
    pub fn parse(driver: Option<&str>) -> Result<Self> {
        let Some(options) = driver.and_then(|driver| driver.strip_prefix("intel/")) else {
            return Ok(Self::default());
        };
        let mut parsed = Self::default();
        for option in options.split('+') {
            let (key, value) = option.split_once(':').ok_or_else(|| {
                Error::InvalidMetadata(format!("invalid Intel driver option: {option}"))
            })?;
            let bytes = decode_hex(value)?;
            match key {
                "ddc_addon" => parsed.ddc_addon = Some(bytes),
                "ddc_override" => parsed.ddc_override = Some(bytes),
                _ => {}
            }
        }
        Ok(parsed)
    }
}

fn decode_hex(value: &str) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return Err(Error::InvalidMetadata("hex value has odd length".into()));
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| Error::InvalidMetadata(format!("invalid hex value: {value}")))
        })
        .collect()
}

pub fn check(metadata: &HciMetadata) -> bool {
    if metadata
        .get("driver")
        .is_some_and(|driver| driver == "intel" || driver.starts_with("intel/"))
    {
        return true;
    }
    let Some(vendor_id) = metadata_u16(metadata, "vendor_id") else {
        return false;
    };
    let Some(product_id) = metadata_u16(metadata, "product_id") else {
        return false;
    };
    INTEL_USB_PRODUCTS.contains(&(vendor_id, product_id))
}

pub fn firmware_base_name(info: &DeviceInfo) -> Result<String> {
    let cnvi = info
        .get(&ValueType::CNVI)
        .and_then(Value::as_controller_version)
        .ok_or_else(|| Error::InvalidResponse("Intel device info is missing CNVI".into()))?;
    let cnvr = info
        .get(&ValueType::CNVR)
        .and_then(Value::as_controller_version)
        .ok_or_else(|| Error::InvalidResponse("Intel device info is missing CNVR".into()))?;
    Ok(format!("ibt-{cnvi:04X}-{cnvr:04X}"))
}

/// Ordered filesystem lookup matching Bumble's environment override rules.
#[derive(Clone, Debug, Default)]
pub struct FirmwareSearch {
    pub environment_directory: Option<PathBuf>,
    pub project_directory: Option<PathBuf>,
    pub package_directory: Option<PathBuf>,
    pub system_directory: Option<PathBuf>,
    pub current_directory: Option<PathBuf>,
}

impl FirmwareSearch {
    pub fn from_environment(
        project_directory: Option<PathBuf>,
        package_directory: Option<PathBuf>,
    ) -> Self {
        Self {
            environment_directory: std::env::var_os(INTEL_FIRMWARE_DIR_ENV).map(PathBuf::from),
            project_directory,
            package_directory,
            system_directory: cfg!(target_os = "linux")
                .then(|| PathBuf::from(INTEL_LINUX_FIRMWARE_DIR)),
            current_directory: std::env::current_dir().ok(),
        }
    }

    pub fn find(&self, file_name: &str) -> Option<PathBuf> {
        if let Some(directory) = &self.environment_directory {
            return existing_file(directory, file_name);
        }
        [
            self.project_directory.as_ref(),
            self.package_directory.as_ref(),
            self.system_directory.as_ref(),
            self.current_directory.as_ref(),
        ]
        .into_iter()
        .flatten()
        .find_map(|directory| existing_file(directory, file_name))
    }
}

fn existing_file(directory: &Path, file_name: &str) -> Option<PathBuf> {
    let path = directory.join(file_name);
    path.is_file().then_some(path)
}

impl FirmwareProvider for FirmwareSearch {
    fn load(&self, file_name: &str) -> Result<Option<Vec<u8>>> {
        self.find(file_name)
            .map(fs::read)
            .transpose()
            .map_err(Into::into)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecureSend {
    pub data_type: u8,
    pub data: Vec<u8>,
}

impl SecureSend {
    pub fn command(&self) -> Command {
        let mut parameters = Vec::with_capacity(1 + self.data.len());
        parameters.push(self.data_type);
        parameters.extend_from_slice(&self.data);
        Command::Generic {
            op_code: HCI_INTEL_SECURE_SEND_COMMAND,
            parameters,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirmwarePlan {
    pub secure_sends: Vec<SecureSend>,
    pub boot_address: u32,
}

impl FirmwarePlan {
    pub fn parse(image: &[u8], engine: SecureBootEngineType) -> Result<Self> {
        let boot = engine.boot_params();
        if image.len() < boot.write_offset {
            return Err(Error::InvalidFirmware(format!(
                "Intel image has {} bytes, needs at least {}",
                image.len(),
                boot.write_offset
            )));
        }

        let mut secure_sends = Vec::new();
        append_secure_data(
            &mut secure_sends,
            0x00,
            checked_section(image, boot.css_header_offset, boot.css_header_size)?,
        );
        append_secure_data(
            &mut secure_sends,
            0x03,
            checked_section(image, boot.pki_offset, boot.pki_size)?,
        );
        append_secure_data(
            &mut secure_sends,
            0x02,
            checked_section(image, boot.signature_offset, boot.signature_size)?,
        );

        let mut offset = boot.write_offset;
        let mut group_start = offset;
        let mut boot_address = 0;
        while offset < image.len() {
            if image.len() - offset < 3 {
                return Err(Error::InvalidFirmware(
                    "truncated Intel firmware command header".into(),
                ));
            }
            let command_opcode = u16::from_le_bytes([image[offset], image[offset + 1]]);
            let command_size = usize::from(image[offset + 2]);
            let command_end = offset
                .checked_add(3 + command_size)
                .ok_or_else(|| Error::InvalidFirmware("Intel command length overflow".into()))?;
            if command_end > image.len() {
                return Err(Error::InvalidFirmware(format!(
                    "truncated Intel firmware command 0x{command_opcode:04X}"
                )));
            }
            if command_opcode == HCI_INTEL_WRITE_BOOT_PARAMS_COMMAND {
                if command_size < 4 {
                    return Err(Error::InvalidFirmware(
                        "Intel write-boot-params command has no boot address".into(),
                    ));
                }
                boot_address = u32::from_le_bytes(
                    image[offset + 3..offset + 7]
                        .try_into()
                        .expect("validated boot address"),
                );
            }
            offset = command_end;
            if (offset - group_start).is_multiple_of(4) {
                append_secure_data(&mut secure_sends, 0x01, &image[group_start..offset]);
                group_start = offset;
            }
        }
        if group_start != image.len() {
            return Err(Error::InvalidFirmware(
                "Intel firmware command stream is not four-byte aligned".into(),
            ));
        }

        Ok(Self {
            secure_sends,
            boot_address,
        })
    }

    pub fn commands(&self) -> Vec<Command> {
        self.secure_sends.iter().map(SecureSend::command).collect()
    }
}

fn checked_section(image: &[u8], offset: usize, length: usize) -> Result<&[u8]> {
    image
        .get(offset..offset + length)
        .ok_or_else(|| Error::InvalidFirmware("Intel image section is truncated".into()))
}

fn append_secure_data(output: &mut Vec<SecureSend>, data_type: u8, mut data: &[u8]) {
    while !data.is_empty() {
        let length = data.len().min(MAX_FRAGMENT_SIZE);
        output.push(SecureSend {
            data_type,
            data: data[..length].to_vec(),
        });
        data = &data[length..];
    }
}

/// Split a DDC blob into its length-prefixed records.
pub fn ddc_records(mut data: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut records = Vec::new();
    while !data.is_empty() {
        let record_length = 1 + usize::from(data[0]);
        if record_length > data.len() {
            return Err(Error::InvalidFirmware(format!(
                "truncated Intel DDC record: needs {record_length} bytes, has {}",
                data.len()
            )));
        }
        records.push(data[..record_length].to_vec());
        data = &data[record_length..];
    }
    Ok(records)
}

pub fn write_device_config_command(record: Vec<u8>) -> Command {
    Command::Generic {
        op_code: HCI_INTEL_WRITE_DEVICE_CONFIG_COMMAND,
        parameters: record,
    }
}

pub fn reset_command(
    reset_type: u8,
    patch_enable: u8,
    ddc_reload: u8,
    boot_option: u8,
    boot_address: u32,
) -> Command {
    let mut parameters = vec![reset_type, patch_enable, ddc_reload, boot_option];
    parameters.extend_from_slice(&boot_address.to_le_bytes());
    Command::Generic {
        op_code: HCI_INTEL_RESET_COMMAND,
        parameters,
    }
}

pub struct Driver {
    options: DriverOptions,
}

impl Driver {
    pub fn for_host(host: &impl DriverHost, force: bool) -> Result<Option<Self>> {
        if !force && !check(host.metadata()) {
            return Ok(None);
        }
        Ok(Some(Self {
            options: DriverOptions::parse(host.metadata().get("driver").map(String::as_str))?,
        }))
    }

    pub fn options(&self) -> &DriverOptions {
        &self.options
    }

    pub fn read_device_info(&self, host: &mut impl DriverHost) -> Result<DeviceInfo> {
        let reset = host.transact(Command::Reset)?;
        match reset.status() {
            Some(0x00 | 0x01) => {}
            Some(status) => {
                return Err(Error::InvalidResponse(format!(
                    "Intel HCI reset returned status 0x{status:02X}"
                )))
            }
            None => {
                return Err(Error::InvalidResponse(
                    "Intel HCI reset returned no status".into(),
                ))
            }
        }

        let response = host.transact(Command::Generic {
            op_code: HCI_INTEL_READ_VERSION_COMMAND,
            parameters: vec![0xFF],
        })?;
        require_success(&response, "Intel read version")?;
        device_info_from_tlv(&response.return_parameters[1..])
    }

    pub fn init_controller(
        &self,
        host: &mut impl DriverHost,
        firmware: &impl FirmwareProvider,
    ) -> Result<InitOutcome> {
        let device_info = self.read_device_info(host)?;
        if device_info
            .get(&ValueType::CURRENT_MODE_OF_OPERATION)
            .and_then(Value::as_u8)
            == Some(ModeOfOperation::Operational as u8)
        {
            self.load_ddc(host, firmware, None)?;
            return Ok(InitOutcome::AlreadyOperational);
        }

        let hardware = match device_info.get(&ValueType::HARDWARE_INFO) {
            Some(Value::HardwareInfo(hardware)) => *hardware,
            _ => {
                return Err(Error::InvalidResponse(
                    "Intel device info is missing hardware info".into(),
                ))
            }
        };
        if hardware.platform != 0x37 {
            return Err(Error::Unsupported(format!(
                "Intel hardware platform 0x{:02X}",
                hardware.platform
            )));
        }
        if !matches!(hardware.variant, 0x17 | 0x19 | 0x1C) {
            return Err(Error::Unsupported(format!(
                "Intel hardware variant 0x{:02X}",
                hardware.variant
            )));
        }

        let base_name = firmware_base_name(&device_info)?;
        let firmware_name = format!("{base_name}.sfi");
        let image = firmware
            .load(&firmware_name)?
            .ok_or_else(|| Error::MissingFirmware(firmware_name.clone()))?;
        let engine = device_info
            .get(&ValueType::SECURE_BOOT_ENGINE_TYPE)
            .and_then(Value::as_u8)
            .ok_or_else(|| {
                Error::InvalidResponse("Intel device info is missing secure boot engine".into())
            })?
            .try_into()?;
        let plan = FirmwarePlan::parse(&image, engine)?;

        let responses = host.transact_batch(plan.commands())?;
        if responses.len() != plan.secure_sends.len() {
            return Err(Error::InvalidResponse(format!(
                "Intel secure-send batch returned {} completions for {} commands",
                responses.len(),
                plan.secure_sends.len()
            )));
        }
        for response in &responses {
            require_success(response, "Intel secure send")?;
        }
        let _ = host.wait_vendor_event(0x06)?;

        host.send_without_response(reset_command(0x00, 0x01, 0x00, 0x01, plan.boot_address))?;
        let _ = host.wait_vendor_event(0x02)?;
        self.load_ddc(host, firmware, Some(&base_name))?;

        Ok(InitOutcome::FirmwareLoaded {
            firmware_name,
            boot_address: plan.boot_address,
            secure_send_count: plan.secure_sends.len(),
        })
    }

    fn load_ddc(
        &self,
        host: &mut impl DriverHost,
        firmware: &impl FirmwareProvider,
        base_name: Option<&str>,
    ) -> Result<()> {
        if let Some(override_data) = &self.options.ddc_override {
            load_device_config(host, override_data)?;
        } else if let Some(base_name) = base_name {
            if let Some(data) = firmware.load(&format!("{base_name}.ddc"))? {
                load_device_config(host, &data)?;
            }
        }
        if let Some(addon) = &self.options.ddc_addon {
            load_device_config(host, addon)?;
        }
        Ok(())
    }
}

fn load_device_config(host: &mut impl DriverHost, data: &[u8]) -> Result<()> {
    for record in ddc_records(data)? {
        let response = host.transact(write_device_config_command(record))?;
        require_success(&response, "Intel write device config")?;
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InitOutcome {
    AlreadyOperational,
    FirmwareLoaded {
        firmware_name: String,
        boot_address: u32,
        secure_send_count: usize,
    },
}

/// Raw HCI reset opcode, exported for scripted driver-host implementations.
pub const STANDARD_RESET_OPCODE: u16 = HCI_RESET_COMMAND;
