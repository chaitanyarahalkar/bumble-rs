# Introduction

**bumble-rs** is a complete synchronous Rust implementation of
[Bumble](https://github.com/google/bumble), the dual-mode Bluetooth host stack
and software controller originally written in Python.

It provides typed Bluetooth protocols, LE and BR/EDR host behavior, a
deterministic software controller, external HCI transports, a full profile
catalog, media support, command-line applications, and Pandora conformance
services — all in one Rust workspace.

## Why bumble-rs

- **Synchronous, deterministic core.** The Python implementation is built on
  `asyncio`; bumble-rs exposes the same Bluetooth behavior through explicit
  polling, event journals, listeners, queues, and deterministic timers. No
  async runtime is required for the core stack.
- **Typed, open wire models.** Known packets use typed Rust structures and
  enums, while unknown, vendor-specific, or future values remain lossless
  through open numeric wrappers and generic payload variants.
- **Real and virtual controllers.** The same high-level `Device` API works
  with the deterministic in-process controller (ideal for tests) and with
  external controllers reached over USB, serial, TCP, and many other
  transports.
- **Verified against ground truth.** Wire formats are pinned to byte-exact
  outputs from the Python Bumble reference, Bluetooth specification vectors,
  and RFC test vectors — more than 1,000 tests across the workspace.

## What's included

| Area | Coverage |
|---|---|
| Core | UUIDs, addresses, advertising data, assigned numbers, device configuration, pairing keys, persistent key stores |
| HCI | Typed catalog of 197 command opcodes and 81 event/LE-meta codes; ACL, SCO, and ISO framing; vendor extensions |
| Controller | LE advertising, scanning, connections, privacy, periodic advertising, PAST, ISO, Classic inquiry/connections, LMP, SCO/eSCO |
| Host | Power/reset lifecycle, LE and Classic connections, discovery, privacy, GATT, pairing, Channel Sounding |
| L2CAP | Classic basic mode and ERTM, LE credit-based and enhanced credit-based channels |
| ATT/GATT | Complete ATT PDU catalog, database server, client operations, queued and signed writes, EATT |
| Security | SMP Legacy and Secure Connections pairing, all association models, CTKD, privacy/RPA resolution, JSON-backed bonds |
| Classic profiles | SDP, RFCOMM, HFP, AVDTP/A2DP, AV/C, AVCTP/AVRCP, HID |
| LE profiles | GAP, GATT, Battery, DIS, Heart Rate, ASHA, HAP, CSIP, VCS, VOCS, AICS, MCP, GMCS, BAP, PACS, ASCS, BASS, CAP, TMAP, GMAP, PBP, AMS, ANCS |
| Audio | RTP, SBC/AAC/Opus, G.722, LC3, PCM/WAVE I/O, SCO/eSCO, CIS, BIS |
| Conformance | Pandora Host, Security, SecurityStorage, and L2CAP gRPC services |

## Where to go next

- [Getting Started](guide/getting-started.md) — build the workspace and run
  your first tool.
- [Architecture](guide/architecture.md) — how the synchronous design works.
- [Examples](examples/overview.md) — small end-to-end programs.
- [API Documentation](reference/api.md) — full rustdoc for every crate.

## License

bumble-rs is licensed under the
[Apache License, Version 2.0](https://github.com/chaitanyarahalkar/bumble-rs/blob/main/LICENSE),
matching upstream Bumble.
