use bumble::{Address, AddressType};
use bumble_att::AttPdu;
use bumble_controller::{Controller, LocalLink};
use bumble_host::{pump, Device, ExtendedAdvertisingConfig, LeSubrateRequestParameters};

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

fn public_address(value: &str) -> Address {
    Address::parse(value, AddressType::PUBLIC_DEVICE).unwrap()
}

#[test]
fn device_api_advertises_scans_connects_and_disconnects_without_raw_hci() {
    let central_address = address("C4:F2:17:1A:1D:AA");
    let peripheral_address = address("C4:F2:17:1A:1D:BB");
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new("central", address("00:00:00:00:00:01")));
    let peripheral_id =
        link.add_controller(Controller::new("peripheral", address("00:00:00:00:00:02")));
    let mut devices = [Device::new(central_id), Device::new(peripheral_id)];
    devices[0].set_random_address(&mut link, central_address);
    devices[1].set_random_address(&mut link, peripheral_address.clone());
    assert!(devices[1].start_advertising(&mut link, &[2, 0x01, 0x06, 3, 0x09, b'R', b'S']));
    assert!(!devices[1].start_advertising(&mut link, &[0; 32]));
    devices[0].start_scanning(&mut link, true, false);
    pump(&mut link, &mut devices);

    link.propagate_advertising();
    pump(&mut link, &mut devices);
    let reports = devices[0].take_advertising_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].address, peripheral_address);
    assert_eq!(reports[0].data, vec![2, 0x01, 0x06, 3, 0x09, b'R', b'S']);
    devices[0].stop_scanning(&mut link);

    devices[0].connect_le(&mut link, peripheral_address.clone());
    pump(&mut link, &mut devices);
    assert!(devices[0].is_connected());
    assert!(devices[1].is_connected());
    assert_eq!(devices[0].peer_address(), Some(&peripheral_address));
    assert_eq!(devices[0].connection_role(), Some(0));
    assert_eq!(devices[1].connection_role(), Some(1));

    assert!(devices[0].disconnect(&mut link, 0x13));
    pump(&mut link, &mut devices);
    assert!(!devices[0].is_connected());
    assert!(!devices[1].is_connected());
    devices[1].stop_advertising(&mut link);
}

#[test]
fn device_tracks_sniff_mode_and_le_subrate_changes() {
    let central_address = address("C4:F2:17:1A:1D:AA");
    let peripheral_address = address("C4:F2:17:1A:1D:BB");
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new(
        "central",
        public_address("00:00:00:00:00:01/P"),
    ));
    let peripheral_id = link.add_controller(Controller::new(
        "peripheral",
        public_address("00:00:00:00:00:02/P"),
    ));
    let mut devices = [Device::new(central_id), Device::new(peripheral_id)];
    devices[0].set_random_address(&mut link, central_address);
    devices[1].set_random_address(&mut link, peripheral_address.clone());
    assert!(devices[1].start_advertising(&mut link, &[]));
    devices[0].connect_le(&mut link, peripheral_address);
    pump(&mut link, &mut devices);
    let handle = devices[0].connection_handle().unwrap();

    let subrate = LeSubrateRequestParameters {
        subrate_min: 2,
        subrate_max: 2,
        max_latency: 2,
        continuation_number: 1,
        supervision_timeout: 2,
    };
    assert!(devices[0].request_le_subrate_on_handle(&mut link, handle, subrate));
    pump(&mut link, &mut devices);
    let connection = devices[0].le_connection(handle).unwrap();
    assert_eq!(connection.parameters.subrate_factor, 2);
    assert_eq!(connection.parameters.peripheral_latency, 2);
    assert_eq!(connection.parameters.continuation_number, 1);
    assert_eq!(connection.parameters.supervision_timeout, 2);

    assert!(devices[0].enter_sniff_mode_on_handle(&mut link, handle, 2, 2, 2));
    pump(&mut link, &mut devices);
    let connection = devices[0].le_connection(handle).unwrap();
    assert_eq!(connection.classic_mode, 0x02);
    assert_eq!(connection.classic_interval, 2);

    assert!(devices[0].exit_sniff_mode_on_handle(&mut link, handle));
    pump(&mut link, &mut devices);
    let connection = devices[0].le_connection(handle).unwrap();
    assert_eq!(connection.classic_mode, 0x00);
    assert_eq!(connection.classic_interval, 2);
    assert!(!devices[0].request_le_subrate_on_handle(&mut link, 0x0FFF, subrate));
    assert!(!devices[0].enter_sniff_mode_on_handle(&mut link, 0x0FFF, 2, 2, 2));
}

#[test]
fn device_api_fragments_extended_advertising_scans_and_connects() {
    let central_address = address("C4:F2:17:1A:1D:AA");
    let peripheral_address = address("C4:F2:17:1A:1D:BB");
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new("central", address("00:00:00:00:00:01")));
    let peripheral_id =
        link.add_controller(Controller::new("peripheral", address("00:00:00:00:00:02")));
    let mut devices = [Device::new(central_id), Device::new(peripheral_id)];
    devices[0].set_random_address(&mut link, central_address);
    let mut config =
        ExtendedAdvertisingConfig::connectable_scannable(4, peripheral_address.clone());
    config.secondary_phy = 2;
    config.sid = 9;
    let data: Vec<_> = (0..600).map(|value| value as u8).collect();
    let scan_response = vec![9, 8, 7, 6];
    assert!(devices[1].start_extended_advertising(&mut link, &config, &data, &scan_response,));
    assert!(!devices[1].start_extended_advertising(&mut link, &config, &[0; 1651], &[]));
    devices[0].start_extended_scanning(&mut link, true, false);
    pump(&mut link, &mut devices);

    link.propagate_advertising();
    pump(&mut link, &mut devices);
    let reports = devices[0].take_extended_advertising_reports();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].address, peripheral_address);
    assert_eq!(reports[0].advertising_sid, 9);
    assert_eq!(reports[0].secondary_phy, 2);
    assert_eq!(reports[0].data, data);
    assert_eq!(reports[1].event_type, 0x0008);
    assert_eq!(reports[1].data, scan_response);
    devices[0].stop_extended_scanning(&mut link);

    devices[0].connect_le_extended(&mut link, peripheral_address.clone());
    pump(&mut link, &mut devices);
    assert!(devices[0].is_connected());
    assert!(devices[1].is_connected());
    assert_eq!(devices[0].peer_address(), Some(&peripheral_address));
    assert_eq!(devices[0].connection_role(), Some(0));
    assert_eq!(devices[1].connection_role(), Some(1));
    devices[1].stop_extended_advertising(&mut link, 4);
}

#[test]
fn device_owns_and_routes_multiple_le_connections_by_handle() {
    let central_address = address("C4:F2:17:1A:1D:A0");
    let first_address = address("C4:F2:17:1A:1D:B1");
    let second_address = address("C4:F2:17:1A:1D:B2");
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new("central", address("00:00:00:00:00:01")));
    let first_id = link.add_controller(Controller::new("first", address("00:00:00:00:00:02")));
    let second_id = link.add_controller(Controller::new("second", address("00:00:00:00:00:03")));
    let mut devices = [
        Device::new(central_id),
        Device::new(first_id),
        Device::new(second_id),
    ];
    devices[0].set_random_address(&mut link, central_address);
    devices[1].set_random_address(&mut link, first_address.clone());
    devices[2].set_random_address(&mut link, second_address.clone());

    assert!(devices[1].start_advertising(&mut link, &[]));
    devices[0].connect_le(&mut link, first_address.clone());
    pump(&mut link, &mut devices);
    let first_central_handle = devices[0]
        .connection_handle_for_peer(&first_address)
        .expect("first central handle");
    let first_peer_handle = devices[1].connection_handle().expect("first peer handle");

    assert!(devices[2].start_advertising(&mut link, &[]));
    devices[0].connect_le(&mut link, second_address.clone());
    pump(&mut link, &mut devices);
    let second_central_handle = devices[0]
        .connection_handle_for_peer(&second_address)
        .expect("second central handle");
    let second_peer_handle = devices[2].connection_handle().expect("second peer handle");

    assert_ne!(first_central_handle, second_central_handle);
    assert_eq!(devices[0].le_connections().count(), 2);
    assert_eq!(devices[0].connection_handle(), Some(second_central_handle));

    assert!(devices[0].send_l2cap_on_handle(&mut link, first_central_handle, 0x0040, b"first"));
    assert!(devices[0].send_l2cap_on_handle(&mut link, second_central_handle, 0x0040, b"second"));
    assert!(devices[0].send_att_on_handle(
        &mut link,
        first_central_handle,
        &AttPdu::HandleValueNotification {
            attribute_handle: 1,
            attribute_value: b"first-att".to_vec(),
        },
    ));
    assert!(devices[0].send_att_on_handle(
        &mut link,
        second_central_handle,
        &AttPdu::HandleValueNotification {
            attribute_handle: 2,
            attribute_value: b"second-att".to_vec(),
        },
    ));
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[1].take_l2cap_on_handle(first_peer_handle, 0x0040),
        [b"first".to_vec()]
    );
    assert_eq!(
        devices[2].take_l2cap_on_handle(second_peer_handle, 0x0040),
        [b"second".to_vec()]
    );
    assert_eq!(
        devices[1].take_inbox_on_handle(first_peer_handle),
        [AttPdu::HandleValueNotification {
            attribute_handle: 1,
            attribute_value: b"first-att".to_vec(),
        }]
    );
    assert_eq!(
        devices[2].take_inbox_on_handle(second_peer_handle),
        [AttPdu::HandleValueNotification {
            attribute_handle: 2,
            attribute_value: b"second-att".to_vec(),
        }]
    );

    assert!(devices[0].select_connection(first_central_handle));
    assert_eq!(devices[0].peer_address(), Some(&first_address));
    assert!(devices[0].select_connection(second_central_handle));
    assert!(devices[0].disconnect_handle(&mut link, second_central_handle, 0x13));
    pump(&mut link, &mut devices);
    assert_eq!(devices[0].le_connections().count(), 1);
    assert_eq!(devices[0].connection_handle(), Some(first_central_handle));
    assert!(!devices[2].is_connected());
}

#[test]
fn device_owns_and_routes_multiple_classic_connections_by_handle() {
    let central_address = public_address("11:11:11:11:11:11");
    let first_address = public_address("22:22:22:22:22:22");
    let second_address = public_address("33:33:33:33:33:33");
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new("central", central_address.clone()));
    let first_id = link.add_controller(Controller::new("first", first_address.clone()));
    let second_id = link.add_controller(Controller::new("second", second_address.clone()));
    let mut devices = [
        Device::new(central_id),
        Device::new(first_id),
        Device::new(second_id),
    ];

    devices[0].connect_classic(&mut link, first_address.clone());
    devices[0].poll(&mut link);
    link.pump_classic();
    devices[1].poll(&mut link);
    devices[1].accept_classic(&mut link, central_address.clone());
    pump(&mut link, &mut devices);
    let first_central_handle = devices[0]
        .classic_connection_handle_for_peer(&first_address)
        .expect("first central handle");
    let first_peer_handle = devices[1]
        .classic_connection_handle()
        .expect("first peer handle");

    devices[0].connect_classic(&mut link, second_address.clone());
    devices[0].poll(&mut link);
    link.pump_classic();
    devices[2].poll(&mut link);
    devices[2].accept_classic(&mut link, central_address);
    pump(&mut link, &mut devices);
    let second_central_handle = devices[0]
        .classic_connection_handle_for_peer(&second_address)
        .expect("second central handle");
    let second_peer_handle = devices[2]
        .classic_connection_handle()
        .expect("second peer handle");

    assert_ne!(first_central_handle, second_central_handle);
    assert_eq!(devices[0].classic_connections().count(), 2);
    assert_eq!(
        devices[0].classic_connection_handle(),
        Some(second_central_handle)
    );
    assert!(devices[0].send_l2cap_on_handle(
        &mut link,
        first_central_handle,
        0x0007,
        b"first-classic"
    ));
    assert!(devices[0].send_l2cap_on_handle(
        &mut link,
        second_central_handle,
        0x0007,
        b"second-classic"
    ));
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[1].take_l2cap_on_handle(first_peer_handle, 0x0007),
        [b"first-classic".to_vec()]
    );
    assert_eq!(
        devices[2].take_l2cap_on_handle(second_peer_handle, 0x0007),
        [b"second-classic".to_vec()]
    );

    assert!(devices[0].select_classic_connection(first_central_handle));
    assert!(devices[0].select_classic_connection(second_central_handle));
    assert!(devices[0].disconnect_handle(&mut link, second_central_handle, 0x13));
    pump(&mut link, &mut devices);
    assert_eq!(devices[0].classic_connections().count(), 1);
    assert_eq!(
        devices[0].classic_connection_handle(),
        Some(first_central_handle)
    );
    assert!(devices[2].classic_connections().next().is_none());
}
