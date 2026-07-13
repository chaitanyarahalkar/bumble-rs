use std::sync::{Arc, Mutex};

use bumble::{Address, AddressType};
use bumble_crypto::EccKey;
use bumble_smp::{
    IoCapability, KeyDistribution, PairingCapabilities, PairingConfig, PairingDelegate,
    PairingFailureReason, PairingMethod, PairingRole, ScPairingSession, ScPairingState, SmpPdu,
};

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

#[derive(Default)]
struct DelegateState {
    confirms: usize,
    comparisons: Vec<u32>,
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
