use bumble::keys::{KeyStore, MemoryKeyStore};
use bumble::{Address, AddressType};
use bumble_smp::{
    security_request, AcceptAllDelegate, AuthReq, ClassicCtkdState, ManagedPairingState,
    PairingConfig, PairingConnection, PairingManager, PairingRole, ScPairingState,
};

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

fn manager() -> PairingManager {
    PairingManager::new(
        PairingConfig {
            mitm: false,
            ..PairingConfig::default()
        },
        Box::new(|_, _| Box::new(AcceptAllDelegate)),
    )
}

#[test]
fn manager_uses_and_reuses_the_bluetooth_sc_debug_key() {
    let mut manager = manager();
    manager.set_debug_mode(true);

    let expected_x = [
        0x20, 0xB0, 0x03, 0xD2, 0xF2, 0x97, 0xBE, 0x2C, 0x5E, 0x2C, 0x83, 0xA7, 0xE9, 0xF9, 0xA5,
        0xB9, 0xEF, 0xF4, 0x91, 0x11, 0xAC, 0xF4, 0xFD, 0xDB, 0xCC, 0x03, 0x01, 0x48, 0x0E, 0x35,
        0x9D, 0xE6,
    ];
    let expected_y = [
        0xDC, 0x80, 0x9C, 0x49, 0x65, 0x2A, 0xEB, 0x6D, 0x63, 0x32, 0x9A, 0xBF, 0x5A, 0x52, 0x15,
        0x5C, 0x76, 0x63, 0x45, 0xC2, 0x8F, 0xED, 0x30, 0x24, 0x74, 0x1C, 0x8E, 0xD0, 0x15, 0x89,
        0xD2, 0x8B,
    ];

    assert!(manager.debug_mode());
    let public_key = manager.ecc_public_key();
    assert_eq!(public_key.0, expected_x);
    assert_eq!(public_key.1, expected_y);
    assert_eq!(manager.ecc_public_key(), public_key);
}

fn relay(left: &mut PairingManager, right: &mut PairingManager) {
    for _ in 0..200 {
        let mut progress = false;
        for (handle, pdu) in left.drain_outbound() {
            right.receive(handle, pdu).unwrap();
            progress = true;
        }
        for (handle, pdu) in right.drain_outbound() {
            left.receive(handle, pdu).unwrap();
            progress = true;
        }
        if !progress {
            return;
        }
    }
    panic!("pairing managers did not quiesce");
}

#[test]
fn manager_runs_two_concurrent_sc_sessions_and_persists_each_bond() {
    let mut central = manager();
    let mut peripheral = manager();
    for (handle, central_text, peripheral_text) in [
        (0x0040, "C4:F2:17:1A:1D:A1", "C4:F2:17:1A:1D:B1"),
        (0x0041, "C4:F2:17:1A:1D:A2", "C4:F2:17:1A:1D:B2"),
    ] {
        let central_address = address(central_text);
        let peripheral_address = address(peripheral_text);
        central
            .register_connection(PairingConnection::le(
                handle,
                PairingRole::Initiator,
                central_address.clone(),
                peripheral_address.clone(),
            ))
            .unwrap();
        peripheral
            .register_connection(PairingConnection::le(
                handle,
                PairingRole::Responder,
                peripheral_address,
                central_address,
            ))
            .unwrap();
        central.pair(handle).unwrap();
    }

    relay(&mut central, &mut peripheral);
    for handle in [0x0040, 0x0041] {
        assert_eq!(
            central.state(handle),
            Some(ManagedPairingState::SecureConnections(
                ScPairingState::WaitEncryption
            ))
        );
        assert_eq!(
            peripheral.state(handle),
            Some(ManagedPairingState::SecureConnections(
                ScPairingState::WaitEncryption
            ))
        );
        central.mark_encrypted(handle).unwrap();
        peripheral.mark_encrypted(handle).unwrap();
    }
    relay(&mut central, &mut peripheral);

    let mut central_store = MemoryKeyStore::new();
    let mut peripheral_store = MemoryKeyStore::new();
    for handle in [0x0040, 0x0041] {
        assert_eq!(
            central.state(handle),
            Some(ManagedPairingState::SecureConnections(
                ScPairingState::Complete
            ))
        );
        assert!(central.pairing_keys(handle).unwrap().ltk.is_some());
        assert!(central.store_bond(handle, &mut central_store).unwrap());
        assert!(peripheral
            .store_bond(handle, &mut peripheral_store)
            .unwrap());
    }
    assert_eq!(central_store.get_all().unwrap().len(), 2);
    assert_eq!(peripheral_store.get_all().unwrap().len(), 2);
    assert_eq!(central.connection_count(), 2);
    assert_eq!(central.session_count(), 2);
}

#[test]
fn manager_routes_security_requests_rejects_invalid_lifecycle_and_cleans_disconnect() {
    let mut central = manager();
    central
        .register_connection(PairingConnection::le(
            7,
            PairingRole::Initiator,
            address("C4:F2:17:1A:1D:AA"),
            address("C4:F2:17:1A:1D:BB"),
        ))
        .unwrap();
    assert!(central
        .register_connection(PairingConnection::le(
            7,
            PairingRole::Initiator,
            address("C4:F2:17:1A:1D:AA"),
            address("C4:F2:17:1A:1D:BB"),
        ))
        .is_err());
    let requested = AuthReq::from_booleans(true, true, true, false, true);
    central.receive(7, security_request(requested)).unwrap();
    assert_eq!(central.poll_security_request(), Some((7, requested)));
    assert!(central.mark_encrypted(7).is_err());
    central
        .set_connection_role(7, PairingRole::Responder)
        .unwrap();
    assert!(central.pair(7).is_err());
    central
        .set_connection_role(7, PairingRole::Initiator)
        .unwrap();
    central.pair(7).unwrap();
    assert!(central
        .set_connection_role(7, PairingRole::Responder)
        .is_err());
    assert!(central.pair(7).is_err());
    assert!(central.disconnect(7));
    assert_eq!(central.connection_count(), 0);
    assert_eq!(central.session_count(), 0);
    assert!(central.poll_outbound().is_none());
    assert!(!central.disconnect(7));
}

#[test]
fn manager_selects_classic_ctkd_for_encrypted_br_edr_connections() {
    let mut initiator = manager();
    let mut responder = manager();
    let a = address("11:11:11:11:11:11");
    let b = address("22:22:22:22:22:22");
    let link_key = [0xD4; 16];
    initiator
        .register_connection(PairingConnection::br_edr(
            9,
            PairingRole::Initiator,
            a.clone(),
            b.clone(),
            link_key,
            true,
            true,
        ))
        .unwrap();
    responder
        .register_connection(PairingConnection::br_edr(
            9,
            PairingRole::Responder,
            b,
            a,
            link_key,
            true,
            true,
        ))
        .unwrap();
    initiator.pair(9).unwrap();
    relay(&mut initiator, &mut responder);
    assert_eq!(
        initiator.state(9),
        Some(ManagedPairingState::ClassicCtkd(ClassicCtkdState::Complete))
    );
    assert_eq!(initiator.encryption_key(9), responder.encryption_key(9));
    assert_eq!(
        initiator.pairing_keys(9).unwrap().link_key.unwrap().value,
        vec![0xD4; 16]
    );
}
