//! Slice-18 capstone: a real GATT client driving a real GATT server. The
//! [`GattClient`] issues ATT requests; the [`GattServer`] answers them (the
//! blanket `AttTransport` impl wires the two directly, standing in for the
//! L2CAP/ACL transport a live stack would use). The flow exercises discovery,
//! short and long reads, writes with and without response, and
//! notify/indicate subscriptions end-to-end.

use std::sync::{Arc, Mutex};

use bumble::Uuid;
use bumble_att::AttPdu;
use bumble_gatt::{
    properties, AttRequestHandler, AttTransport, Characteristic, GattClient, GattError, GattServer,
    Service, ServiceDefinition,
};

struct FailingTransport;

impl AttTransport for FailingTransport {
    fn request(&mut self, _request: &AttPdu) -> AttPdu {
        unreachable!("the fallible path must be used")
    }

    fn try_request(&mut self, _request: &AttPdu) -> Result<AttPdu, String> {
        Err("bearer closed".into())
    }
}

#[test]
fn surfaces_transport_failures_without_fabricating_att_errors() {
    let mut client = GattClient::new();
    assert_eq!(
        client.read_value(&mut FailingTransport, 1, false),
        Err(GattError::Transport("bearer closed".into()))
    );
}

/// Device Information (0x180A) with:
/// - Device Name (0x2A00), READ, a short value;
/// - Serial Number (0x2A25), READ, a 25-byte value (forces a long read);
/// - Heart Rate Measurement (0x2A37), NOTIFY|INDICATE (gets a CCCD).
///
/// Resulting handle layout:
/// 1 service · 2/3 name decl/value · 4/5 serial decl/value ·
/// 6/7 HRM decl/value · 8 CCCD.
fn sample_server() -> GattServer {
    GattServer::new(vec![Service {
        uuid: Uuid::from_16_bits(0x180A),
        characteristics: vec![
            Characteristic {
                uuid: Uuid::from_16_bits(0x2A00),
                properties: properties::READ,
                value: b"Hi".to_vec(),
            },
            Characteristic {
                uuid: Uuid::from_16_bits(0x2A25),
                properties: properties::READ,
                value: (0u8..25).collect(),
            },
            Characteristic {
                uuid: Uuid::from_16_bits(0x2A37),
                properties: properties::NOTIFY | properties::INDICATE,
                value: vec![0x00],
            },
        ],
    }])
}

struct FailingCccdClear {
    server: GattServer,
    fail_clear: bool,
}

impl AttTransport for FailingCccdClear {
    fn request(&mut self, request: &AttPdu) -> AttPdu {
        self.server.handle_request(request)
    }

    fn try_request(&mut self, request: &AttPdu) -> Result<AttPdu, String> {
        if self.fail_clear
            && matches!(
                request,
                AttPdu::WriteRequest {
                    attribute_handle: 8,
                    attribute_value,
                } if attribute_value == &[0, 0]
            )
        {
            return Err("CCCD bearer failed".into());
        }
        Ok(self.server.handle_request(request))
    }
}

#[test]
fn client_discovers_reads_writes_and_subscribes() {
    let mut server = sample_server();
    let mut client = GattClient::new();

    // MTU exchange: server caps at the default 23.
    let mtu = client.exchange_mtu(&mut server, 517).unwrap();
    assert_eq!(mtu, 23);
    assert_eq!(client.mtu(), 23);

    // Discover all primary services.
    let services = client.discover_services(&mut server).unwrap();
    assert_eq!(services.len(), 1);
    let service = &services[0];
    assert_eq!(service.handle, 1);
    assert_eq!(service.end_group_handle, 8);
    assert_eq!(service.uuid, Uuid::from_16_bits(0x180A));

    let attributes = client.discover_attributes(&mut server).unwrap();
    assert_eq!(attributes.len(), 8);
    assert_eq!(attributes.first().unwrap().handle, 1);
    assert_eq!(attributes.last().unwrap().handle, 8);

    // Discover the service by UUID (Find By Type Value) — same result.
    let by_uuid = client
        .discover_service_by_uuid(&mut server, &Uuid::from_16_bits(0x180A))
        .unwrap();
    assert_eq!(by_uuid.len(), 1);
    assert_eq!(by_uuid[0].handle, 1);
    assert_eq!(by_uuid[0].end_group_handle, 8);

    // Discover characteristics.
    let chars = client
        .discover_characteristics(&mut server, service)
        .unwrap();
    assert_eq!(chars.len(), 3);

    let name = &chars[0];
    assert_eq!(name.handle, 3); // value handle
    assert_eq!(name.uuid, Uuid::from_16_bits(0x2A00));
    assert_eq!(name.properties, properties::READ);

    let serial = &chars[1];
    assert_eq!(serial.handle, 5);
    assert_eq!(serial.uuid, Uuid::from_16_bits(0x2A25));

    let hrm = &chars[2];
    assert_eq!(hrm.handle, 7);
    assert_eq!(hrm.uuid, Uuid::from_16_bits(0x2A37));
    assert_eq!(hrm.properties, properties::NOTIFY | properties::INDICATE);
    // The last characteristic extends to the end of the service group.
    assert_eq!(hrm.end_group_handle, 8);

    // Discover the HRM characteristic's descriptors — the CCCD at handle 8.
    let descriptors = client.discover_descriptors(&mut server, hrm).unwrap();
    assert_eq!(descriptors.len(), 1);
    assert_eq!(descriptors[0].handle, 8);
    assert_eq!(descriptors[0].uuid, Uuid::from_16_bits(0x2902));
    let cccd_handle = descriptors[0].handle;

    // Short read.
    assert_eq!(
        client.read_value(&mut server, name.handle, false).unwrap(),
        b"Hi"
    );

    // Long read: the 25-byte value spans a Read Request + one Read Blob.
    let long = client
        .read_value(&mut server, serial.handle, false)
        .unwrap();
    assert_eq!(long, (0u8..25).collect::<Vec<u8>>());

    // Write with response, then read it back.
    client
        .write_value(&mut server, hrm.handle, vec![0xAB, 0xCD], true)
        .unwrap();
    assert_eq!(
        client.read_value(&mut server, hrm.handle, false).unwrap(),
        vec![0xAB, 0xCD]
    );

    // Write without response (command), then read it back.
    client
        .write_value(&mut server, hrm.handle, vec![0x11], false)
        .unwrap();
    assert_eq!(
        client.read_value(&mut server, hrm.handle, false).unwrap(),
        vec![0x11]
    );

    // Subscribe for notifications: writes the CCCD with 0x0001.
    client
        .subscribe(&mut server, hrm.handle, cccd_handle, false)
        .unwrap();
    assert_eq!(
        client.read_value(&mut server, cccd_handle, false).unwrap(),
        vec![0x01, 0x00]
    );

    // The server notifies; the client caches the value against the subscription.
    let notification = server.notify(hrm.handle, vec![0x22, 0x33]);
    assert!(client.on_notification(&notification).unwrap());
    assert_eq!(client.cached_value(hrm.handle), Some(&[0x22, 0x33][..]));

    // Switch to indications: writes the CCCD with 0x0002.
    client
        .subscribe(&mut server, hrm.handle, cccd_handle, true)
        .unwrap();
    assert_eq!(
        client.read_value(&mut server, cccd_handle, false).unwrap(),
        vec![0x02, 0x00]
    );

    // An indication is cached and must be confirmed back to the server.
    let indication = server.indicate(hrm.handle, vec![0x44]);
    let confirmation = client.on_indication(&indication).unwrap();
    assert_eq!(confirmation, bumble_att::AttPdu::HandleValueConfirmation);
    assert_eq!(client.cached_value(hrm.handle), Some(&[0x44][..]));

    // Reading a missing handle surfaces the ATT error.
    match client.read_value(&mut server, 0x0099, false) {
        Err(GattError::Att { error_code, .. }) => assert_eq!(error_code, 0x0A),
        other => panic!("expected ATT not-found error, got {other:?}"),
    }
}

#[test]
fn discovery_on_empty_server_returns_nothing() {
    let mut server = GattServer::new(vec![]);
    let mut client = GattClient::new();
    assert!(client.discover_services(&mut server).unwrap().is_empty());
    assert!(client
        .discover_service_by_uuid(&mut server, &Uuid::from_16_bits(0x180A))
        .unwrap()
        .is_empty());
}

#[test]
fn discovers_secondary_and_mixed_width_included_services() {
    let custom = Uuid::parse("3A12C182-14E2-4FE0-8C5B-65D7C569F9DB").unwrap();
    let primary_uuid = Uuid::from_16_bits(0x1844);
    let mut server = GattServer::from_definitions(vec![
        ServiceDefinition {
            uuid: Uuid::from_16_bits(0x1845),
            primary: false,
            included_services: Vec::new(),
            characteristics: Vec::new(),
        },
        ServiceDefinition {
            uuid: custom.clone(),
            primary: false,
            included_services: Vec::new(),
            characteristics: Vec::new(),
        },
        ServiceDefinition {
            uuid: primary_uuid.clone(),
            primary: true,
            included_services: vec![0, 1],
            characteristics: Vec::new(),
        },
    ])
    .unwrap();
    let mut client = GattClient::new();
    let primary = client
        .discover_service_by_uuid(&mut server, &primary_uuid)
        .unwrap()
        .remove(0);
    let included = client
        .discover_included_services(&mut server, &primary)
        .unwrap();
    assert_eq!(included.len(), 2);
    assert_eq!(included[0].handle, 1);
    assert_eq!(included[0].end_group_handle, 1);
    assert_eq!(included[0].uuid, Uuid::from_16_bits(0x1845));
    assert_eq!(included[1].handle, 2);
    assert_eq!(included[1].end_group_handle, 2);
    assert_eq!(included[1].uuid, custom.clone());

    let secondary = client
        .discover_secondary_service_by_uuid(&mut server, &custom)
        .unwrap();
    assert_eq!(secondary, [included[1].clone()]);
}

#[test]
fn multiple_value_listeners_share_cccd_until_the_last_listener_leaves() {
    let mut server = sample_server();
    let mut client = GattClient::default();
    assert_eq!(client.mtu(), 23);
    let value_handle = 7;
    let cccd_handle = 8;
    let calls = Arc::new(Mutex::new(Vec::new()));

    let first_calls = Arc::clone(&calls);
    let first = client
        .subscribe_with_listener(
            &mut server,
            value_handle,
            cccd_handle,
            false,
            move |value| first_calls.lock().unwrap().push((1, value.to_vec())),
        )
        .unwrap();
    let second_calls = Arc::clone(&calls);
    let second = client
        .subscribe_with_listener(
            &mut server,
            value_handle,
            cccd_handle,
            false,
            move |value| second_calls.lock().unwrap().push((2, value.to_vec())),
        )
        .unwrap();
    assert!(first < second);
    assert!(client.is_subscribed(value_handle, false));
    assert_eq!(client.subscription_listener_count(value_handle, false), 2);

    assert!(client
        .on_notification(&server.notify(value_handle, vec![0x10]))
        .unwrap());
    assert_eq!(
        *calls.lock().unwrap(),
        vec![(1, vec![0x10]), (2, vec![0x10])]
    );
    assert_eq!(client.cached_value(value_handle), Some(&[0x10][..]));

    assert!(client
        .unsubscribe_listener(&mut server, value_handle, cccd_handle, first, false)
        .unwrap());
    assert_eq!(client.subscription_listener_count(value_handle, false), 1);
    assert_eq!(
        client.read_value(&mut server, cccd_handle, false).unwrap(),
        vec![0x01, 0x00]
    );
    assert!(client
        .on_notification(&server.notify(value_handle, vec![0x20]))
        .unwrap());
    assert_eq!(
        *calls.lock().unwrap(),
        vec![(1, vec![0x10]), (2, vec![0x10]), (2, vec![0x20])]
    );
    assert!(!client
        .unsubscribe_listener(&mut server, value_handle, cccd_handle, first, false)
        .unwrap());

    assert!(client
        .unsubscribe_listener(&mut server, value_handle, cccd_handle, second, false)
        .unwrap());
    assert!(!client.is_subscribed(value_handle, false));
    assert_eq!(
        client.read_value(&mut server, cccd_handle, false).unwrap(),
        vec![0x00, 0x00]
    );
    assert!(!client
        .on_notification(&server.notify(value_handle, vec![0x30]))
        .unwrap());
    assert_eq!(client.cached_value(value_handle), Some(&[0x30][..]));
}

#[test]
fn indication_callbacks_implicit_subscriptions_and_forced_cleanup_match_upstream() {
    let mut server = sample_server();
    let mut client = GattClient::new();
    let value_handle = 7;
    let cccd_handle = 8;
    let calls = Arc::new(Mutex::new(Vec::new()));

    let indication_calls = Arc::clone(&calls);
    let indication = client
        .subscribe_with_listener(&mut server, value_handle, cccd_handle, true, move |value| {
            indication_calls.lock().unwrap().push(value.to_vec())
        })
        .unwrap();
    assert!(client.is_subscribed(value_handle, true));
    assert_eq!(
        client
            .on_indication(&server.indicate(value_handle, vec![0x44]))
            .unwrap(),
        AttPdu::HandleValueConfirmation
    );
    assert_eq!(*calls.lock().unwrap(), vec![vec![0x44]]);
    assert!(client
        .unsubscribe_listener(&mut server, value_handle, cccd_handle, indication, false,)
        .unwrap());
    assert!(!client.is_subscribed(value_handle, true));

    client
        .subscribe(&mut server, value_handle, cccd_handle, false)
        .unwrap();
    let notification_calls = Arc::clone(&calls);
    let notification = client
        .subscribe_with_listener(
            &mut server,
            value_handle,
            cccd_handle,
            false,
            move |value| notification_calls.lock().unwrap().push(value.to_vec()),
        )
        .unwrap();
    assert!(client
        .unsubscribe_listener(&mut server, value_handle, cccd_handle, notification, false,)
        .unwrap());
    assert!(client.is_subscribed(value_handle, false));
    assert_eq!(client.subscription_listener_count(value_handle, false), 0);
    assert_eq!(
        client.read_value(&mut server, cccd_handle, false).unwrap(),
        vec![0x01, 0x00]
    );
    assert!(client
        .unsubscribe_all(&mut server, value_handle, cccd_handle, false)
        .unwrap());
    assert!(!client
        .unsubscribe_all(&mut server, value_handle, cccd_handle, false)
        .unwrap());

    client
        .write_value(&mut server, cccd_handle, vec![0x01, 0x00], true)
        .unwrap();
    assert!(!client
        .unsubscribe_all(&mut server, value_handle, cccd_handle, false)
        .unwrap());
    assert_eq!(
        client.read_value(&mut server, cccd_handle, false).unwrap(),
        vec![0x01, 0x00]
    );
    assert!(!client
        .unsubscribe_all(&mut server, value_handle, cccd_handle, true)
        .unwrap());
    assert_eq!(
        client.read_value(&mut server, cccd_handle, false).unwrap(),
        vec![0x00, 0x00]
    );
}

#[test]
fn failed_last_listener_cleanup_preserves_state_for_retry() {
    let mut transport = FailingCccdClear {
        server: sample_server(),
        fail_clear: true,
    };
    let calls = Arc::new(Mutex::new(Vec::new()));
    let listener_calls = Arc::clone(&calls);
    let mut client = GattClient::new();
    let listener = client
        .subscribe_with_listener(&mut transport, 7, 8, false, move |value| {
            listener_calls.lock().unwrap().push(value.to_vec());
        })
        .unwrap();

    assert_eq!(
        client.unsubscribe_listener(&mut transport, 7, 8, listener, false),
        Err(GattError::Transport("CCCD bearer failed".into()))
    );
    assert!(client.is_subscribed(7, false));
    assert_eq!(client.subscription_listener_count(7, false), 1);
    assert!(client
        .on_notification(&transport.server.notify(7, vec![0x55]))
        .unwrap());
    assert_eq!(*calls.lock().unwrap(), vec![vec![0x55]]);

    transport.fail_clear = false;
    assert!(client
        .unsubscribe_listener(&mut transport, 7, 8, listener, false)
        .unwrap());
    assert!(!client.is_subscribed(7, false));
}
