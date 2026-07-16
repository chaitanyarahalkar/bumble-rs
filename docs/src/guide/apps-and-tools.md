# Apps and Tools

The workspace ships runnable command-line applications, most of them in
`bumble-transport`. Run any of them with `--help` (or no arguments) to see
its full usage. `TRANSPORT` arguments use the
[transport spec syntax](transports.md).

```bash
cargo run -p bumble-transport --bin <tool> -- <args>
```

## Discovery and inspection

### `bumble-scan`

LE scanner with RSSI filtering, PHY selection, and RPA resolution.

```text
bumble-scan [--min-rssi RSSI] [--passive] [--scan-interval MS] [--scan-window MS]
            [--phy 1m|coded] [--filter-duplicates true|false] [--raw]
            [--irk IRK_HEX:ADDRESS] [--keystore-file PATH] [--device-config PATH]
            <transport>
```

### `bumble-controller-info`

Prints controller capabilities; can optionally probe HCI command latency.

```text
bumble-controller-info [--latency-probes N] [--latency-probe-interval MS]
                       [--latency-probe-command HEX] <transport>
```

### `bumble-device-info`

Reads a peer's GATT Device Information Service.

```text
bumble-device-info [--device-config PATH] [--encrypt] <transport> [address-or-name]
```

### `bumble-gatt-dump`

Discovers and dumps a peer's complete GATT database.

```text
bumble-gatt-dump [--device-config PATH] [--encrypt] <transport> [address-or-name]
```

### `bumble-usb-probe`

Enumerates USB devices and identifies Bluetooth controllers.

```text
bumble-usb-probe [--verbose] [--hci-only] [--manufacturer NAME] [--product NAME]
```

### `bumble-show`

Offline decoder for H4 and BTSnoop capture files.

```text
bumble-show [--format h4|snoop] [--vendor android|zephyr] <filename>
```

## Pairing and security

### `bumble-pair`

Runs LE, Classic, or dual-mode pairing with configurable I/O capabilities,
bonding, CTKD, and OOB data.

```text
bumble-pair [--mode le|classic|dual] [--sc BOOL] [--mitm BOOL] [--bond BOOL]
            [--ctkd BOOL] [--advertising-address random|public]
            [--identity-address random|public] [--linger]
            [--io keyboard|display|display+keyboard|display+yes/no|none]
            [--oob HEX|-] [--prompt] [--request] [--print-keys]
            [--keystore-file PATH] [--advertise-service-uuid UUID]
            [--advertise-appearance APPEARANCE]
            <device-config> <transport> [address-or-name]
```

### `bumble-unbond`

Removes bonding keys, either directly from a keystore file or via a live
controller.

```text
bumble-unbond --keystore-file <file> [--namespace <name>] [address]
bumble-unbond --hci-transport <transport> [device-config] [address]
```

### `bumble-rpa-tool` (in `bumble-smp`)

Generates IRKs and resolvable private addresses, and verifies an RPA against
an IRK.

```text
bumble-rpa-tool gen-irk
bumble-rpa-tool gen-rpa <irk>
bumble-rpa-tool verify-rpa <irk> <rpa>
```

## Interactive

### `bumble-console`

Interactive device console over a transport.

```text
bumble-console [--device-config PATH] TRANSPORT
```

## Bridges

### `bumble-hci-bridge`

Bidirectional HCI bridge between a host-side and a controller-side transport,
with an optional list of command opcodes to answer locally instead of
forwarding.

```text
bumble-hci-bridge <host-transport-spec> <controller-transport-spec> [command-short-circuit-list]
```

### `bumble-l2cap-bridge`

Bridges an L2CAP LE credit-based channel to a TCP socket, in server or client
role.

```text
bumble-l2cap-bridge --device-config PATH --hci-transport TRANSPORT
                    [--psm PSM] [--l2cap-max-credits N] [--l2cap-mtu N] [--l2cap-mps N]
                    <server [--tcp-host HOST] [--tcp-port PORT]
                     | client BLUETOOTH-ADDRESS [--tcp-host HOST] [--tcp-port PORT]>
```

### `bumble-rfcomm-bridge`

Bridges an RFCOMM channel to a TCP socket.

```text
bumble-rfcomm-bridge [--device-config PATH] --hci-transport TRANSPORT [--trace]
                     [--channel 0..30] [--uuid UUID]
                     <server [--tcp-host HOST] [--tcp-port PORT]
                      | client BLUETOOTH-ADDRESS [--tcp-host HOST] [--tcp-port PORT]
                        [--authenticate] [--encrypt]>
```

### `bumble-gg-bridge`

"Golden Gate" bridge between a BLE peer's GattLink service and UDP sockets.

```text
bumble-gg-bridge HCI_TRANSPORT DEVICE_ADDRESS <node|PEER_ADDRESS>
                 [-sh|--send-host HOST] [-sp|--send-port PORT]
                 [-rh|--receive-host HOST] [-rp|--receive-port PORT]
```

## Controllers

### `bumble-controllers`

Links two HCI transports to a shared virtual radio — e.g. expose two virtual
controllers on PTYs for other stacks to use.

```text
bumble-controllers <hci-transport-1> <hci-transport-2>
```

### `bumble-controller-loopback`

Controller loopback throughput and round-trip test.

```text
bumble-controller-loopback [--packet-size SIZE] [--packet-count COUNT]
                           [--connection-type acl|sco] [--mode throughput|rtt]
                           [--interval MS] <transport>
```

## Audio and media

### `bumble-player`

A2DP source: discover, inquire, pair, and play an audio file to a sink.

```text
bumble-player --hci-transport TRANSPORT [--device-config PATH]
              [--authenticate] [--encrypt]
              <discover | inquire ADDRESS | pair ADDRESS
               | play [--connect ADDRESS] [-f|--audio-format auto|sbc|aac|opus] AUDIO_FILE>
```

### `bumble-speaker`

A2DP sink decoding SBC, AAC, or Opus, with optional local sound output.

```text
bumble-speaker [--codec sbc|aac|opus] [--sampling-frequency HZ] [--bitrate BPS]
               [--vbr|--no-vbr] [--discover] [--output NAME] [--ui-port PORT]
               [--connect ADDRESS_OR_NAME] [--device-config PATH] TRANSPORT
```

### `bumble-auracast`

Auracast broadcast scanner, assistant, receiver, and transmitter.

```text
bumble-auracast scan [--filter-duplicates] [--sync-timeout SECONDS] TRANSPORT
bumble-auracast assist [--broadcast-name NAME] [--source-id ID] --command COMMAND TRANSPORT ADDRESS
bumble-auracast pair TRANSPORT ADDRESS
bumble-auracast receive [OPTIONS] TRANSPORT [BROADCAST_ID]
bumble-auracast transmit [OPTIONS] TRANSPORT
```

### `bumble-lea-unicast`

LE Audio unicast source streaming an LC3 file.

```text
bumble-lea-unicast [--ui-port PORT] [--device-config PATH] TRANSPORT LC3_FILE
```

## Benchmarking

### `bumble-bench`

Multi-mode Bluetooth benchmark, compatible with upstream `apps/bench.py`.

```text
bumble-bench [OPTIONS] <central TRANSPORT [CENTRAL-OPTIONS] | peripheral TRANSPORT>

scenarios: send, receive, ping, pong
modes: gatt-client, gatt-server, l2cap-client, l2cap-server,
       rfcomm-client, rfcomm-server, iso-client, iso-server
```

## Conformance

### `bumble-pandora-server` (in `bumble-pandora`)

gRPC server exposing the Pandora Host, Security, SecurityStorage, and L2CAP
test services (default gRPC port 7999).

```text
bumble-pandora-server [--grpc-port PORT] [--rootcanal-port PORT]
                      [--transport TRANSPORT] [--config FILE]
```

See [Testing and Conformance](../reference/testing.md).
