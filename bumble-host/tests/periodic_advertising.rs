use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_host::{pump, Device, ExtendedAdvertisingConfig, PeriodicAdvertisingConfig};

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

fn devices() -> (LocalLink, [Device; 2], Address) {
    let advertiser_address = address("C4:F2:17:1A:1D:BB");
    let mut link = LocalLink::new();
    let scanner = link.add_controller(Controller::new("scanner", address("00:00:00:00:00:01")));
    let advertiser =
        link.add_controller(Controller::new("advertiser", address("00:00:00:00:00:02")));
    (
        link,
        [Device::new(scanner), Device::new(advertiser)],
        advertiser_address,
    )
}

#[test]
fn periodic_advertising_sync_reassembles_reports_and_controls_reception() {
    let (mut link, mut devices, advertiser_address) = devices();
    let mut extended =
        ExtendedAdvertisingConfig::connectable_scannable(4, advertiser_address.clone());
    extended.event_properties = 0;
    extended.sid = 9;
    extended.secondary_phy = 2;
    assert!(devices[1].start_extended_advertising(&mut link, &extended, b"primary", &[]));

    let mut periodic = PeriodicAdvertisingConfig::new(4);
    periodic.interval_min = 0x00C0;
    periodic.interval_max = 0x00D0;
    periodic.include_adi = true;
    let periodic_data: Vec<_> = (0..600).map(|value| value as u8).collect();
    assert!(devices[1].start_periodic_advertising(&mut link, periodic, &periodic_data));
    assert!(!devices[1].start_periodic_advertising(&mut link, periodic, &[0; 0x0673]));
    assert!(link.controller(1).periodic_advertising_enabled(4));

    assert!(devices[0].create_periodic_advertising_sync(
        &mut link,
        advertiser_address.clone(),
        9,
        0,
        0x0100,
        true,
    ));
    pump(&mut link, &mut devices);
    assert!(devices[0].periodic_syncs().is_empty());

    link.propagate_advertising();
    pump(&mut link, &mut devices);
    let sync = devices[0].periodic_syncs().values().next().unwrap().clone();
    assert_eq!(sync.advertiser_address, advertiser_address);
    assert_eq!(sync.advertising_sid, 9);
    assert_eq!(sync.advertiser_phy, 2);
    assert_eq!(sync.interval, 0x00C0);
    assert_eq!(
        link.controller(0).periodic_sync_handles(),
        [sync.sync_handle]
    );

    let advertisements = devices[0].take_periodic_advertisements();
    assert_eq!(advertisements.len(), 1);
    assert_eq!(advertisements[0].sync_handle, sync.sync_handle);
    assert_eq!(advertisements[0].data, periodic_data);
    assert!(!advertisements[0].truncated);

    devices[0].set_periodic_advertising_receive_enabled(&mut link, sync.sync_handle, false);
    pump(&mut link, &mut devices);
    link.propagate_advertising();
    pump(&mut link, &mut devices);
    assert!(devices[0].take_periodic_advertisements().is_empty());

    devices[0].set_periodic_advertising_receive_enabled(&mut link, sync.sync_handle, true);
    pump(&mut link, &mut devices);
    link.propagate_advertising();
    pump(&mut link, &mut devices);
    assert_eq!(devices[0].take_periodic_advertisements().len(), 1);

    devices[1].stop_periodic_advertising(&mut link, 4);
    pump(&mut link, &mut devices);
    assert!(!link.controller(1).periodic_advertising_enabled(4));
    link.propagate_advertising();
    pump(&mut link, &mut devices);
    assert!(devices[0].take_periodic_advertisements().is_empty());

    devices[0].terminate_periodic_advertising_sync(&mut link, sync.sync_handle);
    pump(&mut link, &mut devices);
    assert!(devices[0].periodic_syncs().is_empty());
    assert!(link.controller(0).periodic_sync_handles().is_empty());
}

#[test]
fn pending_periodic_sync_can_be_cancelled() {
    let (mut link, mut devices, advertiser_address) = devices();
    assert!(devices[0].create_periodic_advertising_sync(
        &mut link,
        advertiser_address,
        3,
        0,
        0x0100,
        false,
    ));
    devices[0].cancel_periodic_advertising_sync(&mut link);
    pump(&mut link, &mut devices);
    assert_eq!(devices[0].take_periodic_sync_errors(), [0x44]);
    assert!(devices[0].periodic_syncs().is_empty());
}

#[test]
fn periodic_advertising_set_info_transfers_over_an_acl_connection() {
    let (mut link, mut devices, advertiser_address) = devices();
    let mut extended =
        ExtendedAdvertisingConfig::connectable_scannable(7, advertiser_address.clone());
    extended.sid = 5;
    assert!(devices[1].start_extended_advertising(&mut link, &extended, b"connect", &[]));
    assert!(devices[1].start_periodic_advertising(
        &mut link,
        PeriodicAdvertisingConfig::new(7),
        b"transferred set",
    ));

    devices[0].connect_le_extended(&mut link, advertiser_address.clone());
    pump(&mut link, &mut devices);
    assert!(devices[0].is_connected());
    assert!(devices[1].is_connected());
    assert!(devices[1].transfer_periodic_advertising_set_info(&mut link, 7, 0xCAFE));
    pump(&mut link, &mut devices);

    let transfers = devices[0].take_periodic_sync_transfers();
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers[0].service_data, 0xCAFE);
    assert_eq!(transfers[0].sync.advertising_sid, 5);
    assert_eq!(transfers[0].sync.advertiser_address, advertiser_address);
    assert!(devices[0]
        .periodic_syncs()
        .contains_key(&transfers[0].sync.sync_handle));

    link.propagate_advertising();
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[0].take_periodic_advertisements()[0].data,
        b"transferred set"
    );
}

#[test]
fn established_periodic_sync_transfers_to_a_connected_peer() {
    let advertiser_address = address("C4:F2:17:1A:1D:C3");
    let receiver_address = address("C4:F2:17:1A:1D:B2");
    let mut link = LocalLink::new();
    let sender_id = link.add_controller(Controller::new("sender", address("00:00:00:00:00:11")));
    let receiver_id =
        link.add_controller(Controller::new("receiver", address("00:00:00:00:00:12")));
    let advertiser_id =
        link.add_controller(Controller::new("advertiser", address("00:00:00:00:00:13")));
    let mut devices = [
        Device::new(sender_id),
        Device::new(receiver_id),
        Device::new(advertiser_id),
    ];
    devices[1].set_random_address(&mut link, receiver_address.clone());
    devices[2].set_random_address(&mut link, advertiser_address.clone());

    let mut extended =
        ExtendedAdvertisingConfig::connectable_scannable(2, advertiser_address.clone());
    extended.event_properties = 0;
    extended.sid = 6;
    assert!(devices[2].start_extended_advertising(&mut link, &extended, b"train", &[]));
    assert!(devices[2].start_periodic_advertising(
        &mut link,
        PeriodicAdvertisingConfig::new(2),
        b"sync transfer",
    ));
    assert!(devices[0].create_periodic_advertising_sync(
        &mut link,
        advertiser_address.clone(),
        6,
        0,
        0x0100,
        false,
    ));
    link.propagate_advertising();
    pump(&mut link, &mut devices);
    let source_sync_handle = *devices[0].periodic_syncs().keys().next().unwrap();

    assert!(devices[1].start_advertising(&mut link, b"receiver"));
    devices[0].connect_le(&mut link, receiver_address);
    pump(&mut link, &mut devices);
    assert!(devices[0].is_connected());
    assert!(devices[1].is_connected());
    assert!(devices[0].transfer_periodic_advertising_sync(&mut link, source_sync_handle, 0x1234,));
    pump(&mut link, &mut devices);

    let transfers = devices[1].take_periodic_sync_transfers();
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers[0].service_data, 0x1234);
    assert_eq!(transfers[0].sync.advertiser_address, advertiser_address);
    assert!(link
        .controller(receiver_id)
        .periodic_sync_handles()
        .contains(&transfers[0].sync.sync_handle));
    link.propagate_advertising();
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[1].take_periodic_advertisements()[0].data,
        b"sync transfer"
    );
}
