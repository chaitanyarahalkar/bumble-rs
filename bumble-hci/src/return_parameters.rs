//! HCI Command Complete return parameters.
//!
//! Ported from `bumble.hci` return-parameter classes. All typed return
//! parameters begin with a status byte; per `HCI_StatusReturnParameters`, when
//! the status is not SUCCESS the parser does not decode any remaining fields
//! and falls back to [`ReturnParameters::Status`].

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
    StatusAndConnectionHandle {
        status: u8,
        connection_handle: u16,
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
    LeReadAllLocalSupportedFeatures {
        status: u8,
        max_page: u8,
        le_features: Box<[u8; 248]>,
    },
    LeCsReadLocalSupportedCapabilities {
        status: u8,
        num_config_supported: u8,
        max_consecutive_procedures_supported: u16,
        num_antennas_supported: u8,
        max_antenna_paths_supported: u8,
        roles_supported: u8,
        modes_supported: u8,
        rtt_capability: u8,
        rtt_aa_only_n: u8,
        rtt_sounding_n: u8,
        rtt_random_sequence_n: u8,
        nadm_sounding_capability: u16,
        nadm_random_capability: u16,
        cs_sync_phys_supported: u8,
        subfeatures_supported: u16,
        t_ip1_times_supported: u16,
        t_ip2_times_supported: u16,
        t_fcs_times_supported: u16,
        t_pm_times_supported: u16,
        t_sw_time_supported: u8,
        tx_snr_capability: u8,
    },
    ReadBufferSize {
        status: u8,
        hc_acl_data_packet_length: u16,
        hc_synchronous_data_packet_length: u8,
        hc_total_num_acl_data_packets: u16,
        hc_total_num_synchronous_data_packets: u16,
    },
    ReadClassOfDevice {
        status: u8,
        class_of_device: u32,
    },
    ReadSynchronousFlowControlEnable {
        status: u8,
        synchronous_flow_control_enable: u8,
    },
    ReadLeHostSupport {
        status: u8,
        le_supported_host: u8,
        unused: u8,
    },
    WriteAuthenticatedPayloadTimeout {
        status: u8,
        connection_handle: u16,
    },
    ReadVoiceSetting {
        status: u8,
        voice_setting: u16,
    },
    ReadRssi {
        status: u8,
        handle: u16,
        rssi: i8,
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
    LeReadAdvertisingPhysicalChannelTxPower {
        status: u8,
        tx_power_level: i8,
    },
    LeReadFilterAcceptListSize {
        status: u8,
        filter_accept_list_size: u8,
    },
    LeReadSupportedStates {
        status: u8,
        le_states: [u8; 8],
    },
    LeReadResolvingListSize {
        status: u8,
        resolving_list_size: u8,
    },
    LeReadPhy {
        status: u8,
        connection_handle: u16,
        tx_phy: u8,
        rx_phy: u8,
    },
    LeReadIsoTxSync {
        status: u8,
        connection_handle: u16,
        packet_sequence_number: u16,
        tx_time_stamp: u32,
        time_offset: u32,
    },
    LeRemoveCig {
        status: u8,
        cig_id: u8,
    },
    LeReadTransmitPower {
        status: u8,
        min_tx_power: u8,
        max_tx_power: u8,
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
            | ReturnParameters::StatusAndConnectionHandle { status, .. }
            | ReturnParameters::LeReadBufferSize { status, .. }
            | ReturnParameters::LeReadBufferSizeV2 { status, .. }
            | ReturnParameters::ReadLocalVersionInformation { status, .. }
            | ReturnParameters::ReadLocalSupportedCommands { status, .. }
            | ReturnParameters::ReadLocalSupportedFeatures { status, .. }
            | ReturnParameters::ReadLocalExtendedFeatures { status, .. }
            | ReturnParameters::LeReadLocalSupportedFeatures { status, .. }
            | ReturnParameters::LeReadAllLocalSupportedFeatures { status, .. }
            | ReturnParameters::LeCsReadLocalSupportedCapabilities { status, .. }
            | ReturnParameters::ReadBufferSize { status, .. }
            | ReturnParameters::ReadClassOfDevice { status, .. }
            | ReturnParameters::ReadSynchronousFlowControlEnable { status, .. }
            | ReturnParameters::ReadLeHostSupport { status, .. }
            | ReturnParameters::WriteAuthenticatedPayloadTimeout { status, .. }
            | ReturnParameters::ReadVoiceSetting { status, .. }
            | ReturnParameters::ReadRssi { status, .. }
            | ReturnParameters::ReadLoopbackMode { status, .. }
            | ReturnParameters::LeReadSuggestedDefaultDataLength { status, .. }
            | ReturnParameters::LeReadMaximumDataLength { status, .. }
            | ReturnParameters::LeReadMaximumAdvertisingDataLength { status, .. }
            | ReturnParameters::LeReadNumberOfSupportedAdvertisingSets { status, .. }
            | ReturnParameters::LeReadAdvertisingPhysicalChannelTxPower { status, .. }
            | ReturnParameters::LeReadFilterAcceptListSize { status, .. }
            | ReturnParameters::LeReadSupportedStates { status, .. }
            | ReturnParameters::LeReadResolvingListSize { status, .. }
            | ReturnParameters::LeReadPhy { status, .. }
            | ReturnParameters::LeReadIsoTxSync { status, .. }
            | ReturnParameters::LeRemoveCig { status, .. }
            | ReturnParameters::LeReadTransmitPower { status, .. }
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
            ReturnParameters::StatusAndConnectionHandle {
                status,
                connection_handle,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
            }
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
            ReturnParameters::LeReadAllLocalSupportedFeatures {
                status,
                max_page,
                le_features,
            } => {
                p.push(*status);
                p.push(*max_page);
                p.extend_from_slice(le_features.as_ref());
            }
            ReturnParameters::LeCsReadLocalSupportedCapabilities {
                status,
                num_config_supported,
                max_consecutive_procedures_supported,
                num_antennas_supported,
                max_antenna_paths_supported,
                roles_supported,
                modes_supported,
                rtt_capability,
                rtt_aa_only_n,
                rtt_sounding_n,
                rtt_random_sequence_n,
                nadm_sounding_capability,
                nadm_random_capability,
                cs_sync_phys_supported,
                subfeatures_supported,
                t_ip1_times_supported,
                t_ip2_times_supported,
                t_fcs_times_supported,
                t_pm_times_supported,
                t_sw_time_supported,
                tx_snr_capability,
            } => {
                p.push(*status);
                p.push(*num_config_supported);
                p.extend_from_slice(&max_consecutive_procedures_supported.to_le_bytes());
                p.push(*num_antennas_supported);
                p.push(*max_antenna_paths_supported);
                p.push(*roles_supported);
                p.push(*modes_supported);
                p.push(*rtt_capability);
                p.push(*rtt_aa_only_n);
                p.push(*rtt_sounding_n);
                p.push(*rtt_random_sequence_n);
                p.extend_from_slice(&nadm_sounding_capability.to_le_bytes());
                p.extend_from_slice(&nadm_random_capability.to_le_bytes());
                p.push(*cs_sync_phys_supported);
                p.extend_from_slice(&subfeatures_supported.to_le_bytes());
                p.extend_from_slice(&t_ip1_times_supported.to_le_bytes());
                p.extend_from_slice(&t_ip2_times_supported.to_le_bytes());
                p.extend_from_slice(&t_fcs_times_supported.to_le_bytes());
                p.extend_from_slice(&t_pm_times_supported.to_le_bytes());
                p.push(*t_sw_time_supported);
                p.push(*tx_snr_capability);
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
            ReturnParameters::ReadClassOfDevice {
                status,
                class_of_device,
            } => {
                p.push(*status);
                p.extend_from_slice(&class_of_device.to_le_bytes()[..3]);
            }
            ReturnParameters::ReadSynchronousFlowControlEnable {
                status,
                synchronous_flow_control_enable,
            } => {
                p.push(*status);
                p.push(*synchronous_flow_control_enable);
            }
            ReturnParameters::ReadLeHostSupport {
                status,
                le_supported_host,
                unused,
            } => {
                p.push(*status);
                p.push(*le_supported_host);
                p.push(*unused);
            }
            ReturnParameters::WriteAuthenticatedPayloadTimeout {
                status,
                connection_handle,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
            }
            ReturnParameters::ReadVoiceSetting {
                status,
                voice_setting,
            } => {
                p.push(*status);
                p.extend_from_slice(&voice_setting.to_le_bytes());
            }
            ReturnParameters::ReadRssi {
                status,
                handle,
                rssi,
            } => {
                p.push(*status);
                p.extend_from_slice(&handle.to_le_bytes());
                p.push(*rssi as u8);
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
            ReturnParameters::LeReadAdvertisingPhysicalChannelTxPower {
                status,
                tx_power_level,
            } => {
                p.push(*status);
                p.push(*tx_power_level as u8);
            }
            ReturnParameters::LeReadFilterAcceptListSize {
                status,
                filter_accept_list_size,
            } => {
                p.push(*status);
                p.push(*filter_accept_list_size);
            }
            ReturnParameters::LeReadSupportedStates { status, le_states } => {
                p.push(*status);
                p.extend_from_slice(le_states);
            }
            ReturnParameters::LeReadResolvingListSize {
                status,
                resolving_list_size,
            } => {
                p.push(*status);
                p.push(*resolving_list_size);
            }
            ReturnParameters::LeReadPhy {
                status,
                connection_handle,
                tx_phy,
                rx_phy,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*tx_phy);
                p.push(*rx_phy);
            }
            ReturnParameters::LeReadIsoTxSync {
                status,
                connection_handle,
                packet_sequence_number,
                tx_time_stamp,
                time_offset,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(&packet_sequence_number.to_le_bytes());
                p.extend_from_slice(&tx_time_stamp.to_le_bytes());
                p.extend_from_slice(&time_offset.to_le_bytes());
            }
            ReturnParameters::LeRemoveCig { status, cig_id } => {
                p.push(*status);
                p.push(*cig_id);
            }
            ReturnParameters::LeReadTransmitPower {
                status,
                min_tx_power,
                max_tx_power,
            } => {
                p.push(*status);
                p.push(*min_tx_power);
                p.push(*max_tx_power);
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
                | HCI_LE_READ_ALL_LOCAL_SUPPORTED_FEATURES_COMMAND
                | HCI_LE_CS_READ_LOCAL_SUPPORTED_CAPABILITIES_COMMAND
                | HCI_READ_BUFFER_SIZE_COMMAND
                | HCI_READ_CLASS_OF_DEVICE_COMMAND
                | HCI_READ_SYNCHRONOUS_FLOW_CONTROL_ENABLE_COMMAND
                | HCI_READ_LE_HOST_SUPPORT_COMMAND
                | HCI_WRITE_AUTHENTICATED_PAYLOAD_TIMEOUT_COMMAND
                | HCI_READ_VOICE_SETTING_COMMAND
                | HCI_READ_RSSI_COMMAND
                | HCI_READ_LOOPBACK_MODE_COMMAND
                | HCI_LE_READ_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND
                | HCI_LE_READ_MAXIMUM_DATA_LENGTH_COMMAND
                | HCI_LE_READ_MAXIMUM_ADVERTISING_DATA_LENGTH_COMMAND
                | HCI_LE_READ_NUMBER_OF_SUPPORTED_ADVERTISING_SETS_COMMAND
                | HCI_LE_READ_ADVERTISING_PHYSICAL_CHANNEL_TX_POWER_COMMAND
                | HCI_LE_READ_FILTER_ACCEPT_LIST_SIZE_COMMAND
                | HCI_LE_READ_SUPPORTED_STATES_COMMAND
                | HCI_LE_READ_RESOLVING_LIST_SIZE_COMMAND
                | HCI_LE_READ_PHY_COMMAND
                | HCI_LE_READ_ISO_TX_SYNC_COMMAND
                | HCI_LE_REMOVE_CIG_COMMAND
                | HCI_LE_SETUP_ISO_DATA_PATH_COMMAND
                | HCI_LE_REMOVE_ISO_DATA_PATH_COMMAND
                | HCI_LE_READ_TRANSMIT_POWER_COMMAND
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

        // Typed: on a non-SUCCESS status upstream stops after the status even if
        // a producer included trailing command-specific fields.
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
            HCI_LE_READ_ALL_LOCAL_SUPPORTED_FEATURES_COMMAND => {
                ReturnParameters::LeReadAllLocalSupportedFeatures {
                    status,
                    max_page: r.u8()?,
                    le_features: Box::new(r.array::<248>()?),
                }
            }
            HCI_LE_CS_READ_LOCAL_SUPPORTED_CAPABILITIES_COMMAND => {
                ReturnParameters::LeCsReadLocalSupportedCapabilities {
                    status,
                    num_config_supported: r.u8()?,
                    max_consecutive_procedures_supported: r.u16_le()?,
                    num_antennas_supported: r.u8()?,
                    max_antenna_paths_supported: r.u8()?,
                    roles_supported: r.u8()?,
                    modes_supported: r.u8()?,
                    rtt_capability: r.u8()?,
                    rtt_aa_only_n: r.u8()?,
                    rtt_sounding_n: r.u8()?,
                    rtt_random_sequence_n: r.u8()?,
                    nadm_sounding_capability: r.u16_le()?,
                    nadm_random_capability: r.u16_le()?,
                    cs_sync_phys_supported: r.u8()?,
                    subfeatures_supported: r.u16_le()?,
                    t_ip1_times_supported: r.u16_le()?,
                    t_ip2_times_supported: r.u16_le()?,
                    t_fcs_times_supported: r.u16_le()?,
                    t_pm_times_supported: r.u16_le()?,
                    t_sw_time_supported: r.u8()?,
                    tx_snr_capability: r.u8()?,
                }
            }
            HCI_READ_BUFFER_SIZE_COMMAND => ReturnParameters::ReadBufferSize {
                status,
                hc_acl_data_packet_length: r.u16_le()?,
                hc_synchronous_data_packet_length: r.u8()?,
                hc_total_num_acl_data_packets: r.u16_le()?,
                hc_total_num_synchronous_data_packets: r.u16_le()?,
            },
            HCI_READ_CLASS_OF_DEVICE_COMMAND => ReturnParameters::ReadClassOfDevice {
                status,
                class_of_device: r.u24_le()?,
            },
            HCI_READ_SYNCHRONOUS_FLOW_CONTROL_ENABLE_COMMAND => {
                ReturnParameters::ReadSynchronousFlowControlEnable {
                    status,
                    synchronous_flow_control_enable: r.u8()?,
                }
            }
            HCI_READ_LE_HOST_SUPPORT_COMMAND => ReturnParameters::ReadLeHostSupport {
                status,
                le_supported_host: r.u8()?,
                unused: r.u8()?,
            },
            HCI_WRITE_AUTHENTICATED_PAYLOAD_TIMEOUT_COMMAND => {
                ReturnParameters::WriteAuthenticatedPayloadTimeout {
                    status,
                    connection_handle: r.u16_le()?,
                }
            }
            HCI_READ_VOICE_SETTING_COMMAND => ReturnParameters::ReadVoiceSetting {
                status,
                voice_setting: r.u16_le()?,
            },
            HCI_READ_RSSI_COMMAND => ReturnParameters::ReadRssi {
                status,
                handle: r.u16_le()?,
                rssi: r.u8()? as i8,
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
            HCI_LE_READ_ADVERTISING_PHYSICAL_CHANNEL_TX_POWER_COMMAND => {
                ReturnParameters::LeReadAdvertisingPhysicalChannelTxPower {
                    status,
                    tx_power_level: r.u8()? as i8,
                }
            }
            HCI_LE_READ_FILTER_ACCEPT_LIST_SIZE_COMMAND => {
                ReturnParameters::LeReadFilterAcceptListSize {
                    status,
                    filter_accept_list_size: r.u8()?,
                }
            }
            HCI_LE_READ_SUPPORTED_STATES_COMMAND => ReturnParameters::LeReadSupportedStates {
                status,
                le_states: r.array::<8>()?,
            },
            HCI_LE_READ_RESOLVING_LIST_SIZE_COMMAND => ReturnParameters::LeReadResolvingListSize {
                status,
                resolving_list_size: r.u8()?,
            },
            HCI_LE_READ_PHY_COMMAND => ReturnParameters::LeReadPhy {
                status,
                connection_handle: r.u16_le()?,
                tx_phy: r.u8()?,
                rx_phy: r.u8()?,
            },
            HCI_LE_READ_ISO_TX_SYNC_COMMAND => ReturnParameters::LeReadIsoTxSync {
                status,
                connection_handle: r.u16_le()?,
                packet_sequence_number: r.u16_le()?,
                tx_time_stamp: r.u32_le()?,
                time_offset: r.u32_le()?,
            },
            HCI_LE_SETUP_ISO_DATA_PATH_COMMAND | HCI_LE_REMOVE_ISO_DATA_PATH_COMMAND => {
                ReturnParameters::StatusAndConnectionHandle {
                    status,
                    connection_handle: r.u16_le()?,
                }
            }
            HCI_LE_REMOVE_CIG_COMMAND => ReturnParameters::LeRemoveCig {
                status,
                cig_id: r.u8()?,
            },
            HCI_LE_READ_TRANSMIT_POWER_COMMAND => ReturnParameters::LeReadTransmitPower {
                status,
                min_tx_power: r.u8()?,
                max_tx_power: r.u8()?,
            },
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
