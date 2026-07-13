//! Android Bluetooth vendor-specific HCI definitions.

use super::{command, exact_length, vendor_command_op_code};
use crate::{Address, AddressType, Command, Error, Event, Reader, Result};

pub const HCI_LE_GET_VENDOR_CAPABILITIES_COMMAND: u16 = vendor_command_op_code(0x153);
pub const HCI_LE_APCF_COMMAND: u16 = vendor_command_op_code(0x157);
pub const HCI_GET_CONTROLLER_ACTIVITY_ENERGY_INFO_COMMAND: u16 = vendor_command_op_code(0x159);
pub const HCI_A2DP_HARDWARE_OFFLOAD_COMMAND: u16 = vendor_command_op_code(0x15D);
pub const HCI_BLUETOOTH_QUALITY_REPORT_COMMAND: u16 = vendor_command_op_code(0x15E);
pub const HCI_DYNAMIC_AUDIO_BUFFER_COMMAND: u16 = vendor_command_op_code(0x15F);

pub const HCI_BLUETOOTH_QUALITY_REPORT_EVENT: u8 = 0x58;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeGetVendorCapabilitiesCommand;

impl LeGetVendorCapabilitiesCommand {
    pub fn to_command(self) -> Command {
        command(HCI_LE_GET_VENDOR_CAPABILITIES_COMMAND, Vec::new())
    }
}

/// Version-extensible return parameters for LE Get Vendor Capabilities.
///
/// Android has extended this structure several times. Just like upstream,
/// fields not present in an older controller response remain zero.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LeGetVendorCapabilitiesReturnParameters {
    pub status: u8,
    pub max_advt_instances: u8,
    pub offloaded_resolution_of_private_address: u8,
    pub total_scan_results_storage: u16,
    pub max_irk_list_sz: u8,
    pub filtering_support: u8,
    pub max_filter: u8,
    pub activity_energy_info_support: u8,
    pub version_supported: u16,
    pub total_num_of_advt_tracked: u16,
    pub extended_scan_support: u8,
    pub debug_logging_supported: u8,
    pub le_address_generation_offloading_support: u8,
    pub a2dp_source_offload_capability_mask: u32,
    pub bluetooth_quality_report_support: u8,
    pub dynamic_audio_buffer_support: u32,
}

impl LeGetVendorCapabilitiesReturnParameters {
    pub fn parse(data: &[u8]) -> Self {
        let mut value = Self::default();
        let mut offset = 0;
        macro_rules! next_u8 {
            ($field:ident) => {
                if let Some(field) = data.get(offset) {
                    value.$field = *field;
                    offset += 1;
                } else {
                    return value;
                }
            };
        }
        macro_rules! next_u16 {
            ($field:ident) => {
                if let Some(field) = data.get(offset..offset + 2) {
                    value.$field = u16::from_le_bytes([field[0], field[1]]);
                    offset += 2;
                } else {
                    return value;
                }
            };
        }
        macro_rules! next_u32 {
            ($field:ident) => {
                if let Some(field) = data.get(offset..offset + 4) {
                    value.$field = u32::from_le_bytes([field[0], field[1], field[2], field[3]]);
                    offset += 4;
                } else {
                    return value;
                }
            };
        }

        next_u8!(status);
        next_u8!(max_advt_instances);
        next_u8!(offloaded_resolution_of_private_address);
        next_u16!(total_scan_results_storage);
        next_u8!(max_irk_list_sz);
        next_u8!(filtering_support);
        next_u8!(max_filter);
        next_u8!(activity_energy_info_support);
        next_u16!(version_supported);
        next_u16!(total_num_of_advt_tracked);
        next_u8!(extended_scan_support);
        next_u8!(debug_logging_supported);
        next_u8!(le_address_generation_offloading_support);
        next_u32!(a2dp_source_offload_capability_mask);
        next_u8!(bluetooth_quality_report_support);
        next_u32!(dynamic_audio_buffer_support);
        let _ = offset;
        value
    }
}

/// Open Android APCF subcommand value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeApcfOpcode(pub u8);

impl LeApcfOpcode {
    pub const ENABLE: Self = Self(0x00);
    pub const SET_FILTERING_PARAMETERS: Self = Self(0x01);
    pub const BROADCASTER_ADDRESS: Self = Self(0x02);
    pub const SERVICE_UUID: Self = Self(0x03);
    pub const SERVICE_SOLICITATION_UUID: Self = Self(0x04);
    pub const LOCAL_NAME: Self = Self(0x05);
    pub const MANUFACTURER_DATA: Self = Self(0x06);
    pub const SERVICE_DATA: Self = Self(0x07);
    pub const TRANSPORT_DISCOVERY_SERVICE: Self = Self(0x08);
    pub const AD_TYPE_FILTER: Self = Self(0x09);
    pub const READ_EXTENDED_FEATURES: Self = Self(0xFF);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeApcfCommand {
    pub opcode: LeApcfOpcode,
    pub payload: Vec<u8>,
}

impl LeApcfCommand {
    pub fn to_command(&self) -> Command {
        let mut parameters = Vec::with_capacity(1 + self.payload.len());
        parameters.push(self.opcode.0);
        parameters.extend_from_slice(&self.payload);
        command(HCI_LE_APCF_COMMAND, parameters)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeApcfReturnParameters {
    pub status: u8,
    pub opcode: LeApcfOpcode,
    pub payload: Vec<u8>,
}

impl LeApcfReturnParameters {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 2 {
            return Err(Error::InvalidPacket(
                "Android APCF return parameters are truncated".into(),
            ));
        }
        Ok(Self {
            status: data[0],
            opcode: LeApcfOpcode(data[1]),
            payload: data[2..].to_vec(),
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GetControllerActivityEnergyInfoCommand;

impl GetControllerActivityEnergyInfoCommand {
    pub fn to_command(self) -> Command {
        command(HCI_GET_CONTROLLER_ACTIVITY_ENERGY_INFO_COMMAND, Vec::new())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GetControllerActivityEnergyInfoReturnParameters {
    pub status: u8,
    pub total_tx_time_ms: u32,
    pub total_rx_time_ms: u32,
    pub total_idle_time_ms: u32,
    pub total_energy_used: u32,
}

impl GetControllerActivityEnergyInfoReturnParameters {
    pub fn parse(data: &[u8]) -> Result<Self> {
        exact_length(data, 17, "Android controller energy return parameters")?;
        let mut reader = Reader::new(data, 0);
        Ok(Self {
            status: reader.u8()?,
            total_tx_time_ms: reader.u32_le()?,
            total_rx_time_ms: reader.u32_le()?,
            total_idle_time_ms: reader.u32_le()?,
            total_energy_used: reader.u32_le()?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct A2dpHardwareOffloadOpcode(pub u8);

impl A2dpHardwareOffloadOpcode {
    pub const START_A2DP_OFFLOAD: Self = Self(0x01);
    pub const STOP_A2DP_OFFLOAD: Self = Self(0x02);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct A2dpHardwareOffloadCommand {
    pub opcode: A2dpHardwareOffloadOpcode,
    pub payload: Vec<u8>,
}

impl A2dpHardwareOffloadCommand {
    pub fn to_command(&self) -> Command {
        let mut parameters = Vec::with_capacity(1 + self.payload.len());
        parameters.push(self.opcode.0);
        parameters.extend_from_slice(&self.payload);
        command(HCI_A2DP_HARDWARE_OFFLOAD_COMMAND, parameters)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct A2dpHardwareOffloadReturnParameters {
    pub status: u8,
    pub opcode: A2dpHardwareOffloadOpcode,
    pub payload: Vec<u8>,
}

impl A2dpHardwareOffloadReturnParameters {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 2 {
            return Err(Error::InvalidPacket(
                "Android A2DP offload return parameters are truncated".into(),
            ));
        }
        Ok(Self {
            status: data[0],
            opcode: A2dpHardwareOffloadOpcode(data[1]),
            payload: data[2..].to_vec(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DynamicAudioBufferOpcode(pub u8);

impl DynamicAudioBufferOpcode {
    pub const GET_AUDIO_BUFFER_TIME_CAPABILITY: Self = Self(0x01);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicAudioBufferCommand {
    pub opcode: DynamicAudioBufferOpcode,
    pub payload: Vec<u8>,
}

impl DynamicAudioBufferCommand {
    pub fn to_command(&self) -> Command {
        let mut parameters = Vec::with_capacity(1 + self.payload.len());
        parameters.push(self.opcode.0);
        parameters.extend_from_slice(&self.payload);
        command(HCI_DYNAMIC_AUDIO_BUFFER_COMMAND, parameters)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicAudioBufferReturnParameters {
    pub status: u8,
    pub opcode: DynamicAudioBufferOpcode,
    pub payload: Vec<u8>,
}

impl DynamicAudioBufferReturnParameters {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 2 {
            return Err(Error::InvalidPacket(
                "Android dynamic audio buffer return parameters are truncated".into(),
            ));
        }
        Ok(Self {
            status: data[0],
            opcode: DynamicAudioBufferOpcode(data[1]),
            payload: data[2..].to_vec(),
        })
    }
}

/// Android Bluetooth Quality Report payload carried by HCI vendor event 0xFF.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BluetoothQualityReportEvent {
    pub quality_report_id: u8,
    pub packet_types: u8,
    pub connection_handle: u16,
    pub connection_role: u8,
    pub tx_power_level: i8,
    pub rssi: i8,
    pub snr: u8,
    pub unused_afh_channel_count: u8,
    pub afh_select_unideal_channel_count: u8,
    pub lsto: u16,
    pub connection_piconet_clock: u32,
    pub retransmission_count: u32,
    pub no_rx_count: u32,
    pub nak_count: u32,
    pub last_tx_ack_timestamp: u32,
    pub flow_off_count: u32,
    pub last_flow_on_timestamp: u32,
    pub buffer_overflow_bytes: u32,
    pub buffer_underflow_bytes: u32,
    pub bdaddr: Address,
    pub cal_failed_item_count: u8,
    pub tx_total_packets: u32,
    pub tx_unacked_packets: u32,
    pub tx_flushed_packets: u32,
    pub tx_last_subevent_packets: u32,
    pub crc_error_packets: u32,
    pub rx_duplicate_packets: u32,
    pub rx_unreceived_packets: u32,
    pub vendor_specific_parameters: Vec<u8>,
}

impl BluetoothQualityReportEvent {
    /// Decode a recognized BQR subevent. Unknown report IDs deliberately return
    /// `None`, matching Android's vendor-event factory behavior upstream.
    pub fn parse_vendor_parameters(parameters: &[u8]) -> Result<Option<Self>> {
        let Some((&subevent, rest)) = parameters.split_first() else {
            return Err(Error::InvalidPacket("empty Android vendor event".into()));
        };
        if subevent != HCI_BLUETOOTH_QUALITY_REPORT_EVENT {
            return Ok(None);
        }
        let Some(&quality_report_id) = rest.first() else {
            return Err(Error::InvalidPacket(
                "Android quality report ID is missing".into(),
            ));
        };
        if !matches!(quality_report_id, 0x01..=0x04 | 0x07..=0x09) {
            return Ok(None);
        }

        let mut reader = Reader::new(rest, 0);
        let value = Self {
            quality_report_id: reader.u8()?,
            packet_types: reader.u8()?,
            connection_handle: reader.u16_le()?,
            connection_role: reader.u8()?,
            tx_power_level: reader.u8()? as i8,
            rssi: reader.u8()? as i8,
            snr: reader.u8()?,
            unused_afh_channel_count: reader.u8()?,
            afh_select_unideal_channel_count: reader.u8()?,
            lsto: reader.u16_le()?,
            connection_piconet_clock: reader.u32_le()?,
            retransmission_count: reader.u32_le()?,
            no_rx_count: reader.u32_le()?,
            nak_count: reader.u32_le()?,
            last_tx_ack_timestamp: reader.u32_le()?,
            flow_off_count: reader.u32_le()?,
            last_flow_on_timestamp: reader.u32_le()?,
            buffer_overflow_bytes: reader.u32_le()?,
            buffer_underflow_bytes: reader.u32_le()?,
            bdaddr: Address::from_bytes(reader.array::<6>()?, AddressType::PUBLIC_DEVICE),
            cal_failed_item_count: reader.u8()?,
            tx_total_packets: reader.u32_le()?,
            tx_unacked_packets: reader.u32_le()?,
            tx_flushed_packets: reader.u32_le()?,
            tx_last_subevent_packets: reader.u32_le()?,
            crc_error_packets: reader.u32_le()?,
            rx_duplicate_packets: reader.u32_le()?,
            rx_unreceived_packets: reader.u32_le()?,
            vendor_specific_parameters: reader.rest().to_vec(),
        };
        Ok(Some(value))
    }

    pub fn from_event(event: &Event) -> Result<Option<Self>> {
        match event {
            Event::Vendor { data } => Self::parse_vendor_parameters(data),
            _ => Ok(None),
        }
    }
}
