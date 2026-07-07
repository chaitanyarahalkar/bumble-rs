# Contributing to bumble-rs

Thanks for your interest in contributing! bumble-rs is an incremental, Rust
port of [`google/bumble`](https://github.com/google/bumble), built one
verified vertical slice at a time.

## Ground rules

By participating you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

## Getting started

```bash
git clone <this-repo>
cd bumble-rs
cargo test        # run the whole workspace
```

The workspace is a set of layered crates (`bumble` → `bumble-hci` →
`bumble-controller` / `bumble-l2cap` / `bumble-att` / `bumble-gatt` /
`bumble-crypto` / `bumble-smp` → `bumble-host`). See the [README](README.md) for
the layer map and the design specs under `docs/superpowers/`.

## The bar every change must clear

CI (and reviewers) require all of:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --workspace --release
```

- **Minimum supported Rust version is 1.87** (checked in CI). Don't use
  features newer than that without bumping the MSRV in `Cargo.toml`
  (`[workspace.package] rust-version`) and the CI job.
- Public items should carry doc comments; `cargo doc` runs with
  `-D warnings` in CI.

## The verification philosophy (please keep it)

This port's correctness rests on **ground-truth verification**, not on the code
agreeing with itself:

- **Codecs** (HCI, L2CAP, ATT, SMP, core types) are pinned to byte-exact
  outputs captured from the real Python Bumble, or to the Bluetooth Core
  Specification. When you add or change a wire format, add a test that asserts
  the serialized bytes against a value obtained from the reference — not just a
  serialize→parse round-trip (a symmetric bug passes a pure round-trip; it does
  not pass a byte-literal check).
- **Crypto** (`bumble-crypto`) is pinned to the published spec / RFC 4493 test
  vectors.
- **Cross-layer flows** are exercised end-to-end through the `bumble-host`
  `Device` API where possible, so the integration is real rather than mocked.

If a test would only prove `f(x) == f(x)`, it isn't pulling its weight — make it
compare against an independent source of truth.

## Scope and honesty

Each crate's module docs and the README are explicit about what is implemented
vs. deferred. Keep that accurate: if a slice covers a representative subset,
say so rather than implying full coverage.

## Commits and pull requests

- Keep commits focused; write a clear subject line and a body explaining the
  *why*.
- One logical change (ideally one "slice") per PR is easiest to review.
- Update `CHANGELOG.md` under "Unreleased" for user-visible changes.
- Make sure the full check list above passes locally before opening the PR.

## License

By contributing, you agree that your contributions will be licensed under the
[Apache License 2.0](LICENSE).
