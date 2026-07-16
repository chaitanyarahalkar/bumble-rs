# Architecture

bumble-rs ports the behavior of Python Bumble while replacing its `asyncio`
foundation with a synchronous, deterministic design.

## Synchronous core

The host, controller, protocols, and profiles are explicit state machines with
deterministic drive methods. Instead of awaiting futures, applications:

- **poll** devices and channels to make progress,
- **read events** from typed journals, listeners, and queues,
- **advance time** through deterministic timers.

This makes the stack easy to embed in synchronous programs, trivially
reproducible in tests, and free of any required async runtime. Network-facing
applications (the gRPC Pandora server, WebSocket transports) may use Tokio or
tonic at their outer boundary, but nothing async leaks into the core stack.

## Layering

Each layer is its own crate with a narrow interface:

1. **Core types** (`bumble`) — UUIDs, addresses, advertising data, keys.
2. **Wire codecs** (`bumble-hci`, `bumble-l2cap`, `bumble-att`, `bumble-smp`,
   `bumble-sdp`, `bumble-rfcomm`, …) — pure, transport-neutral packet
   models.
3. **Runtimes** (`bumble-gatt`, `bumble-controller`, channel managers,
   pairing sessions) — state machines that consume and produce typed packets.
4. **Host** (`bumble-host`) — the `Device` API that owns
   ATT-over-L2CAP-over-ACL sequencing, connections, pairing, and ISO streams.
5. **Profiles and media** (`bumble-profiles`, `bumble-a2dp`, `bumble-hfp`,
   …) — built on the layers below.
6. **I/O boundary** (`bumble-transport`, `bumble-drivers`,
   `bumble-pandora`) — the only crates that touch sockets, USB, serial
   ports, or gRPC.

See [Workspace Crates](crates.md) for the full map.

## Typed, open wire models

Known packets are typed Rust structures and enums; parsing validates lengths
and field shapes rather than relying on unchecked indexing. Unknown,
vendor-specific, or future values never fail closed: every packet enum carries
a `Generic` (or open numeric) variant, so round-tripping a packet the crate
doesn't recognize is lossless. This mirrors Bumble's philosophy of being a
protocol laboratory: you can always see, produce, and forward bytes the stack
doesn't yet model.

## Real and virtual controllers

The same high-level `Device` API works against two kinds of controller:

- **The in-process software controller** (`bumble-controller`): a
  deterministic controller implementation attached to a `LocalLink` virtual
  radio. Multiple devices on one link form a complete, reproducible Bluetooth
  network in a single process — this is how the workspace's integration tests
  run, and how you can test your own code without hardware.
- **External controllers** (`bumble-transport`): real controllers reached
  over USB, serial, TCP, UDP, Unix sockets, WebSockets, PTY, VHCI, raw HCI
  sockets, or Android emulator/netsim, using Bumble-style
  `<scheme>:<parameters>` spec strings.

Controller capabilities, packet pools, command credits, event masks, and
terminal transport state are applied consistently on both paths, so code
developed against the virtual controller behaves the same on real radios.

## Determinism and testing

Because every state machine is driven explicitly, a full multi-device exchange
(advertise → scan → connect → discover → pair → transfer) executes as a
plain, single-threaded function call sequence. The workspace's more than 1,000
tests exploit this: wire formats are pinned byte-for-byte against the Python
reference and specification vectors, and end-to-end flows run through the real
stack over virtual links. See
[Testing and Conformance](../reference/testing.md).
