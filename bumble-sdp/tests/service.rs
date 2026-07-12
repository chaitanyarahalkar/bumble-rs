//! Slice-20 acceptance: an SDP client and server talking to each other
//! in-process, with the server's responses pinned to the real upstream server.
//!
//! The [`SdpServer`] answers requests synchronously; the [`SdpClient`] wraps it
//! directly (every request handler is a transport — the same blanket-impl shape
//! as `bumble_gatt`'s `AttTransport`) and drives the continuation loop. No
//! socket is involved: this is transport-agnostic, because no live Classic
//! L2CAP connection-oriented channel is ported.
//!
//! Three properties are checked:
//!
//! 1. **The server's response bytes are pinned to upstream.** For a fixed
//!    database + request, the Service-Search-Attribute, Service-Attribute and
//!    Service-Search response PDUs are asserted against hex captured from the
//!    real upstream Python `Server` (see `scratchpad/sdp_oracle.py`). The
//!    matching, attribute selection and chunking are a pure function of the
//!    inputs, so this is genuine ground truth, not self-agreement.
//!
//! 2. **Continuation state chunks and reassembles across multiple round-trips.**
//!    A small server MTU forces the answer into four response PDUs; the pinned
//!    bytes show the `01 00` continuation markers and the final `00`
//!    terminator, and the client is shown to reassemble the identical record
//!    set it gets in the single-PDU case.
//!
//! 3. **Client and server agree end-to-end** for all three query types,
//!    including the invalid-handle error path.

use bumble::Uuid;
use bumble_sdp::error_code;
use bumble_sdp::service::{AttributeId, ClientError, SdpClient, SdpRequestHandler, SdpServer};
use bumble_sdp::{DataElement, SdpPdu, ServiceAttribute};

fn uuid16(v: u16) -> Uuid {
    Uuid::from_16_bits(v)
}

/// Two service records, both advertising the SerialPort class (0x1101), matching
/// the oracle's database exactly.
fn build_server(mtu: u16) -> SdpServer {
    let mut server = SdpServer::new(mtu);
    server.add_service(
        0x0001_0000,
        vec![
            ServiceAttribute::new(0x0000, DataElement::unsigned_integer_32(0x0001_0000)),
            ServiceAttribute::new(
                0x0001,
                DataElement::sequence([DataElement::uuid(uuid16(0x1101))]),
            ),
            ServiceAttribute::new(
                0x0004,
                DataElement::sequence([
                    DataElement::sequence([DataElement::uuid(uuid16(0x0100))]),
                    DataElement::sequence([
                        DataElement::uuid(uuid16(0x0003)),
                        DataElement::unsigned_integer_8(1),
                    ]),
                ]),
            ),
        ],
    );
    server.add_service(
        0x0001_0001,
        vec![
            ServiceAttribute::new(0x0000, DataElement::unsigned_integer_32(0x0001_0001)),
            ServiceAttribute::new(
                0x0001,
                DataElement::sequence([DataElement::uuid(uuid16(0x1101))]),
            ),
        ],
    );
    server
}

fn hex(pdu: &SdpPdu) -> String {
    pdu.to_bytes()
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn pattern() -> DataElement {
    DataElement::sequence([DataElement::uuid(uuid16(0x1101))])
}

/// The `[0x0000..=0xFFFF]` range selector (a 32-bit value), selecting every
/// attribute.
fn all_attributes() -> DataElement {
    DataElement::sequence([DataElement::unsigned_integer_32(0x0000_FFFF)])
}

#[test]
fn service_search_attribute_response_matches_upstream() {
    // Large MTU: the whole answer fits in one PDU.
    let mut server = build_server(512);
    let response = server.handle_request(&SdpPdu::ServiceSearchAttributeRequest {
        transaction_id: 1,
        service_search_pattern: pattern(),
        maximum_attribute_byte_count: 0xFFFF,
        attribute_id_list: all_attributes(),
        continuation_state: vec![0],
    });
    // Ground truth from upstream Python Bumble
    // (commit 1d26b99865f96a3e7359009424c0ddf2934acd0b).
    assert_eq!(
        hex(&response),
        "070001003a0037353535210900000a000100000900013503191101090004350c35031901003505190003080135100900000a00010001090001350319110100"
    );
}

#[test]
fn continuation_chunks_match_upstream_and_reassemble() {
    // Small MTU (cap = mtu - 9 = 16 bytes/chunk): forces four response PDUs.
    let mut server = build_server(25);
    let mut continuation_state = vec![0u8];
    let mut transaction_id = 1u16;
    let mut rounds = Vec::new();
    loop {
        let response = server.handle_request(&SdpPdu::ServiceSearchAttributeRequest {
            transaction_id,
            service_search_pattern: pattern(),
            maximum_attribute_byte_count: 0xFFFF,
            attribute_id_list: all_attributes(),
            continuation_state: continuation_state.clone(),
        });
        rounds.push(hex(&response));
        continuation_state = match &response {
            SdpPdu::ServiceSearchAttributeResponse {
                continuation_state, ..
            } => continuation_state.clone(),
            other => panic!("expected SSA response, got {other:?}"),
        };
        if continuation_state == [0] {
            break;
        }
        transaction_id += 1;
    }

    // Four PDUs, byte-pinned to upstream: three carrying the 01 00 "more"
    // marker, the last carrying the 00 terminator.
    assert_eq!(
        rounds,
        vec![
            "07000100140010353535210900000a00010000090001350100",
            "0700020014001003191101090004350c350319010035050100",
            "07000300140010190003080135100900000a00010001090100",
            "070004000a00070001350319110100",
        ],
        "continuation must chunk into four pinned PDUs"
    );
    assert!(rounds.len() >= 2, "the continuation path must actually run");
}

/// Assert the parsed record set is the one both the single-PDU and the
/// continuation exchanges must produce.
fn assert_expected_records(records: &[Vec<ServiceAttribute>]) {
    assert_eq!(records.len(), 2, "two services match 0x1101");

    let record0 = &records[0];
    assert_eq!(record0.len(), 3);
    assert_eq!(
        ServiceAttribute::find(record0, 0x0000),
        Some(&DataElement::unsigned_integer_32(0x0001_0000))
    );
    assert_eq!(
        ServiceAttribute::find(record0, 0x0001),
        Some(&DataElement::sequence([DataElement::uuid(uuid16(0x1101))]))
    );
    assert!(ServiceAttribute::find(record0, 0x0004).is_some());

    let record1 = &records[1];
    assert_eq!(record1.len(), 2);
    assert_eq!(
        ServiceAttribute::find(record1, 0x0000),
        Some(&DataElement::unsigned_integer_32(0x0001_0001))
    );
}

#[test]
fn client_reassembles_the_same_records_regardless_of_mtu() {
    // Single-PDU case.
    let mut single = SdpClient::new(build_server(512));
    let records_single = single
        .service_search_attribute(&[uuid16(0x1101)], &[AttributeId::Range(0x0000, 0xFFFF)])
        .unwrap();
    assert_expected_records(&records_single);

    // Multi-PDU case: the client must stitch four chunks back into the same set.
    let mut chunked = SdpClient::new(build_server(25));
    let records_chunked = chunked
        .service_search_attribute(&[uuid16(0x1101)], &[AttributeId::Range(0x0000, 0xFFFF)])
        .unwrap();
    assert_eq!(records_single, records_chunked);
}

#[test]
fn service_attribute_and_search_round_trip() {
    let mut client = SdpClient::new(build_server(512));

    // Service Search: the handles of the matching records, in order.
    let handles = client.search_services(&[uuid16(0x1101)]).unwrap();
    assert_eq!(handles, vec![0x0001_0000, 0x0001_0001]);

    // Service Attribute: all attributes of one record.
    let attributes = client
        .get_attributes(0x0001_0000, &[AttributeId::Range(0x0000, 0xFFFF)])
        .unwrap();
    assert_eq!(attributes.len(), 3);
    assert_eq!(
        ServiceAttribute::find(&attributes, 0x0000),
        Some(&DataElement::unsigned_integer_32(0x0001_0000))
    );

    // Service Attribute on a missing handle: the invalid-handle error path.
    let err = client
        .get_attributes(0x0009_9999, &[AttributeId::Range(0x0000, 0xFFFF)])
        .unwrap_err();
    assert_eq!(
        err,
        ClientError::Protocol(error_code::INVALID_SERVICE_RECORD_HANDLE)
    );
}

#[test]
fn service_attribute_and_search_responses_match_upstream() {
    // Service Attribute response for one record.
    let mut server = build_server(512);
    let sa = server.handle_request(&SdpPdu::ServiceAttributeRequest {
        transaction_id: 7,
        service_record_handle: 0x0001_0000,
        maximum_attribute_byte_count: 0xFFFF,
        attribute_id_list: all_attributes(),
        continuation_state: vec![0],
    });
    assert_eq!(
        hex(&sa),
        "0500070026002335210900000a000100000900013503191101090004350c35031901003505190003080100"
    );

    // Service Search response.
    let mut server = build_server(512);
    let ss = server.handle_request(&SdpPdu::ServiceSearchRequest {
        transaction_id: 9,
        service_search_pattern: pattern(),
        maximum_service_record_count: 0xFFFF,
        continuation_state: vec![0],
    });
    assert_eq!(hex(&ss), "030009000d00020002000100000001000100");
}
