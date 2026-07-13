# hcigen — HCI catalog generator

`command.rs` (the full typed `Command` enum) and its opcode constants in
`codes.rs`, plus the oracle-pinned tests in `tests/generated_commands.rs`, are
**generated** from upstream [`google/bumble`](https://github.com/google/bumble)'s
declarative `bumble.hci` field specs. This keeps the ~200-command HCI catalog
faithful to the source instead of hand-transcribed.

## How it works

1. **`extract.py`** imports `bumble.hci`, walks every `HCI_*_Command` class's
   `fields` declaration, and normalizes each field to a codec descriptor
   (`u8`, `u16`, `u24`, `u32`, `i8`, `bytes:N`, `addr`, `codingformat`, `rest`,
   `varbytes`, or `array`). For each class it also serializes a representative
   instance — using **distinct, position-revealing byte values** — via
   upstream's own `HCI_Object.dict_to_bytes`, capturing ground-truth wire bytes.
   Output: `spec.json`.

2. **`gen_commands.py`** consumes `spec.json` and emits `src/codes.rs` (command
   opcodes), `src/command.rs` (the `Command` enum + `op_code()` + `parameters()`
   + `from_parameters()`), and `tests/generated_commands.rs`. Before emitting a
   single line, it **replays the same value generation and independently
   recomputes the wire bytes, asserting they equal upstream's captured bytes** —
   so the generator's codec model is proven against the oracle at generation
   time, and the emitted Rust tests re-verify it at `cargo test` time.

Two commands (`LE_Set_Extended_Scan_Parameters`, `LE_Extended_Create_Connection`)
are hand-written and embedded verbatim in the generator, because their array
element count comes from a PHY bitmask rather than a leading count byte, so they
are not derivable from the declarative field spec.

## Oracle coverage caveats

The self-check uses upstream's own serializer as ground truth, so a codec
misclassification surfaces as a length/content mismatch. Two things the current
distinct-value samples don't fully exercise (both low-risk, noted for honesty):

- **Array fields are sampled at one element.** Per-element layout is verified
  byte-exact; the count-loop itself is only run for count=1.
- **`CodingFormat`'s sample is `02 00 00 00 00`** (CVSD), so `company_id` vs
  `vendor_specific_codec_id` ordering isn't position-revealing — both are zero.

## Regenerating

Requires a Python environment with `bumble` importable:

```sh
export HCIGEN_OUT=/tmp/hcigen           # scratch dir for spec.json (optional)
export PYTHONPATH=/path/to/bumble       # upstream bumble checkout
python3 extract.py                      # -> $HCIGEN_OUT/spec.json
python3 gen_commands.py                 # -> ../../src/{codes,command}.rs, ../../tests/generated_commands.rs
cargo test -p bumble-hci                # verify oracle-pinned tests pass
```

`HCIGEN_OUT` defaults to this directory; output paths are resolved relative to
the script (the `bumble-hci` crate root).

## `gen_events.py`, `gen_metadata.py`, and `gen_surface.py`

- **`gen_events.py`** mirrors `gen_commands.py` for the event catalog — it emits
  `src/event.rs` (the `Event` / `LeMetaEvent` enums), regenerates the full
  `src/codes.rs` (command opcodes + event/sub-event codes), and
  `tests/generated_events.rs`. Run it after `gen_commands.py`. Four events are
  hand-written and embedded from `event_embed.json` (rebuilt by `make_embed.py`):
  `Command_Complete` (typed `ReturnParameters`) and the two nested-report
  advertising events.
- **`gen_surface.py`** reads upstream `controller.py`'s `on_hci_*_command`
  handlers, categorizes each by response shape (Command Complete status-only /
  data / Command Status), and emits `bumble-controller/src/command_surface.rs` —
  the table the software controller uses to reply to the full command surface.
  Set `BUMBLE_SRC` to the upstream checkout (default `/tmp/bumble-scope`).
- **`gen_metadata.py`** parses upstream `bumble/hci.py` without importing it and
  emits `src/metadata_tables.rs`: specification-version, LE-feature, codec-ID,
  and all 64-byte Supported Commands bitmap labels. Set `BUMBLE_SRC` to the
  upstream checkout (default `/tmp/bumble-scope`).
