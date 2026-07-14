//! HCI Event packets (Vol 2, Part E - 5.4.4), including LE Meta events.
//!
//! Wire form: `[0x04, event_code, param_len, parameters…]`. For LE Meta events
//! (`event_code == 0x3E`) the parameters begin with a sub-event code byte.
//!
//! The `Event` / `LeMetaEvent` enums are GENERATED from upstream `bumble.hci`
//! (see `tools/hcigen`). `Command_Complete` (typed return parameters) and the
//! two advertising-report events (nested report objects) are hand-written and
//! embedded by the generator.

use crate::codes::*;
use crate::return_parameters::ReturnParameters;
use crate::{Error, Reader, Result};
use bumble::{Address, AddressType};

/// An HCI event. Typed variants carry decoded fields; [`Event::Generic`]
/// preserves raw parameters for event codes with no typed model.
#[allow(clippy::large_enum_variant, clippy::enum_variant_names)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    InquiryComplete {
        status: u8,
    },
    InquiryResult {
        bd_addr: Vec<Address>,
        page_scan_repetition_mode: Vec<u8>,
        reserved_0: Vec<u8>,
        reserved_1: Vec<u8>,
        class_of_device: Vec<u32>,
        clock_offset: Vec<u16>,
    },
    ConnectionComplete {
        status: u8,
        connection_handle: u16,
        bd_addr: Address,
        link_type: u8,
        encryption_enabled: u8,
    },
    ConnectionRequest {
        bd_addr: Address,
        class_of_device: u32,
        link_type: u8,
    },
    DisconnectionComplete {
        status: u8,
        connection_handle: u16,
        reason: u8,
    },
    AuthenticationComplete {
        status: u8,
        connection_handle: u16,
    },
    RemoteNameRequestComplete {
        status: u8,
        bd_addr: Address,
        remote_name: [u8; 248],
    },
    EncryptionChange {
        status: u8,
        connection_handle: u16,
        encryption_enabled: u8,
    },
    ReadRemoteSupportedFeaturesComplete {
        status: u8,
        connection_handle: u16,
        lmp_features: [u8; 8],
    },
    ReadRemoteVersionInformationComplete {
        status: u8,
        connection_handle: u16,
        version: u8,
        manufacturer_name: u16,
        subversion: u16,
    },
    QosSetupComplete {
        status: u8,
        connection_handle: u16,
        unused: u8,
        service_type: u8,
    },
    CommandStatus {
        status: u8,
        num_hci_command_packets: u8,
        command_opcode: u16,
    },
    RoleChange {
        status: u8,
        bd_addr: Address,
        new_role: u8,
    },
    NumberOfCompletedPackets {
        connection_handles: Vec<u16>,
        num_completed_packets: Vec<u16>,
    },
    ModeChange {
        status: u8,
        connection_handle: u16,
        current_mode: u8,
        interval: u16,
    },
    PinCodeRequest {
        bd_addr: Address,
    },
    LinkKeyRequest {
        bd_addr: Address,
    },
    LinkKeyNotification {
        bd_addr: Address,
        link_key: [u8; 16],
        key_type: u8,
    },
    MaxSlotsChange {
        connection_handle: u16,
        lmp_max_slots: u8,
    },
    ReadClockOffsetComplete {
        status: u8,
        connection_handle: u16,
        clock_offset: u16,
    },
    ConnectionPacketTypeChanged {
        status: u8,
        connection_handle: u16,
        packet_type: u16,
    },
    PageScanRepetitionModeChange {
        bd_addr: Address,
        page_scan_repetition_mode: u8,
    },
    InquiryResultWithRssi {
        bd_addr: Vec<Address>,
        page_scan_repetition_mode: Vec<u8>,
        reserved: Vec<u8>,
        class_of_device: Vec<u32>,
        clock_offset: Vec<u16>,
        rssi: Vec<i8>,
    },
    ReadRemoteExtendedFeaturesComplete {
        status: u8,
        connection_handle: u16,
        page_number: u8,
        maximum_page_number: u8,
        extended_lmp_features: [u8; 8],
    },
    SynchronousConnectionComplete {
        status: u8,
        connection_handle: u16,
        bd_addr: Address,
        link_type: u8,
        transmission_interval: u8,
        retransmission_window: u8,
        rx_packet_length: u16,
        tx_packet_length: u16,
        air_mode: u8,
    },
    SynchronousConnectionChanged {
        status: u8,
        connection_handle: u16,
        transmission_interval: u8,
        retransmission_window: u8,
        rx_packet_length: u16,
        tx_packet_length: u16,
    },
    SniffSubrating {
        status: u8,
        connection_handle: u16,
        max_tx_latency: u16,
        max_rx_latency: u16,
        min_remote_timeout: u16,
        min_local_timeout: u16,
    },
    ExtendedInquiryResult {
        num_responses: u8,
        bd_addr: Address,
        page_scan_repetition_mode: u8,
        reserved: u8,
        class_of_device: u32,
        clock_offset: u16,
        rssi: i8,
        extended_inquiry_response: [u8; 240],
    },
    EncryptionKeyRefreshComplete {
        status: u8,
        connection_handle: u16,
    },
    IoCapabilityRequest {
        bd_addr: Address,
    },
    IoCapabilityResponse {
        bd_addr: Address,
        io_capability: u8,
        oob_data_present: u8,
        authentication_requirements: u8,
    },
    UserConfirmationRequest {
        bd_addr: Address,
        numeric_value: u32,
    },
    UserPasskeyRequest {
        bd_addr: Address,
    },
    RemoteOobDataRequest {
        bd_addr: Address,
    },
    SimplePairingComplete {
        status: u8,
        bd_addr: Address,
    },
    LinkSupervisionTimeoutChanged {
        connection_handle: u16,
        link_supervision_timeout: u16,
    },
    EnhancedFlushComplete {
        handle: u16,
    },
    UserPasskeyNotification {
        bd_addr: Address,
        passkey: u32,
    },
    KeypressNotification {
        bd_addr: Address,
        notification_type: u8,
    },
    RemoteHostSupportedFeaturesNotification {
        bd_addr: Address,
        host_supported_features: [u8; 8],
    },
    EncryptionChangeV2 {
        status: u8,
        connection_handle: u16,
        encryption_enabled: u8,
        encryption_key_size: u8,
    },
    Vendor {
        data: Vec<u8>,
    },
    CommandComplete {
        num_hci_command_packets: u8,
        command_opcode: u16,
        return_parameters: ReturnParameters,
    },
    LeMeta(LeMetaEvent),
    /// Any event with no typed model: raw event code + parameters.
    Generic {
        event_code: u8,
        parameters: Vec<u8>,
    },
}

/// One entry in an LE Advertising Report event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdvertisingReport {
    pub event_type: u8,
    pub address_type: u8,
    pub address: Address,
    pub data: Vec<u8>,
    pub rssi: i8,
}

/// One entry in an LE Extended Advertising Report event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtendedAdvertisingReport {
    pub event_type: u16,
    pub address_type: u8,
    pub address: Address,
    pub primary_phy: u8,
    pub secondary_phy: u8,
    pub advertising_sid: u8,
    pub tx_power: i8,
    pub rssi: i8,
    pub periodic_advertising_interval: u16,
    pub direct_address_type: u8,
    pub direct_address: Address,
    pub data: Vec<u8>,
}

/// An LE Meta sub-event.
#[allow(clippy::large_enum_variant, clippy::enum_variant_names)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeMetaEvent {
    ConnectionComplete {
        status: u8,
        connection_handle: u16,
        role: u8,
        peer_address_type: u8,
        peer_address: Address,
        connection_interval: u16,
        peripheral_latency: u16,
        supervision_timeout: u16,
        central_clock_accuracy: u8,
    },
    ConnectionUpdateComplete {
        status: u8,
        connection_handle: u16,
        connection_interval: u16,
        peripheral_latency: u16,
        supervision_timeout: u16,
    },
    ReadRemoteFeaturesComplete {
        status: u8,
        connection_handle: u16,
        le_features: [u8; 8],
    },
    LongTermKeyRequest {
        connection_handle: u16,
        random_number: [u8; 8],
        encryption_diversifier: u16,
    },
    RemoteConnectionParameterRequest {
        connection_handle: u16,
        interval_min: u16,
        interval_max: u16,
        max_latency: u16,
        timeout: u16,
    },
    DataLengthChange {
        connection_handle: u16,
        max_tx_octets: u16,
        max_tx_time: u16,
        max_rx_octets: u16,
        max_rx_time: u16,
    },
    EnhancedConnectionComplete {
        status: u8,
        connection_handle: u16,
        role: u8,
        peer_address_type: u8,
        peer_address: Address,
        local_resolvable_private_address: Address,
        peer_resolvable_private_address: Address,
        connection_interval: u16,
        peripheral_latency: u16,
        supervision_timeout: u16,
        central_clock_accuracy: u8,
    },
    PhyUpdateComplete {
        status: u8,
        connection_handle: u16,
        tx_phy: u8,
        rx_phy: u8,
    },
    PeriodicAdvertisingSyncEstablished {
        status: u8,
        sync_handle: u16,
        advertising_sid: u8,
        advertiser_address_type: u8,
        advertiser_address: Address,
        advertiser_phy: u8,
        periodic_advertising_interval: u16,
        advertiser_clock_accuracy: u8,
    },
    PeriodicAdvertisingReport {
        sync_handle: u16,
        tx_power: i8,
        rssi: i8,
        cte_type: u8,
        data_status: u8,
        data: Vec<u8>,
    },
    PeriodicAdvertisingSyncLost {
        sync_handle: u16,
    },
    AdvertisingSetTerminated {
        status: u8,
        advertising_handle: u8,
        connection_handle: u16,
        num_completed_extended_advertising_events: u8,
    },
    ChannelSelectionAlgorithm {
        connection_handle: u16,
        channel_selection_algorithm: u8,
    },
    PeriodicAdvertisingSyncTransferReceived {
        status: u8,
        connection_handle: u16,
        service_data: u16,
        sync_handle: u16,
        advertising_sid: u8,
        advertiser_address_type: u8,
        advertiser_address: Address,
        advertiser_phy: u8,
        periodic_advertising_interval: u16,
        advertiser_clock_accuracy: u8,
    },
    CisEstablished {
        status: u8,
        connection_handle: u16,
        cig_sync_delay: u32,
        cis_sync_delay: u32,
        transport_latency_c_to_p: u32,
        transport_latency_p_to_c: u32,
        phy_c_to_p: u8,
        phy_p_to_c: u8,
        nse: u8,
        bn_c_to_p: u8,
        bn_p_to_c: u8,
        ft_c_to_p: u8,
        ft_p_to_c: u8,
        max_pdu_c_to_p: u16,
        max_pdu_p_to_c: u16,
        iso_interval: u16,
    },
    CisRequest {
        acl_connection_handle: u16,
        cis_connection_handle: u16,
        cig_id: u8,
        cis_id: u8,
    },
    CreateBigComplete {
        status: u8,
        big_handle: u8,
        big_sync_delay: u32,
        transport_latency_big: u32,
        phy: u8,
        nse: u8,
        bn: u8,
        pto: u8,
        irc: u8,
        max_pdu: u16,
        iso_interval: u16,
        connection_handle: Vec<u16>,
    },
    TerminateBigComplete {
        big_handle: u8,
        reason: u8,
    },
    BigSyncEstablished {
        status: u8,
        big_handle: u8,
        transport_latency_big: u32,
        nse: u8,
        bn: u8,
        pto: u8,
        irc: u8,
        max_pdu: u16,
        iso_interval: u16,
        connection_handle: Vec<u16>,
    },
    BigSyncLost {
        big_handle: u8,
        reason: u8,
    },
    BiginfoAdvertisingReport {
        sync_handle: u16,
        num_bis: u8,
        nse: u8,
        iso_interval: u16,
        bn: u8,
        pto: u8,
        irc: u8,
        max_pdu: u16,
        sdu_interval: u32,
        max_sdu: u16,
        phy: u8,
        framing: u8,
        encryption: u8,
    },
    SubrateChange {
        status: u8,
        connection_handle: u16,
        subrate_factor: u16,
        peripheral_latency: u16,
        continuation_number: u16,
        supervision_timeout: u16,
    },
    PeriodicAdvertisingSyncEstablishedV2 {
        status: u8,
        sync_handle: u16,
        advertising_sid: u8,
        advertiser_address_type: u8,
        advertiser_address: Address,
        advertiser_phy: u8,
        periodic_advertising_interval: u16,
        advertiser_clock_accuracy: u8,
        num_subevents: u8,
        subevent_interval: u8,
        response_slot_delay: u8,
        response_slot_spacing: u8,
    },
    PeriodicAdvertisingReportV2 {
        sync_handle: u16,
        tx_power: i8,
        rssi: i8,
        cte_type: u8,
        periodic_event_counter: u16,
        subevent: u8,
        data_status: u8,
        data: Vec<u8>,
    },
    PeriodicAdvertisingSyncTransferReceivedV2 {
        status: u8,
        connection_handle: u16,
        service_data: u16,
        sync_handle: u16,
        advertising_sid: u8,
        advertiser_address_type: u8,
        advertiser_address: Address,
        advertiser_phy: u8,
        periodic_advertising_interval: u16,
        advertiser_clock_accuracy: u8,
        num_subevents: u8,
        subevent_interval: u8,
        response_slot_delay: u8,
        response_slot_spacing: u8,
    },
    EnhancedConnectionCompleteV2 {
        status: u8,
        connection_handle: u16,
        role: u8,
        peer_address_type: u8,
        peer_address: Address,
        local_resolvable_private_address: Address,
        peer_resolvable_private_address: Address,
        connection_interval: u16,
        peripheral_latency: u16,
        supervision_timeout: u16,
        central_clock_accuracy: u8,
        advertising_handle: u8,
        sync_handle: u16,
    },
    CsReadRemoteSupportedCapabilitiesComplete {
        status: u8,
        connection_handle: u16,
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
    CsReadRemoteFaeTableComplete {
        status: u8,
        connection_handle: u16,
        remote_fae_table: [u8; 72],
    },
    CsSecurityEnableComplete {
        status: u8,
        connection_handle: u16,
    },
    CsConfigComplete {
        status: u8,
        connection_handle: u16,
        config_id: u8,
        action: u8,
        main_mode_type: u8,
        sub_mode_type: u8,
        min_main_mode_steps: u8,
        max_main_mode_steps: u8,
        main_mode_repetition: u8,
        mode_0_steps: u8,
        role: u8,
        rtt_type: u8,
        cs_sync_phy: u8,
        channel_map: [u8; 10],
        channel_map_repetition: u8,
        channel_selection_type: u8,
        ch3c_shape: u8,
        ch3c_jump: u8,
        reserved: u8,
        t_ip1_time: u8,
        t_ip2_time: u8,
        t_fcs_time: u8,
        t_pm_time: u8,
    },
    CsProcedureEnableComplete {
        status: u8,
        connection_handle: u16,
        config_id: u8,
        state: u8,
        tone_antenna_config_selection: u8,
        selected_tx_power: i8,
        subevent_len: u32,
        subevents_per_event: u8,
        subevent_interval: u16,
        event_interval: u16,
        procedure_interval: u16,
        procedure_count: u16,
        max_procedure_len: u16,
    },
    CsSubeventResult {
        connection_handle: u16,
        config_id: u8,
        start_acl_conn_event_counter: u16,
        procedure_counter: u16,
        frequency_compensation: u16,
        reference_power_level: i8,
        procedure_done_status: u8,
        subevent_done_status: u8,
        abort_reason: u8,
        num_antenna_paths: u8,
        step_mode: Vec<u8>,
        step_channel: Vec<u8>,
        step_data: Vec<Vec<u8>>,
    },
    CsSubeventResultContinue {
        connection_handle: u16,
        config_id: u8,
        procedure_done_status: u8,
        subevent_done_status: u8,
        abort_reason: u8,
        num_antenna_paths: u8,
        step_mode: Vec<u8>,
        step_channel: Vec<u8>,
        step_data: Vec<Vec<u8>>,
    },
    CsTestEndComplete {
        connection_handle: u16,
        status: u8,
    },
    ConnectionRateChange {
        status: u8,
        connection_handle: u16,
        connection_interval: u16,
        subrate_factor: u16,
        peripheral_latency: u16,
        continuation_number: u16,
        supervision_timeout: u16,
    },
    AdvertisingReport {
        reports: Vec<AdvertisingReport>,
    },
    ExtendedAdvertisingReport {
        reports: Vec<ExtendedAdvertisingReport>,
    },
    /// Any LE sub-event with no typed model.
    Generic {
        subevent_code: u8,
        parameters: Vec<u8>,
    },
}

impl Event {
    /// The 8-bit event code.
    pub fn event_code(&self) -> u8 {
        match self {
            Event::InquiryComplete { .. } => HCI_INQUIRY_COMPLETE_EVENT,
            Event::InquiryResult { .. } => HCI_INQUIRY_RESULT_EVENT,
            Event::ConnectionComplete { .. } => HCI_CONNECTION_COMPLETE_EVENT,
            Event::ConnectionRequest { .. } => HCI_CONNECTION_REQUEST_EVENT,
            Event::DisconnectionComplete { .. } => HCI_DISCONNECTION_COMPLETE_EVENT,
            Event::AuthenticationComplete { .. } => HCI_AUTHENTICATION_COMPLETE_EVENT,
            Event::RemoteNameRequestComplete { .. } => HCI_REMOTE_NAME_REQUEST_COMPLETE_EVENT,
            Event::EncryptionChange { .. } => HCI_ENCRYPTION_CHANGE_EVENT,
            Event::ReadRemoteSupportedFeaturesComplete { .. } => {
                HCI_READ_REMOTE_SUPPORTED_FEATURES_COMPLETE_EVENT
            }
            Event::ReadRemoteVersionInformationComplete { .. } => {
                HCI_READ_REMOTE_VERSION_INFORMATION_COMPLETE_EVENT
            }
            Event::QosSetupComplete { .. } => HCI_QOS_SETUP_COMPLETE_EVENT,
            Event::CommandStatus { .. } => HCI_COMMAND_STATUS_EVENT,
            Event::RoleChange { .. } => HCI_ROLE_CHANGE_EVENT,
            Event::NumberOfCompletedPackets { .. } => HCI_NUMBER_OF_COMPLETED_PACKETS_EVENT,
            Event::ModeChange { .. } => HCI_MODE_CHANGE_EVENT,
            Event::PinCodeRequest { .. } => HCI_PIN_CODE_REQUEST_EVENT,
            Event::LinkKeyRequest { .. } => HCI_LINK_KEY_REQUEST_EVENT,
            Event::LinkKeyNotification { .. } => HCI_LINK_KEY_NOTIFICATION_EVENT,
            Event::MaxSlotsChange { .. } => HCI_MAX_SLOTS_CHANGE_EVENT,
            Event::ReadClockOffsetComplete { .. } => HCI_READ_CLOCK_OFFSET_COMPLETE_EVENT,
            Event::ConnectionPacketTypeChanged { .. } => HCI_CONNECTION_PACKET_TYPE_CHANGED_EVENT,
            Event::PageScanRepetitionModeChange { .. } => {
                HCI_PAGE_SCAN_REPETITION_MODE_CHANGE_EVENT
            }
            Event::InquiryResultWithRssi { .. } => HCI_INQUIRY_RESULT_WITH_RSSI_EVENT,
            Event::ReadRemoteExtendedFeaturesComplete { .. } => {
                HCI_READ_REMOTE_EXTENDED_FEATURES_COMPLETE_EVENT
            }
            Event::SynchronousConnectionComplete { .. } => {
                HCI_SYNCHRONOUS_CONNECTION_COMPLETE_EVENT
            }
            Event::SynchronousConnectionChanged { .. } => HCI_SYNCHRONOUS_CONNECTION_CHANGED_EVENT,
            Event::SniffSubrating { .. } => HCI_SNIFF_SUBRATING_EVENT,
            Event::ExtendedInquiryResult { .. } => HCI_EXTENDED_INQUIRY_RESULT_EVENT,
            Event::EncryptionKeyRefreshComplete { .. } => HCI_ENCRYPTION_KEY_REFRESH_COMPLETE_EVENT,
            Event::IoCapabilityRequest { .. } => HCI_IO_CAPABILITY_REQUEST_EVENT,
            Event::IoCapabilityResponse { .. } => HCI_IO_CAPABILITY_RESPONSE_EVENT,
            Event::UserConfirmationRequest { .. } => HCI_USER_CONFIRMATION_REQUEST_EVENT,
            Event::UserPasskeyRequest { .. } => HCI_USER_PASSKEY_REQUEST_EVENT,
            Event::RemoteOobDataRequest { .. } => HCI_REMOTE_OOB_DATA_REQUEST_EVENT,
            Event::SimplePairingComplete { .. } => HCI_SIMPLE_PAIRING_COMPLETE_EVENT,
            Event::LinkSupervisionTimeoutChanged { .. } => {
                HCI_LINK_SUPERVISION_TIMEOUT_CHANGED_EVENT
            }
            Event::EnhancedFlushComplete { .. } => HCI_ENHANCED_FLUSH_COMPLETE_EVENT,
            Event::UserPasskeyNotification { .. } => HCI_USER_PASSKEY_NOTIFICATION_EVENT,
            Event::KeypressNotification { .. } => HCI_KEYPRESS_NOTIFICATION_EVENT,
            Event::RemoteHostSupportedFeaturesNotification { .. } => {
                HCI_REMOTE_HOST_SUPPORTED_FEATURES_NOTIFICATION_EVENT
            }
            Event::EncryptionChangeV2 { .. } => HCI_ENCRYPTION_CHANGE_V2_EVENT,
            Event::Vendor { .. } => HCI_VENDOR_EVENT,
            Event::CommandComplete { .. } => HCI_COMMAND_COMPLETE_EVENT,
            Event::LeMeta(_) => HCI_LE_META_EVENT,
            Event::Generic { event_code, .. } => *event_code,
        }
    }

    /// The serialized event parameters (without the packet/event-code header).
    #[allow(clippy::needless_range_loop, clippy::vec_init_then_push)]
    pub fn parameters(&self) -> Vec<u8> {
        let mut p = Vec::new();
        match self {
            Event::InquiryComplete { status } => {
                p.push(*status);
            }
            Event::InquiryResult {
                bd_addr,
                page_scan_repetition_mode,
                reserved_0,
                reserved_1,
                class_of_device,
                clock_offset,
            } => {
                p.push(bd_addr.len() as u8);
                for i in 0..bd_addr.len() {
                    p.extend_from_slice(bd_addr[i].address_bytes());
                    p.push(page_scan_repetition_mode[i]);
                    p.push(reserved_0[i]);
                    p.push(reserved_1[i]);
                    p.extend_from_slice(&class_of_device[i].to_le_bytes()[..3]);
                    p.extend_from_slice(&clock_offset[i].to_le_bytes());
                }
            }
            Event::ConnectionComplete {
                status,
                connection_handle,
                bd_addr,
                link_type,
                encryption_enabled,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(bd_addr.address_bytes());
                p.push(*link_type);
                p.push(*encryption_enabled);
            }
            Event::ConnectionRequest {
                bd_addr,
                class_of_device,
                link_type,
            } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.extend_from_slice(&class_of_device.to_le_bytes()[..3]);
                p.push(*link_type);
            }
            Event::DisconnectionComplete {
                status,
                connection_handle,
                reason,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*reason);
            }
            Event::AuthenticationComplete {
                status,
                connection_handle,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
            }
            Event::RemoteNameRequestComplete {
                status,
                bd_addr,
                remote_name,
            } => {
                p.push(*status);
                p.extend_from_slice(bd_addr.address_bytes());
                p.extend_from_slice(remote_name);
            }
            Event::EncryptionChange {
                status,
                connection_handle,
                encryption_enabled,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*encryption_enabled);
            }
            Event::ReadRemoteSupportedFeaturesComplete {
                status,
                connection_handle,
                lmp_features,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(lmp_features);
            }
            Event::ReadRemoteVersionInformationComplete {
                status,
                connection_handle,
                version,
                manufacturer_name,
                subversion,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*version);
                p.extend_from_slice(&manufacturer_name.to_le_bytes());
                p.extend_from_slice(&subversion.to_le_bytes());
            }
            Event::QosSetupComplete {
                status,
                connection_handle,
                unused,
                service_type,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*unused);
                p.push(*service_type);
            }
            Event::CommandStatus {
                status,
                num_hci_command_packets,
                command_opcode,
            } => {
                p.push(*status);
                p.push(*num_hci_command_packets);
                p.extend_from_slice(&command_opcode.to_le_bytes());
            }
            Event::RoleChange {
                status,
                bd_addr,
                new_role,
            } => {
                p.push(*status);
                p.extend_from_slice(bd_addr.address_bytes());
                p.push(*new_role);
            }
            Event::NumberOfCompletedPackets {
                connection_handles,
                num_completed_packets,
            } => {
                p.push(connection_handles.len() as u8);
                for i in 0..connection_handles.len() {
                    p.extend_from_slice(&connection_handles[i].to_le_bytes());
                    p.extend_from_slice(&num_completed_packets[i].to_le_bytes());
                }
            }
            Event::ModeChange {
                status,
                connection_handle,
                current_mode,
                interval,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*current_mode);
                p.extend_from_slice(&interval.to_le_bytes());
            }
            Event::PinCodeRequest { bd_addr } => {
                p.extend_from_slice(bd_addr.address_bytes());
            }
            Event::LinkKeyRequest { bd_addr } => {
                p.extend_from_slice(bd_addr.address_bytes());
            }
            Event::LinkKeyNotification {
                bd_addr,
                link_key,
                key_type,
            } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.extend_from_slice(link_key);
                p.push(*key_type);
            }
            Event::MaxSlotsChange {
                connection_handle,
                lmp_max_slots,
            } => {
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*lmp_max_slots);
            }
            Event::ReadClockOffsetComplete {
                status,
                connection_handle,
                clock_offset,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(&clock_offset.to_le_bytes());
            }
            Event::ConnectionPacketTypeChanged {
                status,
                connection_handle,
                packet_type,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(&packet_type.to_le_bytes());
            }
            Event::PageScanRepetitionModeChange {
                bd_addr,
                page_scan_repetition_mode,
            } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.push(*page_scan_repetition_mode);
            }
            Event::InquiryResultWithRssi {
                bd_addr,
                page_scan_repetition_mode,
                reserved,
                class_of_device,
                clock_offset,
                rssi,
            } => {
                p.push(bd_addr.len() as u8);
                for i in 0..bd_addr.len() {
                    p.extend_from_slice(bd_addr[i].address_bytes());
                    p.push(page_scan_repetition_mode[i]);
                    p.push(reserved[i]);
                    p.extend_from_slice(&class_of_device[i].to_le_bytes()[..3]);
                    p.extend_from_slice(&clock_offset[i].to_le_bytes());
                    p.push(rssi[i] as u8);
                }
            }
            Event::ReadRemoteExtendedFeaturesComplete {
                status,
                connection_handle,
                page_number,
                maximum_page_number,
                extended_lmp_features,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*page_number);
                p.push(*maximum_page_number);
                p.extend_from_slice(extended_lmp_features);
            }
            Event::SynchronousConnectionComplete {
                status,
                connection_handle,
                bd_addr,
                link_type,
                transmission_interval,
                retransmission_window,
                rx_packet_length,
                tx_packet_length,
                air_mode,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(bd_addr.address_bytes());
                p.push(*link_type);
                p.push(*transmission_interval);
                p.push(*retransmission_window);
                p.extend_from_slice(&rx_packet_length.to_le_bytes());
                p.extend_from_slice(&tx_packet_length.to_le_bytes());
                p.push(*air_mode);
            }
            Event::SynchronousConnectionChanged {
                status,
                connection_handle,
                transmission_interval,
                retransmission_window,
                rx_packet_length,
                tx_packet_length,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*transmission_interval);
                p.push(*retransmission_window);
                p.extend_from_slice(&rx_packet_length.to_le_bytes());
                p.extend_from_slice(&tx_packet_length.to_le_bytes());
            }
            Event::SniffSubrating {
                status,
                connection_handle,
                max_tx_latency,
                max_rx_latency,
                min_remote_timeout,
                min_local_timeout,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(&max_tx_latency.to_le_bytes());
                p.extend_from_slice(&max_rx_latency.to_le_bytes());
                p.extend_from_slice(&min_remote_timeout.to_le_bytes());
                p.extend_from_slice(&min_local_timeout.to_le_bytes());
            }
            Event::ExtendedInquiryResult {
                num_responses,
                bd_addr,
                page_scan_repetition_mode,
                reserved,
                class_of_device,
                clock_offset,
                rssi,
                extended_inquiry_response,
            } => {
                p.push(*num_responses);
                p.extend_from_slice(bd_addr.address_bytes());
                p.push(*page_scan_repetition_mode);
                p.push(*reserved);
                p.extend_from_slice(&class_of_device.to_le_bytes()[..3]);
                p.extend_from_slice(&clock_offset.to_le_bytes());
                p.push(*rssi as u8);
                p.extend_from_slice(extended_inquiry_response);
            }
            Event::EncryptionKeyRefreshComplete {
                status,
                connection_handle,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
            }
            Event::IoCapabilityRequest { bd_addr } => {
                p.extend_from_slice(bd_addr.address_bytes());
            }
            Event::IoCapabilityResponse {
                bd_addr,
                io_capability,
                oob_data_present,
                authentication_requirements,
            } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.push(*io_capability);
                p.push(*oob_data_present);
                p.push(*authentication_requirements);
            }
            Event::UserConfirmationRequest {
                bd_addr,
                numeric_value,
            } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.extend_from_slice(&numeric_value.to_le_bytes());
            }
            Event::UserPasskeyRequest { bd_addr } => {
                p.extend_from_slice(bd_addr.address_bytes());
            }
            Event::RemoteOobDataRequest { bd_addr } => {
                p.extend_from_slice(bd_addr.address_bytes());
            }
            Event::SimplePairingComplete { status, bd_addr } => {
                p.push(*status);
                p.extend_from_slice(bd_addr.address_bytes());
            }
            Event::LinkSupervisionTimeoutChanged {
                connection_handle,
                link_supervision_timeout,
            } => {
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(&link_supervision_timeout.to_le_bytes());
            }
            Event::EnhancedFlushComplete { handle } => {
                p.extend_from_slice(&handle.to_le_bytes());
            }
            Event::UserPasskeyNotification { bd_addr, passkey } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.extend_from_slice(&passkey.to_le_bytes());
            }
            Event::KeypressNotification {
                bd_addr,
                notification_type,
            } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.push(*notification_type);
            }
            Event::RemoteHostSupportedFeaturesNotification {
                bd_addr,
                host_supported_features,
            } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.extend_from_slice(host_supported_features);
            }
            Event::EncryptionChangeV2 {
                status,
                connection_handle,
                encryption_enabled,
                encryption_key_size,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*encryption_enabled);
                p.push(*encryption_key_size);
            }
            Event::Vendor { data } => {
                p.extend_from_slice(data);
            }
            Event::CommandComplete {
                num_hci_command_packets,
                command_opcode,
                return_parameters,
            } => {
                p.push(*num_hci_command_packets);
                p.extend_from_slice(&command_opcode.to_le_bytes());
                p.extend_from_slice(&return_parameters.to_bytes());
            }
            Event::LeMeta(m) => p.extend_from_slice(&m.parameters()),
            Event::Generic { parameters, .. } => p.extend_from_slice(parameters),
        }
        p
    }

    /// Serialize to the full wire packet.
    pub fn to_bytes(&self) -> Vec<u8> {
        let params = self.parameters();
        let mut out = Vec::with_capacity(3 + params.len());
        out.push(HCI_EVENT_PACKET);
        out.push(self.event_code());
        out.push(params.len() as u8);
        out.extend_from_slice(&params);
        out
    }

    /// Parse a complete event packet (including the leading type byte).
    pub fn from_bytes(packet: &[u8]) -> Result<Event> {
        if packet.len() < 3 {
            return Err(Error::InvalidPacket("event packet too short".into()));
        }
        let event_code = packet[1];
        let parameters_length = packet[2] as usize;
        if packet.len() < 3 + parameters_length {
            return Err(Error::InvalidPacket(format!(
                "invalid parameters length: expected {parameters_length}, got {}",
                packet.len() - 3
            )));
        }
        let parameters = &packet[3..3 + parameters_length];
        Event::from_code_and_parameters(event_code, parameters)
    }

    /// Build a typed event from its event code and raw parameters.
    #[allow(clippy::redundant_closure_call)]
    pub fn from_code_and_parameters(event_code: u8, parameters: &[u8]) -> Result<Event> {
        if event_code == HCI_LE_META_EVENT {
            let subevent_code = *parameters
                .first()
                .ok_or_else(|| Error::InvalidPacket("empty LE meta parameters".into()))?;
            return Ok(Event::LeMeta(LeMetaEvent::from_subevent(
                subevent_code,
                &parameters[1..],
            )?));
        }
        let addr = |r: &mut Reader| -> Result<Address> {
            Ok(Address::from_bytes(
                r.array::<6>()?,
                AddressType::PUBLIC_DEVICE,
            ))
        };
        let mut r = Reader::new(parameters, 0);
        let _ = (&addr, &r);
        Ok(match event_code {
            HCI_INQUIRY_COMPLETE_EVENT => {
                let status = r.u8()?;
                Event::InquiryComplete { status }
            }
            HCI_INQUIRY_RESULT_EVENT => {
                let count0 = r.u8()? as usize;
                let mut bd_addr = Vec::with_capacity(count0);
                let mut page_scan_repetition_mode = Vec::with_capacity(count0);
                let mut reserved_0 = Vec::with_capacity(count0);
                let mut reserved_1 = Vec::with_capacity(count0);
                let mut class_of_device = Vec::with_capacity(count0);
                let mut clock_offset = Vec::with_capacity(count0);
                for _ in 0..count0 {
                    bd_addr.push(addr(&mut r)?);
                    page_scan_repetition_mode.push(r.u8()?);
                    reserved_0.push(r.u8()?);
                    reserved_1.push(r.u8()?);
                    class_of_device.push(r.u24_le()?);
                    clock_offset.push(r.u16_le()?);
                }
                Event::InquiryResult {
                    bd_addr,
                    page_scan_repetition_mode,
                    reserved_0,
                    reserved_1,
                    class_of_device,
                    clock_offset,
                }
            }
            HCI_CONNECTION_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let bd_addr = addr(&mut r)?;
                let link_type = r.u8()?;
                let encryption_enabled = r.u8()?;
                Event::ConnectionComplete {
                    status,
                    connection_handle,
                    bd_addr,
                    link_type,
                    encryption_enabled,
                }
            }
            HCI_CONNECTION_REQUEST_EVENT => {
                let bd_addr = addr(&mut r)?;
                let class_of_device = r.u24_le()?;
                let link_type = r.u8()?;
                Event::ConnectionRequest {
                    bd_addr,
                    class_of_device,
                    link_type,
                }
            }
            HCI_DISCONNECTION_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let reason = r.u8()?;
                Event::DisconnectionComplete {
                    status,
                    connection_handle,
                    reason,
                }
            }
            HCI_AUTHENTICATION_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                Event::AuthenticationComplete {
                    status,
                    connection_handle,
                }
            }
            HCI_REMOTE_NAME_REQUEST_COMPLETE_EVENT => {
                let status = r.u8()?;
                let bd_addr = addr(&mut r)?;
                let remote_name = r.array::<248>()?;
                Event::RemoteNameRequestComplete {
                    status,
                    bd_addr,
                    remote_name,
                }
            }
            HCI_ENCRYPTION_CHANGE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let encryption_enabled = r.u8()?;
                Event::EncryptionChange {
                    status,
                    connection_handle,
                    encryption_enabled,
                }
            }
            HCI_READ_REMOTE_SUPPORTED_FEATURES_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let lmp_features = r.array::<8>()?;
                Event::ReadRemoteSupportedFeaturesComplete {
                    status,
                    connection_handle,
                    lmp_features,
                }
            }
            HCI_READ_REMOTE_VERSION_INFORMATION_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let version = r.u8()?;
                let manufacturer_name = r.u16_le()?;
                let subversion = r.u16_le()?;
                Event::ReadRemoteVersionInformationComplete {
                    status,
                    connection_handle,
                    version,
                    manufacturer_name,
                    subversion,
                }
            }
            HCI_QOS_SETUP_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let unused = r.u8()?;
                let service_type = r.u8()?;
                Event::QosSetupComplete {
                    status,
                    connection_handle,
                    unused,
                    service_type,
                }
            }
            HCI_COMMAND_STATUS_EVENT => {
                let status = r.u8()?;
                let num_hci_command_packets = r.u8()?;
                let command_opcode = r.u16_le()?;
                Event::CommandStatus {
                    status,
                    num_hci_command_packets,
                    command_opcode,
                }
            }
            HCI_ROLE_CHANGE_EVENT => {
                let status = r.u8()?;
                let bd_addr = addr(&mut r)?;
                let new_role = r.u8()?;
                Event::RoleChange {
                    status,
                    bd_addr,
                    new_role,
                }
            }
            HCI_NUMBER_OF_COMPLETED_PACKETS_EVENT => {
                let count0 = r.u8()? as usize;
                let mut connection_handles = Vec::with_capacity(count0);
                let mut num_completed_packets = Vec::with_capacity(count0);
                for _ in 0..count0 {
                    connection_handles.push(r.u16_le()?);
                    num_completed_packets.push(r.u16_le()?);
                }
                Event::NumberOfCompletedPackets {
                    connection_handles,
                    num_completed_packets,
                }
            }
            HCI_MODE_CHANGE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let current_mode = r.u8()?;
                let interval = r.u16_le()?;
                Event::ModeChange {
                    status,
                    connection_handle,
                    current_mode,
                    interval,
                }
            }
            HCI_PIN_CODE_REQUEST_EVENT => {
                let bd_addr = addr(&mut r)?;
                Event::PinCodeRequest { bd_addr }
            }
            HCI_LINK_KEY_REQUEST_EVENT => {
                let bd_addr = addr(&mut r)?;
                Event::LinkKeyRequest { bd_addr }
            }
            HCI_LINK_KEY_NOTIFICATION_EVENT => {
                let bd_addr = addr(&mut r)?;
                let link_key = r.array::<16>()?;
                let key_type = r.u8()?;
                Event::LinkKeyNotification {
                    bd_addr,
                    link_key,
                    key_type,
                }
            }
            HCI_MAX_SLOTS_CHANGE_EVENT => {
                let connection_handle = r.u16_le()?;
                let lmp_max_slots = r.u8()?;
                Event::MaxSlotsChange {
                    connection_handle,
                    lmp_max_slots,
                }
            }
            HCI_READ_CLOCK_OFFSET_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let clock_offset = r.u16_le()?;
                Event::ReadClockOffsetComplete {
                    status,
                    connection_handle,
                    clock_offset,
                }
            }
            HCI_CONNECTION_PACKET_TYPE_CHANGED_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let packet_type = r.u16_le()?;
                Event::ConnectionPacketTypeChanged {
                    status,
                    connection_handle,
                    packet_type,
                }
            }
            HCI_PAGE_SCAN_REPETITION_MODE_CHANGE_EVENT => {
                let bd_addr = addr(&mut r)?;
                let page_scan_repetition_mode = r.u8()?;
                Event::PageScanRepetitionModeChange {
                    bd_addr,
                    page_scan_repetition_mode,
                }
            }
            HCI_INQUIRY_RESULT_WITH_RSSI_EVENT => {
                let count0 = r.u8()? as usize;
                let mut bd_addr = Vec::with_capacity(count0);
                let mut page_scan_repetition_mode = Vec::with_capacity(count0);
                let mut reserved = Vec::with_capacity(count0);
                let mut class_of_device = Vec::with_capacity(count0);
                let mut clock_offset = Vec::with_capacity(count0);
                let mut rssi = Vec::with_capacity(count0);
                for _ in 0..count0 {
                    bd_addr.push(addr(&mut r)?);
                    page_scan_repetition_mode.push(r.u8()?);
                    reserved.push(r.u8()?);
                    class_of_device.push(r.u24_le()?);
                    clock_offset.push(r.u16_le()?);
                    rssi.push(r.u8()? as i8);
                }
                Event::InquiryResultWithRssi {
                    bd_addr,
                    page_scan_repetition_mode,
                    reserved,
                    class_of_device,
                    clock_offset,
                    rssi,
                }
            }
            HCI_READ_REMOTE_EXTENDED_FEATURES_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let page_number = r.u8()?;
                let maximum_page_number = r.u8()?;
                let extended_lmp_features = r.array::<8>()?;
                Event::ReadRemoteExtendedFeaturesComplete {
                    status,
                    connection_handle,
                    page_number,
                    maximum_page_number,
                    extended_lmp_features,
                }
            }
            HCI_SYNCHRONOUS_CONNECTION_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let bd_addr = addr(&mut r)?;
                let link_type = r.u8()?;
                let transmission_interval = r.u8()?;
                let retransmission_window = r.u8()?;
                let rx_packet_length = r.u16_le()?;
                let tx_packet_length = r.u16_le()?;
                let air_mode = r.u8()?;
                Event::SynchronousConnectionComplete {
                    status,
                    connection_handle,
                    bd_addr,
                    link_type,
                    transmission_interval,
                    retransmission_window,
                    rx_packet_length,
                    tx_packet_length,
                    air_mode,
                }
            }
            HCI_SYNCHRONOUS_CONNECTION_CHANGED_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let transmission_interval = r.u8()?;
                let retransmission_window = r.u8()?;
                let rx_packet_length = r.u16_le()?;
                let tx_packet_length = r.u16_le()?;
                Event::SynchronousConnectionChanged {
                    status,
                    connection_handle,
                    transmission_interval,
                    retransmission_window,
                    rx_packet_length,
                    tx_packet_length,
                }
            }
            HCI_SNIFF_SUBRATING_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let max_tx_latency = r.u16_le()?;
                let max_rx_latency = r.u16_le()?;
                let min_remote_timeout = r.u16_le()?;
                let min_local_timeout = r.u16_le()?;
                Event::SniffSubrating {
                    status,
                    connection_handle,
                    max_tx_latency,
                    max_rx_latency,
                    min_remote_timeout,
                    min_local_timeout,
                }
            }
            HCI_EXTENDED_INQUIRY_RESULT_EVENT => {
                let num_responses = r.u8()?;
                let bd_addr = addr(&mut r)?;
                let page_scan_repetition_mode = r.u8()?;
                let reserved = r.u8()?;
                let class_of_device = r.u24_le()?;
                let clock_offset = r.u16_le()?;
                let rssi = r.u8()? as i8;
                let extended_inquiry_response = r.array::<240>()?;
                Event::ExtendedInquiryResult {
                    num_responses,
                    bd_addr,
                    page_scan_repetition_mode,
                    reserved,
                    class_of_device,
                    clock_offset,
                    rssi,
                    extended_inquiry_response,
                }
            }
            HCI_ENCRYPTION_KEY_REFRESH_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                Event::EncryptionKeyRefreshComplete {
                    status,
                    connection_handle,
                }
            }
            HCI_IO_CAPABILITY_REQUEST_EVENT => {
                let bd_addr = addr(&mut r)?;
                Event::IoCapabilityRequest { bd_addr }
            }
            HCI_IO_CAPABILITY_RESPONSE_EVENT => {
                let bd_addr = addr(&mut r)?;
                let io_capability = r.u8()?;
                let oob_data_present = r.u8()?;
                let authentication_requirements = r.u8()?;
                Event::IoCapabilityResponse {
                    bd_addr,
                    io_capability,
                    oob_data_present,
                    authentication_requirements,
                }
            }
            HCI_USER_CONFIRMATION_REQUEST_EVENT => {
                let bd_addr = addr(&mut r)?;
                let numeric_value = r.u32_le()?;
                Event::UserConfirmationRequest {
                    bd_addr,
                    numeric_value,
                }
            }
            HCI_USER_PASSKEY_REQUEST_EVENT => {
                let bd_addr = addr(&mut r)?;
                Event::UserPasskeyRequest { bd_addr }
            }
            HCI_REMOTE_OOB_DATA_REQUEST_EVENT => {
                let bd_addr = addr(&mut r)?;
                Event::RemoteOobDataRequest { bd_addr }
            }
            HCI_SIMPLE_PAIRING_COMPLETE_EVENT => {
                let status = r.u8()?;
                let bd_addr = addr(&mut r)?;
                Event::SimplePairingComplete { status, bd_addr }
            }
            HCI_LINK_SUPERVISION_TIMEOUT_CHANGED_EVENT => {
                let connection_handle = r.u16_le()?;
                let link_supervision_timeout = r.u16_le()?;
                Event::LinkSupervisionTimeoutChanged {
                    connection_handle,
                    link_supervision_timeout,
                }
            }
            HCI_ENHANCED_FLUSH_COMPLETE_EVENT => {
                let handle = r.u16_le()?;
                Event::EnhancedFlushComplete { handle }
            }
            HCI_USER_PASSKEY_NOTIFICATION_EVENT => {
                let bd_addr = addr(&mut r)?;
                let passkey = r.u32_le()?;
                Event::UserPasskeyNotification { bd_addr, passkey }
            }
            HCI_KEYPRESS_NOTIFICATION_EVENT => {
                let bd_addr = addr(&mut r)?;
                let notification_type = r.u8()?;
                Event::KeypressNotification {
                    bd_addr,
                    notification_type,
                }
            }
            HCI_REMOTE_HOST_SUPPORTED_FEATURES_NOTIFICATION_EVENT => {
                let bd_addr = addr(&mut r)?;
                let host_supported_features = r.array::<8>()?;
                Event::RemoteHostSupportedFeaturesNotification {
                    bd_addr,
                    host_supported_features,
                }
            }
            HCI_ENCRYPTION_CHANGE_V2_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let encryption_enabled = r.u8()?;
                let encryption_key_size = r.u8()?;
                Event::EncryptionChangeV2 {
                    status,
                    connection_handle,
                    encryption_enabled,
                    encryption_key_size,
                }
            }
            HCI_VENDOR_EVENT => {
                let data = r.rest().to_vec();
                Event::Vendor { data }
            }
            HCI_COMMAND_COMPLETE_EVENT => {
                let num_hci_command_packets = r.u8()?;
                let command_opcode = r.u16_le()?;
                let return_parameters = ReturnParameters::parse(command_opcode, r.rest())?;
                Event::CommandComplete {
                    num_hci_command_packets,
                    command_opcode,
                    return_parameters,
                }
            }
            _ => Event::Generic {
                event_code,
                parameters: parameters.to_vec(),
            },
        })
    }
}

impl LeMetaEvent {
    /// The LE sub-event code.
    pub fn subevent_code(&self) -> u8 {
        match self {
            LeMetaEvent::ConnectionComplete { .. } => HCI_LE_CONNECTION_COMPLETE_EVENT,
            LeMetaEvent::ConnectionUpdateComplete { .. } => HCI_LE_CONNECTION_UPDATE_COMPLETE_EVENT,
            LeMetaEvent::ReadRemoteFeaturesComplete { .. } => {
                HCI_LE_READ_REMOTE_FEATURES_COMPLETE_EVENT
            }
            LeMetaEvent::LongTermKeyRequest { .. } => HCI_LE_LONG_TERM_KEY_REQUEST_EVENT,
            LeMetaEvent::RemoteConnectionParameterRequest { .. } => {
                HCI_LE_REMOTE_CONNECTION_PARAMETER_REQUEST_EVENT
            }
            LeMetaEvent::DataLengthChange { .. } => HCI_LE_DATA_LENGTH_CHANGE_EVENT,
            LeMetaEvent::EnhancedConnectionComplete { .. } => {
                HCI_LE_ENHANCED_CONNECTION_COMPLETE_EVENT
            }
            LeMetaEvent::PhyUpdateComplete { .. } => HCI_LE_PHY_UPDATE_COMPLETE_EVENT,
            LeMetaEvent::PeriodicAdvertisingSyncEstablished { .. } => {
                HCI_LE_PERIODIC_ADVERTISING_SYNC_ESTABLISHED_EVENT
            }
            LeMetaEvent::PeriodicAdvertisingReport { .. } => {
                HCI_LE_PERIODIC_ADVERTISING_REPORT_EVENT
            }
            LeMetaEvent::PeriodicAdvertisingSyncLost { .. } => {
                HCI_LE_PERIODIC_ADVERTISING_SYNC_LOST_EVENT
            }
            LeMetaEvent::AdvertisingSetTerminated { .. } => HCI_LE_ADVERTISING_SET_TERMINATED_EVENT,
            LeMetaEvent::ChannelSelectionAlgorithm { .. } => {
                HCI_LE_CHANNEL_SELECTION_ALGORITHM_EVENT
            }
            LeMetaEvent::PeriodicAdvertisingSyncTransferReceived { .. } => {
                HCI_LE_PERIODIC_ADVERTISING_SYNC_TRANSFER_RECEIVED_EVENT
            }
            LeMetaEvent::CisEstablished { .. } => HCI_LE_CIS_ESTABLISHED_EVENT,
            LeMetaEvent::CisRequest { .. } => HCI_LE_CIS_REQUEST_EVENT,
            LeMetaEvent::CreateBigComplete { .. } => HCI_LE_CREATE_BIG_COMPLETE_EVENT,
            LeMetaEvent::TerminateBigComplete { .. } => HCI_LE_TERMINATE_BIG_COMPLETE_EVENT,
            LeMetaEvent::BigSyncEstablished { .. } => HCI_LE_BIG_SYNC_ESTABLISHED_EVENT,
            LeMetaEvent::BigSyncLost { .. } => HCI_LE_BIG_SYNC_LOST_EVENT,
            LeMetaEvent::BiginfoAdvertisingReport { .. } => HCI_LE_BIGINFO_ADVERTISING_REPORT_EVENT,
            LeMetaEvent::SubrateChange { .. } => HCI_LE_SUBRATE_CHANGE_EVENT,
            LeMetaEvent::PeriodicAdvertisingSyncEstablishedV2 { .. } => {
                HCI_LE_PERIODIC_ADVERTISING_SYNC_ESTABLISHED_V2_EVENT
            }
            LeMetaEvent::PeriodicAdvertisingReportV2 { .. } => {
                HCI_LE_PERIODIC_ADVERTISING_REPORT_V2_EVENT
            }
            LeMetaEvent::PeriodicAdvertisingSyncTransferReceivedV2 { .. } => {
                HCI_LE_PERIODIC_ADVERTISING_SYNC_TRANSFER_RECEIVED_V2_EVENT
            }
            LeMetaEvent::EnhancedConnectionCompleteV2 { .. } => {
                HCI_LE_ENHANCED_CONNECTION_COMPLETE_V2_EVENT
            }
            LeMetaEvent::CsReadRemoteSupportedCapabilitiesComplete { .. } => {
                HCI_LE_CS_READ_REMOTE_SUPPORTED_CAPABILITIES_COMPLETE_EVENT
            }
            LeMetaEvent::CsReadRemoteFaeTableComplete { .. } => {
                HCI_LE_CS_READ_REMOTE_FAE_TABLE_COMPLETE_EVENT
            }
            LeMetaEvent::CsSecurityEnableComplete { .. } => {
                HCI_LE_CS_SECURITY_ENABLE_COMPLETE_EVENT
            }
            LeMetaEvent::CsConfigComplete { .. } => HCI_LE_CS_CONFIG_COMPLETE_EVENT,
            LeMetaEvent::CsProcedureEnableComplete { .. } => {
                HCI_LE_CS_PROCEDURE_ENABLE_COMPLETE_EVENT
            }
            LeMetaEvent::CsSubeventResult { .. } => HCI_LE_CS_SUBEVENT_RESULT_EVENT,
            LeMetaEvent::CsSubeventResultContinue { .. } => {
                HCI_LE_CS_SUBEVENT_RESULT_CONTINUE_EVENT
            }
            LeMetaEvent::CsTestEndComplete { .. } => HCI_LE_CS_TEST_END_COMPLETE_EVENT,
            LeMetaEvent::ConnectionRateChange { .. } => HCI_LE_CONNECTION_RATE_CHANGE_EVENT,
            LeMetaEvent::AdvertisingReport { .. } => HCI_LE_ADVERTISING_REPORT_EVENT,
            LeMetaEvent::ExtendedAdvertisingReport { .. } => {
                HCI_LE_EXTENDED_ADVERTISING_REPORT_EVENT
            }
            LeMetaEvent::Generic { subevent_code, .. } => *subevent_code,
        }
    }

    /// Full LE-meta parameters: sub-event code byte followed by the fields.
    #[allow(clippy::needless_range_loop)]
    pub fn parameters(&self) -> Vec<u8> {
        let mut p = vec![self.subevent_code()];
        match self {
            LeMetaEvent::ConnectionComplete {
                status,
                connection_handle,
                role,
                peer_address_type,
                peer_address,
                connection_interval,
                peripheral_latency,
                supervision_timeout,
                central_clock_accuracy,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*role);
                p.push(*peer_address_type);
                p.extend_from_slice(peer_address.address_bytes());
                p.extend_from_slice(&connection_interval.to_le_bytes());
                p.extend_from_slice(&peripheral_latency.to_le_bytes());
                p.extend_from_slice(&supervision_timeout.to_le_bytes());
                p.push(*central_clock_accuracy);
            }
            LeMetaEvent::ConnectionUpdateComplete {
                status,
                connection_handle,
                connection_interval,
                peripheral_latency,
                supervision_timeout,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(&connection_interval.to_le_bytes());
                p.extend_from_slice(&peripheral_latency.to_le_bytes());
                p.extend_from_slice(&supervision_timeout.to_le_bytes());
            }
            LeMetaEvent::ReadRemoteFeaturesComplete {
                status,
                connection_handle,
                le_features,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(le_features);
            }
            LeMetaEvent::LongTermKeyRequest {
                connection_handle,
                random_number,
                encryption_diversifier,
            } => {
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(random_number);
                p.extend_from_slice(&encryption_diversifier.to_le_bytes());
            }
            LeMetaEvent::RemoteConnectionParameterRequest {
                connection_handle,
                interval_min,
                interval_max,
                max_latency,
                timeout,
            } => {
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(&interval_min.to_le_bytes());
                p.extend_from_slice(&interval_max.to_le_bytes());
                p.extend_from_slice(&max_latency.to_le_bytes());
                p.extend_from_slice(&timeout.to_le_bytes());
            }
            LeMetaEvent::DataLengthChange {
                connection_handle,
                max_tx_octets,
                max_tx_time,
                max_rx_octets,
                max_rx_time,
            } => {
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(&max_tx_octets.to_le_bytes());
                p.extend_from_slice(&max_tx_time.to_le_bytes());
                p.extend_from_slice(&max_rx_octets.to_le_bytes());
                p.extend_from_slice(&max_rx_time.to_le_bytes());
            }
            LeMetaEvent::EnhancedConnectionComplete {
                status,
                connection_handle,
                role,
                peer_address_type,
                peer_address,
                local_resolvable_private_address,
                peer_resolvable_private_address,
                connection_interval,
                peripheral_latency,
                supervision_timeout,
                central_clock_accuracy,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*role);
                p.push(*peer_address_type);
                p.extend_from_slice(peer_address.address_bytes());
                p.extend_from_slice(local_resolvable_private_address.address_bytes());
                p.extend_from_slice(peer_resolvable_private_address.address_bytes());
                p.extend_from_slice(&connection_interval.to_le_bytes());
                p.extend_from_slice(&peripheral_latency.to_le_bytes());
                p.extend_from_slice(&supervision_timeout.to_le_bytes());
                p.push(*central_clock_accuracy);
            }
            LeMetaEvent::PhyUpdateComplete {
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
            LeMetaEvent::PeriodicAdvertisingSyncEstablished {
                status,
                sync_handle,
                advertising_sid,
                advertiser_address_type,
                advertiser_address,
                advertiser_phy,
                periodic_advertising_interval,
                advertiser_clock_accuracy,
            } => {
                p.push(*status);
                p.extend_from_slice(&sync_handle.to_le_bytes());
                p.push(*advertising_sid);
                p.push(*advertiser_address_type);
                p.extend_from_slice(advertiser_address.address_bytes());
                p.push(*advertiser_phy);
                p.extend_from_slice(&periodic_advertising_interval.to_le_bytes());
                p.push(*advertiser_clock_accuracy);
            }
            LeMetaEvent::PeriodicAdvertisingReport {
                sync_handle,
                tx_power,
                rssi,
                cte_type,
                data_status,
                data,
            } => {
                p.extend_from_slice(&sync_handle.to_le_bytes());
                p.push(*tx_power as u8);
                p.push(*rssi as u8);
                p.push(*cte_type);
                p.push(*data_status);
                p.push(data.len() as u8);
                p.extend_from_slice(data);
            }
            LeMetaEvent::PeriodicAdvertisingSyncLost { sync_handle } => {
                p.extend_from_slice(&sync_handle.to_le_bytes());
            }
            LeMetaEvent::AdvertisingSetTerminated {
                status,
                advertising_handle,
                connection_handle,
                num_completed_extended_advertising_events,
            } => {
                p.push(*status);
                p.push(*advertising_handle);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*num_completed_extended_advertising_events);
            }
            LeMetaEvent::ChannelSelectionAlgorithm {
                connection_handle,
                channel_selection_algorithm,
            } => {
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*channel_selection_algorithm);
            }
            LeMetaEvent::PeriodicAdvertisingSyncTransferReceived {
                status,
                connection_handle,
                service_data,
                sync_handle,
                advertising_sid,
                advertiser_address_type,
                advertiser_address,
                advertiser_phy,
                periodic_advertising_interval,
                advertiser_clock_accuracy,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(&service_data.to_le_bytes());
                p.extend_from_slice(&sync_handle.to_le_bytes());
                p.push(*advertising_sid);
                p.push(*advertiser_address_type);
                p.extend_from_slice(advertiser_address.address_bytes());
                p.push(*advertiser_phy);
                p.extend_from_slice(&periodic_advertising_interval.to_le_bytes());
                p.push(*advertiser_clock_accuracy);
            }
            LeMetaEvent::CisEstablished {
                status,
                connection_handle,
                cig_sync_delay,
                cis_sync_delay,
                transport_latency_c_to_p,
                transport_latency_p_to_c,
                phy_c_to_p,
                phy_p_to_c,
                nse,
                bn_c_to_p,
                bn_p_to_c,
                ft_c_to_p,
                ft_p_to_c,
                max_pdu_c_to_p,
                max_pdu_p_to_c,
                iso_interval,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(&cig_sync_delay.to_le_bytes()[..3]);
                p.extend_from_slice(&cis_sync_delay.to_le_bytes()[..3]);
                p.extend_from_slice(&transport_latency_c_to_p.to_le_bytes()[..3]);
                p.extend_from_slice(&transport_latency_p_to_c.to_le_bytes()[..3]);
                p.push(*phy_c_to_p);
                p.push(*phy_p_to_c);
                p.push(*nse);
                p.push(*bn_c_to_p);
                p.push(*bn_p_to_c);
                p.push(*ft_c_to_p);
                p.push(*ft_p_to_c);
                p.extend_from_slice(&max_pdu_c_to_p.to_le_bytes());
                p.extend_from_slice(&max_pdu_p_to_c.to_le_bytes());
                p.extend_from_slice(&iso_interval.to_le_bytes());
            }
            LeMetaEvent::CisRequest {
                acl_connection_handle,
                cis_connection_handle,
                cig_id,
                cis_id,
            } => {
                p.extend_from_slice(&acl_connection_handle.to_le_bytes());
                p.extend_from_slice(&cis_connection_handle.to_le_bytes());
                p.push(*cig_id);
                p.push(*cis_id);
            }
            LeMetaEvent::CreateBigComplete {
                status,
                big_handle,
                big_sync_delay,
                transport_latency_big,
                phy,
                nse,
                bn,
                pto,
                irc,
                max_pdu,
                iso_interval,
                connection_handle,
            } => {
                p.push(*status);
                p.push(*big_handle);
                p.extend_from_slice(&big_sync_delay.to_le_bytes()[..3]);
                p.extend_from_slice(&transport_latency_big.to_le_bytes()[..3]);
                p.push(*phy);
                p.push(*nse);
                p.push(*bn);
                p.push(*pto);
                p.push(*irc);
                p.extend_from_slice(&max_pdu.to_le_bytes());
                p.extend_from_slice(&iso_interval.to_le_bytes());
                p.push(connection_handle.len() as u8);
                for i in 0..connection_handle.len() {
                    p.extend_from_slice(&connection_handle[i].to_le_bytes());
                }
            }
            LeMetaEvent::TerminateBigComplete { big_handle, reason } => {
                p.push(*big_handle);
                p.push(*reason);
            }
            LeMetaEvent::BigSyncEstablished {
                status,
                big_handle,
                transport_latency_big,
                nse,
                bn,
                pto,
                irc,
                max_pdu,
                iso_interval,
                connection_handle,
            } => {
                p.push(*status);
                p.push(*big_handle);
                p.extend_from_slice(&transport_latency_big.to_le_bytes()[..3]);
                p.push(*nse);
                p.push(*bn);
                p.push(*pto);
                p.push(*irc);
                p.extend_from_slice(&max_pdu.to_le_bytes());
                p.extend_from_slice(&iso_interval.to_le_bytes());
                p.push(connection_handle.len() as u8);
                for i in 0..connection_handle.len() {
                    p.extend_from_slice(&connection_handle[i].to_le_bytes());
                }
            }
            LeMetaEvent::BigSyncLost { big_handle, reason } => {
                p.push(*big_handle);
                p.push(*reason);
            }
            LeMetaEvent::BiginfoAdvertisingReport {
                sync_handle,
                num_bis,
                nse,
                iso_interval,
                bn,
                pto,
                irc,
                max_pdu,
                sdu_interval,
                max_sdu,
                phy,
                framing,
                encryption,
            } => {
                p.extend_from_slice(&sync_handle.to_le_bytes());
                p.push(*num_bis);
                p.push(*nse);
                p.extend_from_slice(&iso_interval.to_le_bytes());
                p.push(*bn);
                p.push(*pto);
                p.push(*irc);
                p.extend_from_slice(&max_pdu.to_le_bytes());
                p.extend_from_slice(&sdu_interval.to_le_bytes()[..3]);
                p.extend_from_slice(&max_sdu.to_le_bytes());
                p.push(*phy);
                p.push(*framing);
                p.push(*encryption);
            }
            LeMetaEvent::SubrateChange {
                status,
                connection_handle,
                subrate_factor,
                peripheral_latency,
                continuation_number,
                supervision_timeout,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(&subrate_factor.to_le_bytes());
                p.extend_from_slice(&peripheral_latency.to_le_bytes());
                p.extend_from_slice(&continuation_number.to_le_bytes());
                p.extend_from_slice(&supervision_timeout.to_le_bytes());
            }
            LeMetaEvent::PeriodicAdvertisingSyncEstablishedV2 {
                status,
                sync_handle,
                advertising_sid,
                advertiser_address_type,
                advertiser_address,
                advertiser_phy,
                periodic_advertising_interval,
                advertiser_clock_accuracy,
                num_subevents,
                subevent_interval,
                response_slot_delay,
                response_slot_spacing,
            } => {
                p.push(*status);
                p.extend_from_slice(&sync_handle.to_le_bytes());
                p.push(*advertising_sid);
                p.push(*advertiser_address_type);
                p.extend_from_slice(advertiser_address.address_bytes());
                p.push(*advertiser_phy);
                p.extend_from_slice(&periodic_advertising_interval.to_le_bytes());
                p.push(*advertiser_clock_accuracy);
                p.push(*num_subevents);
                p.push(*subevent_interval);
                p.push(*response_slot_delay);
                p.push(*response_slot_spacing);
            }
            LeMetaEvent::PeriodicAdvertisingReportV2 {
                sync_handle,
                tx_power,
                rssi,
                cte_type,
                periodic_event_counter,
                subevent,
                data_status,
                data,
            } => {
                p.extend_from_slice(&sync_handle.to_le_bytes());
                p.push(*tx_power as u8);
                p.push(*rssi as u8);
                p.push(*cte_type);
                p.extend_from_slice(&periodic_event_counter.to_le_bytes());
                p.push(*subevent);
                p.push(*data_status);
                p.push(data.len() as u8);
                p.extend_from_slice(data);
            }
            LeMetaEvent::PeriodicAdvertisingSyncTransferReceivedV2 {
                status,
                connection_handle,
                service_data,
                sync_handle,
                advertising_sid,
                advertiser_address_type,
                advertiser_address,
                advertiser_phy,
                periodic_advertising_interval,
                advertiser_clock_accuracy,
                num_subevents,
                subevent_interval,
                response_slot_delay,
                response_slot_spacing,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(&service_data.to_le_bytes());
                p.extend_from_slice(&sync_handle.to_le_bytes());
                p.push(*advertising_sid);
                p.push(*advertiser_address_type);
                p.extend_from_slice(advertiser_address.address_bytes());
                p.push(*advertiser_phy);
                p.extend_from_slice(&periodic_advertising_interval.to_le_bytes());
                p.push(*advertiser_clock_accuracy);
                p.push(*num_subevents);
                p.push(*subevent_interval);
                p.push(*response_slot_delay);
                p.push(*response_slot_spacing);
            }
            LeMetaEvent::EnhancedConnectionCompleteV2 {
                status,
                connection_handle,
                role,
                peer_address_type,
                peer_address,
                local_resolvable_private_address,
                peer_resolvable_private_address,
                connection_interval,
                peripheral_latency,
                supervision_timeout,
                central_clock_accuracy,
                advertising_handle,
                sync_handle,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*role);
                p.push(*peer_address_type);
                p.extend_from_slice(peer_address.address_bytes());
                p.extend_from_slice(local_resolvable_private_address.address_bytes());
                p.extend_from_slice(peer_resolvable_private_address.address_bytes());
                p.extend_from_slice(&connection_interval.to_le_bytes());
                p.extend_from_slice(&peripheral_latency.to_le_bytes());
                p.extend_from_slice(&supervision_timeout.to_le_bytes());
                p.push(*central_clock_accuracy);
                p.push(*advertising_handle);
                p.extend_from_slice(&sync_handle.to_le_bytes());
            }
            LeMetaEvent::CsReadRemoteSupportedCapabilitiesComplete {
                status,
                connection_handle,
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
                p.extend_from_slice(&connection_handle.to_le_bytes());
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
            LeMetaEvent::CsReadRemoteFaeTableComplete {
                status,
                connection_handle,
                remote_fae_table,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(remote_fae_table);
            }
            LeMetaEvent::CsSecurityEnableComplete {
                status,
                connection_handle,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
            }
            LeMetaEvent::CsConfigComplete {
                status,
                connection_handle,
                config_id,
                action,
                main_mode_type,
                sub_mode_type,
                min_main_mode_steps,
                max_main_mode_steps,
                main_mode_repetition,
                mode_0_steps,
                role,
                rtt_type,
                cs_sync_phy,
                channel_map,
                channel_map_repetition,
                channel_selection_type,
                ch3c_shape,
                ch3c_jump,
                reserved,
                t_ip1_time,
                t_ip2_time,
                t_fcs_time,
                t_pm_time,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*config_id);
                p.push(*action);
                p.push(*main_mode_type);
                p.push(*sub_mode_type);
                p.push(*min_main_mode_steps);
                p.push(*max_main_mode_steps);
                p.push(*main_mode_repetition);
                p.push(*mode_0_steps);
                p.push(*role);
                p.push(*rtt_type);
                p.push(*cs_sync_phy);
                p.extend_from_slice(channel_map);
                p.push(*channel_map_repetition);
                p.push(*channel_selection_type);
                p.push(*ch3c_shape);
                p.push(*ch3c_jump);
                p.push(*reserved);
                p.push(*t_ip1_time);
                p.push(*t_ip2_time);
                p.push(*t_fcs_time);
                p.push(*t_pm_time);
            }
            LeMetaEvent::CsProcedureEnableComplete {
                status,
                connection_handle,
                config_id,
                state,
                tone_antenna_config_selection,
                selected_tx_power,
                subevent_len,
                subevents_per_event,
                subevent_interval,
                event_interval,
                procedure_interval,
                procedure_count,
                max_procedure_len,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*config_id);
                p.push(*state);
                p.push(*tone_antenna_config_selection);
                p.push(*selected_tx_power as u8);
                p.extend_from_slice(&subevent_len.to_le_bytes()[..3]);
                p.push(*subevents_per_event);
                p.extend_from_slice(&subevent_interval.to_le_bytes());
                p.extend_from_slice(&event_interval.to_le_bytes());
                p.extend_from_slice(&procedure_interval.to_le_bytes());
                p.extend_from_slice(&procedure_count.to_le_bytes());
                p.extend_from_slice(&max_procedure_len.to_le_bytes());
            }
            LeMetaEvent::CsSubeventResult {
                connection_handle,
                config_id,
                start_acl_conn_event_counter,
                procedure_counter,
                frequency_compensation,
                reference_power_level,
                procedure_done_status,
                subevent_done_status,
                abort_reason,
                num_antenna_paths,
                step_mode,
                step_channel,
                step_data,
            } => {
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*config_id);
                p.extend_from_slice(&start_acl_conn_event_counter.to_le_bytes());
                p.extend_from_slice(&procedure_counter.to_le_bytes());
                p.extend_from_slice(&frequency_compensation.to_le_bytes());
                p.push(*reference_power_level as u8);
                p.push(*procedure_done_status);
                p.push(*subevent_done_status);
                p.push(*abort_reason);
                p.push(*num_antenna_paths);
                p.push(step_mode.len() as u8);
                for i in 0..step_mode.len() {
                    p.push(step_mode[i]);
                    p.push(step_channel[i]);
                    p.push(step_data[i].len() as u8);
                    p.extend_from_slice(&step_data[i]);
                }
            }
            LeMetaEvent::CsSubeventResultContinue {
                connection_handle,
                config_id,
                procedure_done_status,
                subevent_done_status,
                abort_reason,
                num_antenna_paths,
                step_mode,
                step_channel,
                step_data,
            } => {
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*config_id);
                p.push(*procedure_done_status);
                p.push(*subevent_done_status);
                p.push(*abort_reason);
                p.push(*num_antenna_paths);
                p.push(step_mode.len() as u8);
                for i in 0..step_mode.len() {
                    p.push(step_mode[i]);
                    p.push(step_channel[i]);
                    p.push(step_data[i].len() as u8);
                    p.extend_from_slice(&step_data[i]);
                }
            }
            LeMetaEvent::CsTestEndComplete {
                connection_handle,
                status,
            } => {
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.push(*status);
            }
            LeMetaEvent::ConnectionRateChange {
                status,
                connection_handle,
                connection_interval,
                subrate_factor,
                peripheral_latency,
                continuation_number,
                supervision_timeout,
            } => {
                p.push(*status);
                p.extend_from_slice(&connection_handle.to_le_bytes());
                p.extend_from_slice(&connection_interval.to_le_bytes());
                p.extend_from_slice(&subrate_factor.to_le_bytes());
                p.extend_from_slice(&peripheral_latency.to_le_bytes());
                p.extend_from_slice(&continuation_number.to_le_bytes());
                p.extend_from_slice(&supervision_timeout.to_le_bytes());
            }
            LeMetaEvent::AdvertisingReport { reports } => {
                p.push(reports.len() as u8);
                for r in reports {
                    p.push(r.event_type);
                    p.push(r.address_type);
                    p.extend_from_slice(r.address.address_bytes());
                    p.push(r.data.len() as u8);
                    p.extend_from_slice(&r.data);
                    p.push(r.rssi as u8);
                }
            }
            LeMetaEvent::ExtendedAdvertisingReport { reports } => {
                p.push(reports.len() as u8);
                for r in reports {
                    p.extend_from_slice(&r.event_type.to_le_bytes());
                    p.push(r.address_type);
                    p.extend_from_slice(r.address.address_bytes());
                    p.push(r.primary_phy);
                    p.push(r.secondary_phy);
                    p.push(r.advertising_sid);
                    p.push(r.tx_power as u8);
                    p.push(r.rssi as u8);
                    p.extend_from_slice(&r.periodic_advertising_interval.to_le_bytes());
                    p.push(r.direct_address_type);
                    p.extend_from_slice(r.direct_address.address_bytes());
                    p.push(r.data.len() as u8);
                    p.extend_from_slice(&r.data);
                }
            }
            LeMetaEvent::Generic { parameters, .. } => {
                p.extend_from_slice(parameters);
            }
        }
        p
    }

    /// Parse an LE sub-event from its sub-event code and field bytes (the bytes
    /// after the sub-event code).
    #[allow(clippy::redundant_closure_call)]
    pub fn from_subevent(subevent_code: u8, fields: &[u8]) -> Result<LeMetaEvent> {
        let addr = |r: &mut Reader| -> Result<Address> {
            Ok(Address::from_bytes(
                r.array::<6>()?,
                AddressType::RANDOM_DEVICE,
            ))
        };
        let mut r = Reader::new(fields, 0);
        let _ = (&addr, &r);
        Ok(match subevent_code {
            HCI_LE_CONNECTION_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let role = r.u8()?;
                let peer_address_type = r.u8()?;
                let peer_address = addr(&mut r)?;
                let connection_interval = r.u16_le()?;
                let peripheral_latency = r.u16_le()?;
                let supervision_timeout = r.u16_le()?;
                let central_clock_accuracy = r.u8()?;
                LeMetaEvent::ConnectionComplete {
                    status,
                    connection_handle,
                    role,
                    peer_address_type,
                    peer_address,
                    connection_interval,
                    peripheral_latency,
                    supervision_timeout,
                    central_clock_accuracy,
                }
            }
            HCI_LE_CONNECTION_UPDATE_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let connection_interval = r.u16_le()?;
                let peripheral_latency = r.u16_le()?;
                let supervision_timeout = r.u16_le()?;
                LeMetaEvent::ConnectionUpdateComplete {
                    status,
                    connection_handle,
                    connection_interval,
                    peripheral_latency,
                    supervision_timeout,
                }
            }
            HCI_LE_READ_REMOTE_FEATURES_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let le_features = r.array::<8>()?;
                LeMetaEvent::ReadRemoteFeaturesComplete {
                    status,
                    connection_handle,
                    le_features,
                }
            }
            HCI_LE_LONG_TERM_KEY_REQUEST_EVENT => {
                let connection_handle = r.u16_le()?;
                let random_number = r.array::<8>()?;
                let encryption_diversifier = r.u16_le()?;
                LeMetaEvent::LongTermKeyRequest {
                    connection_handle,
                    random_number,
                    encryption_diversifier,
                }
            }
            HCI_LE_REMOTE_CONNECTION_PARAMETER_REQUEST_EVENT => {
                let connection_handle = r.u16_le()?;
                let interval_min = r.u16_le()?;
                let interval_max = r.u16_le()?;
                let max_latency = r.u16_le()?;
                let timeout = r.u16_le()?;
                LeMetaEvent::RemoteConnectionParameterRequest {
                    connection_handle,
                    interval_min,
                    interval_max,
                    max_latency,
                    timeout,
                }
            }
            HCI_LE_DATA_LENGTH_CHANGE_EVENT => {
                let connection_handle = r.u16_le()?;
                let max_tx_octets = r.u16_le()?;
                let max_tx_time = r.u16_le()?;
                let max_rx_octets = r.u16_le()?;
                let max_rx_time = r.u16_le()?;
                LeMetaEvent::DataLengthChange {
                    connection_handle,
                    max_tx_octets,
                    max_tx_time,
                    max_rx_octets,
                    max_rx_time,
                }
            }
            HCI_LE_ENHANCED_CONNECTION_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let role = r.u8()?;
                let peer_address_type = r.u8()?;
                let peer_address = addr(&mut r)?;
                let local_resolvable_private_address = addr(&mut r)?;
                let peer_resolvable_private_address = addr(&mut r)?;
                let connection_interval = r.u16_le()?;
                let peripheral_latency = r.u16_le()?;
                let supervision_timeout = r.u16_le()?;
                let central_clock_accuracy = r.u8()?;
                LeMetaEvent::EnhancedConnectionComplete {
                    status,
                    connection_handle,
                    role,
                    peer_address_type,
                    peer_address,
                    local_resolvable_private_address,
                    peer_resolvable_private_address,
                    connection_interval,
                    peripheral_latency,
                    supervision_timeout,
                    central_clock_accuracy,
                }
            }
            HCI_LE_PHY_UPDATE_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let tx_phy = r.u8()?;
                let rx_phy = r.u8()?;
                LeMetaEvent::PhyUpdateComplete {
                    status,
                    connection_handle,
                    tx_phy,
                    rx_phy,
                }
            }
            HCI_LE_PERIODIC_ADVERTISING_SYNC_ESTABLISHED_EVENT => {
                let status = r.u8()?;
                let sync_handle = r.u16_le()?;
                let advertising_sid = r.u8()?;
                let advertiser_address_type = r.u8()?;
                let advertiser_address = addr(&mut r)?;
                let advertiser_phy = r.u8()?;
                let periodic_advertising_interval = r.u16_le()?;
                let advertiser_clock_accuracy = r.u8()?;
                LeMetaEvent::PeriodicAdvertisingSyncEstablished {
                    status,
                    sync_handle,
                    advertising_sid,
                    advertiser_address_type,
                    advertiser_address,
                    advertiser_phy,
                    periodic_advertising_interval,
                    advertiser_clock_accuracy,
                }
            }
            HCI_LE_PERIODIC_ADVERTISING_REPORT_EVENT => {
                let sync_handle = r.u16_le()?;
                let tx_power = r.u8()? as i8;
                let rssi = r.u8()? as i8;
                let cte_type = r.u8()?;
                let data_status = r.u8()?;
                let data = {
                    let n = r.u8()? as usize;
                    r.take(n)?.to_vec()
                };
                LeMetaEvent::PeriodicAdvertisingReport {
                    sync_handle,
                    tx_power,
                    rssi,
                    cte_type,
                    data_status,
                    data,
                }
            }
            HCI_LE_PERIODIC_ADVERTISING_SYNC_LOST_EVENT => {
                let sync_handle = r.u16_le()?;
                LeMetaEvent::PeriodicAdvertisingSyncLost { sync_handle }
            }
            HCI_LE_ADVERTISING_SET_TERMINATED_EVENT => {
                let status = r.u8()?;
                let advertising_handle = r.u8()?;
                let connection_handle = r.u16_le()?;
                let num_completed_extended_advertising_events = r.u8()?;
                LeMetaEvent::AdvertisingSetTerminated {
                    status,
                    advertising_handle,
                    connection_handle,
                    num_completed_extended_advertising_events,
                }
            }
            HCI_LE_CHANNEL_SELECTION_ALGORITHM_EVENT => {
                let connection_handle = r.u16_le()?;
                let channel_selection_algorithm = r.u8()?;
                LeMetaEvent::ChannelSelectionAlgorithm {
                    connection_handle,
                    channel_selection_algorithm,
                }
            }
            HCI_LE_PERIODIC_ADVERTISING_SYNC_TRANSFER_RECEIVED_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let service_data = r.u16_le()?;
                let sync_handle = r.u16_le()?;
                let advertising_sid = r.u8()?;
                let advertiser_address_type = r.u8()?;
                let advertiser_address = addr(&mut r)?;
                let advertiser_phy = r.u8()?;
                let periodic_advertising_interval = r.u16_le()?;
                let advertiser_clock_accuracy = r.u8()?;
                LeMetaEvent::PeriodicAdvertisingSyncTransferReceived {
                    status,
                    connection_handle,
                    service_data,
                    sync_handle,
                    advertising_sid,
                    advertiser_address_type,
                    advertiser_address,
                    advertiser_phy,
                    periodic_advertising_interval,
                    advertiser_clock_accuracy,
                }
            }
            HCI_LE_CIS_ESTABLISHED_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let cig_sync_delay = r.u24_le()?;
                let cis_sync_delay = r.u24_le()?;
                let transport_latency_c_to_p = r.u24_le()?;
                let transport_latency_p_to_c = r.u24_le()?;
                let phy_c_to_p = r.u8()?;
                let phy_p_to_c = r.u8()?;
                let nse = r.u8()?;
                let bn_c_to_p = r.u8()?;
                let bn_p_to_c = r.u8()?;
                let ft_c_to_p = r.u8()?;
                let ft_p_to_c = r.u8()?;
                let max_pdu_c_to_p = r.u16_le()?;
                let max_pdu_p_to_c = r.u16_le()?;
                let iso_interval = r.u16_le()?;
                LeMetaEvent::CisEstablished {
                    status,
                    connection_handle,
                    cig_sync_delay,
                    cis_sync_delay,
                    transport_latency_c_to_p,
                    transport_latency_p_to_c,
                    phy_c_to_p,
                    phy_p_to_c,
                    nse,
                    bn_c_to_p,
                    bn_p_to_c,
                    ft_c_to_p,
                    ft_p_to_c,
                    max_pdu_c_to_p,
                    max_pdu_p_to_c,
                    iso_interval,
                }
            }
            HCI_LE_CIS_REQUEST_EVENT => {
                let acl_connection_handle = r.u16_le()?;
                let cis_connection_handle = r.u16_le()?;
                let cig_id = r.u8()?;
                let cis_id = r.u8()?;
                LeMetaEvent::CisRequest {
                    acl_connection_handle,
                    cis_connection_handle,
                    cig_id,
                    cis_id,
                }
            }
            HCI_LE_CREATE_BIG_COMPLETE_EVENT => {
                let status = r.u8()?;
                let big_handle = r.u8()?;
                let big_sync_delay = r.u24_le()?;
                let transport_latency_big = r.u24_le()?;
                let phy = r.u8()?;
                let nse = r.u8()?;
                let bn = r.u8()?;
                let pto = r.u8()?;
                let irc = r.u8()?;
                let max_pdu = r.u16_le()?;
                let iso_interval = r.u16_le()?;
                let count0 = r.u8()? as usize;
                let mut connection_handle = Vec::with_capacity(count0);
                for _ in 0..count0 {
                    connection_handle.push(r.u16_le()?);
                }
                LeMetaEvent::CreateBigComplete {
                    status,
                    big_handle,
                    big_sync_delay,
                    transport_latency_big,
                    phy,
                    nse,
                    bn,
                    pto,
                    irc,
                    max_pdu,
                    iso_interval,
                    connection_handle,
                }
            }
            HCI_LE_TERMINATE_BIG_COMPLETE_EVENT => {
                let big_handle = r.u8()?;
                let reason = r.u8()?;
                LeMetaEvent::TerminateBigComplete { big_handle, reason }
            }
            HCI_LE_BIG_SYNC_ESTABLISHED_EVENT => {
                let status = r.u8()?;
                let big_handle = r.u8()?;
                let transport_latency_big = r.u24_le()?;
                let nse = r.u8()?;
                let bn = r.u8()?;
                let pto = r.u8()?;
                let irc = r.u8()?;
                let max_pdu = r.u16_le()?;
                let iso_interval = r.u16_le()?;
                let count0 = r.u8()? as usize;
                let mut connection_handle = Vec::with_capacity(count0);
                for _ in 0..count0 {
                    connection_handle.push(r.u16_le()?);
                }
                LeMetaEvent::BigSyncEstablished {
                    status,
                    big_handle,
                    transport_latency_big,
                    nse,
                    bn,
                    pto,
                    irc,
                    max_pdu,
                    iso_interval,
                    connection_handle,
                }
            }
            HCI_LE_BIG_SYNC_LOST_EVENT => {
                let big_handle = r.u8()?;
                let reason = r.u8()?;
                LeMetaEvent::BigSyncLost { big_handle, reason }
            }
            HCI_LE_BIGINFO_ADVERTISING_REPORT_EVENT => {
                let sync_handle = r.u16_le()?;
                let num_bis = r.u8()?;
                let nse = r.u8()?;
                let iso_interval = r.u16_le()?;
                let bn = r.u8()?;
                let pto = r.u8()?;
                let irc = r.u8()?;
                let max_pdu = r.u16_le()?;
                let sdu_interval = r.u24_le()?;
                let max_sdu = r.u16_le()?;
                let phy = r.u8()?;
                let framing = r.u8()?;
                let encryption = r.u8()?;
                LeMetaEvent::BiginfoAdvertisingReport {
                    sync_handle,
                    num_bis,
                    nse,
                    iso_interval,
                    bn,
                    pto,
                    irc,
                    max_pdu,
                    sdu_interval,
                    max_sdu,
                    phy,
                    framing,
                    encryption,
                }
            }
            HCI_LE_SUBRATE_CHANGE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let subrate_factor = r.u16_le()?;
                let peripheral_latency = r.u16_le()?;
                let continuation_number = r.u16_le()?;
                let supervision_timeout = r.u16_le()?;
                LeMetaEvent::SubrateChange {
                    status,
                    connection_handle,
                    subrate_factor,
                    peripheral_latency,
                    continuation_number,
                    supervision_timeout,
                }
            }
            HCI_LE_PERIODIC_ADVERTISING_SYNC_ESTABLISHED_V2_EVENT => {
                let status = r.u8()?;
                let sync_handle = r.u16_le()?;
                let advertising_sid = r.u8()?;
                let advertiser_address_type = r.u8()?;
                let advertiser_address = addr(&mut r)?;
                let advertiser_phy = r.u8()?;
                let periodic_advertising_interval = r.u16_le()?;
                let advertiser_clock_accuracy = r.u8()?;
                let num_subevents = r.u8()?;
                let subevent_interval = r.u8()?;
                let response_slot_delay = r.u8()?;
                let response_slot_spacing = r.u8()?;
                LeMetaEvent::PeriodicAdvertisingSyncEstablishedV2 {
                    status,
                    sync_handle,
                    advertising_sid,
                    advertiser_address_type,
                    advertiser_address,
                    advertiser_phy,
                    periodic_advertising_interval,
                    advertiser_clock_accuracy,
                    num_subevents,
                    subevent_interval,
                    response_slot_delay,
                    response_slot_spacing,
                }
            }
            HCI_LE_PERIODIC_ADVERTISING_REPORT_V2_EVENT => {
                let sync_handle = r.u16_le()?;
                let tx_power = r.u8()? as i8;
                let rssi = r.u8()? as i8;
                let cte_type = r.u8()?;
                let periodic_event_counter = r.u16_le()?;
                let subevent = r.u8()?;
                let data_status = r.u8()?;
                let data = {
                    let n = r.u8()? as usize;
                    r.take(n)?.to_vec()
                };
                LeMetaEvent::PeriodicAdvertisingReportV2 {
                    sync_handle,
                    tx_power,
                    rssi,
                    cte_type,
                    periodic_event_counter,
                    subevent,
                    data_status,
                    data,
                }
            }
            HCI_LE_PERIODIC_ADVERTISING_SYNC_TRANSFER_RECEIVED_V2_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let service_data = r.u16_le()?;
                let sync_handle = r.u16_le()?;
                let advertising_sid = r.u8()?;
                let advertiser_address_type = r.u8()?;
                let advertiser_address = addr(&mut r)?;
                let advertiser_phy = r.u8()?;
                let periodic_advertising_interval = r.u16_le()?;
                let advertiser_clock_accuracy = r.u8()?;
                let num_subevents = r.u8()?;
                let subevent_interval = r.u8()?;
                let response_slot_delay = r.u8()?;
                let response_slot_spacing = r.u8()?;
                LeMetaEvent::PeriodicAdvertisingSyncTransferReceivedV2 {
                    status,
                    connection_handle,
                    service_data,
                    sync_handle,
                    advertising_sid,
                    advertiser_address_type,
                    advertiser_address,
                    advertiser_phy,
                    periodic_advertising_interval,
                    advertiser_clock_accuracy,
                    num_subevents,
                    subevent_interval,
                    response_slot_delay,
                    response_slot_spacing,
                }
            }
            HCI_LE_ENHANCED_CONNECTION_COMPLETE_V2_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let role = r.u8()?;
                let peer_address_type = r.u8()?;
                let peer_address = addr(&mut r)?;
                let local_resolvable_private_address = addr(&mut r)?;
                let peer_resolvable_private_address = addr(&mut r)?;
                let connection_interval = r.u16_le()?;
                let peripheral_latency = r.u16_le()?;
                let supervision_timeout = r.u16_le()?;
                let central_clock_accuracy = r.u8()?;
                let advertising_handle = r.u8()?;
                let sync_handle = r.u16_le()?;
                LeMetaEvent::EnhancedConnectionCompleteV2 {
                    status,
                    connection_handle,
                    role,
                    peer_address_type,
                    peer_address,
                    local_resolvable_private_address,
                    peer_resolvable_private_address,
                    connection_interval,
                    peripheral_latency,
                    supervision_timeout,
                    central_clock_accuracy,
                    advertising_handle,
                    sync_handle,
                }
            }
            HCI_LE_CS_READ_REMOTE_SUPPORTED_CAPABILITIES_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let num_config_supported = r.u8()?;
                let max_consecutive_procedures_supported = r.u16_le()?;
                let num_antennas_supported = r.u8()?;
                let max_antenna_paths_supported = r.u8()?;
                let roles_supported = r.u8()?;
                let modes_supported = r.u8()?;
                let rtt_capability = r.u8()?;
                let rtt_aa_only_n = r.u8()?;
                let rtt_sounding_n = r.u8()?;
                let rtt_random_sequence_n = r.u8()?;
                let nadm_sounding_capability = r.u16_le()?;
                let nadm_random_capability = r.u16_le()?;
                let cs_sync_phys_supported = r.u8()?;
                let subfeatures_supported = r.u16_le()?;
                let t_ip1_times_supported = r.u16_le()?;
                let t_ip2_times_supported = r.u16_le()?;
                let t_fcs_times_supported = r.u16_le()?;
                let t_pm_times_supported = r.u16_le()?;
                let t_sw_time_supported = r.u8()?;
                let tx_snr_capability = r.u8()?;
                LeMetaEvent::CsReadRemoteSupportedCapabilitiesComplete {
                    status,
                    connection_handle,
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
                }
            }
            HCI_LE_CS_READ_REMOTE_FAE_TABLE_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let remote_fae_table = r.array::<72>()?;
                LeMetaEvent::CsReadRemoteFaeTableComplete {
                    status,
                    connection_handle,
                    remote_fae_table,
                }
            }
            HCI_LE_CS_SECURITY_ENABLE_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                LeMetaEvent::CsSecurityEnableComplete {
                    status,
                    connection_handle,
                }
            }
            HCI_LE_CS_CONFIG_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let config_id = r.u8()?;
                let action = r.u8()?;
                let main_mode_type = r.u8()?;
                let sub_mode_type = r.u8()?;
                let min_main_mode_steps = r.u8()?;
                let max_main_mode_steps = r.u8()?;
                let main_mode_repetition = r.u8()?;
                let mode_0_steps = r.u8()?;
                let role = r.u8()?;
                let rtt_type = r.u8()?;
                let cs_sync_phy = r.u8()?;
                let channel_map = r.array::<10>()?;
                let channel_map_repetition = r.u8()?;
                let channel_selection_type = r.u8()?;
                let ch3c_shape = r.u8()?;
                let ch3c_jump = r.u8()?;
                let reserved = r.u8()?;
                let t_ip1_time = r.u8()?;
                let t_ip2_time = r.u8()?;
                let t_fcs_time = r.u8()?;
                let t_pm_time = r.u8()?;
                LeMetaEvent::CsConfigComplete {
                    status,
                    connection_handle,
                    config_id,
                    action,
                    main_mode_type,
                    sub_mode_type,
                    min_main_mode_steps,
                    max_main_mode_steps,
                    main_mode_repetition,
                    mode_0_steps,
                    role,
                    rtt_type,
                    cs_sync_phy,
                    channel_map,
                    channel_map_repetition,
                    channel_selection_type,
                    ch3c_shape,
                    ch3c_jump,
                    reserved,
                    t_ip1_time,
                    t_ip2_time,
                    t_fcs_time,
                    t_pm_time,
                }
            }
            HCI_LE_CS_PROCEDURE_ENABLE_COMPLETE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let config_id = r.u8()?;
                let state = r.u8()?;
                let tone_antenna_config_selection = r.u8()?;
                let selected_tx_power = r.u8()? as i8;
                let subevent_len = r.u24_le()?;
                let subevents_per_event = r.u8()?;
                let subevent_interval = r.u16_le()?;
                let event_interval = r.u16_le()?;
                let procedure_interval = r.u16_le()?;
                let procedure_count = r.u16_le()?;
                let max_procedure_len = r.u16_le()?;
                LeMetaEvent::CsProcedureEnableComplete {
                    status,
                    connection_handle,
                    config_id,
                    state,
                    tone_antenna_config_selection,
                    selected_tx_power,
                    subevent_len,
                    subevents_per_event,
                    subevent_interval,
                    event_interval,
                    procedure_interval,
                    procedure_count,
                    max_procedure_len,
                }
            }
            HCI_LE_CS_SUBEVENT_RESULT_EVENT => {
                let connection_handle = r.u16_le()?;
                let config_id = r.u8()?;
                let start_acl_conn_event_counter = r.u16_le()?;
                let procedure_counter = r.u16_le()?;
                let frequency_compensation = r.u16_le()?;
                let reference_power_level = r.u8()? as i8;
                let procedure_done_status = r.u8()?;
                let subevent_done_status = r.u8()?;
                let abort_reason = r.u8()?;
                let num_antenna_paths = r.u8()?;
                let count0 = r.u8()? as usize;
                let mut step_mode = Vec::with_capacity(count0);
                let mut step_channel = Vec::with_capacity(count0);
                let mut step_data = Vec::with_capacity(count0);
                for _ in 0..count0 {
                    step_mode.push(r.u8()?);
                    step_channel.push(r.u8()?);
                    step_data.push({
                        let n = r.u8()? as usize;
                        r.take(n)?.to_vec()
                    });
                }
                LeMetaEvent::CsSubeventResult {
                    connection_handle,
                    config_id,
                    start_acl_conn_event_counter,
                    procedure_counter,
                    frequency_compensation,
                    reference_power_level,
                    procedure_done_status,
                    subevent_done_status,
                    abort_reason,
                    num_antenna_paths,
                    step_mode,
                    step_channel,
                    step_data,
                }
            }
            HCI_LE_CS_SUBEVENT_RESULT_CONTINUE_EVENT => {
                let connection_handle = r.u16_le()?;
                let config_id = r.u8()?;
                let procedure_done_status = r.u8()?;
                let subevent_done_status = r.u8()?;
                let abort_reason = r.u8()?;
                let num_antenna_paths = r.u8()?;
                let count0 = r.u8()? as usize;
                let mut step_mode = Vec::with_capacity(count0);
                let mut step_channel = Vec::with_capacity(count0);
                let mut step_data = Vec::with_capacity(count0);
                for _ in 0..count0 {
                    step_mode.push(r.u8()?);
                    step_channel.push(r.u8()?);
                    step_data.push({
                        let n = r.u8()? as usize;
                        r.take(n)?.to_vec()
                    });
                }
                LeMetaEvent::CsSubeventResultContinue {
                    connection_handle,
                    config_id,
                    procedure_done_status,
                    subevent_done_status,
                    abort_reason,
                    num_antenna_paths,
                    step_mode,
                    step_channel,
                    step_data,
                }
            }
            HCI_LE_CS_TEST_END_COMPLETE_EVENT => {
                let connection_handle = r.u16_le()?;
                let status = r.u8()?;
                LeMetaEvent::CsTestEndComplete {
                    connection_handle,
                    status,
                }
            }
            HCI_LE_CONNECTION_RATE_CHANGE_EVENT => {
                let status = r.u8()?;
                let connection_handle = r.u16_le()?;
                let connection_interval = r.u16_le()?;
                let subrate_factor = r.u16_le()?;
                let peripheral_latency = r.u16_le()?;
                let continuation_number = r.u16_le()?;
                let supervision_timeout = r.u16_le()?;
                LeMetaEvent::ConnectionRateChange {
                    status,
                    connection_handle,
                    connection_interval,
                    subrate_factor,
                    peripheral_latency,
                    continuation_number,
                    supervision_timeout,
                }
            }
            HCI_LE_ADVERTISING_REPORT_EVENT => {
                let num_reports = r.u8()? as usize;
                let mut reports = Vec::with_capacity(num_reports);
                for _ in 0..num_reports {
                    let event_type = r.u8()?;
                    let address_type = r.u8()?;
                    let address = Address::from_bytes(r.array::<6>()?, AddressType(address_type));
                    let data_length = r.u8()? as usize;
                    let data = r.take(data_length)?.to_vec();
                    let rssi = r.u8()? as i8;
                    reports.push(AdvertisingReport {
                        event_type,
                        address_type,
                        address,
                        data,
                        rssi,
                    });
                }
                LeMetaEvent::AdvertisingReport { reports }
            }
            HCI_LE_EXTENDED_ADVERTISING_REPORT_EVENT => {
                let num_reports = r.u8()? as usize;
                let mut reports = Vec::with_capacity(num_reports);
                for _ in 0..num_reports {
                    let event_type = r.u16_le()?;
                    let address_type = r.u8()?;
                    let address = Address::from_bytes(r.array::<6>()?, AddressType(address_type));
                    let primary_phy = r.u8()?;
                    let secondary_phy = r.u8()?;
                    let advertising_sid = r.u8()?;
                    let tx_power = r.u8()? as i8;
                    let rssi = r.u8()? as i8;
                    let periodic_advertising_interval = r.u16_le()?;
                    let direct_address_type = r.u8()?;
                    let direct_address =
                        Address::from_bytes(r.array::<6>()?, AddressType(direct_address_type));
                    let data_length = r.u8()? as usize;
                    let data = r.take(data_length)?.to_vec();
                    reports.push(ExtendedAdvertisingReport {
                        event_type,
                        address_type,
                        address,
                        primary_phy,
                        secondary_phy,
                        advertising_sid,
                        tx_power,
                        rssi,
                        periodic_advertising_interval,
                        direct_address_type,
                        direct_address,
                        data,
                    });
                }
                LeMetaEvent::ExtendedAdvertisingReport { reports }
            }
            _ => LeMetaEvent::Generic {
                subevent_code,
                parameters: fields.to_vec(),
            },
        })
    }
}
