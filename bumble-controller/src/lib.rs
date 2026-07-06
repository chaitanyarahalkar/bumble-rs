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
//! Implemented: `Reset`, `LE_Set_Random_Address`, `LE_Set_Advertising_Data`,
//! `LE_Set_Advertising_Enable`, `LE_Set_Scan_Enable`, and `LE_Create_Connection`,
//! producing the resulting Command Complete / Command Status acknowledgements,
//! LE Advertising Report events, and — via [`LocalLink::establish_connections`]
//! — LE Connection Complete events on both the central and the peripheral
//! (slice 7).
//!
//! Deferred to later slices: ACL data, LL control PDUs, disconnection, extended
//! advertising sets, CIS/ISO, encryption, and classic/LMP — the bulk of
//! Bumble's `controller.py`.

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
            _ => self.ack(op_code, UNKNOWN_HCI_COMMAND_ERROR),
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
        self.host_queue
            .push(HciPacket::Event(Event::CommandComplete {
                num_hci_command_packets: 1,
                command_opcode,
                return_parameters: ReturnParameters::Status { status },
            }));
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
}
