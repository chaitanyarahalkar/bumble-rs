# bumble-rs

[![CI](https://github.com/block/bumble-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/block/bumble-rs/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
![MSRV](https://img.shields.io/badge/MSRV-1.87-blue)

An incremental Rust port of [`google/bumble`](https://github.com/google/bumble),
the Python Bluetooth stack.

Bumble is a ~70,000-line dual-mode Bluetooth host stack plus a software
controller. A full port is a large, multi-slice effort. This repository ports
it **one vertical slice at a time**, each slice a compiling, fully-tested Rust
crate whose behavior is verified against the upstream Python.

## Status

| Slice | Crate | Status |
|-------|-------|--------|
| 1. Core types & advertising data | `bumble` | ✅ complete — 16/16 tests green |
| 2. HCI packet codec (framing + **full** command/event catalog + return params) | `bumble-hci` | ✅ 320/320 tests green |
| 3+7. Software controller + virtual link (advertising + LE connections + read/PHY/data-length commands) | `bumble-controller` | ✅ 17/17 tests green |
| 4. L2CAP frame codec (PDU + signaling frames + FCS) | `bumble-l2cap` | ✅ 8/8 tests green |
| 5. ATT protocol PDU codec (incl. Find_Information, Read_Blob, indications) | `bumble-att` | ✅ 16/16 tests green |
| 6. SMP cryptographic toolbox | `bumble-crypto` | ✅ 10/10 vectors green |
| 7. LE connection establishment (in the controller) | `bumble-controller` | ✅ (see slice 3+7) |
| 8. ACL data path (ATT-over-L2CAP-over-ACL, cross-layer) | `bumble-controller` | ✅ 8/8 controller tests |
| 9. Minimal GATT/ATT server (end-to-end attribute read/write) | `bumble-gatt` | ✅ 5/5 tests green |
| 10. Host/Device glue (ATT↔L2CAP↔ACL sequencing as a library API) | `bumble-host` | ✅ 3/3 tests green |
| 11. GATT server model + primary discovery (service/characteristic) | `bumble-gatt` | ✅ 7/7 tests green |
| 12. GATT notifications (server → client) | `bumble-host` | ✅ |
| 13. LE disconnection (Disconnect → Disconnection Complete both sides) | `bumble-controller` | ✅ |
| 14. SMP PDU codec + LE Legacy pairing (wires in `bumble-crypto`) | `bumble-smp` | ✅ 2/2 tests green |
| 16. SDP codec (data elements + PDUs) — first Classic (BR/EDR) piece | `bumble-sdp` | ✅ 23/23 tests green |
| 17. RFCOMM frame + MCC codec (serial-cable emulation over L2CAP) | `bumble-rfcomm` | ✅ 14/14 tests green |
| 18. GATT client (discovery, read/long-read, write, subscribe) | `bumble-gatt` | ✅ client tests green |
| 19+. LE Secure Connections pairing, more classic (A2DP/AVRCP/HFP…), async runtimes | — | planned |

The LE lifecycle is now complete end-to-end through library APIs: **connect →
discover → read/write → notify → disconnect** between two virtual devices — and
**every crate is integrated**, with `bumble-crypto` now driving SMP pairing.

The HCI codec is now a **complete typed catalog**: all 196 command op codes and
81 event / LE-meta sub-event codes, **generated** from upstream `bumble.hci`'s
declarative field specs by [`tools/hcigen`](bumble-hci/tools/hcigen/). The
generator introspects each command/event class, normalizes its fields to a
small codec vocabulary (`u8`/`u16`/`u24`/`u32`/`i8`/`bytes:N`/`addr`/
`codingformat`/`rest`/`varbytes`/`array`), and captures ground-truth wire bytes
— using **distinct, position-revealing values** — via upstream's own serializer.
Before emitting a line of Rust it re-derives those bytes and asserts they match
the captured oracle, so the codec model is proven against real Python Bumble at
generation time; the 320 emitted tests re-verify it at `cargo test` time, and
every packet round-trips byte-exact and re-parses to the same variant. Four
classes are hand-written (two phys-derived array commands whose count comes from
a PHY bitmask, and the two advertising-report events with nested report objects
— none derivable from a flat field spec); `Command_Complete` carries a typed
`ReturnParameters` model. Unmodeled/vendor op codes still fall through to the
open-enum `Generic` tail losslessly. Of `hci_test.py`'s ~46 hand tests, the 4
not mirrored are the vendor-event factory and three registry-iterating
parametrized tests — neither has an analog in an enum-based port.

## Porting status vs. `google/bumble`

A module-by-module tracker of the upstream Python (`bumble/`) against this port.
The [Status](#status) table above tracks the *slices* built so far; this table
tracks *coverage of the source*.

**Legend:** ✅ ported (complete for this project's scope) · 🟡 partial (a
representative subset — more of the module remains) · ⬜ not started.

Because the port targets the **LE core**, most touched modules are partial by
design; the notes say what's covered vs. deferred. LOC is the upstream module
size, to convey remaining surface.

### Core & utilities — ✅ done
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `core.py` (2.1k), `data_types.py` (1.0k) | `bumble` | ✅ | Core types (`Uuid`, `Address`, `Appearance`, `ClassOfDevice`, `AdvertisingData`), the full typed `DataType` AD hierarchy (~40 types, oracle-pinned), well-known 16-bit UUID names, and `PhysicalTransport`/`LeRole`. |
| `company_ids.py` (3.3k) | `bumble::company_ids` | ✅ | 3,327-entry SIG company table + `company_name()` binary-search lookup. |
| `keys.py` (0.4k) | `bumble::keys` | ✅ | `PairingKeys` / `Key` data structures. Persistent key stores (JSON/async I/O) deferred. |
| `utils.py` (0.5k) | `bumble::util` (+ spread) | ✅ | Generic helpers (`bit_flags_to_strings`, `name_or_number`); `crc_16` lives in `bumble-l2cap`; the open-enum/flag pattern is realized as newtypes throughout. The asyncio event infra (`EventEmitter`/`AsyncRunner`/`FlowControlAsyncPipe`) is **N/A** for this synchronous port. |
| `colors`, `logging`, `helpers`, `snoop`, `decoder` | — | N/A | Debug/logging tooling with idiomatic Rust equivalents rather than library surface: `colors` (ANSI), `logging` (→ `log`/`tracing`), `helpers.PacketTracer` (debug trace), `snoop` (BTSnoop/pcap capture). `decoder.py` is a **G.722 audio codec** — it belongs with the audio subsystem, not core. |

### HCI, controller & link — 🟡 HCI codec complete (full catalog, oracle-pinned); controller/link behavior partial
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `hci.py` (8.3k) | `bumble-hci` | ✅ | **Full typed catalog: 196 command op codes + 81 event / LE-meta sub-event codes**, generated from upstream's declarative field specs by [`tools/hcigen`](bumble-hci/tools/hcigen/) and **byte-pinned against real Python Bumble** (320 oracle tests). Framing (Command/Event/ACL/SCO/ISO), `Command_Complete` with a typed `ReturnParameters` model, and the open-enum `Generic` tail for any future/vendor opcode (still lossless). Two phys-derived array commands and the two nested-report events are hand-written; everything else is generated. |
| `controller.py` (2.8k) | `bumble-controller` | 🟡 | **Full command surface**: every command upstream's `controller.py` handles (93, via the generated [`command_surface`](bumble-controller/src/command_surface.rs) table) gets a reply of the matching HCI shape — Command Complete + SUCCESS for config/set commands, Command Status for operations completing via a later event, and the spec-correct "Unknown HCI Command" for anything upstream also doesn't handle. **Functionally simulated**: LE advertising/scanning, connection establishment, ACL routing, disconnection, the read commands (`Read_BD_ADDR`/`Read_Local_Name`/`LE_Read_Buffer_Size`/`LE_Read_Local_Supported_Features`/`LE_Rand`), per-connection `LE_Set_Data_Length`/`LE_Set_PHY` (with follow-up meta events), and — via LL control-PDU exchange over the link — **encryption start** (`LE_Enable_Encryption` → `Encryption Change` on both sides), **remote-features** (`FeatureReq`/`FeatureRsp` → `LE_Read_Remote_Features_Complete`), and **CIS establishment** (LE Audio: `LE_Set_CIG_Parameters`/`LE_Create_CIS` → `LE CIS Request` → `LE_Accept_CIS_Request` → `LE CIS Established` on both sides). Also **classic (BR/EDR)**: ACL connection establishment (`Create_Connection` → `Connection Request` → `Accept_Connection_Request` → `Connection Complete`), `Remote_Name_Request`, and `Read_Remote_Supported_Features`, via simplified LMP PDUs over the link. Other read commands are acknowledged SUCCESS **without a synthesized payload** (a documented stub, not a full read). A **behavioral simulation with placeholder values** (as upstream's `controller.py` also is) — *not* oracle-pinned like the HCI codec. Deferred (behavior, not codec): LTK verification, ISO data-path streaming, remote-version exchange, extended/periodic advertising, and classic auth/encryption/role-switch/SCO sub-flows. |
| `link.py` (0.15k) | `bumble-controller` | 🟡 | In-process **synchronous** `LocalLink`. Deferred: LL control PDUs, LMP routing, async scheduling. |
| `ll.py` (0.2k) | `bumble-controller` | 🟡 | Advertising/connection PDUs modeled as in-process structs, not serialized LL PDUs. Control PDUs (`EncReq`, `FeatureReq`/`PeripheralFeatureReq`/`FeatureRsp`, `TerminateInd`) are exchanged between controllers via `LocalLink::pump_ll` to drive the encryption-start, remote-features, and CIS-establishment (`CisReq`/`CisRsp`/`CisInd`) flows. |
| `host.py` (2.1k) | `bumble-host` | 🟡 | `Device` glue (ATT↔L2CAP↔ACL sequencing + pairing transport). Not the full host feature set. |
| `device.py` (7.0k) | `bumble-host` | 🟡 | Minimal `Device`/`pump`; the high-level device API (advertising/scanning/connection orchestration, GATT client, listeners) is not ported. |
| `lmp.py` (0.4k) | `bumble-controller::lmp` | 🟡 | Classic Link Manager Protocol PDUs modeled as in-process structs (`HostConnectionReq`/`Accepted`, `NameReq`/`NameRes`, `FeaturesReq`/`FeaturesRes`, `Detach`) driving the classic connection/name/features flows via `LocalLink::pump_classic`. The role-switch / authentication / encryption LMP sub-dance is simplified away. |

### L2CAP
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `l2cap.py` (3.1k) | `bumble-l2cap` | 🟡 | PDU + signaling frames + FCS. Deferred: channel manager, fragmentation/reassembly, credit-based flow-control runtime. |

### ATT / GATT
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `att.py` (1.1k) | `bumble-att` | 🟡 | PDUs incl. discovery (Read_By_Type/Group_Type, Find_Information, Find_By_Type_Value), reads (Read/Read_Blob), writes (Request/Command), and notifications/indications + confirmation — all oracle-pinned. Deferred: prepared/queued (Prepare/Execute), Read_Multiple, and signed writes. |
| `gatt.py` (0.6k), `gatt_server.py` (1.2k) | `bumble-gatt` | 🟡 | Attribute DB, service/characteristic model, primary discovery, read/write/notify, plus Find_Information/Find_By_Type_Value discovery, a CCCD descriptor per notify/indicate characteristic, MTU-sized reads with Read_Blob, and server-initiated notify/indicate. Deferred: included services, prepared writes. |
| `gatt_client.py` (1.2k), `gatt_adapters.py` (0.4k) | `bumble-gatt` | 🟡 | **`GattClient` (slice 18)**: service / characteristic / descriptor discovery, reads (with long-read via Read_Blob), writes (with and without response), and notify/indicate subscriptions (CCCD write + notification/indication handling), over an `AttTransport`. Verified by a two-party client↔server integration test. Deferred (matching the synchronous port): the async bearer, `gatt_adapters` typed-value proxies, and event listeners. |

### Security (SMP + crypto)
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `crypto/` | `bumble-crypto` | ✅ | All SMP **symmetric** security functions — `e`, AES-CMAC, `c1`, `s1`, `f4`/`f5`/`f6`, `g2`, `h6`/`h7`, `ah` — spec/RFC-4493 vector-verified. Deferred: ECC P-256 (`EccKey`) and RNG. |
| `smp.py` (2.0k), `pairing.py` (0.3k) | `bumble-smp` | 🟡 | PDU codec + LE Legacy (JustWorks) pairing run over the link. Deferred: full pairing state machine, LE Secure Connections, key distribution, MITM/OOB, passkey. |

### Transports & drivers
| Upstream | Rust crate | Status | Notes |
|---|---|---|---|
| `transport/*` — USB, UART/serial, TCP, WebSocket, UDP, PTY, android-netsim, vhci, … | — | ⬜ | The link is in-process only; no real transports (so no talking to real hardware or netsim yet). |
| `drivers/*` — Intel, Realtek | — | ⬜ | Vendor controller firmware/init. |

### Classic Bluetooth (BR/EDR)
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `rfcomm.py` (1.2k) | `bumble-rfcomm` | 🟡 | **Frame codec complete, oracle-pinned**: the `RfcommFrame` TS 07.10 framing (SABM/UA/DM/DISC/UIH, 1- and 2-byte length indicators, credit-based UIH flow control), the CRC-8 `compute_fcs`, and the `RfcommMccPn`/`RfcommMccMsc` MCC messages plus the MCC type/length header. Deferred (matching the port's synchronous, codec-first approach): the asyncio `DLC`, `Multiplexer`, `Client`/`Server` credit-flow state machine and the SDP-record helpers. |
| `sdp.py` (1.4k) | `bumble-sdp` | 🟡 | **Codec complete, oracle-pinned**: the recursive `DataElement` type-length-value format (all nine element types, all eight size-index encodings, 16/32/128-bit UUIDs), the `ServiceAttribute` service-record model, and all seven `SdpPdu` messages (Error / Service Search / Service Attribute / Service Search Attribute request+response) with continuation state carried verbatim. Deferred (matching the port's synchronous, codec-first approach): the asyncio `Client`/`Server`, the continuation-state reassembly loop, and the service-record database. |
| `hfp.py` (2.1k), `at.py` (0.1k) | — | ⬜ | Hands-Free Profile. |
| `hid.py` (0.6k) | — | ⬜ | Human Interface Device. |
| `a2dp` (1.0k), `avdtp` (2.4k), `avrcp` (2.9k), `avc` (0.5k), `avctp` (0.3k), `rtp` (0.1k), `codecs` (0.5k) | — | ⬜ | A/V distribution + remote control + audio. |

### Profiles & apps
| Upstream | Rust crate | Status | Notes |
|---|---|---|---|
| `profiles/*` — GAP, Battery, Device Info, Heart Rate, ASHA, LE Audio (BAP/PACS/ASCS/…), HAP, CSIP, … (24 modules) | — | ⬜ | None implemented. The GATT layer can express them, but no profile is built on it. |
| `bridge.py`, `pandora/`, apps | — | ⬜ | Test harnesses / apps — out of scope. |

### Roughly where that leaves things

Fully or substantially covered for the **LE core data + security path**: core
types, HCI framing, L2CAP/ATT/GATT/SMP codecs, the SMP crypto toolbox, both
sides of GATT (server **and** a client that discovers, reads, writes, and
subscribes), and a controller/link/host that runs the LE lifecycle end-to-end.
Classic Bluetooth has its **first two foundation pieces** — the SDP codec
(`bumble-sdp`), which the classic profiles build service records on, and the
RFCOMM frame codec (`bumble-rfcomm`), the serial-cable transport those profiles
run over. Everything else — the full high-level device/host orchestration, LE
Secure Connections, real transports, and the **rest of Classic Bluetooth
(A2DP/AVRCP/HFP/HID/…) and the profiles** — is still the large majority of the
~82k upstream lines and remains to do.

## Slice 1 — what's here

The shared primitives every higher Bluetooth layer depends on, ported to
idiomatic Rust in the [`bumble`](bumble/) crate (std-only, no dependencies):

- **`Uuid`** — 16/32/128-bit UUIDs, little-endian storage, big-endian strings,
  128-bit-expansion equality & hashing.
- **`Address` / `AddressType`** — little-endian device addresses, string parsing
  (`"C4:F2:17:1A:1D:BB"`, `/P` suffix), and the resolvable/static/identity
  predicates.
- **`Appearance`** — GAP appearance encode/decode with open-enum semantics.
- **`ClassOfDevice`** — Class of Device packing and string rendering.
- **`AdvertisingData`** — raw TLV codec (`append`/`get`/`get_all`/`to_bytes`).

### Design notes

- **Open enums.** `AddressType`, appearance `Category`/subcategory,
  `AdvertisingData::Type`, and the Class-of-Device fields are newtypes over
  integers, so values outside the named set round-trip unchanged — matching
  Bumble's `OpenIntEnum`/`CompatibleIntFlag`.
- **Byte- and string-exact.** Encodings and formatted strings match Bumble
  exactly; verified by a differential check against the Python implementation.
- **Deferred** (no upstream test exercises them): the `company_ids` table, the
  typed `data_types` value hierarchy, and crypto-based address generation.

## Slice 2 — what's here

The HCI packet codec in the [`bumble-hci`](bumble-hci/) crate (depends on
`bumble` for `Address`):

- **`HciPacket`** — top-level dispatch on the packet type byte.
- **`Command`** — 22 typed commands (Reset, Disconnect, PIN_Code_Request_Reply,
  Set/LE_Set_Event_Mask, LE_Set_Random_Address, LE advertising/scan/connection
  commands including the per-PHY array forms Extended_Create_Connection /
  Set_Extended_Scan_Parameters / Set_Extended_Advertising_Enable,
  LE_Setup_ISO_Data_Path, and the Read_Local_* commands), plus a `Generic`
  fallback.
- **`Event` / `LeMetaEvent`** — Command_Complete, Command_Status,
  Number_Of_Completed_Packets, the LE Connection_Complete /
  Connection_Update_Complete / Channel_Selection_Algorithm /
  Read_Remote_Features_Complete meta events, and both LE Advertising Report
  events (nested per-report structs), plus `Generic` fallbacks.
- **`ReturnParameters`** — typed Command_Complete return parameters
  (LE_Read_Buffer_Size, Read_BD_ADDR, Read_Local_Name,
  Read_Local_Supported_Codecs + V2) with the status-based short-response
  fallback, plus a `Raw` fallback.
- **Data packets** — ACL, Synchronous (SCO), ISO (with the timestamp / SDU-info
  blocks), and the custom passthrough packet.

### Design notes

- **Enum dispatch with a `Generic` fallback.** Each typed variant decodes its
  fields; unrecognized op/event codes round-trip as raw bytes.
- **Oracle-verified.** Every acceptance test asserts the serialized bytes
  against a ground-truth hex literal captured from real Python Bumble
  (`bytes(x).hex()`). This is the load-bearing correctness check — a pure
  round-trip would pass on a symmetric-but-wrong layout (and in fact the oracle
  caught exactly such a bug in `Number_Of_Completed_Packets`).

## Slice 3 — what's here

A minimal software controller and an in-process link in the
[`bumble-controller`](bumble-controller/) crate — the first slice where two
virtual devices actually talk:

- **`Controller`** — LE state driven by HCI commands (`Reset`,
  `LE_Set_Random_Address`, `LE_Set_Advertising_Data`, `LE_Set_Advertising_Enable`,
  `LE_Set_Scan_Enable`), producing Command Complete acks and, when scanning,
  LE Advertising Report events.
- **`LocalLink`** — an in-process bus that broadcasts an advertiser's PDU to
  scanning controllers, and (slice 7) establishes LE connections: an initiating
  central (`LE_Create_Connection`) plus a connectable advertiser produce an
  `LE_Connection_Complete` on both hosts (central role / peripheral role, each
  seeing the other's address), and the advertiser stops.

### Design notes

- **Synchronous link.** Bumble's `LocalLink` schedules delivery on an asyncio
  loop; this slice models it synchronously (`propagate_advertising` delivers
  PDUs when called, and host events are drained from a queue) — deterministic
  and dependency-free, with the same packet flow, only the real-time scheduling
  dropped.
- **End-to-end.** The acceptance test wires two controllers to a link: one
  advertises, the other scans, and the scanner's host receives an Advertising
  Report carrying the advertiser's address and data — which then round-trips
  through the `bumble-hci` codec.
- **ACL data path (slice 8).** Once connected, `LocalLink::send_acl_data` routes
  a host's ACL payload to the peer host on its own connection handle. The
  controller treats the payload as opaque bytes — the integration test builds an
  **ATT PDU → L2CAP PDU → ACL** on the sender and parses it back up the stack on
  the receiver, composing four crates (`bumble-controller`, `bumble-hci`,
  `bumble-l2cap`, `bumble-att`) into one end-to-end flow.
- **Deferred:** LL control PDUs, disconnection, extended advertising sets,
  CIS/ISO, encryption, and classic/LMP.

## Slice 4 — what's here

The L2CAP frame codec in the [`bumble-l2cap`](bumble-l2cap/) crate (std-only —
the frame format is independent of HCI and addresses):

- **`L2capPdu`** — the L2CAP data-packet frame with an optional Frame Check
  Sequence (`crc_16`, CRC-16-IBM), verified against Bumble's FCS test vectors.
- **`serialize_psm` / `parse_psm`** — the variable-length Protocol/Service
  Multiplexer encoding.
- **`ControlFrame`** — signaling frames: Connection_Request and the four
  credit-based frames (Connection Request/Response, Reconfigure
  Request/Response), plus a `Generic` fallback for other signaling codes.

Deferred: the full signaling command set, configuration options,
enhanced-retransmission control fields, and the channel manager / reassembly.

## Slice 5 — what's here

The ATT (Attribute Protocol) PDU codec in the [`bumble-att`](bumble-att/) crate
(depends on `bumble` for `Uuid`):

- **`AttPdu`** — `[op_code, payload…]` framing with typed variants:
  Error_Response, Exchange_MTU_Request/Response, Read_Request/Response,
  Read_By_Group_Type_Request (UUID group type), Write_Request/Response,
  Handle_Value_Notification, plus a `Generic` fallback and the `is_command` /
  `is_signed` op-code bit helpers.

Deferred: the remaining ATT PDUs (Find_Information, grouped
Read_By_Type_Response, prepared/queued and signed writes, indications) and the
GATT client/server layers.

## Slice 6 — what's here

The SMP cryptographic toolbox in the [`bumble-crypto`](bumble-crypto/) crate
(Vol 3, Part H - 2.2), on top of the audited `aes` crate:

- **`e`** — the AES block security function (byte-swapped I/O).
- **`aes_cmac`** — RFC 4493 AES-CMAC, hand-implemented (subkey generation +
  padding) over AES-128.
- **`c1` / `s1` / `ah`** — LE Legacy confirm/key/hash functions.
- **`f4` / `f5` / `f6` / `g2` / `h6` / `h7`** — LE Secure Connections
  confirm/key/check/numeric-comparison and link-key conversion functions.

Every function is pinned to the published Bluetooth-spec and RFC 4493 test
vectors — the strongest correctness check in the whole port. ECC P-256 key
agreement and RNG are out of scope for this slice.

## Slice 9 — what's here (the capstone)

A minimal GATT/ATT server in the [`bumble-gatt`](bumble-gatt/) crate:

- **`AttServer`** — an attribute table (handle → value) that turns an incoming
  ATT request into the correct response: Exchange_MTU, Read_Request,
  Write_Request, with Error_Response for missing attributes.

Its integration test is the real payoff — a **characteristic write-then-read
between two virtual devices, end-to-end through every layer**: the central
issues ATT requests that travel ATT → L2CAP → ACL → link → peer host; the
peripheral feeds them to the `AttServer` and returns the responses the same way.
Central writes `[0xBB, 0xCC]` to handle `0x0025` and reads back exactly that.

This composes all seven crates and is the first point where the port does
something a Bluetooth stack is actually *for* — read/write a characteristic
between two devices — rather than exercising a single layer in isolation.

## Slice 10 — what's here

The host-side glue in the [`bumble-host`](bumble-host/) crate — this is what
makes the cross-layer composition a **library capability** rather than test
wiring:

- **`Device`** — sits above a controller (by id on a shared `LocalLink`), owns
  the ATT↔L2CAP↔ACL sequencing: learns its connection handle from the
  Connection Complete event, sends ATT PDUs with `send_att`, and on `poll`
  processes inbound ACL (an optional server-role `AttServer` answers requests
  automatically; responses/notifications are queued for the client).
- **`pump`** — drives a set of devices to quiescence (the synchronous event
  loop this port needs).

The acceptance test does the same attribute write/read as slice 9, but the test
now only performs connection setup and high-level `send_att` calls — the layer
sequencing lives entirely in `Device`. A `full_le_lifecycle` test exercises the
whole flow in one scenario — **connect → discover → write → read → notify →
disconnect** — through the `Device` API.

Deferred: L2CAP fragmentation/reassembly across multiple ACL packets (each ATT
PDU is assumed to fit one packet), the LE signaling channel, and multiple
connections per device.

## Slice 11 — what's here

A real GATT layer in [`bumble-gatt`](bumble-gatt/), on top of the slice-9
`AttServer`:

- **`GattServer`** — takes a set of `Service`s (each with `Characteristic`s) and
  builds the standard attribute database: a Primary Service declaration, then
  per characteristic a declaration attribute and its value attribute, with
  sequential handles. It answers **primary discovery** — Read_By_Group_Type for
  services and Read_By_Type for characteristics — plus reads and writes.
- **`AttRequestHandler`** trait — both `AttServer` and `GattServer` implement it,
  so a `bumble-host` `Device` can be given either.

The end-to-end test does a genuine GATT client flow over the full stack:
discover the primary service, discover its characteristic (learning the value
handle from the declaration), then read the value — `"bumble-rs"` — by that
discovered handle. This is real GATT discovery, not raw fixed handles. Slice 5
gained the ATT `Read_By_Type`/`Read_By_Group_Type` response PDUs to support it.

## Slice 14 — what's here

The SMP layer in [`bumble-smp`](bumble-smp/) — the slice that wires the
previously standalone `bumble-crypto` into a real protocol:

- **`SmpPdu`** — the Security Manager PDUs (Pairing Request/Response/Confirm/
  Random/Failed) over L2CAP CID `0x0006`, oracle-pinned against Python.
- **`legacy_confirm` / `legacy_stk`** — the LE Legacy pairing `c1`/`s1`
  computations, wrapping `bumble-crypto`; the unit test pins the confirm to the
  published Bluetooth-spec `c1` vector.

The `bumble-host` integration test runs a **real JustWorks pairing handshake
over the connection**: two peers exchange Pairing Request/Response/Confirm/Random
on the SMP channel (CID `0x0006`), each verifies the other's confirm by
recomputing `c1` with the *received* random, and both independently derive the
same Short Term Key. This wires the last crate into the connection flow — all
nine crates now genuinely compose (SMP PDUs cross the L2CAP/ACL/link boundary
using the crypto toolbox).

## Slice 16 — what's here

The Service Discovery Protocol codec in [`bumble-sdp`](bumble-sdp/) — the first
piece of Classic Bluetooth (BR/EDR) infrastructure. SDP is how a classic device
discovers which services a peer offers and how to reach them, and its
self-describing data-element format is the value encoding every classic profile
(RFCOMM/SPP, A2DP, AVRCP, HFP, HID, …) builds its service records from:

- **`DataElement`** — the recursive type-length-value element format (Vol 3,
  Part B - 3.3): nil, unsigned/signed integers (1/2/4/8 bytes), 16/32/128-bit
  UUIDs, text strings, booleans, sequences, alternatives and URLs — all eight
  size-index encodings, including the 2-byte and 4-byte length forms exercised
  by 300-byte and 100,000-byte strings.
- **`ServiceAttribute`** — the `(attribute-id, value)` pair a service record is
  built from, plus the flat alternating-element list encoding a record uses.
- **`SdpPdu`** — the seven Protocol Data Units (Vol 3, Part B - 4.4–4.7), with
  the common `[pdu-id, transaction-id, parameter-length, parameters…]` framing.

Every serialization is **oracle-pinned** to a hex literal captured from upstream
Python Bumble (commit `1d26b99`), mirroring `tests/sdp_test.py::test_data_elements`.
The oracle immediately earned its keep: it caught that `SDP_ErrorResponse`'s
`error_code` is serialized **little-endian** (upstream's default u16 encoding)
while every other SDP integer field is big-endian — a quirk a round-trip test
alone would have missed. Deferred, matching the port's synchronous, codec-first
approach: the asyncio `Client`/`Server`, the continuation-state reassembly loop,
and the higher-level service-record database.

## Slice 17 — what's here

The RFCOMM frame + MCC codec in [`bumble-rfcomm`](bumble-rfcomm/) — the second
piece of Classic infrastructure. RFCOMM (TS 07.10) emulates serial cables over
L2CAP and is the transport the Serial Port Profile and many other classic
profiles run on; a device finds a peer's RFCOMM server channel through an SDP
service record (slice 16), then speaks this framing to it:

- **`RfcommFrame`** — the SABM/UA/DM/DISC/UIH frame layout
  `[address, control, length, information…, fcs]`, with the 1- and 2-byte
  length indicators (EA bit), the credit-based flow-control variant of UIH
  (the leading credit octet excluded from the length), and the FCS.
- **`compute_fcs`** — the CRC-8 frame check sequence over the TS 07.10 table.
- **`RfcommMccPn` / `RfcommMccMsc`** — the Parameter Negotiation and Modem
  Status Command MCC messages, plus `make_mcc`/`parse_mcc` for the MCC header.

Every serialization is **oracle-pinned** to a hex literal from upstream
(commit `1d26b99`), mirroring the byte round-trip in
`tests/rfcomm_test.py::basic_frame_check`, with `compute_fcs` pinned directly so
a single-nibble error in the hand-transcribed 256-byte table fails locally.
Deferred, matching the codec-first approach: the asyncio `DLC`, `Multiplexer`,
`Client`/`Server` credit-flow state machine and the SDP-record helpers.

## Slice 18 — what's here

The **GATT client** in [`bumble-gatt`](bumble-gatt/) — the read/write/subscribe
counterpart to the server built in slices 9–12. `GattClient` is a synchronous
port of the discovery and access logic in upstream `gatt_client.py`:

- **Discovery** — all primary services (Read_By_Group_Type), service-by-UUID
  (Find_By_Type_Value), a service's characteristics (Read_By_Type, computing
  each characteristic's handle range the way upstream does), and a
  characteristic's descriptors (Find_Information) — each with upstream's
  iterate-until-`ATTRIBUTE_NOT_FOUND` termination.
- **Read** — `read_value`, including the long-read fallback that continues with
  Read_Blob when a value fills the MTU.
- **Write** — `write_value` with response (Write_Request) or without
  (Write_Command).
- **Subscribe** — writes the CCCD (notification or indication bits) and handles
  incoming notifications (cache) and indications (cache + return the required
  Handle_Value_Confirmation).

The client emits ATT PDUs through an `AttTransport`; a blanket impl makes any
server usable as a transport, so the crate's
[`tests/client.rs`](bumble-gatt/tests/client.rs) runs a real client against a
real `GattServer` end-to-end — discover → read (short and long) → write (with
and without response) → subscribe → notify/indicate. The nine ATT PDUs the
client needs (Find_Information, Find_By_Type_Value, Read_Blob, Write_Command,
Handle_Value_Indication/Confirmation) were added to `bumble-att` and
oracle-pinned. Deferred, matching the synchronous port: the async bearer, the
`gatt_adapters` typed-value proxies, and event listeners.

## Acceptance

The port's contract is the upstream Python test suite, ported 1:1:

| Rust test | Upstream source |
|-----------|-----------------|
| `test_ad_data` | `tests/core_test.py` |
| `test_get_dict_key_by_value` | `tests/core_test.py` |
| `test_uuid_to_hex_str` | `tests/core_test.py` |
| `test_uuid_hash` | `tests/core_test.py` |
| `test_appearance` | `tests/core_test.py` |
| `test_class_of_device` | `tests/core_test.py` |
| `test_address` | `tests/hci_test.py` |

These live in [`bumble/tests/acceptance.rs`](bumble/tests/acceptance.rs); the
same behaviors are also covered by inline unit tests in each module.

Slice 2's 42 HCI tests live in
[`bumble-hci/tests/acceptance.rs`](bumble-hci/tests/acceptance.rs), each ported
from a `tests/hci_test.py` case and pinned to Python-oracle bytes.

## Running

```bash
cargo test                              # all tests (debug)
cargo test --release                    # all tests (release)
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## Layout

```
bumble-rs/
├── Cargo.toml                 # workspace
├── bumble/                    # slice-1 library crate
│   ├── src/{lib,uuid,address,appearance,class_of_device,advertising_data}.rs
│   └── tests/acceptance.rs    # ported upstream tests
├── bumble-hci/                # slice-2 HCI codec crate
│   ├── src/{lib,codes,command,event,packet,return_parameters}.rs
│   └── tests/acceptance.rs    # ported hci_test.py cases (oracle-pinned)
├── bumble-controller/         # slice-3 controller + virtual link crate
│   ├── src/lib.rs
│   └── tests/scenario.rs      # end-to-end advertising→scan→report scenario
├── bumble-l2cap/              # slice-4 L2CAP frame codec crate
│   ├── src/lib.rs
│   └── tests/acceptance.rs    # ported l2cap_test.py codec cases (oracle-pinned)
├── bumble-att/                # slice-5 ATT protocol PDU codec crate
│   ├── src/lib.rs
│   └── tests/acceptance.rs    # ported gatt_test.py ATT cases (oracle-pinned)
├── bumble-crypto/             # slice-6 SMP crypto toolbox crate
│   ├── src/lib.rs
│   └── tests/vectors.rs       # ported smp_test.py spec/RFC vectors
├── bumble-gatt/               # slice-9 GATT/ATT server + slice-18 GATT client
│   ├── src/lib.rs             # AttServer, GattServer
│   ├── src/client.rs         # GattClient (slice 18)
│   ├── tests/end_to_end.rs   # attribute write/read across the full stack
│   └── tests/client.rs       # two-party client↔server discovery/read/write/subscribe
├── bumble-host/               # slice-10 Host/Device glue crate
│   ├── src/lib.rs
│   └── tests/gatt_over_host.rs # full LE lifecycle via the Device API
├── bumble-smp/                # slice-14 SMP codec + legacy pairing crate
│   └── src/lib.rs             # wires bumble-crypto (c1/s1) into pairing
├── bumble-sdp/                # slice-16 SDP codec crate (first Classic piece)
│   ├── src/{lib,pdu}.rs       # DataElement + ServiceAttribute + SdpPdu
│   └── tests/acceptance.rs    # ported sdp_test.py cases (oracle-pinned)
├── bumble-rfcomm/             # slice-17 RFCOMM frame + MCC codec crate
│   ├── src/lib.rs             # RfcommFrame + compute_fcs + MCC PN/MSC
│   └── tests/acceptance.rs    # ported rfcomm_test.py frame check (oracle-pinned)
└── docs/superpowers/          # design specs + implementation plans
```

## Contributing

Contributions are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) for the
build/test bar and the ground-truth verification philosophy, and
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). To report a vulnerability, see
[SECURITY.md](SECURITY.md).

```bash
cargo test --workspace                     # all tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## License

Licensed under the [Apache License, Version 2.0](LICENSE), matching upstream
Bumble. See [NOTICE](NOTICE) for attribution. Unless you explicitly state
otherwise, any contribution you submit shall be licensed as above, without
additional terms.
