//! End-to-end classic (BR/EDR) flows over the link: ACL connection
//! establishment, remote-name request, and remote-features request — driven by
//! simplified LMP PDUs (see `bumble_controller::lmp`). Classic connections are
//! addressed by public device address and routed with `LocalLink::pump_classic`.

use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_hci::{Command, Event, HciPacket};

fn pub_addr(s: &str) -> Address {
    Address::parse(s, AddressType::PUBLIC_DEVICE).unwrap()
}

fn create_connection(bd_addr: &Address) -> Command {
    Command::CreateConnection {
        bd_addr: bd_addr.clone(),
        packet_type: 0,
        page_scan_repetition_mode: 0,
        reserved: 0,
        clock_offset: 0,
        allow_role_switch: 1,
    }
}

/// Connection Complete's (status, handle, bd_addr), if present.
fn connection_complete(events: &[HciPacket]) -> Option<(u8, u16, Address)> {
    events.iter().find_map(|e| match e {
        HciPacket::Event(Event::ConnectionComplete {
            status,
            connection_handle,
            bd_addr,
            ..
        }) => Some((*status, *connection_handle, bd_addr.clone())),
        _ => None,
    })
}

#[test]
fn classic_connection_establishment_end_to_end() {
    for accepted_role in [
        bumble_controller::ROLE_CENTRAL,
        bumble_controller::ROLE_PERIPHERAL,
    ] {
        let mut link = LocalLink::new();
        let central_addr = pub_addr("11:11:11:11:11:11");
        let peripheral_addr = pub_addr("22:22:22:22:22:22");
        let central = link.add_controller(Controller::new("C", central_addr.clone()));
        let peripheral = link.add_controller(Controller::new("P", peripheral_addr.clone()));

        link.handle_command(central, create_connection(&peripheral_addr));
        assert!(link.drain_host_events(central).iter().any(|e| matches!(
            e,
            HciPacket::Event(Event::CommandStatus { command_opcode, status: 0, .. })
                if *command_opcode == bumble_hci::HCI_CREATE_CONNECTION_COMMAND
        )));

        link.pump_classic();
        let req = link.drain_host_events(peripheral);
        assert!(req.iter().any(|e| matches!(
            e,
            HciPacket::Event(Event::ConnectionRequest { bd_addr, .. }) if *bd_addr == central_addr
        )));

        link.handle_command(
            peripheral,
            Command::AcceptConnectionRequest {
                bd_addr: central_addr.clone(),
                role: accepted_role,
            },
        );
        link.pump_classic();

        let (pstatus, phandle, ppeer) =
            connection_complete(&link.drain_host_events(peripheral)).expect("acceptor completes");
        assert_eq!((pstatus, ppeer), (0, central_addr));
        assert_ne!(phandle, 0);

        let (cstatus, chandle, cpeer) =
            connection_complete(&link.drain_host_events(central)).expect("initiator completes");
        assert_eq!((cstatus, cpeer), (0, peripheral_addr));
        assert_ne!(chandle, 0);

        let initiating_role = if accepted_role == bumble_controller::ROLE_CENTRAL {
            bumble_controller::ROLE_PERIPHERAL
        } else {
            bumble_controller::ROLE_CENTRAL
        };
        assert_eq!(
            link.controller(central).classic_connections()[0].role,
            initiating_role
        );
        assert_eq!(
            link.controller(peripheral).classic_connections()[0].role,
            accepted_role
        );
    }
}

#[test]
fn classic_role_switch_can_be_rejected_during_accept() {
    let mut link = LocalLink::new();
    let initiator_address = pub_addr("11:11:11:11:11:11");
    let acceptor_address = pub_addr("22:22:22:22:22:22");
    let initiator = link.add_controller(Controller::new("I", initiator_address.clone()));
    let acceptor = link.add_controller(Controller::new("A", acceptor_address.clone()));

    let mut command = create_connection(&acceptor_address);
    if let Command::CreateConnection {
        allow_role_switch, ..
    } = &mut command
    {
        *allow_role_switch = 0;
    }
    link.handle_command(initiator, command);
    link.drain_host_events(initiator);
    link.pump_classic();
    link.drain_host_events(acceptor);
    link.handle_command(
        acceptor,
        Command::AcceptConnectionRequest {
            bd_addr: initiator_address.clone(),
            role: bumble_controller::ROLE_CENTRAL,
        },
    );
    link.pump_classic();

    for controller in [initiator, acceptor] {
        assert!(link
            .drain_host_events(controller)
            .iter()
            .any(|event| matches!(
                event,
                HciPacket::Event(Event::ConnectionComplete {
                    status: 0x21,
                    connection_handle: 0,
                    ..
                })
            )));
        assert!(link.controller(controller).classic_connections().is_empty());
    }
}

#[test]
fn switch_role_changes_both_established_endpoints() {
    let mut link = LocalLink::new();
    let initiator_address = pub_addr("11:11:11:11:11:11");
    let acceptor_address = pub_addr("22:22:22:22:22:22");
    let initiator = link.add_controller(Controller::new("I", initiator_address.clone()));
    let acceptor = link.add_controller(Controller::new("A", acceptor_address.clone()));

    link.handle_command(initiator, create_connection(&acceptor_address));
    link.drain_host_events(initiator);
    link.pump_classic();
    link.drain_host_events(acceptor);
    link.handle_command(
        acceptor,
        Command::AcceptConnectionRequest {
            bd_addr: initiator_address,
            role: bumble_controller::ROLE_PERIPHERAL,
        },
    );
    link.pump_classic();
    link.drain_host_events(initiator);
    link.drain_host_events(acceptor);

    link.handle_command(
        initiator,
        Command::SwitchRole {
            bd_addr: acceptor_address,
            role: bumble_controller::ROLE_PERIPHERAL,
        },
    );
    link.pump_classic();

    assert_eq!(
        link.controller(initiator).classic_connections()[0].role,
        bumble_controller::ROLE_PERIPHERAL
    );
    assert_eq!(
        link.controller(acceptor).classic_connections()[0].role,
        bumble_controller::ROLE_CENTRAL
    );
    for controller in [initiator, acceptor] {
        assert!(link
            .drain_host_events(controller)
            .iter()
            .any(|event| matches!(event, HciPacket::Event(Event::RoleChange { status: 0, .. }))));
    }
}

#[test]
fn classic_remote_name_request() {
    let mut link = LocalLink::new();
    let central = link.add_controller(Controller::new("C", pub_addr("11:11:11:11:11:11")));
    let _peripheral =
        link.add_controller(Controller::new("Peripheral", pub_addr("22:22:22:22:22:22")));

    // No connection needed for a name request (it is a paging operation).
    link.handle_command(
        central,
        Command::RemoteNameRequest {
            bd_addr: pub_addr("22:22:22:22:22:22"),
            page_scan_repetition_mode: 0,
            reserved: 0,
            clock_offset: 0,
        },
    );
    let _ = link.drain_host_events(central); // Command Status
    link.pump_classic();

    let done = link.drain_host_events(central);
    let name = done
        .iter()
        .find_map(|e| match e {
            HciPacket::Event(Event::RemoteNameRequestComplete { remote_name, .. }) => {
                Some(remote_name.to_vec())
            }
            _ => None,
        })
        .expect("central must receive Remote Name Request Complete");
    assert_eq!(&name[..10], b"Peripheral");
    assert!(name[10..].iter().all(|&b| b == 0));
    assert_eq!(name.len(), 248);
}

#[test]
fn classic_read_remote_supported_features() {
    let mut link = LocalLink::new();
    let central_addr = pub_addr("11:11:11:11:11:11");
    let peripheral_addr = pub_addr("22:22:22:22:22:22");
    let central = link.add_controller(Controller::new("C", central_addr.clone()));
    let peripheral = link.add_controller(Controller::new("P", peripheral_addr.clone()));

    // Establish a classic connection first.
    link.handle_command(central, create_connection(&peripheral_addr));
    let _ = link.drain_host_events(central);
    link.pump_classic();
    let _ = link.drain_host_events(peripheral);
    link.handle_command(
        peripheral,
        Command::AcceptConnectionRequest {
            bd_addr: central_addr.clone(),
            role: 0,
        },
    );
    let _ = link.drain_host_events(peripheral);
    link.pump_classic();
    let (_, chandle, _) =
        connection_complete(&link.drain_host_events(central)).expect("central completes");

    // Read the peer's LMP features over the established connection.
    link.handle_command(
        central,
        Command::ReadRemoteSupportedFeatures {
            connection_handle: chandle,
        },
    );
    let _ = link.drain_host_events(central); // Command Status
    link.pump_classic();

    let done = link.drain_host_events(central);
    assert!(
        done.iter().any(|e| matches!(
            e,
            HciPacket::Event(Event::ReadRemoteSupportedFeaturesComplete {
                status: 0,
                connection_handle,
                lmp_features,
            }) if *connection_handle == chandle
                && *lmp_features == [0, 0, 0, 0, 0x60, 0, 0, 0x80]
        )),
        "expected Read Remote Supported Features Complete, got {done:?}"
    );

    link.handle_command(
        central,
        Command::ReadRemoteExtendedFeatures {
            connection_handle: chandle,
            page_number: 2,
        },
    );
    let status = link.drain_host_events(central);
    assert!(status.iter().any(|event| matches!(
        event,
        HciPacket::Event(Event::CommandStatus {
            status: 0,
            command_opcode,
            ..
        }) if *command_opcode == bumble_hci::HCI_READ_REMOTE_EXTENDED_FEATURES_COMMAND
    )));
    link.pump_classic();
    let extended = link.drain_host_events(central);
    assert!(extended.iter().any(|event| matches!(
        event,
        HciPacket::Event(Event::ReadRemoteExtendedFeaturesComplete {
            status: 0,
            connection_handle,
            page_number: 2,
            maximum_page_number: 3,
            extended_lmp_features,
        }) if *connection_handle == chandle && *extended_lmp_features == [0; 8]
    )));
}
