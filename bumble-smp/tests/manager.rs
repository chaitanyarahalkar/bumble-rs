use bumble::keys::{KeyStore, MemoryKeyStore};
use bumble::{Address, AddressType};
use bumble_smp::{
    security_request, AcceptAllDelegate, AuthReq, ManagedPairingState, PairingConfig,
    PairingConnection, PairingManager, PairingRole, ScPairingState,
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
            .register_connection(PairingConnection {
                handle,
                role: PairingRole::Initiator,
                local_address: central_address.clone(),
                peer_address: peripheral_address.clone(),
            })
            .unwrap();
        peripheral
            .register_connection(PairingConnection {
                handle,
                role: PairingRole::Responder,
                local_address: peripheral_address,
                peer_address: central_address,
            })
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
        .register_connection(PairingConnection {
            handle: 7,
            role: PairingRole::Initiator,
            local_address: address("C4:F2:17:1A:1D:AA"),
            peer_address: address("C4:F2:17:1A:1D:BB"),
        })
        .unwrap();
    assert!(central
        .register_connection(PairingConnection {
            handle: 7,
            role: PairingRole::Initiator,
            local_address: address("C4:F2:17:1A:1D:AA"),
            peer_address: address("C4:F2:17:1A:1D:BB"),
        })
        .is_err());
    let requested = AuthReq::from_booleans(true, true, true, false, true);
    central.receive(7, security_request(requested)).unwrap();
    assert_eq!(central.poll_security_request(), Some((7, requested)));
    assert!(central.mark_encrypted(7).is_err());
    central.pair(7).unwrap();
    assert!(central.pair(7).is_err());
    assert!(central.disconnect(7));
    assert_eq!(central.connection_count(), 0);
    assert_eq!(central.session_count(), 0);
    assert!(central.poll_outbound().is_none());
    assert!(!central.disconnect(7));
}
