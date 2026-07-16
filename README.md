# bumble-rs

[![CI](https://github.com/chaitanyarahalkar/bumble-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/chaitanyarahalkar/bumble-rs/actions/workflows/ci.yml)
[![Docs](https://github.com/chaitanyarahalkar/bumble-rs/actions/workflows/docs.yml/badge.svg)](https://chaitanyarahalkar.github.io/bumble-rs/)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
![MSRV](https://img.shields.io/badge/MSRV-1.87-blue)

**[Documentation](https://chaitanyarahalkar.github.io/bumble-rs/)** ·
**[API reference](https://chaitanyarahalkar.github.io/bumble-rs/api/)**

A complete synchronous Rust implementation of
[`google/bumble`](https://github.com/google/bumble), the dual-mode Bluetooth
host stack and software controller.

`bumble-rs` provides typed Bluetooth protocols, LE and BR/EDR host behavior,
a deterministic software controller, external HCI transports, profiles, media
support, command-line applications, and Pandora conformance services in one
Rust workspace.

The Python implementation uses `asyncio`; the Rust API exposes the same
Bluetooth behavior through explicit polling, event journals, listeners, queues,
and deterministic timers. It does not require an async runtime for the core
stack.

## Highlights

- Dual-mode LE and Classic host stack with high-level device APIs.
- Complete typed HCI command, event, ACL, SCO, and ISO packet model.
- Deterministic in-process controller and virtual radio for integration tests.
- ATT, GATT, EATT, L2CAP, SMP, SDP, RFCOMM, HFP, A2DP, AVRCP, HID, and LE Audio.
- Legacy and Secure Connections pairing, privacy, CTKD, key storage, and signed
  ATT writes.
- Connected and broadcast isochronous audio with CIG/CIS and BIG/BIS support.
- USB, serial, TCP, UDP, Unix socket, WebSocket, PTY, VHCI, raw HCI socket,
  Android emulator, and Android netsim transports.
- Intel and Realtek controller initialization and firmware-loading support.
- All 23 Bumble profile modules, runnable tools, and Pandora gRPC services.
- Oracle-pinned wire tests and end-to-end tests across the complete workspace.

## Bluetooth coverage

| Area | Included |
|---|---|
| Core | UUIDs, addresses, advertising data, assigned numbers, device configuration, pairing keys, and persistent key stores |
| HCI | Typed catalog of 197 command opcodes and 81 event/LE-meta codes; Command Complete return parameters; ACL, SCO, and ISO framing; vendor extensions |
| Controller | LE advertising, scanning, connections, privacy, periodic advertising, PAST, ACL flow control, ISO, Classic inquiry/connections, LMP, SCO/eSCO, and deterministic link scheduling |
| Host and device | Power/reset lifecycle, capability discovery, LE and Classic connections, discovery, privacy, GATT, pairing, data queues, connection control, Channel Sounding, and transport-loss cleanup |
| L2CAP | Classic basic mode and ERTM, LE credit-based channels, enhanced credit channels, signaling, segmentation, reassembly, flow control, and reconfiguration |
| ATT and GATT | Complete ATT PDU catalog, database server, client discovery/read/write/subscribe operations, queued writes, signed writes, CCCDs, MTU negotiation, and EATT |
| Security | SMP Legacy and Secure Connections pairing, every association model, encryption, key distribution, privacy/RPA resolution, CTKD, CSRK signing, and JSON-backed bonds |
| Classic profiles | SDP, RFCOMM, HFP, AVDTP/A2DP, AV/C, AVCTP/AVRCP, and HID |
| LE profiles | GAP, GATT, Battery, Device Information, Heart Rate, ASHA, HAP, CSIP, VCS, VOCS, AICS, MCP, GMCS, BAP, PACS, ASCS, BASS, CAP, TMAP, GMAP, PBP, AMS, and ANCS |
| Audio and media | RTP, SBC/AAC/Opus capability and packet support, G.722, LC3 workers, PCM/WAVE I/O, optional CPAL devices, SCO/eSCO, CIS, and BIS |
| Conformance | Pandora Host, Security, SecurityStorage, and L2CAP services using the canonical bt-test-interfaces v0.0.6 protobufs |

## Workspace

The workspace is split into focused crates that can be used independently or
through the high-level host and transport layers.

| Crate | Purpose |
|---|---|
| [`bumble`](bumble/) | Core Bluetooth types, advertising data, company IDs, utilities, and key stores |
| [`bumble-hci`](bumble-hci/) | HCI codecs, typed commands/events, return parameters, fragmentation, and vendor packets |
| [`bumble-controller`](bumble-controller/) | Software controller, virtual link, Link Layer control, and Classic LMP |
| [`bumble-host`](bumble-host/) | High-level host/device lifecycle, connection ownership, discovery, GATT, pairing, and ISO |
| [`bumble-l2cap`](bumble-l2cap/) | Classic, ERTM, LE credit-based, and enhanced credit-based channels |
| [`bumble-att`](bumble-att/) and [`bumble-gatt`](bumble-gatt/) | ATT wire protocol plus GATT client/server runtimes |
| [`bumble-crypto`](bumble-crypto/) and [`bumble-smp`](bumble-smp/) | Bluetooth security primitives, P-256, pairing, privacy, signing, and key distribution |
| [`bumble-sdp`](bumble-sdp/) and [`bumble-rfcomm`](bumble-rfcomm/) | Classic service discovery and serial-port multiplexing |
| [`bumble-at`](bumble-at/) and [`bumble-hfp`](bumble-hfp/) | AT parsing and Hands-Free Profile control/audio |
| [`bumble-avdtp`](bumble-avdtp/), [`bumble-a2dp`](bumble-a2dp/), and [`bumble-rtp`](bumble-rtp/) | Bluetooth media negotiation, codecs, streams, and RTP |
| [`bumble-avc`](bumble-avc/), [`bumble-avctp`](bumble-avctp/), and [`bumble-avrcp`](bumble-avrcp/) | Audio/video control and remote-control protocols |
| [`bumble-hid`](bumble-hid/) | HIDP messages and Classic control/interrupt channels |
| [`bumble-codecs`](bumble-codecs/) and [`bumble-audio`](bumble-audio/) | Media bitstreams, G.722, LC3, PCM, WAVE, subprocess, and optional sound-device I/O |
| [`bumble-transport`](bumble-transport/) and [`bumble-drivers`](bumble-drivers/) | External HCI transports, bridges, capture formats, controller tools, and vendor initialization |
| [`bumble-profiles`](bumble-profiles/) | Typed services, clients, proxies, and state machines for the Bluetooth profile catalog |
| [`bumble-pandora`](bumble-pandora/) | Pandora protobufs, conversions, runtime, and gRPC server |

## Getting started

### Requirements

- Rust 1.87 or newer.
- A C toolchain for native dependencies.
- Appropriate system permissions when accessing USB controllers, raw HCI
  sockets, or Linux VHCI.
- Platform audio development libraries only when enabling the optional
  `sound-device` feature.

Clone and build the complete workspace:

```bash
git clone https://github.com/chaitanyarahalkar/bumble-rs.git
cd bumble-rs
cargo build --workspace --all-targets
```

## Documentation

The user guide and full rustdoc API reference are published at
[chaitanyarahalkar.github.io/bumble-rs](https://chaitanyarahalkar.github.io/bumble-rs/).
The guide sources live in [`docs/`](docs/) and are built with
[mdBook](https://rust-lang.github.io/mdBook/):

```bash
mdbook serve docs --open
```

Generate local API documentation:

```bash
cargo doc --workspace --all-features --no-deps --open
```

## External HCI transports

Applications use Bumble-style `<scheme>:<parameters>` transport
specifications. Supported schemes are:

- `file`
- `serial`
- `pty`
- `tcp-client` and `tcp-server`
- `udp`
- `unix`, `unix-client`, and `unix-server`
- `ws-client` and `ws-server`
- `usb` and `pyusb`
- `hci-socket`
- `vhci`
- `android-emulator`
- `android-netsim`

Serial transports support custom baud rates, RTS/CTS, DSR/DTR, and optional
post-open delay. USB supports controller discovery, forced interface selection,
ACL, SCO/eSCO, and ISO-compatible transfers.

Examples:

```text
serial:/dev/tty.usbmodem0001,1000000,rtscts
tcp-client:127.0.0.1:6402
ws-client:ws://127.0.0.1:8080
```

## Command-line applications

The `bumble-transport` crate includes runnable applications for:

- Scanning, pairing, unbonding, controller inspection, device inspection, GATT
  discovery, USB probing, and an interactive console.
- HCI, L2CAP, RFCOMM, and Golden Gate bridges.
- Two-controller virtual-radio operation and local-controller loopback.
- A2DP playback/speaker operation, Auracast, and LE Audio unicast.
- Multi-transport benchmarks and HCI/BTSnoop capture decoding.

Use `--help` to inspect an application's complete interface:

```bash
cargo run -p bumble-transport --bin bumble-scan -- --help
cargo run -p bumble-transport --bin bumble-pair -- --help
cargo run -p bumble-transport --bin bumble-controller-info -- --help
cargo run -p bumble-transport --bin bumble-bench -- --help
```

The security and conformance tools are available from their own crates:

```bash
cargo run -p bumble-smp --bin bumble-rpa-tool -- --help
cargo run -p bumble-pandora --bin bumble-pandora-server -- --help
```

## Verification

Wire formats are pinned against upstream Bumble serialization, Bluetooth
specification vectors, RFC vectors, and captured protocol fixtures. Integration
tests exercise the complete stack through virtual controllers, real loopback
sockets, gRPC services, external transport boundaries, GATT, pairing, Classic
profiles, and isochronous audio.

Run the same workspace gates used by CI:

```bash
cargo test --workspace --all-targets
cargo test --release --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
```

The repository contains more than 1,000 debug- and release-verified tests across
unit, integration, transport, application, and conformance targets.

## Design

### Synchronous core

The host, controller, protocols, and profiles use explicit state machines and
deterministic drive methods. Events are exposed through typed journals,
listeners, and queues, making behavior easy to embed in synchronous programs and
reproducible tests. Network-facing applications may use Tokio or tonic at their
outer boundary without imposing an async runtime on the core stack.

### Typed, open wire models

Known packets use typed Rust structures and enums. Unknown, vendor-specific, or
future values remain lossless through open numeric wrappers and generic payload
variants. Parsers validate lengths and field shapes rather than relying on
unchecked indexing.

### Real and virtual controllers

The same high-level `Device` API works with the deterministic in-process
controller and with external controllers opened through
`bumble-transport`. Controller capabilities, packet pools, command credits,
event masks, and terminal transport state are applied consistently on both
paths.

## Project structure

```text
bumble-rs/
├── bumble/                 # core types and key storage
├── bumble-hci/             # HCI packet model
├── bumble-controller/      # software controller and virtual link
├── bumble-host/            # high-level host and Device API
├── bumble-{l2cap,att,gatt}/
├── bumble-{crypto,smp}/
├── bumble-{sdp,rfcomm,at,hfp}/
├── bumble-{avdtp,a2dp,rtp,avc,avctp,avrcp,hid}/
├── bumble-{codecs,audio}/
├── bumble-{transport,drivers}/
├── bumble-profiles/
├── bumble-pandora/
└── Cargo.toml
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the build and test requirements and
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) for community expectations. Report
security issues according to [SECURITY.md](SECURITY.md).

## License

Licensed under the [Apache License, Version 2.0](LICENSE), matching upstream
Bumble. See [NOTICE](NOTICE) for attribution.
