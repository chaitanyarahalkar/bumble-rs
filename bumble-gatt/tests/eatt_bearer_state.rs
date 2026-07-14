use bumble::Uuid;
use bumble_att::AttPdu;
use bumble_gatt::{
    permissions, properties, AccessContext, CharacteristicDefinition, GattServer, ServiceDefinition,
};

fn context(bearer_id: u64) -> AccessContext {
    AccessContext {
        bearer_id,
        ..AccessContext::default()
    }
}

fn server(value: Vec<u8>, properties: u8) -> GattServer {
    GattServer::from_definitions(vec![ServiceDefinition {
        uuid: Uuid::from_16_bits(0xABCD),
        primary: true,
        included_services: vec![],
        characteristics: vec![CharacteristicDefinition {
            uuid: Uuid::from_16_bits(0x1234),
            properties,
            permissions: permissions::READABLE | permissions::WRITEABLE,
            value,
            descriptors: vec![],
        }],
    }])
    .unwrap()
}

#[test]
fn mtu_and_queued_write_state_are_isolated_per_bearer() {
    let mut server = server(vec![0; 150], properties::READ | properties::WRITE);
    assert!(server.set_max_mtu(100));
    assert_eq!(
        server.on_request_with_context(
            &AttPdu::ExchangeMtuRequest { client_rx_mtu: 80 },
            context(1),
        ),
        AttPdu::ExchangeMtuResponse { server_rx_mtu: 100 }
    );
    let first = server.on_request_with_context(
        &AttPdu::ReadRequest {
            attribute_handle: 3,
        },
        context(1),
    );
    let second = server.on_request_with_context(
        &AttPdu::ReadRequest {
            attribute_handle: 3,
        },
        context(2),
    );
    assert!(matches!(
        first,
        AttPdu::ReadResponse { attribute_value } if attribute_value.len() == 79
    ));
    assert!(matches!(
        second,
        AttPdu::ReadResponse { attribute_value } if attribute_value.len() == 22
    ));

    for (bearer_id, bytes) in [(1, b"one".to_vec()), (2, b"two".to_vec())] {
        assert!(matches!(
            server.on_request_with_context(
                &AttPdu::PrepareWriteRequest {
                    attribute_handle: 3,
                    value_offset: 0,
                    part_attribute_value: bytes,
                },
                context(bearer_id),
            ),
            AttPdu::PrepareWriteResponse { .. }
        ));
    }
    assert_eq!(server.prepared_write_count(), 2);
    assert_eq!(
        server.on_request_with_context(&AttPdu::ExecuteWriteRequest { flags: 1 }, context(1),),
        AttPdu::ExecuteWriteResponse
    );
    assert_eq!(server.prepared_write_count(), 1);
    assert!(matches!(
        server.on_request_with_context(
            &AttPdu::ReadRequest {
                attribute_handle: 3,
            },
            context(1),
        ),
        AttPdu::ReadResponse { attribute_value } if attribute_value.starts_with(b"one")
    ));
    assert_eq!(
        server.on_request_with_context(&AttPdu::ExecuteWriteRequest { flags: 1 }, context(2),),
        AttPdu::ExecuteWriteResponse
    );
    assert!(matches!(
        server.on_request_with_context(
            &AttPdu::ReadRequest {
                attribute_handle: 3,
            },
            context(2),
        ),
        AttPdu::ReadResponse { attribute_value } if attribute_value.starts_with(b"two")
    ));
}

#[test]
fn cccd_values_and_cleanup_are_bearer_scoped() {
    let mut server = server(
        vec![0],
        properties::READ | properties::NOTIFY | properties::INDICATE,
    );
    for (bearer_id, bits) in [(10, 1u16), (20, 2u16)] {
        assert_eq!(
            server.on_request_with_context(
                &AttPdu::WriteRequest {
                    attribute_handle: 4,
                    attribute_value: bits.to_le_bytes().to_vec(),
                },
                context(bearer_id),
            ),
            AttPdu::WriteResponse
        );
    }
    assert_eq!(server.subscription_bits(10, 3), 1);
    assert_eq!(server.subscription_bits(20, 3), 2);
    assert_eq!(
        server.on_request_with_context(
            &AttPdu::ReadRequest {
                attribute_handle: 4,
            },
            context(10),
        ),
        AttPdu::ReadResponse {
            attribute_value: 1u16.to_le_bytes().to_vec(),
        }
    );
    server.remove_bearer(10);
    assert_eq!(server.subscription_bits(10, 3), 0);
    assert_eq!(server.subscription_bits(20, 3), 2);
}
