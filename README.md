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
| 5. ATT protocol PDU codec | `bumble-att` | ✅ 8/8 tests green |
| 6. SMP cryptographic toolbox | `bumble-crypto` | ✅ 10/10 vectors green |
| 7. LE connection establishment (in the controller) | `bumble-controller` | ✅ (see slice 3+7) |
| 8. ACL data path (ATT-over-L2CAP-over-ACL, cross-layer) | `bumble-controller` | ✅ 8/8 controller tests |
| 9. Minimal GATT/ATT server (end-to-end attribute read/write) | `bumble-gatt` | ✅ 5/5 tests green |
| 10. Host/Device glue (ATT↔L2CAP↔ACL sequencing as a library API) | `bumble-host` | ✅ 3/3 tests green |
| 11. GATT server model + primary discovery (service/characteristic) | `bumble-gatt` | ✅ 7/7 tests green |
| 12. GATT notifications (server → client) | `bumble-host` | ✅ |
| 13. LE disconnection (Disconnect → Disconnection Complete both sides) | `bumble-controller` | ✅ |
| 14. SMP PDU codec + LE Legacy pairing (wires in `bumble-crypto`) | `bumble-smp` | ✅ 2/2 tests green |
| 15+. LE Secure Connections pairing, GATT descriptors, classic (RFCOMM/SDP/A2DP…) | — | planned |

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
| `controller.py` (2.8k) | `bumble-controller` | 🟡 | **Full command surface**: every command upstream's `controller.py` handles (93, via the generated [`command_surface`](bumble-controller/src/command_surface.rs) table) gets a well-formed reply of the matching HCI shape — Command Complete + SUCCESS for config/set commands, Command Status for operations completing via a later event, and the spec-correct "Unknown HCI Command" for anything upstream also doesn't handle. **Functionally simulated**: LE advertising/scanning, connection establishment, ACL routing, disconnection, the read commands, and per-connection `LE_Set_Data_Length`/`LE_Set_PHY` (with follow-up meta events). A **behavioral simulation with placeholder values** (as upstream's `controller.py` also is) — *not* oracle-pinned like the HCI codec. Deferred (behavior, not codec): CIS/ISO, encryption/LTK, remote-version exchange, extended/periodic advertising, classic/LMP LL state machines. |
| `link.py` (0.15k) | `bumble-controller` | 🟡 | In-process **synchronous** `LocalLink`. Deferred: LL control PDUs, LMP routing, async scheduling. |
| `ll.py` (0.2k) | `bumble-controller` | 🟡 | Advertising/connection PDUs modeled as in-process structs, not serialized LL PDUs. |
| `host.py` (2.1k) | `bumble-host` | 🟡 | `Device` glue (ATT↔L2CAP↔ACL sequencing + pairing transport). Not the full host feature set. |
| `device.py` (7.0k) | `bumble-host` | 🟡 | Minimal `Device`/`pump`; the high-level device API (advertising/scanning/connection orchestration, GATT client, listeners) is not ported. |
| `lmp.py` (0.4k) | — | ⬜ | Classic Link Manager Protocol. |

### L2CAP
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `l2cap.py` (3.1k) | `bumble-l2cap` | 🟡 | PDU + signaling frames + FCS. Deferred: channel manager, fragmentation/reassembly, credit-based flow-control runtime. |

### ATT / GATT
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `att.py` (1.1k) | `bumble-att` | 🟡 | Representative PDUs incl. discovery. Deferred: Find_Information, prepared/queued & signed writes, indications. |
| `gatt.py` (0.6k), `gatt_server.py` (1.2k) | `bumble-gatt` | 🟡 | Attribute DB, service/characteristic model, primary discovery, read/write/notify. Deferred: descriptors/CCCD subscriptions, included services. |
| `gatt_client.py` (1.2k), `gatt_adapters.py` (0.4k) | — | ⬜ | No client abstraction; discovery is driven request-by-request in tests. |

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
| `rfcomm.py` (1.2k) | — | ⬜ | Serial port emulation. |
| `sdp.py` (1.4k) | — | ⬜ | Service Discovery Protocol. |
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
types, HCI framing, L2CAP/ATT/GATT/SMP codecs, the SMP crypto toolbox, and a
controller/link/host that runs the LE lifecycle end-to-end. Everything else —
the exhaustive HCI catalog, the full device/host/GATT-client abstractions, LE
Secure Connections, real transports, and **all of Classic Bluetooth and the
profiles** — is the large majority of the ~82k upstream lines and remains to do.

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
├── bumble-gatt/               # slice-9 minimal GATT/ATT server crate
│   ├── src/lib.rs
│   └── tests/end_to_end.rs    # attribute write/read across the full stack
├── bumble-host/               # slice-10 Host/Device glue crate
│   ├── src/lib.rs
│   └── tests/gatt_over_host.rs # full LE lifecycle via the Device API
├── bumble-smp/                # slice-14 SMP codec + legacy pairing crate
│   └── src/lib.rs             # wires bumble-crypto (c1/s1) into pairing
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
