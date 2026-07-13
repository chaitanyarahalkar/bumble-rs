use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_crypto::EccKey;
use bumble_hci::Command;
use bumble_host::{pump, Device};
use bumble_smp::{
    AcceptAllDelegate, IoCapability, KeyDistribution, LegacyPairingSession, PairingCapabilities,
    PairingConfig, PairingRole, PairingState, ScPairingSession, ScPairingState, SmpPdu, SMP_CID,
};

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

fn connect(link: &mut LocalLink, central: usize, peripheral: usize) {
    let central_address = address("C4:F2:17:1A:1D:AA");
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
            random_address: central_address,
        },
    );
    link.handle_command(
        central,
        Command::LeCreateConnection {
            le_scan_interval: 16,
            le_scan_window: 16,
            initiator_filter_policy: 0,
            peer_address_type: 1,
            peer_address: peripheral_address,
            own_address_type: 1,
            connection_interval_min: 24,
            connection_interval_max: 40,
            max_latency: 0,
            supervision_timeout: 42,
            min_ce_length: 0,
            max_ce_length: 0,
        },
    );
    link.establish_connections();
}

fn config() -> PairingConfig {
    PairingConfig {
        secure_connections: false,
        mitm: false,
        bonding: true,
        capabilities: PairingCapabilities {
            io_capability: IoCapability::NoInputNoOutput,
            local_initiator_key_distribution: KeyDistribution::DEFAULT,
            local_responder_key_distribution: KeyDistribution::DEFAULT,
            maximum_encryption_key_size: 16,
        },
        identity_address_type: None,
        oob: None,
    }
}

fn drive_sessions(
    link: &mut LocalLink,
    devices: &mut [Device; 2],
    sessions: &mut [LegacyPairingSession; 2],
) {
    for _ in 0..100 {
        let mut progress = false;
        for index in 0..2 {
            for pdu in sessions[index].drain_outbound() {
                assert!(devices[index].send_l2cap(link, SMP_CID, &pdu.to_bytes()));
                progress = true;
            }
        }
        pump(link, devices);
        for index in 0..2 {
            for bytes in devices[index].take_l2cap(SMP_CID) {
                sessions[index]
                    .process(SmpPdu::from_bytes(&bytes).unwrap())
                    .unwrap();
                progress = true;
            }
        }
        if !progress {
            return;
        }
    }
    panic!("host-backed SMP sessions did not quiesce");
}

fn drive_sc_sessions(
    link: &mut LocalLink,
    devices: &mut [Device; 2],
    sessions: &mut [ScPairingSession; 2],
) {
    for _ in 0..100 {
        let mut progress = false;
        for index in 0..2 {
            for pdu in sessions[index].drain_outbound() {
                assert!(devices[index].send_l2cap(link, SMP_CID, &pdu.to_bytes()));
                progress = true;
            }
        }
        pump(link, devices);
        for index in 0..2 {
            for bytes in devices[index].take_l2cap(SMP_CID) {
                sessions[index]
                    .process(SmpPdu::from_bytes(&bytes).unwrap())
                    .unwrap();
                progress = true;
            }
        }
        if !progress {
            return;
        }
    }
    panic!("host-backed SC sessions did not quiesce");
}

#[test]
fn live_legacy_session_derives_stk_and_enables_encryption_on_both_hosts() {
    let mut link = LocalLink::new();
    let central = link.add_controller(Controller::new("C", address("00:00:00:00:00:01")));
    let peripheral = link.add_controller(Controller::new("P", address("00:00:00:00:00:02")));
    let mut devices = [Device::new(central), Device::new(peripheral)];
    connect(&mut link, central, peripheral);
    pump(&mut link, &mut devices);

    let initiator_address = address("C4:F2:17:1A:1D:AA");
    let responder_address = address("C4:F2:17:1A:1D:BB");
    let mut sessions = [
        LegacyPairingSession::new(
            PairingRole::Initiator,
            config(),
            Box::new(AcceptAllDelegate),
            initiator_address.clone(),
            responder_address.clone(),
            [0x11; 16],
        )
        .unwrap(),
        LegacyPairingSession::new(
            PairingRole::Responder,
            config(),
            Box::new(AcceptAllDelegate),
            initiator_address,
            responder_address,
            [0x22; 16],
        )
        .unwrap(),
    ];
    sessions[0].start().unwrap();
    drive_sessions(&mut link, &mut devices, &mut sessions);

    assert_eq!(sessions[0].state(), PairingState::WaitEncryption);
    assert_eq!(sessions[1].state(), PairingState::WaitEncryption);
    let stk = sessions[0].stk().unwrap();
    assert_eq!(Some(stk), sessions[1].stk());
    assert!(devices[0].enable_encryption(&mut link, stk));
    pump(&mut link, &mut devices);
    assert!(devices[0].is_encrypted());
    assert!(devices[1].is_encrypted());

    sessions[0].mark_encrypted().unwrap();
    sessions[1].mark_encrypted().unwrap();
    assert_eq!(sessions[0].state(), PairingState::Complete);
    assert_eq!(sessions[1].state(), PairingState::Complete);

    assert!(devices[0].disconnect(&mut link, 0x13));
    pump(&mut link, &mut devices);
    assert!(!devices[0].is_encrypted());
    assert!(!devices[1].is_encrypted());
}

#[test]
fn live_sc_session_derives_ltk_and_enables_encryption_on_both_hosts() {
    let mut link = LocalLink::new();
    let central = link.add_controller(Controller::new("C", address("00:00:00:00:00:01")));
    let peripheral = link.add_controller(Controller::new("P", address("00:00:00:00:00:02")));
    let mut devices = [Device::new(central), Device::new(peripheral)];
    connect(&mut link, central, peripheral);
    pump(&mut link, &mut devices);

    let sc_config = || PairingConfig {
        secure_connections: true,
        ..config()
    };
    let initiator_address = address("C4:F2:17:1A:1D:AA");
    let responder_address = address("C4:F2:17:1A:1D:BB");
    let mut sessions = [
        ScPairingSession::new(
            PairingRole::Initiator,
            sc_config(),
            Box::new(AcceptAllDelegate),
            initiator_address.clone(),
            responder_address.clone(),
            EccKey::from_private_key_bytes(&(1u8..=32).collect::<Vec<_>>()).unwrap(),
            [0xA0; 16],
        )
        .unwrap(),
        ScPairingSession::new(
            PairingRole::Responder,
            sc_config(),
            Box::new(AcceptAllDelegate),
            initiator_address,
            responder_address,
            EccKey::from_private_key_bytes(&(33u8..=64).collect::<Vec<_>>()).unwrap(),
            [0xB0; 16],
        )
        .unwrap(),
    ];
    sessions[0].start().unwrap();
    drive_sc_sessions(&mut link, &mut devices, &mut sessions);
    assert_eq!(sessions[0].state(), ScPairingState::WaitEncryption);
    assert_eq!(sessions[1].state(), ScPairingState::WaitEncryption);
    let ltk = sessions[0].ltk().unwrap();
    assert_eq!(Some(ltk), sessions[1].ltk());

    assert!(devices[0].enable_encryption(&mut link, ltk));
    pump(&mut link, &mut devices);
    assert!(devices[0].is_encrypted());
    assert!(devices[1].is_encrypted());
    sessions[0].mark_encrypted().unwrap();
    sessions[1].mark_encrypted().unwrap();
    assert_eq!(sessions[0].state(), ScPairingState::Complete);
    assert_eq!(sessions[1].state(), ScPairingState::Complete);
}
