//! Controller handling of the read commands and the per-connection
//! `LE_Set_Data_Length` / `LE_Set_PHY` requests added in the "HCI, controller &
//! link" slice.
//!
//! Bumble tests the controller only through the full Device/host stack, so these
//! are self-defined; each still drives the real `bumble-hci` codec and asserts
//! the exact events a host would observe.

use bumble::{Address, AddressType};
use bumble_controller::Controller;
use bumble_hci::{Command, Event, HciPacket, LeMetaEvent, ReturnParameters};

fn addr(s: &str) -> Address {
    Address::parse(s, AddressType::RANDOM_DEVICE).unwrap()
}

/// The single Command Complete's return parameters, panicking otherwise.
fn only_complete(events: &[HciPacket]) -> &ReturnParameters {
    assert_eq!(events.len(), 1, "expected exactly one event: {events:?}");
    match &events[0] {
        HciPacket::Event(Event::CommandComplete {
            return_parameters, ..
        }) => return_parameters,
        other => panic!("expected Command Complete, got {other:?}"),
    }
}

fn complete(controller: &mut Controller, command: Command) -> ReturnParameters {
    controller.handle_command(command);
    only_complete(&controller.drain_host_events()).clone()
}

#[test]
fn read_bd_addr_returns_public_address() {
    let public = Address::parse("11:22:33:44:55:66", AddressType::PUBLIC_DEVICE).unwrap();
    let mut c = Controller::new("C", public.clone());
    c.handle_command(Command::ReadBdAddr);
    match only_complete(&c.drain_host_events()) {
        ReturnParameters::ReadBdAddr { status, bd_addr } => {
            assert_eq!(*status, 0);
            assert_eq!(*bd_addr, public);
        }
        other => panic!("expected ReadBdAddr params, got {other:?}"),
    }
}

#[test]
fn read_local_name_returns_padded_name() {
    let mut c = Controller::new("Bumble", addr("00:11:22:33:44:55"));
    c.handle_command(Command::ReadLocalName);
    match only_complete(&c.drain_host_events()) {
        ReturnParameters::ReadLocalName { status, local_name } => {
            assert_eq!(*status, 0);
            assert_eq!(local_name.len(), 248);
            assert_eq!(&local_name[..6], b"Bumble");
            assert!(local_name[6..].iter().all(|&b| b == 0));
        }
        other => panic!("expected ReadLocalName params, got {other:?}"),
    }
}

#[test]
fn le_read_buffer_size_returns_params() {
    let mut c = Controller::new("C", addr("00:11:22:33:44:55"));
    c.handle_command(Command::LeReadBufferSize);
    match only_complete(&c.drain_host_events()) {
        ReturnParameters::LeReadBufferSize {
            status,
            le_acl_data_packet_length,
            total_num_le_acl_data_packets,
        } => {
            assert_eq!(*status, 0);
            assert_eq!(*le_acl_data_packet_length, 27);
            assert_eq!(*total_num_le_acl_data_packets, 64);
        }
        other => panic!("expected LeReadBufferSize params, got {other:?}"),
    }
}

#[test]
fn le_read_local_supported_features_returns_bitmap() {
    let mut c = Controller::new("C", addr("00:11:22:33:44:55"));
    c.handle_command(Command::LeReadLocalSupportedFeatures);
    match only_complete(&c.drain_host_events()) {
        ReturnParameters::LeReadLocalSupportedFeatures {
            status,
            le_features,
        } => {
            assert_eq!(*status, 0);
            assert_eq!(*le_features, [0x00, 0x10, 0x00, 0xF0, 0, 0, 0, 0]);
        }
        other => panic!("expected LE feature params, got {other:?}"),
    }
}

#[test]
fn le_read_all_local_supported_features_returns_padded_catalog() {
    let mut controller = Controller::new("C", addr("00:11:22:33:44:55"));
    controller.handle_command(Command::ReadLocalSupportedCommands);
    match only_complete(&controller.drain_host_events()) {
        ReturnParameters::ReadLocalSupportedCommands {
            supported_commands, ..
        } => assert_ne!(supported_commands[47] & 0x04, 0),
        other => panic!("expected supported commands, got {other:?}"),
    }

    controller.handle_command(Command::LeReadAllLocalSupportedFeatures);
    match only_complete(&controller.drain_host_events()) {
        ReturnParameters::LeReadAllLocalSupportedFeatures {
            status,
            max_page,
            le_features,
        } => {
            assert_eq!(*status, 0);
            assert_eq!(*max_page, 0);
            assert_eq!(&le_features[..8], &[0x00, 0x10, 0x00, 0xF0, 0, 0, 0, 0]);
            assert!(le_features[8..].iter().all(|feature| *feature == 0));
        }
        other => panic!("expected all LE feature params, got {other:?}"),
    }
}

#[test]
fn read_local_extended_features_returns_all_pages_and_rejects_overflow() {
    let mut controller = Controller::new("C", addr("00:11:22:33:44:55"));
    for page_number in 0..=3 {
        controller.handle_command(Command::ReadLocalExtendedFeatures { page_number });
        match only_complete(&controller.drain_host_events()) {
            ReturnParameters::ReadLocalExtendedFeatures {
                status,
                page_number: actual_page_number,
                maximum_page_number,
                extended_lmp_features,
            } => {
                assert_eq!(*status, 0);
                assert_eq!(*actual_page_number, page_number);
                assert_eq!(*maximum_page_number, 3);
                if page_number == 0 {
                    assert_eq!(*extended_lmp_features, [0, 0, 0, 0, 0x60, 0, 0, 0x80]);
                } else {
                    assert_eq!(*extended_lmp_features, [0; 8]);
                }
            }
            other => panic!("expected ReadLocalExtendedFeatures params, got {other:?}"),
        }
    }

    controller.handle_command(Command::ReadLocalExtendedFeatures { page_number: 4 });
    assert_eq!(
        only_complete(&controller.drain_host_events()),
        &ReturnParameters::Status { status: 0x12 }
    );
}

#[test]
fn remaining_data_queries_return_upstream_defaults() {
    let mut controller = Controller::new("C", addr("00:11:22:33:44:55"));
    assert_eq!(
        complete(&mut controller, Command::ReadClassOfDevice),
        ReturnParameters::ReadClassOfDevice {
            status: 0,
            class_of_device: 0,
        }
    );
    assert_eq!(
        complete(
            &mut controller,
            Command::LeReadAdvertisingPhysicalChannelTxPower,
        ),
        ReturnParameters::LeReadAdvertisingPhysicalChannelTxPower {
            status: 0,
            tx_power_level: 0,
        }
    );
    assert_eq!(
        complete(&mut controller, Command::LeReadFilterAcceptListSize),
        ReturnParameters::LeReadFilterAcceptListSize {
            status: 0,
            filter_accept_list_size: 8,
        }
    );
    assert_eq!(
        complete(&mut controller, Command::LeReadSupportedStates),
        ReturnParameters::LeReadSupportedStates {
            status: 0,
            le_states: [0xFF, 0xFF, 0x3F, 0xFF, 0xFF, 0x03, 0, 0],
        }
    );
    assert_eq!(
        complete(&mut controller, Command::LeReadResolvingListSize),
        ReturnParameters::LeReadResolvingListSize {
            status: 0,
            resolving_list_size: 8,
        }
    );
    assert_eq!(
        complete(
            &mut controller,
            Command::LeReadPhy {
                connection_handle: 0x0ABC,
            },
        ),
        ReturnParameters::LeReadPhy {
            status: 0,
            connection_handle: 0x0ABC,
            tx_phy: 1,
            rx_phy: 1,
        }
    );
    assert_eq!(
        complete(&mut controller, Command::LeReadTransmitPower),
        ReturnParameters::LeReadTransmitPower {
            status: 0,
            min_tx_power: 0,
            max_tx_power: 0,
        }
    );
}

#[test]
fn state_backed_queries_follow_writes() {
    let mut controller = Controller::new("C", addr("00:11:22:33:44:55"));
    let mut local_name = [0; 248];
    local_name[..7].copy_from_slice(b"Renamed");
    assert_eq!(
        complete(&mut controller, Command::WriteLocalName { local_name }),
        ReturnParameters::Status { status: 0 }
    );
    match complete(&mut controller, Command::ReadLocalName) {
        ReturnParameters::ReadLocalName { local_name, .. } => {
            assert_eq!(&local_name[..7], b"Renamed");
            assert!(local_name[7..].iter().all(|byte| *byte == 0));
        }
        other => panic!("expected local name, got {other:?}"),
    }

    assert_eq!(
        complete(&mut controller, Command::ReadSynchronousFlowControlEnable,),
        ReturnParameters::ReadSynchronousFlowControlEnable {
            status: 0,
            synchronous_flow_control_enable: 0,
        }
    );
    assert_eq!(
        complete(
            &mut controller,
            Command::WriteSynchronousFlowControlEnable {
                synchronous_flow_control_enable: 1,
            },
        ),
        ReturnParameters::Status { status: 0 }
    );
    assert_eq!(
        complete(&mut controller, Command::ReadSynchronousFlowControlEnable,),
        ReturnParameters::ReadSynchronousFlowControlEnable {
            status: 0,
            synchronous_flow_control_enable: 1,
        }
    );
    assert_eq!(
        complete(
            &mut controller,
            Command::WriteSynchronousFlowControlEnable {
                synchronous_flow_control_enable: 2,
            },
        ),
        ReturnParameters::Status { status: 0x12 }
    );

    complete(
        &mut controller,
        Command::WriteSimplePairingMode {
            simple_pairing_mode: 1,
        },
    );
    complete(
        &mut controller,
        Command::WriteLeHostSupport {
            le_supported_host: 1,
            simultaneous_le_host: 1,
        },
    );
    assert_eq!(
        complete(&mut controller, Command::ReadLeHostSupport),
        ReturnParameters::ReadLeHostSupport {
            status: 0,
            le_supported_host: 1,
            unused: 0,
        }
    );
    match complete(
        &mut controller,
        Command::ReadLocalExtendedFeatures { page_number: 1 },
    ) {
        ReturnParameters::ReadLocalExtendedFeatures {
            extended_lmp_features,
            ..
        } => assert_eq!(extended_lmp_features[0] & 0x03, 0x03),
        other => panic!("expected extended features, got {other:?}"),
    }

    assert_eq!(
        complete(
            &mut controller,
            Command::WriteAuthenticatedPayloadTimeout {
                connection_handle: 0x0ABC,
                authenticated_payload_timeout: 0x0100,
            },
        ),
        ReturnParameters::WriteAuthenticatedPayloadTimeout {
            status: 0,
            connection_handle: 0x0ABC,
        }
    );

    assert_eq!(
        complete(
            &mut controller,
            Command::LeWriteSuggestedDefaultDataLength {
                suggested_max_tx_octets: 251,
                suggested_max_tx_time: 2_120,
            },
        ),
        ReturnParameters::Status { status: 0 }
    );
    assert_eq!(
        complete(&mut controller, Command::LeReadSuggestedDefaultDataLength,),
        ReturnParameters::LeReadSuggestedDefaultDataLength {
            status: 0,
            suggested_max_tx_octets: 251,
            suggested_max_tx_time: 2_120,
        }
    );
}

#[test]
fn remove_cig_returns_id_and_tracks_existing_groups() {
    let mut controller = Controller::new("C", addr("00:11:22:33:44:55"));
    let set_response = complete(
        &mut controller,
        Command::LeSetCigParameters {
            cig_id: 7,
            sdu_interval_c_to_p: 10_000,
            sdu_interval_p_to_c: 10_000,
            worst_case_sca: 0,
            packing: 0,
            framing: 0,
            max_transport_latency_c_to_p: 10,
            max_transport_latency_p_to_c: 10,
            cis_id: vec![1],
            max_sdu_c_to_p: vec![120],
            max_sdu_p_to_c: vec![120],
            phy_c_to_p: vec![1],
            phy_p_to_c: vec![1],
            rtn_c_to_p: vec![3],
            rtn_p_to_c: vec![3],
        },
    );
    assert!(matches!(
        set_response,
        ReturnParameters::Raw { ref data } if data.first() == Some(&0)
    ));

    let removed = complete(&mut controller, Command::LeRemoveCig { cig_id: 7 });
    assert_eq!(
        removed,
        ReturnParameters::LeRemoveCig {
            status: 0,
            cig_id: 7,
        }
    );
    assert_eq!(
        ReturnParameters::parse(
            Command::LeRemoveCig { cig_id: 7 }.op_code(),
            &removed.to_bytes()
        )
        .unwrap(),
        removed
    );
    assert_eq!(
        complete(&mut controller, Command::LeRemoveCig { cig_id: 7 }),
        ReturnParameters::LeRemoveCig {
            status: 0x12,
            cig_id: 7,
        }
    );
}

#[test]
fn controller_information_queries_have_typed_serializable_payloads() {
    let mut c = Controller::new("C", addr("00:11:22:33:44:55"));
    let commands = [
        Command::ReadLocalVersionInformation,
        Command::ReadLocalSupportedCommands,
        Command::ReadLocalSupportedFeatures,
        Command::ReadClassOfDevice,
        Command::ReadSynchronousFlowControlEnable,
        Command::ReadLeHostSupport,
        Command::WriteAuthenticatedPayloadTimeout {
            connection_handle: 0x0ABC,
            authenticated_payload_timeout: 0x0100,
        },
        Command::ReadBufferSize,
        Command::LeReadBufferSizeV2,
        Command::LeReadLocalSupportedFeatures,
        Command::LeReadAllLocalSupportedFeatures,
        Command::LeReadSuggestedDefaultDataLength,
        Command::LeReadMaximumDataLength,
        Command::LeReadMaximumAdvertisingDataLength,
        Command::LeReadNumberOfSupportedAdvertisingSets,
        Command::LeReadAdvertisingPhysicalChannelTxPower,
        Command::LeReadFilterAcceptListSize,
        Command::LeReadSupportedStates,
        Command::LeReadResolvingListSize,
        Command::LeReadPhy {
            connection_handle: 0x0ABC,
        },
        Command::LeReadTransmitPower,
    ];

    for command in commands {
        c.handle_command(command);
        let packet = c.drain_host_events().remove(0);
        let reparsed = HciPacket::from_bytes(&packet.to_bytes()).unwrap();
        assert_eq!(reparsed, packet);
    }
}

#[test]
fn le_rand_is_deterministic_but_changes() {
    let mut c = Controller::new("C", addr("00:11:22:33:44:55"));
    c.handle_command(Command::LeRand);
    c.handle_command(Command::LeRand);
    let events = c.drain_host_events();
    assert_eq!(events.len(), 2);
    let value = |e: &HciPacket| match e {
        HciPacket::Event(Event::CommandComplete {
            return_parameters: ReturnParameters::Raw { data },
            ..
        }) => {
            assert_eq!(data.len(), 9); // status + 8 random bytes
            assert_eq!(data[0], 0);
            data[1..].to_vec()
        }
        other => panic!("expected Raw params, got {other:?}"),
    };
    // Successive calls yield different values (counter-backed).
    assert_ne!(value(&events[0]), value(&events[1]));
}

/// Give a controller one connection (handle 1) as a peripheral.
fn connected() -> (Controller, u16) {
    let mut c = Controller::new("C", addr("00:11:22:33:44:55"));
    c.connect_as_peripheral(addr("AA:BB:CC:DD:EE:FF"), 1);
    let _ = c.drain_host_events(); // discard the Connection Complete
    let handle = c.connections()[0].handle;
    (c, handle)
}

#[test]
fn le_set_data_length_reports_change_on_known_connection() {
    let (mut c, handle) = connected();
    c.handle_command(Command::LeSetDataLength {
        connection_handle: handle,
        tx_octets: 251,
        tx_time: 2120,
    });
    let events = c.drain_host_events();
    assert_eq!(events.len(), 2);

    // 1) Command Complete: status + connection handle.
    match &events[0] {
        HciPacket::Event(Event::CommandComplete {
            return_parameters: ReturnParameters::Raw { data },
            ..
        }) => {
            assert_eq!(data, &[0, handle as u8, (handle >> 8) as u8]);
        }
        other => panic!("expected Command Complete, got {other:?}"),
    }
    // 2) Data Length Change mirroring the requested TX limits onto RX.
    match &events[1] {
        HciPacket::Event(Event::LeMeta(LeMetaEvent::DataLengthChange {
            connection_handle,
            max_tx_octets,
            max_tx_time,
            max_rx_octets,
            max_rx_time,
        })) => {
            assert_eq!(*connection_handle, handle);
            assert_eq!(*max_tx_octets, 251);
            assert_eq!(*max_tx_time, 2120);
            assert_eq!(*max_rx_octets, 251);
            assert_eq!(*max_rx_time, 2120);
        }
        other => panic!("expected Data Length Change, got {other:?}"),
    }
}

#[test]
fn le_set_data_length_errors_on_unknown_connection() {
    let mut c = Controller::new("C", addr("00:11:22:33:44:55"));
    c.handle_command(Command::LeSetDataLength {
        connection_handle: 0x00FF,
        tx_octets: 251,
        tx_time: 2120,
    });
    let events = c.drain_host_events();
    assert_eq!(events.len(), 1); // only the error ack, no Data Length Change
    match &events[0] {
        HciPacket::Event(Event::CommandComplete {
            return_parameters: ReturnParameters::Raw { data },
            ..
        }) => assert_eq!(data[0], 0x02), // Unknown Connection Identifier
        other => panic!("expected Command Complete, got {other:?}"),
    }
}

#[test]
fn le_set_phy_reports_update_on_known_connection() {
    let (mut c, handle) = connected();
    c.handle_command(Command::LeSetPhy {
        connection_handle: handle,
        all_phys: 0,
        tx_phys: 0x02, // prefer LE 2M
        rx_phys: 0x01, // prefer LE 1M
        phy_options: 0,
    });
    let events = c.drain_host_events();
    assert_eq!(events.len(), 2);

    // 1) Command Status (LE_Set_PHY is acknowledged with a status).
    assert!(matches!(
        &events[0],
        HciPacket::Event(Event::CommandStatus { status: 0, .. })
    ));
    // 2) PHY Update Complete with the resolved PHYs (2M tx, 1M rx).
    match &events[1] {
        HciPacket::Event(Event::LeMeta(LeMetaEvent::PhyUpdateComplete {
            status,
            connection_handle,
            tx_phy,
            rx_phy,
        })) => {
            assert_eq!(*status, 0);
            assert_eq!(*connection_handle, handle);
            assert_eq!(*tx_phy, 2);
            assert_eq!(*rx_phy, 1);
        }
        other => panic!("expected PHY Update Complete, got {other:?}"),
    }
}

#[test]
fn le_set_phy_no_update_on_unknown_connection() {
    let mut c = Controller::new("C", addr("00:11:22:33:44:55"));
    c.handle_command(Command::LeSetPhy {
        connection_handle: 0x00FF,
        all_phys: 0,
        tx_phys: 1,
        rx_phys: 1,
        phy_options: 0,
    });
    let events = c.drain_host_events();
    assert_eq!(events.len(), 1); // Command Status only, no PHY Update Complete
    assert!(matches!(
        &events[0],
        HciPacket::Event(Event::CommandStatus { .. })
    ));
}
