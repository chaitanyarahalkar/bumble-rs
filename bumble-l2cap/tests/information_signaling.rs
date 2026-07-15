use bumble_l2cap::{
    ChannelManager, ControlFrame, InformationCapabilities, L2capPdu, LeCreditChannelManager,
    INFORMATION_RESULT_NOT_SUPPORTED, INFORMATION_RESULT_SUCCESS,
    INFORMATION_TYPE_CONNECTIONLESS_MTU, INFORMATION_TYPE_EXTENDED_FEATURES_SUPPORTED,
    INFORMATION_TYPE_FIXED_CHANNELS_SUPPORTED, L2CAP_LE_SIGNALING_CID, L2CAP_SIGNALING_CID,
};

fn relay_classic(from: &mut ChannelManager, to: &mut ChannelManager) {
    for pdu in from.drain_outbound() {
        to.process_pdu(pdu).unwrap();
    }
}

fn relay_le(from: &mut LeCreditChannelManager, to: &mut LeCreditChannelManager) {
    for pdu in from.drain_outbound() {
        to.process_pdu(pdu).unwrap();
    }
}

fn configured_capabilities() -> InformationCapabilities {
    let mut capabilities = InformationCapabilities::new([0x0080, 0x0020, 0x0008]);
    for cid in [4, 6, 7] {
        capabilities.register_fixed_channel(cid).unwrap();
    }
    capabilities
}

#[test]
fn classic_information_requests_return_upstream_capabilities() {
    let mut client = ChannelManager::new();
    let mut server = ChannelManager::with_information_capabilities(configured_capabilities());

    let cases = [
        (INFORMATION_TYPE_CONNECTIONLESS_MTU, vec![0x00, 0x04]),
        (
            INFORMATION_TYPE_EXTENDED_FEATURES_SUPPORTED,
            vec![0xA8, 0x00, 0x00, 0x00],
        ),
        (
            INFORMATION_TYPE_FIXED_CHANNELS_SUPPORTED,
            vec![0xF2, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        ),
    ];
    for (info_type, expected_data) in cases {
        let identifier = client.request_information(info_type);
        relay_classic(&mut client, &mut server);
        relay_classic(&mut server, &mut client);
        assert_eq!(
            client.poll_information_response().unwrap(),
            bumble_l2cap::InformationResponse {
                identifier,
                info_type,
                result: INFORMATION_RESULT_SUCCESS,
                data: expected_data,
            }
        );
    }

    let identifier = client.request_information(0xFFFF);
    relay_classic(&mut client, &mut server);
    relay_classic(&mut server, &mut client);
    let response = client.poll_information_response().unwrap();
    assert_eq!(response.identifier, identifier);
    assert_eq!(response.result, INFORMATION_RESULT_NOT_SUPPORTED);
    assert!(response.data.is_empty());
}

#[test]
fn le_signaling_supports_information_requests_and_echo() {
    let mut client = LeCreditChannelManager::new();
    let mut server =
        LeCreditChannelManager::with_information_capabilities(configured_capabilities());

    let identifier = client.request_information(INFORMATION_TYPE_EXTENDED_FEATURES_SUPPORTED);
    relay_le(&mut client, &mut server);
    relay_le(&mut server, &mut client);
    let response = client.poll_information_response().unwrap();
    assert_eq!(response.identifier, identifier);
    assert_eq!(response.data, [0xA8, 0x00, 0x00, 0x00]);

    server
        .process_pdu(L2capPdu::new(
            L2CAP_LE_SIGNALING_CID,
            ControlFrame::EchoRequest {
                identifier: 9,
                data: b"echo".to_vec(),
            }
            .to_bytes(),
        ))
        .unwrap();
    let echo = server.poll_outbound().unwrap();
    assert_eq!(echo.cid, L2CAP_LE_SIGNALING_CID);
    assert_eq!(
        ControlFrame::from_bytes(&echo.payload).unwrap(),
        ControlFrame::EchoResponse {
            identifier: 9,
            data: b"echo".to_vec(),
        }
    );
}

#[test]
fn classic_echo_and_fixed_channel_registration_are_live() {
    let mut capabilities = InformationCapabilities::default();
    assert_eq!(
        capabilities.fixed_channels(),
        (1_u64 << L2CAP_SIGNALING_CID) | (1_u64 << L2CAP_LE_SIGNALING_CID)
    );
    capabilities.register_fixed_channel(7).unwrap();
    assert!(capabilities.deregister_fixed_channel(7));
    assert!(!capabilities.deregister_fixed_channel(7));
    assert!(capabilities.register_fixed_channel(64).is_err());

    let mut manager = ChannelManager::new();
    manager
        .process_pdu(L2capPdu::new(
            L2CAP_SIGNALING_CID,
            ControlFrame::EchoRequest {
                identifier: 3,
                data: b"ping".to_vec(),
            }
            .to_bytes(),
        ))
        .unwrap();
    let echo = manager.poll_outbound().unwrap();
    assert_eq!(
        ControlFrame::from_bytes(&echo.payload).unwrap(),
        ControlFrame::EchoResponse {
            identifier: 3,
            data: b"ping".to_vec(),
        }
    );
}
