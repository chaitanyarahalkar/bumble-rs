use bumble::Uuid;
use bumble_att::{codes, AttPdu};
use bumble_gatt::{
    properties, AttServer, Characteristic, GattServer, Service, ATT_INVALID_OFFSET_ERROR,
};

fn assert_error(response: AttPdu, opcode: u8, handle: u16, code: u8) {
    assert_eq!(
        response,
        AttPdu::ErrorResponse {
            request_opcode_in_error: opcode,
            attribute_handle_in_error: handle,
            error_code: code,
        }
    );
}

#[test]
fn bare_server_supports_multiple_reads_and_atomic_queued_writes() {
    let mut server = AttServer::new();
    server.set_attribute(1, b"hello".to_vec());
    server.set_attribute(2, b"world".to_vec());
    assert_eq!(
        server.on_request(&AttPdu::ReadMultipleRequest {
            set_of_handles: vec![1, 2]
        }),
        AttPdu::ReadMultipleResponse {
            set_of_values: b"helloworld".to_vec()
        }
    );
    assert_eq!(
        server.on_request(&AttPdu::ReadMultipleVariableRequest {
            set_of_handles: vec![1, 2]
        }),
        AttPdu::ReadMultipleVariableResponse {
            length_value_tuples: vec![(5, b"hello".to_vec()), (5, b"world".to_vec())]
        }
    );

    assert_eq!(
        server.on_request(&AttPdu::PrepareWriteRequest {
            attribute_handle: 1,
            value_offset: 1,
            part_attribute_value: b"XX".to_vec(),
        }),
        AttPdu::PrepareWriteResponse {
            attribute_handle: 1,
            value_offset: 1,
            part_attribute_value: b"XX".to_vec(),
        }
    );
    server.on_request(&AttPdu::PrepareWriteRequest {
        attribute_handle: 1,
        value_offset: 3,
        part_attribute_value: b"YY".to_vec(),
    });
    assert_eq!(server.prepared_write_count(), 2);
    assert_eq!(server.attribute(1), Some(&b"hello"[..]));
    assert_eq!(
        server.on_request(&AttPdu::ExecuteWriteRequest { flags: 1 }),
        AttPdu::ExecuteWriteResponse
    );
    assert_eq!(server.attribute(1), Some(&b"hXXYY"[..]));
    assert_eq!(server.prepared_write_count(), 0);

    server.on_request(&AttPdu::PrepareWriteRequest {
        attribute_handle: 1,
        value_offset: 0,
        part_attribute_value: b"cancel".to_vec(),
    });
    server.on_request(&AttPdu::ExecuteWriteRequest { flags: 0 });
    assert_eq!(server.attribute(1), Some(&b"hXXYY"[..]));

    server.on_request(&AttPdu::PrepareWriteRequest {
        attribute_handle: 1,
        value_offset: 99,
        part_attribute_value: vec![1],
    });
    assert_error(
        server.on_request(&AttPdu::ExecuteWriteRequest { flags: 1 }),
        codes::ATT_EXECUTE_WRITE_REQUEST,
        1,
        ATT_INVALID_OFFSET_ERROR,
    );
    assert_eq!(server.attribute(1), Some(&b"hXXYY"[..]));

    server.on_request(&AttPdu::SignedWriteCommand {
        attribute_handle: 1,
        attribute_value: vec![0; 12],
    });
    assert_eq!(server.attribute(1), Some(&b"hXXYY"[..]));
}

#[test]
fn full_gatt_server_runs_the_same_requests_on_value_handles() {
    let mut server = GattServer::new(vec![Service {
        uuid: Uuid::from_16_bits(0x180F),
        characteristics: vec![Characteristic {
            uuid: Uuid::from_16_bits(0x2A19),
            properties: properties::READ | properties::WRITE,
            value: vec![40, 41, 42],
        }],
    }]);
    let value_handle = 3;
    assert_eq!(
        server.on_request(&AttPdu::ReadMultipleRequest {
            set_of_handles: vec![value_handle, value_handle]
        }),
        AttPdu::ReadMultipleResponse {
            set_of_values: vec![40, 41, 42, 40, 41, 42]
        }
    );
    server.on_request(&AttPdu::PrepareWriteRequest {
        attribute_handle: value_handle,
        value_offset: 1,
        part_attribute_value: vec![99, 100],
    });
    assert_eq!(server.prepared_write_count(), 1);
    assert_eq!(
        server.on_request(&AttPdu::ExecuteWriteRequest { flags: 1 }),
        AttPdu::ExecuteWriteResponse
    );
    assert_eq!(
        server.on_request(&AttPdu::ReadRequest {
            attribute_handle: value_handle
        }),
        AttPdu::ReadResponse {
            attribute_value: vec![40, 99, 100]
        }
    );
}

#[test]
fn missing_multiple_read_handle_returns_the_failing_handle() {
    let mut server = AttServer::new();
    server.set_attribute(1, vec![1]);
    let response = server.on_request(&AttPdu::ReadMultipleRequest {
        set_of_handles: vec![1, 99],
    });
    assert_error(
        response,
        codes::ATT_READ_MULTIPLE_REQUEST,
        99,
        bumble_gatt::ATT_ATTRIBUTE_NOT_FOUND_ERROR,
    );
}
