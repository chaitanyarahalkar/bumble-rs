//! Slice-10 acceptance: the same characteristic write/read as slice 9, but now
//! the ATT↔L2CAP↔ACL sequencing lives in the [`Device`] library type, not the
//! test. The test only does connection setup and high-level ATT operations;
//! [`pump`] drives the exchange.

use bumble::{Address, AddressType};
use bumble_att::AttPdu;
use bumble_controller::{Controller, LocalLink};
use bumble_gatt::AttServer;
use bumble_hci::Command;
use bumble_host::{pump, Device};

fn addr(s: &str) -> Address {
    Address::parse(s, AddressType::RANDOM_DEVICE).unwrap()
}

/// Run the LE connection handshake between two controllers (by id) on the link.
fn connect(link: &mut LocalLink, central: usize, peripheral: usize) {
    link.handle_command(
        peripheral,
        Command::LeSetRandomAddress {
            random_address: addr("C4:F2:17:1A:1D:BB"),
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
            random_address: addr("C4:F2:17:1A:1D:AA"),
        },
    );
    link.handle_command(
        central,
        Command::LeCreateConnection {
            le_scan_interval: 16,
            le_scan_window: 16,
            initiator_filter_policy: 0,
            peer_address_type: 1,
            peer_address: addr("C4:F2:17:1A:1D:BB"),
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
}

#[test]
fn characteristic_write_read_via_device_api() {
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new("C", addr("00:00:00:00:00:01")));
    let peripheral_id = link.add_controller(Controller::new("P", addr("00:00:00:00:00:02")));

    // Peripheral hosts an attribute at handle 0x0025.
    let mut server = AttServer::new();
    server.set_attribute(0x0025, vec![0xAA]);

    let mut devices = [
        Device::new(central_id),
        Device::with_server(peripheral_id, server),
    ];

    // Establish the connection, then let the devices learn their handles.
    connect(&mut link, central_id, peripheral_id);
    pump(&mut link, &mut devices);
    assert!(devices[0].connection_handle().is_some());
    assert!(devices[1].connection_handle().is_some());

    // Central writes a new value — one high-level call, glue handled by Device.
    assert!(devices[0].send_att(
        &mut link,
        &AttPdu::WriteRequest {
            attribute_handle: 0x0025,
            attribute_value: vec![0xBB, 0xCC],
        },
    ));
    pump(&mut link, &mut devices);
    assert_eq!(devices[0].take_inbox(), vec![AttPdu::WriteResponse]);

    // Central reads it back.
    assert!(devices[0].send_att(
        &mut link,
        &AttPdu::ReadRequest {
            attribute_handle: 0x0025,
        },
    ));
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[0].take_inbox(),
        vec![AttPdu::ReadResponse {
            attribute_value: vec![0xBB, 0xCC],
        }]
    );

    // The server actually stored the written value.
    assert_eq!(
        devices[1].server().unwrap().attribute(0x0025),
        Some(&[0xBB, 0xCC][..])
    );
}

#[test]
fn reading_missing_attribute_returns_error() {
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new("C", addr("00:00:00:00:00:01")));
    let peripheral_id = link.add_controller(Controller::new("P", addr("00:00:00:00:00:02")));

    let mut devices = [
        Device::new(central_id),
        Device::with_server(peripheral_id, AttServer::new()),
    ];
    connect(&mut link, central_id, peripheral_id);
    pump(&mut link, &mut devices);

    devices[0].send_att(
        &mut link,
        &AttPdu::ReadRequest {
            attribute_handle: 0x0099,
        },
    );
    pump(&mut link, &mut devices);

    let inbox = devices[0].take_inbox();
    assert_eq!(inbox.len(), 1);
    assert!(matches!(inbox[0], AttPdu::ErrorResponse { .. }));
}
