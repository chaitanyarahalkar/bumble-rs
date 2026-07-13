use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use bumble::Uuid;
use bumble_att::{codes, AttPdu};
use bumble_gatt::{
    permissions, properties, AccessContext, CharacteristicDefinition, DatabaseError, DynamicValue,
    GattServer, ServiceDefinition, ATT_ATTRIBUTE_NOT_LONG_ERROR,
    ATT_INSUFFICIENT_AUTHORIZATION_ERROR, ATT_WRITE_NOT_PERMITTED_ERROR,
};

fn server() -> GattServer {
    GattServer::from_definitions(vec![ServiceDefinition {
        uuid: Uuid::from_16_bits(0x1800),
        primary: true,
        included_services: vec![],
        characteristics: vec![CharacteristicDefinition {
            uuid: Uuid::from_16_bits(0x2A00),
            properties: properties::READ | properties::WRITE,
            permissions: permissions::READABLE | permissions::WRITEABLE,
            value: b"static".to_vec(),
            descriptors: vec![],
        }],
    }])
    .unwrap()
}

fn context(bearer_id: u64) -> AccessContext {
    AccessContext {
        bearer_id,
        ..AccessContext::default()
    }
}

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
fn callbacks_are_bearer_aware_shared_by_clones_and_clearable() {
    let values = Arc::new(Mutex::new(BTreeMap::<u64, Vec<u8>>::new()));
    let read_values = Arc::clone(&values);
    let write_values = Arc::clone(&values);
    let dynamic = DynamicValue::read_write(
        move |access| {
            Ok(read_values
                .lock()
                .unwrap()
                .get(&access.bearer_id)
                .cloned()
                .unwrap_or_else(|| vec![access.bearer_id as u8]))
        },
        move |access, value| {
            write_values
                .lock()
                .unwrap()
                .insert(access.bearer_id, value.to_vec());
            Ok(())
        },
    );

    let mut first = server();
    first.set_dynamic_value(3, dynamic).unwrap();
    let mut second = first.clone();
    assert_eq!(
        first.on_request_with_context(
            &AttPdu::ReadRequest {
                attribute_handle: 3,
            },
            context(7),
        ),
        AttPdu::ReadResponse {
            attribute_value: vec![7],
        }
    );
    assert_eq!(
        first.on_request_with_context(
            &AttPdu::WriteRequest {
                attribute_handle: 3,
                attribute_value: b"seven".to_vec(),
            },
            context(7),
        ),
        AttPdu::WriteResponse
    );
    assert_eq!(
        second.on_request_with_context(
            &AttPdu::ReadRequest {
                attribute_handle: 3,
            },
            context(7),
        ),
        AttPdu::ReadResponse {
            attribute_value: b"seven".to_vec(),
        }
    );
    assert_eq!(
        second.on_request_with_context(
            &AttPdu::ReadRequest {
                attribute_handle: 3,
            },
            context(8),
        ),
        AttPdu::ReadResponse {
            attribute_value: vec![8],
        }
    );

    second.clear_dynamic_value(3).unwrap();
    assert_eq!(
        second.on_request(&AttPdu::ReadRequest {
            attribute_handle: 3,
        }),
        AttPdu::ReadResponse {
            attribute_value: b"static".to_vec(),
        }
    );
    assert_eq!(
        second
            .set_dynamic_value(99, DynamicValue::default())
            .unwrap_err(),
        DatabaseError::UnknownAttribute(99)
    );
}

#[test]
fn dynamic_reads_cover_blob_multiple_and_discovery_paths() {
    let value: Vec<u8> = (0..30).collect();
    let callback_value = value.clone();
    let mut server = server();
    server
        .set_dynamic_value(
            3,
            DynamicValue::read_only(move |_| Ok(callback_value.clone())),
        )
        .unwrap();

    assert_eq!(
        server.on_request(&AttPdu::ReadRequest {
            attribute_handle: 3,
        }),
        AttPdu::ReadResponse {
            attribute_value: value[..22].to_vec(),
        }
    );
    assert_eq!(
        server.on_request(&AttPdu::ReadBlobRequest {
            attribute_handle: 3,
            value_offset: 22,
        }),
        AttPdu::ReadBlobResponse {
            part_attribute_value: value[22..].to_vec(),
        }
    );
    assert_eq!(
        server.on_request(&AttPdu::ReadMultipleVariableRequest {
            set_of_handles: vec![3],
        }),
        AttPdu::ReadMultipleVariableResponse {
            length_value_tuples: vec![(30, value[..20].to_vec())],
        }
    );
    assert_eq!(
        server.on_request(&AttPdu::FindByTypeValueRequest {
            starting_handle: 1,
            ending_handle: 3,
            attribute_type: Uuid::from_16_bits(0x2A00),
            attribute_value: value,
        }),
        AttPdu::FindByTypeValueResponse {
            handles_information_list: vec![3, 0, 3, 0],
        }
    );
    assert_error(
        server.on_request(&AttPdu::PrepareWriteRequest {
            attribute_handle: 3,
            value_offset: 0,
            part_attribute_value: vec![1],
        }),
        codes::ATT_PREPARE_WRITE_REQUEST,
        3,
        ATT_ATTRIBUTE_NOT_LONG_ERROR,
    );
}

#[test]
fn callback_errors_are_returned_with_the_request_handle() {
    let mut server = server();
    server
        .set_dynamic_value(
            3,
            DynamicValue::read_only(|_| Err(ATT_INSUFFICIENT_AUTHORIZATION_ERROR)),
        )
        .unwrap();
    assert_error(
        server.on_request(&AttPdu::ReadRequest {
            attribute_handle: 3,
        }),
        codes::ATT_READ_REQUEST,
        3,
        ATT_INSUFFICIENT_AUTHORIZATION_ERROR,
    );
    assert_error(
        server.on_request(&AttPdu::WriteRequest {
            attribute_handle: 3,
            attribute_value: vec![1],
        }),
        codes::ATT_WRITE_REQUEST,
        3,
        ATT_WRITE_NOT_PERMITTED_ERROR,
    );
}
