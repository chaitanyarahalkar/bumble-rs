use bumble::keys::{Key, PairingKeys};
use bumble_smp::{
    security_request, security_request_action, AuthReq, PairingRole, SecurityRequestAction, SmpPdu,
};

fn key(value: u8, authenticated: bool) -> Key {
    Key {
        value: vec![value; 16],
        authenticated,
        ediv: None,
        rand: None,
        sign_counter: None,
    }
}

#[test]
fn security_request_serializes_requested_authentication() {
    let auth = AuthReq::from_booleans(true, true, true, false, true);
    let pdu = security_request(auth);
    assert_eq!(pdu, SmpPdu::SecurityRequest { auth_req: 0x2D });
    assert_eq!(pdu.to_bytes(), vec![0x0B, 0x2D]);
}

#[test]
fn sc_bond_satisfies_sc_and_mitm_only_when_authenticated() {
    let requested = AuthReq::SECURE_CONNECTIONS | AuthReq::MITM;
    let authenticated = PairingKeys {
        ltk: Some(key(0xA5, true)),
        ..PairingKeys::default()
    };
    let SecurityRequestAction::EnableEncryption(encryption) =
        security_request_action(requested, PairingRole::Initiator, Some(&authenticated))
    else {
        panic!("authenticated SC bond should be reused");
    };
    assert_eq!(encryption.long_term_key, [0xA5; 16]);
    assert_eq!(encryption.encrypted_diversifier, 0);
    assert_eq!(encryption.random_number, [0; 8]);
    assert!(encryption.secure_connections);
    assert!(encryption.authenticated);

    let unauthenticated = PairingKeys {
        ltk: Some(key(0xA5, false)),
        ..PairingKeys::default()
    };
    assert_eq!(
        security_request_action(requested, PairingRole::Initiator, Some(&unauthenticated)),
        SecurityRequestAction::Pair
    );
}

#[test]
fn legacy_bond_selects_role_direction_and_preserves_ediv_rand() {
    let legacy = PairingKeys {
        ltk_central: Some(Key {
            value: vec![0xC0; 16],
            authenticated: true,
            ediv: Some(0x1234),
            rand: Some(vec![0x56; 8]),
            sign_counter: None,
        }),
        ltk_peripheral: Some(Key {
            value: vec![0xD0; 16],
            authenticated: true,
            ediv: Some(0xABCD),
            rand: Some(vec![0x78; 8]),
            sign_counter: None,
        }),
        ..PairingKeys::default()
    };
    let SecurityRequestAction::EnableEncryption(central) =
        security_request_action(AuthReq::MITM, PairingRole::Initiator, Some(&legacy))
    else {
        panic!("central key should be selected");
    };
    assert_eq!(central.long_term_key, [0xC0; 16]);
    assert_eq!(central.encrypted_diversifier, 0x1234);
    assert_eq!(central.random_number, [0x56; 8]);

    let SecurityRequestAction::EnableEncryption(peripheral) =
        security_request_action(AuthReq::MITM, PairingRole::Responder, Some(&legacy))
    else {
        panic!("peripheral key should be selected");
    };
    assert_eq!(peripheral.long_term_key, [0xD0; 16]);
    assert_eq!(peripheral.encrypted_diversifier, 0xABCD);
    assert_eq!(peripheral.random_number, [0x78; 8]);
    assert!(!peripheral.secure_connections);
}

#[test]
fn missing_malformed_or_insufficient_bond_requests_pairing() {
    assert_eq!(
        security_request_action(AuthReq::BONDING, PairingRole::Initiator, None),
        SecurityRequestAction::Pair
    );
    let malformed = PairingKeys {
        ltk: Some(Key::new(vec![1; 15])),
        ..PairingKeys::default()
    };
    assert_eq!(
        security_request_action(AuthReq::BONDING, PairingRole::Initiator, Some(&malformed)),
        SecurityRequestAction::Pair
    );
    let legacy = PairingKeys {
        ltk_central: Some(key(2, true)),
        ..PairingKeys::default()
    };
    assert_eq!(
        security_request_action(
            AuthReq::SECURE_CONNECTIONS,
            PairingRole::Initiator,
            Some(&legacy)
        ),
        SecurityRequestAction::Pair
    );
}
