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
- **`bumble-att`** — ATT protocol PDU codec, including the discovery
  request/response PDUs.
- **`bumble-crypto`** — SMP cryptographic toolbox: `e`, AES-CMAC, `c1`, `s1`,
  `f4`/`f5`/`f6`, `g2`, `h6`/`h7`, `ah`.
- **`bumble-gatt`** — a minimal ATT attribute server and a `GattServer` with a
  service/characteristic model and primary discovery.
- **`bumble-smp`** — SMP PDU codec and LE Legacy pairing (`c1`/`s1`) helpers.
- **`bumble-controller`** — a synchronous software controller and in-process
  `LocalLink`: advertising/scanning, LE connection establishment, ACL routing,
  and disconnection.
- **`bumble-host`** — a `Device`/`pump` host layer that owns the
  ATT↔L2CAP↔ACL sequencing, so two virtual devices run the full LE lifecycle
  — connect → discover → read/write → notify → pair (JustWorks) → disconnect —
  through a library API.

### Known limitations

- LE Secure Connections pairing (ECDH + `f4`/`f5`/`f6`/`g2` handshake) is not
  wired, though the crypto primitives exist and are vector-verified.
- No GATT descriptors/CCCD subscriptions, L2CAP fragmentation/reassembly, or
  Classic Bluetooth (RFCOMM/SDP/A2DP/AVRCP/HFP).
- The controller/link are synchronous (no async runtime) by design.
