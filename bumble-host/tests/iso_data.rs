use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_hci::{HCI_LE_ACCEPT_CIS_REQUEST_COMMAND, HCI_LE_CREATE_CIS_COMMAND};
use bumble_host::{pump, CigParameters, CisControlEvent, CisParameters, Device};

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

#[test]
fn batch_cis_accept_and_reject_preserve_results_and_link_metadata() {
    let (mut link, mut devices) = connected_devices();
    let acl_handle = devices[0].connection_handle().unwrap();
    let parameters = CigParameters::new(
        7,
        vec![CisParameters::new(8), CisParameters::new(9)],
        7_500,
        10_000,
    );
    assert!(devices[0].configure_cig_with_parameters(&mut link, &parameters));
    pump(&mut link, &mut devices);
    let configured = devices[0].take_configured_cis_handles();
    assert_eq!(configured.len(), 2);
    assert_eq!(
        devices[0].take_cis_control_events(),
        vec![CisControlEvent::CigConfigured {
            status: 0,
            cig_id: 7,
            connection_handles: configured.clone(),
        }]
    );

    assert!(devices[0].create_cis_pairs(
        &mut link,
        &[(configured[0], acl_handle), (configured[1], acl_handle)],
    ));
    pump(&mut link, &mut devices);
    assert_eq!(
        devices[0].take_cis_control_events(),
        vec![CisControlEvent::CommandStatus {
            command_opcode: HCI_LE_CREATE_CIS_COMMAND,
            status: 0,
        }]
    );
    let requests = devices[1].take_cis_requests();
    assert_eq!(requests.len(), 2);

    devices[1].reject_cis(&mut link, requests[0].cis_connection_handle, 0x0D);
    devices[1].accept_cis(&mut link, requests[1].cis_connection_handle);
    pump(&mut link, &mut devices);

    let central_results = devices[0].take_cis_control_events();
    assert_eq!(central_results.len(), 2);
    assert!(matches!(
        central_results[0],
        CisControlEvent::Established {
            status: 0x0D,
            link
        } if link.connection_handle == configured[0]
    ));
    assert!(matches!(
        central_results[1],
        CisControlEvent::Established {
            status: 0,
            link
        } if link.connection_handle == configured[1]
    ));
    assert!(devices[0].cis_link(configured[0]).is_none());
    assert_eq!(devices[0].cis_link(configured[1]).unwrap().phy_c_to_p, 1);

    let peripheral_results = devices[1].take_cis_control_events();
    assert_eq!(peripheral_results.len(), 3);
    assert!(matches!(
        peripheral_results[0],
        CisControlEvent::CommandStatus {
            command_opcode: bumble_hci::HCI_LE_REJECT_CIS_REQUEST_COMMAND,
            status: 0,
        }
    ));
    assert!(matches!(
        peripheral_results[1],
        CisControlEvent::CommandStatus {
            command_opcode: HCI_LE_ACCEPT_CIS_REQUEST_COMMAND,
            status: 0,
        }
    ));
    assert!(matches!(
        peripheral_results[2],
        CisControlEvent::Established {
            status: 0,
            link
        } if link.connection_handle == requests[1].cis_connection_handle
    ));
}
