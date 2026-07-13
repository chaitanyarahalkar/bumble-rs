use std::sync::{Arc, Mutex};

use bumble::{Address, AddressType};
use bumble_crypto::EccKey;
use bumble_smp::{
    IoCapability, KeyDistribution, OobConfig, OobContext, PairingCapabilities, PairingConfig,
    PairingDelegate, PairingFailureReason, PairingMethod, PairingRole, ScPairingSession,
    ScPairingState, SmpPdu,
};

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

#[derive(Default)]
struct DelegateState {
    confirms: usize,
    comparisons: Vec<u32>,
    displayed: Vec<u32>,
}

struct PasskeyDelegate {
    passkey: u32,
    state: Arc<Mutex<DelegateState>>,
}

impl PairingDelegate for PasskeyDelegate {
    fn get_number(&mut self) -> Option<u32> {
        Some(self.passkey)
    }

    fn generate_passkey(&mut self) -> u32 {
        self.passkey
    }

    fn display_number(&mut self, number: u32, digits: u8) {
        assert_eq!(digits, 6);
        self.state.lock().unwrap().displayed.push(number);
    }
}

fn passkey_delegate(passkey: u32) -> (Box<dyn PairingDelegate>, Arc<Mutex<DelegateState>>) {
    let state = Arc::new(Mutex::new(DelegateState::default()));
    (
        Box::new(PasskeyDelegate {
            passkey,
            state: state.clone(),
        }),
        state,
    )
}

struct Delegate {
    accept: bool,
    approve: bool,
    state: Arc<Mutex<DelegateState>>,
}

impl PairingDelegate for Delegate {
    fn accept(&mut self) -> bool {
        self.accept
    }

    fn confirm(&mut self, _auto: bool) -> bool {
        self.state.lock().unwrap().confirms += 1;
        self.approve
    }

    fn compare_numbers(&mut self, number: u32, digits: u8) -> bool {
        assert_eq!(digits, 6);
        self.state.lock().unwrap().comparisons.push(number);
        self.approve
    }
}

fn delegate(accept: bool, approve: bool) -> (Box<dyn PairingDelegate>, Arc<Mutex<DelegateState>>) {
    let state = Arc::new(Mutex::new(DelegateState::default()));
    (
        Box::new(Delegate {
            accept,
            approve,
            state: state.clone(),
        }),
        state,
    )
}

fn config(io: IoCapability, mitm: bool, key_size: u8) -> PairingConfig {
    PairingConfig {
        secure_connections: true,
        ct2: false,
        mitm,
        bonding: true,
        capabilities: PairingCapabilities {
            io_capability: io,
            local_initiator_key_distribution: KeyDistribution::ALL,
            local_responder_key_distribution: KeyDistribution::DEFAULT,
            maximum_encryption_key_size: key_size,
        },
        identity_address_type: None,
        oob: None,
    }
}

fn sessions(
    initiator_config: PairingConfig,
    responder_config: PairingConfig,
    initiator_delegate: Box<dyn PairingDelegate>,
    responder_delegate: Box<dyn PairingDelegate>,
) -> (ScPairingSession, ScPairingSession) {
    let initiator_address = address("C4:F2:17:1A:1D:AA");
    let responder_address = address("C4:F2:17:1A:1D:BB");
    (
        ScPairingSession::new(
            PairingRole::Initiator,
            initiator_config,
            initiator_delegate,
            initiator_address.clone(),
            responder_address.clone(),
            EccKey::from_private_key_bytes(&(1u8..=32).collect::<Vec<_>>()).unwrap(),
            [0xA0; 16],
        )
        .unwrap(),
        ScPairingSession::new(
            PairingRole::Responder,
            responder_config,
            responder_delegate,
            initiator_address,
            responder_address,
            EccKey::from_private_key_bytes(&(33u8..=64).collect::<Vec<_>>()).unwrap(),
            [0xB0; 16],
        )
        .unwrap(),
    )
}

fn relay(initiator: &mut ScPairingSession, responder: &mut ScPairingSession) {
    for _ in 0..100 {
        let mut progress = false;
        for pdu in initiator.drain_outbound() {
            responder.process(pdu).unwrap();
            progress = true;
        }
        for pdu in responder.drain_outbound() {
            initiator.process(pdu).unwrap();
            progress = true;
        }
        if !progress {
            return;
        }
    }
    panic!("SC sessions did not quiesce");
}

#[test]
fn just_works_runs_public_key_nonce_and_dhkey_checks_to_matching_ltk() {
    let (initiator_delegate, initiator_state) = delegate(true, true);
    let (responder_delegate, responder_state) = delegate(true, true);
    let (mut initiator, mut responder) = sessions(
        config(IoCapability::NoInputNoOutput, false, 16),
        config(IoCapability::NoInputNoOutput, false, 12),
        initiator_delegate,
        responder_delegate,
    );
    initiator.start().unwrap();
    relay(&mut initiator, &mut responder);

    assert_eq!(initiator.state(), ScPairingState::WaitEncryption);
    assert_eq!(responder.state(), ScPairingState::WaitEncryption);
    assert_eq!(initiator.method(), Some(PairingMethod::JustWorks));
    assert_eq!(initiator.ltk(), responder.ltk());
    assert!(initiator.ltk().unwrap()[12..].iter().all(|byte| *byte == 0));
    assert_eq!(
        initiator.outcome().unwrap().mac_key,
        responder.outcome().unwrap().mac_key
    );
    assert_eq!(
        initiator.outcome().unwrap().numeric_check,
        responder.outcome().unwrap().numeric_check
    );
    assert!(!initiator.outcome().unwrap().authenticated);
    assert_eq!(initiator_state.lock().unwrap().confirms, 1);
    assert_eq!(responder_state.lock().unwrap().confirms, 1);

    initiator.mark_encrypted().unwrap();
    responder.mark_encrypted().unwrap();
    relay(&mut initiator, &mut responder);
    assert_eq!(initiator.state(), ScPairingState::Complete);
    assert_eq!(responder.state(), ScPairingState::Complete);
}

#[test]
fn numeric_comparison_shows_the_same_six_digit_value_to_both_delegates() {
    let (initiator_delegate, initiator_state) = delegate(true, true);
    let (responder_delegate, responder_state) = delegate(true, true);
    let (mut initiator, mut responder) = sessions(
        config(IoCapability::DisplayYesNo, true, 16),
        config(IoCapability::KeyboardDisplay, true, 16),
        initiator_delegate,
        responder_delegate,
    );
    initiator.start().unwrap();
    relay(&mut initiator, &mut responder);

    assert_eq!(initiator.method(), Some(PairingMethod::NumericComparison));
    assert_eq!(initiator.ltk(), responder.ltk());
    assert!(initiator.outcome().unwrap().authenticated);
    let initiator_numbers = &initiator_state.lock().unwrap().comparisons;
    let responder_numbers = &responder_state.lock().unwrap().comparisons;
    assert_eq!(initiator_numbers, responder_numbers);
    assert_eq!(initiator_numbers.len(), 1);
    assert!(initiator_numbers[0] < 1_000_000);
}

#[test]
fn numeric_rejection_and_tampered_confirm_fail_both_peers() {
    let (initiator_delegate, _) = delegate(true, false);
    let (responder_delegate, _) = delegate(true, true);
    let (mut initiator, mut responder) = sessions(
        config(IoCapability::DisplayYesNo, true, 16),
        config(IoCapability::DisplayYesNo, true, 16),
        initiator_delegate,
        responder_delegate,
    );
    initiator.start().unwrap();
    relay(&mut initiator, &mut responder);
    assert_eq!(initiator.state(), ScPairingState::Failed);
    assert_eq!(responder.state(), ScPairingState::Failed);
    assert_eq!(
        initiator.failure(),
        Some(PairingFailureReason::ConfirmValueFailed)
    );

    let (initiator_delegate, _) = delegate(true, true);
    let (responder_delegate, _) = delegate(true, true);
    let (mut initiator, mut responder) = sessions(
        config(IoCapability::NoInputNoOutput, false, 16),
        config(IoCapability::NoInputNoOutput, false, 16),
        initiator_delegate,
        responder_delegate,
    );
    initiator.start().unwrap();
    responder
        .process(initiator.poll_outbound().unwrap())
        .unwrap();
    initiator
        .process(responder.poll_outbound().unwrap())
        .unwrap();
    responder
        .process(initiator.poll_outbound().unwrap())
        .unwrap();
    initiator
        .process(responder.poll_outbound().unwrap())
        .unwrap(); // public key
    let confirm = responder.poll_outbound().unwrap();
    assert!(matches!(confirm, SmpPdu::PairingConfirm { .. }));
    initiator
        .process(SmpPdu::PairingConfirm {
            confirm_value: [0xFF; 16],
        })
        .unwrap();
    responder
        .process(initiator.poll_outbound().unwrap())
        .unwrap();
    initiator
        .process(responder.poll_outbound().unwrap())
        .unwrap();
    relay(&mut initiator, &mut responder);
    assert_eq!(initiator.state(), ScPairingState::Failed);
    assert_eq!(responder.state(), ScPairingState::Failed);
    assert_eq!(
        initiator.failure(),
        Some(PairingFailureReason::ConfirmValueFailed)
    );
}

#[test]
fn invalid_peer_public_key_is_rejected_before_nonce_exchange() {
    let (initiator_delegate, _) = delegate(true, true);
    let (responder_delegate, _) = delegate(true, true);
    let (mut initiator, mut responder) = sessions(
        config(IoCapability::NoInputNoOutput, false, 16),
        config(IoCapability::NoInputNoOutput, false, 16),
        initiator_delegate,
        responder_delegate,
    );
    initiator.start().unwrap();
    responder
        .process(initiator.poll_outbound().unwrap())
        .unwrap();
    initiator
        .process(responder.poll_outbound().unwrap())
        .unwrap();
    let _real_key = initiator.poll_outbound().unwrap();
    responder
        .process(SmpPdu::PairingPublicKey {
            public_key_x: [0; 32],
            public_key_y: [0; 32],
        })
        .unwrap();
    assert_eq!(responder.state(), ScPairingState::Failed);
    assert_eq!(
        responder.failure(),
        Some(PairingFailureReason::InvalidParameters)
    );
}

#[test]
fn passkey_completes_all_twenty_commitment_rounds() {
    let passkey = 678_901;
    let (initiator_delegate, initiator_state) = passkey_delegate(passkey);
    let (responder_delegate, responder_state) = passkey_delegate(passkey);
    let (mut initiator, mut responder) = sessions(
        config(IoCapability::DisplayOnly, true, 16),
        config(IoCapability::KeyboardOnly, true, 16),
        initiator_delegate,
        responder_delegate,
    );
    initiator.start().unwrap();
    relay(&mut initiator, &mut responder);

    assert_eq!(initiator.state(), ScPairingState::WaitEncryption);
    assert_eq!(responder.state(), ScPairingState::WaitEncryption);
    assert_eq!(initiator.method(), Some(PairingMethod::Passkey));
    assert_eq!(initiator.ltk(), responder.ltk());
    assert_eq!(
        initiator.outcome().unwrap().mac_key,
        responder.outcome().unwrap().mac_key
    );
    assert!(initiator.outcome().unwrap().authenticated);
    assert_eq!(initiator_state.lock().unwrap().displayed, vec![passkey]);
    assert!(responder_state.lock().unwrap().displayed.is_empty());
}

#[test]
fn wrong_sc_passkey_fails_during_commitment_rounds() {
    let (initiator_delegate, _) = passkey_delegate(123_456);
    let (responder_delegate, _) = passkey_delegate(123_457);
    let (mut initiator, mut responder) = sessions(
        config(IoCapability::DisplayOnly, true, 16),
        config(IoCapability::KeyboardOnly, true, 16),
        initiator_delegate,
        responder_delegate,
    );
    initiator.start().unwrap();
    relay(&mut initiator, &mut responder);
    assert_eq!(initiator.state(), ScPairingState::Failed);
    assert_eq!(responder.state(), ScPairingState::Failed);
    assert_eq!(
        initiator.failure(),
        Some(PairingFailureReason::ConfirmValueFailed)
    );
}

#[test]
fn sc_oob_verifies_shared_data_and_uses_oob_r_in_dhkey_checks() {
    let initiator_context = OobContext::new(
        Some(EccKey::from_private_key_bytes(&(1u8..=32).collect::<Vec<_>>()).unwrap()),
        Some([0x31; 16]),
    );
    let responder_context = OobContext::new(
        Some(EccKey::from_private_key_bytes(&(33u8..=64).collect::<Vec<_>>()).unwrap()),
        Some([0x42; 16]),
    );
    let initiator_shared = initiator_context.share();
    let responder_shared = responder_context.share();
    let mut initiator_config = config(IoCapability::NoInputNoOutput, true, 16);
    initiator_config.oob = Some(OobConfig {
        our_context: Some(initiator_context),
        peer_data: Some(responder_shared),
        legacy_context: None,
    });
    let mut responder_config = config(IoCapability::NoInputNoOutput, true, 16);
    responder_config.oob = Some(OobConfig {
        our_context: Some(responder_context),
        peer_data: Some(initiator_shared),
        legacy_context: None,
    });
    let (initiator_delegate, initiator_state) = delegate(true, false);
    let (responder_delegate, responder_state) = delegate(true, false);
    let (mut initiator, mut responder) = sessions(
        initiator_config,
        responder_config,
        initiator_delegate,
        responder_delegate,
    );
    initiator.start().unwrap();
    relay(&mut initiator, &mut responder);

    assert_eq!(initiator.method(), Some(PairingMethod::Oob));
    assert_eq!(initiator.state(), ScPairingState::WaitEncryption);
    assert_eq!(responder.state(), ScPairingState::WaitEncryption);
    assert_eq!(initiator.ltk(), responder.ltk());
    assert!(initiator.outcome().unwrap().authenticated);
    assert_eq!(initiator_state.lock().unwrap().confirms, 0);
    assert_eq!(responder_state.lock().unwrap().confirms, 0);
}

#[test]
fn tampered_sc_oob_confirmation_is_rejected_at_public_key_exchange() {
    let initiator_context = OobContext::new(
        Some(EccKey::from_private_key_bytes(&(1u8..=32).collect::<Vec<_>>()).unwrap()),
        Some([0x31; 16]),
    );
    let responder_context = OobContext::new(
        Some(EccKey::from_private_key_bytes(&(33u8..=64).collect::<Vec<_>>()).unwrap()),
        Some([0x42; 16]),
    );
    let mut bad_responder_data = responder_context.share();
    bad_responder_data.c[0] ^= 0x80;
    let initiator_shared = initiator_context.share();
    let mut initiator_config = config(IoCapability::NoInputNoOutput, true, 16);
    initiator_config.oob = Some(OobConfig {
        our_context: Some(initiator_context),
        peer_data: Some(bad_responder_data),
        legacy_context: None,
    });
    let mut responder_config = config(IoCapability::NoInputNoOutput, true, 16);
    responder_config.oob = Some(OobConfig {
        our_context: Some(responder_context),
        peer_data: Some(initiator_shared),
        legacy_context: None,
    });
    let (initiator_delegate, _) = delegate(true, true);
    let (responder_delegate, _) = delegate(true, true);
    let (mut initiator, mut responder) = sessions(
        initiator_config,
        responder_config,
        initiator_delegate,
        responder_delegate,
    );
    initiator.start().unwrap();
    relay(&mut initiator, &mut responder);
    assert_eq!(initiator.state(), ScPairingState::Failed);
    assert_eq!(responder.state(), ScPairingState::Failed);
    assert_eq!(
        initiator.failure(),
        Some(PairingFailureReason::ConfirmValueFailed)
    );
}

#[test]
fn ct2_is_negotiated_and_used_for_link_key_distribution() {
    let (initiator_delegate, _) = delegate(true, true);
    let (responder_delegate, _) = delegate(true, true);
    let mut initiator_config = config(IoCapability::NoInputNoOutput, false, 16);
    initiator_config.ct2 = true;
    initiator_config
        .capabilities
        .local_initiator_key_distribution = KeyDistribution::ALL;
    initiator_config
        .capabilities
        .local_responder_key_distribution = KeyDistribution::ALL;
    let mut responder_config = config(IoCapability::NoInputNoOutput, false, 16);
    responder_config.ct2 = true;
    responder_config
        .capabilities
        .local_initiator_key_distribution = KeyDistribution::ALL;
    responder_config
        .capabilities
        .local_responder_key_distribution = KeyDistribution::ALL;
    let (mut initiator, mut responder) = sessions(
        initiator_config,
        responder_config,
        initiator_delegate,
        responder_delegate,
    );
    initiator.start().unwrap();
    relay(&mut initiator, &mut responder);
    assert!(initiator.outcome().unwrap().ct2);
    assert!(responder.outcome().unwrap().ct2);
    initiator.mark_encrypted().unwrap();
    responder.mark_encrypted().unwrap();
    relay(&mut initiator, &mut responder);
    let expected = bumble_smp::derive_link_key(&initiator.ltk().unwrap(), true);
    assert_eq!(
        initiator.pairing_keys().unwrap().link_key.unwrap().value,
        expected
    );
    assert_eq!(
        responder.pairing_keys().unwrap().link_key.unwrap().value,
        expected
    );
}
