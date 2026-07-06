# bumble-rs — Slice 1: Core Types & Advertising Data

**Date:** 2026-07-06
**Status:** Approved (design)
**Source of truth:** `google/bumble` (`bumble/core.py`, `bumble/data_types.py`, `bumble/hci.py`), pinned via a shallow clone at design time.

## Context

`google/bumble` is a ~70,000-line Python dual-mode Bluetooth host stack plus a software
controller. A full port is a multi-engineer, multi-month effort. We are porting it to Rust
**slice by slice**, each slice a compiling, fully-tested vertical.

This is **slice 1: the shared primitives** — the types every higher layer (HCI, L2CAP,
ATT/GATT, SMP) depends on. It is intentionally self-contained: no async, no I/O, no hardware,
std-only.

## Scope

### In scope (idiomatic Rust ports of exactly what the acceptance tests exercise)

- **`Uuid`** — 16/32/128-bit UUIDs, little-endian internal storage, big-endian strings.
  - Parse from `"b5ea"` (16-bit), `"df5ce654"` (32-bit), and dashed 36-char 128-bit form.
  - `from_bytes` (2/4/16 bytes), `from_16_bits`, `from_32_bits`.
  - `to_hex_str(separator)`, `to_bytes(force_128)`, 128-bit expansion.
  - Equality & hashing on the **128-bit expansion** (a 16-bit UUID equals its 128-bit form).
- **`Address`** — Bluetooth device address (from `hci.py`).
  - 6-byte little-endian storage; parse from `"C4:F2:17:1A:1D:BB"` and `.../P` (public) forms.
  - `AddressType` (open): PUBLIC_DEVICE, RANDOM_DEVICE, PUBLIC_IDENTITY, RANDOM_IDENTITY.
  - Predicates: `is_public`, `is_random`, `is_resolvable`, `is_resolved`, `is_static`
    (top-2-bits-of-MSB logic on `address_bytes[5]`).
- **`Appearance`** — `int = (category << 6) | subcategory`; `from_int` = `(v >> 6, v & 0x3F)`.
  - Note: constructor / `to_int` do **not** mask the subcategory.
  - String forms: `"COMPUTER/LAPTOP"`, and for unknown subcategory
    `"HUMAN_INTERFACE_DEVICE/HumanInterfaceDeviceSubcategory[119]"`.
- **`ClassOfDevice`** — packing `major_service_classes << 13 | major_device_class << 8 | minor << 2`.
  - String form: `"ClassOfDevice(RENDERING|AUDIO,AUDIO_VIDEO/CAMCORDER)"`; unknown minor renders
    as `0x123` (Python `hex()` style).
- **`AdvertisingData`** — **raw TLV only**: `from_bytes`, `append`, `get`, `get_all`, `to_bytes`.
  - `AdvertisingData::Type` open enum (FLAGS, TX_POWER_LEVEL, … MANUFACTURER_SPECIFIC_DATA).
- **`get_dict_key_by_value`** helper and a crate error type (`Error`).

### Deferred (no acceptance test touches these — called out so "done" is honest)

- The 3,349-line `company_ids` table (only feeds an untested manufacturer-data string).
- The typed `DataType` subclass hierarchy in `data_types.py` (raw TLV covers `test_ad_data`;
  a `DataType` trait seam is left for slice 2).
- `Address::generate_static_address` / `generate_private_address` (would pull in crypto/RNG for
  an untested surface).

## Acceptance contract (closed set)

"All tests pass" means these **7 Python tests, ported 1:1** to Rust `#[test]`s, matching
observable behavior (byte-level encoding and asserted string outputs) exactly:

| Test | Source | Exercises |
|------|--------|-----------|
| `test_ad_data` | `tests/core_test.py` | AdvertisingData TLV append/get/get_all/bytes |
| `test_get_dict_key_by_value` | `tests/core_test.py` | reverse-lookup helper |
| `test_uuid_to_hex_str` | `tests/core_test.py` | 16/32/128-bit hex formatting, `-` separator |
| `test_uuid_hash` | `tests/core_test.py` | 16-bit UUID equals/hashes as its 128-bit form |
| `test_appearance` | `tests/core_test.py` | encode/decode + open-enum unknown values + strings |
| `test_class_of_device` | `tests/core_test.py` | CoD string forms incl. unknown minor |
| `test_address` | `tests/hci_test.py` | address parse + is_public/random/resolvable/resolved/static |

Green `cargo test` over the ported suite = slice complete.

## Key design decisions

1. **Open enums.** `Category`, subcategory groups, `AddressType`, `AdvertisingData::Type`, and
   CoD class fields are newtypes over the integer with named associated consts (or an
   `Unknown(n)` representation) — **not** closed Rust `enum`s. The tests deliberately feed
   unknown values (`Appearance::from_int(0x3333)` ⇒ `category == 0xCC`, `subcategory == 0x33`;
   `Appearance::new(HID, 0x77)` where 0x77 is 7 bits and is **not** masked). Closed enums fail these.
2. **Little-endian storage** for `Uuid` and `Address` bytes, matching Python; strings big-endian.
3. **128-bit equality/hash** for `Uuid`.
4. **Drop the global `UUID.UUIDS` registry** — un-idiomatic, no test reads it.
5. **Cargo workspace** rooted at `bumble-rs/`, with the `bumble` library crate as the first
   member so slices 2+ (hci, l2cap, …) become sibling crates. std-only for this slice.
6. **Behavior-faithful, idiomatic API** (chosen over mechanical 1:1 transliteration): match bytes
   and asserted strings exactly; use `Result`, newtypes, and traits for the API surface.

## Module layout

```
bumble-rs/
  Cargo.toml                 # workspace
  bumble/
    Cargo.toml
    src/
      lib.rs                 # re-exports, get_dict_key_by_value, Error
      uuid.rs
      address.rs
      appearance.rs
      class_of_device.rs
      advertising_data.rs
    tests/
      core_test.rs           # ported from tests/core_test.py
      address_test.rs        # ported test_address from tests/hci_test.py
```

## Testing strategy

- Port the 7 Python tests verbatim in intent as Rust integration tests under `bumble/tests/`.
- Unit tests inline where they clarify a tricky encoding (UUID little-endian, Appearance masking).
- No mocking needed — everything is pure functions over bytes/strings.

## Future slices (not this work)

HCI packet codec → software controller + virtual link → L2CAP → ATT/GATT → SMP. Each gets its
own spec → plan → implementation cycle and becomes a sibling crate in the workspace.
