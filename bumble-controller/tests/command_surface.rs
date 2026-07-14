//! The software controller gives every command upstream `controller.py` handles
//! a well-formed reply of the matching HCI shape (Command Complete vs Command
//! Status), and reports the spec-correct "Unknown HCI Command" for anything
//! upstream also doesn't handle. See `bumble_controller::command_surface`.

use bumble::{Address, AddressType};
use bumble_controller::Controller;
use bumble_hci::{Command, Event, HciPacket, ReturnParameters};

fn ctrl() -> Controller {
    Controller::new(
        "C",
        Address::parse("00:11:22:33:44:55", AddressType::PUBLIC_DEVICE).unwrap(),
    )
}

/// The single queued event, as (is_command_complete, status).
fn one_reply(c: &mut Controller) -> HciPacket {
    let mut ev = c.drain_host_events();
    assert_eq!(ev.len(), 1, "expected exactly one reply: {ev:?}");
    ev.remove(0)
}

#[test]
fn status_only_command_is_accepted() {
    // LE_Set_Event_Mask (0x2001): a config command upstream stores + SUCCESSes.
    let mut c = ctrl();
    c.handle_command(Command::LeSetEventMask {
        le_event_mask: [0xFF; 8],
    });
    match one_reply(&mut c) {
        HciPacket::Event(Event::CommandComplete {
            command_opcode,
            return_parameters,
            ..
        }) => {
            assert_eq!(command_opcode, 0x2001);
            assert_eq!(return_parameters, ReturnParameters::Status { status: 0 });
        }
        other => panic!("expected Command Complete SUCCESS, got {other:?}"),
    }
}

#[test]
fn data_command_is_acknowledged() {
    // Read_Local_Version_Information (0x1001): a data read the sim doesn't model;
    // it is acknowledged SUCCESS (documented stub), never rejected.
    let mut c = ctrl();
    c.handle_command(Command::ReadLocalVersionInformation);
    match one_reply(&mut c) {
        HciPacket::Event(Event::CommandComplete {
            command_opcode,
            return_parameters,
            ..
        }) => {
            assert_eq!(command_opcode, 0x1001);
            assert_eq!(return_parameters.status(), Some(0));
        }
        other => panic!("expected Command Complete, got {other:?}"),
    }
}

#[test]
fn status_command_gets_command_status() {
    // Read_Remote_Extended_Features (0x041C) is a Status-category command. Its
    // functional handler rejects this absent handle, but still has to use the
    // Command Status response shape rather than Command Complete.
    let mut c = ctrl();
    c.handle_command(Command::ReadRemoteExtendedFeatures {
        connection_handle: 0x0001,
        page_number: 0,
    });
    match one_reply(&mut c) {
        HciPacket::Event(Event::CommandStatus {
            status,
            command_opcode,
            ..
        }) => {
            assert_eq!(status, 0x02);
            assert_eq!(command_opcode, 0x041C);
        }
        other => panic!("expected Command Status, got {other:?}"),
    }
}

#[test]
fn command_outside_the_surface_is_unknown() {
    // LE_Connection_Update (0x2013) is NOT handled by upstream controller.py, so
    // the honest reply is "Unknown HCI Command" (0x01), not a fake SUCCESS.
    let mut c = ctrl();
    c.handle_command(Command::LeConnectionUpdate {
        connection_handle: 1,
        connection_interval_min: 24,
        connection_interval_max: 40,
        max_latency: 0,
        supervision_timeout: 42,
        min_ce_length: 0,
        max_ce_length: 0,
    });
    match one_reply(&mut c) {
        HciPacket::Event(Event::CommandComplete {
            command_opcode,
            return_parameters,
            ..
        }) => {
            assert_eq!(command_opcode, 0x2013);
            assert_eq!(return_parameters, ReturnParameters::Status { status: 0x01 });
        }
        other => panic!("expected Unknown-Command reply, got {other:?}"),
    }
}
