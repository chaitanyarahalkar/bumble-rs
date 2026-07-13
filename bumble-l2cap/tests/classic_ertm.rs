use bumble_l2cap::{
    ChannelManager, ClassicChannelMode, ClassicChannelSpec, ClassicChannelState, ErtmChannelSpec,
    CONFIGURATION_UNACCEPTABLE_PARAMETERS,
};

fn relay(left: &mut ChannelManager, right: &mut ChannelManager) {
    for _ in 0..100_000 {
        let mut progress = false;
        while let Some(pdu) = left.poll_outbound() {
            right.process_pdu(pdu).unwrap();
            progress = true;
        }
        while let Some(pdu) = right.poll_outbound() {
            left.process_pdu(pdu).unwrap();
            progress = true;
        }
        if !progress {
            return;
        }
    }
    panic!("Classic ERTM relay did not quiesce");
}

fn connect_pair(
    client_spec: ErtmChannelSpec,
    server_spec: ErtmChannelSpec,
) -> (ChannelManager, u16, ChannelManager, u16) {
    let mut client = ChannelManager::new();
    let mut server = ChannelManager::new();
    let psm = server
        .register_ertm_server(Some(0x1001), server_spec)
        .unwrap();
    let client_cid = client.connect_ertm(psm, client_spec).unwrap();
    relay(&mut client, &mut server);
    let server_cid = server.poll_accepted_channel().unwrap();
    (client, client_cid, server, server_cid)
}

#[test]
fn upstream_mtu_matrix_transfers_bidirectionally_over_live_ertm_channels() {
    for mtu in [50, 255, 256, 1_000] {
        let client_spec = ErtmChannelSpec {
            mtu,
            mps: 1_024,
            tx_window_size: 3,
            ..ErtmChannelSpec::default()
        };
        let server_spec = ErtmChannelSpec {
            mtu,
            mps: 256,
            tx_window_size: 2,
            ..ErtmChannelSpec::default()
        };
        let (mut client, client_cid, mut server, server_cid) =
            connect_pair(client_spec, server_spec);

        let client_channel = client.channel(client_cid).unwrap();
        let server_channel = server.channel(server_cid).unwrap();
        assert_eq!(client_channel.state, ClassicChannelState::Open);
        assert_eq!(server_channel.state, ClassicChannelState::Open);
        assert_eq!(
            client_channel.mode,
            ClassicChannelMode::EnhancedRetransmission
        );
        assert_eq!(client_channel.peer_mps, 256);
        assert_eq!(server_channel.peer_mps, 1_024);

        let messages: Vec<Vec<u8>> = [21usize, 70, 700, 5_523]
            .into_iter()
            .map(|length| (0..length).map(|index| (index % 8) as u8).collect())
            .collect();
        for message in &messages {
            client.send(client_cid, message).unwrap();
        }
        relay(&mut client, &mut server);
        let received: Vec<_> =
            std::iter::from_fn(|| server.channel_mut(server_cid).unwrap().pop_received()).collect();
        assert_eq!(received, messages);

        for message in &messages {
            server.send(server_cid, message).unwrap();
        }
        relay(&mut client, &mut server);
        let received: Vec<_> =
            std::iter::from_fn(|| client.channel_mut(client_cid).unwrap().pop_received()).collect();
        assert_eq!(received, messages);
        assert_eq!(client.ertm_pending_frames(client_cid), Some(0));
        assert_eq!(server.ertm_pending_frames(server_cid), Some(0));
    }
}

#[test]
fn live_channel_recovers_a_dropped_window_via_reject() {
    let spec = ErtmChannelSpec {
        mtu: 1_000,
        mps: 100,
        tx_window_size: 3,
        max_retransmissions: 3,
        ..ErtmChannelSpec::default()
    };
    let (mut client, client_cid, mut server, server_cid) = connect_pair(spec, spec);
    let payload: Vec<_> = (0..300).map(|value| value as u8).collect();
    client.send(client_cid, &payload).unwrap();
    let mut window = client.drain_outbound();
    assert_eq!(window.len(), 3);

    window.remove(0);
    for pdu in window {
        server.process_pdu(pdu).unwrap();
    }
    while let Some(reject) = server.poll_outbound() {
        client.process_pdu(reject).unwrap();
    }
    relay(&mut client, &mut server);
    assert_eq!(
        server.channel_mut(server_cid).unwrap().pop_received(),
        Some(payload)
    );
    assert!(server
        .channel_mut(server_cid)
        .unwrap()
        .pop_received()
        .is_none());
    assert_eq!(client.ertm_pending_frames(client_cid), Some(0));
}

#[test]
fn live_busy_and_logical_timeout_paths_resume_without_data_loss() {
    let spec = ErtmChannelSpec {
        mtu: 500,
        mps: 100,
        tx_window_size: 1,
        max_retransmissions: 2,
        retransmission_timeout_ms: 5,
        ..ErtmChannelSpec::default()
    };
    let (mut client, client_cid, mut server, server_cid) = connect_pair(spec, spec);

    server.set_receiver_busy(server_cid, true).unwrap();
    relay(&mut client, &mut server);
    client.send(client_cid, b"wait behind RNR").unwrap();
    assert!(client.poll_outbound().is_none());
    server.set_receiver_busy(server_cid, false).unwrap();
    relay(&mut client, &mut server);
    assert_eq!(
        server.channel_mut(server_cid).unwrap().pop_received(),
        Some(b"wait behind RNR".to_vec())
    );

    client
        .send(client_cid, b"first transmission is lost")
        .unwrap();
    let lost = client.poll_outbound().unwrap();
    client.tick(4).unwrap();
    assert!(client.poll_outbound().is_none());
    client.tick(1).unwrap();
    assert_eq!(client.poll_outbound(), Some(lost.clone()));
    server.process_pdu(lost).unwrap();
    relay(&mut client, &mut server);
    assert_eq!(
        server.channel_mut(server_cid).unwrap().pop_received(),
        Some(b"first transmission is lost".to_vec())
    );
}

#[test]
fn optional_fcs_is_verified_before_ertm_processing() {
    let spec = ErtmChannelSpec {
        mtu: 200,
        mps: 100,
        fcs_enabled: true,
        ..ErtmChannelSpec::default()
    };
    let mut server = ChannelManager::new();
    let basic_psm = server
        .register_server(Some(0x1003), ClassicChannelSpec::default())
        .unwrap();
    let mut occupier = ChannelManager::new();
    occupier
        .connect(basic_psm, ClassicChannelSpec::default())
        .unwrap();
    relay(&mut occupier, &mut server);
    assert_eq!(server.poll_accepted_channel(), Some(0x0040));

    let ertm_psm = server.register_ertm_server(Some(0x1001), spec).unwrap();
    let mut client = ChannelManager::new();
    let client_cid = client.connect_ertm(ertm_psm, spec).unwrap();
    relay(&mut client, &mut server);
    let server_cid = server.poll_accepted_channel().unwrap();
    assert_eq!((client_cid, server_cid), (0x0040, 0x0041));
    assert!(client.channel(client_cid).unwrap().fcs_enabled);
    assert!(server.channel(server_cid).unwrap().fcs_enabled);

    client.send(client_cid, b"FCS protected").unwrap();
    let good = client.poll_outbound().unwrap();
    assert!(good.payload.len() >= 4);
    server.process_pdu(good.clone()).unwrap();
    relay(&mut client, &mut server);
    assert_eq!(
        server.channel_mut(server_cid).unwrap().pop_received(),
        Some(b"FCS protected".to_vec())
    );

    client.send(client_cid, b"corrupted").unwrap();
    let mut bad = client.poll_outbound().unwrap();
    bad.payload[2] ^= 0x80;
    assert!(server.process_pdu(bad).is_err());
}

#[test]
fn mode_mismatch_and_invalid_specs_fail_cleanly() {
    let mut client = ChannelManager::new();
    let mut server = ChannelManager::new();
    let psm = server
        .register_ertm_server(Some(0x1001), ErtmChannelSpec::default())
        .unwrap();
    let client_cid = client.connect(psm, ClassicChannelSpec::default()).unwrap();
    relay(&mut client, &mut server);
    assert_eq!(
        client.channel(client_cid).unwrap().state,
        ClassicChannelState::Closed
    );
    assert_eq!(
        client.channel(client_cid).unwrap().connection_result,
        Some(CONFIGURATION_UNACCEPTABLE_PARAMETERS)
    );
    assert!(server.poll_accepted_channel().is_none());

    assert!(client
        .connect_ertm(
            psm,
            ErtmChannelSpec {
                tx_window_size: 0,
                ..ErtmChannelSpec::default()
            }
        )
        .is_err());
    assert!(server
        .register_ertm_server(
            Some(0x1003),
            ErtmChannelSpec {
                mps: 0,
                ..ErtmChannelSpec::default()
            }
        )
        .is_err());
}
