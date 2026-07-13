use bumble_l2cap::ControlFrame;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn check(frame: ControlFrame, oracle: &str) {
    assert_eq!(hex(&frame.to_bytes()), oracle);
    assert_eq!(ControlFrame::from_bytes(&frame.to_bytes()).unwrap(), frame);
}

#[test]
fn remaining_upstream_signaling_catalog_matches_wire_oracles() {
    check(
        ControlFrame::CommandReject {
            identifier: 1,
            reason: 2,
            data: vec![0x40, 0x00, 0x41, 0x00],
        },
        "01010600020040004100",
    );
    check(
        ControlFrame::EchoRequest {
            identifier: 2,
            data: b"ping".to_vec(),
        },
        "0802040070696e67",
    );
    check(
        ControlFrame::EchoResponse {
            identifier: 2,
            data: b"pong".to_vec(),
        },
        "09020400706f6e67",
    );
    check(
        ControlFrame::InformationRequest {
            identifier: 3,
            info_type: 2,
        },
        "0a0302000200",
    );
    check(
        ControlFrame::InformationResponse {
            identifier: 3,
            info_type: 2,
            result: 0,
            data: vec![0xAA, 0xBB, 0xCC, 0xDD],
        },
        "0b03080002000000aabbccdd",
    );
    check(
        ControlFrame::ConnectionParameterUpdateRequest {
            identifier: 4,
            interval_min: 6,
            interval_max: 12,
            latency: 1,
            timeout: 200,
        },
        "1204080006000c000100c800",
    );
    check(
        ControlFrame::ConnectionParameterUpdateResponse {
            identifier: 4,
            result: 0,
        },
        "130402000000",
    );
    check(
        ControlFrame::LeCreditBasedConnectionRequest {
            identifier: 5,
            le_psm: 0x0025,
            source_cid: 0x0040,
            mtu: 512,
            mps: 128,
            initial_credits: 10,
        },
        "14050a0025004000000280000a00",
    );
    check(
        ControlFrame::LeCreditBasedConnectionResponse {
            identifier: 5,
            destination_cid: 0x0041,
            mtu: 512,
            mps: 128,
            initial_credits: 10,
            result: 0,
        },
        "15050a004100000280000a000000",
    );
    check(
        ControlFrame::LeFlowControlCredit {
            identifier: 6,
            cid: 0x0041,
            credits: 7,
        },
        "1606040041000700",
    );
}

#[test]
fn typed_frames_reject_truncated_fields_and_odd_cid_lists() {
    assert!(ControlFrame::from_bytes(&[0x01, 1, 1, 0, 0]).is_err());
    assert!(ControlFrame::from_bytes(&[0x12, 1, 7, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());

    // Enhanced Credit Based Connection Request: the fixed 8-byte prefix plus
    // an odd one-byte CID tail must not be silently truncated.
    assert!(ControlFrame::from_bytes(&[0x17, 1, 9, 0, 1, 0, 2, 0, 3, 0, 4, 0, 0x40,]).is_err());
}
