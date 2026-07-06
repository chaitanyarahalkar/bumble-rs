//! Slice-8 acceptance: the ACL data path, exercised end-to-end across four
//! crates. Two controllers connect (slices 3+7), then a host sends ACL data
//! carrying an L2CAP PDU that wraps an ATT PDU; the peer host receives it and
//! it is parsed back up the stack (ACL → L2CAP → ATT).
//!
//! The controller/link treat the ACL payload as opaque bytes; L2CAP and ATT
//! are dev-dependencies used only to build and verify the payload here.

use bumble::{Address, AddressType};
use bumble_att::AttPdu;
use bumble_controller::{Controller, LocalLink};
use bumble_hci::{Command, Event, HciPacket, LeMetaEvent};
use bumble_l2cap::L2capPdu;

/// The fixed L2CAP channel id for the Attribute Protocol.
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

/// Connect a central and a peripheral; return their respective connection handles.
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

    let central_handle = connection_handle(&link.drain_host_events(central));
    let peripheral_handle = connection_handle(&link.drain_host_events(peripheral));
    (central_handle, peripheral_handle)
}

#[test]
fn acl_l2cap_att_end_to_end() {
    let mut link = LocalLink::new();
    let central = link.add_controller(Controller::new("C", addr("00:00:00:00:00:01")));
    let peripheral = link.add_controller(Controller::new("P", addr("00:00:00:00:00:02")));
    let central_addr = addr("C4:F2:17:1A:1D:AA");
    let peripheral_addr = addr("C4:F2:17:1A:1D:BB");

    let (central_handle, peripheral_handle) = setup_connection(
        &mut link,
        central,
        peripheral,
        &central_addr,
        &peripheral_addr,
    );

    // Central host: ATT Write Request → L2CAP PDU on the ATT CID → ACL payload.
    let att = AttPdu::WriteRequest {
        attribute_handle: 0x0025,
        attribute_value: vec![0x01, 0x02],
    };
    let l2cap = L2capPdu::new(ATT_CID, att.to_bytes());
    let acl_payload = l2cap.to_bytes(false);

    assert!(link.send_acl_data(central, central_handle, &acl_payload));

    // Peripheral host receives the ACL packet on its own handle.
    let events = link.drain_host_events(peripheral);
    let acl = events
        .iter()
        .find_map(|e| match e {
            HciPacket::AclData(p) => Some(p.clone()),
            _ => None,
        })
        .expect("expected an ACL data packet");
    assert_eq!(acl.connection_handle, peripheral_handle);

    // The ACL packet is itself a valid HCI packet (round-trips through the codec).
    let acl_bytes = HciPacket::AclData(acl.clone()).to_bytes();
    assert_eq!(
        HciPacket::from_bytes(&acl_bytes).unwrap().to_bytes(),
        acl_bytes
    );

    // Parse back up the stack: ACL payload → L2CAP → ATT.
    let l2cap_rx = L2capPdu::from_bytes(&acl.data).unwrap();
    assert_eq!(l2cap_rx.cid, ATT_CID);
    let att_rx = AttPdu::from_bytes(&l2cap_rx.payload).unwrap();
    assert_eq!(att_rx, att);
}

#[test]
fn acl_not_routed_without_connection() {
    let mut link = LocalLink::new();
    let central = link.add_controller(Controller::new("C", addr("00:00:00:00:00:01")));
    let _peripheral = link.add_controller(Controller::new("P", addr("00:00:00:00:00:02")));

    // No connection established → nothing to route on handle 1.
    assert!(!link.send_acl_data(central, 1, &[0x00, 0x00, 0x04, 0x00]));
}
