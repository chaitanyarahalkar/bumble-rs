//! Slice-9 capstone: a real characteristic write-then-read between two virtual
//! devices, driven over the whole stack. The central issues ATT requests that
//! travel ATT → L2CAP → ACL → link → peer host; the peripheral feeds them to an
//! [`AttServer`] and sends the ATT responses back the same way.

use bumble::{Address, AddressType};
use bumble_att::AttPdu;
use bumble_controller::{Controller, LocalLink};
use bumble_gatt::AttServer;
use bumble_hci::{Command, Event, HciPacket, LeMetaEvent};
use bumble_l2cap::L2capPdu;

const ATT_CID: u16 = 0x0004;

fn addr(s: &str) -> Address {
    Address::parse(s, AddressType::RANDOM_DEVICE).unwrap()
}

fn connection_handle(events: &[HciPacket]) -> u16 {
    events
        .iter()
        .find_map(|e| match e {
            HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionComplete {
                connection_handle,
                ..
            })) => Some(*connection_handle),
            _ => None,
        })
        .expect("expected a Connection Complete")
}

fn setup_connection(
    link: &mut LocalLink,
    central: usize,
    peripheral: usize,
    central_addr: &Address,
    peripheral_addr: &Address,
) -> (u16, u16) {
    link.handle_command(
        peripheral,
        Command::LeSetRandomAddress {
            random_address: peripheral_addr.clone(),
        },
    );
    link.handle_command(
        peripheral,
        Command::LeSetAdvertisingEnable {
            advertising_enable: 1,
        },
    );
    link.handle_command(
        central,
        Command::LeSetRandomAddress {
            random_address: central_addr.clone(),
        },
    );
    link.handle_command(
        central,
        Command::LeCreateConnection {
            le_scan_interval: 16,
            le_scan_window: 16,
            initiator_filter_policy: 0,
            peer_address_type: 1,
            peer_address: peripheral_addr.clone(),
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
    let ch = connection_handle(&link.drain_host_events(central));
    let ph = connection_handle(&link.drain_host_events(peripheral));
    (ch, ph)
}

/// Extract the ATT PDU from the first ACL packet in a host's drained events.
fn received_att(events: &[HciPacket]) -> AttPdu {
    let acl = events
        .iter()
        .find_map(|e| match e {
            HciPacket::AclData(p) => Some(p),
            _ => None,
        })
        .expect("expected an ACL packet");
    let l2cap = L2capPdu::from_bytes(&acl.data).unwrap();
    assert_eq!(l2cap.cid, ATT_CID);
    AttPdu::from_bytes(&l2cap.payload).unwrap()
}

fn send_att(link: &mut LocalLink, from: usize, handle: u16, pdu: &AttPdu) {
    let frame = L2capPdu::new(ATT_CID, pdu.to_bytes()).to_bytes(false);
    assert!(link.send_acl_data(from, handle, &frame));
}

/// A full ATT request/response round-trip: `client` sends `request`; the
/// peripheral's `server` answers; the response is returned to the client.
fn att_exchange(
    link: &mut LocalLink,
    client: usize,
    client_handle: u16,
    server_id: usize,
    server_handle: u16,
    server: &mut AttServer,
    request: AttPdu,
) -> AttPdu {
    send_att(link, client, client_handle, &request);
    let request_rx = received_att(&link.drain_host_events(server_id));
    let response = server.on_request(&request_rx);
    send_att(link, server_id, server_handle, &response);
    received_att(&link.drain_host_events(client))
}

#[test]
fn characteristic_write_then_read_end_to_end() {
    let mut link = LocalLink::new();
    let central = link.add_controller(Controller::new("C", addr("00:00:00:00:00:01")));
    let peripheral = link.add_controller(Controller::new("P", addr("00:00:00:00:00:02")));
    let (ch, ph) = setup_connection(
        &mut link,
        central,
        peripheral,
        &addr("C4:F2:17:1A:1D:AA"),
        &addr("C4:F2:17:1A:1D:BB"),
    );

    // The peripheral hosts a characteristic value at handle 0x0025.
    let mut server = AttServer::new();
    server.set_attribute(0x0025, vec![0xAA]);

    // MTU exchange.
    let mtu = att_exchange(
        &mut link,
        central,
        ch,
        peripheral,
        ph,
        &mut server,
        AttPdu::ExchangeMtuRequest { client_rx_mtu: 517 },
    );
    assert!(matches!(mtu, AttPdu::ExchangeMtuResponse { .. }));

    // Central writes a new value.
    let write_resp = att_exchange(
        &mut link,
        central,
        ch,
        peripheral,
        ph,
        &mut server,
        AttPdu::WriteRequest {
            attribute_handle: 0x0025,
            attribute_value: vec![0xBB, 0xCC],
        },
    );
    assert_eq!(write_resp, AttPdu::WriteResponse);

    // Central reads it back — and gets exactly what it wrote.
    let read_resp = att_exchange(
        &mut link,
        central,
        ch,
        peripheral,
        ph,
        &mut server,
        AttPdu::ReadRequest {
            attribute_handle: 0x0025,
        },
    );
    assert_eq!(
        read_resp,
        AttPdu::ReadResponse {
            attribute_value: vec![0xBB, 0xCC]
        }
    );

    // Reading a missing handle yields an ATT Error Response.
    let missing = att_exchange(
        &mut link,
        central,
        ch,
        peripheral,
        ph,
        &mut server,
        AttPdu::ReadRequest {
            attribute_handle: 0x0099,
        },
    );
    assert!(matches!(missing, AttPdu::ErrorResponse { .. }));
}
