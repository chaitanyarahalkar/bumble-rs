use bumble_l2cap::{LeCreditBasedChannel, LeCreditBasedChannelSpec, LeCreditBasedChannelState};

fn make_channel(credits: u16) -> LeCreditBasedChannel {
    LeCreditBasedChannel::connected(
        0x0025,
        0x0040,
        0x0041,
        LeCreditBasedChannelSpec {
            psm: Some(0x0025),
            mtu: 64,
            mps: 23,
            max_credits: 4,
        },
        30,
        23,
        credits,
    )
    .unwrap()
}

#[test]
fn validates_channel_parameters() {
    assert!(LeCreditBasedChannelSpec {
        max_credits: 0,
        ..LeCreditBasedChannelSpec::default()
    }
    .validate()
    .is_err());
    assert!(LeCreditBasedChannelSpec {
        mtu: 22,
        ..LeCreditBasedChannelSpec::default()
    }
    .validate()
    .is_err());
    assert!(LeCreditBasedChannelSpec {
        mps: 22,
        ..LeCreditBasedChannelSpec::default()
    }
    .validate()
    .is_err());
    assert!(LeCreditBasedChannel::connected(
        1,
        0x40,
        0x41,
        LeCreditBasedChannelSpec::default(),
        22,
        23,
        1,
    )
    .is_err());
}

#[test]
fn segments_stream_into_mtu_bounded_sdus_and_mps_bounded_pdus() {
    let mut channel = make_channel(2);
    let payload: Vec<u8> = (0..50).collect();
    channel.write(&payload).unwrap();
    assert_eq!(channel.credits, 0);

    let first = channel.poll_outbound_pdu().unwrap();
    assert_eq!(first.len(), 23);
    assert_eq!(&first[..2], &[30, 0]);
    assert_eq!(&first[2..], &payload[..21]);
    let second = channel.poll_outbound_pdu().unwrap();
    assert_eq!(second, payload[21..30]);
    assert!(channel.poll_outbound_pdu().is_none());
    assert!(!channel.is_drained());

    channel.add_credits(1).unwrap();
    let third = channel.poll_outbound_pdu().unwrap();
    assert_eq!(&third[..2], &[20, 0]);
    assert_eq!(&third[2..], &payload[30..]);
    assert!(channel.is_drained());
}

#[test]
fn reassembles_inbound_sdus_and_replenishes_credits_at_threshold() {
    let mut channel = make_channel(3);
    let payload: Vec<u8> = (100..130).collect();
    let mut framed = vec![30, 0];
    framed.extend_from_slice(&payload);
    channel.receive_pdu(&framed[..23]).unwrap();
    assert!(channel.pop_received().is_none());
    assert_eq!(channel.peer_credits, 3);
    assert!(channel.poll_credit_grant().is_none());

    channel.receive_pdu(&framed[23..]).unwrap();
    assert_eq!(channel.pop_received().unwrap(), payload);
    assert_eq!(channel.peer_credits, 4);
    assert_eq!(channel.poll_credit_grant(), Some(2));
    assert_eq!(channel.poll_credit_grant(), None);
}

#[test]
fn rejects_credit_mps_mtu_and_sdu_overflow_violations() {
    let mut channel = make_channel(u16::MAX);
    assert!(channel.add_credits(1).is_err());
    assert!(channel.receive_pdu(&[0; 24]).is_err());

    let mut channel = make_channel(1);
    channel.receive_pdu(&[65, 0]).unwrap_err();
    channel.receive_pdu(&[3, 0, 1, 2, 3, 4]).unwrap_err();

    let mut no_credits = LeCreditBasedChannel::connected(
        1,
        0x40,
        0x41,
        LeCreditBasedChannelSpec {
            max_credits: 1,
            ..LeCreditBasedChannelSpec::default()
        },
        23,
        23,
        1,
    )
    .unwrap();
    no_credits.peer_credits = 0;
    assert!(no_credits.receive_pdu(&[0, 0]).is_err());
}

#[test]
fn disconnect_flushes_state_and_blocks_io() {
    let mut channel = make_channel(0);
    channel.write(b"queued").unwrap();
    channel.disconnect().unwrap();
    assert_eq!(channel.state, LeCreditBasedChannelState::Disconnected);
    assert!(channel.write(b"later").is_err());
    assert!(channel.add_credits(1).is_err());
    assert!(channel.receive_pdu(&[0, 0]).is_err());
    assert!(channel.is_drained());
}
