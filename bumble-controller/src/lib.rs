//! bumble-controller — a Rust port of the software controller + virtual link
//! from [`google/bumble`](https://github.com/google/bumble).
//!
//! **Slice 3** of the incremental port: a minimal software [`Controller`] and
//! an in-process [`LocalLink`] that together implement the LE
//! advertising → scanning → advertising-report flow, driven by the HCI codec
//! from `bumble-hci`. This is the first slice where two virtual devices
//! actually talk to each other.
//!
//! ## Synchronous model
//!
//! Bumble's `LocalLink` schedules delivery on an asyncio event loop. To keep
//! this slice deterministic and dependency-free, the link here is
//! **synchronous**: a controller consumes an HCI [`Command`] and pushes
//! host-bound HCI packets into a queue (drained with
//! [`Controller::drain_host_events`]), and [`LocalLink::propagate_advertising`]
//! delivers advertising PDUs to scanning controllers when called. The packet
//! flow matches Bumble; only the real-time scheduling is dropped.
//!
//! ## Scope
//!
//! Implemented: legacy and extended LE advertising/scanning commands and both
//! create-connection forms (slices 7 and 95), ACL data routing between connected
//! controllers (slice 8, via [`LocalLink::send_acl_data`]), and disconnection
//! (slice 13, via
//! [`LocalLink::disconnect`], emitting Disconnection Complete on both sides).
//! Also handled locally: the read commands (`Read_BD_ADDR`, `Read_Local_Name`,
//! `LE_Read_Buffer_Size`, `LE_Read_Local_Supported_Features`, `LE_Rand`) and the
//! per-connection `LE_Set_Data_Length` / `LE_Set_PHY` requests, which report
//! back through `LE_Data_Length_Change` / `LE_PHY_Update_Complete`.
//! The LE resolving-list commands also hold real IRK state: an initiator may
//! target a bonded identity while the peer advertises with an RPA, and the
//! central receives the resolved identity while link routing retains the RPA.
//! Periodic advertising retains per-set parameters and fragmented data;
//! create/cancel/terminate sync and report delivery run across the same link.
//!
//! ## Full command surface
//!
//! Every command upstream's `controller.py` handles gets a well-formed reply of
//! the matching HCI shape, driven by the generated [`command_surface`] table:
//! configuration/"set" commands are acknowledged with Command Complete + SUCCESS
//! (state is retained for the functionally modeled commands), read commands the
//! sim can't model are acknowledged SUCCESS without a synthesized payload, and operations that
//! complete via a later event (connect, encryption start, remote-features…) are
//! answered with Command Status. A command upstream *also* doesn't handle gets
//! the spec-correct "Unknown HCI Command" — an honest report, not a fake success.
//!
//! ## LL control-PDU exchange
//!
//! Two deep-behavior flows are simulated via Link-Layer control PDUs
//! ([`ll::ControlPdu`]) exchanged between controllers and routed by
//! [`LocalLink::pump_ll`], mirroring upstream `controller.py`:
//!
//! - **Encryption start** (`LE_Enable_Encryption`): the central sends an
//!   `EncReq` and encrypts its side; the peripheral encrypts on receiving it, so
//!   both hosts see an `Encryption Change` (as upstream, without the full LTK
//!   handshake — the key is carried but not yet verified).
//! - **Remote features** (`LE_Read_Remote_Features`): a `FeatureReq` /
//!   `FeatureRsp` round trip completes with an `LE_Read_Remote_Features_Complete`.
//! - **CIS establishment** (LE Audio): `LE_Set_CIG_Parameters` allocates CIS
//!   handles; `LE_Create_CIS` sends a `CisReq`; the peripheral raises an
//!   `LE CIS Request`, and on `LE_Accept_CIS_Request` a `CisRsp`/`CisInd`
//!   exchange yields `LE CIS Established` on both sides (timing params are
//!   placeholders, as upstream).
//! - **CIS data**: Setup/Remove ISO Data Path retain directional state, and the
//!   link routes HCI ISO fragments between peer CIS handles with completed-
//!   packet flow events.
//!
//! ## Classic (BR/EDR)
//!
//! A simplified classic path runs over [`lmp::ClassicPdu`] control PDUs, routed
//! by [`LocalLink::pump_classic`] (addressed by public device address):
//! ACL connection establishment (`Create_Connection` → `Connection Request` →
//! `Accept_Connection_Request` → `Connection Complete` on both sides),
//! role negotiation during accept and explicit `Switch_Role` (including
//! allow/deny policy and `Role Change` events),
//! `Remote_Name_Request` (→ `Remote Name Request Complete`), and
//! `Read_Remote_Supported_Features` (→ the matching complete event).
//!
//! ## Upstream TODO parity
//!
//! Upstream's software controller does not handle Classic authentication or
//! `Read_Remote_Version_Information`, and its LE encryption handler explicitly
//! skips LTK verification. This port retains those boundaries rather than
//! synthesizing behavior or values absent from the Python implementation. Their
//! HCI packet codecs in `bumble-hci` remain complete and oracle-pinned.

pub mod command_surface;
pub mod ll;
pub mod lmp;

use bumble::{Address, AddressType};
use bumble_crypto::ah;
use bumble_hci::codes::*;
use bumble_hci::{
    AclDataPacket, AdvertisingReport, Command, Event, ExtendedAdvertisingReport, HciPacket,
    IsoDataPacket, LeMetaEvent, ReturnParameters, SynchronousDataPacket,
};

/// Legacy connectable-and-scannable undirected advertising event type.
const ADV_IND: u8 = 0x00;
/// Address type used for public device addresses.
const ADDRESS_TYPE_PUBLIC: u8 = 0;
/// Address type used for random device addresses.
const ADDRESS_TYPE_RANDOM: u8 = 1;
/// A fixed RSSI reported for received advertisements (dBm).
const DEFAULT_RSSI: i8 = -40;
/// HCI "Unknown HCI Command" error, returned for commands this slice ignores.
const UNKNOWN_HCI_COMMAND_ERROR: u8 = 0x01;
/// HCI "Invalid HCI Command Parameters" error (e.g. an unknown connection handle).
const INVALID_COMMAND_PARAMETERS: u8 = 0x12;
/// HCI "Unknown Advertising Identifier" error.
const UNKNOWN_ADVERTISING_IDENTIFIER_ERROR: u8 = 0x42;
/// HCI "Command Disallowed" error.
const COMMAND_DISALLOWED_ERROR: u8 = 0x0C;
/// HCI "Role Change Not Allowed" error.
const ROLE_CHANGE_NOT_ALLOWED_ERROR: u8 = 0x21;
/// LE connection role: central (initiator).
pub const ROLE_CENTRAL: u8 = 0x00;
/// LE connection role: peripheral (advertiser).
pub const ROLE_PERIPHERAL: u8 = 0x01;

// Fixed LE connection parameters reported in Connection Complete (matching
// Bumble's placeholder values).
const CONNECTION_INTERVAL: u16 = 10;
const PERIPHERAL_LATENCY: u16 = 0;
const SUPERVISION_TIMEOUT: u16 = 10;
const CENTRAL_CLOCK_ACCURACY: u8 = 7;

/// HCI "Unknown Connection Identifier" error, returned for commands that
/// reference a connection handle this controller does not know.
const UNKNOWN_CONNECTION_IDENTIFIER_ERROR: u8 = 0x02;
/// LE ACL data buffer parameters reported by `LE_Read_Buffer_Size` — fixed
/// placeholders for this in-process controller.
const LE_ACL_DATA_PACKET_LENGTH: u16 = 27;
const TOTAL_NUM_LE_ACL_DATA_PACKETS: u8 = 64;
/// The LE features bitmap reported by `LE_Read_Local_Supported_Features`.
/// Bit 12 advertises the extended-advertising set/scan implementation and bits
/// 28/29 advertise the central/peripheral CIS paths implemented below.
const LOCAL_LE_FEATURES: [u8; 8] = [0x00, 0x10, 0x00, 0x30, 0, 0, 0, 0];
/// PHY value for LE 1M, reported when no specific PHY was requested.
const LE_1M_PHY: u8 = 1;
/// The classic LMP features bitmap reported by `Read_Remote_Supported_Features`
/// (all zero — no optional classic features, an honest report).
const LMP_FEATURES: [u8; 8] = [0; 8];
/// Classic ACL link type, reported in classic Connection Complete / Request.
const LINK_TYPE_ACL: u8 = 0x01;
/// Classic synchronous link types from the HCI Connection Request/Complete events.
pub const LINK_TYPE_SCO: u8 = 0x00;
pub const LINK_TYPE_ESCO: u8 = 0x02;
const AIR_MODE_CVSD: u8 = 0x02;
const AIR_MODE_TRANSPARENT: u8 = 0x03;

/// An LE or Classic ACL connection on a controller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    pub handle: u16,
    pub role: u8,
    /// Whether this Classic connection permits a peer-initiated role switch.
    /// This is unused for LE connections.
    pub classic_allow_role_switch: bool,
    /// The address this controller uses for the connection.
    pub self_address: Address,
    pub peer_address: Address,
}

/// A synchronous SCO/eSCO logical link carried alongside a Classic ACL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SynchronousConnection {
    pub handle: u16,
    pub acl_handle: u16,
    pub self_address: Address,
    pub peer_address: Address,
    pub link_type: u8,
    pub air_mode: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionKind {
    LeAcl,
    ClassicAcl,
    Synchronous,
    Iso { cig_id: u8, cis_id: u8 },
}

/// A Connected Isochronous Stream (CIS) link, established over an ACL connection
/// (LE Audio). Mirrors upstream `controller.py::CisLink`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CisLink {
    cig_id: u8,
    cis_id: u8,
    /// The CIS connection handle (distinct from the ACL handle).
    handle: u16,
    /// The endpoints of the ACL connection carrying this CIS.
    acl_self: Address,
    acl_peer: Address,
    /// Installed ISO data paths, indexed by Setup ISO Data Path direction.
    data_paths: u8,
}

/// A pending outgoing connection recorded by `LE_Create_Connection`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingConnection {
    peer_address: Address,
    peer_address_type: u8,
    own_address_type: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClassicRoleSwitchPurpose {
    AcceptConnection,
    SwitchRoleCommand { requested_role: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingClassicRoleSwitch {
    peer_address: Address,
    purpose: ClassicRoleSwitchPurpose,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResolvingListEntry {
    peer_identity_address_type: u8,
    peer_identity_address: Address,
    peer_irk: [u8; 16],
    local_irk: [u8; 16],
}

/// The subset of extended-advertising parameters that affects the in-process
/// link. The full command remains available through `bumble-hci`; these fields
/// are the ones upstream's software controller retains for packet emission.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ExtendedAdvertisingParameters {
    advertising_event_properties: u16,
    own_address_type: u8,
    peer_address_type: u8,
    peer_address: Address,
    advertising_tx_power: i8,
    primary_advertising_phy: u8,
    secondary_advertising_phy: u8,
    advertising_sid: u8,
}

/// One stateful LE extended-advertising set, keyed by its HCI handle.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ExtendedAdvertisingSet {
    handle: u8,
    parameters: Option<ExtendedAdvertisingParameters>,
    data: Vec<u8>,
    scan_response_data: Vec<u8>,
    enabled: bool,
    random_address: Option<Address>,
    periodic_parameters: Option<PeriodicAdvertisingParameters>,
    periodic_data: Vec<u8>,
    periodic_enabled: bool,
}

impl ExtendedAdvertisingSet {
    fn new(handle: u8) -> Self {
        Self {
            handle,
            parameters: None,
            data: Vec::new(),
            scan_response_data: Vec::new(),
            enabled: false,
            random_address: None,
            periodic_parameters: None,
            periodic_data: Vec::new(),
            periodic_enabled: false,
        }
    }

    fn address(&self, public_address: &Address) -> Option<Address> {
        let parameters = self.parameters.as_ref()?;
        if parameters.own_address_type == ADDRESS_TYPE_PUBLIC {
            Some(public_address.clone())
        } else {
            self.random_address.clone()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PeriodicAdvertisingParameters {
    interval_min: u16,
    interval_max: u16,
    properties: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingPeriodicSync {
    options: u8,
    advertising_sid: u8,
    advertiser_address_type: u8,
    advertiser_address: Address,
    skip: u16,
    sync_timeout: u16,
    sync_cte_type: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PeriodicSync {
    handle: u16,
    advertising_sid: u8,
    advertiser_address_type: u8,
    advertiser_address: Address,
    advertiser_phy: u8,
    interval: u16,
    advertiser_clock_accuracy: u8,
    receive_enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PeriodicSyncTransfer {
    service_data: u16,
    advertising_sid: u8,
    advertiser_address_type: u8,
    advertiser_address: Address,
    advertiser_phy: u8,
    interval: u16,
    advertiser_clock_accuracy: u8,
}

/// An advertising PDU as it travels over the [`LocalLink`]. Since the link is
/// in-process, this is a plain struct rather than a serialized LL PDU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdvertisingPdu {
    pub event_type: u8,
    pub address_type: u8,
    pub address: Address,
    pub data: Vec<u8>,
    pub scan_response_data: Vec<u8>,
    pub extended: bool,
    pub advertising_handle: u8,
    pub advertising_sid: u8,
    pub primary_phy: u8,
    pub secondary_phy: u8,
    pub tx_power: i8,
    pub direct_address: Option<Address>,
}

/// A periodic advertising event as it travels over the in-process link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeriodicAdvertisingPdu {
    pub address_type: u8,
    pub address: Address,
    pub advertising_sid: u8,
    pub advertiser_phy: u8,
    pub interval: u16,
    pub tx_power: i8,
    pub data: Vec<u8>,
}

/// A minimal LE software controller: it consumes HCI commands from a host and
/// produces host-bound HCI packets.
#[derive(Debug)]
pub struct Controller {
    pub name: String,
    public_address: Address,
    random_address: Address,
    advertising_data: Vec<u8>,
    advertising_enabled: bool,
    scanning_enabled: bool,
    extended_scanning: bool,
    extended_advertising_sets: Vec<ExtendedAdvertisingSet>,
    pending_periodic_sync: Option<PendingPeriodicSync>,
    periodic_syncs: Vec<PeriodicSync>,
    next_periodic_sync_handle: u16,
    outbound_periodic_sync_transfers: Vec<(Address, Address, PeriodicSyncTransfer)>,
    connections: Vec<Connection>,
    initiating: Option<PendingConnection>,
    resolving_list: Vec<ResolvingListEntry>,
    address_resolution_enabled: bool,
    rpa_timeout: u16,
    next_handle: u16,
    /// Monotonic counter backing `LE_Rand` — the software controller has no
    /// entropy source, so it returns a deterministic, ever-changing value.
    rand_counter: u64,
    host_queue: Vec<HciPacket>,
    /// LL control PDUs waiting to be delivered to a peer controller, as
    /// `(sender_self_address, receiver_peer_address, pdu)`. Drained by the link.
    outbound_ll: Vec<(Address, Address, ll::ControlPdu)>,
    /// CIS links created as the central (by `LE_Set_CIG_Parameters`).
    central_cis_links: Vec<CisLink>,
    /// CIS links pending/accepted as the peripheral (from an incoming `CisReq`).
    peripheral_cis_links: Vec<CisLink>,
    /// Classic (BR/EDR) ACL connections, keyed by peer address in `peer_address`.
    classic_connections: Vec<Connection>,
    /// Controller-wide policy copied to incoming Classic connections.
    classic_allow_role_switch: bool,
    /// Local HCI operations awaiting an LMP role-switch response.
    pending_classic_role_switches: Vec<PendingClassicRoleSwitch>,
    /// SCO/eSCO logical links over established Classic ACL connections.
    synchronous_connections: Vec<SynchronousConnection>,
    /// Classic LMP PDUs waiting for a peer, as `(sender_public, receiver, pdu)`.
    outbound_classic: Vec<(Address, Address, lmp::ClassicPdu)>,
}

impl Controller {
    /// Create a controller with the given name and public address. The random
    /// address starts as all-zero until set via `LE_Set_Random_Address`.
    pub fn new(name: &str, public_address: Address) -> Controller {
        Controller {
            name: name.to_string(),
            public_address,
            random_address: Address::from_bytes([0; 6], AddressType::RANDOM_DEVICE),
            advertising_data: Vec::new(),
            advertising_enabled: false,
            scanning_enabled: false,
            extended_scanning: false,
            extended_advertising_sets: Vec::new(),
            pending_periodic_sync: None,
            periodic_syncs: Vec::new(),
            next_periodic_sync_handle: 1,
            outbound_periodic_sync_transfers: Vec::new(),
            connections: Vec::new(),
            initiating: None,
            resolving_list: Vec::new(),
            address_resolution_enabled: false,
            rpa_timeout: 900,
            next_handle: 1,
            rand_counter: 0,
            host_queue: Vec::new(),
            outbound_ll: Vec::new(),
            central_cis_links: Vec::new(),
            peripheral_cis_links: Vec::new(),
            classic_connections: Vec::new(),
            classic_allow_role_switch: true,
            pending_classic_role_switches: Vec::new(),
            synchronous_connections: Vec::new(),
            outbound_classic: Vec::new(),
        }
    }

    pub fn public_address(&self) -> &Address {
        &self.public_address
    }

    pub fn random_address(&self) -> &Address {
        &self.random_address
    }

    /// Set the policy copied to subsequently received Classic ACL requests.
    pub fn set_classic_allow_role_switch(&mut self, allow: bool) {
        self.classic_allow_role_switch = allow;
    }

    pub fn classic_allow_role_switch(&self) -> bool {
        self.classic_allow_role_switch
    }

    pub fn is_advertising(&self) -> bool {
        self.advertising_enabled || self.extended_advertising_sets.iter().any(|set| set.enabled)
    }

    pub fn is_scanning(&self) -> bool {
        self.scanning_enabled
    }

    /// Handle a single HCI command from the host, updating state and queuing a
    /// Command Complete acknowledgement.
    pub fn handle_command(&mut self, command: Command) {
        let op_code = command.op_code();
        match command {
            Command::Reset => {
                self.advertising_enabled = false;
                self.scanning_enabled = false;
                self.extended_scanning = false;
                self.advertising_data.clear();
                self.extended_advertising_sets.clear();
                self.pending_periodic_sync = None;
                self.periodic_syncs.clear();
                self.next_periodic_sync_handle = 1;
                self.outbound_periodic_sync_transfers.clear();
                self.connections.clear();
                self.classic_connections.clear();
                self.synchronous_connections.clear();
                self.initiating = None;
                self.resolving_list.clear();
                self.address_resolution_enabled = false;
                self.rpa_timeout = 900;
                self.ack(op_code, HCI_SUCCESS);
            }
            Command::LeCreateConnection {
                peer_address,
                peer_address_type,
                own_address_type,
                ..
            } => {
                self.initiating = Some(PendingConnection {
                    peer_address,
                    peer_address_type,
                    own_address_type,
                });
                // LE_Create_Connection is acknowledged with a Command Status;
                // the Connection Complete follows once the link connects.
                self.host_queue.push(HciPacket::Event(Event::CommandStatus {
                    status: HCI_SUCCESS,
                    num_hci_command_packets: 1,
                    command_opcode: op_code,
                }));
            }
            Command::LeSetRandomAddress { random_address } => {
                self.random_address = random_address;
                self.ack(op_code, HCI_SUCCESS);
            }
            Command::LeSetAdvertisingData { advertising_data } => {
                self.advertising_data = advertising_data;
                self.ack(op_code, HCI_SUCCESS);
            }
            Command::LeSetAdvertisingEnable { advertising_enable } => {
                self.advertising_enabled = advertising_enable != 0;
                self.ack(op_code, HCI_SUCCESS);
            }
            Command::LeSetScanEnable { le_scan_enable, .. } => {
                self.scanning_enabled = le_scan_enable != 0;
                self.extended_scanning = false;
                self.ack(op_code, HCI_SUCCESS);
            }
            Command::LeSetAdvertisingSetRandomAddress {
                advertising_handle,
                random_address,
            } => {
                self.extended_advertising_set_mut(advertising_handle)
                    .random_address = Some(random_address);
                self.ack(op_code, HCI_SUCCESS);
            }
            Command::LeSetExtendedAdvertisingParameters {
                advertising_handle,
                advertising_event_properties,
                own_address_type,
                peer_address_type,
                peer_address,
                advertising_tx_power,
                primary_advertising_phy,
                secondary_advertising_phy,
                advertising_sid,
                ..
            } => {
                let tx_power = advertising_tx_power as i8;
                self.extended_advertising_set_mut(advertising_handle)
                    .parameters = Some(ExtendedAdvertisingParameters {
                    advertising_event_properties,
                    own_address_type,
                    peer_address_type,
                    peer_address,
                    advertising_tx_power: tx_power,
                    primary_advertising_phy,
                    secondary_advertising_phy,
                    advertising_sid,
                });
                self.complete(
                    op_code,
                    ReturnParameters::Raw {
                        data: vec![HCI_SUCCESS, 0],
                    },
                );
            }
            Command::LeSetExtendedAdvertisingData {
                advertising_handle,
                operation,
                advertising_data,
                ..
            } => self.handle_extended_advertising_data(
                op_code,
                advertising_handle,
                operation,
                &advertising_data,
                false,
            ),
            Command::LeSetExtendedScanResponseData {
                advertising_handle,
                operation,
                scan_response_data,
                ..
            } => self.handle_extended_advertising_data(
                op_code,
                advertising_handle,
                operation,
                &scan_response_data,
                true,
            ),
            Command::LeSetExtendedAdvertisingEnable {
                enable,
                advertising_handles,
                ..
            } => {
                self.handle_extended_advertising_enable(enable, &advertising_handles);
                self.ack(op_code, HCI_SUCCESS);
            }
            Command::LeReadMaximumAdvertisingDataLength => {
                self.complete(
                    op_code,
                    ReturnParameters::Raw {
                        data: vec![HCI_SUCCESS, 0x72, 0x06],
                    },
                );
            }
            Command::LeReadNumberOfSupportedAdvertisingSets => {
                self.complete(
                    op_code,
                    ReturnParameters::Raw {
                        data: vec![HCI_SUCCESS, 0xF0],
                    },
                );
            }
            Command::LeRemoveAdvertisingSet { advertising_handle } => {
                self.extended_advertising_sets
                    .retain(|set| set.handle != advertising_handle);
                self.ack(op_code, HCI_SUCCESS);
            }
            Command::LeClearAdvertisingSets => {
                self.extended_advertising_sets.clear();
                self.ack(op_code, HCI_SUCCESS);
            }
            Command::LeSetPeriodicAdvertisingParameters {
                advertising_handle,
                periodic_advertising_interval_min,
                periodic_advertising_interval_max,
                periodic_advertising_properties,
            } => self.handle_periodic_advertising_parameters(
                op_code,
                advertising_handle,
                periodic_advertising_interval_min,
                periodic_advertising_interval_max,
                periodic_advertising_properties,
            ),
            Command::LeSetPeriodicAdvertisingData {
                advertising_handle,
                operation,
                advertising_data,
            } => self.handle_periodic_advertising_data(
                op_code,
                advertising_handle,
                operation,
                &advertising_data,
            ),
            Command::LeSetPeriodicAdvertisingEnable {
                enable,
                advertising_handle,
            } => self.handle_periodic_advertising_enable(op_code, advertising_handle, enable),
            Command::LeSetExtendedScanParameters { .. } => {
                self.extended_scanning = true;
                self.ack(op_code, HCI_SUCCESS);
            }
            Command::LeSetExtendedScanEnable { enable, .. } => {
                self.scanning_enabled = enable != 0;
                self.extended_scanning = true;
                self.ack(op_code, HCI_SUCCESS);
            }
            Command::LeExtendedCreateConnection {
                peer_address,
                peer_address_type,
                own_address_type,
                ..
            } => {
                self.initiating = Some(PendingConnection {
                    peer_address,
                    peer_address_type,
                    own_address_type,
                });
                self.command_status(op_code, HCI_SUCCESS);
            }
            Command::LePeriodicAdvertisingCreateSync {
                options,
                advertising_sid,
                advertiser_address_type,
                advertiser_address,
                skip,
                sync_timeout,
                sync_cte_type,
            } => self.handle_periodic_advertising_create_sync(
                op_code,
                PendingPeriodicSync {
                    options,
                    advertising_sid,
                    advertiser_address_type,
                    advertiser_address,
                    skip,
                    sync_timeout,
                    sync_cte_type,
                },
            ),
            Command::LePeriodicAdvertisingCreateSyncCancel => {
                self.handle_periodic_advertising_create_sync_cancel(op_code)
            }
            Command::LePeriodicAdvertisingTerminateSync { sync_handle } => {
                self.handle_periodic_advertising_terminate_sync(op_code, sync_handle)
            }
            Command::LeSetPeriodicAdvertisingReceiveEnable {
                sync_handle,
                enable,
            } => self.handle_periodic_advertising_receive_enable(op_code, sync_handle, enable),
            Command::LePeriodicAdvertisingSyncTransfer {
                connection_handle,
                service_data,
                sync_handle,
            } => self.handle_periodic_advertising_sync_transfer(
                op_code,
                connection_handle,
                service_data,
                sync_handle,
            ),
            Command::LePeriodicAdvertisingSetInfoTransfer {
                connection_handle,
                service_data,
                advertising_handle,
            } => self.handle_periodic_advertising_set_info_transfer(
                op_code,
                connection_handle,
                service_data,
                advertising_handle,
            ),
            Command::ReadBdAddr => {
                self.complete(
                    op_code,
                    ReturnParameters::ReadBdAddr {
                        status: HCI_SUCCESS,
                        bd_addr: self.public_address.clone(),
                    },
                );
            }
            Command::ReadLocalName => {
                // The local name is a fixed 248-byte, null-padded field.
                let mut local_name = self.name.as_bytes().to_vec();
                local_name.resize(248, 0);
                self.complete(
                    op_code,
                    ReturnParameters::ReadLocalName {
                        status: HCI_SUCCESS,
                        local_name,
                    },
                );
            }
            Command::LeReadBufferSize => {
                self.complete(
                    op_code,
                    ReturnParameters::LeReadBufferSize {
                        status: HCI_SUCCESS,
                        le_acl_data_packet_length: LE_ACL_DATA_PACKET_LENGTH,
                        total_num_le_acl_data_packets: TOTAL_NUM_LE_ACL_DATA_PACKETS,
                    },
                );
            }
            Command::LeReadLocalSupportedFeatures => {
                // No typed return-parameter variant exists for this command; the
                // controller returns status + the 8-byte LE features bitmap.
                let mut data = vec![HCI_SUCCESS];
                data.extend_from_slice(&LOCAL_LE_FEATURES);
                self.complete(op_code, ReturnParameters::Raw { data });
            }
            Command::LeRand => {
                // Deterministic stand-in for a hardware RNG (see `rand_counter`).
                let value = self.rand_counter.to_le_bytes();
                self.rand_counter += 1;
                let mut data = vec![HCI_SUCCESS];
                data.extend_from_slice(&value);
                self.complete(op_code, ReturnParameters::Raw { data });
            }
            Command::LeAddDeviceToResolvingList {
                peer_identity_address_type,
                peer_identity_address,
                peer_irk,
                local_irk,
            } => {
                if peer_identity_address_type > ADDRESS_TYPE_RANDOM {
                    self.ack(op_code, INVALID_COMMAND_PARAMETERS);
                } else {
                    self.resolving_list
                        .retain(|entry| entry.peer_identity_address != peer_identity_address);
                    self.resolving_list.push(ResolvingListEntry {
                        peer_identity_address_type,
                        peer_identity_address,
                        peer_irk,
                        local_irk,
                    });
                    self.ack(op_code, HCI_SUCCESS);
                }
            }
            Command::LeClearResolvingList => {
                self.resolving_list.clear();
                self.ack(op_code, HCI_SUCCESS);
            }
            Command::LeReadResolvingListSize => {
                self.complete(
                    op_code,
                    ReturnParameters::Raw {
                        data: vec![HCI_SUCCESS, 16],
                    },
                );
            }
            Command::LeSetAddressResolutionEnable {
                address_resolution_enable,
            } => {
                if address_resolution_enable > 1 {
                    self.ack(op_code, INVALID_COMMAND_PARAMETERS);
                } else {
                    self.address_resolution_enabled = address_resolution_enable != 0;
                    self.ack(op_code, HCI_SUCCESS);
                }
            }
            Command::LeSetResolvablePrivateAddressTimeout { rpa_timeout } => {
                if rpa_timeout == 0 {
                    self.ack(op_code, INVALID_COMMAND_PARAMETERS);
                } else {
                    self.rpa_timeout = rpa_timeout;
                    self.ack(op_code, HCI_SUCCESS);
                }
            }
            Command::LeSetDataLength {
                connection_handle,
                tx_octets,
                tx_time,
            } => self.handle_set_data_length(op_code, connection_handle, tx_octets, tx_time),
            Command::LeSetPhy {
                connection_handle,
                all_phys,
                tx_phys,
                rx_phys,
                ..
            } => self.handle_set_phy(connection_handle, all_phys, tx_phys, rx_phys),
            Command::LeEnableEncryption {
                connection_handle,
                random_number,
                encrypted_diversifier,
                long_term_key,
            } => self.handle_enable_encryption(
                connection_handle,
                random_number,
                encrypted_diversifier,
                long_term_key,
            ),
            Command::LeReadRemoteFeatures { connection_handle } => {
                self.handle_read_remote_features(connection_handle)
            }
            Command::LeSetCigParameters { cig_id, cis_id, .. } => {
                self.handle_set_cig_parameters(cig_id, &cis_id)
            }
            Command::LeCreateCis {
                cis_connection_handle,
                acl_connection_handle,
            } => self.handle_create_cis(&cis_connection_handle, &acl_connection_handle),
            Command::LeAcceptCisRequest { connection_handle } => {
                self.handle_accept_cis_request(connection_handle)
            }
            Command::LeSetupIsoDataPath {
                connection_handle,
                data_path_direction,
                ..
            } => self.handle_setup_iso_data_path(connection_handle, data_path_direction),
            Command::LeRemoveIsoDataPath {
                connection_handle,
                data_path_direction,
            } => self.handle_remove_iso_data_path(connection_handle, data_path_direction),
            Command::SetConnectionEncryption {
                connection_handle,
                encryption_enable,
            } => self.handle_set_classic_encryption(connection_handle, encryption_enable),
            Command::CreateConnection {
                bd_addr,
                allow_role_switch,
                ..
            } => self.handle_create_connection(bd_addr, allow_role_switch),
            Command::AcceptConnectionRequest { bd_addr, role } => {
                self.handle_accept_connection_request(bd_addr, role)
            }
            Command::SwitchRole { bd_addr, role } => self.handle_switch_role(bd_addr, role),
            Command::RemoteNameRequest { bd_addr, .. } => self.handle_remote_name_request(bd_addr),
            Command::ReadRemoteSupportedFeatures { connection_handle } => {
                self.handle_read_remote_supported_features(connection_handle)
            }
            Command::EnhancedSetupSynchronousConnection {
                connection_handle,
                transmit_coding_format,
                ..
            } => self.handle_setup_synchronous_connection(
                connection_handle,
                LINK_TYPE_ESCO,
                air_mode_for_coding_format(transmit_coding_format.coding_format),
            ),
            Command::EnhancedAcceptSynchronousConnectionRequest {
                bd_addr,
                transmit_coding_format,
                ..
            } => self.handle_accept_synchronous_connection(
                HCI_ENHANCED_ACCEPT_SYNCHRONOUS_CONNECTION_REQUEST_COMMAND,
                bd_addr,
                air_mode_for_coding_format(transmit_coding_format.coding_format),
            ),
            Command::AcceptSynchronousConnectionRequest { bd_addr, .. } => self
                .handle_accept_synchronous_connection(
                    HCI_ACCEPT_SYNCHRONOUS_CONNECTION_REQUEST_COMMAND,
                    bd_addr,
                    AIR_MODE_CVSD,
                ),
            Command::RejectSynchronousConnectionRequest { bd_addr, reason } => {
                self.handle_reject_synchronous_connection(bd_addr, reason)
            }
            // Any command not modelled functionally above: reply with the same
            // response *shape* upstream `controller.py` uses for it (see
            // [`command_surface`]). A command upstream also doesn't handle gets
            // the spec-correct "Unknown HCI Command".
            _ => match command_surface::response_kind(op_code) {
                Some(command_surface::Resp::StatusOnly) | Some(command_surface::Resp::Data) => {
                    self.ack(op_code, HCI_SUCCESS)
                }
                Some(command_surface::Resp::Status) => self.command_status(op_code, HCI_SUCCESS),
                None => self.ack(op_code, UNKNOWN_HCI_COMMAND_ERROR),
            },
        }
    }

    fn extended_advertising_set_mut(&mut self, handle: u8) -> &mut ExtendedAdvertisingSet {
        if let Some(index) = self
            .extended_advertising_sets
            .iter()
            .position(|set| set.handle == handle)
        {
            return &mut self.extended_advertising_sets[index];
        }
        self.extended_advertising_sets
            .push(ExtendedAdvertisingSet::new(handle));
        self.extended_advertising_sets
            .last_mut()
            .expect("advertising set was just inserted")
    }

    fn handle_extended_advertising_data(
        &mut self,
        op_code: u16,
        handle: u8,
        operation: u8,
        fragment: &[u8],
        scan_response: bool,
    ) {
        let Some(set) = self
            .extended_advertising_sets
            .iter_mut()
            .find(|set| set.handle == handle)
        else {
            return self.ack(op_code, UNKNOWN_ADVERTISING_IDENTIFIER_ERROR);
        };
        let data = if scan_response {
            &mut set.scan_response_data
        } else {
            &mut set.data
        };
        match operation {
            // INTERMEDIATE_FRAGMENT or LAST_FRAGMENT.
            0x00 | 0x02 => data.extend_from_slice(fragment),
            // FIRST_FRAGMENT or COMPLETE_DATA.
            0x01 | 0x03 => {
                data.clear();
                data.extend_from_slice(fragment);
            }
            // UNCHANGED_DATA leaves the existing value intact, matching Bumble.
            0x04 => {}
            _ => return self.ack(op_code, INVALID_COMMAND_PARAMETERS),
        }
        self.ack(op_code, HCI_SUCCESS);
    }

    fn handle_extended_advertising_enable(&mut self, enable: u8, handles: &[u8]) {
        if enable == 0 && handles.is_empty() {
            for set in &mut self.extended_advertising_sets {
                set.enabled = false;
            }
            return;
        }
        for handle in handles {
            if let Some(set) = self
                .extended_advertising_sets
                .iter_mut()
                .find(|set| set.handle == *handle)
            {
                set.enabled = enable != 0;
            }
        }
    }

    fn handle_periodic_advertising_parameters(
        &mut self,
        op_code: u16,
        handle: u8,
        interval_min: u16,
        interval_max: u16,
        properties: u16,
    ) {
        let Some(set) = self
            .extended_advertising_sets
            .iter_mut()
            .find(|set| set.handle == handle)
        else {
            return self.ack(op_code, UNKNOWN_ADVERTISING_IDENTIFIER_ERROR);
        };
        if interval_min < 0x0006 || interval_min > interval_max {
            return self.ack(op_code, INVALID_COMMAND_PARAMETERS);
        }
        set.periodic_parameters = Some(PeriodicAdvertisingParameters {
            interval_min,
            interval_max,
            properties,
        });
        self.ack(op_code, HCI_SUCCESS);
    }

    fn handle_periodic_advertising_data(
        &mut self,
        op_code: u16,
        handle: u8,
        operation: u8,
        fragment: &[u8],
    ) {
        let Some(set) = self
            .extended_advertising_sets
            .iter_mut()
            .find(|set| set.handle == handle)
        else {
            return self.ack(op_code, UNKNOWN_ADVERTISING_IDENTIFIER_ERROR);
        };
        let replacing = matches!(operation, 0x01 | 0x03);
        let appending = matches!(operation, 0x00 | 0x02);
        if !replacing && !appending {
            return self.ack(op_code, INVALID_COMMAND_PARAMETERS);
        }
        let resulting_len = if replacing {
            fragment.len()
        } else {
            set.periodic_data.len().saturating_add(fragment.len())
        };
        if resulting_len > 0x0672 {
            return self.ack(op_code, INVALID_COMMAND_PARAMETERS);
        }
        if replacing {
            set.periodic_data.clear();
        }
        set.periodic_data.extend_from_slice(fragment);
        self.ack(op_code, HCI_SUCCESS);
    }

    fn handle_periodic_advertising_enable(&mut self, op_code: u16, handle: u8, enable: u8) {
        let Some(set) = self
            .extended_advertising_sets
            .iter_mut()
            .find(|set| set.handle == handle)
        else {
            return self.ack(op_code, UNKNOWN_ADVERTISING_IDENTIFIER_ERROR);
        };
        if enable & !0x03 != 0 {
            return self.ack(op_code, INVALID_COMMAND_PARAMETERS);
        }
        if enable & 1 != 0 && set.periodic_parameters.is_none() {
            return self.ack(op_code, COMMAND_DISALLOWED_ERROR);
        }
        set.periodic_enabled = enable & 1 != 0;
        self.ack(op_code, HCI_SUCCESS);
    }

    fn handle_periodic_advertising_create_sync(
        &mut self,
        op_code: u16,
        pending: PendingPeriodicSync,
    ) {
        if self.pending_periodic_sync.is_some() {
            return self.command_status(op_code, COMMAND_DISALLOWED_ERROR);
        }
        if pending.advertising_sid > 0x0F
            || pending.advertiser_address_type > ADDRESS_TYPE_RANDOM
            || pending.skip > 0x01F3
            || !(0x000A..=0x4000).contains(&pending.sync_timeout)
        {
            return self.command_status(op_code, INVALID_COMMAND_PARAMETERS);
        }
        self.pending_periodic_sync = Some(pending);
        self.command_status(op_code, HCI_SUCCESS);
    }

    fn handle_periodic_advertising_create_sync_cancel(&mut self, op_code: u16) {
        let Some(pending) = self.pending_periodic_sync.take() else {
            return self.ack(op_code, COMMAND_DISALLOWED_ERROR);
        };
        self.ack(op_code, HCI_SUCCESS);
        self.host_queue.push(HciPacket::Event(Event::LeMeta(
            LeMetaEvent::PeriodicAdvertisingSyncEstablished {
                status: 0x44,
                sync_handle: 0,
                advertising_sid: pending.advertising_sid,
                advertiser_address_type: pending.advertiser_address_type,
                advertiser_address: pending.advertiser_address,
                advertiser_phy: 0,
                periodic_advertising_interval: 0,
                advertiser_clock_accuracy: 0,
            },
        )));
    }

    fn handle_periodic_advertising_terminate_sync(&mut self, op_code: u16, sync_handle: u16) {
        let original_len = self.periodic_syncs.len();
        self.periodic_syncs
            .retain(|sync| sync.handle != sync_handle);
        self.ack(
            op_code,
            if self.periodic_syncs.len() == original_len {
                UNKNOWN_CONNECTION_IDENTIFIER_ERROR
            } else {
                HCI_SUCCESS
            },
        );
    }

    fn handle_periodic_advertising_receive_enable(
        &mut self,
        op_code: u16,
        sync_handle: u16,
        enable: u8,
    ) {
        if enable > 1 {
            return self.ack(op_code, INVALID_COMMAND_PARAMETERS);
        }
        let Some(sync) = self
            .periodic_syncs
            .iter_mut()
            .find(|sync| sync.handle == sync_handle)
        else {
            return self.ack(op_code, UNKNOWN_CONNECTION_IDENTIFIER_ERROR);
        };
        sync.receive_enabled = enable != 0;
        self.ack(op_code, HCI_SUCCESS);
    }

    fn handle_periodic_advertising_sync_transfer(
        &mut self,
        op_code: u16,
        connection_handle: u16,
        service_data: u16,
        sync_handle: u16,
    ) {
        let Some(connection) = self
            .connections
            .iter()
            .find(|connection| connection.handle == connection_handle)
            .cloned()
        else {
            return self.ack(op_code, UNKNOWN_CONNECTION_IDENTIFIER_ERROR);
        };
        let Some(sync) = self
            .periodic_syncs
            .iter()
            .find(|sync| sync.handle == sync_handle)
            .cloned()
        else {
            return self.ack(op_code, UNKNOWN_CONNECTION_IDENTIFIER_ERROR);
        };
        self.outbound_periodic_sync_transfers.push((
            connection.self_address,
            connection.peer_address,
            PeriodicSyncTransfer {
                service_data,
                advertising_sid: sync.advertising_sid,
                advertiser_address_type: sync.advertiser_address_type,
                advertiser_address: sync.advertiser_address,
                advertiser_phy: sync.advertiser_phy,
                interval: sync.interval,
                advertiser_clock_accuracy: sync.advertiser_clock_accuracy,
            },
        ));
        self.ack(op_code, HCI_SUCCESS);
    }

    fn handle_periodic_advertising_set_info_transfer(
        &mut self,
        op_code: u16,
        connection_handle: u16,
        service_data: u16,
        advertising_handle: u8,
    ) {
        let Some(connection) = self
            .connections
            .iter()
            .find(|connection| connection.handle == connection_handle)
            .cloned()
        else {
            return self.ack(op_code, UNKNOWN_CONNECTION_IDENTIFIER_ERROR);
        };
        let Some(set) = self
            .extended_advertising_sets
            .iter()
            .find(|set| set.handle == advertising_handle)
        else {
            return self.ack(op_code, UNKNOWN_ADVERTISING_IDENTIFIER_ERROR);
        };
        if !set.periodic_enabled {
            return self.ack(op_code, COMMAND_DISALLOWED_ERROR);
        }
        let (Some(parameters), Some(periodic), Some(address)) = (
            set.parameters.as_ref(),
            set.periodic_parameters,
            set.address(&self.public_address),
        ) else {
            return self.ack(op_code, COMMAND_DISALLOWED_ERROR);
        };
        self.outbound_periodic_sync_transfers.push((
            connection.self_address,
            connection.peer_address,
            PeriodicSyncTransfer {
                service_data,
                advertising_sid: parameters.advertising_sid,
                advertiser_address_type: address.address_type().0,
                advertiser_address: address,
                advertiser_phy: parameters.secondary_advertising_phy,
                interval: periodic.interval_min,
                advertiser_clock_accuracy: 0,
            },
        ));
        self.ack(op_code, HCI_SUCCESS);
    }

    /// `LE_Set_Data_Length`: acknowledge with the connection handle, then (on a
    /// known connection) report the negotiated lengths via an
    /// [`LeMetaEvent::DataLengthChange`]. The controller mirrors the requested
    /// TX limits onto RX, matching a peer with identical capability.
    fn handle_set_data_length(
        &mut self,
        op_code: u16,
        connection_handle: u16,
        tx_octets: u16,
        tx_time: u16,
    ) {
        let known = self.connection_by_handle(connection_handle).is_some();
        let status = if known {
            HCI_SUCCESS
        } else {
            UNKNOWN_CONNECTION_IDENTIFIER_ERROR
        };
        // Command Complete carries status + connection handle (no typed variant).
        let mut data = vec![status];
        data.extend_from_slice(&connection_handle.to_le_bytes());
        self.complete(op_code, ReturnParameters::Raw { data });

        if known {
            self.host_queue.push(HciPacket::Event(Event::LeMeta(
                LeMetaEvent::DataLengthChange {
                    connection_handle,
                    max_tx_octets: tx_octets,
                    max_tx_time: tx_time,
                    max_rx_octets: tx_octets,
                    max_rx_time: tx_time,
                },
            )));
        }
    }

    /// `LE_Set_PHY`: acknowledge with a Command Status, then (on a known
    /// connection) report the resolved PHYs via an
    /// [`LeMetaEvent::PhyUpdateComplete`].
    fn handle_set_phy(&mut self, connection_handle: u16, all_phys: u8, tx_phys: u8, rx_phys: u8) {
        self.host_queue.push(HciPacket::Event(Event::CommandStatus {
            status: HCI_SUCCESS,
            num_hci_command_packets: 1,
            command_opcode: HCI_LE_SET_PHY_COMMAND,
        }));
        if self.connection_by_handle(connection_handle).is_some() {
            // Bit 0 of all_phys = "no TX preference"; bit 1 = "no RX preference".
            let tx_phy = resolve_phy(all_phys & 0x01 != 0, tx_phys);
            let rx_phy = resolve_phy(all_phys & 0x02 != 0, rx_phys);
            self.host_queue.push(HciPacket::Event(Event::LeMeta(
                LeMetaEvent::PhyUpdateComplete {
                    status: HCI_SUCCESS,
                    connection_handle,
                    tx_phy,
                    rx_phy,
                },
            )));
        }
    }

    /// `LE_Enable_Encryption` (central): acknowledge with Command Status, send an
    /// `EncReq` LL PDU to the peer, and start encryption on this side. The peer
    /// starts encryption when it receives the `EncReq` (see [`on_ll_control_pdu`]).
    /// This mirrors upstream `controller.py`, which completes encryption without
    /// the full LTK handshake (the LTK is carried but not yet verified).
    ///
    /// [`on_ll_control_pdu`]: Controller::on_ll_control_pdu
    fn handle_enable_encryption(
        &mut self,
        connection_handle: u16,
        random_number: [u8; 8],
        encrypted_diversifier: u16,
        long_term_key: [u8; 16],
    ) {
        let Some(conn) = self.connection_by_handle(connection_handle) else {
            return self
                .command_status(HCI_LE_ENABLE_ENCRYPTION_COMMAND, INVALID_COMMAND_PARAMETERS);
        };
        let (self_addr, peer_addr) = (conn.self_address.clone(), conn.peer_address.clone());
        self.queue_ll(
            self_addr,
            peer_addr,
            ll::ControlPdu::EncReq {
                rand: random_number,
                ediv: encrypted_diversifier,
                ltk: long_term_key,
            },
        );
        self.command_status(HCI_LE_ENABLE_ENCRYPTION_COMMAND, HCI_SUCCESS);
        self.on_le_encrypted(connection_handle);
    }

    /// `LE_Read_Remote_Features`: acknowledge with Command Status, then send a
    /// feature-request LL PDU to the peer. The peer answers with a `FeatureRsp`,
    /// which this controller turns into an `LE_Read_Remote_Features_Complete`
    /// event (see [`on_ll_control_pdu`](Controller::on_ll_control_pdu)).
    fn handle_read_remote_features(&mut self, connection_handle: u16) {
        let Some(conn) = self.connection_by_handle(connection_handle) else {
            return self.command_status(
                HCI_LE_READ_REMOTE_FEATURES_COMMAND,
                INVALID_COMMAND_PARAMETERS,
            );
        };
        let (self_addr, peer_addr, role) = (
            conn.self_address.clone(),
            conn.peer_address.clone(),
            conn.role,
        );
        self.command_status(HCI_LE_READ_REMOTE_FEATURES_COMMAND, HCI_SUCCESS);
        let req = if role == ROLE_CENTRAL {
            ll::ControlPdu::FeatureReq {
                feature_set: LOCAL_LE_FEATURES,
            }
        } else {
            ll::ControlPdu::PeripheralFeatureReq {
                feature_set: LOCAL_LE_FEATURES,
            }
        };
        self.queue_ll(self_addr, peer_addr, req);
    }

    /// `LE_Set_CIG_Parameters` (central): allocate a CIS connection handle per
    /// requested `cis_id`, record a central CIS link, and return the CIG id and
    /// allocated handles. The ACL endpoints are bound later by `LE_Create_CIS`.
    fn handle_set_cig_parameters(&mut self, cig_id: u8, cis_ids: &[u8]) {
        self.central_cis_links.retain(|l| l.cig_id != cig_id);
        let unset = Address::from_bytes([0; 6], AddressType::RANDOM_DEVICE);
        let mut handles = Vec::with_capacity(cis_ids.len());
        for &cis_id in cis_ids {
            let handle = self.allocate_handle();
            handles.push(handle);
            self.central_cis_links.push(CisLink {
                cig_id,
                cis_id,
                handle,
                acl_self: unset.clone(),
                acl_peer: unset.clone(),
                data_paths: 0,
            });
        }
        // Return parameters: status, cig_id, num_cis, then each handle (u16 LE).
        let mut data = vec![HCI_SUCCESS, cig_id, handles.len() as u8];
        for h in &handles {
            data.extend_from_slice(&h.to_le_bytes());
        }
        self.complete(
            HCI_LE_SET_CIG_PARAMETERS_COMMAND,
            ReturnParameters::Raw { data },
        );
    }

    /// `LE_Create_CIS` (central): bind each CIS to its ACL connection and send a
    /// `CisReq` to the peer. Acknowledged with Command Status.
    fn handle_create_cis(&mut self, cis_handles: &[u16], acl_handles: &[u16]) {
        for (&cis_handle, &acl_handle) in cis_handles.iter().zip(acl_handles) {
            let Some(conn) = self.connection_by_handle(acl_handle) else {
                return self.command_status(HCI_LE_CREATE_CIS_COMMAND, INVALID_COMMAND_PARAMETERS);
            };
            let (acl_self, acl_peer) = (conn.self_address.clone(), conn.peer_address.clone());
            let Some(link) = self
                .central_cis_links
                .iter_mut()
                .find(|l| l.handle == cis_handle)
            else {
                return self.command_status(HCI_LE_CREATE_CIS_COMMAND, INVALID_COMMAND_PARAMETERS);
            };
            link.acl_self = acl_self.clone();
            link.acl_peer = acl_peer.clone();
            let (cig_id, cis_id) = (link.cig_id, link.cis_id);
            self.queue_ll(
                acl_self,
                acl_peer,
                ll::ControlPdu::CisReq { cig_id, cis_id },
            );
        }
        self.command_status(HCI_LE_CREATE_CIS_COMMAND, HCI_SUCCESS);
    }

    /// `LE_Accept_CIS_Request` (peripheral): send a `CisRsp` for the pending CIS
    /// and acknowledge with Command Status.
    fn handle_accept_cis_request(&mut self, connection_handle: u16) {
        let Some(link) = self
            .peripheral_cis_links
            .iter()
            .find(|l| l.handle == connection_handle)
        else {
            return self.command_status(
                HCI_LE_ACCEPT_CIS_REQUEST_COMMAND,
                INVALID_COMMAND_PARAMETERS,
            );
        };
        let (acl_self, acl_peer, cig_id, cis_id) = (
            link.acl_self.clone(),
            link.acl_peer.clone(),
            link.cig_id,
            link.cis_id,
        );
        self.queue_ll(
            acl_self,
            acl_peer,
            ll::ControlPdu::CisRsp { cig_id, cis_id },
        );
        self.command_status(HCI_LE_ACCEPT_CIS_REQUEST_COMMAND, HCI_SUCCESS);
    }

    fn handle_setup_iso_data_path(&mut self, connection_handle: u16, direction: u8) {
        let status = if direction > 1 {
            INVALID_COMMAND_PARAMETERS
        } else if let Some(link) = self.cis_link_by_handle_mut(connection_handle) {
            let bit = 1 << direction;
            if link.data_paths & bit != 0 {
                COMMAND_DISALLOWED_ERROR
            } else {
                link.data_paths |= bit;
                HCI_SUCCESS
            }
        } else {
            UNKNOWN_CONNECTION_IDENTIFIER_ERROR
        };
        self.complete_status_and_handle(
            HCI_LE_SETUP_ISO_DATA_PATH_COMMAND,
            status,
            connection_handle,
        );
    }

    fn handle_remove_iso_data_path(&mut self, connection_handle: u16, directions: u8) {
        let status = if directions & !0x03 != 0 {
            INVALID_COMMAND_PARAMETERS
        } else if let Some(link) = self.cis_link_by_handle_mut(connection_handle) {
            if link.data_paths & directions != directions {
                COMMAND_DISALLOWED_ERROR
            } else {
                link.data_paths &= !directions;
                HCI_SUCCESS
            }
        } else {
            UNKNOWN_CONNECTION_IDENTIFIER_ERROR
        };
        self.complete_status_and_handle(
            HCI_LE_REMOVE_ISO_DATA_PATH_COMMAND,
            status,
            connection_handle,
        );
    }

    fn complete_status_and_handle(&mut self, op_code: u16, status: u8, handle: u16) {
        let mut data = vec![status];
        data.extend_from_slice(&handle.to_le_bytes());
        self.complete(op_code, ReturnParameters::Raw { data });
    }

    /// Handle an incoming `CisReq` (peripheral side): record a pending CIS link
    /// and raise an `LE CIS Request` event to the host.
    fn on_le_cis_request(
        &mut self,
        acl_self: Address,
        acl_peer: Address,
        acl_handle: u16,
        cig_id: u8,
        cis_id: u8,
    ) {
        let handle = self.allocate_handle();
        self.peripheral_cis_links.push(CisLink {
            cig_id,
            cis_id,
            handle,
            acl_self,
            acl_peer,
            data_paths: 0,
        });
        self.host_queue
            .push(HciPacket::Event(Event::LeMeta(LeMetaEvent::CisRequest {
                acl_connection_handle: acl_handle,
                cis_connection_handle: handle,
                cig_id,
                cis_id,
            })));
    }

    /// Emit an `LE CIS Established` for the CIS identified by `(cig_id, cis_id)`.
    /// CIS timing parameters are placeholders (as upstream — they are ignored).
    fn on_le_cis_established(&mut self, cig_id: u8, cis_id: u8) {
        let Some(handle) = self
            .central_cis_links
            .iter()
            .chain(self.peripheral_cis_links.iter())
            .find(|l| l.cig_id == cig_id && l.cis_id == cis_id)
            .map(|l| l.handle)
        else {
            return;
        };
        self.host_queue.push(HciPacket::Event(Event::LeMeta(
            LeMetaEvent::CisEstablished {
                status: HCI_SUCCESS,
                connection_handle: handle,
                cig_sync_delay: 0,
                cis_sync_delay: 0,
                transport_latency_c_to_p: 0,
                transport_latency_p_to_c: 0,
                phy_c_to_p: LE_1M_PHY,
                phy_p_to_c: LE_1M_PHY,
                nse: 0,
                bn_c_to_p: 0,
                bn_p_to_c: 0,
                ft_c_to_p: 0,
                ft_p_to_c: 0,
                max_pdu_c_to_p: 0,
                max_pdu_p_to_c: 0,
                iso_interval: 0,
            },
        )));
    }

    /// Emit an `Encryption Change` (enabled) for a connection.
    fn on_le_encrypted(&mut self, connection_handle: u16) {
        self.host_queue
            .push(HciPacket::Event(Event::EncryptionChange {
                status: HCI_SUCCESS,
                connection_handle,
                encryption_enabled: 1,
            }));
    }

    // ---- Classic (BR/EDR) ----

    /// `Create_Connection` (classic, central): record a pending classic
    /// connection and page the peer with an `LmpHostConnectionReq`. Acknowledged
    /// with Command Status; Connection Complete follows once the peer accepts.
    fn handle_create_connection(&mut self, bd_addr: Address, allow_role_switch: u8) {
        self.classic_connections.push(Connection {
            handle: 0,
            role: ROLE_CENTRAL,
            classic_allow_role_switch: allow_role_switch != 0,
            self_address: self.public_address.clone(),
            peer_address: bd_addr.clone(),
        });
        self.command_status(HCI_CREATE_CONNECTION_COMMAND, HCI_SUCCESS);
        let self_addr = self.public_address.clone();
        self.queue_classic(self_addr, bd_addr, lmp::ClassicPdu::HostConnectionReq);
    }

    /// `Accept_Connection_Request`: accept as Peripheral immediately, or first
    /// negotiate an LMP role switch when the host requests the Central role.
    fn handle_accept_connection_request(&mut self, bd_addr: Address, role: u8) {
        if role > ROLE_PERIPHERAL {
            return self.command_status(
                HCI_ACCEPT_CONNECTION_REQUEST_COMMAND,
                INVALID_COMMAND_PARAMETERS,
            );
        }
        let Some(_) = self
            .classic_connections
            .iter()
            .position(|c| c.peer_address == bd_addr)
        else {
            return self.command_status(
                HCI_ACCEPT_CONNECTION_REQUEST_COMMAND,
                UNKNOWN_CONNECTION_IDENTIFIER_ERROR,
            );
        };
        self.command_status(HCI_ACCEPT_CONNECTION_REQUEST_COMMAND, HCI_SUCCESS);
        if role == ROLE_CENTRAL {
            self.pending_classic_role_switches
                .push(PendingClassicRoleSwitch {
                    peer_address: bd_addr.clone(),
                    purpose: ClassicRoleSwitchPurpose::AcceptConnection,
                });
            let self_addr = self.public_address.clone();
            self.queue_classic(self_addr, bd_addr, lmp::ClassicPdu::SwitchReq);
        } else {
            self.complete_classic_accept(bd_addr);
        }
    }

    /// `Switch_Role`: report the current role immediately when no change is
    /// needed, otherwise wait for the peer's LMP response.
    fn handle_switch_role(&mut self, bd_addr: Address, role: u8) {
        if role > ROLE_PERIPHERAL {
            return self.command_status(HCI_SWITCH_ROLE_COMMAND, INVALID_COMMAND_PARAMETERS);
        }
        let Some(connection) = self
            .classic_connections
            .iter()
            .find(|connection| connection.peer_address == bd_addr && connection.handle != 0)
        else {
            return self.command_status(HCI_SWITCH_ROLE_COMMAND, COMMAND_DISALLOWED_ERROR);
        };
        let current_role = connection.role;
        self.command_status(HCI_SWITCH_ROLE_COMMAND, HCI_SUCCESS);
        if current_role == role {
            self.push_role_change(HCI_SUCCESS, bd_addr, current_role);
            return;
        }
        self.pending_classic_role_switches
            .push(PendingClassicRoleSwitch {
                peer_address: bd_addr.clone(),
                purpose: ClassicRoleSwitchPurpose::SwitchRoleCommand {
                    requested_role: role,
                },
            });
        let self_addr = self.public_address.clone();
        self.queue_classic(self_addr, bd_addr, lmp::ClassicPdu::SwitchReq);
    }

    fn complete_classic_accept(&mut self, bd_addr: Address) {
        let Some(index) = self
            .classic_connections
            .iter()
            .position(|connection| connection.peer_address == bd_addr)
        else {
            return;
        };
        let handle = self.allocate_handle();
        self.classic_connections[index].handle = handle;
        self.push_classic_connection_complete(handle, bd_addr.clone());
        let self_addr = self.public_address.clone();
        self.queue_classic(self_addr, bd_addr, lmp::ClassicPdu::Accepted);
    }

    /// `Remote_Name_Request` (classic): page the peer for its name.
    fn handle_remote_name_request(&mut self, bd_addr: Address) {
        self.command_status(HCI_REMOTE_NAME_REQUEST_COMMAND, HCI_SUCCESS);
        let self_addr = self.public_address.clone();
        self.queue_classic(self_addr, bd_addr, lmp::ClassicPdu::NameReq);
    }

    /// `Read_Remote_Supported_Features` (classic): request the peer's LMP features.
    fn handle_read_remote_supported_features(&mut self, connection_handle: u16) {
        let Some(conn) = self
            .classic_connections
            .iter()
            .find(|c| c.handle == connection_handle)
        else {
            return self.command_status(
                HCI_READ_REMOTE_SUPPORTED_FEATURES_COMMAND,
                INVALID_COMMAND_PARAMETERS,
            );
        };
        let (self_addr, peer_addr) = (conn.self_address.clone(), conn.peer_address.clone());
        self.command_status(HCI_READ_REMOTE_SUPPORTED_FEATURES_COMMAND, HCI_SUCCESS);
        self.queue_classic(self_addr, peer_addr, lmp::ClassicPdu::FeaturesReq);
    }

    fn handle_set_classic_encryption(&mut self, connection_handle: u16, encryption_enable: u8) {
        let Some(connection) = self
            .classic_connections
            .iter()
            .find(|connection| connection.handle == connection_handle)
        else {
            return self.command_status(
                HCI_SET_CONNECTION_ENCRYPTION_COMMAND,
                UNKNOWN_CONNECTION_IDENTIFIER_ERROR,
            );
        };
        if encryption_enable > 1 {
            return self.command_status(
                HCI_SET_CONNECTION_ENCRYPTION_COMMAND,
                INVALID_COMMAND_PARAMETERS,
            );
        }
        let self_address = connection.self_address.clone();
        let peer_address = connection.peer_address.clone();
        self.command_status(HCI_SET_CONNECTION_ENCRYPTION_COMMAND, HCI_SUCCESS);
        self.host_queue
            .push(HciPacket::Event(Event::EncryptionChange {
                status: HCI_SUCCESS,
                connection_handle,
                encryption_enabled: encryption_enable,
            }));
        self.queue_classic(
            self_address,
            peer_address,
            lmp::ClassicPdu::EncryptionModeReq {
                enable: encryption_enable != 0,
            },
        );
    }

    /// Start an SCO/eSCO logical link over an established Classic ACL.
    fn handle_setup_synchronous_connection(
        &mut self,
        acl_handle: u16,
        link_type: u8,
        air_mode: u8,
    ) {
        let Some(acl) = self
            .classic_connections
            .iter()
            .find(|connection| connection.handle == acl_handle)
        else {
            return self.command_status(
                HCI_ENHANCED_SETUP_SYNCHRONOUS_CONNECTION_COMMAND,
                UNKNOWN_CONNECTION_IDENTIFIER_ERROR,
            );
        };
        let (self_address, peer_address) = (acl.self_address.clone(), acl.peer_address.clone());
        if self
            .synchronous_connections
            .iter()
            .any(|connection| connection.peer_address == peer_address && connection.handle != 0)
        {
            return self.command_status(
                HCI_ENHANCED_SETUP_SYNCHRONOUS_CONNECTION_COMMAND,
                INVALID_COMMAND_PARAMETERS,
            );
        }
        self.synchronous_connections.push(SynchronousConnection {
            handle: 0,
            acl_handle,
            self_address: self_address.clone(),
            peer_address: peer_address.clone(),
            link_type,
            air_mode,
        });
        self.command_status(
            HCI_ENHANCED_SETUP_SYNCHRONOUS_CONNECTION_COMMAND,
            HCI_SUCCESS,
        );
        self.queue_classic(
            self_address,
            peer_address,
            lmp::ClassicPdu::SynchronousConnectionReq {
                link_type,
                air_mode,
            },
        );
    }

    fn handle_accept_synchronous_connection(
        &mut self,
        command_opcode: u16,
        bd_addr: Address,
        requested_air_mode: u8,
    ) {
        let Some(index) = self
            .synchronous_connections
            .iter()
            .position(|connection| connection.peer_address == bd_addr && connection.handle == 0)
        else {
            return self.command_status(command_opcode, UNKNOWN_CONNECTION_IDENTIFIER_ERROR);
        };
        let handle = self.allocate_handle();
        let connection = &mut self.synchronous_connections[index];
        connection.handle = handle;
        connection.air_mode = requested_air_mode;
        let (self_address, peer_address, link_type, air_mode) = (
            connection.self_address.clone(),
            connection.peer_address.clone(),
            connection.link_type,
            connection.air_mode,
        );
        self.command_status(command_opcode, HCI_SUCCESS);
        self.push_synchronous_connection_complete(handle, bd_addr, link_type, air_mode);
        self.queue_classic(
            self_address,
            peer_address,
            lmp::ClassicPdu::SynchronousConnectionAccepted {
                link_type,
                air_mode,
            },
        );
    }

    fn handle_reject_synchronous_connection(&mut self, bd_addr: Address, reason: u8) {
        let Some(index) = self
            .synchronous_connections
            .iter()
            .position(|connection| connection.peer_address == bd_addr && connection.handle == 0)
        else {
            return self.command_status(
                HCI_REJECT_SYNCHRONOUS_CONNECTION_REQUEST_COMMAND,
                UNKNOWN_CONNECTION_IDENTIFIER_ERROR,
            );
        };
        let connection = self.synchronous_connections.remove(index);
        self.command_status(
            HCI_REJECT_SYNCHRONOUS_CONNECTION_REQUEST_COMMAND,
            HCI_SUCCESS,
        );
        self.queue_classic(
            connection.self_address,
            connection.peer_address,
            lmp::ClassicPdu::SynchronousConnectionRejected { reason },
        );
    }

    fn push_synchronous_connection_complete(
        &mut self,
        handle: u16,
        bd_addr: Address,
        link_type: u8,
        air_mode: u8,
    ) {
        self.host_queue
            .push(HciPacket::Event(Event::SynchronousConnectionComplete {
                status: HCI_SUCCESS,
                connection_handle: handle,
                bd_addr,
                link_type,
                transmission_interval: 0,
                retransmission_window: 0,
                rx_packet_length: 0,
                tx_packet_length: 0,
                air_mode,
            }));
    }

    fn push_classic_connection_complete(&mut self, handle: u16, bd_addr: Address) {
        self.host_queue
            .push(HciPacket::Event(Event::ConnectionComplete {
                status: HCI_SUCCESS,
                connection_handle: handle,
                bd_addr,
                link_type: LINK_TYPE_ACL,
                encryption_enabled: 0,
            }));
    }

    fn push_classic_connection_failure(&mut self, status: u8, bd_addr: Address) {
        self.host_queue
            .push(HciPacket::Event(Event::ConnectionComplete {
                status,
                connection_handle: 0,
                bd_addr,
                link_type: LINK_TYPE_ACL,
                encryption_enabled: 0,
            }));
    }

    fn push_role_change(&mut self, status: u8, bd_addr: Address, new_role: u8) {
        self.host_queue.push(HciPacket::Event(Event::RoleChange {
            status,
            bd_addr,
            new_role,
        }));
    }

    fn queue_classic(&mut self, self_addr: Address, peer_addr: Address, pdu: lmp::ClassicPdu) {
        self.outbound_classic.push((self_addr, peer_addr, pdu));
    }

    fn take_outbound_classic(&mut self) -> Vec<(Address, Address, lmp::ClassicPdu)> {
        std::mem::take(&mut self.outbound_classic)
    }

    /// Handle a classic LMP PDU received from the peer at `sender_address`.
    fn on_classic_pdu(&mut self, sender_address: &Address, pdu: lmp::ClassicPdu) {
        match pdu {
            lmp::ClassicPdu::HostConnectionReq => {
                self.classic_connections.push(Connection {
                    handle: 0,
                    role: ROLE_PERIPHERAL,
                    classic_allow_role_switch: self.classic_allow_role_switch,
                    self_address: self.public_address.clone(),
                    peer_address: sender_address.clone(),
                });
                self.host_queue
                    .push(HciPacket::Event(Event::ConnectionRequest {
                        bd_addr: sender_address.clone(),
                        class_of_device: 0,
                        link_type: LINK_TYPE_ACL,
                    }));
            }
            lmp::ClassicPdu::Accepted => {
                if let Some(idx) = self
                    .classic_connections
                    .iter()
                    .position(|c| c.peer_address == *sender_address && c.handle == 0)
                {
                    let handle = self.allocate_handle();
                    self.classic_connections[idx].handle = handle;
                    self.push_classic_connection_complete(handle, sender_address.clone());
                }
            }
            lmp::ClassicPdu::Rejected { reason } => {
                if let Some(index) = self.classic_connections.iter().position(|connection| {
                    connection.peer_address == *sender_address && connection.handle == 0
                }) {
                    self.classic_connections.remove(index);
                    self.push_classic_connection_failure(reason, sender_address.clone());
                }
            }
            lmp::ClassicPdu::SwitchReq => {
                let Some(index) = self
                    .classic_connections
                    .iter()
                    .position(|connection| connection.peer_address == *sender_address)
                else {
                    return;
                };
                let self_addr = self.public_address.clone();
                if !self.classic_connections[index].classic_allow_role_switch {
                    self.queue_classic(
                        self_addr,
                        sender_address.clone(),
                        lmp::ClassicPdu::SwitchRejected {
                            reason: ROLE_CHANGE_NOT_ALLOWED_ERROR,
                        },
                    );
                } else {
                    let new_role = if self.classic_connections[index].role == ROLE_CENTRAL {
                        ROLE_PERIPHERAL
                    } else {
                        ROLE_CENTRAL
                    };
                    self.classic_connections[index].role = new_role;
                    self.push_role_change(HCI_SUCCESS, sender_address.clone(), new_role);
                    self.queue_classic(
                        self_addr,
                        sender_address.clone(),
                        lmp::ClassicPdu::SwitchAccepted,
                    );
                }
            }
            lmp::ClassicPdu::SwitchAccepted => {
                let Some(pending_index) = self
                    .pending_classic_role_switches
                    .iter()
                    .position(|pending| pending.peer_address == *sender_address)
                else {
                    return;
                };
                let pending = self.pending_classic_role_switches.remove(pending_index);
                match pending.purpose {
                    ClassicRoleSwitchPurpose::AcceptConnection => {
                        if let Some(connection) = self
                            .classic_connections
                            .iter_mut()
                            .find(|connection| connection.peer_address == *sender_address)
                        {
                            connection.role = ROLE_CENTRAL;
                        }
                        self.push_role_change(HCI_SUCCESS, sender_address.clone(), ROLE_CENTRAL);
                        self.complete_classic_accept(sender_address.clone());
                    }
                    ClassicRoleSwitchPurpose::SwitchRoleCommand { requested_role } => {
                        if let Some(connection) = self
                            .classic_connections
                            .iter_mut()
                            .find(|connection| connection.peer_address == *sender_address)
                        {
                            connection.role = requested_role;
                        }
                        self.push_role_change(HCI_SUCCESS, sender_address.clone(), requested_role);
                    }
                }
            }
            lmp::ClassicPdu::SwitchRejected { reason } => {
                let Some(pending_index) = self
                    .pending_classic_role_switches
                    .iter()
                    .position(|pending| pending.peer_address == *sender_address)
                else {
                    return;
                };
                let pending = self.pending_classic_role_switches.remove(pending_index);
                match pending.purpose {
                    ClassicRoleSwitchPurpose::AcceptConnection => {
                        self.push_classic_connection_failure(reason, sender_address.clone());
                        self.classic_connections
                            .retain(|connection| connection.peer_address != *sender_address);
                        let self_addr = self.public_address.clone();
                        self.queue_classic(
                            self_addr,
                            sender_address.clone(),
                            lmp::ClassicPdu::Rejected { reason },
                        );
                    }
                    ClassicRoleSwitchPurpose::SwitchRoleCommand { .. } => {
                        let role = self
                            .classic_connections
                            .iter()
                            .find(|connection| connection.peer_address == *sender_address)
                            .map_or(ROLE_CENTRAL, |connection| connection.role);
                        self.push_role_change(reason, sender_address.clone(), role);
                    }
                }
            }
            lmp::ClassicPdu::NameReq => {
                let mut name = self.name.as_bytes().to_vec();
                name.resize(248, 0);
                let self_addr = self.public_address.clone();
                self.queue_classic(
                    self_addr,
                    sender_address.clone(),
                    lmp::ClassicPdu::NameRes { name },
                );
            }
            lmp::ClassicPdu::NameRes { name } => {
                let mut remote_name = [0u8; 248];
                let n = name.len().min(248);
                remote_name[..n].copy_from_slice(&name[..n]);
                self.host_queue
                    .push(HciPacket::Event(Event::RemoteNameRequestComplete {
                        status: HCI_SUCCESS,
                        bd_addr: sender_address.clone(),
                        remote_name,
                    }));
            }
            lmp::ClassicPdu::FeaturesReq => {
                let self_addr = self.public_address.clone();
                self.queue_classic(
                    self_addr,
                    sender_address.clone(),
                    lmp::ClassicPdu::FeaturesRes {
                        features: LMP_FEATURES,
                    },
                );
            }
            lmp::ClassicPdu::FeaturesRes { features } => {
                if let Some(conn) = self
                    .classic_connections
                    .iter()
                    .find(|c| c.peer_address == *sender_address)
                {
                    let handle = conn.handle;
                    self.host_queue.push(HciPacket::Event(
                        Event::ReadRemoteSupportedFeaturesComplete {
                            status: HCI_SUCCESS,
                            connection_handle: handle,
                            lmp_features: features,
                        },
                    ));
                }
            }
            lmp::ClassicPdu::EncryptionModeReq { enable } => {
                if let Some(connection) = self
                    .classic_connections
                    .iter()
                    .find(|connection| connection.peer_address == *sender_address)
                {
                    self.host_queue
                        .push(HciPacket::Event(Event::EncryptionChange {
                            status: HCI_SUCCESS,
                            connection_handle: connection.handle,
                            encryption_enabled: u8::from(enable),
                        }));
                }
            }
            lmp::ClassicPdu::SynchronousConnectionReq {
                link_type,
                air_mode,
            } => {
                let Some(acl_handle) = self
                    .classic_connections
                    .iter()
                    .find(|connection| connection.peer_address == *sender_address)
                    .map(|connection| connection.handle)
                else {
                    return;
                };
                self.synchronous_connections.push(SynchronousConnection {
                    handle: 0,
                    acl_handle,
                    self_address: self.public_address.clone(),
                    peer_address: sender_address.clone(),
                    link_type,
                    air_mode,
                });
                self.host_queue
                    .push(HciPacket::Event(Event::ConnectionRequest {
                        bd_addr: sender_address.clone(),
                        class_of_device: 0,
                        link_type,
                    }));
            }
            lmp::ClassicPdu::SynchronousConnectionAccepted {
                link_type,
                air_mode,
            } => {
                if let Some(index) = self.synchronous_connections.iter().position(|connection| {
                    connection.peer_address == *sender_address && connection.handle == 0
                }) {
                    let handle = self.allocate_handle();
                    let connection = &mut self.synchronous_connections[index];
                    connection.handle = handle;
                    connection.link_type = link_type;
                    connection.air_mode = air_mode;
                    self.push_synchronous_connection_complete(
                        handle,
                        sender_address.clone(),
                        link_type,
                        air_mode,
                    );
                }
            }
            lmp::ClassicPdu::SynchronousConnectionRejected { reason } => {
                if let Some(index) = self.synchronous_connections.iter().position(|connection| {
                    connection.peer_address == *sender_address && connection.handle == 0
                }) {
                    let connection = self.synchronous_connections.remove(index);
                    self.host_queue
                        .push(HciPacket::Event(Event::SynchronousConnectionComplete {
                            status: reason,
                            connection_handle: 0,
                            bd_addr: sender_address.clone(),
                            link_type: connection.link_type,
                            transmission_interval: 0,
                            retransmission_window: 0,
                            rx_packet_length: 0,
                            tx_packet_length: 0,
                            air_mode: connection.air_mode,
                        }));
                }
            }
            lmp::ClassicPdu::SynchronousDetach { error_code } => {
                if let Some(index) = self.synchronous_connections.iter().position(|connection| {
                    connection.peer_address == *sender_address && connection.handle != 0
                }) {
                    let connection = self.synchronous_connections.remove(index);
                    self.host_queue
                        .push(HciPacket::Event(Event::DisconnectionComplete {
                            status: HCI_SUCCESS,
                            connection_handle: connection.handle,
                            reason: error_code,
                        }));
                }
            }
            lmp::ClassicPdu::Detach { error_code } => {
                if let Some(idx) = self
                    .classic_connections
                    .iter()
                    .position(|c| c.peer_address == *sender_address)
                {
                    let conn = self.classic_connections.remove(idx);
                    self.host_queue
                        .push(HciPacket::Event(Event::DisconnectionComplete {
                            status: HCI_SUCCESS,
                            connection_handle: conn.handle,
                            reason: error_code,
                        }));
                }
            }
        }
    }

    /// Queue an LL control PDU for delivery to the peer at `peer_addr`.
    fn queue_ll(&mut self, self_addr: Address, peer_addr: Address, pdu: ll::ControlPdu) {
        self.outbound_ll.push((self_addr, peer_addr, pdu));
    }

    /// Remove and return the LL control PDUs queued for peers, as
    /// `(sender_self_address, receiver_peer_address, pdu)`.
    fn take_outbound_ll(&mut self) -> Vec<(Address, Address, ll::ControlPdu)> {
        std::mem::take(&mut self.outbound_ll)
    }

    fn take_outbound_periodic_sync_transfers(
        &mut self,
    ) -> Vec<(Address, Address, PeriodicSyncTransfer)> {
        std::mem::take(&mut self.outbound_periodic_sync_transfers)
    }

    fn on_periodic_sync_transfer(
        &mut self,
        sender_address: &Address,
        transfer: PeriodicSyncTransfer,
    ) {
        let Some(connection_handle) = self
            .connections
            .iter()
            .find(|connection| connection.peer_address == *sender_address)
            .map(|connection| connection.handle)
        else {
            return;
        };
        let sync_handle = self.next_periodic_sync_handle;
        self.next_periodic_sync_handle = self.next_periodic_sync_handle.wrapping_add(1);
        self.periodic_syncs.push(PeriodicSync {
            handle: sync_handle,
            advertising_sid: transfer.advertising_sid,
            advertiser_address_type: transfer.advertiser_address_type,
            advertiser_address: transfer.advertiser_address.clone(),
            advertiser_phy: transfer.advertiser_phy,
            interval: transfer.interval,
            advertiser_clock_accuracy: transfer.advertiser_clock_accuracy,
            receive_enabled: true,
        });
        self.host_queue.push(HciPacket::Event(Event::LeMeta(
            LeMetaEvent::PeriodicAdvertisingSyncTransferReceived {
                status: HCI_SUCCESS,
                connection_handle,
                service_data: transfer.service_data,
                sync_handle,
                advertising_sid: transfer.advertising_sid,
                advertiser_address_type: transfer.advertiser_address_type,
                advertiser_address: transfer.advertiser_address,
                advertiser_phy: transfer.advertiser_phy,
                periodic_advertising_interval: transfer.interval,
                advertiser_clock_accuracy: transfer.advertiser_clock_accuracy,
            },
        )));
    }

    /// Handle an LL control PDU received from the peer at `sender_address`,
    /// mirroring upstream `controller.py::on_ll_control_pdu`.
    fn on_ll_control_pdu(&mut self, sender_address: &Address, pdu: ll::ControlPdu) {
        let Some(conn) = self
            .connections
            .iter()
            .find(|c| c.peer_address == *sender_address)
        else {
            return;
        };
        let (self_addr, handle) = (conn.self_address.clone(), conn.handle);
        match pdu {
            ll::ControlPdu::EncReq { .. } => self.on_le_encrypted(handle),
            ll::ControlPdu::FeatureReq { .. } | ll::ControlPdu::PeripheralFeatureReq { .. } => {
                self.queue_ll(
                    self_addr,
                    sender_address.clone(),
                    ll::ControlPdu::FeatureRsp {
                        feature_set: LOCAL_LE_FEATURES,
                    },
                );
            }
            ll::ControlPdu::FeatureRsp { feature_set } => {
                self.host_queue.push(HciPacket::Event(Event::LeMeta(
                    LeMetaEvent::ReadRemoteFeaturesComplete {
                        status: HCI_SUCCESS,
                        connection_handle: handle,
                        le_features: feature_set,
                    },
                )));
            }
            ll::ControlPdu::CisReq { cig_id, cis_id } => {
                self.on_le_cis_request(self_addr, sender_address.clone(), handle, cig_id, cis_id);
            }
            ll::ControlPdu::CisRsp { cig_id, cis_id } => {
                self.on_le_cis_established(cig_id, cis_id);
                self.queue_ll(
                    self_addr,
                    sender_address.clone(),
                    ll::ControlPdu::CisInd { cig_id, cis_id },
                );
            }
            ll::ControlPdu::CisInd { cig_id, cis_id } => {
                self.on_le_cis_established(cig_id, cis_id);
            }
            ll::ControlPdu::TerminateInd { error_code } => {
                let peer = sender_address.clone();
                self.on_peer_disconnect(&self_addr, &peer, ConnectionKind::LeAcl, error_code);
            }
        }
    }

    /// Remove and return all host-bound HCI packets queued so far.
    pub fn drain_host_events(&mut self) -> Vec<HciPacket> {
        std::mem::take(&mut self.host_queue)
    }

    /// The advertising PDU this controller currently broadcasts, if advertising
    /// is enabled.
    pub fn advertising_pdu(&self) -> Option<AdvertisingPdu> {
        if !self.advertising_enabled {
            return None;
        }
        Some(AdvertisingPdu {
            event_type: ADV_IND,
            address_type: ADDRESS_TYPE_RANDOM,
            address: self.random_address.clone(),
            data: self.advertising_data.clone(),
            scan_response_data: Vec::new(),
            extended: false,
            advertising_handle: 0,
            advertising_sid: 0,
            primary_phy: LE_1M_PHY,
            secondary_phy: LE_1M_PHY,
            tx_power: 0,
            direct_address: None,
        })
    }

    /// Every legacy or extended advertising PDU currently emitted by this
    /// controller. Extended sets without parameters or a usable own address do
    /// not go on air, matching upstream's `AdvertisingSet.address` guard.
    pub fn advertising_pdus(&self) -> Vec<AdvertisingPdu> {
        let mut pdus = Vec::new();
        if let Some(legacy) = self.advertising_pdu() {
            pdus.push(legacy);
        }
        for set in self
            .extended_advertising_sets
            .iter()
            .filter(|set| set.enabled)
        {
            let Some(parameters) = set.parameters.as_ref() else {
                continue;
            };
            let Some(address) = set.address(&self.public_address) else {
                continue;
            };
            let direct_address = if parameters.advertising_event_properties & 0x0004 != 0 {
                Some(Address::from_bytes(
                    *parameters.peer_address.address_bytes(),
                    AddressType(parameters.peer_address_type),
                ))
            } else {
                None
            };
            pdus.push(AdvertisingPdu {
                event_type: (parameters.advertising_event_properties & 0x1F) as u8,
                address_type: address.address_type().0,
                address,
                data: set.data.clone(),
                scan_response_data: set.scan_response_data.clone(),
                extended: true,
                advertising_handle: set.handle,
                advertising_sid: parameters.advertising_sid,
                primary_phy: parameters.primary_advertising_phy,
                secondary_phy: parameters.secondary_advertising_phy,
                tx_power: parameters.advertising_tx_power,
                direct_address,
            });
        }
        pdus
    }

    /// Every enabled periodic advertising train currently on air.
    pub fn periodic_advertising_pdus(&self) -> Vec<PeriodicAdvertisingPdu> {
        self.extended_advertising_sets
            .iter()
            .filter(|set| set.enabled && set.periodic_enabled)
            .filter_map(|set| {
                let parameters = set.parameters.as_ref()?;
                let periodic = set.periodic_parameters?;
                let address = set.address(&self.public_address)?;
                Some(PeriodicAdvertisingPdu {
                    address_type: address.address_type().0,
                    address,
                    advertising_sid: parameters.advertising_sid,
                    advertiser_phy: parameters.secondary_advertising_phy,
                    interval: periodic.interval_min,
                    tx_power: parameters.advertising_tx_power,
                    data: set.periodic_data.clone(),
                })
            })
            .collect()
    }

    pub fn periodic_advertising_enabled(&self, handle: u8) -> bool {
        self.extended_advertising_sets
            .iter()
            .any(|set| set.handle == handle && set.periodic_enabled)
    }

    pub fn periodic_sync_handles(&self) -> Vec<u16> {
        self.periodic_syncs.iter().map(|sync| sync.handle).collect()
    }

    /// Establish a pending synchronization and emit reports for active syncs.
    pub fn on_periodic_advertising_pdu(&mut self, pdu: &PeriodicAdvertisingPdu) {
        if self.pending_periodic_sync.as_ref().is_some_and(|pending| {
            pending.advertising_sid == pdu.advertising_sid
                && pending.advertiser_address_type == pdu.address_type
                && pending.advertiser_address == pdu.address
        }) {
            let Some(pending) = self.pending_periodic_sync.take() else {
                return;
            };
            let sync_handle = self.next_periodic_sync_handle;
            self.next_periodic_sync_handle = self.next_periodic_sync_handle.wrapping_add(1);
            self.periodic_syncs.push(PeriodicSync {
                handle: sync_handle,
                advertising_sid: pending.advertising_sid,
                advertiser_address_type: pending.advertiser_address_type,
                advertiser_address: pending.advertiser_address.clone(),
                advertiser_phy: pdu.advertiser_phy,
                interval: pdu.interval,
                advertiser_clock_accuracy: 0,
                receive_enabled: true,
            });
            self.host_queue.push(HciPacket::Event(Event::LeMeta(
                LeMetaEvent::PeriodicAdvertisingSyncEstablished {
                    status: HCI_SUCCESS,
                    sync_handle,
                    advertising_sid: pending.advertising_sid,
                    advertiser_address_type: pending.advertiser_address_type,
                    advertiser_address: pending.advertiser_address,
                    advertiser_phy: pdu.advertiser_phy,
                    periodic_advertising_interval: pdu.interval,
                    advertiser_clock_accuracy: 0,
                },
            )));
        }

        let matching_handles: Vec<_> = self
            .periodic_syncs
            .iter()
            .filter(|sync| {
                sync.receive_enabled
                    && sync.advertising_sid == pdu.advertising_sid
                    && sync.advertiser_address_type == pdu.address_type
                    && sync.advertiser_address == pdu.address
            })
            .map(|sync| sync.handle)
            .collect();
        let chunks: Vec<_> = if pdu.data.is_empty() {
            vec![&[][..]]
        } else {
            pdu.data.chunks(247).collect()
        };
        for sync_handle in matching_handles {
            for (index, chunk) in chunks.iter().enumerate() {
                self.host_queue.push(HciPacket::Event(Event::LeMeta(
                    LeMetaEvent::PeriodicAdvertisingReport {
                        sync_handle,
                        tx_power: pdu.tx_power,
                        rssi: DEFAULT_RSSI,
                        cte_type: 0xFF,
                        data_status: u8::from(index + 1 != chunks.len()),
                        data: chunk.to_vec(),
                    },
                )));
            }
        }
    }

    /// Handle an advertising PDU received over the link. If scanning is
    /// enabled, queue an LE Advertising Report event to the host.
    pub fn on_advertising_pdu(&mut self, pdu: &AdvertisingPdu) {
        if !self.scanning_enabled {
            return;
        }
        if self.extended_scanning {
            self.push_extended_advertising_report(pdu, false);
            if !pdu.scan_response_data.is_empty() {
                self.push_extended_advertising_report(pdu, true);
            }
        } else {
            self.host_queue.push(HciPacket::Event(Event::LeMeta(
                LeMetaEvent::AdvertisingReport {
                    reports: vec![AdvertisingReport {
                        event_type: pdu.event_type,
                        address_type: pdu.address_type,
                        address: pdu.address.clone(),
                        data: pdu.data.clone(),
                        rssi: DEFAULT_RSSI,
                    }],
                },
            )));
        }
    }

    fn push_extended_advertising_report(&mut self, pdu: &AdvertisingPdu, scan_response: bool) {
        let direct_address = pdu
            .direct_address
            .clone()
            .unwrap_or_else(|| Address::from_bytes([0; 6], AddressType::PUBLIC_DEVICE));
        self.host_queue.push(HciPacket::Event(Event::LeMeta(
            LeMetaEvent::ExtendedAdvertisingReport {
                reports: vec![ExtendedAdvertisingReport {
                    event_type: if scan_response {
                        0x0008
                    } else {
                        u16::from(pdu.event_type)
                    },
                    address_type: pdu.address_type,
                    address: pdu.address.clone(),
                    primary_phy: pdu.primary_phy,
                    secondary_phy: pdu.secondary_phy,
                    advertising_sid: pdu.advertising_sid,
                    tx_power: pdu.tx_power,
                    rssi: DEFAULT_RSSI,
                    periodic_advertising_interval: 0,
                    direct_address_type: pdu
                        .direct_address
                        .as_ref()
                        .map_or(ADDRESS_TYPE_PUBLIC, |address| address.address_type().0),
                    direct_address,
                    data: if scan_response {
                        pdu.scan_response_data.clone()
                    } else {
                        pdu.data.clone()
                    },
                }],
            },
        )));
    }

    /// The connections currently established on this controller.
    pub fn connections(&self) -> &[Connection] {
        &self.connections
    }

    pub fn classic_connections(&self) -> &[Connection] {
        &self.classic_connections
    }

    pub fn synchronous_connections(&self) -> &[SynchronousConnection] {
        &self.synchronous_connections
    }

    fn has_connection(
        &self,
        self_address: &Address,
        peer_address: &Address,
        kind: ConnectionKind,
    ) -> bool {
        let matches = |connection: &Connection| {
            connection.self_address == *self_address && connection.peer_address == *peer_address
        };
        match kind {
            ConnectionKind::LeAcl => self.connections.iter().any(matches),
            ConnectionKind::ClassicAcl => self.classic_connections.iter().any(matches),
            ConnectionKind::Synchronous => self.synchronous_connections.iter().any(|connection| {
                connection.self_address == *self_address && connection.peer_address == *peer_address
            }),
            ConnectionKind::Iso { cig_id, cis_id } => self
                .central_cis_links
                .iter()
                .chain(self.peripheral_cis_links.iter())
                .any(|link| {
                    link.cig_id == cig_id
                        && link.cis_id == cis_id
                        && link.acl_self == *self_address
                        && link.acl_peer == *peer_address
                }),
        }
    }

    /// `true` if an `LE_Create_Connection` is pending (initiating).
    pub fn is_initiating(&self) -> bool {
        self.initiating.is_some()
    }

    /// `true` when the identified extended advertising set is enabled.
    pub fn is_extended_advertising(&self, handle: u8) -> bool {
        self.extended_advertising_sets
            .iter()
            .any(|set| set.handle == handle && set.enabled)
    }

    fn allocate_handle(&mut self) -> u16 {
        let handle = self.next_handle;
        self.next_handle += 1;
        handle
    }

    /// The address this controller presents while initiating, and its type,
    /// if a connection is pending.
    fn initiating_self_address(&self) -> Option<(Address, u8)> {
        self.initiating.as_ref().map(|p| {
            if p.own_address_type == ADDRESS_TYPE_PUBLIC {
                (self.public_address.clone(), ADDRESS_TYPE_PUBLIC)
            } else {
                (self.random_address.clone(), ADDRESS_TYPE_RANDOM)
            }
        })
    }

    fn push_connection_complete(
        &mut self,
        connection: &Connection,
        reported_peer_address: Address,
        peer_address_type: u8,
    ) {
        self.host_queue.push(HciPacket::Event(Event::LeMeta(
            LeMetaEvent::ConnectionComplete {
                status: HCI_SUCCESS,
                connection_handle: connection.handle,
                role: connection.role,
                peer_address_type,
                peer_address: reported_peer_address,
                connection_interval: CONNECTION_INTERVAL,
                peripheral_latency: PERIPHERAL_LATENCY,
                supervision_timeout: SUPERVISION_TIMEOUT,
                central_clock_accuracy: CENTRAL_CLOCK_ACCURACY,
            },
        )));
    }

    /// Complete the pending connection as the central. Emits a Connection
    /// Complete (role = central) and clears the initiating state.
    pub fn connect_as_central(&mut self) {
        let Some(pending) = self.initiating.as_ref() else {
            return;
        };
        self.connect_as_central_to(
            pending.peer_address.clone(),
            pending.peer_address.clone(),
            pending.peer_address_type,
        );
    }

    fn connect_as_central_to(
        &mut self,
        link_peer_address: Address,
        reported_peer_address: Address,
        reported_peer_address_type: u8,
    ) {
        let Some(pending) = self.initiating.take() else {
            return;
        };
        let self_address = if pending.own_address_type == ADDRESS_TYPE_PUBLIC {
            self.public_address.clone()
        } else {
            self.random_address.clone()
        };
        let handle = self.allocate_handle();
        let connection = Connection {
            handle,
            role: ROLE_CENTRAL,
            classic_allow_role_switch: false,
            self_address,
            peer_address: link_peer_address,
        };
        self.push_connection_complete(
            &connection,
            reported_peer_address,
            reported_peer_address_type,
        );
        self.connections.push(connection);
    }

    /// Accept an incoming connection as the peripheral. Emits a Connection
    /// Complete (role = peripheral) and stops advertising.
    pub fn connect_as_peripheral(&mut self, central_address: Address, central_address_type: u8) {
        self.connect_as_peripheral_at(
            self.random_address.clone(),
            central_address,
            central_address_type,
        );
    }

    fn connect_as_peripheral_at(
        &mut self,
        self_address: Address,
        central_address: Address,
        central_address_type: u8,
    ) {
        let handle = self.allocate_handle();
        let connection = Connection {
            handle,
            role: ROLE_PERIPHERAL,
            classic_allow_role_switch: false,
            self_address,
            peer_address: central_address,
        };
        self.push_connection_complete(
            &connection,
            connection.peer_address.clone(),
            central_address_type,
        );
        self.connections.push(connection);
        self.advertising_enabled = false;
    }

    fn resolve_peer_identity(&self, target: &Address, rpa: &Address) -> Option<Address> {
        if !self.address_resolution_enabled || !rpa.is_resolvable() {
            return None;
        }
        let bytes = rpa.address_bytes();
        let hash = &bytes[..3];
        let prand = &bytes[3..];
        self.resolving_list
            .iter()
            .find(|entry| {
                entry.peer_identity_address.address_bytes() == target.address_bytes()
                    && ah(&entry.peer_irk, prand).as_slice() == hash
            })
            .map(|entry| {
                let address_type = if entry.peer_identity_address_type == ADDRESS_TYPE_PUBLIC {
                    AddressType::PUBLIC_IDENTITY
                } else {
                    AddressType::RANDOM_IDENTITY
                };
                Address::from_bytes(*entry.peer_identity_address.address_bytes(), address_type)
            })
    }

    /// Handle a host-initiated `HCI_Disconnect`: acknowledge with a Command
    /// Status, emit a Disconnection Complete, and drop the connection. Returns
    /// the endpoint addresses and connection kind so the link can notify the
    /// peer, or `None` if no such connection existed.
    pub fn request_disconnect(
        &mut self,
        handle: u16,
        reason: u8,
    ) -> Option<(Address, Address, ConnectionKind)> {
        let (self_address, peer_address, kind, dependent_handles) = if let Some(index) =
            self.connections.iter().position(|c| c.handle == handle)
        {
            let connection = self.connections.remove(index);
            let dependent_handles =
                self.remove_cis_for_acl(&connection.self_address, &connection.peer_address);
            (
                connection.self_address,
                connection.peer_address,
                ConnectionKind::LeAcl,
                dependent_handles,
            )
        } else if let Some(index) = self
            .classic_connections
            .iter()
            .position(|c| c.handle == handle)
        {
            let connection = self.classic_connections.remove(index);
            let dependent_synchronous_handles = self
                .remove_synchronous_for_peer(&connection.self_address, &connection.peer_address);
            (
                connection.self_address,
                connection.peer_address,
                ConnectionKind::ClassicAcl,
                dependent_synchronous_handles,
            )
        } else if let Some(index) = self
            .synchronous_connections
            .iter()
            .position(|c| c.handle == handle)
        {
            let connection = self.synchronous_connections.remove(index);
            (
                connection.self_address,
                connection.peer_address,
                ConnectionKind::Synchronous,
                Vec::new(),
            )
        } else if let Some(connection) = self.remove_cis_by_handle(handle) {
            (
                connection.acl_self,
                connection.acl_peer,
                ConnectionKind::Iso {
                    cig_id: connection.cig_id,
                    cis_id: connection.cis_id,
                },
                Vec::new(),
            )
        } else {
            return None;
        };
        self.host_queue.push(HciPacket::Event(Event::CommandStatus {
            status: HCI_SUCCESS,
            num_hci_command_packets: 1,
            command_opcode: HCI_DISCONNECT_COMMAND,
        }));
        for dependent_handle in dependent_handles {
            self.push_disconnection_complete(dependent_handle, reason);
        }
        self.push_disconnection_complete(handle, reason);
        Some((self_address, peer_address, kind))
    }

    /// Notify this controller that the peer dropped the connection identified by
    /// (this controller's) `self_address`/`peer_address`. Emits a Disconnection
    /// Complete and drops the connection.
    pub fn on_peer_disconnect(
        &mut self,
        self_address: &Address,
        peer_address: &Address,
        kind: ConnectionKind,
        reason: u8,
    ) {
        let connection = match kind {
            ConnectionKind::LeAcl => self
                .connections
                .iter()
                .position(|c| c.self_address == *self_address && c.peer_address == *peer_address)
                .map(|index| self.connections.remove(index))
                .map(|connection| {
                    let dependent_handles = self.remove_cis_for_acl(self_address, peer_address);
                    (connection.handle, dependent_handles)
                }),
            ConnectionKind::ClassicAcl => self
                .classic_connections
                .iter()
                .position(|c| c.self_address == *self_address && c.peer_address == *peer_address)
                .map(|index| self.classic_connections.remove(index).handle)
                .map(|handle| {
                    (
                        handle,
                        self.remove_synchronous_for_peer(self_address, peer_address),
                    )
                }),
            ConnectionKind::Synchronous => self
                .synchronous_connections
                .iter()
                .position(|c| c.self_address == *self_address && c.peer_address == *peer_address)
                .map(|index| {
                    (
                        self.synchronous_connections.remove(index).handle,
                        Vec::new(),
                    )
                }),
            ConnectionKind::Iso { cig_id, cis_id } => self
                .remove_cis_for_peer(self_address, peer_address, cig_id, cis_id)
                .map(|connection| (connection.handle, Vec::new())),
        };
        if let Some((handle, dependent_synchronous_handles)) = connection {
            for dependent_handle in dependent_synchronous_handles {
                self.push_disconnection_complete(dependent_handle, reason);
            }
            self.push_disconnection_complete(handle, reason);
        }
    }

    fn remove_synchronous_for_peer(
        &mut self,
        self_address: &Address,
        peer_address: &Address,
    ) -> Vec<u16> {
        let mut handles = Vec::new();
        self.synchronous_connections.retain(|connection| {
            if connection.self_address == *self_address && connection.peer_address == *peer_address
            {
                if connection.handle != 0 {
                    handles.push(connection.handle);
                }
                false
            } else {
                true
            }
        });
        handles
    }

    fn remove_cis_for_acl(&mut self, self_address: &Address, peer_address: &Address) -> Vec<u16> {
        let mut handles = Vec::new();
        self.central_cis_links
            .retain(|link| retain_other_cis(link, self_address, peer_address, &mut handles));
        self.peripheral_cis_links
            .retain(|link| retain_other_cis(link, self_address, peer_address, &mut handles));
        handles
    }

    fn remove_cis_by_handle(&mut self, handle: u16) -> Option<CisLink> {
        if let Some(index) = self
            .central_cis_links
            .iter()
            .position(|link| link.handle == handle)
        {
            return Some(self.central_cis_links.remove(index));
        }
        self.peripheral_cis_links
            .iter()
            .position(|link| link.handle == handle)
            .map(|index| self.peripheral_cis_links.remove(index))
    }

    fn remove_cis_for_peer(
        &mut self,
        self_address: &Address,
        peer_address: &Address,
        cig_id: u8,
        cis_id: u8,
    ) -> Option<CisLink> {
        if let Some(index) = self.central_cis_links.iter().position(|link| {
            link.cig_id == cig_id
                && link.cis_id == cis_id
                && link.acl_self == *self_address
                && link.acl_peer == *peer_address
        }) {
            return Some(self.central_cis_links.remove(index));
        }
        self.peripheral_cis_links
            .iter()
            .position(|link| {
                link.cig_id == cig_id
                    && link.cis_id == cis_id
                    && link.acl_self == *self_address
                    && link.acl_peer == *peer_address
            })
            .map(|index| self.peripheral_cis_links.remove(index))
    }

    fn push_disconnection_complete(&mut self, connection_handle: u16, reason: u8) {
        self.host_queue
            .push(HciPacket::Event(Event::DisconnectionComplete {
                status: HCI_SUCCESS,
                connection_handle,
                reason,
            }));
    }

    fn deliver_acl_packet(&mut self, packet: AclDataPacket) {
        self.host_queue.push(HciPacket::AclData(packet));
    }

    fn complete_acl_packets(&mut self, connection_handle: u16, count: u16) {
        self.host_queue
            .push(HciPacket::Event(Event::NumberOfCompletedPackets {
                connection_handles: vec![connection_handle],
                num_completed_packets: vec![count],
            }));
    }

    fn deliver_synchronous(&mut self, connection_handle: u16, packet_status: u8, data: &[u8]) {
        let Ok(data_total_length) = u8::try_from(data.len()) else {
            return;
        };
        self.host_queue
            .push(HciPacket::SyncData(SynchronousDataPacket {
                connection_handle,
                packet_status,
                data_total_length,
                data: data.to_vec(),
            }));
    }

    fn deliver_iso(&mut self, connection_handle: u16, packet: IsoDataPacket) {
        self.host_queue.push(HciPacket::IsoData(IsoDataPacket {
            connection_handle,
            ..packet
        }));
    }

    fn connection_by_handle(&self, handle: u16) -> Option<&Connection> {
        self.connections.iter().find(|c| c.handle == handle)
    }

    fn connection_by_handle_any(&self, handle: u16) -> Option<&Connection> {
        self.connections
            .iter()
            .chain(self.classic_connections.iter())
            .find(|connection| connection.handle == handle)
    }

    fn cis_link_by_handle(&self, handle: u16) -> Option<&CisLink> {
        self.central_cis_links
            .iter()
            .chain(self.peripheral_cis_links.iter())
            .find(|link| link.handle == handle)
    }

    fn cis_link_by_handle_mut(&mut self, handle: u16) -> Option<&mut CisLink> {
        self.central_cis_links
            .iter_mut()
            .chain(self.peripheral_cis_links.iter_mut())
            .find(|link| link.handle == handle)
    }

    fn ack(&mut self, command_opcode: u16, status: u8) {
        self.complete(command_opcode, ReturnParameters::Status { status });
    }

    /// Queue a Command Status acknowledgement (for commands that complete via a
    /// later event).
    fn command_status(&mut self, command_opcode: u16, status: u8) {
        self.host_queue.push(HciPacket::Event(Event::CommandStatus {
            status,
            num_hci_command_packets: 1,
            command_opcode,
        }));
    }

    /// Queue a Command Complete carrying the given return parameters.
    fn complete(&mut self, command_opcode: u16, return_parameters: ReturnParameters) {
        self.host_queue
            .push(HciPacket::Event(Event::CommandComplete {
                num_hci_command_packets: 1,
                command_opcode,
                return_parameters,
            }));
    }
}

/// Resolve a PHY *value* (1 = LE 1M, 2 = LE 2M, 3 = LE Coded) from an
/// `LE_Set_PHY` preference. With no preference (or an empty mask) the
/// controller keeps LE 1M; otherwise it picks the lowest-numbered PHY the host
/// allows.
fn resolve_phy(no_preference: bool, phys_mask: u8) -> u8 {
    if no_preference || phys_mask == 0 {
        LE_1M_PHY
    } else {
        (phys_mask.trailing_zeros() as u8) + 1
    }
}

fn retain_other_cis(
    link: &CisLink,
    self_address: &Address,
    peer_address: &Address,
    removed_handles: &mut Vec<u16>,
) -> bool {
    if link.acl_self == *self_address && link.acl_peer == *peer_address {
        removed_handles.push(link.handle);
        false
    } else {
        true
    }
}

fn air_mode_for_coding_format(coding_format: u8) -> u8 {
    if coding_format == AIR_MODE_CVSD {
        AIR_MODE_CVSD
    } else {
        AIR_MODE_TRANSPARENT
    }
}

/// An in-process bus that connects controllers so they can exchange
/// advertising PDUs. Owns its controllers; callers address them by the index
/// returned from [`LocalLink::add_controller`].
#[derive(Debug, Default)]
pub struct LocalLink {
    controllers: Vec<Controller>,
}

impl LocalLink {
    pub fn new() -> LocalLink {
        LocalLink::default()
    }

    /// Register a controller, returning its id (index).
    pub fn add_controller(&mut self, controller: Controller) -> usize {
        self.controllers.push(controller);
        self.controllers.len() - 1
    }

    pub fn controller(&self, id: usize) -> &Controller {
        &self.controllers[id]
    }

    pub fn controller_mut(&mut self, id: usize) -> &mut Controller {
        &mut self.controllers[id]
    }

    /// Deliver an HCI command to a controller.
    pub fn handle_command(&mut self, id: usize, command: Command) {
        self.controllers[id].handle_command(command);
    }

    /// Drain host-bound events from a controller.
    pub fn drain_host_events(&mut self, id: usize) -> Vec<HciPacket> {
        self.controllers[id].drain_host_events()
    }

    /// Deliver every advertising controller's PDU to every other controller
    /// (scanning controllers turn it into an LE Advertising Report).
    pub fn propagate_advertising(&mut self) {
        let pdus: Vec<(usize, AdvertisingPdu)> = self
            .controllers
            .iter()
            .enumerate()
            .flat_map(|(i, controller)| {
                controller
                    .advertising_pdus()
                    .into_iter()
                    .map(move |pdu| (i, pdu))
            })
            .collect();

        for (sender, pdu) in pdus {
            for (i, controller) in self.controllers.iter_mut().enumerate() {
                if i != sender {
                    controller.on_advertising_pdu(&pdu);
                }
            }
        }

        let periodic_pdus: Vec<(usize, PeriodicAdvertisingPdu)> = self
            .controllers
            .iter()
            .enumerate()
            .flat_map(|(index, controller)| {
                controller
                    .periodic_advertising_pdus()
                    .into_iter()
                    .map(move |pdu| (index, pdu))
            })
            .collect();
        for (sender, pdu) in periodic_pdus {
            for (index, controller) in self.controllers.iter_mut().enumerate() {
                if index != sender {
                    controller.on_periodic_advertising_pdu(&pdu);
                }
            }
        }
    }

    /// Route queued LL control PDUs between controllers until none remain.
    ///
    /// A single PDU can provoke a reply (e.g. `FeatureReq` → `FeatureRsp`), so
    /// this drains-and-delivers in rounds until the exchange is quiescent. The
    /// round count is bounded to guard against a pathological feedback loop.
    pub fn pump_ll(&mut self) {
        for _ in 0..16 {
            let mut pending: Vec<(Address, Address, ll::ControlPdu)> = Vec::new();
            for c in &mut self.controllers {
                pending.extend(c.take_outbound_ll());
            }
            if pending.is_empty() {
                return;
            }
            for (sender_addr, receiver_addr, pdu) in pending {
                // The receiver is the controller holding a connection whose own
                // address is the PDU's destination and whose peer is the sender.
                if let Some(dst) = self.controllers.iter_mut().find(|c| {
                    c.connections().iter().any(|cx| {
                        cx.self_address == receiver_addr && cx.peer_address == sender_addr
                    })
                }) {
                    dst.on_ll_control_pdu(&sender_addr, pdu);
                }
            }
        }
    }

    /// Route Periodic Advertising Sync Transfer (PAST) messages over existing
    /// LE ACL connections.
    pub fn pump_periodic_sync_transfers(&mut self) {
        let mut pending = Vec::new();
        for controller in &mut self.controllers {
            pending.extend(controller.take_outbound_periodic_sync_transfers());
        }
        for (sender, receiver, transfer) in pending {
            if let Some(destination) = self.controllers.iter_mut().find(|controller| {
                controller.connections().iter().any(|connection| {
                    connection.self_address == receiver && connection.peer_address == sender
                })
            }) {
                destination.on_periodic_sync_transfer(&sender, transfer);
            }
        }
    }

    /// Route queued classic (LMP) PDUs between controllers until none remain.
    /// Classic connections are addressed by public device address, so a PDU is
    /// delivered to the controller whose public address is the destination.
    pub fn pump_classic(&mut self) {
        for _ in 0..16 {
            let mut pending: Vec<(Address, Address, lmp::ClassicPdu)> = Vec::new();
            for c in &mut self.controllers {
                pending.extend(c.take_outbound_classic());
            }
            if pending.is_empty() {
                return;
            }
            for (sender, receiver, pdu) in pending {
                if let Some(dst) = self
                    .controllers
                    .iter_mut()
                    .find(|c| *c.public_address() == receiver)
                {
                    dst.on_classic_pdu(&sender, pdu);
                }
            }
        }
    }

    /// Complete pending connections: for each initiating central, find a
    /// connectable advertiser at the target address and connect the two,
    /// emitting a Connection Complete to each host.
    pub fn establish_connections(&mut self) {
        // Match (central, peripheral) pairs first, to avoid aliasing during mutation.
        let mut pairs: Vec<(usize, usize, Address, u8, Address, Address, u8)> = Vec::new();
        for (ci, central) in self.controllers.iter().enumerate() {
            let Some((central_addr, central_addr_type)) = central.initiating_self_address() else {
                continue;
            };
            let target = central.initiating.as_ref().unwrap().peer_address.clone();
            if let Some((pi, actual_peer, reported_peer, reported_type)) = self
                .controllers
                .iter()
                .enumerate()
                .find_map(|(pi, peripheral)| {
                    peripheral.advertising_pdus().into_iter().find_map(|pdu| {
                        let actual = pdu.address;
                        if actual == target {
                            return Some((
                                pi,
                                actual.clone(),
                                actual.clone(),
                                actual.address_type().0,
                            ));
                        }
                        central
                            .resolve_peer_identity(&target, &actual)
                            .map(|identity| {
                                (pi, actual, identity.clone(), identity.address_type().0)
                            })
                    })
                })
            {
                if pi != ci {
                    pairs.push((
                        ci,
                        pi,
                        central_addr,
                        central_addr_type,
                        actual_peer,
                        reported_peer,
                        reported_type,
                    ));
                }
            }
        }

        for (ci, pi, central_addr, central_addr_type, actual_peer, reported_peer, reported_type) in
            pairs
        {
            // Peripheral accepts, seeing the central's address.
            self.controllers[pi].connect_as_peripheral_at(
                actual_peer.clone(),
                central_addr,
                central_addr_type,
            );
            // Central completes its pending connection.
            self.controllers[ci].connect_as_central_to(actual_peer, reported_peer, reported_type);
        }
    }

    /// Route ACL data sent by controller `from` on `connection_handle` to the
    /// peer controller, delivering it to that peer's host on its own handle for
    /// the connection. Returns `true` if a peer received the data.
    ///
    /// The controller treats the payload as opaque bytes (typically an L2CAP
    /// PDU); it does not parse it.
    pub fn send_acl_data(&mut self, from: usize, connection_handle: u16, data: &[u8]) -> bool {
        self.send_acl_packet(
            from,
            AclDataPacket {
                connection_handle,
                pb_flag: 0,
                bc_flag: 0,
                data_total_length: data.len() as u16,
                data: data.to_vec(),
            },
        )
    }

    /// Route one HCI ACL fragment while preserving its packet-boundary and
    /// broadcast flags.
    pub fn send_acl_packet(&mut self, from: usize, packet: AclDataPacket) -> bool {
        let connection_handle = packet.connection_handle;
        // Resolve the sender's connection endpoints.
        let Some(conn) = self.controllers[from].connection_by_handle_any(connection_handle) else {
            return false;
        };
        let source_address = conn.self_address.clone();
        let peer_address = conn.peer_address.clone();

        // Find the destination controller and its handle for the mirror connection.
        let destination = self.controllers.iter().enumerate().find_map(|(i, ctrl)| {
            if i == from {
                return None;
            }
            ctrl.connections
                .iter()
                .chain(ctrl.classic_connections.iter())
                .find(|c| c.self_address == peer_address && c.peer_address == source_address)
                .map(|c| (i, c.handle))
        });

        if let Some((i, handle)) = destination {
            self.controllers[i].deliver_acl_packet(AclDataPacket {
                connection_handle: handle,
                ..packet
            });
            self.controllers[from].complete_acl_packets(connection_handle, 1);
            true
        } else {
            false
        }
    }

    /// Route one HCI SCO/eSCO payload to the peer synchronous connection.
    pub fn send_synchronous_data(
        &mut self,
        from: usize,
        connection_handle: u16,
        packet_status: u8,
        data: &[u8],
    ) -> bool {
        if data.len() > u8::MAX as usize {
            return false;
        }
        let Some(connection) = self.controllers[from]
            .synchronous_connections()
            .iter()
            .find(|connection| connection.handle == connection_handle)
        else {
            return false;
        };
        let source_address = connection.self_address.clone();
        let peer_address = connection.peer_address.clone();
        let destination = self
            .controllers
            .iter()
            .enumerate()
            .find_map(|(index, controller)| {
                if index == from {
                    return None;
                }
                controller
                    .synchronous_connections()
                    .iter()
                    .find(|connection| {
                        connection.handle != 0
                            && connection.self_address == peer_address
                            && connection.peer_address == source_address
                    })
                    .map(|connection| (index, connection.handle))
            });
        if let Some((index, handle)) = destination {
            self.controllers[index].deliver_synchronous(handle, packet_status, data);
            true
        } else {
            false
        }
    }

    /// Route one HCI ISO fragment over an established CIS. The sender must
    /// have installed the Host-to-Controller data path (direction 0) and the
    /// peer must have installed Controller-to-Host (direction 1). Packet
    /// boundary/timestamp/SDU metadata are preserved while the connection
    /// handle is translated to the peer's CIS handle.
    pub fn send_iso_packet(&mut self, from: usize, packet: IsoDataPacket) -> bool {
        let source_handle = packet.connection_handle;
        let Some(source) = self.controllers[from].cis_link_by_handle(source_handle) else {
            return false;
        };
        if source.data_paths & 0x01 == 0 {
            return false;
        }
        let (acl_self, acl_peer, cig_id, cis_id) = (
            source.acl_self.clone(),
            source.acl_peer.clone(),
            source.cig_id,
            source.cis_id,
        );
        let destination = self
            .controllers
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != from)
            .find_map(|(index, controller)| {
                controller
                    .central_cis_links
                    .iter()
                    .chain(controller.peripheral_cis_links.iter())
                    .find(|link| {
                        link.cig_id == cig_id
                            && link.cis_id == cis_id
                            && link.acl_self == acl_peer
                            && link.acl_peer == acl_self
                            && link.data_paths & 0x02 != 0
                    })
                    .map(|link| (index, link.handle))
            });
        let Some((destination_index, destination_handle)) = destination else {
            return false;
        };
        self.controllers[destination_index].deliver_iso(destination_handle, packet);
        self.controllers[from].complete_acl_packets(source_handle, 1);
        true
    }

    /// Disconnect the connection `connection_handle` on controller `from`,
    /// notifying both sides with a Disconnection Complete. Returns `true` if the
    /// connection existed.
    pub fn disconnect(&mut self, from: usize, connection_handle: u16, reason: u8) -> bool {
        let Some((self_address, peer_address, kind)) =
            self.controllers[from].request_disconnect(connection_handle, reason)
        else {
            return false;
        };

        // Notify the peer (its endpoints are the mirror of ours).
        let peer = self.controllers.iter().enumerate().position(|(i, ctrl)| {
            i != from && ctrl.has_connection(&peer_address, &self_address, kind)
        });
        if let Some(i) = peer {
            self.controllers[i].on_peer_disconnect(&peer_address, &self_address, kind, reason);
        }
        true
    }
}
