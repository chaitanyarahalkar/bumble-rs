use bumble::keys::{KeyStore, MemoryKeyStore};
use bumble::{Address, AddressType};
use bumble_smp::{
    derive_link_key, KeyDistribution, KeyDistributionConfig, KeyDistributionSession,
    KeyDistributionState, LocalKeyMaterial, PairingFailureReason, PairingRole, SmpPdu,
};

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

fn material(seed: u8, identity_address: Address) -> LocalKeyMaterial {
    LocalKeyMaterial {
        ltk: [seed; 16],
        ediv: u16::from(seed) * 0x101,
        rand: [seed.wrapping_add(1); 8],
        irk: [seed.wrapping_add(2); 16],
        identity_address,
        csrk: [seed.wrapping_add(3); 16],
    }
}

fn pair(
    secure_connections: bool,
    initiator_flags: KeyDistribution,
    responder_flags: KeyDistribution,
) -> (KeyDistributionSession, KeyDistributionSession) {
    pair_with_ct2(secure_connections, false, initiator_flags, responder_flags)
}

fn pair_with_ct2(
    secure_connections: bool,
    ct2: bool,
    initiator_flags: KeyDistribution,
    responder_flags: KeyDistribution,
) -> (KeyDistributionSession, KeyDistributionSession) {
    let initiator_address = address("C4:F2:17:1A:1D:AA");
    let responder_address = address("C4:F2:17:1A:1D:BB");
    let common = [0x55; 16];
    (
        KeyDistributionSession::new(KeyDistributionConfig {
            role: PairingRole::Initiator,
            secure_connections,
            ct2,
            authenticated: true,
            maximum_encryption_key_size: 16,
            pairing_ltk: common,
            initiator_keys: initiator_flags,
            responder_keys: responder_flags,
            local_keys: material(0x10, initiator_address.clone()),
            peer_address: responder_address.clone(),
        }),
        KeyDistributionSession::new(KeyDistributionConfig {
            role: PairingRole::Responder,
            secure_connections,
            ct2,
            authenticated: true,
            maximum_encryption_key_size: 16,
            pairing_ltk: common,
            initiator_keys: initiator_flags,
            responder_keys: responder_flags,
            local_keys: material(0x20, responder_address),
            peer_address: initiator_address,
        }),
    )
}

fn relay(initiator: &mut KeyDistributionSession, responder: &mut KeyDistributionSession) {
    for _ in 0..20 {
        let mut progress = false;
        for pdu in initiator.drain_outbound() {
            responder.process(pdu);
            progress = true;
        }
        for pdu in responder.drain_outbound() {
            initiator.process(pdu);
            progress = true;
        }
        if !progress {
            return;
        }
    }
    panic!("key distribution did not quiesce");
}

#[test]
fn legacy_responder_distributes_first_and_both_roles_assemble_bond_keys() {
    let flags = KeyDistribution::ENCRYPTION_KEY
        | KeyDistribution::IDENTITY_KEY
        | KeyDistribution::SIGNING_KEY;
    let (mut initiator, mut responder) = pair(false, flags, flags);

    initiator.mark_encrypted();
    assert!(initiator.poll_outbound().is_none());
    responder.mark_encrypted();
    let responder_pdus = responder.drain_outbound();
    assert!(matches!(
        responder_pdus.as_slice(),
        [
            SmpPdu::EncryptionInformation { .. },
            SmpPdu::MasterIdentification { .. },
            SmpPdu::IdentityInformation { .. },
            SmpPdu::IdentityAddressInformation { .. },
            SmpPdu::SigningInformation { .. }
        ]
    ));
    // Bumble tracks an expected set, not a strict sequence: delivery order is
    // deliberately reversed after verifying the sender's canonical order.
    for pdu in responder_pdus.into_iter().rev() {
        initiator.process(pdu);
    }
    relay(&mut initiator, &mut responder);

    assert_eq!(initiator.state(), KeyDistributionState::Complete);
    assert_eq!(responder.state(), KeyDistributionState::Complete);
    let initiator_keys = initiator.pairing_keys().unwrap();
    assert_eq!(initiator_keys.ltk_central.unwrap().value, vec![0x20; 16]);
    assert_eq!(initiator_keys.ltk_peripheral.unwrap().value, vec![0x10; 16]);
    assert_eq!(initiator_keys.irk.unwrap().value, vec![0x22; 16]);
    assert_eq!(initiator_keys.csrk.unwrap().value, vec![0x23; 16]);
    assert_eq!(
        initiator.peer_address().to_string(false),
        "C4:F2:17:1A:1D:BB"
    );

    let responder_keys = responder.pairing_keys().unwrap();
    assert_eq!(responder_keys.ltk_central.unwrap().value, vec![0x20; 16]);
    assert_eq!(responder_keys.ltk_peripheral.unwrap().value, vec![0x10; 16]);
    assert_eq!(responder_keys.irk.unwrap().value, vec![0x12; 16]);
}

#[test]
fn every_negotiated_key_mask_pair_quiesces_in_legacy_and_sc_modes() {
    for secure_connections in [false, true] {
        for initiator_mask in 0..=KeyDistribution::ALL.0 {
            for responder_mask in 0..=KeyDistribution::ALL.0 {
                let (mut initiator, mut responder) = pair(
                    secure_connections,
                    KeyDistribution(initiator_mask),
                    KeyDistribution(responder_mask),
                );
                initiator.mark_encrypted();
                responder.mark_encrypted();
                relay(&mut initiator, &mut responder);
                assert_eq!(
                    initiator.state(),
                    KeyDistributionState::Complete,
                    "initiator mask {initiator_mask:#x}, responder mask {responder_mask:#x}, SC={secure_connections}"
                );
                assert_eq!(responder.state(), KeyDistributionState::Complete);
                assert!(initiator.pairing_keys().is_some());
                assert!(responder.pairing_keys().is_some());
            }
        }
    }
}

#[test]
fn secure_connections_skips_legacy_ltk_pdus_and_derives_link_key() {
    let flags = KeyDistribution::ALL;
    let (mut initiator, mut responder) = pair(true, flags, flags);
    initiator.mark_encrypted();
    responder.mark_encrypted();
    let responder_pdus = responder.drain_outbound();
    assert!(responder_pdus.iter().all(|pdu| !matches!(
        pdu,
        SmpPdu::EncryptionInformation { .. } | SmpPdu::MasterIdentification { .. }
    )));
    for pdu in responder_pdus {
        initiator.process(pdu);
    }
    relay(&mut initiator, &mut responder);

    let keys = initiator.pairing_keys().unwrap();
    assert_eq!(keys.ltk.unwrap().value, vec![0x55; 16]);
    assert_eq!(
        keys.link_key.unwrap().value,
        derive_link_key(&[0x55; 16], false)
    );
}

#[test]
fn negotiated_ct2_selects_h7_ctkd() {
    let flags = KeyDistribution::LINK_KEY;
    let (mut initiator, mut responder) = pair_with_ct2(true, true, flags, flags);
    initiator.mark_encrypted();
    responder.mark_encrypted();
    relay(&mut initiator, &mut responder);
    let keys = initiator.pairing_keys().unwrap();
    assert_eq!(
        keys.link_key.unwrap().value,
        derive_link_key(&[0x55; 16], true)
    );
}

#[test]
fn completed_distribution_persists_and_reads_back_from_key_store() {
    let flags = KeyDistribution::IDENTITY_KEY | KeyDistribution::SIGNING_KEY;
    let (mut initiator, mut responder) = pair(true, flags, flags);
    initiator.mark_encrypted();
    responder.mark_encrypted();
    relay(&mut initiator, &mut responder);

    let mut store = MemoryKeyStore::new();
    assert!(initiator.store_bond(&mut store).unwrap());
    let stored = store.get("C4:F2:17:1A:1D:BB").unwrap().unwrap();
    assert_eq!(stored.address_type, Some(AddressType::RANDOM_DEVICE));
    assert_eq!(stored.ltk.unwrap().value, vec![0x55; 16]);
    assert_eq!(stored.irk.unwrap().value, vec![0x22; 16]);
    assert_eq!(stored.csrk.unwrap().value, vec![0x23; 16]);
}

#[test]
fn unexpected_or_pre_encryption_distribution_fails() {
    let (mut initiator, _) = pair(
        false,
        KeyDistribution::IDENTITY_KEY,
        KeyDistribution::IDENTITY_KEY,
    );
    initiator.process(SmpPdu::IdentityInformation {
        identity_resolving_key: [1; 16],
    });
    assert_eq!(initiator.state(), KeyDistributionState::Failed);
    assert_eq!(
        initiator.failure(),
        Some(PairingFailureReason::UnspecifiedReason)
    );
    assert!(matches!(
        initiator.poll_outbound(),
        Some(SmpPdu::PairingFailed { reason: 0x08 })
    ));
}
