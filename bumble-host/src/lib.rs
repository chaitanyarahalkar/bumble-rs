//! bumble-host — the host-side glue of the [`google/bumble`](https://github.com/google/bumble)
//! port.
//!
//! **Slice 10** of the incremental port: a [`Device`] that owns the sequencing
//! the earlier integration tests wired by hand — wrapping ATT PDUs in L2CAP and
//! ACL to send, and unwrapping received ACL back up to ATT. This turns the
//! cross-layer composition into a real library capability.
//!
//! A `Device` sits above a [`bumble_controller::Controller`] (addressed by id
//! on a shared [`bumble_controller::LocalLink`]). It:
//! - learns its connection handle from the LE Connection Complete event,
//! - sends ATT PDUs on the ATT channel with [`Device::send_att`],
//! - on [`Device::poll`], processes inbound ACL: an optional server-role
//!   [`bumble_gatt::AttServer`] answers requests automatically; other ATT PDUs (responses /
//!   notifications) are queued for the client to collect.
//!
//! [`pump`] drives a set of devices to quiescence, which is all the
//! (synchronous) event loop this port needs.
//!
//! ## Scope
//!
//! ATT traffic over the fixed ATT CID plus raw fixed/dynamic L2CAP channels,
//! with controller-buffer-sized ACL fragmentation/reassembly. Deferred: direct
//! integration of the LE signaling manager and multiple simultaneous
//! connections per device.

use std::collections::BTreeMap;

use bumble::Address;
use bumble_att::AttPdu;
use bumble_controller::LocalLink;
use bumble_gatt::AttRequestHandler;
use bumble_hci::{
    fragment_l2cap_pdu, AclDataPacket, AclDataPacketAssembler, Command, Event, HciPacket,
    LeMetaEvent, SynchronousDataPacket,
};
use bumble_l2cap::L2capPdu;

/// The fixed L2CAP channel id for the Attribute Protocol.
pub const ATT_CID: u16 = 0x0004;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SynchronousConnectionInfo {
    pub connection_handle: u16,
    pub peer_address: Address,
    pub link_type: u8,
    pub air_mode: u8,
}

/// A host attached to a controller on a [`LocalLink`]. Owns the
/// ATT↔L2CAP↔ACL sequencing.
pub struct Device {
    controller_id: usize,
    server: Option<Box<dyn AttRequestHandler>>,
    connection_handle: Option<u16>,
    classic_connection_handle: Option<u16>,
    synchronous_connections: Vec<SynchronousConnectionInfo>,
    synchronous_requests: Vec<(Address, u8)>,
    synchronous_inbox: Vec<SynchronousDataPacket>,
    inbox: Vec<AttPdu>,
    /// Received payloads on non-ATT L2CAP channels, as `(cid, payload)`.
    l2cap_inbox: Vec<(u16, Vec<u8>)>,
    acl_data_packet_length: usize,
    acl_assemblers: BTreeMap<u16, AclDataPacketAssembler>,
}

impl Device {
    /// A client-only device (no attribute server).
    pub fn new(controller_id: usize) -> Device {
        Device {
            controller_id,
            server: None,
            connection_handle: None,
            classic_connection_handle: None,
            synchronous_connections: Vec::new(),
            synchronous_requests: Vec::new(),
            synchronous_inbox: Vec::new(),
            inbox: Vec::new(),
            l2cap_inbox: Vec::new(),
            acl_data_packet_length: 27,
            acl_assemblers: BTreeMap::new(),
        }
    }

    /// A device that also answers ATT requests using the given handler
    /// (an [`bumble_gatt::AttServer`] or a full [`bumble_gatt::GattServer`]).
    pub fn with_server(controller_id: usize, server: impl AttRequestHandler + 'static) -> Device {
        Device {
            controller_id,
            server: Some(Box::new(server)),
            connection_handle: None,
            classic_connection_handle: None,
            synchronous_connections: Vec::new(),
            synchronous_requests: Vec::new(),
            synchronous_inbox: Vec::new(),
            inbox: Vec::new(),
            l2cap_inbox: Vec::new(),
            acl_data_packet_length: 27,
            acl_assemblers: BTreeMap::new(),
        }
    }

    pub fn controller_id(&self) -> usize {
        self.controller_id
    }

    /// The connection handle, once connected (and `None` after disconnection).
    pub fn connection_handle(&self) -> Option<u16> {
        self.connection_handle
    }

    /// `true` while a connection is established.
    pub fn is_connected(&self) -> bool {
        self.connection_handle.is_some()
    }

    pub fn classic_connection_handle(&self) -> Option<u16> {
        self.classic_connection_handle
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

    /// Submit any typed HCI command through this device's attached controller.
    pub fn send_hci_command(&mut self, link: &mut LocalLink, command: Command) {
        link.handle_command(self.controller_id, command);
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
                allow_role_switch: 0,
            },
        );
    }

    pub fn accept_classic(&mut self, link: &mut LocalLink, peer_address: Address) {
        self.send_hci_command(
            link,
            Command::AcceptConnectionRequest {
                bd_addr: peer_address,
                role: 0,
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
        link.disconnect(self.controller_id, connection_handle, reason)
    }

    /// Disconnect the current connection with the given reason. Both this device
    /// and the peer receive a Disconnection Complete (processed on the next
    /// [`pump`]).
    pub fn disconnect(&mut self, link: &mut LocalLink, reason: u8) -> bool {
        let Some(handle) = self.connection_handle else {
            return false;
        };
        link.disconnect(self.controller_id, handle, reason)
    }

    /// `true` if this device has an attribute server (server role).
    pub fn has_server(&self) -> bool {
        self.server.is_some()
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

    /// Remove and return the ATT PDUs received so far that were not handled by
    /// the server (i.e. responses and notifications destined for a client).
    pub fn take_inbox(&mut self) -> Vec<AttPdu> {
        std::mem::take(&mut self.inbox)
    }

    /// Send a raw payload on an L2CAP channel to the peer. Requires an
    /// established connection.
    pub fn send_l2cap(&mut self, link: &mut LocalLink, cid: u16, payload: &[u8]) -> bool {
        let Some(handle) = self.connection_handle else {
            return false;
        };
        self.send_l2cap_on_handle(link, handle, cid, payload)
    }

    fn send_l2cap_on_handle(
        &mut self,
        link: &mut LocalLink,
        handle: u16,
        cid: u16,
        payload: &[u8],
    ) -> bool {
        let frame = L2capPdu::new(cid, payload.to_vec()).to_bytes(false);
        let Ok(fragments) =
            fragment_l2cap_pdu(handle, 0, self.acl_data_packet_length, &frame, false)
        else {
            return false;
        };
        fragments
            .into_iter()
            .all(|packet| link.send_acl_packet(self.controller_id, packet))
    }

    /// Send an ATT PDU to the peer on the ATT channel.
    pub fn send_att(&mut self, link: &mut LocalLink, pdu: &AttPdu) -> bool {
        self.send_l2cap(link, ATT_CID, &pdu.to_bytes())
    }

    /// Remove and return payloads received on the given (non-ATT) L2CAP channel,
    /// e.g. SMP on CID `0x0006`.
    pub fn take_l2cap(&mut self, cid: u16) -> Vec<Vec<u8>> {
        let (matching, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.l2cap_inbox)
            .into_iter()
            .partition(|(c, _)| *c == cid);
        self.l2cap_inbox = rest;
        matching.into_iter().map(|(_, payload)| payload).collect()
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
                    connection_handle,
                    ..
                })) => {
                    self.connection_handle = Some(connection_handle);
                }
                HciPacket::Event(Event::DisconnectionComplete {
                    connection_handle, ..
                }) => {
                    self.acl_assemblers.remove(&connection_handle);
                    if self.connection_handle == Some(connection_handle) {
                        self.connection_handle = None;
                    }
                    if self.classic_connection_handle == Some(connection_handle) {
                        self.classic_connection_handle = None;
                    }
                    self.synchronous_connections
                        .retain(|connection| connection.connection_handle != connection_handle);
                }
                HciPacket::Event(Event::ConnectionComplete {
                    status: 0,
                    connection_handle,
                    link_type: 1,
                    ..
                }) => self.classic_connection_handle = Some(connection_handle),
                HciPacket::Event(Event::ConnectionRequest {
                    bd_addr, link_type, ..
                }) if link_type != 1 => self.synchronous_requests.push((bd_addr, link_type)),
                HciPacket::Event(Event::SynchronousConnectionComplete {
                    status: 0,
                    connection_handle,
                    bd_addr,
                    link_type,
                    air_mode,
                    ..
                }) => self
                    .synchronous_connections
                    .push(SynchronousConnectionInfo {
                        connection_handle,
                        peer_address: bd_addr,
                        link_type,
                        air_mode,
                    }),
                HciPacket::AclData(acl) => self.on_acl(link, acl),
                HciPacket::SyncData(packet) => self.synchronous_inbox.push(packet),
                _ => {}
            }
        }
        true
    }

    fn on_acl(&mut self, link: &mut LocalLink, acl: AclDataPacket) {
        let handle = acl.connection_handle;
        let Ok(Some(data)) = self.acl_assemblers.entry(handle).or_default().feed(&acl) else {
            return;
        };
        let Ok(l2cap) = L2capPdu::from_bytes(&data) else {
            return;
        };
        // Non-ATT channels (e.g. SMP on 0x0006) are queued raw for the caller.
        if l2cap.cid != ATT_CID {
            self.l2cap_inbox.push((l2cap.cid, l2cap.payload));
            return;
        }
        let Ok(pdu) = AttPdu::from_bytes(&l2cap.payload) else {
            return;
        };

        // A server answers requests automatically; everything else is for the
        // client (this device's user) to collect.
        if is_request(&pdu) {
            let response = self
                .server
                .as_mut()
                .map(|server| server.handle_request(&pdu).to_bytes());
            if let Some(response) = response {
                self.send_l2cap_on_handle(link, handle, ATT_CID, &response);
                return;
            }
        }
        self.inbox.push(pdu);
    }
}

/// `true` if the ATT PDU is a request that expects a response.
fn is_request(pdu: &AttPdu) -> bool {
    matches!(
        pdu,
        AttPdu::ExchangeMtuRequest { .. }
            | AttPdu::ReadRequest { .. }
            | AttPdu::ReadByTypeRequest { .. }
            | AttPdu::ReadByGroupTypeRequest { .. }
            | AttPdu::WriteRequest { .. }
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
