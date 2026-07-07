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
//! ATT traffic over the fixed ATT CID only (including GATT discovery requests,
//! which are just ATT requests answered by a server-role handler). Deferred:
//! L2CAP fragmentation and reassembly across multiple ACL packets (each ATT PDU
//! is assumed to fit one ACL packet), the LE signaling channel, and multiple
//! simultaneous connections per device.

use bumble_att::AttPdu;
use bumble_controller::LocalLink;
use bumble_gatt::AttRequestHandler;
use bumble_hci::{Event, HciPacket, LeMetaEvent};
use bumble_l2cap::L2capPdu;

/// The fixed L2CAP channel id for the Attribute Protocol.
pub const ATT_CID: u16 = 0x0004;

/// A host attached to a controller on a [`LocalLink`]. Owns the
/// ATT↔L2CAP↔ACL sequencing.
pub struct Device {
    controller_id: usize,
    server: Option<Box<dyn AttRequestHandler>>,
    connection_handle: Option<u16>,
    inbox: Vec<AttPdu>,
    /// Received payloads on non-ATT L2CAP channels, as `(cid, payload)`.
    l2cap_inbox: Vec<(u16, Vec<u8>)>,
}

impl Device {
    /// A client-only device (no attribute server).
    pub fn new(controller_id: usize) -> Device {
        Device {
            controller_id,
            server: None,
            connection_handle: None,
            inbox: Vec::new(),
            l2cap_inbox: Vec::new(),
        }
    }

    /// A device that also answers ATT requests using the given handler
    /// (an [`bumble_gatt::AttServer`] or a full [`bumble_gatt::GattServer`]).
    pub fn with_server(controller_id: usize, server: impl AttRequestHandler + 'static) -> Device {
        Device {
            controller_id,
            server: Some(Box::new(server)),
            connection_handle: None,
            inbox: Vec::new(),
            l2cap_inbox: Vec::new(),
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
        let frame = L2capPdu::new(cid, payload.to_vec()).to_bytes(false);
        link.send_acl_data(self.controller_id, handle, &frame)
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
                HciPacket::Event(Event::DisconnectionComplete { .. }) => {
                    self.connection_handle = None;
                }
                HciPacket::AclData(acl) => self.on_acl(link, acl.connection_handle, &acl.data),
                _ => {}
            }
        }
        true
    }

    fn on_acl(&mut self, link: &mut LocalLink, handle: u16, data: &[u8]) {
        let Ok(l2cap) = L2capPdu::from_bytes(data) else {
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
            if let Some(server) = &mut self.server {
                let response = server.handle_request(&pdu);
                let frame = L2capPdu::new(ATT_CID, response.to_bytes()).to_bytes(false);
                link.send_acl_data(self.controller_id, handle, &frame);
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
