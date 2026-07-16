# Examples

These examples are small, complete programs that exercise the stack the same
way the workspace's integration tests do. They all follow the same shape:

1. Create one or more `Device`s.
2. Attach each device to a controller — the in-process software controller
   for hardware-free examples, or an external transport for real radios.
3. Drive the devices synchronously and read typed events.

| Example | What it shows |
|---|---|
| [Two Devices on a Virtual Link](virtual-link.md) | The core pattern: advertising, scanning, and connecting entirely in-process |
| [GATT Server and Client](gatt.md) | Registering services and discovering/reading/writing them |
| [Pairing](pairing.md) | SMP pairing between two devices |
| [Using a Real Controller](real-controller.md) | Opening an external transport and running against real hardware |

For larger, runnable programs, see the command-line applications in
`bumble-transport/src/bin/` — each one is a complete worked example of the
APIs documented here ([Apps and Tools](../guide/apps-and-tools.md)).
