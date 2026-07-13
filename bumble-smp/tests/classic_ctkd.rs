use bumble::keys::{KeyStore, MemoryKeyStore};
use bumble::{Address, AddressType};
use bumble_smp::{
    derive_ltk, ClassicCtkdSession, ClassicCtkdState, IoCapability, KeyDistribution,
    LocalKeyMaterial, PairingCapabilities, PairingConfig, PairingMethod, PairingRole, SmpPdu,
};

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::PUBLIC_DEVICE).unwrap()
}

fn config(ct2: bool) -> PairingConfig {
    PairingConfig {
        secure_connections: true,
        ct2,
        mitm: true,
        bonding: true,
        capabilities: PairingCapabilities {
            io_capability: IoCapability::NoInputNoOutput,
            local_initiator_key_distribution: KeyDistribution::ALL,
            local_responder_key_distribution: KeyDistribution::ALL,
            maximum_encryption_key_size: 16,
        },
        identity_address_type: None,
        oob: None,
    }
}

fn local_keys(seed: u8, identity_address: Address) -> LocalKeyMaterial {
    LocalKeyMaterial {
        ltk: [seed; 16],
        ediv: u16::from(seed),
        rand: [seed; 8],
        irk: [seed + 1; 16],
        identity_address,
        csrk: [seed + 2; 16],
    }
}

fn sessions(ct2_a: bool, ct2_b: bool) -> (ClassicCtkdSession, ClassicCtkdSession) {
    let a = address("11:11:11:11:11:11");
    let b = address("22:22:22:22:22:22");
    let link_key = [0xA5; 16];
    let mut initiator = ClassicCtkdSession::new(
        PairingRole::Initiator,
        config(ct2_a),
        a.clone(),
        b.clone(),
        link_key,
        true,
        true,
    )
    .unwrap();
    let mut responder = ClassicCtkdSession::new(
        PairingRole::Responder,
        config(ct2_b),
        a.clone(),
        b.clone(),
        link_key,
        true,
        true,
    )
    .unwrap();
    initiator
        .set_local_key_material(local_keys(0x10, a))
        .unwrap();
    responder
        .set_local_key_material(local_keys(0x20, b))
        .unwrap();
    (initiator, responder)
}

fn relay(initiator: &mut ClassicCtkdSession, responder: &mut ClassicCtkdSession) -> Vec<SmpPdu> {
    let mut transcript = Vec::new();
    for _ in 0..50 {
        let mut progress = false;
        for pdu in initiator.drain_outbound() {
            transcript.push(pdu.clone());
            responder.process(pdu).unwrap();
            progress = true;
        }
        for pdu in responder.drain_outbound() {
            transcript.push(pdu.clone());
            initiator.process(pdu).unwrap();
            progress = true;
        }
        if !progress {
            return transcript;
        }
    }
    panic!("Classic CTKD sessions did not quiesce");
}

#[test]
fn encrypted_classic_feature_exchange_derives_h7_ltk_and_distributes_non_enc_keys() {
    let (mut initiator, mut responder) = sessions(true, true);
    initiator.start().unwrap();
    let transcript = relay(&mut initiator, &mut responder);
    assert_eq!(initiator.state(), ClassicCtkdState::Complete);
    assert_eq!(responder.state(), ClassicCtkdState::Complete);
    assert_eq!(
        initiator.outcome().unwrap().method,
        PairingMethod::CtkdOverClassic
    );
    assert!(initiator.outcome().unwrap().ct2);
    assert_eq!(
        initiator.outcome().unwrap().ltk,
        derive_ltk(&[0xA5; 16], true)
    );
    assert_eq!(initiator.outcome().unwrap(), responder.outcome().unwrap());
    assert!(transcript
        .iter()
        .any(|pdu| matches!(pdu, SmpPdu::PairingRequest(_))));
    assert!(transcript
        .iter()
        .any(|pdu| matches!(pdu, SmpPdu::PairingResponse(_))));
    assert!(transcript.iter().all(|pdu| !matches!(
        pdu,
        SmpPdu::PairingConfirm { .. }
            | SmpPdu::PairingRandom { .. }
            | SmpPdu::PairingPublicKey { .. }
            | SmpPdu::PairingDhKeyCheck { .. }
            | SmpPdu::EncryptionInformation { .. }
            | SmpPdu::MasterIdentification { .. }
    )));

    let keys = initiator.pairing_keys().unwrap();
    assert_eq!(keys.ltk.unwrap().value, derive_ltk(&[0xA5; 16], true));
    assert_eq!(keys.link_key.unwrap().value, vec![0xA5; 16]);
    assert_eq!(keys.irk.unwrap().value, vec![0x21; 16]);
    assert_eq!(keys.csrk.unwrap().value, vec![0x22; 16]);
    let mut store = MemoryKeyStore::new();
    assert!(initiator.store_bond(&mut store).unwrap());
    assert!(store.get("22:22:22:22:22:22").unwrap().is_some());
}

#[test]
fn ct2_requires_both_peers_and_unencrypted_classic_is_rejected() {
    let (mut initiator, mut responder) = sessions(true, false);
    initiator.start().unwrap();
    relay(&mut initiator, &mut responder);
    assert!(!initiator.outcome().unwrap().ct2);
    assert_eq!(
        initiator.outcome().unwrap().ltk,
        derive_ltk(&[0xA5; 16], false)
    );

    assert!(ClassicCtkdSession::new(
        PairingRole::Initiator,
        config(true),
        address("11:11:11:11:11:11"),
        address("22:22:22:22:22:22"),
        [0xA5; 16],
        true,
        false,
    )
    .is_err());
}
