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
| 4+21. L2CAP codec + Classic connection-oriented channel runtime | `bumble-l2cap` | ✅ 13/13 tests green |
| 5. ATT protocol PDU codec (incl. Find_Information, Read_Blob, indications) | `bumble-att` | ✅ 16/16 tests green |
| 6. SMP cryptographic toolbox (+ P-256 ECC/ECDH, slice 19) | `bumble-crypto` | ✅ 14/14 tests green |
| 7. LE connection establishment (in the controller) | `bumble-controller` | ✅ (see slice 3+7) |
| 8. ACL data path (ATT-over-L2CAP-over-ACL, cross-layer) | `bumble-controller` | ✅ 8/8 controller tests |
| 9. Minimal GATT/ATT server (end-to-end attribute read/write) | `bumble-gatt` | ✅ 5/5 tests green |
| 10. Host/Device glue (ATT↔L2CAP↔ACL sequencing as a library API) | `bumble-host` | ✅ 3/3 tests green |
| 11. GATT server model + primary discovery (service/characteristic) | `bumble-gatt` | ✅ 7/7 tests green |
| 12. GATT notifications (server → client) | `bumble-host` | ✅ |
| 13. LE disconnection (Disconnect → Disconnection Complete both sides) | `bumble-controller` | ✅ |
| 14. SMP PDU codec + LE Legacy pairing (wires in `bumble-crypto`) | `bumble-smp` | ✅ 2/2 tests green |
| 16. SDP codec (data elements + PDUs) — first Classic (BR/EDR) piece | `bumble-sdp` | ✅ 28/28 tests green |
| 17. RFCOMM frame + MCC codec (serial-cable emulation over L2CAP) | `bumble-rfcomm` | ✅ 16/16 tests green |
| 18. GATT client (discovery, read/long-read, write, subscribe) | `bumble-gatt` | ✅ client tests green |
| 19. LE Secure Connections pairing (P-256 ECDH + JustWorks derivation) | `bumble-crypto` / `bumble-smp` | ✅ oracle + two-party green |
| 20. RFCOMM + SDP session runtimes (Multiplexer/DLC credit flow, SDP client/server) | `bumble-rfcomm` / `bumble-sdp` | ✅ oracle + two-party green |
| 21. Classic L2CAP channels (PSM/CID allocation, configure/MTU, data, disconnect) | `bumble-l2cap` | ✅ oracle + two-party green |
| 22. RFCOMM + SDP bindings over live Classic L2CAP channels | `bumble-rfcomm` / `bumble-sdp` | ✅ two-party green |
| 23. AT parameter + HFP command/response streaming parser | `bumble-at` | ✅ 5/5 tests green |
| 24. HFP service-level connection (HF↔AG feature/indicator negotiation) | `bumble-hfp` | ✅ transcript + RFCOMM/L2CAP green |
| 25. HFP call control, indicators, unsolicited events, codec negotiation | `bumble-hfp` | ✅ direct + RFCOMM/L2CAP green |
| 26. HFP HF/AG SDP record generation and discovery parsing | `bumble-hfp` | ✅ SDP client/server green |
| 27. HFP SCO/eSCO parameters, controller/host connection lifecycle, and audio routing | `bumble-hfp` / `bumble-controller` / `bumble-host` | ✅ CVSD + mSBC, two-party green |
| 28. Remaining HFP normative models, AG controls, typed metadata, and public helpers | `bumble-hfp` | ✅ upstream behavior families green |
| 29. AVDTP signaling catalog, capability codec, and safe PDU fragmentation | `bumble-avdtp` | ✅ 38 messages payload-pinned |
| 30. AVDTP endpoint/session state machine and live Classic L2CAP binding | `bumble-avdtp` | ✅ full lifecycle, fragmented config green |
| 31. A2DP SBC, AAC, and vendor Opus codec capability models | `bumble-a2dp` | ✅ upstream vectors + AVDTP integration green |
| 32. RTP packet codec with CSRC, extension, padding, and malformed-input safety | `bumble-rtp` | ✅ exact round trips green |
| 33. A2DP SBC frame parsing and MTU-aware RTP aggregation | `bumble-a2dp` | ✅ upstream fixture + final-flush coverage green |
| 34. A2DP ADTS AAC parsing and exact LATM/RTP packet source | `bumble-a2dp` | ✅ upstream fixture green |
| 35. A2DP Ogg Opus parsing and RTP packet source | `bumble-a2dp` | ✅ upstream + multi-page fixtures green |
| 36. A2DP RTP packets over a live AVDTP Classic L2CAP media channel | `bumble-a2dp` | ✅ source→sink packet equality green |
| 37. A2DP source/sink SDP records and discovery parsing | `bumble-a2dp` | ✅ SDP client/server green |
| 38. High-level A2DP SEP discovery, codec selection, and stream orchestration | `bumble-a2dp` | ✅ live signaling lifecycle green |
| 39. AV/C generic, vendor-dependent, and panel pass-through frame codec | `bumble-avc` | ✅ upstream vectors green |
| 40. AVCTP fragmentation/reassembly and live Classic L2CAP protocol | `bumble-avctp` | ✅ upstream + two-party green |
| 41. AVRCP vendor-PDU envelope and independent fragmentation assembler | `bumble-avrcp` | ✅ upstream vectors green |
| 42. Complete AVRCP typed command catalog | `bumble-avrcp` | ✅ 22/22 Python-oracle vectors green |
| 43. Complete AVRCP typed notification-event catalog | `bumble-avrcp` | ✅ 9/9 Python-oracle vectors green |
| 44. Complete AVRCP typed response and browseable-item catalog | `bumble-avrcp` | ✅ 23/23 Python-oracle vectors green |
| 45. AVRCP controller/target runtime over live AVCTP/L2CAP | `bumble-avrcp` | ✅ command, notification, pass-through green |
| 46. AVRCP controller/target SDP records and discovery | `bumble-avrcp` | ✅ SDP client/server green |
| 47. HIDP host/device protocol and paired Classic L2CAP channels | `bumble-hid` | ✅ control + interrupt green |
| 48. Common bitstreams and MPEG-4 LATM AAC-to-ADTS codec | `bumble-codecs` | ✅ upstream fixture green |
| 49+. Remaining modules… | — | planned |

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
| `controller.py` (2.8k) | `bumble-controller` | 🟡 | **Full command surface**: every command upstream's `controller.py` handles (93, via the generated [`command_surface`](bumble-controller/src/command_surface.rs) table) gets a reply of the matching HCI shape — Command Complete + SUCCESS for config/set commands, Command Status for operations completing via a later event, and the spec-correct "Unknown HCI Command" for anything upstream also doesn't handle. **Functionally simulated**: LE advertising/scanning, connection establishment, ACL routing, disconnection, the read commands (`Read_BD_ADDR`/`Read_Local_Name`/`LE_Read_Buffer_Size`/`LE_Read_Local_Supported_Features`/`LE_Rand`), per-connection `LE_Set_Data_Length`/`LE_Set_PHY` (with follow-up meta events), and — via LL control-PDU exchange over the link — **encryption start** (`LE_Enable_Encryption` → `Encryption Change` on both sides), **remote-features** (`FeatureReq`/`FeatureRsp` → `LE_Read_Remote_Features_Complete`), and **CIS establishment** (LE Audio: `LE_Set_CIG_Parameters`/`LE_Create_CIS` → `LE CIS Request` → `LE_Accept_CIS_Request` → `LE CIS Established` on both sides). Also **classic (BR/EDR)**: ACL connection establishment (`Create_Connection` → `Connection Request` → `Accept_Connection_Request` → `Connection Complete`), `Remote_Name_Request`, `Read_Remote_Supported_Features`, and SCO/eSCO request/accept/reject/disconnect with HCI synchronous-data routing, via simplified LMP PDUs over the link. Other read commands are acknowledged SUCCESS **without a synthesized payload** (a documented stub, not a full read). A **behavioral simulation with placeholder values** (as upstream's `controller.py` also is) — *not* oracle-pinned like the HCI codec. Deferred (behavior, not codec): LTK verification, ISO data-path streaming, remote-version exchange, extended/periodic advertising, and classic auth/encryption/role-switch sub-flows. |
| `link.py` (0.15k) | `bumble-controller` | 🟡 | In-process **synchronous** `LocalLink` with LL-control, simplified LMP, ACL, and SCO/eSCO routing. Deferred: serialized over-the-air PDUs and async scheduling. |
| `ll.py` (0.2k) | `bumble-controller` | 🟡 | Advertising/connection PDUs modeled as in-process structs, not serialized LL PDUs. Control PDUs (`EncReq`, `FeatureReq`/`PeripheralFeatureReq`/`FeatureRsp`, `TerminateInd`) are exchanged between controllers via `LocalLink::pump_ll` to drive the encryption-start, remote-features, and CIS-establishment (`CisReq`/`CisRsp`/`CisInd`) flows. |
| `host.py` (2.1k) | `bumble-host` | 🟡 | `Device` glue (ATT↔L2CAP↔ACL sequencing + pairing transport), plus Classic ACL and synchronous connection/request/data APIs. Not the full host feature set. |
| `device.py` (7.0k) | `bumble-host` | 🟡 | Minimal `Device`/`pump`; the high-level device API (advertising/scanning/connection orchestration, GATT client, listeners) is not ported. |
| `lmp.py` (0.4k) | `bumble-controller::lmp` | 🟡 | Classic Link Manager Protocol PDUs modeled as in-process structs (`HostConnectionReq`/`Accepted`, `NameReq`/`NameRes`, `FeaturesReq`/`FeaturesRes`, synchronous request/accept/reject, `Detach`) driving the classic connection/name/features/SCO-eSCO flows via `LocalLink::pump_classic`. The role-switch / authentication / encryption LMP sub-dance is simplified away. |

### L2CAP
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `l2cap.py` (3.1k) | `bumble-l2cap` | 🟡 | PDU + signaling frames + FCS, plus a synchronous Classic connection-oriented `ChannelManager`: valid dynamic PSM/CID allocation, Connection/Configure/Disconnection signaling, MTU option negotiation, PSM refusal, bidirectional basic-mode SDUs, and deterministic server accept. Deferred: ACL fragmentation/reassembly, enhanced retransmission mode, LE credit-based channel runtime, and remaining signaling commands. |

### ATT / GATT
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `att.py` (1.1k) | `bumble-att` | 🟡 | PDUs incl. discovery (Read_By_Type/Group_Type, Find_Information, Find_By_Type_Value), reads (Read/Read_Blob), writes (Request/Command), and notifications/indications + confirmation — all oracle-pinned. Deferred: prepared/queued (Prepare/Execute), Read_Multiple, and signed writes. |
| `gatt.py` (0.6k), `gatt_server.py` (1.2k) | `bumble-gatt` | 🟡 | Attribute DB, service/characteristic model, primary discovery, read/write/notify, plus Find_Information/Find_By_Type_Value discovery, a CCCD descriptor per notify/indicate characteristic, MTU-sized reads with Read_Blob, and server-initiated notify/indicate. Deferred: included services, prepared writes. |
| `gatt_client.py` (1.2k), `gatt_adapters.py` (0.4k) | `bumble-gatt` | 🟡 | **`GattClient` (slice 18)**: service / characteristic / descriptor discovery, reads (with long-read via Read_Blob), writes (with and without response), and notify/indicate subscriptions (CCCD write + notification/indication handling), over an `AttTransport`. Verified by a two-party client↔server integration test. Deferred (matching the synchronous port): the async bearer, `gatt_adapters` typed-value proxies, and event listeners. |

### Security (SMP + crypto)
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `crypto/` | `bumble-crypto` | ✅ | All SMP **symmetric** security functions — `e`, AES-CMAC, `c1`, `s1`, `f4`/`f5`/`f6`, `g2`, `h6`/`h7`, `ah` — spec/RFC-4493 vector-verified, plus **P-256 `EccKey`** (slice 19: keygen, `from_private_key_bytes`, public-key coordinates, ECDH) oracle-pinned to upstream. Deferred: none of the crypto primitives. |
| `smp.py` (2.0k), `pairing.py` (0.3k) | `bumble-smp` | 🟡 | PDU codec (incl. all **LE Secure Connections** PDUs — public key, DHKey check, keypress, key-distribution), LE Legacy (JustWorks) pairing over the link, and the **SC JustWorks derivation** (`sc` module: `f4` confirm + `f5`/`f6`/`g2` keys) oracle-pinned and run as a two-party handshake. Deferred: full pairing state machine, Numeric Comparison / passkey / OOB entry UX, key distribution over the wire, bonding storage. |

### Transports & drivers
| Upstream | Rust crate | Status | Notes |
|---|---|---|---|
| `transport/*` — USB, UART/serial, TCP, WebSocket, UDP, PTY, android-netsim, vhci, … | — | ⬜ | The link is in-process only; no real transports (so no talking to real hardware or netsim yet). |
| `drivers/*` — Intel, Realtek | — | ⬜ | Vendor controller firmware/init. |

### Classic Bluetooth (BR/EDR)
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `rfcomm.py` (1.2k) | `bumble-rfcomm` | 🟡 | **Frame codec + session runtime + L2CAP binding**: the `RfcommFrame` TS 07.10 framing (SABM/UA/DM/DISC/UIH, 1- and 2-byte length indicators, credit-based UIH flow control), CRC-8, and PN/MSC MCC messages are oracle-pinned. Slice 20 adds `mux::{Multiplexer, Dlc}` for session/DLC open and credit flow; slice 22 adds `l2cap::L2capMultiplexer`, which derives its frame ceiling from the negotiated peer MTU and runs the complete session, DLC, replenishment, data, and disconnect flows over a live Classic channel. Deferred: retransmission (upstream also uses `max_retransmissions = 0`), aggregate flow control, and socket/async convenience APIs. |
| `sdp.py` (1.4k) | `bumble-sdp` | 🟡 | **Codec + client/server runtime + L2CAP binding**: all `DataElement` encodings, `ServiceAttribute`, and seven `SdpPdu` messages are oracle-pinned. Slice 20 adds `service::{SdpServer, SdpClient}` with matching, selection, and continuation; slice 22 adds `l2cap::{SdpL2capServer, L2capSdpTransport}`, including fallible transport propagation and continuation over negotiated Classic channels. Deferred: async/event convenience APIs. |
| `at.py` (0.1k) + HFP AT models | `bumble-at` | ✅ | Parameter tokenizer/parser ported 1:1, nested values, HFP `AtCommand`/`AtResponse` forms, and incremental command (`\r`) / response (`\r\n`) stream framing. |
| `hfp.py` (2.1k) | `bumble-hfp` | 🟡 | Normative HF/AG models and paired SLC state machines, serialized post-SLC command completion, call control/current-call listing, HF/AG indicators, ring/volume/typed caller-ID/typed voice events, codec request/selection, CMEE/CCWA/BIA/CLIP controls, HF/AG SDP record generation/discovery, and all eight upstream HFP 1.8 SCO/eSCO parameter presets. Control flows run end-to-end over RFCOMM/L2CAP and records through SDP client/server; negotiated CVSD/mSBC codecs establish and route audio through the host/controller link. The core synchronous protocol surface covers the upstream behavior families; deferred: asyncio/event-emitter convenience and actual CVSD/mSBC media encoding. |
| `hid.py` (0.6k) | `bumble-hid` | ✅ | Complete HIDP message codec (handshake/control/get+set report/get+set protocol/data), open protocol identifiers, exact little-endian GET_REPORT buffer sizing, host/device dispatch, callback-to-handshake mapping, suspend/unplug events, role-correct input/output reports, MTU enforcement, and paired control (`0x0011`) + interrupt (`0x0013`) transports over live Classic L2CAP. |
| `avdtp.py` (2.4k) | `bumble-avdtp` | 🟡 | Slice 29 ports all 38 upstream signaling command/accept/reject forms, endpoint descriptors, generic and media-codec capability TLVs, open protocol enums, exact payload encoding/decoding, unknown-signal preservation, and safe single/fragmented PDU assembly. Slice 30 adds local endpoint registration, command dispatch, atomic multi-SEP validation, the configured/open/streaming/idle lifecycle, event capture, transaction labels, and a live Classic L2CAP binding. Deferred: initiator-side high-level stream proxy, RTP media channel/pump, listener convenience, and SDP discovery. |
| `a2dp.py` (1.0k) | `bumble-a2dp` | ✅ | Open codec identifiers and exact SBC, MPEG-2/4 AAC, vendor-specific, and Opus capability models; upstream byte vectors; SBC/ADTS AAC/Ogg Opus parsers and RTP packet sources; live Classic L2CAP media transport; source/sink SDP records; and a high-level initiator that discovers SEPs, verifies media transport + codec compatibility, and drives configure/open/start/suspend/close over AVDTP. Async generators/listeners are represented by synchronous collections and a caller-supplied drive callback. |
| `rtp.py` (0.1k) | `bumble-rtp` | ✅ | Slice 32 ports RTP v2 media packet parsing/serialization with marker/payload type, wrapping sequence/timestamp fields, SSRC and correctly spaced CSRC entries. It additionally implements standard header extensions and padding, validates bit fields/lengths, and returns errors for truncated input instead of upstream's unchecked indexing. |
| `avc.py` (0.5k) | `bumble-avc` | 🟡 | Slice 39 ports open subunit/opcode/command/response/operation identifiers; generic command and response frames; single and double-extended subunit IDs; 24-bit-company vendor-dependent frames; and panel pass-through press/release operations with bounded operation data. Upstream AVRCP vectors are byte-pinned and malformed frames return errors. Deferred: additional typed AV/C opcode subclasses beyond the two used by AVRCP. |
| `avctp.py` (0.3k) | `bumble-avctp` | 🟡 | Slice 40 ports transaction labels, single/start/continue/end packets, command/response and IPID flags, 16-bit PIDs, safe fragmented-message assembly, MTU-aware outbound fragmentation, and a live Classic L2CAP binding. Registered PIDs receive commands; unknown PIDs automatically produce IPID responses. Deferred: handler callbacks and browsing-channel policy are provided by the higher AVRCP runtime. |
| `avrcp` (2.9k) | `bumble-avrcp` | ✅ | Slices 41–46 port the complete typed wire catalog, bounded controller/target runtime, delegate behavior, interim→changed notifications, pass-through keys, both fragmentation layers over live Classic L2CAP, and controller/target SDP records + discovery. The browsing PSM is advertised exactly when supported; upstream itself does not implement a separate browsing-channel runtime. Async iterators are represented by explicit `RuntimeEvent` values. |
| `codecs.py` (0.5k) | `bumble-codecs` | ✅ | Complete bit reader/writer plus MPEG-4 LATM `AudioMuxElement`, `StreamMuxConfig`, `AudioSpecificConfig`, GA config, AAC-LC constructor, arbitrary-length payload framing, and ADTS conversion. Upstream's long LATM fixture produces the exact ADTS oracle; unaligned bit chunks and 255/510-byte length boundaries round-trip safely. |

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
Classic Bluetooth now has its **two foundation protocols and their channel
layer** — SDP
(`bumble-sdp`: codec + a client/server continuation runtime), which the classic
profiles build service records on, and RFCOMM (`bumble-rfcomm`: frame codec + a
`Multiplexer`/`DLC` credit-flow session runtime), the serial-cable transport
those profiles run over, plus a Classic L2CAP connection-oriented runtime with
configuration and MTU negotiation. Both protocol runtimes now bind directly to
those channels. Everything else — the full high-level device/host
orchestration, LE Secure Connections state machine, real transports, and the
**rest of Classic Bluetooth (A2DP/AVRCP/HFP/HID/…) and the profiles** — is still
the large majority of the ~82k upstream lines and remains to do.

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

## Slice 19 — what's here

**LE Secure Connections** pairing crypto, the counterpart to the LE Legacy
handshake from slice 14. Two pieces:

- **P-256 ECC in [`bumble-crypto`](bumble-crypto/)** — an `EccKey` (backed by
  the RustCrypto `p256` crate) porting upstream `crypto.EccKey`: `generate`,
  `from_private_key_bytes`, big-endian public-key coordinates (`public_x` /
  `public_y`), and ECDH (`dh`). The public keys and the Diffie-Hellman shared
  secret are pinned to values captured from upstream Python's `EccKey` in
  [`tests/ecc.rs`](bumble-crypto/tests/ecc.rs), and bad peer coordinates are
  rejected.
- **The SC JustWorks derivation in [`bumble-smp`](bumble-smp/)** — a `sc` module
  composing the symmetric functions exactly as upstream `smp.py` does:
  the responder confirm `Cb = f4(PKb, PKa, Nb, 0)`, `(MacKey, LTK) = f5(…)`,
  the DHKey checks `Ea`/`Eb = f6(…)`, and the 6-digit numeric value
  `g2(…) % 10⁶` — all pinned to a Python oracle, with careful attention to the
  little-endian byte order upstream uses on the wire and into the crypto
  functions. All nine remaining SMP PDUs (public key, DHKey check, keypress,
  and the five key-distribution PDUs) were added to the codec and oracle-pinned.

The whole exchange runs as a **two-party handshake** in
[`bumble-host/tests/smp_sc_pairing.rs`](bumble-host/tests/smp_sc_pairing.rs):
two peers each own a key pair, exchange public keys and nonces on the SMP
channel, each derives its DHKey from the *peer's* transmitted public key, the
initiator verifies the responder's `f4` confirm, both cross-verify the `f6`
DHKey checks, and both arrive at the **same LTK** — a genuine agreement, not a
self-comparison. Deferred: the full pairing state machine, Numeric
Comparison / passkey / OOB entry UX, key distribution over the wire, and
bonding storage.

## Slice 20 — what's here

The **session runtimes** for the two Classic codecs — the state machines that
drive a live exchange over the wire formats from slices 16–17. Both were
introduced as **sans-I/O** state machines: neither runtime touches a socket —
they consume and produce PDUs, and a caller relays the bytes. Slice 21 supplies
the Classic L2CAP channel state machine beneath them, and slice 22 binds both
runtimes to it. Each runtime is also verified independently over an in-memory
relay.

- **RFCOMM `Multiplexer`/`DLC` in [`bumble-rfcomm`](bumble-rfcomm/)** (module
  [`mux`](bumble-rfcomm/src/mux.rs)) — a synchronous port of the asyncio
  `Multiplexer`/`DLC`: session open on DLCI 0 (SABM/UA), per-channel DLC
  parameter negotiation (PN) + open (SABM/UA) + modem-status (MSC) exchange, and
  the credit-based flow-control engine (`process_tx`). Upstream's
  DLC-holds-Multiplexer back-reference is flattened into a single owner to fit
  Rust ownership; the wire behavior is identical.
- **SDP `Server`/`Client` in [`bumble-sdp`](bumble-sdp/)** (module
  [`service`](bumble-sdp/src/service.rs)) — a synchronous port of the asyncio
  `Server`/`Client`: a service-record database, UUID matching, attribute
  selection, and continuation-state chunking + reassembly on both sides, for all
  three query types (Service Search / Service Attribute / Service Search
  Attribute). The client drives the continuation loop through an `SdpTransport`,
  the same blanket-impl shape as the GATT client's `AttTransport`.

Both go beyond self-agreement: the RFCOMM open-handshake frames
([`tests/session.rs`](bumble-rfcomm/tests/session.rs)) and the SDP server
responses ([`tests/service.rs`](bumble-sdp/tests/service.rs)) are pinned
byte-for-byte to captures from the **real upstream state machines** driven over
the same relays, so the field-value choices (PN convergence layers, credit and
frame-size negotiation, MSC signals; SDP matching, selection and chunking) are
ground-truth, not just internally consistent. The two subtle paths are forced
explicitly: RFCOMM credit **exhaustion + replenishment** (a write past the
transmit budget stalls with data buffered, then drains once the peer grants
credits), and SDP **continuation across four round-trips** (a small server MTU
splits the answer; the client reassembles the identical record set it gets in
the single-PDU case). Deferred: RFCOMM retransmission and aggregate flow
control; async/event convenience APIs for both protocols.

## Slice 21 — what's here

The synchronous Classic connection-oriented channel runtime in
[`bumble-l2cap`](bumble-l2cap/) removes the missing layer below RFCOMM and SDP:

- Typed Connection Response, Configure Request/Response, and Disconnection
  Request/Response signaling frames, plus strict configuration-option TLV
  encoding and decoding. Their bytes are pinned to upstream Bumble's wire
  layout alongside the existing Connection Request oracle.
- `ChannelManager`, `ClassicChannel`, and `ClassicChannelSpec`: validated
  Classic PSM registration (including deterministic dynamic allocation),
  dynamic CID allocation, outgoing connect and incoming accept/refusal,
  bidirectional configuration with asymmetric MTU negotiation, basic-mode SDU
  delivery, and clean disconnect on both peers.
- A two-party in-memory relay test that opens a channel, verifies both peers'
  local/remote CIDs and negotiated MTUs, exchanges RFCOMM/SDP-shaped payloads in
  both directions, enforces the peer MTU, and closes both endpoints. A separate
  path verifies the spec result for an unsupported PSM.

The manager stays sans-I/O and emits/consumes complete `L2capPdu` values, so a
host can place it over an ACL data path without coupling L2CAP to a socket or
async runtime. Deferred: ACL fragmentation/reassembly, enhanced retransmission
mode, and LE credit-based channels.

## Slice 22 — what's here

The Classic protocol runtimes now operate over the slice-21 channel layer:

- `bumble-rfcomm::l2cap::L2capMultiplexer` binds a `Multiplexer` to an open
  Classic channel, derives RFCOMM's frame ceiling from the negotiated peer MTU,
  preserves one-frame-per-SDU ordering, parses received frames, and flushes
  state-machine responses. Its integration test performs session and DLC open,
  crosses a two-credit flow-control boundary, delivers 100 ordered bytes, and
  disconnects through two L2CAP peers.
- `bumble-sdp::l2cap::SdpL2capServer` parses request SDUs and returns server
  responses on the same channel. `L2capSdpTransport` plugs that exchange into
  the existing high-level `SdpClient`; transport failures are now explicit
  `ClientError::Transport` values instead of being impossible to represent.
  Its integration test performs service discovery and attribute retrieval over
  L2CAP and proves that the negotiated 48-byte MTU forces a continuation across
  multiple request/response SDUs.

The adapters remain executor-neutral: their caller drives the surrounding ACL
link through a callback, so they work with the in-process controller today and
can sit above future socket/USB transports without an API split.

## Slice 23 — what's here

[`bumble-at`](bumble-at/) ports upstream's AT parameter grammar and extracts the
protocol-neutral command/response models that HFP previously kept internally:

- `tokenize_parameters` and `parse_parameters` match `bumble/at.py`, including
  ignored unquoted spaces, quoted comma preservation, empty values, and nested
  parenthesized lists. The two upstream tests are ported 1:1.
- `AtCommand` recognizes extended set/test/read forms plus basic `ATA` and
  `ATD…` commands; `AtResponse` parses status and unsolicited response lines.
- `CommandStream` and `ResponseStream` preserve incomplete input across RFCOMM
  packets and emit every coalesced command or response once its AT delimiter
  arrives. Tests exercise both fragmentation and multiple messages per packet,
  as well as malformed nesting.

This is the codec boundary for the next HFP protocol slice; feature exchange,
indicator synchronization, call control, codec negotiation, and audio-link
orchestration remain in that profile layer.

## Slice 24 — what's here

[`bumble-hfp`](bumble-hfp/) adds paired, synchronous Hands-Free and Audio
Gateway service-level-connection state machines:

- Normative HF and AG feature bitmaps, open codec and HF-indicator identifiers,
  AG indicator descriptions/current values, and call-hold operation models.
- The mandatory SLC sequence (`BRSF`, `CIND=?`, `CIND?`, `CMER`) plus the
  feature-gated codec list (`BAC`), three-way calling (`CHLD=?`), and HF
  indicator (`BIND`, `BIND=?`, `BIND?`) exchanges. The HF validates response
  cardinality and parses advertised ranges; the AG validates commands and
  tracks the negotiated peer state.
- Minimal and full-feature tests pin the exact AT command transcript from
  upstream's `initiate_slc`. A third test opens Classic L2CAP and RFCOMM, opens
  a DLC, and completes the full optional HF↔AG SLC through every lower layer.

Both roles remain executor-neutral and expose byte queues, so the same state
machines work over the in-process stack and future real transports. Later
slices add call/event behavior and HFP SDP; synchronous audio links remain.

## Slice 25 — what's here

The HFP state machines continue after SLC completion instead of stopping at
negotiation:

- The HF serializes one pending command at a time, validates none/single/multiple
  response cardinality, exposes completed command results, and offers helpers
  for answer, dial, hang-up, indexed call hold, current-call queries, HF
  indicator reports, and codec selection. `+CLCC` results parse into typed call
  direction/status/mode/conference records.
- The AG handles those commands and emits typed application events. It also
  processes voice-recognition, volume, codec-connection, HF-indicator, and
  codec-selection commands while maintaining negotiated state.
- Unsolicited AG indicator updates, ring, speaker/microphone volume, caller ID,
  voice recognition, and codec proposals update HF state and produce typed
  events. Codec proposals complete through the normative `+BCS` / `AT+BCS`
  handshake on both peers.
- Direct tests cover the upstream `hfp_test.py` behavior families. The live
  integration continues beyond SLC on the existing RFCOMM DLC to verify answer,
  `+CIEV`, and codec negotiation through Classic L2CAP.

Synchronous SCO/eSCO audio setup remains the next profile dependency; the
controller still needs a complete audio data path.

## Slice 26 — what's here

HFP can now advertise and discover both profile roles through SDP:

- `make_hf_sdp_record` and `make_ag_sdp_record` produce the upstream five-
  attribute record shape: handle, service classes, L2CAP/RFCOMM descriptors and
  server channel, profile UUID/version, and role-specific supported features.
- Runtime HF/AG feature bits map to the distinct SDP feature assignments;
  advertising mSBC sets Wide Band Speech exactly as upstream does.
- `parse_hf_sdp_record` and `parse_ag_sdp_record` recover the RFCOMM channel,
  open profile version, and feature bitmap while rejecting a record for the
  opposite HFP role or malformed descriptor nesting.
- Tests mirror upstream's HF/AG discovery assertions and also register both
  records in `SdpServer`, query each UUID through `SdpClient`, and parse the
  returned attributes.

The next slice closes synchronous SCO/eSCO audio establishment and data routing
across HFP parameters, controller behavior, and host APIs.

## Slice 27 — what's here

HFP codec negotiation can now drive a live synchronous audio link:

- `DefaultCodecParameters` and `EscoParameters` port all eight HFP 1.8 section
  5.7 parameter sets (SCO CVSD D0/D1, eSCO CVSD S1-S4, and eSCO mSBC T1/T2),
  including enhanced setup/accept HCI command construction.
- The controller models SCO/eSCO request, accept, reject, complete, and
  disconnection over simplified LMP; Classic ACL teardown also removes its
  dependent synchronous links.
- `LocalLink` routes HCI synchronous-data packets bidirectionally with the
  destination controller's local handle. The host `Device` exposes Classic and
  synchronous connection state, incoming requests, packet inboxes, setup,
  accept, send, and disconnect APIs.
- Two-party tests establish CVSD/eSCO directly at the controller boundary and
  an mSBC link through the host plus HFP preset, exchange audio payloads, test
  rejection, and verify both independent and ACL-cascaded teardown.

Media encoding/decoding and real controller transports remain separate future
work; this slice provides the profile-to-HCI connection and packet boundary.

## Slice 28 — what's here

The remaining synchronous `hfp.py` protocol surface is filled in:

- Open normative models now include response-hold, call-setup/call-held,
  voice-recognition, and CME-error values. `CallLineIdentification` parses and
  serializes the complete optional subaddress, alpha, and validity fields.
- The AG handles `CMEE`, `CCWA`, `BIA`, and `CLIP`, tracks their enabled state,
  emits extended `+CME ERROR` responses when requested, and exposes in-band
  ringtone and typed caller-ID helpers.
- HF events preserve typed caller-ID and voice-recognition data. Public helpers
  match upstream's reject-incoming-call, terminate-call, and audio-connection
  request methods; unknown optional unsolicited responses are safely ignored as
  upstream does.
- Tests cover batched commands in one RFCOMM write, all control-state changes,
  operation-not-supported CME output, rich `+CLIP` round trips, enhanced voice
  state, optional `+BSIR`, and the three public helpers.

The remaining HFP differences are executor integration (the Rust port is
synchronous) and media codecs, rather than missing HF/AG command behavior.

## Slice 29 — what's here

The first A2DP dependency is a complete transport-neutral AVDTP signaling
codec:

- All 38 message forms exercised by upstream `avdtp_test.py` are represented:
  discover; get/get-all/set/reconfigure configuration; open/start/close/
  suspend/abort; security control; general reject; and delay report.
- Stream endpoint descriptors and capability TLVs round-trip, including typed
  media-codec capabilities and lossless unknown categories/signals.
- Every message payload is pinned to its exact AVDTP bytes, adding stronger
  coverage than upstream's object-only round trip.
- `encode_pdus` and `MessageAssembler` support single and fragmented signaling
  packets with transaction labels and MTU limits. Empty/truncated frames,
  malformed capability lengths, mismatched fragments, and oversized messages
  fail safely instead of panicking or tearing down a channel.

The next A/V slice builds protocol transactions and stream endpoint lifecycle
on this codec and binds it to Classic L2CAP PSM `0x0019`.

## Slice 30 — what's here

AVDTP signaling now drives real stream endpoint state:

- `session::Session` registers source/sink endpoints, advertises live in-use
  flags, returns capabilities/configuration, and handles every upstream command
  through configuration, reconfiguration, open, start, suspend, close, abort,
  security control, and delay reporting.
- Transitions enforce the AVDTP state model and produce the matching reject
  shape/error (`BAD_ACP_SEID`, `SEP_IN_USE`, or `BAD_STATE`). Multi-endpoint
  start/suspend validation is atomic, so a later invalid SEID cannot partially
  mutate earlier streams.
- `l2cap::L2capSession` assigns 4-bit transaction labels, sends MTU-fragmented
  messages, reassembles channel input, dispatches commands, returns responses
  on the original label, and queues initiator-side results.
- A two-party Classic L2CAP test runs discovery, capability exchange, a
  configuration deliberately larger than the 48-byte minimum MTU, then the
  open → start → suspend → close lifecycle while asserting responder state.

The next layer is A2DP codec negotiation and RTP media packets over the AVDTP
media channel.

## Slice 31 — what's here

A2DP can now express and negotiate its codec-specific capabilities:

- `CodecType` preserves standard and unknown codec identifiers. SBC models all
  sampling-frequency, channel-mode, block-length, subband, allocation, and
  bitpool fields; AAC models object type, 12 sampling frequencies, channels,
  VBR, and the 23-bit bitrate.
- Vendor-specific information implements the A2DP little-endian 32-bit vendor
  and 16-bit codec header. The registered Opus form exposes channel mode,
  10/20 ms frame size, and 48 kHz support.
- The upstream SBC `3fff0235`, AAC `f0018c83e800`, and Opus `92` fixtures are
  byte-pinned in both directions. Truncated records and an overflowing AAC
  bitrate return errors rather than indexing or truncating silently.
- `MediaCodecInformation` dynamically selects SBC/AAC/Opus/vendor/raw forms and
  creates an AVDTP audio `MEDIA_CODEC` capability without losing bytes.

The next slice ports RTP framing and the SBC/AAC/Opus frame parser/packet-source
boundary used by the AVDTP media channel.

## Slice 32 — what's here

[`bumble-rtp`](bumble-rtp/) supplies the shared A/V media packet boundary:

- RTP v2 fixed headers, marker/payload type, sequence number, timestamp, SSRC,
  and up to 15 CSRC identifiers serialize and parse in network byte order.
- Header extensions preserve their 16-bit profile and word-counted data.
  Standard trailing padding is validated, removed from the exposed payload,
  and restored byte-for-byte on serialization.
- Tests pin a Bumble-style A2DP packet, a packet containing two CSRCs plus an
  extension and padding, and truncated CSRC/extension inputs. This also fixes
  the upstream parser's CSRC-offset typo by advancing four bytes per entry.

The next slice uses this packet type for the SBC/AAC/Opus A2DP parsers and
packet sources.

## Slice 33 — what's here

The required A2DP SBC media boundary now runs on `bumble-rtp`:

- `SbcFrame::parse` validates the sync word, decodes sampling frequency,
  blocks, channel/allocation mode, subbands and bitpool, computes the spec frame
  length with ceiling division, and rejects truncated input.
- Frame sample count, bitrate, duration, and concatenated-stream parsing are
  exposed without an async runtime.
- `packetize_sbc` aggregates up to 15 complete frames under the negotiated MTU,
  emits the A2DP one-byte frame-count header, and advances wrapping RTP sequence
  and sample-clock timestamps without fragmenting a frame.
- Tests reproduce upstream's `9c800800` fixture and 23-byte-MTU packet-source
  case. They also assert the final buffered frame is emitted; upstream's async
  generator currently loses that frame when the input ends without another
  frame triggering a flush.

AAC ADTS/LATM and Ogg Opus parsing/packetization are next.

## Slice 34 — what's here

AAC media frames now cross the same parser-to-RTP boundary:

- `AacFrame` validates ADTS sync/layer/frequency/frame length and exposes the
  profile, sampling frequency, channel configuration, 1024-sample duration,
  and raw access unit.
- The simple LATM writer implements the upstream AudioMuxElement/
  StreamMuxConfig bit layout, including sampling-frequency index, channel
  configuration, GA config, payload-length bytes, and byte alignment.
- RTP packetization emits one AAC access unit per packet and advances wrapping
  sequence numbers and timestamps by 1024 samples.
- The upstream `fff0100001a000` header plus six-byte payload produces the exact
  `20001200000030…` LATM fixture. Stream parsing, timestamp progression,
  invalid sync words, and truncated declared lengths are covered.

Ogg Opus parsing and RTP packetization are next.

## Slice 35 — what's here

The third upstream A2DP media family is complete:

- `parse_ogg_opus` validates Ogg capture/version, selects the first logical
  bitstream, enforces page sequence numbers, handles continuation/lacing
  segments, and recognizes `OpusHead` and `OpusTags` before emitting audio.
- Channel count from `OpusHead` maps to mono/stereo state. Audio packets carry
  the upstream 20 ms / 48 kHz defaults and preserve their encoded bytes.
- RTP packetization emits one complete Opus frame per payload with the A2DP
  `01` header, wrapping sequence numbers, and 960-sample timestamp increments.
- Tests reproduce upstream's one-page fixture, add a second page to prove
  sequence/timestamp continuity, and reject truncated pages, bad capture
  patterns, and sequence gaps. The parser also corrects upstream's accidental
  assignment of the channel mode into its packet counter.

All three upstream A2DP media parser/packet-source families are now present;
the next slice carries their RTP packets over a live AVDTP media channel.

## Slice 36 — what's here

A2DP media now crosses a real channel rather than stopping at packet creation:

- `transport::L2capMediaTransport` binds to an open Classic channel, records
  the negotiated peer MTU, serializes `bumble-rtp::MediaPacket` SDUs, and parses
  received SDUs into a typed packet inbox.
- RTP packets larger than the peer MTU are rejected before entering L2CAP,
  keeping the no-media-fragmentation contract explicit.
- The integration opens an AVDTP PSM `0x0019` channel between two
  `ChannelManager`s, parses three SBC frames, aggregates them under the MTU,
  sends them source-to-sink, and verifies exact typed packet equality after the
  complete RTP → L2CAP → RTP round trip.

The remaining A2DP work is high-level discovery/codec selection/stream
orchestration and profile SDP; its signaling, state, codecs, media parsers,
packetizers, RTP, and live channel transport are now present.

## Slice 37 — what's here

A2DP roles can now advertise and discover their profile endpoints:

- Source and sink builders reproduce upstream's five attributes: record
  handle, Public Browse Root, role service class, L2CAP/AVDTP protocol
  descriptors with PSM `0x0019`, and the Advanced Audio Distribution profile
  descriptor.
- AVDTP and A2DP profile versions are open `ProfileVersion` values with common
  1.2/1.3/1.4 constants.
- `parse_sdp_record` distinguishes source and sink roles and validates the
  complete L2CAP PSM, AVDTP UUID/version, and A2DP profile descriptor rather
  than accepting a service-class match alone.
- Both records are registered, queried by UUID, and parsed through the existing
  continuation-aware SDP client/server runtime; missing protocol descriptors
  and unrelated service classes are rejected.

The remaining A2DP slice is a high-level initiator that selects a compatible
remote codec/SEP and drives signaling plus media-channel setup as one operation.

## Slice 38 — what's here

The synchronous A2DP profile surface is now connected end-to-end:

- `profile::A2dpClient` owns transaction request/response driving through a
  caller-supplied executor-neutral callback, with a bounded response watchdog.
- Discovery fetches every remote SEP and its complete capabilities. Sink
  selection requires an unused sink, media transport, matching codec type, and
  matching vendor/codec identifiers for non-A2DP codecs.
- Stream creation sends the selected media transport + codec configuration and
  completes set-configuration, open, and start. Typed handles subsequently
  suspend, restart, and close the remote stream.
- A live two-party Classic L2CAP test registers an SBC sink, discovers and
  selects it, and verifies the responder transitions through STREAMING, OPEN,
  STREAMING, and IDLE as the high-level client operates it.

This completes the core synchronous `a2dp.py` behavior family. Work now moves
to the AVRCP dependency stack (`avc.py`, `avctp.py`, then `avrcp.py`).

## Slice 39 — what's here

The first AVRCP dependency is a complete transport-neutral AV/C frame boundary:

- Generic commands and responses preserve open category, subunit type, opcode,
  and operand values while distinguishing command types from response codes.
- Standard, one-byte-extended, and two-byte-extended subunit IDs parse and
  serialize, including upstream's ID 7 and ID 260 fixtures. Reserved encodings
  and unsupported extended subunit types fail explicitly.
- Vendor-dependent frames preserve the 24-bit company ID and arbitrary payload;
  the upstream `0148000019581000000103` command is byte-exact.
- Panel pass-through frames support pressed/released state, open operation IDs,
  and up to 255 operation-data bytes. Play press matches the upstream AVRCP
  fixture; non-empty data uses the spec-correct offset that upstream currently
  parses incorrectly.

AVCTP fragmentation/reassembly and its Classic L2CAP binding are next.

## Slice 40 — what's here

AV/C frames can now cross the AVRCP transport:

- `Message` models 4-bit transaction labels, command/response, IPID, 16-bit PID,
  and lossless payloads. Commands cannot set IPID and all bit fields are
  validated.
- MTU-aware encoding supports single and start/continue/end packets, retaining
  the PID and response flags on every fragment as upstream expects.
- `MessageAssembler` validates label, PID, command/response, IPID, fragment
  count, and ordering while safely dropping empty/truncated/mismatched input.
- `L2capProtocol` binds AVCTP PSM `0x0017` to an open Classic channel, queues
  registered-PID traffic, and automatically answers unsupported command PIDs
  with IPID. Tests pin upstream's assembler vectors and force an 80-byte
  fragmented command through two live channel managers.

The next slice begins AVRCP's vendor-dependent PDU assembler and typed command,
event, and response catalog on this transport.

## Slice 41 — what's here

AVRCP's protocol-specific envelope now sits above AV/C and AVCTP:

- `VendorPdu` implements the exact `pdu_id`, packet type, big-endian parameter
  length, and parameter byte layout, preserving unknown PDU identifiers.
- `PduAssembler` ports upstream's independent single/start/continue/end state
  machine, including a new single/start replacing an unfinished PDU and stray
  or mismatched continuation fragments being discarded safely.
- Malformed lengths reset assembly and return a typed error rather than relying
  on unchecked Python indexing. MTU-oriented outbound fragmentation generates
  the matching packet sequence and round-trips through the assembler.
- AV/C helpers wrap and extract Bluetooth SIG company `0x001958` vendor data on
  the panel subunit. The upstream GET_CAPABILITIES command envelope is
  byte-exact, and AVRCP's AVCTP PID `0x110E` is exposed for the runtime layer.

The next slice ports the complete typed command catalog before adding typed
responses/events and the controller/target runtime.

## Slice 42 — what's here

Every command class registered by upstream `bumble.avrcp.Command` is now typed:

- All 22 command forms cover capabilities, player application settings,
  element metadata, playback/volume/notifications, addressed and browsed
  players, folder browsing, search, play, and now-playing insertion.
- Counted parallel setting fields are represented as paired Rust values;
  counted identifiers, 64-bit UIDs, and length-prefixed UTF-8 strings reject
  truncation, overflow, invalid UTF-8, and trailing bytes.
- Open newtypes retain vendor/future enum values, while unknown PDU IDs retain
  their parameter bytes losslessly. The two continuing-response identifiers
  that upstream declares without typed command classes therefore remain usable.
- All upstream test instances were serialized by the real Python Bumble and
  pinned byte-for-byte in Rust, including its unusual little-endian attribute
  IDs for `GetItemAttributes`.

Typed responses and notification events are next.

## Slice 43 — what's here

Every event class registered by upstream `bumble.avrcp.Event` is now typed:

- Playback status/position, track UID, player-setting changes, now-playing and
  available-player changes, addressed-player identity, UID counter, and volume
  encode and parse through one exhaustive event enum.
- The player-application-setting event uses paired attribute/value entries,
  retaining unknown values without sacrificing the named standard constants.
- Event IDs with no upstream typed class are preserved as generic events, while
  known events require their exact parameter lengths and reject truncation or
  trailing data.
- All 9 instances in upstream's parametrized event test are byte-for-byte
  pinned against the Python serializer.

The complete response and browseable-item catalog is next.

## Slice 44 — what's here

The AVRCP wire catalog is now complete for every upstream response class:

- All 23 regular response forms cover capabilities, player settings and text,
  play status, media metadata, volume and notifications, player selection,
  browsing, search, item playback, and now-playing insertion.
- Browseable media-player, folder, and media-element items preserve their
  length-delimited envelopes, open identifiers, 128-bit little-endian feature
  masks, nested attributes, character sets, and display strings.
- AV/C rejected and not-implemented forms are explicit variants, while unknown
  response PDU and browse-item types remain lossless. Every nested count and
  length is bounded and malformed UTF-8 or trailing data is rejected.
- All 23 upstream parametrized response instances are byte-for-byte pinned
  against the Python serializer, including the three-item browse response.

With commands, responses, events, and both fragmentation layers complete, the
next slice wires the controller/target transaction runtime over live AVCTP.

## Slice 45 — what's here

The typed catalog now participates in complete controller/target transactions:

- `Runtime` allocates and recycles the 16 AVCTP transaction labels, matches
  typed responses to pending PDU IDs, keeps notification registrations pending
  across `INTERIM`, and releases them on `CHANGED` or any other final response.
- Incoming AV/C vendor frames enforce the Bluetooth SIG company, panel subunit,
  consistent transaction label/type across fragments, and accepted response
  codes. Unsupported commands produce typed AVRCP rejection; unsupported AV/C
  opcodes produce `NOT_IMPLEMENTED` without losing their operands.
- `Delegate` is the target extension point. `BasicDelegate` ports upstream's
  capability, volume, play-status, player-setting, play-item, notification, and
  pass-through behavior with inspectable in-memory state.
- Live tests force a large capability response through AVRCP PDU fragmentation
  and AVCTP fragmentation over a 48-byte Classic L2CAP channel, then verify
  notification lifecycle, pass-through press handling, rejection, and label
  exhaustion.

AVRCP SDP records/discovery and the separate browsing channel are next.

## Slice 46 — what's here

AVRCP service advertisement and discovery now close the profile:

- Controller records advertise both A/V Remote Control (`0x110E`) and Controller
  (`0x110F`); target records advertise Target (`0x110C`). Both carry the public
  browse root, AVCTP PSM/version, AVRCP profile/version, handle, and the exact
  role-specific supported-feature mask.
- Controller and target feature newtypes expose every upstream feature bit.
  Enabling browsing adds the upstream additional protocol descriptor for AVCTP
  browsing PSM `0x001B`; records without it retain the six-attribute shape.
- Strict parsers validate role UUIDs, L2CAP/AVCTP descriptors, primary PSM,
  profile UUID, integer widths, and required fields.
- Discovery helpers run through `SdpClient`; tests add both records to a real
  `SdpServer`, force continuation with a small response budget, and recover the
  original typed records.

This completes upstream `avrcp.py` in the synchronous Rust architecture. Work
now moves to HID and the remaining unported modules.

## Slice 47 — what's here

Upstream `hid.py` is now complete in `bumble-hid`:

- `Message` parses and serializes handshake, control, get/set report, get/set
  protocol, and data transactions. Unknown message types are lossless, while
  known fixed-size forms reject truncation and trailing bytes.
- The exact wire details are pinned: report-type bits, GET_REPORT's buffer flag
  and little-endian size, protocol-mode bit, all handshake results, and suspend,
  exit-suspend, and virtual-cable-unplug control values.
- `DeviceRuntime` dispatches report/protocol callbacks and maps every upstream
  return status to the proper handshake. Successful GET_REPORT data includes
  the report ID and observes upstream's strict peer-MTU check.
- `HostRuntime` exposes the host commands and parses handshake/control/interrupt
  results. `L2capTransport` binds the paired control PSM `0x0011` and interrupt
  PSM `0x0013`, validates role/MTU/state, and carries live two-party traffic.

The port now advances through the remaining partial core layers and unstarted
profile families rather than stopping at the Classic media stack.

## Slice 48 — what's here

Upstream `codecs.py` is now complete in `bumble-codecs`:

- Bounded `BitReader` and `BitWriter` handle aligned and unaligned reads,
  byte blocks, skipping, zero-width operations, and up to 32-bit fields without
  the Python implementation's large-integer cache dependency.
- The MPEG-4 LATM hierarchy ports GA-specific config, audio-specific config,
  stream-mux config, audio-mux elements, extended object/frequency parsing,
  payload-length escape bytes, optional other-data skipping, and byte alignment.
- A simple AAC-LC constructor supports every standard sampling-frequency index
  and arbitrary channel configuration. LATM payloads at the 255-byte escape
  boundary and beyond round-trip exactly.
- Upstream's long captured RTP/LATM payload parses and converts to the exact
  seven-byte-header ADTS oracle, with frame-size overflow rejected explicitly.

The next work targets the remaining partial foundational protocols before the
large GATT profile catalog.

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
│   ├── tests/scenario.rs      # end-to-end advertising→scan→report scenario
│   └── tests/synchronous.rs   # slice-27 SCO/eSCO lifecycle and data routing
├── bumble-l2cap/              # slice-4 codec + slice-21 Classic channel runtime
│   ├── src/{lib,classic}.rs
│   ├── tests/acceptance.rs    # ported l2cap_test.py codec cases (oracle-pinned)
│   └── tests/classic_channels.rs # two-party Classic channel lifecycle
├── bumble-att/                # slice-5 ATT protocol PDU codec crate
│   ├── src/lib.rs
│   └── tests/acceptance.rs    # ported gatt_test.py ATT cases (oracle-pinned)
├── bumble-crypto/             # slice-6 SMP crypto toolbox + slice-19 P-256 ECC
│   ├── src/lib.rs             # symmetric functions + EccKey (P-256 ECDH)
│   ├── tests/vectors.rs       # ported smp_test.py spec/RFC vectors
│   └── tests/ecc.rs           # P-256 public keys + ECDH pinned to oracle
├── bumble-gatt/               # slice-9 GATT/ATT server + slice-18 GATT client
│   ├── src/lib.rs             # AttServer, GattServer
│   ├── src/client.rs         # GattClient (slice 18)
│   ├── tests/end_to_end.rs   # attribute write/read across the full stack
│   └── tests/client.rs       # two-party client↔server discovery/read/write/subscribe
├── bumble-host/               # slice-10 Host/Device glue crate
│   ├── src/lib.rs
│   ├── tests/gatt_over_host.rs # full LE lifecycle via the Device API
│   ├── tests/smp_pairing.rs    # two-party LE Legacy JustWorks handshake
│   ├── tests/smp_sc_pairing.rs # two-party LE Secure Connections handshake (slice 19)
│   └── tests/synchronous_audio.rs # HFP mSBC over host/controller (slice 27)
├── bumble-smp/                # slice-14 SMP codec + legacy pairing + slice-19 SC
│   └── src/lib.rs             # wires bumble-crypto; sc:: JustWorks derivation
├── bumble-sdp/                # codec + runtime + slice-22 L2CAP binding
│   ├── src/{lib,pdu}.rs       # DataElement + ServiceAttribute + SdpPdu
│   ├── src/service.rs         # SdpServer + SdpClient (continuation runtime, slice 20)
│   ├── src/l2cap.rs           # live Classic channel server/client transport
│   ├── tests/acceptance.rs    # ported sdp_test.py cases (oracle-pinned)
│   ├── tests/service.rs       # client↔server, responses pinned to upstream (slice 20)
│   └── tests/l2cap_binding.rs # continuation over negotiated Classic L2CAP
├── bumble-rfcomm/             # codec + session runtime + slice-22 L2CAP binding
│   ├── src/lib.rs             # RfcommFrame + compute_fcs + MCC PN/MSC
│   ├── src/mux.rs             # Multiplexer + DLC credit-flow state machine (slice 20)
│   ├── src/l2cap.rs           # Multiplexer bound to a live Classic channel
│   ├── tests/acceptance.rs    # ported rfcomm_test.py frame check (oracle-pinned)
│   ├── tests/session.rs       # two-party session, handshake pinned to upstream (slice 20)
│   └── tests/l2cap_binding.rs # session/DLC/data/disconnect over Classic L2CAP
├── bumble-at/                 # slice-23 AT/HFP command and response parsing
│   ├── src/lib.rs             # parameters, models, incremental stream parsers
│   └── tests/acceptance.rs    # upstream AT tests + HFP framing cases
├── bumble-hfp/                # slice-24 HF/AG service-level connection
│   ├── src/lib.rs             # features, events, paired HFP state machines
│   ├── src/sdp.rs             # slice-26 HF/AG records and discovery parsing
│   ├── src/audio.rs           # slice-27 SCO/eSCO presets + HCI commands
│   ├── tests/slc.rs           # minimal/full transcript-pinned negotiation
│   ├── tests/post_slc.rs      # call control, events, indicators, codec flow
│   ├── tests/extended_control.rs # slice-28 models, controls, typed metadata
│   ├── tests/sdp.rs           # records and client/server discovery
│   └── tests/rfcomm_slc.rs    # SLC over RFCOMM over Classic L2CAP
├── bumble-avdtp/              # slice-29 A/V distribution transport codec
│   ├── src/lib.rs             # messages, capabilities, PDU fragmentation
│   ├── src/session.rs         # slice-30 endpoint and stream state machine
│   ├── src/l2cap.rs           # transaction runtime over Classic L2CAP
│   ├── tests/acceptance.rs    # 38 exact payloads + malformed PDU coverage
│   ├── tests/session.rs       # lifecycle, errors, atomic multi-SEP commands
│   └── tests/l2cap_binding.rs # fragmented signaling over live channels
├── bumble-a2dp/               # slice-31 Advanced Audio Distribution Profile
│   ├── src/lib.rs             # SBC/AAC/vendor Opus capability models
│   ├── src/media.rs           # slice-33 SBC parser + RTP aggregation
│   ├── src/transport.rs       # slice-36 RTP over Classic L2CAP
│   ├── src/sdp.rs             # slice-37 source/sink records + discovery
│   ├── src/profile.rs         # slice-38 discovery/selection/lifecycle client
│   ├── tests/codecs.rs        # upstream exact vectors + invalid inputs
│   ├── tests/media.rs         # SBC/AAC/Opus fixtures and packet sources
│   ├── tests/l2cap_media.rs   # source→sink RTP over live AVDTP channel
│   ├── tests/profile.rs       # live high-level stream orchestration
│   └── tests/sdp.rs           # source/sink discovery through SDP runtime
├── bumble-rtp/                # slice-32 RTP media packet codec
│   ├── src/lib.rs             # header, CSRC, extension, payload, padding
│   └── tests/packets.rs       # exact, full-featured, and malformed packets
├── bumble-avc/                # slice-39 AV/C frame codec for AVRCP
│   ├── src/lib.rs             # generic/vendor/pass-through frames
│   └── tests/frames.rs        # upstream exact vectors + malformed inputs
├── bumble-avctp/              # slice-40 AV/C transport over Classic L2CAP
│   ├── src/lib.rs             # messages, fragmentation, L2CAP protocol
│   └── tests/protocol.rs      # upstream assembler + live PID/IPID flows
├── bumble-avrcp/              # slice-41 AVRCP vendor-PDU foundation
│   ├── src/lib.rs             # PDU codec/assembler and AV/C/AVCTP envelope
│   ├── src/command.rs         # slice-42 complete typed command catalog
│   ├── src/event.rs           # slice-43 complete typed event catalog
│   ├── src/response.rs        # slice-44 responses + browseable item codec
│   ├── src/runtime.rs         # slice-45 controller/target transaction engine
│   ├── src/sdp.rs             # slice-46 controller/target records + discovery
│   ├── tests/commands.rs      # 22 Python-oracle parameter vectors
│   ├── tests/events.rs        # 9 Python-oracle notification vectors
│   ├── tests/responses.rs     # 23 Python-oracle response vectors
│   ├── tests/runtime.rs       # live AVCTP/L2CAP + notifications/pass-through
│   └── tests/sdp.rs           # role records + SDP client/server discovery
├── bumble-hid/                # slice-47 Human Interface Device Profile
│   ├── src/lib.rs             # HIDP codec + host/device callback dispatch
│   ├── src/l2cap.rs           # paired control/interrupt Classic transport
│   ├── tests/protocol.rs      # exact messages, callbacks, malformed inputs
│   └── tests/l2cap.rs         # live host/device report flows
├── bumble-codecs/             # slice-48 common media bitstreams/codecs
│   ├── src/lib.rs             # bit I/O + MPEG-4 LATM AAC and ADTS conversion
│   └── tests/codecs.rs        # upstream fixture + length-boundary round trips
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
