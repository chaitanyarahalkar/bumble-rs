# bumble-rs Slice 1 (Core Types & Advertising Data) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port Bumble's shared Bluetooth primitives (UUID, Address, Appearance, ClassOfDevice, AdvertisingData TLV) to an idiomatic Rust `bumble` crate with the 7 Python acceptance tests passing.

**Architecture:** A Cargo workspace rooted at `bumble-rs/` with one library crate `bumble`. Each primitive is one focused module. Open enums are newtypes-over-int so unknown values round-trip. Bytes are little-endian internally (matching Python); strings are big-endian.

**Tech Stack:** Rust 2021, std-only, `cargo test`. No external dependencies for this slice.

## Global Constraints

- Rust edition 2021; toolchain ≥ 1.95 (available).
- Zero external crate dependencies (std only).
- Byte-level encoding and asserted string outputs MUST match Python exactly.
- Open enums (`AddressType`, `Appearance` category/subcategory, `AdvertisingData::Type`, CoD fields) must preserve unknown integer values (no closed-enum truncation).
- UUID equality & hashing operate on the 128-bit expansion.
- Frequent commits: one per task.

---

### Task 1: Workspace + crate scaffold + `get_dict_key_by_value` + `Error`

**Files:**
- Create: `Cargo.toml` (workspace), `bumble/Cargo.toml`, `bumble/src/lib.rs`

**Interfaces:**
- Produces: crate `bumble`; `pub fn get_dict_key_by_value<K: Clone, V: PartialEq>(map: &[(K, V)], value: &V) -> Option<K>`; `pub enum Error { InvalidArgument(String), InvalidPacket(String) }` with `Display`+`std::error::Error`; `pub type Result<T> = core::result::Result<T, Error>`.

- [ ] **Step 1:** Write workspace `Cargo.toml`:
```toml
[workspace]
members = ["bumble"]
resolver = "2"
```
- [ ] **Step 2:** Write `bumble/Cargo.toml`:
```toml
[package]
name = "bumble"
version = "0.1.0"
edition = "2021"

[dependencies]
```
- [ ] **Step 3:** Write `bumble/src/lib.rs` with `Error`, `Result`, `get_dict_key_by_value`, module declarations (`mod uuid; mod address; ...`) added as later tasks land, and an inline unit test for `get_dict_key_by_value` mirroring `test_get_dict_key_by_value` (map `[("A",1),("B",2)]`: value 1→"A", 2→"B", 3→None).
- [ ] **Step 4:** Run `cargo test -p bumble` — expect compile + the helper test passing.
- [ ] **Step 5:** Commit `feat: workspace scaffold + get_dict_key_by_value + Error`.

---

### Task 2: `Uuid`

**Files:**
- Create: `bumble/src/uuid.rs`; Modify: `bumble/src/lib.rs` (add `pub mod uuid;`)

**Interfaces:**
- Produces: `pub struct Uuid { bytes: Vec<u8> }` (2/4/16 LE bytes). Methods: `from_16_bits(u16)`, `from_32_bits(u32)`, `from_bytes(&[u8]) -> Result<Uuid>` (len 2/4/16), `parse(&str) -> Result<Uuid>` (4/8/32 hex chars or 36-char dashed), `to_bytes(force_128: bool) -> Vec<u8>`, `uuid_128_bytes() -> [u8;16]`, `to_hex_str(sep: &str) -> String`. `impl PartialEq/Eq/Hash` on 128-bit expansion. `BASE_UUID` const = `00001000800000805F9B34FB` reversed (LE).

**Encoding notes (from `core.py`):**
- 16-bit stored as `u16.to_le_bytes()`. String parse: hex decode big-endian then reverse into LE.
- `uuid_128_bytes`: 2→`BASE_UUID + bytes + [0,0]`; 4→`BASE_UUID + bytes`; 16→bytes.
- `to_hex_str`: for 2/4 bytes → `reversed(bytes).hex().upper()` (no separator use). For 16 bytes → 5 groups joined by `sep`: `[12:16],[10:12],[8:10],[6:8],[0:6]` each reversed, hex, then whole uppercased.

- [ ] **Step 1:** Write failing tests in `uuid.rs` `#[cfg(test)]`: replicate `test_uuid_to_hex_str` (`"b5ea"`→`"B5EA"`, `"df5ce654"`→`"DF5CE654"`, dashed→32-char upper; `sep="-"` variants incl. dashed→`"DF5CE654-E059-11ED-B5EA-0242AC120002"`) and `test_uuid_hash` (16-bit UUID equals `Uuid::from_bytes(uuid.to_bytes(true))`; both directions in a `HashSet`).
- [ ] **Step 2:** Run `cargo test -p bumble uuid` — expect FAIL (methods missing).
- [ ] **Step 3:** Implement `Uuid` per encoding notes.
- [ ] **Step 4:** Run `cargo test -p bumble uuid` — expect PASS.
- [ ] **Step 5:** Commit `feat: Uuid with LE storage + 128-bit eq/hash`.

---

### Task 3: `Address` + `AddressType`

**Files:**
- Create: `bumble/src/address.rs`; Modify: `bumble/src/lib.rs`

**Interfaces:**
- Produces: `pub struct AddressType(pub u8)` with consts `PUBLIC_DEVICE=0, RANDOM_DEVICE=1, PUBLIC_IDENTITY=2, RANDOM_IDENTITY=3`. `pub struct Address { bytes: [u8;6], address_type: AddressType }`. `Address::parse(&str, AddressType) -> Result<Address>` (handles `":"` separators and `/P` suffix → PUBLIC_DEVICE), `from_bytes([u8;6], AddressType)`. Predicates `is_public/is_random/is_resolvable/is_resolved/is_static -> bool`. `to_string(with_type_qualifier: bool)`.

**Logic (from `hci.py`):**
- String parse: strip `/P` suffix (Python strips last 2 chars `[:-2]` after detecting trailing `P`, and forces PUBLIC_DEVICE); if len==17 remove `:`; hex-decode big-endian then reverse into LE `bytes[0..6]`.
- `is_public`: type ∈ {PUBLIC_DEVICE, PUBLIC_IDENTITY}. `is_random`: `!is_public`.
- `is_resolved`: type ∈ {PUBLIC_IDENTITY, RANDOM_IDENTITY}.
- `is_resolvable`: type==RANDOM_DEVICE && `bytes[5] >> 6 == 1`.
- `is_static`: `is_random && bytes[5] >> 6 == 3`.

- [ ] **Step 1:** Write failing test replicating `test_address`: `Address::parse("C4:F2:17:1A:1D:BB", RANDOM_DEVICE)` → `!is_public`, `is_random`, `address_type==RANDOM_DEVICE`, `!is_resolvable`, `!is_resolved`, `is_static`.
- [ ] **Step 2:** Run `cargo test -p bumble address` — expect FAIL.
- [ ] **Step 3:** Implement `Address`/`AddressType`.
- [ ] **Step 4:** Run — expect PASS.
- [ ] **Step 5:** Commit `feat: Address + AddressType with predicates`.

---

### Task 4: `Appearance`

**Files:**
- Create: `bumble/src/appearance.rs`; Modify: `bumble/src/lib.rs`

**Interfaces:**
- Produces: `pub struct Category(pub u16)` (open) with consts incl. `COMPUTER=0x0002, HUMAN_INTERFACE_DEVICE=0x000F, BLOOD_PRESSURE=0x000E`; a `category_name(u16) -> Option<&'static str>`. `pub struct Appearance { category: Category, subcategory: u16 }`. `Appearance::new(Category, u16)`, `from_int(u16) -> Appearance` (`category = v>>6`, `subcategory = v & 0x3F`), `to_int(&self) -> u16` (`(category.0 << 6) | subcategory`, NO mask), `Display`.
- Subcategory naming: per-category tables. For the test we need `COMPUTER`+`ComputerSubcategory` (`LAPTOP=0x03`), `HUMAN_INTERFACE_DEVICE`+`HumanInterfaceDeviceSubcategory`, `BLOOD_PRESSURE`+`BloodPressureSubcategory` (`ARM_BLOOD_PRESSURE=0x01`).

**String forms (`__str__` = `f"{category.name}/{subcategory.name}"`):**
- Known subcat name → e.g. `"COMPUTER/LAPTOP"`.
- Unknown subcat within a known category (open-enum) → Python renders the OpenIntEnum name as `"<ClassName>[<int>]"`, e.g. `"HUMAN_INTERFACE_DEVICE/HumanInterfaceDeviceSubcategory[119]"`.
- Unknown category (`0x3333>>6 = 0xCC`) → we only assert `category==0xCC`, `subcategory==0x33`, `to_int==0x3333`; no string asserted.

- [ ] **Step 1:** Write failing test replicating `test_appearance` (5 cases incl. `new(COMPUTER, LAPTOP)`→str `"COMPUTER/LAPTOP"`, int `0x0083`; `new(HID, 0x77)`→str `"HUMAN_INTERFACE_DEVICE/HumanInterfaceDeviceSubcategory[119]"`, int `0x03C0|0x77`; `from_int(0x0381)`→BLOOD_PRESSURE/ARM_BLOOD_PRESSURE, int 0x381; `from_int(0x038A)`→BLOOD_PRESSURE/0x0A, int 0x38A; `from_int(0x3333)`→cat 0xCC, subcat 0x33, int 0x3333).
- [ ] **Step 2:** Run `cargo test -p bumble appearance` — expect FAIL.
- [ ] **Step 3:** Implement `Appearance` + the 3 needed subcategory name tables + category names. Use a per-category `subcategory_name(category, sub) -> Option<&'static str>` and a `subcategory_class_name(category) -> &'static str` for the `Name[int]` fallback.
- [ ] **Step 4:** Run — expect PASS.
- [ ] **Step 5:** Commit `feat: Appearance encode/decode + open-enum string forms`.

---

### Task 5: `ClassOfDevice`

**Files:**
- Create: `bumble/src/class_of_device.rs`; Modify: `bumble/src/lib.rs`

**Interfaces:**
- Produces: `pub struct MajorServiceClasses(pub u16)` (bitflags, open) with `AUDIO`, `RENDERING` consts + `composite_name()`. `pub struct MajorDeviceClass(pub u8)` open with `AUDIO_VIDEO` + name. `pub struct ClassOfDevice { service: MajorServiceClasses, major: MajorDeviceClass, minor: u32 }`. `new(service, major, minor)`, `from_int(u32)`, `to_int(&self)->u32`, `Display`.

**Logic (`core.py`):**
- `from_int`: service = `v>>13 & 0x7FF`, major = `v>>8 & 0x1F`, minor = `v>>2 & 0x3F`.
- `to_int`: `service<<13 | major<<8 | minor<<2`.
- `Display`: `ClassOfDevice({service.composite_name},{major.name}/{minor_name})` where `minor_name` = known name or `0x{minor:X}` (Python `hex()` → lowercase `0x` prefix, e.g. `0x123`).
- `composite_name`: `"|".join(bit_flags_to_strings(...))` iterating the MajorServiceClasses bit names; for the test the value `RENDERING|AUDIO` must render exactly as `"RENDERING|AUDIO"` — replicate Python bit iteration order (see `bit_flags_to_strings`, iterates from LSB). AUDIO bit and RENDERING bit values determine order; verify against `"RENDERING|AUDIO,AUDIO_VIDEO/CAMCORDER"`.
- Minor device class for AUDIO_VIDEO includes `CAMCORDER`. Read exact enum values during impl.

- [ ] **Step 1:** Write failing test replicating `test_class_of_device` (c1 → `"ClassOfDevice(RENDERING|AUDIO,AUDIO_VIDEO/CAMCORDER)"`; c2 with minor `0x123` → `"ClassOfDevice(AUDIO,AUDIO_VIDEO/0x123)"`).
- [ ] **Step 2:** Run — expect FAIL.
- [ ] **Step 3:** Implement, reading exact `MajorServiceClasses`, `MajorDeviceClass.AUDIO_VIDEO`, `AudioVideoMinorDeviceClass.CAMCORDER` values and `bit_flags_to_strings` order from `core.py`.
- [ ] **Step 4:** Run — expect PASS.
- [ ] **Step 5:** Commit `feat: ClassOfDevice packing + string form`.

---

### Task 6: `AdvertisingData` (raw TLV) + `Type`

**Files:**
- Create: `bumble/src/advertising_data.rs`; Modify: `bumble/src/lib.rs`

**Interfaces:**
- Produces: `pub struct Type(pub u8)` open, consts incl. `FLAGS=0x01, TX_POWER_LEVEL=0x0A, COMPLETE_LOCAL_NAME=0x09, MANUFACTURER_SPECIFIC_DATA=0xFF`. `pub struct AdvertisingData { ad_structures: Vec<(Type, Vec<u8>)> }`. `from_bytes(&[u8])`, `append(&mut self, &[u8])`, `get(&self, Type) -> Option<Vec<u8>>` (first match), `get_all(&self, Type) -> Vec<Vec<u8>>`, `to_bytes(&self) -> Vec<u8>` (per structure: `[len=1+data.len(), type, data...]`).

**Logic (`core.py append`):** iterate `while offset+1 < len`: `length = data[offset]`, `ad_type = data[offset+1]`, `value = data[offset+2 .. offset+1+length]`, push `(Type(ad_type), value)`, `offset += length + 1`.

- [ ] **Step 1:** Write failing test replicating `test_ad_data` exactly (build `[2, TX_POWER_LEVEL, 123]`, round-trip bytes equal; `get(COMPLETE_LOCAL_NAME)` None; `get(TX_POWER_LEVEL)` == `[123]`; `get_all` variants; append second `[2, TX_POWER_LEVEL, 234]`; combined bytes; `get_all(TX_POWER_LEVEL)` == `[[123],[234]]`).
- [ ] **Step 2:** Run — expect FAIL.
- [ ] **Step 3:** Implement `AdvertisingData` + `Type`.
- [ ] **Step 4:** Run — expect PASS.
- [ ] **Step 5:** Commit `feat: AdvertisingData raw TLV codec`.

---

### Task 7: Ported acceptance test suite (integration) + full green

**Files:**
- Create: `bumble/tests/acceptance.rs` (all 7 ported tests as `#[test]` fns using the public API); Modify: `bumble/src/lib.rs` (ensure all needed items are `pub`).

**Interfaces:**
- Consumes: everything above through the crate's public API.

- [ ] **Step 1:** Write `bumble/tests/acceptance.rs` porting all 7 Python tests verbatim in intent (they may duplicate the inline unit tests — that's fine; this file is the contract mirror of `core_test.py`/`hci_test.py`).
- [ ] **Step 2:** Run `cargo test` (whole workspace) — expect all tests PASS.
- [ ] **Step 3:** Run `cargo build --release` and `cargo test --release` — expect clean.
- [ ] **Step 4:** Add a `README.md` documenting the slice, the acceptance mapping, and how to run tests.
- [ ] **Step 5:** Commit `test: ported acceptance suite; slice 1 green`.

---

## Self-Review

**Spec coverage:** Every in-scope spec item maps to a task — Uuid(T2), Address(T3), Appearance(T4), ClassOfDevice(T5), AdvertisingData(T6), get_dict_key_by_value/Error(T1), acceptance suite(T7). Deferred items (company_ids, DataType hierarchy, address generation) intentionally have no task.

**Placeholder scan:** Implementation-detail lookups deferred to impl time are limited to exact enum integer values (CoD minor classes, MajorServiceClasses bit order) which are read from `core.py` during the task — not vague "handle edge cases." All test contracts are fully specified.

**Type consistency:** `Uuid`, `Address`, `AddressType`, `Appearance`, `Category`, `ClassOfDevice`, `AdvertisingData`, `Type` names are used consistently across tasks and interfaces blocks.
