# bumble-rs

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
| 2. HCI packet codec (framing + representative commands/events) | `bumble-hci` | ✅ complete — 18/18 tests green |
| 3. Software controller + virtual link | — | planned |
| 4+. L2CAP → ATT/GATT → SMP | — | planned |

Slice 2 covers the HCI **framing foundation** plus a representative subset of
the command/event catalog — not the full ~700-entry catalog. Deferred to later
HCI slices: Advertising Report events, Command_Complete return-parameters, the
vendor-event factory, the parametrized full-registry checks, and complex
multi-array commands.

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
- **`Command`** — Reset, Disconnect, Set_Event_Mask, LE_Set_Event_Mask,
  LE_Set_Random_Address, LE_Set_Scan_Enable, and the three
  Read_Local_Supported/Version commands, plus a `Generic` fallback.
- **`Event` / `LeMetaEvent`** — Command_Status, Number_Of_Completed_Packets, and
  the LE Connection_Complete / Connection_Update_Complete /
  Channel_Selection_Algorithm / Read_Remote_Features_Complete meta events, plus
  `Generic` fallbacks.
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

Slice 2's 18 HCI tests live in
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
│   ├── src/{lib,codes,command,event,packet}.rs
│   └── tests/acceptance.rs    # ported hci_test.py cases (oracle-pinned)
└── docs/superpowers/          # design specs + implementation plans
```

## License

Apache-2.0, matching upstream Bumble.
