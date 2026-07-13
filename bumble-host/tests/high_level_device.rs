use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_host::{pump, Device, ExtendedAdvertisingConfig};

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
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
