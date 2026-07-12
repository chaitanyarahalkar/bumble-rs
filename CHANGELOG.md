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
  variable-length PSM, and signaling `ControlFrame`s.
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
  oracle-pinned to upstream `sdp_test.py`.
- **`bumble-rfcomm`** — RFCOMM frame + MCC codec (serial-cable emulation over
  L2CAP): the `RfcommFrame` TS 07.10 framing (SABM/UA/DM/DISC/UIH, 1- and
  2-byte length indicators, credit-based UIH flow control), the CRC-8
  `compute_fcs`, and the `RfcommMccPn`/`RfcommMccMsc` MCC messages — oracle-
  pinned to upstream `rfcomm_test.py`.
- **`bumble-controller`** — a synchronous software controller and in-process
  `LocalLink`: advertising/scanning, LE connection establishment, ACL routing,
  and disconnection.
- **`bumble-host`** — a `Device`/`pump` host layer that owns the
  ATT↔L2CAP↔ACL sequencing, so two virtual devices run the full LE lifecycle
  — connect → discover → read/write → notify → pair (JustWorks) → disconnect —
  through a library API. Slice 19 adds a second pairing integration test: a
  two-party **LE Secure Connections** JustWorks handshake (public-key + nonce
  exchange, `f4` confirm, `f6` DHKey checks) in which both peers derive the same
  LTK.

### Known limitations

- LE Secure Connections: the crypto (P-256 ECDH) and the JustWorks derivation
  (`f4`/`f5`/`f6`/`g2`) are wired and two-party verified, but only for JustWorks
  — the full pairing state machine, Numeric Comparison / passkey / OOB entry UX,
  key distribution over the wire, and bonding storage are not ported.
- GATT supports the CCCD descriptor and notify/indicate subscriptions, but not
  the full descriptor set, included services, or prepared/queued writes; no
  L2CAP fragmentation/reassembly.
- Of Classic Bluetooth, only the SDP and RFCOMM codecs exist; A2DP/AVRCP/HFP/HID
  and the async runtimes — the SDP client/server + service-record database and
  the RFCOMM DLC/multiplexer credit-flow state machine — are not ported.
- The controller/link are synchronous (no async runtime) by design.
