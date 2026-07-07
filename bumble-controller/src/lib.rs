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
//! Implemented: the LE advertising/scanning commands and `LE_Create_Connection`
//! (slice 7), ACL data routing between connected controllers (slice 8, via
//! [`LocalLink::send_acl_data`]), and disconnection (slice 13, via
//! [`LocalLink::disconnect`], emitting Disconnection Complete on both sides).
//! Also handled locally: the read commands (`Read_BD_ADDR`, `Read_Local_Name`,
//! `LE_Read_Buffer_Size`, `LE_Read_Local_Supported_Features`, `LE_Rand`) and the
//! per-connection `LE_Set_Data_Length` / `LE_Set_PHY` requests, which report
//! back through `LE_Data_Length_Change` / `LE_PHY_Update_Complete`.
//!
//! ## Full command surface
//!
//! Every command upstream's `controller.py` handles gets a well-formed reply of
//! the matching HCI shape, driven by the generated [`command_surface`] table:
//! configuration/"set" commands are acknowledged with Command Complete + SUCCESS
//! (upstream stores state and returns SUCCESS; the in-process sim has no state to
//! store, so it simply acknowledges), read commands the sim can't model are
//! acknowledged SUCCESS without a synthesized payload, and operations that
//! complete via a later event (connect, encryption start, remote-features…) are
//! answered with Command Status. A command upstream *also* doesn't handle gets
//! the spec-correct "Unknown HCI Command" — an honest report, not a fake success.
//!
//! ## Deferred (behavioral simulation, not the codec)
//!
//! The full LL/state-machine *behavior* behind many of those acknowledgements is
//! not simulated: LL control PDUs, extended/periodic advertising sets, CIS/ISO,
//! encryption (`LE_Enable_Encryption` / LTK exchange), remote-version exchange,
//! and classic/LMP. The HCI *codec* for all of them (in `bumble-hci`) is complete
//! and oracle-pinned; what remains is controller-side behavior, which — unlike
//! the codec — has no ground-truth oracle to pin against (upstream's controller
//! is itself a simulation with placeholder values).

pub mod command_surface;

use bumble::{Address, AddressType};
use bumble_hci::codes::*;
use bumble_hci::{
    AclDataPacket, AdvertisingReport, Command, Event, HciPacket, LeMetaEvent, ReturnParameters,
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
/// The LE features bitmap reported by `LE_Read_Local_Supported_Features`. All
/// zero: this controller implements no optional LE features (an honest report,
/// since encryption, extended advertising, etc. are deferred).
const LOCAL_LE_FEATURES: [u8; 8] = [0; 8];
/// PHY value for LE 1M, reported when no specific PHY was requested.
const LE_1M_PHY: u8 = 1;

/// An established LE connection on a controller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    pub handle: u16,
    pub role: u8,
    /// The address this controller uses for the connection.
    pub self_address: Address,
    pub peer_address: Address,
}

/// A pending outgoing connection recorded by `LE_Create_Connection`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingConnection {
    peer_address: Address,
    peer_address_type: u8,
    own_address_type: u8,
}

/// An advertising PDU as it travels over the [`LocalLink`]. Since the link is
/// in-process, this is a plain struct rather than a serialized LL PDU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdvertisingPdu {
    pub event_type: u8,
    pub address_type: u8,
    pub address: Address,
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
    connections: Vec<Connection>,
    initiating: Option<PendingConnection>,
    next_handle: u16,
    /// Monotonic counter backing `LE_Rand` — the software controller has no
    /// entropy source, so it returns a deterministic, ever-changing value.
    rand_counter: u64,
    host_queue: Vec<HciPacket>,
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
            connections: Vec::new(),
            initiating: None,
            next_handle: 1,
            rand_counter: 0,
            host_queue: Vec::new(),
        }
    }

    pub fn public_address(&self) -> &Address {
        &self.public_address
    }

    pub fn random_address(&self) -> &Address {
        &self.random_address
    }

    pub fn is_advertising(&self) -> bool {
        self.advertising_enabled
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
                self.advertising_data.clear();
                self.connections.clear();
                self.initiating = None;
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
                self.ack(op_code, HCI_SUCCESS);
            }
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
        })
    }

    /// Handle an advertising PDU received over the link. If scanning is
    /// enabled, queue an LE Advertising Report event to the host.
    pub fn on_advertising_pdu(&mut self, pdu: &AdvertisingPdu) {
        if !self.scanning_enabled {
            return;
        }
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

    /// The connections currently established on this controller.
    pub fn connections(&self) -> &[Connection] {
        &self.connections
    }

    /// `true` if an `LE_Create_Connection` is pending (initiating).
    pub fn is_initiating(&self) -> bool {
        self.initiating.is_some()
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

    fn push_connection_complete(&mut self, connection: &Connection, peer_address_type: u8) {
        self.host_queue.push(HciPacket::Event(Event::LeMeta(
            LeMetaEvent::ConnectionComplete {
                status: HCI_SUCCESS,
                connection_handle: connection.handle,
                role: connection.role,
                peer_address_type,
                peer_address: connection.peer_address.clone(),
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
            self_address,
            peer_address: pending.peer_address,
        };
        self.push_connection_complete(&connection, pending.peer_address_type);
        self.connections.push(connection);
    }

    /// Accept an incoming connection as the peripheral. Emits a Connection
    /// Complete (role = peripheral) and stops advertising.
    pub fn connect_as_peripheral(&mut self, central_address: Address, central_address_type: u8) {
        let handle = self.allocate_handle();
        let connection = Connection {
            handle,
            role: ROLE_PERIPHERAL,
            self_address: self.random_address.clone(),
            peer_address: central_address,
        };
        self.push_connection_complete(&connection, central_address_type);
        self.connections.push(connection);
        self.advertising_enabled = false;
    }

    /// Handle a host-initiated `HCI_Disconnect`: acknowledge with a Command
    /// Status, emit a Disconnection Complete, and drop the connection. Returns
    /// the `(self_address, peer_address)` of the dropped connection so the link
    /// can notify the peer, or `None` if no such connection existed.
    pub fn request_disconnect(&mut self, handle: u16, reason: u8) -> Option<(Address, Address)> {
        let index = self.connections.iter().position(|c| c.handle == handle)?;
        let connection = self.connections.remove(index);
        self.host_queue.push(HciPacket::Event(Event::CommandStatus {
            status: HCI_SUCCESS,
            num_hci_command_packets: 1,
            command_opcode: HCI_DISCONNECT_COMMAND,
        }));
        self.host_queue
            .push(HciPacket::Event(Event::DisconnectionComplete {
                status: HCI_SUCCESS,
                connection_handle: handle,
                reason,
            }));
        Some((connection.self_address, connection.peer_address))
    }

    /// Notify this controller that the peer dropped the connection identified by
    /// (this controller's) `self_address`/`peer_address`. Emits a Disconnection
    /// Complete and drops the connection.
    pub fn on_peer_disconnect(
        &mut self,
        self_address: &Address,
        peer_address: &Address,
        reason: u8,
    ) {
        if let Some(index) = self
            .connections
            .iter()
            .position(|c| c.self_address == *self_address && c.peer_address == *peer_address)
        {
            let connection = self.connections.remove(index);
            self.host_queue
                .push(HciPacket::Event(Event::DisconnectionComplete {
                    status: HCI_SUCCESS,
                    connection_handle: connection.handle,
                    reason,
                }));
        }
    }

    /// Deliver received ACL data to the host as an HCI ACL Data packet on the
    /// given connection handle.
    fn deliver_acl(&mut self, connection_handle: u16, data: &[u8]) {
        self.host_queue.push(HciPacket::AclData(AclDataPacket {
            connection_handle,
            pb_flag: 0,
            bc_flag: 0,
            data_total_length: data.len() as u16,
            data: data.to_vec(),
        }));
    }

    fn connection_by_handle(&self, handle: u16) -> Option<&Connection> {
        self.connections.iter().find(|c| c.handle == handle)
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
            .filter_map(|(i, c)| c.advertising_pdu().map(|pdu| (i, pdu)))
            .collect();

        for (sender, pdu) in pdus {
            for (i, controller) in self.controllers.iter_mut().enumerate() {
                if i != sender {
                    controller.on_advertising_pdu(&pdu);
                }
            }
        }
    }

    /// Complete pending connections: for each initiating central, find a
    /// connectable advertiser at the target address and connect the two,
    /// emitting a Connection Complete to each host.
    pub fn establish_connections(&mut self) {
        // Match (central, peripheral) pairs first, to avoid aliasing during mutation.
        let mut pairs: Vec<(usize, usize, Address, u8)> = Vec::new();
        for (ci, central) in self.controllers.iter().enumerate() {
            let Some((central_addr, central_addr_type)) = central.initiating_self_address() else {
                continue;
            };
            let target = central.initiating.as_ref().unwrap().peer_address.clone();
            if let Some(pi) = self
                .controllers
                .iter()
                .position(|p| p.is_advertising() && *p.random_address() == target)
            {
                if pi != ci {
                    pairs.push((ci, pi, central_addr, central_addr_type));
                }
            }
        }

        for (ci, pi, central_addr, central_addr_type) in pairs {
            // Peripheral accepts, seeing the central's address.
            self.controllers[pi].connect_as_peripheral(central_addr, central_addr_type);
            // Central completes its pending connection.
            self.controllers[ci].connect_as_central();
        }
    }

    /// Route ACL data sent by controller `from` on `connection_handle` to the
    /// peer controller, delivering it to that peer's host on its own handle for
    /// the connection. Returns `true` if a peer received the data.
    ///
    /// The controller treats the payload as opaque bytes (typically an L2CAP
    /// PDU); it does not parse it.
    pub fn send_acl_data(&mut self, from: usize, connection_handle: u16, data: &[u8]) -> bool {
        // Resolve the sender's connection endpoints.
        let Some(conn) = self.controllers[from].connection_by_handle(connection_handle) else {
            return false;
        };
        let source_address = conn.self_address.clone();
        let peer_address = conn.peer_address.clone();

        // Find the destination controller and its handle for the mirror connection.
        let destination = self.controllers.iter().enumerate().find_map(|(i, ctrl)| {
            if i == from {
                return None;
            }
            ctrl.connections()
                .iter()
                .find(|c| c.self_address == peer_address && c.peer_address == source_address)
                .map(|c| (i, c.handle))
        });

        if let Some((i, handle)) = destination {
            self.controllers[i].deliver_acl(handle, data);
            true
        } else {
            false
        }
    }

    /// Disconnect the connection `connection_handle` on controller `from`,
    /// notifying both sides with a Disconnection Complete. Returns `true` if the
    /// connection existed.
    pub fn disconnect(&mut self, from: usize, connection_handle: u16, reason: u8) -> bool {
        let Some((self_address, peer_address)) =
            self.controllers[from].request_disconnect(connection_handle, reason)
        else {
            return false;
        };

        // Notify the peer (its endpoints are the mirror of ours).
        let peer = self.controllers.iter().enumerate().position(|(i, ctrl)| {
            i != from
                && ctrl
                    .connections()
                    .iter()
                    .any(|c| c.self_address == peer_address && c.peer_address == self_address)
        });
        if let Some(i) = peer {
            self.controllers[i].on_peer_disconnect(&peer_address, &self_address, reason);
        }
        true
    }
}
