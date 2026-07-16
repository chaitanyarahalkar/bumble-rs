# Getting Started

## Requirements

- **Rust 1.87 or newer** (the workspace MSRV, checked in CI).
- A C toolchain for native dependencies.
- Appropriate system permissions when accessing USB controllers, raw HCI
  sockets, or Linux VHCI.
- Platform audio development libraries only when enabling the optional
  `sound-device` feature.

## Build the workspace

```bash
git clone https://github.com/chaitanyarahalkar/bumble-rs.git
cd bumble-rs
cargo build --workspace --all-targets
```

## Run the tests

```bash
cargo test --workspace --all-targets
```

The repository contains more than 1,000 tests across unit, integration,
transport, application, and conformance targets. They run entirely against
in-process virtual controllers and local sockets — no Bluetooth hardware is
needed.

## Try a tool

The `bumble-transport` crate ships runnable command-line applications. With a
Bluetooth controller attached (see [Transports](transports.md) for the spec
syntax):

```bash
# Scan for nearby devices using a USB controller
cargo run -p bumble-transport --bin bumble-scan -- usb:0

# Inspect what a controller supports
cargo run -p bumble-transport --bin bumble-controller-info -- usb:0
```

Every application accepts `--help`:

```bash
cargo run -p bumble-transport --bin bumble-scan -- --help
```

See [Apps and Tools](apps-and-tools.md) for the complete catalog.

## Use the crates in your project

Add the crates you need to your `Cargo.toml`. The high-level entry point is
`bumble-host`, which provides the `Device` API; `bumble-controller` provides
the in-process software controller; `bumble-transport` opens real controllers.

```toml
[dependencies]
bumble = { git = "https://github.com/chaitanyarahalkar/bumble-rs" }
bumble-host = { git = "https://github.com/chaitanyarahalkar/bumble-rs" }
bumble-controller = { git = "https://github.com/chaitanyarahalkar/bumble-rs" }
```

Then head to the [examples](../examples/overview.md) for minimal end-to-end
programs.

## Local API documentation

```bash
cargo doc --workspace --no-deps --open
```

The same documentation is published online — see
[API Documentation](../reference/api.md).
