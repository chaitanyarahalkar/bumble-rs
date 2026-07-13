//! HCI Command packets (Vol 2, Part E - 5.4.1).
//!
//! Wire form: `[0x01, op_code_lo, op_code_hi, param_len, parameters…]`.
//!
//! The [`Command`] enum is GENERATED from upstream `bumble.hci` (see
//! `tools/hcigen`): one typed variant per command class, plus [`Command::Generic`]
//! for op codes with no typed model. Two phys-derived array commands
//! (`LE_Set_Extended_Scan_Parameters`, `LE_Extended_Create_Connection`) are
//! hand-written because their array count comes from a PHY bitmask, not a
//! leading count byte.

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

    pub fn to_bytes(self) -> [u8; 5] {
        let mut out = [0u8; 5];
        out[0] = self.coding_format;
        out[1..3].copy_from_slice(&self.company_id.to_le_bytes());
        out[3..5].copy_from_slice(&self.vendor_specific_codec_id.to_le_bytes());
        out
    }

    /// Parse the exact five-byte HCI Coding Format representation.
    pub fn from_bytes(data: &[u8]) -> Result<CodingFormat> {
        if data.len() != 5 {
            return Err(Error::InvalidPacket(format!(
                "coding format has length {}, expected 5",
                data.len()
            )));
        }
        Self::read(&mut Reader::new(data, 0))
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
/// preserves the raw parameters for op codes with no typed model.
#[allow(clippy::large_enum_variant, clippy::enum_variant_names)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Inquiry {
        lap: u32,
        inquiry_length: u8,
        num_responses: u8,
    },
    InquiryCancel,
    CreateConnection {
        bd_addr: Address,
        packet_type: u16,
        page_scan_repetition_mode: u8,
        reserved: u8,
        clock_offset: u16,
        allow_role_switch: u8,
    },
    Disconnect {
        connection_handle: u16,
        reason: u8,
    },
    CreateConnectionCancel {
        bd_addr: Address,
    },
    AcceptConnectionRequest {
        bd_addr: Address,
        role: u8,
    },
    RejectConnectionRequest {
        bd_addr: Address,
        reason: u8,
    },
    LinkKeyRequestReply {
        bd_addr: Address,
        link_key: [u8; 16],
    },
    LinkKeyRequestNegativeReply {
        bd_addr: Address,
    },
    PinCodeRequestReply {
        bd_addr: Address,
        pin_code_length: u8,
        pin_code: [u8; 16],
    },
    PinCodeRequestNegativeReply {
        bd_addr: Address,
    },
    ChangeConnectionPacketType {
        connection_handle: u16,
        packet_type: u16,
    },
    AuthenticationRequested {
        connection_handle: u16,
    },
    SetConnectionEncryption {
        connection_handle: u16,
        encryption_enable: u8,
    },
    RemoteNameRequest {
        bd_addr: Address,
        page_scan_repetition_mode: u8,
        reserved: u8,
        clock_offset: u16,
    },
    ReadRemoteSupportedFeatures {
        connection_handle: u16,
    },
    ReadRemoteExtendedFeatures {
        connection_handle: u16,
        page_number: u8,
    },
    ReadRemoteVersionInformation {
        connection_handle: u16,
    },
    ReadClockOffset {
        connection_handle: u16,
    },
    AcceptSynchronousConnectionRequest {
        bd_addr: Address,
        transmit_bandwidth: u32,
        receive_bandwidth: u32,
        max_latency: u16,
        voice_setting: u16,
        retransmission_effort: u8,
        packet_type: u16,
    },
    RejectSynchronousConnectionRequest {
        bd_addr: Address,
        reason: u8,
    },
    IoCapabilityRequestReply {
        bd_addr: Address,
        io_capability: u8,
        oob_data_present: u8,
        authentication_requirements: u8,
    },
    UserConfirmationRequestReply {
        bd_addr: Address,
    },
    UserConfirmationRequestNegativeReply {
        bd_addr: Address,
    },
    UserPasskeyRequestReply {
        bd_addr: Address,
        numeric_value: u32,
    },
    UserPasskeyRequestNegativeReply {
        bd_addr: Address,
    },
    RemoteOobDataRequestReply {
        bd_addr: Address,
        c: [u8; 16],
        r: [u8; 16],
    },
    RemoteOobDataRequestNegativeReply {
        bd_addr: Address,
    },
    IoCapabilityRequestNegativeReply {
        bd_addr: Address,
        reason: u8,
    },
    EnhancedSetupSynchronousConnection {
        connection_handle: u16,
        transmit_bandwidth: u32,
        receive_bandwidth: u32,
        transmit_coding_format: CodingFormat,
        receive_coding_format: CodingFormat,
        transmit_codec_frame_size: u16,
        receive_codec_frame_size: u16,
        input_bandwidth: u32,
        output_bandwidth: u32,
        input_coding_format: CodingFormat,
        output_coding_format: CodingFormat,
        input_coded_data_size: u16,
        output_coded_data_size: u16,
        input_pcm_data_format: u8,
        output_pcm_data_format: u8,
        input_pcm_sample_payload_msb_position: u8,
        output_pcm_sample_payload_msb_position: u8,
        input_data_path: u8,
        output_data_path: u8,
        input_transport_unit_size: u8,
        output_transport_unit_size: u8,
        max_latency: u16,
        packet_type: u16,
        retransmission_effort: u8,
    },
    EnhancedAcceptSynchronousConnectionRequest {
        bd_addr: Address,
        transmit_bandwidth: u32,
        receive_bandwidth: u32,
        transmit_coding_format: CodingFormat,
        receive_coding_format: CodingFormat,
        transmit_codec_frame_size: u16,
        receive_codec_frame_size: u16,
        input_bandwidth: u32,
        output_bandwidth: u32,
        input_coding_format: CodingFormat,
        output_coding_format: CodingFormat,
        input_coded_data_size: u16,
        output_coded_data_size: u16,
        input_pcm_data_format: u8,
        output_pcm_data_format: u8,
        input_pcm_sample_payload_msb_position: u8,
        output_pcm_sample_payload_msb_position: u8,
        input_data_path: u8,
        output_data_path: u8,
        input_transport_unit_size: u8,
        output_transport_unit_size: u8,
        max_latency: u16,
        packet_type: u16,
        retransmission_effort: u8,
    },
    TruncatedPage {
        bd_addr: Address,
        page_scan_repetition_mode: u8,
        clock_offset: u16,
    },
    TruncatedPageCancel {
        bd_addr: Address,
    },
    SetConnectionlessPeripheralBroadcast {
        enable: u8,
        lt_addr: u8,
        lpo_allowed: u8,
        packet_type: u16,
        interval_min: u16,
        interval_max: u16,
        supervision_timeout: u16,
    },
    SetConnectionlessPeripheralBroadcastReceive {
        enable: u8,
        bd_addr: Address,
        lt_addr: u8,
        interval: u16,
        clock_offset: u32,
        next_connectionless_peripheral_broadcast_clock: u32,
        supervision_timeout: u16,
        remote_timing_accuracy: u8,
        skip: u8,
        packet_type: u16,
        afh_channel_map: [u8; 10],
    },
    StartSynchronizationTrain,
    ReceiveSynchronizationTrain {
        bd_addr: Address,
        sync_scan_timeout: u16,
        sync_scan_window: u16,
        sync_scan_interval: u16,
    },
    RemoteOobExtendedDataRequestReply {
        bd_addr: Address,
        c_192: [u8; 16],
        r_192: [u8; 16],
        c_256: [u8; 16],
        r_256: [u8; 16],
    },
    SniffMode {
        connection_handle: u16,
        sniff_max_interval: u16,
        sniff_min_interval: u16,
        sniff_attempt: u16,
        sniff_timeout: u16,
    },
    ExitSniffMode {
        connection_handle: u16,
    },
    SwitchRole {
        bd_addr: Address,
        role: u8,
    },
    WriteLinkPolicySettings {
        connection_handle: u16,
        link_policy_settings: u16,
    },
    WriteDefaultLinkPolicySettings {
        default_link_policy_settings: u16,
    },
    SniffSubrating {
        connection_handle: u16,
        maximum_latency: u16,
        minimum_remote_timeout: u16,
        minimum_local_timeout: u16,
    },
    SetEventMask {
        event_mask: [u8; 8],
    },
    Reset,
    SetEventFilter {
        filter_type: u8,
        filter_condition: Vec<u8>,
    },
    ReadStoredLinkKey {
        bd_addr: Address,
        read_all_flag: u8,
    },
    DeleteStoredLinkKey {
        bd_addr: Address,
        delete_all_flag: u8,
    },
    WriteLocalName {
        local_name: [u8; 248],
    },
    ReadLocalName,
    WriteConnectionAcceptTimeout {
        connection_accept_timeout: u16,
    },
    WritePageTimeout {
        page_timeout: u16,
    },
    WriteScanEnable {
        scan_enable: u8,
    },
    ReadPageScanActivity,
    WritePageScanActivity {
        page_scan_interval: u16,
        page_scan_window: u16,
    },
    WriteInquiryScanActivity {
        inquiry_scan_interval: u16,
        inquiry_scan_window: u16,
    },
    ReadAuthenticationEnable,
    WriteAuthenticationEnable {
        authentication_enable: u8,
    },
    ReadClassOfDevice,
    WriteClassOfDevice {
        class_of_device: u32,
    },
    ReadVoiceSetting,
    WriteVoiceSetting {
        voice_setting: u16,
    },
    ReadSynchronousFlowControlEnable,
    WriteSynchronousFlowControlEnable {
        synchronous_flow_control_enable: u8,
    },
    SetControllerToHostFlowControl {
        flow_control_enable: u8,
    },
    HostBufferSize {
        host_acl_data_packet_length: u16,
        host_synchronous_data_packet_length: u8,
        host_total_num_acl_data_packets: u16,
        host_total_num_synchronous_data_packets: u16,
    },
    WriteLinkSupervisionTimeout {
        handle: u16,
        link_supervision_timeout: u16,
    },
    ReadNumberOfSupportedIac,
    ReadCurrentIacLap,
    WriteInquiryScanType {
        scan_type: u8,
    },
    WriteInquiryMode {
        inquiry_mode: u8,
    },
    ReadPageScanType,
    WritePageScanType {
        page_scan_type: u8,
    },
    WriteExtendedInquiryResponse {
        fec_required: u8,
        extended_inquiry_response: [u8; 240],
    },
    WriteSimplePairingMode {
        simple_pairing_mode: u8,
    },
    ReadLocalOobData,
    ReadInquiryResponseTransmitPowerLevel,
    ReadDefaultErroneousDataReporting,
    SetEventMaskPage2 {
        event_mask_page_2: [u8; 8],
    },
    ReadLeHostSupport,
    WriteLeHostSupport {
        le_supported_host: u8,
        simultaneous_le_host: u8,
    },
    WriteSecureConnectionsHostSupport {
        secure_connections_host_support: u8,
    },
    WriteAuthenticatedPayloadTimeout {
        connection_handle: u16,
        authenticated_payload_timeout: u16,
    },
    ReadLocalOobExtendedData,
    ConfigureDataPath {
        data_path_direction: u8,
        data_path_id: u8,
        vendor_specific_config: Vec<u8>,
    },
    ReadLocalVersionInformation,
    ReadLocalSupportedCommands,
    ReadLocalSupportedFeatures,
    ReadLocalExtendedFeatures {
        page_number: u8,
    },
    ReadBufferSize,
    ReadBdAddr,
    ReadLocalSupportedCodecs,
    ReadLocalSupportedCodecsV2,
    ReadRssi {
        handle: u16,
    },
    ReadEncryptionKeySize {
        connection_handle: u16,
    },
    ReadLoopbackMode,
    WriteLoopbackMode {
        loopback_mode: u8,
    },
    LeSetEventMask {
        le_event_mask: [u8; 8],
    },
    LeReadBufferSize,
    LeReadLocalSupportedFeatures,
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
    LeReadAdvertisingPhysicalChannelTxPower,
    LeSetAdvertisingData {
        advertising_data: Vec<u8>,
    },
    LeSetScanResponseData {
        scan_response_data: Vec<u8>,
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
    LeCreateConnectionCancel,
    LeReadFilterAcceptListSize,
    LeClearFilterAcceptList,
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
    LeRand,
    LeEnableEncryption {
        connection_handle: u16,
        random_number: [u8; 8],
        encrypted_diversifier: u16,
        long_term_key: [u8; 16],
    },
    LeLongTermKeyRequestReply {
        connection_handle: u16,
        long_term_key: [u8; 16],
    },
    LeLongTermKeyRequestNegativeReply {
        connection_handle: u16,
    },
    LeReadSupportedStates,
    LeRemoteConnectionParameterRequestReply {
        connection_handle: u16,
        interval_min: u16,
        interval_max: u16,
        max_latency: u16,
        timeout: u16,
        min_ce_length: u16,
        max_ce_length: u16,
    },
    LeRemoteConnectionParameterRequestNegativeReply {
        connection_handle: u16,
        reason: u8,
    },
    LeSetDataLength {
        connection_handle: u16,
        tx_octets: u16,
        tx_time: u16,
    },
    LeReadSuggestedDefaultDataLength,
    LeWriteSuggestedDefaultDataLength {
        suggested_max_tx_octets: u16,
        suggested_max_tx_time: u16,
    },
    LeReadLocalP256PublicKey,
    LeAddDeviceToResolvingList {
        peer_identity_address_type: u8,
        peer_identity_address: Address,
        peer_irk: [u8; 16],
        local_irk: [u8; 16],
    },
    LeClearResolvingList,
    LeReadResolvingListSize,
    LeSetAddressResolutionEnable {
        address_resolution_enable: u8,
    },
    LeSetResolvablePrivateAddressTimeout {
        rpa_timeout: u16,
    },
    LeReadMaximumDataLength,
    LeReadPhy {
        connection_handle: u16,
    },
    LeSetDefaultPhy {
        all_phys: u8,
        tx_phys: u8,
        rx_phys: u8,
    },
    LeSetPhy {
        connection_handle: u16,
        all_phys: u8,
        tx_phys: u8,
        rx_phys: u8,
        phy_options: u16,
    },
    LeSetAdvertisingSetRandomAddress {
        advertising_handle: u8,
        random_address: Address,
    },
    LeSetExtendedAdvertisingParameters {
        advertising_handle: u8,
        advertising_event_properties: u16,
        primary_advertising_interval_min: u32,
        primary_advertising_interval_max: u32,
        primary_advertising_channel_map: u8,
        own_address_type: u8,
        peer_address_type: u8,
        peer_address: Address,
        advertising_filter_policy: u8,
        advertising_tx_power: u8,
        primary_advertising_phy: u8,
        secondary_advertising_max_skip: u8,
        secondary_advertising_phy: u8,
        advertising_sid: u8,
        scan_request_notification_enable: u8,
    },
    LeSetExtendedAdvertisingData {
        advertising_handle: u8,
        operation: u8,
        fragment_preference: u8,
        advertising_data: Vec<u8>,
    },
    LeSetExtendedScanResponseData {
        advertising_handle: u8,
        operation: u8,
        fragment_preference: u8,
        scan_response_data: Vec<u8>,
    },
    LeSetExtendedAdvertisingEnable {
        enable: u8,
        advertising_handles: Vec<u8>,
        durations: Vec<u16>,
        max_extended_advertising_events: Vec<u8>,
    },
    LeReadMaximumAdvertisingDataLength,
    LeReadNumberOfSupportedAdvertisingSets,
    LeRemoveAdvertisingSet {
        advertising_handle: u8,
    },
    LeClearAdvertisingSets,
    LeSetPeriodicAdvertisingParameters {
        advertising_handle: u8,
        periodic_advertising_interval_min: u16,
        periodic_advertising_interval_max: u16,
        periodic_advertising_properties: u16,
    },
    LeSetPeriodicAdvertisingData {
        advertising_handle: u8,
        operation: u8,
        advertising_data: Vec<u8>,
    },
    LeSetPeriodicAdvertisingEnable {
        enable: u8,
        advertising_handle: u8,
    },
    LeSetExtendedScanEnable {
        enable: u8,
        filter_duplicates: u8,
        duration: u16,
        period: u16,
    },
    LePeriodicAdvertisingCreateSync {
        options: u8,
        advertising_sid: u8,
        advertiser_address_type: u8,
        advertiser_address: Address,
        skip: u16,
        sync_timeout: u16,
        sync_cte_type: u8,
    },
    LePeriodicAdvertisingCreateSyncCancel,
    LePeriodicAdvertisingTerminateSync {
        sync_handle: u16,
    },
    LeReadTransmitPower,
    LeSetPrivacyMode {
        peer_identity_address_type: u8,
        peer_identity_address: Address,
        privacy_mode: u8,
    },
    LeSetPeriodicAdvertisingReceiveEnable {
        sync_handle: u16,
        enable: u8,
    },
    LePeriodicAdvertisingSyncTransfer {
        connection_handle: u16,
        service_data: u16,
        sync_handle: u16,
    },
    LePeriodicAdvertisingSetInfoTransfer {
        connection_handle: u16,
        service_data: u16,
        advertising_handle: u8,
    },
    LeSetPeriodicAdvertisingSyncTransferParameters {
        connection_handle: u16,
        mode: u8,
        skip: u16,
        sync_timeout: u16,
        cte_type: u8,
    },
    LeSetDefaultPeriodicAdvertisingSyncTransferParameters {
        mode: u8,
        skip: u16,
        sync_timeout: u16,
        cte_type: u8,
    },
    LeReadBufferSizeV2,
    LeReadIsoTxSync {
        connection_handle: u16,
    },
    LeSetCigParameters {
        cig_id: u8,
        sdu_interval_c_to_p: u32,
        sdu_interval_p_to_c: u32,
        worst_case_sca: u8,
        packing: u8,
        framing: u8,
        max_transport_latency_c_to_p: u16,
        max_transport_latency_p_to_c: u16,
        cis_id: Vec<u8>,
        max_sdu_c_to_p: Vec<u16>,
        max_sdu_p_to_c: Vec<u16>,
        phy_c_to_p: Vec<u8>,
        phy_p_to_c: Vec<u8>,
        rtn_c_to_p: Vec<u8>,
        rtn_p_to_c: Vec<u8>,
    },
    LeCreateCis {
        cis_connection_handle: Vec<u16>,
        acl_connection_handle: Vec<u16>,
    },
    LeRemoveCig {
        cig_id: u8,
    },
    LeAcceptCisRequest {
        connection_handle: u16,
    },
    LeRejectCisRequest {
        connection_handle: u16,
        reason: u8,
    },
    LeCreateBig {
        big_handle: u8,
        advertising_handle: u8,
        num_bis: u8,
        sdu_interval: u32,
        max_sdu: u16,
        max_transport_latency: u16,
        rtn: u8,
        phy: u8,
        packing: u8,
        framing: u8,
        encryption: u8,
        broadcast_code: [u8; 16],
    },
    LeTerminateBig {
        big_handle: u8,
        reason: u8,
    },
    LeBigCreateSync {
        big_handle: u8,
        sync_handle: u16,
        encryption: u8,
        broadcast_code: [u8; 16],
        mse: u8,
        big_sync_timeout: u16,
        bis: Vec<u8>,
    },
    LeBigTerminateSync {
        big_handle: u8,
    },
    LeSetupIsoDataPath {
        connection_handle: u16,
        data_path_direction: u8,
        data_path_id: u8,
        codec_id: CodingFormat,
        controller_delay: u32,
        codec_configuration: Vec<u8>,
    },
    LeRemoveIsoDataPath {
        connection_handle: u16,
        data_path_direction: u8,
    },
    LeSetHostFeature {
        bit_number: u8,
        bit_value: u8,
    },
    LeSetDefaultSubrate {
        subrate_min: u16,
        subrate_max: u16,
        max_latency: u16,
        continuation_number: u16,
        supervision_timeout: u16,
    },
    LeSubrateRequest {
        connection_handle: u16,
        subrate_min: u16,
        subrate_max: u16,
        max_latency: u16,
        continuation_number: u16,
        supervision_timeout: u16,
    },
    LeCsReadLocalSupportedCapabilities,
    LeCsReadRemoteSupportedCapabilities {
        connection_handle: u16,
    },
    LeCsWriteCachedRemoteSupportedCapabilities {
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
    LeCsSecurityEnable {
        connection_handle: u16,
    },
    LeCsSetDefaultSettings {
        connection_handle: u16,
        role_enable: u8,
        cs_sync_antenna_selection: u8,
        max_tx_power: u8,
    },
    LeCsReadRemoteFaeTable {
        connection_handle: u16,
    },
    LeCsWriteCachedRemoteFaeTable {
        connection_handle: u16,
        remote_fae_table: [u8; 72],
    },
    LeCsCreateConfig {
        connection_handle: u16,
        config_id: u8,
        create_context: u8,
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
    },
    LeCsRemoveConfig {
        connection_handle: u16,
        config_id: u8,
    },
    LeCsSetChannelClassification {
        channel_classification: [u8; 10],
    },
    LeCsSetProcedureParameters {
        connection_handle: u16,
        config_id: u8,
        max_procedure_len: u16,
        min_procedure_interval: u16,
        max_procedure_interval: u16,
        max_procedure_count: u16,
        min_subevent_len: u32,
        max_subevent_len: u32,
        tone_antenna_config_selection: u8,
        phy: u8,
        tx_power_delta: u8,
        preferred_peer_antenna: u8,
        snr_control_initiator: u8,
        snr_control_reflector: u8,
    },
    LeCsProcedureEnable {
        connection_handle: u16,
        config_id: u8,
        enable: u8,
    },
    LeCsTest {
        main_mode_type: u8,
        sub_mode_type: u8,
        main_mode_repetition: u8,
        mode_0_steps: u8,
        role: u8,
        rtt_type: u8,
        cs_sync_phy: u8,
        cs_sync_antenna_selection: u8,
        subevent_len: u32,
        subevent_interval: u16,
        max_num_subevents: u8,
        transmit_power_level: u8,
        t_ip1_time: u8,
        t_ip2_time: u8,
        t_fcs_time: u8,
        t_pm_time: u8,
        t_sw_time: u8,
        tone_antenna_config_selection: u8,
        reserved: u8,
        snr_control_initiator: u8,
        snr_control_reflector: u8,
        drbg_nonce: u16,
        channel_map_repetition: u8,
        override_config: u16,
        override_parameters_data: Vec<u8>,
    },
    LeCsTestEnd,
    LeFrameSpaceUpdate {
        connection_handle: u16,
        frame_space_min: u16,
        frame_space_max: u16,
        phys: u8,
        spacing_types: u16,
    },
    LeConnectionRateRequest {
        connection_handle: u16,
        connection_interval_min: u16,
        connection_interval_max: u16,
        subrate_min: u16,
        subrate_max: u16,
        max_latency: u16,
        continuation_number: u16,
        supervision_timeout: u16,
        min_ce_length: u16,
        max_ce_length: u16,
    },
    LeSetDefaultRateParameters {
        connection_interval_min: u16,
        connection_interval_max: u16,
        subrate_min: u16,
        subrate_max: u16,
        max_latency: u16,
        continuation_number: u16,
        supervision_timeout: u16,
        min_ce_length: u16,
        max_ce_length: u16,
    },
    LeReadMinimumSupportedConnectionInterval,
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
    /// Any command with no typed model: raw op code + parameters.
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
            Command::Inquiry { .. } => HCI_INQUIRY_COMMAND,
            Command::InquiryCancel => HCI_INQUIRY_CANCEL_COMMAND,
            Command::CreateConnection { .. } => HCI_CREATE_CONNECTION_COMMAND,
            Command::Disconnect { .. } => HCI_DISCONNECT_COMMAND,
            Command::CreateConnectionCancel { .. } => HCI_CREATE_CONNECTION_CANCEL_COMMAND,
            Command::AcceptConnectionRequest { .. } => HCI_ACCEPT_CONNECTION_REQUEST_COMMAND,
            Command::RejectConnectionRequest { .. } => HCI_REJECT_CONNECTION_REQUEST_COMMAND,
            Command::LinkKeyRequestReply { .. } => HCI_LINK_KEY_REQUEST_REPLY_COMMAND,
            Command::LinkKeyRequestNegativeReply { .. } => {
                HCI_LINK_KEY_REQUEST_NEGATIVE_REPLY_COMMAND
            }
            Command::PinCodeRequestReply { .. } => HCI_PIN_CODE_REQUEST_REPLY_COMMAND,
            Command::PinCodeRequestNegativeReply { .. } => {
                HCI_PIN_CODE_REQUEST_NEGATIVE_REPLY_COMMAND
            }
            Command::ChangeConnectionPacketType { .. } => HCI_CHANGE_CONNECTION_PACKET_TYPE_COMMAND,
            Command::AuthenticationRequested { .. } => HCI_AUTHENTICATION_REQUESTED_COMMAND,
            Command::SetConnectionEncryption { .. } => HCI_SET_CONNECTION_ENCRYPTION_COMMAND,
            Command::RemoteNameRequest { .. } => HCI_REMOTE_NAME_REQUEST_COMMAND,
            Command::ReadRemoteSupportedFeatures { .. } => {
                HCI_READ_REMOTE_SUPPORTED_FEATURES_COMMAND
            }
            Command::ReadRemoteExtendedFeatures { .. } => HCI_READ_REMOTE_EXTENDED_FEATURES_COMMAND,
            Command::ReadRemoteVersionInformation { .. } => {
                HCI_READ_REMOTE_VERSION_INFORMATION_COMMAND
            }
            Command::ReadClockOffset { .. } => HCI_READ_CLOCK_OFFSET_COMMAND,
            Command::AcceptSynchronousConnectionRequest { .. } => {
                HCI_ACCEPT_SYNCHRONOUS_CONNECTION_REQUEST_COMMAND
            }
            Command::RejectSynchronousConnectionRequest { .. } => {
                HCI_REJECT_SYNCHRONOUS_CONNECTION_REQUEST_COMMAND
            }
            Command::IoCapabilityRequestReply { .. } => HCI_IO_CAPABILITY_REQUEST_REPLY_COMMAND,
            Command::UserConfirmationRequestReply { .. } => {
                HCI_USER_CONFIRMATION_REQUEST_REPLY_COMMAND
            }
            Command::UserConfirmationRequestNegativeReply { .. } => {
                HCI_USER_CONFIRMATION_REQUEST_NEGATIVE_REPLY_COMMAND
            }
            Command::UserPasskeyRequestReply { .. } => HCI_USER_PASSKEY_REQUEST_REPLY_COMMAND,
            Command::UserPasskeyRequestNegativeReply { .. } => {
                HCI_USER_PASSKEY_REQUEST_NEGATIVE_REPLY_COMMAND
            }
            Command::RemoteOobDataRequestReply { .. } => HCI_REMOTE_OOB_DATA_REQUEST_REPLY_COMMAND,
            Command::RemoteOobDataRequestNegativeReply { .. } => {
                HCI_REMOTE_OOB_DATA_REQUEST_NEGATIVE_REPLY_COMMAND
            }
            Command::IoCapabilityRequestNegativeReply { .. } => {
                HCI_IO_CAPABILITY_REQUEST_NEGATIVE_REPLY_COMMAND
            }
            Command::EnhancedSetupSynchronousConnection { .. } => {
                HCI_ENHANCED_SETUP_SYNCHRONOUS_CONNECTION_COMMAND
            }
            Command::EnhancedAcceptSynchronousConnectionRequest { .. } => {
                HCI_ENHANCED_ACCEPT_SYNCHRONOUS_CONNECTION_REQUEST_COMMAND
            }
            Command::TruncatedPage { .. } => HCI_TRUNCATED_PAGE_COMMAND,
            Command::TruncatedPageCancel { .. } => HCI_TRUNCATED_PAGE_CANCEL_COMMAND,
            Command::SetConnectionlessPeripheralBroadcast { .. } => {
                HCI_SET_CONNECTIONLESS_PERIPHERAL_BROADCAST_COMMAND
            }
            Command::SetConnectionlessPeripheralBroadcastReceive { .. } => {
                HCI_SET_CONNECTIONLESS_PERIPHERAL_BROADCAST_RECEIVE_COMMAND
            }
            Command::StartSynchronizationTrain => HCI_START_SYNCHRONIZATION_TRAIN_COMMAND,
            Command::ReceiveSynchronizationTrain { .. } => {
                HCI_RECEIVE_SYNCHRONIZATION_TRAIN_COMMAND
            }
            Command::RemoteOobExtendedDataRequestReply { .. } => {
                HCI_REMOTE_OOB_EXTENDED_DATA_REQUEST_REPLY_COMMAND
            }
            Command::SniffMode { .. } => HCI_SNIFF_MODE_COMMAND,
            Command::ExitSniffMode { .. } => HCI_EXIT_SNIFF_MODE_COMMAND,
            Command::SwitchRole { .. } => HCI_SWITCH_ROLE_COMMAND,
            Command::WriteLinkPolicySettings { .. } => HCI_WRITE_LINK_POLICY_SETTINGS_COMMAND,
            Command::WriteDefaultLinkPolicySettings { .. } => {
                HCI_WRITE_DEFAULT_LINK_POLICY_SETTINGS_COMMAND
            }
            Command::SniffSubrating { .. } => HCI_SNIFF_SUBRATING_COMMAND,
            Command::SetEventMask { .. } => HCI_SET_EVENT_MASK_COMMAND,
            Command::Reset => HCI_RESET_COMMAND,
            Command::SetEventFilter { .. } => HCI_SET_EVENT_FILTER_COMMAND,
            Command::ReadStoredLinkKey { .. } => HCI_READ_STORED_LINK_KEY_COMMAND,
            Command::DeleteStoredLinkKey { .. } => HCI_DELETE_STORED_LINK_KEY_COMMAND,
            Command::WriteLocalName { .. } => HCI_WRITE_LOCAL_NAME_COMMAND,
            Command::ReadLocalName => HCI_READ_LOCAL_NAME_COMMAND,
            Command::WriteConnectionAcceptTimeout { .. } => {
                HCI_WRITE_CONNECTION_ACCEPT_TIMEOUT_COMMAND
            }
            Command::WritePageTimeout { .. } => HCI_WRITE_PAGE_TIMEOUT_COMMAND,
            Command::WriteScanEnable { .. } => HCI_WRITE_SCAN_ENABLE_COMMAND,
            Command::ReadPageScanActivity => HCI_READ_PAGE_SCAN_ACTIVITY_COMMAND,
            Command::WritePageScanActivity { .. } => HCI_WRITE_PAGE_SCAN_ACTIVITY_COMMAND,
            Command::WriteInquiryScanActivity { .. } => HCI_WRITE_INQUIRY_SCAN_ACTIVITY_COMMAND,
            Command::ReadAuthenticationEnable => HCI_READ_AUTHENTICATION_ENABLE_COMMAND,
            Command::WriteAuthenticationEnable { .. } => HCI_WRITE_AUTHENTICATION_ENABLE_COMMAND,
            Command::ReadClassOfDevice => HCI_READ_CLASS_OF_DEVICE_COMMAND,
            Command::WriteClassOfDevice { .. } => HCI_WRITE_CLASS_OF_DEVICE_COMMAND,
            Command::ReadVoiceSetting => HCI_READ_VOICE_SETTING_COMMAND,
            Command::WriteVoiceSetting { .. } => HCI_WRITE_VOICE_SETTING_COMMAND,
            Command::ReadSynchronousFlowControlEnable => {
                HCI_READ_SYNCHRONOUS_FLOW_CONTROL_ENABLE_COMMAND
            }
            Command::WriteSynchronousFlowControlEnable { .. } => {
                HCI_WRITE_SYNCHRONOUS_FLOW_CONTROL_ENABLE_COMMAND
            }
            Command::SetControllerToHostFlowControl { .. } => {
                HCI_SET_CONTROLLER_TO_HOST_FLOW_CONTROL_COMMAND
            }
            Command::HostBufferSize { .. } => HCI_HOST_BUFFER_SIZE_COMMAND,
            Command::WriteLinkSupervisionTimeout { .. } => {
                HCI_WRITE_LINK_SUPERVISION_TIMEOUT_COMMAND
            }
            Command::ReadNumberOfSupportedIac => HCI_READ_NUMBER_OF_SUPPORTED_IAC_COMMAND,
            Command::ReadCurrentIacLap => HCI_READ_CURRENT_IAC_LAP_COMMAND,
            Command::WriteInquiryScanType { .. } => HCI_WRITE_INQUIRY_SCAN_TYPE_COMMAND,
            Command::WriteInquiryMode { .. } => HCI_WRITE_INQUIRY_MODE_COMMAND,
            Command::ReadPageScanType => HCI_READ_PAGE_SCAN_TYPE_COMMAND,
            Command::WritePageScanType { .. } => HCI_WRITE_PAGE_SCAN_TYPE_COMMAND,
            Command::WriteExtendedInquiryResponse { .. } => {
                HCI_WRITE_EXTENDED_INQUIRY_RESPONSE_COMMAND
            }
            Command::WriteSimplePairingMode { .. } => HCI_WRITE_SIMPLE_PAIRING_MODE_COMMAND,
            Command::ReadLocalOobData => HCI_READ_LOCAL_OOB_DATA_COMMAND,
            Command::ReadInquiryResponseTransmitPowerLevel => {
                HCI_READ_INQUIRY_RESPONSE_TRANSMIT_POWER_LEVEL_COMMAND
            }
            Command::ReadDefaultErroneousDataReporting => {
                HCI_READ_DEFAULT_ERRONEOUS_DATA_REPORTING_COMMAND
            }
            Command::SetEventMaskPage2 { .. } => HCI_SET_EVENT_MASK_PAGE_2_COMMAND,
            Command::ReadLeHostSupport => HCI_READ_LE_HOST_SUPPORT_COMMAND,
            Command::WriteLeHostSupport { .. } => HCI_WRITE_LE_HOST_SUPPORT_COMMAND,
            Command::WriteSecureConnectionsHostSupport { .. } => {
                HCI_WRITE_SECURE_CONNECTIONS_HOST_SUPPORT_COMMAND
            }
            Command::WriteAuthenticatedPayloadTimeout { .. } => {
                HCI_WRITE_AUTHENTICATED_PAYLOAD_TIMEOUT_COMMAND
            }
            Command::ReadLocalOobExtendedData => HCI_READ_LOCAL_OOB_EXTENDED_DATA_COMMAND,
            Command::ConfigureDataPath { .. } => HCI_CONFIGURE_DATA_PATH_COMMAND,
            Command::ReadLocalVersionInformation => HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND,
            Command::ReadLocalSupportedCommands => HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND,
            Command::ReadLocalSupportedFeatures => HCI_READ_LOCAL_SUPPORTED_FEATURES_COMMAND,
            Command::ReadLocalExtendedFeatures { .. } => HCI_READ_LOCAL_EXTENDED_FEATURES_COMMAND,
            Command::ReadBufferSize => HCI_READ_BUFFER_SIZE_COMMAND,
            Command::ReadBdAddr => HCI_READ_BD_ADDR_COMMAND,
            Command::ReadLocalSupportedCodecs => HCI_READ_LOCAL_SUPPORTED_CODECS_COMMAND,
            Command::ReadLocalSupportedCodecsV2 => HCI_READ_LOCAL_SUPPORTED_CODECS_V2_COMMAND,
            Command::ReadRssi { .. } => HCI_READ_RSSI_COMMAND,
            Command::ReadEncryptionKeySize { .. } => HCI_READ_ENCRYPTION_KEY_SIZE_COMMAND,
            Command::ReadLoopbackMode => HCI_READ_LOOPBACK_MODE_COMMAND,
            Command::WriteLoopbackMode { .. } => HCI_WRITE_LOOPBACK_MODE_COMMAND,
            Command::LeSetEventMask { .. } => HCI_LE_SET_EVENT_MASK_COMMAND,
            Command::LeReadBufferSize => HCI_LE_READ_BUFFER_SIZE_COMMAND,
            Command::LeReadLocalSupportedFeatures => HCI_LE_READ_LOCAL_SUPPORTED_FEATURES_COMMAND,
            Command::LeSetRandomAddress { .. } => HCI_LE_SET_RANDOM_ADDRESS_COMMAND,
            Command::LeSetAdvertisingParameters { .. } => HCI_LE_SET_ADVERTISING_PARAMETERS_COMMAND,
            Command::LeReadAdvertisingPhysicalChannelTxPower => {
                HCI_LE_READ_ADVERTISING_PHYSICAL_CHANNEL_TX_POWER_COMMAND
            }
            Command::LeSetAdvertisingData { .. } => HCI_LE_SET_ADVERTISING_DATA_COMMAND,
            Command::LeSetScanResponseData { .. } => HCI_LE_SET_SCAN_RESPONSE_DATA_COMMAND,
            Command::LeSetAdvertisingEnable { .. } => HCI_LE_SET_ADVERTISING_ENABLE_COMMAND,
            Command::LeSetScanParameters { .. } => HCI_LE_SET_SCAN_PARAMETERS_COMMAND,
            Command::LeSetScanEnable { .. } => HCI_LE_SET_SCAN_ENABLE_COMMAND,
            Command::LeCreateConnection { .. } => HCI_LE_CREATE_CONNECTION_COMMAND,
            Command::LeCreateConnectionCancel => HCI_LE_CREATE_CONNECTION_CANCEL_COMMAND,
            Command::LeReadFilterAcceptListSize => HCI_LE_READ_FILTER_ACCEPT_LIST_SIZE_COMMAND,
            Command::LeClearFilterAcceptList => HCI_LE_CLEAR_FILTER_ACCEPT_LIST_COMMAND,
            Command::LeAddDeviceToFilterAcceptList { .. } => {
                HCI_LE_ADD_DEVICE_TO_FILTER_ACCEPT_LIST_COMMAND
            }
            Command::LeRemoveDeviceFromFilterAcceptList { .. } => {
                HCI_LE_REMOVE_DEVICE_FROM_FILTER_ACCEPT_LIST_COMMAND
            }
            Command::LeConnectionUpdate { .. } => HCI_LE_CONNECTION_UPDATE_COMMAND,
            Command::LeReadRemoteFeatures { .. } => HCI_LE_READ_REMOTE_FEATURES_COMMAND,
            Command::LeRand => HCI_LE_RAND_COMMAND,
            Command::LeEnableEncryption { .. } => HCI_LE_ENABLE_ENCRYPTION_COMMAND,
            Command::LeLongTermKeyRequestReply { .. } => HCI_LE_LONG_TERM_KEY_REQUEST_REPLY_COMMAND,
            Command::LeLongTermKeyRequestNegativeReply { .. } => {
                HCI_LE_LONG_TERM_KEY_REQUEST_NEGATIVE_REPLY_COMMAND
            }
            Command::LeReadSupportedStates => HCI_LE_READ_SUPPORTED_STATES_COMMAND,
            Command::LeRemoteConnectionParameterRequestReply { .. } => {
                HCI_LE_REMOTE_CONNECTION_PARAMETER_REQUEST_REPLY_COMMAND
            }
            Command::LeRemoteConnectionParameterRequestNegativeReply { .. } => {
                HCI_LE_REMOTE_CONNECTION_PARAMETER_REQUEST_NEGATIVE_REPLY_COMMAND
            }
            Command::LeSetDataLength { .. } => HCI_LE_SET_DATA_LENGTH_COMMAND,
            Command::LeReadSuggestedDefaultDataLength => {
                HCI_LE_READ_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND
            }
            Command::LeWriteSuggestedDefaultDataLength { .. } => {
                HCI_LE_WRITE_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND
            }
            Command::LeReadLocalP256PublicKey => HCI_LE_READ_LOCAL_P_256_PUBLIC_KEY_COMMAND,
            Command::LeAddDeviceToResolvingList { .. } => {
                HCI_LE_ADD_DEVICE_TO_RESOLVING_LIST_COMMAND
            }
            Command::LeClearResolvingList => HCI_LE_CLEAR_RESOLVING_LIST_COMMAND,
            Command::LeReadResolvingListSize => HCI_LE_READ_RESOLVING_LIST_SIZE_COMMAND,
            Command::LeSetAddressResolutionEnable { .. } => {
                HCI_LE_SET_ADDRESS_RESOLUTION_ENABLE_COMMAND
            }
            Command::LeSetResolvablePrivateAddressTimeout { .. } => {
                HCI_LE_SET_RESOLVABLE_PRIVATE_ADDRESS_TIMEOUT_COMMAND
            }
            Command::LeReadMaximumDataLength => HCI_LE_READ_MAXIMUM_DATA_LENGTH_COMMAND,
            Command::LeReadPhy { .. } => HCI_LE_READ_PHY_COMMAND,
            Command::LeSetDefaultPhy { .. } => HCI_LE_SET_DEFAULT_PHY_COMMAND,
            Command::LeSetPhy { .. } => HCI_LE_SET_PHY_COMMAND,
            Command::LeSetAdvertisingSetRandomAddress { .. } => {
                HCI_LE_SET_ADVERTISING_SET_RANDOM_ADDRESS_COMMAND
            }
            Command::LeSetExtendedAdvertisingParameters { .. } => {
                HCI_LE_SET_EXTENDED_ADVERTISING_PARAMETERS_COMMAND
            }
            Command::LeSetExtendedAdvertisingData { .. } => {
                HCI_LE_SET_EXTENDED_ADVERTISING_DATA_COMMAND
            }
            Command::LeSetExtendedScanResponseData { .. } => {
                HCI_LE_SET_EXTENDED_SCAN_RESPONSE_DATA_COMMAND
            }
            Command::LeSetExtendedAdvertisingEnable { .. } => {
                HCI_LE_SET_EXTENDED_ADVERTISING_ENABLE_COMMAND
            }
            Command::LeReadMaximumAdvertisingDataLength => {
                HCI_LE_READ_MAXIMUM_ADVERTISING_DATA_LENGTH_COMMAND
            }
            Command::LeReadNumberOfSupportedAdvertisingSets => {
                HCI_LE_READ_NUMBER_OF_SUPPORTED_ADVERTISING_SETS_COMMAND
            }
            Command::LeRemoveAdvertisingSet { .. } => HCI_LE_REMOVE_ADVERTISING_SET_COMMAND,
            Command::LeClearAdvertisingSets => HCI_LE_CLEAR_ADVERTISING_SETS_COMMAND,
            Command::LeSetPeriodicAdvertisingParameters { .. } => {
                HCI_LE_SET_PERIODIC_ADVERTISING_PARAMETERS_COMMAND
            }
            Command::LeSetPeriodicAdvertisingData { .. } => {
                HCI_LE_SET_PERIODIC_ADVERTISING_DATA_COMMAND
            }
            Command::LeSetPeriodicAdvertisingEnable { .. } => {
                HCI_LE_SET_PERIODIC_ADVERTISING_ENABLE_COMMAND
            }
            Command::LeSetExtendedScanEnable { .. } => HCI_LE_SET_EXTENDED_SCAN_ENABLE_COMMAND,
            Command::LePeriodicAdvertisingCreateSync { .. } => {
                HCI_LE_PERIODIC_ADVERTISING_CREATE_SYNC_COMMAND
            }
            Command::LePeriodicAdvertisingCreateSyncCancel => {
                HCI_LE_PERIODIC_ADVERTISING_CREATE_SYNC_CANCEL_COMMAND
            }
            Command::LePeriodicAdvertisingTerminateSync { .. } => {
                HCI_LE_PERIODIC_ADVERTISING_TERMINATE_SYNC_COMMAND
            }
            Command::LeReadTransmitPower => HCI_LE_READ_TRANSMIT_POWER_COMMAND,
            Command::LeSetPrivacyMode { .. } => HCI_LE_SET_PRIVACY_MODE_COMMAND,
            Command::LeSetPeriodicAdvertisingReceiveEnable { .. } => {
                HCI_LE_SET_PERIODIC_ADVERTISING_RECEIVE_ENABLE_COMMAND
            }
            Command::LePeriodicAdvertisingSyncTransfer { .. } => {
                HCI_LE_PERIODIC_ADVERTISING_SYNC_TRANSFER_COMMAND
            }
            Command::LePeriodicAdvertisingSetInfoTransfer { .. } => {
                HCI_LE_PERIODIC_ADVERTISING_SET_INFO_TRANSFER_COMMAND
            }
            Command::LeSetPeriodicAdvertisingSyncTransferParameters { .. } => {
                HCI_LE_SET_PERIODIC_ADVERTISING_SYNC_TRANSFER_PARAMETERS_COMMAND
            }
            Command::LeSetDefaultPeriodicAdvertisingSyncTransferParameters { .. } => {
                HCI_LE_SET_DEFAULT_PERIODIC_ADVERTISING_SYNC_TRANSFER_PARAMETERS_COMMAND
            }
            Command::LeReadBufferSizeV2 => HCI_LE_READ_BUFFER_SIZE_V2_COMMAND,
            Command::LeReadIsoTxSync { .. } => HCI_LE_READ_ISO_TX_SYNC_COMMAND,
            Command::LeSetCigParameters { .. } => HCI_LE_SET_CIG_PARAMETERS_COMMAND,
            Command::LeCreateCis { .. } => HCI_LE_CREATE_CIS_COMMAND,
            Command::LeRemoveCig { .. } => HCI_LE_REMOVE_CIG_COMMAND,
            Command::LeAcceptCisRequest { .. } => HCI_LE_ACCEPT_CIS_REQUEST_COMMAND,
            Command::LeRejectCisRequest { .. } => HCI_LE_REJECT_CIS_REQUEST_COMMAND,
            Command::LeCreateBig { .. } => HCI_LE_CREATE_BIG_COMMAND,
            Command::LeTerminateBig { .. } => HCI_LE_TERMINATE_BIG_COMMAND,
            Command::LeBigCreateSync { .. } => HCI_LE_BIG_CREATE_SYNC_COMMAND,
            Command::LeBigTerminateSync { .. } => HCI_LE_BIG_TERMINATE_SYNC_COMMAND,
            Command::LeSetupIsoDataPath { .. } => HCI_LE_SETUP_ISO_DATA_PATH_COMMAND,
            Command::LeRemoveIsoDataPath { .. } => HCI_LE_REMOVE_ISO_DATA_PATH_COMMAND,
            Command::LeSetHostFeature { .. } => HCI_LE_SET_HOST_FEATURE_COMMAND,
            Command::LeSetDefaultSubrate { .. } => HCI_LE_SET_DEFAULT_SUBRATE_COMMAND,
            Command::LeSubrateRequest { .. } => HCI_LE_SUBRATE_REQUEST_COMMAND,
            Command::LeCsReadLocalSupportedCapabilities => {
                HCI_LE_CS_READ_LOCAL_SUPPORTED_CAPABILITIES_COMMAND
            }
            Command::LeCsReadRemoteSupportedCapabilities { .. } => {
                HCI_LE_CS_READ_REMOTE_SUPPORTED_CAPABILITIES_COMMAND
            }
            Command::LeCsWriteCachedRemoteSupportedCapabilities { .. } => {
                HCI_LE_CS_WRITE_CACHED_REMOTE_SUPPORTED_CAPABILITIES_COMMAND
            }
            Command::LeCsSecurityEnable { .. } => HCI_LE_CS_SECURITY_ENABLE_COMMAND,
            Command::LeCsSetDefaultSettings { .. } => HCI_LE_CS_SET_DEFAULT_SETTINGS_COMMAND,
            Command::LeCsReadRemoteFaeTable { .. } => HCI_LE_CS_READ_REMOTE_FAE_TABLE_COMMAND,
            Command::LeCsWriteCachedRemoteFaeTable { .. } => {
                HCI_LE_CS_WRITE_CACHED_REMOTE_FAE_TABLE_COMMAND
            }
            Command::LeCsCreateConfig { .. } => HCI_LE_CS_CREATE_CONFIG_COMMAND,
            Command::LeCsRemoveConfig { .. } => HCI_LE_CS_REMOVE_CONFIG_COMMAND,
            Command::LeCsSetChannelClassification { .. } => {
                HCI_LE_CS_SET_CHANNEL_CLASSIFICATION_COMMAND
            }
            Command::LeCsSetProcedureParameters { .. } => {
                HCI_LE_CS_SET_PROCEDURE_PARAMETERS_COMMAND
            }
            Command::LeCsProcedureEnable { .. } => HCI_LE_CS_PROCEDURE_ENABLE_COMMAND,
            Command::LeCsTest { .. } => HCI_LE_CS_TEST_COMMAND,
            Command::LeCsTestEnd => HCI_LE_CS_TEST_END_COMMAND,
            Command::LeFrameSpaceUpdate { .. } => HCI_LE_FRAME_SPACE_UPDATE_COMMAND,
            Command::LeConnectionRateRequest { .. } => HCI_LE_CONNECTION_RATE_REQUEST_COMMAND,
            Command::LeSetDefaultRateParameters { .. } => {
                HCI_LE_SET_DEFAULT_RATE_PARAMETERS_COMMAND
            }
            Command::LeReadMinimumSupportedConnectionInterval => {
                HCI_LE_READ_MINIMUM_SUPPORTED_CONNECTION_INTERVAL_COMMAND
            }
            Command::LeSetExtendedScanParameters { .. } => {
                HCI_LE_SET_EXTENDED_SCAN_PARAMETERS_COMMAND
            }
            Command::LeExtendedCreateConnection { .. } => HCI_LE_EXTENDED_CREATE_CONNECTION_COMMAND,
            Command::Generic { op_code, .. } => *op_code,
        }
    }

    /// The serialized command parameters (without the packet/op-code header).
    #[allow(clippy::needless_range_loop)]
    pub fn parameters(&self) -> Vec<u8> {
        let mut p = Vec::new();
        match self {
            Command::InquiryCancel
            | Command::StartSynchronizationTrain
            | Command::Reset
            | Command::ReadLocalName
            | Command::ReadPageScanActivity
            | Command::ReadAuthenticationEnable
            | Command::ReadClassOfDevice
            | Command::ReadVoiceSetting
            | Command::ReadSynchronousFlowControlEnable
            | Command::ReadNumberOfSupportedIac
            | Command::ReadCurrentIacLap
            | Command::ReadPageScanType
            | Command::ReadLocalOobData
            | Command::ReadInquiryResponseTransmitPowerLevel
            | Command::ReadDefaultErroneousDataReporting
            | Command::ReadLeHostSupport
            | Command::ReadLocalOobExtendedData
            | Command::ReadLocalVersionInformation
            | Command::ReadLocalSupportedCommands
            | Command::ReadLocalSupportedFeatures
            | Command::ReadBufferSize
            | Command::ReadBdAddr
            | Command::ReadLocalSupportedCodecs
            | Command::ReadLocalSupportedCodecsV2
            | Command::ReadLoopbackMode
            | Command::LeReadBufferSize
            | Command::LeReadLocalSupportedFeatures
            | Command::LeReadAdvertisingPhysicalChannelTxPower
            | Command::LeCreateConnectionCancel
            | Command::LeReadFilterAcceptListSize
            | Command::LeClearFilterAcceptList
            | Command::LeRand
            | Command::LeReadSupportedStates
            | Command::LeReadSuggestedDefaultDataLength
            | Command::LeReadLocalP256PublicKey
            | Command::LeClearResolvingList
            | Command::LeReadResolvingListSize
            | Command::LeReadMaximumDataLength
            | Command::LeReadMaximumAdvertisingDataLength
            | Command::LeReadNumberOfSupportedAdvertisingSets
            | Command::LeClearAdvertisingSets
            | Command::LePeriodicAdvertisingCreateSyncCancel
            | Command::LeReadTransmitPower
            | Command::LeReadBufferSizeV2
            | Command::LeCsReadLocalSupportedCapabilities
            | Command::LeCsTestEnd
            | Command::LeReadMinimumSupportedConnectionInterval => {}
            Command::Inquiry {
                lap,
                inquiry_length,
                num_responses,
            } => {
                push_u24(&mut p, *lap);
                p.push(*inquiry_length);
                p.push(*num_responses);
            }
            Command::CreateConnection {
                bd_addr,
                packet_type,
                page_scan_repetition_mode,
                reserved,
                clock_offset,
                allow_role_switch,
            } => {
                p.extend_from_slice(bd_addr.address_bytes());
                push_u16(&mut p, *packet_type);
                p.push(*page_scan_repetition_mode);
                p.push(*reserved);
                push_u16(&mut p, *clock_offset);
                p.push(*allow_role_switch);
            }
            Command::Disconnect {
                connection_handle,
                reason,
            } => {
                push_u16(&mut p, *connection_handle);
                p.push(*reason);
            }
            Command::CreateConnectionCancel { bd_addr } => {
                p.extend_from_slice(bd_addr.address_bytes());
            }
            Command::AcceptConnectionRequest { bd_addr, role } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.push(*role);
            }
            Command::RejectConnectionRequest { bd_addr, reason } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.push(*reason);
            }
            Command::LinkKeyRequestReply { bd_addr, link_key } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.extend_from_slice(link_key);
            }
            Command::LinkKeyRequestNegativeReply { bd_addr } => {
                p.extend_from_slice(bd_addr.address_bytes());
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
            Command::PinCodeRequestNegativeReply { bd_addr } => {
                p.extend_from_slice(bd_addr.address_bytes());
            }
            Command::ChangeConnectionPacketType {
                connection_handle,
                packet_type,
            } => {
                push_u16(&mut p, *connection_handle);
                push_u16(&mut p, *packet_type);
            }
            Command::AuthenticationRequested { connection_handle } => {
                push_u16(&mut p, *connection_handle);
            }
            Command::SetConnectionEncryption {
                connection_handle,
                encryption_enable,
            } => {
                push_u16(&mut p, *connection_handle);
                p.push(*encryption_enable);
            }
            Command::RemoteNameRequest {
                bd_addr,
                page_scan_repetition_mode,
                reserved,
                clock_offset,
            } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.push(*page_scan_repetition_mode);
                p.push(*reserved);
                push_u16(&mut p, *clock_offset);
            }
            Command::ReadRemoteSupportedFeatures { connection_handle } => {
                push_u16(&mut p, *connection_handle);
            }
            Command::ReadRemoteExtendedFeatures {
                connection_handle,
                page_number,
            } => {
                push_u16(&mut p, *connection_handle);
                p.push(*page_number);
            }
            Command::ReadRemoteVersionInformation { connection_handle } => {
                push_u16(&mut p, *connection_handle);
            }
            Command::ReadClockOffset { connection_handle } => {
                push_u16(&mut p, *connection_handle);
            }
            Command::AcceptSynchronousConnectionRequest {
                bd_addr,
                transmit_bandwidth,
                receive_bandwidth,
                max_latency,
                voice_setting,
                retransmission_effort,
                packet_type,
            } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.extend_from_slice(&transmit_bandwidth.to_le_bytes());
                p.extend_from_slice(&receive_bandwidth.to_le_bytes());
                push_u16(&mut p, *max_latency);
                push_u16(&mut p, *voice_setting);
                p.push(*retransmission_effort);
                push_u16(&mut p, *packet_type);
            }
            Command::RejectSynchronousConnectionRequest { bd_addr, reason } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.push(*reason);
            }
            Command::IoCapabilityRequestReply {
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
            Command::UserConfirmationRequestReply { bd_addr } => {
                p.extend_from_slice(bd_addr.address_bytes());
            }
            Command::UserConfirmationRequestNegativeReply { bd_addr } => {
                p.extend_from_slice(bd_addr.address_bytes());
            }
            Command::UserPasskeyRequestReply {
                bd_addr,
                numeric_value,
            } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.extend_from_slice(&numeric_value.to_le_bytes());
            }
            Command::UserPasskeyRequestNegativeReply { bd_addr } => {
                p.extend_from_slice(bd_addr.address_bytes());
            }
            Command::RemoteOobDataRequestReply { bd_addr, c, r } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.extend_from_slice(c);
                p.extend_from_slice(r);
            }
            Command::RemoteOobDataRequestNegativeReply { bd_addr } => {
                p.extend_from_slice(bd_addr.address_bytes());
            }
            Command::IoCapabilityRequestNegativeReply { bd_addr, reason } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.push(*reason);
            }
            Command::EnhancedSetupSynchronousConnection {
                connection_handle,
                transmit_bandwidth,
                receive_bandwidth,
                transmit_coding_format,
                receive_coding_format,
                transmit_codec_frame_size,
                receive_codec_frame_size,
                input_bandwidth,
                output_bandwidth,
                input_coding_format,
                output_coding_format,
                input_coded_data_size,
                output_coded_data_size,
                input_pcm_data_format,
                output_pcm_data_format,
                input_pcm_sample_payload_msb_position,
                output_pcm_sample_payload_msb_position,
                input_data_path,
                output_data_path,
                input_transport_unit_size,
                output_transport_unit_size,
                max_latency,
                packet_type,
                retransmission_effort,
            } => {
                push_u16(&mut p, *connection_handle);
                p.extend_from_slice(&transmit_bandwidth.to_le_bytes());
                p.extend_from_slice(&receive_bandwidth.to_le_bytes());
                p.extend_from_slice(&transmit_coding_format.to_bytes());
                p.extend_from_slice(&receive_coding_format.to_bytes());
                push_u16(&mut p, *transmit_codec_frame_size);
                push_u16(&mut p, *receive_codec_frame_size);
                p.extend_from_slice(&input_bandwidth.to_le_bytes());
                p.extend_from_slice(&output_bandwidth.to_le_bytes());
                p.extend_from_slice(&input_coding_format.to_bytes());
                p.extend_from_slice(&output_coding_format.to_bytes());
                push_u16(&mut p, *input_coded_data_size);
                push_u16(&mut p, *output_coded_data_size);
                p.push(*input_pcm_data_format);
                p.push(*output_pcm_data_format);
                p.push(*input_pcm_sample_payload_msb_position);
                p.push(*output_pcm_sample_payload_msb_position);
                p.push(*input_data_path);
                p.push(*output_data_path);
                p.push(*input_transport_unit_size);
                p.push(*output_transport_unit_size);
                push_u16(&mut p, *max_latency);
                push_u16(&mut p, *packet_type);
                p.push(*retransmission_effort);
            }
            Command::EnhancedAcceptSynchronousConnectionRequest {
                bd_addr,
                transmit_bandwidth,
                receive_bandwidth,
                transmit_coding_format,
                receive_coding_format,
                transmit_codec_frame_size,
                receive_codec_frame_size,
                input_bandwidth,
                output_bandwidth,
                input_coding_format,
                output_coding_format,
                input_coded_data_size,
                output_coded_data_size,
                input_pcm_data_format,
                output_pcm_data_format,
                input_pcm_sample_payload_msb_position,
                output_pcm_sample_payload_msb_position,
                input_data_path,
                output_data_path,
                input_transport_unit_size,
                output_transport_unit_size,
                max_latency,
                packet_type,
                retransmission_effort,
            } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.extend_from_slice(&transmit_bandwidth.to_le_bytes());
                p.extend_from_slice(&receive_bandwidth.to_le_bytes());
                p.extend_from_slice(&transmit_coding_format.to_bytes());
                p.extend_from_slice(&receive_coding_format.to_bytes());
                push_u16(&mut p, *transmit_codec_frame_size);
                push_u16(&mut p, *receive_codec_frame_size);
                p.extend_from_slice(&input_bandwidth.to_le_bytes());
                p.extend_from_slice(&output_bandwidth.to_le_bytes());
                p.extend_from_slice(&input_coding_format.to_bytes());
                p.extend_from_slice(&output_coding_format.to_bytes());
                push_u16(&mut p, *input_coded_data_size);
                push_u16(&mut p, *output_coded_data_size);
                p.push(*input_pcm_data_format);
                p.push(*output_pcm_data_format);
                p.push(*input_pcm_sample_payload_msb_position);
                p.push(*output_pcm_sample_payload_msb_position);
                p.push(*input_data_path);
                p.push(*output_data_path);
                p.push(*input_transport_unit_size);
                p.push(*output_transport_unit_size);
                push_u16(&mut p, *max_latency);
                push_u16(&mut p, *packet_type);
                p.push(*retransmission_effort);
            }
            Command::TruncatedPage {
                bd_addr,
                page_scan_repetition_mode,
                clock_offset,
            } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.push(*page_scan_repetition_mode);
                push_u16(&mut p, *clock_offset);
            }
            Command::TruncatedPageCancel { bd_addr } => {
                p.extend_from_slice(bd_addr.address_bytes());
            }
            Command::SetConnectionlessPeripheralBroadcast {
                enable,
                lt_addr,
                lpo_allowed,
                packet_type,
                interval_min,
                interval_max,
                supervision_timeout,
            } => {
                p.push(*enable);
                p.push(*lt_addr);
                p.push(*lpo_allowed);
                push_u16(&mut p, *packet_type);
                push_u16(&mut p, *interval_min);
                push_u16(&mut p, *interval_max);
                push_u16(&mut p, *supervision_timeout);
            }
            Command::SetConnectionlessPeripheralBroadcastReceive {
                enable,
                bd_addr,
                lt_addr,
                interval,
                clock_offset,
                next_connectionless_peripheral_broadcast_clock,
                supervision_timeout,
                remote_timing_accuracy,
                skip,
                packet_type,
                afh_channel_map,
            } => {
                p.push(*enable);
                p.extend_from_slice(bd_addr.address_bytes());
                p.push(*lt_addr);
                push_u16(&mut p, *interval);
                p.extend_from_slice(&clock_offset.to_le_bytes());
                p.extend_from_slice(&next_connectionless_peripheral_broadcast_clock.to_le_bytes());
                push_u16(&mut p, *supervision_timeout);
                p.push(*remote_timing_accuracy);
                p.push(*skip);
                push_u16(&mut p, *packet_type);
                p.extend_from_slice(afh_channel_map);
            }
            Command::ReceiveSynchronizationTrain {
                bd_addr,
                sync_scan_timeout,
                sync_scan_window,
                sync_scan_interval,
            } => {
                p.extend_from_slice(bd_addr.address_bytes());
                push_u16(&mut p, *sync_scan_timeout);
                push_u16(&mut p, *sync_scan_window);
                push_u16(&mut p, *sync_scan_interval);
            }
            Command::RemoteOobExtendedDataRequestReply {
                bd_addr,
                c_192,
                r_192,
                c_256,
                r_256,
            } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.extend_from_slice(c_192);
                p.extend_from_slice(r_192);
                p.extend_from_slice(c_256);
                p.extend_from_slice(r_256);
            }
            Command::SniffMode {
                connection_handle,
                sniff_max_interval,
                sniff_min_interval,
                sniff_attempt,
                sniff_timeout,
            } => {
                push_u16(&mut p, *connection_handle);
                push_u16(&mut p, *sniff_max_interval);
                push_u16(&mut p, *sniff_min_interval);
                push_u16(&mut p, *sniff_attempt);
                push_u16(&mut p, *sniff_timeout);
            }
            Command::ExitSniffMode { connection_handle } => {
                push_u16(&mut p, *connection_handle);
            }
            Command::SwitchRole { bd_addr, role } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.push(*role);
            }
            Command::WriteLinkPolicySettings {
                connection_handle,
                link_policy_settings,
            } => {
                push_u16(&mut p, *connection_handle);
                push_u16(&mut p, *link_policy_settings);
            }
            Command::WriteDefaultLinkPolicySettings {
                default_link_policy_settings,
            } => {
                push_u16(&mut p, *default_link_policy_settings);
            }
            Command::SniffSubrating {
                connection_handle,
                maximum_latency,
                minimum_remote_timeout,
                minimum_local_timeout,
            } => {
                push_u16(&mut p, *connection_handle);
                push_u16(&mut p, *maximum_latency);
                push_u16(&mut p, *minimum_remote_timeout);
                push_u16(&mut p, *minimum_local_timeout);
            }
            Command::SetEventMask { event_mask } => {
                p.extend_from_slice(event_mask);
            }
            Command::SetEventFilter {
                filter_type,
                filter_condition,
            } => {
                p.push(*filter_type);
                p.extend_from_slice(filter_condition);
            }
            Command::ReadStoredLinkKey {
                bd_addr,
                read_all_flag,
            } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.push(*read_all_flag);
            }
            Command::DeleteStoredLinkKey {
                bd_addr,
                delete_all_flag,
            } => {
                p.extend_from_slice(bd_addr.address_bytes());
                p.push(*delete_all_flag);
            }
            Command::WriteLocalName { local_name } => {
                p.extend_from_slice(local_name);
            }
            Command::WriteConnectionAcceptTimeout {
                connection_accept_timeout,
            } => {
                push_u16(&mut p, *connection_accept_timeout);
            }
            Command::WritePageTimeout { page_timeout } => {
                push_u16(&mut p, *page_timeout);
            }
            Command::WriteScanEnable { scan_enable } => {
                p.push(*scan_enable);
            }
            Command::WritePageScanActivity {
                page_scan_interval,
                page_scan_window,
            } => {
                push_u16(&mut p, *page_scan_interval);
                push_u16(&mut p, *page_scan_window);
            }
            Command::WriteInquiryScanActivity {
                inquiry_scan_interval,
                inquiry_scan_window,
            } => {
                push_u16(&mut p, *inquiry_scan_interval);
                push_u16(&mut p, *inquiry_scan_window);
            }
            Command::WriteAuthenticationEnable {
                authentication_enable,
            } => {
                p.push(*authentication_enable);
            }
            Command::WriteClassOfDevice { class_of_device } => {
                push_u24(&mut p, *class_of_device);
            }
            Command::WriteVoiceSetting { voice_setting } => {
                push_u16(&mut p, *voice_setting);
            }
            Command::WriteSynchronousFlowControlEnable {
                synchronous_flow_control_enable,
            } => {
                p.push(*synchronous_flow_control_enable);
            }
            Command::SetControllerToHostFlowControl {
                flow_control_enable,
            } => {
                p.push(*flow_control_enable);
            }
            Command::HostBufferSize {
                host_acl_data_packet_length,
                host_synchronous_data_packet_length,
                host_total_num_acl_data_packets,
                host_total_num_synchronous_data_packets,
            } => {
                push_u16(&mut p, *host_acl_data_packet_length);
                p.push(*host_synchronous_data_packet_length);
                push_u16(&mut p, *host_total_num_acl_data_packets);
                push_u16(&mut p, *host_total_num_synchronous_data_packets);
            }
            Command::WriteLinkSupervisionTimeout {
                handle,
                link_supervision_timeout,
            } => {
                push_u16(&mut p, *handle);
                push_u16(&mut p, *link_supervision_timeout);
            }
            Command::WriteInquiryScanType { scan_type } => {
                p.push(*scan_type);
            }
            Command::WriteInquiryMode { inquiry_mode } => {
                p.push(*inquiry_mode);
            }
            Command::WritePageScanType { page_scan_type } => {
                p.push(*page_scan_type);
            }
            Command::WriteExtendedInquiryResponse {
                fec_required,
                extended_inquiry_response,
            } => {
                p.push(*fec_required);
                p.extend_from_slice(extended_inquiry_response);
            }
            Command::WriteSimplePairingMode {
                simple_pairing_mode,
            } => {
                p.push(*simple_pairing_mode);
            }
            Command::SetEventMaskPage2 { event_mask_page_2 } => {
                p.extend_from_slice(event_mask_page_2);
            }
            Command::WriteLeHostSupport {
                le_supported_host,
                simultaneous_le_host,
            } => {
                p.push(*le_supported_host);
                p.push(*simultaneous_le_host);
            }
            Command::WriteSecureConnectionsHostSupport {
                secure_connections_host_support,
            } => {
                p.push(*secure_connections_host_support);
            }
            Command::WriteAuthenticatedPayloadTimeout {
                connection_handle,
                authenticated_payload_timeout,
            } => {
                push_u16(&mut p, *connection_handle);
                push_u16(&mut p, *authenticated_payload_timeout);
            }
            Command::ConfigureDataPath {
                data_path_direction,
                data_path_id,
                vendor_specific_config,
            } => {
                p.push(*data_path_direction);
                p.push(*data_path_id);
                p.extend_from_slice(vendor_specific_config);
            }
            Command::ReadLocalExtendedFeatures { page_number } => {
                p.push(*page_number);
            }
            Command::ReadRssi { handle } => {
                push_u16(&mut p, *handle);
            }
            Command::ReadEncryptionKeySize { connection_handle } => {
                push_u16(&mut p, *connection_handle);
            }
            Command::WriteLoopbackMode { loopback_mode } => {
                p.push(*loopback_mode);
            }
            Command::LeSetEventMask { le_event_mask } => {
                p.extend_from_slice(le_event_mask);
            }
            Command::LeSetRandomAddress { random_address } => {
                p.extend_from_slice(random_address.address_bytes());
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
                p.push(advertising_data.len() as u8);
                p.extend_from_slice(advertising_data);
                p.resize(1 + 31, 0);
            }
            Command::LeSetScanResponseData { scan_response_data } => {
                p.push(scan_response_data.len() as u8);
                p.extend_from_slice(scan_response_data);
                p.resize(1 + 31, 0);
            }
            Command::LeSetAdvertisingEnable { advertising_enable } => {
                p.push(*advertising_enable);
            }
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
            } => {
                p.push(*address_type);
                p.extend_from_slice(address.address_bytes());
            }
            Command::LeRemoveDeviceFromFilterAcceptList {
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
                push_u16(&mut p, *connection_handle);
            }
            Command::LeEnableEncryption {
                connection_handle,
                random_number,
                encrypted_diversifier,
                long_term_key,
            } => {
                push_u16(&mut p, *connection_handle);
                p.extend_from_slice(random_number);
                push_u16(&mut p, *encrypted_diversifier);
                p.extend_from_slice(long_term_key);
            }
            Command::LeLongTermKeyRequestReply {
                connection_handle,
                long_term_key,
            } => {
                push_u16(&mut p, *connection_handle);
                p.extend_from_slice(long_term_key);
            }
            Command::LeLongTermKeyRequestNegativeReply { connection_handle } => {
                push_u16(&mut p, *connection_handle);
            }
            Command::LeRemoteConnectionParameterRequestReply {
                connection_handle,
                interval_min,
                interval_max,
                max_latency,
                timeout,
                min_ce_length,
                max_ce_length,
            } => {
                push_u16(&mut p, *connection_handle);
                push_u16(&mut p, *interval_min);
                push_u16(&mut p, *interval_max);
                push_u16(&mut p, *max_latency);
                push_u16(&mut p, *timeout);
                push_u16(&mut p, *min_ce_length);
                push_u16(&mut p, *max_ce_length);
            }
            Command::LeRemoteConnectionParameterRequestNegativeReply {
                connection_handle,
                reason,
            } => {
                push_u16(&mut p, *connection_handle);
                p.push(*reason);
            }
            Command::LeSetDataLength {
                connection_handle,
                tx_octets,
                tx_time,
            } => {
                push_u16(&mut p, *connection_handle);
                push_u16(&mut p, *tx_octets);
                push_u16(&mut p, *tx_time);
            }
            Command::LeWriteSuggestedDefaultDataLength {
                suggested_max_tx_octets,
                suggested_max_tx_time,
            } => {
                push_u16(&mut p, *suggested_max_tx_octets);
                push_u16(&mut p, *suggested_max_tx_time);
            }
            Command::LeAddDeviceToResolvingList {
                peer_identity_address_type,
                peer_identity_address,
                peer_irk,
                local_irk,
            } => {
                p.push(*peer_identity_address_type);
                p.extend_from_slice(peer_identity_address.address_bytes());
                p.extend_from_slice(peer_irk);
                p.extend_from_slice(local_irk);
            }
            Command::LeSetAddressResolutionEnable {
                address_resolution_enable,
            } => {
                p.push(*address_resolution_enable);
            }
            Command::LeSetResolvablePrivateAddressTimeout { rpa_timeout } => {
                push_u16(&mut p, *rpa_timeout);
            }
            Command::LeReadPhy { connection_handle } => {
                push_u16(&mut p, *connection_handle);
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
            Command::LeSetPhy {
                connection_handle,
                all_phys,
                tx_phys,
                rx_phys,
                phy_options,
            } => {
                push_u16(&mut p, *connection_handle);
                p.push(*all_phys);
                p.push(*tx_phys);
                p.push(*rx_phys);
                push_u16(&mut p, *phy_options);
            }
            Command::LeSetAdvertisingSetRandomAddress {
                advertising_handle,
                random_address,
            } => {
                p.push(*advertising_handle);
                p.extend_from_slice(random_address.address_bytes());
            }
            Command::LeSetExtendedAdvertisingParameters {
                advertising_handle,
                advertising_event_properties,
                primary_advertising_interval_min,
                primary_advertising_interval_max,
                primary_advertising_channel_map,
                own_address_type,
                peer_address_type,
                peer_address,
                advertising_filter_policy,
                advertising_tx_power,
                primary_advertising_phy,
                secondary_advertising_max_skip,
                secondary_advertising_phy,
                advertising_sid,
                scan_request_notification_enable,
            } => {
                p.push(*advertising_handle);
                push_u16(&mut p, *advertising_event_properties);
                push_u24(&mut p, *primary_advertising_interval_min);
                push_u24(&mut p, *primary_advertising_interval_max);
                p.push(*primary_advertising_channel_map);
                p.push(*own_address_type);
                p.push(*peer_address_type);
                p.extend_from_slice(peer_address.address_bytes());
                p.push(*advertising_filter_policy);
                p.push(*advertising_tx_power);
                p.push(*primary_advertising_phy);
                p.push(*secondary_advertising_max_skip);
                p.push(*secondary_advertising_phy);
                p.push(*advertising_sid);
                p.push(*scan_request_notification_enable);
            }
            Command::LeSetExtendedAdvertisingData {
                advertising_handle,
                operation,
                fragment_preference,
                advertising_data,
            } => {
                p.push(*advertising_handle);
                p.push(*operation);
                p.push(*fragment_preference);
                p.push(advertising_data.len() as u8);
                p.extend_from_slice(advertising_data);
            }
            Command::LeSetExtendedScanResponseData {
                advertising_handle,
                operation,
                fragment_preference,
                scan_response_data,
            } => {
                p.push(*advertising_handle);
                p.push(*operation);
                p.push(*fragment_preference);
                p.push(scan_response_data.len() as u8);
                p.extend_from_slice(scan_response_data);
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
            Command::LeRemoveAdvertisingSet { advertising_handle } => {
                p.push(*advertising_handle);
            }
            Command::LeSetPeriodicAdvertisingParameters {
                advertising_handle,
                periodic_advertising_interval_min,
                periodic_advertising_interval_max,
                periodic_advertising_properties,
            } => {
                p.push(*advertising_handle);
                push_u16(&mut p, *periodic_advertising_interval_min);
                push_u16(&mut p, *periodic_advertising_interval_max);
                push_u16(&mut p, *periodic_advertising_properties);
            }
            Command::LeSetPeriodicAdvertisingData {
                advertising_handle,
                operation,
                advertising_data,
            } => {
                p.push(*advertising_handle);
                p.push(*operation);
                p.push(advertising_data.len() as u8);
                p.extend_from_slice(advertising_data);
            }
            Command::LeSetPeriodicAdvertisingEnable {
                enable,
                advertising_handle,
            } => {
                p.push(*enable);
                p.push(*advertising_handle);
            }
            Command::LeSetExtendedScanEnable {
                enable,
                filter_duplicates,
                duration,
                period,
            } => {
                p.push(*enable);
                p.push(*filter_duplicates);
                push_u16(&mut p, *duration);
                push_u16(&mut p, *period);
            }
            Command::LePeriodicAdvertisingCreateSync {
                options,
                advertising_sid,
                advertiser_address_type,
                advertiser_address,
                skip,
                sync_timeout,
                sync_cte_type,
            } => {
                p.push(*options);
                p.push(*advertising_sid);
                p.push(*advertiser_address_type);
                p.extend_from_slice(advertiser_address.address_bytes());
                push_u16(&mut p, *skip);
                push_u16(&mut p, *sync_timeout);
                p.push(*sync_cte_type);
            }
            Command::LePeriodicAdvertisingTerminateSync { sync_handle } => {
                push_u16(&mut p, *sync_handle);
            }
            Command::LeSetPrivacyMode {
                peer_identity_address_type,
                peer_identity_address,
                privacy_mode,
            } => {
                p.push(*peer_identity_address_type);
                p.extend_from_slice(peer_identity_address.address_bytes());
                p.push(*privacy_mode);
            }
            Command::LeSetPeriodicAdvertisingReceiveEnable {
                sync_handle,
                enable,
            } => {
                push_u16(&mut p, *sync_handle);
                p.push(*enable);
            }
            Command::LePeriodicAdvertisingSyncTransfer {
                connection_handle,
                service_data,
                sync_handle,
            } => {
                push_u16(&mut p, *connection_handle);
                push_u16(&mut p, *service_data);
                push_u16(&mut p, *sync_handle);
            }
            Command::LePeriodicAdvertisingSetInfoTransfer {
                connection_handle,
                service_data,
                advertising_handle,
            } => {
                push_u16(&mut p, *connection_handle);
                push_u16(&mut p, *service_data);
                p.push(*advertising_handle);
            }
            Command::LeSetPeriodicAdvertisingSyncTransferParameters {
                connection_handle,
                mode,
                skip,
                sync_timeout,
                cte_type,
            } => {
                push_u16(&mut p, *connection_handle);
                p.push(*mode);
                push_u16(&mut p, *skip);
                push_u16(&mut p, *sync_timeout);
                p.push(*cte_type);
            }
            Command::LeSetDefaultPeriodicAdvertisingSyncTransferParameters {
                mode,
                skip,
                sync_timeout,
                cte_type,
            } => {
                p.push(*mode);
                push_u16(&mut p, *skip);
                push_u16(&mut p, *sync_timeout);
                p.push(*cte_type);
            }
            Command::LeReadIsoTxSync { connection_handle } => {
                push_u16(&mut p, *connection_handle);
            }
            Command::LeSetCigParameters {
                cig_id,
                sdu_interval_c_to_p,
                sdu_interval_p_to_c,
                worst_case_sca,
                packing,
                framing,
                max_transport_latency_c_to_p,
                max_transport_latency_p_to_c,
                cis_id,
                max_sdu_c_to_p,
                max_sdu_p_to_c,
                phy_c_to_p,
                phy_p_to_c,
                rtn_c_to_p,
                rtn_p_to_c,
            } => {
                p.push(*cig_id);
                push_u24(&mut p, *sdu_interval_c_to_p);
                push_u24(&mut p, *sdu_interval_p_to_c);
                p.push(*worst_case_sca);
                p.push(*packing);
                p.push(*framing);
                push_u16(&mut p, *max_transport_latency_c_to_p);
                push_u16(&mut p, *max_transport_latency_p_to_c);
                p.push(cis_id.len() as u8);
                for i in 0..cis_id.len() {
                    p.push(cis_id[i]);
                    push_u16(&mut p, max_sdu_c_to_p[i]);
                    push_u16(&mut p, max_sdu_p_to_c[i]);
                    p.push(phy_c_to_p[i]);
                    p.push(phy_p_to_c[i]);
                    p.push(rtn_c_to_p[i]);
                    p.push(rtn_p_to_c[i]);
                }
            }
            Command::LeCreateCis {
                cis_connection_handle,
                acl_connection_handle,
            } => {
                p.push(cis_connection_handle.len() as u8);
                for i in 0..cis_connection_handle.len() {
                    push_u16(&mut p, cis_connection_handle[i]);
                    push_u16(&mut p, acl_connection_handle[i]);
                }
            }
            Command::LeRemoveCig { cig_id } => {
                p.push(*cig_id);
            }
            Command::LeAcceptCisRequest { connection_handle } => {
                push_u16(&mut p, *connection_handle);
            }
            Command::LeRejectCisRequest {
                connection_handle,
                reason,
            } => {
                push_u16(&mut p, *connection_handle);
                p.push(*reason);
            }
            Command::LeCreateBig {
                big_handle,
                advertising_handle,
                num_bis,
                sdu_interval,
                max_sdu,
                max_transport_latency,
                rtn,
                phy,
                packing,
                framing,
                encryption,
                broadcast_code,
            } => {
                p.push(*big_handle);
                p.push(*advertising_handle);
                p.push(*num_bis);
                push_u24(&mut p, *sdu_interval);
                push_u16(&mut p, *max_sdu);
                push_u16(&mut p, *max_transport_latency);
                p.push(*rtn);
                p.push(*phy);
                p.push(*packing);
                p.push(*framing);
                p.push(*encryption);
                p.extend_from_slice(broadcast_code);
            }
            Command::LeTerminateBig { big_handle, reason } => {
                p.push(*big_handle);
                p.push(*reason);
            }
            Command::LeBigCreateSync {
                big_handle,
                sync_handle,
                encryption,
                broadcast_code,
                mse,
                big_sync_timeout,
                bis,
            } => {
                p.push(*big_handle);
                push_u16(&mut p, *sync_handle);
                p.push(*encryption);
                p.extend_from_slice(broadcast_code);
                p.push(*mse);
                push_u16(&mut p, *big_sync_timeout);
                p.push(bis.len() as u8);
                for i in 0..bis.len() {
                    p.push(bis[i]);
                }
            }
            Command::LeBigTerminateSync { big_handle } => {
                p.push(*big_handle);
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
            Command::LeRemoveIsoDataPath {
                connection_handle,
                data_path_direction,
            } => {
                push_u16(&mut p, *connection_handle);
                p.push(*data_path_direction);
            }
            Command::LeSetHostFeature {
                bit_number,
                bit_value,
            } => {
                p.push(*bit_number);
                p.push(*bit_value);
            }
            Command::LeSetDefaultSubrate {
                subrate_min,
                subrate_max,
                max_latency,
                continuation_number,
                supervision_timeout,
            } => {
                push_u16(&mut p, *subrate_min);
                push_u16(&mut p, *subrate_max);
                push_u16(&mut p, *max_latency);
                push_u16(&mut p, *continuation_number);
                push_u16(&mut p, *supervision_timeout);
            }
            Command::LeSubrateRequest {
                connection_handle,
                subrate_min,
                subrate_max,
                max_latency,
                continuation_number,
                supervision_timeout,
            } => {
                push_u16(&mut p, *connection_handle);
                push_u16(&mut p, *subrate_min);
                push_u16(&mut p, *subrate_max);
                push_u16(&mut p, *max_latency);
                push_u16(&mut p, *continuation_number);
                push_u16(&mut p, *supervision_timeout);
            }
            Command::LeCsReadRemoteSupportedCapabilities { connection_handle } => {
                push_u16(&mut p, *connection_handle);
            }
            Command::LeCsWriteCachedRemoteSupportedCapabilities {
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
                push_u16(&mut p, *connection_handle);
                p.push(*num_config_supported);
                push_u16(&mut p, *max_consecutive_procedures_supported);
                p.push(*num_antennas_supported);
                p.push(*max_antenna_paths_supported);
                p.push(*roles_supported);
                p.push(*modes_supported);
                p.push(*rtt_capability);
                p.push(*rtt_aa_only_n);
                p.push(*rtt_sounding_n);
                p.push(*rtt_random_sequence_n);
                push_u16(&mut p, *nadm_sounding_capability);
                push_u16(&mut p, *nadm_random_capability);
                p.push(*cs_sync_phys_supported);
                push_u16(&mut p, *subfeatures_supported);
                push_u16(&mut p, *t_ip1_times_supported);
                push_u16(&mut p, *t_ip2_times_supported);
                push_u16(&mut p, *t_fcs_times_supported);
                push_u16(&mut p, *t_pm_times_supported);
                p.push(*t_sw_time_supported);
                p.push(*tx_snr_capability);
            }
            Command::LeCsSecurityEnable { connection_handle } => {
                push_u16(&mut p, *connection_handle);
            }
            Command::LeCsSetDefaultSettings {
                connection_handle,
                role_enable,
                cs_sync_antenna_selection,
                max_tx_power,
            } => {
                push_u16(&mut p, *connection_handle);
                p.push(*role_enable);
                p.push(*cs_sync_antenna_selection);
                p.push(*max_tx_power);
            }
            Command::LeCsReadRemoteFaeTable { connection_handle } => {
                push_u16(&mut p, *connection_handle);
            }
            Command::LeCsWriteCachedRemoteFaeTable {
                connection_handle,
                remote_fae_table,
            } => {
                push_u16(&mut p, *connection_handle);
                p.extend_from_slice(remote_fae_table);
            }
            Command::LeCsCreateConfig {
                connection_handle,
                config_id,
                create_context,
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
            } => {
                push_u16(&mut p, *connection_handle);
                p.push(*config_id);
                p.push(*create_context);
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
            }
            Command::LeCsRemoveConfig {
                connection_handle,
                config_id,
            } => {
                push_u16(&mut p, *connection_handle);
                p.push(*config_id);
            }
            Command::LeCsSetChannelClassification {
                channel_classification,
            } => {
                p.extend_from_slice(channel_classification);
            }
            Command::LeCsSetProcedureParameters {
                connection_handle,
                config_id,
                max_procedure_len,
                min_procedure_interval,
                max_procedure_interval,
                max_procedure_count,
                min_subevent_len,
                max_subevent_len,
                tone_antenna_config_selection,
                phy,
                tx_power_delta,
                preferred_peer_antenna,
                snr_control_initiator,
                snr_control_reflector,
            } => {
                push_u16(&mut p, *connection_handle);
                p.push(*config_id);
                push_u16(&mut p, *max_procedure_len);
                push_u16(&mut p, *min_procedure_interval);
                push_u16(&mut p, *max_procedure_interval);
                push_u16(&mut p, *max_procedure_count);
                push_u24(&mut p, *min_subevent_len);
                push_u24(&mut p, *max_subevent_len);
                p.push(*tone_antenna_config_selection);
                p.push(*phy);
                p.push(*tx_power_delta);
                p.push(*preferred_peer_antenna);
                p.push(*snr_control_initiator);
                p.push(*snr_control_reflector);
            }
            Command::LeCsProcedureEnable {
                connection_handle,
                config_id,
                enable,
            } => {
                push_u16(&mut p, *connection_handle);
                p.push(*config_id);
                p.push(*enable);
            }
            Command::LeCsTest {
                main_mode_type,
                sub_mode_type,
                main_mode_repetition,
                mode_0_steps,
                role,
                rtt_type,
                cs_sync_phy,
                cs_sync_antenna_selection,
                subevent_len,
                subevent_interval,
                max_num_subevents,
                transmit_power_level,
                t_ip1_time,
                t_ip2_time,
                t_fcs_time,
                t_pm_time,
                t_sw_time,
                tone_antenna_config_selection,
                reserved,
                snr_control_initiator,
                snr_control_reflector,
                drbg_nonce,
                channel_map_repetition,
                override_config,
                override_parameters_data,
            } => {
                p.push(*main_mode_type);
                p.push(*sub_mode_type);
                p.push(*main_mode_repetition);
                p.push(*mode_0_steps);
                p.push(*role);
                p.push(*rtt_type);
                p.push(*cs_sync_phy);
                p.push(*cs_sync_antenna_selection);
                push_u24(&mut p, *subevent_len);
                push_u16(&mut p, *subevent_interval);
                p.push(*max_num_subevents);
                p.push(*transmit_power_level);
                p.push(*t_ip1_time);
                p.push(*t_ip2_time);
                p.push(*t_fcs_time);
                p.push(*t_pm_time);
                p.push(*t_sw_time);
                p.push(*tone_antenna_config_selection);
                p.push(*reserved);
                p.push(*snr_control_initiator);
                p.push(*snr_control_reflector);
                push_u16(&mut p, *drbg_nonce);
                p.push(*channel_map_repetition);
                push_u16(&mut p, *override_config);
                p.push(override_parameters_data.len() as u8);
                p.extend_from_slice(override_parameters_data);
            }
            Command::LeFrameSpaceUpdate {
                connection_handle,
                frame_space_min,
                frame_space_max,
                phys,
                spacing_types,
            } => {
                push_u16(&mut p, *connection_handle);
                push_u16(&mut p, *frame_space_min);
                push_u16(&mut p, *frame_space_max);
                p.push(*phys);
                push_u16(&mut p, *spacing_types);
            }
            Command::LeConnectionRateRequest {
                connection_handle,
                connection_interval_min,
                connection_interval_max,
                subrate_min,
                subrate_max,
                max_latency,
                continuation_number,
                supervision_timeout,
                min_ce_length,
                max_ce_length,
            } => {
                push_u16(&mut p, *connection_handle);
                push_u16(&mut p, *connection_interval_min);
                push_u16(&mut p, *connection_interval_max);
                push_u16(&mut p, *subrate_min);
                push_u16(&mut p, *subrate_max);
                push_u16(&mut p, *max_latency);
                push_u16(&mut p, *continuation_number);
                push_u16(&mut p, *supervision_timeout);
                push_u16(&mut p, *min_ce_length);
                push_u16(&mut p, *max_ce_length);
            }
            Command::LeSetDefaultRateParameters {
                connection_interval_min,
                connection_interval_max,
                subrate_min,
                subrate_max,
                max_latency,
                continuation_number,
                supervision_timeout,
                min_ce_length,
                max_ce_length,
            } => {
                push_u16(&mut p, *connection_interval_min);
                push_u16(&mut p, *connection_interval_max);
                push_u16(&mut p, *subrate_min);
                push_u16(&mut p, *subrate_max);
                push_u16(&mut p, *max_latency);
                push_u16(&mut p, *continuation_number);
                push_u16(&mut p, *supervision_timeout);
                push_u16(&mut p, *min_ce_length);
                push_u16(&mut p, *max_ce_length);
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
    #[allow(clippy::redundant_closure_call)]
    pub fn from_parameters(op_code: u16, parameters: &[u8]) -> Result<Command> {
        // HCI address fields do not carry the address type on the wire; the type
        // is reconstructed as a random device address (does not affect bytes).
        let addr = |r: &mut Reader| -> Result<Address> {
            Ok(Address::from_bytes(
                r.array::<6>()?,
                AddressType::RANDOM_DEVICE,
            ))
        };

        let mut r = Reader::new(parameters, 0);
        let _ = &mut r;
        Ok(match op_code {
            HCI_INQUIRY_COMMAND => {
                let lap = r.u24_le()?;
                let inquiry_length = r.u8()?;
                let num_responses = r.u8()?;
                Command::Inquiry {
                    lap,
                    inquiry_length,
                    num_responses,
                }
            }
            HCI_INQUIRY_CANCEL_COMMAND => Command::InquiryCancel,
            HCI_CREATE_CONNECTION_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let packet_type = r.u16_le()?;
                let page_scan_repetition_mode = r.u8()?;
                let reserved = r.u8()?;
                let clock_offset = r.u16_le()?;
                let allow_role_switch = r.u8()?;
                Command::CreateConnection {
                    bd_addr,
                    packet_type,
                    page_scan_repetition_mode,
                    reserved,
                    clock_offset,
                    allow_role_switch,
                }
            }
            HCI_DISCONNECT_COMMAND => {
                let connection_handle = r.u16_le()?;
                let reason = r.u8()?;
                Command::Disconnect {
                    connection_handle,
                    reason,
                }
            }
            HCI_CREATE_CONNECTION_CANCEL_COMMAND => {
                let bd_addr = addr(&mut r)?;
                Command::CreateConnectionCancel { bd_addr }
            }
            HCI_ACCEPT_CONNECTION_REQUEST_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let role = r.u8()?;
                Command::AcceptConnectionRequest { bd_addr, role }
            }
            HCI_REJECT_CONNECTION_REQUEST_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let reason = r.u8()?;
                Command::RejectConnectionRequest { bd_addr, reason }
            }
            HCI_LINK_KEY_REQUEST_REPLY_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let link_key = r.array::<16>()?;
                Command::LinkKeyRequestReply { bd_addr, link_key }
            }
            HCI_LINK_KEY_REQUEST_NEGATIVE_REPLY_COMMAND => {
                let bd_addr = addr(&mut r)?;
                Command::LinkKeyRequestNegativeReply { bd_addr }
            }
            HCI_PIN_CODE_REQUEST_REPLY_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let pin_code_length = r.u8()?;
                let pin_code = r.array::<16>()?;
                Command::PinCodeRequestReply {
                    bd_addr,
                    pin_code_length,
                    pin_code,
                }
            }
            HCI_PIN_CODE_REQUEST_NEGATIVE_REPLY_COMMAND => {
                let bd_addr = addr(&mut r)?;
                Command::PinCodeRequestNegativeReply { bd_addr }
            }
            HCI_CHANGE_CONNECTION_PACKET_TYPE_COMMAND => {
                let connection_handle = r.u16_le()?;
                let packet_type = r.u16_le()?;
                Command::ChangeConnectionPacketType {
                    connection_handle,
                    packet_type,
                }
            }
            HCI_AUTHENTICATION_REQUESTED_COMMAND => {
                let connection_handle = r.u16_le()?;
                Command::AuthenticationRequested { connection_handle }
            }
            HCI_SET_CONNECTION_ENCRYPTION_COMMAND => {
                let connection_handle = r.u16_le()?;
                let encryption_enable = r.u8()?;
                Command::SetConnectionEncryption {
                    connection_handle,
                    encryption_enable,
                }
            }
            HCI_REMOTE_NAME_REQUEST_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let page_scan_repetition_mode = r.u8()?;
                let reserved = r.u8()?;
                let clock_offset = r.u16_le()?;
                Command::RemoteNameRequest {
                    bd_addr,
                    page_scan_repetition_mode,
                    reserved,
                    clock_offset,
                }
            }
            HCI_READ_REMOTE_SUPPORTED_FEATURES_COMMAND => {
                let connection_handle = r.u16_le()?;
                Command::ReadRemoteSupportedFeatures { connection_handle }
            }
            HCI_READ_REMOTE_EXTENDED_FEATURES_COMMAND => {
                let connection_handle = r.u16_le()?;
                let page_number = r.u8()?;
                Command::ReadRemoteExtendedFeatures {
                    connection_handle,
                    page_number,
                }
            }
            HCI_READ_REMOTE_VERSION_INFORMATION_COMMAND => {
                let connection_handle = r.u16_le()?;
                Command::ReadRemoteVersionInformation { connection_handle }
            }
            HCI_READ_CLOCK_OFFSET_COMMAND => {
                let connection_handle = r.u16_le()?;
                Command::ReadClockOffset { connection_handle }
            }
            HCI_ACCEPT_SYNCHRONOUS_CONNECTION_REQUEST_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let transmit_bandwidth = r.u32_le()?;
                let receive_bandwidth = r.u32_le()?;
                let max_latency = r.u16_le()?;
                let voice_setting = r.u16_le()?;
                let retransmission_effort = r.u8()?;
                let packet_type = r.u16_le()?;
                Command::AcceptSynchronousConnectionRequest {
                    bd_addr,
                    transmit_bandwidth,
                    receive_bandwidth,
                    max_latency,
                    voice_setting,
                    retransmission_effort,
                    packet_type,
                }
            }
            HCI_REJECT_SYNCHRONOUS_CONNECTION_REQUEST_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let reason = r.u8()?;
                Command::RejectSynchronousConnectionRequest { bd_addr, reason }
            }
            HCI_IO_CAPABILITY_REQUEST_REPLY_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let io_capability = r.u8()?;
                let oob_data_present = r.u8()?;
                let authentication_requirements = r.u8()?;
                Command::IoCapabilityRequestReply {
                    bd_addr,
                    io_capability,
                    oob_data_present,
                    authentication_requirements,
                }
            }
            HCI_USER_CONFIRMATION_REQUEST_REPLY_COMMAND => {
                let bd_addr = addr(&mut r)?;
                Command::UserConfirmationRequestReply { bd_addr }
            }
            HCI_USER_CONFIRMATION_REQUEST_NEGATIVE_REPLY_COMMAND => {
                let bd_addr = addr(&mut r)?;
                Command::UserConfirmationRequestNegativeReply { bd_addr }
            }
            HCI_USER_PASSKEY_REQUEST_REPLY_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let numeric_value = r.u32_le()?;
                Command::UserPasskeyRequestReply {
                    bd_addr,
                    numeric_value,
                }
            }
            HCI_USER_PASSKEY_REQUEST_NEGATIVE_REPLY_COMMAND => {
                let bd_addr = addr(&mut r)?;
                Command::UserPasskeyRequestNegativeReply { bd_addr }
            }
            HCI_REMOTE_OOB_DATA_REQUEST_REPLY_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let c = r.array::<16>()?;
                let r = r.array::<16>()?;
                Command::RemoteOobDataRequestReply { bd_addr, c, r }
            }
            HCI_REMOTE_OOB_DATA_REQUEST_NEGATIVE_REPLY_COMMAND => {
                let bd_addr = addr(&mut r)?;
                Command::RemoteOobDataRequestNegativeReply { bd_addr }
            }
            HCI_IO_CAPABILITY_REQUEST_NEGATIVE_REPLY_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let reason = r.u8()?;
                Command::IoCapabilityRequestNegativeReply { bd_addr, reason }
            }
            HCI_ENHANCED_SETUP_SYNCHRONOUS_CONNECTION_COMMAND => {
                let connection_handle = r.u16_le()?;
                let transmit_bandwidth = r.u32_le()?;
                let receive_bandwidth = r.u32_le()?;
                let transmit_coding_format = CodingFormat::read(&mut r)?;
                let receive_coding_format = CodingFormat::read(&mut r)?;
                let transmit_codec_frame_size = r.u16_le()?;
                let receive_codec_frame_size = r.u16_le()?;
                let input_bandwidth = r.u32_le()?;
                let output_bandwidth = r.u32_le()?;
                let input_coding_format = CodingFormat::read(&mut r)?;
                let output_coding_format = CodingFormat::read(&mut r)?;
                let input_coded_data_size = r.u16_le()?;
                let output_coded_data_size = r.u16_le()?;
                let input_pcm_data_format = r.u8()?;
                let output_pcm_data_format = r.u8()?;
                let input_pcm_sample_payload_msb_position = r.u8()?;
                let output_pcm_sample_payload_msb_position = r.u8()?;
                let input_data_path = r.u8()?;
                let output_data_path = r.u8()?;
                let input_transport_unit_size = r.u8()?;
                let output_transport_unit_size = r.u8()?;
                let max_latency = r.u16_le()?;
                let packet_type = r.u16_le()?;
                let retransmission_effort = r.u8()?;
                Command::EnhancedSetupSynchronousConnection {
                    connection_handle,
                    transmit_bandwidth,
                    receive_bandwidth,
                    transmit_coding_format,
                    receive_coding_format,
                    transmit_codec_frame_size,
                    receive_codec_frame_size,
                    input_bandwidth,
                    output_bandwidth,
                    input_coding_format,
                    output_coding_format,
                    input_coded_data_size,
                    output_coded_data_size,
                    input_pcm_data_format,
                    output_pcm_data_format,
                    input_pcm_sample_payload_msb_position,
                    output_pcm_sample_payload_msb_position,
                    input_data_path,
                    output_data_path,
                    input_transport_unit_size,
                    output_transport_unit_size,
                    max_latency,
                    packet_type,
                    retransmission_effort,
                }
            }
            HCI_ENHANCED_ACCEPT_SYNCHRONOUS_CONNECTION_REQUEST_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let transmit_bandwidth = r.u32_le()?;
                let receive_bandwidth = r.u32_le()?;
                let transmit_coding_format = CodingFormat::read(&mut r)?;
                let receive_coding_format = CodingFormat::read(&mut r)?;
                let transmit_codec_frame_size = r.u16_le()?;
                let receive_codec_frame_size = r.u16_le()?;
                let input_bandwidth = r.u32_le()?;
                let output_bandwidth = r.u32_le()?;
                let input_coding_format = CodingFormat::read(&mut r)?;
                let output_coding_format = CodingFormat::read(&mut r)?;
                let input_coded_data_size = r.u16_le()?;
                let output_coded_data_size = r.u16_le()?;
                let input_pcm_data_format = r.u8()?;
                let output_pcm_data_format = r.u8()?;
                let input_pcm_sample_payload_msb_position = r.u8()?;
                let output_pcm_sample_payload_msb_position = r.u8()?;
                let input_data_path = r.u8()?;
                let output_data_path = r.u8()?;
                let input_transport_unit_size = r.u8()?;
                let output_transport_unit_size = r.u8()?;
                let max_latency = r.u16_le()?;
                let packet_type = r.u16_le()?;
                let retransmission_effort = r.u8()?;
                Command::EnhancedAcceptSynchronousConnectionRequest {
                    bd_addr,
                    transmit_bandwidth,
                    receive_bandwidth,
                    transmit_coding_format,
                    receive_coding_format,
                    transmit_codec_frame_size,
                    receive_codec_frame_size,
                    input_bandwidth,
                    output_bandwidth,
                    input_coding_format,
                    output_coding_format,
                    input_coded_data_size,
                    output_coded_data_size,
                    input_pcm_data_format,
                    output_pcm_data_format,
                    input_pcm_sample_payload_msb_position,
                    output_pcm_sample_payload_msb_position,
                    input_data_path,
                    output_data_path,
                    input_transport_unit_size,
                    output_transport_unit_size,
                    max_latency,
                    packet_type,
                    retransmission_effort,
                }
            }
            HCI_TRUNCATED_PAGE_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let page_scan_repetition_mode = r.u8()?;
                let clock_offset = r.u16_le()?;
                Command::TruncatedPage {
                    bd_addr,
                    page_scan_repetition_mode,
                    clock_offset,
                }
            }
            HCI_TRUNCATED_PAGE_CANCEL_COMMAND => {
                let bd_addr = addr(&mut r)?;
                Command::TruncatedPageCancel { bd_addr }
            }
            HCI_SET_CONNECTIONLESS_PERIPHERAL_BROADCAST_COMMAND => {
                let enable = r.u8()?;
                let lt_addr = r.u8()?;
                let lpo_allowed = r.u8()?;
                let packet_type = r.u16_le()?;
                let interval_min = r.u16_le()?;
                let interval_max = r.u16_le()?;
                let supervision_timeout = r.u16_le()?;
                Command::SetConnectionlessPeripheralBroadcast {
                    enable,
                    lt_addr,
                    lpo_allowed,
                    packet_type,
                    interval_min,
                    interval_max,
                    supervision_timeout,
                }
            }
            HCI_SET_CONNECTIONLESS_PERIPHERAL_BROADCAST_RECEIVE_COMMAND => {
                let enable = r.u8()?;
                let bd_addr = addr(&mut r)?;
                let lt_addr = r.u8()?;
                let interval = r.u16_le()?;
                let clock_offset = r.u32_le()?;
                let next_connectionless_peripheral_broadcast_clock = r.u32_le()?;
                let supervision_timeout = r.u16_le()?;
                let remote_timing_accuracy = r.u8()?;
                let skip = r.u8()?;
                let packet_type = r.u16_le()?;
                let afh_channel_map = r.array::<10>()?;
                Command::SetConnectionlessPeripheralBroadcastReceive {
                    enable,
                    bd_addr,
                    lt_addr,
                    interval,
                    clock_offset,
                    next_connectionless_peripheral_broadcast_clock,
                    supervision_timeout,
                    remote_timing_accuracy,
                    skip,
                    packet_type,
                    afh_channel_map,
                }
            }
            HCI_START_SYNCHRONIZATION_TRAIN_COMMAND => Command::StartSynchronizationTrain,
            HCI_RECEIVE_SYNCHRONIZATION_TRAIN_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let sync_scan_timeout = r.u16_le()?;
                let sync_scan_window = r.u16_le()?;
                let sync_scan_interval = r.u16_le()?;
                Command::ReceiveSynchronizationTrain {
                    bd_addr,
                    sync_scan_timeout,
                    sync_scan_window,
                    sync_scan_interval,
                }
            }
            HCI_REMOTE_OOB_EXTENDED_DATA_REQUEST_REPLY_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let c_192 = r.array::<16>()?;
                let r_192 = r.array::<16>()?;
                let c_256 = r.array::<16>()?;
                let r_256 = r.array::<16>()?;
                Command::RemoteOobExtendedDataRequestReply {
                    bd_addr,
                    c_192,
                    r_192,
                    c_256,
                    r_256,
                }
            }
            HCI_SNIFF_MODE_COMMAND => {
                let connection_handle = r.u16_le()?;
                let sniff_max_interval = r.u16_le()?;
                let sniff_min_interval = r.u16_le()?;
                let sniff_attempt = r.u16_le()?;
                let sniff_timeout = r.u16_le()?;
                Command::SniffMode {
                    connection_handle,
                    sniff_max_interval,
                    sniff_min_interval,
                    sniff_attempt,
                    sniff_timeout,
                }
            }
            HCI_EXIT_SNIFF_MODE_COMMAND => {
                let connection_handle = r.u16_le()?;
                Command::ExitSniffMode { connection_handle }
            }
            HCI_SWITCH_ROLE_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let role = r.u8()?;
                Command::SwitchRole { bd_addr, role }
            }
            HCI_WRITE_LINK_POLICY_SETTINGS_COMMAND => {
                let connection_handle = r.u16_le()?;
                let link_policy_settings = r.u16_le()?;
                Command::WriteLinkPolicySettings {
                    connection_handle,
                    link_policy_settings,
                }
            }
            HCI_WRITE_DEFAULT_LINK_POLICY_SETTINGS_COMMAND => {
                let default_link_policy_settings = r.u16_le()?;
                Command::WriteDefaultLinkPolicySettings {
                    default_link_policy_settings,
                }
            }
            HCI_SNIFF_SUBRATING_COMMAND => {
                let connection_handle = r.u16_le()?;
                let maximum_latency = r.u16_le()?;
                let minimum_remote_timeout = r.u16_le()?;
                let minimum_local_timeout = r.u16_le()?;
                Command::SniffSubrating {
                    connection_handle,
                    maximum_latency,
                    minimum_remote_timeout,
                    minimum_local_timeout,
                }
            }
            HCI_SET_EVENT_MASK_COMMAND => {
                let event_mask = r.array::<8>()?;
                Command::SetEventMask { event_mask }
            }
            HCI_RESET_COMMAND => Command::Reset,
            HCI_SET_EVENT_FILTER_COMMAND => {
                let filter_type = r.u8()?;
                let filter_condition = r.rest().to_vec();
                Command::SetEventFilter {
                    filter_type,
                    filter_condition,
                }
            }
            HCI_READ_STORED_LINK_KEY_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let read_all_flag = r.u8()?;
                Command::ReadStoredLinkKey {
                    bd_addr,
                    read_all_flag,
                }
            }
            HCI_DELETE_STORED_LINK_KEY_COMMAND => {
                let bd_addr = addr(&mut r)?;
                let delete_all_flag = r.u8()?;
                Command::DeleteStoredLinkKey {
                    bd_addr,
                    delete_all_flag,
                }
            }
            HCI_WRITE_LOCAL_NAME_COMMAND => {
                let local_name = r.array::<248>()?;
                Command::WriteLocalName { local_name }
            }
            HCI_READ_LOCAL_NAME_COMMAND => Command::ReadLocalName,
            HCI_WRITE_CONNECTION_ACCEPT_TIMEOUT_COMMAND => {
                let connection_accept_timeout = r.u16_le()?;
                Command::WriteConnectionAcceptTimeout {
                    connection_accept_timeout,
                }
            }
            HCI_WRITE_PAGE_TIMEOUT_COMMAND => {
                let page_timeout = r.u16_le()?;
                Command::WritePageTimeout { page_timeout }
            }
            HCI_WRITE_SCAN_ENABLE_COMMAND => {
                let scan_enable = r.u8()?;
                Command::WriteScanEnable { scan_enable }
            }
            HCI_READ_PAGE_SCAN_ACTIVITY_COMMAND => Command::ReadPageScanActivity,
            HCI_WRITE_PAGE_SCAN_ACTIVITY_COMMAND => {
                let page_scan_interval = r.u16_le()?;
                let page_scan_window = r.u16_le()?;
                Command::WritePageScanActivity {
                    page_scan_interval,
                    page_scan_window,
                }
            }
            HCI_WRITE_INQUIRY_SCAN_ACTIVITY_COMMAND => {
                let inquiry_scan_interval = r.u16_le()?;
                let inquiry_scan_window = r.u16_le()?;
                Command::WriteInquiryScanActivity {
                    inquiry_scan_interval,
                    inquiry_scan_window,
                }
            }
            HCI_READ_AUTHENTICATION_ENABLE_COMMAND => Command::ReadAuthenticationEnable,
            HCI_WRITE_AUTHENTICATION_ENABLE_COMMAND => {
                let authentication_enable = r.u8()?;
                Command::WriteAuthenticationEnable {
                    authentication_enable,
                }
            }
            HCI_READ_CLASS_OF_DEVICE_COMMAND => Command::ReadClassOfDevice,
            HCI_WRITE_CLASS_OF_DEVICE_COMMAND => {
                let class_of_device = r.u24_le()?;
                Command::WriteClassOfDevice { class_of_device }
            }
            HCI_READ_VOICE_SETTING_COMMAND => Command::ReadVoiceSetting,
            HCI_WRITE_VOICE_SETTING_COMMAND => {
                let voice_setting = r.u16_le()?;
                Command::WriteVoiceSetting { voice_setting }
            }
            HCI_READ_SYNCHRONOUS_FLOW_CONTROL_ENABLE_COMMAND => {
                Command::ReadSynchronousFlowControlEnable
            }
            HCI_WRITE_SYNCHRONOUS_FLOW_CONTROL_ENABLE_COMMAND => {
                let synchronous_flow_control_enable = r.u8()?;
                Command::WriteSynchronousFlowControlEnable {
                    synchronous_flow_control_enable,
                }
            }
            HCI_SET_CONTROLLER_TO_HOST_FLOW_CONTROL_COMMAND => {
                let flow_control_enable = r.u8()?;
                Command::SetControllerToHostFlowControl {
                    flow_control_enable,
                }
            }
            HCI_HOST_BUFFER_SIZE_COMMAND => {
                let host_acl_data_packet_length = r.u16_le()?;
                let host_synchronous_data_packet_length = r.u8()?;
                let host_total_num_acl_data_packets = r.u16_le()?;
                let host_total_num_synchronous_data_packets = r.u16_le()?;
                Command::HostBufferSize {
                    host_acl_data_packet_length,
                    host_synchronous_data_packet_length,
                    host_total_num_acl_data_packets,
                    host_total_num_synchronous_data_packets,
                }
            }
            HCI_WRITE_LINK_SUPERVISION_TIMEOUT_COMMAND => {
                let handle = r.u16_le()?;
                let link_supervision_timeout = r.u16_le()?;
                Command::WriteLinkSupervisionTimeout {
                    handle,
                    link_supervision_timeout,
                }
            }
            HCI_READ_NUMBER_OF_SUPPORTED_IAC_COMMAND => Command::ReadNumberOfSupportedIac,
            HCI_READ_CURRENT_IAC_LAP_COMMAND => Command::ReadCurrentIacLap,
            HCI_WRITE_INQUIRY_SCAN_TYPE_COMMAND => {
                let scan_type = r.u8()?;
                Command::WriteInquiryScanType { scan_type }
            }
            HCI_WRITE_INQUIRY_MODE_COMMAND => {
                let inquiry_mode = r.u8()?;
                Command::WriteInquiryMode { inquiry_mode }
            }
            HCI_READ_PAGE_SCAN_TYPE_COMMAND => Command::ReadPageScanType,
            HCI_WRITE_PAGE_SCAN_TYPE_COMMAND => {
                let page_scan_type = r.u8()?;
                Command::WritePageScanType { page_scan_type }
            }
            HCI_WRITE_EXTENDED_INQUIRY_RESPONSE_COMMAND => {
                let fec_required = r.u8()?;
                let extended_inquiry_response = r.array::<240>()?;
                Command::WriteExtendedInquiryResponse {
                    fec_required,
                    extended_inquiry_response,
                }
            }
            HCI_WRITE_SIMPLE_PAIRING_MODE_COMMAND => {
                let simple_pairing_mode = r.u8()?;
                Command::WriteSimplePairingMode {
                    simple_pairing_mode,
                }
            }
            HCI_READ_LOCAL_OOB_DATA_COMMAND => Command::ReadLocalOobData,
            HCI_READ_INQUIRY_RESPONSE_TRANSMIT_POWER_LEVEL_COMMAND => {
                Command::ReadInquiryResponseTransmitPowerLevel
            }
            HCI_READ_DEFAULT_ERRONEOUS_DATA_REPORTING_COMMAND => {
                Command::ReadDefaultErroneousDataReporting
            }
            HCI_SET_EVENT_MASK_PAGE_2_COMMAND => {
                let event_mask_page_2 = r.array::<8>()?;
                Command::SetEventMaskPage2 { event_mask_page_2 }
            }
            HCI_READ_LE_HOST_SUPPORT_COMMAND => Command::ReadLeHostSupport,
            HCI_WRITE_LE_HOST_SUPPORT_COMMAND => {
                let le_supported_host = r.u8()?;
                let simultaneous_le_host = r.u8()?;
                Command::WriteLeHostSupport {
                    le_supported_host,
                    simultaneous_le_host,
                }
            }
            HCI_WRITE_SECURE_CONNECTIONS_HOST_SUPPORT_COMMAND => {
                let secure_connections_host_support = r.u8()?;
                Command::WriteSecureConnectionsHostSupport {
                    secure_connections_host_support,
                }
            }
            HCI_WRITE_AUTHENTICATED_PAYLOAD_TIMEOUT_COMMAND => {
                let connection_handle = r.u16_le()?;
                let authenticated_payload_timeout = r.u16_le()?;
                Command::WriteAuthenticatedPayloadTimeout {
                    connection_handle,
                    authenticated_payload_timeout,
                }
            }
            HCI_READ_LOCAL_OOB_EXTENDED_DATA_COMMAND => Command::ReadLocalOobExtendedData,
            HCI_CONFIGURE_DATA_PATH_COMMAND => {
                let data_path_direction = r.u8()?;
                let data_path_id = r.u8()?;
                let vendor_specific_config = r.rest().to_vec();
                Command::ConfigureDataPath {
                    data_path_direction,
                    data_path_id,
                    vendor_specific_config,
                }
            }
            HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND => Command::ReadLocalVersionInformation,
            HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND => Command::ReadLocalSupportedCommands,
            HCI_READ_LOCAL_SUPPORTED_FEATURES_COMMAND => Command::ReadLocalSupportedFeatures,
            HCI_READ_LOCAL_EXTENDED_FEATURES_COMMAND => {
                let page_number = r.u8()?;
                Command::ReadLocalExtendedFeatures { page_number }
            }
            HCI_READ_BUFFER_SIZE_COMMAND => Command::ReadBufferSize,
            HCI_READ_BD_ADDR_COMMAND => Command::ReadBdAddr,
            HCI_READ_LOCAL_SUPPORTED_CODECS_COMMAND => Command::ReadLocalSupportedCodecs,
            HCI_READ_LOCAL_SUPPORTED_CODECS_V2_COMMAND => Command::ReadLocalSupportedCodecsV2,
            HCI_READ_RSSI_COMMAND => {
                let handle = r.u16_le()?;
                Command::ReadRssi { handle }
            }
            HCI_READ_ENCRYPTION_KEY_SIZE_COMMAND => {
                let connection_handle = r.u16_le()?;
                Command::ReadEncryptionKeySize { connection_handle }
            }
            HCI_READ_LOOPBACK_MODE_COMMAND => Command::ReadLoopbackMode,
            HCI_WRITE_LOOPBACK_MODE_COMMAND => {
                let loopback_mode = r.u8()?;
                Command::WriteLoopbackMode { loopback_mode }
            }
            HCI_LE_SET_EVENT_MASK_COMMAND => {
                let le_event_mask = r.array::<8>()?;
                Command::LeSetEventMask { le_event_mask }
            }
            HCI_LE_READ_BUFFER_SIZE_COMMAND => Command::LeReadBufferSize,
            HCI_LE_READ_LOCAL_SUPPORTED_FEATURES_COMMAND => Command::LeReadLocalSupportedFeatures,
            HCI_LE_SET_RANDOM_ADDRESS_COMMAND => {
                let random_address = addr(&mut r)?;
                Command::LeSetRandomAddress { random_address }
            }
            HCI_LE_SET_ADVERTISING_PARAMETERS_COMMAND => {
                let advertising_interval_min = r.u16_le()?;
                let advertising_interval_max = r.u16_le()?;
                let advertising_type = r.u8()?;
                let own_address_type = r.u8()?;
                let peer_address_type = r.u8()?;
                let peer_address = addr(&mut r)?;
                let advertising_channel_map = r.u8()?;
                let advertising_filter_policy = r.u8()?;
                Command::LeSetAdvertisingParameters {
                    advertising_interval_min,
                    advertising_interval_max,
                    advertising_type,
                    own_address_type,
                    peer_address_type,
                    peer_address,
                    advertising_channel_map,
                    advertising_filter_policy,
                }
            }
            HCI_LE_READ_ADVERTISING_PHYSICAL_CHANNEL_TX_POWER_COMMAND => {
                Command::LeReadAdvertisingPhysicalChannelTxPower
            }
            HCI_LE_SET_ADVERTISING_DATA_COMMAND => {
                let advertising_data = {
                    let n = r.u8()? as usize;
                    let f = r.array::<31>()?;
                    f[..n.min(31)].to_vec()
                };
                Command::LeSetAdvertisingData { advertising_data }
            }
            HCI_LE_SET_SCAN_RESPONSE_DATA_COMMAND => {
                let scan_response_data = {
                    let n = r.u8()? as usize;
                    let f = r.array::<31>()?;
                    f[..n.min(31)].to_vec()
                };
                Command::LeSetScanResponseData { scan_response_data }
            }
            HCI_LE_SET_ADVERTISING_ENABLE_COMMAND => {
                let advertising_enable = r.u8()?;
                Command::LeSetAdvertisingEnable { advertising_enable }
            }
            HCI_LE_SET_SCAN_PARAMETERS_COMMAND => {
                let le_scan_type = r.u8()?;
                let le_scan_interval = r.u16_le()?;
                let le_scan_window = r.u16_le()?;
                let own_address_type = r.u8()?;
                let scanning_filter_policy = r.u8()?;
                Command::LeSetScanParameters {
                    le_scan_type,
                    le_scan_interval,
                    le_scan_window,
                    own_address_type,
                    scanning_filter_policy,
                }
            }
            HCI_LE_SET_SCAN_ENABLE_COMMAND => {
                let le_scan_enable = r.u8()?;
                let filter_duplicates = r.u8()?;
                Command::LeSetScanEnable {
                    le_scan_enable,
                    filter_duplicates,
                }
            }
            HCI_LE_CREATE_CONNECTION_COMMAND => {
                let le_scan_interval = r.u16_le()?;
                let le_scan_window = r.u16_le()?;
                let initiator_filter_policy = r.u8()?;
                let peer_address_type = r.u8()?;
                let peer_address = addr(&mut r)?;
                let own_address_type = r.u8()?;
                let connection_interval_min = r.u16_le()?;
                let connection_interval_max = r.u16_le()?;
                let max_latency = r.u16_le()?;
                let supervision_timeout = r.u16_le()?;
                let min_ce_length = r.u16_le()?;
                let max_ce_length = r.u16_le()?;
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
                }
            }
            HCI_LE_CREATE_CONNECTION_CANCEL_COMMAND => Command::LeCreateConnectionCancel,
            HCI_LE_READ_FILTER_ACCEPT_LIST_SIZE_COMMAND => Command::LeReadFilterAcceptListSize,
            HCI_LE_CLEAR_FILTER_ACCEPT_LIST_COMMAND => Command::LeClearFilterAcceptList,
            HCI_LE_ADD_DEVICE_TO_FILTER_ACCEPT_LIST_COMMAND => {
                let address_type = r.u8()?;
                let address = addr(&mut r)?;
                Command::LeAddDeviceToFilterAcceptList {
                    address_type,
                    address,
                }
            }
            HCI_LE_REMOVE_DEVICE_FROM_FILTER_ACCEPT_LIST_COMMAND => {
                let address_type = r.u8()?;
                let address = addr(&mut r)?;
                Command::LeRemoveDeviceFromFilterAcceptList {
                    address_type,
                    address,
                }
            }
            HCI_LE_CONNECTION_UPDATE_COMMAND => {
                let connection_handle = r.u16_le()?;
                let connection_interval_min = r.u16_le()?;
                let connection_interval_max = r.u16_le()?;
                let max_latency = r.u16_le()?;
                let supervision_timeout = r.u16_le()?;
                let min_ce_length = r.u16_le()?;
                let max_ce_length = r.u16_le()?;
                Command::LeConnectionUpdate {
                    connection_handle,
                    connection_interval_min,
                    connection_interval_max,
                    max_latency,
                    supervision_timeout,
                    min_ce_length,
                    max_ce_length,
                }
            }
            HCI_LE_READ_REMOTE_FEATURES_COMMAND => {
                let connection_handle = r.u16_le()?;
                Command::LeReadRemoteFeatures { connection_handle }
            }
            HCI_LE_RAND_COMMAND => Command::LeRand,
            HCI_LE_ENABLE_ENCRYPTION_COMMAND => {
                let connection_handle = r.u16_le()?;
                let random_number = r.array::<8>()?;
                let encrypted_diversifier = r.u16_le()?;
                let long_term_key = r.array::<16>()?;
                Command::LeEnableEncryption {
                    connection_handle,
                    random_number,
                    encrypted_diversifier,
                    long_term_key,
                }
            }
            HCI_LE_LONG_TERM_KEY_REQUEST_REPLY_COMMAND => {
                let connection_handle = r.u16_le()?;
                let long_term_key = r.array::<16>()?;
                Command::LeLongTermKeyRequestReply {
                    connection_handle,
                    long_term_key,
                }
            }
            HCI_LE_LONG_TERM_KEY_REQUEST_NEGATIVE_REPLY_COMMAND => {
                let connection_handle = r.u16_le()?;
                Command::LeLongTermKeyRequestNegativeReply { connection_handle }
            }
            HCI_LE_READ_SUPPORTED_STATES_COMMAND => Command::LeReadSupportedStates,
            HCI_LE_REMOTE_CONNECTION_PARAMETER_REQUEST_REPLY_COMMAND => {
                let connection_handle = r.u16_le()?;
                let interval_min = r.u16_le()?;
                let interval_max = r.u16_le()?;
                let max_latency = r.u16_le()?;
                let timeout = r.u16_le()?;
                let min_ce_length = r.u16_le()?;
                let max_ce_length = r.u16_le()?;
                Command::LeRemoteConnectionParameterRequestReply {
                    connection_handle,
                    interval_min,
                    interval_max,
                    max_latency,
                    timeout,
                    min_ce_length,
                    max_ce_length,
                }
            }
            HCI_LE_REMOTE_CONNECTION_PARAMETER_REQUEST_NEGATIVE_REPLY_COMMAND => {
                let connection_handle = r.u16_le()?;
                let reason = r.u8()?;
                Command::LeRemoteConnectionParameterRequestNegativeReply {
                    connection_handle,
                    reason,
                }
            }
            HCI_LE_SET_DATA_LENGTH_COMMAND => {
                let connection_handle = r.u16_le()?;
                let tx_octets = r.u16_le()?;
                let tx_time = r.u16_le()?;
                Command::LeSetDataLength {
                    connection_handle,
                    tx_octets,
                    tx_time,
                }
            }
            HCI_LE_READ_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND => {
                Command::LeReadSuggestedDefaultDataLength
            }
            HCI_LE_WRITE_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND => {
                let suggested_max_tx_octets = r.u16_le()?;
                let suggested_max_tx_time = r.u16_le()?;
                Command::LeWriteSuggestedDefaultDataLength {
                    suggested_max_tx_octets,
                    suggested_max_tx_time,
                }
            }
            HCI_LE_READ_LOCAL_P_256_PUBLIC_KEY_COMMAND => Command::LeReadLocalP256PublicKey,
            HCI_LE_ADD_DEVICE_TO_RESOLVING_LIST_COMMAND => {
                let peer_identity_address_type = r.u8()?;
                let peer_identity_address = addr(&mut r)?;
                let peer_irk = r.array::<16>()?;
                let local_irk = r.array::<16>()?;
                Command::LeAddDeviceToResolvingList {
                    peer_identity_address_type,
                    peer_identity_address,
                    peer_irk,
                    local_irk,
                }
            }
            HCI_LE_CLEAR_RESOLVING_LIST_COMMAND => Command::LeClearResolvingList,
            HCI_LE_READ_RESOLVING_LIST_SIZE_COMMAND => Command::LeReadResolvingListSize,
            HCI_LE_SET_ADDRESS_RESOLUTION_ENABLE_COMMAND => {
                let address_resolution_enable = r.u8()?;
                Command::LeSetAddressResolutionEnable {
                    address_resolution_enable,
                }
            }
            HCI_LE_SET_RESOLVABLE_PRIVATE_ADDRESS_TIMEOUT_COMMAND => {
                let rpa_timeout = r.u16_le()?;
                Command::LeSetResolvablePrivateAddressTimeout { rpa_timeout }
            }
            HCI_LE_READ_MAXIMUM_DATA_LENGTH_COMMAND => Command::LeReadMaximumDataLength,
            HCI_LE_READ_PHY_COMMAND => {
                let connection_handle = r.u16_le()?;
                Command::LeReadPhy { connection_handle }
            }
            HCI_LE_SET_DEFAULT_PHY_COMMAND => {
                let all_phys = r.u8()?;
                let tx_phys = r.u8()?;
                let rx_phys = r.u8()?;
                Command::LeSetDefaultPhy {
                    all_phys,
                    tx_phys,
                    rx_phys,
                }
            }
            HCI_LE_SET_PHY_COMMAND => {
                let connection_handle = r.u16_le()?;
                let all_phys = r.u8()?;
                let tx_phys = r.u8()?;
                let rx_phys = r.u8()?;
                let phy_options = r.u16_le()?;
                Command::LeSetPhy {
                    connection_handle,
                    all_phys,
                    tx_phys,
                    rx_phys,
                    phy_options,
                }
            }
            HCI_LE_SET_ADVERTISING_SET_RANDOM_ADDRESS_COMMAND => {
                let advertising_handle = r.u8()?;
                let random_address = addr(&mut r)?;
                Command::LeSetAdvertisingSetRandomAddress {
                    advertising_handle,
                    random_address,
                }
            }
            HCI_LE_SET_EXTENDED_ADVERTISING_PARAMETERS_COMMAND => {
                let advertising_handle = r.u8()?;
                let advertising_event_properties = r.u16_le()?;
                let primary_advertising_interval_min = r.u24_le()?;
                let primary_advertising_interval_max = r.u24_le()?;
                let primary_advertising_channel_map = r.u8()?;
                let own_address_type = r.u8()?;
                let peer_address_type = r.u8()?;
                let peer_address = addr(&mut r)?;
                let advertising_filter_policy = r.u8()?;
                let advertising_tx_power = r.u8()?;
                let primary_advertising_phy = r.u8()?;
                let secondary_advertising_max_skip = r.u8()?;
                let secondary_advertising_phy = r.u8()?;
                let advertising_sid = r.u8()?;
                let scan_request_notification_enable = r.u8()?;
                Command::LeSetExtendedAdvertisingParameters {
                    advertising_handle,
                    advertising_event_properties,
                    primary_advertising_interval_min,
                    primary_advertising_interval_max,
                    primary_advertising_channel_map,
                    own_address_type,
                    peer_address_type,
                    peer_address,
                    advertising_filter_policy,
                    advertising_tx_power,
                    primary_advertising_phy,
                    secondary_advertising_max_skip,
                    secondary_advertising_phy,
                    advertising_sid,
                    scan_request_notification_enable,
                }
            }
            HCI_LE_SET_EXTENDED_ADVERTISING_DATA_COMMAND => {
                let advertising_handle = r.u8()?;
                let operation = r.u8()?;
                let fragment_preference = r.u8()?;
                let advertising_data = {
                    let n = r.u8()? as usize;
                    r.take(n)?.to_vec()
                };
                Command::LeSetExtendedAdvertisingData {
                    advertising_handle,
                    operation,
                    fragment_preference,
                    advertising_data,
                }
            }
            HCI_LE_SET_EXTENDED_SCAN_RESPONSE_DATA_COMMAND => {
                let advertising_handle = r.u8()?;
                let operation = r.u8()?;
                let fragment_preference = r.u8()?;
                let scan_response_data = {
                    let n = r.u8()? as usize;
                    r.take(n)?.to_vec()
                };
                Command::LeSetExtendedScanResponseData {
                    advertising_handle,
                    operation,
                    fragment_preference,
                    scan_response_data,
                }
            }
            HCI_LE_SET_EXTENDED_ADVERTISING_ENABLE_COMMAND => {
                let enable = r.u8()?;
                let count0 = r.u8()? as usize;
                let mut advertising_handles = Vec::with_capacity(count0);
                let mut durations = Vec::with_capacity(count0);
                let mut max_extended_advertising_events = Vec::with_capacity(count0);
                for _ in 0..count0 {
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
            HCI_LE_READ_MAXIMUM_ADVERTISING_DATA_LENGTH_COMMAND => {
                Command::LeReadMaximumAdvertisingDataLength
            }
            HCI_LE_READ_NUMBER_OF_SUPPORTED_ADVERTISING_SETS_COMMAND => {
                Command::LeReadNumberOfSupportedAdvertisingSets
            }
            HCI_LE_REMOVE_ADVERTISING_SET_COMMAND => {
                let advertising_handle = r.u8()?;
                Command::LeRemoveAdvertisingSet { advertising_handle }
            }
            HCI_LE_CLEAR_ADVERTISING_SETS_COMMAND => Command::LeClearAdvertisingSets,
            HCI_LE_SET_PERIODIC_ADVERTISING_PARAMETERS_COMMAND => {
                let advertising_handle = r.u8()?;
                let periodic_advertising_interval_min = r.u16_le()?;
                let periodic_advertising_interval_max = r.u16_le()?;
                let periodic_advertising_properties = r.u16_le()?;
                Command::LeSetPeriodicAdvertisingParameters {
                    advertising_handle,
                    periodic_advertising_interval_min,
                    periodic_advertising_interval_max,
                    periodic_advertising_properties,
                }
            }
            HCI_LE_SET_PERIODIC_ADVERTISING_DATA_COMMAND => {
                let advertising_handle = r.u8()?;
                let operation = r.u8()?;
                let advertising_data = {
                    let n = r.u8()? as usize;
                    r.take(n)?.to_vec()
                };
                Command::LeSetPeriodicAdvertisingData {
                    advertising_handle,
                    operation,
                    advertising_data,
                }
            }
            HCI_LE_SET_PERIODIC_ADVERTISING_ENABLE_COMMAND => {
                let enable = r.u8()?;
                let advertising_handle = r.u8()?;
                Command::LeSetPeriodicAdvertisingEnable {
                    enable,
                    advertising_handle,
                }
            }
            HCI_LE_SET_EXTENDED_SCAN_ENABLE_COMMAND => {
                let enable = r.u8()?;
                let filter_duplicates = r.u8()?;
                let duration = r.u16_le()?;
                let period = r.u16_le()?;
                Command::LeSetExtendedScanEnable {
                    enable,
                    filter_duplicates,
                    duration,
                    period,
                }
            }
            HCI_LE_PERIODIC_ADVERTISING_CREATE_SYNC_COMMAND => {
                let options = r.u8()?;
                let advertising_sid = r.u8()?;
                let advertiser_address_type = r.u8()?;
                let advertiser_address = addr(&mut r)?;
                let skip = r.u16_le()?;
                let sync_timeout = r.u16_le()?;
                let sync_cte_type = r.u8()?;
                Command::LePeriodicAdvertisingCreateSync {
                    options,
                    advertising_sid,
                    advertiser_address_type,
                    advertiser_address,
                    skip,
                    sync_timeout,
                    sync_cte_type,
                }
            }
            HCI_LE_PERIODIC_ADVERTISING_CREATE_SYNC_CANCEL_COMMAND => {
                Command::LePeriodicAdvertisingCreateSyncCancel
            }
            HCI_LE_PERIODIC_ADVERTISING_TERMINATE_SYNC_COMMAND => {
                let sync_handle = r.u16_le()?;
                Command::LePeriodicAdvertisingTerminateSync { sync_handle }
            }
            HCI_LE_READ_TRANSMIT_POWER_COMMAND => Command::LeReadTransmitPower,
            HCI_LE_SET_PRIVACY_MODE_COMMAND => {
                let peer_identity_address_type = r.u8()?;
                let peer_identity_address = addr(&mut r)?;
                let privacy_mode = r.u8()?;
                Command::LeSetPrivacyMode {
                    peer_identity_address_type,
                    peer_identity_address,
                    privacy_mode,
                }
            }
            HCI_LE_SET_PERIODIC_ADVERTISING_RECEIVE_ENABLE_COMMAND => {
                let sync_handle = r.u16_le()?;
                let enable = r.u8()?;
                Command::LeSetPeriodicAdvertisingReceiveEnable {
                    sync_handle,
                    enable,
                }
            }
            HCI_LE_PERIODIC_ADVERTISING_SYNC_TRANSFER_COMMAND => {
                let connection_handle = r.u16_le()?;
                let service_data = r.u16_le()?;
                let sync_handle = r.u16_le()?;
                Command::LePeriodicAdvertisingSyncTransfer {
                    connection_handle,
                    service_data,
                    sync_handle,
                }
            }
            HCI_LE_PERIODIC_ADVERTISING_SET_INFO_TRANSFER_COMMAND => {
                let connection_handle = r.u16_le()?;
                let service_data = r.u16_le()?;
                let advertising_handle = r.u8()?;
                Command::LePeriodicAdvertisingSetInfoTransfer {
                    connection_handle,
                    service_data,
                    advertising_handle,
                }
            }
            HCI_LE_SET_PERIODIC_ADVERTISING_SYNC_TRANSFER_PARAMETERS_COMMAND => {
                let connection_handle = r.u16_le()?;
                let mode = r.u8()?;
                let skip = r.u16_le()?;
                let sync_timeout = r.u16_le()?;
                let cte_type = r.u8()?;
                Command::LeSetPeriodicAdvertisingSyncTransferParameters {
                    connection_handle,
                    mode,
                    skip,
                    sync_timeout,
                    cte_type,
                }
            }
            HCI_LE_SET_DEFAULT_PERIODIC_ADVERTISING_SYNC_TRANSFER_PARAMETERS_COMMAND => {
                let mode = r.u8()?;
                let skip = r.u16_le()?;
                let sync_timeout = r.u16_le()?;
                let cte_type = r.u8()?;
                Command::LeSetDefaultPeriodicAdvertisingSyncTransferParameters {
                    mode,
                    skip,
                    sync_timeout,
                    cte_type,
                }
            }
            HCI_LE_READ_BUFFER_SIZE_V2_COMMAND => Command::LeReadBufferSizeV2,
            HCI_LE_READ_ISO_TX_SYNC_COMMAND => {
                let connection_handle = r.u16_le()?;
                Command::LeReadIsoTxSync { connection_handle }
            }
            HCI_LE_SET_CIG_PARAMETERS_COMMAND => {
                let cig_id = r.u8()?;
                let sdu_interval_c_to_p = r.u24_le()?;
                let sdu_interval_p_to_c = r.u24_le()?;
                let worst_case_sca = r.u8()?;
                let packing = r.u8()?;
                let framing = r.u8()?;
                let max_transport_latency_c_to_p = r.u16_le()?;
                let max_transport_latency_p_to_c = r.u16_le()?;
                let count0 = r.u8()? as usize;
                let mut cis_id = Vec::with_capacity(count0);
                let mut max_sdu_c_to_p = Vec::with_capacity(count0);
                let mut max_sdu_p_to_c = Vec::with_capacity(count0);
                let mut phy_c_to_p = Vec::with_capacity(count0);
                let mut phy_p_to_c = Vec::with_capacity(count0);
                let mut rtn_c_to_p = Vec::with_capacity(count0);
                let mut rtn_p_to_c = Vec::with_capacity(count0);
                for _ in 0..count0 {
                    cis_id.push(r.u8()?);
                    max_sdu_c_to_p.push(r.u16_le()?);
                    max_sdu_p_to_c.push(r.u16_le()?);
                    phy_c_to_p.push(r.u8()?);
                    phy_p_to_c.push(r.u8()?);
                    rtn_c_to_p.push(r.u8()?);
                    rtn_p_to_c.push(r.u8()?);
                }
                Command::LeSetCigParameters {
                    cig_id,
                    sdu_interval_c_to_p,
                    sdu_interval_p_to_c,
                    worst_case_sca,
                    packing,
                    framing,
                    max_transport_latency_c_to_p,
                    max_transport_latency_p_to_c,
                    cis_id,
                    max_sdu_c_to_p,
                    max_sdu_p_to_c,
                    phy_c_to_p,
                    phy_p_to_c,
                    rtn_c_to_p,
                    rtn_p_to_c,
                }
            }
            HCI_LE_CREATE_CIS_COMMAND => {
                let count0 = r.u8()? as usize;
                let mut cis_connection_handle = Vec::with_capacity(count0);
                let mut acl_connection_handle = Vec::with_capacity(count0);
                for _ in 0..count0 {
                    cis_connection_handle.push(r.u16_le()?);
                    acl_connection_handle.push(r.u16_le()?);
                }
                Command::LeCreateCis {
                    cis_connection_handle,
                    acl_connection_handle,
                }
            }
            HCI_LE_REMOVE_CIG_COMMAND => {
                let cig_id = r.u8()?;
                Command::LeRemoveCig { cig_id }
            }
            HCI_LE_ACCEPT_CIS_REQUEST_COMMAND => {
                let connection_handle = r.u16_le()?;
                Command::LeAcceptCisRequest { connection_handle }
            }
            HCI_LE_REJECT_CIS_REQUEST_COMMAND => {
                let connection_handle = r.u16_le()?;
                let reason = r.u8()?;
                Command::LeRejectCisRequest {
                    connection_handle,
                    reason,
                }
            }
            HCI_LE_CREATE_BIG_COMMAND => {
                let big_handle = r.u8()?;
                let advertising_handle = r.u8()?;
                let num_bis = r.u8()?;
                let sdu_interval = r.u24_le()?;
                let max_sdu = r.u16_le()?;
                let max_transport_latency = r.u16_le()?;
                let rtn = r.u8()?;
                let phy = r.u8()?;
                let packing = r.u8()?;
                let framing = r.u8()?;
                let encryption = r.u8()?;
                let broadcast_code = r.array::<16>()?;
                Command::LeCreateBig {
                    big_handle,
                    advertising_handle,
                    num_bis,
                    sdu_interval,
                    max_sdu,
                    max_transport_latency,
                    rtn,
                    phy,
                    packing,
                    framing,
                    encryption,
                    broadcast_code,
                }
            }
            HCI_LE_TERMINATE_BIG_COMMAND => {
                let big_handle = r.u8()?;
                let reason = r.u8()?;
                Command::LeTerminateBig { big_handle, reason }
            }
            HCI_LE_BIG_CREATE_SYNC_COMMAND => {
                let big_handle = r.u8()?;
                let sync_handle = r.u16_le()?;
                let encryption = r.u8()?;
                let broadcast_code = r.array::<16>()?;
                let mse = r.u8()?;
                let big_sync_timeout = r.u16_le()?;
                let count0 = r.u8()? as usize;
                let mut bis = Vec::with_capacity(count0);
                for _ in 0..count0 {
                    bis.push(r.u8()?);
                }
                Command::LeBigCreateSync {
                    big_handle,
                    sync_handle,
                    encryption,
                    broadcast_code,
                    mse,
                    big_sync_timeout,
                    bis,
                }
            }
            HCI_LE_BIG_TERMINATE_SYNC_COMMAND => {
                let big_handle = r.u8()?;
                Command::LeBigTerminateSync { big_handle }
            }
            HCI_LE_SETUP_ISO_DATA_PATH_COMMAND => {
                let connection_handle = r.u16_le()?;
                let data_path_direction = r.u8()?;
                let data_path_id = r.u8()?;
                let codec_id = CodingFormat::read(&mut r)?;
                let controller_delay = r.u24_le()?;
                let codec_configuration = {
                    let n = r.u8()? as usize;
                    r.take(n)?.to_vec()
                };
                Command::LeSetupIsoDataPath {
                    connection_handle,
                    data_path_direction,
                    data_path_id,
                    codec_id,
                    controller_delay,
                    codec_configuration,
                }
            }
            HCI_LE_REMOVE_ISO_DATA_PATH_COMMAND => {
                let connection_handle = r.u16_le()?;
                let data_path_direction = r.u8()?;
                Command::LeRemoveIsoDataPath {
                    connection_handle,
                    data_path_direction,
                }
            }
            HCI_LE_SET_HOST_FEATURE_COMMAND => {
                let bit_number = r.u8()?;
                let bit_value = r.u8()?;
                Command::LeSetHostFeature {
                    bit_number,
                    bit_value,
                }
            }
            HCI_LE_SET_DEFAULT_SUBRATE_COMMAND => {
                let subrate_min = r.u16_le()?;
                let subrate_max = r.u16_le()?;
                let max_latency = r.u16_le()?;
                let continuation_number = r.u16_le()?;
                let supervision_timeout = r.u16_le()?;
                Command::LeSetDefaultSubrate {
                    subrate_min,
                    subrate_max,
                    max_latency,
                    continuation_number,
                    supervision_timeout,
                }
            }
            HCI_LE_SUBRATE_REQUEST_COMMAND => {
                let connection_handle = r.u16_le()?;
                let subrate_min = r.u16_le()?;
                let subrate_max = r.u16_le()?;
                let max_latency = r.u16_le()?;
                let continuation_number = r.u16_le()?;
                let supervision_timeout = r.u16_le()?;
                Command::LeSubrateRequest {
                    connection_handle,
                    subrate_min,
                    subrate_max,
                    max_latency,
                    continuation_number,
                    supervision_timeout,
                }
            }
            HCI_LE_CS_READ_LOCAL_SUPPORTED_CAPABILITIES_COMMAND => {
                Command::LeCsReadLocalSupportedCapabilities
            }
            HCI_LE_CS_READ_REMOTE_SUPPORTED_CAPABILITIES_COMMAND => {
                let connection_handle = r.u16_le()?;
                Command::LeCsReadRemoteSupportedCapabilities { connection_handle }
            }
            HCI_LE_CS_WRITE_CACHED_REMOTE_SUPPORTED_CAPABILITIES_COMMAND => {
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
                Command::LeCsWriteCachedRemoteSupportedCapabilities {
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
            HCI_LE_CS_SECURITY_ENABLE_COMMAND => {
                let connection_handle = r.u16_le()?;
                Command::LeCsSecurityEnable { connection_handle }
            }
            HCI_LE_CS_SET_DEFAULT_SETTINGS_COMMAND => {
                let connection_handle = r.u16_le()?;
                let role_enable = r.u8()?;
                let cs_sync_antenna_selection = r.u8()?;
                let max_tx_power = r.u8()?;
                Command::LeCsSetDefaultSettings {
                    connection_handle,
                    role_enable,
                    cs_sync_antenna_selection,
                    max_tx_power,
                }
            }
            HCI_LE_CS_READ_REMOTE_FAE_TABLE_COMMAND => {
                let connection_handle = r.u16_le()?;
                Command::LeCsReadRemoteFaeTable { connection_handle }
            }
            HCI_LE_CS_WRITE_CACHED_REMOTE_FAE_TABLE_COMMAND => {
                let connection_handle = r.u16_le()?;
                let remote_fae_table = r.array::<72>()?;
                Command::LeCsWriteCachedRemoteFaeTable {
                    connection_handle,
                    remote_fae_table,
                }
            }
            HCI_LE_CS_CREATE_CONFIG_COMMAND => {
                let connection_handle = r.u16_le()?;
                let config_id = r.u8()?;
                let create_context = r.u8()?;
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
                Command::LeCsCreateConfig {
                    connection_handle,
                    config_id,
                    create_context,
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
                }
            }
            HCI_LE_CS_REMOVE_CONFIG_COMMAND => {
                let connection_handle = r.u16_le()?;
                let config_id = r.u8()?;
                Command::LeCsRemoveConfig {
                    connection_handle,
                    config_id,
                }
            }
            HCI_LE_CS_SET_CHANNEL_CLASSIFICATION_COMMAND => {
                let channel_classification = r.array::<10>()?;
                Command::LeCsSetChannelClassification {
                    channel_classification,
                }
            }
            HCI_LE_CS_SET_PROCEDURE_PARAMETERS_COMMAND => {
                let connection_handle = r.u16_le()?;
                let config_id = r.u8()?;
                let max_procedure_len = r.u16_le()?;
                let min_procedure_interval = r.u16_le()?;
                let max_procedure_interval = r.u16_le()?;
                let max_procedure_count = r.u16_le()?;
                let min_subevent_len = r.u24_le()?;
                let max_subevent_len = r.u24_le()?;
                let tone_antenna_config_selection = r.u8()?;
                let phy = r.u8()?;
                let tx_power_delta = r.u8()?;
                let preferred_peer_antenna = r.u8()?;
                let snr_control_initiator = r.u8()?;
                let snr_control_reflector = r.u8()?;
                Command::LeCsSetProcedureParameters {
                    connection_handle,
                    config_id,
                    max_procedure_len,
                    min_procedure_interval,
                    max_procedure_interval,
                    max_procedure_count,
                    min_subevent_len,
                    max_subevent_len,
                    tone_antenna_config_selection,
                    phy,
                    tx_power_delta,
                    preferred_peer_antenna,
                    snr_control_initiator,
                    snr_control_reflector,
                }
            }
            HCI_LE_CS_PROCEDURE_ENABLE_COMMAND => {
                let connection_handle = r.u16_le()?;
                let config_id = r.u8()?;
                let enable = r.u8()?;
                Command::LeCsProcedureEnable {
                    connection_handle,
                    config_id,
                    enable,
                }
            }
            HCI_LE_CS_TEST_COMMAND => {
                let main_mode_type = r.u8()?;
                let sub_mode_type = r.u8()?;
                let main_mode_repetition = r.u8()?;
                let mode_0_steps = r.u8()?;
                let role = r.u8()?;
                let rtt_type = r.u8()?;
                let cs_sync_phy = r.u8()?;
                let cs_sync_antenna_selection = r.u8()?;
                let subevent_len = r.u24_le()?;
                let subevent_interval = r.u16_le()?;
                let max_num_subevents = r.u8()?;
                let transmit_power_level = r.u8()?;
                let t_ip1_time = r.u8()?;
                let t_ip2_time = r.u8()?;
                let t_fcs_time = r.u8()?;
                let t_pm_time = r.u8()?;
                let t_sw_time = r.u8()?;
                let tone_antenna_config_selection = r.u8()?;
                let reserved = r.u8()?;
                let snr_control_initiator = r.u8()?;
                let snr_control_reflector = r.u8()?;
                let drbg_nonce = r.u16_le()?;
                let channel_map_repetition = r.u8()?;
                let override_config = r.u16_le()?;
                let override_parameters_data = {
                    let n = r.u8()? as usize;
                    r.take(n)?.to_vec()
                };
                Command::LeCsTest {
                    main_mode_type,
                    sub_mode_type,
                    main_mode_repetition,
                    mode_0_steps,
                    role,
                    rtt_type,
                    cs_sync_phy,
                    cs_sync_antenna_selection,
                    subevent_len,
                    subevent_interval,
                    max_num_subevents,
                    transmit_power_level,
                    t_ip1_time,
                    t_ip2_time,
                    t_fcs_time,
                    t_pm_time,
                    t_sw_time,
                    tone_antenna_config_selection,
                    reserved,
                    snr_control_initiator,
                    snr_control_reflector,
                    drbg_nonce,
                    channel_map_repetition,
                    override_config,
                    override_parameters_data,
                }
            }
            HCI_LE_CS_TEST_END_COMMAND => Command::LeCsTestEnd,
            HCI_LE_FRAME_SPACE_UPDATE_COMMAND => {
                let connection_handle = r.u16_le()?;
                let frame_space_min = r.u16_le()?;
                let frame_space_max = r.u16_le()?;
                let phys = r.u8()?;
                let spacing_types = r.u16_le()?;
                Command::LeFrameSpaceUpdate {
                    connection_handle,
                    frame_space_min,
                    frame_space_max,
                    phys,
                    spacing_types,
                }
            }
            HCI_LE_CONNECTION_RATE_REQUEST_COMMAND => {
                let connection_handle = r.u16_le()?;
                let connection_interval_min = r.u16_le()?;
                let connection_interval_max = r.u16_le()?;
                let subrate_min = r.u16_le()?;
                let subrate_max = r.u16_le()?;
                let max_latency = r.u16_le()?;
                let continuation_number = r.u16_le()?;
                let supervision_timeout = r.u16_le()?;
                let min_ce_length = r.u16_le()?;
                let max_ce_length = r.u16_le()?;
                Command::LeConnectionRateRequest {
                    connection_handle,
                    connection_interval_min,
                    connection_interval_max,
                    subrate_min,
                    subrate_max,
                    max_latency,
                    continuation_number,
                    supervision_timeout,
                    min_ce_length,
                    max_ce_length,
                }
            }
            HCI_LE_SET_DEFAULT_RATE_PARAMETERS_COMMAND => {
                let connection_interval_min = r.u16_le()?;
                let connection_interval_max = r.u16_le()?;
                let subrate_min = r.u16_le()?;
                let subrate_max = r.u16_le()?;
                let max_latency = r.u16_le()?;
                let continuation_number = r.u16_le()?;
                let supervision_timeout = r.u16_le()?;
                let min_ce_length = r.u16_le()?;
                let max_ce_length = r.u16_le()?;
                Command::LeSetDefaultRateParameters {
                    connection_interval_min,
                    connection_interval_max,
                    subrate_min,
                    subrate_max,
                    max_latency,
                    continuation_number,
                    supervision_timeout,
                    min_ce_length,
                    max_ce_length,
                }
            }
            HCI_LE_READ_MINIMUM_SUPPORTED_CONNECTION_INTERVAL_COMMAND => {
                Command::LeReadMinimumSupportedConnectionInterval
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
            _ => Command::Generic {
                op_code,
                parameters: parameters.to_vec(),
            },
        })
    }
}
