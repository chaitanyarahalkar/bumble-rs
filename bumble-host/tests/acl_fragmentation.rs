use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_hci::Command;
use bumble_host::{pump, ControllerBufferInfo, Device};

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

#[test]
fn external_controller_packet_pools_remain_distinct_and_zero_le_shares_classic() {
    let classic = ControllerBufferInfo {
        data_packet_length: 1021,
        total_num_data_packets: 8,
    };
    let le = ControllerBufferInfo {
        data_packet_length: 251,
        total_num_data_packets: 12,
    };
    let iso = ControllerBufferInfo {
        data_packet_length: 120,
        total_num_data_packets: 6,
    };
    let mut device = Device::new(0);

    assert!(device.configure_controller_packet_pools(Some(classic), Some(le), Some(iso)));
    assert_eq!(device.classic_acl_buffer(), Some(classic));
    assert_eq!(device.le_acl_buffer(), Some(le));
    assert_eq!(device.iso_buffer(), Some(iso));
    assert_eq!(device.acl_data_packet_length(), 1021);
    assert_eq!(device.acl_max_in_flight(), 8);
    assert_eq!(device.le_acl_data_packet_length(), 251);
    assert_eq!(device.le_acl_max_in_flight(), 12);
    assert_eq!(device.iso_data_packet_length(), Some(120));
    assert_eq!(device.iso_max_in_flight(), Some(6));

    let zero_le = ControllerBufferInfo {
        data_packet_length: 0,
        total_num_data_packets: 0,
    };
    let zero_iso = ControllerBufferInfo {
        data_packet_length: 0,
        total_num_data_packets: 0,
    };
    assert!(device.configure_controller_packet_pools(Some(classic), Some(zero_le), Some(zero_iso),));
    assert_eq!(device.le_acl_buffer(), Some(zero_le));
    assert_eq!(device.iso_buffer(), Some(zero_iso));
    assert_eq!(device.le_acl_data_packet_length(), 1021);
    assert_eq!(device.le_acl_max_in_flight(), 8);
    assert_eq!(device.iso_data_packet_length(), None);
    assert_eq!(device.iso_max_in_flight(), None);
}

#[test]
fn external_controller_packet_pool_reconfiguration_preserves_pending_state() {
    let mut link = LocalLink::new();
    let central = link.add_controller(Controller::new("C", addr("00:00:00:00:00:01")));
    let peripheral = link.add_controller(Controller::new("P", addr("00:00:00:00:00:02")));
    let mut devices = [Device::new(central), Device::new(peripheral)];
    let classic = ControllerBufferInfo {
        data_packet_length: 16,
        total_num_data_packets: 2,
    };
    let le = ControllerBufferInfo {
        data_packet_length: 8,
        total_num_data_packets: 1,
    };
    assert!(devices[0].configure_controller_packet_pools(Some(classic), Some(le), None));
    connect(&mut link, central, peripheral);
    pump(&mut link, &mut devices);

    assert!(devices[0].send_l2cap(&mut link, 0x0040, &[0xA5; 24]));
    assert!(devices[0].acl_packets_pending() > 0);
    assert!(!devices[0].configure_controller_packet_pools(
        Some(ControllerBufferInfo {
            data_packet_length: 1021,
            total_num_data_packets: 8,
        }),
        None,
        None,
    ));
    assert_eq!(devices[0].classic_acl_buffer(), Some(classic));
    assert_eq!(devices[0].le_acl_buffer(), Some(le));
    assert_eq!(devices[0].acl_data_packet_length(), 16);
    assert_eq!(devices[0].le_acl_data_packet_length(), 8);
    assert_eq!(devices[0].le_acl_max_in_flight(), 1);
}
