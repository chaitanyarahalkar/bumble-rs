use bumble_l2cap::{
    EnhancedControlField, ErtmConfig, ErtmEngine, SegmentationAndReassembly, SupervisoryFunction,
};

fn pair(peer_mps: u16, window: u8) -> (ErtmEngine, ErtmEngine) {
    let left = ErtmEngine::new(ErtmConfig {
        local_mtu: 2_000,
        peer_mtu: 2_000,
        local_mps: peer_mps,
        peer_mps,
        tx_window_size: window,
        max_retransmissions: 3,
        retransmission_timeout_ticks: 10,
    })
    .unwrap();
    (left.clone(), left)
}

fn relay(left: &mut ErtmEngine, right: &mut ErtmEngine) {
    for _ in 0..100_000 {
        let mut progress = false;
        for frame in left.drain_outbound() {
            right.receive_frame(&frame).unwrap();
            progress = true;
        }
        for frame in right.drain_outbound() {
            left.receive_frame(&frame).unwrap();
            progress = true;
        }
        if !progress {
            return;
        }
    }
    panic!("ERTM engines did not quiesce");
}

#[test]
fn enhanced_control_fields_match_upstream_vectors_and_round_trip() {
    let information = EnhancedControlField::Information {
        tx_seq: 37,
        req_seq: 18,
        sar: SegmentationAndReassembly::Continuation,
        final_bit: true,
    };
    assert_eq!(information.to_bytes().unwrap(), [0xCA, 0xD2]);
    assert_eq!(
        EnhancedControlField::from_bytes(&information.to_bytes().unwrap()).unwrap(),
        information
    );

    let reject = EnhancedControlField::Supervisory {
        function: SupervisoryFunction::Reject,
        poll: false,
        final_bit: true,
        req_seq: 42,
    };
    assert_eq!(reject.to_bytes().unwrap(), [0x85, 0x2A]);
    assert_eq!(
        EnhancedControlField::from_bytes(&reject.to_bytes().unwrap()).unwrap(),
        reject
    );

    // Bumble currently aliases a poll to bit 7 when serializing. The Rust
    // codec keeps the Bluetooth control-field distinction: P is bit 4 and F
    // is bit 7, while parsing accepts both independently.
    let poll = EnhancedControlField::Supervisory {
        function: SupervisoryFunction::ReceiverReady,
        poll: true,
        final_bit: false,
        req_seq: 7,
    };
    assert_eq!(poll.to_bytes().unwrap(), [0x11, 0x07]);
    assert_eq!(
        EnhancedControlField::from_bytes(&poll.to_bytes().unwrap()).unwrap(),
        poll
    );

    assert!(EnhancedControlField::from_bytes(&[0]).is_err());
    assert!(EnhancedControlField::Information {
        tx_seq: 64,
        final_bit: false,
        req_seq: 0,
        sar: SegmentationAndReassembly::Unsegmented,
    }
    .to_bytes()
    .is_err());
}

#[test]
fn segments_reassembles_respects_window_and_wraps_sequence_numbers() {
    let (mut left, mut right) = pair(23, 3);
    for index in 0..70u8 {
        let payload: Vec<_> = (0..(47 + usize::from(index % 5)))
            .map(|offset| index.wrapping_add(offset as u8))
            .collect();
        left.send_sdu(&payload).unwrap();
        assert!(left.unacked_frames() <= 3);
        relay(&mut left, &mut right);
        assert_eq!(right.pop_received(), Some(payload));
        assert_eq!(left.pending_frames(), 0);
    }

    let reply = vec![0xE7; 777];
    right.send_sdu(&reply).unwrap();
    relay(&mut left, &mut right);
    assert_eq!(left.pop_received(), Some(reply));
    assert_eq!(right.pending_frames(), 0);
}

#[test]
fn reject_retransmits_a_lost_window_without_duplicate_delivery() {
    let (mut sender, mut receiver) = pair(50, 3);
    let payload: Vec<_> = (0..150u8).collect();
    sender.send_sdu(&payload).unwrap();
    let mut first_window = sender.drain_outbound();
    assert_eq!(first_window.len(), 3);

    // Lose sequence 0. Sequences 1 and 2 both provoke REJ(0); the sender
    // retransmits its entire still-unacknowledged window.
    first_window.remove(0);
    for frame in first_window {
        receiver.receive_frame(&frame).unwrap();
    }
    for reject in receiver.drain_outbound() {
        sender.receive_frame(&reject).unwrap();
    }
    relay(&mut sender, &mut receiver);

    assert_eq!(receiver.pop_received(), Some(payload));
    assert!(receiver.pop_received().is_none());
    assert_eq!(sender.pending_frames(), 0);
}

#[test]
fn receiver_busy_stalls_and_ready_resumes_output() {
    let (mut sender, mut receiver) = pair(100, 2);
    receiver.set_receiver_busy(true).unwrap();
    relay(&mut sender, &mut receiver);
    assert!(sender.remote_is_busy());

    sender.send_sdu(b"held until RNR clears").unwrap();
    assert!(sender.poll_outbound().is_none());
    assert_eq!(sender.pending_frames(), 1);

    receiver.set_receiver_busy(false).unwrap();
    relay(&mut sender, &mut receiver);
    assert!(!sender.remote_is_busy());
    assert_eq!(
        receiver.pop_received(),
        Some(b"held until RNR clears".to_vec())
    );
    assert_eq!(sender.pending_frames(), 0);
}

#[test]
fn logical_timeout_retransmits_and_enforces_retry_limit() {
    let mut sender = ErtmEngine::new(ErtmConfig {
        local_mtu: 100,
        peer_mtu: 100,
        local_mps: 50,
        peer_mps: 50,
        tx_window_size: 1,
        max_retransmissions: 2,
        retransmission_timeout_ticks: 5,
    })
    .unwrap();
    sender.send_sdu(b"lost frame").unwrap();
    let original = sender.poll_outbound().unwrap();

    sender.tick(4).unwrap();
    assert!(sender.poll_outbound().is_none());
    sender.tick(1).unwrap();
    assert_eq!(sender.poll_outbound(), Some(original.clone()));
    sender.tick(5).unwrap();
    assert_eq!(sender.poll_outbound(), Some(original));
    assert!(sender.tick(5).is_err());
    assert!(sender.is_failed());
    assert!(sender.send_sdu(b"after failure").is_err());
    assert_eq!(sender.unacked_frames(), 1);
}

#[test]
fn rejects_malformed_acknowledgments_and_sar_sequences() {
    let (mut sender, mut receiver) = pair(50, 3);
    sender.send_sdu(b"one").unwrap();
    assert!(sender
        .receive_frame(
            &EnhancedControlField::Supervisory {
                function: SupervisoryFunction::ReceiverReady,
                poll: false,
                final_bit: false,
                req_seq: 2,
            }
            .to_bytes()
            .unwrap()
        )
        .is_err());

    let continuation = EnhancedControlField::Information {
        tx_seq: 0,
        final_bit: false,
        req_seq: 0,
        sar: SegmentationAndReassembly::Continuation,
    }
    .to_bytes()
    .unwrap();
    assert!(receiver.receive_frame(&continuation).is_err());

    assert!(ErtmEngine::new(ErtmConfig {
        tx_window_size: 0,
        ..ErtmConfig::default()
    })
    .is_err());
}
