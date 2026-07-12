# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Initial incremental port of [`google/bumble`](https://github.com/google/bumble)
to Rust — a minimal but end-to-end LE Bluetooth stack, built one verified slice
at a time. All wire formats are pinned to byte-exact reference output; the SMP
crypto is pinned to Bluetooth-spec / RFC 4493 vectors.

### Added

- **`bumble`** — core types: `Uuid`, `Address`/`AddressType`, `Appearance`,
  `ClassOfDevice`, and `AdvertisingData` (raw TLV).
- **`bumble-hci`** — HCI packet codec: framing, ~22 commands, events (incl. LE
  meta events, Command Complete + typed return parameters, Disconnection
  Complete), and ACL/SCO/ISO data packets.
- **`bumble-l2cap`** — L2CAP frame codec: `L2capPdu` with FCS (CRC-16),
  variable-length PSM, and signaling `ControlFrame`s. Plus (slice 21) the
  Classic Connection/Configure/Disconnection frames and a synchronous
  connection-oriented `ChannelManager`: validated PSM registration, dynamic
  PSM/CID allocation, MTU negotiation, incoming accept/refusal, basic-mode SDU
  transfer, and disconnect, verified peer-to-peer.
- **`bumble-att`** — ATT protocol PDU codec: discovery (Read_By_Type/Group_Type,
  Find_Information, Find_By_Type_Value), reads (Read, Read_Blob), writes
  (Write_Request, Write_Command), and notifications/indications with
  confirmation — all oracle-pinned.
- **`bumble-crypto`** — SMP cryptographic toolbox: `e`, AES-CMAC, `c1`, `s1`,
  `f4`/`f5`/`f6`, `g2`, `h6`/`h7`, `ah`. Plus (slice 19) a P-256 `EccKey`
  (`generate`, `from_private_key_bytes`, public-key coordinates, ECDH `dh`)
  ported from upstream `crypto.EccKey` and oracle-pinned (public keys +
  Diffie-Hellman shared secret).
- **`bumble-gatt`** — a minimal ATT attribute server and a `GattServer` with a
  service/characteristic model, primary discovery, Find_Information /
  Find_By_Type_Value discovery, a CCCD descriptor per notify/indicate
  characteristic, MTU-sized reads with Read_Blob, and server-initiated
  notify/indicate. Plus (slice 18) a synchronous `GattClient` — service /
  characteristic / descriptor discovery, reads (with long-read), writes (with
  and without response), and notify/indicate subscriptions — verified by a
  two-party client↔server integration test.
- **`bumble-smp`** — SMP PDU codec and LE Legacy pairing (`c1`/`s1`) helpers.
  Plus (slice 19) the remaining LE Secure Connections PDUs (public key, DHKey
  check, keypress, and the five key-distribution PDUs) and an `sc` module with
  the SC JustWorks derivation — the `f4` responder confirm, `f5` `(MacKey, LTK)`,
  the `f6` DHKey checks, and the `g2` numeric value — composing `bumble-crypto`.
  The PDUs are pinned to captures from upstream's command classes; the
  derivation is pinned to a Python reference built from upstream's `crypto`
  functions arranged exactly as `smp.py` arranges them (verified line-for-line
  against `smp.py`), the arrangement being the one link the spec-verified
  primitives and oracle-pinned DHKey don't independently cover.
- **`bumble-sdp`** — Service Discovery Protocol codec (the first Classic/BR-EDR
  piece): the recursive `DataElement` type-length-value format, the
  `ServiceAttribute` service-record model, and all seven `SdpPdu` messages —
  oracle-pinned to upstream `sdp_test.py`. Plus (slice 20) a `service` module
  with a synchronous `SdpServer`/`SdpClient` runtime — a service-record
  database, UUID matching, attribute selection, and continuation-state chunking
  + reassembly for all three query types — verified by a two-party in-process
  test in which the server's response PDUs are pinned to the real upstream
  Python `Server` (single-PDU and a forced four-round continuation).
  Slice 22 adds `SdpL2capServer` and `L2capSdpTransport`, carrying those same
  requests and continuation responses over negotiated Classic L2CAP channels
  with explicit transport-error propagation.
- **`bumble-rfcomm`** — RFCOMM frame + MCC codec (serial-cable emulation over
  L2CAP): the `RfcommFrame` TS 07.10 framing (SABM/UA/DM/DISC/UIH, 1- and
  2-byte length indicators, credit-based UIH flow control), the CRC-8
  `compute_fcs`, and the `RfcommMccPn`/`RfcommMccMsc` MCC messages — oracle-
  pinned to upstream `rfcomm_test.py`. Plus (slice 20) a `mux` module with a
  synchronous, sans-I/O `Multiplexer`/`DLC` session runtime — session open,
  the PN/SABM/MSC DLC handshake, and the `process_tx` credit-flow engine —
  verified by a two-party in-memory relay test in which the open-handshake
  frames are pinned to the real upstream state machine and credit exhaustion +
  replenishment is forced explicitly. Slice 22 adds `L2capMultiplexer`, binding
  the runtime to a Classic channel and verifying session/DLC open, credit
  replenishment, ordered application data, and disconnect across two L2CAP
  peers.
- **`bumble-at`** — upstream-compatible AT parameter tokenization/parsing,
  typed HFP command/response forms, and incremental command/response stream
  parsers that handle RFCOMM fragmentation and coalescing (slice 23).
- **`bumble-hfp`** — paired HF/AG service-level-connection state machines with
  normative feature, codec, indicator, and call-hold models. The mandatory
  BRSF/CIND/CMER flow and optional BAC/CHLD/BIND branches are transcript-pinned
  and verified end-to-end over RFCOMM and Classic L2CAP (slice 24). Slice 25
  continues with call control/current-call queries, HF and AG indicator updates,
  unsolicited ring/volume/caller-ID/voice events, serialized command results,
  and the BCS codec handshake, including live RFCOMM/L2CAP coverage. Slice 26
  adds role-correct HF/AG SDP records, feature mapping, and discovery parsing,
  verified through the SDP client/server runtime. Slice 27 ports all eight HFP
  1.8 SCO/eSCO default parameter sets and builds enhanced setup/accept HCI
  commands for negotiated CVSD and mSBC audio. Slice 28 closes the remaining
  synchronous protocol gaps with response-hold/call-state/voice/CME models,
  full caller-ID metadata, CMEE/CCWA/BIA/CLIP AG controls, in-band ringtone
  state, upstream public call/audio helpers, and batched-command coverage.
- **`bumble-controller`** — a synchronous software controller and in-process
  `LocalLink`: advertising/scanning, LE connection establishment, ACL routing,
  and disconnection. Slice 27 adds Classic SCO/eSCO request, accept, reject,
  connection completion, independent and ACL-cascaded teardown, and
  bidirectional HCI synchronous-data routing.
- **`bumble-host`** — a `Device`/`pump` host layer that owns the
  ATT↔L2CAP↔ACL sequencing, so two virtual devices run the full LE lifecycle
  — connect → discover → read/write → notify → pair (JustWorks) → disconnect —
  through a library API. Slice 19 adds a second pairing integration test: a
  two-party **LE Secure Connections** JustWorks handshake (public-key + nonce
  exchange, `f4` confirm, `f6` DHKey checks) in which both peers derive the same
  LTK. Slice 27 adds Classic ACL and SCO/eSCO connection/request/data APIs and
  verifies an HFP mSBC audio link through the complete host/controller boundary.
- **`bumble-avdtp`** — slice 29 starts the Classic A/V stack with all 38 AVDTP
  signaling message forms, endpoint and capability codecs, exact payload
  vectors, lossless unknown signals, and MTU-aware single/fragmented PDU
  encoding and safe reassembly. Slice 30 adds the endpoint/session state
  machine, atomic multi-stream validation, lifecycle/security/delay events, and
  transaction-labeled signaling over live Classic L2CAP channels.
- **`bumble-a2dp`** — slice 31 ports the SBC, MPEG-2/4 AAC, vendor-specific, and
  Opus codec capability models, pins the upstream byte vectors, validates
  malformed/range inputs, and converts typed codec information into AVDTP
  media-codec capabilities. Slice 33 adds safe SBC frame parsing and MTU-aware
  RTP aggregation, including a correct final-buffer flush missing upstream.
  Slice 34 adds ADTS AAC parsing and exact simple LATM/RTP packet construction.
  Slice 35 adds validated Ogg Opus logical-stream parsing and 20 ms RTP packets.
  Slice 36 binds typed RTP packets to a live Classic L2CAP media channel with
  negotiated-MTU enforcement and source-to-sink verification. Slice 37 adds
  upstream-shaped source/sink SDP records and strict discovery parsing. Slice
  38 completes the synchronous profile client with remote SEP discovery, codec
  selection, and configure/open/start/suspend/close orchestration over live
  AVDTP signaling.
- **`bumble-rtp`** — slice 32 adds safe RTP media packet parsing/serialization,
  including CSRC lists, header extensions, padding, exact byte round trips, and
  explicit errors for malformed remote input.
- **`bumble-avc`** — slice 39 starts AVRCP's dependency stack with generic AV/C
  command/response frames, extended subunit IDs, 24-bit vendor-dependent
  payloads, panel pass-through keys, exact upstream vectors, and malformed
  input validation.
- **`bumble-avctp`** — slice 40 adds transaction/PID messages, safe
  single/fragmented assembly, MTU-aware encoding, live Classic L2CAP delivery,
  registered PID routing, and automatic IPID responses.

### Known limitations

- LE Secure Connections: the crypto (P-256 ECDH) and the JustWorks derivation
  (`f4`/`f5`/`f6`/`g2`) are wired and two-party verified, but only for JustWorks
  — the full pairing state machine, Numeric Comparison / passkey / OOB entry UX,
  key distribution over the wire, and bonding storage are not ported.
- GATT supports the CCCD descriptor and notify/indicate subscriptions, but not
  the full descriptor set, included services, or prepared/queued writes; no
  L2CAP fragmentation/reassembly.
- Of Classic Bluetooth, the SDP and RFCOMM codecs plus their session runtimes
  (SDP client/server + service-record database; the RFCOMM multiplexer/DLC
  credit-flow state machine) and the basic-mode Classic L2CAP channel runtime
  exist as synchronous, sans-I/O components with live channel bindings.
  Enhanced retransmission, aggregate RFCOMM flow control, socket/async
  convenience APIs, and A2DP/AVRCP/HID profile behavior are not ported. The
  AVDTP signaling and endpoint runtime exist over Classic L2CAP, but its
  initiator convenience and RTP media channel/pump remain. A2DP codec
  capabilities exist, while media frame parsing/packet sources remain. HFP's
  AT grammar, stream framing, service-level connection, SDP records, and
  SCO/eSCO HCI audio-link orchestration are available. Its synchronous command
  behavior now covers the upstream HFP test families; executor conveniences and
  media codec implementations remain.
- The controller/link are synchronous (no async runtime) by design.
