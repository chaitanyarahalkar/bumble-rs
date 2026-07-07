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
        allow_role_switch: 0,
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
    let mut link = LocalLink::new();
    let central_addr = pub_addr("11:11:11:11:11:11");
    let peripheral_addr = pub_addr("22:22:22:22:22:22");
    let central = link.add_controller(Controller::new("C", central_addr.clone()));
    let peripheral = link.add_controller(Controller::new("P", peripheral_addr.clone()));

    // Central pages the peripheral.
    link.handle_command(central, create_connection(&peripheral_addr));
    assert!(link.drain_host_events(central).iter().any(|e| matches!(
        e,
        HciPacket::Event(Event::CommandStatus { command_opcode, status: 0, .. })
            if *command_opcode == bumble_hci::HCI_CREATE_CONNECTION_COMMAND
    )));

    // Peripheral receives a Connection Request naming the central.
    link.pump_classic();
    let req = link.drain_host_events(peripheral);
    assert!(req.iter().any(|e| matches!(
        e,
        HciPacket::Event(Event::ConnectionRequest { bd_addr, .. }) if *bd_addr == central_addr
    )));

    // Peripheral accepts: it completes, then the central completes.
    link.handle_command(
        peripheral,
        Command::AcceptConnectionRequest {
            bd_addr: central_addr.clone(),
            role: 0,
        },
    );
    let (pstatus, _phandle, ppeer) =
        connection_complete(&link.drain_host_events(peripheral)).expect("peripheral completes");
    assert_eq!(pstatus, 0);
    assert_eq!(ppeer, central_addr);

    link.pump_classic();
    let (cstatus, chandle, cpeer) =
        connection_complete(&link.drain_host_events(central)).expect("central completes");
    assert_eq!(cstatus, 0);
    assert_eq!(cpeer, peripheral_addr);
    assert!(chandle != 0);
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
                ..
            }) if *connection_handle == chandle
        )),
        "expected Read Remote Supported Features Complete, got {done:?}"
    );
}
