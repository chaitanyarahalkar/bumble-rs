//! HCI Command packets (Vol 2, Part E - 5.4.1).
//!
//! Wire form: `[0x01, op_code_lo, op_code_hi, param_len, parameters…]`.
//! Ported from `bumble.hci.HCI_Command` and the specific command classes.

use crate::codes::*;
use crate::{Error, Reader, Result};
use bumble::{Address, AddressType};

/// A codec identifier (Coding Format), 5 bytes on the wire:
/// `coding_format(1) + company_id(2 LE) + vendor_specific_codec_id(2 LE)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodingFormat {
    pub coding_format: u8,
    pub company_id: u16,
    pub vendor_specific_codec_id: u16,
}

impl CodingFormat {
    /// `CodecID::TRANSPARENT` (0x03) with no company/vendor id.
    pub const TRANSPARENT: CodingFormat = CodingFormat {
        coding_format: 0x03,
        company_id: 0,
        vendor_specific_codec_id: 0,
    };

    fn to_bytes(self) -> [u8; 5] {
        let mut out = [0u8; 5];
        out[0] = self.coding_format;
        out[1..3].copy_from_slice(&self.company_id.to_le_bytes());
        out[3..5].copy_from_slice(&self.vendor_specific_codec_id.to_le_bytes());
        out
    }

    fn read(r: &mut Reader) -> Result<CodingFormat> {
        Ok(CodingFormat {
            coding_format: r.u8()?,
            company_id: r.u16_le()?,
            vendor_specific_codec_id: r.u16_le()?,
        })
    }
}

/// An HCI command. Typed variants carry decoded fields; [`Command::Generic`]
/// preserves the raw parameters for op codes this slice does not yet decode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Reset,
    Disconnect {
        connection_handle: u16,
        reason: u8,
    },
    PinCodeRequestReply {
        bd_addr: Address,
        pin_code_length: u8,
        pin_code: [u8; 16],
    },
    SetEventMask {
        event_mask: [u8; 8],
    },
    LeSetEventMask {
        le_event_mask: [u8; 8],
    },
    LeSetRandomAddress {
        random_address: Address,
    },
    LeSetAdvertisingParameters {
        advertising_interval_min: u16,
        advertising_interval_max: u16,
        advertising_type: u8,
        own_address_type: u8,
        peer_address_type: u8,
        peer_address: Address,
        advertising_channel_map: u8,
        advertising_filter_policy: u8,
    },
    LeSetAdvertisingData {
        /// The meaningful advertising bytes (serialized into a fixed 31-byte
        /// field, zero-padded, preceded by a length byte).
        advertising_data: Vec<u8>,
    },
    LeSetAdvertisingEnable {
        advertising_enable: u8,
    },
    LeSetScanParameters {
        le_scan_type: u8,
        le_scan_interval: u16,
        le_scan_window: u16,
        own_address_type: u8,
        scanning_filter_policy: u8,
    },
    LeSetScanEnable {
        le_scan_enable: u8,
        filter_duplicates: u8,
    },
    LeCreateConnection {
        le_scan_interval: u16,
        le_scan_window: u16,
        initiator_filter_policy: u8,
        peer_address_type: u8,
        peer_address: Address,
        own_address_type: u8,
        connection_interval_min: u16,
        connection_interval_max: u16,
        max_latency: u16,
        supervision_timeout: u16,
        min_ce_length: u16,
        max_ce_length: u16,
    },
    LeAddDeviceToFilterAcceptList {
        address_type: u8,
        address: Address,
    },
    LeRemoveDeviceFromFilterAcceptList {
        address_type: u8,
        address: Address,
    },
    LeConnectionUpdate {
        connection_handle: u16,
        connection_interval_min: u16,
        connection_interval_max: u16,
        max_latency: u16,
        supervision_timeout: u16,
        min_ce_length: u16,
        max_ce_length: u16,
    },
    LeReadRemoteFeatures {
        connection_handle: u16,
    },
    LeSetDefaultPhy {
        all_phys: u8,
        tx_phys: u8,
        rx_phys: u8,
    },
    /// Per-advertising-set arrays; the count is the leading `num_sets` byte.
    LeSetExtendedAdvertisingEnable {
        enable: u8,
        advertising_handles: Vec<u8>,
        durations: Vec<u16>,
        max_extended_advertising_events: Vec<u8>,
    },
    /// Per-PHY arrays; the count is `scanning_phys.count_ones()`.
    LeSetExtendedScanParameters {
        own_address_type: u8,
        scanning_filter_policy: u8,
        scanning_phys: u8,
        scan_types: Vec<u8>,
        scan_intervals: Vec<u16>,
        scan_windows: Vec<u16>,
    },
    /// Per-PHY arrays; the count is `initiating_phys.count_ones()`.
    LeExtendedCreateConnection {
        initiator_filter_policy: u8,
        own_address_type: u8,
        peer_address_type: u8,
        peer_address: Address,
        initiating_phys: u8,
        scan_intervals: Vec<u16>,
        scan_windows: Vec<u16>,
        connection_interval_mins: Vec<u16>,
        connection_interval_maxs: Vec<u16>,
        max_latencies: Vec<u16>,
        supervision_timeouts: Vec<u16>,
        min_ce_lengths: Vec<u16>,
        max_ce_lengths: Vec<u16>,
    },
    LeSetupIsoDataPath {
        connection_handle: u16,
        data_path_direction: u8,
        data_path_id: u8,
        codec_id: CodingFormat,
        controller_delay: u32,
        codec_configuration: Vec<u8>,
    },
    ReadLocalVersionInformation,
    ReadLocalSupportedCommands,
    ReadLocalSupportedFeatures,
    /// Any command not decoded by this slice: raw op code + parameters.
    Generic {
        op_code: u16,
        parameters: Vec<u8>,
    },
}

// Small serialization helpers.
fn push_u16(p: &mut Vec<u8>, v: u16) {
    p.extend_from_slice(&v.to_le_bytes());
}
fn push_u24(p: &mut Vec<u8>, v: u32) {
    p.extend_from_slice(&v.to_le_bytes()[..3]);
}

impl Command {
    /// The 16-bit op code for this command.
    pub fn op_code(&self) -> u16 {
        match self {
            Command::Reset => HCI_RESET_COMMAND,
            Command::Disconnect { .. } => HCI_DISCONNECT_COMMAND,
            Command::PinCodeRequestReply { .. } => HCI_PIN_CODE_REQUEST_REPLY_COMMAND,
            Command::SetEventMask { .. } => HCI_SET_EVENT_MASK_COMMAND,
            Command::LeSetEventMask { .. } => HCI_LE_SET_EVENT_MASK_COMMAND,
            Command::LeSetRandomAddress { .. } => HCI_LE_SET_RANDOM_ADDRESS_COMMAND,
            Command::LeSetAdvertisingParameters { .. } => HCI_LE_SET_ADVERTISING_PARAMETERS_COMMAND,
            Command::LeSetAdvertisingData { .. } => HCI_LE_SET_ADVERTISING_DATA_COMMAND,
            Command::LeSetAdvertisingEnable { .. } => HCI_LE_SET_ADVERTISING_ENABLE_COMMAND,
            Command::LeSetScanParameters { .. } => HCI_LE_SET_SCAN_PARAMETERS_COMMAND,
            Command::LeSetScanEnable { .. } => HCI_LE_SET_SCAN_ENABLE_COMMAND,
            Command::LeCreateConnection { .. } => HCI_LE_CREATE_CONNECTION_COMMAND,
            Command::LeAddDeviceToFilterAcceptList { .. } => {
                HCI_LE_ADD_DEVICE_TO_FILTER_ACCEPT_LIST_COMMAND
            }
            Command::LeRemoveDeviceFromFilterAcceptList { .. } => {
                HCI_LE_REMOVE_DEVICE_FROM_FILTER_ACCEPT_LIST_COMMAND
            }
            Command::LeConnectionUpdate { .. } => HCI_LE_CONNECTION_UPDATE_COMMAND,
            Command::LeReadRemoteFeatures { .. } => HCI_LE_READ_REMOTE_FEATURES_COMMAND,
            Command::LeSetDefaultPhy { .. } => HCI_LE_SET_DEFAULT_PHY_COMMAND,
            Command::LeSetExtendedAdvertisingEnable { .. } => {
                HCI_LE_SET_EXTENDED_ADVERTISING_ENABLE_COMMAND
            }
            Command::LeSetExtendedScanParameters { .. } => {
                HCI_LE_SET_EXTENDED_SCAN_PARAMETERS_COMMAND
            }
            Command::LeExtendedCreateConnection { .. } => HCI_LE_EXTENDED_CREATE_CONNECTION_COMMAND,
            Command::LeSetupIsoDataPath { .. } => HCI_LE_SETUP_ISO_DATA_PATH_COMMAND,
            Command::ReadLocalVersionInformation => HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND,
            Command::ReadLocalSupportedCommands => HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND,
            Command::ReadLocalSupportedFeatures => HCI_READ_LOCAL_SUPPORTED_FEATURES_COMMAND,
            Command::Generic { op_code, .. } => *op_code,
        }
    }

    /// The serialized command parameters (without the packet/op-code header).
    pub fn parameters(&self) -> Vec<u8> {
        let mut p = Vec::new();
        match self {
            Command::Reset
            | Command::ReadLocalVersionInformation
            | Command::ReadLocalSupportedCommands
            | Command::ReadLocalSupportedFeatures => {}
            Command::Disconnect {
                connection_handle,
                reason,
            } => {
                push_u16(&mut p, *connection_handle);
                p.push(*reason);
            }
            Command::PinCodeRequestReply {
                bd_addr,
                pin_code_length,
                pin_code,
            } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.push(*pin_code_length);
                p.extend_from_slice(pin_code);
            }
            Command::SetEventMask { event_mask } => p.extend_from_slice(event_mask),
            Command::LeSetEventMask { le_event_mask } => p.extend_from_slice(le_event_mask),
            Command::LeSetRandomAddress { random_address } => {
                p.extend_from_slice(random_address.address_bytes())
            }
            Command::LeSetAdvertisingParameters {
                advertising_interval_min,
                advertising_interval_max,
                advertising_type,
                own_address_type,
                peer_address_type,
                peer_address,
                advertising_channel_map,
                advertising_filter_policy,
            } => {
                push_u16(&mut p, *advertising_interval_min);
                push_u16(&mut p, *advertising_interval_max);
                p.push(*advertising_type);
                p.push(*own_address_type);
                p.push(*peer_address_type);
                p.extend_from_slice(peer_address.address_bytes());
                p.push(*advertising_channel_map);
                p.push(*advertising_filter_policy);
            }
            Command::LeSetAdvertisingData { advertising_data } => {
                // Advertising data is a fixed 31-byte field (Core Spec caps it
                // at 31); longer input would be silently truncated by the
                // resize below.
                debug_assert!(
                    advertising_data.len() <= 31,
                    "advertising_data exceeds the 31-byte field"
                );
                p.push(advertising_data.len() as u8);
                p.extend_from_slice(advertising_data);
                // Pad to the fixed 31-byte advertising-data field.
                p.resize(1 + 31, 0);
            }
            Command::LeSetAdvertisingEnable { advertising_enable } => p.push(*advertising_enable),
            Command::LeSetScanParameters {
                le_scan_type,
                le_scan_interval,
                le_scan_window,
                own_address_type,
                scanning_filter_policy,
            } => {
                p.push(*le_scan_type);
                push_u16(&mut p, *le_scan_interval);
                push_u16(&mut p, *le_scan_window);
                p.push(*own_address_type);
                p.push(*scanning_filter_policy);
            }
            Command::LeSetScanEnable {
                le_scan_enable,
                filter_duplicates,
            } => {
                p.push(*le_scan_enable);
                p.push(*filter_duplicates);
            }
            Command::LeCreateConnection {
                le_scan_interval,
                le_scan_window,
                initiator_filter_policy,
                peer_address_type,
                peer_address,
                own_address_type,
                connection_interval_min,
                connection_interval_max,
                max_latency,
                supervision_timeout,
                min_ce_length,
                max_ce_length,
            } => {
                push_u16(&mut p, *le_scan_interval);
                push_u16(&mut p, *le_scan_window);
                p.push(*initiator_filter_policy);
                p.push(*peer_address_type);
                p.extend_from_slice(peer_address.address_bytes());
                p.push(*own_address_type);
                push_u16(&mut p, *connection_interval_min);
                push_u16(&mut p, *connection_interval_max);
                push_u16(&mut p, *max_latency);
                push_u16(&mut p, *supervision_timeout);
                push_u16(&mut p, *min_ce_length);
                push_u16(&mut p, *max_ce_length);
            }
            Command::LeAddDeviceToFilterAcceptList {
                address_type,
                address,
            }
            | Command::LeRemoveDeviceFromFilterAcceptList {
                address_type,
                address,
            } => {
                p.push(*address_type);
                p.extend_from_slice(address.address_bytes());
            }
            Command::LeConnectionUpdate {
                connection_handle,
                connection_interval_min,
                connection_interval_max,
                max_latency,
                supervision_timeout,
                min_ce_length,
                max_ce_length,
            } => {
                push_u16(&mut p, *connection_handle);
                push_u16(&mut p, *connection_interval_min);
                push_u16(&mut p, *connection_interval_max);
                push_u16(&mut p, *max_latency);
                push_u16(&mut p, *supervision_timeout);
                push_u16(&mut p, *min_ce_length);
                push_u16(&mut p, *max_ce_length);
            }
            Command::LeReadRemoteFeatures { connection_handle } => {
                push_u16(&mut p, *connection_handle)
            }
            Command::LeSetDefaultPhy {
                all_phys,
                tx_phys,
                rx_phys,
            } => {
                p.push(*all_phys);
                p.push(*tx_phys);
                p.push(*rx_phys);
            }
            Command::LeSetExtendedAdvertisingEnable {
                enable,
                advertising_handles,
                durations,
                max_extended_advertising_events,
            } => {
                p.push(*enable);
                p.push(advertising_handles.len() as u8);
                for i in 0..advertising_handles.len() {
                    p.push(advertising_handles[i]);
                    push_u16(&mut p, durations[i]);
                    p.push(max_extended_advertising_events[i]);
                }
            }
            Command::LeSetExtendedScanParameters {
                own_address_type,
                scanning_filter_policy,
                scanning_phys,
                scan_types,
                scan_intervals,
                scan_windows,
            } => {
                p.push(*own_address_type);
                p.push(*scanning_filter_policy);
                p.push(*scanning_phys);
                for i in 0..scan_types.len() {
                    p.push(scan_types[i]);
                    push_u16(&mut p, scan_intervals[i]);
                    push_u16(&mut p, scan_windows[i]);
                }
            }
            Command::LeExtendedCreateConnection {
                initiator_filter_policy,
                own_address_type,
                peer_address_type,
                peer_address,
                initiating_phys,
                scan_intervals,
                scan_windows,
                connection_interval_mins,
                connection_interval_maxs,
                max_latencies,
                supervision_timeouts,
                min_ce_lengths,
                max_ce_lengths,
            } => {
                p.push(*initiator_filter_policy);
                p.push(*own_address_type);
                p.push(*peer_address_type);
                p.extend_from_slice(peer_address.address_bytes());
                p.push(*initiating_phys);
                for i in 0..scan_intervals.len() {
                    push_u16(&mut p, scan_intervals[i]);
                    push_u16(&mut p, scan_windows[i]);
                    push_u16(&mut p, connection_interval_mins[i]);
                    push_u16(&mut p, connection_interval_maxs[i]);
                    push_u16(&mut p, max_latencies[i]);
                    push_u16(&mut p, supervision_timeouts[i]);
                    push_u16(&mut p, min_ce_lengths[i]);
                    push_u16(&mut p, max_ce_lengths[i]);
                }
            }
            Command::LeSetupIsoDataPath {
                connection_handle,
                data_path_direction,
                data_path_id,
                codec_id,
                controller_delay,
                codec_configuration,
            } => {
                push_u16(&mut p, *connection_handle);
                p.push(*data_path_direction);
                p.push(*data_path_id);
                p.extend_from_slice(&codec_id.to_bytes());
                push_u24(&mut p, *controller_delay);
                p.push(codec_configuration.len() as u8);
                p.extend_from_slice(codec_configuration);
            }
            Command::Generic { parameters, .. } => p.extend_from_slice(parameters),
        }
        p
    }

    /// Serialize to the full wire packet.
    pub fn to_bytes(&self) -> Vec<u8> {
        let params = self.parameters();
        let mut out = Vec::with_capacity(4 + params.len());
        out.push(HCI_COMMAND_PACKET);
        out.extend_from_slice(&self.op_code().to_le_bytes());
        out.push(params.len() as u8);
        out.extend_from_slice(&params);
        out
    }

    /// Parse a complete command packet (including the leading type byte).
    pub fn from_bytes(packet: &[u8]) -> Result<Command> {
        let mut r = Reader::new(packet, 1);
        let op_code = r.u16_le()?;
        let length = r.u8()? as usize;
        let parameters = r
            .take(length)
            .map_err(|_| Error::InvalidPacket("invalid packet length".into()))?;
        Command::from_parameters(op_code, parameters)
    }

    /// Build a typed command from its op code and raw parameters.
    pub fn from_parameters(op_code: u16, parameters: &[u8]) -> Result<Command> {
        // HCI address fields do not carry the address type on the wire; the type
        // is reconstructed as a random device address (it does not affect the
        // serialized form).
        let addr = |r: &mut Reader| -> Result<Address> {
            Ok(Address::from_bytes(
                r.array::<6>()?,
                AddressType::RANDOM_DEVICE,
            ))
        };

        let mut r = Reader::new(parameters, 0);
        Ok(match op_code {
            HCI_RESET_COMMAND => Command::Reset,
            HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND => Command::ReadLocalVersionInformation,
            HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND => Command::ReadLocalSupportedCommands,
            HCI_READ_LOCAL_SUPPORTED_FEATURES_COMMAND => Command::ReadLocalSupportedFeatures,
            HCI_DISCONNECT_COMMAND => Command::Disconnect {
                connection_handle: r.u16_le()?,
                reason: r.u8()?,
            },
            HCI_PIN_CODE_REQUEST_REPLY_COMMAND => Command::PinCodeRequestReply {
                bd_addr: addr(&mut r)?,
                pin_code_length: r.u8()?,
                pin_code: r.array::<16>()?,
            },
            HCI_SET_EVENT_MASK_COMMAND => Command::SetEventMask {
                event_mask: r.array::<8>()?,
            },
            HCI_LE_SET_EVENT_MASK_COMMAND => Command::LeSetEventMask {
                le_event_mask: r.array::<8>()?,
            },
            HCI_LE_SET_RANDOM_ADDRESS_COMMAND => Command::LeSetRandomAddress {
                random_address: addr(&mut r)?,
            },
            HCI_LE_SET_ADVERTISING_PARAMETERS_COMMAND => Command::LeSetAdvertisingParameters {
                advertising_interval_min: r.u16_le()?,
                advertising_interval_max: r.u16_le()?,
                advertising_type: r.u8()?,
                own_address_type: r.u8()?,
                peer_address_type: r.u8()?,
                peer_address: addr(&mut r)?,
                advertising_channel_map: r.u8()?,
                advertising_filter_policy: r.u8()?,
            },
            HCI_LE_SET_ADVERTISING_DATA_COMMAND => {
                let length = r.u8()? as usize;
                let field = r.array::<31>()?;
                Command::LeSetAdvertisingData {
                    advertising_data: field[..length.min(31)].to_vec(),
                }
            }
            HCI_LE_SET_ADVERTISING_ENABLE_COMMAND => Command::LeSetAdvertisingEnable {
                advertising_enable: r.u8()?,
            },
            HCI_LE_SET_SCAN_PARAMETERS_COMMAND => Command::LeSetScanParameters {
                le_scan_type: r.u8()?,
                le_scan_interval: r.u16_le()?,
                le_scan_window: r.u16_le()?,
                own_address_type: r.u8()?,
                scanning_filter_policy: r.u8()?,
            },
            HCI_LE_SET_SCAN_ENABLE_COMMAND => Command::LeSetScanEnable {
                le_scan_enable: r.u8()?,
                filter_duplicates: r.u8()?,
            },
            HCI_LE_CREATE_CONNECTION_COMMAND => Command::LeCreateConnection {
                le_scan_interval: r.u16_le()?,
                le_scan_window: r.u16_le()?,
                initiator_filter_policy: r.u8()?,
                peer_address_type: r.u8()?,
                peer_address: addr(&mut r)?,
                own_address_type: r.u8()?,
                connection_interval_min: r.u16_le()?,
                connection_interval_max: r.u16_le()?,
                max_latency: r.u16_le()?,
                supervision_timeout: r.u16_le()?,
                min_ce_length: r.u16_le()?,
                max_ce_length: r.u16_le()?,
            },
            HCI_LE_ADD_DEVICE_TO_FILTER_ACCEPT_LIST_COMMAND => {
                Command::LeAddDeviceToFilterAcceptList {
                    address_type: r.u8()?,
                    address: addr(&mut r)?,
                }
            }
            HCI_LE_REMOVE_DEVICE_FROM_FILTER_ACCEPT_LIST_COMMAND => {
                Command::LeRemoveDeviceFromFilterAcceptList {
                    address_type: r.u8()?,
                    address: addr(&mut r)?,
                }
            }
            HCI_LE_CONNECTION_UPDATE_COMMAND => Command::LeConnectionUpdate {
                connection_handle: r.u16_le()?,
                connection_interval_min: r.u16_le()?,
                connection_interval_max: r.u16_le()?,
                max_latency: r.u16_le()?,
                supervision_timeout: r.u16_le()?,
                min_ce_length: r.u16_le()?,
                max_ce_length: r.u16_le()?,
            },
            HCI_LE_READ_REMOTE_FEATURES_COMMAND => Command::LeReadRemoteFeatures {
                connection_handle: r.u16_le()?,
            },
            HCI_LE_SET_DEFAULT_PHY_COMMAND => Command::LeSetDefaultPhy {
                all_phys: r.u8()?,
                tx_phys: r.u8()?,
                rx_phys: r.u8()?,
            },
            HCI_LE_SET_EXTENDED_ADVERTISING_ENABLE_COMMAND => {
                let enable = r.u8()?;
                let num_sets = r.u8()? as usize;
                let mut advertising_handles = Vec::with_capacity(num_sets);
                let mut durations = Vec::with_capacity(num_sets);
                let mut max_extended_advertising_events = Vec::with_capacity(num_sets);
                for _ in 0..num_sets {
                    advertising_handles.push(r.u8()?);
                    durations.push(r.u16_le()?);
                    max_extended_advertising_events.push(r.u8()?);
                }
                Command::LeSetExtendedAdvertisingEnable {
                    enable,
                    advertising_handles,
                    durations,
                    max_extended_advertising_events,
                }
            }
            HCI_LE_SET_EXTENDED_SCAN_PARAMETERS_COMMAND => {
                let own_address_type = r.u8()?;
                let scanning_filter_policy = r.u8()?;
                let scanning_phys = r.u8()?;
                let n = scanning_phys.count_ones() as usize;
                let mut scan_types = Vec::with_capacity(n);
                let mut scan_intervals = Vec::with_capacity(n);
                let mut scan_windows = Vec::with_capacity(n);
                for _ in 0..n {
                    scan_types.push(r.u8()?);
                    scan_intervals.push(r.u16_le()?);
                    scan_windows.push(r.u16_le()?);
                }
                Command::LeSetExtendedScanParameters {
                    own_address_type,
                    scanning_filter_policy,
                    scanning_phys,
                    scan_types,
                    scan_intervals,
                    scan_windows,
                }
            }
            HCI_LE_EXTENDED_CREATE_CONNECTION_COMMAND => {
                let initiator_filter_policy = r.u8()?;
                let own_address_type = r.u8()?;
                let peer_address_type = r.u8()?;
                let peer_address = addr(&mut r)?;
                let initiating_phys = r.u8()?;
                let n = initiating_phys.count_ones() as usize;
                let mut scan_intervals = Vec::with_capacity(n);
                let mut scan_windows = Vec::with_capacity(n);
                let mut connection_interval_mins = Vec::with_capacity(n);
                let mut connection_interval_maxs = Vec::with_capacity(n);
                let mut max_latencies = Vec::with_capacity(n);
                let mut supervision_timeouts = Vec::with_capacity(n);
                let mut min_ce_lengths = Vec::with_capacity(n);
                let mut max_ce_lengths = Vec::with_capacity(n);
                for _ in 0..n {
                    scan_intervals.push(r.u16_le()?);
                    scan_windows.push(r.u16_le()?);
                    connection_interval_mins.push(r.u16_le()?);
                    connection_interval_maxs.push(r.u16_le()?);
                    max_latencies.push(r.u16_le()?);
                    supervision_timeouts.push(r.u16_le()?);
                    min_ce_lengths.push(r.u16_le()?);
                    max_ce_lengths.push(r.u16_le()?);
                }
                Command::LeExtendedCreateConnection {
                    initiator_filter_policy,
                    own_address_type,
                    peer_address_type,
                    peer_address,
                    initiating_phys,
                    scan_intervals,
                    scan_windows,
                    connection_interval_mins,
                    connection_interval_maxs,
                    max_latencies,
                    supervision_timeouts,
                    min_ce_lengths,
                    max_ce_lengths,
                }
            }
            HCI_LE_SETUP_ISO_DATA_PATH_COMMAND => {
                let connection_handle = r.u16_le()?;
                let data_path_direction = r.u8()?;
                let data_path_id = r.u8()?;
                let codec_id = CodingFormat::read(&mut r)?;
                let controller_delay = r.u24_le()?;
                let config_length = r.u8()? as usize;
                let codec_configuration = r.take(config_length)?.to_vec();
                Command::LeSetupIsoDataPath {
                    connection_handle,
                    data_path_direction,
                    data_path_id,
                    codec_id,
                    controller_delay,
                    codec_configuration,
                }
            }
            _ => Command::Generic {
                op_code,
                parameters: parameters.to_vec(),
            },
        })
    }
}
