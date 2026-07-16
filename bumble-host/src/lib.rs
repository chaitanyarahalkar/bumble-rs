//! bumble-host — the host-side glue of the [`google/bumble`](https://github.com/google/bumble)
//! port.
//!
//! **Slice 10** of the incremental port: a [`Device`] that owns the sequencing
//! the earlier integration tests wired by hand — wrapping ATT PDUs in L2CAP and
//! ACL to send, and unwrapping received ACL back up to ATT. This turns the
//! cross-layer composition into a real library capability.
//!
//! A `Device` sits above a [`HostTransport`], either an in-process
//! [`bumble_controller::LocalLink`] or an external-controller adapter. It:
//! - owns its LE connections by handle and exposes a selectable current one,
//! - sends ATT PDUs on the ATT channel with [`Device::send_att`],
//! - on [`Device::poll`], processes inbound ACL: an optional server-role
//!   [`bumble_gatt::AttServer`] answers requests automatically; other ATT PDUs (responses /
//!   notifications) are queued for the client to collect.
//!
//! [`pump`] drives a set of devices to quiescence for deterministic in-process
//! operation; external adapters can wait for transport activity between polls.
//!
//! ## Scope
//!
//! ATT traffic over both the fixed ATT CID and Enhanced ATT LE credit channels,
//! plus raw fixed/dynamic L2CAP channels, with controller-buffer-sized ACL
//! fragmentation/reassembly. High-level
//! legacy and extended advertising, scanning, and connection setup are also
//! available, along with periodic advertising/synchronization, CIG/CIS and
//! BIG/BIS control, PAST transfer, ISO SDU fragmentation/reassembly, and handle-scoped LE
//! credit-based channel managers driven over the same ACL path.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use bumble::keys::{JsonKeyStore, Key, KeyStore, KeyStoreError, MemoryKeyStore, PairingKeys};
use bumble::{Address, AdvertisingData};
use bumble_att::AttPdu;
use bumble_controller::LocalLink as ControllerLocalLink;
use bumble_gatt::{AccessContext, AttRequestHandler, ATT_DEFAULT_MTU};
use bumble_hci::{
    fragment_l2cap_pdu, AclDataPacket, AclDataPacketAssembler, AdvertisingReport, CodingFormat,
    Command, Event, ExtendedAdvertisingReport, HciPacket, IsoDataPacket, LeMetaEvent,
    SynchronousDataPacket,
};
use bumble_l2cap::{
    ChannelManager as ClassicChannelManager, ClassicChannel, ClassicChannelSpec,
    Error as L2capError, InformationCapabilities, InformationResponse, L2capPdu,
    LeCreditBasedChannel, LeCreditBasedChannelSpec, LeCreditChannelManager,
    L2CAP_LE_PSM_DYNAMIC_RANGE_END, L2CAP_LE_PSM_DYNAMIC_RANGE_START, L2CAP_LE_SIGNALING_CID,
    L2CAP_SIGNALING_CID,
};
use bumble_smp::{
    AddressResolver, ClassicCtkdState, ManagedPairingState, PairingConnection,
    PairingFailureReason, PairingManager, PairingRole, PairingState, ScPairingState, SmpPdu,
    SMP_BR_CID,
};

mod configuration;
mod data_queue;
pub use configuration::{
    DeviceConfiguration, DeviceConfigurationError, DEVICE_DEFAULT_ADDRESS,
    DEVICE_DEFAULT_ADVERTISING_INTERVAL, DEVICE_DEFAULT_CLASS_OF_DEVICE,
    DEVICE_DEFAULT_LE_RPA_TIMEOUT, DEVICE_DEFAULT_NAME,
};
pub use data_queue::{DataPacketQueue, DataPacketQueueError};

/// The fixed L2CAP channel id for the Attribute Protocol.
pub const ATT_CID: u16 = 0x0004;
/// LE Protocol/Service Multiplexer assigned to Enhanced ATT.
pub const EATT_PSM: u16 = 0x0027;
/// The fixed L2CAP channel id for LE SMP.
pub const SMP_CID: u16 = 0x0006;

/// HCI identifier for the mandatory LE 1M PHY.
pub const LE_1M_PHY: u8 = 0x01;
/// HCI identifier for the optional LE 2M PHY.
pub const LE_2M_PHY: u8 = 0x02;
/// HCI identifier for the optional LE Coded PHY.
pub const LE_CODED_PHY: u8 = 0x03;

/// LE feature-bit identifiers used by upstream `Device` capability helpers.
pub const LE_FEATURE_2M_PHY: u8 = 8;
pub const LE_FEATURE_CODED_PHY: u8 = 11;
pub const LE_FEATURE_EXTENDED_ADVERTISING: u8 = 12;
pub const LE_FEATURE_PERIODIC_ADVERTISING: u8 = 13;
/// LMP feature-bit identifiers used by upstream's Classic scan setup.
pub const LMP_FEATURE_INTERLACED_INQUIRY_SCAN: u16 = 28;
pub const LMP_FEATURE_INTERLACED_PAGE_SCAN: u16 = 29;

/// Classic event mask installed by upstream `Host.reset`.
pub const HOST_EVENT_MASK: [u8; 8] = [0xFF, 0x9F, 0xFF, 0xBF, 0x07, 0xF8, 0xBF, 0x3D];
/// Page-2 event mask enabling Encryption Change V2.
pub const HOST_EVENT_MASK_PAGE_2: [u8; 8] = [0, 0, 0, 2, 0, 0, 0, 0];
/// Complete LE Meta event mask installed for controllers newer than Bluetooth 4.0.
pub const HOST_LE_EVENT_MASK: [u8; 8] = [0xFF, 0xFF, 0xF7, 0xFF, 0x0F, 0xED, 0x7B, 0x00];
/// Conservative LE Meta event mask used for Bluetooth 4.0 and older controllers.
pub const HOST_LE_EVENT_MASK_LEGACY: [u8; 8] = [0x1F, 0, 0, 0, 0, 0, 0, 0];
/// Upstream Host reset target for the controller's suggested maximum TX octets.
pub const HOST_SUGGESTED_MAX_TX_OCTETS: u16 = 251;
/// Upstream Host reset target for the controller's suggested maximum TX time.
pub const HOST_SUGGESTED_MAX_TX_TIME: u16 = 2_120;
/// Legacy advertising-data capacity retained when the extended query is unavailable.
pub const HOST_DEFAULT_MAXIMUM_ADVERTISING_DATA_LENGTH: u16 = 31;

fn advertising_interval_units(milliseconds: f64) -> Option<u16> {
    let units = (milliseconds / 0.625).trunc();
    (milliseconds.is_finite() && (0x0020 as f64..=0x4000 as f64).contains(&units))
        .then_some(units as u16)
}

const LE_FEATURE_CONNECTED_ISOCHRONOUS_STREAM: u8 = 32;
const LE_FEATURE_CONNECTION_SUBRATING_HOST_SUPPORT: u8 = 38;
const LE_FEATURE_CHANNEL_SOUNDING_HOST_SUPPORT: u8 = 47;
const LE_FEATURE_SHORTER_CONNECTION_INTERVALS_HOST_SUPPORT: u8 = 73;

/// Validation failures raised before a configured [`Device`] is powered on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DevicePowerError {
    InvalidIrkLength { actual: usize },
    LocalNameTooLong { actual: usize, maximum: usize },
    ClassOfDeviceOutOfRange { value: u32 },
    NotPoweredOn,
    PrivacyDisabled,
}

impl std::fmt::Display for DevicePowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIrkLength { actual } => {
                write!(f, "IRK must contain 16 bytes, got {actual}")
            }
            Self::LocalNameTooLong { actual, maximum } => write!(
                f,
                "UTF-8 local name is {actual} bytes, maximum is {maximum}"
            ),
            Self::ClassOfDeviceOutOfRange { value } => {
                write!(f, "Class of Device 0x{value:08X} does not fit in 24 bits")
            }
            Self::NotPoweredOn => write!(f, "device is not powered on"),
            Self::PrivacyDisabled => write!(f, "LE privacy is disabled"),
        }
    }
}

impl std::error::Error for DevicePowerError {}

/// Invalid physical-layer identifiers passed to [`Device::supports_le_phy`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LePhyError {
    InvalidPhy { phy: u8 },
}

impl std::fmt::Display for LePhyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPhy { phy } => write!(f, "invalid LE PHY 0x{phy:02X}"),
        }
    }
}

impl std::error::Error for LePhyError {}

/// Failures produced while loading, updating, or using configured pairing bonds.
#[derive(Debug)]
pub enum DeviceKeyStoreError {
    Store(KeyStoreError),
    NoConnection,
    BondNotFound {
        peer_address: Address,
    },
    NoLongTermKey {
        peer_address: Address,
    },
    InvalidKeyLength {
        field: &'static str,
        expected: usize,
        actual: usize,
    },
    NotCentral {
        connection_handle: u16,
    },
}

impl std::fmt::Display for DeviceKeyStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(f, "key store error: {error}"),
            Self::NoConnection => write!(f, "no active LE connection"),
            Self::BondNotFound { peer_address } => {
                write!(f, "no bond found for {peer_address}")
            }
            Self::NoLongTermKey { peer_address } => {
                write!(f, "bond for {peer_address} has no usable LTK")
            }
            Self::InvalidKeyLength {
                field,
                expected,
                actual,
            } => {
                write!(f, "{field} must contain {expected} bytes, got {actual}")
            }
            Self::NotCentral { connection_handle } => write!(
                f,
                "LE connection 0x{connection_handle:04X} is not locally central"
            ),
        }
    }
}

impl std::error::Error for DeviceKeyStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            _ => None,
        }
    }
}

impl From<KeyStoreError> for DeviceKeyStoreError {
    fn from(error: KeyStoreError) -> Self {
        Self::Store(error)
    }
}

fn random_static_address() -> Address {
    let random = bumble_crypto::random_128();
    let mut bytes: [u8; 6] = random[..6].try_into().expect("six-byte slice");
    bytes[5] |= 0xC0;
    Address::from_bytes(bytes, bumble::AddressType::RANDOM_DEVICE)
}

fn padded_local_name(name: &str) -> Result<[u8; 248], DevicePowerError> {
    let bytes = name.as_bytes();
    if bytes.len() > 248 {
        return Err(DevicePowerError::LocalNameTooLong {
            actual: bytes.len(),
            maximum: 248,
        });
    }
    let mut local_name = [0; 248];
    local_name[..bytes.len()].copy_from_slice(bytes);
    Ok(local_name)
}

fn default_inquiry_response(name: &str) -> Result<[u8; 240], DevicePowerError> {
    let bytes = name.as_bytes();
    if bytes.len() > 238 {
        return Err(DevicePowerError::LocalNameTooLong {
            actual: bytes.len(),
            maximum: 238,
        });
    }
    let mut response = [0; 240];
    response[0] = (bytes.len() + 1) as u8;
    response[1] = 0x09;
    response[2..2 + bytes.len()].copy_from_slice(bytes);
    Ok(response)
}

/// Stable server context identity for a connection's fixed ATT bearer.
pub const fn att_bearer_id(connection_handle: u16) -> u64 {
    connection_handle as u64
}

/// Stable server context identity for an Enhanced ATT bearer.
pub const fn eatt_bearer_id(connection_handle: u16, source_cid: u16) -> u64 {
    (1u64 << 63) | ((connection_handle as u64) << 16) | source_cid as u64
}

/// Transport-neutral HCI link used by [`Device`].
///
/// [`bumble_controller::LocalLink`] implements this interface for deterministic
/// in-process tests and simulations. External-controller adapters can implement
/// the same operations by writing HCI packets and returning packets read from a
/// real transport.
pub trait HostTransport {
    fn handle_command(&mut self, controller_id: usize, command: Command);

    fn send_acl_packet(&mut self, controller_id: usize, packet: AclDataPacket) -> bool;

    fn send_synchronous_data(
        &mut self,
        controller_id: usize,
        connection_handle: u16,
        packet_status: u8,
        data: &[u8],
    ) -> bool;

    fn send_iso_packet(&mut self, controller_id: usize, packet: IsoDataPacket) -> bool;

    fn disconnect(&mut self, controller_id: usize, connection_handle: u16, reason: u8) -> bool {
        self.handle_command(
            controller_id,
            Command::Disconnect {
                connection_handle,
                reason,
            },
        );
        true
    }

    fn drain_host_events(&mut self, controller_id: usize) -> Vec<HciPacket>;

    /// Advance any in-process connection setup. External controllers progress
    /// independently, so their implementation may leave this as a no-op.
    fn establish_connections(&mut self) {}

    /// Advance pending LE link-layer control procedures when applicable.
    fn pump_ll(&mut self) {}

    /// Advance pending Classic baseband procedures when applicable.
    fn pump_classic(&mut self) {}

    /// Advance pending periodic sync transfers when applicable.
    fn pump_periodic_sync_transfers(&mut self) {}

    /// Advance broadcast-group termination notifications when applicable.
    fn pump_big_terminations(&mut self) {}
}

impl HostTransport for ControllerLocalLink {
    fn handle_command(&mut self, controller_id: usize, command: Command) {
        ControllerLocalLink::handle_command(self, controller_id, command);
    }

    fn send_acl_packet(&mut self, controller_id: usize, packet: AclDataPacket) -> bool {
        ControllerLocalLink::send_acl_packet(self, controller_id, packet)
    }

    fn send_synchronous_data(
        &mut self,
        controller_id: usize,
        connection_handle: u16,
        packet_status: u8,
        data: &[u8],
    ) -> bool {
        ControllerLocalLink::send_synchronous_data(
            self,
            controller_id,
            connection_handle,
            packet_status,
            data,
        )
    }

    fn send_iso_packet(&mut self, controller_id: usize, packet: IsoDataPacket) -> bool {
        ControllerLocalLink::send_iso_packet(self, controller_id, packet)
    }

    fn disconnect(&mut self, controller_id: usize, connection_handle: u16, reason: u8) -> bool {
        ControllerLocalLink::disconnect(self, controller_id, connection_handle, reason)
    }

    fn drain_host_events(&mut self, controller_id: usize) -> Vec<HciPacket> {
        ControllerLocalLink::drain_host_events(self, controller_id)
    }

    fn establish_connections(&mut self) {
        ControllerLocalLink::establish_connections(self);
    }

    fn pump_ll(&mut self) {
        ControllerLocalLink::pump_ll(self);
    }

    fn pump_classic(&mut self) {
        ControllerLocalLink::pump_classic(self);
    }

    fn pump_periodic_sync_transfers(&mut self) {
        ControllerLocalLink::pump_periodic_sync_transfers(self);
    }

    fn pump_big_terminations(&mut self) {
        ControllerLocalLink::pump_big_terminations(self);
    }
}

/// Dynamically dispatched host link accepted by [`Device`] operations.
pub type LocalLink = dyn HostTransport;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SynchronousConnectionInfo {
    pub connection_handle: u16,
    pub peer_address: Address,
    pub link_type: u8,
    pub air_mode: u8,
}

/// Current HCI connection parameters for one established LE ACL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeConnectionParameters {
    pub connection_interval: u16,
    pub peripheral_latency: u16,
    pub supervision_timeout: u16,
    pub subrate_factor: u16,
    pub continuation_number: u16,
}

/// Requested bounds for the legacy LE Connection Update procedure, expressed
/// in HCI units (1.25 ms intervals, 10 ms supervision timeout, and 0.625 ms
/// connection-event lengths).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeConnectionUpdateParameters {
    pub connection_interval_min: u16,
    pub connection_interval_max: u16,
    pub max_latency: u16,
    pub supervision_timeout: u16,
    pub min_ce_length: u16,
    pub max_ce_length: u16,
}

/// Bluetooth 6.2 connection-rate request, expressed in the command's native
/// HCI units (0.125 ms intervals/event lengths and 10 ms timeout units).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeConnectionRateParameters {
    pub connection_interval_min: u16,
    pub connection_interval_max: u16,
    pub subrate_min: u16,
    pub subrate_max: u16,
    pub max_latency: u16,
    pub continuation_number: u16,
    pub supervision_timeout: u16,
    pub min_ce_length: u16,
    pub max_ce_length: u16,
}

/// Negotiated LE data-length values reported by the controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeDataLength {
    pub max_tx_octets: u16,
    pub max_tx_time: u16,
    pub max_rx_octets: u16,
    pub max_rx_time: u16,
}

/// Current transmit and receive LE PHY identifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LePhy {
    pub tx_phy: u8,
    pub rx_phy: u8,
}

/// Completion journal for the upstream LE connection-control conveniences.
///
/// Python Bumble resolves futures and emits connection events. The synchronous
/// Rust host retains the same result information in order for callers to drain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeConnectionControlEvent {
    ConnectionParametersUpdate {
        status: u8,
        connection_handle: u16,
        parameters: LeConnectionParameters,
    },
    DataLengthRequestComplete {
        status: u8,
        connection_handle: u16,
    },
    DataLengthChange {
        connection_handle: u16,
        data_length: LeDataLength,
    },
    PhyRead {
        status: u8,
        connection_handle: u16,
        phy: LePhy,
    },
    PhyUpdate {
        status: u8,
        connection_handle: u16,
        phy: LePhy,
    },
    RssiRead {
        status: u8,
        connection_handle: u16,
        rssi: i8,
    },
    CommandStatus {
        command_opcode: u16,
        status: u8,
        connection_handle: Option<u16>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeSubrateRequestParameters {
    pub subrate_min: u16,
    pub subrate_max: u16,
    pub max_latency: u16,
    pub continuation_number: u16,
    pub supervision_timeout: u16,
}

/// The upstream-safe default CS channel map. Bluetooth channels 0, 1,
/// 23-25, and 76-79 are deliberately disabled.
pub const DEFAULT_CHANNEL_SOUNDING_CHANNEL_MAP: [u8; 10] =
    [0x54, 0x55, 0x55, 0x54, 0x55, 0x55, 0x55, 0x55, 0x55, 0x05];

pub const MIN_CHANNEL_SOUNDING_CONFIG_ID: u8 = 0;
pub const MAX_CHANNEL_SOUNDING_CONFIG_ID: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelSoundingCapabilities {
    pub num_config_supported: u8,
    pub max_consecutive_procedures_supported: u16,
    pub num_antennas_supported: u8,
    pub max_antenna_paths_supported: u8,
    pub roles_supported: u8,
    pub modes_supported: u8,
    pub rtt_capability: u8,
    pub rtt_aa_only_n: u8,
    pub rtt_sounding_n: u8,
    pub rtt_random_sequence_n: u8,
    pub nadm_sounding_capability: u16,
    pub nadm_random_capability: u16,
    pub cs_sync_phys_supported: u8,
    pub subfeatures_supported: u16,
    pub t_ip1_times_supported: u16,
    pub t_ip2_times_supported: u16,
    pub t_fcs_times_supported: u16,
    pub t_pm_times_supported: u16,
    pub t_sw_time_supported: u8,
    pub tx_snr_capability: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelSoundingConfig {
    pub config_id: u8,
    pub main_mode_type: u8,
    pub sub_mode_type: u8,
    pub min_main_mode_steps: u8,
    pub max_main_mode_steps: u8,
    pub main_mode_repetition: u8,
    pub mode_0_steps: u8,
    pub role: u8,
    pub rtt_type: u8,
    pub cs_sync_phy: u8,
    pub channel_map: [u8; 10],
    pub channel_map_repetition: u8,
    pub channel_selection_type: u8,
    pub ch3c_shape: u8,
    pub ch3c_jump: u8,
    pub reserved: u8,
    pub t_ip1_time: u8,
    pub t_ip2_time: u8,
    pub t_fcs_time: u8,
    pub t_pm_time: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelSoundingProcedure {
    pub config_id: u8,
    pub state: u8,
    pub tone_antenna_config_selection: u8,
    pub selected_tx_power: i8,
    pub subevent_len: u32,
    pub subevents_per_event: u8,
    pub subevent_interval: u16,
    pub event_interval: u16,
    pub procedure_interval: u16,
    pub procedure_count: u16,
    pub max_procedure_len: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelSoundingDefaultSettings {
    pub role_enable: u8,
    pub cs_sync_antenna_selection: u8,
    pub max_tx_power: u8,
}

impl Default for ChannelSoundingDefaultSettings {
    fn default() -> Self {
        Self {
            role_enable: 0x03,
            cs_sync_antenna_selection: 0xFF,
            max_tx_power: 0x04,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelSoundingCreateConfigParameters {
    pub create_context: u8,
    pub main_mode_type: u8,
    pub sub_mode_type: u8,
    pub min_main_mode_steps: u8,
    pub max_main_mode_steps: u8,
    pub main_mode_repetition: u8,
    pub mode_0_steps: u8,
    pub role: u8,
    pub rtt_type: u8,
    pub cs_sync_phy: u8,
    pub channel_map: [u8; 10],
    pub channel_map_repetition: u8,
    pub channel_selection_type: u8,
    pub ch3c_shape: u8,
    pub ch3c_jump: u8,
}

impl Default for ChannelSoundingCreateConfigParameters {
    fn default() -> Self {
        Self {
            create_context: 0x01,
            main_mode_type: 0x02,
            sub_mode_type: 0xFF,
            min_main_mode_steps: 0x02,
            max_main_mode_steps: 0x05,
            main_mode_repetition: 0x00,
            mode_0_steps: 0x03,
            role: 0x00,
            rtt_type: 0x00,
            cs_sync_phy: 0x01,
            channel_map: DEFAULT_CHANNEL_SOUNDING_CHANNEL_MAP,
            channel_map_repetition: 0x01,
            channel_selection_type: 0x00,
            ch3c_shape: 0x00,
            ch3c_jump: 0x03,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelSoundingProcedureParameters {
    pub tone_antenna_config_selection: u8,
    pub preferred_peer_antenna: u8,
    pub max_procedure_len: u16,
    pub min_procedure_interval: u16,
    pub max_procedure_interval: u16,
    pub max_procedure_count: u16,
    pub min_subevent_len: u32,
    pub max_subevent_len: u32,
    pub phy: u8,
    pub tx_power_delta: u8,
    pub snr_control_initiator: u8,
    pub snr_control_reflector: u8,
}

impl Default for ChannelSoundingProcedureParameters {
    fn default() -> Self {
        Self {
            tone_antenna_config_selection: 0x00,
            preferred_peer_antenna: 0x00,
            max_procedure_len: 0x2710,
            min_procedure_interval: 0x01,
            max_procedure_interval: 0xFF,
            max_procedure_count: 0x01,
            min_subevent_len: 0x0004E2,
            max_subevent_len: 0x1E8480,
            phy: 0x01,
            tx_power_delta: 0x00,
            snr_control_initiator: 0xFF,
            snr_control_reflector: 0xFF,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelSoundingOperation {
    ReadRemoteCapabilities,
    SecurityEnable,
    Config,
    ProcedureEnable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelSoundingError {
    pub operation: ChannelSoundingOperation,
    pub connection_handle: u16,
    pub config_id: Option<u8>,
    pub status: u8,
}

/// One complete Channel Sounding subevent result emitted by the controller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelSoundingSubeventResult {
    pub connection_handle: u16,
    pub config_id: u8,
    pub start_acl_conn_event_counter: u16,
    pub procedure_counter: u16,
    pub frequency_compensation: u16,
    pub reference_power_level: i8,
    pub procedure_done_status: u8,
    pub subevent_done_status: u8,
    pub abort_reason: u8,
    pub num_antenna_paths: u8,
    pub step_mode: Vec<u8>,
    pub step_channel: Vec<u8>,
    pub step_data: Vec<Vec<u8>>,
}

/// Continuation fragment for a Channel Sounding subevent result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelSoundingSubeventResultContinue {
    pub connection_handle: u16,
    pub config_id: u8,
    pub procedure_done_status: u8,
    pub subevent_done_status: u8,
    pub abort_reason: u8,
    pub num_antenna_paths: u8,
    pub step_mode: Vec<u8>,
    pub step_channel: Vec<u8>,
    pub step_data: Vec<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionFeatureTransport {
    Le,
    Classic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectionFeatureError {
    pub transport: ConnectionFeatureTransport,
    pub connection_handle: u16,
    pub page_number: Option<u8>,
    pub status: u8,
}

/// Controller version information learned during the upstream Host reset flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalVersionInformation {
    pub hci_version: u8,
    pub hci_subversion: u16,
    pub lmp_version: u8,
    pub company_identifier: u16,
    pub lmp_subversion: u16,
}

/// One controller-owned HCI packet pool learned during Host reset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControllerBufferInfo {
    pub data_packet_length: u16,
    pub total_num_data_packets: u16,
}

/// Controller suggestion used as the default LE connection data length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeSuggestedDefaultDataLength {
    pub suggested_max_tx_octets: u16,
    pub suggested_max_tx_time: u16,
}

/// Host-owned metadata for one established LE ACL connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeConnectionInfo {
    pub connection_handle: u16,
    pub role: u8,
    pub peer_address: Address,
    pub parameters: LeConnectionParameters,
    pub data_length: Option<LeDataLength>,
    pub phy: LePhy,
    pub rssi: Option<i8>,
    pub encryption_enabled: u8,
    pub encryption_key_size: u8,
    pub qos_service_type: Option<u8>,
    pub classic_mode: u8,
    pub classic_interval: u16,
    pub peer_le_features: Option<[u8; 8]>,
    pub channel_sounding_capabilities: Option<ChannelSoundingCapabilities>,
    pub channel_sounding_configs: BTreeMap<u8, ChannelSoundingConfig>,
    pub channel_sounding_procedures: BTreeMap<u8, ChannelSoundingProcedure>,
}

/// Controller request for the key needed to complete LE link encryption.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LongTermKeyRequestInfo {
    pub connection_handle: u16,
    pub random_number: [u8; 8],
    pub encryption_diversifier: u16,
}

/// Host-owned metadata for one established Classic ACL connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassicConnectionInfo {
    pub connection_handle: u16,
    pub role: u8,
    pub peer_address: Address,
    pub peer_name: Option<String>,
    pub encryption_enabled: u8,
    pub encryption_key_size: u8,
    pub qos_service_type: Option<u8>,
    pub classic_mode: u8,
    pub classic_interval: u16,
    pub peer_lmp_features: BTreeMap<u8, [u8; 8]>,
    pub peer_lmp_max_page_number: Option<u8>,
    pub peer_host_supported_features: Option<[u8; 8]>,
}

/// Why a Classic remote-name request did not produce a UTF-8 peer name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteNameError {
    HciStatus(u8),
    InvalidUtf8 {
        valid_up_to: usize,
        error_len: Option<usize>,
    },
}

/// Completion journal entry for one Classic remote-name request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteNameResult {
    pub peer_address: Address,
    pub result: Result<String, RemoteNameError>,
}

/// One Classic inquiry report, retaining the discovery metadata applications
/// use to identify audio devices and render Extended Inquiry Response data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassicInquiryResultInfo {
    pub peer_address: Address,
    pub class_of_device: u32,
    pub rssi: Option<i8>,
    pub extended_inquiry_response: Vec<u8>,
}

/// High-level legacy or extended advertising result.
///
/// The flag interpretation and default radio values match upstream Bumble's
/// `Advertisement`, `LegacyAdvertisement`, and `ExtendedAdvertisement` models.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Advertisement {
    pub address: Address,
    pub rssi: i8,
    pub is_legacy: bool,
    pub is_anonymous: bool,
    pub is_connectable: bool,
    pub is_directed: bool,
    pub is_scannable: bool,
    pub is_scan_response: bool,
    pub is_complete: bool,
    pub is_truncated: bool,
    pub primary_phy: u8,
    pub secondary_phy: u8,
    pub tx_power: i8,
    pub sid: u8,
    pub data_bytes: Vec<u8>,
    pub data: AdvertisingData,
}

impl Advertisement {
    pub const TX_POWER_NOT_AVAILABLE: i8 = 0x7F;
    pub const RSSI_NOT_AVAILABLE: i8 = 0x7F;

    pub fn from_legacy_report(report: &AdvertisingReport) -> Self {
        const ADV_IND: u8 = 0x00;
        const ADV_DIRECT_IND: u8 = 0x01;
        const ADV_SCAN_IND: u8 = 0x02;
        const SCAN_RSP: u8 = 0x04;

        let data_bytes = report.data.clone();
        Self {
            address: report.address.clone(),
            rssi: report.rssi,
            is_legacy: true,
            is_anonymous: false,
            is_connectable: matches!(report.event_type, ADV_IND | ADV_DIRECT_IND),
            is_directed: report.event_type == ADV_DIRECT_IND,
            is_scannable: matches!(report.event_type, ADV_IND | ADV_SCAN_IND),
            is_scan_response: report.event_type == SCAN_RSP,
            is_complete: true,
            is_truncated: false,
            primary_phy: 0,
            secondary_phy: 0,
            tx_power: Self::TX_POWER_NOT_AVAILABLE,
            sid: 0,
            data: AdvertisingData::from_bytes(&data_bytes),
            data_bytes,
        }
    }

    pub fn from_extended_report(report: &ExtendedAdvertisingReport) -> Self {
        const CONNECTABLE: u16 = 1 << 0;
        const SCANNABLE: u16 = 1 << 1;
        const DIRECTED: u16 = 1 << 2;
        const SCAN_RESPONSE: u16 = 1 << 3;
        const LEGACY_PDU: u16 = 1 << 4;
        const DATA_COMPLETE: u16 = 0;
        const DATA_TRUNCATED: u16 = 2;
        const ANONYMOUS_ADDRESS_TYPE: u8 = 0xFF;

        let data_bytes = report.data.clone();
        let data_status = (report.event_type >> 5) & 0x03;
        Self {
            address: report.address.clone(),
            rssi: report.rssi,
            is_legacy: report.event_type & LEGACY_PDU != 0,
            is_anonymous: report.address_type == ANONYMOUS_ADDRESS_TYPE,
            is_connectable: report.event_type & CONNECTABLE != 0,
            is_directed: report.event_type & DIRECTED != 0,
            is_scannable: report.event_type & SCANNABLE != 0,
            is_scan_response: report.event_type & SCAN_RESPONSE != 0,
            is_complete: data_status == DATA_COMPLETE,
            is_truncated: data_status == DATA_TRUNCATED,
            primary_phy: report.primary_phy,
            secondary_phy: report.secondary_phy,
            tx_power: report.tx_power,
            sid: report.advertising_sid,
            data: AdvertisingData::from_bytes(&data_bytes),
            data_bytes,
        }
    }
}

/// Per-advertiser active/passive scan accumulator.
#[derive(Clone, Debug)]
pub struct AdvertisementDataAccumulator {
    pub last_advertisement: Option<Advertisement>,
    pub last_data: Vec<u8>,
    pub passive: bool,
}

impl AdvertisementDataAccumulator {
    pub fn new(passive: bool) -> Self {
        Self {
            last_advertisement: None,
            last_data: Vec::new(),
            passive,
        }
    }

    pub fn update_legacy(&mut self, report: &AdvertisingReport) -> Option<Advertisement> {
        self.update(Advertisement::from_legacy_report(report))
    }

    pub fn update_extended(&mut self, report: &ExtendedAdvertisingReport) -> Option<Advertisement> {
        self.update(Advertisement::from_extended_report(report))
    }

    fn update(&mut self, advertisement: Advertisement) -> Option<Advertisement> {
        let mut result = None;
        if advertisement.is_scan_response {
            if let Some(previous) = self
                .last_advertisement
                .as_ref()
                .filter(|previous| !previous.is_scan_response)
            {
                let mut combined = advertisement.clone();
                combined.is_connectable = previous.is_connectable;
                combined.is_scannable = true;
                let mut data = self.last_data.clone();
                data.extend_from_slice(&advertisement.data_bytes);
                combined.data = AdvertisingData::from_bytes(&data);
                result = Some(combined);
            }
            self.last_data.clear();
        } else {
            if self.passive
                || !advertisement.is_scannable
                || self
                    .last_advertisement
                    .as_ref()
                    .is_some_and(|previous| !previous.is_scan_response)
            {
                result = Some(advertisement.clone());
            }
            self.last_data.clone_from(&advertisement.data_bytes);
        }
        self.last_advertisement = Some(advertisement);
        result
    }
}

/// Physical link family associated with a connection lifecycle event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceConnectionTransport {
    Le,
    Classic,
    Synchronous { link_type: u8 },
}

/// Radio family used by a peer-name lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerLookupTransport {
    Le,
    Classic,
}

/// Stable identifier for one pending peer lookup.
pub type PeerLookupId = u64;

/// Completed peer lookup retained by the synchronous result journal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerLookupResult {
    pub lookup_id: PeerLookupId,
    pub transport: PeerLookupTransport,
    pub peer_address: Address,
}

/// Failures that prevent a peer lookup from starting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerLookupError {
    NoAddressResolver,
}

impl std::fmt::Display for PeerLookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAddressResolver => write!(f, "device has no address resolver"),
        }
    }
}

impl std::error::Error for PeerLookupError {}

#[derive(Clone, Debug)]
enum PeerLookupRequest {
    Name {
        name: String,
        transport: PeerLookupTransport,
    },
    Identity {
        identity_address: Address,
    },
}

impl PeerLookupRequest {
    fn transport(&self) -> PeerLookupTransport {
        match self {
            Self::Name { transport, .. } => *transport,
            Self::Identity { .. } => PeerLookupTransport::Le,
        }
    }
}

/// Typed high-level events emitted by [`Device`].
///
/// This is the synchronous Rust counterpart to upstream Bumble's device and
/// connection event emitters. Events are retained in a drainable journal and
/// delivered immediately to registered listeners after the corresponding
/// host state has been updated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceEvent {
    Flush,
    LeConnectionEstablished(LeConnectionInfo),
    ClassicConnectionEstablished(ClassicConnectionInfo),
    SynchronousConnectionEstablished(SynchronousConnectionInfo),
    ConnectionFailed {
        transport: DeviceConnectionTransport,
        peer_address: Address,
        status: u8,
    },
    Disconnected {
        connection_handle: u16,
        reason: u8,
    },
    DisconnectionFailed {
        connection_handle: u16,
        status: u8,
    },
    ConnectionRequest {
        peer_address: Address,
        class_of_device: u32,
        link_type: u8,
    },
    Advertisement(Advertisement),
    AdvertisingReport(AdvertisingReport),
    ExtendedAdvertisingReport(ExtendedAdvertisingReport),
    InquiryResult(ClassicInquiryResultInfo),
    InquiryComplete {
        status: u8,
    },
    PeerFound(PeerLookupResult),
    RemoteName {
        status: u8,
        peer_address: Address,
        name: String,
    },
    RemoteNameFailure {
        peer_address: Address,
        error: RemoteNameError,
    },
    LeConnectionControl(LeConnectionControlEvent),
    ClassicPairing(ClassicPairingEvent),
    PairingComplete {
        connection_handle: u16,
        keys: Box<PairingKeys>,
    },
    PairingFailed {
        connection_handle: u16,
        reason: PairingFailureReason,
    },
    KeyStoreUpdated,
    EncryptionChange {
        status: u8,
        connection_handle: u16,
        encryption_enabled: u8,
        encryption_key_size: u8,
    },
    EncryptionKeyRefresh {
        connection_handle: u16,
    },
    EncryptionKeyRefreshFailed {
        connection_handle: u16,
        status: u8,
    },
    QosSetup {
        connection_handle: u16,
        service_type: u8,
    },
    QosSetupFailed {
        connection_handle: u16,
        status: u8,
    },
    RemoteHostSupportedFeatures {
        peer_address: Address,
        host_supported_features: [u8; 8],
    },
    ChannelSoundingSubeventResult(ChannelSoundingSubeventResult),
    ChannelSoundingSubeventResultContinue(ChannelSoundingSubeventResultContinue),
    VendorEvent(Vec<u8>),
}

/// Stable identifier returned when registering a [`DeviceEvent`] listener.
pub type DeviceEventListenerId = u64;

type DeviceEventListener = Box<dyn FnMut(&DeviceEvent) + Send + 'static>;

/// Host-facing events that participate in Classic PIN or Secure Simple
/// Pairing authentication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClassicPairingEvent {
    AuthenticationComplete {
        status: u8,
        connection_handle: u16,
    },
    PinCodeRequest {
        peer_address: Address,
    },
    LinkKeyRequest {
        peer_address: Address,
    },
    LinkKeyNotification {
        peer_address: Address,
        link_key: [u8; 16],
        key_type: u8,
    },
    IoCapabilityRequest {
        peer_address: Address,
    },
    IoCapabilityResponse {
        peer_address: Address,
        io_capability: u8,
        authentication_requirements: u8,
    },
    UserConfirmationRequest {
        peer_address: Address,
        numeric_value: u32,
    },
    UserPasskeyRequest {
        peer_address: Address,
    },
    RemoteOobDataRequest {
        peer_address: Address,
    },
    SimplePairingComplete {
        status: u8,
        peer_address: Address,
    },
    UserPasskeyNotification {
        peer_address: Address,
        passkey: u32,
    },
}

impl ClassicPairingEvent {
    fn belongs_to(&self, connection_handle: u16, peer_address: &Address) -> bool {
        match self {
            Self::AuthenticationComplete {
                connection_handle: event_handle,
                ..
            } => *event_handle == connection_handle,
            Self::PinCodeRequest {
                peer_address: event_peer,
            }
            | Self::LinkKeyRequest {
                peer_address: event_peer,
            }
            | Self::LinkKeyNotification {
                peer_address: event_peer,
                ..
            }
            | Self::IoCapabilityRequest {
                peer_address: event_peer,
            }
            | Self::IoCapabilityResponse {
                peer_address: event_peer,
                ..
            }
            | Self::UserConfirmationRequest {
                peer_address: event_peer,
                ..
            }
            | Self::UserPasskeyRequest {
                peer_address: event_peer,
            }
            | Self::RemoteOobDataRequest {
                peer_address: event_peer,
            }
            | Self::SimplePairingComplete {
                peer_address: event_peer,
                ..
            }
            | Self::UserPasskeyNotification {
                peer_address: event_peer,
                ..
            } => event_peer == peer_address,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CisRequestInfo {
    pub acl_connection_handle: u16,
    pub cis_connection_handle: u16,
    pub cig_id: u8,
    pub cis_id: u8,
}

/// Upstream defaults for one Connected Isochronous Stream.
pub const DEFAULT_ISO_CIS_MAX_SDU: u16 = 251;
pub const DEFAULT_ISO_CIS_RTN: u8 = 10;
pub const DEFAULT_ISO_CIS_MAX_TRANSPORT_LATENCY: u16 = 100;

/// Directional parameters for one CIS in a Connected Isochronous Group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CisParameters {
    pub cis_id: u8,
    pub max_sdu_c_to_p: u16,
    pub max_sdu_p_to_c: u16,
    pub phy_c_to_p: u8,
    pub phy_p_to_c: u8,
    pub rtn_c_to_p: u8,
    pub rtn_p_to_c: u8,
}

impl CisParameters {
    /// Construct one bidirectional CIS using Bumble's defaults.
    pub const fn new(cis_id: u8) -> Self {
        Self {
            cis_id,
            max_sdu_c_to_p: DEFAULT_ISO_CIS_MAX_SDU,
            max_sdu_p_to_c: DEFAULT_ISO_CIS_MAX_SDU,
            phy_c_to_p: 0x02,
            phy_p_to_c: 0x02,
            rtn_c_to_p: DEFAULT_ISO_CIS_RTN,
            rtn_p_to_c: DEFAULT_ISO_CIS_RTN,
        }
    }

    /// Apply upstream's unidirectional retransmission normalization.
    ///
    /// A zero-sized direction forces RTN to zero for compatibility with older
    /// controller firmware, while its configured PHY remains valid. Python
    /// Bumble performs this in `__post_init__`; Rust applies it again when the
    /// containing CIG is serialized so later field updates cannot reintroduce
    /// a nonzero RTN.
    pub const fn normalized(mut self) -> Self {
        if self.max_sdu_c_to_p == 0 {
            self.rtn_c_to_p = 0;
        }
        if self.max_sdu_p_to_c == 0 {
            self.rtn_p_to_c = 0;
        }
        self
    }
}

/// Complete parameters for `LE Set CIG Parameters`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CigParameters {
    pub cig_id: u8,
    pub cis_parameters: Vec<CisParameters>,
    /// Central-to-peripheral SDU interval, in microseconds.
    pub sdu_interval_c_to_p: u32,
    /// Peripheral-to-central SDU interval, in microseconds.
    pub sdu_interval_p_to_c: u32,
    pub worst_case_sca: u8,
    pub packing: u8,
    pub framing: u8,
    /// Central-to-peripheral maximum transport latency, in milliseconds.
    pub max_transport_latency_c_to_p: u16,
    /// Peripheral-to-central maximum transport latency, in milliseconds.
    pub max_transport_latency_p_to_c: u16,
}

impl CigParameters {
    pub fn new(
        cig_id: u8,
        cis_parameters: Vec<CisParameters>,
        sdu_interval_c_to_p: u32,
        sdu_interval_p_to_c: u32,
    ) -> Self {
        Self {
            cig_id,
            cis_parameters,
            sdu_interval_c_to_p,
            sdu_interval_p_to_c,
            worst_case_sca: 0x00,
            packing: 0x00,
            framing: 0x00,
            max_transport_latency_c_to_p: DEFAULT_ISO_CIS_MAX_TRANSPORT_LATENCY,
            max_transport_latency_p_to_c: DEFAULT_ISO_CIS_MAX_TRANSPORT_LATENCY,
        }
    }
}

/// Timing and transport state reported by `LE CIS Established`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CisLinkInfo {
    pub connection_handle: u16,
    pub cig_sync_delay: u32,
    pub cis_sync_delay: u32,
    pub transport_latency_c_to_p: u32,
    pub transport_latency_p_to_c: u32,
    pub phy_c_to_p: u8,
    pub phy_p_to_c: u8,
    pub nse: u8,
    pub bn_c_to_p: u8,
    pub bn_p_to_c: u8,
    pub ft_c_to_p: u8,
    pub ft_p_to_c: u8,
    pub max_pdu_c_to_p: u16,
    pub max_pdu_p_to_c: u16,
    pub iso_interval: u16,
}

/// Ordered completion journal for CIG/CIS commands and establishment events.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CisControlEvent {
    CigConfigured {
        status: u8,
        cig_id: u8,
        connection_handles: Vec<u16>,
    },
    CommandStatus {
        command_opcode: u16,
        status: u8,
    },
    Established {
        status: u8,
        link: CisLinkInfo,
    },
}

/// Complete parameters for `LE Setup ISO Data Path`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsoDataPathParameters {
    pub direction: u8,
    pub data_path_id: u8,
    pub codec_id: CodingFormat,
    /// Controller delay in microseconds, encoded as a 24-bit HCI value.
    pub controller_delay: u32,
    pub codec_configuration: Vec<u8>,
}

impl IsoDataPathParameters {
    /// Host-controller-interface data path with transparent codec framing.
    pub fn hci(direction: u8) -> Self {
        Self {
            direction,
            data_path_id: 0,
            codec_id: CodingFormat::TRANSPARENT,
            controller_delay: 0,
            codec_configuration: Vec::new(),
        }
    }
}

/// Successful `LE Read ISO TX Sync` result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsoTxSyncInfo {
    pub connection_handle: u16,
    pub packet_sequence_number: u16,
    pub tx_time_stamp: u32,
    pub time_offset: u32,
}

/// Ordered completion journal for ISO data-path and TX-sync commands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IsoControlEvent {
    DataPathSetup {
        status: u8,
        connection_handle: u16,
        parameters: IsoDataPathParameters,
    },
    DataPathRemoved {
        status: u8,
        connection_handle: u16,
        directions: u8,
    },
    TxSync {
        status: u8,
        connection_handle: u16,
        sync: Option<IsoTxSyncInfo>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsoSdu {
    pub connection_handle: u16,
    pub packet_sequence_number: u16,
    pub packet_status_flag: u8,
    pub data: Vec<u8>,
}

/// Parameters for creating a Broadcast Isochronous Group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigParameters {
    pub big_handle: u8,
    pub advertising_handle: u8,
    pub num_bis: u8,
    pub sdu_interval: u32,
    pub max_sdu: u16,
    pub max_transport_latency: u16,
    pub rtn: u8,
    pub phy: u8,
    pub packing: u8,
    pub framing: u8,
    pub broadcast_code: Option<[u8; 16]>,
}

impl BigParameters {
    pub fn new(big_handle: u8, advertising_handle: u8, num_bis: u8) -> Self {
        Self {
            big_handle,
            advertising_handle,
            num_bis,
            sdu_interval: 10_000,
            max_sdu: 120,
            max_transport_latency: 65,
            rtn: 4,
            phy: 2,
            packing: 0,
            framing: 0,
            broadcast_code: None,
        }
    }
}

/// Parameters for synchronizing to selected BIS indices in a remote BIG.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigSyncParameters {
    pub big_handle: u8,
    pub sync_handle: u16,
    pub bis: Vec<u8>,
    pub mse: u8,
    pub big_sync_timeout: u16,
    pub broadcast_code: Option<[u8; 16]>,
}

impl BigSyncParameters {
    pub fn new(big_handle: u8, sync_handle: u16, bis: Vec<u8>) -> Self {
        Self {
            big_handle,
            sync_handle,
            bis,
            mse: 0,
            big_sync_timeout: 0x4000,
            broadcast_code: None,
        }
    }
}

/// The controller's BIGInfo report associated with a periodic sync.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigInfoReport {
    pub sync_handle: u16,
    pub num_bis: u8,
    pub nse: u8,
    pub iso_interval: u16,
    pub bn: u8,
    pub pto: u8,
    pub irc: u8,
    pub max_pdu: u16,
    pub sdu_interval: u32,
    pub max_sdu: u16,
    pub phy: u8,
    pub framing: u8,
    pub encrypted: bool,
}

#[derive(Debug, Default)]
struct IsoSduAssembler {
    pending: Option<IsoSdu>,
    expected_length: usize,
}

impl IsoSduAssembler {
    fn push(&mut self, packet: IsoDataPacket) -> Option<IsoSdu> {
        match packet.pb_flag {
            0b00 | 0b10 => {
                let (Some(sequence), Some(length), Some(status)) = (
                    packet.packet_sequence_number,
                    packet.iso_sdu_length,
                    packet.packet_status_flag,
                ) else {
                    self.pending = None;
                    return None;
                };
                let sdu = IsoSdu {
                    connection_handle: packet.connection_handle,
                    packet_sequence_number: sequence,
                    packet_status_flag: status,
                    data: packet.iso_sdu_fragment,
                };
                if packet.pb_flag == 0b10 {
                    self.pending = None;
                    return (sdu.data.len() == usize::from(length)).then_some(sdu);
                }
                self.expected_length = usize::from(length);
                self.pending = Some(sdu);
                None
            }
            0b01 | 0b11 => {
                let pending = self.pending.as_mut()?;
                pending.data.extend_from_slice(&packet.iso_sdu_fragment);
                if pending.data.len() > self.expected_length {
                    self.pending = None;
                    return None;
                }
                if packet.pb_flag == 0b11 {
                    let complete = self.pending.take().expect("pending ISO SDU exists");
                    return (complete.data.len() == self.expected_length).then_some(complete);
                }
                None
            }
            _ => None,
        }
    }
}

/// Parameters for one high-level extended-advertising set. Values map directly
/// to `LE Set Extended Advertising Parameters`; data is supplied separately so
/// the host can fragment it to HCI-sized commands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtendedAdvertisingConfig {
    pub handle: u8,
    pub event_properties: u16,
    pub interval_min: u32,
    pub interval_max: u32,
    pub channel_map: u8,
    pub own_address_type: u8,
    pub peer_address_type: u8,
    pub peer_address: Address,
    pub filter_policy: u8,
    pub tx_power: i8,
    pub primary_phy: u8,
    pub secondary_max_skip: u8,
    pub secondary_phy: u8,
    pub sid: u8,
    pub scan_request_notification: bool,
    pub duration: u16,
    pub max_events: u8,
    pub random_address: Option<Address>,
}

impl ExtendedAdvertisingConfig {
    pub fn connectable_scannable(handle: u8, random_address: Address) -> Self {
        Self {
            handle,
            event_properties: 0x0003,
            interval_min: 0x20,
            interval_max: 0x40,
            channel_map: 7,
            own_address_type: 1,
            peer_address_type: 0,
            peer_address: Address::from_bytes([0; 6], bumble::AddressType::PUBLIC_DEVICE),
            filter_policy: 0,
            tx_power: 0,
            primary_phy: 1,
            secondary_max_skip: 0,
            secondary_phy: 1,
            sid: handle & 0x0F,
            scan_request_notification: false,
            duration: 0,
            max_events: 0,
            random_address: Some(random_address),
        }
    }
}

/// Parameters for a periodic advertising train attached to an extended set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeriodicAdvertisingConfig {
    pub handle: u8,
    pub interval_min: u16,
    pub interval_max: u16,
    pub properties: u16,
    pub include_adi: bool,
}

impl PeriodicAdvertisingConfig {
    pub fn new(handle: u8) -> Self {
        Self {
            handle,
            interval_min: 0x00A0,
            interval_max: 0x00A0,
            properties: 0,
            include_adi: false,
        }
    }
}

/// An established periodic advertising synchronization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeriodicAdvertisingSyncInfo {
    pub sync_handle: u16,
    pub advertising_sid: u8,
    pub advertiser_address_type: u8,
    pub advertiser_address: Address,
    pub advertiser_phy: u8,
    pub interval: u16,
    pub advertiser_clock_accuracy: u8,
}

/// Metadata accompanying a sync received through PAST over an LE connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeriodicAdvertisingSyncTransferInfo {
    pub connection_handle: u16,
    pub service_data: u16,
    pub sync: PeriodicAdvertisingSyncInfo,
}

/// One complete periodic advertisement after HCI report-fragment assembly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeriodicAdvertisement {
    pub sync_handle: u16,
    pub advertiser_address: Address,
    pub advertising_sid: u8,
    pub tx_power: i8,
    pub rssi: i8,
    pub truncated: bool,
    pub data: Vec<u8>,
}

/// A host attached to a controller through a [`HostTransport`]. Owns the
/// ATT↔L2CAP↔ACL sequencing.
pub struct Device {
    pub config: DeviceConfiguration,
    powered_on: bool,
    public_address: Option<Address>,
    static_address: Address,
    random_address: Address,
    local_supported_commands: Option<[u8; 64]>,
    local_supported_commands_status: Option<u8>,
    local_version: Option<LocalVersionInformation>,
    local_version_status: Option<u8>,
    local_lmp_features: BTreeMap<u8, [u8; 8]>,
    local_lmp_features_max_page: Option<u8>,
    local_lmp_feature_statuses: BTreeMap<u8, u8>,
    pending_local_lmp_feature_pages: VecDeque<u8>,
    local_le_features: Option<Vec<u8>>,
    local_le_features_max_page: Option<u8>,
    local_le_features_status: Option<u8>,
    host_initialization_started: bool,
    host_initialization_complete: bool,
    event_mask_status: Option<u8>,
    event_mask_page_2_status: Option<u8>,
    le_event_mask_status: Option<u8>,
    classic_buffer_status: Option<u8>,
    classic_acl_buffer: Option<ControllerBufferInfo>,
    le_buffer_status: Option<u8>,
    le_acl_buffer: Option<ControllerBufferInfo>,
    iso_buffer: Option<ControllerBufferInfo>,
    suggested_default_data_length_read_status: Option<u8>,
    suggested_default_data_length: Option<LeSuggestedDefaultDataLength>,
    suggested_default_data_length_write_required: bool,
    suggested_default_data_length_write_status: Option<u8>,
    number_of_supported_advertising_sets_status: Option<u8>,
    number_of_supported_advertising_sets: u8,
    maximum_advertising_data_length_status: Option<u8>,
    maximum_advertising_data_length: u16,
    local_channel_sounding_capabilities: Option<ChannelSoundingCapabilities>,
    local_channel_sounding_capabilities_status: Option<u8>,
    controller_id: usize,
    server: Option<Box<dyn AttRequestHandler>>,
    connection_handle: Option<u16>,
    connection_role: Option<u8>,
    peer_address: Option<Address>,
    le_connections: BTreeMap<u16, LeConnectionInfo>,
    le_credit_managers: BTreeMap<u16, LeCreditChannelManager>,
    le_credit_server_specs: BTreeMap<u16, LeCreditBasedChannelSpec>,
    le_credit_errors: Vec<(u16, String)>,
    eatt_inbox: Vec<(u16, u16, AttPdu)>,
    pending_att_indications: BTreeSet<(u16, u16)>,
    classic_connection_handle: Option<u16>,
    classic_connection_role: Option<u8>,
    classic_connections: BTreeMap<u16, ClassicConnectionInfo>,
    classic_link_keys: BTreeMap<u16, ([u8; 16], bool)>,
    classic_channel_managers: BTreeMap<u16, ClassicChannelManager>,
    classic_channel_server_specs: BTreeMap<u32, ClassicChannelSpec>,
    classic_channel_errors: Vec<(u16, String)>,
    classic_connection_requests: Vec<Address>,
    classic_inquiry_results: Vec<Address>,
    classic_inquiry_result_details: Vec<ClassicInquiryResultInfo>,
    classic_inquiry_complete: Vec<u8>,
    classic_inquiry_response: Option<[u8; 240]>,
    classic_discovering: bool,
    classic_auto_restart_inquiry: bool,
    classic_remote_names: Vec<(u8, Address, String)>,
    classic_remote_name_results: Vec<RemoteNameResult>,
    pending_remote_name_commands: VecDeque<Address>,
    pending_remote_name_requests: Vec<(Address, usize)>,
    classic_pairing_events: Vec<ClassicPairingEvent>,
    pending_classic_roles: Vec<(Address, u8)>,
    synchronous_connections: Vec<SynchronousConnectionInfo>,
    synchronous_requests: Vec<(Address, u8)>,
    synchronous_inbox: Vec<SynchronousDataPacket>,
    cis_requests: Vec<CisRequestInfo>,
    configured_cis_handles: Vec<u16>,
    cis_links: BTreeMap<u16, CisLinkInfo>,
    cis_control_events: Vec<CisControlEvent>,
    iso_data_paths: BTreeMap<(u16, u8), IsoDataPathParameters>,
    pending_iso_data_path_setups: VecDeque<(u16, IsoDataPathParameters)>,
    pending_iso_data_path_removals: VecDeque<(u16, u8)>,
    pending_iso_tx_syncs: VecDeque<u16>,
    iso_tx_syncs: BTreeMap<u16, IsoTxSyncInfo>,
    iso_control_events: Vec<IsoControlEvent>,
    bigs: BTreeMap<u8, Vec<u16>>,
    pending_bigs: BTreeSet<u8>,
    pending_big_commands: VecDeque<u8>,
    big_syncs: BTreeMap<u8, Vec<u16>>,
    pending_big_syncs: BTreeSet<u8>,
    pending_big_sync_commands: VecDeque<u8>,
    bis_directions: BTreeMap<u16, u8>,
    biginfo_reports: Vec<BigInfoReport>,
    big_errors: Vec<(u8, u8)>,
    terminated_bigs: Vec<(u8, u8)>,
    iso_sequence_numbers: BTreeMap<u16, u16>,
    iso_assemblers: BTreeMap<u16, IsoSduAssembler>,
    iso_inbox: Vec<IsoSdu>,
    inbox: Vec<(u16, AttPdu)>,
    /// Received payloads on non-ATT L2CAP channels, as `(handle, cid, payload)`.
    l2cap_inbox: Vec<(u16, u16, Vec<u8>)>,
    security_requests: Vec<(u16, u8)>,
    pairing_manager: Option<PairingManager>,
    pairing_encryption_started: BTreeSet<u16>,
    pairing_terminal_handles: BTreeSet<u16>,
    pairing_errors: Vec<(u16, String)>,
    key_store: Option<Box<dyn KeyStore>>,
    address_resolver: Option<AddressResolver>,
    key_store_errors: Vec<(Option<u16>, String)>,
    long_term_key_requests: Vec<LongTermKeyRequestInfo>,
    connection_feature_errors: Vec<ConnectionFeatureError>,
    connection_control_events: Vec<LeConnectionControlEvent>,
    pending_connection_controls: BTreeMap<u16, VecDeque<u16>>,
    pending_disconnections: BTreeSet<u16>,
    device_events: Vec<DeviceEvent>,
    event_listeners: BTreeMap<DeviceEventListenerId, DeviceEventListener>,
    next_event_listener_id: DeviceEventListenerId,
    pending_peer_lookups: BTreeMap<PeerLookupId, PeerLookupRequest>,
    peer_lookup_results: Vec<PeerLookupResult>,
    next_peer_lookup_id: PeerLookupId,
    peer_lookup_started_scanning: Option<bool>,
    peer_lookup_started_discovery: bool,
    pending_channel_sounding_configs: BTreeSet<(u16, u8)>,
    channel_sounding_errors: Vec<ChannelSoundingError>,
    channel_sounding_security_results: Vec<(u16, u8)>,
    channel_sounding_subevent_results: Vec<ChannelSoundingSubeventResult>,
    channel_sounding_subevent_result_continuations: Vec<ChannelSoundingSubeventResultContinue>,
    vendor_events: Vec<Vec<u8>>,
    advertising_reports: Vec<AdvertisingReport>,
    extended_advertising_reports: Vec<ExtendedAdvertisingReport>,
    advertisement_accumulators: BTreeMap<(u8, [u8; 6]), AdvertisementDataAccumulator>,
    advertisements: Vec<Advertisement>,
    scanning_is_passive: bool,
    scanning: bool,
    legacy_advertising: bool,
    extended_advertising_handles: BTreeSet<u8>,
    le_connecting: bool,
    rpa_timeout_elapsed_seconds: u64,
    periodic_syncs: BTreeMap<u16, PeriodicAdvertisingSyncInfo>,
    periodic_report_accumulators: BTreeMap<u16, Vec<u8>>,
    periodic_advertisements: Vec<PeriodicAdvertisement>,
    periodic_sync_errors: Vec<u8>,
    lost_periodic_syncs: Vec<u16>,
    periodic_sync_transfers: Vec<PeriodicAdvertisingSyncTransferInfo>,
    acl_data_packet_length: usize,
    acl_assemblers: BTreeMap<u16, AclDataPacketAssembler>,
    acl_packet_queue: DataPacketQueue<AclDataPacket>,
    le_acl_data_packet_length: Option<usize>,
    le_acl_packet_queue: Option<DataPacketQueue<AclDataPacket>>,
    iso_data_packet_length: Option<usize>,
    iso_packet_queue: Option<DataPacketQueue<IsoDataPacket>>,
    encrypted_handles: BTreeSet<u16>,
}

impl Device {
    /// A client-only device (no attribute server).
    pub fn new(controller_id: usize) -> Device {
        let config = DeviceConfiguration::default();
        let static_address = config.address.clone();
        Device {
            config,
            powered_on: false,
            public_address: None,
            random_address: static_address.clone(),
            static_address,
            local_supported_commands: None,
            local_supported_commands_status: None,
            local_version: None,
            local_version_status: None,
            local_lmp_features: BTreeMap::new(),
            local_lmp_features_max_page: None,
            local_lmp_feature_statuses: BTreeMap::new(),
            pending_local_lmp_feature_pages: VecDeque::new(),
            local_le_features: None,
            local_le_features_max_page: None,
            local_le_features_status: None,
            host_initialization_started: false,
            host_initialization_complete: false,
            event_mask_status: None,
            event_mask_page_2_status: None,
            le_event_mask_status: None,
            classic_buffer_status: None,
            classic_acl_buffer: None,
            le_buffer_status: None,
            le_acl_buffer: None,
            iso_buffer: None,
            suggested_default_data_length_read_status: None,
            suggested_default_data_length: None,
            suggested_default_data_length_write_required: false,
            suggested_default_data_length_write_status: None,
            number_of_supported_advertising_sets_status: None,
            number_of_supported_advertising_sets: 0,
            maximum_advertising_data_length_status: None,
            maximum_advertising_data_length: HOST_DEFAULT_MAXIMUM_ADVERTISING_DATA_LENGTH,
            local_channel_sounding_capabilities: None,
            local_channel_sounding_capabilities_status: None,
            controller_id,
            server: None,
            connection_handle: None,
            connection_role: None,
            peer_address: None,
            le_connections: BTreeMap::new(),
            le_credit_managers: BTreeMap::new(),
            le_credit_server_specs: BTreeMap::new(),
            le_credit_errors: Vec::new(),
            eatt_inbox: Vec::new(),
            pending_att_indications: BTreeSet::new(),
            classic_connection_handle: None,
            classic_connection_role: None,
            classic_connections: BTreeMap::new(),
            classic_link_keys: BTreeMap::new(),
            classic_channel_managers: BTreeMap::new(),
            classic_channel_server_specs: BTreeMap::new(),
            classic_channel_errors: Vec::new(),
            classic_connection_requests: Vec::new(),
            classic_inquiry_results: Vec::new(),
            classic_inquiry_result_details: Vec::new(),
            classic_inquiry_complete: Vec::new(),
            classic_inquiry_response: None,
            classic_discovering: false,
            classic_auto_restart_inquiry: true,
            classic_remote_names: Vec::new(),
            classic_remote_name_results: Vec::new(),
            pending_remote_name_commands: VecDeque::new(),
            pending_remote_name_requests: Vec::new(),
            classic_pairing_events: Vec::new(),
            pending_classic_roles: Vec::new(),
            synchronous_connections: Vec::new(),
            synchronous_requests: Vec::new(),
            synchronous_inbox: Vec::new(),
            cis_requests: Vec::new(),
            configured_cis_handles: Vec::new(),
            cis_links: BTreeMap::new(),
            cis_control_events: Vec::new(),
            iso_data_paths: BTreeMap::new(),
            pending_iso_data_path_setups: VecDeque::new(),
            pending_iso_data_path_removals: VecDeque::new(),
            pending_iso_tx_syncs: VecDeque::new(),
            iso_tx_syncs: BTreeMap::new(),
            iso_control_events: Vec::new(),
            bigs: BTreeMap::new(),
            pending_bigs: BTreeSet::new(),
            pending_big_commands: VecDeque::new(),
            big_syncs: BTreeMap::new(),
            pending_big_syncs: BTreeSet::new(),
            pending_big_sync_commands: VecDeque::new(),
            bis_directions: BTreeMap::new(),
            biginfo_reports: Vec::new(),
            big_errors: Vec::new(),
            terminated_bigs: Vec::new(),
            iso_sequence_numbers: BTreeMap::new(),
            iso_assemblers: BTreeMap::new(),
            iso_inbox: Vec::new(),
            inbox: Vec::new(),
            l2cap_inbox: Vec::new(),
            security_requests: Vec::new(),
            pairing_manager: None,
            pairing_encryption_started: BTreeSet::new(),
            pairing_terminal_handles: BTreeSet::new(),
            pairing_errors: Vec::new(),
            key_store: None,
            address_resolver: None,
            key_store_errors: Vec::new(),
            long_term_key_requests: Vec::new(),
            connection_feature_errors: Vec::new(),
            connection_control_events: Vec::new(),
            pending_connection_controls: BTreeMap::new(),
            pending_disconnections: BTreeSet::new(),
            device_events: Vec::new(),
            event_listeners: BTreeMap::new(),
            next_event_listener_id: 1,
            pending_peer_lookups: BTreeMap::new(),
            peer_lookup_results: Vec::new(),
            next_peer_lookup_id: 1,
            peer_lookup_started_scanning: None,
            peer_lookup_started_discovery: false,
            pending_channel_sounding_configs: BTreeSet::new(),
            channel_sounding_errors: Vec::new(),
            channel_sounding_security_results: Vec::new(),
            channel_sounding_subevent_results: Vec::new(),
            channel_sounding_subevent_result_continuations: Vec::new(),
            vendor_events: Vec::new(),
            advertising_reports: Vec::new(),
            extended_advertising_reports: Vec::new(),
            advertisement_accumulators: BTreeMap::new(),
            advertisements: Vec::new(),
            scanning_is_passive: false,
            scanning: false,
            legacy_advertising: false,
            extended_advertising_handles: BTreeSet::new(),
            le_connecting: false,
            rpa_timeout_elapsed_seconds: 0,
            periodic_syncs: BTreeMap::new(),
            periodic_report_accumulators: BTreeMap::new(),
            periodic_advertisements: Vec::new(),
            periodic_sync_errors: Vec::new(),
            lost_periodic_syncs: Vec::new(),
            periodic_sync_transfers: Vec::new(),
            acl_data_packet_length: 27,
            acl_assemblers: BTreeMap::new(),
            acl_packet_queue: DataPacketQueue::new(64).expect("nonzero ACL queue capacity"),
            le_acl_data_packet_length: None,
            le_acl_packet_queue: None,
            iso_data_packet_length: None,
            iso_packet_queue: None,
            encrypted_handles: BTreeSet::new(),
        }
    }

    /// A device that also answers ATT requests using the given handler
    /// (an [`bumble_gatt::AttServer`] or a full [`bumble_gatt::GattServer`]).
    pub fn with_server(controller_id: usize, server: impl AttRequestHandler + 'static) -> Device {
        let config = DeviceConfiguration::default();
        let static_address = config.address.clone();
        Device {
            config,
            powered_on: false,
            public_address: None,
            random_address: static_address.clone(),
            static_address,
            local_supported_commands: None,
            local_supported_commands_status: None,
            local_version: None,
            local_version_status: None,
            local_lmp_features: BTreeMap::new(),
            local_lmp_features_max_page: None,
            local_lmp_feature_statuses: BTreeMap::new(),
            pending_local_lmp_feature_pages: VecDeque::new(),
            local_le_features: None,
            local_le_features_max_page: None,
            local_le_features_status: None,
            host_initialization_started: false,
            host_initialization_complete: false,
            event_mask_status: None,
            event_mask_page_2_status: None,
            le_event_mask_status: None,
            classic_buffer_status: None,
            classic_acl_buffer: None,
            le_buffer_status: None,
            le_acl_buffer: None,
            iso_buffer: None,
            suggested_default_data_length_read_status: None,
            suggested_default_data_length: None,
            suggested_default_data_length_write_required: false,
            suggested_default_data_length_write_status: None,
            number_of_supported_advertising_sets_status: None,
            number_of_supported_advertising_sets: 0,
            maximum_advertising_data_length_status: None,
            maximum_advertising_data_length: HOST_DEFAULT_MAXIMUM_ADVERTISING_DATA_LENGTH,
            local_channel_sounding_capabilities: None,
            local_channel_sounding_capabilities_status: None,
            controller_id,
            server: Some(Box::new(server)),
            connection_handle: None,
            connection_role: None,
            peer_address: None,
            le_connections: BTreeMap::new(),
            le_credit_managers: BTreeMap::new(),
            le_credit_server_specs: BTreeMap::new(),
            le_credit_errors: Vec::new(),
            eatt_inbox: Vec::new(),
            pending_att_indications: BTreeSet::new(),
            classic_connection_handle: None,
            classic_connection_role: None,
            classic_connections: BTreeMap::new(),
            classic_link_keys: BTreeMap::new(),
            classic_channel_managers: BTreeMap::new(),
            classic_channel_server_specs: BTreeMap::new(),
            classic_channel_errors: Vec::new(),
            classic_connection_requests: Vec::new(),
            classic_inquiry_results: Vec::new(),
            classic_inquiry_result_details: Vec::new(),
            classic_inquiry_complete: Vec::new(),
            classic_inquiry_response: None,
            classic_discovering: false,
            classic_auto_restart_inquiry: true,
            classic_remote_names: Vec::new(),
            classic_remote_name_results: Vec::new(),
            pending_remote_name_commands: VecDeque::new(),
            pending_remote_name_requests: Vec::new(),
            classic_pairing_events: Vec::new(),
            pending_classic_roles: Vec::new(),
            synchronous_connections: Vec::new(),
            synchronous_requests: Vec::new(),
            synchronous_inbox: Vec::new(),
            cis_requests: Vec::new(),
            configured_cis_handles: Vec::new(),
            cis_links: BTreeMap::new(),
            cis_control_events: Vec::new(),
            iso_data_paths: BTreeMap::new(),
            pending_iso_data_path_setups: VecDeque::new(),
            pending_iso_data_path_removals: VecDeque::new(),
            pending_iso_tx_syncs: VecDeque::new(),
            iso_tx_syncs: BTreeMap::new(),
            iso_control_events: Vec::new(),
            bigs: BTreeMap::new(),
            pending_bigs: BTreeSet::new(),
            pending_big_commands: VecDeque::new(),
            big_syncs: BTreeMap::new(),
            pending_big_syncs: BTreeSet::new(),
            pending_big_sync_commands: VecDeque::new(),
            bis_directions: BTreeMap::new(),
            biginfo_reports: Vec::new(),
            big_errors: Vec::new(),
            terminated_bigs: Vec::new(),
            iso_sequence_numbers: BTreeMap::new(),
            iso_assemblers: BTreeMap::new(),
            iso_inbox: Vec::new(),
            inbox: Vec::new(),
            l2cap_inbox: Vec::new(),
            security_requests: Vec::new(),
            pairing_manager: None,
            pairing_encryption_started: BTreeSet::new(),
            pairing_terminal_handles: BTreeSet::new(),
            pairing_errors: Vec::new(),
            key_store: None,
            address_resolver: None,
            key_store_errors: Vec::new(),
            long_term_key_requests: Vec::new(),
            connection_feature_errors: Vec::new(),
            connection_control_events: Vec::new(),
            pending_connection_controls: BTreeMap::new(),
            pending_disconnections: BTreeSet::new(),
            device_events: Vec::new(),
            event_listeners: BTreeMap::new(),
            next_event_listener_id: 1,
            pending_peer_lookups: BTreeMap::new(),
            peer_lookup_results: Vec::new(),
            next_peer_lookup_id: 1,
            peer_lookup_started_scanning: None,
            peer_lookup_started_discovery: false,
            pending_channel_sounding_configs: BTreeSet::new(),
            channel_sounding_errors: Vec::new(),
            channel_sounding_security_results: Vec::new(),
            channel_sounding_subevent_results: Vec::new(),
            channel_sounding_subevent_result_continuations: Vec::new(),
            vendor_events: Vec::new(),
            advertising_reports: Vec::new(),
            extended_advertising_reports: Vec::new(),
            advertisement_accumulators: BTreeMap::new(),
            advertisements: Vec::new(),
            scanning_is_passive: false,
            scanning: false,
            legacy_advertising: false,
            extended_advertising_handles: BTreeSet::new(),
            le_connecting: false,
            rpa_timeout_elapsed_seconds: 0,
            periodic_syncs: BTreeMap::new(),
            periodic_report_accumulators: BTreeMap::new(),
            periodic_advertisements: Vec::new(),
            periodic_sync_errors: Vec::new(),
            lost_periodic_syncs: Vec::new(),
            periodic_sync_transfers: Vec::new(),
            acl_data_packet_length: 27,
            acl_assemblers: BTreeMap::new(),
            acl_packet_queue: DataPacketQueue::new(64).expect("nonzero ACL queue capacity"),
            le_acl_data_packet_length: None,
            le_acl_packet_queue: None,
            iso_data_packet_length: None,
            iso_packet_queue: None,
            encrypted_handles: BTreeSet::new(),
        }
    }

    /// Build an upstream-style configured device with its GATT/ATT server.
    pub fn from_config(
        controller_id: usize,
        config: DeviceConfiguration,
    ) -> Result<Device, DeviceConfigurationError> {
        let server = config.build_gatt_server()?;
        Self::with_server_and_config(controller_id, config, server)
    }

    /// Load an upstream-style configured device and GATT server from a JSON file.
    pub fn from_config_file(
        controller_id: usize,
        filename: impl AsRef<std::path::Path>,
    ) -> Result<Device, DeviceConfigurationError> {
        Self::from_config(controller_id, DeviceConfiguration::from_file(filename)?)
    }

    /// Build a configured device that also owns an ATT request handler.
    pub fn with_server_and_config(
        controller_id: usize,
        config: DeviceConfiguration,
        server: impl AttRequestHandler + 'static,
    ) -> Result<Device, DeviceConfigurationError> {
        let eatt_enabled = config.eatt_enabled;
        let pairing_manager = config.build_pairing_manager()?;
        let mut device = Self::with_server(controller_id, server);
        device.install_configuration(config);
        device.pairing_manager = Some(pairing_manager);
        device.initialize_memory_key_store();
        if eatt_enabled {
            device
                .register_eatt_server(LeCreditBasedChannelSpec::default())
                .map_err(|error| DeviceConfigurationError::InvalidField {
                    field: "eatt_enabled",
                    message: error.to_string(),
                })?;
        }
        Ok(device)
    }

    fn install_configuration(&mut self, config: DeviceConfiguration) {
        self.static_address = config.address.clone();
        self.random_address = config.address.clone();
        self.config = config;
    }

    fn l2cap_information_capabilities(&self) -> InformationCapabilities {
        let mut capabilities =
            InformationCapabilities::new(self.config.l2cap_extended_features.iter().copied());
        for cid in [ATT_CID, SMP_CID] {
            capabilities
                .register_fixed_channel(cid)
                .expect("built-in fixed L2CAP CID fits the information mask");
        }
        if self.config.classic_smp_enabled {
            capabilities
                .register_fixed_channel(SMP_BR_CID)
                .expect("BR/EDR SMP fixed CID fits the information mask");
        }
        capabilities
    }

    fn initialize_memory_key_store(&mut self) {
        let uses_json_store = self
            .config
            .keystore
            .as_deref()
            .is_some_and(|spec| spec.split(':').next() == Some("JsonKeyStore"));
        if !uses_json_store {
            self.key_store = Some(Box::new(MemoryKeyStore::new()));
        }
    }

    /// Whether the configured controller setup has been submitted successfully.
    pub fn is_powered_on(&self) -> bool {
        self.powered_on
    }

    /// Public controller address learned from `HCI_Read_BD_ADDR` after power-on.
    pub fn public_address(&self) -> Option<&Address> {
        self.public_address.as_ref()
    }

    /// Stable random identity configured for this device.
    pub fn static_address(&self) -> &Address {
        &self.static_address
    }

    /// Random address currently programmed into the controller.
    pub fn random_address(&self) -> &Address {
        &self.random_address
    }

    /// Controller Supported Commands bitmap learned during power-on.
    pub fn local_supported_commands(&self) -> Option<&[u8; 64]> {
        self.local_supported_commands.as_ref()
    }

    /// Completion status for the power-on Supported Commands read.
    pub fn local_supported_commands_status(&self) -> Option<u8> {
        self.local_supported_commands_status
    }

    /// Controller HCI/LMP version information learned during reset.
    pub fn local_version(&self) -> Option<LocalVersionInformation> {
        self.local_version
    }

    pub fn local_version_status(&self) -> Option<u8> {
        self.local_version_status
    }

    /// Whether every reset-time Host initialization command has completed.
    pub fn host_initialization_complete(&self) -> bool {
        self.host_initialization_complete
    }

    /// Whether reset-time Host initialization completed successfully.
    pub fn host_initialization_succeeded(&self) -> bool {
        if !self.host_initialization_complete
            || self.event_mask_status != Some(0)
            || self.le_event_mask_status != Some(0)
        {
            return false;
        }
        if self.supports_command_name("HCI_SET_EVENT_MASK_PAGE_2_COMMAND")
            && self.event_mask_page_2_status != Some(0)
        {
            return false;
        }
        if self.supports_command_name("HCI_READ_BUFFER_SIZE_COMMAND")
            && self.classic_buffer_status != Some(0)
        {
            return false;
        }
        if (self.supports_command_name("HCI_LE_READ_BUFFER_SIZE_V2_COMMAND")
            || self.supports_command_name("HCI_LE_READ_BUFFER_SIZE_COMMAND"))
            && self.le_buffer_status != Some(0)
        {
            return false;
        }
        if self.manages_suggested_default_data_length()
            && (self.suggested_default_data_length_read_status != Some(0)
                || (self.suggested_default_data_length_write_required
                    && self.suggested_default_data_length_write_status != Some(0)))
        {
            return false;
        }
        true
    }

    pub fn event_mask_status(&self) -> Option<u8> {
        self.event_mask_status
    }

    pub fn event_mask_page_2_status(&self) -> Option<u8> {
        self.event_mask_page_2_status
    }

    pub fn le_event_mask_status(&self) -> Option<u8> {
        self.le_event_mask_status
    }

    pub fn classic_buffer_status(&self) -> Option<u8> {
        self.classic_buffer_status
    }

    pub fn classic_acl_buffer(&self) -> Option<ControllerBufferInfo> {
        self.classic_acl_buffer
    }

    pub fn le_buffer_status(&self) -> Option<u8> {
        self.le_buffer_status
    }

    pub fn le_acl_buffer(&self) -> Option<ControllerBufferInfo> {
        self.le_acl_buffer
    }

    pub fn iso_buffer(&self) -> Option<ControllerBufferInfo> {
        self.iso_buffer
    }

    pub fn suggested_default_data_length_read_status(&self) -> Option<u8> {
        self.suggested_default_data_length_read_status
    }

    /// Effective controller suggestion after any reset-time corrective write.
    pub fn suggested_default_data_length(&self) -> Option<LeSuggestedDefaultDataLength> {
        self.suggested_default_data_length
    }

    pub fn suggested_default_data_length_write_status(&self) -> Option<u8> {
        self.suggested_default_data_length_write_status
    }

    pub fn number_of_supported_advertising_sets_status(&self) -> Option<u8> {
        self.number_of_supported_advertising_sets_status
    }

    pub fn number_of_supported_advertising_sets(&self) -> u8 {
        self.number_of_supported_advertising_sets
    }

    pub fn maximum_advertising_data_length_status(&self) -> Option<u8> {
        self.maximum_advertising_data_length_status
    }

    pub fn maximum_advertising_data_length(&self) -> u16 {
        self.maximum_advertising_data_length
    }

    /// One local LMP feature page learned during reset.
    pub fn local_lmp_feature_page(&self, page_number: u8) -> Option<&[u8; 8]> {
        self.local_lmp_features.get(&page_number)
    }

    pub fn local_lmp_features_max_page(&self) -> Option<u8> {
        self.local_lmp_features_max_page
    }

    pub fn local_lmp_feature_status(&self, page_number: u8) -> Option<u8> {
        self.local_lmp_feature_statuses.get(&page_number).copied()
    }

    /// Whether one absolute upstream `LmpFeature` bit is set.
    pub fn supports_lmp_feature(&self, feature: u16) -> bool {
        let page_number = (feature / 64) as u8;
        let page_bit = usize::from(feature % 64);
        self.local_lmp_features
            .get(&page_number)
            .is_some_and(|page| page[page_bit / 8] & (1 << (page_bit % 8)) != 0)
    }

    pub fn supports_lmp_features(&self, features: &[u16]) -> bool {
        features
            .iter()
            .all(|feature| self.supports_lmp_feature(*feature))
    }

    fn supports_command_name(&self, command_name: &str) -> bool {
        self.local_supported_commands
            .as_ref()
            .is_some_and(|commands| {
                bumble_hci::metadata::supported_command_names(commands).contains(&command_name)
            })
    }

    fn manages_suggested_default_data_length(&self) -> bool {
        self.supports_command_name("HCI_LE_READ_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND")
            && self.supports_command_name("HCI_LE_WRITE_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND")
    }

    fn capability_discovery_complete(&self) -> bool {
        if self.local_supported_commands.is_none() {
            return false;
        }
        if self.supports_command_name("HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND")
            && self.local_version_status.is_none()
        {
            return false;
        }
        if (self.supports_command_name("HCI_LE_READ_ALL_LOCAL_SUPPORTED_FEATURES_COMMAND")
            || self.supports_command_name("HCI_LE_READ_LOCAL_SUPPORTED_FEATURES_COMMAND"))
            && self.local_le_features_status.is_none()
        {
            return false;
        }
        if self.supports_command_name("HCI_READ_LOCAL_EXTENDED_FEATURES_COMMAND") {
            if !self.pending_local_lmp_feature_pages.is_empty()
                || self.local_lmp_feature_statuses.is_empty()
            {
                return false;
            }
            if self
                .local_lmp_feature_statuses
                .values()
                .any(|status| *status != 0)
            {
                return true;
            }
            let Some(maximum_page_number) = self.local_lmp_features_max_page else {
                return false;
            };
            if !(0..=maximum_page_number)
                .all(|page_number| self.local_lmp_feature_statuses.contains_key(&page_number))
            {
                return false;
            }
        } else if self.supports_command_name("HCI_READ_LOCAL_SUPPORTED_FEATURES_COMMAND")
            && !self.local_lmp_feature_statuses.contains_key(&0)
        {
            return false;
        }
        true
    }

    fn maybe_start_host_initialization(&mut self, link: &mut LocalLink) {
        if self.host_initialization_started || !self.capability_discovery_complete() {
            return;
        }
        self.host_initialization_started = true;
        self.send_hci_command(
            link,
            Command::SetEventMask {
                event_mask: HOST_EVENT_MASK,
            },
        );
        if self.supports_command_name("HCI_SET_EVENT_MASK_PAGE_2_COMMAND") {
            self.send_hci_command(
                link,
                Command::SetEventMaskPage2 {
                    event_mask_page_2: HOST_EVENT_MASK_PAGE_2,
                },
            );
        }
        let le_event_mask = if self
            .local_version
            .is_some_and(|version| version.hci_version <= 6)
        {
            HOST_LE_EVENT_MASK_LEGACY
        } else {
            HOST_LE_EVENT_MASK
        };
        self.send_hci_command(link, Command::LeSetEventMask { le_event_mask });
        if self.supports_command_name("HCI_READ_BUFFER_SIZE_COMMAND") {
            self.send_hci_command(link, Command::ReadBufferSize);
        }
        if self.supports_command_name("HCI_LE_READ_BUFFER_SIZE_V2_COMMAND") {
            self.send_hci_command(link, Command::LeReadBufferSizeV2);
        } else if self.supports_command_name("HCI_LE_READ_BUFFER_SIZE_COMMAND") {
            self.send_hci_command(link, Command::LeReadBufferSize);
        }
        if self.manages_suggested_default_data_length() {
            self.send_hci_command(link, Command::LeReadSuggestedDefaultDataLength);
        }
        if self.supports_command_name("HCI_LE_READ_NUMBER_OF_SUPPORTED_ADVERTISING_SETS_COMMAND") {
            self.send_hci_command(link, Command::LeReadNumberOfSupportedAdvertisingSets);
        }
        if self.supports_command_name("HCI_LE_READ_MAXIMUM_ADVERTISING_DATA_LENGTH_COMMAND") {
            self.send_hci_command(link, Command::LeReadMaximumAdvertisingDataLength);
        }
    }

    fn maybe_finish_host_initialization(&mut self, link: &mut LocalLink) {
        if !self.host_initialization_started
            || self.host_initialization_complete
            || self.event_mask_status.is_none()
            || self.le_event_mask_status.is_none()
            || (self.supports_command_name("HCI_SET_EVENT_MASK_PAGE_2_COMMAND")
                && self.event_mask_page_2_status.is_none())
            || (self.supports_command_name("HCI_READ_BUFFER_SIZE_COMMAND")
                && self.classic_buffer_status.is_none())
            || ((self.supports_command_name("HCI_LE_READ_BUFFER_SIZE_V2_COMMAND")
                || self.supports_command_name("HCI_LE_READ_BUFFER_SIZE_COMMAND"))
                && self.le_buffer_status.is_none())
            || (self.manages_suggested_default_data_length()
                && self.suggested_default_data_length_read_status.is_none())
            || (self.suggested_default_data_length_write_required
                && self.suggested_default_data_length_write_status.is_none())
            || (self
                .supports_command_name("HCI_LE_READ_NUMBER_OF_SUPPORTED_ADVERTISING_SETS_COMMAND")
                && self.number_of_supported_advertising_sets_status.is_none())
            || (self.supports_command_name("HCI_LE_READ_MAXIMUM_ADVERTISING_DATA_LENGTH_COMMAND")
                && self.maximum_advertising_data_length_status.is_none())
        {
            return;
        }
        self.host_initialization_complete = true;
        if self.host_initialization_succeeded() {
            self.apply_supported_classic_scan_types(link);
        }
    }

    fn clear_host_initialization_state(&mut self) {
        self.host_initialization_started = false;
        self.host_initialization_complete = false;
        self.event_mask_status = None;
        self.event_mask_page_2_status = None;
        self.le_event_mask_status = None;
        self.classic_buffer_status = None;
        self.classic_acl_buffer = None;
        self.le_buffer_status = None;
        self.le_acl_buffer = None;
        self.iso_buffer = None;
        self.suggested_default_data_length_read_status = None;
        self.suggested_default_data_length = None;
        self.suggested_default_data_length_write_required = false;
        self.suggested_default_data_length_write_status = None;
        self.number_of_supported_advertising_sets_status = None;
        self.number_of_supported_advertising_sets = 0;
        self.maximum_advertising_data_length_status = None;
        self.maximum_advertising_data_length = HOST_DEFAULT_MAXIMUM_ADVERTISING_DATA_LENGTH;
        self.acl_data_packet_length = 27;
        self.acl_packet_queue =
            DataPacketQueue::new(64).expect("nonzero default ACL queue capacity");
        self.le_acl_data_packet_length = None;
        self.le_acl_packet_queue = None;
        self.iso_data_packet_length = None;
        self.iso_packet_queue = None;
    }

    fn request_local_lmp_feature_page(&mut self, link: &mut LocalLink, page_number: u8) {
        self.pending_local_lmp_feature_pages.push_back(page_number);
        self.send_hci_command(link, Command::ReadLocalExtendedFeatures { page_number });
    }

    fn apply_supported_classic_scan_types(&mut self, link: &mut LocalLink) {
        if !self.config.classic_enabled || !self.config.classic_interlaced_scan_enabled {
            return;
        }
        if self.supports_lmp_feature(LMP_FEATURE_INTERLACED_PAGE_SCAN) {
            self.send_hci_command(link, Command::WritePageScanType { page_scan_type: 1 });
        }
        if self.supports_lmp_feature(LMP_FEATURE_INTERLACED_INQUIRY_SCAN) {
            self.send_hci_command(link, Command::WriteInquiryScanType { scan_type: 1 });
        }
    }

    /// Controller LE feature bitmap learned during power-on.
    ///
    /// Controllers advertising the Bluetooth 6.1 all-page command return the
    /// complete 248-byte catalog. Older controllers retain the legacy first
    /// eight bytes instead.
    pub fn local_le_features(&self) -> Option<&[u8]> {
        self.local_le_features.as_deref()
    }

    /// Highest returned all-page LE feature page, or `None` for legacy reads.
    pub fn local_le_features_max_page(&self) -> Option<u8> {
        self.local_le_features_max_page
    }

    /// Completion status for the selected power-on LE feature read.
    pub fn local_le_features_status(&self) -> Option<u8> {
        self.local_le_features_status
    }

    /// Whether one controller LE feature bit is set.
    pub fn supports_le_feature(&self, feature: u8) -> bool {
        let feature = usize::from(feature);
        self.local_le_features
            .as_deref()
            .and_then(|features| features.get(feature / 8))
            .is_some_and(|byte| byte & (1 << (feature % 8)) != 0)
    }

    /// Whether every requested controller LE feature bit is set.
    pub fn supports_le_features(&self, features: &[u8]) -> bool {
        features
            .iter()
            .all(|feature| self.supports_le_feature(*feature))
    }

    /// Whether the controller supports an upstream `Phy` identifier.
    pub fn supports_le_phy(&self, phy: u8) -> Result<bool, LePhyError> {
        match phy {
            LE_1M_PHY => Ok(true),
            LE_2M_PHY => Ok(self.supports_le_feature(LE_FEATURE_2M_PHY)),
            LE_CODED_PHY => Ok(self.supports_le_feature(LE_FEATURE_CODED_PHY)),
            _ => Err(LePhyError::InvalidPhy { phy }),
        }
    }

    /// Whether the controller supports LE Extended Advertising.
    pub fn supports_le_extended_advertising(&self) -> bool {
        self.supports_le_feature(LE_FEATURE_EXTENDED_ADVERTISING)
    }

    /// Whether the controller supports LE Periodic Advertising.
    pub fn supports_le_periodic_advertising(&self) -> bool {
        self.supports_le_feature(LE_FEATURE_PERIODIC_ADVERTISING)
    }

    /// Local controller Channel Sounding capabilities learned during power-on.
    pub fn local_channel_sounding_capabilities(&self) -> Option<ChannelSoundingCapabilities> {
        self.local_channel_sounding_capabilities
    }

    /// Completion status for the power-on local Channel Sounding capability read.
    pub fn local_channel_sounding_capabilities_status(&self) -> Option<u8> {
        self.local_channel_sounding_capabilities_status
    }

    /// Flush all host-side connection state.
    ///
    /// This is the synchronous counterpart to upstream `Host.flush` and
    /// `Device.on_flush`: listeners observe [`DeviceEvent::Flush`] first, then
    /// one zero-reason disconnection for every live LE, Classic, CIS, or
    /// synchronous connection after its state has been removed.
    pub fn flush(&mut self) {
        let mut connection_handles = BTreeSet::new();
        connection_handles.extend(self.le_connections.keys().copied());
        connection_handles.extend(self.classic_connections.keys().copied());
        connection_handles.extend(self.cis_links.keys().copied());
        connection_handles.extend(
            self.synchronous_connections
                .iter()
                .map(|connection| connection.connection_handle),
        );

        self.emit_device_event(DeviceEvent::Flush);

        let eatt_bearers = self
            .le_credit_managers
            .iter()
            .flat_map(|(connection_handle, manager)| {
                manager
                    .channels()
                    .filter(|channel| channel.psm == EATT_PSM)
                    .map(|channel| (*connection_handle, channel.source_cid))
            })
            .collect::<Vec<_>>();
        for connection_handle in self.le_connections.keys().copied().collect::<Vec<_>>() {
            self.remove_att_bearer_state(connection_handle, ATT_CID);
        }
        for (connection_handle, source_cid) in eatt_bearers {
            self.remove_att_bearer_state(connection_handle, source_cid);
        }
        if let Some(manager) = self.pairing_manager.as_mut() {
            for connection_handle in &connection_handles {
                manager.disconnect(*connection_handle);
            }
        }
        for connection_handle in &connection_handles {
            self.clear_iso_control_state(*connection_handle);
        }

        self.connection_handle = None;
        self.connection_role = None;
        self.peer_address = None;
        self.le_connections.clear();
        self.le_credit_managers.clear();
        self.eatt_inbox.clear();
        self.pending_att_indications.clear();
        self.classic_connection_handle = None;
        self.classic_connection_role = None;
        self.classic_connections.clear();
        self.classic_link_keys.clear();
        self.classic_channel_managers.clear();
        self.pending_classic_roles.clear();
        self.pending_remote_name_commands.clear();
        self.pending_remote_name_requests.clear();
        self.pending_local_lmp_feature_pages.clear();
        self.synchronous_connections.clear();
        self.synchronous_requests.clear();
        self.synchronous_inbox.clear();
        self.cis_requests.clear();
        self.cis_links.clear();
        self.pending_iso_data_path_setups.clear();
        self.pending_iso_data_path_removals.clear();
        self.pending_iso_tx_syncs.clear();
        self.iso_data_paths
            .retain(|(handle, _), _| !connection_handles.contains(handle));
        self.iso_tx_syncs
            .retain(|handle, _| !connection_handles.contains(handle));
        self.iso_sequence_numbers
            .retain(|handle, _| !connection_handles.contains(handle));
        self.iso_assemblers
            .retain(|handle, _| !connection_handles.contains(handle));
        self.iso_inbox
            .retain(|sdu| !connection_handles.contains(&sdu.connection_handle));
        self.inbox.clear();
        self.l2cap_inbox.clear();
        self.security_requests.clear();
        self.pairing_encryption_started.clear();
        self.pairing_terminal_handles.clear();
        self.long_term_key_requests.clear();
        self.pending_connection_controls.clear();
        self.pending_disconnections.clear();
        self.pending_peer_lookups.clear();
        self.peer_lookup_started_scanning = None;
        self.peer_lookup_started_discovery = false;
        self.pending_channel_sounding_configs.clear();
        self.acl_assemblers.clear();
        self.acl_packet_queue.flush_all();
        if let Some(queue) = self.le_acl_packet_queue.as_mut() {
            queue.flush_all();
        }
        if let Some(queue) = self.iso_packet_queue.as_mut() {
            queue.flush_all();
        }
        self.encrypted_handles.clear();
        self.le_connecting = false;

        for connection_handle in connection_handles {
            self.emit_device_event(DeviceEvent::Disconnected {
                connection_handle,
                reason: 0,
            });
        }
    }

    /// Reset and configure the controller from this device's loaded configuration.
    ///
    /// This is the command-oriented counterpart to upstream `Device.power_on`.
    /// Command completions remain asynchronous and are consumed by [`Device::poll`],
    /// matching the rest of this crate's explicit event-journal design.
    pub fn power_on(&mut self, link: &mut LocalLink) -> Result<(), DevicePowerError> {
        let (local_name, inquiry_response) = if self.config.classic_enabled {
            if self.config.class_of_device > 0x00FF_FFFF {
                return Err(DevicePowerError::ClassOfDeviceOutOfRange {
                    value: self.config.class_of_device,
                });
            }
            let inquiry_response = match self.classic_inquiry_response {
                Some(response) => response,
                None => default_inquiry_response(&self.config.name)?,
            };
            (
                Some(padded_local_name(&self.config.name)?),
                Some(inquiry_response),
            )
        } else {
            (None, None)
        };

        let irk: Option<[u8; 16]> = if self.config.le_privacy_enabled {
            Some(self.config.irk.as_slice().try_into().map_err(|_| {
                DevicePowerError::InvalidIrkLength {
                    actual: self.config.irk.len(),
                }
            })?)
        } else {
            None
        };

        if self.powered_on
            || !self.le_connections.is_empty()
            || !self.classic_connections.is_empty()
            || !self.cis_links.is_empty()
            || !self.synchronous_connections.is_empty()
            || !self.pending_remote_name_requests.is_empty()
            || !self.pending_local_lmp_feature_pages.is_empty()
        {
            self.flush();
        }

        let any_random = Address::from_bytes([0; 6], bumble::AddressType::RANDOM_DEVICE);
        if self.static_address == any_random {
            self.static_address = random_static_address();
        }
        self.random_address = if let Some(irk) = irk {
            bumble_smp::generate_resolvable_private_address(&irk)
        } else {
            self.static_address.clone()
        };

        self.powered_on = false;
        self.scanning = false;
        self.legacy_advertising = false;
        self.extended_advertising_handles.clear();
        self.le_connecting = false;
        self.rpa_timeout_elapsed_seconds = 0;
        self.classic_discovering = false;
        self.classic_auto_restart_inquiry = true;
        self.pending_disconnections.clear();
        self.public_address = None;
        self.local_supported_commands = None;
        self.local_supported_commands_status = None;
        self.local_version = None;
        self.local_version_status = None;
        self.local_lmp_features.clear();
        self.local_lmp_features_max_page = None;
        self.local_lmp_feature_statuses.clear();
        self.pending_local_lmp_feature_pages.clear();
        self.local_le_features = None;
        self.local_le_features_max_page = None;
        self.local_le_features_status = None;
        self.clear_host_initialization_state();
        self.address_resolver = None;
        self.local_channel_sounding_capabilities = None;
        self.local_channel_sounding_capabilities_status = None;
        self.pending_peer_lookups.clear();
        self.peer_lookup_results.clear();
        self.peer_lookup_started_scanning = None;
        self.peer_lookup_started_discovery = false;
        self.pending_remote_name_commands.clear();
        self.pending_remote_name_requests.clear();
        self.pending_bigs.clear();
        self.pending_big_commands.clear();
        self.pending_big_syncs.clear();
        self.pending_big_sync_commands.clear();
        self.send_hci_command(link, Command::Reset);
        self.send_hci_command(link, Command::ReadBdAddr);
        self.send_hci_command(
            link,
            Command::WriteLeHostSupport {
                le_supported_host: u8::from(self.config.le_enabled),
                simultaneous_le_host: u8::from(self.config.le_simultaneous_enabled),
            },
        );

        if self.config.le_enabled {
            self.send_hci_command(
                link,
                Command::LeSetRandomAddress {
                    random_address: self.random_address.clone(),
                },
            );
            if self.config.address_resolution_offload {
                self.send_hci_command(
                    link,
                    Command::LeSetAddressResolutionEnable {
                        address_resolution_enable: 1,
                    },
                );
            }
            if self.config.cis_enabled {
                self.send_hci_command(
                    link,
                    Command::LeSetHostFeature {
                        bit_number: LE_FEATURE_CONNECTED_ISOCHRONOUS_STREAM,
                        bit_value: 1,
                    },
                );
            }
            if self.config.le_subrate_enabled {
                self.send_hci_command(
                    link,
                    Command::LeSetHostFeature {
                        bit_number: LE_FEATURE_CONNECTION_SUBRATING_HOST_SUPPORT,
                        bit_value: 1,
                    },
                );
            }
            if self.config.channel_sounding_enabled {
                self.send_hci_command(
                    link,
                    Command::LeSetHostFeature {
                        bit_number: LE_FEATURE_CHANNEL_SOUNDING_HOST_SUPPORT,
                        bit_value: 1,
                    },
                );
                self.send_hci_command(link, Command::LeCsReadLocalSupportedCapabilities);
            }
            if self.config.le_shorter_connection_intervals_enabled {
                self.send_hci_command(
                    link,
                    Command::LeSetHostFeature {
                        bit_number: LE_FEATURE_SHORTER_CONNECTION_INTERVALS_HOST_SUPPORT,
                        bit_value: 1,
                    },
                );
            }
        }

        if self.config.classic_enabled {
            self.classic_inquiry_response = inquiry_response;
            self.send_hci_command(
                link,
                Command::WriteLocalName {
                    local_name: local_name.expect("validated for Classic configuration"),
                },
            );
            self.send_hci_command(
                link,
                Command::WriteClassOfDevice {
                    class_of_device: self.config.class_of_device,
                },
            );
            self.send_hci_command(
                link,
                Command::WriteSimplePairingMode {
                    simple_pairing_mode: u8::from(self.config.classic_ssp_enabled),
                },
            );
            self.send_hci_command(
                link,
                Command::WriteSecureConnectionsHostSupport {
                    secure_connections_host_support: u8::from(self.config.classic_sc_enabled),
                },
            );
            let scan_enable =
                u8::from(self.config.discoverable) | (u8::from(self.config.connectable) << 1);
            // Upstream applies connectability, then discoverability and its EIR.
            self.send_hci_command(link, Command::WriteScanEnable { scan_enable });
            self.send_hci_command(
                link,
                Command::WriteExtendedInquiryResponse {
                    fec_required: 0,
                    extended_inquiry_response: self
                        .classic_inquiry_response
                        .expect("validated for Classic configuration"),
                },
            );
            self.send_hci_command(link, Command::WriteScanEnable { scan_enable });
        }

        // Upstream Host initialization first learns the Supported Commands
        // bitmap, then chooses the all-page LE feature read when available and
        // falls back to the legacy eight-byte read. The second command is
        // selected from the completion handler because this host is explicitly
        // event-driven.
        self.send_hci_command(link, Command::ReadLocalSupportedCommands);

        self.powered_on = true;
        Ok(())
    }

    /// Reset the controller and restart Host capability discovery.
    ///
    /// Like upstream `Device.reset`, this preserves the device's powered flag,
    /// while an already-ready host is flushed before the reset command.
    pub fn reset(&mut self, link: &mut LocalLink) {
        if self.powered_on
            || !self.le_connections.is_empty()
            || !self.classic_connections.is_empty()
            || !self.cis_links.is_empty()
            || !self.synchronous_connections.is_empty()
            || !self.pending_peer_lookups.is_empty()
            || !self.pending_remote_name_requests.is_empty()
            || !self.pending_local_lmp_feature_pages.is_empty()
            || self.le_connecting
            || !self.pending_disconnections.is_empty()
        {
            self.flush();
        }

        self.local_supported_commands = None;
        self.local_supported_commands_status = None;
        self.local_version = None;
        self.local_version_status = None;
        self.local_lmp_features.clear();
        self.local_lmp_features_max_page = None;
        self.local_lmp_feature_statuses.clear();
        self.pending_local_lmp_feature_pages.clear();
        self.local_le_features = None;
        self.local_le_features_max_page = None;
        self.local_le_features_status = None;
        self.clear_host_initialization_state();
        self.send_hci_command(link, Command::Reset);
        self.send_hci_command(link, Command::ReadLocalSupportedCommands);
    }

    /// Mark the host side powered off after any transport-specific flush.
    pub fn power_off(&mut self) {
        if self.powered_on
            || !self.le_connections.is_empty()
            || !self.classic_connections.is_empty()
            || !self.cis_links.is_empty()
            || !self.synchronous_connections.is_empty()
            || !self.pending_peer_lookups.is_empty()
            || !self.pending_remote_name_requests.is_empty()
            || self.le_connecting
            || !self.pending_disconnections.is_empty()
        {
            self.flush();
        }
        self.powered_on = false;
        self.scanning = false;
        self.legacy_advertising = false;
        self.extended_advertising_handles.clear();
        self.le_connecting = false;
        self.rpa_timeout_elapsed_seconds = 0;
        self.classic_discovering = false;
        self.classic_auto_restart_inquiry = true;
        self.pending_disconnections.clear();
        self.pending_peer_lookups.clear();
        self.peer_lookup_results.clear();
        self.peer_lookup_started_scanning = None;
        self.peer_lookup_started_discovery = false;
        self.pending_remote_name_commands.clear();
        self.pending_remote_name_requests.clear();
        self.pending_bigs.clear();
        self.pending_big_commands.clear();
        self.pending_big_syncs.clear();
        self.pending_big_sync_commands.clear();
    }

    /// Generate and program a fresh resolvable private address.
    ///
    /// This is the explicit, forced rotation surface. Configured periodic
    /// rotation should be driven through [`Device::advance_rpa_timeout`], which
    /// applies upstream's advertising/scanning/connecting suppression policy.
    pub fn update_rpa(&mut self, link: &mut LocalLink) -> Result<Address, DevicePowerError> {
        if !self.powered_on {
            return Err(DevicePowerError::NotPoweredOn);
        }
        if !self.config.le_privacy_enabled {
            return Err(DevicePowerError::PrivacyDisabled);
        }
        let irk: &[u8; 16] = self.config.irk.as_slice().try_into().map_err(|_| {
            DevicePowerError::InvalidIrkLength {
                actual: self.config.irk.len(),
            }
        })?;
        let address = bumble_smp::generate_resolvable_private_address(irk);
        self.random_address = address.clone();
        self.send_hci_command(
            link,
            Command::LeSetRandomAddress {
                random_address: address.clone(),
            },
        );
        Ok(address)
    }

    /// Advance the configured periodic RPA timer by a caller-supplied duration.
    ///
    /// The synchronous host deliberately owns no async runtime or wall clock.
    /// Its event loop supplies elapsed whole seconds here, and this method
    /// performs the work of upstream's `_run_rpa_periodic_update` task. A late
    /// wake is coalesced into one attempt and starts a fresh timeout interval,
    /// just like a delayed async sleep. Rotation is suppressed while legacy or
    /// extended advertising, scanning, or LE connection initiation is active.
    /// A suppressed attempt also starts a fresh interval.
    ///
    /// `Ok(Some(address))` means a new RPA was submitted to the controller.
    /// `Ok(None)` means the timer is disabled, not armed, not yet due, or the
    /// device is currently busy.
    pub fn advance_rpa_timeout(
        &mut self,
        link: &mut LocalLink,
        elapsed_seconds: u64,
    ) -> Result<Option<Address>, DevicePowerError> {
        if !self.powered_on || !self.config.le_privacy_enabled || self.config.le_rpa_timeout == 0 {
            self.rpa_timeout_elapsed_seconds = 0;
            return Ok(None);
        }

        self.rpa_timeout_elapsed_seconds = self
            .rpa_timeout_elapsed_seconds
            .saturating_add(elapsed_seconds);
        if self.rpa_timeout_elapsed_seconds < self.config.le_rpa_timeout {
            return Ok(None);
        }
        self.rpa_timeout_elapsed_seconds = 0;

        if self.is_advertising() || self.scanning || self.le_connecting {
            return Ok(None);
        }

        self.update_rpa(link).map(Some)
    }

    /// Whether any legacy or extended LE advertising procedure is active.
    pub fn is_advertising(&self) -> bool {
        self.legacy_advertising || !self.extended_advertising_handles.is_empty()
    }

    /// Whether legacy or extended LE scanning is active.
    pub fn is_scanning(&self) -> bool {
        self.scanning
    }

    /// Whether this device has submitted an LE connection attempt that has not
    /// yet completed or failed.
    pub fn is_le_connecting(&self) -> bool {
        self.le_connecting
    }

    pub fn controller_id(&self) -> usize {
        self.controller_id
    }

    /// The selected LE connection handle, or the most recently established one.
    pub fn connection_handle(&self) -> Option<u16> {
        self.connection_handle
    }

    /// Iterate over all established LE ACL connections, ordered by handle.
    pub fn le_connections(&self) -> impl Iterator<Item = &LeConnectionInfo> {
        self.le_connections.values()
    }

    pub fn le_connection(&self, connection_handle: u16) -> Option<&LeConnectionInfo> {
        self.le_connections.get(&connection_handle)
    }

    pub fn connection_handle_for_peer(&self, peer_address: &Address) -> Option<u16> {
        self.le_connections
            .values()
            .find(|connection| connection.peer_address == *peer_address)
            .map(|connection| connection.connection_handle)
    }

    /// Select which LE connection the convenience methods without a handle use.
    pub fn select_connection(&mut self, connection_handle: u16) -> bool {
        let Some(connection) = self.le_connections.get(&connection_handle).cloned() else {
            return false;
        };
        self.connection_handle = Some(connection.connection_handle);
        self.connection_role = Some(connection.role);
        self.peer_address = Some(connection.peer_address);
        true
    }

    /// `true` while at least one LE connection is established.
    pub fn is_connected(&self) -> bool {
        !self.le_connections.is_empty()
    }

    pub fn is_connected_on_handle(&self, connection_handle: u16) -> bool {
        self.le_connections.contains_key(&connection_handle)
    }

    pub fn connection_role(&self) -> Option<u8> {
        self.connection_role
    }

    pub fn peer_address(&self) -> Option<&Address> {
        self.peer_address.as_ref()
    }

    pub fn read_remote_le_features_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
    ) -> bool {
        if !self.le_connections.contains_key(&connection_handle) {
            return false;
        }
        self.send_hci_command(link, Command::LeReadRemoteFeatures { connection_handle });
        true
    }

    pub fn read_remote_classic_features_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
    ) -> bool {
        if !self.classic_connections.contains_key(&connection_handle) {
            return false;
        }
        self.send_hci_command(
            link,
            Command::ReadRemoteSupportedFeatures { connection_handle },
        );
        true
    }

    pub fn take_connection_feature_errors(&mut self) -> Vec<ConnectionFeatureError> {
        std::mem::take(&mut self.connection_feature_errors)
    }

    /// Drain completed LE connection-control requests and asynchronous changes.
    pub fn take_connection_control_events(&mut self) -> Vec<LeConnectionControlEvent> {
        std::mem::take(&mut self.connection_control_events)
    }

    /// Register a listener invoked synchronously for every high-level device
    /// event. The returned identifier can be passed to
    /// [`Self::remove_event_listener`].
    pub fn add_event_listener(
        &mut self,
        listener: impl FnMut(&DeviceEvent) + Send + 'static,
    ) -> DeviceEventListenerId {
        let mut listener_id = self.next_event_listener_id;
        while self.event_listeners.contains_key(&listener_id) {
            listener_id = listener_id.wrapping_add(1).max(1);
        }
        self.next_event_listener_id = listener_id.wrapping_add(1).max(1);
        self.event_listeners.insert(listener_id, Box::new(listener));
        listener_id
    }

    /// Remove a previously registered event listener.
    pub fn remove_event_listener(&mut self, listener_id: DeviceEventListenerId) -> bool {
        self.event_listeners.remove(&listener_id).is_some()
    }

    /// Drain the high-level event journal in emission order.
    pub fn take_device_events(&mut self) -> Vec<DeviceEvent> {
        std::mem::take(&mut self.device_events)
    }

    fn emit_device_event(&mut self, event: DeviceEvent) {
        self.device_events.push(event.clone());
        for listener in self.event_listeners.values_mut() {
            listener(&event);
        }
    }

    fn record_connection_control_event(&mut self, event: LeConnectionControlEvent) {
        self.connection_control_events.push(event.clone());
        self.emit_device_event(DeviceEvent::LeConnectionControl(event));
    }

    fn record_classic_pairing_event(&mut self, event: ClassicPairingEvent) {
        self.classic_pairing_events.push(event.clone());
        self.emit_device_event(DeviceEvent::ClassicPairing(event));
    }

    /// Request legacy LE connection parameters on one established ACL.
    pub fn update_connection_parameters_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        parameters: LeConnectionUpdateParameters,
    ) -> bool {
        if !self.le_connections.contains_key(&connection_handle) {
            return false;
        }
        self.send_connection_control_command(
            link,
            connection_handle,
            Command::LeConnectionUpdate {
                connection_handle,
                connection_interval_min: parameters.connection_interval_min,
                connection_interval_max: parameters.connection_interval_max,
                max_latency: parameters.max_latency,
                supervision_timeout: parameters.supervision_timeout,
                min_ce_length: parameters.min_ce_length,
                max_ce_length: parameters.max_ce_length,
            },
        );
        true
    }

    /// Request Bluetooth 6.2 connection-rate and subrate parameters on one ACL.
    pub fn update_connection_rate_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        parameters: LeConnectionRateParameters,
    ) -> bool {
        if !self.le_connections.contains_key(&connection_handle) {
            return false;
        }
        self.send_connection_control_command(
            link,
            connection_handle,
            Command::LeConnectionRateRequest {
                connection_handle,
                connection_interval_min: parameters.connection_interval_min,
                connection_interval_max: parameters.connection_interval_max,
                subrate_min: parameters.subrate_min,
                subrate_max: parameters.subrate_max,
                max_latency: parameters.max_latency,
                continuation_number: parameters.continuation_number,
                supervision_timeout: parameters.supervision_timeout,
                min_ce_length: parameters.min_ce_length,
                max_ce_length: parameters.max_ce_length,
            },
        );
        true
    }

    /// Set controller-wide Bluetooth 6.2 connection-rate defaults.
    pub fn set_default_connection_rate(
        &mut self,
        link: &mut LocalLink,
        parameters: LeConnectionRateParameters,
    ) {
        self.send_hci_command(
            link,
            Command::LeSetDefaultRateParameters {
                connection_interval_min: parameters.connection_interval_min,
                connection_interval_max: parameters.connection_interval_max,
                subrate_min: parameters.subrate_min,
                subrate_max: parameters.subrate_max,
                max_latency: parameters.max_latency,
                continuation_number: parameters.continuation_number,
                supervision_timeout: parameters.supervision_timeout,
                min_ce_length: parameters.min_ce_length,
                max_ce_length: parameters.max_ce_length,
            },
        );
    }

    /// Set controller-wide subrate defaults for new connections.
    pub fn set_default_subrate(
        &mut self,
        link: &mut LocalLink,
        parameters: LeSubrateRequestParameters,
    ) {
        self.send_hci_command(
            link,
            Command::LeSetDefaultSubrate {
                subrate_min: parameters.subrate_min,
                subrate_max: parameters.subrate_max,
                max_latency: parameters.max_latency,
                continuation_number: parameters.continuation_number,
                supervision_timeout: parameters.supervision_timeout,
            },
        );
    }

    /// Set the preferred LE data length for one established ACL.
    ///
    /// The bounds match upstream `Device.set_data_length`.
    pub fn set_data_length_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        tx_octets: u16,
        tx_time: u16,
    ) -> bool {
        if !self.le_connections.contains_key(&connection_handle)
            || !(0x001B..=0x00FB).contains(&tx_octets)
            || !(0x0148..=0x4290).contains(&tx_time)
        {
            return false;
        }
        self.send_connection_control_command(
            link,
            connection_handle,
            Command::LeSetDataLength {
                connection_handle,
                tx_octets,
                tx_time,
            },
        );
        true
    }

    /// Query the current LE PHY for one established ACL.
    pub fn read_phy_on_handle(&mut self, link: &mut LocalLink, connection_handle: u16) -> bool {
        if !self.le_connections.contains_key(&connection_handle) {
            return false;
        }
        self.send_connection_control_command(
            link,
            connection_handle,
            Command::LeReadPhy { connection_handle },
        );
        true
    }

    /// Request transmit and receive LE PHY preferences for one ACL.
    /// `None` means no preference in that direction.
    pub fn set_phy_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        tx_phys: Option<u8>,
        rx_phys: Option<u8>,
        phy_options: u16,
    ) -> bool {
        if !self.le_connections.contains_key(&connection_handle) {
            return false;
        }
        let all_phys = u8::from(tx_phys.is_none()) | (u8::from(rx_phys.is_none()) << 1);
        self.send_connection_control_command(
            link,
            connection_handle,
            Command::LeSetPhy {
                connection_handle,
                all_phys,
                tx_phys: tx_phys.unwrap_or_default(),
                rx_phys: rx_phys.unwrap_or_default(),
                phy_options,
            },
        );
        true
    }

    /// Set the controller-wide LE PHY preferences used for new ACLs.
    pub fn set_default_phy(
        &mut self,
        link: &mut LocalLink,
        tx_phys: Option<u8>,
        rx_phys: Option<u8>,
    ) {
        let all_phys = u8::from(tx_phys.is_none()) | (u8::from(rx_phys.is_none()) << 1);
        self.send_hci_command(
            link,
            Command::LeSetDefaultPhy {
                all_phys,
                tx_phys: tx_phys.unwrap_or_default(),
                rx_phys: rx_phys.unwrap_or_default(),
            },
        );
    }

    /// Query the controller's RSSI for one established LE ACL.
    pub fn read_rssi_on_handle(&mut self, link: &mut LocalLink, connection_handle: u16) -> bool {
        if !self.le_connections.contains_key(&connection_handle) {
            return false;
        }
        self.send_connection_control_command(
            link,
            connection_handle,
            Command::ReadRssi {
                handle: connection_handle,
            },
        );
        true
    }

    pub fn request_le_subrate_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        parameters: LeSubrateRequestParameters,
    ) -> bool {
        if !self.le_connections.contains_key(&connection_handle) {
            return false;
        }
        self.send_connection_control_command(
            link,
            connection_handle,
            Command::LeSubrateRequest {
                connection_handle,
                subrate_min: parameters.subrate_min,
                subrate_max: parameters.subrate_max,
                max_latency: parameters.max_latency,
                continuation_number: parameters.continuation_number,
                supervision_timeout: parameters.supervision_timeout,
            },
        );
        true
    }

    pub fn read_remote_channel_sounding_capabilities_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
    ) -> bool {
        if !self.le_connections.contains_key(&connection_handle) {
            return false;
        }
        self.send_hci_command(
            link,
            Command::LeCsReadRemoteSupportedCapabilities { connection_handle },
        );
        true
    }

    pub fn set_default_channel_sounding_settings_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        settings: ChannelSoundingDefaultSettings,
    ) -> bool {
        if !self.le_connections.contains_key(&connection_handle) {
            return false;
        }
        self.send_hci_command(
            link,
            Command::LeCsSetDefaultSettings {
                connection_handle,
                role_enable: settings.role_enable,
                cs_sync_antenna_selection: settings.cs_sync_antenna_selection,
                max_tx_power: settings.max_tx_power,
            },
        );
        true
    }

    pub fn create_channel_sounding_config_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        config_id: Option<u8>,
        parameters: ChannelSoundingCreateConfigParameters,
    ) -> Option<u8> {
        let connection = self.le_connections.get(&connection_handle)?;
        let config_id = match config_id {
            Some(config_id)
                if (MIN_CHANNEL_SOUNDING_CONFIG_ID..=MAX_CHANNEL_SOUNDING_CONFIG_ID)
                    .contains(&config_id)
                    && !connection.channel_sounding_configs.contains_key(&config_id)
                    && !self
                        .pending_channel_sounding_configs
                        .contains(&(connection_handle, config_id)) =>
            {
                config_id
            }
            Some(_) => return None,
            None => (MIN_CHANNEL_SOUNDING_CONFIG_ID..=MAX_CHANNEL_SOUNDING_CONFIG_ID).find(
                |config_id| {
                    !connection.channel_sounding_configs.contains_key(config_id)
                        && !self
                            .pending_channel_sounding_configs
                            .contains(&(connection_handle, *config_id))
                },
            )?,
        };
        self.pending_channel_sounding_configs
            .insert((connection_handle, config_id));
        self.send_hci_command(
            link,
            Command::LeCsCreateConfig {
                connection_handle,
                config_id,
                create_context: parameters.create_context,
                main_mode_type: parameters.main_mode_type,
                sub_mode_type: parameters.sub_mode_type,
                min_main_mode_steps: parameters.min_main_mode_steps,
                max_main_mode_steps: parameters.max_main_mode_steps,
                main_mode_repetition: parameters.main_mode_repetition,
                mode_0_steps: parameters.mode_0_steps,
                role: parameters.role,
                rtt_type: parameters.rtt_type,
                cs_sync_phy: parameters.cs_sync_phy,
                channel_map: parameters.channel_map,
                channel_map_repetition: parameters.channel_map_repetition,
                channel_selection_type: parameters.channel_selection_type,
                ch3c_shape: parameters.ch3c_shape,
                ch3c_jump: parameters.ch3c_jump,
                reserved: 0,
            },
        );
        Some(config_id)
    }

    pub fn remove_channel_sounding_config_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        config_id: u8,
    ) -> bool {
        if !self
            .le_connections
            .get(&connection_handle)
            .is_some_and(|connection| connection.channel_sounding_configs.contains_key(&config_id))
        {
            return false;
        }
        self.send_hci_command(
            link,
            Command::LeCsRemoveConfig {
                connection_handle,
                config_id,
            },
        );
        true
    }

    pub fn enable_channel_sounding_security_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
    ) -> bool {
        if !self.le_connections.contains_key(&connection_handle) {
            return false;
        }
        self.send_hci_command(link, Command::LeCsSecurityEnable { connection_handle });
        true
    }

    pub fn set_channel_sounding_procedure_parameters_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        config_id: u8,
        parameters: ChannelSoundingProcedureParameters,
    ) -> bool {
        if !self
            .le_connections
            .get(&connection_handle)
            .is_some_and(|connection| connection.channel_sounding_configs.contains_key(&config_id))
        {
            return false;
        }
        self.send_hci_command(
            link,
            Command::LeCsSetProcedureParameters {
                connection_handle,
                config_id,
                max_procedure_len: parameters.max_procedure_len,
                min_procedure_interval: parameters.min_procedure_interval,
                max_procedure_interval: parameters.max_procedure_interval,
                max_procedure_count: parameters.max_procedure_count,
                min_subevent_len: parameters.min_subevent_len,
                max_subevent_len: parameters.max_subevent_len,
                tone_antenna_config_selection: parameters.tone_antenna_config_selection,
                phy: parameters.phy,
                tx_power_delta: parameters.tx_power_delta,
                preferred_peer_antenna: parameters.preferred_peer_antenna,
                snr_control_initiator: parameters.snr_control_initiator,
                snr_control_reflector: parameters.snr_control_reflector,
            },
        );
        true
    }

    pub fn enable_channel_sounding_procedure_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        config_id: u8,
        enabled: bool,
    ) -> bool {
        if !self
            .le_connections
            .get(&connection_handle)
            .is_some_and(|connection| connection.channel_sounding_configs.contains_key(&config_id))
        {
            return false;
        }
        self.send_hci_command(
            link,
            Command::LeCsProcedureEnable {
                connection_handle,
                config_id,
                enable: u8::from(enabled),
            },
        );
        true
    }

    pub fn take_channel_sounding_errors(&mut self) -> Vec<ChannelSoundingError> {
        std::mem::take(&mut self.channel_sounding_errors)
    }

    pub fn take_channel_sounding_security_results(&mut self) -> Vec<(u16, u8)> {
        std::mem::take(&mut self.channel_sounding_security_results)
    }

    pub fn take_channel_sounding_subevent_results(&mut self) -> Vec<ChannelSoundingSubeventResult> {
        std::mem::take(&mut self.channel_sounding_subevent_results)
    }

    pub fn take_channel_sounding_subevent_result_continuations(
        &mut self,
    ) -> Vec<ChannelSoundingSubeventResultContinue> {
        std::mem::take(&mut self.channel_sounding_subevent_result_continuations)
    }

    pub fn take_vendor_events(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.vendor_events)
    }

    pub fn enter_sniff_mode_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        interval: u16,
        attempt: u16,
        timeout: u16,
    ) -> bool {
        if !self.le_connections.contains_key(&connection_handle)
            && !self.classic_connections.contains_key(&connection_handle)
        {
            return false;
        }
        self.send_hci_command(
            link,
            Command::SniffMode {
                connection_handle,
                sniff_max_interval: interval,
                sniff_min_interval: interval,
                sniff_attempt: attempt,
                sniff_timeout: timeout,
            },
        );
        true
    }

    pub fn exit_sniff_mode_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
    ) -> bool {
        if !self.le_connections.contains_key(&connection_handle)
            && !self.classic_connections.contains_key(&connection_handle)
        {
            return false;
        }
        self.send_hci_command(link, Command::ExitSniffMode { connection_handle });
        true
    }

    pub fn set_random_address(&mut self, link: &mut LocalLink, address: Address) {
        self.send_hci_command(
            link,
            Command::LeSetRandomAddress {
                random_address: address,
            },
        );
    }

    pub fn start_advertising(&mut self, link: &mut LocalLink, data: &[u8]) -> bool {
        if data.len() > 31 {
            return false;
        }
        self.send_hci_command(
            link,
            Command::LeSetAdvertisingParameters {
                advertising_interval_min: 0x0800,
                advertising_interval_max: 0x0800,
                advertising_type: 0,
                own_address_type: 1,
                peer_address_type: 0,
                peer_address: Address::from_bytes([0; 6], bumble::AddressType::PUBLIC_DEVICE),
                advertising_channel_map: 7,
                advertising_filter_policy: 0,
            },
        );
        self.send_hci_command(
            link,
            Command::LeSetAdvertisingData {
                advertising_data: data.to_vec(),
            },
        );
        self.send_hci_command(
            link,
            Command::LeSetAdvertisingEnable {
                advertising_enable: 1,
            },
        );
        self.legacy_advertising = true;
        true
    }

    /// Start legacy advertising with this device's loaded configuration.
    ///
    /// This is the synchronous counterpart to upstream `Device.start_advertising()`
    /// when no per-call data or interval overrides are supplied.
    pub fn start_configured_advertising(&mut self, link: &mut LocalLink) -> bool {
        let Some(advertising_interval_min) =
            advertising_interval_units(self.config.advertising_interval_min)
        else {
            return false;
        };
        let Some(advertising_interval_max) =
            advertising_interval_units(self.config.advertising_interval_max)
        else {
            return false;
        };
        if advertising_interval_min > advertising_interval_max
            || self.config.advertising_data.len() > 31
            || self.config.scan_response_data.len() > 31
        {
            return false;
        }

        self.send_hci_command(
            link,
            Command::LeSetAdvertisingData {
                advertising_data: self.config.advertising_data.clone(),
            },
        );
        self.send_hci_command(
            link,
            Command::LeSetScanResponseData {
                scan_response_data: self.config.scan_response_data.clone(),
            },
        );
        self.send_hci_command(
            link,
            Command::LeSetAdvertisingParameters {
                advertising_interval_min,
                advertising_interval_max,
                advertising_type: 0,
                own_address_type: 1,
                peer_address_type: 0,
                peer_address: Address::from_bytes([0; 6], bumble::AddressType::PUBLIC_DEVICE),
                advertising_channel_map: 7,
                advertising_filter_policy: 0,
            },
        );
        self.send_hci_command(
            link,
            Command::LeSetAdvertisingEnable {
                advertising_enable: 1,
            },
        );
        self.legacy_advertising = true;
        true
    }

    pub fn stop_advertising(&mut self, link: &mut LocalLink) {
        self.legacy_advertising = false;
        self.send_hci_command(
            link,
            Command::LeSetAdvertisingEnable {
                advertising_enable: 0,
            },
        );
    }

    /// Configure and enable one extended-advertising set. Data larger than a
    /// single HCI command is fragmented with the standard first/intermediate/
    /// last operations; the controller reassembles up to Bumble's 1650-byte
    /// advertised maximum.
    pub fn start_extended_advertising(
        &mut self,
        link: &mut LocalLink,
        config: &ExtendedAdvertisingConfig,
        data: &[u8],
        scan_response_data: &[u8],
    ) -> bool {
        if data.len() > 0x0672 || scan_response_data.len() > 0x0672 || config.sid > 0x0F {
            return false;
        }
        if let Some(random_address) = config.random_address.clone() {
            self.send_hci_command(
                link,
                Command::LeSetAdvertisingSetRandomAddress {
                    advertising_handle: config.handle,
                    random_address,
                },
            );
        }
        self.send_hci_command(
            link,
            Command::LeSetExtendedAdvertisingParameters {
                advertising_handle: config.handle,
                advertising_event_properties: config.event_properties,
                primary_advertising_interval_min: config.interval_min,
                primary_advertising_interval_max: config.interval_max,
                primary_advertising_channel_map: config.channel_map,
                own_address_type: config.own_address_type,
                peer_address_type: config.peer_address_type,
                peer_address: config.peer_address.clone(),
                advertising_filter_policy: config.filter_policy,
                advertising_tx_power: config.tx_power as u8,
                primary_advertising_phy: config.primary_phy,
                secondary_advertising_max_skip: config.secondary_max_skip,
                secondary_advertising_phy: config.secondary_phy,
                advertising_sid: config.sid,
                scan_request_notification_enable: u8::from(config.scan_request_notification),
            },
        );
        self.send_extended_advertising_data(link, config.handle, data, false);
        self.send_extended_advertising_data(link, config.handle, scan_response_data, true);
        self.send_hci_command(
            link,
            Command::LeSetExtendedAdvertisingEnable {
                enable: 1,
                advertising_handles: vec![config.handle],
                durations: vec![config.duration],
                max_extended_advertising_events: vec![config.max_events],
            },
        );
        self.extended_advertising_handles.insert(config.handle);
        true
    }

    pub fn stop_extended_advertising(&mut self, link: &mut LocalLink, handle: u8) {
        self.extended_advertising_handles.remove(&handle);
        self.send_hci_command(
            link,
            Command::LeSetExtendedAdvertisingEnable {
                enable: 0,
                advertising_handles: vec![handle],
                durations: vec![0],
                max_extended_advertising_events: vec![0],
            },
        );
    }

    /// Configure, load, and enable a periodic advertising train.
    pub fn start_periodic_advertising(
        &mut self,
        link: &mut LocalLink,
        config: PeriodicAdvertisingConfig,
        data: &[u8],
    ) -> bool {
        if data.len() > 0x0672
            || config.interval_min < 0x0006
            || config.interval_min > config.interval_max
        {
            return false;
        }
        self.send_hci_command(
            link,
            Command::LeSetPeriodicAdvertisingParameters {
                advertising_handle: config.handle,
                periodic_advertising_interval_min: config.interval_min,
                periodic_advertising_interval_max: config.interval_max,
                periodic_advertising_properties: config.properties,
            },
        );
        let chunks: Vec<_> = if data.is_empty() {
            vec![&[][..]]
        } else {
            data.chunks(251).collect()
        };
        for (index, chunk) in chunks.iter().enumerate() {
            let operation = if chunks.len() == 1 {
                0x03
            } else if index == 0 {
                0x01
            } else if index + 1 == chunks.len() {
                0x02
            } else {
                0x00
            };
            self.send_hci_command(
                link,
                Command::LeSetPeriodicAdvertisingData {
                    advertising_handle: config.handle,
                    operation,
                    advertising_data: chunk.to_vec(),
                },
            );
        }
        self.send_hci_command(
            link,
            Command::LeSetPeriodicAdvertisingEnable {
                enable: 1 | (u8::from(config.include_adi) << 1),
                advertising_handle: config.handle,
            },
        );
        true
    }

    pub fn stop_periodic_advertising(&mut self, link: &mut LocalLink, handle: u8) {
        self.send_hci_command(
            link,
            Command::LeSetPeriodicAdvertisingEnable {
                enable: 0,
                advertising_handle: handle,
            },
        );
    }

    /// Begin synchronization to a periodic advertiser. Completion is reported
    /// through [`Self::periodic_syncs`] after the link carries a matching train.
    pub fn create_periodic_advertising_sync(
        &mut self,
        link: &mut LocalLink,
        advertiser_address: Address,
        advertising_sid: u8,
        skip: u16,
        sync_timeout: u16,
        filter_duplicates: bool,
    ) -> bool {
        if advertising_sid > 0x0F || skip > 0x01F3 || !(0x000A..=0x4000).contains(&sync_timeout) {
            return false;
        }
        self.send_hci_command(
            link,
            Command::LePeriodicAdvertisingCreateSync {
                options: u8::from(filter_duplicates),
                advertising_sid,
                advertiser_address_type: advertiser_address.address_type().0,
                advertiser_address,
                skip,
                sync_timeout,
                sync_cte_type: 0,
            },
        );
        true
    }

    pub fn cancel_periodic_advertising_sync(&mut self, link: &mut LocalLink) {
        self.send_hci_command(link, Command::LePeriodicAdvertisingCreateSyncCancel);
    }

    pub fn terminate_periodic_advertising_sync(&mut self, link: &mut LocalLink, sync_handle: u16) {
        self.send_hci_command(
            link,
            Command::LePeriodicAdvertisingTerminateSync { sync_handle },
        );
        self.periodic_syncs.remove(&sync_handle);
        self.periodic_report_accumulators.remove(&sync_handle);
    }

    pub fn set_periodic_advertising_receive_enabled(
        &mut self,
        link: &mut LocalLink,
        sync_handle: u16,
        enabled: bool,
    ) {
        self.send_hci_command(
            link,
            Command::LeSetPeriodicAdvertisingReceiveEnable {
                sync_handle,
                enable: u8::from(enabled),
            },
        );
    }

    pub fn transfer_periodic_advertising_sync(
        &mut self,
        link: &mut LocalLink,
        sync_handle: u16,
        service_data: u16,
    ) -> bool {
        let Some(connection_handle) = self.connection_handle else {
            return false;
        };
        self.transfer_periodic_advertising_sync_on_handle(
            link,
            connection_handle,
            sync_handle,
            service_data,
        )
    }

    pub fn transfer_periodic_advertising_sync_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        sync_handle: u16,
        service_data: u16,
    ) -> bool {
        if !self.le_connections.contains_key(&connection_handle) {
            return false;
        }
        self.send_hci_command(
            link,
            Command::LePeriodicAdvertisingSyncTransfer {
                connection_handle,
                service_data,
                sync_handle,
            },
        );
        true
    }

    pub fn transfer_periodic_advertising_set_info(
        &mut self,
        link: &mut LocalLink,
        advertising_handle: u8,
        service_data: u16,
    ) -> bool {
        let Some(connection_handle) = self.connection_handle else {
            return false;
        };
        self.transfer_periodic_advertising_set_info_on_handle(
            link,
            connection_handle,
            advertising_handle,
            service_data,
        )
    }

    pub fn transfer_periodic_advertising_set_info_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        advertising_handle: u8,
        service_data: u16,
    ) -> bool {
        if !self.le_connections.contains_key(&connection_handle) {
            return false;
        }
        self.send_hci_command(
            link,
            Command::LePeriodicAdvertisingSetInfoTransfer {
                connection_handle,
                service_data,
                advertising_handle,
            },
        );
        true
    }

    fn send_extended_advertising_data(
        &mut self,
        link: &mut LocalLink,
        handle: u8,
        data: &[u8],
        scan_response: bool,
    ) {
        let chunks: Vec<_> = if data.is_empty() {
            vec![&[][..]]
        } else {
            data.chunks(251).collect()
        };
        for (index, chunk) in chunks.iter().enumerate() {
            let operation = if chunks.len() == 1 {
                0x03
            } else if index == 0 {
                0x01
            } else if index + 1 == chunks.len() {
                0x02
            } else {
                0x00
            };
            let command = if scan_response {
                Command::LeSetExtendedScanResponseData {
                    advertising_handle: handle,
                    operation,
                    fragment_preference: 1,
                    scan_response_data: chunk.to_vec(),
                }
            } else {
                Command::LeSetExtendedAdvertisingData {
                    advertising_handle: handle,
                    operation,
                    fragment_preference: 1,
                    advertising_data: chunk.to_vec(),
                }
            };
            self.send_hci_command(link, command);
        }
    }

    pub fn start_scanning(&mut self, link: &mut LocalLink, active: bool, filter_duplicates: bool) {
        self.peer_lookup_started_scanning = None;
        self.advertisement_accumulators.clear();
        self.scanning_is_passive = !active;
        self.scanning = true;
        self.send_hci_command(
            link,
            Command::LeSetScanParameters {
                le_scan_type: u8::from(active),
                le_scan_interval: 0x0010,
                le_scan_window: 0x0010,
                own_address_type: 1,
                scanning_filter_policy: 0,
            },
        );
        self.send_hci_command(
            link,
            Command::LeSetScanEnable {
                le_scan_enable: 1,
                filter_duplicates: u8::from(filter_duplicates),
            },
        );
    }

    pub fn stop_scanning(&mut self, link: &mut LocalLink) {
        self.peer_lookup_started_scanning = None;
        self.scanning = false;
        self.send_hci_command(
            link,
            Command::LeSetScanEnable {
                le_scan_enable: 0,
                filter_duplicates: 0,
            },
        );
    }

    pub fn start_extended_scanning(
        &mut self,
        link: &mut LocalLink,
        active: bool,
        filter_duplicates: bool,
    ) {
        self.peer_lookup_started_scanning = None;
        self.advertisement_accumulators.clear();
        self.scanning_is_passive = !active;
        self.scanning = true;
        self.send_hci_command(
            link,
            Command::LeSetExtendedScanParameters {
                own_address_type: 1,
                scanning_filter_policy: 0,
                scanning_phys: 1,
                scan_types: vec![u8::from(active)],
                scan_intervals: vec![0x0010],
                scan_windows: vec![0x0010],
            },
        );
        self.send_hci_command(
            link,
            Command::LeSetExtendedScanEnable {
                enable: 1,
                filter_duplicates: u8::from(filter_duplicates),
                duration: 0,
                period: 0,
            },
        );
    }

    pub fn stop_extended_scanning(&mut self, link: &mut LocalLink) {
        self.peer_lookup_started_scanning = None;
        self.scanning = false;
        self.send_hci_command(
            link,
            Command::LeSetExtendedScanEnable {
                enable: 0,
                filter_duplicates: 0,
                duration: 0,
                period: 0,
            },
        );
    }

    pub fn take_advertising_reports(&mut self) -> Vec<AdvertisingReport> {
        std::mem::take(&mut self.advertising_reports)
    }

    pub fn take_extended_advertising_reports(&mut self) -> Vec<ExtendedAdvertisingReport> {
        std::mem::take(&mut self.extended_advertising_reports)
    }

    /// Drain high-level advertisements emitted by the per-address scan
    /// accumulators. Active scans combine an advertisement with its scan
    /// response; passive scans deliver scannable advertisements immediately.
    pub fn take_advertisements(&mut self) -> Vec<Advertisement> {
        std::mem::take(&mut self.advertisements)
    }

    fn allocate_peer_lookup_id(&mut self) -> PeerLookupId {
        let mut lookup_id = self.next_peer_lookup_id;
        while self.pending_peer_lookups.contains_key(&lookup_id) {
            lookup_id = lookup_id.wrapping_add(1).max(1);
        }
        self.next_peer_lookup_id = lookup_id.wrapping_add(1).max(1);
        lookup_id
    }

    /// Start finding a peer by Complete or Shortened Local Name.
    ///
    /// This is the runtime-neutral counterpart to upstream's awaitable
    /// `find_peer_by_name`: the method returns a stable lookup identifier,
    /// starts scanning or inquiry only when the application was not already
    /// doing so, and publishes completion through
    /// [`Self::take_peer_lookup_results`] and [`DeviceEvent::PeerFound`].
    pub fn find_peer_by_name(
        &mut self,
        link: &mut LocalLink,
        name: impl Into<String>,
        transport: PeerLookupTransport,
    ) -> PeerLookupId {
        let lookup_id = self.allocate_peer_lookup_id();
        self.pending_peer_lookups.insert(
            lookup_id,
            PeerLookupRequest::Name {
                name: name.into(),
                transport,
            },
        );

        match transport {
            PeerLookupTransport::Le if !self.scanning => {
                let extended = self.supports_le_extended_advertising();
                if extended {
                    self.start_extended_scanning(link, true, true);
                } else {
                    self.start_scanning(link, true, true);
                }
                self.peer_lookup_started_scanning = Some(extended);
            }
            PeerLookupTransport::Classic if !self.classic_discovering => {
                self.start_discovery(link, true);
                self.peer_lookup_started_discovery = true;
            }
            _ => {}
        }
        lookup_id
    }

    /// Start finding the current RPA for a bonded identity address.
    pub fn find_peer_by_identity_address(
        &mut self,
        link: &mut LocalLink,
        identity_address: Address,
    ) -> Result<PeerLookupId, PeerLookupError> {
        if self.address_resolver.is_none() {
            return Err(PeerLookupError::NoAddressResolver);
        }
        let lookup_id = self.allocate_peer_lookup_id();
        self.pending_peer_lookups
            .insert(lookup_id, PeerLookupRequest::Identity { identity_address });
        if !self.scanning {
            let extended = self.supports_le_extended_advertising();
            if extended {
                self.start_extended_scanning(link, true, true);
            } else {
                self.start_scanning(link, true, true);
            }
            self.peer_lookup_started_scanning = Some(extended);
        }
        Ok(lookup_id)
    }

    /// Cancel one pending lookup and restore lookup-owned discovery activity
    /// when it was the final lookup using that transport.
    pub fn cancel_peer_lookup(&mut self, link: &mut LocalLink, lookup_id: PeerLookupId) -> bool {
        if self.pending_peer_lookups.remove(&lookup_id).is_none() {
            return false;
        }
        self.finish_peer_lookup_activity(link);
        true
    }

    pub fn is_peer_lookup_pending(&self, lookup_id: PeerLookupId) -> bool {
        self.pending_peer_lookups.contains_key(&lookup_id)
    }

    pub fn pending_peer_lookup_count(&self) -> usize {
        self.pending_peer_lookups.len()
    }

    /// Drain completed peer lookups in discovery order.
    pub fn take_peer_lookup_results(&mut self) -> Vec<PeerLookupResult> {
        std::mem::take(&mut self.peer_lookup_results)
    }

    fn complete_peer_lookups(
        &mut self,
        link: &mut LocalLink,
        transport: PeerLookupTransport,
        peer_address: &Address,
        advertising_data: &AdvertisingData,
    ) {
        let local_name = advertising_data
            .get(bumble::advertising_data::Type::COMPLETE_LOCAL_NAME)
            .or_else(|| advertising_data.get(bumble::advertising_data::Type::SHORTENED_LOCAL_NAME));
        let resolved_address = self
            .address_resolver
            .as_ref()
            .and_then(|resolver| resolver.resolve(peer_address));
        let completed = self
            .pending_peer_lookups
            .iter()
            .filter_map(|(lookup_id, request)| {
                let matches = match request {
                    PeerLookupRequest::Name {
                        name,
                        transport: request_transport,
                    } => {
                        *request_transport == transport
                            && local_name.as_deref() == Some(name.as_bytes())
                    }
                    PeerLookupRequest::Identity { identity_address } => {
                        transport == PeerLookupTransport::Le
                            && (peer_address == identity_address
                                || resolved_address.as_ref() == Some(identity_address))
                    }
                };
                matches.then_some(*lookup_id)
            })
            .collect::<Vec<_>>();

        for lookup_id in completed {
            let request = self
                .pending_peer_lookups
                .remove(&lookup_id)
                .expect("completed lookup remains pending");
            let result = PeerLookupResult {
                lookup_id,
                transport: request.transport(),
                peer_address: peer_address.clone(),
            };
            self.peer_lookup_results.push(result.clone());
            self.emit_device_event(DeviceEvent::PeerFound(result));
        }
        self.finish_peer_lookup_activity(link);
    }

    fn finish_peer_lookup_activity(&mut self, link: &mut LocalLink) {
        let le_pending = self
            .pending_peer_lookups
            .values()
            .any(|request| request.transport() == PeerLookupTransport::Le);
        if !le_pending {
            if let Some(extended) = self.peer_lookup_started_scanning.take() {
                if extended {
                    self.stop_extended_scanning(link);
                } else {
                    self.stop_scanning(link);
                }
            }
        }

        let classic_pending = self
            .pending_peer_lookups
            .values()
            .any(|request| request.transport() == PeerLookupTransport::Classic);
        if !classic_pending && self.peer_lookup_started_discovery {
            self.peer_lookup_started_discovery = false;
            self.stop_discovery(link);
        }
    }

    pub fn periodic_syncs(&self) -> &BTreeMap<u16, PeriodicAdvertisingSyncInfo> {
        &self.periodic_syncs
    }

    pub fn take_periodic_advertisements(&mut self) -> Vec<PeriodicAdvertisement> {
        std::mem::take(&mut self.periodic_advertisements)
    }

    pub fn take_periodic_sync_errors(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.periodic_sync_errors)
    }

    pub fn take_lost_periodic_syncs(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.lost_periodic_syncs)
    }

    pub fn take_periodic_sync_transfers(&mut self) -> Vec<PeriodicAdvertisingSyncTransferInfo> {
        std::mem::take(&mut self.periodic_sync_transfers)
    }

    pub fn connect_le(&mut self, link: &mut LocalLink, peer_address: Address) -> bool {
        if self.le_connecting {
            return false;
        }
        self.le_connecting = true;
        self.send_hci_command(
            link,
            Command::LeCreateConnection {
                le_scan_interval: 0x0010,
                le_scan_window: 0x0010,
                initiator_filter_policy: 0,
                peer_address_type: u8::from(!peer_address.is_public()),
                peer_address,
                own_address_type: 1,
                connection_interval_min: 24,
                connection_interval_max: 40,
                max_latency: 0,
                supervision_timeout: 42,
                min_ce_length: 0,
                max_ce_length: 0,
            },
        );
        link.establish_connections();
        true
    }

    pub fn connect_le_extended(&mut self, link: &mut LocalLink, peer_address: Address) -> bool {
        if self.le_connecting {
            return false;
        }
        self.le_connecting = true;
        self.send_hci_command(
            link,
            Command::LeExtendedCreateConnection {
                initiator_filter_policy: 0,
                own_address_type: 1,
                peer_address_type: u8::from(!peer_address.is_public()),
                peer_address,
                initiating_phys: 1,
                scan_intervals: vec![0x0010],
                scan_windows: vec![0x0010],
                connection_interval_mins: vec![24],
                connection_interval_maxs: vec![40],
                max_latencies: vec![0],
                supervision_timeouts: vec![42],
                min_ce_lengths: vec![0],
                max_ce_lengths: vec![0],
            },
        );
        link.establish_connections();
        true
    }

    /// Cancel a pending LE connection attempt. Returns `false` when no LE
    /// connection procedure is active.
    pub fn cancel_le_connection(&mut self, link: &mut LocalLink) -> bool {
        if !self.le_connecting {
            return false;
        }
        self.send_hci_command(link, Command::LeCreateConnectionCancel);
        self.le_connecting = false;
        true
    }

    /// Submit the BR/EDR Create Connection Cancel command for a peer. As in
    /// upstream Bumble, the controller decides whether that peer is pending.
    pub fn cancel_classic_connection(&mut self, link: &mut LocalLink, peer_address: Address) {
        self.send_hci_command(
            link,
            Command::CreateConnectionCancel {
                bd_addr: peer_address,
            },
        );
    }

    pub fn is_encrypted(&self) -> bool {
        self.connection_handle
            .is_some_and(|handle| self.encrypted_handles.contains(&handle))
    }

    pub fn is_encrypted_on_handle(&self, connection_handle: u16) -> bool {
        self.le_connections.contains_key(&connection_handle)
            && self.encrypted_handles.contains(&connection_handle)
    }

    pub fn is_classic_encrypted(&self) -> bool {
        self.classic_connection_handle
            .is_some_and(|handle| self.encrypted_handles.contains(&handle))
    }

    pub fn is_classic_encrypted_on_handle(&self, connection_handle: u16) -> bool {
        self.classic_connections.contains_key(&connection_handle)
            && self.encrypted_handles.contains(&connection_handle)
    }

    /// Enable LE encryption with a pairing-derived STK/LTK. The peer receives
    /// the corresponding LL encryption request through the virtual link.
    pub fn enable_encryption(&mut self, link: &mut LocalLink, key: [u8; 16]) -> bool {
        self.enable_encryption_with_parameters(link, key, 0, [0; 8])
    }

    /// Enable LE encryption from a persisted Legacy or SC bond. Legacy bonds
    /// preserve their EDIV/RAND metadata; SC bonds pass zero values.
    pub fn enable_encryption_with_parameters(
        &mut self,
        link: &mut LocalLink,
        key: [u8; 16],
        encrypted_diversifier: u16,
        random_number: [u8; 8],
    ) -> bool {
        let Some(connection_handle) = self.connection_handle else {
            return false;
        };
        self.enable_encryption_with_parameters_on_handle(
            link,
            connection_handle,
            key,
            encrypted_diversifier,
            random_number,
        )
    }

    pub fn enable_encryption_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        key: [u8; 16],
    ) -> bool {
        self.enable_encryption_with_parameters_on_handle(link, connection_handle, key, 0, [0; 8])
    }

    pub fn enable_encryption_with_parameters_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        key: [u8; 16],
        encrypted_diversifier: u16,
        random_number: [u8; 8],
    ) -> bool {
        if !self.le_connections.contains_key(&connection_handle) {
            return false;
        }
        self.send_hci_command(
            link,
            Command::LeEnableEncryption {
                connection_handle,
                random_number,
                encrypted_diversifier,
                long_term_key: key,
            },
        );
        true
    }

    /// Answer a controller LTK request with pairing or bond-derived key
    /// material.
    pub fn reply_long_term_key_request(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        long_term_key: [u8; 16],
    ) -> bool {
        if !self.le_connections.contains_key(&connection_handle) {
            return false;
        }
        self.send_hci_command(
            link,
            Command::LeLongTermKeyRequestReply {
                connection_handle,
                long_term_key,
            },
        );
        true
    }

    /// Reject a controller LTK request when no suitable key is available.
    pub fn reject_long_term_key_request(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
    ) -> bool {
        if !self.le_connections.contains_key(&connection_handle) {
            return false;
        }
        self.send_hci_command(
            link,
            Command::LeLongTermKeyRequestNegativeReply { connection_handle },
        );
        true
    }

    /// Program the controller resolving list from `KeyStore::get_resolving_keys`.
    /// Invalid-length IRKs are skipped; the returned count is the number loaded.
    pub fn configure_address_resolution(
        &mut self,
        link: &mut LocalLink,
        resolving_keys: &[(Vec<u8>, Address)],
        local_irk: [u8; 16],
    ) -> usize {
        self.send_hci_command(link, Command::LeClearResolvingList);
        let mut loaded = 0;
        for (peer_irk, identity) in resolving_keys {
            let Ok(peer_irk) = peer_irk.as_slice().try_into() else {
                continue;
            };
            self.send_hci_command(
                link,
                Command::LeAddDeviceToResolvingList {
                    peer_identity_address_type: u8::from(!identity.is_public()),
                    peer_identity_address: identity.clone(),
                    peer_irk,
                    local_irk,
                },
            );
            loaded += 1;
        }
        self.send_hci_command(
            link,
            Command::LeSetAddressResolutionEnable {
                address_resolution_enable: u8::from(loaded != 0),
            },
        );
        loaded
    }

    pub fn classic_connection_handle(&self) -> Option<u16> {
        self.classic_connection_handle
    }

    pub fn classic_connections(&self) -> impl Iterator<Item = &ClassicConnectionInfo> {
        self.classic_connections.values()
    }

    pub fn classic_connection(&self, connection_handle: u16) -> Option<&ClassicConnectionInfo> {
        self.classic_connections.get(&connection_handle)
    }

    pub fn classic_connection_handle_for_peer(&self, peer_address: &Address) -> Option<u16> {
        self.classic_connections
            .values()
            .find(|connection| connection.peer_address == *peer_address)
            .map(|connection| connection.connection_handle)
    }

    pub fn classic_channel(
        &self,
        connection_handle: u16,
        source_cid: u16,
    ) -> Option<&ClassicChannel> {
        self.classic_channel_managers
            .get(&connection_handle)?
            .channel(source_cid)
    }

    pub fn register_classic_channel_server(
        &mut self,
        psm: Option<u32>,
        spec: ClassicChannelSpec,
    ) -> bumble_l2cap::Result<u32> {
        let mut registry = ClassicChannelManager::new();
        for (registered_psm, registered_spec) in &self.classic_channel_server_specs {
            registry.register_server(Some(*registered_psm), *registered_spec)?;
        }
        let psm = registry.register_server(psm, spec)?;
        for manager in self.classic_channel_managers.values_mut() {
            manager.register_server(Some(psm), spec)?;
        }
        self.classic_channel_server_specs.insert(psm, spec);
        Ok(psm)
    }

    pub fn unregister_classic_channel_server(&mut self, psm: u32) -> bool {
        let removed = self.classic_channel_server_specs.remove(&psm).is_some();
        for manager in self.classic_channel_managers.values_mut() {
            manager.unregister_server(psm);
        }
        removed
    }

    pub fn connect_classic_channel(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        psm: u32,
        spec: ClassicChannelSpec,
    ) -> bumble_l2cap::Result<u16> {
        let source_cid = self
            .classic_channel_manager_mut(connection_handle)?
            .connect(psm, spec)?;
        self.flush_classic_channel_manager(link, connection_handle)?;
        Ok(source_cid)
    }

    pub fn take_accepted_classic_channels(&mut self, connection_handle: u16) -> Vec<u16> {
        let Some(manager) = self.classic_channel_managers.get_mut(&connection_handle) else {
            return Vec::new();
        };
        std::iter::from_fn(|| manager.poll_accepted_channel()).collect()
    }

    pub fn send_classic_channel_sdu(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        source_cid: u16,
        data: &[u8],
    ) -> bumble_l2cap::Result<()> {
        self.classic_channel_manager_mut(connection_handle)?
            .send(source_cid, data)?;
        self.flush_classic_channel_manager(link, connection_handle)
    }

    pub fn take_classic_channel_sdus(
        &mut self,
        connection_handle: u16,
        source_cid: u16,
    ) -> Vec<Vec<u8>> {
        let Some(channel) = self
            .classic_channel_managers
            .get_mut(&connection_handle)
            .and_then(|manager| manager.channel_mut(source_cid))
        else {
            return Vec::new();
        };
        std::iter::from_fn(|| channel.pop_received()).collect()
    }

    pub fn disconnect_classic_channel(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        source_cid: u16,
    ) -> bumble_l2cap::Result<()> {
        self.classic_channel_manager_mut(connection_handle)?
            .disconnect(source_cid)?;
        self.flush_classic_channel_manager(link, connection_handle)
    }

    pub fn classic_channel_output_is_drained(&self, connection_handle: u16) -> bool {
        self.acl_output_is_drained(connection_handle)
    }

    /// Whether all host-to-controller ACL packets queued for this connection
    /// have been acknowledged by controller flow control.
    pub fn acl_output_is_drained(&self, connection_handle: u16) -> bool {
        if self.le_connections.contains_key(&connection_handle) {
            if let Some(queue) = self.le_acl_packet_queue.as_ref() {
                return queue.is_drained(connection_handle);
            }
        }
        self.acl_packet_queue.is_drained(connection_handle)
    }

    pub fn take_classic_channel_errors(&mut self) -> Vec<(u16, String)> {
        std::mem::take(&mut self.classic_channel_errors)
    }

    pub fn select_classic_connection(&mut self, connection_handle: u16) -> bool {
        let Some(connection) = self.classic_connections.get(&connection_handle).cloned() else {
            return false;
        };
        self.classic_connection_handle = Some(connection.connection_handle);
        self.classic_connection_role = Some(connection.role);
        true
    }

    /// The local role on the established Classic ACL (`0` Central, `1` Peripheral).
    pub fn classic_connection_role(&self) -> Option<u8> {
        self.classic_connection_role
    }

    pub fn take_classic_connection_requests(&mut self) -> Vec<Address> {
        std::mem::take(&mut self.classic_connection_requests)
    }

    pub fn take_classic_inquiry_results(&mut self) -> Vec<Address> {
        std::mem::take(&mut self.classic_inquiry_results)
    }

    pub fn take_classic_inquiry_result_details(&mut self) -> Vec<ClassicInquiryResultInfo> {
        std::mem::take(&mut self.classic_inquiry_result_details)
    }

    pub fn take_classic_inquiry_complete(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.classic_inquiry_complete)
    }

    /// Start BR/EDR inquiry using upstream Bumble's extended-result mode,
    /// General Inquiry LAP, 10.24-second inquiry length, and unlimited results.
    ///
    /// When `auto_restart` is true, each Inquiry Complete event submits the
    /// same two-command sequence again. This retains upstream's discovery loop
    /// without binding the synchronous host to an async runtime.
    pub fn start_discovery(&mut self, link: &mut LocalLink, auto_restart: bool) {
        self.peer_lookup_started_discovery = false;
        self.send_hci_command(link, Command::WriteInquiryMode { inquiry_mode: 2 });
        self.classic_discovering = false;
        self.send_hci_command(
            link,
            Command::Inquiry {
                lap: 0x009E_8B33,
                inquiry_length: 8,
                num_responses: 0,
            },
        );
        self.classic_auto_restart_inquiry = auto_restart;
        self.classic_discovering = true;
    }

    /// Stop BR/EDR inquiry if one is active and restore the default automatic
    /// restart policy for the next discovery run.
    pub fn stop_discovery(&mut self, link: &mut LocalLink) {
        self.peer_lookup_started_discovery = false;
        if self.classic_discovering {
            self.send_hci_command(link, Command::InquiryCancel);
        }
        self.classic_auto_restart_inquiry = true;
        self.classic_discovering = false;
    }

    pub fn is_discovering(&self) -> bool {
        self.classic_discovering
    }

    pub fn discovery_auto_restart_enabled(&self) -> bool {
        self.classic_auto_restart_inquiry
    }

    /// The 240-byte Extended Inquiry Response used for Classic discovery.
    pub fn classic_inquiry_response(&self) -> Option<&[u8; 240]> {
        self.classic_inquiry_response.as_ref()
    }

    /// Override or clear the Extended Inquiry Response retained by this device.
    /// Clearing it makes the next discoverability update synthesize a Complete
    /// Local Name field from the configured device name.
    pub fn set_classic_inquiry_response(&mut self, response: Option<[u8; 240]>) {
        self.classic_inquiry_response = response;
    }

    /// Update the controller's Classic inquiry/page scan bits directly.
    pub fn set_classic_scan_enable(
        &mut self,
        link: &mut LocalLink,
        inquiry_scan_enabled: bool,
        page_scan_enabled: bool,
    ) {
        let scan_enable = u8::from(inquiry_scan_enabled) | (u8::from(page_scan_enabled) << 1);
        self.send_hci_command(link, Command::WriteScanEnable { scan_enable });
    }

    /// Update configured Classic discoverability and its Extended Inquiry
    /// Response, then apply the combined inquiry/page scan state.
    pub fn set_discoverable(
        &mut self,
        link: &mut LocalLink,
        discoverable: bool,
    ) -> Result<(), DevicePowerError> {
        self.config.discoverable = discoverable;
        if !self.config.classic_enabled {
            return Ok(());
        }

        let response = match self.classic_inquiry_response {
            Some(response) => response,
            None => {
                let response = default_inquiry_response(&self.config.name)?;
                self.classic_inquiry_response = Some(response);
                response
            }
        };
        self.send_hci_command(
            link,
            Command::WriteExtendedInquiryResponse {
                fec_required: 0,
                extended_inquiry_response: response,
            },
        );
        self.set_classic_scan_enable(link, self.config.discoverable, self.config.connectable);
        Ok(())
    }

    /// Update configured Classic page-scan connectability and apply the
    /// combined inquiry/page scan state.
    pub fn set_connectable(&mut self, link: &mut LocalLink, connectable: bool) {
        self.config.connectable = connectable;
        if self.config.classic_enabled {
            self.set_classic_scan_enable(link, self.config.discoverable, self.config.connectable);
        }
    }

    /// Request a Classic peer's user-friendly name by address.
    ///
    /// This is the event-driven counterpart to upstream
    /// `Device.request_remote_name(Address)`. Completion is reported through
    /// [`DeviceEvent::RemoteName`], [`DeviceEvent::RemoteNameFailure`], and
    /// [`Self::take_classic_remote_name_results`].
    pub fn request_remote_name(&mut self, link: &mut LocalLink, peer_address: Address) {
        self.pending_remote_name_commands
            .push_back(peer_address.clone());
        if let Some((_, count)) = self
            .pending_remote_name_requests
            .iter_mut()
            .find(|(address, _)| *address == peer_address)
        {
            *count += 1;
        } else {
            self.pending_remote_name_requests
                .push((peer_address.clone(), 1));
        }
        self.send_hci_command(
            link,
            Command::RemoteNameRequest {
                bd_addr: peer_address,
                page_scan_repetition_mode: 2,
                reserved: 0,
                clock_offset: 0,
            },
        );
    }

    /// Request the name of one established Classic ACL peer.
    pub fn request_remote_name_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
    ) -> bool {
        let Some(peer_address) = self
            .classic_connections
            .get(&connection_handle)
            .map(|connection| connection.peer_address.clone())
        else {
            return false;
        };
        self.request_remote_name(link, peer_address);
        true
    }

    pub fn is_remote_name_pending(&self, peer_address: &Address) -> bool {
        self.pending_remote_name_requests
            .iter()
            .any(|(address, _)| address == peer_address)
    }

    pub fn pending_remote_name_count(&self) -> usize {
        self.pending_remote_name_requests
            .iter()
            .map(|(_, count)| count)
            .sum()
    }

    pub fn take_classic_remote_names(&mut self) -> Vec<(u8, Address, String)> {
        std::mem::take(&mut self.classic_remote_names)
    }

    /// Drain successful and failed Classic remote-name completions in order.
    pub fn take_classic_remote_name_results(&mut self) -> Vec<RemoteNameResult> {
        std::mem::take(&mut self.classic_remote_name_results)
    }

    fn remove_pending_remote_name_command(&mut self, peer_address: &Address) {
        if let Some(index) = self
            .pending_remote_name_commands
            .iter()
            .position(|address| address == peer_address)
        {
            self.pending_remote_name_commands.remove(index);
        }
    }

    fn complete_remote_name_request(&mut self, peer_address: &Address) {
        let Some(index) = self
            .pending_remote_name_requests
            .iter()
            .position(|(address, _)| address == peer_address)
        else {
            return;
        };
        let count = &mut self.pending_remote_name_requests[index].1;
        *count -= 1;
        if *count == 0 {
            self.pending_remote_name_requests.remove(index);
        }
    }

    fn record_remote_name_result(
        &mut self,
        peer_address: Address,
        result: Result<String, RemoteNameError>,
    ) {
        let completion = RemoteNameResult {
            peer_address: peer_address.clone(),
            result: result.clone(),
        };
        self.classic_remote_name_results.push(completion);
        match result {
            Ok(name) => {
                for connection in self
                    .classic_connections
                    .values_mut()
                    .filter(|connection| connection.peer_address == peer_address)
                {
                    connection.peer_name = Some(name.clone());
                }
                self.classic_remote_names
                    .push((0, peer_address.clone(), name.clone()));
                self.emit_device_event(DeviceEvent::RemoteName {
                    status: 0,
                    peer_address,
                    name,
                });
            }
            Err(error) => {
                self.emit_device_event(DeviceEvent::RemoteNameFailure {
                    peer_address,
                    error,
                });
            }
        }
    }

    pub fn authenticate_classic_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
    ) -> bool {
        if !self.classic_connections.contains_key(&connection_handle) {
            return false;
        }
        self.send_hci_command(link, Command::AuthenticationRequested { connection_handle });
        true
    }

    /// Remove all pending Classic PIN/SSP authentication events.
    pub fn take_classic_pairing_events(&mut self) -> Vec<ClassicPairingEvent> {
        std::mem::take(&mut self.classic_pairing_events)
    }

    /// Remove Classic pairing events belonging to one ACL while preserving
    /// concurrent sessions.
    pub fn take_classic_pairing_events_for(
        &mut self,
        connection_handle: u16,
        peer_address: &Address,
    ) -> Vec<ClassicPairingEvent> {
        let (matching, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.classic_pairing_events)
            .into_iter()
            .partition(|event| event.belongs_to(connection_handle, peer_address));
        self.classic_pairing_events = rest;
        matching
    }

    pub fn set_classic_encryption(&mut self, link: &mut LocalLink, enabled: bool) -> bool {
        let Some(connection_handle) = self.classic_connection_handle else {
            return false;
        };
        self.set_classic_encryption_on_handle(link, connection_handle, enabled)
    }

    pub fn set_classic_encryption_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        enabled: bool,
    ) -> bool {
        if !self.classic_connections.contains_key(&connection_handle) {
            return false;
        }
        self.send_hci_command(
            link,
            Command::SetConnectionEncryption {
                connection_handle,
                encryption_enable: u8::from(enabled),
            },
        );
        true
    }

    pub fn synchronous_connections(&self) -> &[SynchronousConnectionInfo] {
        &self.synchronous_connections
    }

    pub fn take_synchronous_requests(&mut self) -> Vec<(Address, u8)> {
        std::mem::take(&mut self.synchronous_requests)
    }

    pub fn take_synchronous_inbox(&mut self) -> Vec<SynchronousDataPacket> {
        std::mem::take(&mut self.synchronous_inbox)
    }

    /// Configure a CIG using Bumble's default per-CIS QoS values. The allocated
    /// CIS handles become available through [`Device::take_configured_cis_handles`]
    /// after [`pump`].
    pub fn configure_cig(&mut self, link: &mut LocalLink, cig_id: u8, cis_ids: &[u8]) -> bool {
        let parameters = CigParameters::new(
            cig_id,
            cis_ids.iter().copied().map(CisParameters::new).collect(),
            10_000,
            10_000,
        );
        self.configure_cig_with_parameters(link, &parameters)
    }

    /// Configure a CIG with the complete upstream `CigParameters` surface.
    pub fn configure_cig_with_parameters(
        &mut self,
        link: &mut LocalLink,
        parameters: &CigParameters,
    ) -> bool {
        if parameters.cis_parameters.is_empty()
            || parameters.cis_parameters.len() > u8::MAX as usize
            || parameters.sdu_interval_c_to_p > 0x00FF_FFFF
            || parameters.sdu_interval_p_to_c > 0x00FF_FFFF
        {
            return false;
        }
        let cis_parameters = parameters
            .cis_parameters
            .iter()
            .copied()
            .map(CisParameters::normalized)
            .collect::<Vec<_>>();
        self.send_hci_command(
            link,
            Command::LeSetCigParameters {
                cig_id: parameters.cig_id,
                sdu_interval_c_to_p: parameters.sdu_interval_c_to_p,
                sdu_interval_p_to_c: parameters.sdu_interval_p_to_c,
                worst_case_sca: parameters.worst_case_sca,
                packing: parameters.packing,
                framing: parameters.framing,
                max_transport_latency_c_to_p: parameters.max_transport_latency_c_to_p,
                max_transport_latency_p_to_c: parameters.max_transport_latency_p_to_c,
                cis_id: cis_parameters.iter().map(|cis| cis.cis_id).collect(),
                max_sdu_c_to_p: cis_parameters
                    .iter()
                    .map(|cis| cis.max_sdu_c_to_p)
                    .collect(),
                max_sdu_p_to_c: cis_parameters
                    .iter()
                    .map(|cis| cis.max_sdu_p_to_c)
                    .collect(),
                phy_c_to_p: cis_parameters.iter().map(|cis| cis.phy_c_to_p).collect(),
                phy_p_to_c: cis_parameters.iter().map(|cis| cis.phy_p_to_c).collect(),
                rtn_c_to_p: cis_parameters.iter().map(|cis| cis.rtn_c_to_p).collect(),
                rtn_p_to_c: cis_parameters.iter().map(|cis| cis.rtn_p_to_c).collect(),
            },
        );
        true
    }

    pub fn take_configured_cis_handles(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.configured_cis_handles)
    }

    pub fn create_cis(&mut self, link: &mut LocalLink, cis_handle: u16) -> bool {
        let Some(acl_handle) = self.connection_handle else {
            return false;
        };
        self.create_cis_on_handle(link, acl_handle, cis_handle)
    }

    pub fn create_cis_on_handle(
        &mut self,
        link: &mut LocalLink,
        acl_handle: u16,
        cis_handle: u16,
    ) -> bool {
        self.create_cis_pairs(link, &[(cis_handle, acl_handle)])
    }

    /// Establish one or more configured CIS handles in a single HCI command.
    /// Each tuple is `(cis_connection_handle, acl_connection_handle)`.
    pub fn create_cis_pairs(&mut self, link: &mut LocalLink, cis_acl_pairs: &[(u16, u16)]) -> bool {
        if cis_acl_pairs.is_empty()
            || cis_acl_pairs.len() > u8::MAX as usize
            || cis_acl_pairs
                .iter()
                .any(|(_, acl_handle)| !self.le_connections.contains_key(acl_handle))
        {
            return false;
        }
        self.send_hci_command(
            link,
            Command::LeCreateCis {
                cis_connection_handle: cis_acl_pairs.iter().map(|pair| pair.0).collect(),
                acl_connection_handle: cis_acl_pairs.iter().map(|pair| pair.1).collect(),
            },
        );
        true
    }

    pub fn take_cis_requests(&mut self) -> Vec<CisRequestInfo> {
        std::mem::take(&mut self.cis_requests)
    }

    pub fn accept_cis(&mut self, link: &mut LocalLink, cis_handle: u16) {
        self.send_hci_command(
            link,
            Command::LeAcceptCisRequest {
                connection_handle: cis_handle,
            },
        );
    }

    /// Reject an incoming CIS request with the supplied HCI reason code.
    pub fn reject_cis(&mut self, link: &mut LocalLink, cis_handle: u16, reason: u8) {
        self.send_hci_command(
            link,
            Command::LeRejectCisRequest {
                connection_handle: cis_handle,
                reason,
            },
        );
    }

    pub fn established_cis_handles(&self) -> impl Iterator<Item = u16> + '_ {
        self.cis_links.keys().copied()
    }

    pub fn cis_link(&self, connection_handle: u16) -> Option<&CisLinkInfo> {
        self.cis_links.get(&connection_handle)
    }

    pub fn take_cis_control_events(&mut self) -> Vec<CisControlEvent> {
        std::mem::take(&mut self.cis_control_events)
    }

    /// Create a BIG attached to an active periodic advertising set. BIS handles
    /// become available through [`Self::big_bis_handles`] after polling.
    pub fn create_big(&mut self, link: &mut LocalLink, parameters: BigParameters) -> bool {
        if parameters.big_handle > 0xEF
            || self.bigs.contains_key(&parameters.big_handle)
            || self.pending_bigs.contains(&parameters.big_handle)
            || self.big_syncs.contains_key(&parameters.big_handle)
            || self.pending_big_syncs.contains(&parameters.big_handle)
            || parameters.num_bis == 0
            || parameters.num_bis > 31
            || !(0xFF..=0x00FF_FFFF).contains(&parameters.sdu_interval)
            || !(1..=0x0FFF).contains(&parameters.max_sdu)
            || !(5..=4_000).contains(&parameters.max_transport_latency)
            || parameters.rtn > 0x1E
            || parameters.phy == 0
            || parameters.phy & !0x07 != 0
            || parameters.packing > 1
            || parameters.framing > 1
        {
            return false;
        }
        self.pending_bigs.insert(parameters.big_handle);
        self.pending_big_commands.push_back(parameters.big_handle);
        let encrypted = parameters.broadcast_code.is_some();
        self.send_hci_command(
            link,
            Command::LeCreateBig {
                big_handle: parameters.big_handle,
                advertising_handle: parameters.advertising_handle,
                num_bis: parameters.num_bis,
                sdu_interval: parameters.sdu_interval,
                max_sdu: parameters.max_sdu,
                max_transport_latency: parameters.max_transport_latency,
                rtn: parameters.rtn,
                phy: parameters.phy,
                packing: parameters.packing,
                framing: parameters.framing,
                encryption: u8::from(encrypted),
                broadcast_code: parameters.broadcast_code.unwrap_or([0; 16]),
            },
        );
        true
    }

    pub fn terminate_big(&mut self, link: &mut LocalLink, big_handle: u8, reason: u8) -> bool {
        if !self.bigs.contains_key(&big_handle) {
            return false;
        }
        self.send_hci_command(link, Command::LeTerminateBig { big_handle, reason });
        true
    }

    pub fn big_bis_handles(&self, big_handle: u8) -> Option<&[u16]> {
        self.bigs.get(&big_handle).map(Vec::as_slice)
    }

    pub fn is_big_pending(&self, big_handle: u8) -> bool {
        self.pending_bigs.contains(&big_handle)
    }

    /// Start receiver synchronization to selected BIS indices. The periodic
    /// sync must already exist and BIGInfo must subsequently arrive over it.
    pub fn create_big_sync(&mut self, link: &mut LocalLink, parameters: BigSyncParameters) -> bool {
        let mut unique_bis = parameters.bis.clone();
        unique_bis.sort_unstable();
        unique_bis.dedup();
        if parameters.big_handle > 0xEF
            || self.bigs.contains_key(&parameters.big_handle)
            || self.pending_bigs.contains(&parameters.big_handle)
            || self.big_syncs.contains_key(&parameters.big_handle)
            || self.pending_big_syncs.contains(&parameters.big_handle)
            || !self.periodic_syncs.contains_key(&parameters.sync_handle)
            || parameters.bis.is_empty()
            || parameters.bis.len() > 31
            || parameters.bis.iter().any(|index| !(1..=31).contains(index))
            || unique_bis.len() != parameters.bis.len()
            || parameters.mse > 0x1F
            || !(0x000A..=0x4000).contains(&parameters.big_sync_timeout)
        {
            return false;
        }
        self.pending_big_syncs.insert(parameters.big_handle);
        self.pending_big_sync_commands
            .push_back(parameters.big_handle);
        let encrypted = parameters.broadcast_code.is_some();
        self.send_hci_command(
            link,
            Command::LeBigCreateSync {
                big_handle: parameters.big_handle,
                sync_handle: parameters.sync_handle,
                encryption: u8::from(encrypted),
                broadcast_code: parameters.broadcast_code.unwrap_or([0; 16]),
                mse: parameters.mse,
                big_sync_timeout: parameters.big_sync_timeout,
                bis: parameters.bis,
            },
        );
        true
    }

    pub fn terminate_big_sync(&mut self, link: &mut LocalLink, big_handle: u8) -> bool {
        if !self.big_syncs.contains_key(&big_handle) {
            return false;
        }
        self.send_hci_command(link, Command::LeBigTerminateSync { big_handle });
        true
    }

    pub fn big_sync_bis_handles(&self, big_handle: u8) -> Option<&[u16]> {
        self.big_syncs.get(&big_handle).map(Vec::as_slice)
    }

    pub fn is_big_sync_pending(&self, big_handle: u8) -> bool {
        self.pending_big_syncs.contains(&big_handle)
    }

    pub fn established_bis_handles(&self) -> impl Iterator<Item = u16> + '_ {
        self.bis_directions.keys().copied()
    }

    pub fn take_biginfo_reports(&mut self) -> Vec<BigInfoReport> {
        std::mem::take(&mut self.biginfo_reports)
    }

    pub fn take_big_errors(&mut self) -> Vec<(u8, u8)> {
        std::mem::take(&mut self.big_errors)
    }

    pub fn take_terminated_bigs(&mut self) -> Vec<(u8, u8)> {
        std::mem::take(&mut self.terminated_bigs)
    }

    pub fn setup_iso_data_path(
        &mut self,
        link: &mut LocalLink,
        iso_handle: u16,
        direction: u8,
    ) -> bool {
        self.setup_iso_data_path_with_parameters(
            link,
            iso_handle,
            IsoDataPathParameters::hci(direction),
        )
    }

    /// Configure an ISO data path with the codec and controller-delay fields
    /// exposed by upstream Bumble's `_IsoLink.setup_data_path`.
    pub fn setup_iso_data_path_with_parameters(
        &mut self,
        link: &mut LocalLink,
        iso_handle: u16,
        parameters: IsoDataPathParameters,
    ) -> bool {
        let direction = parameters.direction;
        let established = self.cis_links.contains_key(&iso_handle)
            || self.bis_directions.get(&iso_handle) == Some(&direction);
        if !established
            || direction > 1
            || parameters.controller_delay > 0x00FF_FFFF
            || parameters.codec_configuration.len() > u8::MAX as usize
        {
            return false;
        }
        let key = (iso_handle, direction);
        if self.iso_data_paths.contains_key(&key)
            || self
                .pending_iso_data_path_setups
                .iter()
                .any(|(handle, pending)| *handle == iso_handle && pending.direction == direction)
        {
            return true;
        }
        self.pending_iso_data_path_setups
            .push_back((iso_handle, parameters.clone()));
        self.send_hci_command(
            link,
            Command::LeSetupIsoDataPath {
                connection_handle: iso_handle,
                data_path_direction: direction,
                data_path_id: parameters.data_path_id,
                codec_id: parameters.codec_id,
                controller_delay: parameters.controller_delay,
                codec_configuration: parameters.codec_configuration,
            },
        );
        true
    }

    pub fn remove_iso_data_path(
        &mut self,
        link: &mut LocalLink,
        iso_handle: u16,
        directions: u8,
    ) -> bool {
        let established = self.cis_links.contains_key(&iso_handle)
            || self.bis_directions.contains_key(&iso_handle);
        if !established || directions & !0x03 != 0 || directions == 0 {
            return false;
        }
        let installed_directions = (0..=1).fold(0, |mask, direction| {
            if directions & (1 << direction) != 0
                && self.iso_data_paths.contains_key(&(iso_handle, direction))
            {
                mask | (1 << direction)
            } else {
                mask
            }
        });
        if installed_directions == 0
            || self
                .pending_iso_data_path_removals
                .iter()
                .any(|(handle, pending)| *handle == iso_handle && *pending == installed_directions)
        {
            return true;
        }
        self.pending_iso_data_path_removals
            .push_back((iso_handle, installed_directions));
        self.send_hci_command(
            link,
            Command::LeRemoveIsoDataPath {
                connection_handle: iso_handle,
                data_path_direction: installed_directions,
            },
        );
        true
    }

    /// Request synchronization metadata for the most recently transmitted ISO
    /// SDU on an established CIS or BIS.
    pub fn read_iso_tx_sync(&mut self, link: &mut LocalLink, iso_handle: u16) -> bool {
        let established = self.cis_links.contains_key(&iso_handle)
            || self.bis_directions.contains_key(&iso_handle);
        if !established {
            return false;
        }
        self.pending_iso_tx_syncs.push_back(iso_handle);
        self.send_hci_command(
            link,
            Command::LeReadIsoTxSync {
                connection_handle: iso_handle,
            },
        );
        true
    }

    pub fn iso_data_path(&self, iso_handle: u16, direction: u8) -> Option<&IsoDataPathParameters> {
        self.iso_data_paths.get(&(iso_handle, direction))
    }

    pub fn iso_tx_sync(&self, iso_handle: u16) -> Option<&IsoTxSyncInfo> {
        self.iso_tx_syncs.get(&iso_handle)
    }

    pub fn take_iso_control_events(&mut self) -> Vec<IsoControlEvent> {
        std::mem::take(&mut self.iso_control_events)
    }

    /// Fragment and send one ISO SDU through an established CIS or broadcaster
    /// BIS. Reset-time V2 buffer discovery supplies the packet size and flow
    /// window; the software-controller default remains the pre-reset fallback.
    pub fn send_iso_sdu(&mut self, link: &mut LocalLink, iso_handle: u16, sdu: &[u8]) -> bool {
        const DEFAULT_ISO_PACKET_LENGTH: usize = 960;
        const SDU_INFO_LENGTH: usize = 4;
        let can_send = self.cis_links.contains_key(&iso_handle)
            || self.bis_directions.get(&iso_handle) == Some(&0);
        if !can_send || sdu.len() > 0x0FFF {
            return false;
        }
        let packet_length = self
            .iso_data_packet_length
            .unwrap_or(DEFAULT_ISO_PACKET_LENGTH);
        if packet_length <= SDU_INFO_LENGTH {
            return false;
        }
        let sequence = *self.iso_sequence_numbers.entry(iso_handle).or_default();
        let mut offset = 0;
        loop {
            let first = offset == 0;
            let capacity = packet_length - if first { SDU_INFO_LENGTH } else { 0 };
            let end = (offset + capacity).min(sdu.len());
            let last = end == sdu.len();
            let fragment = sdu[offset..end].to_vec();
            let packet = IsoDataPacket {
                connection_handle: iso_handle,
                pb_flag: match (first, last) {
                    (true, true) => 0b10,
                    (true, false) => 0b00,
                    (false, true) => 0b11,
                    (false, false) => 0b01,
                },
                ts_flag: 0,
                data_total_length: (fragment.len() + if first { SDU_INFO_LENGTH } else { 0 })
                    as u16,
                time_stamp: None,
                packet_sequence_number: first.then_some(sequence),
                iso_sdu_length: first.then_some(sdu.len() as u16),
                packet_status_flag: first.then_some(0),
                iso_sdu_fragment: fragment,
            };
            if let Some(queue) = self.iso_packet_queue.as_mut() {
                queue.enqueue(packet, iso_handle);
            } else if !link.send_iso_packet(self.controller_id, packet) {
                return false;
            }
            if last {
                break;
            }
            offset = end;
        }
        self.iso_sequence_numbers
            .insert(iso_handle, sequence.wrapping_add(1));
        self.flush_iso_queue(link)
    }

    pub fn take_iso_sdus(&mut self, cis_handle: u16) -> Vec<IsoSdu> {
        let (matching, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.iso_inbox)
            .into_iter()
            .partition(|sdu| sdu.connection_handle == cis_handle);
        self.iso_inbox = rest;
        matching
    }

    /// Submit any typed HCI command through this device's attached controller.
    pub fn send_hci_command(&mut self, link: &mut LocalLink, command: Command) {
        match &command {
            Command::CreateConnection { bd_addr, .. } => {
                self.set_pending_classic_role(bd_addr.clone(), bumble_controller::ROLE_CENTRAL);
            }
            Command::AcceptConnectionRequest { bd_addr, role } => {
                self.set_pending_classic_role(bd_addr.clone(), *role);
            }
            _ => {}
        }
        link.handle_command(self.controller_id, command);
    }

    fn send_connection_control_command(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        command: Command,
    ) {
        self.pending_connection_controls
            .entry(command.op_code())
            .or_default()
            .push_back(connection_handle);
        self.send_hci_command(link, command);
    }

    fn complete_connection_control(
        &mut self,
        command_opcode: u16,
        connection_handle: u16,
    ) -> Option<u16> {
        let (removed, empty) = {
            let pending = self.pending_connection_controls.get_mut(&command_opcode)?;
            let index = pending
                .iter()
                .position(|handle| *handle == connection_handle)?;
            let removed = pending.remove(index);
            (removed, pending.is_empty())
        };
        if empty {
            self.pending_connection_controls.remove(&command_opcode);
        }
        removed
    }

    fn fail_next_connection_control(&mut self, command_opcode: u16) -> Option<u16> {
        let (removed, empty) = {
            let pending = self.pending_connection_controls.get_mut(&command_opcode)?;
            let removed = pending.pop_front();
            (removed, pending.is_empty())
        };
        if empty {
            self.pending_connection_controls.remove(&command_opcode);
        }
        removed
    }

    fn set_pending_classic_role(&mut self, peer_address: Address, role: u8) {
        if let Some((_, pending_role)) = self
            .pending_classic_roles
            .iter_mut()
            .find(|(address, _)| *address == peer_address)
        {
            *pending_role = role;
        } else {
            self.pending_classic_roles.push((peer_address, role));
        }
    }

    pub fn connect_classic(&mut self, link: &mut LocalLink, peer_address: Address) {
        self.send_hci_command(
            link,
            Command::CreateConnection {
                bd_addr: peer_address,
                packet_type: 0,
                page_scan_repetition_mode: 0,
                reserved: 0,
                clock_offset: 0,
                allow_role_switch: 1,
            },
        );
    }

    pub fn accept_classic(&mut self, link: &mut LocalLink, peer_address: Address) {
        self.accept_classic_with_role(link, peer_address, bumble_controller::ROLE_PERIPHERAL);
    }

    /// Accept a pending Classic connection using the requested local role.
    pub fn accept_classic_with_role(
        &mut self,
        link: &mut LocalLink,
        peer_address: Address,
        role: u8,
    ) {
        self.set_pending_classic_role(peer_address.clone(), role);
        self.send_hci_command(
            link,
            Command::AcceptConnectionRequest {
                bd_addr: peer_address,
                role,
            },
        );
    }

    /// Request a role change on an established Classic connection.
    pub fn switch_classic_role(&mut self, link: &mut LocalLink, peer_address: Address, role: u8) {
        self.send_hci_command(
            link,
            Command::SwitchRole {
                bd_addr: peer_address,
                role,
            },
        );
    }

    pub fn send_synchronous(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        packet_status: u8,
        data: &[u8],
    ) -> bool {
        link.send_synchronous_data(self.controller_id, connection_handle, packet_status, data)
    }

    pub fn disconnect_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        reason: u8,
    ) -> bool {
        if !link.disconnect(self.controller_id, connection_handle, reason) {
            return false;
        }
        self.pending_disconnections.insert(connection_handle);
        true
    }

    /// Disconnect the current connection with the given reason. Both this device
    /// and the peer receive a Disconnection Complete (processed on the next
    /// [`pump`]).
    pub fn disconnect(&mut self, link: &mut LocalLink, reason: u8) -> bool {
        let Some(handle) = self.connection_handle else {
            return false;
        };
        self.disconnect_handle(link, handle, reason)
    }

    /// Whether any submitted disconnection is awaiting a completion event.
    pub fn is_disconnecting(&self) -> bool {
        !self.pending_disconnections.is_empty()
    }

    /// Whether one handle has a submitted disconnection awaiting completion.
    pub fn is_disconnecting_on_handle(&self, connection_handle: u16) -> bool {
        self.pending_disconnections.contains(&connection_handle)
    }

    /// `true` if this device has an attribute server (server role).
    pub fn has_server(&self) -> bool {
        self.server.is_some()
    }

    /// Atomically apply the controller-owned Classic ACL, LE ACL, and ISO
    /// packet pools discovered by an external Host reset.
    ///
    /// A pool is usable only when both its packet length and packet count are
    /// nonzero. An unusable LE pool shares the Classic ACL queue, matching
    /// upstream Bumble, while an unusable ISO pool remains unavailable. The
    /// raw descriptors are retained even when they report zero capacity.
    /// Reconfiguration is rejected while any existing pool has queued or
    /// in-flight packets so flow-control accounting cannot be discarded.
    pub fn configure_controller_packet_pools(
        &mut self,
        classic_acl: Option<ControllerBufferInfo>,
        le_acl: Option<ControllerBufferInfo>,
        iso: Option<ControllerBufferInfo>,
    ) -> bool {
        if self.acl_packet_queue.pending() != 0
            || self
                .le_acl_packet_queue
                .as_ref()
                .is_some_and(|queue| queue.pending() != 0)
            || self
                .iso_packet_queue
                .as_ref()
                .is_some_and(|queue| queue.pending() != 0)
        {
            return false;
        }

        let usable_classic = classic_acl
            .filter(|buffer| buffer.data_packet_length != 0 && buffer.total_num_data_packets != 0);
        let acl_data_packet_length = usable_classic
            .map(|buffer| usize::from(buffer.data_packet_length))
            .unwrap_or(27);
        let acl_packet_queue = DataPacketQueue::new(
            usable_classic
                .map(|buffer| usize::from(buffer.total_num_data_packets))
                .unwrap_or(64),
        )
        .expect("default and discovered Classic ACL pools have nonzero capacity");

        let usable_le = le_acl
            .filter(|buffer| buffer.data_packet_length != 0 && buffer.total_num_data_packets != 0);
        let le_acl_data_packet_length =
            usable_le.map(|buffer| usize::from(buffer.data_packet_length));
        let le_acl_packet_queue = usable_le.map(|buffer| {
            DataPacketQueue::new(usize::from(buffer.total_num_data_packets))
                .expect("usable LE ACL pool has nonzero capacity")
        });

        let usable_iso = iso
            .filter(|buffer| buffer.data_packet_length != 0 && buffer.total_num_data_packets != 0);
        let iso_data_packet_length =
            usable_iso.map(|buffer| usize::from(buffer.data_packet_length));
        let iso_packet_queue = usable_iso.map(|buffer| {
            DataPacketQueue::new(usize::from(buffer.total_num_data_packets))
                .expect("usable ISO pool has nonzero capacity")
        });

        self.classic_acl_buffer = classic_acl;
        self.le_acl_buffer = le_acl;
        self.iso_buffer = iso;
        self.acl_data_packet_length = acl_data_packet_length;
        self.acl_packet_queue = acl_packet_queue;
        self.le_acl_data_packet_length = le_acl_data_packet_length;
        self.le_acl_packet_queue = le_acl_packet_queue;
        self.iso_data_packet_length = iso_data_packet_length;
        self.iso_packet_queue = iso_packet_queue;
        true
    }

    /// Set the controller's maximum ACL data payload, normally learned from
    /// Read Buffer Size / LE Read Buffer Size.
    pub fn set_acl_data_packet_length(&mut self, length: usize) -> bool {
        if length == 0 || length > u16::MAX as usize {
            return false;
        }
        self.acl_data_packet_length = length;
        true
    }

    /// Set the controller's total ACL packet window while no packets are
    /// pending. This mirrors Read Buffer Size's packet-count field.
    pub fn set_acl_max_in_flight(&mut self, count: usize) -> bool {
        if self.acl_packet_queue.pending() != 0 {
            return false;
        }
        let Ok(queue) = DataPacketQueue::new(count) else {
            return false;
        };
        self.acl_packet_queue = queue;
        true
    }

    pub fn acl_packets_pending(&self) -> usize {
        self.acl_packet_queue.pending()
            + self
                .le_acl_packet_queue
                .as_ref()
                .map(DataPacketQueue::pending)
                .unwrap_or(0)
    }

    pub fn acl_data_packet_length(&self) -> usize {
        self.acl_data_packet_length
    }

    pub fn acl_max_in_flight(&self) -> usize {
        self.acl_packet_queue.max_in_flight()
    }

    /// Effective LE ACL packet size, using the Classic pool when the controller shares it.
    pub fn le_acl_data_packet_length(&self) -> usize {
        self.le_acl_data_packet_length
            .unwrap_or(self.acl_data_packet_length)
    }

    /// Effective LE ACL in-flight window, using the Classic pool when shared.
    pub fn le_acl_max_in_flight(&self) -> usize {
        self.le_acl_packet_queue
            .as_ref()
            .map(DataPacketQueue::max_in_flight)
            .unwrap_or_else(|| self.acl_packet_queue.max_in_flight())
    }

    pub fn iso_data_packet_length(&self) -> Option<usize> {
        self.iso_data_packet_length
    }

    pub fn iso_max_in_flight(&self) -> Option<usize> {
        self.iso_packet_queue
            .as_ref()
            .map(DataPacketQueue::max_in_flight)
    }

    pub fn iso_packets_pending(&self) -> Option<usize> {
        self.iso_packet_queue.as_ref().map(DataPacketQueue::pending)
    }

    pub fn iso_output_is_drained(&self, connection_handle: u16) -> bool {
        self.iso_packet_queue
            .as_ref()
            .is_none_or(|queue| queue.is_drained(connection_handle))
    }

    /// Remove and return the ATT PDUs received so far that were not handled by
    /// the server (i.e. responses and notifications destined for a client).
    pub fn take_inbox(&mut self) -> Vec<AttPdu> {
        std::mem::take(&mut self.inbox)
            .into_iter()
            .map(|(_, pdu)| pdu)
            .collect()
    }

    /// Remove client-bound ATT PDUs received on one LE connection.
    pub fn take_inbox_on_handle(&mut self, connection_handle: u16) -> Vec<AttPdu> {
        let (matching, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.inbox)
            .into_iter()
            .partition(|(handle, _)| *handle == connection_handle);
        self.inbox = rest;
        matching.into_iter().map(|(_, pdu)| pdu).collect()
    }

    /// Send a raw payload on an L2CAP channel to the peer. Requires an
    /// established connection.
    pub fn send_l2cap(&mut self, link: &mut LocalLink, cid: u16, payload: &[u8]) -> bool {
        let Some(handle) = self.connection_handle else {
            return false;
        };
        self.send_l2cap_on_handle(link, handle, cid, payload)
    }

    pub fn send_l2cap_on_handle(
        &mut self,
        link: &mut LocalLink,
        handle: u16,
        cid: u16,
        payload: &[u8],
    ) -> bool {
        let frame = L2capPdu::new(cid, payload.to_vec()).to_bytes(false);
        let uses_separate_le_pool =
            self.le_connections.contains_key(&handle) && self.le_acl_packet_queue.is_some();
        let packet_length = if uses_separate_le_pool {
            self.le_acl_data_packet_length
                .expect("separate LE ACL queue has a packet length")
        } else {
            self.acl_data_packet_length
        };
        let Ok(fragments) = fragment_l2cap_pdu(handle, 0, packet_length, &frame, false) else {
            return false;
        };
        for packet in fragments {
            if uses_separate_le_pool {
                self.le_acl_packet_queue
                    .as_mut()
                    .expect("separate LE ACL queue exists")
                    .enqueue(packet, handle);
            } else {
                self.acl_packet_queue.enqueue(packet, handle);
            }
        }
        self.flush_acl_queue(link)
    }

    /// Send an L2CAP Information Request on the signaling channel appropriate
    /// for an established BR/EDR or LE connection.
    pub fn request_l2cap_information(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        info_type: u16,
    ) -> bumble_l2cap::Result<u8> {
        if let Some(manager) = self.classic_channel_managers.get_mut(&connection_handle) {
            let identifier = manager.request_information(info_type);
            self.flush_classic_channel_manager(link, connection_handle)?;
            return Ok(identifier);
        }
        if let Some(manager) = self.le_credit_managers.get_mut(&connection_handle) {
            let identifier = manager.request_information(info_type);
            self.flush_le_credit_manager(link, connection_handle)?;
            return Ok(identifier);
        }
        Err(L2capError::InvalidPacket(format!(
            "unknown connection handle {connection_handle:#06x}"
        )))
    }

    /// Drain peer Information Responses received on one logical link.
    pub fn take_l2cap_information_responses(
        &mut self,
        connection_handle: u16,
    ) -> Vec<InformationResponse> {
        if let Some(manager) = self.classic_channel_managers.get_mut(&connection_handle) {
            return manager.drain_information_responses();
        }
        self.le_credit_managers
            .get_mut(&connection_handle)
            .map(LeCreditChannelManager::drain_information_responses)
            .unwrap_or_default()
    }

    pub fn le_credit_channel(
        &self,
        connection_handle: u16,
        source_cid: u16,
    ) -> Option<&LeCreditBasedChannel> {
        self.le_credit_managers
            .get(&connection_handle)?
            .channel(source_cid)
    }

    /// Connected EATT source CIDs on one LE connection.
    pub fn eatt_bearers(&self, connection_handle: u16) -> Vec<u16> {
        self.le_credit_managers
            .get(&connection_handle)
            .into_iter()
            .flat_map(LeCreditChannelManager::channels)
            .filter(|channel| channel.psm == EATT_PSM)
            .map(|channel| channel.source_cid)
            .collect()
    }

    pub fn register_le_credit_server(
        &mut self,
        mut spec: LeCreditBasedChannelSpec,
    ) -> bumble_l2cap::Result<u16> {
        spec = spec.validate()?;
        let psm = match spec.psm {
            Some(0) => return Err(L2capError::InvalidPacket("LE PSM cannot be zero".into())),
            Some(psm) => psm,
            None => (L2CAP_LE_PSM_DYNAMIC_RANGE_START..=L2CAP_LE_PSM_DYNAMIC_RANGE_END)
                .find(|candidate| !self.le_credit_server_specs.contains_key(candidate))
                .ok_or_else(|| L2capError::InvalidPacket("no free LE PSM".into()))?,
        };
        if self.le_credit_server_specs.contains_key(&psm) {
            return Err(L2capError::InvalidPacket(format!(
                "LE PSM {psm:#06x} is already in use"
            )));
        }
        spec.psm = Some(psm);
        for manager in self.le_credit_managers.values_mut() {
            manager.register_server(spec)?;
        }
        self.le_credit_server_specs.insert(psm, spec);
        Ok(psm)
    }

    pub fn unregister_le_credit_server(&mut self, psm: u16) -> bool {
        let removed = self.le_credit_server_specs.remove(&psm).is_some();
        for manager in self.le_credit_managers.values_mut() {
            manager.unregister_server(psm);
        }
        removed
    }

    /// Register Enhanced ATT on its assigned LE SPSM for existing and future
    /// connections. Incoming EATT SDUs are routed through this device's ATT
    /// server with bearer-scoped context.
    pub fn register_eatt_server(
        &mut self,
        mut spec: LeCreditBasedChannelSpec,
    ) -> bumble_l2cap::Result<u16> {
        if self.server.is_none() {
            return Err(L2capError::InvalidPacket(
                "cannot register EATT without an ATT server".into(),
            ));
        }
        spec.psm = Some(EATT_PSM);
        self.register_le_credit_server(spec)
    }

    pub fn unregister_eatt_server(&mut self) -> bool {
        self.unregister_le_credit_server(EATT_PSM)
    }

    pub fn connect_le_credit_channel(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        psm: u16,
        spec: LeCreditBasedChannelSpec,
    ) -> bumble_l2cap::Result<u16> {
        let source_cid = self
            .le_credit_manager_mut(connection_handle)?
            .connect(psm, spec)?;
        self.flush_le_credit_manager(link, connection_handle)?;
        Ok(source_cid)
    }

    pub fn connect_enhanced_le_credit_channels(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        psm: u16,
        spec: LeCreditBasedChannelSpec,
        count: usize,
    ) -> bumble_l2cap::Result<Vec<u16>> {
        let source_cids = self
            .le_credit_manager_mut(connection_handle)?
            .connect_enhanced(psm, spec, count)?;
        self.flush_le_credit_manager(link, connection_handle)?;
        Ok(source_cids)
    }

    /// Open one to five EATT bearers using the enhanced LE credit-based
    /// connection procedure, matching upstream `Client.connect_eatt()`.
    pub fn connect_eatt(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        mut spec: LeCreditBasedChannelSpec,
        count: usize,
    ) -> bumble_l2cap::Result<Vec<u16>> {
        spec.psm = Some(EATT_PSM);
        self.connect_enhanced_le_credit_channels(link, connection_handle, EATT_PSM, spec, count)
    }

    pub fn reconfigure_le_credit_channels(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        source_cids: &[u16],
        mtu: u16,
        mps: u16,
    ) -> bumble_l2cap::Result<u8> {
        let identifier =
            self.le_credit_manager_mut(connection_handle)?
                .reconfigure(source_cids, mtu, mps)?;
        self.flush_le_credit_manager(link, connection_handle)?;
        Ok(identifier)
    }

    pub fn le_credit_connection_result(
        &self,
        connection_handle: u16,
        source_cid: u16,
    ) -> Option<u16> {
        self.le_credit_managers
            .get(&connection_handle)?
            .connection_result(source_cid)
    }

    pub fn le_credit_reconfiguration_result(
        &self,
        connection_handle: u16,
        identifier: u8,
    ) -> Option<u16> {
        self.le_credit_managers
            .get(&connection_handle)?
            .reconfiguration_result(identifier)
    }

    pub fn take_accepted_le_credit_channels(&mut self, connection_handle: u16) -> Vec<u16> {
        let Some(manager) = self.le_credit_managers.get_mut(&connection_handle) else {
            return Vec::new();
        };
        std::iter::from_fn(|| manager.poll_accepted_channel()).collect()
    }

    pub fn send_le_credit_sdu(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        source_cid: u16,
        data: &[u8],
    ) -> bumble_l2cap::Result<()> {
        self.le_credit_manager_mut(connection_handle)?
            .send(source_cid, data)?;
        self.flush_le_credit_manager(link, connection_handle)
    }

    /// Apply or release application-level receive backpressure on an LE
    /// credit-based channel. Releasing backpressure flushes any newly restored
    /// credits immediately.
    pub fn set_le_credit_reading_paused(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        source_cid: u16,
        paused: bool,
    ) -> bumble_l2cap::Result<()> {
        self.le_credit_manager_mut(connection_handle)?
            .set_reading_paused(source_cid, paused)?;
        self.flush_le_credit_manager(link, connection_handle)
    }

    /// Whether both the channel framing queue and the controller ACL queue are
    /// drained for this connection. Stream bridges use this to avoid reading
    /// unbounded data ahead of controller flow control.
    pub fn le_credit_output_is_drained(&self, connection_handle: u16, source_cid: u16) -> bool {
        self.le_credit_channel(connection_handle, source_cid)
            .is_some_and(LeCreditBasedChannel::is_drained)
            && self.acl_output_is_drained(connection_handle)
    }

    pub fn take_le_credit_sdus(&mut self, connection_handle: u16, source_cid: u16) -> Vec<Vec<u8>> {
        let Some(channel) = self
            .le_credit_managers
            .get_mut(&connection_handle)
            .and_then(|manager| manager.channel_mut(source_cid))
        else {
            return Vec::new();
        };
        std::iter::from_fn(|| channel.pop_received()).collect()
    }

    /// Send one typed ATT PDU on an established EATT bearer.
    pub fn send_eatt(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        source_cid: u16,
        pdu: &AttPdu,
    ) -> bumble_l2cap::Result<()> {
        let channel = self
            .le_credit_channel(connection_handle, source_cid)
            .ok_or_else(|| {
                L2capError::InvalidPacket(format!("unknown EATT CID {source_cid:#06x}"))
            })?;
        if channel.psm != EATT_PSM {
            return Err(L2capError::InvalidPacket(format!(
                "CID {source_cid:#06x} is not an EATT bearer"
            )));
        }
        self.send_le_credit_sdu(link, connection_handle, source_cid, &pdu.to_bytes())
    }

    /// Remove client-bound ATT PDUs received on one EATT bearer.
    pub fn take_eatt_inbox_on_bearer(
        &mut self,
        connection_handle: u16,
        source_cid: u16,
    ) -> Vec<AttPdu> {
        let (matching, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.eatt_inbox)
            .into_iter()
            .partition(|(handle, cid, _)| *handle == connection_handle && *cid == source_cid);
        self.eatt_inbox = rest;
        matching.into_iter().map(|(_, _, pdu)| pdu).collect()
    }

    /// Remove every client-bound EATT PDU, retaining bearer coordinates.
    pub fn take_eatt_inbox(&mut self) -> Vec<(u16, u16, AttPdu)> {
        std::mem::take(&mut self.eatt_inbox)
    }

    /// Whether a server-sent indication is still awaiting confirmation on a
    /// fixed (`ATT_CID`) or enhanced (source CID) bearer.
    pub fn indication_pending(&self, connection_handle: u16, source_cid: u16) -> bool {
        self.pending_att_indications
            .contains(&(connection_handle, source_cid))
    }

    /// Notify every subscribed fixed or enhanced ATT bearer on one connection.
    pub fn notify_subscribers_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        value_handle: u16,
        value: &[u8],
        force: bool,
    ) -> bumble_l2cap::Result<usize> {
        self.send_subscribed_value(link, connection_handle, value_handle, value, force, false)
    }

    /// Indicate to every subscribed fixed or enhanced ATT bearer on one
    /// connection. A bearer with an outstanding indication is left untouched.
    pub fn indicate_subscribers_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        value_handle: u16,
        value: &[u8],
        force: bool,
    ) -> bumble_l2cap::Result<usize> {
        self.send_subscribed_value(link, connection_handle, value_handle, value, force, true)
    }

    /// Notify every subscribed bearer on every LE connection.
    pub fn notify_subscribers(
        &mut self,
        link: &mut LocalLink,
        value_handle: u16,
        value: &[u8],
        force: bool,
    ) -> bumble_l2cap::Result<usize> {
        self.send_subscribed_value_on_all_connections(link, value_handle, value, force, false)
    }

    /// Indicate to every subscribed bearer on every LE connection.
    pub fn indicate_subscribers(
        &mut self,
        link: &mut LocalLink,
        value_handle: u16,
        value: &[u8],
        force: bool,
    ) -> bumble_l2cap::Result<usize> {
        self.send_subscribed_value_on_all_connections(link, value_handle, value, force, true)
    }

    pub fn disconnect_le_credit_channel(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        source_cid: u16,
    ) -> bumble_l2cap::Result<()> {
        self.le_credit_manager_mut(connection_handle)?
            .disconnect(source_cid)?;
        self.flush_le_credit_manager(link, connection_handle)
    }

    pub fn take_le_credit_errors(&mut self) -> Vec<(u16, String)> {
        std::mem::take(&mut self.le_credit_errors)
    }

    fn send_subscribed_value_on_all_connections(
        &mut self,
        link: &mut LocalLink,
        value_handle: u16,
        value: &[u8],
        force: bool,
        indicate: bool,
    ) -> bumble_l2cap::Result<usize> {
        let handles: Vec<u16> = self.le_connections.keys().copied().collect();
        let mut sent = 0;
        for handle in handles {
            sent +=
                self.send_subscribed_value(link, handle, value_handle, value, force, indicate)?;
        }
        Ok(sent)
    }

    fn send_subscribed_value(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        value_handle: u16,
        value: &[u8],
        force: bool,
        indicate: bool,
    ) -> bumble_l2cap::Result<usize> {
        let server = self.server.as_ref().ok_or_else(|| {
            L2capError::InvalidPacket("cannot send subscribers without an ATT server".into())
        })?;
        let required_bit = if indicate { 0x0002 } else { 0x0001 };
        let mut bearers = vec![ATT_CID];
        if let Some(manager) = self.le_credit_managers.get(&connection_handle) {
            bearers.extend(
                manager
                    .channels()
                    .filter(|channel| channel.psm == EATT_PSM)
                    .map(|channel| channel.source_cid),
            );
        }
        let targets: Vec<(u16, usize)> = bearers
            .into_iter()
            .filter_map(|source_cid| {
                let bearer_id = if source_cid == ATT_CID {
                    att_bearer_id(connection_handle)
                } else {
                    eatt_bearer_id(connection_handle, source_cid)
                };
                let subscribed = server.subscription_bits(bearer_id, value_handle);
                (force || subscribed & required_bit != 0).then(|| {
                    let mtu = server.bearer_mtu(bearer_id).max(ATT_DEFAULT_MTU);
                    (source_cid, usize::from(mtu.saturating_sub(3)))
                })
            })
            .collect();

        let mut sent = 0;
        for (source_cid, max_value_length) in targets {
            if indicate
                && self
                    .pending_att_indications
                    .contains(&(connection_handle, source_cid))
            {
                continue;
            }
            let attribute_value = value[..value.len().min(max_value_length)].to_vec();
            let pdu = if indicate {
                AttPdu::HandleValueIndication {
                    attribute_handle: value_handle,
                    attribute_value,
                }
            } else {
                AttPdu::HandleValueNotification {
                    attribute_handle: value_handle,
                    attribute_value,
                }
            };
            if source_cid == ATT_CID {
                if !self.send_att_on_handle(link, connection_handle, &pdu) {
                    return Err(L2capError::InvalidPacket(format!(
                        "failed to send ATT value on handle {connection_handle:#06x}"
                    )));
                }
            } else {
                self.send_eatt(link, connection_handle, source_cid, &pdu)?;
            }
            if indicate {
                self.pending_att_indications
                    .insert((connection_handle, source_cid));
            }
            sent += 1;
        }
        Ok(sent)
    }

    fn classic_channel_manager_mut(
        &mut self,
        connection_handle: u16,
    ) -> bumble_l2cap::Result<&mut ClassicChannelManager> {
        self.classic_channel_managers
            .get_mut(&connection_handle)
            .ok_or_else(|| {
                L2capError::InvalidPacket(format!(
                    "unknown Classic connection handle {connection_handle:#06x}"
                ))
            })
    }

    fn flush_classic_channel_manager(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
    ) -> bumble_l2cap::Result<()> {
        let outbound = self
            .classic_channel_manager_mut(connection_handle)?
            .drain_outbound();
        for pdu in outbound {
            if !self.send_l2cap_on_handle(link, connection_handle, pdu.cid, &pdu.payload) {
                return Err(L2capError::InvalidPacket(format!(
                    "failed to send Classic channel PDU on handle {connection_handle:#06x}"
                )));
            }
        }
        Ok(())
    }

    fn le_credit_manager_mut(
        &mut self,
        connection_handle: u16,
    ) -> bumble_l2cap::Result<&mut LeCreditChannelManager> {
        self.le_credit_managers
            .get_mut(&connection_handle)
            .ok_or_else(|| {
                L2capError::InvalidPacket(format!(
                    "unknown LE connection handle {connection_handle:#06x}"
                ))
            })
    }

    fn flush_le_credit_manager(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
    ) -> bumble_l2cap::Result<()> {
        let outbound = self
            .le_credit_manager_mut(connection_handle)?
            .drain_outbound();
        for pdu in outbound {
            if !self.send_l2cap_on_handle(link, connection_handle, pdu.cid, &pdu.payload) {
                return Err(L2capError::InvalidPacket(format!(
                    "failed to send LE credit PDU on handle {connection_handle:#06x}"
                )));
            }
        }
        Ok(())
    }

    /// Send an ATT PDU to the peer on the ATT channel.
    pub fn send_att(&mut self, link: &mut LocalLink, pdu: &AttPdu) -> bool {
        self.send_l2cap(link, ATT_CID, &pdu.to_bytes())
    }

    pub fn send_att_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        pdu: &AttPdu,
    ) -> bool {
        self.send_l2cap_on_handle(link, connection_handle, ATT_CID, &pdu.to_bytes())
    }

    /// Remove and return payloads received on the given (non-ATT) L2CAP channel,
    /// e.g. SMP on CID `0x0006`.
    pub fn take_l2cap(&mut self, cid: u16) -> Vec<Vec<u8>> {
        let (matching, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.l2cap_inbox)
            .into_iter()
            .partition(|(_, packet_cid, _)| *packet_cid == cid);
        self.l2cap_inbox = rest;
        matching
            .into_iter()
            .map(|(_, _, payload)| payload)
            .collect()
    }

    pub fn take_l2cap_on_handle(&mut self, connection_handle: u16, cid: u16) -> Vec<Vec<u8>> {
        let (matching, rest): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.l2cap_inbox).into_iter().partition(
                |(handle, packet_cid, _)| *handle == connection_handle && *packet_cid == cid,
            );
        self.l2cap_inbox = rest;
        matching
            .into_iter()
            .map(|(_, _, payload)| payload)
            .collect()
    }

    /// Remove Security Request authentication bitmasks observed on the SMP
    /// fixed channel. Unmanaged devices also retain the raw PDU through
    /// [`Self::take_l2cap`].
    pub fn take_security_requests(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.security_requests)
            .into_iter()
            .map(|(_, authentication)| authentication)
            .collect()
    }

    pub fn take_security_requests_on_handle(&mut self, connection_handle: u16) -> Vec<u8> {
        let (matching, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.security_requests)
            .into_iter()
            .partition(|(handle, _)| *handle == connection_handle);
        self.security_requests = rest;
        matching
            .into_iter()
            .map(|(_, authentication)| authentication)
            .collect()
    }

    fn key_store_namespace(&self) -> Option<String> {
        self.public_address
            .as_ref()
            .filter(|address| address.address_bytes().iter().any(|byte| *byte != 0))
            .or_else(|| {
                self.random_address
                    .address_bytes()
                    .iter()
                    .any(|byte| *byte != 0)
                    .then_some(&self.random_address)
            })
            .map(|address| address.to_string(false))
    }

    fn ensure_key_store(&mut self) {
        if self.key_store.is_some() {
            return;
        }

        let spec = self.config.keystore.clone();
        let key_store: Box<dyn KeyStore> = match spec.as_deref() {
            Some(spec) if spec.split(':').next() == Some("JsonKeyStore") => {
                let namespace = self.key_store_namespace();
                match spec.split_once(':').map(|(_, filename)| filename) {
                    Some(filename) if !filename.is_empty() => Box::new(JsonKeyStore::new(
                        namespace.as_deref(),
                        std::path::PathBuf::from(filename),
                    )),
                    _ => Box::new(JsonKeyStore::with_default_path(namespace.as_deref())),
                }
            }
            _ => Box::new(MemoryKeyStore::new()),
        };
        self.key_store = Some(key_store);
    }

    /// Replace the configured store with an application-provided implementation.
    pub fn set_key_store(&mut self, key_store: impl KeyStore + 'static) {
        self.key_store = Some(Box::new(key_store));
        self.address_resolver = None;
    }

    pub fn address_resolver(&self) -> Option<&AddressResolver> {
        self.address_resolver.as_ref()
    }

    /// Rebuild the host-side RPA resolver from the current pairing key store.
    pub fn refresh_address_resolver(&mut self) -> Result<usize, DeviceKeyStoreError> {
        self.ensure_key_store();
        let resolving_keys = self
            .key_store
            .as_ref()
            .expect("the key store was initialized")
            .get_resolving_keys()?;
        let loaded = resolving_keys
            .iter()
            .filter(|(irk, _)| irk.len() == 16)
            .count();
        self.address_resolver = Some(AddressResolver::new(resolving_keys));
        Ok(loaded)
    }

    /// Rebuild host resolution state and program configured controller offload.
    pub fn refresh_resolving_list(
        &mut self,
        link: &mut LocalLink,
    ) -> Result<usize, DeviceKeyStoreError> {
        self.refresh_configured_resolving_list(link)
    }

    /// Whether this device owns or is configured to create a pairing key store.
    pub fn has_key_store(&self) -> bool {
        self.key_store.is_some() || self.pairing_manager.is_some()
    }

    pub fn bonds(&mut self) -> Result<Vec<(String, PairingKeys)>, DeviceKeyStoreError> {
        self.ensure_key_store();
        Ok(self
            .key_store
            .as_ref()
            .expect("the key store was initialized")
            .get_all()?)
    }

    pub fn bond(
        &mut self,
        peer_address: &Address,
    ) -> Result<Option<PairingKeys>, DeviceKeyStoreError> {
        self.ensure_key_store();
        Ok(self
            .key_store
            .as_ref()
            .expect("the key store was initialized")
            .get(&peer_address.to_string(false))?)
    }

    pub fn delete_bond(&mut self, peer_address: &Address) -> Result<(), DeviceKeyStoreError> {
        self.ensure_key_store();
        self.key_store
            .as_mut()
            .expect("the key store was initialized")
            .delete(&peer_address.to_string(false))?;
        Ok(())
    }

    pub fn delete_all_bonds(&mut self) -> Result<(), DeviceKeyStoreError> {
        self.ensure_key_store();
        self.key_store
            .as_mut()
            .expect("the key store was initialized")
            .delete_all()?;
        Ok(())
    }

    pub fn take_key_store_errors(&mut self) -> Vec<(Option<u16>, String)> {
        std::mem::take(&mut self.key_store_errors)
    }

    fn stored_classic_link_key(
        &mut self,
        peer_address: &Address,
    ) -> Result<Option<([u8; 16], bool)>, DeviceKeyStoreError> {
        self.ensure_key_store();
        let Some(keys) = self
            .key_store
            .as_ref()
            .expect("the key store was initialized")
            .get(&peer_address.to_string(false))?
        else {
            return Ok(None);
        };
        let Some(key) = keys.link_key else {
            return Ok(None);
        };
        let link_key =
            key.value
                .as_slice()
                .try_into()
                .map_err(|_| DeviceKeyStoreError::InvalidKeyLength {
                    field: "Link Key",
                    expected: 16,
                    actual: key.value.len(),
                })?;
        Ok(Some((link_key, key.authenticated)))
    }

    fn load_classic_link_key(
        &mut self,
        connection_handle: u16,
    ) -> Result<bool, DeviceKeyStoreError> {
        let peer_address = self
            .classic_connections
            .get(&connection_handle)
            .ok_or(DeviceKeyStoreError::NoConnection)?
            .peer_address
            .clone();
        let Some(link_key) = self.stored_classic_link_key(&peer_address)? else {
            return Ok(false);
        };
        self.classic_link_keys.insert(connection_handle, link_key);
        Ok(true)
    }

    fn register_classic_pairing_connection(
        &mut self,
        connection_handle: u16,
    ) -> bumble_smp::Result<bool> {
        if !self.config.classic_smp_enabled {
            return Ok(false);
        }
        let Some(connection) = self.classic_connections.get(&connection_handle).cloned() else {
            return Ok(false);
        };
        if connection.encryption_enabled == 0 {
            return Ok(false);
        }
        let Some((link_key, authenticated)) =
            self.classic_link_keys.get(&connection_handle).copied()
        else {
            return Ok(false);
        };
        let local_address = self
            .public_address
            .clone()
            .unwrap_or_else(|| self.static_address.clone());
        let pairing_role = if connection.role == bumble_controller::ROLE_CENTRAL {
            PairingRole::Initiator
        } else {
            PairingRole::Responder
        };
        let Some(manager) = self.pairing_manager.as_mut() else {
            return Ok(false);
        };
        if manager.has_connection(connection_handle) {
            return Ok(true);
        }
        manager.register_connection(PairingConnection::br_edr(
            connection_handle,
            pairing_role,
            local_address,
            connection.peer_address,
            link_key,
            authenticated,
            true,
        ))?;
        Ok(true)
    }

    fn synchronize_classic_pairing_connection(&mut self, connection_handle: u16) {
        let Some(connection) = self.classic_connections.get(&connection_handle) else {
            return;
        };
        if connection.encryption_enabled == 0 {
            if let Some(manager) = self.pairing_manager.as_mut() {
                manager.disconnect(connection_handle);
            }
            return;
        }
        if !self.classic_link_keys.contains_key(&connection_handle) {
            if let Err(error) = self.load_classic_link_key(connection_handle) {
                self.key_store_errors
                    .push((Some(connection_handle), error.to_string()));
                return;
            }
        }
        if let Err(error) = self.register_classic_pairing_connection(connection_handle) {
            self.pairing_errors
                .push((connection_handle, error.to_string()));
        }
    }

    fn persist_classic_link_key(
        &mut self,
        link: &mut LocalLink,
        peer_address: &Address,
        link_key: [u8; 16],
        key_type: u8,
    ) {
        let connection_handle = self.classic_connection_handle_for_peer(peer_address);
        let authenticated = matches!(key_type, 0x05 | 0x08);
        if let Some(connection_handle) = connection_handle {
            self.classic_link_keys
                .insert(connection_handle, (link_key, authenticated));
        }
        self.ensure_key_store();
        let name = peer_address.to_string(false);
        let result = (|| -> Result<(), DeviceKeyStoreError> {
            let store = self
                .key_store
                .as_mut()
                .expect("the key store was initialized");
            let mut keys = store.get(&name)?.unwrap_or_default();
            keys.link_key = Some(Key {
                value: link_key.to_vec(),
                authenticated,
                ..Key::default()
            });
            keys.link_key_type = Some(key_type);
            store.update(&name, keys)?;
            Ok(())
        })();
        match result {
            Ok(()) => match self.refresh_configured_resolving_list(link) {
                Ok(_) => self.emit_device_event(DeviceEvent::KeyStoreUpdated),
                Err(error) => self
                    .key_store_errors
                    .push((connection_handle, error.to_string())),
            },
            Err(error) => self
                .key_store_errors
                .push((connection_handle, error.to_string())),
        }
        if let Some(connection_handle) = connection_handle {
            self.synchronize_classic_pairing_connection(connection_handle);
        }
    }

    fn stored_encryption_parameters(
        &mut self,
        connection_handle: u16,
    ) -> Result<([u8; 16], u16, [u8; 8]), DeviceKeyStoreError> {
        let connection = self
            .le_connections
            .get(&connection_handle)
            .ok_or(DeviceKeyStoreError::NoConnection)?;
        let peer_address = connection.peer_address.clone();
        let role = connection.role;
        self.ensure_key_store();
        let keys = self
            .key_store
            .as_ref()
            .expect("the key store was initialized")
            .get(&peer_address.to_string(false))?
            .ok_or_else(|| DeviceKeyStoreError::BondNotFound {
                peer_address: peer_address.clone(),
            })?;
        let key = keys.ltk.as_ref().or({
            if role == bumble_controller::ROLE_CENTRAL {
                keys.ltk_central.as_ref()
            } else {
                keys.ltk_peripheral.as_ref()
            }
        });
        let key = key.ok_or_else(|| DeviceKeyStoreError::NoLongTermKey {
            peer_address: peer_address.clone(),
        })?;
        let long_term_key =
            key.value
                .as_slice()
                .try_into()
                .map_err(|_| DeviceKeyStoreError::InvalidKeyLength {
                    field: "LTK",
                    expected: 16,
                    actual: key.value.len(),
                })?;
        let random_number = match key.rand.as_deref() {
            Some(random) => {
                random
                    .try_into()
                    .map_err(|_| DeviceKeyStoreError::InvalidKeyLength {
                        field: "LTK RAND",
                        expected: 8,
                        actual: random.len(),
                    })?
            }
            None => [0; 8],
        };
        Ok((long_term_key, key.ediv.unwrap_or(0), random_number))
    }

    /// Start LE encryption using the persisted bond for the current connection.
    pub fn enable_encryption_with_bond(
        &mut self,
        link: &mut LocalLink,
    ) -> Result<bool, DeviceKeyStoreError> {
        let connection_handle = self
            .connection_handle
            .ok_or(DeviceKeyStoreError::NoConnection)?;
        self.enable_encryption_with_bond_on_handle(link, connection_handle)
    }

    /// Start LE encryption using the persisted bond for one connection handle.
    pub fn enable_encryption_with_bond_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
    ) -> Result<bool, DeviceKeyStoreError> {
        let connection = self
            .le_connections
            .get(&connection_handle)
            .ok_or(DeviceKeyStoreError::NoConnection)?;
        if connection.role != bumble_controller::ROLE_CENTRAL {
            return Err(DeviceKeyStoreError::NotCentral { connection_handle });
        }
        let (key, encrypted_diversifier, random_number) =
            self.stored_encryption_parameters(connection_handle)?;
        Ok(self.enable_encryption_with_parameters_on_handle(
            link,
            connection_handle,
            key,
            encrypted_diversifier,
            random_number,
        ))
    }

    pub fn has_pairing_manager(&self) -> bool {
        self.pairing_manager.is_some()
    }

    pub fn pairing_debug_mode(&self) -> Option<bool> {
        self.pairing_manager
            .as_ref()
            .map(PairingManager::debug_mode)
    }

    pub fn pairing_ecc_public_key(&mut self) -> Option<([u8; 32], [u8; 32])> {
        self.pairing_manager
            .as_mut()
            .map(PairingManager::ecc_public_key)
    }

    pub fn pairing_state(&self, connection_handle: u16) -> Option<ManagedPairingState> {
        self.pairing_manager
            .as_ref()
            .and_then(|manager| manager.state(connection_handle))
    }

    pub fn pairing_failure(&self, connection_handle: u16) -> Option<PairingFailureReason> {
        self.pairing_manager
            .as_ref()
            .and_then(|manager| manager.failure(connection_handle))
    }

    pub fn pairing_keys(&self, connection_handle: u16) -> Option<PairingKeys> {
        self.pairing_manager
            .as_ref()
            .and_then(|manager| manager.pairing_keys(connection_handle))
    }

    pub fn take_pairing_errors(&mut self) -> Vec<(u16, String)> {
        std::mem::take(&mut self.pairing_errors)
    }

    pub fn pair(&mut self, link: &mut LocalLink) -> bumble_smp::Result<()> {
        let handle = self
            .connection_handle
            .ok_or_else(|| bumble_smp::Error::InvalidPacket("no active LE connection".into()))?;
        self.pair_on_handle(link, handle)
    }

    /// Start configured SMP/CTKD on the selected encrypted BR/EDR connection.
    pub fn pair_classic(&mut self, link: &mut LocalLink) -> bumble_smp::Result<()> {
        let handle = self.classic_connection_handle.ok_or_else(|| {
            bumble_smp::Error::InvalidPacket("no active Classic connection".into())
        })?;
        self.pair_on_handle(link, handle)
    }

    pub fn pair_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
    ) -> bumble_smp::Result<()> {
        let manager = self.pairing_manager.as_mut().ok_or_else(|| {
            bumble_smp::Error::InvalidPacket("device has no configured pairing manager".into())
        })?;
        manager.set_connection_role(connection_handle, PairingRole::Initiator)?;
        manager.pair(connection_handle)?;
        self.pairing_encryption_started.remove(&connection_handle);
        self.pairing_terminal_handles.remove(&connection_handle);
        self.flush_pairing_manager(link, connection_handle)
    }

    /// Remove all pending LE Long Term Key requests emitted by the controller.
    pub fn take_long_term_key_requests(&mut self) -> Vec<LongTermKeyRequestInfo> {
        std::mem::take(&mut self.long_term_key_requests)
    }

    /// Remove pending LE Long Term Key requests for one connection while
    /// preserving requests belonging to other connections.
    pub fn take_long_term_key_requests_on_handle(
        &mut self,
        connection_handle: u16,
    ) -> Vec<LongTermKeyRequestInfo> {
        let (matching, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.long_term_key_requests)
            .into_iter()
            .partition(|request| request.connection_handle == connection_handle);
        self.long_term_key_requests = rest;
        matching
    }

    fn refresh_configured_resolving_list(
        &mut self,
        link: &mut LocalLink,
    ) -> Result<usize, DeviceKeyStoreError> {
        self.ensure_key_store();
        let resolving_keys = self
            .key_store
            .as_ref()
            .expect("the key store was initialized")
            .get_resolving_keys()?;
        let host_loaded = resolving_keys
            .iter()
            .filter(|(irk, _)| irk.len() == 16)
            .count();
        self.address_resolver = Some(AddressResolver::new(resolving_keys.clone()));

        if !self.config.address_resolution_offload && !self.config.address_generation_offload {
            return Ok(host_loaded);
        }
        let local_irk: [u8; 16] = self.config.irk.as_slice().try_into().map_err(|_| {
            DeviceKeyStoreError::InvalidKeyLength {
                field: "IRK",
                expected: 16,
                actual: self.config.irk.len(),
            }
        })?;

        self.send_hci_command(link, Command::LeClearResolvingList);
        self.send_hci_command(
            link,
            Command::LeAddDeviceToResolvingList {
                peer_identity_address_type: 0,
                peer_identity_address: Address::from_bytes(
                    [0; 6],
                    bumble::AddressType::PUBLIC_DEVICE,
                ),
                peer_irk: [0; 16],
                local_irk,
            },
        );
        let mut loaded = 0;
        for (peer_irk, identity) in resolving_keys {
            let Ok(peer_irk) = peer_irk.as_slice().try_into() else {
                continue;
            };
            self.send_hci_command(
                link,
                Command::LeAddDeviceToResolvingList {
                    peer_identity_address_type: u8::from(!identity.is_public()),
                    peer_identity_address: identity,
                    peer_irk,
                    local_irk,
                },
            );
            loaded += 1;
        }
        if self.config.address_resolution_offload {
            self.send_hci_command(
                link,
                Command::LeSetAddressResolutionEnable {
                    address_resolution_enable: 1,
                },
            );
        }
        Ok(loaded)
    }

    fn persist_pairing_bond(&mut self, link: &mut LocalLink, connection_handle: u16) {
        self.ensure_key_store();
        let result = {
            let manager = self
                .pairing_manager
                .as_ref()
                .expect("configured pairing manager exists");
            let store = self
                .key_store
                .as_mut()
                .expect("the key store was initialized");
            manager.store_bond(connection_handle, store.as_mut())
        };
        match result {
            Ok(true) => match self.refresh_configured_resolving_list(link) {
                Ok(_) => self.emit_device_event(DeviceEvent::KeyStoreUpdated),
                Err(error) => self
                    .key_store_errors
                    .push((Some(connection_handle), error.to_string())),
            },
            Ok(false) => {}
            Err(error) => self
                .key_store_errors
                .push((Some(connection_handle), error.to_string())),
        }
    }

    /// Send an unsolicited Handle Value Notification for `value_handle` to the
    /// peer (server → client). The peer collects it from its inbox.
    pub fn notify(&mut self, link: &mut LocalLink, value_handle: u16, value: Vec<u8>) -> bool {
        self.send_att(
            link,
            &AttPdu::HandleValueNotification {
                attribute_handle: value_handle,
                attribute_value: value,
            },
        )
    }

    pub fn notify_on_handle(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        value_handle: u16,
        value: Vec<u8>,
    ) -> bool {
        self.send_att_on_handle(
            link,
            connection_handle,
            &AttPdu::HandleValueNotification {
                attribute_handle: value_handle,
                attribute_value: value,
            },
        )
    }

    fn flush_pairing_manager(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
    ) -> bumble_smp::Result<()> {
        let (outbound, state, encryption_key, failure, pairing_keys) = {
            let manager = self.pairing_manager.as_mut().ok_or_else(|| {
                bumble_smp::Error::InvalidPacket("device has no configured pairing manager".into())
            })?;
            (
                manager.drain_outbound(),
                manager.state(connection_handle),
                manager.encryption_key(connection_handle),
                manager.failure(connection_handle),
                manager.pairing_keys(connection_handle),
            )
        };

        for (handle, pdu) in outbound {
            let cid = if self.classic_connections.contains_key(&handle) {
                SMP_BR_CID
            } else {
                SMP_CID
            };
            if !self.send_l2cap_on_handle(link, handle, cid, &pdu.to_bytes()) {
                return Err(bumble_smp::Error::InvalidPacket(format!(
                    "failed to send SMP PDU on handle 0x{handle:04X}"
                )));
            }
        }

        let waiting_for_encryption = matches!(
            state,
            Some(ManagedPairingState::Legacy(PairingState::WaitEncryption))
                | Some(ManagedPairingState::SecureConnections(
                    ScPairingState::WaitEncryption
                ))
        );
        let local_is_central = self
            .le_connections
            .get(&connection_handle)
            .is_some_and(|connection| connection.role == bumble_controller::ROLE_CENTRAL);
        if waiting_for_encryption
            && local_is_central
            && self.pairing_encryption_started.insert(connection_handle)
        {
            let key = encryption_key.ok_or_else(|| {
                bumble_smp::Error::InvalidPacket(
                    "pairing reached encryption without an STK/LTK".into(),
                )
            })?;
            if !self.enable_encryption_on_handle(link, connection_handle, key) {
                return Err(bumble_smp::Error::InvalidPacket(format!(
                    "failed to start pairing encryption on handle 0x{connection_handle:04X}"
                )));
            }
        }

        if let Some(reason) = failure {
            if self.pairing_terminal_handles.insert(connection_handle) {
                self.emit_device_event(DeviceEvent::PairingFailed {
                    connection_handle,
                    reason,
                });
            }
        } else if matches!(
            state,
            Some(ManagedPairingState::Legacy(PairingState::Complete))
                | Some(ManagedPairingState::SecureConnections(
                    ScPairingState::Complete
                ))
                | Some(ManagedPairingState::ClassicCtkd(ClassicCtkdState::Complete))
        ) && self.pairing_terminal_handles.insert(connection_handle)
        {
            let keys = pairing_keys.ok_or_else(|| {
                bumble_smp::Error::InvalidPacket(
                    "completed pairing did not retain pairing keys".into(),
                )
            })?;
            self.persist_pairing_bond(link, connection_handle);
            self.emit_device_event(DeviceEvent::PairingComplete {
                connection_handle,
                keys: Box::new(keys),
            });
        }

        Ok(())
    }

    fn on_le_connection_complete(
        &mut self,
        connection_handle: u16,
        role: u8,
        peer_address: Address,
        connection_interval: u16,
        peripheral_latency: u16,
        supervision_timeout: u16,
    ) {
        if role == bumble_controller::ROLE_CENTRAL {
            self.le_connecting = false;
        } else {
            self.legacy_advertising = false;
        }
        self.encrypted_handles.remove(&connection_handle);
        self.le_connections.insert(
            connection_handle,
            LeConnectionInfo {
                connection_handle,
                role,
                peer_address,
                parameters: LeConnectionParameters {
                    connection_interval,
                    peripheral_latency,
                    supervision_timeout,
                    subrate_factor: 1,
                    continuation_number: 0,
                },
                data_length: None,
                phy: LePhy {
                    tx_phy: 1,
                    rx_phy: 1,
                },
                rssi: None,
                encryption_enabled: 0,
                encryption_key_size: 0,
                qos_service_type: None,
                classic_mode: 0,
                classic_interval: 0,
                peer_le_features: None,
                channel_sounding_capabilities: None,
                channel_sounding_configs: BTreeMap::new(),
                channel_sounding_procedures: BTreeMap::new(),
            },
        );
        if let Some(manager) = self.pairing_manager.as_mut() {
            let pairing_role = if role == bumble_controller::ROLE_CENTRAL {
                PairingRole::Initiator
            } else {
                PairingRole::Responder
            };
            if let Err(error) = manager.register_connection(PairingConnection::le(
                connection_handle,
                pairing_role,
                self.random_address.clone(),
                self.le_connections
                    .get(&connection_handle)
                    .expect("connection was just inserted")
                    .peer_address
                    .clone(),
            )) {
                self.pairing_errors
                    .push((connection_handle, error.to_string()));
            }
        }
        let mut manager = LeCreditChannelManager::with_information_capabilities(
            self.l2cap_information_capabilities(),
        );
        for spec in self.le_credit_server_specs.values().copied() {
            manager
                .register_server(spec)
                .expect("stored LE credit server spec is valid");
        }
        self.le_credit_managers.insert(connection_handle, manager);
        self.select_connection(connection_handle);
        let connection = self
            .le_connections
            .get(&connection_handle)
            .expect("connection was just inserted")
            .clone();
        self.emit_device_event(DeviceEvent::LeConnectionEstablished(connection));
    }

    fn update_connection_encryption(
        &mut self,
        connection_handle: u16,
        encryption_enabled: u8,
        encryption_key_size: u8,
    ) {
        if let Some(connection) = self.le_connections.get_mut(&connection_handle) {
            connection.encryption_enabled = encryption_enabled;
            connection.encryption_key_size = encryption_key_size;
        }
        if let Some(connection) = self.classic_connections.get_mut(&connection_handle) {
            connection.encryption_enabled = encryption_enabled;
            connection.encryption_key_size = encryption_key_size;
        }
        if encryption_enabled != 0 {
            self.encrypted_handles.insert(connection_handle);
        } else {
            self.encrypted_handles.remove(&connection_handle);
        }
    }

    fn advance_pairing_encryption(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        encryption_enabled: u8,
    ) {
        if encryption_enabled == 0 {
            return;
        }
        let waiting = self.pairing_manager.as_ref().is_some_and(|manager| {
            matches!(
                manager.state(connection_handle),
                Some(ManagedPairingState::Legacy(PairingState::WaitEncryption))
                    | Some(ManagedPairingState::SecureConnections(
                        ScPairingState::WaitEncryption
                    ))
            )
        });
        if !waiting {
            return;
        }
        let result = self
            .pairing_manager
            .as_mut()
            .expect("pairing manager was checked above")
            .mark_encrypted(connection_handle)
            .and_then(|()| self.flush_pairing_manager(link, connection_handle));
        if let Err(error) = result {
            self.pairing_errors
                .push((connection_handle, error.to_string()));
        }
    }

    fn update_connection_qos(&mut self, connection_handle: u16, service_type: u8) {
        if let Some(connection) = self.le_connections.get_mut(&connection_handle) {
            connection.qos_service_type = Some(service_type);
        }
        if let Some(connection) = self.classic_connections.get_mut(&connection_handle) {
            connection.qos_service_type = Some(service_type);
        }
    }

    /// Drain and process this device's controller events. Returns `true` if any
    /// event was consumed (used by [`pump`] to detect quiescence).
    pub fn poll(&mut self, link: &mut LocalLink) -> bool {
        let events = link.drain_host_events(self.controller_id);
        if events.is_empty() {
            return false;
        }

        for event in events {
            match event {
                HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionComplete {
                    status: 0,
                    connection_handle,
                    role,
                    peer_address,
                    connection_interval,
                    peripheral_latency,
                    supervision_timeout,
                    ..
                })) => self.on_le_connection_complete(
                    connection_handle,
                    role,
                    peer_address,
                    connection_interval,
                    peripheral_latency,
                    supervision_timeout,
                ),
                HciPacket::Event(Event::LeMeta(
                    LeMetaEvent::EnhancedConnectionComplete {
                        status: 0,
                        connection_handle,
                        role,
                        peer_address,
                        connection_interval,
                        peripheral_latency,
                        supervision_timeout,
                        ..
                    }
                    | LeMetaEvent::EnhancedConnectionCompleteV2 {
                        status: 0,
                        connection_handle,
                        role,
                        peer_address,
                        connection_interval,
                        peripheral_latency,
                        supervision_timeout,
                        ..
                    },
                )) => self.on_le_connection_complete(
                    connection_handle,
                    role,
                    peer_address,
                    connection_interval,
                    peripheral_latency,
                    supervision_timeout,
                ),
                HciPacket::Event(Event::LeMeta(
                    LeMetaEvent::ConnectionComplete {
                        status,
                        peer_address,
                        ..
                    }
                    | LeMetaEvent::EnhancedConnectionComplete {
                        status,
                        peer_address,
                        ..
                    }
                    | LeMetaEvent::EnhancedConnectionCompleteV2 {
                        status,
                        peer_address,
                        ..
                    },
                )) => {
                    self.le_connecting = false;
                    self.emit_device_event(DeviceEvent::ConnectionFailed {
                        transport: DeviceConnectionTransport::Le,
                        peer_address,
                        status,
                    });
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionUpdateComplete {
                    status,
                    connection_handle,
                    connection_interval,
                    peripheral_latency,
                    supervision_timeout,
                })) => {
                    let parameters = self
                        .le_connections
                        .get(&connection_handle)
                        .map(|connection| LeConnectionParameters {
                            connection_interval,
                            peripheral_latency,
                            supervision_timeout,
                            ..connection.parameters
                        })
                        .unwrap_or(LeConnectionParameters {
                            connection_interval,
                            peripheral_latency,
                            supervision_timeout,
                            subrate_factor: 1,
                            continuation_number: 0,
                        });
                    if status == 0 {
                        if let Some(connection) = self.le_connections.get_mut(&connection_handle) {
                            connection.parameters = parameters;
                        }
                    }
                    self.complete_connection_control(
                        bumble_hci::HCI_LE_CONNECTION_UPDATE_COMMAND,
                        connection_handle,
                    );
                    self.record_connection_control_event(
                        LeConnectionControlEvent::ConnectionParametersUpdate {
                            status,
                            connection_handle,
                            parameters,
                        },
                    );
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::SubrateChange {
                    status,
                    connection_handle,
                    subrate_factor,
                    peripheral_latency,
                    continuation_number,
                    supervision_timeout,
                })) => {
                    let parameters = self
                        .le_connections
                        .get(&connection_handle)
                        .map(|connection| LeConnectionParameters {
                            subrate_factor,
                            peripheral_latency,
                            continuation_number,
                            supervision_timeout,
                            ..connection.parameters
                        })
                        .unwrap_or(LeConnectionParameters {
                            connection_interval: 0,
                            peripheral_latency,
                            supervision_timeout,
                            subrate_factor,
                            continuation_number,
                        });
                    if status == 0 {
                        if let Some(connection) = self.le_connections.get_mut(&connection_handle) {
                            connection.parameters = parameters;
                        }
                    }
                    self.complete_connection_control(
                        bumble_hci::HCI_LE_SUBRATE_REQUEST_COMMAND,
                        connection_handle,
                    );
                    self.record_connection_control_event(
                        LeConnectionControlEvent::ConnectionParametersUpdate {
                            status,
                            connection_handle,
                            parameters,
                        },
                    );
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionRateChange {
                    status,
                    connection_handle,
                    connection_interval,
                    subrate_factor,
                    peripheral_latency,
                    continuation_number,
                    supervision_timeout,
                })) => {
                    let parameters = LeConnectionParameters {
                        connection_interval,
                        peripheral_latency,
                        supervision_timeout,
                        subrate_factor,
                        continuation_number,
                    };
                    if status == 0 {
                        if let Some(connection) = self.le_connections.get_mut(&connection_handle) {
                            connection.parameters = parameters;
                        }
                    }
                    self.complete_connection_control(
                        bumble_hci::HCI_LE_CONNECTION_RATE_REQUEST_COMMAND,
                        connection_handle,
                    );
                    self.record_connection_control_event(
                        LeConnectionControlEvent::ConnectionParametersUpdate {
                            status,
                            connection_handle,
                            parameters,
                        },
                    );
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::DataLengthChange {
                    connection_handle,
                    max_tx_octets,
                    max_tx_time,
                    max_rx_octets,
                    max_rx_time,
                })) => {
                    let data_length = LeDataLength {
                        max_tx_octets,
                        max_tx_time,
                        max_rx_octets,
                        max_rx_time,
                    };
                    if let Some(connection) = self.le_connections.get_mut(&connection_handle) {
                        connection.data_length = Some(data_length);
                    }
                    self.record_connection_control_event(
                        LeConnectionControlEvent::DataLengthChange {
                            connection_handle,
                            data_length,
                        },
                    );
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::PhyUpdateComplete {
                    status,
                    connection_handle,
                    tx_phy,
                    rx_phy,
                })) => {
                    let phy = LePhy { tx_phy, rx_phy };
                    if status == 0 {
                        if let Some(connection) = self.le_connections.get_mut(&connection_handle) {
                            connection.phy = phy;
                        }
                    }
                    self.complete_connection_control(
                        bumble_hci::HCI_LE_SET_PHY_COMMAND,
                        connection_handle,
                    );
                    self.record_connection_control_event(LeConnectionControlEvent::PhyUpdate {
                        status,
                        connection_handle,
                        phy,
                    });
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::ReadRemoteFeaturesComplete {
                    status,
                    connection_handle,
                    le_features,
                })) => {
                    if let Some(connection) = self.le_connections.get_mut(&connection_handle) {
                        if status == 0 {
                            connection.peer_le_features = Some(le_features);
                        } else {
                            self.connection_feature_errors.push(ConnectionFeatureError {
                                transport: ConnectionFeatureTransport::Le,
                                connection_handle,
                                page_number: None,
                                status,
                            });
                        }
                    }
                }
                HciPacket::Event(Event::LeMeta(
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
                    },
                )) => {
                    if let Some(connection) = self.le_connections.get_mut(&connection_handle) {
                        if status == 0 {
                            connection.channel_sounding_capabilities =
                                Some(ChannelSoundingCapabilities {
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
                                });
                        } else {
                            self.channel_sounding_errors.push(ChannelSoundingError {
                                operation: ChannelSoundingOperation::ReadRemoteCapabilities,
                                connection_handle,
                                config_id: None,
                                status,
                            });
                        }
                    }
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::CsSecurityEnableComplete {
                    status,
                    connection_handle,
                })) => {
                    if self.le_connections.contains_key(&connection_handle) {
                        self.channel_sounding_security_results
                            .push((connection_handle, status));
                        if status != 0 {
                            self.channel_sounding_errors.push(ChannelSoundingError {
                                operation: ChannelSoundingOperation::SecurityEnable,
                                connection_handle,
                                config_id: None,
                                status,
                            });
                        }
                    }
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::CsConfigComplete {
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
                })) => {
                    self.pending_channel_sounding_configs
                        .remove(&(connection_handle, config_id));
                    if let Some(connection) = self.le_connections.get_mut(&connection_handle) {
                        if status != 0 {
                            self.channel_sounding_errors.push(ChannelSoundingError {
                                operation: ChannelSoundingOperation::Config,
                                connection_handle,
                                config_id: Some(config_id),
                                status,
                            });
                        } else if action == 1 {
                            connection.channel_sounding_configs.insert(
                                config_id,
                                ChannelSoundingConfig {
                                    config_id,
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
                                },
                            );
                        } else if action == 0 {
                            connection.channel_sounding_configs.remove(&config_id);
                        }
                    }
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::CsProcedureEnableComplete {
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
                })) => {
                    if let Some(connection) = self.le_connections.get_mut(&connection_handle) {
                        if status == 0 {
                            connection.channel_sounding_procedures.insert(
                                config_id,
                                ChannelSoundingProcedure {
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
                                },
                            );
                        } else {
                            self.channel_sounding_errors.push(ChannelSoundingError {
                                operation: ChannelSoundingOperation::ProcedureEnable,
                                connection_handle,
                                config_id: Some(config_id),
                                status,
                            });
                        }
                    }
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::CsSubeventResult {
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
                })) => {
                    let result = ChannelSoundingSubeventResult {
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
                    };
                    self.channel_sounding_subevent_results.push(result.clone());
                    self.emit_device_event(DeviceEvent::ChannelSoundingSubeventResult(result));
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::CsSubeventResultContinue {
                    connection_handle,
                    config_id,
                    procedure_done_status,
                    subevent_done_status,
                    abort_reason,
                    num_antenna_paths,
                    step_mode,
                    step_channel,
                    step_data,
                })) => {
                    let result = ChannelSoundingSubeventResultContinue {
                        connection_handle,
                        config_id,
                        procedure_done_status,
                        subevent_done_status,
                        abort_reason,
                        num_antenna_paths,
                        step_mode,
                        step_channel,
                        step_data,
                    };
                    self.channel_sounding_subevent_result_continuations
                        .push(result.clone());
                    self.emit_device_event(DeviceEvent::ChannelSoundingSubeventResultContinue(
                        result,
                    ));
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::AdvertisingReport { reports })) => {
                    for report in reports {
                        self.advertising_reports.push(report.clone());
                        self.emit_device_event(DeviceEvent::AdvertisingReport(report.clone()));
                        let passive = self.scanning_is_passive;
                        let advertisement = self
                            .advertisement_accumulators
                            .entry((
                                report.address.address_type().0,
                                *report.address.address_bytes(),
                            ))
                            .or_insert_with(|| AdvertisementDataAccumulator::new(passive))
                            .update_legacy(&report);
                        if let Some(advertisement) = advertisement {
                            self.advertisements.push(advertisement.clone());
                            self.emit_device_event(DeviceEvent::Advertisement(
                                advertisement.clone(),
                            ));
                            self.complete_peer_lookups(
                                link,
                                PeerLookupTransport::Le,
                                &advertisement.address,
                                &advertisement.data,
                            );
                        }
                    }
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::ExtendedAdvertisingReport {
                    reports,
                })) => {
                    for report in reports {
                        self.extended_advertising_reports.push(report.clone());
                        self.emit_device_event(DeviceEvent::ExtendedAdvertisingReport(
                            report.clone(),
                        ));
                        let passive = self.scanning_is_passive;
                        let advertisement = self
                            .advertisement_accumulators
                            .entry((
                                report.address.address_type().0,
                                *report.address.address_bytes(),
                            ))
                            .or_insert_with(|| AdvertisementDataAccumulator::new(passive))
                            .update_extended(&report);
                        if let Some(advertisement) = advertisement {
                            self.advertisements.push(advertisement.clone());
                            self.emit_device_event(DeviceEvent::Advertisement(
                                advertisement.clone(),
                            ));
                            self.complete_peer_lookups(
                                link,
                                PeerLookupTransport::Le,
                                &advertisement.address,
                                &advertisement.data,
                            );
                        }
                    }
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::LongTermKeyRequest {
                    connection_handle,
                    random_number,
                    encryption_diversifier,
                })) => {
                    let pairing_key = self
                        .pairing_manager
                        .as_ref()
                        .and_then(|manager| manager.encryption_key(connection_handle));
                    let stored_key = if pairing_key.is_none() && self.has_key_store() {
                        match self.stored_encryption_parameters(connection_handle) {
                            Ok((key, _, _)) => Some(key),
                            Err(DeviceKeyStoreError::BondNotFound { .. })
                            | Err(DeviceKeyStoreError::NoLongTermKey { .. }) => None,
                            Err(error) => {
                                self.key_store_errors
                                    .push((Some(connection_handle), error.to_string()));
                                None
                            }
                        }
                    } else {
                        None
                    };
                    if let Some(key) = pairing_key.or(stored_key) {
                        self.reply_long_term_key_request(link, connection_handle, key);
                    } else {
                        self.long_term_key_requests.push(LongTermKeyRequestInfo {
                            connection_handle,
                            random_number,
                            encryption_diversifier,
                        });
                    }
                }
                HciPacket::Event(Event::LeMeta(
                    LeMetaEvent::PeriodicAdvertisingSyncEstablished {
                        status,
                        sync_handle,
                        advertising_sid,
                        advertiser_address_type,
                        advertiser_address,
                        advertiser_phy,
                        periodic_advertising_interval,
                        advertiser_clock_accuracy,
                    },
                )) => {
                    if status == 0 {
                        self.periodic_syncs.insert(
                            sync_handle,
                            PeriodicAdvertisingSyncInfo {
                                sync_handle,
                                advertising_sid,
                                advertiser_address_type,
                                advertiser_address,
                                advertiser_phy,
                                interval: periodic_advertising_interval,
                                advertiser_clock_accuracy,
                            },
                        );
                    } else {
                        self.periodic_sync_errors.push(status);
                    }
                }
                HciPacket::Event(Event::LeMeta(
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
                    },
                )) => {
                    if status == 0 {
                        let sync = PeriodicAdvertisingSyncInfo {
                            sync_handle,
                            advertising_sid,
                            advertiser_address_type,
                            advertiser_address,
                            advertiser_phy,
                            interval: periodic_advertising_interval,
                            advertiser_clock_accuracy,
                        };
                        self.periodic_syncs.insert(sync_handle, sync.clone());
                        self.periodic_sync_transfers
                            .push(PeriodicAdvertisingSyncTransferInfo {
                                connection_handle,
                                service_data,
                                sync,
                            });
                    } else {
                        self.periodic_sync_errors.push(status);
                    }
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::PeriodicAdvertisingReport {
                    sync_handle,
                    tx_power,
                    rssi,
                    data_status,
                    data,
                    ..
                })) => {
                    if let Some(sync) = self.periodic_syncs.get(&sync_handle) {
                        self.periodic_report_accumulators
                            .entry(sync_handle)
                            .or_default()
                            .extend_from_slice(&data);
                        if data_status != 1 {
                            let data = self
                                .periodic_report_accumulators
                                .remove(&sync_handle)
                                .unwrap_or_default();
                            self.periodic_advertisements.push(PeriodicAdvertisement {
                                sync_handle,
                                advertiser_address: sync.advertiser_address.clone(),
                                advertising_sid: sync.advertising_sid,
                                tx_power,
                                rssi,
                                truncated: data_status == 2,
                                data,
                            });
                        }
                    }
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::PeriodicAdvertisingSyncLost {
                    sync_handle,
                })) => {
                    self.periodic_syncs.remove(&sync_handle);
                    self.periodic_report_accumulators.remove(&sync_handle);
                    self.lost_periodic_syncs.push(sync_handle);
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::AdvertisingSetTerminated {
                    advertising_handle,
                    ..
                })) => {
                    self.extended_advertising_handles
                        .remove(&advertising_handle);
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::BiginfoAdvertisingReport {
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
                })) => self.biginfo_reports.push(BigInfoReport {
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
                    encrypted: encryption != 0,
                }),
                HciPacket::Event(Event::LeMeta(LeMetaEvent::CreateBigComplete {
                    status,
                    big_handle,
                    connection_handle,
                    ..
                })) => {
                    self.pending_bigs.remove(&big_handle);
                    if let Some(index) = self
                        .pending_big_commands
                        .iter()
                        .position(|pending| *pending == big_handle)
                    {
                        self.pending_big_commands.remove(index);
                    }
                    if status == 0 {
                        for handle in &connection_handle {
                            self.bis_directions.insert(*handle, 0);
                            self.iso_sequence_numbers.entry(*handle).or_default();
                        }
                        self.bigs.insert(big_handle, connection_handle);
                    } else {
                        self.bigs.remove(&big_handle);
                        self.big_errors.push((big_handle, status));
                    }
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::TerminateBigComplete {
                    big_handle,
                    reason,
                })) => {
                    if let Some(handles) = self.bigs.remove(&big_handle) {
                        for handle in handles {
                            self.clear_bis_handle(handle);
                        }
                    }
                    self.terminated_bigs.push((big_handle, reason));
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::BigSyncEstablished {
                    status,
                    big_handle,
                    connection_handle,
                    ..
                })) => {
                    self.pending_big_syncs.remove(&big_handle);
                    if let Some(index) = self
                        .pending_big_sync_commands
                        .iter()
                        .position(|pending| *pending == big_handle)
                    {
                        self.pending_big_sync_commands.remove(index);
                    }
                    if status == 0 {
                        for handle in &connection_handle {
                            self.bis_directions.insert(*handle, 1);
                            self.iso_sequence_numbers.entry(*handle).or_default();
                        }
                        self.big_syncs.insert(big_handle, connection_handle);
                    } else {
                        self.big_syncs.remove(&big_handle);
                        self.big_errors.push((big_handle, status));
                    }
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::BigSyncLost {
                    big_handle,
                    reason,
                })) => {
                    if let Some(handles) = self.big_syncs.remove(&big_handle) {
                        for handle in handles {
                            self.clear_bis_handle(handle);
                        }
                    }
                    self.terminated_bigs.push((big_handle, reason));
                }
                HciPacket::Event(Event::LeMeta(LeMetaEvent::CisRequest {
                    acl_connection_handle,
                    cis_connection_handle,
                    cig_id,
                    cis_id,
                })) => self.cis_requests.push(CisRequestInfo {
                    acl_connection_handle,
                    cis_connection_handle,
                    cig_id,
                    cis_id,
                }),
                HciPacket::Event(Event::LeMeta(LeMetaEvent::CisEstablished {
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
                })) => {
                    let link = CisLinkInfo {
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
                    };
                    if status == 0 {
                        self.cis_links.insert(connection_handle, link);
                        self.iso_sequence_numbers
                            .entry(connection_handle)
                            .or_default();
                    }
                    self.cis_control_events
                        .push(CisControlEvent::Established { status, link });
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::ReadLocalSupportedCommands {
                            status,
                            supported_commands,
                        },
                    ..
                }) if command_opcode == bumble_hci::HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND => {
                    self.local_supported_commands_status = Some(status);
                    if status == 0 {
                        self.local_supported_commands = Some(supported_commands);
                        let supported_names =
                            bumble_hci::metadata::supported_command_names(&supported_commands);
                        if supported_names.contains(&"HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND") {
                            self.send_hci_command(link, Command::ReadLocalVersionInformation);
                        }
                        if supported_names
                            .contains(&"HCI_LE_READ_ALL_LOCAL_SUPPORTED_FEATURES_COMMAND")
                        {
                            self.send_hci_command(link, Command::LeReadAllLocalSupportedFeatures);
                        } else if supported_names
                            .contains(&"HCI_LE_READ_LOCAL_SUPPORTED_FEATURES_COMMAND")
                        {
                            self.send_hci_command(link, Command::LeReadLocalSupportedFeatures);
                        }
                        if supported_names.contains(&"HCI_READ_LOCAL_EXTENDED_FEATURES_COMMAND") {
                            self.request_local_lmp_feature_page(link, 0);
                        } else if supported_names
                            .contains(&"HCI_READ_LOCAL_SUPPORTED_FEATURES_COMMAND")
                        {
                            self.send_hci_command(link, Command::ReadLocalSupportedFeatures);
                        }
                        self.maybe_start_host_initialization(link);
                    }
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::ReadLocalVersionInformation {
                            status,
                            hci_version,
                            hci_subversion,
                            lmp_version,
                            company_identifier,
                            lmp_subversion,
                        },
                    ..
                }) if command_opcode == bumble_hci::HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND => {
                    self.local_version_status = Some(status);
                    if status == 0 {
                        self.local_version = Some(LocalVersionInformation {
                            hci_version,
                            hci_subversion,
                            lmp_version,
                            company_identifier,
                            lmp_subversion,
                        });
                    }
                    self.maybe_start_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::ReadLocalExtendedFeatures {
                            status,
                            page_number,
                            maximum_page_number,
                            extended_lmp_features,
                        },
                    ..
                }) if command_opcode == bumble_hci::HCI_READ_LOCAL_EXTENDED_FEATURES_COMMAND => {
                    if let Some(index) = self
                        .pending_local_lmp_feature_pages
                        .iter()
                        .position(|pending_page| *pending_page == page_number)
                    {
                        self.pending_local_lmp_feature_pages.remove(index);
                    }
                    self.local_lmp_feature_statuses.insert(page_number, status);
                    if status == 0 {
                        self.local_lmp_features
                            .insert(page_number, extended_lmp_features);
                        self.local_lmp_features_max_page = Some(maximum_page_number);
                        if page_number < maximum_page_number {
                            self.request_local_lmp_feature_page(link, page_number + 1);
                        }
                    }
                    self.maybe_start_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::ReadLocalSupportedFeatures {
                            status,
                            lmp_features,
                        },
                    ..
                }) if command_opcode == bumble_hci::HCI_READ_LOCAL_SUPPORTED_FEATURES_COMMAND => {
                    self.local_lmp_feature_statuses.insert(0, status);
                    if status == 0 {
                        self.local_lmp_features.insert(0, lmp_features);
                        self.local_lmp_features_max_page = Some(0);
                    }
                    self.maybe_start_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::LeReadAllLocalSupportedFeatures {
                            status,
                            max_page,
                            le_features,
                        },
                    ..
                }) if command_opcode
                    == bumble_hci::HCI_LE_READ_ALL_LOCAL_SUPPORTED_FEATURES_COMMAND =>
                {
                    self.local_le_features_status = Some(status);
                    if status == 0 {
                        self.local_le_features = Some(le_features.to_vec());
                        self.local_le_features_max_page = Some(max_page);
                    }
                    self.maybe_start_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::LeReadLocalSupportedFeatures {
                            status,
                            le_features,
                        },
                    ..
                }) if command_opcode
                    == bumble_hci::HCI_LE_READ_LOCAL_SUPPORTED_FEATURES_COMMAND =>
                {
                    self.local_le_features_status = Some(status);
                    if status == 0 {
                        self.local_le_features = Some(le_features.to_vec());
                        self.local_le_features_max_page = None;
                    }
                    self.maybe_start_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters: bumble_hci::ReturnParameters::Status { status },
                    ..
                }) if command_opcode == bumble_hci::HCI_READ_LOCAL_VERSION_INFORMATION_COMMAND => {
                    self.local_version_status = Some(status);
                    self.maybe_start_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters: bumble_hci::ReturnParameters::Status { status },
                    ..
                }) if command_opcode == bumble_hci::HCI_READ_LOCAL_SUPPORTED_FEATURES_COMMAND => {
                    self.local_lmp_feature_statuses.insert(0, status);
                    self.maybe_start_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters: bumble_hci::ReturnParameters::Status { status },
                    ..
                }) if command_opcode == bumble_hci::HCI_READ_LOCAL_EXTENDED_FEATURES_COMMAND => {
                    if let Some(page_number) = self.pending_local_lmp_feature_pages.pop_front() {
                        self.local_lmp_feature_statuses.insert(page_number, status);
                    }
                    self.maybe_start_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters: bumble_hci::ReturnParameters::Status { status },
                    ..
                }) if command_opcode == bumble_hci::HCI_READ_LOCAL_SUPPORTED_COMMANDS_COMMAND => {
                    self.local_supported_commands_status = Some(status);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode:
                        bumble_hci::HCI_LE_READ_LOCAL_SUPPORTED_FEATURES_COMMAND
                        | bumble_hci::HCI_LE_READ_ALL_LOCAL_SUPPORTED_FEATURES_COMMAND,
                    return_parameters: bumble_hci::ReturnParameters::Status { status },
                    ..
                }) => {
                    self.local_le_features_status = Some(status);
                    self.maybe_start_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters: bumble_hci::ReturnParameters::Status { status },
                    ..
                }) if command_opcode == bumble_hci::HCI_SET_EVENT_MASK_COMMAND => {
                    self.event_mask_status = Some(status);
                    self.maybe_finish_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters: bumble_hci::ReturnParameters::Status { status },
                    ..
                }) if command_opcode == bumble_hci::HCI_SET_EVENT_MASK_PAGE_2_COMMAND => {
                    self.event_mask_page_2_status = Some(status);
                    self.maybe_finish_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters: bumble_hci::ReturnParameters::Status { status },
                    ..
                }) if command_opcode == bumble_hci::HCI_LE_SET_EVENT_MASK_COMMAND => {
                    self.le_event_mask_status = Some(status);
                    self.maybe_finish_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::ReadBufferSize {
                            status,
                            hc_acl_data_packet_length,
                            hc_total_num_acl_data_packets,
                            ..
                        },
                    ..
                }) if command_opcode == bumble_hci::HCI_READ_BUFFER_SIZE_COMMAND => {
                    self.classic_buffer_status = Some(status);
                    if status == 0 {
                        self.classic_acl_buffer = Some(ControllerBufferInfo {
                            data_packet_length: hc_acl_data_packet_length,
                            total_num_data_packets: hc_total_num_acl_data_packets,
                        });
                        if hc_acl_data_packet_length != 0 && hc_total_num_acl_data_packets != 0 {
                            self.acl_data_packet_length = usize::from(hc_acl_data_packet_length);
                            self.acl_packet_queue =
                                DataPacketQueue::new(usize::from(hc_total_num_acl_data_packets))
                                    .expect("nonzero controller ACL packet count");
                        }
                    }
                    self.maybe_finish_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::LeReadBufferSize {
                            status,
                            le_acl_data_packet_length,
                            total_num_le_acl_data_packets,
                        },
                    ..
                }) if command_opcode == bumble_hci::HCI_LE_READ_BUFFER_SIZE_COMMAND => {
                    self.le_buffer_status = Some(status);
                    if status == 0 {
                        self.le_acl_buffer = Some(ControllerBufferInfo {
                            data_packet_length: le_acl_data_packet_length,
                            total_num_data_packets: u16::from(total_num_le_acl_data_packets),
                        });
                        if le_acl_data_packet_length != 0 && total_num_le_acl_data_packets != 0 {
                            self.le_acl_data_packet_length =
                                Some(usize::from(le_acl_data_packet_length));
                            self.le_acl_packet_queue = Some(
                                DataPacketQueue::new(usize::from(total_num_le_acl_data_packets))
                                    .expect("nonzero controller LE ACL packet count"),
                            );
                        } else {
                            self.le_acl_data_packet_length = None;
                            self.le_acl_packet_queue = None;
                        }
                    }
                    self.maybe_finish_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::LeReadBufferSizeV2 {
                            status,
                            le_acl_data_packet_length,
                            total_num_le_acl_data_packets,
                            iso_data_packet_length,
                            total_num_iso_data_packets,
                        },
                    ..
                }) if command_opcode == bumble_hci::HCI_LE_READ_BUFFER_SIZE_V2_COMMAND => {
                    self.le_buffer_status = Some(status);
                    if status == 0 {
                        self.le_acl_buffer = Some(ControllerBufferInfo {
                            data_packet_length: le_acl_data_packet_length,
                            total_num_data_packets: u16::from(total_num_le_acl_data_packets),
                        });
                        if le_acl_data_packet_length != 0 && total_num_le_acl_data_packets != 0 {
                            self.le_acl_data_packet_length =
                                Some(usize::from(le_acl_data_packet_length));
                            self.le_acl_packet_queue = Some(
                                DataPacketQueue::new(usize::from(total_num_le_acl_data_packets))
                                    .expect("nonzero controller LE ACL packet count"),
                            );
                        } else {
                            self.le_acl_data_packet_length = None;
                            self.le_acl_packet_queue = None;
                        }
                        self.iso_buffer = Some(ControllerBufferInfo {
                            data_packet_length: iso_data_packet_length,
                            total_num_data_packets: u16::from(total_num_iso_data_packets),
                        });
                        if iso_data_packet_length != 0 && total_num_iso_data_packets != 0 {
                            self.iso_data_packet_length = Some(usize::from(iso_data_packet_length));
                            self.iso_packet_queue = Some(
                                DataPacketQueue::new(usize::from(total_num_iso_data_packets))
                                    .expect("nonzero controller ISO packet count"),
                            );
                        } else {
                            self.iso_data_packet_length = None;
                            self.iso_packet_queue = None;
                        }
                    }
                    self.maybe_finish_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters: bumble_hci::ReturnParameters::Status { status },
                    ..
                }) if matches!(
                    command_opcode,
                    bumble_hci::HCI_READ_BUFFER_SIZE_COMMAND
                        | bumble_hci::HCI_LE_READ_BUFFER_SIZE_COMMAND
                        | bumble_hci::HCI_LE_READ_BUFFER_SIZE_V2_COMMAND
                ) =>
                {
                    if matches!(command_opcode, bumble_hci::HCI_READ_BUFFER_SIZE_COMMAND) {
                        self.classic_buffer_status = Some(status);
                    } else {
                        self.le_buffer_status = Some(status);
                    }
                    self.maybe_finish_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::LeReadSuggestedDefaultDataLength {
                            status,
                            suggested_max_tx_octets,
                            suggested_max_tx_time,
                        },
                    ..
                }) if command_opcode
                    == bumble_hci::HCI_LE_READ_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND =>
                {
                    self.suggested_default_data_length_read_status = Some(status);
                    if status == 0 {
                        let suggestion = LeSuggestedDefaultDataLength {
                            suggested_max_tx_octets,
                            suggested_max_tx_time,
                        };
                        self.suggested_default_data_length = Some(suggestion);
                        self.suggested_default_data_length_write_required = suggestion
                            != (LeSuggestedDefaultDataLength {
                                suggested_max_tx_octets: HOST_SUGGESTED_MAX_TX_OCTETS,
                                suggested_max_tx_time: HOST_SUGGESTED_MAX_TX_TIME,
                            });
                        if self.suggested_default_data_length_write_required {
                            self.send_hci_command(
                                link,
                                Command::LeWriteSuggestedDefaultDataLength {
                                    suggested_max_tx_octets: HOST_SUGGESTED_MAX_TX_OCTETS,
                                    suggested_max_tx_time: HOST_SUGGESTED_MAX_TX_TIME,
                                },
                            );
                        }
                    }
                    self.maybe_finish_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::LeReadNumberOfSupportedAdvertisingSets {
                            status,
                            num_supported_advertising_sets,
                        },
                    ..
                }) if command_opcode
                    == bumble_hci::HCI_LE_READ_NUMBER_OF_SUPPORTED_ADVERTISING_SETS_COMMAND =>
                {
                    self.number_of_supported_advertising_sets_status = Some(status);
                    if status == 0 {
                        self.number_of_supported_advertising_sets = num_supported_advertising_sets;
                    }
                    self.maybe_finish_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::LeReadMaximumAdvertisingDataLength {
                            status,
                            max_advertising_data_length,
                        },
                    ..
                }) if command_opcode
                    == bumble_hci::HCI_LE_READ_MAXIMUM_ADVERTISING_DATA_LENGTH_COMMAND =>
                {
                    self.maximum_advertising_data_length_status = Some(status);
                    if status == 0 {
                        self.maximum_advertising_data_length = max_advertising_data_length;
                    }
                    self.maybe_finish_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters: bumble_hci::ReturnParameters::Status { status },
                    ..
                }) if matches!(
                    command_opcode,
                    bumble_hci::HCI_LE_READ_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND
                        | bumble_hci::HCI_LE_WRITE_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND
                        | bumble_hci::HCI_LE_READ_NUMBER_OF_SUPPORTED_ADVERTISING_SETS_COMMAND
                        | bumble_hci::HCI_LE_READ_MAXIMUM_ADVERTISING_DATA_LENGTH_COMMAND
                ) =>
                {
                    match command_opcode {
                        bumble_hci::HCI_LE_READ_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND => {
                            self.suggested_default_data_length_read_status = Some(status);
                        }
                        bumble_hci::HCI_LE_WRITE_SUGGESTED_DEFAULT_DATA_LENGTH_COMMAND => {
                            self.suggested_default_data_length_write_status = Some(status);
                            if status == 0 {
                                self.suggested_default_data_length =
                                    Some(LeSuggestedDefaultDataLength {
                                        suggested_max_tx_octets: HOST_SUGGESTED_MAX_TX_OCTETS,
                                        suggested_max_tx_time: HOST_SUGGESTED_MAX_TX_TIME,
                                    });
                            }
                        }
                        bumble_hci::HCI_LE_READ_NUMBER_OF_SUPPORTED_ADVERTISING_SETS_COMMAND => {
                            self.number_of_supported_advertising_sets_status = Some(status);
                        }
                        bumble_hci::HCI_LE_READ_MAXIMUM_ADVERTISING_DATA_LENGTH_COMMAND => {
                            self.maximum_advertising_data_length_status = Some(status);
                        }
                        _ => unreachable!("guard restricts Host reset tail opcodes"),
                    }
                    self.maybe_finish_host_initialization(link);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::LeCsReadLocalSupportedCapabilities {
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
                        },
                    ..
                }) if command_opcode
                    == bumble_hci::HCI_LE_CS_READ_LOCAL_SUPPORTED_CAPABILITIES_COMMAND =>
                {
                    self.local_channel_sounding_capabilities_status = Some(status);
                    if status == 0 {
                        self.local_channel_sounding_capabilities =
                            Some(ChannelSoundingCapabilities {
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
                            });
                    }
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters: bumble_hci::ReturnParameters::Status { status },
                    ..
                }) if command_opcode
                    == bumble_hci::HCI_LE_CS_READ_LOCAL_SUPPORTED_CAPABILITIES_COMMAND =>
                {
                    self.local_channel_sounding_capabilities_status = Some(status);
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::ReadBdAddr { status: 0, bd_addr },
                    ..
                }) if command_opcode == bumble_hci::HCI_READ_BD_ADDR_COMMAND => {
                    self.public_address = Some(bd_addr);
                    self.ensure_key_store();
                    if let Err(error) = self.refresh_configured_resolving_list(link) {
                        self.key_store_errors.push((None, error.to_string()));
                    }
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::StatusAndConnectionHandle {
                            status,
                            connection_handle,
                        },
                    ..
                }) if command_opcode == bumble_hci::HCI_LE_SETUP_ISO_DATA_PATH_COMMAND => {
                    let pending = self
                        .pending_iso_data_path_setups
                        .iter()
                        .position(|(handle, _)| *handle == connection_handle)
                        .and_then(|index| self.pending_iso_data_path_setups.remove(index));
                    if let Some((_, parameters)) = pending {
                        if status == 0 {
                            self.iso_data_paths.insert(
                                (connection_handle, parameters.direction),
                                parameters.clone(),
                            );
                        }
                        self.iso_control_events
                            .push(IsoControlEvent::DataPathSetup {
                                status,
                                connection_handle,
                                parameters,
                            });
                    }
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::StatusAndConnectionHandle {
                            status,
                            connection_handle,
                        },
                    ..
                }) if command_opcode == bumble_hci::HCI_LE_REMOVE_ISO_DATA_PATH_COMMAND => {
                    let pending = self
                        .pending_iso_data_path_removals
                        .iter()
                        .position(|(handle, _)| *handle == connection_handle)
                        .and_then(|index| self.pending_iso_data_path_removals.remove(index));
                    if let Some((_, directions)) = pending {
                        if status == 0 {
                            for direction in 0..=1 {
                                if directions & (1 << direction) != 0 {
                                    self.iso_data_paths.remove(&(connection_handle, direction));
                                }
                            }
                        }
                        self.iso_control_events
                            .push(IsoControlEvent::DataPathRemoved {
                                status,
                                connection_handle,
                                directions,
                            });
                    }
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::LeReadIsoTxSync {
                            status,
                            connection_handle,
                            packet_sequence_number,
                            tx_time_stamp,
                            time_offset,
                        },
                    ..
                }) if command_opcode == bumble_hci::HCI_LE_READ_ISO_TX_SYNC_COMMAND => {
                    let pending = self
                        .pending_iso_tx_syncs
                        .iter()
                        .position(|handle| *handle == connection_handle)
                        .and_then(|index| self.pending_iso_tx_syncs.remove(index));
                    if pending.is_some() {
                        let sync = (status == 0).then_some(IsoTxSyncInfo {
                            connection_handle,
                            packet_sequence_number,
                            tx_time_stamp,
                            time_offset,
                        });
                        if let Some(sync) = sync {
                            self.iso_tx_syncs.insert(connection_handle, sync);
                        }
                        self.iso_control_events.push(IsoControlEvent::TxSync {
                            status,
                            connection_handle,
                            sync,
                        });
                    }
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters: bumble_hci::ReturnParameters::Status { status },
                    ..
                }) if status != 0
                    && matches!(
                        command_opcode,
                        bumble_hci::HCI_LE_SETUP_ISO_DATA_PATH_COMMAND
                            | bumble_hci::HCI_LE_REMOVE_ISO_DATA_PATH_COMMAND
                            | bumble_hci::HCI_LE_READ_ISO_TX_SYNC_COMMAND
                    ) =>
                {
                    match command_opcode {
                        bumble_hci::HCI_LE_SETUP_ISO_DATA_PATH_COMMAND => {
                            if let Some((connection_handle, parameters)) =
                                self.pending_iso_data_path_setups.pop_front()
                            {
                                self.iso_control_events
                                    .push(IsoControlEvent::DataPathSetup {
                                        status,
                                        connection_handle,
                                        parameters,
                                    });
                            }
                        }
                        bumble_hci::HCI_LE_REMOVE_ISO_DATA_PATH_COMMAND => {
                            if let Some((connection_handle, directions)) =
                                self.pending_iso_data_path_removals.pop_front()
                            {
                                self.iso_control_events
                                    .push(IsoControlEvent::DataPathRemoved {
                                        status,
                                        connection_handle,
                                        directions,
                                    });
                            }
                        }
                        bumble_hci::HCI_LE_READ_ISO_TX_SYNC_COMMAND => {
                            if let Some(connection_handle) = self.pending_iso_tx_syncs.pop_front() {
                                self.iso_control_events.push(IsoControlEvent::TxSync {
                                    status,
                                    connection_handle,
                                    sync: None,
                                });
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::LeReadPhy {
                            status,
                            connection_handle,
                            tx_phy,
                            rx_phy,
                        },
                    ..
                }) if command_opcode == bumble_hci::HCI_LE_READ_PHY_COMMAND => {
                    let phy = LePhy { tx_phy, rx_phy };
                    if status == 0 {
                        if let Some(connection) = self.le_connections.get_mut(&connection_handle) {
                            connection.phy = phy;
                        }
                    }
                    self.complete_connection_control(command_opcode, connection_handle);
                    self.record_connection_control_event(LeConnectionControlEvent::PhyRead {
                        status,
                        connection_handle,
                        phy,
                    });
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters: bumble_hci::ReturnParameters::Status { status },
                    ..
                }) if status != 0
                    && matches!(
                        command_opcode,
                        bumble_hci::HCI_LE_READ_PHY_COMMAND
                            | bumble_hci::HCI_LE_SET_DATA_LENGTH_COMMAND
                            | bumble_hci::HCI_READ_RSSI_COMMAND
                            | bumble_hci::HCI_LE_CONNECTION_UPDATE_COMMAND
                            | bumble_hci::HCI_LE_CONNECTION_RATE_REQUEST_COMMAND
                    ) =>
                {
                    let connection_handle = self.fail_next_connection_control(command_opcode);
                    self.record_connection_control_event(LeConnectionControlEvent::CommandStatus {
                        command_opcode,
                        status,
                        connection_handle,
                    });
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters: bumble_hci::ReturnParameters::Raw { data },
                    ..
                }) if command_opcode == bumble_hci::HCI_LE_SET_DATA_LENGTH_COMMAND
                    && data.len() >= 3 =>
                {
                    let status = data[0];
                    let connection_handle = u16::from_le_bytes([data[1], data[2]]);
                    self.complete_connection_control(command_opcode, connection_handle);
                    self.record_connection_control_event(
                        LeConnectionControlEvent::DataLengthRequestComplete {
                            status,
                            connection_handle,
                        },
                    );
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters:
                        bumble_hci::ReturnParameters::ReadRssi {
                            status,
                            handle: connection_handle,
                            rssi,
                        },
                    ..
                }) if command_opcode == bumble_hci::HCI_READ_RSSI_COMMAND => {
                    if status == 0 {
                        if let Some(connection) = self.le_connections.get_mut(&connection_handle) {
                            connection.rssi = Some(rssi);
                        }
                    }
                    self.complete_connection_control(command_opcode, connection_handle);
                    self.record_connection_control_event(LeConnectionControlEvent::RssiRead {
                        status,
                        connection_handle,
                        rssi,
                    });
                }
                HciPacket::Event(Event::CommandStatus {
                    status,
                    command_opcode,
                    ..
                }) if command_opcode == bumble_hci::HCI_REMOTE_NAME_REQUEST_COMMAND => {
                    let peer_address = self.pending_remote_name_commands.pop_front();
                    if status != 0 {
                        if let Some(peer_address) = peer_address {
                            self.complete_remote_name_request(&peer_address);
                            self.record_remote_name_result(
                                peer_address,
                                Err(RemoteNameError::HciStatus(status)),
                            );
                        }
                    }
                }
                HciPacket::Event(Event::CommandStatus {
                    status,
                    command_opcode,
                    ..
                }) if status != 0
                    && matches!(
                        command_opcode,
                        bumble_hci::HCI_LE_CONNECTION_UPDATE_COMMAND
                            | bumble_hci::HCI_LE_SET_PHY_COMMAND
                            | bumble_hci::HCI_LE_SUBRATE_REQUEST_COMMAND
                            | bumble_hci::HCI_LE_CONNECTION_RATE_REQUEST_COMMAND
                    ) =>
                {
                    let connection_handle = self.fail_next_connection_control(command_opcode);
                    self.record_connection_control_event(LeConnectionControlEvent::CommandStatus {
                        command_opcode,
                        status,
                        connection_handle,
                    });
                }
                HciPacket::Event(Event::CommandStatus {
                    status,
                    command_opcode,
                    ..
                }) if matches!(
                    command_opcode,
                    bumble_hci::HCI_LE_CREATE_CIS_COMMAND
                        | bumble_hci::HCI_LE_ACCEPT_CIS_REQUEST_COMMAND
                ) =>
                {
                    self.cis_control_events
                        .push(CisControlEvent::CommandStatus {
                            command_opcode,
                            status,
                        });
                }
                HciPacket::Event(Event::CommandStatus {
                    status,
                    command_opcode,
                    ..
                }) if matches!(
                    command_opcode,
                    bumble_hci::HCI_LE_CREATE_BIG_COMMAND
                        | bumble_hci::HCI_LE_BIG_CREATE_SYNC_COMMAND
                ) =>
                {
                    let pending_handle = if command_opcode == bumble_hci::HCI_LE_CREATE_BIG_COMMAND
                    {
                        self.pending_big_commands.pop_front()
                    } else {
                        self.pending_big_sync_commands.pop_front()
                    };
                    if status != 0 {
                        if let Some(big_handle) = pending_handle {
                            if command_opcode == bumble_hci::HCI_LE_CREATE_BIG_COMMAND {
                                self.pending_bigs.remove(&big_handle);
                                self.bigs.remove(&big_handle);
                            } else {
                                self.pending_big_syncs.remove(&big_handle);
                                self.big_syncs.remove(&big_handle);
                            }
                            self.big_errors.push((big_handle, status));
                        }
                    }
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters: bumble_hci::ReturnParameters::Status { status },
                    ..
                }) if command_opcode == bumble_hci::HCI_LE_REJECT_CIS_REQUEST_COMMAND => {
                    self.cis_control_events
                        .push(CisControlEvent::CommandStatus {
                            command_opcode,
                            status,
                        });
                }
                HciPacket::Event(Event::CommandComplete {
                    command_opcode,
                    return_parameters: bumble_hci::ReturnParameters::Raw { data },
                    ..
                }) if command_opcode == bumble_hci::HCI_LE_SET_CIG_PARAMETERS_COMMAND => {
                    if data.len() >= 3 {
                        let status = data[0];
                        let cig_id = data[1];
                        let count = usize::from(data[2]);
                        if data.len() == 3 + count * 2 {
                            let connection_handles = data[3..]
                                .chunks_exact(2)
                                .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                                .collect::<Vec<_>>();
                            if status == 0 {
                                self.configured_cis_handles = connection_handles.clone();
                            }
                            self.cis_control_events
                                .push(CisControlEvent::CigConfigured {
                                    status,
                                    cig_id,
                                    connection_handles,
                                });
                        }
                    }
                }
                HciPacket::Event(Event::DisconnectionComplete {
                    status,
                    connection_handle,
                    reason,
                }) => {
                    self.pending_disconnections.remove(&connection_handle);
                    let known_connection = self.le_connections.contains_key(&connection_handle)
                        || self.classic_connections.contains_key(&connection_handle)
                        || self.cis_links.contains_key(&connection_handle)
                        || self
                            .synchronous_connections
                            .iter()
                            .any(|connection| connection.connection_handle == connection_handle);
                    if status != 0 {
                        if known_connection {
                            self.emit_device_event(DeviceEvent::DisconnectionFailed {
                                connection_handle,
                                status,
                            });
                        }
                        continue;
                    }
                    let disconnected_classic_peer = self
                        .classic_connections
                        .get(&connection_handle)
                        .map(|connection| connection.peer_address.clone());
                    self.encrypted_handles.remove(&connection_handle);
                    self.cis_links.remove(&connection_handle);
                    self.clear_iso_control_state(connection_handle);
                    self.iso_sequence_numbers.remove(&connection_handle);
                    self.iso_assemblers.remove(&connection_handle);
                    self.iso_inbox
                        .retain(|sdu| sdu.connection_handle != connection_handle);
                    self.acl_assemblers.remove(&connection_handle);
                    self.acl_packet_queue.flush(connection_handle);
                    if let Some(queue) = self.le_acl_packet_queue.as_mut() {
                        queue.flush(connection_handle);
                    }
                    if let Some(queue) = self.iso_packet_queue.as_mut() {
                        queue.flush(connection_handle);
                    }
                    self.pending_connection_controls.retain(|_, handles| {
                        handles.retain(|handle| *handle != connection_handle);
                        !handles.is_empty()
                    });
                    let eatt_cids: Vec<u16> = self
                        .le_credit_managers
                        .get(&connection_handle)
                        .into_iter()
                        .flat_map(LeCreditChannelManager::channels)
                        .filter(|channel| channel.psm == EATT_PSM)
                        .map(|channel| channel.source_cid)
                        .collect();
                    self.remove_att_bearer_state(connection_handle, ATT_CID);
                    for source_cid in eatt_cids {
                        self.remove_att_bearer_state(connection_handle, source_cid);
                    }
                    self.le_connections.remove(&connection_handle);
                    self.le_credit_managers.remove(&connection_handle);
                    self.le_credit_errors
                        .retain(|(handle, _)| *handle != connection_handle);
                    self.classic_channel_managers.remove(&connection_handle);
                    self.classic_channel_errors
                        .retain(|(handle, _)| *handle != connection_handle);
                    self.inbox
                        .retain(|(handle, _)| *handle != connection_handle);
                    self.l2cap_inbox
                        .retain(|(handle, _, _)| *handle != connection_handle);
                    self.security_requests
                        .retain(|(handle, _)| *handle != connection_handle);
                    if let Some(manager) = self.pairing_manager.as_mut() {
                        manager.disconnect(connection_handle);
                    }
                    self.classic_link_keys.remove(&connection_handle);
                    self.pairing_encryption_started.remove(&connection_handle);
                    self.pairing_terminal_handles.remove(&connection_handle);
                    self.pairing_errors
                        .retain(|(handle, _)| *handle != connection_handle);
                    self.long_term_key_requests
                        .retain(|request| request.connection_handle != connection_handle);
                    self.connection_feature_errors
                        .retain(|error| error.connection_handle != connection_handle);
                    self.pending_channel_sounding_configs
                        .retain(|(handle, _)| *handle != connection_handle);
                    self.channel_sounding_errors
                        .retain(|error| error.connection_handle != connection_handle);
                    self.channel_sounding_security_results
                        .retain(|(handle, _)| *handle != connection_handle);
                    if let Some(peer_address) = disconnected_classic_peer.as_ref() {
                        self.classic_pairing_events
                            .retain(|event| !event.belongs_to(connection_handle, peer_address));
                    }
                    if self.connection_handle == Some(connection_handle) {
                        if let Some(next_handle) = self.le_connections.keys().next().copied() {
                            self.select_connection(next_handle);
                        } else {
                            self.connection_handle = None;
                            self.connection_role = None;
                            self.peer_address = None;
                        }
                    }
                    if self.classic_connection_handle == Some(connection_handle) {
                        self.classic_connections.remove(&connection_handle);
                        if let Some(next_handle) = self.classic_connections.keys().next().copied() {
                            self.select_classic_connection(next_handle);
                        } else {
                            self.classic_connection_handle = None;
                            self.classic_connection_role = None;
                        }
                    } else {
                        self.classic_connections.remove(&connection_handle);
                    }
                    self.synchronous_connections
                        .retain(|connection| connection.connection_handle != connection_handle);
                    if known_connection {
                        self.emit_device_event(DeviceEvent::Disconnected {
                            connection_handle,
                            reason,
                        });
                    }
                }
                HciPacket::Event(Event::InquiryComplete { status }) => {
                    self.classic_inquiry_complete.push(status);
                    self.emit_device_event(DeviceEvent::InquiryComplete { status });
                    if self.classic_discovering && self.classic_auto_restart_inquiry {
                        let lookup_owned = self.peer_lookup_started_discovery;
                        self.start_discovery(link, true);
                        self.peer_lookup_started_discovery = lookup_owned;
                    } else {
                        self.classic_auto_restart_inquiry = true;
                        self.classic_discovering = false;
                    }
                }
                HciPacket::Event(Event::InquiryResult {
                    bd_addr,
                    class_of_device,
                    ..
                }) => {
                    for (index, peer_address) in bd_addr.into_iter().enumerate() {
                        self.classic_inquiry_results.push(peer_address.clone());
                        let result = ClassicInquiryResultInfo {
                            peer_address,
                            class_of_device: class_of_device
                                .get(index)
                                .copied()
                                .unwrap_or_default(),
                            rssi: None,
                            extended_inquiry_response: Vec::new(),
                        };
                        self.classic_inquiry_result_details.push(result.clone());
                        self.emit_device_event(DeviceEvent::InquiryResult(result.clone()));
                        self.complete_peer_lookups(
                            link,
                            PeerLookupTransport::Classic,
                            &result.peer_address,
                            &AdvertisingData::from_bytes(&result.extended_inquiry_response),
                        );
                    }
                }
                HciPacket::Event(Event::InquiryResultWithRssi {
                    bd_addr,
                    class_of_device,
                    rssi,
                    ..
                }) => {
                    for (index, peer_address) in bd_addr.into_iter().enumerate() {
                        self.classic_inquiry_results.push(peer_address.clone());
                        let result = ClassicInquiryResultInfo {
                            peer_address,
                            class_of_device: class_of_device
                                .get(index)
                                .copied()
                                .unwrap_or_default(),
                            rssi: rssi.get(index).copied(),
                            extended_inquiry_response: Vec::new(),
                        };
                        self.classic_inquiry_result_details.push(result.clone());
                        self.emit_device_event(DeviceEvent::InquiryResult(result.clone()));
                        self.complete_peer_lookups(
                            link,
                            PeerLookupTransport::Classic,
                            &result.peer_address,
                            &AdvertisingData::from_bytes(&result.extended_inquiry_response),
                        );
                    }
                }
                HciPacket::Event(Event::ExtendedInquiryResult {
                    bd_addr,
                    class_of_device,
                    rssi,
                    extended_inquiry_response,
                    ..
                }) => {
                    self.classic_inquiry_results.push(bd_addr.clone());
                    let result = ClassicInquiryResultInfo {
                        peer_address: bd_addr,
                        class_of_device,
                        rssi: Some(rssi),
                        extended_inquiry_response: extended_inquiry_response.to_vec(),
                    };
                    self.classic_inquiry_result_details.push(result.clone());
                    self.emit_device_event(DeviceEvent::InquiryResult(result.clone()));
                    self.complete_peer_lookups(
                        link,
                        PeerLookupTransport::Classic,
                        &result.peer_address,
                        &AdvertisingData::from_bytes(&result.extended_inquiry_response),
                    );
                }
                HciPacket::Event(Event::RemoteNameRequestComplete {
                    status,
                    bd_addr,
                    remote_name,
                }) => {
                    self.remove_pending_remote_name_command(&bd_addr);
                    self.complete_remote_name_request(&bd_addr);
                    if status != 0 {
                        self.record_remote_name_result(
                            bd_addr,
                            Err(RemoteNameError::HciStatus(status)),
                        );
                        continue;
                    }
                    let length = remote_name
                        .iter()
                        .position(|byte| *byte == 0)
                        .unwrap_or(remote_name.len());
                    match std::str::from_utf8(&remote_name[..length]) {
                        Ok(name) => {
                            self.record_remote_name_result(bd_addr, Ok(name.to_owned()));
                        }
                        Err(error) => {
                            self.record_remote_name_result(
                                bd_addr,
                                Err(RemoteNameError::InvalidUtf8 {
                                    valid_up_to: error.valid_up_to(),
                                    error_len: error.error_len(),
                                }),
                            );
                        }
                    }
                }
                HciPacket::Event(Event::ReadRemoteSupportedFeaturesComplete {
                    status,
                    connection_handle,
                    lmp_features,
                }) => {
                    let mut request_extended_page_zero = false;
                    if let Some(connection) = self.classic_connections.get_mut(&connection_handle) {
                        if status == 0 {
                            connection.peer_lmp_features.insert(0, lmp_features);
                            request_extended_page_zero = lmp_features[7] & 0x80 != 0;
                            connection.peer_lmp_max_page_number =
                                (!request_extended_page_zero).then_some(0);
                        } else {
                            self.connection_feature_errors.push(ConnectionFeatureError {
                                transport: ConnectionFeatureTransport::Classic,
                                connection_handle,
                                page_number: None,
                                status,
                            });
                        }
                    }
                    if request_extended_page_zero {
                        self.send_hci_command(
                            link,
                            Command::ReadRemoteExtendedFeatures {
                                connection_handle,
                                page_number: 0,
                            },
                        );
                    }
                }
                HciPacket::Event(Event::ReadRemoteExtendedFeaturesComplete {
                    status,
                    connection_handle,
                    page_number,
                    maximum_page_number,
                    extended_lmp_features,
                }) => {
                    let mut next_page = None;
                    if let Some(connection) = self.classic_connections.get_mut(&connection_handle) {
                        if status == 0 {
                            connection
                                .peer_lmp_features
                                .insert(page_number, extended_lmp_features);
                            connection.peer_lmp_max_page_number = Some(maximum_page_number);
                            next_page = page_number
                                .checked_add(1)
                                .filter(|page| *page <= maximum_page_number);
                        } else {
                            self.connection_feature_errors.push(ConnectionFeatureError {
                                transport: ConnectionFeatureTransport::Classic,
                                connection_handle,
                                page_number: Some(page_number),
                                status,
                            });
                        }
                    }
                    if let Some(page_number) = next_page {
                        self.send_hci_command(
                            link,
                            Command::ReadRemoteExtendedFeatures {
                                connection_handle,
                                page_number,
                            },
                        );
                    }
                }
                HciPacket::Event(Event::ConnectionComplete {
                    status,
                    connection_handle,
                    bd_addr,
                    link_type: 1,
                    encryption_enabled,
                }) => {
                    if status == 0 {
                        let role = self
                            .pending_classic_roles
                            .iter()
                            .position(|(address, _)| *address == bd_addr)
                            .map(|index| self.pending_classic_roles.remove(index).1)
                            .unwrap_or(bumble_controller::ROLE_CENTRAL);
                        self.classic_connections.insert(
                            connection_handle,
                            ClassicConnectionInfo {
                                connection_handle,
                                role,
                                peer_address: bd_addr,
                                peer_name: None,
                                encryption_enabled,
                                encryption_key_size: 0,
                                qos_service_type: None,
                                classic_mode: 0,
                                classic_interval: 0,
                                peer_lmp_features: BTreeMap::new(),
                                peer_lmp_max_page_number: None,
                                peer_host_supported_features: None,
                            },
                        );
                        if encryption_enabled != 0 {
                            self.encrypted_handles.insert(connection_handle);
                        } else {
                            self.encrypted_handles.remove(&connection_handle);
                        }
                        if self.pairing_manager.is_some() && self.config.classic_smp_enabled {
                            if let Err(error) = self.load_classic_link_key(connection_handle) {
                                self.key_store_errors
                                    .push((Some(connection_handle), error.to_string()));
                            }
                            self.synchronize_classic_pairing_connection(connection_handle);
                        }
                        let mut manager = ClassicChannelManager::with_information_capabilities(
                            self.l2cap_information_capabilities(),
                        );
                        for (psm, spec) in &self.classic_channel_server_specs {
                            manager
                                .register_server(Some(*psm), *spec)
                                .expect("stored Classic channel server spec is valid");
                        }
                        self.classic_channel_managers
                            .insert(connection_handle, manager);
                        self.select_classic_connection(connection_handle);
                        let connection = self
                            .classic_connections
                            .get(&connection_handle)
                            .expect("connection was just inserted")
                            .clone();
                        self.emit_device_event(DeviceEvent::ClassicConnectionEstablished(
                            connection,
                        ));
                    } else {
                        self.pending_classic_roles
                            .retain(|(address, _)| *address != bd_addr);
                        self.emit_device_event(DeviceEvent::ConnectionFailed {
                            transport: DeviceConnectionTransport::Classic,
                            peer_address: bd_addr,
                            status,
                        });
                    }
                }
                HciPacket::Event(Event::RoleChange {
                    status: 0,
                    bd_addr,
                    new_role,
                }) => {
                    if let Some(handle) = self
                        .classic_connections
                        .values()
                        .find(|connection| connection.peer_address == bd_addr)
                        .map(|connection| connection.connection_handle)
                    {
                        if let Some(connection) = self.classic_connections.get_mut(&handle) {
                            connection.role = new_role;
                        }
                        if self.classic_connection_handle == Some(handle) {
                            self.classic_connection_role = Some(new_role);
                        }
                    } else if let Some((_, role)) = self
                        .pending_classic_roles
                        .iter_mut()
                        .find(|(address, _)| *address == bd_addr)
                    {
                        *role = new_role;
                    } else {
                        self.set_pending_classic_role(bd_addr, new_role);
                    }
                }
                HciPacket::Event(Event::ConnectionRequest {
                    bd_addr,
                    class_of_device,
                    link_type,
                }) => {
                    if link_type == 1 {
                        if self.config.classic_enabled && self.config.classic_accept_any {
                            self.accept_classic(link, bd_addr.clone());
                        } else {
                            self.classic_connection_requests.push(bd_addr.clone());
                        }
                    } else {
                        self.synchronous_requests.push((bd_addr.clone(), link_type));
                    }
                    self.emit_device_event(DeviceEvent::ConnectionRequest {
                        peer_address: bd_addr,
                        class_of_device,
                        link_type,
                    });
                }
                HciPacket::Event(Event::SynchronousConnectionComplete {
                    status: 0,
                    connection_handle,
                    bd_addr,
                    link_type,
                    air_mode,
                    ..
                }) => {
                    let connection = SynchronousConnectionInfo {
                        connection_handle,
                        peer_address: bd_addr,
                        link_type,
                        air_mode,
                    };
                    self.synchronous_connections.push(connection.clone());
                    self.emit_device_event(DeviceEvent::SynchronousConnectionEstablished(
                        connection,
                    ));
                }
                HciPacket::Event(Event::SynchronousConnectionComplete {
                    status,
                    bd_addr,
                    link_type,
                    ..
                }) => self.emit_device_event(DeviceEvent::ConnectionFailed {
                    transport: DeviceConnectionTransport::Synchronous { link_type },
                    peer_address: bd_addr,
                    status,
                }),
                HciPacket::Event(Event::AuthenticationComplete {
                    status,
                    connection_handle,
                }) => {
                    self.record_classic_pairing_event(ClassicPairingEvent::AuthenticationComplete {
                        status,
                        connection_handle,
                    })
                }
                HciPacket::Event(Event::ModeChange {
                    status: 0,
                    connection_handle,
                    current_mode,
                    interval,
                }) => {
                    if let Some(connection) = self.le_connections.get_mut(&connection_handle) {
                        connection.classic_mode = current_mode;
                        connection.classic_interval = interval;
                    }
                    if let Some(connection) = self.classic_connections.get_mut(&connection_handle) {
                        connection.classic_mode = current_mode;
                        connection.classic_interval = interval;
                    }
                }
                HciPacket::Event(Event::PinCodeRequest { bd_addr }) => self
                    .record_classic_pairing_event(ClassicPairingEvent::PinCodeRequest {
                        peer_address: bd_addr,
                    }),
                HciPacket::Event(Event::LinkKeyRequest { bd_addr }) => {
                    if self.pairing_manager.is_some() {
                        let command = match self.stored_classic_link_key(&bd_addr) {
                            Ok(Some((link_key, authenticated))) => {
                                if let Some(connection_handle) =
                                    self.classic_connection_handle_for_peer(&bd_addr)
                                {
                                    self.classic_link_keys
                                        .insert(connection_handle, (link_key, authenticated));
                                }
                                Command::LinkKeyRequestReply {
                                    bd_addr: bd_addr.clone(),
                                    link_key,
                                }
                            }
                            Ok(None) => Command::LinkKeyRequestNegativeReply {
                                bd_addr: bd_addr.clone(),
                            },
                            Err(error) => {
                                let connection_handle =
                                    self.classic_connection_handle_for_peer(&bd_addr);
                                self.key_store_errors
                                    .push((connection_handle, error.to_string()));
                                Command::LinkKeyRequestNegativeReply {
                                    bd_addr: bd_addr.clone(),
                                }
                            }
                        };
                        self.send_hci_command(link, command);
                    }
                    self.record_classic_pairing_event(ClassicPairingEvent::LinkKeyRequest {
                        peer_address: bd_addr,
                    });
                }
                HciPacket::Event(Event::LinkKeyNotification {
                    bd_addr,
                    link_key,
                    key_type,
                }) => {
                    if self.pairing_manager.is_some() {
                        self.persist_classic_link_key(link, &bd_addr, link_key, key_type);
                    }
                    self.record_classic_pairing_event(ClassicPairingEvent::LinkKeyNotification {
                        peer_address: bd_addr,
                        link_key,
                        key_type,
                    });
                }
                HciPacket::Event(Event::IoCapabilityRequest { bd_addr }) => self
                    .record_classic_pairing_event(ClassicPairingEvent::IoCapabilityRequest {
                        peer_address: bd_addr,
                    }),
                HciPacket::Event(Event::IoCapabilityResponse {
                    bd_addr,
                    io_capability,
                    authentication_requirements,
                    ..
                }) => {
                    self.record_classic_pairing_event(ClassicPairingEvent::IoCapabilityResponse {
                        peer_address: bd_addr,
                        io_capability,
                        authentication_requirements,
                    })
                }
                HciPacket::Event(Event::UserConfirmationRequest {
                    bd_addr,
                    numeric_value,
                }) => self.record_classic_pairing_event(
                    ClassicPairingEvent::UserConfirmationRequest {
                        peer_address: bd_addr,
                        numeric_value,
                    },
                ),
                HciPacket::Event(Event::UserPasskeyRequest { bd_addr }) => self
                    .record_classic_pairing_event(ClassicPairingEvent::UserPasskeyRequest {
                        peer_address: bd_addr,
                    }),
                HciPacket::Event(Event::RemoteOobDataRequest { bd_addr }) => self
                    .record_classic_pairing_event(ClassicPairingEvent::RemoteOobDataRequest {
                        peer_address: bd_addr,
                    }),
                HciPacket::Event(Event::SimplePairingComplete { status, bd_addr }) => self
                    .record_classic_pairing_event(ClassicPairingEvent::SimplePairingComplete {
                        status,
                        peer_address: bd_addr,
                    }),
                HciPacket::Event(Event::UserPasskeyNotification { bd_addr, passkey }) => self
                    .record_classic_pairing_event(ClassicPairingEvent::UserPasskeyNotification {
                        peer_address: bd_addr,
                        passkey,
                    }),
                HciPacket::Event(Event::NumberOfCompletedPackets {
                    connection_handles,
                    num_completed_packets,
                }) => {
                    for (handle, count) in connection_handles
                        .into_iter()
                        .zip(num_completed_packets.into_iter())
                    {
                        let count = usize::from(count);
                        if self.cis_links.contains_key(&handle)
                            || self.bis_directions.contains_key(&handle)
                        {
                            if let Some(queue) = self.iso_packet_queue.as_mut() {
                                let _ = queue.on_packets_completed(count, handle);
                            }
                        } else if self.le_connections.contains_key(&handle)
                            && self.le_acl_packet_queue.is_some()
                        {
                            let _ = self
                                .le_acl_packet_queue
                                .as_mut()
                                .expect("separate LE ACL queue exists")
                                .on_packets_completed(count, handle);
                        } else {
                            let _ = self.acl_packet_queue.on_packets_completed(count, handle);
                        }
                    }
                    self.flush_acl_queue(link);
                    self.flush_iso_queue(link);
                }
                HciPacket::Event(Event::EncryptionChange {
                    status,
                    connection_handle,
                    encryption_enabled,
                }) => {
                    if status == 0 {
                        self.update_connection_encryption(connection_handle, encryption_enabled, 0);
                        self.synchronize_classic_pairing_connection(connection_handle);
                        self.advance_pairing_encryption(
                            link,
                            connection_handle,
                            encryption_enabled,
                        );
                    }
                    self.emit_device_event(DeviceEvent::EncryptionChange {
                        status,
                        connection_handle,
                        encryption_enabled,
                        encryption_key_size: 0,
                    });
                }
                HciPacket::Event(Event::EncryptionChangeV2 {
                    status,
                    connection_handle,
                    encryption_enabled,
                    encryption_key_size,
                }) => {
                    if status == 0 {
                        self.update_connection_encryption(
                            connection_handle,
                            encryption_enabled,
                            encryption_key_size,
                        );
                        self.synchronize_classic_pairing_connection(connection_handle);
                        self.advance_pairing_encryption(
                            link,
                            connection_handle,
                            encryption_enabled,
                        );
                    }
                    self.emit_device_event(DeviceEvent::EncryptionChange {
                        status,
                        connection_handle,
                        encryption_enabled,
                        encryption_key_size,
                    });
                }
                HciPacket::Event(Event::EncryptionKeyRefreshComplete {
                    status: 0,
                    connection_handle,
                }) => {
                    self.emit_device_event(DeviceEvent::EncryptionKeyRefresh { connection_handle })
                }
                HciPacket::Event(Event::EncryptionKeyRefreshComplete {
                    status,
                    connection_handle,
                }) => self.emit_device_event(DeviceEvent::EncryptionKeyRefreshFailed {
                    connection_handle,
                    status,
                }),
                HciPacket::Event(Event::QosSetupComplete {
                    status: 0,
                    connection_handle,
                    service_type,
                    ..
                }) => {
                    self.update_connection_qos(connection_handle, service_type);
                    self.emit_device_event(DeviceEvent::QosSetup {
                        connection_handle,
                        service_type,
                    });
                }
                HciPacket::Event(Event::QosSetupComplete {
                    status,
                    connection_handle,
                    ..
                }) => self.emit_device_event(DeviceEvent::QosSetupFailed {
                    connection_handle,
                    status,
                }),
                HciPacket::Event(Event::RemoteHostSupportedFeaturesNotification {
                    bd_addr,
                    host_supported_features,
                }) => {
                    if let Some(connection) = self
                        .classic_connections
                        .values_mut()
                        .find(|connection| connection.peer_address == bd_addr)
                    {
                        connection.peer_host_supported_features = Some(host_supported_features);
                    }
                    self.emit_device_event(DeviceEvent::RemoteHostSupportedFeatures {
                        peer_address: bd_addr,
                        host_supported_features,
                    });
                }
                HciPacket::Event(Event::Vendor { data }) => {
                    self.vendor_events.push(data.clone());
                    self.emit_device_event(DeviceEvent::VendorEvent(data));
                }
                HciPacket::AclData(acl) => self.on_acl(link, acl),
                HciPacket::SyncData(packet) => self.synchronous_inbox.push(packet),
                HciPacket::IsoData(packet) => {
                    let handle = packet.connection_handle;
                    if let Some(sdu) = self.iso_assemblers.entry(handle).or_default().push(packet) {
                        self.iso_inbox.push(sdu);
                    }
                }
                _ => {}
            }
        }
        true
    }

    fn clear_bis_handle(&mut self, handle: u16) {
        self.bis_directions.remove(&handle);
        self.clear_iso_control_state(handle);
        self.iso_sequence_numbers.remove(&handle);
        self.iso_assemblers.remove(&handle);
        self.iso_inbox.retain(|sdu| sdu.connection_handle != handle);
    }

    fn clear_iso_control_state(&mut self, handle: u16) {
        self.iso_data_paths
            .retain(|(connection_handle, _), _| *connection_handle != handle);
        self.pending_iso_data_path_setups
            .retain(|(connection_handle, _)| *connection_handle != handle);
        self.pending_iso_data_path_removals
            .retain(|(connection_handle, _)| *connection_handle != handle);
        self.pending_iso_tx_syncs
            .retain(|connection_handle| *connection_handle != handle);
        self.iso_tx_syncs.remove(&handle);
    }

    fn flush_acl_queue(&mut self, link: &mut LocalLink) -> bool {
        let mut success = true;
        while let Some(packet) = self.acl_packet_queue.poll_ready() {
            let handle = packet.connection_handle;
            if !link.send_acl_packet(self.controller_id, packet) {
                let _ = self.acl_packet_queue.on_packets_completed(1, handle);
                success = false;
            }
        }
        if let Some(queue) = self.le_acl_packet_queue.as_mut() {
            while let Some(packet) = queue.poll_ready() {
                let handle = packet.connection_handle;
                if !link.send_acl_packet(self.controller_id, packet) {
                    let _ = queue.on_packets_completed(1, handle);
                    success = false;
                }
            }
        }
        success
    }

    fn flush_iso_queue(&mut self, link: &mut LocalLink) -> bool {
        let Some(queue) = self.iso_packet_queue.as_mut() else {
            return true;
        };
        let mut success = true;
        while let Some(packet) = queue.poll_ready() {
            let handle = packet.connection_handle;
            if !link.send_iso_packet(self.controller_id, packet) {
                let _ = queue.on_packets_completed(1, handle);
                success = false;
            }
        }
        success
    }

    fn att_access_context(&self, connection_handle: u16, source_cid: u16) -> AccessContext {
        AccessContext {
            bearer_id: if source_cid == ATT_CID {
                att_bearer_id(connection_handle)
            } else {
                eatt_bearer_id(connection_handle, source_cid)
            },
            encrypted: self.encrypted_handles.contains(&connection_handle),
            authenticated: false,
            authorized: false,
        }
    }

    fn remove_att_bearer_state(&mut self, connection_handle: u16, source_cid: u16) {
        let bearer_id = if source_cid == ATT_CID {
            att_bearer_id(connection_handle)
        } else {
            eatt_bearer_id(connection_handle, source_cid)
        };
        if let Some(server) = self.server.as_mut() {
            server.remove_bearer(bearer_id);
        }
        self.eatt_inbox
            .retain(|(handle, cid, _)| *handle != connection_handle || *cid != source_cid);
        self.pending_att_indications
            .remove(&(connection_handle, source_cid));
    }

    fn process_eatt_bearer(
        &mut self,
        link: &mut LocalLink,
        connection_handle: u16,
        source_cid: u16,
    ) -> bumble_l2cap::Result<()> {
        let sdus = self.take_le_credit_sdus(connection_handle, source_cid);
        for bytes in sdus {
            let pdu = AttPdu::from_bytes(&bytes).map_err(|error| {
                L2capError::InvalidPacket(format!(
                    "invalid ATT PDU on EATT CID {source_cid:#06x}: {error}"
                ))
            })?;
            let context = self.att_access_context(connection_handle, source_cid);
            if pdu == AttPdu::HandleValueConfirmation
                && self
                    .pending_att_indications
                    .remove(&(connection_handle, source_cid))
            {
                continue;
            }
            if pdu.is_command() {
                if let Some(server) = self.server.as_mut() {
                    let _ = server.handle_request_with_context(&pdu, context);
                }
                continue;
            }
            if is_request(&pdu) {
                if let Some(response) = self
                    .server
                    .as_mut()
                    .map(|server| server.handle_request_with_context(&pdu, context))
                {
                    self.send_eatt(link, connection_handle, source_cid, &response)?;
                    continue;
                }
            }
            self.eatt_inbox.push((connection_handle, source_cid, pdu));
        }
        Ok(())
    }

    fn on_acl(&mut self, link: &mut LocalLink, acl: AclDataPacket) {
        let handle = acl.connection_handle;
        let Ok(Some(data)) = self.acl_assemblers.entry(handle).or_default().feed(&acl) else {
            return;
        };
        let Ok(l2cap) = L2capPdu::from_bytes(&data) else {
            return;
        };
        let managed_classic_pdu =
            self.classic_channel_managers
                .get(&handle)
                .is_some_and(|manager| {
                    l2cap.cid == L2CAP_SIGNALING_CID || manager.channel(l2cap.cid).is_some()
                });
        if managed_classic_pdu {
            if let Err(error) = self
                .classic_channel_managers
                .get_mut(&handle)
                .expect("manager was just found")
                .process_pdu(l2cap)
            {
                self.classic_channel_errors
                    .push((handle, error.to_string()));
                return;
            }
            if let Err(error) = self.flush_classic_channel_manager(link, handle) {
                self.classic_channel_errors
                    .push((handle, error.to_string()));
            }
            return;
        }
        let managed_le_credit_pdu = self.le_credit_managers.get(&handle).is_some_and(|manager| {
            l2cap.cid == L2CAP_LE_SIGNALING_CID || manager.channel(l2cap.cid).is_some()
        });
        if managed_le_credit_pdu {
            let source_cid = l2cap.cid;
            let eatt_before: BTreeSet<u16> = self
                .le_credit_managers
                .get(&handle)
                .into_iter()
                .flat_map(LeCreditChannelManager::channels)
                .filter(|channel| channel.psm == EATT_PSM)
                .map(|channel| channel.source_cid)
                .collect();
            if let Err(error) = self
                .le_credit_managers
                .get_mut(&handle)
                .expect("manager was just found")
                .process_pdu(l2cap)
            {
                self.le_credit_errors.push((handle, error.to_string()));
                return;
            }
            if let Err(error) = self.flush_le_credit_manager(link, handle) {
                self.le_credit_errors.push((handle, error.to_string()));
            }
            let eatt_after: BTreeSet<u16> = self
                .le_credit_managers
                .get(&handle)
                .into_iter()
                .flat_map(LeCreditChannelManager::channels)
                .filter(|channel| channel.psm == EATT_PSM)
                .map(|channel| channel.source_cid)
                .collect();
            for closed_cid in eatt_before.difference(&eatt_after) {
                self.remove_att_bearer_state(handle, *closed_cid);
            }
            if eatt_after.contains(&source_cid) {
                if let Err(error) = self.process_eatt_bearer(link, handle, source_cid) {
                    self.le_credit_errors.push((handle, error.to_string()));
                }
            }
            return;
        }
        // Configured devices route LE and BR/EDR SMP through their handle-keyed
        // pairing manager. Client-only devices retain the raw channel behavior.
        if l2cap.cid != ATT_CID {
            if l2cap.cid == SMP_CID && l2cap.payload.len() == 2 && l2cap.payload[0] == 0x0B {
                self.security_requests.push((handle, l2cap.payload[1]));
            }
            if l2cap.cid == SMP_BR_CID
                && self.config.classic_smp_enabled
                && self.pairing_manager.is_some()
            {
                let pdu = match SmpPdu::from_bytes(&l2cap.payload) {
                    Ok(pdu) => pdu,
                    Err(error) => {
                        self.pairing_errors.push((handle, error.to_string()));
                        return;
                    }
                };
                let registered = self
                    .pairing_manager
                    .as_ref()
                    .is_some_and(|manager| manager.has_connection(handle));
                if !registered {
                    if matches!(pdu, SmpPdu::PairingRequest(_)) {
                        let failure = SmpPdu::PairingFailed {
                            reason: PairingFailureReason::CrossTransportKeyDerivationNotAllowed
                                as u8,
                        };
                        self.send_l2cap_on_handle(link, handle, SMP_BR_CID, &failure.to_bytes());
                    }
                    self.pairing_errors.push((
                        handle,
                        "BR/EDR SMP requires an encrypted connection with a stored Link Key".into(),
                    ));
                    return;
                }
                let result = self
                    .pairing_manager
                    .as_mut()
                    .expect("pairing manager was checked above")
                    .receive(handle, pdu)
                    .and_then(|()| self.flush_pairing_manager(link, handle));
                if let Err(error) = result {
                    self.pairing_errors.push((handle, error.to_string()));
                }
                return;
            }
            if l2cap.cid == SMP_CID && self.pairing_manager.is_some() {
                let result = SmpPdu::from_bytes(&l2cap.payload).and_then(|pdu| {
                    let manager = self
                        .pairing_manager
                        .as_mut()
                        .expect("pairing manager was checked above");
                    if matches!(pdu, SmpPdu::PairingRequest(_)) && manager.state(handle).is_none() {
                        manager.set_connection_role(handle, PairingRole::Responder)?;
                    }
                    manager.receive(handle, pdu)
                });
                if let Err(error) = result.and_then(|()| self.flush_pairing_manager(link, handle)) {
                    self.pairing_errors.push((handle, error.to_string()));
                }
                return;
            }
            self.l2cap_inbox.push((handle, l2cap.cid, l2cap.payload));
            return;
        }
        let Ok(pdu) = AttPdu::from_bytes(&l2cap.payload) else {
            return;
        };
        let context = self.att_access_context(handle, ATT_CID);

        if pdu == AttPdu::HandleValueConfirmation
            && self.pending_att_indications.remove(&(handle, ATT_CID))
        {
            return;
        }

        // ATT commands are server inputs but never produce a response.
        if pdu.is_command() {
            if let Some(server) = self.server.as_mut() {
                let _ = server.handle_request_with_context(&pdu, context);
            }
            return;
        }

        // A server answers requests automatically; everything else is for the
        // client (this device's user) to collect.
        if is_request(&pdu) {
            let response = self
                .server
                .as_mut()
                .map(|server| server.handle_request_with_context(&pdu, context).to_bytes());
            if let Some(response) = response {
                self.send_l2cap_on_handle(link, handle, ATT_CID, &response);
                return;
            }
        }
        self.inbox.push((handle, pdu));
    }
}

/// `true` if the ATT PDU is a request that expects a response.
fn is_request(pdu: &AttPdu) -> bool {
    matches!(
        pdu,
        AttPdu::ExchangeMtuRequest { .. }
            | AttPdu::FindInformationRequest { .. }
            | AttPdu::FindByTypeValueRequest { .. }
            | AttPdu::ReadRequest { .. }
            | AttPdu::ReadBlobRequest { .. }
            | AttPdu::ReadMultipleRequest { .. }
            | AttPdu::ReadByTypeRequest { .. }
            | AttPdu::ReadByGroupTypeRequest { .. }
            | AttPdu::ReadMultipleVariableRequest { .. }
            | AttPdu::WriteRequest { .. }
            | AttPdu::PrepareWriteRequest { .. }
            | AttPdu::ExecuteWriteRequest { .. }
    )
}

/// Drive the devices until no further packets flow (quiescence). Each round
/// polls every device; the loop ends when a full round consumes nothing.
///
/// The cap is a safety backstop — a request/response exchange settles in a few
/// rounds because each ACL packet is consumed once and the server answers each
/// request exactly once.
pub fn pump(link: &mut LocalLink, devices: &mut [Device]) {
    for _ in 0..64 {
        // Commands such as LE Enable Encryption and remote-feature exchange
        // enqueue link-layer control PDUs rather than host events directly.
        link.pump_ll();
        link.pump_classic();
        link.pump_periodic_sync_transfers();
        link.pump_big_terminations();
        let mut active = false;
        for device in devices.iter_mut() {
            if device.poll(link) {
                active = true;
            }
        }
        if !active {
            break;
        }
    }
}
