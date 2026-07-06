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
//! `LE_Set_Advertising_Enable`, `LE_Set_Scan_Enable`, and the resulting
//! Command Complete acknowledgements and LE Advertising Report events.
//!
//! Deferred to later slices: LE connections, ACL data, LL control PDUs,
//! extended advertising sets, CIS/ISO, encryption, and classic/LMP — the bulk
//! of Bumble's `controller.py`.

use bumble::{Address, AddressType};
use bumble_hci::codes::*;
use bumble_hci::{AdvertisingReport, Command, Event, HciPacket, LeMetaEvent, ReturnParameters};

/// Legacy connectable-and-scannable undirected advertising event type.
const ADV_IND: u8 = 0x00;
/// Address type used for random device addresses.
const ADDRESS_TYPE_RANDOM: u8 = 1;
/// A fixed RSSI reported for received advertisements (dBm).
const DEFAULT_RSSI: i8 = -40;
/// HCI "Unknown HCI Command" error, returned for commands this slice ignores.
const UNKNOWN_HCI_COMMAND_ERROR: u8 = 0x01;

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
                self.ack(op_code, HCI_SUCCESS);
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
}
