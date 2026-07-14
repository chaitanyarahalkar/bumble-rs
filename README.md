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
| 2. HCI packet codec (framing + **full** command/event catalog + return params) | `bumble-hci` | ✅ 321/321 tests green |
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
| 78. Linux VHCI bootstrap and H4 transport | `bumble-transport` | ✅ config/index handshake green |
| 79. USB HCI discovery and command/event/ACL transport | `bumble-transport` | ✅ libusb backend + transfer mock green |
| 80. Linux raw HCI user-channel socket transport | `bumble-transport` | ✅ Linux target check + packet-I/O mock green |
| 81. Android emulator host/controller gRPC transport | `bumble-transport` | ✅ real bidirectional gRPC loopback green |
| 82. Android netsim host/controller packet-stream transport | `bumble-transport` | ✅ startup/INI/lease + live gRPC green |
| 83. Intel USB controller firmware driver | `bumble-drivers` | ✅ TLV/SFI/DDC + scripted cold start green |
| 84. Realtek USB controller firmware driver and driver selection | `bumble-drivers` | ✅ epatch/probe/download + selector green |
| 85. Foundational GAP/GATT/Battery/Device Info/Heart Rate profiles | `bumble-profiles` | ✅ live service/proxy + database hash green |
| 86. ASHA hearing-aid streaming and Coordinated Set Identification | `bumble-profiles` | ✅ control/state + CSIS crypto/live encrypted GATT green |
| 87. Volume Control, Volume Offset Control, and Audio Input Control | `bumble-profiles` / `bumble-gatt` | ✅ encrypted control matrices + included-service discovery green |
| 88. Media Control and Generic Media Control | `bumble-profiles` | ✅ full model/proxy catalog + live notification handshake green |
| 89. LE Audio metadata, BAP codec foundations, and PACS | `bumble-profiles` / `bumble-hci` | ✅ LTV/PAC codecs + live capability discovery green |
| 90. Telephony/Media, Gaming Audio, and Public Broadcast profiles | `bumble-profiles` | ✅ role/features + announcement vectors/live reads green |
| 91. Basic Audio announcements and Audio Stream Control | `bumble-profiles` | ✅ all ASE operations + live sink/source state machine green |
| 92. Broadcast Audio Scan and Common Audio profiles | `bumble-profiles` | ✅ all operations/states + live encrypted BASS/CAS inclusion green |
| 93. Hearing Access Profile | `bumble-profiles` | ✅ encrypted preset lifecycle + indications/synchronization green |
| 94. Apple Media and Notification Center profiles | `bumble-profiles` | ✅ 128-bit GATT + commands/fragmented data/live clients green |
| 95. Extended advertising sets, scanning, reports, and connection setup | `bumble-controller` / `bumble-host` | ✅ fragmented 1650-byte data + live two-device flow green |
| 96. Connected ISO data paths and SDU streaming | `bumble-controller` / `bumble-host` / `bumble-hci` | ✅ setup/remove + fragmentation/reassembly + live CIS routing green |
| 97. G.722 64 kbit/s audio decoder | `bumble-codecs` | ✅ upstream fixture PCM byte-exact + incremental-state green |
| 98. Portable PCM audio input and output | `bumble-audio` | ✅ format/raw/WAVE/subprocess paths + looping and threaded output green |
| 99. Android and Zephyr vendor HCI codecs | `bumble-hci` | ✅ exact command envelopes + versioned returns/BQR parsing green |
| 100. Bidirectional filtered HCI bridge | `bumble-transport` | ✅ replacement/short-circuit/trace paths green |
| 101. Periodic advertising and synchronization | `bumble-controller` / `bumble-host` | ✅ 600-byte train + create/cancel/receive/terminate flow green |
| 102. Periodic Advertising Sync Transfer | `bumble-controller` / `bumble-host` | ✅ sync + set-info transfer over live LE ACL green |
| 135. Broadcast ISO groups and streams | `bumble-controller` / `bumble-host` | ✅ BIGInfo + encrypted BIG sync + one-to-many BIS SDUs green |
| 145. All-page local LE feature discovery | `bumble-hci` / `bumble-controller` / `bumble-transport` | ✅ 197th command + 248-byte return + preferred host reset path green |
| 146. Complete controller data-return surface | `bumble-hci` / `bumble-controller` | ✅ all 31 data commands return upstream payloads; query-backed writes retain state |
| 147. Complete software-controller configuration semantics | `bumble-controller` | ✅ all stateful upstream handlers explicit; legacy advertising/scan and pending-operation behavior green |
| 148. Complete virtual-link and LL teardown semantics | `bumble-controller` / `bumble-host` | ✅ full LL opcode/model surface + pumped ACL/Classic/SCO/CIS teardown and reusable central CIS handles green |
| 149. Complete Classic LMP codec and catalog | `bumble-controller::lmp` | ✅ 88 open opcodes + all 18 registered packet classes + strict byte round trips green |
| 150. Complete common LE connection-control conveniences | `bumble-hci` / `bumble-host` | ✅ update/rate/subrate, data length, PHY, RSSI, defaults, state, and correlated completion journal green |
| 151. Typed Device lifecycle listeners | `bumble-host` | ✅ ordered journal/listeners + connection failures + non-destructive disconnection failures green |
| 152. Multi-listener GATT subscriptions | `bumble-gatt` | ✅ ordered notify/indicate callbacks + last-subscriber/forced CCCD cleanup green |
| 153. USB SCO/eSCO isochronous transport | `bumble-transport` | ✅ alternate selection + fragmented input + multi-packet output green |
| 154. Enhanced ATT bearers | `bumble-gatt` / `bumble-host` / `bumble-transport` | ✅ LE CoC read/write + bearer-scoped CCCDs/MTUs/queues + notify/indicate fan-out green |
| 155. RFCOMM receive-queue completion | `bumble-rfcomm` | ✅ upstream 32-packet bound + oldest eviction + retained order green |
| 156. Platform audio devices | `bumble-audio` | ✅ optional CPAL enumeration + float32 output + int16/stereo input green |
| 157. HFP completion audit | `bumble-hfp` | ✅ full upstream behavior families + all indicator factories + no invented media-codec gap |
| 158. SDP completion audit | `bumble-sdp` | ✅ 128-bit integers + depth guard + UUID helper + complete client/server/L2CAP surface green |
| 159. Complete CIG/CIS control surface | `bumble-host` / `bumble-controller` | ✅ full QoS + batching + accept/reject + result/link journals green |
| 160. Complete ISO link controls | `bumble-hci` / `bumble-controller` / `bumble-host` | ✅ custom data paths + typed completions + TX-sync state/journal green |
| 103+. Repository completion audit and remaining gaps | workspace | in progress |

The LE lifecycle is now complete end-to-end through library APIs: **connect →
discover → read/write → notify → disconnect** between two virtual devices — and
**every crate is integrated**, with `bumble-crypto` now driving SMP pairing.

The HCI codec is now a **complete typed catalog**: all 197 command op codes and
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
| `keys.py` (0.4k) | `bumble::keys` | ✅ | Complete `PairingKeys` / `Key` JSON model, replacement-style memory store, namespaced JSON store with upstream merge/default-namespace semantics and atomic replacement, delete/get/get-all/delete-all, platform data-path selection, and IRK resolving-list extraction to typed addresses. JSON deletion reports an absent peer while memory deletion remains a no-op, matching the two upstream backends. Rust uses synchronous filesystem calls rather than wrapping them in nominal async methods. |
| `utils.py` (0.5k) | `bumble::util` (+ spread) | ✅ | Generic helpers (`bit_flags_to_strings`, `name_or_number`); `crc_16` lives in `bumble-l2cap`; the open-enum/flag pattern is realized as newtypes throughout. The asyncio event infra (`EventEmitter`/`AsyncRunner`/`FlowControlAsyncPipe`) is **N/A** for this synchronous port. |
| `colors`, `logging`, `helpers` | — | N/A | Debug/logging tooling with idiomatic Rust equivalents rather than library surface: `colors` (ANSI), `logging` (→ `log`/`tracing`), and `helpers.PacketTracer` (debug trace). |
| `snoop.py` (0.3k) | `bumble-transport::snoop` | ✅ | Byte-exact BTSnoop and PCAP HCI-H4 writers, deterministic timestamp injection, direction/pseudo-header flags, file/pipe specification parsing, file-backed snoopers, and a transparent bidirectional `SnoopingTransport` wrapper. |
| `decoder.py` (0.4k) | `bumble-codecs::g722` | ✅ | Stateful integer G.722 64 kbit/s lower/higher sub-band decoder, receive QMF, predictor adaptation, saturating arithmetic, signed PCM sample API, and little-endian byte output. The upstream sample's first 80-byte frame produces all 320 oracle PCM bytes exactly; split-frame decoding proves state continuity. |

### HCI, controller & link — 🟡 HCI/controller/link/LL/LMP complete; host behavior partial
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `hci.py` (8.3k) | `bumble-hci` | ✅ | **Full typed catalog: 197 command op codes + 81 event / LE-meta sub-event codes**, generated from upstream's declarative field specs by [`tools/hcigen`](bumble-hci/tools/hcigen/) and **byte-pinned against real Python Bumble** (321 oracle tests). Framing (Command/Event/ACL/SCO/ISO), `Command_Complete` with typed `ReturnParameters` including signed `Read_RSSI`, `LE Read ISO TX Sync`, and the shared Setup/Remove ISO status-and-handle result, base/extended LMP pages, and the 248-byte all-page LE feature catalog, the open-enum `Generic` tail, and upstream-equivalent ACL/L2CAP fragmentation/reassembly with PB-flag, length, continuation, handle, and overflow validation. Two phys-derived array commands and the two nested-report events are hand-written; everything else is generated. |
| `vendor/{android,zephyr}/hci.py` | `bumble-hci::vendor` | ✅ | Android vendor-capability responses preserve all historical length prefixes; APCF, energy-info, A2DP-offload, and dynamic-buffer command/return payloads remain open where upstream does. Bluetooth Quality Reports decode every recognized report ID with signed radio metrics and opaque vendor tails. Zephyr read/write TX-power commands and return parameters preserve signed dBm values and open handle types. |
| `controller.py` (2.8k) | `bumble-controller` | ✅ | **Complete upstream command surface**: every one of the 93 generated handlers plus the public Device-facing `LE Reject CIS Request` and `LE Read ISO TX Sync` paths gets the matching Command Complete/Status shape; all 31 generated data commands provide their upstream state/default payloads; every stateful configuration handler retains its values; the remaining table fallbacks correspond exactly to upstream no-ops or TODO acknowledgements; and commands upstream also does not handle receive Unknown HCI Command. **Functionally simulated**: legacy, extended, and periodic LE advertising/scanning/synchronization; multi-set parameters/random addresses/fragmented data/scan responses; sync create/cancel/terminate, receive control, and PAST sync/set-info transfer over ACL; IRK resolving-list offload with identity-targeted RPA connections; ACL routing with PB/BC preservation and Number Of Completed Packets flow events; disconnection; event masks, Classic scan mode, default PHY, local name, suggested data length, synchronous flow control, and LE/SSP host features; per-connection data length, PHY, LE subrate, and Sniff/Active mode changes; and — via LL control-PDU exchange — **encryption start**, **remote features**, and **CIS establishment or rejection**. CIG removal validates its identifier; CIS links retain Setup/Remove ISO data paths, route HCI ISO fragments with handle translation and completed-packet events, and expose the last routed SDU sequence/timestamp through Read ISO TX Sync. Reset clears CIS and TX-sync state. Classic connection/name/base and extended feature-page exchange, accept-time and explicit role switching, and SCO/eSCO request/accept/reject/disconnect are live. Upstream's own LTK-verification TODO and absent Classic-authentication/remote-version handlers remain the boundary. |
| `link.py` (0.15k) | `bumble-controller` | ✅ | Complete in-process link bus for advertising, ACL, LL-control, Classic LMP, synchronous, and ISO traffic. Rust's explicit deterministic pumps are the scheduling equivalent of upstream's `asyncio.call_soon`: connect, feature/encryption/CIS procedures, and ACL/Classic/SCO/CIS teardown all cross the queued PDU path before the peer observes them. No serialized radio transport is deferred here—upstream explicitly defines these objects as context-aware in-process messages without a real physical transport. |
| `ll.py` (0.2k) | `bumble-controller::ll` | ✅ | Complete upstream LL model surface: all 61 control opcodes plus every concrete upstream control PDU (`TerminateInd`, `EncReq`, feature request/response forms, `CisReq`/`CisRsp`/`CisInd`, and `CisTerminateInd`). An internal extended-rejection envelope carries the public Device CIS-reject path across the same context-aware link. Advertising/connect behavior is carried by the controller's typed in-process advertising envelope. CIS rejection reports the supplied failure to the central without creating link state; termination removes peripheral state but preserves and unbinds the central handle, matching upstream so the same configured CIG can establish it again. |
| `host.py` (2.1k) | `bumble-host` | 🟡 | `Device` glue (ATT/EATT↔L2CAP↔ACL sequencing + pairing transport), controller-buffer-sized outbound ACL fragmentation, per-connection inbound reassembly, a global/per-handle `DataPacketQueue` driven by Number Of Completed Packets, handle-indexed LE and Classic connection ownership, live LE signaling/credit-channel managers, LE/Classic encryption and remote-feature completion routing, resolving-list programming, Classic and LE L2CAP, Channel Sounding completion routing, plus connected and broadcast ISO audio APIs. Fixed ATT and EATT requests share the server while retaining bearer-scoped context, inboxes, CCCDs, MTUs, queued writes, and indication confirmations. LE connection update/rate/subrate, data-length, PHY, RSSI, CIG/CIS, ISO data-path, and ISO TX-sync completions update per-handle state and enter correlated result journals, including status-only failures. A typed `DeviceEvent` journal and synchronous listener registry now route LE/Classic/SCO lifecycle, discovery, pairing, encryption, and connection-control outcomes after state mutation; failed disconnects preserve all per-handle state. The host pump advances LL, LMP, periodic-sync-transfer, BIG termination, and HCI/ACL traffic. Deferred: narrower awaitable convenience wrappers. |
| `device.py` (7.0k) | `bumble-host` | 🟡 | High-level legacy, extended, and periodic LE advertising; active/passive scan reports; periodic sync create/cancel/terminate, fragmented-report assembly, and PAST sync/set-info transfer; identity/RPA-aware legacy and extended connection setup; handle-indexed LE/Classic peer, role, connection parameters/rate/subrate, negotiated data length, PHY and RSSI, remote LE and multi-page LMP features, Sniff/Active, and Channel Sounding capability/config/procedure state; and disconnect run through `Device` without raw HCI. `add_event_listener`/`remove_event_listener` and the ordered `take_device_events` journal provide the upstream lifecycle/discovery listener surface without forcing an async runtime. The upstream update, Bluetooth 6.2 rate, data-length, PHY read/set/default, RSSI, default-rate, and default-subrate conveniences all have handle-safe command APIs; explicit result events replace their futures. Extended and periodic data fragment across HCI commands up to the controller's 1650-byte limit. ATT, EATT, L2CAP, encryption, PAST, and ISO operations have explicit handle-selecting forms while legacy convenience calls use a selectable current connection. CIG/CIS exposes every group and directional QoS field, upstream defaults and zero-SDU normalization, batched creation, accept/reject, failure correlation, and established timing/link state; BIG/BIS synchronization, arbitrary data-path IDs/codecs/configuration/controller delays, installed-path state, TX-sync metadata, sequence numbering, 960-byte ISO fragmentation, and receive-side SDU reassembly are live. GATT/ATT, SMP, Classic, and synchronous operations are also exposed by the same type. Deferred: narrower awaitable convenience wrappers. |
| `lmp.py` (0.4k) | `bumble-controller::lmp` | ✅ | Complete open 88-opcode catalog and byte codec for all 18 registered packet classes: base/extended accept/reject, `AuRand`, detach, SCO/eSCO setup/removal, host connection, switch, fragmented-name, and base/extended feature packets, plus unknown-opcode payload preservation and bounded truncation/length errors. Payload layouts are pinned to upstream's field serializer; two-byte escape opcodes use their intended `0x7Fxx` wire values. The controller's in-process semantic variants drive every LMP family upstream actually handles through `pump_classic`. Upstream's controller leaves `AuRand`/authentication unhandled, so no synthetic authentication flow is claimed. |

### L2CAP
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `l2cap.py` (3.1k) | `bumble-l2cap` | 🟡 | PDU + complete typed upstream signaling-frame catalog + FCS, synchronous Classic connection-oriented channels, and paired LE CoC runtimes. Classic covers dynamic PSM/CID allocation, Connection/Configure/Disconnection, MTU negotiation/refusal, bidirectional basic mode, and live ERTM negotiation/segmentation/windows/busy state/acknowledgments/loss recovery/logical timers/FCS. LE covers single and enhanced one-to-five-channel setup, refusal correlation, MTU/MPS segmentation/reassembly, credit stalls/replenishment, atomic reconfiguration, accepted channels, bidirectional transfer, and disconnect cleanup. Each host LE handle now owns a manager whose signaling and dynamic-channel PDUs cross real HCI ACL fragmentation/reassembly; device-wide server registrations apply to existing and future links. Deferred: async/event conveniences. |

### ATT / GATT
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `att.py` (1.1k) | `bumble-att` | ✅ | Complete typed catalog for every upstream `ATT_PDU` subclass: discovery, MTU, Read/Blob/Multiple/Multiple Variable/By Type/By Group, Write/Command/Signed, Prepare/Execute Write, notifications/indications, and confirmation. Signed Write separates value/counter/MAC, computes the CSRK AES-CMAC, and provides monotonic signer/verifier state. All added forms are Python-oracle or independent CMAC-vector pinned; variable tuples and handle sets add safe truncation/shape checks. |
| `gatt.py` (0.6k), `gatt_server.py` (1.2k) | `bumble-gatt` / `bumble-host` | ✅ | Attribute DB, primary/secondary services, include declarations, characteristic descriptors, automatic bearer-scoped CCCDs, explicit access/security permissions, bearer-aware dynamic read/write callbacks, primary discovery, read/write/notify, Find_Information/Find_By_Type_Value, per-bearer MTU-sized Read/Blob, fixed + variable Read Multiple, per-bearer atomic Prepare/Execute Write with cancel/rollback, authenticated signed writes with replay protection, and EATT registration/routing over enhanced LE credit channels. Notification and indication fan-out honors each fixed/enhanced bearer subscription; confirmation and disconnect cleanup are bearer-local. |
| `gatt_client.py` (1.2k) | `bumble-gatt` / `bumble-transport` | ✅ | **`GattClient` (slices 18/152/154)**: primary/secondary/included service, characteristic, and descriptor discovery; reads (with long-read via Read_Blob); writes (with and without response); and notify/indicate subscriptions over an `AttTransport`. Stable listener IDs support multiple ordered callbacks per value handle; cache updates precede delivery, removal preserves the CCCD while any implicit or explicit subscriber remains, last-subscriber removal clears it, and forced unsubscribe writes zero without local state. Included-service discovery resolves both compact 16-bit and readback-based 128-bit declarations. `ExternalEattTransport::connect` performs the enhanced LE credit-channel handshake and drives the same client over a real controller; Rust's synchronous polling replaces upstream async scheduling. |
| `gatt_adapters.py` (0.4k) | `bumble-gatt` | ✅ | Typed server/proxy adapters for delegated, packed, mapped, UTF-8, serializable, and enum values, including typed dynamic server state and cached proxy decoding. `PackedCodec` covers Python 3.14 portable and native-aligned `struct` modes, zero-repeat tail alignment, pointer-sized integers, binary16, and complex32/64, with host-Python oracle vectors. |

### Security (SMP + crypto)
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `crypto/` | `bumble-crypto` | ✅ | All SMP **symmetric** security functions — `e`, AES-CMAC, `c1`, `s1`, `f4`/`f5`/`f6`, `g2`, `h6`/`h7`, `ah` — spec/RFC-4493 vector-verified, plus **P-256 `EccKey`** (slice 19: keygen, `from_private_key_bytes`, public-key coordinates, ECDH) oracle-pinned to upstream. Deferred: none of the crypto primitives. |
| `smp.py` (2.0k), `pairing.py` (0.3k) | `bumble-smp` | ✅ | Complete PDU codec and synchronous protocol behavior: Legacy and SC sessions cover every association model through encryption; responder-first phase 3 retains LTK/IRK/CSRK/Link Key material and counters; h6/h7 CTKD runs over LE and encrypted BR/EDR; bonds drive Security Request reconnect, privacy resolution, and signed ATT; and the handle-keyed manager owns concurrent session lifecycle. RPA generation and direct IRK verification are public helpers. Keypress Notification is codec-complete, matching upstream Bumble whose live session leaves `keypress = False`. |

### Transports & drivers
| Upstream | Rust crate | Status | Notes |
|---|---|---|---|
| `transport/*` — USB, UART/serial, TCP, WebSocket, UDP, PTY, android-netsim, vhci, … | `bumble-transport` | 🟡 | Incremental H4 framing accepts fragmented/coalesced streams and vendor packet layouts. Bumble transport-name/metadata dispatch opens file, serial/UART with RTS/CTS, raw PTY, TCP, UDP, Unix, WebSocket, Linux VHCI/raw HCI user-channel sockets, libusb Bluetooth-controller endpoints, Android emulator gRPC, and Android netsim host/controller packet streams. The typed HCI bridge pumps both directions with packet replacement, sender short-circuit responses, and trace hooks; `HciCommandChannel` correlates synchronous command responses while preserving unrelated traffic. `ExternalHost` resets controllers, discovers version plus the all-page LE feature catalog (with legacy eight-byte fallback) and base-or-multi-page LMP features, installs both event-mask pages, and configures ACL/ISO flow control. BTSnoop/PCAP writers can wrap any bidirectional transport, and the bounded BTSnoop reader handles H1/H4 captures, timestamps, drops, and truncation. USB covers discovery, class/forced interface selection, commands, events, ACL, outgoing ISO-over-bulk compatibility, and `+sco=` SCO/eSCO isochronous input/output through the locked libusb ABI. Deferred: DSR/DTR flow control (not exposed by the current `serialport` backend) and narrower platform-specific endpoints. |
| `drivers/*` — Intel, Realtek | `bumble-drivers` | ✅ | Both upstream driver modules and the RTK-before-Intel selector are ported behind a transport-neutral host contract. Intel covers open version TLVs, RSA/ECDSA SFI secure send, boot/reset vendor events, and DDC priority. Realtek covers all upstream USB IDs and 13 controller descriptors, epatch extension/table parsing, ROM patch choice, config append, download-index wrap/end markers, reset retry, and firmware lookup. The legacy 8723A download remains the same explicit no-op as upstream Bumble. |

### Classic Bluetooth (BR/EDR)
| Upstream (LOC) | Rust crate | Status | Notes |
|---|---|---|---|
| `rfcomm.py` (1.2k) | `bumble-rfcomm` | ✅ | **Complete synchronous protocol surface**: `RfcommFrame` TS 07.10 framing (SABM/UA/DM/DISC/UIH, 1- and 2-byte length indicators, credit-bearing UIH), CRC-8, and upstream's PN/MSC MCC catalog are oracle-pinned. `mux::{Multiplexer, Dlc}` covers session/DLC open, refusal, disconnect, modem-status exchange, credit stalls/replenishment/tuning/backpressure, buffered data, and the upstream 32-packet receive bound with oldest eviction. `l2cap::L2capMultiplexer` derives its frame ceiling from negotiated peer MTU and runs the session over live Classic L2CAP/ACL. Upstream sets `max_retransmissions = 0` and does not implement FCON/FCOFF aggregate-flow MCC commands; Rust preserves those boundaries and uses explicit polling instead of socket/async wrappers. |
| `sdp.py` (1.4k) | `bumble-sdp` | ✅ | **Complete synchronous protocol surface**: every `DataElement` encoding, including 1/2/4/8/16-byte signed and unsigned integers, all UUID/string/container widths, the upstream 32-level recursion guard, `ServiceAttribute` list/find/recursive-UUID helpers, and all seven `SdpPdu` messages are oracle-pinned. `service::{SdpServer, SdpClient}` implements matching, attribute selection, invalid-handle/error responses, continuation chunking/reassembly, and the watchdog; `l2cap::{SdpL2capServer, L2capSdpTransport}` carries fallible multi-round-trip queries over negotiated Classic channels. Rust construction/ownership replaces upstream async context-manager and event conveniences. |
| `at.py` (0.1k) + HFP AT models | `bumble-at` | ✅ | Parameter tokenizer/parser ported 1:1, nested values, HFP `AtCommand`/`AtResponse` forms, and incremental command (`\r`) / response (`\r\n`) stream framing. |
| `hfp.py` (2.1k) | `bumble-hfp` | ✅ | **Complete synchronous protocol surface**: normative HF/AG models and paired SLC state machines, serialized post-SLC command completion, call control/current-call listing, every default AG-indicator factory, HF indicators, ring/volume/typed caller-ID/typed voice events, codec request/selection, CMEE/CCWA/BIA/CLIP controls, HF/AG SDP record generation/discovery, and all eight upstream HFP 1.8 SCO/eSCO parameter presets. Control flows run end-to-end over RFCOMM/L2CAP and records through SDP client/server; negotiated CVSD/mSBC codecs establish and route audio through the host/controller link. Upstream `hfp.py` negotiates codec IDs and synchronous links but contains no CVSD/mSBC media encoder or decoder; Rust preserves that boundary and replaces asyncio/event-emitter scheduling with explicit queues and events. |
| `hid.py` (0.6k) | `bumble-hid` | ✅ | Complete HIDP message codec (handshake/control/get+set report/get+set protocol/data), open protocol identifiers, exact little-endian GET_REPORT buffer sizing, host/device dispatch, callback-to-handshake mapping, suspend/unplug events, role-correct input/output reports, MTU enforcement, and paired control (`0x0011`) + interrupt (`0x0013`) transports over live Classic L2CAP. |
| `avdtp.py` (2.4k) | `bumble-avdtp` / `bumble-a2dp` | ✅ | All 38 upstream signaling command/accept/reject forms, endpoint descriptors, generic and media-codec capability TLVs, safe fragmentation, local endpoint dispatch, atomic multi-SEP validation, lifecycle/event capture, transaction labels, and live Classic L2CAP signaling are present. The high-level initiator/stream orchestration, RTP media channel, packet sources, and SDP discovery live in `bumble-a2dp`; synchronous drive callbacks and explicit event collections replace asyncio listeners/pumps. |
| `a2dp.py` (1.0k) | `bumble-a2dp` | ✅ | Open codec identifiers and exact SBC, MPEG-2/4 AAC, vendor-specific, and Opus capability models; upstream byte vectors; SBC/ADTS AAC/Ogg Opus parsers and RTP packet sources; live Classic L2CAP media transport; source/sink SDP records; and a high-level initiator that discovers SEPs, verifies media transport + codec compatibility, and drives configure/open/start/suspend/close over AVDTP. Async generators/listeners are represented by synchronous collections and a caller-supplied drive callback. |
| `rtp.py` (0.1k) | `bumble-rtp` | ✅ | Slice 32 ports RTP v2 media packet parsing/serialization with marker/payload type, wrapping sequence/timestamp fields, SSRC and correctly spaced CSRC entries. It additionally implements standard header extensions and padding, validates bit fields/lengths, and returns errors for truncated input instead of upstream's unchecked indexing. |
| `avc.py` (0.5k) | `bumble-avc` | ✅ | Open subunit/opcode/command/response/operation identifiers; generic command and response frames; single and double-extended subunit IDs; 24-bit-company vendor-dependent frames; and panel pass-through press/release operations with bounded operation data. These are the only two typed opcode subclasses upstream defines; all other opcodes round-trip through the generic raw form. Upstream AVRCP vectors are byte-pinned and malformed frames return errors. |
| `avctp.py` (0.3k) | `bumble-avctp` | ✅ | Transaction labels, single/start/continue/end packets, command/response and IPID flags, 16-bit PIDs, safe fragmented-message assembly, MTU-aware outbound fragmentation, and a live Classic L2CAP binding are complete. Registered PIDs receive commands, unknown PIDs automatically produce IPID responses, and explicit message queues plus the higher AVRCP runtime replace Python callback registration. |
| `avrcp` (2.9k) | `bumble-avrcp` | ✅ | Slices 41–46 port the complete typed wire catalog, bounded controller/target runtime, delegate behavior, interim→changed notifications, pass-through keys, both fragmentation layers over live Classic L2CAP, and controller/target SDP records + discovery. The browsing PSM is advertised exactly when supported; upstream itself does not implement a separate browsing-channel runtime. Async iterators are represented by explicit `RuntimeEvent` values. |
| `codecs.py` (0.5k) | `bumble-codecs` | ✅ | Complete bit reader/writer plus MPEG-4 LATM `AudioMuxElement`, `StreamMuxConfig`, `AudioSpecificConfig`, GA config, AAC-LC constructor, arbitrary-length payload framing, and ADTS conversion. Upstream's long LATM fixture produces the exact ADTS oracle; unaligned bit chunks and 255/510-byte length boundaries round-trip safely. The same crate also owns the separately tracked G.722 decoder. |
| `audio/io.py` (0.6k) | `bumble-audio` | ✅ | Complete synchronous surface: `PcmFormat`, frame sizing, raw stream/file input, non-blocking threaded stream/file output, format-expanded subprocess output, input/output specification factories, and RIFF/WAVE 16-bit PCM parsing with upstream-compatible rewind-on-EOF looping. The optional `sound-device` feature supplies CPAL-backed global-index/default-device selection and enumeration, non-blocking float32 output, blocking int16 input, mono-to-stereo duplication, and upstream's reported stereo input format without imposing an audio backend on headless builds. |

### Profiles & apps
| Upstream | Rust crate | Status | Notes |
|---|---|---|---|
| `profiles/*` — all 23 modules | `bumble-profiles` | ✅ | All upstream profile modules are live: GAP/GATT/Battery/Device Information/Heart Rate, ASHA/HAP/CSIP, VCS/VOCS/AICS, MCP/GMCS, LE Audio metadata plus BAP/PACS/ASCS/BASS/CAP/TMAP/GMAP/PBP, and AMS/ANCS. Services, typed proxies, control/state runtimes, assigned-number and vendor UUID catalogs, strict wire models, encryption requirements, notifications/indications, and included-service discovery are covered by live tests. |
| `bridge.py` (0.1k) | `bumble-transport::HciBridge` | ✅ | Separate host/controller sources and sinks, directional single-packet pumping, typed replacement filters, responses short-circuited to the sender, post-filter directional tracing, EOF reporting, and transport-error propagation. |
| `apps/show.py` | `bumble-show` (`bumble-transport`) | ✅ | Runnable H4/BTSnoop capture decoder with upstream `--format` and repeatable Android/Zephyr `--vendor` options, typed HCI parsing, direction/timestamp output, and explicit truncated-record reporting. Rust vendor codecs are statically linked rather than dynamically registered. |
| `apps/controller_info.py` | `bumble-controller-info` (`bumble-transport`) | ✅ | Runnable external-controller inspection with reset, optional primed latency probes, symbolic local version/address/name, LE features, all 338 upstream Supported Commands labels, Classic + LE buffers, data/advertising limits, minimum connection intervals, named standard/vendor codecs and transports, typed voice fields, and V2-to-V1 LE buffer fallback. Unsupported commands are skipped and interleaved asynchronous packets are preserved. |
| `apps/controller_loopback.py` | `bumble-controller-loopback` (`bumble-transport`) | ✅ | Runnable local-controller loopback benchmark with the full packet-size/count, ACL/SCO, throughput/RTT, interval, and transport CLI. It validates advertised loopback support, controller buffer limits, the written/read-back mode, waits for the matching connection type, sends bounded in-flight data, validates ordered echoed counters/handles, reassembles ACL/L2CAP packets, and reports RX/TX or RTT statistics. |
| `apps/controllers.py` | `bumble-controllers` (`bumble-transport`) | ✅ | Runnable two-controller software radio over arbitrary split external HCI transports. A serialized shared-link pump routes host commands, ACL/SCO/ISO payloads, advertising and connections, LE control/PAST, Classic LMP, and all resulting host events without aliasing controller state. |
| `apps/hci_bridge.py` | `bumble-hci-bridge` (`bumble-transport`) | ✅ | Runnable full-duplex host/controller bridge with upstream direct-opcode and OGF:OCF success short circuits. Independent read/write halves cover every external transport: file, raw HCI socket, serial, TCP, UDP, USB, VHCI, PTY, Unix, WebSocket, Android emulator, and Android netsim. |
| `apps/gatt_dump.py` | `bumble-gatt-dump` (`bumble-transport`) | ✅ | Runnable external-controller GATT dump in initiator or advertising/listener mode, with address-or-active-name resolution, device-config local address selection, complete service/characteristic/descriptor and all-attribute discovery, per-attribute reads, bounded transport/procedure errors, and real `--encrypt` LE Secure Connections pairing. |
| `apps/device_info.py` | `bumble-device-info` (`bumble-transport`) | ✅ | Runnable external-controller device inspection in initiator or advertising/listener mode, with address-or-active-name resolution, optional real LE encryption, complete service/characteristic discovery, and typed GAP, Device Information, Battery, TMAP, PACS, and VCS reads. Profile-specific protocol errors are reported without suppressing later sections. |
| `apps/l2cap_bridge.py` | `bumble-l2cap-bridge` (`bumble-transport`) | ✅ | Runnable LE credit-based L2CAP-to-TCP bridge with the full upstream client/server, device-config, transport, PSM, credit, MTU/MPS, TCP host, and TCP port surface. The client accepts repeated local TCP connections over one LE ACL; the server accepts CoC channels and opens remote TCP connections. Both directions honor controller flow control and withhold CoC receive credits under TCP backpressure. |
| `apps/rfcomm_bridge.py` | `bumble-rfcomm-bridge` (`bumble-transport`) | ✅ | Runnable Classic RFCOMM-to-TCP bridge with the full upstream client/server, device-config, transport, trace, channel, UUID, TCP, authentication, and encryption surface. Channel zero advertises or resolves the configured UUID through SDP, repeated DLCs reuse one RFCOMM session, and receive credits are withheld under TCP backpressure. |
| `apps/gg_bridge.py` | `bumble-gg-bridge` (`bumble-transport`) | ✅ | Runnable Golden Gate Gattlink bridge with the complete upstream transport/address/role and UDP endpoint CLI. Node mode publishes the RX, TX, and CoC-PSM GATT service and advertises; hub mode discovers/subscribes and prefers LE CoC. Both roles retain GATT fallback, exact one-byte packet framing, bounded UDP queues, and controller-aware backpressure. |
| `apps/player/player.py` | `bumble-player` (`bumble-transport`) | ✅ | Runnable Classic A2DP source with the complete upstream `discover`, `inquire`, `pair`, and `play` command surface. It publishes the source SDP record, persists SSP link keys, discovers sink endpoints, configures SBC/AAC/vendor Opus, opens and controls AVDTP streams, paces RTP media with controller backpressure, and serves AVRCP over incoming AVCTP. |
| `apps/speaker/speaker.py` | `bumble-speaker` (`bumble-transport`) | ✅ | Runnable Classic A2DP sink with the complete upstream codec, sampling-frequency, bitrate, VBR, discovery, output, UI, peer, device-config, and transport surface. It accepts incoming Classic connections or initiates authenticated/encrypted ones, publishes the sink SDP record, negotiates SBC/AAC/vendor Opus through AVDTP, extracts received RTP audio to files or `ffplay`, and serves the live browser UI over HTTP/WebSocket. |
| `apps/console.py` | `bumble-console` (`bumble-transport`) | ✅ | Runnable scriptable interactive LE console over external controllers. It preserves the upstream scan/filter/RSSI, advertising, connect/disconnect, parameter, encryption, MTU, PHY, GATT discovery/read/write/subscription, local-write, status-view, and exit command grammar. The Python fullscreen widgets become terminal views while live controller events remain continuously pumped. |
| `apps/auracast.py` | `bumble-auracast` (`bumble-transport`) | ✅ | Runnable Auracast scanner, BASS broadcast assistant, pairing client, LC3 receiver, and multi-broadcast transmitter over external controllers. It preserves the upstream five-command CLI, TOML broadcast lists, Broadcast Code encoding, BAP/PBP announcements, periodic synchronization, PAST, BIG/BIS setup, and portable PCM input/output paths. The optional platform sound-device backend remains tracked under `bumble-audio`; file, stdio, and `ffplay` paths are live. |
| `apps/bench.py` | `bumble-bench` (`bumble-transport`) | ✅ | Runnable external-controller benchmark with the complete central/peripheral, send/receive/ping/pong, and GATT/LE-CoC/RFCOMM/CIS mode matrix. It preserves the exact benchmark packet and stream framing, SDP channel discovery, RFCOMM credit tuning, ATT MTU, LE connection/scan/advertising/data-length/PHY controls, Classic role switch/authentication/encryption, directional CIG parameters, pacing, repeats, throughput, RTT, windowed-rate, and jitter statistics. |
| `apps/lea_unicast/app.py` | `bumble-lea-unicast` (`bumble-transport`) | ✅ | Runnable LE Audio unicast sink/source over external controllers with the upstream UI-port, device-config, transport, and WAVE-input CLI. It publishes GAP, PACS, and ASCS, advertises the unicast-server announcement, negotiates sink/source ASEs, accepts and binds CIS links, decodes received LC3 to the browser UI, and resamples/encodes looping WAVE PCM into source ISO SDUs. |
| `apps/pair.py` | `bumble-pair` (`bumble-transport`) | ✅ | Runnable LE, Classic, and simultaneous dual-mode listener paths over external controllers with the complete upstream option surface. LE supports direct address/name connection or configurable advertising, Legacy/SC pairing, and OOB data. Classic supports inquiry/name resolution, incoming/outgoing ACL setup, PIN and Secure Simple Pairing delegates, stored link-key reuse, controller encryption, and best-effort SMP-over-BR/EDR CTKD for P-256 link keys. Both paths provide bond policy, JSON key persistence/printing, and linger behavior. |
| `apps/scan.py` | `bumble-scan` (`bumble-transport`) | ✅ | Runnable external HCI scanner with upstream RSSI/passive/interval/window/PHY/duplicate/raw/IRK/key-store/device-config options, extended scanning with legacy fallback, typed legacy + extended report decoding, exact active/passive scan-response accumulation, labeled AD rendering, RSSI bars, and real RPA identity resolution. |
| `apps/usb_probe.py` | `bumble-usb-probe` (`bumble-transport`) | ✅ | Runnable libusb device inventory with upstream `--verbose`, `--hci-only`, manufacturer, and product filters; device/interface-level Bluetooth HCI classification; stable index, VID/PID, duplicate, and serial transport names; string-descriptor error tolerance; and verbose configuration/interface/endpoint details including isochronous packet sizes. |
| `apps/ble_rpa_tool.py` | `bumble-rpa-tool` (`bumble-smp`) | ✅ | Runnable `gen-irk`, `gen-rpa`, and `verify-rpa` commands backed by the OS RNG and the real SMP `ah` primitive. Flexible Python-style hex input, address validation, colored verification results, and malformed/extra argument errors are covered. |
| `apps/unbond.py` | `bumble-unbond` (`bumble-transport`) | ✅ | File-backed and external-controller list/delete modes are runnable with upstream-style colored key rendering and `!!! pairing not found` behavior. Controller mode resets HCI, discovers the public BD_ADDR, falls back to the configured random address, reproduces `JsonKeyStore[:filename]` namespace/path selection, and uses an in-memory store for absent or unknown keystore types. |
| `apps/pandora_server.py` + `pandora/` | `bumble-pandora` | ✅ | Canonical bt-test-interfaces v0.0.6 protobufs, the runnable server/configuration surface, exact Pandora data-type conversion, and every upstream Host, Security, SecurityStorage, and L2CAP RPC are ported. Host, LE Security/bond storage, and LE CoC are verified through real gRPC clients and the two-controller software radio; Classic Security uses a reactive scripted-HCI proof because upstream's software controller intentionally has no Classic-authentication handler. |

### Roughly where that leaves things

The codec and protocol inventory is now broad rather than LE-only: HCI,
L2CAP/ERTM/LE CoC, ATT/GATT, SMP/CTKD/privacy/signing, SDP, RFCOMM, HFP,
AVDTP/A2DP, AV/C/AVCTP/AVRCP, HID, portable audio I/O, transports/drivers, and all 23 profile
modules have live Rust implementations. Legacy, extended, and periodic LE
advertising/scan/sync/connect paths run end-to-end through the high-level `Device`.

The completion audit is therefore concentrated on deeper orchestration rather
than missing wire catalogs: richer host orchestration and listener conveniences,
platform-specific transport edges, and Python-only harness/app
surfaces. Asyncio listeners and generators are represented by explicit events,
queues, and caller-supplied drive callbacks throughout the synchronous port.

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
- **`ReturnParameters`** — typed Command_Complete return parameters for the
  controller-information query surface (local version/commands/features,
  Classic + LE buffer sizes, LE features/data/advertising limits and minimum
  connection intervals, voice setting, address/name, and supported codecs),
  with the status-based short-response fallback plus a `Raw` fallback.
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
oracle-pinned. At this checkpoint the async bearer, `gatt_adapters` typed-value
proxies, and event listeners were deferred; later slices port the synchronous
equivalents, including real EATT credit-channel bearers in Slice 154.

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

At this checkpoint the remaining GATT difference was its asynchronous
bearer/event convenience surface. Slice 154 later closes the protocol gap with
real EATT bearers while retaining the port's synchronous polling model.

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

## Slice 78 — what's here

The Linux virtual-controller device protocol now has a real transport:

- `VhciTransport::open` uses `/dev/vhci` by default or an explicit path from a
  `vhci:` transport name.
- Initialization writes Bumble's `[HCI_VENDOR_PACKET, HCI_BREDR]` controller
  configuration, consumes the four-byte vendor response, and exposes its
  big-endian HCI adapter index before switching to normal H4 framing.
- Malformed non-vendor bootstrap replies fail explicitly rather than entering
  the standard packet parser in a corrupted state.
- A bidirectional stream test acts as the kernel endpoint, verifies the exact
  bootstrap bytes and adapter index, then carries an event and command across
  the initialized transport.

The next direct-hardware layer adds Bluetooth USB controller discovery and HCI
transfer routing; Linux raw HCI sockets follow it.

## Slice 79 — what's here

Bluetooth USB controllers now have a live libusb-backed transport:

- `UsbSpec` ports index, `vendor:product`, duplicate occurrence, serial-number,
  bus/port-path, forced-interface, and `+sco=` selector syntax with strict
  validation.
- Device discovery recognizes Bluetooth HCI at either the device or interface
  class level. Interface selection requires interrupt-in plus bulk-in/out on one
  alternate setting, then configures, claims, and selects that setting.
- Command packets use the class/device control request; ACL uses bulk endpoints;
  ISO output follows upstream PyUSB's bulk-out compatibility path. Event and ACL
  reads are fairly interleaved so sustained traffic cannot starve either source.
- The vendored `rusb` backend makes builds independent of a system libusb
  installation. The mockable transfer boundary verifies exact endpoints,
  request fields, packet-type restoration, timeout handling, partial writes,
  and disconnect propagation without claiming attached hardware.
- `usb:` and `pyusb:` transport dispatch expose vendor/product/bus/address
  metadata. SCO/isochronous transfers fail explicitly because `rusb`'s blocking
  API does not expose them; that remains a separate asynchronous-backend task.

Linux raw HCI sockets are the remaining direct local-controller transport gap.

## Slice 80 — what's here

Linux hosts can now attach directly to a kernel Bluetooth adapter through its
exclusive HCI user channel:

- `HciSocketSpec` accepts Bumble's empty/default selector and decimal 0-based
  adapter indices with bounded `u16` validation. `HciSocketAddress` exposes the
  exact six-byte native `sockaddr_hci` layout used for adapter/channel binding.
- `RawHciSocket` owns an `AF_BLUETOOTH`/`SOCK_RAW`/`BTPROTO_HCI` descriptor,
  binds `HCI_CHANNEL_USER`, closes it through `OwnedFd`, and uses checked
  blocking `recv`/`send` calls. Non-Linux opens fail with a precise unsupported
  error rather than exposing a placeholder endpoint.
- `HciSocketTransport` retains the shared H4 framer, so complete kernel packets,
  fragments, and coalesced reads all produce the same typed packets. A partial
  datagram send is rejected instead of silently dropping the packet tail.
- `hci-socket`, `hci-socket:`, and `hci-socket:<index>` participate in standard
  transport dispatch. Five mock-backed tests cover selectors, ABI bytes,
  fragmentation/coalescing, queued packets, complete/partial sends, truncation,
  and I/O failures; the Linux-only syscall branch is additionally compiled for
  `x86_64-unknown-linux-musl`.

The remaining transport breadth is Android netsim integration and narrower
platform-specific endpoints; USB SCO input still needs an isochronous-capable
backend.

## Slice 81 — what's here

The standalone Android emulator's two HCI-facing gRPC services are now live:

- `AndroidEmulatorSpec` ports the default `localhost:8554` endpoint, explicit
  server addresses, and `mode=host|controller`. Host mode calls
  `EmulatedBluetoothService/registerHCIDevice`; controller mode calls
  `VhciForwardingService/attachVhci`.
- The checked-in protobuf schema preserves the emulator's exact package,
  service, method, enum, and field tags. A vendored `protoc` makes generated
  tonic clients reproducible without a host tool installation; tonic 0.13 is
  used to retain the workspace's Rust 1.87 compatibility.
- A dedicated current-thread Tokio worker owns the bidirectional gRPC stream
  while `AndroidEmulatorTransport` presents the same synchronous
  `PacketSource`/`PacketSink` contract as every other backend. Shutdown is
  explicit, worker errors are preserved, and request-channel closure is never
  reported as a successful write.
- `AndroidEmulatorPacket` splits and restores the H4 type byte exactly for
  command, ACL, SCO, event, and ISO packets. Mock tests cover mapping and error
  propagation; a real local tonic server proves both RPC paths end-to-end.
- `android-emulator:` dispatch is active. The shared metadata parser now also
  distinguishes `[::1]` IPv6 literals from `[key=value]` metadata and passes
  all four prefix/suffix forms exercised by upstream.

Android netsim remains the next emulator transport; it uses a distinct startup
and packet-streaming protocol rather than these emulator services.

## Slice 82 — what's here

Android netsim's separate `PacketStreamer` protocol now works in both roles:

- `AndroidNetsimSpec` accepts the upstream empty/INI form, explicit IPv4/IPv6
  `<host>:<port>` endpoints, `mode=host|controller`, `instance`, `name`, and
  `variant` options. Platform-specific `netsim[_n].ini` discovery reads the
  exact `grpc.port` key; controller publication uses create-new semantics so an
  existing simulator registration is never overwritten.
- The protobuf subset preserves the used `netsim.common`, `netsim.startup`, and
  `netsim.packet` package names, message fields, oneof tags, enum values, and
  `/netsim.packet.PacketStreamer/StreamPackets` RPC path. Exact request,
  response, HCI, and error bytes are pinned in tests.
- Host mode sends `ChipInfo` before HCI traffic, including Bluetooth kind,
  Bumble identity, build/architecture, and variant fields. Remote error and raw
  packet responses remain distinct from typed HCI packets.
- Controller mode is a real tonic server with an exclusive device lease. It
  rejects unsupported chip kinds and concurrent devices, releases leases on
  disconnect, drops output when no device is attached like upstream, and
  force-cancels active streams during local shutdown to avoid a blocking drop.
- `android-netsim:` dispatch exports resolved mode/host/port metadata. A live
  test starts an ephemeral controller, discovers its published INI port when
  available, exchanges command/event packets with a host, rejects a second
  host as `Device busy`, and shuts down the controller before its client.

The main upstream transport catalog is now represented. Remaining transport
work is the explicitly narrower USB SCO/isochronous input and serial DSR/DTR
paths plus platform-specific integrations outside this catalog.

## Slice 83 — what's here

Intel's USB firmware driver now has a complete controller-initialization engine:

- `driver=intel[/...]` forcing and the AX210/AX211/BE200 USB IDs select the
  driver. `ddc_override` and `ddc_addon` metadata use the upstream hex syntax,
  with override/file/addon precedence preserved.
- Open-ended Intel version TLVs retain unknown values while decoding CNVI/CNVR
  nibble mapping, hardware platform/variant, mode, build/timestamp, security,
  USB IDs, and public address fields. Truncated or wrongly sized known TLVs are
  rejected rather than indexing past their input.
- RSA and ECDSA SFI layouts produce the exact CSS, PKI, signature, and command
  stream data types. Secure-send payloads are capped at 252 bytes, embedded HCI
  commands are grouped on four-byte boundaries, and write-boot-params extracts
  the post-download boot address.
- The `DriverHost` contract supports command-complete transactions, batched
  firmware commands, no-response resets, and vendor-event waits. Intel accepts
  the bootloader's expected Unknown HCI Command reset response, loads firmware,
  waits for download/boot events, resets to the extracted address, and applies
  length-prefixed DDC records.
- Firmware lookup preserves the environment override, project, package, Linux
  system, and current-directory order. Tests cover exact vendor command bytes,
  lookup override semantics, malformed firmware/TLV/DDC inputs, already-loaded
  DDC handling, and a complete scripted cold start.

## Slice 84 — what's here

Realtek firmware initialization and the shared driver selector complete
`bumble.drivers`:

- The upstream Realtek USB ID set and all 13 ROM/HCI descriptors select the
  correct 8723/8761/8821/8822/8852 firmware and config names, including the
  8761CU HCI-version wildcard and required-config variants.
- `Firmware` validates the `Realtech` and extension signatures, walks extension
  instructions backward to the project ID, bounds-checks the parallel chip ID,
  length, and offset tables, extracts SVN versions, and replaces each patch tail
  with the epatch firmware version exactly like upstream.
- Probing retries the initial Reset after the upstream 200 ms timeout, parses
  Read Local Version Information, reads the ROM revision, selects chip ID
  `rom_version + 1`, appends optional configuration, and sends 252-byte vendor
  fragments with seven-bit index wrapping and the high-bit final marker.
- Exact command bytes, project mappings, malformed extensions/tables, 130-way
  index wrap, config-required refusal, timeout retry, and a complete
  probe/download/diagnostic-read/reset sequence are pinned in tests. The older
  8723A path deliberately retains upstream Bumble's explicit download no-op.
- `get_driver_for_host` honors a forced driver (discarding runtime options for
  class selection), refuses unknown forced names, and otherwise probes Realtek
  before Intel in upstream order.

## Slice 85 — what's here

The first five `bumble.profiles` modules now compose directly with the live
GATT server/client:

- Generic Access publishes the UTF-8 device name (with the upstream 248-byte
  limit) and packed `Appearance`; its proxy decodes both typed values.
- Battery Level is a dynamic per-bearer callback with READ/NOTIFY properties,
  an automatic CCCD, and a typed one-byte proxy.
- Device Information conditionally builds all six UTF-8 revision/identity
  fields, the regulatory byte list, and the 24-bit-OUI/40-bit-manufacturer
  System ID with checked packing and a typed proxy.
- Heart Rate covers open body-sensor locations, every 8/16-bit measurement flag
  combination, sensor contact, energy, RR intervals in 1/1024-second units,
  dynamic measurement reads, notification CCCD, and the reset-energy control
  point with application error `0x80` for unsupported commands.
- Generic Attribute exposes Service Changed, client/server feature, and Database
  Hash characteristics under the upstream enablement rules. `GattServer` now
  serializes the exact hash-eligible declaration/descriptor set and computes
  zero-key AES-CMAC; the upstream `F1CA2D48ECF58BAC8A8830BBB9FBA990`
  database vector passes through live discovery and readback.

## Slice 86 — what's here

The first hearing profiles extend `bumble-profiles` with ASHA and CSIP:

- ASHA exposes the exact 17-byte read-only properties payload, control/status/
  volume/PSM characteristics, four-byte HiSyncId service advertising, live
  START/STOP/STATUS and volume state transitions, discovery proxy, and an audio
  ingress callback suitable for binding to an LE credit-based channel.
- CSIP implements the Bluetooth `s1`, `k1`, `sef`/`sdf`, `sih`, and RSI
  operations with the upstream cryptographic vectors. Its optional size, lock,
  and rank characteristics enforce encrypted reads, while SIRKs can be served
  plaintext or encrypted from a bearer-aware LTK/LinkKey callback and decoded
  by the client proxy.
- Live encrypted ATT tests cover both SIRK modes and every optional
  characteristic; malformed key/random/value lengths and the RSI random-bit
  requirements are checked without panics.

## Slice 87 — what's here

The Volume Control profile family now composes through real encrypted ATT:

- VCS implements every relative, unmute-relative, absolute, mute, and unmute
  procedure with saturating steps, change-on-success counter increments,
  persisted flags, and the `0x80`/`0x81` application-error paths.
- Secondary VOCS services expose signed volume offsets, the full 32-bit Audio
  Location flag set, UTF-8 output descriptions, and checked offset control with
  counter, opcode, and `-255..=255` range enforcement.
- Secondary AICS services expose typed input state, gain properties, status,
  type, and description values. All five control procedures preserve Bumble's
  manual/automatic-only, mute-disabled, counter, and gain-range behavior.
- `GattClient` now discovers secondary services and Include declarations,
  including mixed 16-bit and 128-bit service UUIDs. A live VCS test discovers
  its included VOCS and AICS records and constructs both proxies from them.

## Slice 88 — what's here

Media Control and Generic Media Control now run over the live GATT client/server:

- The complete open playing-order, media-state, control-opcode, result,
  supported-opcode, search-item, and object-type catalogs are present. The
  48-bit Object ID and seven-byte Group Object models reject malformed or
  overflowing values and round-trip byte-exactly.
- MCS and GMCS publish the same nine server characteristics as Bumble with
  encrypted access and automatic notification CCCDs. Control writes enqueue
  the upstream `[opcode, SUCCESS]` response for delivery as a real GATT
  notification.
- The proxy recognizes all 22 standard optional MCP characteristics, subscribes
  to the six event-bearing values, and decodes control responses, state, track
  change/title, duration, and position into typed synchronous events. Tests
  cover subscription caching, opcode correlation, malformed values, and the
  full write-to-notification handshake.
- The inventory denominator is corrected from 24 to the 23 actual Python
  modules in `bumble/profiles` (excluding `__init__.py`).

## Slice 89 — what's here

The LE Audio data foundation and Published Audio Capabilities Service are live:

- Metadata parses and emits extensible LTV entries without assuming tag
  uniqueness, decodes contexts/text/language/CCIDs/rating/active/assisted
  values, preserves unknown tags, and rejects zero-length or truncated input.
- The BAP foundation covers all Audio Input Type, Context Type, sampling
  frequency, frame-duration, and Audio Location values; channel-count bitsets;
  unicast server advertising; and checked codec-capability/configuration LTVs.
- PAC records support standard LC3-style typed capabilities and raw vendor
  capabilities, exact five-byte HCI Coding Formats, metadata, counted lists,
  and Bumble's `ffe000ffff0000` vendor record vector.
- PACS publishes supported/available sink/source contexts plus optional sink and
  source PAC/location characteristics. The typed proxy discovers and reads all
  variants, while the available-context notification CCCD is verified live.

## Slice 90 — what's here

The compact LE Audio role and public-broadcast profiles are live:

- TMAP publishes the complete 16-bit Telephony and Media Audio role mask and a
  typed proxy discovers and reads it over a live GATT database.
- GMAP conditionally publishes the four gaming-feature characteristics from the
  configured role mask. Gateway, terminal, broadcast sender, and broadcast
  receiver feature bits preserve the upstream assigned-number layout, and the
  proxy represents omitted role characteristics explicitly.
- PBP encodes and decodes public-broadcast feature flags plus LE Audio metadata,
  emits UUID 0x1856 service-data advertising, preserves unknown feature bits,
  and rejects truncated or length-inconsistent announcements.

## Slice 91 — what's here

Basic Audio Profile announcements and Audio Stream Control are live:

- Broadcast Audio Announcement validates and emits the 24-bit Broadcast ID;
  Basic Audio Announcement strictly parses and serializes presentation delay,
  subgroup codec/metadata, and per-BIS codec configurations, including 0x1851
  and 0x1852 service-data advertising.
- ASCS covers all eight ASE control operations and response/reason codes with
  exact counted wire forms, bounded 24-bit values, and rejection of truncated,
  trailing, or oversized fields.
- The live service publishes any number of sink/source ASE characteristics,
  handles dynamic reads and control-point writes, queues control and state
  notifications, and models codec, QoS, enable, CIS establishment, streaming,
  metadata, disable, release, and reset transitions. Its typed proxy discovers,
  subscribes, writes operations, reads states, and decodes notifications.

## Slice 92 — what's here

Broadcast Audio Scan and Common Audio are live:

- BASS strictly parses and emits remote-scan, add/modify source, broadcast-code,
  and remove-source operations; typed subgroup metadata; and both normal and
  bad-code Broadcast Receive State forms. Address byte order, 24-bit Broadcast
  IDs, counted metadata, and the 16-byte code are byte-pinned to upstream.
- The live BASS service publishes configurable encrypted receive-state slots,
  captures typed control operations, updates states, and queues notifications.
  Its proxy discovers all repeated state characteristics, subscribes, writes
  operations, and decodes empty or populated read/notification values.
- CAP's Common Audio Service includes the Coordinated Set Identification
  Service through a real GATT Include declaration, with a typed proxy that
  discovers and verifies the included CSIS instance.

## Slice 93 — what's here

The Hearing Access Profile is live:

- Hearing Aid Features, preset properties/records, every control opcode, Read
  Preset Response, and all four Preset Changed forms have strict typed codecs.
- The encrypted live service validates names and indices, serves sorted partial
  preset reads, rejects overlapping procedures and unavailable presets, updates
  writable names, wraps next/previous selection across available presets, and
  queues the specified indications and Active Preset Index notifications.
- Server APIs cover generic changes, deletion, availability transitions, and
  optional synchronized updates to the other member of a binaural set. The
  typed proxy discovers, subscribes, reads features/current index, writes all
  operations, and decodes both indication and notification channels.

## Slice 94 — what's here

Apple Media Service and Apple Notification Center Service complete the
23-module profile inventory:

- AMS publishes Apple's exact 128-bit service/characteristic UUIDs, captures
  remote commands and entity observations, retains full entity attributes, and
  emits supported-command and entity-update notifications. Its proxy/client
  subscribe, command, observe, recover truncated attributes through the Entity
  Attribute characteristic, and decode every player/queue/track value.
- ANCS covers notifications, all action/category/event/attribute identifiers,
  all three commands, maximum-length rules, app identifiers, dates, message
  sizes, and fragmented Notification/App Attribute response assembly.
- The live ANCS service/proxy/client capture control commands, subscribe to both
  data sources, emit and decode notification-source values, assemble arbitrarily
  fragmented data responses, serialize command access, and perform actions.

## Slice 95 — what's here

Extended LE advertising now runs through both the software controller and the
high-level host API:

- The controller retains multiple advertising sets with independent random
  addresses, full parameters, advertising and scan-response data, enable state,
  remove/clear behavior, and upstream-compatible maximum-length/set-count
  results. First/intermediate/last/complete operations reassemble extended data
  and unknown handles return the specified HCI error.
- Extended scanners receive typed `LE Extended Advertising Report` events with
  PHY, SID, power, address, data, and distinct scan-response records. The link
  propagates every enabled set and extended create-connection uses the actual
  set address on both sides.
- `Device` exposes typed extended advertising configuration, fragments payloads
  up to 1650 bytes into HCI-sized commands, starts/stops extended scanning,
  collects typed reports, and establishes extended LE connections without raw
  HCI. A live two-device test covers a 600-byte fragmented payload through scan
  and connection setup.

## Slice 96 — what's here

Connected Isochronous Streams now carry real host data after the existing CIS
control-plane handshake:

- Each CIS retains independently installed Host-to-Controller and
  Controller-to-Host data paths. Setup/Remove commands return the exact
  status-plus-handle shape and reject unknown handles, duplicate setup,
  uninstalled removal, and invalid directions.
- `LocalLink` routes HCI ISO fragments only when both directional paths exist,
  translates the source CIS handle to its peer handle, preserves PB/TS/sequence/
  status metadata, and produces Number Of Completed Packets flow events.
- `Device` configures CIGs, exposes allocated handles and incoming CIS requests,
  accepts streams, manages data paths, fragments SDUs at the controller's
  960-byte packet limit, wraps sequence numbers, and reassembles first/
  continuation/last fragments into typed `IsoSdu` values.
- The HCI ISO decoder now validates its declared data length against timestamp,
  SDU-info, and fragment bytes, rejecting both truncation and trailing data.
  Live tests carry a 2500-byte SDU and a second sequenced SDU across two hosts.

## Slice 97 — what's here

The previously unported `decoder.py` audio path is now part of
`bumble-codecs::g722`:

- The stateful 64 kbit/s decoder ports both adaptive sub-band decoders, lower
  and higher quantizers, scale-factor adaptation, Block 4 predictors, receive
  QMF, saturation, and signed 16-bit PCM output.
- Callers can receive native `i16` samples or upstream-compatible little-endian
  PCM bytes; state is retained across successive frame calls.
- All 320 PCM bytes generated from the first 80-byte upstream G.722 fixture
  frame match Python Bumble exactly, and decoding the frame in two chunks gives
  the same output as a single call.

## Slice 98 — what's here

The portable portion of upstream `audio/io.py` now lives in `bumble-audio`:

- `PcmFormat` parses the same `int16le,rate,channels` and
  `float32le,rate,channels` strings and exposes exact sample/frame sizing.
- Raw streams and files support frame-oriented input. Stream/file output uses a
  dedicated writer thread, so writes enqueue immediately while close drains and
  flushes the queue. Subprocess output expands sample-rate and mono/stereo
  placeholders and delivers PCM through standard input.
- The RIFF/WAVE reader validates PCM encoding, 16-bit sample width, block
  alignment, arbitrary intervening chunks and padding, then matches upstream's
  behavior of returning a short final frame and rewinding on the next read.
- Factories accept `stdin`, `stdout`, `file:`, implicit existing file paths,
  `auto` WAVE detection, and `ffplay`. Hardware device selection remains a
  platform-backend integration rather than a portable core behavior.

## Slice 99 — what's here

The two upstream vendor HCI modules now live under `bumble-hci::vendor`:

- Android LE vendor capabilities parse every historical response prefix while
  defaulting fields introduced by newer controller versions. APCF, activity
  energy, A2DP hardware-offload, and dynamic-buffer commands use exact Android
  OGF/OCF envelopes and retain subcommand-specific opaque payloads.
- Android Bluetooth Quality Report vendor events recognize the same seven
  report IDs as Python Bumble and safely decode the complete common telemetry
  block, signed TX power/RSSI, Bluetooth address, packet counters, and arbitrary
  vendor-specific suffix.
- Zephyr read/write TX-power commands and responses preserve open handle-type
  values, little-endian handles, and signed dBm values. Exact HCI command bytes,
  historical/truncated capability responses, malformed events, and both vendor
  return families are covered by focused tests.

## Slice 100 — what's here

Upstream `bridge.py` now maps to `bumble-transport::HciBridge`:

- Host and controller sources/sinks remain independently owned, matching
  transports that expose split read/write endpoints. Directional methods pump
  at most one packet and distinguish successful forwarding from source EOF.
- Per-direction filters may leave a packet unchanged, replace it before
  forwarding, or replace it with a response sent directly back to the packet's
  sender. Short-circuited responses are intentionally not traced, matching the
  upstream forwarder order.
- A shared callback sees the direction and final post-filter packet immediately
  before delivery. Focused tests cover unmodified bidirectional flow, EOF,
  replacement, host short-circuit, controller short-circuit, and trace order.

## Slice 101 — what's here

Periodic advertising now runs across controller, link, and high-level host APIs:

- Each extended advertising set retains validated periodic intervals,
  properties, up to 1650 bytes of fragment-assembled data, enablement, and the
  ADI request bit. Disabling the train stops its link emission without removing
  the parent extended set.
- Create Sync records one pending request and completes when a matching
  address/SID train crosses `LocalLink`. Cancel emits the host-cancelled
  establishment status; established handles support report receive toggling
  and termination with unknown-handle errors.
- Link reports split data at the exact 247-byte HCI event ceiling. `Device`
  reassembles complete/truncated report sequences, retains typed sync metadata,
  and exposes establishment errors and lost handles explicitly.
- A live two-device test sends a 600-byte train through three HCI report
  fragments, toggles reception, stops the advertiser, terminates the sync, and
  separately verifies pending-sync cancellation.

## Slice 102 — what's here

Periodic Advertising Sync Transfer (PAST) now crosses live LE ACL links:

- `LE_Periodic_Advertising_Sync_Transfer` validates the ACL and source sync
  handles, then carries the advertiser address/SID/PHY/interval plus service
  data to the peer. The peer allocates its own sync handle and emits the exact
  Sync Transfer Received meta-event fields.
- `LE_Periodic_Advertising_Set_Info_Transfer` derives the same transfer record
  directly from an enabled local periodic advertising set and rejects missing,
  disabled, or unconnected state with the appropriate HCI status.
- `Device` exposes both transfer forms, retains typed transfer metadata, installs
  the received sync, and immediately receives subsequent periodic reports.
  Tests cover direct set-info transfer between two connected devices and sync
  transfer from a synchronized sender to a third connected peer.

## Slice 103 — what's here

Classic role negotiation now follows upstream controller behavior:

- `Create_Connection` retains `allow_role_switch`; accepting as Central sends a
  switch request before completing the ACL, while accepting as Peripheral
  completes directly. A denied switch fails both endpoints with HCI status
  `0x21` and no leaked pending connection.
- Explicit `Switch_Role` handles no-op requests locally and otherwise updates
  both controllers through request/accept/reject LMP PDUs, emitting matching
  `Role Change` events on each host.
- The high-level `Device` defaults Classic connect/accept to the upstream roles,
  exposes role-selecting accept and explicit switch helpers, and tracks the
  established local role. Tests cover both upstream accept-role tuples,
  rejection policy, explicit switching, and host-visible roles.

## Slice 104 — what's here

High-level connection ownership now matches upstream's handle-indexed model:

- `Device` retains every established LE and Classic ACL in ordered handle maps,
  with peer lookup, explicit selection, and backward-compatible current-handle,
  role, and peer accessors. Disconnecting the selected link falls back to a
  remaining connection without discarding unrelated state.
- ATT, raw L2CAP, encryption, PAST, and CIS operations expose handle-specific
  forms. ATT/L2CAP/security-request inboxes retain the receiving handle, while
  the original aggregate drain methods remain available.
- The host pump now advances Classic LMP traffic too. Three-device tests keep
  two LE and two Classic links live at once, route distinct payloads to each
  peer, exercise selection, and verify isolated disconnect cleanup.

## Slice 105 — what's here

LE credit-based channels now run through the high-level host and controller:

- Each LE connection handle owns a `LeCreditChannelManager`. Device-wide server
  registration is copied to current and future links, while connect, enhanced
  connect, reconfigure, send, receive, and disconnect operations select the ACL
  explicitly.
- Incoming LE signaling and known dynamic CIDs are routed into the manager;
  generated credit, response, data, and disconnect PDUs are wrapped back through
  the host's controller-sized HCI ACL fragmentation and flow-control queue.
- A live two-device test negotiates a one-credit channel, transfers long SDUs in
  both directions across HCI fragments and credit replenishment, reconfigures
  MTU/MPS, disconnects cleanly, and asserts no manager errors.

## Slice 106 — what's here

Upstream's HCI capture surface is now a real Rust library capability:

- `BtSnooper` emits the `btsnoop\0` header, H4 data-link type, command/event
  direction flags, drops, microsecond timestamps, and big-endian records exactly
  as `snoop.py`; `PcapSnooper` emits PCAP 2.4 HCI-H4 pseudo-header records and
  flushes each packet for live consumers.
- `SnooperSpec` covers BTSnoop files plus PCAP files/pipes, and `FileSnooper`
  opens the selected sink. Timestamps have deterministic entry points for exact
  tests as well as real-time defaults.
- `SnoopingTransport` records controller-to-host reads and host-to-controller
  writes without altering packets. Tests pin exact writer bytes and both wrapper
  directions.

## Slice 107 — what's here

Capture inspection now completes the first runnable upstream application:

- `BtSnoopReader` validates BTSnoop headers, supports upstream's H1 and H4 data
  links, bounds record allocations, preserves flags/drop counts/timestamps, and
  reconstructs H1 packet types. Truncated records remain inspectable but are
  never passed to typed HCI decoding.
- The `bumble-show` binary reads raw H4 streams through the production framer or
  BTSnoop records through the new reader, then prints typed HCI packets with
  direction and microsecond timestamps. It accepts the upstream format and
  repeatable Android/Zephyr vendor options; those catalogs are statically linked
  in Rust.
- Exact reader round trips cover both directions, Unix timestamp conversion,
  H1 reconstruction, drops, truncation, bad headers, and unsupported links.
  Binary tests exercise both input formats end to end.

## Slice 108 — what's here

The upstream BLE RPA command-line utility is now runnable in Rust:

- `bumble-rpa-tool gen-irk` produces a fresh 128-bit IRK from the operating
  system RNG; `gen-rpa` creates an RPA from that IRK; and `verify-rpa` checks an
  IRK/address pair with the SMP `ah` primitive and upstream-compatible colored
  results.
- `verify_resolvable_private_address` is a public library helper and rejects
  non-resolvable addresses before comparing the generated hash. The existing
  address resolver and the CLI share this same privacy implementation.
- Tests cover the upstream `ah` vector, correct/wrong keys, non-RPAs, all three
  commands, Python-style whitespace-tolerant hex, generated-value round trips,
  and malformed keys, addresses, commands, and arity.

## Slice 109 — what's here

Pairing-key removal now has a runnable file-backed application path:

- `bumble-unbond --keystore-file` lists all entries or atomically deletes one
  address. Optional namespace selection handles multi-controller JSON files,
  and rendering includes every Rust pairing-key field with upstream-style
  address/property coloring.
- `JsonKeyStore::delete` now returns `KeyStoreError::NotFound` for a missing
  peer, matching upstream's `del key_map[name]`; the CLI translates that to
  `!!! pairing not found`. The memory backend intentionally keeps upstream's
  no-op-on-missing behavior.
- Tests create a real namespaced store, list key details, report a missing
  pairing, delete an existing pairing, verify persisted removal, and reject
  ambiguous arguments. Controller-backed mode is parsed but returns an explicit
  dependency error until external host bootstrap is ported.

## Slice 110 — what's here

External transports now have a reusable synchronous HCI command path:

- `HciCommandChannel<T>` sends any typed `Command` through a combined
  `PacketSource + PacketSink`, flushes it, and waits for the matching opcode's
  Command Complete or Command Status event.
- Interleaved advertising, vendor, ACL, or wrong-opcode response packets are
  preserved in arrival order for the caller instead of being discarded or
  mistaken for the current response. Clean EOF before a match is a named remote
  transport error rather than an infinite wait.
- `CommandResponse` exposes command credits, status, and typed return parameters.
  Mock-transport tests cover completion, status, unrelated-packet retention,
  exact outbound command/flush behavior, and premature EOF.

## Slice 111 — what's here

External controllers can now be inspected with a runnable information tool:

- `bumble-controller-info` accepts the upstream transport and latency-probe
  options, resets the controller, measures primed command latency, and reports
  every upstream controller-info query family while cleanly skipping unsupported
  commands and preserving unrelated asynchronous packets.
- Twelve additional Command Complete layouts are typed and byte-pinned:
  version/commands/features, Classic + LE V2 buffer sizing, LE features,
  suggested/maximum data length, advertising limits, minimum connection
  interval groups, and voice settings. Error statuses remain valid short
  responses while successful truncation is rejected.
- The software controller now returns spec-shaped values for each information
  query it advertises instead of successful empty stubs. Tests serialize and
  reparse every new controller response, covering the real HCI boundary rather
  than only in-memory enum matching.

## Slice 112 — what's here

USB controller discovery now has a runnable inspection application:

- `bumble-usb-probe` enumerates libusb devices without opening an HCI transport,
  recognizes Bluetooth HCI class tuples at either the device or interface
  level, and prints every supported Bumble USB selector for each device.
- Upstream filtering and selector semantics are preserved: HCI-only,
  manufacturer/product filters, ordinal HCI indices, VID/PID duplicate suffixes,
  and unique serial selectors. Inaccessible string descriptors are omitted
  without hiding the device.
- Verbose mode renders configurations, alternate interface settings, endpoint
  transfer direction/type, and isochronous maximum packet sizes. Pure fixtures
  cover classification, rendering, argument errors, and duplicate selector
  disambiguation; real local enumeration also exits cleanly when no USB devices
  are visible.

## Slice 113 — what's here

LE scanning now runs against any external HCI transport:

- `bumble-scan` resets and configures the controller, installs Classic and LE
  event masks plus the configured random address, tries multi-PHY extended
  scanning, and falls back to legacy 1M scanning when extended commands are not
  supported. A coded-only request fails explicitly rather than silently changing
  PHY.
- The upstream CLI surface is present: RSSI threshold, active/passive mode,
  interval/window validation, PHY selection, controller duplicate filtering,
  raw-event output, repeatable IRKs, JSON key-store loading, device-config
  address selection, and transport dispatch.
- Legacy and extended advertising reports are decoded through the typed HCI
  events and rendered with address type/qualifiers, connectability, PHYs, RSSI
  bars, and typed advertising-data values. Deterministically generated RPAs are
  resolved to identity addresses in tests; a scripted transport verifies the
  complete command sequence and streaming event path through clean EOF.

## Slice 114 — what's here

Processed scan reporting now matches upstream behavior:

- An address-keyed advertisement accumulator preserves the last advertising
  payload, defers scannable reports during active scans, and combines them with
  the next scan response while carrying the original connectable/scannable
  properties. Passive scans and repeated advertisements emit immediately under
  the same conditions as Bumble's `AdvertisementDataAccumulator`.
- Raw mode remains a direct event view, while processed mode applies RSSI and
  identity-resolution filtering after accumulation so the displayed record is
  the complete advertisement seen by applications.
- Every typed advertising-data variant has a readable label, including company
  names for known manufacturer identifiers and hexadecimal fallbacks for opaque
  values. Focused tests cover active merging, passive immediate delivery, and
  the end-to-end scripted controller scan/response path.

## Slice 115 — what's here

Controller inspection now has full symbolic report parity:

- `bumble-hci::metadata` exposes upstream specification-version, LE-feature,
  standard-codec, codec-transport, Supported Commands, and typed Voice Setting
  decoders. The 338 command-bit labels and assigned-number tables are generated
  directly from `bumble/hci.py`; regeneration needs no Bumble Python imports.
- `bumble-controller-info` renders named HCI/LMP versions, each LE feature and
  supported command on its own line, standard codec and transport names,
  vendor codec company/code pairs, and all five decoded voice fields. Unknown
  open values retain hexadecimal fallbacks.
- Metadata fixtures pin sparse bitmap ordering, newest command-table entries,
  combined codec transports, and voice-setting round trips. The scripted
  controller test exercises the complete report instead of only numeric query
  payloads.

## Slice 116 — what's here

Local controller loopback testing is now runnable over external transports:

- `bumble-controller-loopback` implements the upstream packet-size/count,
  ACL/SCO, throughput/RTT, interval, and transport surface. It rejects invalid
  ranges and SCO payloads above 255 bytes before opening a controller.
- Startup resets the controller, enables only the required event classes,
  checks the Supported Commands bitmap, derives a bounded send window and
  maximum payload from Classic or LE buffer queries, writes local loopback mode,
  and verifies the typed Read Loopback Mode response.
- ACL payloads use CID 0 L2CAP framing and reassembly; SCO payloads use typed
  synchronous packets. Both paths validate the connection handle, packet size,
  and monotonically increasing 16-bit counter before producing receive,
  throughput, and RTT statistics. Scripted controllers exercise asynchronous
  connection events interleaved with command responses and both data paths.

## Slice 117 — what's here

Controller-backed pairing-key removal now works without a crate dependency
cycle:

- `bumble-unbond` now lives beside the external transports it consumes while
  preserving the direct `--keystore-file` list/delete mode and optional
  namespace extension.
- Controller mode loads the upstream device-config `address` and `keystore`
  fields, resets HCI, reads the controller public BD_ADDR when supported, and
  applies Bumble's public-address, configured-address, then default-namespace
  precedence.
- `JsonKeyStore`, `JsonKeyStore:<filename>`, absent, and unknown keystore
  settings reproduce upstream persistent/default-path and in-memory behavior.
  Scripted-controller tests cover public-address selection, configured-address
  fallback, deletion, memory fallback, and invalid configuration.

## Slice 118 — what's here

The HCI bridge is now runnable over independently owned system-transport
halves:

- `open_split_transport` duplicates file descriptors, sockets, serial ports,
  libusb handles, and equivalent handles for file, raw HCI socket, serial, TCP,
  UDP, USB, VHCI, PTY, and Unix endpoints. A blocked reader therefore never
  holds the writer behind a shared mutex.
- `bumble-hci-bridge` opens host and controller transports, forwards both
  directions on dedicated workers, flushes packet sinks, and returns on EOF or
  transport failure.
- The optional short-circuit list accepts direct hexadecimal opcodes and the
  upstream `OGF:OCF` form. Selected commands receive a typed successful Command
  Complete event locally while all other HCI packets reach the controller.
  Parser, forwarding, short-circuit, and live split-TCP tests cover the path.
- WebSocket and Android emulator/netsim gRPC transports still need safe split
  endpoint ownership before this application reaches full transport parity.

## Slice 119 — what's here

The HCI bridge now has split-endpoint parity across the remaining transport
families:

- Android emulator and both Android netsim modes split their inbound standard
  channels from cloneable gRPC send handles. A shared lifetime guard shuts down
  and joins the runtime worker only after both halves are gone; netsim's INI
  registration follows the same lifetime.
- WebSocket splits use a bounded-read shared connection so blocking receives
  regularly release ownership to the writer while preserving one coordinated
  WebSocket protocol state for TLS, control frames, and outgoing data.
- Live echo-server, netsim host/controller, and bidirectional WebSocket tests
  send packets through the independent halves. `open_split_transport` now
  supports every scheme accepted by `open_transport`, completing
  `bumble-hci-bridge` transport parity.

## Slice 120 — what's here

Two software controllers can now serve real external hosts on one in-process
radio link:

- `bumble-controllers` matches the current upstream two-transport CLI, opens
  both as independent HCI halves, and gives each host reader its own blocking
  worker while one serialized runtime owns the shared `LocalLink`.
- Command, ACL, synchronous, and ISO packets are dispatched to the existing
  controller/link APIs. Each input drives advertising, pending connections, LE
  control, periodic sync transfer, and Classic LMP to quiescence before all
  resulting host packets are flushed to their owning transport.
- A scripted pair configures random addresses, advertises, establishes an LE
  connection, and sends ACL data through the exact external-host dispatch path;
  it verifies peer-handle translation and Number Of Completed Packets flow
  control alongside strict CLI arity.

## Slice 121 — what's here

The host stack is no longer coupled to the in-process controller simulation:

- `bumble-host::HostTransport` captures the small HCI/link operation surface used by
  `Device`; `LocalLink` implements it without changing the existing deterministic
  test and simulation behavior.
- `bumble-transport::ExternalHost` owns an arbitrary split HCI transport, keeps
  its blocking reader on a worker, serializes all outbound command/ACL/SCO/ISO
  packets through the caller-owned sink, and exposes bounded activity waits,
  clean EOF, and durable read/write failure state.
- Adapter tests exercise typed outbound traffic, asynchronous inbound events,
  controller-id and SCO-size rejection, and an actual `Device::connect_le`
  transition driven entirely through external HCI packets. This is the shared
  runtime foundation for the remaining device-facing command-line apps.

## Slice 122 — what's here

External hosts can now power on a controller with bounded, lossless command
orchestration:

- `ExternalHost::send_command` waits for the matching Command Complete or
  Command Status under one deadline while retaining every interleaved event and
  data packet for `Device`; EOF, read failure, write failure, non-success status,
  malformed response type, and timeout remain distinct errors.
- `ExternalHost::initialize_device` follows the upstream reset sequence for the
  shared runtime: it reads the Supported Commands bitmap, installs required
  Classic/LE event masks, selects V2 or V1 LE buffer discovery, falls back to
  the shared Classic ACL pool when needed, and applies the resulting packet size
  and in-flight window to `Device`.
- `Device` now recognizes legacy, enhanced, and enhanced-V2 LE connection
  completions from real controllers. Tests cover interleaved command traffic,
  complete initialization and flow-control configuration, and an enhanced-event
  external connection.

## Slice 123 — what's here

ATT/GATT now runs synchronously over an external controller, and the first app
using that path is runnable:

- `AttTransport::try_request` and `GattError::Transport` preserve bearer I/O,
  EOF, disconnect, and timeout failures instead of disguising them as peer ATT
  errors. All GATT client procedures use the fallible path, while existing
  in-process servers keep the original zero-overhead default.
- `ExternalAttTransport` sends ATT through `Device`, waits under one deadline,
  matches Error/normal responses by request opcode, retains interleaved
  notifications, and continuously processes ACL completions and connection
  state. `GattClient::discover_attributes` adds the upstream all-handle Find
  Information walk.
- `bumble-gatt-dump` supports configured random/public local addresses, direct
  address or active local-name resolution, initiator and advertising/listener
  modes, complete hierarchy rendering, and readable per-attribute values over
  every split HCI transport. Its `--encrypt` path remains explicitly gated on
  the external SMP runtime rather than silently running unencrypted.

## Slice 124 — what's here

LE SMP pairing and encryption now run over the external-controller host path:

- `Device` preserves controller Long Term Key Request events and exposes
  positive/negative LTK replies, alongside handle-scoped request collection and
  disconnect cleanup.
- `LePairingSession` drives the existing `PairingManager` over the SMP fixed
  channel without duplicating protocol or cryptographic state. It supports both
  central-initiated Pairing Requests and peripheral Security Requests, handles
  Legacy or Secure Connections encryption transitions, completes key
  distribution, and can persist the resulting bond in any `KeyStore`.
- `bumble-gatt-dump --encrypt` now performs real Just Works Secure Connections
  pairing, waits for controller-confirmed link encryption, and only then starts
  GATT discovery. Deterministic two-controller tests cover both roles, matching
  LTKs, encryption state, and bond persistence; external HCI tests cover LTK
  request/reply routing.

## Slice 125 — what's here

The LE half of upstream `apps/pair.py` is now runnable as `bumble-pair`:

- The complete upstream CLI shape is parsed, including Legacy/SC, MITM,
  bonding/CT2, I/O capability, OOB, identity/address selection, pairing-request,
  linger, key-store, service UUID, and appearance controls. Classic/dual requests
  fail explicitly until the controller SSP event path lands.
- Direct address or active-name connections and advertising/listener mode share
  the external host and `LePairingSession` runtime. Listener sessions may remain
  passive for peer-initiated pairing; active peripherals send an SMP Security
  Request.
- Interactive accept, numeric comparison, passkey input/display, OOB share/TK,
  controller-confirmed encryption, JSON bond persistence, pre-pair key listing,
  and deterministic advertisement construction are covered. External host
  initialization now enables the full upstream Classic/LE host event mask in
  preparation for SSP.

## Slice 126 — what's here

Classic and dual-mode operation in `bumble-pair` now use the external-controller
host path rather than stopping at the CLI boundary:

- `Device` retains Classic inquiry/name/connection-request and authentication
  events per handle and peer. `ClassicPairingSession` answers legacy PIN and
  Secure Simple Pairing IO-capability, confirmation, passkey, OOB, and stored
  link-key requests, reports controller failures, and persists the resulting
  link key.
- Classic direct connections resolve either a public address or inquiry name;
  listener mode publishes the local name and accepts page requests. Dual mode
  advertises over LE while remaining Classic-discoverable/connectable and pairs
  whichever transport connects first.
- `--request` sends an SMP Security Request on the transport's fixed channel
  and then accepts peer-initiated pairing. Logical SMP role is kept separate
  from controller central/peripheral role so the physical central still starts
  LE encryption after a peer-started exchange.
- When CTKD is enabled with a P-256 Secure Connections link key, the application
  encrypts the authenticated Classic ACL and attempts SMP over fixed CID
  `0x0007`. Successful LTK, identity material, and link-key derivation replaces
  the Classic-only bond record; unsupported peers retain that Classic bond. A
  two-controller test drives both wrapper sessions through the live L2CAP/ACL
  path and verifies matching keys and persistence on both peers.

## Slice 127 — what's here

The typed device-information application now runs over a real external
controller:

- `bumble-device-info` preserves the upstream device-config, optional
  encryption, transport, and address-or-name CLI. It supports both active-name
  resolution plus outgoing LE connections and advertising/listener mode.
- The application discovers and renders the complete remote service and
  characteristic hierarchy, then uses the existing typed proxies to read GAP,
  Device Information, Battery, TMAP, PACS, and VCS values.
- Protocol errors remain scoped to their profile section so one inaccessible or
  malformed service does not hide later information. An encrypted in-memory
  server test exercises all six profile sections, including dynamic Battery and
  VCS values, while an unencrypted test pins the error-continuation behavior.

## Slice 128 — what's here

The LE credit-based L2CAP/TCP bridge now runs over external controllers:

- `bumble-l2cap-bridge` implements the upstream group CLI and both modes. The
  server advertises, accepts the configured PSM, and opens one remote TCP stream
  per incoming CoC channel; the client connects to the configured peer and
  opens one CoC channel per accepted local TCP stream.
- Nonblocking TCP pipes preserve channel boundaries while supporting partial
  writes, repeated client connections, simultaneous channels, clean EOF and
  disconnect propagation, bounded HCI waits, and surfaced signaling errors.
- LE CoC channels can now pause and resume receive-credit grants. TCP-bound
  buffering therefore remains bounded by the negotiated credit window, while
  TCP-to-CoC reads wait for both channel framing and controller ACL queues to
  drain. A live two-controller plus loopback-TCP test verifies both directions;
  the lower-level test pins credit withholding and restoration.

## Slice 129 — what's here

The Classic RFCOMM/TCP bridge now runs over external controllers:

- `bumble-rfcomm-bridge` preserves the upstream device-config, transport,
  tracing, channel, UUID, TCP, authentication, and encryption CLI. Server mode
  accepts Classic ACLs and RFCOMM DLCs before opening outbound TCP streams;
  client mode lazily establishes and reuses one ACL/RFCOMM session across
  repeated local TCP connections.
- Channel zero publishes the configured service UUID and allocated RFCOMM
  channel through a live SDP endpoint on the server, and resolves the same UUID
  through an SDP client on the initiator. Explicit channels bypass discovery.
- `Device` now owns Classic dynamic L2CAP managers for every ACL handle, so SDP
  and RFCOMM run over arbitrary external host transports. Individual RFCOMM
  DLCs can close without tearing down the multiplexer, and paused sinks withhold
  receive-credit replenishment to bound TCP backpressure.
- A real loopback TCP test drives the production pipe in both directions across
  two in-memory controllers, Classic L2CAP, and RFCOMM. Focused tests also pin
  Classic channel lifecycle, DLC reuse, and receive-credit pause/resume.

## Slice 130 — what's here

The Golden Gate Gattlink bridge now runs over external controllers:

- `bumble-gg-bridge` preserves the upstream HCI transport, local address,
  `node`-or-peer role, and short/long UDP endpoint options. Both UDP sockets are
  nonblocking, the receive queue is bounded, and invalid zero-length or
  over-256-byte Gattlink datagrams are rejected explicitly.
- Node mode exposes the upstream 128-bit service plus writable RX, notifying
  TX, and readable/notifying CoC-PSM characteristics, advertises `Bumble GG`,
  accepts LE credit channels on PSM `0x00FB`, and falls back to GATT when no CoC
  is active. Hub mode negotiates MTU 256, discovers the service, subscribes,
  reads the PSM, and prefers CoC while retaining RX/TX GATT compatibility.
- CoC data uses the upstream one-octet `packet_length - 1` framing and handles
  packets split across or coalesced within SDUs. Both CoC and GATT writes wait
  for controller ACL output to drain, and dynamic RX queues reject overflow.
- A live integration test crosses real UDP sockets, the production endpoint,
  Gattlink framing, LE credit flow control, controller ACL queues, and two
  software controllers in both directions. Separate tests cover the complete
  GATT database/discovery/write/read/subscription contract and CLI parity.

## Slice 131 — what's here

The upstream Classic A2DP player now runs over external controllers:

- `bumble-player` preserves the upstream device-config, HCI transport,
  authentication/encryption, `discover`, `inquire`, `pair`, and `play` CLI.
  Discovery reports Class of Device, service labels, RSSI, and Extended Inquiry
  Response structures; pairing and outgoing playback reuse namespaced JSON
  link keys.
- Playback publishes the A2DP source SDP record, finds the remote sink through
  SDP, discovers every AVDTP endpoint and capability, selects a compatible
  audio sink, and completes SetConfiguration/Open/Start/Close. SBC, ADTS AAC,
  and Ogg Opus inputs derive their configuration from the first media frame,
  packetize to RTP, pace by RTP timestamps, and bound controller output to one
  drained packet at a time.
- `DeviceSession`, `DeviceMediaTransport`, and `DeviceProtocol` bind AVDTP,
  A2DP media, and AVCTP respectively to `Device`-owned Classic channels. The
  player accepts AVRCP connections and dispatches them through the typed AVRCP
  target runtime while streaming or negotiating.
- Live two-controller tests cover fragmented AVDTP discovery/configuration/
  open/start, RTP media transfer, AVCTP fragmentation, accepted PID delivery,
  and automatic IPID rejection. CLI and codec-derivation tests pin the complete
  player surface and SBC sink bitpool negotiation.

## Slice 132 — what's here

The upstream Classic A2DP speaker now runs over external controllers:

- `bumble-speaker` preserves the upstream SBC/AAC/Opus codec selection,
  repeatable sampling frequencies and outputs, AAC bitrate/VBR controls,
  endpoint discovery, UI port, optional address-or-name connection,
  device-config, and HCI transport CLI. It listens as `Bumble Speaker` by
  default or authenticates and encrypts an outgoing Classic ACL, with
  namespaced JSON link-key persistence in either direction.
- The speaker publishes an A2DP sink SDP record and exposes a live AVDTP sink
  with codec-accurate capabilities. It handles SetConfiguration,
  Reconfigure, Open, Start, Suspend, Close, Abort, and delay reports, attaches
  the negotiated media channel, decodes RTP framing, and writes SBC/Opus
  payloads or ADTS-wrapped AAC to files and optional `ffplay` processes.
- An embedded threaded HTTP/WebSocket service ships the speaker UI,
  reports connection and stream state, and broadcasts extracted audio through
  bounded per-client queues. Endpoint discovery queries all capabilities with
  an AVDTP 1.2-compatible GetCapabilities fallback.
- A live two-controller test drives the production sink through discovery,
  configuration, open, media-channel attachment, start, and exact RTP packet
  receipt. Focused tests also pin the complete CLI, default codec capability
  masks, payload extraction, and embedded UI delivery.

## Slice 133 — what's here

The upstream interactive LE console now runs over external controllers:

- `bumble-console` preserves the upstream `--device-config` plus transport CLI
  and all interactive commands: scan/filter/RSSI, advertising, connect,
  disconnect, connection-parameter updates, pairing/encryption, PHY reads and
  writes, MTU exchange, GATT service/attribute discovery, remote reads/writes,
  subscription management, local writes, status views, and exit aliases.
- A dedicated input worker keeps the console scriptable through standard input
  while the main loop continuously pumps HCI, advertising reports, incoming LE
  connections, passive SMP pairing, notifications, indications, and
  disconnections. The prompt-toolkit fullscreen tabs are represented as
  explicit terminal views without changing the command grammar.
- Remote GATT state retains service, characteristic, descriptor, cached-value,
  and CCCD metadata across commands. Selectors accept upstream-style
  `service.characteristic`, wildcard-service, and hexadecimal-handle forms;
  writes accept hex, integer, or text values.
- The local GAP database exposes dynamic Device Name and Appearance values
  shared with the live ATT server, so `local-write` updates subsequent remote
  reads. A two-controller integration test proves discovery and dynamic reads
  across real HCI, ACL, L2CAP, and ATT; focused tests cover the CLI, complete
  command parser, PHY/value parsing, scan rendering, and selector behavior.

## Slice 134 — what's here

The upstream LE Audio unicast server now runs over external controllers:

- `bumble-lea-unicast` preserves the upstream UI-port, device-config, HCI
  transport, and WAVE-input CLI. Its default identity, random address, PACS
  sink/source records, audio contexts and locations, ASE IDs, advertising
  interval, and unicast-server announcement match the Python app.
- The production loop binds the existing PACS and ASCS services to the live
  ATT server, forwards control-point and ASE notifications, accepts only CIS
  requests matching enabled ASE QoS, sets up both ISO directions exactly once,
  and follows sink/source streaming state across disconnect and re-advertise.
- `bumble-codecs` now provides safe owned LC3 encoder/decoder workers over the
  pure-Rust codec. They preserve per-channel state, support mono/stereo and
  multiple codec-frame blocks per SDU, and expose interleaved signed 16-bit PCM
  without self-referential storage or leaked buffers.
- Received sink SDUs are decoded to PCM and broadcast by the embedded
  HTTP/WebSocket UI. Source media loops a 16-bit WAVE file, continuously
  resamples and maps its channels to the negotiated format, encodes LC3, and
  paces ISO SDUs from the negotiated QoS interval.
- A live two-controller test drives the production GAP/PACS/ASCS database,
  configures both ASEs over real ATT/L2CAP/ACL, establishes a CIS, routes an LC3
  ISO SDU, and decodes it. Focused tests cover the CLI, exact PAC and
  advertising contract, resampling/channel mapping, browser delivery, and LC3
  worker buffer validation. The integration also closed a host gap so every
  typed ATT request—not only the original discovery subset—is answered over a
  live connection.

## Slice 135 — what's here

The broadcast-ISO runtime required by the upstream Auracast app is now live:

- The software controller implements LE Create/Terminate BIG and BIG
  Create/Terminate Sync with real state, BIS handle allocation, BIGInfo reports
  attached to matching periodic trains, encrypted Broadcast Code validation,
  and source-termination propagation to synchronized receivers.
- `bumble-host::Device` exposes typed BIG and BIG-sync parameters, source and
  receiver BIS handle ownership, BIGInfo/error/termination queues, directional
  ISO data-path setup, and the existing 960-byte SDU fragment/reassembly path
  for both CIS and BIS handles.
- The local radio routes each broadcaster BIS fragment to every matching
  synchronized receiver while preserving HCI ISO metadata and completed-packet
  flow control. Broadcaster paths are host-to-controller only and synchronized
  receiver paths are controller-to-host only.
- A live three-controller test creates an encrypted two-BIS BIG, rejects a bad
  Broadcast Code, synchronizes two receivers, fans out a 2,500-byte SDU to both,
  terminates one receiver independently, and propagates source termination to
  the remaining receiver.

## Slice 136 — what's here

The upstream Auracast application is now runnable over external controllers:

- `bumble-auracast` preserves the upstream `scan`, `assist`, `pair`, `receive`,
  and `transmit` commands, including duplicate filtering, sync timeouts,
  broadcast/subgroup selection, Broadcast Codes, BASS source operations, and
  single-source or TOML multi-broadcast transmitter configuration.
- The shared scanner correlates extended Broadcast Audio Announcements with
  periodic Basic Audio Announcements and BIGInfo, serializes controller sync
  procedures, and removes lost sync state. `assist` discovers and subscribes to
  BASS, requests MTU 256, adds/modifies/removes sources, and performs PAST for
  add-source.
- Transmit opens one- or two-channel 16/24/48-kHz portable PCM sources, creates
  LC3 subgroups and sequential BIS assignments, publishes BAP/PBP and optional
  manufacturer data, creates encrypted or clear BIGs, and paces each encoded
  channel into its source BIS. Receive selects a subgroup, synchronizes its BIS
  set, reassembles aligned LC3 channel SDUs, and writes float32 PCM.
- Six focused tests cover every command shape, TOML and Broadcast Code
  compatibility, malformed inputs, service-data/config decoding, and a live
  two-controller extended-advertising to periodic-BAA/BIGInfo scan. The
  external-host initialization test now also pins the BIG/BIS LE event-mask
  bits required on real controllers.

## Slice 137 — what's here

The upstream multi-transport Bluetooth benchmark is now runnable over external
controllers:

- `bumble-bench` implements both roles and all four scenarios over GATT client
  or server, LE credit-based L2CAP client or server, RFCOMM client or server,
  and CIS client or server modes. RESET, SEQUENCE, and ACK packets retain the
  upstream little-endian wire format; stream modes retain their two-byte
  big-endian packet framing across fragmentation and coalescing.
- The full tuning surface is live: ATT MTU, LE scan/advertising and connection
  intervals, data length, PHY, Classic scans and role switching, authentication
  and encryption, RFCOMM L2CAP/frame/credit limits, LE CoC PSM/MTU/MPS/credits,
  and every directional CIG SDU interval, max SDU, transport latency, and RTN.
  RFCOMM channel zero performs real SDP discovery and the server advertises the
  matching service record.
- Send and ping preserve startup/repeat/pacing controls and report throughput or
  RTT sample statistics. Receive and pong validate ordering, report instant,
  64-sample windowed, and average rates, compute signed and absolute jitter, and
  acknowledge the correct terminal packet. GATT server-originated runs wait for
  a real CCCD subscription before sending.
- Nine focused application tests pin the packet codec, stream reassembly,
  GATT notification retention, statistics, role defaults, all eight modes, the
  complete option surface, and invalid combinations. An RFCOMM session
  regression additionally proves that runtime receive-credit limits replenish
  at the configured threshold. Production-binary two-controller runs cover
  GATT, LE CoC, RFCOMM with SDP discovery, and CIS end to end. They also closed
  the shared external-host status fallback for byte-preserved HCI Command
  Complete return parameters, give software-radio controllers distinct F0/F1
  public addresses, and pin generated Classic HCI BD_ADDR decoders to public
  address semantics so LMP traffic routes to the intended peer.

## Slice 138 — what's here

The Pandora conformance server now has its canonical protocol and complete Host
service foundation:

- `bumble-pandora` vendors the exact `bt-test-interfaces==0.0.6` Host,
  Security, and L2CAP protobuf contracts and generates both tonic clients and
  servers with a vendored `protoc`. `bumble-pandora-server` preserves the
  upstream gRPC/rootcanal ports, transport override, and JSON configuration
  command line while accepting graceful Ctrl-C shutdown.
- The shared runtime initializes a real external HCI controller, restores the
  configured random address after reset, reads the public address, owns stable
  big-endian connection cookies, and serializes controller access safely across
  tonic workers. Every upstream Host RPC is present: Classic and LE connect and
  wait paths, disconnect waits, legacy/extended advertising and scanning,
  inquiry, discoverability, connectability, reset, and factory reset. The two
  methods absent from the upstream Python implementation retain canonical
  `UNIMPLEMENTED` responses.
- Pandora `DataTypes` now converts the complete advertising-data catalog,
  including all UUID widths, names, service and solicitation UUIDs, target
  addresses, service/manufacturer data, appearance, URI, LE features, flags,
  and interval fields. Invalid field sizes and values return gRPC argument
  errors instead of malformed HCI commands.
- Nine focused tests cover configuration and CLI compatibility, address and
  connection-cookie contracts, advertising-data round trips, report mapping,
  and a live canonical gRPC client talking through the Host service to a real
  `bumble-controller` over TCP. The live test exercises address reads,
  discoverability/connectability, reset, factory reset, and intentional
  unimplemented parity at the wire boundary.

## Slice 139 — what's here

Pandora's complete L2CAP gRPC service now runs on the shared external-controller
runtime:

- All six canonical RPCs are live for Classic connection-oriented and LE
  credit-based channels: `Connect`, `WaitConnection`, `Send`, `Receive`,
  `Disconnect`, and `WaitDisconnection`. Single-channel enhanced-credit
  requests use the enhanced signaling path, and fixed-channel send/receive is
  supported in addition to the upstream service's dynamic-channel path.
- Dynamic channel tokens preserve upstream's opaque JSON cookie shape exactly.
  Runtime-owned channel/server registries survive tonic service clones and are
  cleared on Host reset. Incoming channels are retained by connection, PSM,
  transport, and source CID so unrelated listeners cannot consume one
  another's accepted channel.
- Classic configuration and LE CoC credit/MTU/MPS values are range checked
  before reaching the controller. L2CAP refusal and stale-channel paths return
  the canonical command-reject results, while malformed RPC structure returns
  precise gRPC argument errors.
- Three new focused tests cover exact cookie and request contracts plus a live
  two-controller proof. Separate Pandora gRPC servers establish an LE ACL,
  accept a real LE CoC, transfer an SDU across the generated clients, disconnect
  the channel, and complete the peer's disconnection wait over the TCP HCI
  transports.

## Slice 140 — what's here

Pandora now exposes the complete canonical Security and SecurityStorage gRPC
surface on the same controller-backed runtime:

- `Secure` and `WaitSecurity` validate transport-specific connection cookies,
  evaluate every Classic Level 0–4 and LE Level 1–4 predicate, drive LE or
  Classic pairing, and report the protocol's exact success/failure variants.
- `OnPairing` enforces its pre-connection/single-stream contract and bridges
  Just Works, Numeric Comparison, passkey entry/display, and legacy PIN input
  between tonic streams and the synchronous pairing delegates.
- Memory and namespaced JSON key stores back `IsBonded` and `DeleteBond`;
  successful pairing persists bonds, Host `FactoryReset` deletes every bond,
  and ordinary reset preserves them.
- The runnable server registers Host, Security, SecurityStorage, and L2CAP
  together. A live two-controller gRPC test pairs and encrypts both sides over
  LE, exercises pairing-event answers, queries/deletes bonds, factory-resets
  storage, then transfers an LE credit-channel SDU. A reactive scripted-HCI
  test proves the Classic Secure RPC through SSP authentication, link
  encryption, and link-key persistence. The scripted boundary is deliberate:
  upstream Bumble's software controller also has no Classic-authentication
  command handler.

## Slice 141 — what's here

The completion audit closes two real controller/device behaviors exercised by
upstream `device_test.py` that were previously only present in the HCI codec:

- `Sniff_Mode` and `Exit_Sniff_Mode` now return Command Status and emit the
  upstream deterministic Mode Change events for Sniff and Active state.
- `LE_Set_Default_Subrate` and `LE_Subrate_Request` implement upstream's range,
  latency-product, and continuation-number validation. Successful requests emit
  the exact fixed-factor LE Subrate Change event used by the software
  controller; invalid requests stop at the appropriate error response.
- `Device` retains the initial LE connection parameters, exposes typed subrate
  requests, updates subrate/latency/continuation/timeout state from controller
  events, and tracks Mode Change state for both LE and Classic connection
  handles. Direct event tests and a live two-device host test mirror upstream's
  enter/exit-sniff and subrate cases without raw-HCI state assertions.

## Slice 142 — what's here

The completion audit ports upstream's experimental Channel Sounding `Device`
orchestration over the already-complete typed HCI catalog:

- Per-LE-connection state retains remote CS capabilities, the four upstream
  configuration slots, and procedure-enable results. Failed capability,
  security, configuration, and procedure completions are routed through a
  typed operation/error queue, and disconnection clears handle-owned state.
- Typed methods read remote capabilities, set the upstream default settings,
  allocate/create/remove configurations, enable CS security, set procedure
  parameters, and enable or disable procedures. Defaults match Python Bumble,
  including its channel map that excludes channels 0, 1, 23-25, and 76-79.
- Scripted external-HCI tests pin every emitted command, successful state
  transition, four-ID exhaustion, removal, and failure path. This deliberately
  stops at the transport boundary because upstream's software controller does
  not implement Channel Sounding command handlers either.

## Slice 143 — what's here

The Device completion audit now includes the upstream-tested LE and Classic
remote-feature discovery flows:

- `Device` exposes handle-scoped LE and Classic feature requests, retains LE
  feature bytes and every Classic LMP feature page on the matching connection,
  and reports typed transport/page/status failures without corrupting state.
- The software controller now exchanges `LmpFeaturesReqExt` and
  `LmpFeaturesResExt`, emits `Read Remote Extended Features Complete`, and uses
  upstream's four-page default LMP feature model. `Device` automatically walks
  pages 0 through the peer-advertised maximum when the base bitmap advertises
  extended features.
- Controller and live dual-mode host tests prove exact base/extended feature
  bytes, automatic four-page collection, LE LL feature discovery, wrong-handle
  rejection, and scripted LE/base/extended failure routing.

## Slice 144 — what's here

External-controller reset initialization now preserves the upstream host's
local capability discovery instead of stopping at command and buffer sizes:

- `Read Local Extended Features` has a typed, byte-round-trippable return model.
  The software controller serves its four LMP pages with exact page/max-page
  metadata and rejects out-of-range requests with Invalid Parameters.
- `ExternalHost::initialize_device` conditionally reads the local controller
  version, LE feature bitmap, and either every advertised extended LMP page or
  the legacy base page. The resulting `ExternalControllerInfo` retains all of
  that state alongside ACL/ISO flow-control limits.
- Initialization now installs Event Mask Page 2 for Encryption Change V2 when
  supported, in addition to the existing Classic and LE masks. Codec,
  controller, and scripted external-host tests pin the full command sequence,
  masks, feature pages, typed fields, error fallback, and device queue sizing.

## Slice 145 — what's here

Current upstream added `LE Read All Local Supported Features` after the prior
196-command HCI snapshot. This slice closes that drift through every layer:

- `hcigen` now extracts and regenerates the command instead of deliberately
  excluding it. Generator templates also retain the existing public coding-
  format helpers and address-type reconstruction across clean regeneration.
- The HCI codec exposes the `0x2087` command and its exact status, max-page, and
  248-byte feature return. The software controller advertises the matching
  Supported Commands bit and returns its LE feature bitmap zero-padded to the
  upstream width.
- External host reset and `bumble-controller-info` prefer the all-page command
  when advertised, retaining the max page and full bitmap, while preserving the
  legacy eight-byte fallback for older controllers. Oracle, typed-return,
  controller, and scripted transport tests pin both paths.

## Slice 146 — what's here

The controller's generated surface previously knew which commands returned data,
but ten paths still emitted only a generic success and the resolving-list size
used a stale raw default. This slice makes the complete 31-command data surface
concrete:

- Typed return parameters now cover Class of Device, synchronous flow control,
  LE host support, authenticated-payload timeout, advertising and controller TX
  power, filter/resolving-list sizes, LE supported states, PHY, and CIG removal.
- The software controller returns upstream's exact defaults for those queries,
  echoes required handles and identifiers, removes only existing CIGs, and keeps
  local-name, suggested-data-length, synchronous-flow-control, and LE/SSP
  host-feature writes visible to later reads and Classic feature exchange.
- Codec round trips pin every new byte shape, while controller tests cover
  defaults, write/read state transitions, invalid values, and successful plus
  repeated CIG removal. The generated command-surface documentation now records
  that every Data entry has a real payload rather than a documented stub.

## Slice 147 — what's here

With the data surface complete, the remaining controller audit isolated the
configuration commands that upstream retains but Rust had treated as stateless
successes. This slice closes that final software-controller gap:

- Classic, page-2, and LE event masks; Classic scan mode; default PHY; legacy
  advertising parameters and scan-response data; legacy scan parameters; and
  duplicate filtering are retained and exposed through typed controller state.
- Legacy advertising now uses the configured public/random address, advertising
  event type, directed peer, and scan-response payload. Active legacy scans emit
  the matching advertising plus scan-response reports, while parameter changes
  during an enabled scan return Command Disallowed without corrupting state.
- Legacy and extended connection starts now reject a second pending procedure
  instead of silently replacing the first. Focused tests cover the stored
  configuration, emitted PDUs/reports, validation, and pending-operation guard.

The generated 93-handler audit now leaves only ten fallback commands, all of
which are deliberate no-ops or TODO acknowledgements in upstream
`controller.py`; every stateful or data-bearing handler is explicit in Rust.

## Slice 148 — what's here

The link audit found that the README's proposed wire-serialization work was not
an upstream requirement: `ll.py` explicitly says its messages are context-aware
in-process objects because Bumble has no real physical LL transport. The actual
missing behavior was teardown delivery and CIS lifetime:

- `bumble-controller::ll` now exposes the complete 61-opcode upstream catalog
  and the missing `CisTerminateInd` model alongside every concrete LL PDU.
- Raw HCI Disconnect commands and the high-level link helper now queue
  `TerminateInd`, Classic `Detach`, synchronous detach, or `CisTerminateInd`.
  The initiating controller completes immediately; the peer changes state only
  when the matching deterministic LL/LMP pump delivers the teardown.
- CIS disconnection now follows upstream's asymmetric lifetime: the peripheral
  entry is removed, while the central handle stays allocated but unbound until
  Remove CIG. A focused end-to-end test disconnects over `CisTerminateInd` and
  then establishes a replacement peer CIS with the same central handle.

This closes `link.py` and `ll.py` for the port's in-process controller scope;
the adjacent Classic LMP byte-codec audit follows in Slice 149.

## Slice 149 — what's here

The adjacent LMP audit separated codec completeness from controller behavior.
Unlike `ll.py`, upstream `lmp.py` does define a serialized packet layer, so this
slice ports that missing layer rather than inventing an authentication state
machine absent from upstream's own controller:

- `lmp::Opcode` is an open 16-bit value with all 88 upstream names, one-byte
  base encoding, two-byte escape encoding, offset parsing, and unknown-value
  preservation.
- `lmp::Packet` covers all 18 registered upstream classes, including the full
  SCO/eSCO parameter records, 24-bit name length, base and extended feature
  pages, and base/extended response opcodes. Unknown packets retain their raw
  payload, while truncated and invalid fixed-size packets return typed errors.
- Exact vectors cover every packet class in both directions. The payload bytes
  come from upstream's HCI field serializer; extended opcode prefixes use the
  intended `0x7Fxx` values, avoiding the source expression's shift-precedence
  ambiguity while preserving the Bluetooth wire format.

Together with the already-live `LocalLink::pump_classic` flows, this closes the
`lmp.py` row. `AuRand` is codec-complete but intentionally has no controller
state machine because current upstream logs it as unhandled too.

## Slice 150 — what's here

The next host/device audit found that several common upstream connection
conveniences had typed HCI commands but no `Device` API or completion routing.
This slice closes that orchestration gap without turning Python futures into a
blocking Rust API:

- Handle-scoped APIs now cover legacy LE Connection Update, Bluetooth 6.2
  Connection Rate Request, Subrate Request, Set Data Length, Read/Set PHY, and
  Read RSSI. Controller-wide default PHY, rate, and subrate setters are exposed
  alongside them.
- `LeConnectionInfo` retains the latest negotiated parameters, data lengths,
  PHYs, and RSSI. `LeConnectionControlEvent` preserves successful completions
  and correlates asynchronous Command Status/Complete failures with the handle
  that initiated them; disconnect removes stale pending correlations.
- `Read_RSSI` has a typed signed return-parameter model pinned to upstream's
  `003412c9` oracle for handle `0x1234` and RSSI `-55`.
- The software controller's existing Set/Read PHY simulation now retains the
  selected PHYs. Connection Update, Connection Rate Request, and Read RSSI stay
  unsupported there because upstream `controller.py` also leaves those
  commands unhandled; scripted external-controller tests prove their success
  routing while local-link tests prove the honest Unknown Command boundary.

## Slice 151 — what's here

The next host/device audit closed the event-listener gap around the state that
the Rust `Device` already maintained:

- `DeviceEvent` provides typed LE, Classic, and synchronous connection
  establishment/failure, disconnection success/failure, incoming connection
  requests, legacy/extended advertising reports, Classic inquiry and remote
  name results, pairing, encryption, and LE connection-control outcomes.
- `add_event_listener` delivers those events synchronously after the associated
  state mutation; `remove_event_listener` is idempotent and the independent
  `take_device_events` journal retains deterministic emission order.
- Nonzero `Disconnection Complete` status now follows upstream precisely: it
  emits a failure for a known handle and preserves the connection, encryption,
  L2CAP managers, queues, pending controls, and security state. Only a success
  tears the handle down.
- Scripted-controller tests cover ordered listener/journal parity, listener
  removal, connection failures for all three link families, discovery and
  pairing delivery, state visibility, and the failed-then-successful disconnect
  transition.

## Slice 152 — what's here

The GATT completion audit found that Rust subscriptions wrote CCCDs and cached
values but reduced upstream's subscriber sets to a single marker. This slice
ports the missing listener semantics without introducing an async runtime:

- `subscribe_with_listener` returns stable `GattValueListenerId` values and
  supports multiple callbacks per notification or indication handle. Delivery
  is deterministic by listener ID and occurs after the value cache is updated.
- Removing one callback leaves the CCCD enabled while another callback or an
  implicit subscription remains. Removing the last subscriber clears the CCCD;
  `unsubscribe_all(..., force = true)` also performs upstream's zero write when
  no local subscription exists.
- Notification packets report whether a matching subscription existed while
  still caching unsolicited values. Indications always run matching callbacks
  and return the mandatory Handle Value Confirmation.
- `GattClient::default()` now uses the required ATT default MTU of 23, matching
  `GattClient::new()` instead of deriving an invalid zero MTU. End-to-end tests
  drive all listener, removal, implicit, indication, unsolicited-cache, and
  forced-cleanup paths against a real `GattServer`.

## Slice 153 — what's here

The transport audit returned to a real upstream behavior gap: `usb:<selector>`
accepted `+sco=<alternate>`, but the Rust backend rejected it because `rusb`'s
safe synchronous API does not expose isochronous transfers. This slice uses the
version-locked libusb ABI beneath `rusb` while keeping the public transport
synchronous and mockable:

- `select_sco_layout` mirrors upstream endpoint discovery. A nonzero alternate
  selects that complete Bluetooth HCI setting; alternate zero automatically
  selects the setting with the largest isochronous IN/OUT packet-size pair.
- `UsbIo` now has isochronous read/write operations and `UsbScoLayout` carries
  the claimed interface, alternate, endpoint addresses, and maximum packet
  sizes. Existing backends remain source-compatible through explicit default
  unsupported methods.
- `SystemUsbTransport` claims and activates the selected SCO interface. Its
  isolated libusb context and shared event lock serialize callbacks across
  split source/sink halves. Transfers own their callback buffers and retain the
  device handle until completion; timeout and event-loop errors cancel and
  drain before any allocation is reclaimed.
- Incoming USB fragments are reassembled with the HCI SCO one-byte length
  field, including headers and payloads split across transfers. Outgoing HCI
  SCO packets are divided into endpoint-sized isochronous packets while HCI ISO
  data keeps its upstream bulk-endpoint compatibility route.
- Focused tests cover automatic/explicit/forced endpoint selection, fragmented
  SCO input, output routing, disabled-SCO behavior, and disconnect propagation;
  the complete transport crate remains green.

## Slice 154 — what's here

The completion audit found that EATT was still absent even though the lower LE
credit-channel runtime already supported the enhanced connection procedure.
This slice binds GATT to that real transport rather than simulating an extra
in-process request path:

- `Device::register_eatt_server` and `Device::connect_eatt` use the assigned
  SPSM `0x0027` and enhanced one-to-five-channel LE credit connection command.
  ATT SDUs cross normal HCI ACL fragmentation, L2CAP signaling, credit flow,
  and channel teardown before server dispatch or client delivery.
- Fixed ATT and every EATT CID receive stable distinct bearer identities.
  `GattServer` retains negotiated MTU, prepared-write queue, and automatic CCCD
  values per bearer; disconnecting one bearer removes only its state.
- Notification and indication helpers fan out across the legacy bearer and all
  EATT bearers on a connection, honoring each bearer's CCCD bits. Values are
  MTU-truncated per target, outstanding indications are serialized per bearer,
  and confirmations clear only their matching pending state.
- `ExternalEattTransport::connect` performs the credit-channel handshake over
  an external controller and implements `AttTransport`, so the existing
  high-level `GattClient` runs unchanged on Enhanced ATT.
- End-to-end software-controller tests cover EATT read/write, two concurrent
  enhanced bearers plus fixed ATT, subscription isolation, notify/indicate
  fan-out, confirmations, and unregistered-SPSM refusal. Direct GATT tests pin
  per-bearer MTU, queued-write, CCCD, and cleanup behavior; an external-host
  test drives a real `GattClient` request over framed EATT ACL traffic.

## Slice 155 — what's here

The RFCOMM completion audit compared every upstream multiplexer/DLC path and
test against the Rust runtime. Upstream defines only PN and MSC multiplexer
commands and deliberately negotiates `max_retransmissions = 0`, so the old
tracker references to missing FCON/FCOFF aggregate flow control and
retransmission were not upstream gaps. One observable difference was real:

- Upstream retains at most 32 received DLC packets before a sink consumes them;
  appending packet 33 evicts the oldest packet. Rust now uses the same bounded
  FIFO rather than an unbounded `Vec`.
- `RFCOMM_DEFAULT_RX_QUEUE_SIZE` exposes the upstream constant, and draining a
  DLC preserves retained arrival order and resets the queue for reuse.
- A focused overflow test injects 40 packets and proves that exactly packets
  8–39 survive in order. Existing session, credit/backpressure, L2CAP binding,
  refusal, and disconnect tests remain green, closing `rfcomm.py` for the
  synchronous port's scope.

## Slice 156 — what's here

The `audio/io.py` completion audit found one real platform gap rather than an
async-only difference: upstream optionally loads PortAudio through
`sounddevice` for live hardware input/output and device enumeration. The
`bumble-audio` crate now exposes the same surface behind an optional
`sound-device` feature backed by CPAL 0.18.1:

- `list_audio_{input,output}_devices` reports backend IDs, Bumble-compatible
  global indices, names, maximum channel counts, and the directional default;
  `check_audio_{input,output}` validates `device`, `device:INDEX`, and
  `device:?`, including upstream's list-and-return-false behavior.
- `SoundDeviceAudioOutput` configures the requested rate/channels with upstream's
  float32 device format. Calls to `write` only enqueue bytes; the hardware
  callback drains them in order and fills underruns with silence.
- `SoundDeviceAudioInput` captures upstream's int16 device format, blocks until
  the requested frame is available, duplicates mono samples into stereo, and
  reports the same two-channel int16 format from `open`.
- CPAL streams stay on dedicated owner threads, so the public audio traits remain
  `Send` across platform backends. Focused tests cover feature-disabled factory
  errors, feature-enabled factory syntax, callback ordering/underflow, and exact
  mono duplication without requiring CI audio hardware.

## Slice 157 — what's here

The HFP completion audit compared the complete upstream public model, both
protocol roles, all 24 `hfp_test.py` behavior families, SDP helpers, and the
eight SCO/eSCO presets against Rust. The tracker had incorrectly treated media
encoding as deferred: upstream `hfp.py` only negotiates CVSD/mSBC/LC3-SWB codec
IDs and establishes the matching synchronous link; it does not encode or decode
their payloads.

- Rust already covers minimal/full SLC, AG/HF indicator exchange, codec
  negotiation, dialing/answer/reject/hang-up/hold/current-call flows, ring and
  volume events, caller ID, voice recognition, batched AT traffic, HF/AG SDP,
  and live eSCO setup/routing through the host/controller stack.
- `AgIndicatorState` now includes the remaining upstream default factories for
  call-held, roam, and battery-charge state. The roam factory deliberately
  preserves upstream's observable `CALL` selection rather than silently fixing
  it in only one language.
- A focused catalog test pins each factory's indicator, current value, supported
  range, and enabled state. With Python-only await/event wrappers represented by
  explicit Rust command/result/event queues, no upstream HFP behavior remains
  deferred.

## Slice 158 — what's here

The SDP completion audit covered every public data type, the seven PDU classes,
all eleven current `sdp_test.py` families, client/server request paths, and the
Classic L2CAP binding. It found one protocol gap hidden by the old tracker:
size-index 4 denotes a 16-byte integer as well as a 128-bit UUID, but Rust's
integer variants previously stored only 64 bits.

- `DataElement::{UnsignedInteger,SignedInteger}` now use `u128`/`i128` and
  serialize/parse the complete 1/2/4/8/16-byte SDP integer domain. Exact
  positive and negative 16-byte wire vectors pin the header, big-endian value,
  sign extension, and round trip.
- Existing profile record parsers retain their intentional `u64` boundary by
  using checked conversion, so oversized values cannot silently truncate into
  A2DP, AVRCP, or HFP fields.
- `ServiceAttribute::is_uuid_in_value` exposes upstream's recursive sequence
  search (and its deliberate alternative boundary), and focused tests now cover
  that helper plus rejection beyond upstream's 32-level nesting limit and
  successful parsing at a reasonable depth.
- Service matching, attribute/range selection, every error response,
  continuation/watchdog behavior, and multi-round-trip L2CAP queries were
  already complete. Python-only async context management maps to Rust ownership,
  leaving no protocol behavior deferred.

## Slice 159 — what's here

The host/device audit found a behavioral ISO gap behind the old “live CIG/CIS”
tracker entry: Rust configured every stream with hard-coded values and created
only one CIS per HCI command, while upstream exposes the complete group and
per-direction QoS model.

- `CigParameters` and `CisParameters` now carry every upstream field and exact
  defaults (251-byte SDUs, LE 2M PHY, 10 retransmissions, and 100 ms latency).
  A zero-sized direction forcibly clears its PHY and retransmission count at
  command emission, matching upstream and avoiding controller error `0x30`.
- `configure_cig_with_parameters` forwards the complete parallel HCI arrays,
  while `create_cis_pairs` preserves upstream's batched CIS/ACL command shape.
- Incoming CIS requests can be accepted or rejected. The local controller
  transports rejection over its queued LL path, reports the supplied failure to
  the central, and does not create phantom stream state.
- `CisControlEvent` retains CIG command results, create/accept/reject statuses,
  successful and failed establishment results; successful `CisLinkInfo` values
  preserve every timing, PHY, burst, flush-timeout, PDU-size, and ISO-interval
  field from `LE CIS Established`.
- Focused tests pin exact parameter arrays, default and unidirectional behavior,
  invalid 24-bit intervals, two-CIS batching, mixed accept/reject outcomes,
  result ordering, and link-state retention.

## Slice 160 — what's here

The next host/device audit compared upstream `_IsoLink` data-path and TX-sync
methods against Rust. The wire command variants already existed, but the public
API hard-coded the HCI path, transparent codec, zero delay, and empty codec
configuration; Read ISO TX Sync had no typed completion or controller/host
runtime.

- `IsoDataPathParameters` exposes direction, data-path ID, coding format,
  24-bit controller delay, and the length-prefixed codec configuration. The
  original convenience method remains as the exact transparent HCI default.
- `Device` correlates Setup/Remove completions with pending handles, retains
  installed paths, makes repeated setup/removal idempotent, journals both typed
  and status-only failures, and clears path/sync state on CIS/BIS teardown.
- `LE Read ISO TX Sync` now has a typed return model. The software controller
  records the latest successfully routed first-fragment sequence and timestamp,
  and the host retains both the latest successful result and ordered outcomes.
- Setup/Remove ISO data-path completions use their shared status-and-handle
  return shape instead of an opaque payload. Exact wire vectors cover all three
  typed returns, while scripted and live two-controller tests cover custom
  command fields, validation, failures, TX-sync state, and removal.

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
│   ├── src/vendor/{android,zephyr}.rs # slice-99 vendor HCI codecs
│   ├── tests/acceptance.rs    # ported hci_test.py cases (oracle-pinned)
│   └── tests/vendor.rs        # Android/Zephyr exact envelopes and events
├── bumble-controller/         # slice-3 controller + virtual link crate
│   ├── src/lib.rs
│   ├── tests/scenario.rs      # end-to-end advertising→scan→report scenario
│   ├── tests/power_modes.rs   # slice-141 Sniff/Active + LE subrate behavior
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
│   ├── tests/high_level_device.rs # legacy/extended advertising and connection
│   ├── tests/periodic_advertising.rs # 600-byte sync/report/receive/cancel flow
│   ├── tests/broadcast_iso.rs # encrypted BIG/BIS fan-out and teardown
│   ├── tests/cig_parameters.rs # slice-159 complete group/directional QoS surface
│   ├── tests/iso_controls.rs   # slice-160 custom paths + TX-sync result journal
│   ├── tests/iso_data.rs       # CIS accept/reject + connected ISO SDUs
│   ├── tests/smp_pairing.rs    # two-party LE Legacy JustWorks handshake
│   ├── tests/smp_sc_pairing.rs # two-party LE Secure Connections handshake (slice 19)
│   └── tests/synchronous_audio.rs # HFP mSBC over host/controller (slice 27)
├── bumble-smp/                # slice-14 SMP codec + legacy pairing + slice-19 SC
│   └── src/lib.rs             # wires bumble-crypto; sc:: JustWorks derivation
├── bumble-sdp/                # slices 16/20/22/158 complete SDP surface
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
├── bumble-hfp/                # slices 24-28/157 complete HFP protocol surface
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
│   ├── src/{l2cap,host}.rs    # transaction runtimes over manager/Device Classic L2CAP
│   ├── tests/acceptance.rs    # 38 exact payloads + malformed PDU coverage
│   ├── tests/session.rs       # lifecycle, errors, atomic multi-SEP commands
│   ├── tests/l2cap_binding.rs # fragmented signaling over live channels
│   └── tests/host_binding.rs  # full Device-owned discover/configure/open/start
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
│   ├── tests/sdp.rs           # source/sink discovery through SDP runtime
│   └── tests/host_media.rs    # RTP over Device-owned Classic channels
├── bumble-rtp/                # slice-32 RTP media packet codec
│   ├── src/lib.rs             # header, CSRC, extension, payload, padding
│   └── tests/packets.rs       # exact, full-featured, and malformed packets
├── bumble-avc/                # slice-39 AV/C frame codec for AVRCP
│   ├── src/lib.rs             # generic/vendor/pass-through frames
│   └── tests/frames.rs        # upstream exact vectors + malformed inputs
├── bumble-avctp/              # slice-40 AV/C transport over Classic L2CAP
│   ├── src/lib.rs             # messages, fragmentation, L2CAP protocol
│   ├── tests/protocol.rs      # upstream assembler + live PID/IPID flows
│   └── tests/host_protocol.rs # fragmented Device-owned PID/IPID flows
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
├── bumble-codecs/             # slice-48 bitstreams + slice-134 LC3 media
│   ├── src/{lib,g722,lc3}.rs  # bit I/O, LATM/ADTS, G.722, owned LC3 workers
│   ├── tests/codecs.rs        # upstream fixture + length-boundary round trips
│   ├── tests/g722.rs          # upstream fixture PCM + state continuity
│   └── tests/lc3.rs           # stateful stereo/multiframe LC3 SDU round trips
├── bumble-audio/              # slices 98/156 portable and platform audio I/O
│   ├── src/lib.rs             # formats, streams/files, WAVE, subprocesses
│   ├── src/sound_device.rs    # optional CPAL enumeration/input/output workers
│   └── tests/io.rs            # framing, looping, factories, worker delivery
├── bumble-transport/          # slices 75-82 external HCI transports
│   ├── build.rs               # vendored-protoc Android gRPC generation
│   ├── proto/{android_emulator,netsim_common,netsim_startup,netsim_packet_streamer}.proto
│   ├── src/{android_emulator,android_netsim,bridge,lib,common,dispatch,file,hci_socket,serial,pty,tcp,udp,usb,unix,websocket,vhci}.rs
│   ├── src/bin/bumble-bench.rs # slice-137 multi-mode external benchmark
│   ├── tests/android_emulator.rs # packet mapping + real host/controller gRPC loopback
│   ├── tests/android_netsim.rs # startup, INI, wire tags, live lease/packet stream
│   ├── tests/bridge.rs        # replacement, response, trace, and EOF paths
│   ├── tests/hci_socket.rs    # selectors, Linux ABI, framing, and I/O failures
│   ├── tests/transports.rs    # fragmentation, EOF, and socket loopbacks
│   ├── tests/specs.rs         # dispatch, serial config, and raw PTY coverage
│   ├── tests/usb.rs           # selectors, endpoints, and transfer routing
│   ├── tests/websocket.rs     # binary framing + client/server handshake
│   └── tests/vhci.rs          # virtual-controller bootstrap + H4 exchange
├── bumble-drivers/            # slices 83-84 vendor controller initialization
│   ├── src/lib.rs             # driver-host and firmware-provider contracts
│   ├── src/intel.rs           # Intel TLV, SFI, DDC, and init sequence
│   ├── src/rtk.rs             # Realtek epatch, matrix, download/init sequence
│   ├── tests/intel.rs         # exact wire, parser, lookup, full cold-start flow
│   ├── tests/rtk.rs           # epatch failures, matrix, wrap, full download flow
│   └── tests/selection.rs     # forced/unknown/automatic driver selection
├── bumble-profiles/           # slices 85-94: all 23 upstream profile modules
│   ├── src/{gap,gatt_service,battery_service,device_information_service,heart_rate_service,asha,hap,csip,vcs,vocs,aics,mcp,le_audio,bap,pacs,tmap,gmap,pbp,ascs,bass,cap,ams,ancs}.rs
│   ├── tests/foundational_services.rs # live typed proxy/control/hash coverage
│   ├── tests/hearing_profiles.rs # ASHA state + CSIS vectors/encrypted reads
│   ├── tests/volume_controls.rs # encrypted VCS/VOCS/AICS control matrices
│   ├── tests/media_control.rs # typed MCS events + GMCS control handshake
│   ├── tests/le_audio_pacs.rs # metadata/LTV/PAC/announcement vectors + live PACS reads
│   ├── tests/role_profiles.rs # TMAP/GMAP live reads + PBP announcement vectors
│   ├── tests/ascs.rs          # all operations + live sink/source ASE lifecycle
│   ├── tests/bass_cap.rs      # BASS wire/live state + CAS included CSIS
│   ├── tests/hap.rs           # encrypted presets, changes, wrap, synchronization
│   └── tests/apple_profiles.rs # AMS/ANCS 128-bit GATT + client/data flows
├── bumble-pandora/            # slices 138-140 complete Pandora services
│   ├── proto/pandora/{host,security,l2cap}.proto # canonical v0.0.6 interfaces
│   ├── src/{config,data_types,runtime,host,security,l2cap}.rs # external-HCI services
│   ├── src/bin/bumble-pandora-server.rs # runnable conformance server
│   └── tests/{host_grpc,l2cap_grpc}.rs # live tonic → controller proofs
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
