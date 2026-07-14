//! HCI Command Complete return parameters.
//!
//! Ported from `bumble.hci` return-parameter classes. All typed return
//! parameters begin with a status byte; per `HCI_StatusReturnParameters`, when
//! the status is not SUCCESS the controller returns only the status and the
//! remaining fields are absent — so parsing falls back to [`ReturnParameters::Status`].

use crate::codes::*;
use crate::{Reader, Result};
use bumble::{Address, AddressType};

/// Decode a null-terminated UTF-8 string (mirrors
/// `bumble.hci.map_null_terminated_utf8_string`). Invalid UTF-8 is returned
/// lossily.
pub fn map_null_terminated_utf8_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// The return parameters carried by an HCI Command Complete event. Typed
/// variants decode known commands; [`ReturnParameters::Raw`] preserves the raw
/// bytes for commands this slice does not model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReturnParameters {
    /// Status-only (an error response, or a command whose only return
    /// parameter is a status).
    Status {
        status: u8,
    },
    LeReadBufferSize {
        status: u8,
        le_acl_data_packet_length: u16,
        total_num_le_acl_data_packets: u8,
    },
    LeReadBufferSizeV2 {
        status: u8,
        le_acl_data_packet_length: u16,
        total_num_le_acl_data_packets: u8,
        iso_data_packet_length: u16,
        total_num_iso_data_packets: u8,
    },
    ReadLocalVersionInformation {
        status: u8,
        hci_version: u8,
        hci_subversion: u16,
        lmp_version: u8,
        company_identifier: u16,
        lmp_subversion: u16,
    },
    ReadLocalSupportedCommands {
        status: u8,
        supported_commands: [u8; 64],
    },
    ReadLocalSupportedFeatures {
        status: u8,
        lmp_features: [u8; 8],
    },
    ReadLocalExtendedFeatures {
        status: u8,
        page_number: u8,
        maximum_page_number: u8,
        extended_lmp_features: [u8; 8],
    },
    LeReadLocalSupportedFeatures {
        status: u8,
        le_features: [u8; 8],
    },
    ReadBufferSize {
        status: u8,
        hc_acl_data_packet_length: u16,
        hc_synchronous_data_packet_length: u8,
        hc_total_num_acl_data_packets: u16,
        hc_total_num_synchronous_data_packets: u16,
    },
    ReadVoiceSetting {
        status: u8,
        voice_setting: u16,
    },
    ReadLoopbackMode {
        status: u8,
        loopback_mode: u8,
    },
    LeReadSuggestedDefaultDataLength {
        status: u8,
        suggested_max_tx_octets: u16,
        suggested_max_tx_time: u16,
    },
    LeReadMaximumDataLength {
        status: u8,
        supported_max_tx_octets: u16,
        supported_max_tx_time: u16,
        supported_max_rx_octets: u16,
        supported_max_rx_time: u16,
    },
    LeReadMaximumAdvertisingDataLength {
        status: u8,
        max_advertising_data_length: u16,
    },
    LeReadNumberOfSupportedAdvertisingSets {
        status: u8,
        num_supported_advertising_sets: u8,
    },
    LeReadMinimumSupportedConnectionInterval {
        status: u8,
        minimum_supported_connection_interval: u8,
        group_min: Vec<u16>,
        group_max: Vec<u16>,
        group_stride: Vec<u16>,
    },
    ReadBdAddr {
        status: u8,
        bd_addr: Address,
    },
    ReadLocalName {
        status: u8,
        /// The fixed 248-byte local name field (see
        /// [`map_null_terminated_utf8_string`]).
        local_name: Vec<u8>,
    },
    ReadLocalSupportedCodecs {
        status: u8,
        standard_codec_ids: Vec<u8>,
        vendor_specific_codec_ids: Vec<u32>,
    },
    ReadLocalSupportedCodecsV2 {
        status: u8,
        standard_codec_ids: Vec<u8>,
        standard_codec_transports: Vec<u8>,
        vendor_specific_codec_ids: Vec<u32>,
        vendor_specific_codec_transports: Vec<u8>,
    },
    Raw {
        data: Vec<u8>,
    },
}

impl ReturnParameters {
    /// The status byte (0 = SUCCESS), or `None` for [`ReturnParameters::Raw`].
    pub fn status(&self) -> Option<u8> {
        Some(match self {
            ReturnParameters::Status { status }
            | ReturnParameters::LeReadBufferSize { status, .. }
            | ReturnParameters::LeReadBufferSizeV2 { status, .. }
            | ReturnParameters::ReadLocalVersionInformation { status, .. }
            | ReturnParameters::ReadLocalSupportedCommands { status, .. }
            | ReturnParameters::ReadLocalSupportedFeatures { status, .. }
            | ReturnParameters::ReadLocalExtendedFeatures { status, .. }
            | ReturnParameters::LeReadLocalSupportedFeatures { status, .. }
            | ReturnParameters::ReadBufferSize { status, .. }
            | ReturnParameters::ReadVoiceSetting { status, .. }
            | ReturnParameters::ReadLoopbackMode { status, .. }
            | ReturnParameters::LeReadSuggestedDefaultDataLength { status, .. }
            | ReturnParameters::LeReadMaximumDataLength { status, .. }
            | ReturnParameters::LeReadMaximumAdvertisingDataLength { status, .. }
            | ReturnParameters::LeReadNumberOfSupportedAdvertisingSets { status, .. }
            | ReturnParameters::LeReadMinimumSupportedConnectionInterval { status, .. }
            | ReturnParameters::ReadBdAddr { status, .. }
            | ReturnParameters::ReadLocalName { status, .. }
            | ReturnParameters::ReadLocalSupportedCodecs { status, .. }
            | ReturnParameters::ReadLocalSupportedCodecsV2 { status, .. } => *status,
            ReturnParameters::Raw { .. } => return None,
        })
    }

    /// Serialize the return parameters.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut p = Vec::new();
        match self {
            ReturnParameters::Status { status } => p.push(*status),
            ReturnParameters::LeReadBufferSize {
                status,
                le_acl_data_packet_length,
                total_num_le_acl_data_packets,
            } => {
                p.push(*status);
                p.extend_from_slice(&le_acl_data_packet_length.to_le_bytes());
                p.push(*total_num_le_acl_data_packets);
            }
            ReturnParameters::LeReadBufferSizeV2 {
                status,
                le_acl_data_packet_length,
                total_num_le_acl_data_packets,
                iso_data_packet_length,
                total_num_iso_data_packets,
            } => {
                p.push(*status);
                p.extend_from_slice(&le_acl_data_packet_length.to_le_bytes());
                p.push(*total_num_le_acl_data_packets);
                p.extend_from_slice(&iso_data_packet_length.to_le_bytes());
                p.push(*total_num_iso_data_packets);
            }
            ReturnParameters::ReadLocalVersionInformation {
                status,
                hci_version,
                hci_subversion,
                lmp_version,
                company_identifier,
                lmp_subversion,
            } => {
                p.push(*status);
                p.push(*hci_version);
                p.extend_from_slice(&hci_subversion.to_le_bytes());
                p.push(*lmp_version);
                p.extend_from_slice(&company_identifier.to_le_bytes());
                p.extend_from_slice(&lmp_subversion.to_le_bytes());
            }
            ReturnParameters::ReadLocalSupportedCommands {
                status,
                supported_commands,
            } => {
                p.push(*status);
                p.extend_from_slice(supported_commands);
            }
            ReturnParameters::ReadLocalSupportedFeatures {
                status,
                lmp_features,
            } => {
                p.push(*status);
                p.extend_from_slice(lmp_features);
            }
            ReturnParameters::ReadLocalExtendedFeatures {
                status,
                page_number,
                maximum_page_number,
                extended_lmp_features,
            } => {
                p.push(*status);
                p.push(*page_number);
                p.push(*maximum_page_number);
                p.extend_from_slice(extended_lmp_features);
            }
            ReturnParameters::LeReadLocalSupportedFeatures {
                status,
                le_features,
            } => {
                p.push(*status);
                p.extend_from_slice(le_features);
            }
            ReturnParameters::ReadBufferSize {
                status,
                hc_acl_data_packet_length,
                hc_synchronous_data_packet_length,
                hc_total_num_acl_data_packets,
                hc_total_num_synchronous_data_packets,
            } => {
                p.push(*status);
                p.extend_from_slice(&hc_acl_data_packet_length.to_le_bytes());
                p.push(*hc_synchronous_data_packet_length);
                p.extend_from_slice(&hc_total_num_acl_data_packets.to_le_bytes());
                p.extend_from_slice(&hc_total_num_synchronous_data_packets.to_le_bytes());
            }
            ReturnParameters::ReadVoiceSetting {
                status,
                voice_setting,
            } => {
                p.push(*status);
                p.extend_from_slice(&voice_setting.to_le_bytes());
            }
            ReturnParameters::ReadLoopbackMode {
                status,
                loopback_mode,
            } => {
                p.push(*status);
                p.push(*loopback_mode);
            }
            ReturnParameters::LeReadSuggestedDefaultDataLength {
                status,
                suggested_max_tx_octets,
                suggested_max_tx_time,
            } => {
                p.push(*status);
                p.extend_from_slice(&suggested_max_tx_octets.to_le_bytes());
                p.extend_from_slice(&suggested_max_tx_time.to_le_bytes());
            }
            ReturnParameters::LeReadMaximumDataLength {
                status,
                supported_max_tx_octets,
                supported_max_tx_time,
                supported_max_rx_octets,
                supported_max_rx_time,
            } => {
                p.push(*status);
                p.extend_from_slice(&supported_max_tx_octets.to_le_bytes());
                p.extend_from_slice(&supported_max_tx_time.to_le_bytes());
                p.extend_from_slice(&supported_max_rx_octets.to_le_bytes());
                p.extend_from_slice(&supported_max_rx_time.to_le_bytes());
            }
            ReturnParameters::LeReadMaximumAdvertisingDataLength {
                status,
                max_advertising_data_length,
            } => {
                p.push(*status);
                p.extend_from_slice(&max_advertising_data_length.to_le_bytes());
            }
            ReturnParameters::LeReadNumberOfSupportedAdvertisingSets {
                status,
                num_supported_advertising_sets,
            } => {
                p.push(*status);
                p.push(*num_supported_advertising_sets);
            }
            ReturnParameters::LeReadMinimumSupportedConnectionInterval {
                status,
                minimum_supported_connection_interval,
                group_min,
                group_max,
                group_stride,
            } => {
                p.push(*status);
                p.push(*minimum_supported_connection_interval);
                let count = group_min
                    .len()
                    .min(group_max.len())
                    .min(group_stride.len())
                    .min(u8::MAX as usize);
                p.push(count as u8);
                for index in 0..count {
                    p.extend_from_slice(&group_min[index].to_le_bytes());
                    p.extend_from_slice(&group_max[index].to_le_bytes());
                    p.extend_from_slice(&group_stride[index].to_le_bytes());
                }
            }
            ReturnParameters::ReadBdAddr { status, bd_addr } => {
                p.push(*status);
                p.extend_from_slice(bd_addr.address_bytes());
            }
            ReturnParameters::ReadLocalName { status, local_name } => {
                p.push(*status);
                p.extend_from_slice(local_name);
            }
            ReturnParameters::ReadLocalSupportedCodecs {
                status,
                standard_codec_ids,
                vendor_specific_codec_ids,
            } => {
                p.push(*status);
                p.push(standard_codec_ids.len() as u8);
                p.extend_from_slice(standard_codec_ids);
                p.push(vendor_specific_codec_ids.len() as u8);
                for v in vendor_specific_codec_ids {
                    p.extend_from_slice(&v.to_le_bytes());
                }
            }
            ReturnParameters::ReadLocalSupportedCodecsV2 {
                status,
                standard_codec_ids,
                standard_codec_transports,
                vendor_specific_codec_ids,
                vendor_specific_codec_transports,
            } => {
                p.push(*status);
                p.push(standard_codec_ids.len() as u8);
                for i in 0..standard_codec_ids.len() {
                    p.push(standard_codec_ids[i]);
                    p.push(standard_codec_transports[i]);
                }
                p.push(vendor_specific_codec_ids.len() as u8);
                for i in 0..vendor_specific_codec_ids.len() {
                    p.extend_from_slice(&vendor_specific_codec_ids[i].to_le_bytes());
                    p.push(vendor_specific_codec_transports[i]);
                }
            }
            ReturnParameters::Raw { data } => p.extend_from_slice(data),
        }
        p
    }

    /// Parse return parameters for a given command op code.
    ///
    /// All typed parameters share the status-based fallback: a non-SUCCESS
    /// status yields [`ReturnParameters::Status`] without decoding further.
    pub fn parse(command_opcode: u16, data: &[u8]) -> Result<ReturnParameters> {
        // Commands whose only return parameter is a status.
        if command_opcode == HCI_RESET_COMMAND {
            return Ok(ReturnParameters::Status {
                status: first_status(data),
            });
        }

        let is_typed = matches!(
            command_opcode,
            HCI_LE_READ_BUFFER_SIZE_COMMAND
                | HCI_LE_READ_BUFFER_SIZE_V2_COMMAND
                | HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND
                | HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND
                | HCI_READ_LOCAL_SUPPORTED_FEATURES_COMMAND
                | HCI_READ_LOCAL_EXTENDED_FEATURES_COMMAND
                | HCI_LE_READ_LOCAL_SUPPORTED_FEATURES_COMMAND
                | HCI_READ_BUFFER_SIZE_COMMAND
                | HCI_READ_VOICE_SETTING_COMMAND
                | HCI_READ_LOOPBACK_MODE_COMMAND
                | HCI_LE_READ_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND
                | HCI_LE_READ_MAXIMUM_DATA_LENGTH_COMMAND
                | HCI_LE_READ_MAXIMUM_ADVERTISING_DATA_LENGTH_COMMAND
                | HCI_LE_READ_NUMBER_OF_SUPPORTED_ADVERTISING_SETS_COMMAND
                | HCI_LE_READ_MINIMUM_SUPPORTED_CONNECTION_INTERVAL_COMMAND
                | HCI_READ_BD_ADDR_COMMAND
                | HCI_READ_LOCAL_NAME_COMMAND
                | HCI_READ_LOCAL_SUPPORTED_CODECS_COMMAND
                | HCI_READ_LOCAL_SUPPORTED_CODECS_V2_COMMAND
        );
        if !is_typed {
            return Ok(ReturnParameters::Raw {
                data: data.to_vec(),
            });
        }

        // Typed: on a non-SUCCESS status the extra fields are absent.
        let status = first_status(data);
        if status != HCI_SUCCESS {
            return Ok(ReturnParameters::Status { status });
        }

        let mut r = Reader::new(data, 0);
        let status = r.u8()?;
        Ok(match command_opcode {
            HCI_LE_READ_BUFFER_SIZE_COMMAND => ReturnParameters::LeReadBufferSize {
                status,
                le_acl_data_packet_length: r.u16_le()?,
                total_num_le_acl_data_packets: r.u8()?,
            },
            HCI_LE_READ_BUFFER_SIZE_V2_COMMAND => ReturnParameters::LeReadBufferSizeV2 {
                status,
                le_acl_data_packet_length: r.u16_le()?,
                total_num_le_acl_data_packets: r.u8()?,
                iso_data_packet_length: r.u16_le()?,
                total_num_iso_data_packets: r.u8()?,
            },
            HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND => {
                ReturnParameters::ReadLocalVersionInformation {
                    status,
                    hci_version: r.u8()?,
                    hci_subversion: r.u16_le()?,
                    lmp_version: r.u8()?,
                    company_identifier: r.u16_le()?,
                    lmp_subversion: r.u16_le()?,
                }
            }
            HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND => {
                ReturnParameters::ReadLocalSupportedCommands {
                    status,
                    supported_commands: r.array::<64>()?,
                }
            }
            HCI_READ_LOCAL_SUPPORTED_FEATURES_COMMAND => {
                ReturnParameters::ReadLocalSupportedFeatures {
                    status,
                    lmp_features: r.array::<8>()?,
                }
            }
            HCI_READ_LOCAL_EXTENDED_FEATURES_COMMAND => {
                ReturnParameters::ReadLocalExtendedFeatures {
                    status,
                    page_number: r.u8()?,
                    maximum_page_number: r.u8()?,
                    extended_lmp_features: r.array::<8>()?,
                }
            }
            HCI_LE_READ_LOCAL_SUPPORTED_FEATURES_COMMAND => {
                ReturnParameters::LeReadLocalSupportedFeatures {
                    status,
                    le_features: r.array::<8>()?,
                }
            }
            HCI_READ_BUFFER_SIZE_COMMAND => ReturnParameters::ReadBufferSize {
                status,
                hc_acl_data_packet_length: r.u16_le()?,
                hc_synchronous_data_packet_length: r.u8()?,
                hc_total_num_acl_data_packets: r.u16_le()?,
                hc_total_num_synchronous_data_packets: r.u16_le()?,
            },
            HCI_READ_VOICE_SETTING_COMMAND => ReturnParameters::ReadVoiceSetting {
                status,
                voice_setting: r.u16_le()?,
            },
            HCI_READ_LOOPBACK_MODE_COMMAND => ReturnParameters::ReadLoopbackMode {
                status,
                loopback_mode: r.u8()?,
            },
            HCI_LE_READ_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND => {
                ReturnParameters::LeReadSuggestedDefaultDataLength {
                    status,
                    suggested_max_tx_octets: r.u16_le()?,
                    suggested_max_tx_time: r.u16_le()?,
                }
            }
            HCI_LE_READ_MAXIMUM_DATA_LENGTH_COMMAND => ReturnParameters::LeReadMaximumDataLength {
                status,
                supported_max_tx_octets: r.u16_le()?,
                supported_max_tx_time: r.u16_le()?,
                supported_max_rx_octets: r.u16_le()?,
                supported_max_rx_time: r.u16_le()?,
            },
            HCI_LE_READ_MAXIMUM_ADVERTISING_DATA_LENGTH_COMMAND => {
                ReturnParameters::LeReadMaximumAdvertisingDataLength {
                    status,
                    max_advertising_data_length: r.u16_le()?,
                }
            }
            HCI_LE_READ_NUMBER_OF_SUPPORTED_ADVERTISING_SETS_COMMAND => {
                ReturnParameters::LeReadNumberOfSupportedAdvertisingSets {
                    status,
                    num_supported_advertising_sets: r.u8()?,
                }
            }
            HCI_LE_READ_MINIMUM_SUPPORTED_CONNECTION_INTERVAL_COMMAND => {
                let minimum_supported_connection_interval = r.u8()?;
                let count = r.u8()? as usize;
                let mut group_min = Vec::with_capacity(count);
                let mut group_max = Vec::with_capacity(count);
                let mut group_stride = Vec::with_capacity(count);
                for _ in 0..count {
                    group_min.push(r.u16_le()?);
                    group_max.push(r.u16_le()?);
                    group_stride.push(r.u16_le()?);
                }
                ReturnParameters::LeReadMinimumSupportedConnectionInterval {
                    status,
                    minimum_supported_connection_interval,
                    group_min,
                    group_max,
                    group_stride,
                }
            }
            HCI_READ_BD_ADDR_COMMAND => ReturnParameters::ReadBdAddr {
                status,
                bd_addr: Address::from_bytes(r.array::<6>()?, AddressType::PUBLIC_DEVICE),
            },
            HCI_READ_LOCAL_NAME_COMMAND => ReturnParameters::ReadLocalName {
                status,
                local_name: r.take(248)?.to_vec(),
            },
            HCI_READ_LOCAL_SUPPORTED_CODECS_COMMAND => {
                let n_std = r.u8()? as usize;
                let standard_codec_ids = (0..n_std).map(|_| r.u8()).collect::<Result<Vec<_>>>()?;
                let n_vendor = r.u8()? as usize;
                let vendor_specific_codec_ids = (0..n_vendor)
                    .map(|_| r.u32_le())
                    .collect::<Result<Vec<_>>>()?;
                ReturnParameters::ReadLocalSupportedCodecs {
                    status,
                    standard_codec_ids,
                    vendor_specific_codec_ids,
                }
            }
            HCI_READ_LOCAL_SUPPORTED_CODECS_V2_COMMAND => {
                let n_std = r.u8()? as usize;
                let mut standard_codec_ids = Vec::with_capacity(n_std);
                let mut standard_codec_transports = Vec::with_capacity(n_std);
                for _ in 0..n_std {
                    standard_codec_ids.push(r.u8()?);
                    standard_codec_transports.push(r.u8()?);
                }
                let n_vendor = r.u8()? as usize;
                let mut vendor_specific_codec_ids = Vec::with_capacity(n_vendor);
                let mut vendor_specific_codec_transports = Vec::with_capacity(n_vendor);
                for _ in 0..n_vendor {
                    vendor_specific_codec_ids.push(r.u32_le()?);
                    vendor_specific_codec_transports.push(r.u8()?);
                }
                ReturnParameters::ReadLocalSupportedCodecsV2 {
                    status,
                    standard_codec_ids,
                    standard_codec_transports,
                    vendor_specific_codec_ids,
                    vendor_specific_codec_transports,
                }
            }
            _ => unreachable!("guarded by is_typed"),
        })
    }
}

/// The leading status byte, or SUCCESS if the buffer is empty.
fn first_status(data: &[u8]) -> u8 {
    data.first().copied().unwrap_or(HCI_SUCCESS)
}
