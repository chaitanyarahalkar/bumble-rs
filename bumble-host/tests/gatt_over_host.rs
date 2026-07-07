//! Slice-10 acceptance: the same characteristic write/read as slice 9, but now
//! the ATT↔L2CAP↔ACL sequencing lives in the [`Device`] library type, not the
//! test. The test only does connection setup and high-level ATT operations;
//! [`pump`] drives the exchange.

use bumble::{Address, AddressType, Uuid};
use bumble_att::AttPdu;
use bumble_controller::{Controller, LocalLink};
use bumble_gatt::{
    AttServer, Characteristic, GattServer, Service, GATT_CHARACTERISTIC_UUID,
    GATT_PRIMARY_SERVICE_UUID,
};
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

    // The read-back above already proves the server stored the written value.
    assert!(devices[1].has_server());
}

/// Only respond to the caller's single request and return the one inbox PDU.
fn request(link: &mut LocalLink, devices: &mut [Device], client: usize, pdu: AttPdu) -> AttPdu {
    assert!(devices[client].send_att(link, &pdu));
    pump(link, devices);
    let mut inbox = devices[client].take_inbox();
    assert_eq!(inbox.len(), 1, "expected exactly one response");
    inbox.pop().unwrap()
}

#[test]
fn gatt_discovery_and_read_end_to_end() {
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new("C", addr("00:00:00:00:00:01")));
    let peripheral_id = link.add_controller(Controller::new("P", addr("00:00:00:00:00:02")));

    // Peripheral hosts a Device Information service with a Device Name char.
    let server = GattServer::new(vec![Service {
        uuid: Uuid::from_16_bits(0x180A),
        characteristics: vec![Characteristic {
            uuid: Uuid::from_16_bits(0x2A00),
            properties: 0x02,
            value: b"bumble-rs".to_vec(),
        }],
    }]);

    let mut devices = [
        Device::new(central_id),
        Device::with_server(peripheral_id, server),
    ];
    connect(&mut link, central_id, peripheral_id);
    pump(&mut link, &mut devices);

    // 1. Discover primary services (Read By Group Type, 0x2800).
    let services = request(
        &mut link,
        &mut devices,
        0,
        AttPdu::ReadByGroupTypeRequest {
            starting_handle: 0x0001,
            ending_handle: 0xFFFF,
            attribute_group_type: Uuid::from_16_bits(GATT_PRIMARY_SERVICE_UUID),
        },
    );
    let (svc_start, svc_end) = match services {
        AttPdu::ReadByGroupTypeResponse {
            attribute_data_list,
            ..
        } => (
            u16::from_le_bytes([attribute_data_list[0], attribute_data_list[1]]),
            u16::from_le_bytes([attribute_data_list[2], attribute_data_list[3]]),
        ),
        other => panic!("expected group type response, got {other:?}"),
    };
    assert_eq!(svc_start, 0x0001);

    // 2. Discover characteristics within the service (Read By Type, 0x2803).
    let chars = request(
        &mut link,
        &mut devices,
        0,
        AttPdu::ReadByTypeRequest {
            starting_handle: svc_start,
            ending_handle: svc_end,
            attribute_type: Uuid::from_16_bits(GATT_CHARACTERISTIC_UUID),
        },
    );
    let value_handle = match chars {
        AttPdu::ReadByTypeResponse {
            attribute_data_list,
            ..
        } => {
            // entry = [decl_handle(2), properties(1), value_handle(2), uuid...]
            u16::from_le_bytes([attribute_data_list[3], attribute_data_list[4]])
        }
        other => panic!("expected type response, got {other:?}"),
    };

    // 3. Read the characteristic value by its discovered handle.
    let value = request(
        &mut link,
        &mut devices,
        0,
        AttPdu::ReadRequest {
            attribute_handle: value_handle,
        },
    );
    assert_eq!(
        value,
        AttPdu::ReadResponse {
            attribute_value: b"bumble-rs".to_vec()
        }
    );
}

#[test]
fn server_notification_reaches_client() {
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new("C", addr("00:00:00:00:00:01")));
    let peripheral_id = link.add_controller(Controller::new("P", addr("00:00:00:00:00:02")));

    let mut devices = [
        Device::new(central_id),
        Device::with_server(peripheral_id, AttServer::new()),
    ];
    connect(&mut link, central_id, peripheral_id);
    pump(&mut link, &mut devices);

    // Peripheral (index 1) notifies; central (index 0) receives it.
    assert!(devices[1].notify(&mut link, 0x0025, vec![0xDE, 0xAD]));
    pump(&mut link, &mut devices);

    assert_eq!(
        devices[0].take_inbox(),
        vec![AttPdu::HandleValueNotification {
            attribute_handle: 0x0025,
            attribute_value: vec![0xDE, 0xAD],
        }]
    );
}

#[test]
fn disconnection_clears_both_sides() {
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new("C", addr("00:00:00:00:00:01")));
    let peripheral_id = link.add_controller(Controller::new("P", addr("00:00:00:00:00:02")));

    let mut devices = [
        Device::new(central_id),
        Device::with_server(peripheral_id, AttServer::new()),
    ];
    connect(&mut link, central_id, peripheral_id);
    pump(&mut link, &mut devices);
    assert!(devices[0].is_connected() && devices[1].is_connected());

    // Central disconnects (reason 0x13 = remote user terminated connection).
    assert!(devices[0].disconnect(&mut link, 0x13));
    pump(&mut link, &mut devices);

    // Both sides observe the disconnection.
    assert!(!devices[0].is_connected());
    assert!(!devices[1].is_connected());

    // Sending on a closed connection now fails.
    assert!(!devices[0].send_att(
        &mut link,
        &AttPdu::ReadRequest {
            attribute_handle: 1
        }
    ));
}

/// The full LE lifecycle in one scenario: connect → discover → write → read →
/// notify → disconnect, entirely through the library's `Device` API.
#[test]
fn full_le_lifecycle() {
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new("C", addr("00:00:00:00:00:01")));
    let peripheral_id = link.add_controller(Controller::new("P", addr("00:00:00:00:00:02")));

    let server = GattServer::new(vec![Service {
        uuid: Uuid::from_16_bits(0x180F), // Battery Service
        characteristics: vec![Characteristic {
            uuid: Uuid::from_16_bits(0x2A19), // Battery Level
            properties: 0x0A,                 // READ | WRITE
            value: vec![100],
        }],
    }]);
    let mut devices = [
        Device::new(central_id),
        Device::with_server(peripheral_id, server),
    ];

    // Connect.
    connect(&mut link, central_id, peripheral_id);
    pump(&mut link, &mut devices);
    assert!(devices[0].is_connected() && devices[1].is_connected());

    // Discover the service and its characteristic value handle.
    let svc = request(
        &mut link,
        &mut devices,
        0,
        AttPdu::ReadByGroupTypeRequest {
            starting_handle: 0x0001,
            ending_handle: 0xFFFF,
            attribute_group_type: Uuid::from_16_bits(GATT_PRIMARY_SERVICE_UUID),
        },
    );
    let (svc_start, svc_end) = match svc {
        AttPdu::ReadByGroupTypeResponse {
            attribute_data_list,
            ..
        } => (
            u16::from_le_bytes([attribute_data_list[0], attribute_data_list[1]]),
            u16::from_le_bytes([attribute_data_list[2], attribute_data_list[3]]),
        ),
        other => panic!("expected group response, got {other:?}"),
    };
    let chars = request(
        &mut link,
        &mut devices,
        0,
        AttPdu::ReadByTypeRequest {
            starting_handle: svc_start,
            ending_handle: svc_end,
            attribute_type: Uuid::from_16_bits(GATT_CHARACTERISTIC_UUID),
        },
    );
    let value_handle = match chars {
        AttPdu::ReadByTypeResponse {
            attribute_data_list,
            ..
        } => u16::from_le_bytes([attribute_data_list[3], attribute_data_list[4]]),
        other => panic!("expected type response, got {other:?}"),
    };

    // Write, then read back.
    assert_eq!(
        request(
            &mut link,
            &mut devices,
            0,
            AttPdu::WriteRequest {
                attribute_handle: value_handle,
                attribute_value: vec![42],
            },
        ),
        AttPdu::WriteResponse
    );
    assert_eq!(
        request(
            &mut link,
            &mut devices,
            0,
            AttPdu::ReadRequest {
                attribute_handle: value_handle,
            },
        ),
        AttPdu::ReadResponse {
            attribute_value: vec![42]
        }
    );

    // Server notifies the client.
    assert!(devices[1].notify(&mut link, value_handle, vec![7]));
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[0].take_inbox(),
        vec![AttPdu::HandleValueNotification {
            attribute_handle: value_handle,
            attribute_value: vec![7],
        }]
    );

    // Disconnect.
    assert!(devices[0].disconnect(&mut link, 0x13));
    pump(&mut link, &mut devices);
    assert!(!devices[0].is_connected() && !devices[1].is_connected());
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
