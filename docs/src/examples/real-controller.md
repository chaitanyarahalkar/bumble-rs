# Using a Real Controller

The `Device` API is controller-agnostic: everything shown against the virtual
link also runs against real hardware. The bridge is `ExternalHost` from
`bumble-transport`, which implements the same `HostTransport` interface as
the in-process link.

```rust
use std::time::Duration;

use bumble_host::Device;
use bumble_transport::{open_split_transport, ExternalHost, ExternalHostActivity};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open the controller from a transport spec — e.g. "usb:0",
    // "serial:/dev/ttyUSB0,1000000", "tcp-client:127.0.0.1:6402".
    let transport = open_split_transport("usb:0")?;
    let mut host = ExternalHost::new(transport);

    // Reset the controller, read its capabilities, and apply its
    // ACL/LE/ISO buffer pools to the device. This is the external
    // equivalent of power-on.
    let mut device = Device::new(0);
    let info = host.initialize_device(&mut device, COMMAND_TIMEOUT)?;
    println!("controller: {info:?}");

    // Scan for a few seconds.
    device.start_scanning(&mut host, true, false);
    loop {
        device.poll(&mut host);
        for report in device.take_advertising_reports() {
            println!("{} RSSI {}", report.address, report.rssi);
        }
        match host.wait_for_activity(Duration::from_secs(10))? {
            ExternalHostActivity::Packet => continue,
            ExternalHostActivity::Timeout => break,
            ExternalHostActivity::Ended => return Err("HCI transport ended".into()),
        }
    }
    device.stop_scanning(&mut host);
    Ok(())
}
```

## The drive loop

With a real controller there is no `pump` helper running the world to
quiescence — packets arrive when the radio delivers them. The pattern is:

1. `device.poll(&mut host)` — process any HCI events already received.
2. Act on state/journals (`take_advertising_reports`,
   `take_device_events`, …).
3. `host.wait_for_activity(timeout)` — block until the reader thread hands
   over another packet (`Packet`), the deadline passes (`Timeout`), or the
   transport ends (`Ended`).

`ExternalHost::wait_for_device_activity(&mut device, timeout)` combines
steps 1 and 3. Direct HCI commands (outside the `Device` state machines) can
be issued with `host.send_command(command, timeout)`.

## Transport loss

If the external transport ends or fails, the state is terminal:
`host.state()` reports `ExternalHostState::Ended` or `Failed(reason)`, and
the `Device` performs transport-loss cleanup (connections and pending
operations are torn down consistently, as on a controller reset).

## Lower-level access

For tools that don't need the full host stack, `bumble-transport` also
provides `HciCommandChannel`, a minimal synchronous command/response channel
over any transport — most of the simple CLI tools
([Apps and Tools](../guide/apps-and-tools.md)) are built on it.

Vendor controllers that need firmware loading before HCI comes up (Intel,
Realtek) are initialized by [`bumble-drivers`](../guide/drivers.md).
