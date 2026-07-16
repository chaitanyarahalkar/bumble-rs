# API Documentation

The complete rustdoc API reference for every crate in the workspace is
published alongside this guide:

**[Open the API reference](https://chaitanyarahalkar.github.io/bumble-rs/api/)**

Direct links to the most commonly used crates:

| Crate | API docs |
|---|---|
| `bumble` | [bumble](https://chaitanyarahalkar.github.io/bumble-rs/api/bumble/) |
| `bumble-host` | [bumble_host](https://chaitanyarahalkar.github.io/bumble-rs/api/bumble_host/) |
| `bumble-controller` | [bumble_controller](https://chaitanyarahalkar.github.io/bumble-rs/api/bumble_controller/) |
| `bumble-hci` | [bumble_hci](https://chaitanyarahalkar.github.io/bumble-rs/api/bumble_hci/) |
| `bumble-gatt` | [bumble_gatt](https://chaitanyarahalkar.github.io/bumble-rs/api/bumble_gatt/) |
| `bumble-l2cap` | [bumble_l2cap](https://chaitanyarahalkar.github.io/bumble-rs/api/bumble_l2cap/) |
| `bumble-smp` | [bumble_smp](https://chaitanyarahalkar.github.io/bumble-rs/api/bumble_smp/) |
| `bumble-transport` | [bumble_transport](https://chaitanyarahalkar.github.io/bumble-rs/api/bumble_transport/) |
| `bumble-profiles` | [bumble_profiles](https://chaitanyarahalkar.github.io/bumble-rs/api/bumble_profiles/) |

To build the same documentation locally:

```bash
cargo doc --workspace --no-deps --open
```

Rustdoc runs with `-D warnings` in CI, so every public item carries
documentation.
