# Drivers

Some controllers need vendor-specific initialization — typically firmware
loading — before they behave as standard HCI controllers. The
`bumble-drivers` crate ports Bumble's driver framework, keeping driver
selection separate from transport discovery.

## How selection works

`get_driver_for_host` picks a driver for an opened transport:

1. If the transport spec carried explicit metadata —
   `usb:[driver=rtk]0` or `usb:[driver=intel]0` — that driver is forced.
2. Otherwise each driver probes the transport metadata (USB
   `vendor_id`/`product_id` published by the `usb:` transport) and the
   controller's reported version: Realtek first, then Intel.

Once selected, `Driver::init_controller` performs the vendor cold-start
sequence, returning a `DriverInitOutcome` describing what was loaded. The
driver operates through two small traits:

- **`DriverHost`** — synchronous HCI command/vendor-event operations against
  the opened transport.
- **`FirmwareProvider`** — resolves firmware blobs by name.

## Realtek (`rtk`)

Supports the RTL8723/8761/8821/8822/8852 USB families. The driver resets the
controller, reads the local version to identify the chip, then downloads the
matching firmware image (and, where required, a config blob) in 252-byte
fragments using the vendor download command.

Firmware files are searched in order: the directory named by the
`BUMBLE_RTK_FIRMWARE_DIR` environment variable (exclusively, if set),
the project and package directories, the system directory
(`/lib/firmware/rtl_bt` on Linux), and the current directory.

## Intel (`intel`)

Supports Intel AX210/AX211/BE200 controllers. If the controller is already
in operational mode, only the Device Data Configuration (`.ddc`) is applied.
Otherwise the driver secure-sends the `ibt-*` firmware image in fragments,
sets the boot parameters, and then loads the DDC.

Firmware search mirrors Realtek's, with `BUMBLE_INTEL_FIRMWARE_DIR` and
`/lib/firmware/intel`.

## Usage

```rust
use bumble_drivers::get_driver_for_host;

// host: an implementation of DriverHost over the opened transport
// firmware: a FirmwareProvider (e.g. the built-in filesystem search)
if let Some(driver) = get_driver_for_host(&mut host, &firmware)? {
    let outcome = driver.init_controller(&mut host, &firmware)?;
    println!("driver initialized: {outcome:?}");
}
// Proceed with normal HCI bring-up.
```

See the
[bumble_drivers API docs](https://chaitanyarahalkar.github.io/bumble-rs/api/bumble_drivers/)
for the trait definitions.
