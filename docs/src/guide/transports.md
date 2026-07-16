# Transports

Applications reach external controllers through Bumble-style transport
specifications:

```text
<scheme>:[metadata]parameters
```

The string is split on the first `:` into a scheme and parameters. An
optional leading `[key=value,...]` metadata section carries hints such as a
forced [driver](drivers.md) selection, e.g. `usb:[driver=rtk]0`.

In code, `bumble_transport::open_transport(spec)` returns an opened
transport, and `open_split_transport(spec)` returns separate packet source
and sink halves (what `ExternalHost` consumes — see
[Using a Real Controller](../examples/real-controller.md)). Server-style
schemes block until the first client connects.

## Scheme reference

### `usb` — USB controllers

```text
usb:<selector>[+sco=<alt>][!]
```

The selector is one of:

| Form | Meaning |
|---|---|
| `usb:0` | Nth Bluetooth controller found (index) |
| `usb:0BDA:8771` | By vendor/product ID (hex) |
| `usb:0BDA:8771/ABC123` | By VID:PID and serial number |
| `usb:0BDA:8771#1` | By VID:PID, second occurrence |
| `usb:1-3.2` | By bus and port path |

Suffixes: `!` forces the device even if it doesn't advertise the Bluetooth
USB class triple; `+sco=<alt>` selects the isochronous alternate setting used
for SCO audio. `pyusb:` is accepted as an alias. After opening, the
transport publishes `vendor_id`/`product_id`/`bus`/`address` metadata, which
drives automatic vendor-driver matching.

### `serial` — UART controllers

```text
serial:<device>[,<baud>][,rtscts][,dsrdtr][,delay]
```

- The first field is the device path (required).
- A bare integer field sets the baud rate (default **1,000,000**).
- `rtscts` enables hardware flow control.
- `dsrdtr` asserts DTR and gates writes on DSR.
- `delay` waits 500 ms after opening before use.

```text
serial:/dev/tty.usbmodem0001,1000000,rtscts
```

### `tcp-client`, `tcp-server`, `udp`

```text
tcp-client:<host>:<port>
tcp-server:<host>:<port>      # `_` as host binds 0.0.0.0
udp:<local>,<remote>
```

### `unix`, `unix-client`, `unix-server` (Unix only)

```text
unix:<socket-path>            # alias: unix-client
unix-server:<socket-path>
```

### `ws-client`, `ws-server` — WebSocket

```text
ws-client:<url>               # e.g. ws-client:ws://127.0.0.1:8080
ws-server:<host>:<port>       # `_` as host binds 0.0.0.0
```

### `file`

```text
file:<path>
```

Reads/writes H4 packets from an arbitrary file-like path (e.g. a character
device).

### `pty` (Unix only)

```text
pty[:<symlink>]
```

Opens a pseudo-terminal and optionally creates a convenience symlink to the
peer end — useful for connecting other tools that expect a serial device.

### `hci-socket` (Linux only)

```text
hci-socket[:<adapter-index>]
```

Opens a raw HCI user channel on the given adapter. Requires
`CAP_NET_ADMIN` and the adapter to be down.

### `vhci` (Linux only)

```text
vhci[:<device-path>]          # default /dev/vhci
```

Attaches a virtual controller to the Linux kernel, so BlueZ can drive a
bumble-rs software controller.

### `android-emulator`

```text
android-emulator[:<address>][,mode=host|controller]
```

Connects to the Bluetooth chip of a running Android emulator over gRPC
(default `localhost:8554`, default mode `host`).

### `android-netsim`

```text
android-netsim[:<host>:<port>][,mode=host|controller][,instance=N][,name=NAME][,variant=VARIANT]
```

Connects to the Android `netsim` network simulator. In `host` mode the gRPC
port can be auto-discovered from the local netsim configuration.

## Packet capture

The `snoop` module records traffic in standard formats, wrapping any
transport with `SnoopingTransport`:

```text
btsnoop:file:<path>           # BTSnoop file
pcapsnoop:file:<path>         # pcap (DLT 201, H4 with pseudo-header)
pcapsnoop:pipe:<path>         # pcap to an existing pipe
```

A `{pid}` placeholder in the path is replaced by the process id. Captures
(and plain H4 dumps) can be decoded offline with the `bumble-show` tool.

## H4 framing

All transports carry complete H4 packets. `PacketFramer` reassembles
fragmented or coalesced byte streams, and `H4Transport` adapts any blocking
`Read + Write` pair into a packet transport — so custom transports are easy
to add on top of anything that moves bytes.
