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
| 4+21. L2CAP codec + Classic and LE connection-oriented channel runtimes | `bumble-l2cap` | ✅ 38/38 tests green |
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
| 49. Complete ATT wire PDU catalog | `bumble-att` | ✅ all upstream subclasses typed |
| 50. GATT multiple reads and atomic queued writes | `bumble-gatt` | ✅ fixed/variable + prepare/execute green |
| 51. Pairing key JSON/memory stores and resolving-list extraction | `bumble` | ✅ atomic persistence green |
| 52. Complete GATT database definitions and access security | `bumble-gatt` | ✅ include/secondary/descriptor/permission green |
| 53. Bearer-aware dynamic GATT value accessors | `bumble-gatt` | ✅ read/write/error callbacks green |
| 54. Typed GATT characteristic and proxy adapters | `bumble-gatt` | ✅ upstream adapter vectors green |
| 55. Complete Python 3.14 packed-value compatibility | `bumble-gatt` | ✅ native/half/complex oracle green |
| 56. Complete L2CAP signaling control-frame catalog | `bumble-l2cap` | ✅ all upstream dataclasses typed |
| 57. LE credit-based channel segmentation and credit engine | `bumble-l2cap` | ✅ MTU/MPS/credit/reassembly green |
| 58. Paired LE credit-based channel manager runtime | `bumble-l2cap` | ✅ connect/transfer/replenish/disconnect green |
| 59. HCI ACL fragmentation and host reassembly | `bumble-hci`, `bumble-host` | ✅ buffer-boundary end-to-end green |
| 60. HCI ACL completed-packet flow-control queue | `bumble-host`, `bumble-controller` | ✅ bounded in-flight window green |
| 61. Enhanced credit-based multi-channel and reconfigure runtime | `bumble-l2cap` | ✅ five-channel + refusal matrix green |
| 62. Enhanced Retransmission Mode control fields and data engine | `bumble-l2cap` | ✅ loss/busy/window/timer paths green |
| 63. Live Classic L2CAP ERTM negotiation and transport | `bumble-l2cap` | ✅ upstream MTU matrix + FCS green |
| 64. SMP pairing policy, OOB data, and CTKD foundation | `bumble-smp` | ✅ method matrix + upstream vectors green |
| 65. Live Legacy SMP session and host encryption transition | `bumble-smp`, `bumble-host` | ✅ JustWorks/passkey/OOB + failure paths green |
| 66. Live SC JustWorks and Numeric Comparison session | `bumble-smp`, `bumble-host` | ✅ ECDH/confirm/DHKey-check/encryption green |
| 67. SC Passkey and OOB association models | `bumble-smp` | ✅ 20 rounds + C/R validation green |
| 68. Encrypted SMP key distribution and bond persistence | `bumble-smp`, `bumble` | ✅ responder-first phase 3 + stores green |
| 69. CT2 negotiation and bonded Security Request reconnect | `bumble-smp`, `bumble-host` | ✅ h7 + live reuse green |
| 70. IRK address resolution and controller privacy offload | `bumble-smp`, `bumble-controller`, `bumble-host` | ✅ identity→RPA reconnect green |
| 71. CSRK authenticated ATT signed writes and persistent counters | `bumble-att`, `bumble-gatt`, `bumble-host` | ✅ CMAC/replay/restart green |
| 72. Multi-connection LE pairing manager | `bumble-smp`, `bumble-host` | ✅ concurrent + live manager green |
| 73. Encrypted SMP-over-BR/EDR CTKD orchestration | `bumble-smp`, `bumble-controller`, `bumble-host` | ✅ h6/h7 + CID 0x0007 green |
| 74. High-level LE advertise, scan, connect, and disconnect API | `bumble-host` | ✅ no raw-HCI lifecycle green |
| 75. H4 framing and file/TCP/UDP/Unix transports | `bumble-transport` | ✅ fragmented streams + socket loopbacks green |
| 76. Transport-spec dispatch, serial, and raw PTY endpoints | `bumble-transport` | ✅ metadata + PTY loopback green |
| 77. WebSocket client/server HCI transport | `bumble-transport` | ✅ binary/coalesced loopback green |
| 78+. Remaining modules… | — | planned |

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
| `keys.py` (0.4k) | `bumble::keys` | ✅ | Complete `PairingKeys` / `Key` JSON model, replacement-style memory store, namespaced JSON store with upstream merge/default-namespace semantics and atomic replacement, delete/get/get-all/delete-all, platform data-path selection, and IRK resolving-list extraction to typed addresses. Rust uses synchronous filesystem calls rather than wrapping them in nominal async methods. |
| `utils.py` (0.5k) | `bumble::util` (+ spread) | ✅ | Generic helpers (`bit_flags_to_strings`, `name_or_number`); `crc_16` lives in `bumble-l2cap`; the open-enum/flag pattern is realized as newtypes throughout. The asyncio event infra (`EventEmitter`/`AsyncRunner`/`FlowControlAsyncPipe`) is **N/A** for this synchronous port. |
| `colors`, `logging`, `helpers`, `snoop`, `decoder` | — | N/A | Debug/logging tooling with idiomatic Rust equivalents rather than library surface: `colors` (ANSI), `logging` (→ `log`/`tracing`), `helpers.PacketTracer` (debug trace), `snoop` (BTSnoop/pcap capture). `decoder.py` is a **G.722 audio codec** — it belongs with the audio subsystem, not core. |

### HCI, controller & link — 🟡 HCI codec complete (full catalog, oracle-pinned); controller/link behavior partial
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `hci.py` (8.3k) | `bumble-hci` | ✅ | **Full typed catalog: 196 command op codes + 81 event / LE-meta sub-event codes**, generated from upstream's declarative field specs by [`tools/hcigen`](bumble-hci/tools/hcigen/) and **byte-pinned against real Python Bumble** (320 oracle tests). Framing (Command/Event/ACL/SCO/ISO), `Command_Complete` with typed `ReturnParameters`, the open-enum `Generic` tail, and upstream-equivalent ACL/L2CAP fragmentation/reassembly with PB-flag, length, continuation, handle, and overflow validation. Two phys-derived array commands and the two nested-report events are hand-written; everything else is generated. |
| `controller.py` (2.8k) | `bumble-controller` | 🟡 | **Full command surface**: every command upstream's `controller.py` handles (93, via the generated [`command_surface`](bumble-controller/src/command_surface.rs) table) gets a reply of the matching HCI shape — Command Complete + SUCCESS for config/set commands, Command Status for operations completing via a later event, and the spec-correct "Unknown HCI Command" for anything upstream also doesn't handle. **Functionally simulated**: LE advertising/scanning, connection establishment, IRK resolving-list offload with identity-targeted RPA connections, ACL routing with PB/BC preservation and Number Of Completed Packets flow events, disconnection, the read commands (`Read_BD_ADDR`/`Read_Local_Name`/`LE_Read_Buffer_Size`/`LE_Read_Local_Supported_Features`/`LE_Rand`), per-connection `LE_Set_Data_Length`/`LE_Set_PHY` (with follow-up meta events), and — via LL control-PDU exchange over the link — **encryption start**, **remote-features**, and **CIS establishment**. Also **classic (BR/EDR)** connection/name/features and SCO/eSCO request/accept/reject/disconnect with synchronous-data routing. Other read commands are acknowledged SUCCESS **without a synthesized payload** (a documented stub, not a full read). Deferred: LTK verification, ISO data-path streaming, remote-version exchange, extended/periodic advertising, and classic auth/encryption/role-switch sub-flows. |
| `link.py` (0.15k) | `bumble-controller` | 🟡 | In-process **synchronous** `LocalLink` with LL-control, simplified LMP, ACL, and SCO/eSCO routing. Deferred: serialized over-the-air PDUs and async scheduling. |
| `ll.py` (0.2k) | `bumble-controller` | 🟡 | Advertising/connection PDUs modeled as in-process structs, not serialized LL PDUs. Control PDUs (`EncReq`, `FeatureReq`/`PeripheralFeatureReq`/`FeatureRsp`, `TerminateInd`) are exchanged between controllers via `LocalLink::pump_ll` to drive the encryption-start, remote-features, and CIS-establishment (`CisReq`/`CisRsp`/`CisInd`) flows. |
| `host.py` (2.1k) | `bumble-host` | 🟡 | `Device` glue (ATT↔L2CAP↔ACL sequencing + pairing transport), controller-buffer-sized outbound ACL fragmentation, per-connection inbound reassembly, a global/per-handle `DataPacketQueue` driven by Number Of Completed Packets, LE/Classic encryption, resolving-list programming, Classic and LE L2CAP, plus synchronous audio APIs. The host pump advances LL control and HCI/ACL traffic. Deferred: direct LE signaling-manager integration and the broader host feature set. |
| `device.py` (7.0k) | `bumble-host` | 🟡 | High-level legacy LE advertising, active/passive scanning with typed report collection, identity/RPA-aware LE connection setup, peer/role state, and disconnect now run through `Device` without raw HCI. GATT/ATT, SMP, Classic, and synchronous operations are also exposed by the same type. Deferred: extended/periodic advertising, multi-connection ownership, and listener/async conveniences. |
| `lmp.py` (0.4k) | `bumble-controller::lmp` | 🟡 | Classic Link Manager Protocol PDUs modeled as in-process structs (`HostConnectionReq`/`Accepted`, `NameReq`/`NameRes`, `FeaturesReq`/`FeaturesRes`, synchronous request/accept/reject, `Detach`) driving the classic connection/name/features/SCO-eSCO flows via `LocalLink::pump_classic`. The role-switch / authentication / encryption LMP sub-dance is simplified away. |

### L2CAP
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `l2cap.py` (3.1k) | `bumble-l2cap` | 🟡 | PDU + complete typed upstream signaling-frame catalog + FCS, synchronous Classic connection-oriented channels, and paired LE CoC runtimes. Classic covers dynamic PSM/CID allocation, Connection/Configure/Disconnection, MTU negotiation/refusal, bidirectional basic mode, and live ERTM negotiation/segmentation/windows/busy state/acknowledgments/loss recovery/logical timers/FCS. LE covers single and enhanced one-to-five-channel setup, refusal correlation, MTU/MPS segmentation/reassembly, credit stalls/replenishment, atomic reconfiguration, accepted channels, bidirectional transfer, and disconnect cleanup. HCI/host fragment and reassemble complete L2CAP PDUs across ACL buffer boundaries. Deferred: async/event conveniences and broader host-manager integration. |

### ATT / GATT
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `att.py` (1.1k) | `bumble-att` | ✅ | Complete typed catalog for every upstream `ATT_PDU` subclass: discovery, MTU, Read/Blob/Multiple/Multiple Variable/By Type/By Group, Write/Command/Signed, Prepare/Execute Write, notifications/indications, and confirmation. Signed Write separates value/counter/MAC, computes the CSRK AES-CMAC, and provides monotonic signer/verifier state. All added forms are Python-oracle or independent CMAC-vector pinned; variable tuples and handle sets add safe truncation/shape checks. |
| `gatt.py` (0.6k), `gatt_server.py` (1.2k) | `bumble-gatt` | 🟡 | Attribute DB, primary/secondary services, include declarations, characteristic descriptors, automatic CCCDs, explicit access/security permissions, bearer-aware dynamic read/write callbacks, primary discovery, read/write/notify, Find_Information/Find_By_Type_Value, MTU-sized Read/Blob, fixed + variable Read Multiple, atomic Prepare/Execute Write with cancel/rollback, and authenticated signed-write client/server handling with replay protection. Deferred: the async bearer/event convenience layer. |
| `gatt_client.py` (1.2k) | `bumble-gatt` | 🟡 | **`GattClient` (slice 18)**: service / characteristic / descriptor discovery, reads (with long-read via Read_Blob), writes (with and without response), and notify/indicate subscriptions (CCCD write + notification/indication handling), over an `AttTransport`. Deferred: async bearer/event listeners. |
| `gatt_adapters.py` (0.4k) | `bumble-gatt` | ✅ | Typed server/proxy adapters for delegated, packed, mapped, UTF-8, serializable, and enum values, including typed dynamic server state and cached proxy decoding. `PackedCodec` covers Python 3.14 portable and native-aligned `struct` modes, zero-repeat tail alignment, pointer-sized integers, binary16, and complex32/64, with host-Python oracle vectors. |

### Security (SMP + crypto)
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `crypto/` | `bumble-crypto` | ✅ | All SMP **symmetric** security functions — `e`, AES-CMAC, `c1`, `s1`, `f4`/`f5`/`f6`, `g2`, `h6`/`h7`, `ah` — spec/RFC-4493 vector-verified, plus **P-256 `EccKey`** (slice 19: keygen, `from_private_key_bytes`, public-key coordinates, ECDH) oracle-pinned to upstream. Deferred: none of the crypto primitives. |
| `smp.py` (2.0k), `pairing.py` (0.3k) | `bumble-smp` | ✅ | Complete PDU codec and synchronous protocol behavior: Legacy and SC sessions cover every association model through encryption; responder-first phase 3 retains LTK/IRK/CSRK/Link Key material and counters; h6/h7 CTKD runs over LE and encrypted BR/EDR; bonds drive Security Request reconnect, privacy resolution, and signed ATT; and the handle-keyed manager owns concurrent session lifecycle. Keypress Notification is codec-complete, matching upstream Bumble whose live session leaves `keypress = False`. |

### Transports & drivers
| Upstream | Rust crate | Status | Notes |
|---|---|---|---|
| `transport/*` — USB, UART/serial, TCP, WebSocket, UDP, PTY, android-netsim, vhci, … | `bumble-transport` | 🟡 | Incremental H4 framing accepts fragmented/coalesced streams and vendor packet layouts. Bumble transport-name/metadata dispatch opens file, serial/UART with RTS/CTS, raw PTY, TCP, UDP, Unix, and WebSocket client/server endpoints; WebSocket clients support `ws://` and `wss://`. Deferred: DSR/DTR flow control, USB, VHCI/HCI sockets, netsim, and other platform-specific endpoints. |
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

## Slice 49 — what's here

The ATT wire catalog now represents every class registered by upstream:

- Fixed and variable Read Multiple requests carry typed handle lists; their
  responses preserve concatenated values or explicit little-endian
  length/value tuples.
- Signed Write Command retains its signed opcode bits and upstream's lossless
  value/signature tail. Prepare Write request/response and Execute Write
  request/response expose handle, offset, fragment, and flag fields.
- Odd handle lists, truncated variable tuples, incomplete prepare fields, and
  missing execute flags return errors rather than indexing past the packet.
- All nine newly typed PDU forms are byte-for-byte pinned against Python Bumble,
  including opcode, little-endian handles/offsets, and variable tuple lengths.

Next, these completed codecs are wired into GATT server behavior for multiple
reads and queued writes.

## Slice 50 — what's here

Both Rust attribute servers now execute the newly completed ATT requests:

- Fixed Read Multiple concatenates values in requested-handle order while
  respecting the negotiated MTU. Variable Read Multiple emits bounded
  little-endian length/value tuples and retains each full value's declared
  length when its transmitted part is truncated.
- A missing handle aborts either request with an error naming the exact failing
  handle, matching upstream's server behavior.
- Prepare Write echoes and queues each handle/offset/fragment without mutating
  the database. Execute flag `0x01` stages every fragment and commits atomically;
  `0x00` cancels. Invalid offsets roll back the whole transaction.
- Write Command remains best-effort. Signed Write Command is intentionally a
  no-op until SMP supplies the connection CSRK and signing counter, preventing
  unauthenticated signature bytes from corrupting an attribute value.

Next work continues closing GATT's service model and access/security semantics.

## Slice 51 — what's here

Bonded-peer key persistence now completes upstream `keys.py`:

- `PairingKeys` and each key's value/authentication/EDIV/random metadata convert
  to and from the same lowercase-hex JSON object shape as Python Bumble.
- `MemoryKeyStore` provides replacement update, get/list/delete/delete-all, and
  deterministic enumeration. IRKs turn into `(key, typed peer address)`
  resolving-list entries with the stored address type or random-device default.
- `JsonKeyStore` groups peers beneath controller namespaces. Partial updates
  merge only present fields; a default store adopts the only existing namespace
  exactly as upstream does.
- Saves create parent directories, serialize deterministically to a sibling
  temporary file, and atomically rename it over the database. Corrupt JSON,
  invalid hex, bad peer addresses, and filesystem errors remain typed failures.

The next slice returns to the remaining GATT service/access model gaps.

## Slice 52 — what's here

The GATT database can now represent the complete static upstream service model:

- `ServiceDefinition` selects primary or secondary declaration type and emits
  Include declarations referencing any service in the same database. Include
  values carry start/end handles and a UUID only for 16-bit Bluetooth UUIDs.
- `CharacteristicDefinition` accepts explicit permissions and an arbitrary
  ordered descriptor list. Notify/indicate characteristics still receive an
  automatic CCCD unless the caller supplied one.
- `AccessContext` carries bearer encryption, authentication, and authorization
  state. Direct/blob/multiple/by-type reads, writes/commands, and queued writes
  share the same permission checks and ATT error codes.
- Security-only permission flags imply access, matching upstream callers that
  specify `READ_REQUIRES_AUTHENTICATION` without a redundant `READABLE` bit.
  The compact pre-permission `GattServer::new` API retains its original
  permissive read/write behavior for compatibility.

The next slice adds dynamic read/write value accessors; after that, work
continues through the larger controller, L2CAP, SMP, profile, and transport
surfaces.

## Slice 53 — what's here

Dynamic GATT values complete the synchronous attribute model:

- `DynamicValue` binds read-only, write-only, or read/write callbacks to any
  database handle. `AccessContext` supplies a stable bearer ID together with
  encryption, authentication, and authorization state, allowing per-peer
  values such as CCCDs.
- Direct and blob reads, both Read Multiple forms, Read By Type, and Find By
  Type Value resolve callbacks instead of stale placeholder bytes. Writes and
  write commands invoke the write callback.
- Callback failures are caller-selected ATT error codes and are returned with
  the original opcode and attribute handle. Missing callback directions map to
  Read/Write Not Permitted.
- Cloned servers share callback state through thread-safe reference counting.
  Clearing a binding restores its retained static value. Prepare Write rejects
  dynamic values as Attribute Not Long, preserving the static queued-write
  path's atomic rollback guarantee.

The remaining GATT difference is its asynchronous bearer/event convenience
surface rather than database expressiveness. Porting now continues through the
larger partial subsystems.

## Slice 54 — what's here

Upstream `gatt_adapters.py` now has a typed synchronous Rust foundation:

- `ValueCodec` and `CharacteristicProxyAdapter` convert raw client reads,
  writes, and cached notification values without duplicating transport logic.
  `CharacteristicAdapter` performs the same conversion for server definitions
  and creates a `DynamicValue` backed by shared typed state.
- Delegated codecs preserve independently missing encoder/decoder errors. UTF-8,
  `ByteSerializable`, and width/endian-aware `IntConvertible` enum codecs cover
  the corresponding upstream adapter classes.
- `PackedCodec` preserves Python's scalar result for one field and tuple result
  for multiple fields. It supports portable endian prefixes, repetition,
  padding, booleans, signed/unsigned integers, 32/64-bit floats, chars, fixed
  byte strings, and Pascal strings. `MappedCodec` assigns those fields to
  ordered names.
- Tests port upstream's `>H`, `>HH`, UTF-8, serializable, and three-byte enum
  vectors, then drive typed proxy and server adapters through the real GATT
  client/server path.

The next adapter slice closes Python-native alignment and remaining uncommon
`struct` codes before moving out of GATT.

## Slice 55 — what's here

The packed adapter now matches the current Python 3.14 `struct` model used by
upstream:

- An omitted prefix and `@` use native byte order, C sizes, and inter-field
  alignment. `=`, `<`, `>`, and `!` retain standard sizes without alignment.
  Native `long`, `ssize_t`/`size_t`, and pointer forms (`l/L`, `n/N`, `P`) use
  target widths, and zero-repeat fields preserve Python's tail-alignment rule.
- Binary16 `e` implements round-to-nearest-even conversion and rejects finite
  overflow. Python 3.14 complex `F`/`D` values serialize real then imaginary
  components at 32- or 64-bit precision.
- Zero-length strings/Pascal strings, native padding, signed extension, integer
  range checks, and enum-width overflow are handled without truncation or
  indexing hazards.
- Tests pin byte-for-byte output to the local Python 3.14.3 oracle for native
  `@bhi`, `@bl`, `@nNP`, `@llh0l`, big/little binary16, and big-endian complex
  values.

With `gatt_adapters.py` complete, work leaves the GATT model and returns to the
larger protocol/runtime gaps.

## Slice 56 — what's here

The L2CAP signaling codec now covers every control-frame dataclass registered by
upstream `l2cap.py`:

- Command Reject, Echo request/response, Information request/response,
  Connection Parameter Update request/response, LE Credit Based Connection
  request/response, and LE Flow Control Credit join the existing Classic and
  enhanced credit-based forms.
- Open numeric fields and variable reject/information/echo data remain lossless;
  every fixed field uses the specification's little-endian width.
- The ten newly typed forms have exact wire vectors and typed round trips.
  Truncated fixed payloads fail cleanly, while enhanced credit-based CID lists
  reject odd byte counts rather than dropping a trailing byte.

The codec catalog is complete; the next L2CAP work is runtime behavior, starting
with LE credit-based channel credit accounting and SDU segmentation/reassembly.

## Slice 57 — what's here

`LeCreditBasedChannel` now ports the data and credit machinery from upstream:

- `LeCreditBasedChannelSpec` enforces the Bluetooth minimum/maximum MTU, MPS,
  and nonzero-credit constraints. A connected channel records local/peer
  parameters and computes ATT MTU as their minimum.
- Writes form little-endian length-prefixed SDUs up to peer MTU, split them into
  K-frames up to peer MPS, consume one credit per frame, and resume without
  duplication when new credits arrive.
- Inbound K-frames consume granted peer credits, assemble split length headers
  and payloads, enforce local MPS/MTU and exact SDU length, and queue complete
  SDUs. Credits replenish to the configured maximum when they reach upstream's
  half-window threshold.
- Credit addition overflow, traffic after exhaustion/disconnect, oversize PDUs,
  oversize/overflowing SDUs, empty outbound writes, and invalid negotiation
  parameters are typed errors. Disconnect flushes all partial state.

The next slice wires this engine into LE signaling, deterministic CID/PSM
allocation, server acceptance, data routing, and disconnect handling.

## Slice 58 — what's here

`LeCreditChannelManager` completes the single-channel LE CoC runtime:

- Servers allocate LE_PSMs deterministically from `0x0080..=0x00FF`; channels
  allocate local CIDs from `0x0040..=0x007F` while excluding active and pending
  connections. Explicit duplicates and exhausted pools fail.
- Outgoing requests correlate responses by nonzero signaling identifier.
  Incoming requests return spec result codes for unsupported PSMs, duplicate
  peer CIDs, unacceptable MTU/MPS, and exhausted resources, or create and queue
  an accepted server channel.
- Data PDUs route by local CID into the Slice 57 engine. Generated K-frames go
  to the peer CID, and half-window replenishments become LE Flow Control Credit
  frames addressed to the correct remote channel.
- Paired-manager tests transfer hundreds of bytes in both directions with a
  one-credit window, forcing repeated stalls and resumes. Disconnect request/
  response validates both CIDs and removes channel state symmetrically.

Remaining L2CAP runtime work is enhanced credit-based multi-channel/reconfigure,
ERTM, and host-level ACL fragmentation/reassembly.

## Slice 59 — what's here

L2CAP PDUs now cross real HCI controller buffer boundaries:

- `fragment_l2cap_pdu` validates the complete L2CAP length, splits at the
  configured ACL payload size, marks the first non-flushable/flushable packet,
  marks continuations, and sets each fragment's exact HCI length.
- `AclDataPacketAssembler` tracks one connection's declared L2CAP size and
  returns only complete PDUs. Continuations without starts, changed handles,
  invalid PB flags, data-length mismatches, and L2CAP overflow are errors that
  reset partial state.
- `LocalLink::send_acl_packet` maps connection handles while preserving PB/BC
  flags. `Device` fragments outbound L2CAP at the controller's configured size
  (27 bytes by default), assembles inbound ACL per handle, and clears assembly
  state on disconnect before ATT/raw-channel routing.
- An end-to-end host test forces an 8-byte ACL payload limit and transfers
  257-byte L2CAP payloads in both directions, yielding exactly one intact
  receiver payload each way.

The next transport-boundary gap is HCI ACL packet-queue flow control and Number
Of Completed Packets accounting; L2CAP itself still needs enhanced modes.

## Slice 60 — what's here

Host-to-controller ACL flow control now matches upstream's bounded queue model:

- Generic `DataPacketQueue<T>` keeps FIFO order across connection handles,
  limits total in-flight packets, tracks per-handle in-flight counts and
  cumulative queued/completed/pending totals, and exposes drain state.
- Completion events free exactly the reported handle's slots. Unknown handles
  and over-completion are typed errors with bounded accounting; disconnect
  flush removes queued packets and implicitly completes that handle's in-flight
  packets.
- The virtual controller emits Number Of Completed Packets after accepting each
  routed ACL fragment. `Device` consumes those events, releases the next queued
  fragments, and flushes queue state on disconnection.
- The full-stack fragmentation test uses an eight-byte ACL payload and only two
  in-flight packets while transferring 257-byte L2CAP payloads both ways. Its
  33 fragments repeatedly exhaust and reopen the controller window, ending at
  zero pending packets and one intact receiver payload.

The HCI ACL transport boundary is now functional; L2CAP's next depth gap is
enhanced retransmission mode, while host/device breadth remains substantial.

## Slice 61 — what's here

The enhanced credit-based signaling frames now drive live LE CoCs rather than
stopping at the codec boundary:

- `connect_enhanced` reserves one to five local CIDs atomically, correlates the
  shared response, and creates every channel only after the peer returns an
  exact, unique destination-CID list and valid common MTU/MPS parameters.
- Incoming setup validates channel count, source-CID range and uniqueness,
  duplicate peer allocation, SPSM support, negotiation parameters, and atomic
  local resource availability. Each failure returns the corresponding enhanced
  result code without leaving a partial channel group.
- `reconfigure` updates one or more connected channels through the `0x19/0x1A`
  exchange. Successful responses update local receive limits and peer send
  limits symmetrically; ATT MTU and queued output are recomputed immediately.
- MTU reductions, multi-channel MPS reductions, invalid/duplicate CIDs, unknown
  channels, invalid parameter ranges, response-count mismatches, and excessive
  group sizes are rejected without mutation.
- Paired-manager tests establish all five permitted channels, force repeated
  one-credit stalls while transferring distinct bidirectional payloads on each,
  exercise multi-channel growth and legal single-channel MPS reduction, and pin
  every reconfiguration refusal class.

Remaining L2CAP protocol depth is enhanced retransmission mode (ERTM).

## Slice 62 — what's here

ERTM now has a standalone deterministic protocol engine suitable for live
Classic-channel binding:

- `EnhancedControlField` losslessly parses and serializes the two-byte
  Information and Supervisory forms, including 6-bit TxSeq/ReqSeq values, SAR,
  RR/REJ/RNR/SREJ functions, and independent Poll/Final bits. Oracle vectors
  pin Bumble's I-frame and REJ bytes; Poll uses the Bluetooth bit-4 position,
  correcting Bumble's currently asymmetric serializer/parser.
- `ErtmEngine` segments SDUs at peer MPS, prepends the declared length to Start
  frames, reassembles and validates local-MTU-bound SDUs, wraps sequence numbers
  modulo 64, and limits unacknowledged frames to the negotiated transmit window.
- RR acknowledgments advance the window, RNR pauses all new and retransmitted
  traffic, RR resumes it, REJ retransmits the outstanding window, and SREJ
  retransmits one requested sequence without duplicate delivery.
- Retransmission uses caller-driven logical ticks rather than a hidden runtime
  clock. Each frame has a bounded retry count; exceeding it permanently fails
  the engine. Invalid acknowledgments and malformed SAR transitions are typed
  errors.
- Tests cross the sequence wrap with 70 multi-frame SDUs, recover from a lost
  first frame, prove busy/ready stalling, exercise repeated timeout recovery,
  enforce the retry ceiling, and reject malformed control flow.

## Slice 63 — what's here

ERTM now runs through the same live Classic `ChannelManager` used by RFCOMM,
SDP, AVDTP, AVCTP, and HID:

- `ErtmChannelSpec`, `register_ertm_server`, and `connect_ertm` add opt-in mode
  configuration without changing the existing one-field `ClassicChannelSpec`
  API. MTU, Retransmission and Flow Control, and optional FCS configuration
  options are exchanged and validated before either endpoint opens.
- The negotiated peer MPS, transmit window, retransmission ceiling, and local
  logical timeout instantiate the Slice 62 engine. Mode mismatch, malformed
  options, zero MPS, invalid windows, and FCS disagreement close both sides with
  Configuration Unacceptable Parameters and never announce a server channel.
- Data frames are routed through the engine, delivered SDUs return through the
  existing `pop_received` API, RNR/RR can be driven at manager level, and
  `tick` advances every channel's deterministic retransmission clock.
- Optional FCS covers the actual L2CAP header, CID, ERTM control field, and
  payload. The receiver reconstructs that exact input and rejects corruption
  before sequence or SAR state can advance.
- The upstream ERTM test is ported with all four MTUs (50, 255, 256, 1000),
  asymmetric 256/1024-byte MPS values, and the exact 21/70/700/5523-byte SDU
  sequence in both directions. Additional live tests drop a transmit window,
  pause/resume with RNR/RR, retransmit on timeout, corrupt FCS, and negotiate a
  Basic/ERTM mismatch.

The named L2CAP protocol-depth gaps are now closed; remaining work is broader
host/device integration and the many still-unported upstream modules.

## Slice 64 — what's here

The policy and interchange layer needed by a complete SMP session is now
ported from `pairing.py` and `smp.py`:

- `IoCapability`, `AuthReq`, and `KeyDistribution` model the exact SMP numeric
  values and bit masks. `PairingCapabilities` validates the 7–16-byte key-size
  range and intersects initiator/responder distribution requests with local
  policy, matching `PairingDelegate.key_distribution_response`.
- `select_pairing_method` implements every entry in Vol 3, Part H, Table 2.8,
  including the legacy-vs-SC differences for Display Yes/No and Keyboard
  Display, plus which Passkey endpoint displays or enters the value. OOB
  selection preserves SC's one-sided and Legacy's two-sided requirements.
- `OobContext` generates or accepts a P-256 key and random value, derives the
  shared `C/R` data with `f4`, and matches a deterministic Python oracle.
  `OobData` losslessly composes/parses address, LE role, SC confirmation/random,
  and Legacy TK Advertising Data structures—including upstream's permissive
  variable-length shared values.
- `PairingConfig` and `OobConfig` capture SC, MITM, bonding, identity-address,
  capability, and OOB policy without imposing an async runtime.
- LE→BR/EDR and BR/EDR→LE CTKD use Bumble's `h6`/`h7` salts and key IDs. Both
  CT2 branches match the four upstream test vectors byte-for-byte.

The next SMP slice uses this foundation to replace the manually scripted host
tests with a live, delegate-driven session state machine.

## Slice 65 — what's here

LE Legacy pairing is now a reusable state machine rather than a test-authored
transcript:

- `LegacyPairingSession` drives Pairing Request/Response, negotiated bonding,
  7–16-byte encryption key size, initiator/responder key-distribution masks,
  Pairing Confirm, Pairing Random, peer-confirm verification, and STK
  derivation. The negotiated key size zeros the STK's most-significant tail.
- A synchronous `PairingDelegate` mirrors Bumble's user decisions. JustWorks
  requests automatic confirmation; Passkey selects the correct display/input
  endpoint and validates the six-digit range; Legacy OOB consumes the shared
  TK without prompting.
- Responder rejection, user confirmation failure, missing/invalid passkeys,
  missing OOB TK, confirm mismatch, undersized encryption keys, malformed
  features, and out-of-order commands emit the matching Pairing Failed reason
  and terminate both peers.
- `Device::enable_encryption` sends the derived STK through the real
  `LE_Enable_Encryption` command. `Device` tracks Encryption Change per handle,
  clears it on disconnect, and the host pump now advances queued LL control
  PDUs so both controllers and both hosts observe the transition.
- Tests cover matching independently derived STKs for JustWorks, Passkey, and
  OOB; negotiated key truncation/distribution; delegate display calls; rejection
  and wrong-passkey propagation; invalid ordering; and the full SMP-over-L2CAP-
  over-fragmented-ACL-to-LL-encryption path.

The next slice brings the same live orchestration to Secure Connections.

## Slice 66 — what's here

Secure Connections JustWorks and Numeric Comparison now run as a real paired
session:

- `ScPairingSession` negotiates SC/bonding/key size/key distribution, exchanges
  little-endian P-256 public keys, rejects a reflected or off-curve peer point,
  and independently derives the shared ECDH secret on each endpoint.
- The responder commits to `Nb` with `f4`, the initiator verifies that commitment
  after nonce exchange, and both derive the same MacKey/LTK/6-digit value with
  `f5`/`g2`. The negotiated encryption size truncates the LTK consistently.
- JustWorks invokes automatic delegate confirmation; Numeric Comparison sends
  the same six-digit number to both delegates. Rejection emits Confirm Value
  Failed, matching Bumble's behavior.
- Initiator `Ea` and responder `Eb` are computed independently with `f6` and
  verified before either session exposes an encryption-ready LTK. Tampered
  commitments and DHKey checks use their distinct failure reasons.
- A host-backed test transports the whole exchange over SMP/L2CAP/fragmented
  ACL, enables the resulting LTK through HCI/LL, and verifies Encryption Change
  at both hosts. Unit tests also cover Numeric Comparison approval/rejection,
  key-size truncation, invalid public keys, and commitment tampering.

The next SC slice adds the 20-round Passkey protocol and OOB association model.

## Slice 67 — what's here

Every LE Secure Connections association model now runs in the paired session:

- Passkey calls the selected display/input delegate endpoints, validates the
  six-digit range, and executes all 20 least-significant-bit-first rounds. Each
  round exchanges independently generated `Nai/Nbi`, commits with
  `f4(PKax,PKbx,Nai,0x80+ri)` / its responder mirror, verifies before advancing,
  and only the final nonce pair enters `f5`.
- The passkey is encoded as the 128-bit `Ra=Rb` input to the final `f6` checks.
  Matching peers derive identical authenticated MacKey/LTK values; even a
  one-value passkey mismatch terminates with Confirm Value Failed during the
  commitment phase.
- SC OOB sessions take their ECC key and local `R` directly from `OobContext`.
  On receiving a public key, each endpoint with peer data verifies the shared
  `C=f4(PKx,PKx,R,0)` before accepting the point or any nonce.
- One-sided OOB remains valid as required by SC. Missing peer data contributes
  a zero remote `R`; supplied data contributes its advertised `R`. Initiator
  and responder map those values to the same `Ra/Rb` ordering for independent
  DHKey-check verification, without confirmation UI.
- Tests cover successful 20-round Passkey, display/input routing, wrong-passkey
  failure, two-sided OOB success, authenticated matching keys, zero delegate
  prompts, and tampered OOB confirmation rejection at public-key exchange.

Those association models feed directly into the encrypted phase below.

## Slice 68 — what's here

SMP phase 3 now runs after the controller reports that the link is encrypted:

- The responder distributes first. The initiator waits for the negotiated peer
  set, sends its own set only after that set is complete, and both sessions
  reject unexpected or pre-encryption distribution PDUs with Pairing Failed.
- Legacy pairing sends Encryption Information plus Master Identification for
  `ENC_KEY`; both modes send IRK plus identity address for `ID_KEY` and CSRK for
  `SIGN_KEY`. Distribution PDUs may arrive in any order, matching Bumble's
  expected-command-set behavior.
- Secure Connections uses its derived shared LTK and deliberately suppresses
  legacy ENC_INFO/MASTER_ID PDUs. Negotiated `LINK_KEY` produces a CTKD link
  key, using h6 or h7 according to the CT2 result described below.
- Completed sessions expose Bumble-compatible `PairingKeys`: SC uses the shared
  `ltk`; Legacy preserves central/peripheral LTK, EDIV, and RAND direction; peer
  IRK/CSRK and the peer identity address are retained with authentication state.
- Bonds write through the existing `KeyStore` interface, so both memory and
  atomic namespaced JSON stores can retrieve them by identity address.
- Deterministic tests cover responder-first ordering, all key PDU families,
  Legacy directional material, SC suppression and CTKD, persistence/readback,
  malformed phase ordering, and live Legacy/SC distribution over host ACL/L2CAP.

## Slice 69 — what's here

Persisted bonds now participate in subsequent connection security:

- `PairingConfig::ct2` is advertised in AuthReq and remains enabled only when
  both pairing peers set CT2. The negotiated result is retained in both Legacy
  and SC outcomes and drives h7 CTKD; otherwise the existing h6 path is used.
- A peripheral can emit a typed Security Request, and `Device` surfaces its
  AuthReq while preserving the raw SMP PDU for normal fixed-channel consumers.
- `security_request_action` evaluates a retrieved `PairingKeys` record. SC and
  MITM requirements must be met by the stored key; missing, malformed, weaker,
  or Legacy-only material requests fresh pairing rather than downgrading.
- SC reconnects select the shared LTK. Legacy reconnects select
  `ltk_central`/`ltk_peripheral` from the local role and preserve EDIV/RAND in
  the HCI LE Enable Encryption command.
- The live host test sends Security Request over SMP/L2CAP/ACL, observes it on
  the central, selects an authenticated SC bond, and reaches encrypted state on
  both controllers without running a new pairing exchange.

## Slice 70 — what's here

Bond identities now survive privacy address rotation:

- `AddressResolver` ports upstream's exact `hash || prand` split and `ah(IRK,
  prand)` lookup, returning public/random identity address types and rejecting
  non-RPAs, wrong IRKs, and malformed stored keys without panicking.
- Deterministic and random RPA generators force the required `0b01` marker and
  are pinned to upstream's published `ah` vector.
- The software controller now implements add/clear/read-size resolving-list,
  address-resolution enable, and RPA-timeout command state rather than merely
  acknowledging those HCI commands.
- `Device::configure_address_resolution` loads the existing key store's
  resolving-key output into that controller state and exposes connection role
  plus the resolved peer identity reported by HCI.
- The end-to-end privacy test stores a peer IRK, advertises under its RPA,
  initiates to the identity address, resolves in the controller, reports a
  Random Identity address to the central host, and sends L2CAP/ACL data across
  the actual RPA-backed link.

## Slice 71 — what's here

The signing keys distributed in phase 3 now protect real ATT traffic:

- Signed Write Command parsing separates the attribute value from its 4-byte
  little-endian sign counter and 8-byte authentication MAC; short trailers are
  rejected instead of being mistaken for value bytes.
- `SignedWriteSigner` computes AES-128-CMAC over opcode, handle, value, and
  counter and truncates the 128-bit result to the required 64-bit signature.
  The fixed vector is independently pinned with OpenSSL CMAC.
- `SignedWriteVerifier` compares the MAC without an early byte exit and accepts
  only counters greater than the last valid packet. Wrong keys, changed values,
  changed signatures, and replays do not advance state.
- Bare ATT and permission-aware GATT servers apply only verified commands; the
  GATT client can produce them directly. The host now dispatches ATT commands
  to its server without fabricating a response.
- Bond records retain both the peer CSRK/last incoming counter and local
  CSRK/next outgoing counter. Signer/verifier state reconstructs from
  `PairingKeys`, writes back through `MemoryKeyStore`/`JsonKeyStore`, and a
  restart test proves that a previously accepted packet remains a replay.
- A live test sends accepted, replayed, and tampered signed writes through
  ATT/L2CAP/ACL and reads back only the last authenticated value.

## Slice 72 — what's here

The LE SMP pieces now run behind a connection-aware manager:

- `PairingManager` registers role/address context by connection handle and owns
  independent Legacy or SC sessions in a map. Initiators start explicitly;
  responders are created automatically by an inbound Pairing Request.
- Every outbound PDU retains its originating handle, so concurrent exchanges
  can interleave without cross-session state. Security Requests are surfaced on
  a separate queue because they are connection security policy, not pairing
  session traffic.
- Encryption keys, state/failure inspection, phase-3 advancement, completed
  `PairingKeys`, key-store persistence, and disconnect cleanup are all exposed
  at the manager boundary. Duplicate handles and invalid role/lifecycle actions
  fail without disturbing other sessions.
- A deterministic concurrency test completes two SC pairings at once, advances
  both through encrypted key distribution, and stores two independent bonds.
- A live host test uses only manager output/input around SMP/L2CAP/ACL, enables
  controller encryption with the manager's LTK, finishes distribution, and
  persists the resulting bond.

## Slice 73 — what's here

Cross-transport derivation now runs on a real Classic ACL:

- HCI Set Connection Encryption is functional for BR/EDR: the initiator emits
  a Classic LMP encryption-mode request and both hosts receive Encryption
  Change. The host tracks Classic encryption separately from LE.
- Classic ACL handles now carry fragmented/reassembled L2CAP through the same
  bounded host queue, enabling SMP fixed CID `0x0007` rather than an isolated
  state-machine transcript.
- `ClassicCtkdSession` requires an encrypted ACL and existing Link Key,
  exchanges only Pairing Request/Response, negotiates CT2/key size/distribution,
  and derives the common LE LTK with h6 or h7. It never runs LE confirm/random,
  public-key/DHKey-check, or legacy ENC_INFO/MASTER_ID phases.
- Identity and signing keys retain responder-first ordering; completed bonds
  contain the derived LTK, original Link Key, IRKs/CSRKs, authentication state,
  and counters.
- `PairingManager` selects this session for registered BR/EDR connections while
  retaining the same handle correlation, bond persistence, and lifecycle API.
- The live test establishes Classic ACL, enables encryption on both controllers,
  runs CTKD over CID `0x0007`, and verifies identical outcomes and retained Link
  Key material.

Upstream Bumble declares Keypress Notification but its live session leaves the
feature disabled, so the codec plus the implemented pairing/distribution paths
now represent the complete synchronous SMP behavior surface.

## Slice 74 — what's here

The common LE lifecycle no longer requires tests or applications to construct
HCI commands directly:

- `Device::set_random_address`, `start_advertising`, and `stop_advertising`
  configure connectable legacy advertising with a bounded 31-byte payload.
- Active/passive scan start/stop methods collect typed `AdvertisingReport`
  values, preserving address/type, data, event type, and RSSI.
- `connect_le` applies the standard scan/connection parameters, chooses the
  peer address type, initiates through the controller/link, and updates peer
  address plus central/peripheral role from Connection Complete.
- The acceptance test advertises, scans, validates payload/report identity,
  connects, checks both roles, and disconnects using only `Device` methods.

The larger remaining device work is extended/periodic advertising,
multi-connection ownership, and higher-level profile/listener conveniences.

## Slice 75 — what's here

External controllers can now exchange typed HCI packets with the Rust stack:

- `PacketFramer` implements upstream Bumble's H4 length-table behavior for
  command, ACL, synchronous, event, and ISO packets. It accepts arbitrary input
  fragmentation, emits coalesced packets in order, and supports registered
  vendor packet layouts.
- `PacketSource` and `PacketSink` provide the synchronous transport boundary;
  `H4Transport<T>` adapts any blocking `Read`/`Write` stream and distinguishes a
  clean EOF from a truncated packet.
- `FileTransport`, TCP client/server, connected UDP, and Unix-domain socket
  client/server endpoints use the same typed contract. UDP preserves Bumble's
  parser behavior, including multiple packets in one datagram.
- Acceptance tests use actual loopback sockets and a temporary file, in
  addition to testing every split point in a coalesced five-packet stream that
  covers every standard H4 packet type.

The next layer adds transport-spec parsing, serial configuration, and PTY
creation on top of this framing foundation.

## Slice 76 — what's here

Host-local transport configuration now follows Bumble's named endpoint model:

- `TransportSpec` parses `<scheme>:[key=value,...]parameters`, including the
  optional trailing metadata comma used upstream, without corrupting colons in
  socket addresses.
- `open_transport` dispatches file, serial, PTY, TCP client/server, UDP, and
  Unix client/server names while retaining source metadata. Synchronous server
  dispatch documents that it blocks for the first client; the separate server
  types remain available when bind and accept must be controlled independently.
- `SerialConfig` matches Bumble's 1 Mbaud default, optional numeric speed,
  `rtscts`, `dsrdtr`, and 500 ms `delay` flags. The live backend configures
  RTS/CTS and asserts DTR; DSR/DTR flow control returns an explicit unsupported
  error because the portable backend cannot enable it safely.
- `PtyTransport` creates a raw primary/replica pair, optionally publishes the
  replica through a symlink, and removes that link on drop. Its acceptance test
  sends typed HCI packets in both directions over the real PTY; a second test
  opens that replica through serial dispatch and verifies the live UART path.

The transport crate now reaches local serial devices and controller processes;
the next layer adds WebSocket connectivity for remote endpoints.

## Slice 77 — what's here

HCI can now cross Bumble's WebSocket transport boundary:

- `WebSocketTransport::connect` supports blocking `ws://` and TLS `wss://`
  clients, and `WebSocketServer` separates bind from accept for controlled
  server lifecycles.
- HCI is emitted only as binary messages. Incoming text frames are ignored,
  control frames remain with the WebSocket protocol engine, and close frames
  become a clean transport EOF.
- Binary messages feed the shared `PacketFramer`, preserving upstream behavior
  when one message contains several packets or one packet spans messages.
- `ws-client` and `ws-server` participate in transport-name dispatch. Real
  loopback tests cover handshake, typed bidirectional traffic, text rejection,
  coalescing, and dispatcher metadata retention.

The remaining external-controller transports are USB, VHCI/HCI sockets,
Android emulator/netsim, and narrower platform integrations.

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
│   ├── src/{lib,uuid,address,appearance,class_of_device,advertising_data,keys}.rs
│   ├── tests/acceptance.rs    # ported upstream tests
│   └── tests/key_store.rs     # slice-51 atomic namespaced persistence
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
│   ├── tests/acceptance.rs    # ported gatt_test.py ATT cases (oracle-pinned)
│   └── tests/complete_catalog.rs # slice-49 remaining upstream PDU forms
├── bumble-crypto/             # slice-6 SMP crypto toolbox + slice-19 P-256 ECC
│   ├── src/lib.rs             # symmetric functions + EccKey (P-256 ECDH)
│   ├── tests/vectors.rs       # ported smp_test.py spec/RFC vectors
│   └── tests/ecc.rs           # P-256 public keys + ECDH pinned to oracle
├── bumble-gatt/               # slice-9 GATT/ATT server + slice-18 GATT client
│   ├── src/lib.rs             # AttServer, GattServer
│   ├── src/client.rs         # GattClient (slice 18)
│   ├── tests/end_to_end.rs   # attribute write/read across the full stack
│   ├── tests/client.rs       # two-party client↔server discovery/read/write/subscribe
│   └── tests/queued_writes.rs # slice-50 multiple reads + atomic queue
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
├── bumble-transport/          # slices 75-77 external HCI transports
│   ├── src/{lib,common,dispatch,file,serial,pty,tcp,udp,unix,websocket}.rs
│   ├── tests/transports.rs    # fragmentation, EOF, and socket loopbacks
│   ├── tests/specs.rs         # dispatch, serial config, and raw PTY coverage
│   └── tests/websocket.rs     # binary framing + client/server handshake
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
