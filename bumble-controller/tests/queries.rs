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
fn controller_information_queries_have_typed_serializable_payloads() {
    let mut c = Controller::new("C", addr("00:11:22:33:44:55"));
    let commands = [
        Command::ReadLocalVersionInformation,
        Command::ReadLocalSupportedCommands,
        Command::ReadLocalSupportedFeatures,
        Command::ReadBufferSize,
        Command::LeReadBufferSizeV2,
        Command::LeReadLocalSupportedFeatures,
        Command::LeReadSuggestedDefaultDataLength,
        Command::LeReadMaximumDataLength,
        Command::LeReadMaximumAdvertisingDataLength,
        Command::LeReadNumberOfSupportedAdvertisingSets,
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
