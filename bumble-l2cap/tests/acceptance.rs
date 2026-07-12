//! L2CAP acceptance suite. Ported from google/bumble `tests/l2cap_test.py`
//! (the self-contained codec tests). Frame bytes are pinned to ground-truth
//! hex captured from real Python Bumble.

use bumble_l2cap::{
    decode_configuration_options, encode_configuration_options, parse_psm, serialize_psm,
    ConfigurationOption, ControlFrame, L2capPdu,
};

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Serialize, compare to the Python oracle bytes, then parse and confirm the
/// round-trip reconstructs the same frame.
fn check(frame: ControlFrame, expected_hex: &str) {
    let bytes = frame.to_bytes();
    assert_eq!(hex(&bytes), expected_hex, "serialization vs Python oracle");
    assert_eq!(
        ControlFrame::from_bytes(&bytes).unwrap(),
        frame,
        "round-trip must reconstruct the frame"
    );
}

// l2cap_test.py::test_helpers
#[test]
fn test_helpers() {
    // serialize_psm
    assert_eq!(serialize_psm(0x01), vec![0x01, 0x00]);
    assert_eq!(serialize_psm(0x1023), vec![0x23, 0x10]);
    assert_eq!(serialize_psm(0x242311), vec![0x11, 0x23, 0x24]);

    // parse_psm (offset 1, as in the upstream test)
    assert_eq!(parse_psm(&[0x00, 0x01, 0x00, 0x44], 1).unwrap(), (3, 0x01));
    assert_eq!(
        parse_psm(&[0x00, 0x23, 0x10, 0x44], 1).unwrap(),
        (3, 0x1023)
    );
    assert_eq!(
        parse_psm(&[0x00, 0x11, 0x23, 0x24, 0x44], 1).unwrap(),
        (4, 0x242311)
    );

    // Connection_Request round-trip.
    let rq = ControlFrame::ConnectionRequest {
        identifier: 0x88,
        psm: 0x01,
        source_cid: 0x44,
    };
    check(rq.clone(), "0288040001004400");
    let parsed = ControlFrame::from_bytes(&rq.to_bytes()).unwrap();
    assert_eq!(parsed, rq);
}

// l2cap_test.py::test_l2cap_credit_based_connection_request
#[test]
fn test_l2cap_credit_based_connection_request() {
    check(
        ControlFrame::CreditBasedConnectionRequest {
            identifier: 1,
            spsm: 2,
            mtu: 3,
            mps: 4,
            initial_credits: 5,
            source_cid: vec![6, 7, 8],
        },
        "17010e000200030004000500060007000800",
    );
}

// l2cap_test.py::test_l2cap_credit_based_connection_response
#[test]
fn test_l2cap_credit_based_connection_response() {
    check(
        ControlFrame::CreditBasedConnectionResponse {
            identifier: 1,
            mtu: 2,
            mps: 3,
            initial_credits: 4,
            result: 0x000e, // ALL_CONNECTIONS_PENDING_AUTHENTICATION_PENDING
            destination_cid: vec![6, 7, 8],
        },
        "18010e000200030004000e00060007000800",
    );
}

// l2cap_test.py::test_l2cap_credit_based_reconfigure_request
#[test]
fn test_l2cap_credit_based_reconfigure_request() {
    check(
        ControlFrame::CreditBasedReconfigureRequest {
            identifier: 1,
            mtu: 2,
            mps: 3,
            destination_cid: vec![6, 7, 8],
        },
        "19010a0002000300060007000800",
    );
}

// l2cap_test.py::test_l2cap_credit_based_reconfigure_response
#[test]
fn test_l2cap_credit_based_reconfigure_response() {
    check(
        ControlFrame::CreditBasedReconfigureResponse {
            identifier: 1,
            result: 0x0004, // RECONFIGURATION_FAILED_OTHER_UNACCEPTABLE_PARAMETERS
        },
        "1a0102000400",
    );
}

// l2cap_test.py::test_unimplemented_control_frame
#[test]
fn test_unimplemented_control_frame() {
    let frame = ControlFrame::Generic {
        code: 0xFF,
        identifier: 1,
        payload: b"123456".to_vec(),
    };
    let parsed = ControlFrame::from_bytes(&frame.to_bytes()).unwrap();
    match parsed {
        ControlFrame::Generic { code, payload, .. } => {
            assert_eq!(code, 0xFF);
            assert_eq!(payload, b"123456");
        }
        other => panic!("expected Generic, got {other:?}"),
    }
}

// l2cap_test.py::test_fcs (Core Spec 6.1, Vol 3, Part A, 3.3.5)
#[test]
fn test_fcs() {
    let pdu = L2capPdu::new(0x0040, unhex("020000010203040506070809"));
    assert_eq!(
        hex(&pdu.to_bytes(true)),
        "0e0040000200000102030405060708093861"
    );

    let pdu = L2capPdu::new(0x0040, unhex("0101"));
    assert_eq!(hex(&pdu.to_bytes(true)), "040040000101d414");
}

// L2CAP PDU basic round-trip (no FCS).
#[test]
fn test_l2cap_pdu_roundtrip() {
    let pdu = L2capPdu::new(0x0040, unhex("deadbeef"));
    let bytes = pdu.to_bytes(false);
    // length=0x0004, cid=0x0040, payload=deadbeef
    assert_eq!(hex(&bytes), "04004000deadbeef");
    assert_eq!(L2capPdu::from_bytes(&bytes).unwrap(), pdu);
}

// l2cap.py Classic signaling dataclasses, pinned to real Bumble's serializer.
#[test]
fn test_classic_control_frames() {
    check(
        ControlFrame::ConnectionResponse {
            identifier: 0x88,
            destination_cid: 0x0041,
            source_cid: 0x0040,
            result: 0,
            status: 0,
        },
        "038808004100400000000000",
    );

    let mtu = encode_configuration_options(&[ConfigurationOption::new(
        1,
        2048u16.to_le_bytes().to_vec(),
    )])
    .unwrap();
    check(
        ControlFrame::ConfigureRequest {
            identifier: 0x89,
            destination_cid: 0x0041,
            flags: 0,
            options: mtu.clone(),
        },
        "048908004100000001020008",
    );
    check(
        ControlFrame::ConfigureResponse {
            identifier: 0x89,
            source_cid: 0x0040,
            flags: 0,
            result: 0,
            options: mtu,
        },
        "05890a0040000000000001020008",
    );
    check(
        ControlFrame::DisconnectionRequest {
            identifier: 0x8a,
            destination_cid: 0x0041,
            source_cid: 0x0040,
        },
        "068a040041004000",
    );
    check(
        ControlFrame::DisconnectionResponse {
            identifier: 0x8a,
            destination_cid: 0x0041,
            source_cid: 0x0040,
        },
        "078a040041004000",
    );
}

#[test]
fn test_configuration_options() {
    let options = vec![
        ConfigurationOption::new(1, 672u16.to_le_bytes().to_vec()),
        ConfigurationOption::hinted(0x22, vec![0xaa, 0xbb]),
    ];
    let encoded = encode_configuration_options(&options).unwrap();
    assert_eq!(hex(&encoded), "0102a002a202aabb");
    assert_eq!(decode_configuration_options(&encoded).unwrap(), options);
    assert!(decode_configuration_options(&[1, 2, 0xaa]).is_err());
}
