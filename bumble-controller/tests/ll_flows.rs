//! End-to-end LL-control-PDU flows across two connected controllers: LE
//! encryption start and remote-features exchange. These mirror upstream
//! `controller.py`'s behavior (an `EncReq` exchange that encrypts both sides; a
//! `FeatureReq`/`FeatureRsp` round trip that completes with a Read Remote
//! Features event) — driven over the in-process `LocalLink`.

use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_hci::{Command, Event, HciPacket, LeMetaEvent};

fn addr(s: &str) -> Address {
    Address::parse(s, AddressType::RANDOM_DEVICE).unwrap()
}

fn create_connection(peer: &Address) -> Command {
    Command::LeCreateConnection {
        le_scan_interval: 16,
        le_scan_window: 16,
        initiator_filter_policy: 0,
        peer_address_type: 1,
        peer_address: peer.clone(),
        own_address_type: 1,
        connection_interval_min: 24,
        connection_interval_max: 40,
        max_latency: 0,
        supervision_timeout: 42,
        min_ce_length: 0,
        max_ce_length: 0,
    }
}

/// Two connected controllers `(link, central, peripheral)` with setup events drained.
fn connected() -> (LocalLink, usize, usize) {
    let mut link = LocalLink::new();
    let central = link.add_controller(Controller::new("C", addr("00:00:00:00:00:01")));
    let peripheral = link.add_controller(Controller::new("P", addr("00:00:00:00:00:02")));
    let peripheral_addr = addr("C4:F2:17:1A:1D:BB");

    link.handle_command(
        peripheral,
        Command::LeSetRandomAddress {
            random_address: peripheral_addr.clone(),
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
            random_address: addr("C4:F2:17:1A:1D:AA"),
        },
    );
    link.handle_command(central, create_connection(&peripheral_addr));
    link.establish_connections();
    let _ = link.drain_host_events(central);
    let _ = link.drain_host_events(peripheral);
    (link, central, peripheral)
}

fn handle_of(link: &LocalLink, id: usize) -> u16 {
    link.controller(id).connections()[0].handle
}

fn has_encryption_change(events: &[HciPacket]) -> bool {
    events.iter().any(|e| {
        matches!(
            e,
            HciPacket::Event(Event::EncryptionChange {
                status: 0,
                encryption_enabled: 1,
                ..
            })
        )
    })
}

#[test]
fn le_encryption_start_encrypts_both_sides() {
    let (mut link, central, peripheral) = connected();
    let handle = handle_of(&link, central);

    link.handle_command(
        central,
        Command::LeEnableEncryption {
            connection_handle: handle,
            random_number: [1, 2, 3, 4, 5, 6, 7, 8],
            encrypted_diversifier: 0x1234,
            long_term_key: [0xAB; 16],
        },
    );

    // Central: Command Status for LE_Enable_Encryption, then its own Encryption Change.
    let central_events = link.drain_host_events(central);
    assert!(central_events.iter().any(|e| matches!(
        e,
        HciPacket::Event(Event::CommandStatus { command_opcode, status: 0, .. })
            if *command_opcode == bumble_hci::HCI_LE_ENABLE_ENCRYPTION_COMMAND
    )));
    assert!(
        has_encryption_change(&central_events),
        "central must encrypt"
    );

    // The EncReq reaches the peripheral, which encrypts too.
    link.pump_ll();
    assert!(
        has_encryption_change(&link.drain_host_events(peripheral)),
        "peripheral must encrypt after receiving EncReq"
    );
}

#[test]
fn le_read_remote_features_completes() {
    let (mut link, central, peripheral) = connected();
    let handle = handle_of(&link, central);

    link.handle_command(
        central,
        Command::LeReadRemoteFeatures {
            connection_handle: handle,
        },
    );
    // Command Status first; no completion yet.
    let status = link.drain_host_events(central);
    assert!(status.iter().any(|e| matches!(
        e,
        HciPacket::Event(Event::CommandStatus { command_opcode, .. })
            if *command_opcode == bumble_hci::HCI_LE_READ_REMOTE_FEATURES_COMMAND
    )));

    // FeatureReq -> peripheral -> FeatureRsp -> central completes.
    link.pump_ll();

    let done = link.drain_host_events(central);
    assert!(
        done.iter().any(|e| matches!(
            e,
            HciPacket::Event(Event::LeMeta(LeMetaEvent::ReadRemoteFeaturesComplete {
                status: 0,
                connection_handle,
                ..
            })) if *connection_handle == handle
        )),
        "central must receive Read Remote Features Complete, got {done:?}"
    );
    // The peripheral saw only the request; it does not raise a host event.
    assert!(link.drain_host_events(peripheral).is_empty());
}

#[test]
fn encryption_on_unknown_handle_is_rejected() {
    let (mut link, central, _peripheral) = connected();
    link.handle_command(
        central,
        Command::LeEnableEncryption {
            connection_handle: 0x00FF,
            random_number: [0; 8],
            encrypted_diversifier: 0,
            long_term_key: [0; 16],
        },
    );
    let events = link.drain_host_events(central);
    // A Command Status with the "invalid parameters" error, and no Encryption Change.
    assert!(events.iter().any(|e| matches!(
        e,
        HciPacket::Event(Event::CommandStatus { status: 0x12, .. })
    )));
    assert!(!has_encryption_change(&events));
}
