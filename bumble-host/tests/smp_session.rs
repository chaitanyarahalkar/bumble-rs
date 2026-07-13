use bumble::keys::{Key, KeyStore, MemoryKeyStore, PairingKeys};
use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_crypto::EccKey;
use bumble_hci::Command;
use bumble_host::{pump, Device};
use bumble_smp::{
    security_request, security_request_action, AcceptAllDelegate, AuthReq, IoCapability,
    KeyDistribution, LegacyPairingSession, ManagedPairingState, PairingCapabilities, PairingConfig,
    PairingConnection, PairingManager, PairingRole, PairingState, ScPairingSession, ScPairingState,
    SecurityRequestAction, SmpPdu, SMP_CID,
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
        ct2: false,
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

fn drive_managers(
    link: &mut LocalLink,
    devices: &mut [Device; 2],
    managers: &mut [PairingManager; 2],
) {
    for _ in 0..200 {
        let mut progress = false;
        for index in 0..2 {
            for (handle, pdu) in managers[index].drain_outbound() {
                assert_eq!(Some(handle), devices[index].connection_handle());
                assert!(devices[index].send_l2cap(link, SMP_CID, &pdu.to_bytes()));
                progress = true;
            }
        }
        pump(link, devices);
        for index in 0..2 {
            let handle = devices[index].connection_handle().unwrap();
            for payload in devices[index].take_l2cap(SMP_CID) {
                managers[index]
                    .receive(handle, SmpPdu::from_bytes(&payload).unwrap())
                    .unwrap();
                progress = true;
            }
        }
        if !progress {
            return;
        }
    }
    panic!("host-backed pairing managers did not quiesce");
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
    drive_sessions(&mut link, &mut devices, &mut sessions);
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
    drive_sc_sessions(&mut link, &mut devices, &mut sessions);
    assert_eq!(sessions[0].state(), ScPairingState::Complete);
    assert_eq!(sessions[1].state(), ScPairingState::Complete);
}

#[test]
fn security_request_reuses_a_satisfactory_persisted_bond() {
    let mut link = LocalLink::new();
    let central = link.add_controller(Controller::new("C", address("00:00:00:00:00:01")));
    let peripheral = link.add_controller(Controller::new("P", address("00:00:00:00:00:02")));
    let mut devices = [Device::new(central), Device::new(peripheral)];
    connect(&mut link, central, peripheral);
    pump(&mut link, &mut devices);

    let requested = AuthReq::from_booleans(true, true, true, false, true);
    assert!(devices[1].send_l2cap(&mut link, SMP_CID, &security_request(requested).to_bytes()));
    pump(&mut link, &mut devices);
    assert_eq!(devices[0].take_security_requests(), vec![requested.0]);

    let bond = PairingKeys {
        ltk: Some(Key {
            value: vec![0xA5; 16],
            authenticated: true,
            ediv: None,
            rand: None,
            sign_counter: None,
        }),
        ..PairingKeys::default()
    };
    let SecurityRequestAction::EnableEncryption(encryption) =
        security_request_action(requested, PairingRole::Initiator, Some(&bond))
    else {
        panic!("stored SC bond should satisfy the request");
    };
    assert!(devices[0].enable_encryption_with_parameters(
        &mut link,
        encryption.long_term_key,
        encryption.encrypted_diversifier,
        encryption.random_number,
    ));
    pump(&mut link, &mut devices);
    assert!(devices[0].is_encrypted());
    assert!(devices[1].is_encrypted());
}

#[test]
fn pairing_manager_owns_live_session_encryption_distribution_and_bonding() {
    let mut link = LocalLink::new();
    let central = link.add_controller(Controller::new("C", address("00:00:00:00:00:01")));
    let peripheral = link.add_controller(Controller::new("P", address("00:00:00:00:00:02")));
    let mut devices = [Device::new(central), Device::new(peripheral)];
    connect(&mut link, central, peripheral);
    pump(&mut link, &mut devices);
    let handle = devices[0].connection_handle().unwrap();
    assert_eq!(devices[1].connection_handle(), Some(handle));

    let manager_config = PairingConfig {
        secure_connections: true,
        ..config()
    };
    let new_manager = || {
        PairingManager::new(
            manager_config.clone(),
            Box::new(|_, _| Box::new(AcceptAllDelegate)),
        )
    };
    let initiator_address = address("C4:F2:17:1A:1D:AA");
    let responder_address = address("C4:F2:17:1A:1D:BB");
    let mut managers = [new_manager(), new_manager()];
    managers[0]
        .register_connection(PairingConnection::le(
            handle,
            PairingRole::Initiator,
            initiator_address.clone(),
            responder_address.clone(),
        ))
        .unwrap();
    managers[1]
        .register_connection(PairingConnection::le(
            handle,
            PairingRole::Responder,
            responder_address,
            initiator_address,
        ))
        .unwrap();
    managers[0].pair(handle).unwrap();
    drive_managers(&mut link, &mut devices, &mut managers);
    assert_eq!(
        managers[0].state(handle),
        Some(ManagedPairingState::SecureConnections(
            ScPairingState::WaitEncryption
        ))
    );
    let ltk = managers[0].encryption_key(handle).unwrap();
    assert_eq!(managers[1].encryption_key(handle), Some(ltk));
    assert!(devices[0].enable_encryption(&mut link, ltk));
    pump(&mut link, &mut devices);
    managers[0].mark_encrypted(handle).unwrap();
    managers[1].mark_encrypted(handle).unwrap();
    drive_managers(&mut link, &mut devices, &mut managers);
    assert_eq!(
        managers[0].state(handle),
        Some(ManagedPairingState::SecureConnections(
            ScPairingState::Complete
        ))
    );
    let mut store = MemoryKeyStore::new();
    assert!(managers[0].store_bond(handle, &mut store).unwrap());
    assert_eq!(store.get_all().unwrap().len(), 1);
}
