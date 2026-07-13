use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_hci::Command;
use bumble_host::{pump, Device};

fn addr(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

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
fn device_fragments_and_reassembles_large_l2cap_payloads() {
    let mut link = LocalLink::new();
    let central = link.add_controller(Controller::new("C", addr("00:00:00:00:00:01")));
    let peripheral = link.add_controller(Controller::new("P", addr("00:00:00:00:00:02")));
    let mut devices = [Device::new(central), Device::new(peripheral)];
    assert!(devices[0].set_acl_data_packet_length(8));
    assert!(devices[1].set_acl_data_packet_length(8));
    assert!(devices[0].set_acl_max_in_flight(2));
    assert!(devices[1].set_acl_max_in_flight(2));
    assert!(!devices[0].set_acl_data_packet_length(0));
    connect(&mut link, central, peripheral);
    pump(&mut link, &mut devices);

    let payload: Vec<u8> = (0..=255).cycle().take(257).collect();
    assert!(devices[0].send_l2cap(&mut link, 0x0040, &payload));
    pump(&mut link, &mut devices);
    assert_eq!(devices[1].take_l2cap(0x0040), vec![payload.clone()]);
    assert_eq!(devices[0].acl_packets_pending(), 0);

    assert!(devices[1].send_l2cap(&mut link, 0x0041, &payload));
    pump(&mut link, &mut devices);
    assert_eq!(devices[0].take_l2cap(0x0041), vec![payload]);
    assert_eq!(devices[1].acl_packets_pending(), 0);
}
