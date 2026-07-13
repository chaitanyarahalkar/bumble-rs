use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_host::{
    pump, BigParameters, BigSyncParameters, Device, ExtendedAdvertisingConfig,
    PeriodicAdvertisingConfig,
};

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

#[test]
fn encrypted_big_fans_bis_sdus_out_to_synchronized_receivers() {
    let broadcast_address = address("C4:F2:17:1A:1D:D0");
    let mut link = LocalLink::new();
    let source_id = link.add_controller(Controller::new("source", address("00:00:00:00:00:01")));
    let first_id = link.add_controller(Controller::new("first", address("00:00:00:00:00:02")));
    let second_id = link.add_controller(Controller::new("second", address("00:00:00:00:00:03")));
    let mut devices = [
        Device::new(source_id),
        Device::new(first_id),
        Device::new(second_id),
    ];

    let mut extended =
        ExtendedAdvertisingConfig::connectable_scannable(4, broadcast_address.clone());
    extended.event_properties = 0;
    extended.sid = 7;
    assert!(devices[0].start_extended_advertising(&mut link, &extended, b"auracast", &[]));
    assert!(devices[0].start_periodic_advertising(
        &mut link,
        PeriodicAdvertisingConfig::new(4),
        b"basic audio announcement",
    ));

    let broadcast_code = *b"broadcast-code!!";
    let mut big_parameters = BigParameters::new(1, 4, 2);
    big_parameters.max_sdu = 155;
    big_parameters.broadcast_code = Some(broadcast_code);
    assert!(devices[0].create_big(&mut link, big_parameters));
    pump(&mut link, &mut devices);
    let source_bis = devices[0].big_bis_handles(1).unwrap().to_vec();
    assert_eq!(source_bis.len(), 2);

    for receiver in &mut devices[1..] {
        assert!(receiver.create_periodic_advertising_sync(
            &mut link,
            broadcast_address.clone(),
            7,
            0,
            0x0100,
            false,
        ));
    }
    link.propagate_advertising();
    pump(&mut link, &mut devices);
    let first_sync = *devices[1].periodic_syncs().keys().next().unwrap();
    let second_sync = *devices[2].periodic_syncs().keys().next().unwrap();
    for receiver in &mut devices[1..] {
        let reports = receiver.take_biginfo_reports();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].num_bis, 2);
        assert_eq!(reports[0].max_sdu, 155);
        assert!(reports[0].encrypted);
    }

    let mut first_parameters = BigSyncParameters::new(2, first_sync, vec![1]);
    first_parameters.broadcast_code = Some(broadcast_code);
    assert!(devices[1].create_big_sync(&mut link, first_parameters));

    let mut wrong_parameters = BigSyncParameters::new(3, second_sync, vec![1]);
    wrong_parameters.broadcast_code = Some(*b"wrong-code!!!!!!");
    assert!(devices[2].create_big_sync(&mut link, wrong_parameters));
    link.propagate_advertising();
    pump(&mut link, &mut devices);
    assert_eq!(devices[2].take_big_errors(), vec![(3, 0x3D)]);

    let mut second_parameters = BigSyncParameters::new(3, second_sync, vec![1]);
    second_parameters.broadcast_code = Some(broadcast_code);
    assert!(devices[2].create_big_sync(&mut link, second_parameters));
    link.propagate_advertising();
    pump(&mut link, &mut devices);

    let first_bis = devices[1].big_sync_bis_handles(2).unwrap()[0];
    let second_bis = devices[2].big_sync_bis_handles(3).unwrap()[0];
    assert!(devices[0].setup_iso_data_path(&mut link, source_bis[0], 0));
    assert!(devices[1].setup_iso_data_path(&mut link, first_bis, 1));
    assert!(devices[2].setup_iso_data_path(&mut link, second_bis, 1));
    pump(&mut link, &mut devices);

    let sdu: Vec<_> = (0..2_500).map(|value| value as u8).collect();
    assert!(devices[0].send_iso_sdu(&mut link, source_bis[0], &sdu));
    pump(&mut link, &mut devices);
    let first_received = devices[1].take_iso_sdus(first_bis);
    assert_eq!(first_received.len(), 1);
    assert_eq!(first_received[0].packet_sequence_number, 0);
    assert_eq!(first_received[0].data, sdu);
    let second_received = devices[2].take_iso_sdus(second_bis);
    assert_eq!(second_received.len(), 1);
    assert_eq!(second_received[0].packet_sequence_number, 0);
    assert_eq!(second_received[0].data, sdu);

    assert!(devices[1].terminate_big_sync(&mut link, 2));
    pump(&mut link, &mut devices);
    assert!(devices[1].big_sync_bis_handles(2).is_none());
    assert!(devices[0].send_iso_sdu(&mut link, source_bis[0], b"second"));
    pump(&mut link, &mut devices);
    assert!(devices[1].take_iso_sdus(first_bis).is_empty());
    assert_eq!(devices[2].take_iso_sdus(second_bis)[0].data, b"second");

    assert!(devices[0].terminate_big(&mut link, 1, 0x16));
    pump(&mut link, &mut devices);
    assert!(devices[0].big_bis_handles(1).is_none());
    assert!(devices[2].big_sync_bis_handles(3).is_none());
    assert_eq!(devices[2].take_terminated_bigs(), vec![(3, 0x16)]);
    assert!(!devices[0].send_iso_sdu(&mut link, source_bis[0], b"stopped"));
}
