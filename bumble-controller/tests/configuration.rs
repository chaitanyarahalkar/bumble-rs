use bumble::{Address, AddressType};
use bumble_controller::{Controller, DefaultPhy, LeScanParameters};
use bumble_hci::{Command, Event, HciPacket, LeMetaEvent, ReturnParameters};

fn random_address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

fn public_address(value: &str) -> Address {
    Address::parse(value, AddressType::PUBLIC_DEVICE).unwrap()
}

fn complete(controller: &mut Controller, command: Command) -> ReturnParameters {
    controller.handle_command(command);
    let events = controller.drain_host_events();
    assert_eq!(events.len(), 1, "expected one completion: {events:?}");
    match &events[0] {
        HciPacket::Event(Event::CommandComplete {
            return_parameters, ..
        }) => return_parameters.clone(),
        other => panic!("expected Command Complete, got {other:?}"),
    }
}

fn command_status(controller: &mut Controller, command: Command) -> u8 {
    controller.handle_command(command);
    let events = controller.drain_host_events();
    assert_eq!(events.len(), 1, "expected one status: {events:?}");
    match events[0] {
        HciPacket::Event(Event::CommandStatus { status, .. }) => status,
        ref other => panic!("expected Command Status, got {other:?}"),
    }
}

#[test]
fn controller_configuration_fields_are_retained() {
    let mut controller = Controller::new("C", public_address("00:11:22:33:44:55"));
    assert_eq!(
        complete(
            &mut controller,
            Command::SetEventMask {
                event_mask: [1, 2, 3, 4, 5, 6, 7, 8],
            },
        ),
        ReturnParameters::Status { status: 0 }
    );
    assert_eq!(
        complete(
            &mut controller,
            Command::SetEventMaskPage2 {
                event_mask_page_2: [8, 7, 6, 5, 4, 3, 2, 1],
            },
        ),
        ReturnParameters::Status { status: 0 }
    );
    assert_eq!(
        complete(
            &mut controller,
            Command::LeSetEventMask {
                le_event_mask: [0xAA; 8],
            },
        ),
        ReturnParameters::Status { status: 0 }
    );
    complete(&mut controller, Command::WriteScanEnable { scan_enable: 3 });
    complete(
        &mut controller,
        Command::LeSetDefaultPhy {
            all_phys: 1,
            tx_phys: 2,
            rx_phys: 4,
        },
    );

    assert_eq!(controller.event_mask(), [1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(controller.event_mask_page_2(), [8, 7, 6, 5, 4, 3, 2, 1]);
    assert_eq!(controller.le_event_mask(), [0xAA; 8]);
    assert_eq!(controller.classic_scan_enable(), 3);
    assert_eq!(
        controller.default_phy(),
        DefaultPhy {
            all_phys: 1,
            tx_phys: 2,
            rx_phys: 4,
        }
    );
}

#[test]
fn legacy_parameters_drive_address_type_and_active_scan_response() {
    let public = public_address("00:11:22:33:44:55");
    let mut advertiser = Controller::new("advertiser", public.clone());
    complete(
        &mut advertiser,
        Command::LeSetRandomAddress {
            random_address: random_address("C4:F2:17:1A:1D:BB"),
        },
    );
    complete(
        &mut advertiser,
        Command::LeSetAdvertisingParameters {
            advertising_interval_min: 0x0800,
            advertising_interval_max: 0x0900,
            advertising_type: 2,
            own_address_type: 0,
            peer_address_type: 1,
            peer_address: random_address("AA:BB:CC:DD:EE:FF"),
            advertising_channel_map: 7,
            advertising_filter_policy: 1,
        },
    );
    complete(
        &mut advertiser,
        Command::LeSetAdvertisingData {
            advertising_data: vec![1, 2, 3],
        },
    );
    complete(
        &mut advertiser,
        Command::LeSetScanResponseData {
            scan_response_data: vec![4, 5, 6],
        },
    );
    complete(
        &mut advertiser,
        Command::LeSetAdvertisingEnable {
            advertising_enable: 1,
        },
    );

    let pdu = advertiser.advertising_pdu().unwrap();
    assert_eq!(pdu.address, public);
    assert_eq!(pdu.address_type, 0);
    assert_eq!(pdu.event_type, 2);
    assert_eq!(pdu.data, vec![1, 2, 3]);
    assert_eq!(pdu.scan_response_data, vec![4, 5, 6]);

    let mut scanner = Controller::new("scanner", public_address("00:11:22:33:44:66"));
    complete(
        &mut scanner,
        Command::LeSetScanParameters {
            le_scan_type: 1,
            le_scan_interval: 0x0020,
            le_scan_window: 0x0010,
            own_address_type: 0,
            scanning_filter_policy: 0,
        },
    );
    complete(
        &mut scanner,
        Command::LeSetScanEnable {
            le_scan_enable: 1,
            filter_duplicates: 1,
        },
    );
    scanner.on_advertising_pdu(&pdu);
    match &scanner.drain_host_events()[0] {
        HciPacket::Event(Event::LeMeta(LeMetaEvent::AdvertisingReport { reports })) => {
            assert_eq!(reports.len(), 2);
            assert_eq!(reports[0].event_type, 2);
            assert_eq!(reports[0].data, vec![1, 2, 3]);
            assert_eq!(reports[1].event_type, 4);
            assert_eq!(reports[1].data, vec![4, 5, 6]);
        }
        other => panic!("expected advertising reports, got {other:?}"),
    }
    assert!(scanner.filter_duplicates());
}

#[test]
fn legacy_scan_parameters_are_disallowed_while_enabled() {
    let mut controller = Controller::new("C", public_address("00:11:22:33:44:55"));
    let initial = LeScanParameters {
        le_scan_type: 1,
        le_scan_interval: 0x0020,
        le_scan_window: 0x0010,
        own_address_type: 1,
        scanning_filter_policy: 2,
    };
    complete(
        &mut controller,
        Command::LeSetScanParameters {
            le_scan_type: initial.le_scan_type,
            le_scan_interval: initial.le_scan_interval,
            le_scan_window: initial.le_scan_window,
            own_address_type: initial.own_address_type,
            scanning_filter_policy: initial.scanning_filter_policy,
        },
    );
    complete(
        &mut controller,
        Command::LeSetScanEnable {
            le_scan_enable: 1,
            filter_duplicates: 0,
        },
    );
    assert_eq!(
        complete(
            &mut controller,
            Command::LeSetScanParameters {
                le_scan_type: 0,
                le_scan_interval: 1,
                le_scan_window: 1,
                own_address_type: 0,
                scanning_filter_policy: 0,
            },
        ),
        ReturnParameters::Status { status: 0x0C }
    );
    assert_eq!(controller.scan_parameters(), initial);
}

#[test]
fn a_second_connection_start_is_disallowed_while_one_is_pending() {
    let mut controller = Controller::new("C", public_address("00:11:22:33:44:55"));
    let create = |peer_address| Command::LeCreateConnection {
        le_scan_interval: 16,
        le_scan_window: 16,
        initiator_filter_policy: 0,
        peer_address_type: 1,
        peer_address,
        own_address_type: 1,
        connection_interval_min: 24,
        connection_interval_max: 40,
        max_latency: 0,
        supervision_timeout: 42,
        min_ce_length: 0,
        max_ce_length: 0,
    };

    assert_eq!(
        command_status(&mut controller, create(random_address("C4:F2:17:1A:1D:01")),),
        0
    );
    assert_eq!(
        command_status(&mut controller, create(random_address("C4:F2:17:1A:1D:02")),),
        0x0C
    );
}
