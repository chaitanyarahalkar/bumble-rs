//! Slice-3 acceptance: an end-to-end LE advertising → scan → report scenario
//! across two controllers on a shared link, plus controller unit tests.
//!
//! There is no isolatable upstream controller test (Bumble tests the controller
//! through the full Device/host stack), so these are self-defined but exercise
//! the real packet flow through the `bumble-hci` codec.

use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_hci::{Command, Event, HciPacket, LeMetaEvent, ReturnParameters};

fn addr(s: &str) -> Address {
    Address::parse(s, AddressType::RANDOM_DEVICE).unwrap()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// One advertiser, one scanner: the scanner's host must receive an Advertising
/// Report carrying the advertiser's address and data.
#[test]
fn advertising_scan_report_end_to_end() {
    let mut link = LocalLink::new();
    let a = link.add_controller(Controller::new("A", addr("AA:AA:AA:AA:AA:AA")));
    let b = link.add_controller(Controller::new("B", addr("BB:BB:BB:BB:BB:BB")));

    let adv_address = addr("C4:F2:17:1A:1D:BB");
    let adv_data = unhex("0201060909426c7565"); // Flags + Complete Local Name "Blue"

    // Advertiser (A) is configured and enabled.
    link.handle_command(
        a,
        Command::LeSetRandomAddress {
            random_address: adv_address.clone(),
        },
    );
    link.handle_command(
        a,
        Command::LeSetAdvertisingData {
            advertising_data: adv_data.clone(),
        },
    );
    link.handle_command(
        a,
        Command::LeSetAdvertisingEnable {
            advertising_enable: 1,
        },
    );

    // Scanner (B) is enabled.
    link.handle_command(
        b,
        Command::LeSetScanEnable {
            le_scan_enable: 1,
            filter_duplicates: 0,
        },
    );

    // Every command was acknowledged with a Command Complete (status SUCCESS).
    let a_acks = link.drain_host_events(a);
    assert_eq!(a_acks.len(), 3);
    for ack in &a_acks {
        match ack {
            HciPacket::Event(Event::CommandComplete {
                return_parameters, ..
            }) => assert_eq!(return_parameters.status(), Some(0)),
            other => panic!("expected Command Complete, got {other:?}"),
        }
    }
    assert_eq!(link.drain_host_events(b).len(), 1); // scan-enable ack

    // Pump the link: A's advertising PDU reaches B.
    link.propagate_advertising();

    // B's host now has exactly one Advertising Report for A.
    let events = link.drain_host_events(b);
    assert_eq!(events.len(), 1);
    let reports = match &events[0] {
        HciPacket::Event(Event::LeMeta(LeMetaEvent::AdvertisingReport { reports })) => reports,
        other => panic!("expected an Advertising Report, got {other:?}"),
    };
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].address, adv_address);
    assert_eq!(reports[0].data, adv_data);

    // The report is a valid HCI packet: it round-trips through the codec.
    let bytes = events[0].to_bytes();
    assert_eq!(HciPacket::from_bytes(&bytes).unwrap(), events[0]);
}

/// A scanner that is not enabled must receive no report.
#[test]
fn no_report_when_scan_disabled() {
    let mut link = LocalLink::new();
    let a = link.add_controller(Controller::new("A", addr("AA:AA:AA:AA:AA:AA")));
    let b = link.add_controller(Controller::new("B", addr("BB:BB:BB:BB:BB:BB")));

    link.handle_command(
        a,
        Command::LeSetAdvertisingEnable {
            advertising_enable: 1,
        },
    );
    // B never enables scanning.
    let _ = link.drain_host_events(a);

    link.propagate_advertising();

    assert!(link.drain_host_events(b).is_empty());
}

/// Advertising can be disabled again, stopping reports.
#[test]
fn disabling_advertising_stops_reports() {
    let mut link = LocalLink::new();
    let a = link.add_controller(Controller::new("A", addr("AA:AA:AA:AA:AA:AA")));
    let b = link.add_controller(Controller::new("B", addr("BB:BB:BB:BB:BB:BB")));

    link.handle_command(
        a,
        Command::LeSetAdvertisingEnable {
            advertising_enable: 1,
        },
    );
    link.handle_command(
        b,
        Command::LeSetScanEnable {
            le_scan_enable: 1,
            filter_duplicates: 0,
        },
    );
    let _ = link.drain_host_events(a);
    let _ = link.drain_host_events(b);

    link.propagate_advertising();
    assert_eq!(link.drain_host_events(b).len(), 1);

    // Disable advertising; no further reports.
    link.handle_command(
        a,
        Command::LeSetAdvertisingEnable {
            advertising_enable: 0,
        },
    );
    let _ = link.drain_host_events(a);
    assert!(!link.controller(a).is_advertising());
    link.propagate_advertising();
    assert!(link.drain_host_events(b).is_empty());
}

/// Reset acknowledges with a status-only Command Complete and clears state.
#[test]
fn reset_acknowledges_and_clears_state() {
    let mut controller = Controller::new("C", addr("00:11:22:33:44:55"));
    controller.handle_command(Command::LeSetAdvertisingEnable {
        advertising_enable: 1,
    });
    let _ = controller.drain_host_events();
    assert!(controller.is_advertising());

    controller.handle_command(Command::Reset);
    assert!(!controller.is_advertising());

    let events = controller.drain_host_events();
    assert_eq!(events.len(), 1);
    match &events[0] {
        HciPacket::Event(Event::CommandComplete {
            command_opcode,
            return_parameters,
            ..
        }) => {
            assert_eq!(*command_opcode, bumble_hci::HCI_RESET_COMMAND);
            assert_eq!(*return_parameters, ReturnParameters::Status { status: 0 });
        }
        other => panic!("expected Command Complete, got {other:?}"),
    }
}
