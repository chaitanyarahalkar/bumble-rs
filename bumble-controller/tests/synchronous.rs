//! Classic SCO/eSCO connection establishment, data routing, rejection, and
//! disconnect over the in-process LMP link.

use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink, LINK_TYPE_ESCO};
use bumble_hci::{CodingFormat, Command, Event, HciPacket};

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::PUBLIC_DEVICE).unwrap()
}

fn create_acl(peer: Address) -> Command {
    Command::CreateConnection {
        bd_addr: peer,
        packet_type: 0,
        page_scan_repetition_mode: 0,
        reserved: 0,
        clock_offset: 0,
        allow_role_switch: 0,
    }
}

fn coding(coding_format: u8) -> CodingFormat {
    CodingFormat {
        coding_format,
        company_id: 0,
        vendor_specific_codec_id: 0,
    }
}

fn setup_sync(acl_handle: u16) -> Command {
    Command::EnhancedSetupSynchronousConnection {
        connection_handle: acl_handle,
        transmit_bandwidth: 8000,
        receive_bandwidth: 8000,
        transmit_coding_format: coding(2),
        receive_coding_format: coding(2),
        transmit_codec_frame_size: 60,
        receive_codec_frame_size: 60,
        input_bandwidth: 16000,
        output_bandwidth: 16000,
        input_coding_format: coding(4),
        output_coding_format: coding(4),
        input_coded_data_size: 16,
        output_coded_data_size: 16,
        input_pcm_data_format: 0,
        output_pcm_data_format: 0,
        input_pcm_sample_payload_msb_position: 0,
        output_pcm_sample_payload_msb_position: 0,
        input_data_path: 0,
        output_data_path: 0,
        input_transport_unit_size: 0,
        output_transport_unit_size: 0,
        max_latency: 7,
        packet_type: 0,
        retransmission_effort: 1,
    }
}

fn accept_sync(peer: Address) -> Command {
    Command::EnhancedAcceptSynchronousConnectionRequest {
        bd_addr: peer,
        transmit_bandwidth: 8000,
        receive_bandwidth: 8000,
        transmit_coding_format: coding(2),
        receive_coding_format: coding(2),
        transmit_codec_frame_size: 60,
        receive_codec_frame_size: 60,
        input_bandwidth: 16000,
        output_bandwidth: 16000,
        input_coding_format: coding(4),
        output_coding_format: coding(4),
        input_coded_data_size: 16,
        output_coded_data_size: 16,
        input_pcm_data_format: 0,
        output_pcm_data_format: 0,
        input_pcm_sample_payload_msb_position: 0,
        output_pcm_sample_payload_msb_position: 0,
        input_data_path: 0,
        output_data_path: 0,
        input_transport_unit_size: 0,
        output_transport_unit_size: 0,
        max_latency: 7,
        packet_type: 0,
        retransmission_effort: 1,
    }
}

fn connection_complete(events: &[HciPacket]) -> Option<u16> {
    events.iter().find_map(|packet| match packet {
        HciPacket::Event(Event::ConnectionComplete {
            status: 0,
            connection_handle,
            ..
        }) => Some(*connection_handle),
        _ => None,
    })
}

fn synchronous_complete(events: &[HciPacket]) -> Option<(u8, u16)> {
    events.iter().find_map(|packet| match packet {
        HciPacket::Event(Event::SynchronousConnectionComplete {
            status,
            connection_handle,
            ..
        }) => Some((*status, *connection_handle)),
        _ => None,
    })
}

fn establish_acl(
    link: &mut LocalLink,
    central: usize,
    peripheral: usize,
    central_address: &Address,
    peripheral_address: &Address,
) -> (u16, u16) {
    link.handle_command(central, create_acl(peripheral_address.clone()));
    link.drain_host_events(central);
    link.pump_classic();
    link.drain_host_events(peripheral);
    link.handle_command(
        peripheral,
        Command::AcceptConnectionRequest {
            bd_addr: central_address.clone(),
            role: bumble_controller::ROLE_PERIPHERAL,
        },
    );
    let peripheral_handle =
        connection_complete(&link.drain_host_events(peripheral)).expect("peripheral ACL");
    link.pump_classic();
    let central_handle =
        connection_complete(&link.drain_host_events(central)).expect("central ACL");
    (central_handle, peripheral_handle)
}

#[test]
fn enhanced_synchronous_connection_data_and_disconnect() {
    let mut link = LocalLink::new();
    let central_address = address("11:11:11:11:11:11");
    let peripheral_address = address("22:22:22:22:22:22");
    let central = link.add_controller(Controller::new("HF", central_address.clone()));
    let peripheral = link.add_controller(Controller::new("AG", peripheral_address.clone()));
    let (central_acl, _peripheral_acl) = establish_acl(
        &mut link,
        central,
        peripheral,
        &central_address,
        &peripheral_address,
    );

    link.handle_command(central, setup_sync(central_acl));
    let status = link.drain_host_events(central);
    assert!(status.iter().any(|packet| matches!(
        packet,
        HciPacket::Event(Event::CommandStatus { status: 0, command_opcode, .. })
            if *command_opcode == bumble_hci::HCI_ENHANCED_SETUP_SYNCHRONOUS_CONNECTION_COMMAND
    )));
    link.pump_classic();
    let request = link.drain_host_events(peripheral);
    assert!(request.iter().any(|packet| matches!(
        packet,
        HciPacket::Event(Event::ConnectionRequest { bd_addr, link_type, .. })
            if *bd_addr == central_address && *link_type == LINK_TYPE_ESCO
    )));

    link.handle_command(peripheral, accept_sync(central_address.clone()));
    let peripheral_events = link.drain_host_events(peripheral);
    let (status, peripheral_sync) = synchronous_complete(&peripheral_events).unwrap();
    assert_eq!(status, 0);
    link.pump_classic();
    let (status, central_sync) = synchronous_complete(&link.drain_host_events(central)).unwrap();
    assert_eq!(status, 0);

    assert!(link.send_synchronous_data(central, central_sync, 0, b"cvsd-audio"));
    let received = link.drain_host_events(peripheral);
    assert!(received.iter().any(|packet| matches!(
        packet,
        HciPacket::SyncData(data)
            if data.connection_handle == peripheral_sync
                && data.packet_status == 0
                && data.data == b"cvsd-audio"
    )));
    assert!(link.send_synchronous_data(peripheral, peripheral_sync, 1, b"return"));
    let received = link.drain_host_events(central);
    assert!(received.iter().any(|packet| matches!(
        packet,
        HciPacket::SyncData(data)
            if data.connection_handle == central_sync
                && data.packet_status == 1
                && data.data == b"return"
    )));

    assert!(link.disconnect(central, central_sync, 0x13));
    let central_done = link.drain_host_events(central);
    let peripheral_done = link.drain_host_events(peripheral);
    assert!(central_done.iter().any(|packet| matches!(
        packet,
        HciPacket::Event(Event::DisconnectionComplete { connection_handle, .. })
            if *connection_handle == central_sync
    )));
    assert!(peripheral_done.iter().any(|packet| matches!(
        packet,
        HciPacket::Event(Event::DisconnectionComplete { connection_handle, .. })
            if *connection_handle == peripheral_sync
    )));
    assert!(link
        .controller(central)
        .synchronous_connections()
        .is_empty());
    assert!(link
        .controller(peripheral)
        .synchronous_connections()
        .is_empty());
    assert_eq!(link.controller(central).classic_connections().len(), 1);
    assert_eq!(link.controller(peripheral).classic_connections().len(), 1);
}

#[test]
fn synchronous_connection_can_be_rejected() {
    let mut link = LocalLink::new();
    let central_address = address("11:11:11:11:11:11");
    let peripheral_address = address("22:22:22:22:22:22");
    let central = link.add_controller(Controller::new("HF", central_address.clone()));
    let peripheral = link.add_controller(Controller::new("AG", peripheral_address.clone()));
    let (central_acl, _) = establish_acl(
        &mut link,
        central,
        peripheral,
        &central_address,
        &peripheral_address,
    );
    link.handle_command(central, setup_sync(central_acl));
    link.drain_host_events(central);
    link.pump_classic();
    link.drain_host_events(peripheral);
    link.handle_command(
        peripheral,
        Command::RejectSynchronousConnectionRequest {
            bd_addr: central_address,
            reason: 0x0f,
        },
    );
    link.drain_host_events(peripheral);
    link.pump_classic();
    assert_eq!(
        synchronous_complete(&link.drain_host_events(central)),
        Some((0x0f, 0))
    );
}

#[test]
fn disconnecting_classic_acl_cascades_to_synchronous_children() {
    let mut link = LocalLink::new();
    let central_address = address("11:11:11:11:11:11");
    let peripheral_address = address("22:22:22:22:22:22");
    let central = link.add_controller(Controller::new("HF", central_address.clone()));
    let peripheral = link.add_controller(Controller::new("AG", peripheral_address.clone()));
    let (central_acl, peripheral_acl) = establish_acl(
        &mut link,
        central,
        peripheral,
        &central_address,
        &peripheral_address,
    );

    link.handle_command(central, setup_sync(central_acl));
    link.drain_host_events(central);
    link.pump_classic();
    link.drain_host_events(peripheral);
    link.handle_command(peripheral, accept_sync(central_address));
    let (_, peripheral_sync) = synchronous_complete(&link.drain_host_events(peripheral)).unwrap();
    link.pump_classic();
    let (_, central_sync) = synchronous_complete(&link.drain_host_events(central)).unwrap();

    assert!(link.disconnect(central, central_acl, 0x13));
    let central_done = link.drain_host_events(central);
    let peripheral_done = link.drain_host_events(peripheral);
    for (events, acl_handle, sync_handle) in [
        (&central_done, central_acl, central_sync),
        (&peripheral_done, peripheral_acl, peripheral_sync),
    ] {
        assert!(events.iter().any(|packet| matches!(
            packet,
            HciPacket::Event(Event::DisconnectionComplete { connection_handle, .. })
                if *connection_handle == acl_handle
        )));
        assert!(events.iter().any(|packet| matches!(
            packet,
            HciPacket::Event(Event::DisconnectionComplete { connection_handle, .. })
                if *connection_handle == sync_handle
        )));
    }
    assert!(link.controller(central).classic_connections().is_empty());
    assert!(link
        .controller(central)
        .synchronous_connections()
        .is_empty());
    assert!(link.controller(peripheral).classic_connections().is_empty());
    assert!(link
        .controller(peripheral)
        .synchronous_connections()
        .is_empty());
}
