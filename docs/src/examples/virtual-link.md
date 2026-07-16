# Two Devices on a Virtual Link

The core bumble-rs pattern: two `Device`s attached to software controllers on
an in-process `LocalLink`, advertising, scanning, and connecting — no
hardware, fully deterministic.

```rust
use bumble::{Address, AddressType};
use bumble_controller::{Controller, LocalLink};
use bumble_host::{pump, Device};

fn address(value: &str) -> Address {
    Address::parse(value, AddressType::RANDOM_DEVICE).unwrap()
}

fn public_address(value: &str) -> Address {
    Address::parse(value, AddressType::PUBLIC_DEVICE).unwrap()
}

fn main() {
    // One virtual radio, two software controllers.
    let mut link = LocalLink::new();
    let central_id =
        link.add_controller(Controller::new("central", public_address("00:00:00:00:00:01")));
    let peripheral_id =
        link.add_controller(Controller::new("peripheral", public_address("00:00:00:00:00:02")));

    let central_address = address("C4:F2:17:1A:1D:AA");
    let peripheral_address = address("C4:F2:17:1A:1D:BB");

    // A Device is bound to its controller by id and drives it through the link.
    let mut devices = [Device::new(central_id), Device::new(peripheral_id)];
    devices[0].set_random_address(&mut link, central_address);
    devices[1].set_random_address(&mut link, peripheral_address.clone());

    // Peripheral advertises (flags + shortened local name "RS").
    assert!(devices[1].start_advertising(&mut link, &[2, 0x01, 0x06, 3, 0x09, b'R', b'S']));

    // Central scans.
    devices[0].start_scanning(&mut link, true, false);
    pump(&mut link, &mut devices);

    // Deliver pending advertising PDUs to scanners, then drain reports.
    link.propagate_advertising();
    pump(&mut link, &mut devices);
    let reports = devices[0].take_advertising_reports();
    assert_eq!(reports[0].address, peripheral_address);
    devices[0].stop_scanning(&mut link);

    // Connect and verify both sides.
    assert!(devices[0].connect_le(&mut link, peripheral_address.clone()));
    pump(&mut link, &mut devices);
    assert!(devices[0].is_connected());
    assert!(devices[1].is_connected());
    assert_eq!(devices[0].connection_role(), Some(0)); // central
    assert_eq!(devices[1].connection_role(), Some(1)); // peripheral

    // Disconnect (0x13 = remote user terminated connection).
    assert!(devices[0].disconnect(&mut link, 0x13));
    pump(&mut link, &mut devices);
}
```

## How the drive loop works

Nothing here is asynchronous. Progress happens through three explicit steps:

- **`Device::poll(&mut link)`** drains and processes the controller's pending
  HCI events for that device, returning `true` if anything happened.
- **`pump(&mut link, &mut devices)`** is the convenience driver: it repeatedly
  pumps the link (`pump_ll`, `pump_classic`, connection establishment, …) and
  polls every device until the whole system is quiescent.
- **`link.propagate_advertising()`** delivers advertising PDUs from
  advertisers to scanners — the virtual-radio equivalent of "airtime".

Every `Device` method that touches the controller takes `&mut LocalLink`
(an alias for `dyn HostTransport`) as its first argument. The concrete
`bumble_controller::LocalLink` implements `HostTransport`, as does the
external-controller bridge (`ExternalHost`) — so this exact code structure
also runs against real hardware
(see [Using a Real Controller](real-controller.md)).

## Events

Besides return values and accessors, devices record everything in a typed
event journal, with optional callbacks:

```rust
use bumble_host::DeviceEvent;

let listener_id = devices[0].add_event_listener(|event| {
    if let DeviceEvent::LeConnectionEstablished(info) = event {
        println!("connected: {info:?}");
    }
});

// ... drive the stack ...

// Or drain the same events as a journal, in emission order:
for event in devices[0].take_device_events() {
    println!("{event:?}");
}
devices[0].remove_event_listener(listener_id);
```
