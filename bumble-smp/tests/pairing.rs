use bumble::{Address, AddressType, AdvertisingData, LeRole};
use bumble_crypto::EccKey;
use bumble_smp::{
    derive_link_key, derive_ltk, select_pairing_method, select_pairing_method_with_oob, AuthReq,
    IoCapability, KeyDistribution, OobContext, OobData, OobLegacyContext, OobSharedData,
    PairingCapabilities, PairingMethod,
};

fn reversed_hex<const N: usize>(value: &str) -> [u8; N] {
    let compact = value.replace(' ', "");
    let mut bytes: Vec<_> = (0..compact.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&compact[offset..offset + 2], 16).unwrap())
        .collect();
    bytes.reverse();
    bytes.try_into().unwrap()
}

fn hex_bytes(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&value[offset..offset + 2], 16).unwrap())
        .collect()
}

#[test]
fn auth_and_key_distribution_flags_match_upstream() {
    let auth = AuthReq::from_booleans(true, true, true, true, true);
    assert_eq!(auth.0, 0b0011_1101);
    assert!(auth.contains(AuthReq::BONDING));
    assert!(auth.contains(AuthReq::MITM));
    assert!(auth.contains(AuthReq::SECURE_CONNECTIONS));
    assert!(auth.contains(AuthReq::KEYPRESS));
    assert!(auth.contains(AuthReq::CT2));

    let capabilities = PairingCapabilities {
        io_capability: IoCapability::KeyboardDisplay,
        local_initiator_key_distribution: KeyDistribution::ENCRYPTION_KEY
            | KeyDistribution::IDENTITY_KEY,
        local_responder_key_distribution: KeyDistribution::IDENTITY_KEY
            | KeyDistribution::SIGNING_KEY,
        maximum_encryption_key_size: 16,
    };
    assert_eq!(
        capabilities.negotiate_key_distribution(
            KeyDistribution::ALL,
            KeyDistribution::ENCRYPTION_KEY | KeyDistribution::SIGNING_KEY
        ),
        (KeyDistribution(0b0011), KeyDistribution::SIGNING_KEY)
    );
    assert!(capabilities.validate().is_ok());
    assert!(PairingCapabilities {
        maximum_encryption_key_size: 6,
        ..capabilities
    }
    .validate()
    .is_err());
}

#[test]
fn pairing_method_matrix_covers_legacy_secure_connections_and_oob() {
    use IoCapability::{
        DisplayOnly as D, DisplayYesNo as Y, KeyboardDisplay as B, KeyboardOnly as K,
        NoInputNoOutput as N,
    };
    let mitm = AuthReq(AuthReq::MITM.0);

    let selection = select_pairing_method(false, true, mitm, D, K);
    assert_eq!(selection.method, PairingMethod::Passkey);
    assert!(selection.initiator_displays);
    assert!(!selection.responder_displays);

    let selection = select_pairing_method(false, true, mitm, K, B);
    assert_eq!(selection.method, PairingMethod::Passkey);
    assert!(!selection.initiator_displays);
    assert!(selection.responder_displays);

    assert_eq!(
        select_pairing_method(false, true, mitm, Y, Y).method,
        PairingMethod::JustWorks
    );
    assert_eq!(
        select_pairing_method(true, true, mitm, Y, Y).method,
        PairingMethod::NumericComparison
    );
    assert_eq!(
        select_pairing_method(true, true, mitm, B, B).method,
        PairingMethod::NumericComparison
    );

    for capability in [D, Y, K, N, B] {
        assert_eq!(
            select_pairing_method(true, true, mitm, N, capability).method,
            PairingMethod::JustWorks
        );
        assert_eq!(
            select_pairing_method(true, true, mitm, capability, N).method,
            PairingMethod::JustWorks
        );
    }
    assert_eq!(
        select_pairing_method(true, false, AuthReq(0), B, B).method,
        PairingMethod::JustWorks
    );

    assert_eq!(
        select_pairing_method_with_oob(true, true, false, true, mitm, N, N).method,
        PairingMethod::Oob
    );
    assert_eq!(
        select_pairing_method_with_oob(false, true, false, true, mitm, N, N).method,
        PairingMethod::JustWorks
    );
    assert_eq!(
        select_pairing_method_with_oob(false, true, true, true, mitm, N, N).method,
        PairingMethod::Oob
    );
}

#[test]
fn oob_data_round_trips_the_upstream_ad_structures() {
    let address = Address::parse("F0:F1:F2:F3:F4:F5", AddressType::PUBLIC_DEVICE).unwrap();
    let oob = OobData {
        address: Some(address.clone()),
        role: Some(LeRole::PERIPHERAL_PREFERRED),
        shared_data: Some(OobSharedData {
            c: b"12".to_vec(),
            r: b"34".to_vec(),
        }),
        legacy_context: Some(OobLegacyContext {
            tk: (0u8..16).collect(),
        }),
    };
    let bytes = oob.to_ad().to_bytes();
    assert_eq!(
        bytes,
        [
            vec![8, 0x1B, 0, 0xF5, 0xF4, 0xF3, 0xF2, 0xF1, 0xF0],
            vec![2, 0x1C, 2],
            vec![3, 0x22, b'1', b'2'],
            vec![3, 0x23, b'3', b'4'],
            [vec![17, 0x10], (0u8..16).collect()].concat(),
        ]
        .concat()
    );

    let parsed = OobData::from_ad(&AdvertisingData::from_bytes(&bytes));
    assert_eq!(parsed, oob);
    assert_eq!(parsed.address, Some(address));

    let confirmation_only = AdvertisingData {
        ad_structures: vec![(
            bumble::advertising_data::Type::LE_SECURE_CONNECTIONS_CONFIRMATION_VALUE,
            vec![1; 16],
        )],
    };
    assert!(OobData::from_ad(&confirmation_only).shared_data.is_none());
}

#[test]
fn deterministic_oob_context_matches_python_oracle() {
    let key = EccKey::from_private_key_bytes(&(1u8..=32).collect::<Vec<_>>()).unwrap();
    let context = OobContext::new(Some(key), Some([0x55; 16]));
    let shared = context.share();
    assert_eq!(shared.c, hex_bytes("20b3002ee03c0a69baa439773dcf1793"));
    assert_eq!(shared.r, vec![0x55; 16]);
}

#[test]
fn cross_transport_key_derivation_matches_upstream_vectors() {
    let ltk = reversed_hex::<16>("368df9bc e3264b58 bd066c33 334fbf64");
    assert_eq!(
        derive_link_key(&ltk, false),
        reversed_hex::<16>("bc1ca4ef 633fc1bd 0d8230af ee388fb0")
    );
    assert_eq!(
        derive_link_key(&ltk, true),
        reversed_hex::<16>("287ad379 dca40253 0a39f1f4 3047b835")
    );

    let link_key = reversed_hex::<16>("05040302 01000908 07060504 03020100");
    assert_eq!(
        derive_ltk(&link_key, false),
        reversed_hex::<16>("a813fb72 f1a3dfa1 8a2c9a43 f10d0a30")
    );
    assert_eq!(
        derive_ltk(&link_key, true),
        reversed_hex::<16>("e85e09eb 5eccb3e2 69418a13 3211bc79")
    );
}
