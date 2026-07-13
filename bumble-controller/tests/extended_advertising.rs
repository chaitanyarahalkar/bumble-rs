use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink, ROLE_CENTRAL, ROLE_PERIPHERAL};
use bumble_hci::{Command, Event, HciPacket, LeMetaEvent, ReturnParameters};

fn addr(value: &str, address_type: AddressType) -> Address {
    Address::parse(value, address_type).unwrap()
}

fn parameters(handle: u8, own_address_type: u8, sid: u8) -> Command {
    Command::LeSetExtendedAdvertisingParameters {
        advertising_handle: handle,
        advertising_event_properties: 0x0003,
        primary_advertising_interval_min: 0x20,
        primary_advertising_interval_max: 0x40,
        primary_advertising_channel_map: 7,
        own_address_type,
        peer_address_type: 0,
        peer_address: addr("00:00:00:00:00:00", AddressType::PUBLIC_DEVICE),
        advertising_filter_policy: 0,
        advertising_tx_power: 0,
        primary_advertising_phy: 1,
        secondary_advertising_max_skip: 0,
        secondary_advertising_phy: 2,
        advertising_sid: sid,
        scan_request_notification_enable: 0,
    }
}

fn extended_scan_parameters() -> Command {
    Command::LeSetExtendedScanParameters {
        own_address_type: 1,
        scanning_filter_policy: 0,
        scanning_phys: 1,
        scan_types: vec![1],
        scan_intervals: vec![0x20],
        scan_windows: vec![0x20],
    }
}

fn enable_set(handle: u8, enable: bool) -> Command {
    Command::LeSetExtendedAdvertisingEnable {
        enable: u8::from(enable),
        advertising_handles: vec![handle],
        durations: vec![0],
        max_extended_advertising_events: vec![0],
    }
}

#[test]
fn fragmented_extended_set_emits_advertising_and_scan_response_reports() {
    let mut link = LocalLink::new();
    let advertiser = link.add_controller(Controller::new(
        "advertiser",
        addr("10:00:00:00:00:01", AddressType::PUBLIC_DEVICE),
    ));
    let scanner = link.add_controller(Controller::new(
        "scanner",
        addr("10:00:00:00:00:02", AddressType::PUBLIC_DEVICE),
    ));
    let advertising_address = addr("C4:F2:17:1A:1D:BB", AddressType::RANDOM_DEVICE);

    link.handle_command(
        advertiser,
        Command::LeSetAdvertisingSetRandomAddress {
            advertising_handle: 7,
            random_address: advertising_address.clone(),
        },
    );
    link.handle_command(advertiser, parameters(7, 1, 5));
    for (operation, fragment) in [(1, vec![1, 2]), (0, vec![3]), (2, vec![4, 5])] {
        link.handle_command(
            advertiser,
            Command::LeSetExtendedAdvertisingData {
                advertising_handle: 7,
                operation,
                fragment_preference: 1,
                advertising_data: fragment,
            },
        );
    }
    link.handle_command(
        advertiser,
        Command::LeSetExtendedScanResponseData {
            advertising_handle: 7,
            operation: 3,
            fragment_preference: 1,
            scan_response_data: vec![9, 8, 7],
        },
    );
    link.handle_command(advertiser, enable_set(7, true));
    assert!(link.controller(advertiser).is_extended_advertising(7));

    link.handle_command(scanner, extended_scan_parameters());
    link.handle_command(
        scanner,
        Command::LeSetExtendedScanEnable {
            enable: 1,
            filter_duplicates: 0,
            duration: 0,
            period: 0,
        },
    );
    let _ = link.drain_host_events(advertiser);
    let _ = link.drain_host_events(scanner);

    link.propagate_advertising();
    let events = link.drain_host_events(scanner);
    assert_eq!(events.len(), 2);
    let reports: Vec<_> = events
        .iter()
        .map(|event| match event {
            HciPacket::Event(Event::LeMeta(LeMetaEvent::ExtendedAdvertisingReport { reports })) => {
                &reports[0]
            }
            other => panic!("expected extended advertising report, got {other:?}"),
        })
        .collect();
    assert_eq!(reports[0].address, advertising_address);
    assert_eq!(reports[0].advertising_sid, 5);
    assert_eq!(reports[0].primary_phy, 1);
    assert_eq!(reports[0].secondary_phy, 2);
    assert_eq!(reports[0].data, vec![1, 2, 3, 4, 5]);
    assert_eq!(reports[1].event_type, 0x0008);
    assert_eq!(reports[1].data, vec![9, 8, 7]);
    for event in events {
        assert_eq!(HciPacket::from_bytes(&event.to_bytes()).unwrap(), event);
    }
}

#[test]
fn advertising_set_reads_unknown_handle_and_lifecycle_match_upstream() {
    let mut controller = Controller::new(
        "controller",
        addr("10:00:00:00:00:01", AddressType::PUBLIC_DEVICE),
    );
    controller.handle_command(Command::LeSetExtendedAdvertisingData {
        advertising_handle: 1,
        operation: 3,
        fragment_preference: 1,
        advertising_data: vec![1],
    });
    let events = controller.drain_host_events();
    assert_eq!(events.len(), 1);
    assert_eq!(
        match &events[0] {
            HciPacket::Event(Event::CommandComplete {
                return_parameters, ..
            }) => return_parameters.status(),
            _ => None,
        },
        Some(0x42)
    );

    controller.handle_command(parameters(1, 0, 1));
    controller.handle_command(Command::LeReadMaximumAdvertisingDataLength);
    controller.handle_command(Command::LeReadNumberOfSupportedAdvertisingSets);
    let events = controller.drain_host_events();
    assert!(matches!(
        &events[0],
        HciPacket::Event(Event::CommandComplete {
            return_parameters: ReturnParameters::Raw { data },
            ..
        }) if data == &[0, 0]
    ));
    assert!(matches!(
        &events[1],
        HciPacket::Event(Event::CommandComplete {
            return_parameters: ReturnParameters::LeReadMaximumAdvertisingDataLength {
                status: 0,
                max_advertising_data_length: 0x0672,
            },
            ..
        })
    ));
    assert!(matches!(
        &events[2],
        HciPacket::Event(Event::CommandComplete {
            return_parameters: ReturnParameters::LeReadNumberOfSupportedAdvertisingSets {
                status: 0,
                num_supported_advertising_sets: 0xF0,
            },
            ..
        })
    ));

    controller.handle_command(enable_set(1, true));
    assert!(controller.is_extended_advertising(1));
    controller.handle_command(Command::LeRemoveAdvertisingSet {
        advertising_handle: 1,
    });
    assert!(!controller.is_extended_advertising(1));
}

#[test]
fn extended_create_connection_uses_the_advertising_set_address() {
    let mut link = LocalLink::new();
    let central_address = addr("C4:F2:17:1A:1D:AA", AddressType::RANDOM_DEVICE);
    let peripheral_address = addr("C4:F2:17:1A:1D:BB", AddressType::RANDOM_DEVICE);
    let central = link.add_controller(Controller::new(
        "central",
        addr("10:00:00:00:00:01", AddressType::PUBLIC_DEVICE),
    ));
    let peripheral = link.add_controller(Controller::new(
        "peripheral",
        addr("10:00:00:00:00:02", AddressType::PUBLIC_DEVICE),
    ));
    link.handle_command(
        central,
        Command::LeSetRandomAddress {
            random_address: central_address.clone(),
        },
    );
    link.handle_command(
        peripheral,
        Command::LeSetAdvertisingSetRandomAddress {
            advertising_handle: 3,
            random_address: peripheral_address.clone(),
        },
    );
    link.handle_command(peripheral, parameters(3, 1, 2));
    link.handle_command(peripheral, enable_set(3, true));
    let _ = link.drain_host_events(central);
    let _ = link.drain_host_events(peripheral);

    link.handle_command(
        central,
        Command::LeExtendedCreateConnection {
            initiator_filter_policy: 0,
            own_address_type: 1,
            peer_address_type: 1,
            peer_address: peripheral_address.clone(),
            initiating_phys: 1,
            scan_intervals: vec![0x20],
            scan_windows: vec![0x20],
            connection_interval_mins: vec![0x18],
            connection_interval_maxs: vec![0x28],
            max_latencies: vec![0],
            supervision_timeouts: vec![0x2A],
            min_ce_lengths: vec![0],
            max_ce_lengths: vec![0],
        },
    );
    assert!(matches!(
        link.drain_host_events(central).as_slice(),
        [HciPacket::Event(Event::CommandStatus { status: 0, .. })]
    ));
    link.establish_connections();

    let central_events = link.drain_host_events(central);
    let peripheral_events = link.drain_host_events(peripheral);
    assert!(matches!(
        central_events.as_slice(),
        [HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionComplete {
            role: ROLE_CENTRAL,
            peer_address,
            ..
        }))] if peer_address == &peripheral_address
    ));
    assert!(matches!(
        peripheral_events.as_slice(),
        [HciPacket::Event(Event::LeMeta(LeMetaEvent::ConnectionComplete {
            role: ROLE_PERIPHERAL,
            peer_address,
            ..
        }))] if peer_address == &central_address
    ));
}
