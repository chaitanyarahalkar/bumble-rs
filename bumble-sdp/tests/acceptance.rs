//! Oracle-pinned acceptance tests for the SDP codec.
//!
//! Every `assert_bytes` hex literal was captured from upstream Python Bumble
//! (`bytes(x).hex()`) at commit `1d26b99865f96a3e7359009424c0ddf2934acd0b`,
//! mirroring the cases in upstream `tests/sdp_test.py::test_data_elements`
//! (all eight size-index encodings, every UUID width, signed negatives, and the
//! explicit-`value_size` round-trip trap) plus one instance of each of the
//! seven PDUs. Each element is checked in both directions: serialize equals the
//! oracle, and parsing that oracle back yields the original value.

use bumble::Uuid;
use bumble_sdp::{DataElement, SdpPdu, ServiceAttribute};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Assert a data element serializes to `oracle` and round-trips back to itself.
fn check(element: DataElement, oracle: &str) {
    let bytes = element.to_bytes().expect("serialize");
    assert_eq!(
        hex(&bytes),
        oracle,
        "serialization mismatch for {element:?}"
    );
    let parsed = DataElement::from_bytes(&bytes).expect("parse");
    assert_eq!(parsed, element, "round-trip mismatch for {element:?}");
}

// --- DataElement scalars -----------------------------------------------------

#[test]
fn data_element_nil() {
    check(DataElement::nil(), "00");
}

#[test]
fn data_element_unsigned_integers() {
    check(DataElement::unsigned_integer(12, 1), "080c");
    check(DataElement::unsigned_integer(1234, 2), "0904d2");
    check(DataElement::unsigned_integer(0x123456, 4), "0a00123456");
    check(
        DataElement::unsigned_integer(0x1_2345_6789, 8),
        "0b0000000123456789",
    );
    // Explicit value_size 4 must survive even though the value fits in 2 bytes.
    check(DataElement::unsigned_integer(0x0000_FFFF, 4), "0a0000ffff");
}

#[test]
fn data_element_signed_integers() {
    check(DataElement::signed_integer(-12, 1), "10f4");
    check(DataElement::signed_integer(-1234, 2), "11fb2e");
    check(DataElement::signed_integer(-0x123456, 4), "12ffedcbaa");
    check(
        DataElement::signed_integer(-0x1_2345_6789, 8),
        "13fffffffedcba9877",
    );
    check(DataElement::signed_integer(0x0000_FFFF, 4), "120000ffff");
}

#[test]
fn data_element_uuids() {
    check(DataElement::uuid(Uuid::from_16_bits(1234)), "1904d2");
    check(
        DataElement::uuid(Uuid::from_32_bits(123456789)),
        "1a075bcd15",
    );
    // The 128-bit vector catches a half-done little/big-endian reversal.
    check(
        DataElement::uuid(Uuid::parse("61A3512C-09BE-4DDC-A6A6-0B03667AAFC6").unwrap()),
        "1c61a3512c09be4ddca6a60b03667aafc6",
    );
}

#[test]
fn data_element_text_boolean_url() {
    check(DataElement::text_string(*b"hello"), "250568656c6c6f");
    check(DataElement::boolean(true), "2801");
    check(DataElement::boolean(false), "2800");
    check(
        DataElement::url("http://example.com"),
        "4512687474703a2f2f6578616d706c652e636f6d",
    );
}

/// A 300-byte string uses size index 6 (a 2-byte length): header `26 012c`.
#[test]
fn data_element_text_size_index_6() {
    let payload = b"hello".repeat(60);
    let element = DataElement::text_string(payload.clone());
    let mut expected = vec![0x26, 0x01, 0x2c];
    expected.extend_from_slice(&payload);
    assert_eq!(element.to_bytes().unwrap(), expected);
    assert_eq!(DataElement::from_bytes(&expected).unwrap(), element);
}

/// A 100,000-byte string uses size index 7 (a 4-byte length): header
/// `27 000186a0`.
#[test]
fn data_element_text_size_index_7() {
    let payload = b"hello".repeat(20000);
    let element = DataElement::text_string(payload.clone());
    let mut expected = vec![0x27, 0x00, 0x01, 0x86, 0xa0];
    expected.extend_from_slice(&payload);
    assert_eq!(element.to_bytes().unwrap(), expected);
    assert_eq!(DataElement::from_bytes(&expected).unwrap(), element);
}

// --- DataElement containers --------------------------------------------------

#[test]
fn data_element_sequences_and_alternatives() {
    check(
        DataElement::sequence([DataElement::boolean(true)]),
        "35022801",
    );
    check(
        DataElement::sequence([
            DataElement::boolean(true),
            DataElement::text_string(*b"hello"),
        ]),
        "35092801250568656c6c6f",
    );
    check(
        DataElement::alternative([DataElement::boolean(true)]),
        "3d022801",
    );
    check(
        DataElement::alternative([
            DataElement::boolean(true),
            DataElement::text_string(*b"hello"),
        ]),
        "3d092801250568656c6c6f",
    );
}

#[test]
fn data_element_nested_sequence() {
    check(
        DataElement::sequence([
            DataElement::sequence([DataElement::uuid(Uuid::from_16_bits(0x0100))]),
            DataElement::unsigned_integer_16(0x0003),
        ]),
        "35083503190100090003",
    );
}

// --- Parse-side and error behavior -------------------------------------------

#[test]
fn from_bytes_ignores_trailing_bytes() {
    // Upstream DataElement.from_bytes parses one element and ignores the rest.
    let element = DataElement::from_bytes(&[0x09, 0x04, 0xd2, 0xff, 0xff]).unwrap();
    assert_eq!(element, DataElement::unsigned_integer(1234, 2));
}

#[test]
fn parse_from_bytes_reports_consumed_offset() {
    let (offset, element) = DataElement::parse_from_bytes(&[0x08, 0x0c, 0x99], 0).unwrap();
    assert_eq!(offset, 2);
    assert_eq!(element, DataElement::unsigned_integer(12, 1));
}

#[test]
fn parse_rejects_unknown_type_code() {
    // Type code 9 (header 0x48) is outside the nine SDP defines.
    assert!(DataElement::from_bytes(&[0x48, 0x00]).is_err());
}

#[test]
fn parse_rejects_truncated_value() {
    // Claims a 4-byte integer but only supplies two.
    assert!(DataElement::from_bytes(&[0x0a, 0x00, 0x01]).is_err());
}

#[test]
fn serialize_rejects_oversized_integer() {
    assert!(DataElement::unsigned_integer(0x1_0000, 2)
        .to_bytes()
        .is_err());
    assert!(DataElement::signed_integer(200, 1).to_bytes().is_err());
    assert!(DataElement::unsigned_integer(0, 3).to_bytes().is_err());
}

// --- SDP PDUs ----------------------------------------------------------------

/// Assert a PDU serializes to `oracle` and round-trips back to itself.
fn check_pdu(pdu: SdpPdu, oracle: &str) {
    let bytes = pdu.to_bytes().expect("serialize");
    assert_eq!(hex(&bytes), oracle, "serialization mismatch for {pdu:?}");
    let parsed = SdpPdu::from_bytes(&bytes).expect("parse");
    assert_eq!(parsed, pdu, "round-trip mismatch for {pdu:?}");
}

fn browse_root_pattern() -> DataElement {
    DataElement::sequence([DataElement::uuid(bumble_sdp::public_browse_root())])
}

#[test]
fn pdu_error_response() {
    check_pdu(
        SdpPdu::ErrorResponse {
            transaction_id: 0x0009,
            error_code: bumble_sdp::error_code::INVALID_REQUEST_SYNTAX,
        },
        "01000900020300",
    );
    // Non-palindromic value pins the (surprising) little-endian error_code.
    check_pdu(
        SdpPdu::ErrorResponse {
            transaction_id: 0x0009,
            error_code: 0x0102,
        },
        "01000900020201",
    );
}

#[test]
fn pdu_service_search_request() {
    check_pdu(
        SdpPdu::ServiceSearchRequest {
            transaction_id: 1,
            service_search_pattern: browse_root_pattern(),
            maximum_service_record_count: 3,
            continuation_state: vec![],
        },
        "020001000735031910020003",
    );
    // With a non-empty continuation state carried verbatim.
    check_pdu(
        SdpPdu::ServiceSearchRequest {
            transaction_id: 0x1234,
            service_search_pattern: browse_root_pattern(),
            maximum_service_record_count: 255,
            continuation_state: vec![0x02, 0xab, 0xcd],
        },
        "021234000a350319100200ff02abcd",
    );
}

#[test]
fn pdu_service_search_response() {
    check_pdu(
        SdpPdu::ServiceSearchResponse {
            transaction_id: 1,
            total_service_record_count: 2,
            service_record_handle_list: vec![0x0001_0001, 0x0001_0002],
            continuation_state: vec![],
        },
        "030001000c000200020001000100010002",
    );
}

#[test]
fn pdu_service_attribute_request() {
    check_pdu(
        SdpPdu::ServiceAttributeRequest {
            transaction_id: 7,
            service_record_handle: 0x0001_0001,
            maximum_attribute_byte_count: 0x1000,
            attribute_id_list: DataElement::sequence([DataElement::unsigned_integer_16(0x0000)]),
            continuation_state: vec![],
        },
        "040007000b0001000110003503090000",
    );
}

#[test]
fn pdu_service_attribute_response() {
    check_pdu(
        SdpPdu::ServiceAttributeResponse {
            transaction_id: 7,
            attribute_list: vec![0x35, 0x03, 0x09, 0x00, 0x00],
            continuation_state: vec![],
        },
        "050007000700053503090000",
    );
}

#[test]
fn pdu_service_search_attribute_request() {
    check_pdu(
        SdpPdu::ServiceSearchAttributeRequest {
            transaction_id: 0x2222,
            service_search_pattern: DataElement::sequence([DataElement::uuid(Uuid::from_16_bits(
                0x0100,
            ))]),
            maximum_attribute_byte_count: 0xFFFF,
            attribute_id_list: DataElement::sequence([DataElement::unsigned_integer(
                0x0000_FFFF,
                4,
            )]),
            continuation_state: vec![],
        },
        "062222000e3503190100ffff35050a0000ffff",
    );
}

#[test]
fn pdu_service_search_attribute_response() {
    check_pdu(
        SdpPdu::ServiceSearchAttributeResponse {
            transaction_id: 0x2222,
            attribute_lists: vec![0x36, 0x00, 0x05, 0x35, 0x03, 0x09, 0x00, 0x00],
            continuation_state: vec![],
        },
        "072222000a00083600053503090000",
    );
}

// --- Service record (attribute list) -----------------------------------------

#[test]
fn service_attribute_record_roundtrip() {
    let attributes = vec![
        ServiceAttribute::new(0x0000, DataElement::unsigned_integer_32(0x0001_0001)),
        ServiceAttribute::new(
            0x0001,
            DataElement::sequence([DataElement::uuid(bumble_sdp::public_browse_root())]),
        ),
    ];
    let element = ServiceAttribute::list_to_data_element(&attributes);
    assert_eq!(
        hex(&element.to_bytes().unwrap()),
        "35100900000a000100010900013503191002"
    );

    // The flat alternating list recovers the original attributes.
    if let DataElement::Sequence(elements) = element {
        let recovered = ServiceAttribute::list_from_data_elements(&elements);
        assert_eq!(recovered, attributes);
        assert_eq!(
            ServiceAttribute::find(&recovered, 0x0000),
            Some(&DataElement::unsigned_integer_32(0x0001_0001))
        );
    } else {
        panic!("expected a sequence");
    }
}
