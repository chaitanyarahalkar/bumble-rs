use std::sync::{Arc, Mutex};

use bumble::{Address, AddressType};
use bumble_smp::{
    IoCapability, KeyDistribution, LegacyPairingSession, OobConfig, OobLegacyContext,
    PairingCapabilities, PairingConfig, PairingDelegate, PairingFailureReason, PairingMethod,
    PairingRole, PairingState, SmpPdu,
};

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

#[derive(Default)]
struct DelegateLog {
    displayed: Vec<(u32, u8)>,
}

struct FixedDelegate {
    accept: bool,
    confirm: bool,
    passkey: Option<u32>,
    log: Arc<Mutex<DelegateLog>>,
}

impl PairingDelegate for FixedDelegate {
    fn accept(&mut self) -> bool {
        self.accept
    }

    fn confirm(&mut self, _auto: bool) -> bool {
        self.confirm
    }

    fn get_number(&mut self) -> Option<u32> {
        self.passkey
    }

    fn generate_passkey(&mut self) -> u32 {
        self.passkey.unwrap_or(0)
    }

    fn display_number(&mut self, number: u32, digits: u8) {
        self.log.lock().unwrap().displayed.push((number, digits));
    }
}

fn delegate(
    accept: bool,
    confirm: bool,
    passkey: Option<u32>,
) -> (Box<dyn PairingDelegate>, Arc<Mutex<DelegateLog>>) {
    let log = Arc::new(Mutex::new(DelegateLog::default()));
    (
        Box::new(FixedDelegate {
            accept,
            confirm,
            passkey,
            log: log.clone(),
        }),
        log,
    )
}

fn config(
    io_capability: IoCapability,
    mitm: bool,
    key_size: u8,
    initiator_keys: KeyDistribution,
    responder_keys: KeyDistribution,
) -> PairingConfig {
    PairingConfig {
        secure_connections: false,
        ct2: false,
        mitm,
        bonding: true,
        capabilities: PairingCapabilities {
            io_capability,
            local_initiator_key_distribution: initiator_keys,
            local_responder_key_distribution: responder_keys,
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
) -> (LegacyPairingSession, LegacyPairingSession) {
    let initiator_address = address("C4:F2:17:1A:1D:AA");
    let responder_address = address("C4:F2:17:1A:1D:BB");
    (
        LegacyPairingSession::new(
            PairingRole::Initiator,
            initiator_config,
            initiator_delegate,
            initiator_address.clone(),
            responder_address.clone(),
            [0x11; 16],
        )
        .unwrap(),
        LegacyPairingSession::new(
            PairingRole::Responder,
            responder_config,
            responder_delegate,
            initiator_address,
            responder_address,
            [0x22; 16],
        )
        .unwrap(),
    )
}

fn relay(initiator: &mut LegacyPairingSession, responder: &mut LegacyPairingSession) {
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
    panic!("Legacy SMP sessions did not quiesce");
}

#[test]
fn just_works_negotiates_features_and_derives_the_same_truncated_stk() {
    let (initiator_delegate, _) = delegate(true, true, Some(0));
    let (responder_delegate, _) = delegate(true, true, Some(0));
    let (mut initiator, mut responder) = sessions(
        config(
            IoCapability::NoInputNoOutput,
            false,
            16,
            KeyDistribution::ALL,
            KeyDistribution::IDENTITY_KEY | KeyDistribution::SIGNING_KEY,
        ),
        config(
            IoCapability::NoInputNoOutput,
            false,
            12,
            KeyDistribution::ENCRYPTION_KEY | KeyDistribution::IDENTITY_KEY,
            KeyDistribution::ALL,
        ),
        initiator_delegate,
        responder_delegate,
    );

    initiator.start().unwrap();
    relay(&mut initiator, &mut responder);
    assert_eq!(initiator.state(), PairingState::WaitEncryption);
    assert_eq!(responder.state(), PairingState::WaitEncryption);
    assert_eq!(initiator.method(), Some(PairingMethod::JustWorks));
    assert_eq!(initiator.stk(), responder.stk());
    let stk = initiator.stk().unwrap();
    assert!(stk[12..].iter().all(|byte| *byte == 0));

    let outcome = initiator.outcome().unwrap();
    assert!(!outcome.authenticated);
    assert!(outcome.bonding);
    assert_eq!(outcome.maximum_encryption_key_size, 12);
    assert_eq!(
        outcome.initiator_key_distribution,
        KeyDistribution::ENCRYPTION_KEY | KeyDistribution::IDENTITY_KEY
    );
    assert_eq!(
        outcome.responder_key_distribution,
        KeyDistribution::IDENTITY_KEY | KeyDistribution::SIGNING_KEY
    );

    initiator.mark_encrypted().unwrap();
    responder.mark_encrypted().unwrap();
    relay(&mut initiator, &mut responder);
    assert_eq!(initiator.state(), PairingState::Complete);
    assert_eq!(responder.state(), PairingState::Complete);
}

#[test]
fn passkey_display_and_input_delegate_actions_produce_authenticated_stk() {
    let passkey = 678_901;
    let (initiator_delegate, initiator_log) = delegate(true, true, Some(passkey));
    let (responder_delegate, responder_log) = delegate(true, true, Some(passkey));
    let (mut initiator, mut responder) = sessions(
        config(
            IoCapability::DisplayOnly,
            true,
            16,
            KeyDistribution::DEFAULT,
            KeyDistribution::DEFAULT,
        ),
        config(
            IoCapability::KeyboardOnly,
            true,
            16,
            KeyDistribution::DEFAULT,
            KeyDistribution::DEFAULT,
        ),
        initiator_delegate,
        responder_delegate,
    );
    initiator.start().unwrap();
    relay(&mut initiator, &mut responder);

    assert_eq!(initiator.method(), Some(PairingMethod::Passkey));
    assert_eq!(initiator.stk(), responder.stk());
    assert!(initiator.outcome().unwrap().authenticated);
    assert_eq!(initiator_log.lock().unwrap().displayed, vec![(passkey, 6)]);
    assert!(responder_log.lock().unwrap().displayed.is_empty());
}

#[test]
fn legacy_oob_uses_shared_tk_without_user_passkey() {
    let tk: Vec<_> = (0x40u8..0x50).collect();
    let mut initiator_config = config(
        IoCapability::NoInputNoOutput,
        true,
        16,
        KeyDistribution::DEFAULT,
        KeyDistribution::DEFAULT,
    );
    initiator_config.oob = Some(OobConfig {
        our_context: None,
        peer_data: None,
        legacy_context: Some(OobLegacyContext { tk: tk.clone() }),
    });
    let mut responder_config = config(
        IoCapability::NoInputNoOutput,
        true,
        16,
        KeyDistribution::DEFAULT,
        KeyDistribution::DEFAULT,
    );
    responder_config.oob = Some(OobConfig {
        our_context: None,
        peer_data: None,
        legacy_context: Some(OobLegacyContext { tk }),
    });
    let (initiator_delegate, _) = delegate(true, true, None);
    let (responder_delegate, _) = delegate(true, true, None);
    let (mut initiator, mut responder) = sessions(
        initiator_config,
        responder_config,
        initiator_delegate,
        responder_delegate,
    );
    initiator.start().unwrap();
    relay(&mut initiator, &mut responder);
    assert_eq!(initiator.method(), Some(PairingMethod::Oob));
    assert_eq!(initiator.stk(), responder.stk());
    assert!(initiator.outcome().unwrap().authenticated);
}

#[test]
fn delegate_rejection_and_wrong_passkey_propagate_pairing_failed() {
    let (initiator_delegate, _) = delegate(true, true, Some(1));
    let (responder_delegate, _) = delegate(false, true, Some(1));
    let (mut initiator, mut responder) = sessions(
        config(
            IoCapability::NoInputNoOutput,
            false,
            16,
            KeyDistribution::DEFAULT,
            KeyDistribution::DEFAULT,
        ),
        config(
            IoCapability::NoInputNoOutput,
            false,
            16,
            KeyDistribution::DEFAULT,
            KeyDistribution::DEFAULT,
        ),
        initiator_delegate,
        responder_delegate,
    );
    initiator.start().unwrap();
    relay(&mut initiator, &mut responder);
    assert_eq!(initiator.state(), PairingState::Failed);
    assert_eq!(responder.state(), PairingState::Failed);
    assert_eq!(
        initiator.failure(),
        Some(PairingFailureReason::PairingNotSupported)
    );

    let (initiator_delegate, _) = delegate(true, true, Some(123_456));
    let (responder_delegate, _) = delegate(true, true, Some(654_321));
    let (mut initiator, mut responder) = sessions(
        config(
            IoCapability::DisplayOnly,
            true,
            16,
            KeyDistribution::DEFAULT,
            KeyDistribution::DEFAULT,
        ),
        config(
            IoCapability::KeyboardOnly,
            true,
            16,
            KeyDistribution::DEFAULT,
            KeyDistribution::DEFAULT,
        ),
        initiator_delegate,
        responder_delegate,
    );
    initiator.start().unwrap();
    relay(&mut initiator, &mut responder);
    assert_eq!(initiator.state(), PairingState::Failed);
    assert_eq!(responder.state(), PairingState::Failed);
    assert_eq!(
        initiator.failure(),
        Some(PairingFailureReason::ConfirmValueFailed)
    );
}

#[test]
fn invalid_order_is_rejected_with_protocol_failure() {
    let (initiator_delegate, _) = delegate(true, true, Some(0));
    let (responder_delegate, _) = delegate(true, true, Some(0));
    let (mut initiator, mut responder) = sessions(
        config(
            IoCapability::NoInputNoOutput,
            false,
            16,
            KeyDistribution::DEFAULT,
            KeyDistribution::DEFAULT,
        ),
        config(
            IoCapability::NoInputNoOutput,
            false,
            16,
            KeyDistribution::DEFAULT,
            KeyDistribution::DEFAULT,
        ),
        initiator_delegate,
        responder_delegate,
    );
    responder
        .process(SmpPdu::PairingRandom {
            random_value: [0; 16],
        })
        .unwrap();
    assert_eq!(responder.state(), PairingState::Failed);
    assert!(matches!(
        responder.poll_outbound(),
        Some(SmpPdu::PairingFailed { reason: 0x0A })
    ));
    assert!(initiator.mark_encrypted().is_err());
}
