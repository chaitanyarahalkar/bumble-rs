# Platform Notes

The core stack is pure Rust and builds on Linux, macOS, and Windows. Access to
real controllers differs per platform.

## Linux

- **USB controllers** work through the `usb:` transport. Detach the kernel
  driver or make sure `bluetoothd` is not claiming the adapter (for example
  `systemctl stop bluetooth`), and ensure your user can access the USB device
  (udev rules or root).
- **`hci-socket:`** opens a raw HCI user channel. This requires
  `CAP_NET_ADMIN` (typically root) and an adapter that is down
  (`hciconfig hci0 down`).
- **`vhci:`** attaches a virtual controller to the kernel through
  `/dev/vhci`, letting BlueZ talk to a bumble-rs software controller.
  Requires the `hci_vhci` kernel module and permission on `/dev/vhci`.
- Serial controllers work through `serial:` with configurable baud rate and
  flow control.

## macOS

- USB Bluetooth controllers are typically claimed by the system Bluetooth
  stack. Use an **external** (secondary) USB controller for the `usb:`
  transport.
- Serial, TCP, UDP, Unix socket, WebSocket, and PTY transports all work
  without special setup.
- The optional `sound-device` feature (audio playback/capture for A2DP and LE
  Audio tools) uses CoreAudio and needs no extra libraries.

## Windows

- The core stack, virtual controllers, and network transports (TCP, UDP,
  WebSocket) build and run.
- For USB controllers, a WinUSB-compatible driver must be bound to the
  adapter (for example with Zadig), since the inbox Bluetooth driver claims
  the radio.
- Unix socket, PTY, `hci-socket:`, and `vhci:` transports are
  platform-specific and not available on Windows.

## Android

Two transports integrate with Android tooling without touching a physical
radio:

- **`android-emulator:`** connects to the Bluetooth chip of a running Android
  emulator (root canal) over gRPC.
- **`android-netsim:`** connects to the Android `netsim` network simulator.

These are useful for driving Android Bluetooth stacks from host-side tests.

## Audio features

Audio device I/O is optional and gated behind the `sound-device` feature of
`bumble-audio` (via [`cpal`](https://crates.io/crates/cpal)):

- Linux: requires ALSA development headers (`libasound2-dev` on Debian/Ubuntu).
- macOS: CoreAudio, no extra packages.
- Windows: WASAPI, no extra packages.

Everything else (SBC/AAC/Opus capability parsing, G.722, LC3 workers,
PCM/WAVE file I/O) is always available.
