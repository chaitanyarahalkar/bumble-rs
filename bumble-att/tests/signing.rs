use std::time::{SystemTime, UNIX_EPOCH};

use bumble::keys::{JsonKeyStore, Key, KeyStore, PairingKeys};
use bumble_att::{signed_write_signature, AttPdu, SignedWriteSigner, SignedWriteVerifier};

#[test]
fn signed_write_cmac_vector_and_wire_shape_are_stable() {
    let csrk = [
        0x2B, 0x7E, 0x15, 0x16, 0x28, 0xAE, 0xD2, 0xA6, 0xAB, 0xF7, 0x15, 0x88, 0x09, 0xCF, 0x4F,
        0x3C,
    ];
    let signature = signed_write_signature(&csrk, 0x1234, b"bumble", 0x0102_0304);
    // Independently pinned with OpenSSL AES-128-CMAC over
    // d2 3412 62756d626c65 04030201.
    assert_eq!(signature, [0x09, 0x16, 0x14, 0x68, 0xFF, 0xDF, 0xE5, 0xE7]);
    let pdu = AttPdu::SignedWriteCommand {
        attribute_handle: 0x1234,
        attribute_value: b"bumble".to_vec(),
        sign_counter: 0x0102_0304,
        signature,
    };
    assert_eq!(
        pdu.to_bytes(),
        [
            [0xD2, 0x34, 0x12].as_slice(),
            b"bumble".as_slice(),
            [0x04, 0x03, 0x02, 0x01].as_slice(),
            signature.as_slice(),
        ]
        .concat()
    );
    assert_eq!(AttPdu::from_bytes(&pdu.to_bytes()).unwrap(), pdu);
}

#[test]
fn signer_and_verifier_enforce_key_mac_and_monotonic_counter() {
    let mut signer = SignedWriteSigner::new([0x44; 16], 7);
    let first = signer.sign(3, b"first".to_vec()).unwrap();
    let second = signer.sign(3, b"second".to_vec()).unwrap();
    assert_eq!(signer.next_counter(), 9);

    let mut verifier = SignedWriteVerifier::new([0x44; 16], None);
    assert!(verifier.verify(&first));
    assert_eq!(verifier.last_counter(), Some(7));
    assert!(!verifier.verify(&first), "replay must fail");
    assert!(verifier.verify(&second));

    let mut tampered = second.clone();
    if let AttPdu::SignedWriteCommand {
        attribute_value, ..
    } = &mut tampered
    {
        attribute_value[0] ^= 1;
    }
    assert!(!SignedWriteVerifier::new([0x44; 16], None).verify(&tampered));
    assert!(!SignedWriteVerifier::new([0x45; 16], None).verify(&second));
}

#[test]
fn outgoing_and_incoming_counters_survive_key_store_restart() {
    let path = std::env::temp_dir().join(format!(
        "bumble-rs-signing-{}-{}.json",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut keys = PairingKeys {
        local_csrk: Some(Key {
            value: vec![0x44; 16],
            authenticated: true,
            ediv: None,
            rand: None,
            sign_counter: Some(10),
        }),
        csrk: Some(Key {
            value: vec![0x55; 16],
            authenticated: true,
            ediv: None,
            rand: None,
            sign_counter: None,
        }),
        ..PairingKeys::default()
    };
    let mut signer = SignedWriteSigner::from_pairing_keys(&keys).unwrap();
    let outgoing = signer.sign(1, b"outgoing".to_vec()).unwrap();
    assert!(signer.save_counter(&mut keys));

    let peer_packet = SignedWriteSigner::new([0x55; 16], 22)
        .sign(2, b"incoming".to_vec())
        .unwrap();
    let mut verifier = SignedWriteVerifier::from_pairing_keys(&keys).unwrap();
    assert!(verifier.verify(&peer_packet));
    assert!(verifier.save_counter(&mut keys));

    let mut store = JsonKeyStore::new(Some("controller"), &path);
    store.update("peer", keys).unwrap();
    let restored = store.get("peer").unwrap().unwrap();
    assert_eq!(
        SignedWriteSigner::from_pairing_keys(&restored)
            .unwrap()
            .next_counter(),
        11
    );
    let mut restored_verifier = SignedWriteVerifier::from_pairing_keys(&restored).unwrap();
    assert_eq!(restored_verifier.last_counter(), Some(22));
    assert!(!restored_verifier.verify(&peer_packet));
    assert!(SignedWriteVerifier::new([0x44; 16], None).verify(&outgoing));
    std::fs::remove_file(path).unwrap();
}
