# Pairing

Pairing is managed by an SMP `PairingManager` that a `Device` builds from its
configuration. A plain `Device::new` has no pairing manager — construct the
device with `Device::from_config` (or `with_server_and_config`) to enable
pairing.

```rust
use bumble::{Address, AddressType};
use bumble_host::{pump, Device, DeviceConfiguration};
use bumble_smp::{IoCapability, ManagedPairingState, ScPairingState};

let mut devices = [
    Device::from_config(
        central_id,
        DeviceConfiguration {
            address: Address::parse("C4:F2:17:1A:1D:AA", AddressType::RANDOM_DEVICE).unwrap(),
            gap_service_enabled: false,
            gatt_service_enabled: false,
            identity_address_type: Some(1),
            io_capability: IoCapability::NoInputNoOutput as u8,
            ..DeviceConfiguration::default()
        },
    )
    .unwrap(),
    Device::from_config(
        peripheral_id,
        DeviceConfiguration {
            address: Address::parse("C4:F2:17:1A:1D:BB", AddressType::RANDOM_DEVICE).unwrap(),
            gap_service_enabled: false,
            gatt_service_enabled: false,
            identity_address_type: Some(1),
            io_capability: IoCapability::NoInputNoOutput as u8,
            ..DeviceConfiguration::default()
        },
    )
    .unwrap(),
];
assert!(devices.iter().all(Device::has_pairing_manager));

// ... advertise, scan, connect as in the virtual-link example ...
pump(&mut link, &mut devices);
let handle = devices[0].connection_handle().unwrap();

// Initiate pairing on the current LE connection.
devices[0].pair(&mut link).unwrap();
pump(&mut link, &mut devices);

for device in &mut devices {
    assert!(device.is_encrypted());
    assert_eq!(
        device.pairing_state(handle),
        Some(ManagedPairingState::SecureConnections(ScPairingState::Complete)),
    );
    assert!(device.pairing_keys(handle).unwrap().ltk.is_some());
}
```

## What's supported

- **LE Legacy and Secure Connections** pairing with every association model
  (Just Works, Numeric Comparison, Passkey Entry, OOB), driven by the
  `io_capability` in the configuration.
- **Classic pairing** via `Device::pair_classic`, and cross-transport key
  derivation (CTKD) between LE and BR/EDR.
- **Key distribution**: LTK, IRK, CSRK — inspect results with
  `Device::pairing_keys(handle)`.
- **Persistent bonds**: configure a `keystore` in `DeviceConfiguration` to
  store `PairingKeys` as JSON, compatible with upstream Bumble's key store.
- **Privacy**: IRK-based resolvable private address generation and
  resolution (see the `bumble-rpa-tool` binary and `bumble_smp`'s
  `AddressResolver`).

## Observing pairing

Pairing progress and results surface through the standard event journal:

```rust
use bumble_host::DeviceEvent;

for event in devices[0].take_device_events() {
    match event {
        DeviceEvent::PairingComplete { connection_handle, keys } => { /* ... */ }
        DeviceEvent::PairingFailed { connection_handle, reason } => { /* ... */ }
        DeviceEvent::EncryptionChange { .. } => { /* ... */ }
        _ => {}
    }
}
```

Errors are also available via `Device::pairing_failure(handle)` and
`Device::take_pairing_errors()`.

Against real controllers, `bumble-transport` provides `LePairingSession` and
`ClassicPairingSession` orchestrators with `run_to_completion` and
`store_bond` helpers — the `bumble-pair` tool is a complete worked example
([Apps and Tools](../guide/apps-and-tools.md)).
