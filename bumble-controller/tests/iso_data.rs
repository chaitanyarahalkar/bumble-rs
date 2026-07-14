use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_hci::{CodingFormat, Command, Event, HciPacket, IsoDataPacket, LeMetaEvent};

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

fn create_connection(peer: Address) -> Command {
    Command::LeCreateConnection {
        le_scan_interval: 16,
        le_scan_window: 16,
        initiator_filter_policy: 0,
        peer_address_type: 1,
        peer_address: peer,
        own_address_type: 1,
        connection_interval_min: 24,
        connection_interval_max: 40,
        max_latency: 0,
        supervision_timeout: 42,
        min_ce_length: 0,
        max_ce_length: 0,
    }
}

fn connected_cis() -> (LocalLink, usize, usize, u16, u16) {
    let mut link = LocalLink::new();
    let central = link.add_controller(Controller::new("central", address("00:00:00:00:00:01")));
    let peripheral =
        link.add_controller(Controller::new("peripheral", address("00:00:00:00:00:02")));
    let peripheral_address = address("C4:F2:17:1A:1D:BB");
    link.handle_command(
        peripheral,
        Command::LeSetRandomAddress {
            random_address: peripheral_address.clone(),
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
            random_address: address("C4:F2:17:1A:1D:AA"),
        },
    );
    link.handle_command(central, create_connection(peripheral_address));
    link.establish_connections();
    let acl_handle = link.controller(central).connections()[0].handle;
    let _ = link.drain_host_events(central);
    let _ = link.drain_host_events(peripheral);

    link.handle_command(
        central,
        Command::LeSetCigParameters {
            cig_id: 1,
            sdu_interval_c_to_p: 10_000,
            sdu_interval_p_to_c: 10_000,
            worst_case_sca: 0,
            packing: 0,
            framing: 0,
            max_transport_latency_c_to_p: 10,
            max_transport_latency_p_to_c: 10,
            cis_id: vec![2],
            max_sdu_c_to_p: vec![120],
            max_sdu_p_to_c: vec![120],
            phy_c_to_p: vec![1],
            phy_p_to_c: vec![1],
            rtn_c_to_p: vec![3],
            rtn_p_to_c: vec![3],
        },
    );
    let central_cis_handle = match &link.drain_host_events(central)[0] {
        HciPacket::Event(Event::CommandComplete {
            return_parameters: bumble_hci::ReturnParameters::Raw { data },
            ..
        }) => u16::from_le_bytes([data[3], data[4]]),
        other => panic!("expected CIG complete, got {other:?}"),
    };
    link.handle_command(
        central,
        Command::LeCreateCis {
            cis_connection_handle: vec![central_cis_handle],
            acl_connection_handle: vec![acl_handle],
        },
    );
    let _ = link.drain_host_events(central);
    link.pump_ll();
    let peripheral_cis_handle = link
        .drain_host_events(peripheral)
        .iter()
        .find_map(|event| match event {
            HciPacket::Event(Event::LeMeta(LeMetaEvent::CisRequest {
                cis_connection_handle,
                ..
            })) => Some(*cis_connection_handle),
            _ => None,
        })
        .unwrap();
    link.handle_command(
        peripheral,
        Command::LeAcceptCisRequest {
            connection_handle: peripheral_cis_handle,
        },
    );
    let _ = link.drain_host_events(peripheral);
    link.pump_ll();
    let _ = link.drain_host_events(central);
    let _ = link.drain_host_events(peripheral);
    (
        link,
        central,
        peripheral,
        central_cis_handle,
        peripheral_cis_handle,
    )
}

fn setup_path(link: &mut LocalLink, controller: usize, handle: u16, direction: u8) -> u8 {
    link.handle_command(
        controller,
        Command::LeSetupIsoDataPath {
            connection_handle: handle,
            data_path_direction: direction,
            data_path_id: 0,
            codec_id: CodingFormat::TRANSPARENT,
            controller_delay: 0,
            codec_configuration: Vec::new(),
        },
    );
    match &link.drain_host_events(controller)[0] {
        HciPacket::Event(Event::CommandComplete {
            return_parameters: bumble_hci::ReturnParameters::Raw { data },
            ..
        }) => {
            assert_eq!(&data[1..], &handle.to_le_bytes());
            data[0]
        }
        other => panic!("expected setup data path complete, got {other:?}"),
    }
}

#[test]
fn iso_data_path_setup_routes_fragments_and_completes_packets() {
    let (mut link, central, peripheral, central_cis, peripheral_cis) = connected_cis();
    assert_eq!(setup_path(&mut link, central, central_cis, 0), 0);
    assert_eq!(setup_path(&mut link, peripheral, peripheral_cis, 1), 0);
    assert_eq!(setup_path(&mut link, central, central_cis, 0), 0x0C);

    let packet = IsoDataPacket {
        connection_handle: central_cis,
        pb_flag: 0b10,
        ts_flag: 0,
        data_total_length: 9,
        time_stamp: None,
        packet_sequence_number: Some(7),
        iso_sdu_length: Some(5),
        packet_status_flag: Some(0),
        iso_sdu_fragment: vec![1, 2, 3, 4, 5],
    };
    assert!(link.send_iso_packet(central, packet.clone()));
    assert!(link.drain_host_events(central).iter().any(|event| matches!(
        event,
        HciPacket::Event(Event::NumberOfCompletedPackets {
            connection_handles,
            num_completed_packets,
        }) if connection_handles == &[central_cis] && num_completed_packets == &[1]
    )));
    let received = link.drain_host_events(peripheral);
    assert_eq!(received.len(), 1);
    assert_eq!(
        received[0],
        HciPacket::IsoData(IsoDataPacket {
            connection_handle: peripheral_cis,
            ..packet
        })
    );

    link.handle_command(
        peripheral,
        Command::LeRemoveIsoDataPath {
            connection_handle: peripheral_cis,
            data_path_direction: 0x02,
        },
    );
    let remove = link.drain_host_events(peripheral);
    assert!(matches!(
        &remove[0],
        HciPacket::Event(Event::CommandComplete {
            return_parameters: bumble_hci::ReturnParameters::Raw { data },
            ..
        }) if data == &[0, peripheral_cis as u8, (peripheral_cis >> 8) as u8]
    ));
    assert!(!link.send_iso_packet(
        central,
        IsoDataPacket {
            connection_handle: central_cis,
            pb_flag: 0b10,
            ts_flag: 0,
            data_total_length: 5,
            time_stamp: None,
            packet_sequence_number: Some(8),
            iso_sdu_length: Some(1),
            packet_status_flag: Some(0),
            iso_sdu_fragment: vec![9],
        }
    ));
}

#[test]
fn iso_data_path_rejects_unknown_handles_and_directions() {
    let (mut link, central, _peripheral, central_cis, _peripheral_cis) = connected_cis();
    assert_eq!(setup_path(&mut link, central, 0x0FFF, 0), 0x02);
    assert_eq!(setup_path(&mut link, central, central_cis, 2), 0x12);
}

#[test]
fn cis_termination_is_pumped_and_central_handle_can_be_reused() {
    let (mut link, central, peripheral, central_cis, peripheral_cis) = connected_cis();
    let acl_handle = link.controller(central).connections()[0].handle;

    link.handle_command(
        central,
        Command::Disconnect {
            connection_handle: central_cis,
            reason: 0x13,
        },
    );
    let central_events = link.drain_host_events(central);
    assert!(central_events.iter().any(|packet| matches!(
        packet,
        HciPacket::Event(Event::CommandStatus { status: 0, .. })
    )));
    assert!(central_events.iter().any(|packet| matches!(
        packet,
        HciPacket::Event(Event::DisconnectionComplete {
            connection_handle,
            reason: 0x13,
            ..
        }) if *connection_handle == central_cis
    )));
    assert!(link.drain_host_events(peripheral).is_empty());

    link.pump_ll();
    let peripheral_events = link.drain_host_events(peripheral);
    assert!(peripheral_events.iter().any(|packet| matches!(
        packet,
        HciPacket::Event(Event::DisconnectionComplete {
            connection_handle,
            reason: 0x13,
            ..
        }) if *connection_handle == peripheral_cis
    )));

    // Upstream keeps central CIS handles allocated after teardown. Reusing the
    // same handle starts a fresh CIS exchange and allocates a new peer handle.
    link.handle_command(
        central,
        Command::LeCreateCis {
            cis_connection_handle: vec![central_cis],
            acl_connection_handle: vec![acl_handle],
        },
    );
    assert!(link
        .drain_host_events(central)
        .iter()
        .any(|packet| matches!(
            packet,
            HciPacket::Event(Event::CommandStatus { status: 0, .. })
        )));
    link.pump_ll();
    let replacement_peripheral_cis = link
        .drain_host_events(peripheral)
        .iter()
        .find_map(|packet| match packet {
            HciPacket::Event(Event::LeMeta(LeMetaEvent::CisRequest {
                cis_connection_handle,
                ..
            })) => Some(*cis_connection_handle),
            _ => None,
        })
        .expect("replacement CIS request");
    assert_ne!(replacement_peripheral_cis, peripheral_cis);

    link.handle_command(
        peripheral,
        Command::LeAcceptCisRequest {
            connection_handle: replacement_peripheral_cis,
        },
    );
    let _ = link.drain_host_events(peripheral);
    link.pump_ll();
    assert!(link
        .drain_host_events(central)
        .iter()
        .any(|packet| matches!(
            packet,
            HciPacket::Event(Event::LeMeta(LeMetaEvent::CisEstablished {
                status: 0,
                connection_handle,
                ..
            })) if *connection_handle == central_cis
        )));
}
