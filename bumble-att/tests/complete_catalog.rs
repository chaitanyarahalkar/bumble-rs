use bumble_att::{codes, AttPdu};

#[test]
fn remaining_upstream_pdu_catalog_is_byte_exact() {
    let samples = [
        (
            AttPdu::ReadMultipleRequest {
                set_of_handles: vec![1, 0x04D2],
            },
            "0e0100d204",
        ),
        (
            AttPdu::ReadMultipleResponse {
                set_of_values: vec![1, 2, 3],
            },
            "0f010203",
        ),
        (
            AttPdu::ReadMultipleVariableRequest {
                set_of_handles: vec![1, 0x04D2],
            },
            "200100d204",
        ),
        (
            AttPdu::ReadMultipleVariableResponse {
                length_value_tuples: vec![(2, vec![0xAA, 0xBB]), (3, vec![1, 2, 3])],
            },
            "210200aabb0300010203",
        ),
        (
            AttPdu::SignedWriteCommand {
                attribute_handle: 0x1234,
                attribute_value: vec![0xDE, 0xAD],
            },
            "d23412dead",
        ),
        (
            AttPdu::PrepareWriteRequest {
                attribute_handle: 0x1234,
                value_offset: 0x5678,
                part_attribute_value: vec![0xAA, 0xBB],
            },
            "1634127856aabb",
        ),
        (
            AttPdu::PrepareWriteResponse {
                attribute_handle: 0x1234,
                value_offset: 0x5678,
                part_attribute_value: vec![0xAA, 0xBB],
            },
            "1734127856aabb",
        ),
        (AttPdu::ExecuteWriteRequest { flags: 1 }, "1801"),
        (AttPdu::ExecuteWriteResponse, "19"),
    ];

    for (pdu, expected) in samples {
        let bytes = pdu.to_bytes();
        assert_eq!(bytes, hex(expected), "{pdu:?}");
        assert_eq!(AttPdu::from_bytes(&bytes).unwrap(), pdu);
    }
}

#[test]
fn command_and_signature_bits_match_the_registered_opcodes() {
    let write = AttPdu::WriteCommand {
        attribute_handle: 1,
        attribute_value: vec![],
    };
    assert!(write.is_command());
    assert!(!write.is_signed());
    let signed = AttPdu::SignedWriteCommand {
        attribute_handle: 1,
        attribute_value: vec![],
    };
    assert_eq!(signed.op_code(), codes::ATT_SIGNED_WRITE_COMMAND);
    assert!(signed.is_command());
    assert!(signed.is_signed());
}

#[test]
fn malformed_handle_sets_and_variable_tuples_are_rejected() {
    assert!(AttPdu::from_bytes(&[codes::ATT_READ_MULTIPLE_REQUEST, 1]).is_err());
    assert!(AttPdu::from_bytes(&[codes::ATT_READ_MULTIPLE_VARIABLE_RESPONSE, 3, 0, 1, 2]).is_err());
    assert!(AttPdu::from_bytes(&[codes::ATT_PREPARE_WRITE_REQUEST, 1, 0, 2]).is_err());
    assert!(AttPdu::from_bytes(&[codes::ATT_EXECUTE_WRITE_REQUEST]).is_err());
}

fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16).unwrap() as u8;
            let low = (pair[1] as char).to_digit(16).unwrap() as u8;
            (high << 4) | low
        })
        .collect()
}
