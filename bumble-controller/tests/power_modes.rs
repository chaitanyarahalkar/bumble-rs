use bumble::{Address, AddressType};
use bumble_controller::Controller;
use bumble_hci::{Command, Event, HciPacket, LeMetaEvent};

fn controller() -> Controller {
    Controller::new(
        "power-modes",
        Address::parse("00:00:00:00:00:01/P", AddressType::PUBLIC_DEVICE).unwrap(),
    )
}

#[test]
fn sniff_and_active_modes_emit_upstream_mode_changes() {
    let mut controller = controller();
    let sniff = Command::SniffMode {
        connection_handle: 0x0123,
        sniff_max_interval: 2,
        sniff_min_interval: 2,
        sniff_attempt: 2,
        sniff_timeout: 2,
    };
    let sniff_opcode = sniff.op_code();
    controller.handle_command(sniff);
    assert_eq!(
        controller.drain_host_events(),
        vec![
            HciPacket::Event(Event::CommandStatus {
                status: 0,
                num_hci_command_packets: 1,
                command_opcode: sniff_opcode,
            }),
            HciPacket::Event(Event::ModeChange {
                status: 0,
                connection_handle: 0x0123,
                current_mode: 0x02,
                interval: 2,
            }),
        ]
    );

    let active = Command::ExitSniffMode {
        connection_handle: 0x0123,
    };
    let active_opcode = active.op_code();
    controller.handle_command(active);
    assert_eq!(
        controller.drain_host_events(),
        vec![
            HciPacket::Event(Event::CommandStatus {
                status: 0,
                num_hci_command_packets: 1,
                command_opcode: active_opcode,
            }),
            HciPacket::Event(Event::ModeChange {
                status: 0,
                connection_handle: 0x0123,
                current_mode: 0x00,
                interval: 2,
            }),
        ]
    );
}

#[test]
fn subrate_validation_and_change_match_upstream() {
    let mut controller = controller();
    controller.handle_command(Command::LeSetDefaultSubrate {
        subrate_min: 3,
        subrate_max: 2,
        max_latency: 2,
        continuation_number: 1,
        supervision_timeout: 2,
    });
    let invalid_default = controller.drain_host_events();
    assert_eq!(invalid_default.len(), 1);
    assert!(matches!(
        &invalid_default[0],
        HciPacket::Event(Event::CommandComplete {
            return_parameters,
            ..
        }) if return_parameters.status() == Some(0x12)
    ));

    let request = Command::LeSubrateRequest {
        connection_handle: 0x0123,
        subrate_min: 2,
        subrate_max: 2,
        max_latency: 2,
        continuation_number: 1,
        supervision_timeout: 2,
    };
    let request_opcode = request.op_code();
    controller.handle_command(request);
    assert_eq!(
        controller.drain_host_events(),
        vec![
            HciPacket::Event(Event::CommandStatus {
                status: 0,
                num_hci_command_packets: 1,
                command_opcode: request_opcode,
            }),
            HciPacket::Event(Event::LeMeta(LeMetaEvent::SubrateChange {
                status: 0,
                connection_handle: 0x0123,
                subrate_factor: 2,
                peripheral_latency: 2,
                continuation_number: 1,
                supervision_timeout: 2,
            })),
        ]
    );

    let invalid = Command::LeSubrateRequest {
        connection_handle: 0x0123,
        subrate_min: 2,
        subrate_max: 2,
        max_latency: 251,
        continuation_number: 1,
        supervision_timeout: 2,
    };
    let invalid_opcode = invalid.op_code();
    controller.handle_command(invalid);
    assert_eq!(
        controller.drain_host_events(),
        vec![HciPacket::Event(Event::CommandStatus {
            status: 0x12,
            num_hci_command_packets: 1,
            command_opcode: invalid_opcode,
        })]
    );
}
