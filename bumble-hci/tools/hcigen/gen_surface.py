import os, re, json, bumble.hci as h
BASE=os.environ.get("HCIGEN_OUT")+"/"
DST=os.path.abspath(os.path.dirname(os.path.abspath(__file__))+"/../../../bumble-controller")+"/"
src=open(os.environ.get("BUMBLE_SRC","/tmp/bumble-scope")+"/bumble/controller.py").read()
pat=re.compile(r"def on_hci_(\w+)_command\(\s*self,[^)]*\)\s*->\s*([^\:]+):", re.S)
cats={}
for m in pat.finditer(src):
    name=m.group(1); ann=m.group(2)
    if "None" in ann: cats[name]="Status"
    elif "StatusReturnParameters" in ann: cats[name]="StatusOnly"
    else: cats[name]="Data"

# map handler name -> op_code via real command classes
op_of={}
for nm in dir(h):
    o=getattr(h,nm)
    if isinstance(o,type) and nm.startswith("HCI_") and nm.endswith("_Command") and hasattr(o,"op_code"):
        handler=nm[len("HCI_"):-len("_Command")].lower()
        op_of[handler]=o.op_code

rows=[]; missing=[]
for name,cat in sorted(cats.items()):
    if name in op_of: rows.append((op_of[name], name, cat))
    else: missing.append(name)
rows.sort()

out=['''//! GENERATED (tools/hcigen/gen_surface.py): the HCI command surface that
//! upstream `controller.py` implements, with each command's HCI response shape.
//! Used by the software controller to give every command a well-formed reply
//! that matches upstream's behavior, instead of a blanket "Unknown Command".
//!
//! - `StatusOnly`: config/set commands upstream accepts and returns
//!   Command Complete + status SUCCESS for. Functionally modeled state is
//!   retained; no-op handlers match upstream no-ops.
//! - `Data`: commands that return read or command-specific data. The controller
//!   provides every entry's upstream state/default payload (see
//!   `handle_command`).
//! - `Status`: commands that start an operation and complete via a later event
//!   (Command Status). Functionally simulated where the in-process link allows
//!   (e.g. connect/disconnect); otherwise acknowledged with Command Status.

/// The HCI response shape a command produces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resp {
    /// Command Complete with a status-only return parameter.
    StatusOnly,
    /// Command Complete carrying read or command-specific data.
    Data,
    /// Command Status (an operation that completes via a later event).
    Status,
}

/// (op_code, response shape) for every command upstream `controller.py` handles.
pub static COMMAND_SURFACE: &[(u16, Resp)] = &[''']
for op,name,cat in rows:
    out.append(f"    (0x{op:04X}, Resp::{cat}), // {name}")
# add the two phys-derived custom commands
out.append("    (0x2041, Resp::StatusOnly), // le_set_extended_scan_parameters")
out.append("    (0x2043, Resp::Status), // le_extended_create_connection")
out.append('''];

/// The response shape upstream's controller uses for `op_code`, if it handles it.
pub fn response_kind(op_code: u16) -> Option<Resp> {
    COMMAND_SURFACE
        .iter()
        .find(|(o, _)| *o == op_code)
        .map(|(_, r)| *r)
}
''')
open(DST+"src/command_surface.rs","w").write("\n".join(out))
print(f"wrote command_surface.rs: {len(rows)+2} commands; unmapped handlers: {missing}")
