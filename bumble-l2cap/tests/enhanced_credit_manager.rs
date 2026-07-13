use bumble_l2cap::{
    ControlFrame, L2capPdu, LeCreditBasedChannelSpec, LeCreditChannelManager,
    CREDIT_BASED_CONNECTION_ALL_SUCCESSFUL, CREDIT_BASED_CONNECTION_REFUSED_INVALID_SOURCE_CID,
    CREDIT_BASED_CONNECTION_REFUSED_SPSM_NOT_SUPPORTED,
    CREDIT_BASED_RECONFIGURATION_FAILED_INVALID_CIDS,
    CREDIT_BASED_RECONFIGURATION_FAILED_MPS_REDUCTION,
    CREDIT_BASED_RECONFIGURATION_FAILED_MTU_REDUCTION,
    CREDIT_BASED_RECONFIGURATION_FAILED_UNACCEPTABLE_PARAMETERS,
    CREDIT_BASED_RECONFIGURATION_SUCCESSFUL, L2CAP_LE_SIGNALING_CID,
};

fn pump(a: &mut LeCreditChannelManager, b: &mut LeCreditChannelManager) {
    for _ in 0..10_000 {
        let mut progress = false;
        for pdu in a.drain_outbound() {
            b.process_pdu(pdu).unwrap();
            progress = true;
        }
        for pdu in b.drain_outbound() {
            a.process_pdu(pdu).unwrap();
            progress = true;
        }
        if !progress {
            return;
        }
    }
    panic!("enhanced CoC managers did not quiesce");
}

fn control_pdu(frame: ControlFrame) -> L2capPdu {
    L2capPdu::new(L2CAP_LE_SIGNALING_CID, frame.to_bytes())
}

fn response(manager: &mut LeCreditChannelManager) -> ControlFrame {
    let pdu = manager.poll_outbound().unwrap();
    assert_eq!(pdu.cid, L2CAP_LE_SIGNALING_CID);
    ControlFrame::from_bytes(&pdu.payload).unwrap()
}

#[test]
fn creates_five_channels_atomically_and_transfers_on_each() {
    let mut client = LeCreditChannelManager::new();
    let mut server = LeCreditChannelManager::new();
    let server_spec = LeCreditBasedChannelSpec {
        mtu: 70,
        mps: 24,
        max_credits: 1,
        ..LeCreditBasedChannelSpec::default()
    };
    let psm = server.register_server(server_spec).unwrap();
    let client_spec = LeCreditBasedChannelSpec {
        mtu: 80,
        mps: 25,
        max_credits: 1,
        ..LeCreditBasedChannelSpec::default()
    };

    let client_cids = client.connect_enhanced(psm, client_spec, 5).unwrap();
    assert_eq!(client_cids, vec![0x0040, 0x0041, 0x0042, 0x0043, 0x0044]);
    pump(&mut client, &mut server);
    let server_cids: Vec<_> = (0..5)
        .map(|_| server.poll_accepted_channel().unwrap())
        .collect();
    assert_eq!(server_cids, client_cids);

    for (index, (client_cid, server_cid)) in client_cids.iter().zip(&server_cids).enumerate() {
        assert_eq!(
            client.connection_result(*client_cid),
            Some(CREDIT_BASED_CONNECTION_ALL_SUCCESSFUL)
        );
        let client_channel = client.channel(*client_cid).unwrap();
        assert_eq!(client_channel.peer_mtu, server_spec.mtu);
        assert_eq!(client_channel.peer_mps, server_spec.mps);
        let server_channel = server.channel(*server_cid).unwrap();
        assert_eq!(server_channel.peer_mtu, client_spec.mtu);
        assert_eq!(server_channel.peer_mps, client_spec.mps);

        let payload = vec![index as u8; 123 + index];
        client.send(*client_cid, &payload).unwrap();
        pump(&mut client, &mut server);
        let mut received = Vec::new();
        while let Some(sdu) = server.channel_mut(*server_cid).unwrap().pop_received() {
            received.extend(sdu);
        }
        assert_eq!(received, payload);

        let reply = vec![0xA0 | index as u8; 91 + index];
        server.send(*server_cid, &reply).unwrap();
        pump(&mut client, &mut server);
        let mut received = Vec::new();
        while let Some(sdu) = client.channel_mut(*client_cid).unwrap().pop_received() {
            received.extend(sdu);
        }
        assert_eq!(received, reply);
    }
}

#[test]
fn refuses_unknown_spsm_and_invalid_source_cids_without_partial_channels() {
    let mut client = LeCreditChannelManager::new();
    let mut server = LeCreditChannelManager::new();
    let cids = client
        .connect_enhanced(0x1234, LeCreditBasedChannelSpec::default(), 3)
        .unwrap();
    pump(&mut client, &mut server);
    for cid in cids {
        assert_eq!(
            client.connection_result(cid),
            Some(CREDIT_BASED_CONNECTION_REFUSED_SPSM_NOT_SUPPORTED)
        );
        assert!(client.channel(cid).is_none());
    }

    let psm = server
        .register_server(LeCreditBasedChannelSpec::default())
        .unwrap();
    server
        .process_pdu(control_pdu(ControlFrame::CreditBasedConnectionRequest {
            identifier: 7,
            spsm: psm,
            mtu: 100,
            mps: 50,
            initial_credits: 2,
            source_cid: vec![0x0040, 0x0040],
        }))
        .unwrap();
    assert!(matches!(
        response(&mut server),
        ControlFrame::CreditBasedConnectionResponse {
            identifier: 7,
            result: CREDIT_BASED_CONNECTION_REFUSED_INVALID_SOURCE_CID,
            destination_cid,
            ..
        } if destination_cid.is_empty()
    ));
    assert!(server.poll_accepted_channel().is_none());

    assert!(client
        .connect_enhanced(psm, LeCreditBasedChannelSpec::default(), 0)
        .is_err());
    assert!(client
        .connect_enhanced(psm, LeCreditBasedChannelSpec::default(), 6)
        .is_err());
}

#[test]
fn reconfigures_multiple_channels_and_applies_new_receive_limits() {
    let mut client = LeCreditChannelManager::new();
    let mut server = LeCreditChannelManager::new();
    let psm = server
        .register_server(LeCreditBasedChannelSpec {
            mtu: 60,
            mps: 24,
            max_credits: 2,
            ..LeCreditBasedChannelSpec::default()
        })
        .unwrap();
    let client_cids = client
        .connect_enhanced(
            psm,
            LeCreditBasedChannelSpec {
                mtu: 50,
                mps: 23,
                max_credits: 2,
                ..LeCreditBasedChannelSpec::default()
            },
            2,
        )
        .unwrap();
    pump(&mut client, &mut server);
    let server_cids = [
        server.poll_accepted_channel().unwrap(),
        server.poll_accepted_channel().unwrap(),
    ];

    let identifier = client.reconfigure(&client_cids, 100, 50).unwrap();
    pump(&mut client, &mut server);
    assert_eq!(
        client.reconfiguration_result(identifier),
        Some(CREDIT_BASED_RECONFIGURATION_SUCCESSFUL)
    );
    for (client_cid, server_cid) in client_cids.iter().zip(server_cids) {
        let client_channel = client.channel(*client_cid).unwrap();
        assert_eq!((client_channel.mtu, client_channel.mps), (100, 50));
        assert_eq!(client_channel.att_mtu, 60);
        let server_channel = server.channel(server_cid).unwrap();
        assert_eq!(
            (server_channel.peer_mtu, server_channel.peer_mps),
            (100, 50)
        );
        assert_eq!(server_channel.att_mtu, 60);
    }

    let payload = vec![0xAB; 90];
    server.send(server_cids[0], &payload).unwrap();
    pump(&mut client, &mut server);
    assert_eq!(
        client.channel_mut(client_cids[0]).unwrap().pop_received(),
        Some(payload)
    );
    assert!(client
        .channel_mut(client_cids[0])
        .unwrap()
        .pop_received()
        .is_none());

    let single = client.reconfigure(&[client_cids[0]], 100, 30).unwrap();
    pump(&mut client, &mut server);
    assert_eq!(
        client.reconfiguration_result(single),
        Some(CREDIT_BASED_RECONFIGURATION_SUCCESSFUL)
    );
    assert_eq!(client.channel(client_cids[0]).unwrap().mps, 30);
    assert_eq!(server.channel(server_cids[0]).unwrap().peer_mps, 30);
}

#[test]
fn rejects_each_invalid_reconfiguration_without_mutating_channels() {
    let mut client = LeCreditChannelManager::new();
    let mut server = LeCreditChannelManager::new();
    let psm = server
        .register_server(LeCreditBasedChannelSpec {
            mtu: 80,
            mps: 40,
            ..LeCreditBasedChannelSpec::default()
        })
        .unwrap();
    let client_cids = client
        .connect_enhanced(
            psm,
            LeCreditBasedChannelSpec {
                mtu: 70,
                mps: 30,
                ..LeCreditBasedChannelSpec::default()
            },
            2,
        )
        .unwrap();
    pump(&mut client, &mut server);
    let server_cids = [
        server.poll_accepted_channel().unwrap(),
        server.poll_accepted_channel().unwrap(),
    ];

    let cases = [
        (
            10,
            69,
            30,
            vec![server_cids[0]],
            CREDIT_BASED_RECONFIGURATION_FAILED_MTU_REDUCTION,
        ),
        (
            11,
            70,
            29,
            server_cids.to_vec(),
            CREDIT_BASED_RECONFIGURATION_FAILED_MPS_REDUCTION,
        ),
        (
            12,
            70,
            30,
            vec![0x007F],
            CREDIT_BASED_RECONFIGURATION_FAILED_INVALID_CIDS,
        ),
        (
            13,
            22,
            23,
            vec![server_cids[0]],
            CREDIT_BASED_RECONFIGURATION_FAILED_UNACCEPTABLE_PARAMETERS,
        ),
    ];
    for (identifier, mtu, mps, destination_cid, expected) in cases {
        server
            .process_pdu(control_pdu(ControlFrame::CreditBasedReconfigureRequest {
                identifier,
                mtu,
                mps,
                destination_cid,
            }))
            .unwrap();
        assert!(matches!(
            response(&mut server),
            ControlFrame::CreditBasedReconfigureResponse { result, .. } if result == expected
        ));
    }

    for (client_cid, server_cid) in client_cids.iter().zip(server_cids) {
        assert_eq!(
            (
                client.channel(*client_cid).unwrap().mtu,
                client.channel(*client_cid).unwrap().mps
            ),
            (70, 30)
        );
        assert_eq!(
            (
                server.channel(server_cid).unwrap().peer_mtu,
                server.channel(server_cid).unwrap().peer_mps
            ),
            (70, 30)
        );
    }

    assert!(client.reconfigure(&[], 90, 50).is_err());
    assert!(client
        .reconfigure(&[client_cids[0], client_cids[0]], 90, 50)
        .is_err());
    assert!(client.reconfigure(&[0x007F], 90, 50).is_err());
    assert!(client.reconfigure(&client_cids, 69, 50).is_err());
    assert!(client.reconfigure(&client_cids, 90, 29).is_err());
}
