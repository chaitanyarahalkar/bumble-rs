# Workspace Crates

The workspace is split into focused crates that can be used independently or
through the high-level host and transport layers. The dependency graph is
strictly layered:

```text
bumble (core types)
  └─ bumble-hci ──────────────┐
       ├─ bumble-drivers      │
       └─ bumble-controller   │
bumble-l2cap  bumble-crypto   │
  │             │             │
  ├─ bumble-att ┘             │
  │    └─ bumble-gatt ── bumble-profiles
  ├─ bumble-smp               │
  ├─ bumble-sdp  bumble-rfcomm  bumble-hid
  │                           │
  └────────── bumble-host ────┘
                 ├─ bumble-avdtp ─ bumble-a2dp (─ bumble-rtp)
                 ├─ bumble-avctp ─ bumble-avrcp (─ bumble-avc)
                 └─ bumble-transport ─ bumble-pandora
bumble-at ─ bumble-hfp        bumble-codecs   bumble-audio
```

## Core

### `bumble`

The shared core every higher layer depends on: `Uuid`, `Address`,
`Appearance`, `ClassOfDevice`, raw and typed advertising data, assigned
company IDs, `PairingKeys` and persistent key stores, and shared enums such as
`PhysicalTransport` and `LeRole`. Pure std — no async, I/O, or hardware.

### `bumble-hci`

The complete HCI packet model. `HciPacket::from_bytes` / `to_bytes` frame the
wire format; typed commands and events cover 197 command opcodes and 81
event/LE-meta codes, with a `Generic` fallback that keeps unknown and vendor
packets lossless. Includes typed Command Complete return parameters, ACL and
ISO reassembly, and Android/Zephyr vendor definitions.

### `bumble-crypto`

The SMP cryptographic toolbox (Core Spec Vol 3 Part H 2.2): block function
`e`, `aes_cmac` (RFC 4493), LE Legacy `c1`/`s1`/`ah`, LE Secure Connections
`f4`/`f5`/`f6`/`g2`/`h6`/`h7`, and P-256 ECDH via `EccKey`. Pinned to
specification and RFC test vectors.

## Controller and host

### `bumble-controller`

The deterministic in-process software controller plus `LocalLink`, an
in-process virtual radio. Covers legacy and extended advertising and scanning,
connections, privacy and resolving lists, periodic advertising, ACL routing,
ISO, Classic inquiry/connections, LMP, and SCO/eSCO — all driven
synchronously (`Controller::drain_host_events`,
`LocalLink::propagate_advertising`, …).

### `bumble-host`

The high-level `Device` API. A `Device` sits above a `HostTransport` (the
in-process `LocalLink` or an external transport adapter) and owns power/reset
lifecycle, LE and Classic connections, discovery, privacy, GATT server and
client wiring, pairing, ACL fragmentation/reassembly, CIG/CIS and BIG/BIS
isochronous streams, and transport-loss cleanup. `pump` drives a set of
devices to quiescence — the core pattern for deterministic tests.

## Protocols

### `bumble-l2cap`

L2CAP framing and channel machinery: `L2capPdu` with optional FCS, signaling
control frames, Classic basic mode and ERTM, LE credit-based and enhanced
credit-based channels, segmentation/reassembly, and a synchronous Classic
channel manager.

### `bumble-att` and `bumble-gatt`

`bumble-att` implements the complete ATT PDU catalog (`AttPdu`), including
signed writes. `bumble-gatt` builds on it with `GattServer` (attribute
database, discovery/read/write, CCCDs, notify/indicate) and `GattClient`
(service discovery, long reads, writes, subscriptions), plus EATT support.

### `bumble-smp`

The Security Manager Protocol: `SmpPdu` codec, Legacy and Secure Connections
pairing sessions, `PairingManager` for concurrent sessions (including BR/EDR
CTKD), key distribution, and privacy helpers (`AddressResolver`, RPA
generation/resolution).

### `bumble-sdp` and `bumble-rfcomm`

Classic service discovery (`DataElement`, `SdpPdu`, a client/server runtime
with continuation state) and TS 07.10 serial-port multiplexing
(`RfcommFrame`, multiplexer/DLC runtime with credit-based flow control). Both
bind to live Classic L2CAP channels.

### `bumble-at` and `bumble-hfp`

`bumble-at` parses the AT command grammar used by HFP. `bumble-hfp`
implements Hands-Free Profile service-level connections for both roles:
feature exchange, SLC initialization, indicators, codec negotiation, SDP
records, and SCO/eSCO parameters.

### `bumble-avdtp`, `bumble-a2dp`, and `bumble-rtp`

Bluetooth media: AVDTP signaling and stream transport, A2DP codec capability
models (SBC, AAC, Opus, vendor), and the RTP media packet codec used for A2DP
streams.

### `bumble-avc`, `bumble-avctp`, and `bumble-avrcp`

Audio/video remote control: AV/C command/response frames, the AVCTP transport
protocol, and AVRCP PDUs, events, and runtime.

### `bumble-hid`

Bluetooth Classic HID: HIDP messages plus synchronous host and device
runtimes over the control and interrupt L2CAP channels.

## Media

### `bumble-codecs` and `bumble-audio`

`bumble-codecs` provides media bitstreams: bit-level readers/writers,
AAC/LATM structures, G.722, and LC3. `bumble-audio` provides portable PCM
audio I/O — stream, file, WAVE, and subprocess inputs/outputs — with an
optional `sound-device` feature for real audio devices via
[`cpal`](https://crates.io/crates/cpal) (the only feature flag in the
workspace).

## Integration

### `bumble-transport` and `bumble-drivers`

`bumble-transport` opens external HCI transports from Bumble-style spec
strings (see [Transports](transports.md)), provides H4 framing, bridges,
BTSnoop capture, and ships the workspace's command-line applications.
`bumble-drivers` performs vendor controller initialization — Intel and
Realtek firmware loading — through a synchronous `Driver` interface.

### `bumble-profiles`

The GATT profile catalog, one module per profile: AICS, AMS, ANCS, ASCS,
ASHA, BAP, BASS, Battery Service, CAP, CSIP, Device Information Service, GAP,
GATT Service, GMAP, HAP, Heart Rate Service, LE Audio, MCP, PACS, PBP, TMAP,
VCS, and VOCS.

### `bumble-pandora`

Pandora conformance services over gRPC using the canonical
bt-test-interfaces v0.0.6 protobufs: Host, Security, SecurityStorage, and
L2CAP services, plus the `bumble-pandora-server` binary. See
[Testing and Conformance](../reference/testing.md).
