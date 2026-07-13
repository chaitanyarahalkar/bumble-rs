use bumble::Uuid;
use bumble_att::{codes, AttPdu};
use bumble_gatt::{
    permissions, properties, AccessContext, CharacteristicDefinition, DatabaseError,
    DescriptorDefinition, GattServer, ServiceDefinition, ATT_INSUFFICIENT_AUTHENTICATION_ERROR,
    ATT_INSUFFICIENT_AUTHORIZATION_ERROR, ATT_INSUFFICIENT_ENCRYPTION_ERROR,
    ATT_READ_NOT_PERMITTED_ERROR, ATT_WRITE_NOT_PERMITTED_ERROR, GATT_INCLUDE_UUID,
    GATT_PRIMARY_SERVICE_UUID, GATT_SECONDARY_SERVICE_UUID,
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

fn database() -> GattServer {
    GattServer::from_definitions(vec![
        ServiceDefinition {
            uuid: Uuid::from_16_bits(0x180F),
            primary: false,
            included_services: vec![],
            characteristics: vec![CharacteristicDefinition {
                uuid: Uuid::from_16_bits(0x2A19),
                properties: properties::READ | properties::NOTIFY,
                permissions: permissions::READABLE,
                value: vec![87],
                descriptors: vec![DescriptorDefinition {
                    uuid: Uuid::from_16_bits(0x2901),
                    permissions: permissions::READABLE,
                    value: b"Battery Level".to_vec(),
                }],
            }],
        },
        ServiceDefinition {
            uuid: Uuid::from_16_bits(0x1843),
            primary: true,
            included_services: vec![0],
            characteristics: vec![CharacteristicDefinition {
                uuid: Uuid::from_16_bits(0x2B7D),
                properties: properties::READ | properties::WRITE,
                permissions: permissions::READ_REQUIRES_ENCRYPTION
                    | permissions::READ_REQUIRES_AUTHENTICATION
                    | permissions::READ_REQUIRES_AUTHORIZATION
                    | permissions::WRITE_REQUIRES_ENCRYPTION
                    | permissions::WRITE_REQUIRES_AUTHENTICATION
                    | permissions::WRITE_REQUIRES_AUTHORIZATION,
                value: vec![1, 2, 3],
                descriptors: vec![],
            }],
        },
    ])
    .unwrap()
}

#[test]
fn builds_secondary_include_descriptor_and_automatic_cccd() {
    let mut server = database();

    assert_eq!(
        server.on_request(&AttPdu::ReadByGroupTypeRequest {
            starting_handle: 1,
            ending_handle: u16::MAX,
            attribute_group_type: Uuid::from_16_bits(GATT_SECONDARY_SERVICE_UUID),
        }),
        AttPdu::ReadByGroupTypeResponse {
            length: 6,
            attribute_data_list: vec![1, 0, 5, 0, 0x0F, 0x18],
        }
    );
    assert_eq!(
        server.on_request(&AttPdu::ReadByGroupTypeRequest {
            starting_handle: 1,
            ending_handle: u16::MAX,
            attribute_group_type: Uuid::from_16_bits(GATT_PRIMARY_SERVICE_UUID),
        }),
        AttPdu::ReadByGroupTypeResponse {
            length: 6,
            attribute_data_list: vec![6, 0, 9, 0, 0x43, 0x18],
        }
    );

    assert_eq!(
        server.on_request(&AttPdu::ReadByTypeRequest {
            starting_handle: 6,
            ending_handle: 9,
            attribute_type: Uuid::from_16_bits(GATT_INCLUDE_UUID),
        }),
        AttPdu::ReadByTypeResponse {
            length: 8,
            attribute_data_list: vec![7, 0, 1, 0, 5, 0, 0x0F, 0x18],
        }
    );
    assert_eq!(
        server.on_request(&AttPdu::ReadRequest {
            attribute_handle: 4,
        }),
        AttPdu::ReadResponse {
            attribute_value: b"Battery Level".to_vec(),
        }
    );
    assert_eq!(
        server.on_request(&AttPdu::ReadRequest {
            attribute_handle: 5,
        }),
        AttPdu::ReadResponse {
            attribute_value: vec![0, 0],
        }
    );
}

#[test]
fn enforces_access_and_security_requirements_in_order() {
    let mut server = database();
    let read = AttPdu::ReadRequest {
        attribute_handle: 9,
    };
    assert_error(
        server.on_request(&read),
        codes::ATT_READ_REQUEST,
        9,
        ATT_INSUFFICIENT_ENCRYPTION_ERROR,
    );
    assert_error(
        server.on_request_with_context(
            &read,
            AccessContext {
                bearer_id: 0,
                encrypted: true,
                ..AccessContext::default()
            },
        ),
        codes::ATT_READ_REQUEST,
        9,
        ATT_INSUFFICIENT_AUTHENTICATION_ERROR,
    );
    assert_error(
        server.on_request_with_context(
            &read,
            AccessContext {
                bearer_id: 0,
                encrypted: true,
                authenticated: true,
                authorized: false,
            },
        ),
        codes::ATT_READ_REQUEST,
        9,
        ATT_INSUFFICIENT_AUTHORIZATION_ERROR,
    );

    let granted = AccessContext {
        bearer_id: 0,
        encrypted: true,
        authenticated: true,
        authorized: true,
    };
    assert_eq!(
        server.on_request_with_context(&read, granted),
        AttPdu::ReadResponse {
            attribute_value: vec![1, 2, 3],
        }
    );
    assert_eq!(
        server.on_request_with_context(
            &AttPdu::WriteRequest {
                attribute_handle: 9,
                attribute_value: vec![4, 5],
            },
            granted,
        ),
        AttPdu::WriteResponse
    );
    assert_eq!(
        server.on_request_with_context(&read, granted),
        AttPdu::ReadResponse {
            attribute_value: vec![4, 5],
        }
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
    let mut write_only = GattServer::from_definitions(vec![ServiceDefinition {
        uuid: Uuid::from_16_bits(0x1800),
        primary: true,
        included_services: vec![],
        characteristics: vec![CharacteristicDefinition {
            uuid: Uuid::from_16_bits(0x2A00),
            properties: properties::WRITE,
            permissions: permissions::WRITEABLE,
            value: vec![],
            descriptors: vec![],
        }],
    }])
    .unwrap();
    assert_error(
        write_only.on_request(&AttPdu::ReadRequest {
            attribute_handle: 3,
        }),
        codes::ATT_READ_REQUEST,
        3,
        ATT_READ_NOT_PERMITTED_ERROR,
    );
}

#[test]
fn queued_and_multiple_operations_share_permission_checks() {
    let mut server = database();
    assert_error(
        server.on_request(&AttPdu::ReadMultipleRequest {
            set_of_handles: vec![3, 9],
        }),
        codes::ATT_READ_MULTIPLE_REQUEST,
        9,
        ATT_INSUFFICIENT_ENCRYPTION_ERROR,
    );
    assert_error(
        server.on_request(&AttPdu::PrepareWriteRequest {
            attribute_handle: 9,
            value_offset: 0,
            part_attribute_value: vec![7],
        }),
        codes::ATT_PREPARE_WRITE_REQUEST,
        9,
        ATT_INSUFFICIENT_ENCRYPTION_ERROR,
    );
    assert_eq!(server.prepared_write_count(), 0);

    let granted = AccessContext {
        bearer_id: 0,
        encrypted: true,
        authenticated: true,
        authorized: true,
    };
    assert!(matches!(
        server.on_request_with_context(
            &AttPdu::PrepareWriteRequest {
                attribute_handle: 9,
                value_offset: 0,
                part_attribute_value: vec![7],
            },
            granted,
        ),
        AttPdu::PrepareWriteResponse { .. }
    ));
    assert_error(
        server.on_request(&AttPdu::ExecuteWriteRequest { flags: 1 }),
        codes::ATT_EXECUTE_WRITE_REQUEST,
        9,
        ATT_INSUFFICIENT_ENCRYPTION_ERROR,
    );
    assert_eq!(server.prepared_write_count(), 0);
}

#[test]
fn omits_uuid_from_128_bit_include_and_rejects_bad_indices() {
    let custom = Uuid::parse("3A12C182-14E2-4FE0-8C5B-65D7C569F9DB").unwrap();
    let mut server = GattServer::from_definitions(vec![
        ServiceDefinition {
            uuid: custom,
            primary: false,
            included_services: vec![],
            characteristics: vec![],
        },
        ServiceDefinition {
            uuid: Uuid::from_16_bits(0x1800),
            primary: true,
            included_services: vec![0],
            characteristics: vec![],
        },
    ])
    .unwrap();
    assert_eq!(
        server.on_request(&AttPdu::ReadRequest {
            attribute_handle: 3,
        }),
        AttPdu::ReadResponse {
            attribute_value: vec![1, 0, 1, 0],
        }
    );

    let error = GattServer::from_definitions(vec![ServiceDefinition {
        uuid: Uuid::from_16_bits(0x1800),
        primary: true,
        included_services: vec![1],
        characteristics: vec![],
    }])
    .unwrap_err();
    assert_eq!(
        error,
        DatabaseError::InvalidIncludedService {
            service: 0,
            included: 1,
        }
    );
}
