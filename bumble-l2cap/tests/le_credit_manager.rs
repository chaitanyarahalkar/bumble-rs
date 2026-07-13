use bumble_l2cap::{
    ControlFrame, L2capPdu, LeCreditBasedChannelSpec, LeCreditChannelManager,
    L2CAP_LE_PSM_DYNAMIC_RANGE_START, L2CAP_LE_SIGNALING_CID,
    LE_CONNECTION_REFUSED_PSM_NOT_SUPPORTED, LE_CONNECTION_REFUSED_UNACCEPTABLE_PARAMETERS,
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
    panic!("LE CoC managers did not quiesce");
}

#[test]
fn connects_transfers_bidirectionally_replenishes_and_disconnects() {
    let mut client = LeCreditChannelManager::new();
    let mut server = LeCreditChannelManager::new();
    let psm = server
        .register_server(LeCreditBasedChannelSpec {
            mtu: 50,
            mps: 23,
            max_credits: 1,
            ..LeCreditBasedChannelSpec::default()
        })
        .unwrap();
    assert_eq!(psm, L2CAP_LE_PSM_DYNAMIC_RANGE_START);

    let client_cid = client
        .connect(
            psm,
            LeCreditBasedChannelSpec {
                mtu: 60,
                mps: 25,
                max_credits: 1,
                ..LeCreditBasedChannelSpec::default()
            },
        )
        .unwrap();
    pump(&mut client, &mut server);
    let server_cid = server.poll_accepted_channel().unwrap();
    assert_eq!(client.connection_result(client_cid), Some(0));
    assert_eq!(client.channel(client_cid).unwrap().peer_mtu, 50);
    assert_eq!(server.channel(server_cid).unwrap().peer_mtu, 60);

    let client_payload: Vec<u8> = (0..=255).cycle().take(511).collect();
    client.send(client_cid, &client_payload).unwrap();
    pump(&mut client, &mut server);
    let mut received = Vec::new();
    while let Some(sdu) = server.channel_mut(server_cid).unwrap().pop_received() {
        received.extend_from_slice(&sdu);
    }
    assert_eq!(received, client_payload);
    assert!(client.channel(client_cid).unwrap().is_drained());

    let server_payload = b"server to client across the same CoC".repeat(4);
    server.send(server_cid, &server_payload).unwrap();
    pump(&mut client, &mut server);
    let mut received = Vec::new();
    while let Some(sdu) = client.channel_mut(client_cid).unwrap().pop_received() {
        received.extend_from_slice(&sdu);
    }
    assert_eq!(received, server_payload);

    client.disconnect(client_cid).unwrap();
    pump(&mut client, &mut server);
    assert!(client.channel(client_cid).is_none());
    assert!(server.channel(server_cid).is_none());

    let reused_cid = client
        .connect(psm, LeCreditBasedChannelSpec::default())
        .unwrap();
    assert_eq!(reused_cid, client_cid);
    assert_eq!(client.connection_result(reused_cid), None);
    pump(&mut client, &mut server);
    assert_eq!(client.connection_result(reused_cid), Some(0));
    assert!(client.channel(reused_cid).is_some());
}

#[test]
fn refuses_unknown_psm_and_invalid_negotiation() {
    let mut client = LeCreditChannelManager::new();
    let mut server = LeCreditChannelManager::new();
    let cid = client
        .connect(0x1234, LeCreditBasedChannelSpec::default())
        .unwrap();
    pump(&mut client, &mut server);
    assert_eq!(
        client.connection_result(cid),
        Some(LE_CONNECTION_REFUSED_PSM_NOT_SUPPORTED)
    );
    assert!(client.channel(cid).is_none());

    let psm = server
        .register_server(LeCreditBasedChannelSpec::default())
        .unwrap();
    server
        .process_pdu(L2capPdu::new(
            L2CAP_LE_SIGNALING_CID,
            ControlFrame::LeCreditBasedConnectionRequest {
                identifier: 9,
                le_psm: psm,
                source_cid: 0x0040,
                mtu: 22,
                mps: 23,
                initial_credits: 1,
            }
            .to_bytes(),
        ))
        .unwrap();
    let response = server.poll_outbound().unwrap();
    assert_eq!(response.cid, L2CAP_LE_SIGNALING_CID);
    assert!(matches!(
        ControlFrame::from_bytes(&response.payload).unwrap(),
        ControlFrame::LeCreditBasedConnectionResponse {
            identifier: 9,
            result: LE_CONNECTION_REFUSED_UNACCEPTABLE_PARAMETERS,
            ..
        }
    ));
}

#[test]
fn allocates_resources_deterministically_and_rejects_duplicates() {
    let mut manager = LeCreditChannelManager::new();
    let first = manager
        .register_server(LeCreditBasedChannelSpec::default())
        .unwrap();
    let second = manager
        .register_server(LeCreditBasedChannelSpec::default())
        .unwrap();
    assert_eq!(first, 0x0080);
    assert_eq!(second, 0x0081);
    assert!(manager
        .register_server(LeCreditBasedChannelSpec {
            psm: Some(first),
            ..LeCreditBasedChannelSpec::default()
        })
        .is_err());
    assert!(manager.unregister_server(first));

    let cid1 = manager
        .connect(0x0080, LeCreditBasedChannelSpec::default())
        .unwrap();
    let cid2 = manager
        .connect(0x0081, LeCreditBasedChannelSpec::default())
        .unwrap();
    assert_eq!(cid1, 0x0040);
    assert_eq!(cid2, 0x0041);
}
