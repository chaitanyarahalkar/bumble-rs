# Testing and Conformance

## Verification philosophy

The port's correctness rests on **ground-truth verification**, not on the code
agreeing with itself:

- **Codecs** (HCI, L2CAP, ATT, SMP, core types) are pinned to byte-exact
  outputs captured from the Python Bumble reference, or to the Bluetooth Core
  Specification. Wire-format tests assert serialized bytes against a value
  obtained from the reference — not just a serialize→parse round-trip.
- **Crypto** (`bumble-crypto`) is pinned to published specification and
  RFC 4493 test vectors.
- **Cross-layer flows** are exercised end-to-end through the `bumble-host`
  `Device` API, so integration is real rather than mocked.

## Running the test suite

The same gates CI enforces:

```bash
cargo test --workspace --all-targets
cargo test --release --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

All tests run against in-process virtual controllers, local sockets, and
captured fixtures — no Bluetooth hardware or elevated privileges required.

## Deterministic integration testing

The in-process software controller (`bumble-controller`) and virtual link make
full-stack integration tests reproducible: two or more `Device` instances
attach to controllers on a shared virtual link, and tests drive the stack
step-by-step with deterministic timers. This is the same mechanism you can use
to test your own application code against bumble-rs without hardware.

## Pandora conformance services

The `bumble-pandora` crate implements the
[Pandora test interfaces](https://github.com/google/bt-test-interfaces)
(v0.0.6 protobufs) over gRPC:

- **Host** — connectivity, advertising, scanning, connection management.
- **Security** — pairing and security-level control.
- **SecurityStorage** — bond persistence and deletion.
- **L2CAP** — connection-oriented channel operations.

Run the server:

```bash
cargo run -p bumble-pandora --bin bumble-pandora-server -- --help
```

This lets standard Pandora-based test suites (as used for the Android
Bluetooth stack) drive bumble-rs like any other implementation under test.
