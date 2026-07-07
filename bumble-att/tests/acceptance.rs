//! ATT acceptance suite. The self-contained codec tests from google/bumble
//! `tests/gatt_test.py` (`test_ATT_Error_Response`,
//! `test_ATT_Read_By_Group_Type_Request`), plus a representative PDU set.
//! Bytes are pinned to ground-truth hex from real Python Bumble.

use bumble::Uuid;
use bumble_att::{AttPdu, ATT_ATTRIBUTE_NOT_FOUND_ERROR};

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
/// round-trip reconstructs the same PDU (mirrors Bumble's `basic_check`).
fn check(pdu: AttPdu, expected_hex: &str) {
    let bytes = pdu.to_bytes();
    assert_eq!(hex(&bytes), expected_hex, "serialization vs Python oracle");
    assert_eq!(
        AttPdu::from_bytes(&bytes).unwrap(),
        pdu,
        "round-trip must reconstruct the PDU"
    );
}

// gatt_test.py::test_ATT_Error_Response
#[test]
fn test_att_error_response() {
    check(
        AttPdu::ErrorResponse {
            request_opcode_in_error: 0x02, // ATT_EXCHANGE_MTU_REQUEST
            attribute_handle_in_error: 0x0000,
            error_code: ATT_ATTRIBUTE_NOT_FOUND_ERROR,
        },
        "010200000a",
    );
}

// gatt_test.py::test_ATT_Read_By_Group_Type_Request
#[test]
fn test_att_read_by_group_type_request() {
    check(
        AttPdu::ReadByGroupTypeRequest {
            starting_handle: 0x0001,
            ending_handle: 0xFFFF,
            attribute_group_type: Uuid::from_16_bits(0x2800),
        },
        "100100ffff0028",
    );
}

#[test]
fn test_exchange_mtu() {
    check(AttPdu::ExchangeMtuRequest { client_rx_mtu: 256 }, "020001");
    check(AttPdu::ExchangeMtuResponse { server_rx_mtu: 256 }, "030001");
}

#[test]
fn test_read() {
    check(
        AttPdu::ReadRequest {
            attribute_handle: 0x0025,
        },
        "0a2500",
    );
    check(
        AttPdu::ReadResponse {
            attribute_value: unhex("deadbeef"),
        },
        "0bdeadbeef",
    );
}

#[test]
fn test_write() {
    check(
        AttPdu::WriteRequest {
            attribute_handle: 0x0025,
            attribute_value: unhex("0102"),
        },
        "1225000102",
    );
    check(AttPdu::WriteResponse, "13");
}

#[test]
fn test_handle_value_notification() {
    check(
        AttPdu::HandleValueNotification {
            attribute_handle: 0x0025,
            attribute_value: unhex("cafe"),
        },
        "1b2500cafe",
    );
}

#[test]
fn test_read_by_type_request() {
    check(
        AttPdu::ReadByTypeRequest {
            starting_handle: 0x0001,
            ending_handle: 0xFFFF,
            attribute_type: Uuid::from_16_bits(0x2803),
        },
        "080100ffff0328",
    );
}

#[test]
fn test_read_by_type_response() {
    check(
        AttPdu::ReadByTypeResponse {
            length: 7,
            attribute_data_list: unhex("26000a2700002a"),
        },
        "090726000a2700002a",
    );
}

#[test]
fn test_read_by_group_type_response() {
    check(
        AttPdu::ReadByGroupTypeResponse {
            length: 6,
            attribute_data_list: unhex("010005000a18"),
        },
        "1106010005000a18",
    );
}

#[test]
fn test_generic_unknown_opcode() {
    let bytes = unhex("ff010203");
    let parsed = AttPdu::from_bytes(&bytes).unwrap();
    match &parsed {
        AttPdu::Generic { op_code, payload } => {
            assert_eq!(*op_code, 0xFF);
            assert_eq!(payload, &unhex("010203"));
        }
        other => panic!("expected Generic, got {other:?}"),
    }
    assert_eq!(parsed.to_bytes(), bytes);
}

#[test]
fn test_op_code_bits() {
    // Write Command (0x52) has the command bit (6) set.
    assert!(AttPdu::Generic {
        op_code: 0x52,
        payload: vec![]
    }
    .is_command());
    // Read Request (0x0A) does not.
    assert!(!AttPdu::ReadRequest {
        attribute_handle: 1
    }
    .is_command());
}
