# GATT Server and Client

A peripheral publishes a GATT service; a central connects, discovers it, and
reads a characteristic. This builds on the
[virtual-link example](virtual-link.md) — the setup of `link`, controllers,
and addresses is the same.

## Registering a server

A GATT server is attached when the `Device` is constructed:

```rust
use bumble::Uuid;
use bumble_gatt::{Characteristic, GattServer, Service};
use bumble_host::Device;

// Device Information service (0x180A) with a readable Device Name (0x2A00).
let server = GattServer::new(vec![Service {
    uuid: Uuid::from_16_bits(0x180A),
    characteristics: vec![Characteristic {
        uuid: Uuid::from_16_bits(0x2A00),
        properties: 0x02, // READ
        value: b"bumble-rs".to_vec(),
    }],
}]);

let mut devices = [
    Device::new(central_id),
    Device::with_server(peripheral_id, server),
];
```

Servers can also be declared in JSON device configuration, matching upstream
Bumble's format:

```rust
use bumble_host::DeviceConfiguration;

let config = DeviceConfiguration::from_json_str(
    r#"{
        "name": "Configured GATT",
        "eatt_enabled": true,
        "gatt_services": [{
            "uuid": "180F",
            "characteristics": [{
                "uuid": "2A19",
                "properties": "READ|WRITE",
                "permissions": "READABLE|WRITEABLE",
                "descriptors": [
                    { "descriptor_type": "2901", "permissions": "READABLE" }
                ]
            }]
        }]
    }"#,
)
.unwrap();
let peripheral = Device::from_config(peripheral_id, config).unwrap();
```

## Client operations

After connecting (see the [virtual-link example](virtual-link.md)), the
central drives ATT directly: send a typed `AttPdu`, `pump`, and drain the
inbox. Discovery uses the standard GATT procedures:

```rust
use bumble::Uuid;
use bumble_att::AttPdu;
use bumble_gatt::{GATT_CHARACTERISTIC_UUID, GATT_PRIMARY_SERVICE_UUID};
use bumble_host::pump;

// 1. Discover primary services (Read By Group Type).
devices[0].send_att(&mut link, &AttPdu::ReadByGroupTypeRequest {
    starting_handle: 0x0001,
    ending_handle: 0xFFFF,
    attribute_group_type: Uuid::from_16_bits(GATT_PRIMARY_SERVICE_UUID),
});
pump(&mut link, &mut devices);
let services = devices[0].take_inbox();

// 2. Discover characteristics within the service range (Read By Type).
devices[0].send_att(&mut link, &AttPdu::ReadByTypeRequest {
    starting_handle: svc_start,
    ending_handle: svc_end,
    attribute_type: Uuid::from_16_bits(GATT_CHARACTERISTIC_UUID),
});
pump(&mut link, &mut devices);
let characteristics = devices[0].take_inbox();

// 3. Read the characteristic value.
devices[0].send_att(&mut link, &AttPdu::ReadRequest { attribute_handle: value_handle });
pump(&mut link, &mut devices);
assert_eq!(
    devices[0].take_inbox(),
    vec![AttPdu::ReadResponse { attribute_value: b"bumble-rs".to_vec() }],
);
```

Writes follow the same shape:

```rust
devices[0].send_att(&mut link, &AttPdu::WriteRequest {
    attribute_handle: 0x0025,
    attribute_value: vec![0xBB, 0xCC],
});
pump(&mut link, &mut devices);
assert_eq!(devices[0].take_inbox(), vec![AttPdu::WriteResponse]);
```

## Notifications and subscriptions

The server side pushes values with `Device::notify` (and
`notify_subscribers` / `indicate_subscribers` for CCCD-based delivery); the
client subscribes by writing to the characteristic's CCCD and then reads
notifications from its inbox like any other PDU.

`bumble-gatt` also provides a `GattClient` runtime that wraps these
procedures (service/characteristic/descriptor discovery, long reads via
Read Blob, subscriptions) over an `AttTransport` — see the
[bumble_gatt API docs](https://chaitanyarahalkar.github.io/bumble-rs/api/bumble_gatt/)
for details.
