# Contributing

Contributions are welcome. The full contribution guide lives in
[CONTRIBUTING.md](https://github.com/chaitanyarahalkar/bumble-rs/blob/main/CONTRIBUTING.md);
this page summarizes the essentials.

## The bar every change must clear

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --workspace --release
```

- The minimum supported Rust version is **1.87** (checked in CI). Don't use
  newer features without bumping the MSRV in `Cargo.toml` and the CI job.
- Public items must carry doc comments; `cargo doc` runs with `-D warnings`
  in CI.

## Verification philosophy

When you add or change a wire format, add a test that asserts the serialized
bytes against a value obtained from an independent reference (the Python
Bumble implementation, the Bluetooth Core Specification, or an RFC vector) —
not just a serialize→parse round-trip. A symmetric bug passes a pure
round-trip; it does not pass a byte-literal check.

See [Testing and Conformance](reference/testing.md) for the details.

## Documentation

This site is built with [mdBook](https://rust-lang.github.io/mdBook/) from the
`docs/` directory and deployed to GitHub Pages on every push to `main`,
together with the rustdoc API reference.

To preview locally:

```bash
mdbook serve docs --open
```

## Community

By participating you agree to abide by the
[Code of Conduct](https://github.com/chaitanyarahalkar/bumble-rs/blob/main/CODE_OF_CONDUCT.md).
Report security issues according to
[SECURITY.md](https://github.com/chaitanyarahalkar/bumble-rs/blob/main/SECURITY.md).

## License

By contributing, you agree that your contributions will be licensed under the
[Apache License 2.0](https://github.com/chaitanyarahalkar/bumble-rs/blob/main/LICENSE).
