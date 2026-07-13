use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_host::{pump, Device};

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

fn connected_devices() -> (LocalLink, [Device; 2]) {
    let central_address = address("C4:F2:17:1A:1D:AA");
    let peripheral_address = address("C4:F2:17:1A:1D:BB");
    let mut link = LocalLink::new();
    let central_id = link.add_controller(Controller::new("central", address("00:00:00:00:00:01")));
    let peripheral_id =
        link.add_controller(Controller::new("peripheral", address("00:00:00:00:00:02")));
    let mut devices = [Device::new(central_id), Device::new(peripheral_id)];
    devices[0].set_random_address(&mut link, central_address);
    devices[1].set_random_address(&mut link, peripheral_address.clone());
    assert!(devices[1].start_advertising(&mut link, &[2, 1, 6]));
    devices[0].connect_le(&mut link, peripheral_address);
    pump(&mut link, &mut devices);
    assert!(devices.iter().all(Device::is_connected));
    (link, devices)
}

fn establish_cis(link: &mut LocalLink, devices: &mut [Device; 2]) -> (u16, u16) {
    assert!(devices[0].configure_cig(link, 1, &[2]));
    pump(link, devices);
    let configured = devices[0].take_configured_cis_handles();
    assert_eq!(configured.len(), 1);
    let central_cis = configured[0];
    assert!(devices[0].create_cis(link, central_cis));
    pump(link, devices);
    let requests = devices[1].take_cis_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].cig_id, 1);
    assert_eq!(requests[0].cis_id, 2);
    let peripheral_cis = requests[0].cis_connection_handle;
    devices[1].accept_cis(link, peripheral_cis);
    pump(link, devices);
    assert_eq!(
        devices[0].established_cis_handles().collect::<Vec<_>>(),
        vec![central_cis]
    );
    assert_eq!(
        devices[1].established_cis_handles().collect::<Vec<_>>(),
        vec![peripheral_cis]
    );
    (central_cis, peripheral_cis)
}

#[test]
fn high_level_cis_fragments_and_reassembles_iso_sdus() {
    let (mut link, mut devices) = connected_devices();
    let (central_cis, peripheral_cis) = establish_cis(&mut link, &mut devices);
    assert!(devices[0].setup_iso_data_path(&mut link, central_cis, 0));
    assert!(devices[1].setup_iso_data_path(&mut link, peripheral_cis, 1));
    pump(&mut link, &mut devices);

    let first: Vec<_> = (0..2500).map(|value| value as u8).collect();
    assert!(devices[0].send_iso_sdu(&mut link, central_cis, &first));
    pump(&mut link, &mut devices);
    let received = devices[1].take_iso_sdus(peripheral_cis);
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].connection_handle, peripheral_cis);
    assert_eq!(received[0].packet_sequence_number, 0);
    assert_eq!(received[0].packet_status_flag, 0);
    assert_eq!(received[0].data, first);

    assert!(devices[0].send_iso_sdu(&mut link, central_cis, &[9, 8, 7]));
    pump(&mut link, &mut devices);
    let received = devices[1].take_iso_sdus(peripheral_cis);
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].packet_sequence_number, 1);
    assert_eq!(received[0].data, vec![9, 8, 7]);

    assert!(devices[1].remove_iso_data_path(&mut link, peripheral_cis, 0x02));
    pump(&mut link, &mut devices);
    assert!(!devices[0].send_iso_sdu(&mut link, central_cis, &[1]));

    assert!(devices[0].disconnect_handle(&mut link, central_cis, 0x13));
    pump(&mut link, &mut devices);
    assert_eq!(devices[0].established_cis_handles().count(), 0);
    assert_eq!(devices[1].established_cis_handles().count(), 0);
}
