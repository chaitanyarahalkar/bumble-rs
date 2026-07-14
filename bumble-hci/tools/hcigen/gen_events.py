#!/usr/bin/env python3
"""Generate bumble-hci event.rs + full codes.rs (command opcodes + event/subevent
codes) + oracle tests, from the introspected upstream spec. Run AFTER
gen_commands.py (this regenerates the complete codes.rs). Self-checks every
generated class's replayed wire bytes against upstream's captured body_hex."""
import os, json, struct, sys

BASE=os.environ.get("HCIGEN_OUT", os.path.dirname(os.path.abspath(__file__)))+"/"
DST=os.path.abspath(os.path.dirname(os.path.abspath(__file__))+"/../..")+"/"
full=json.load(open(BASE+"spec.json"))
cspec=full["commands"]; espec=full["events"]

RESERVED={"type","ref","match","move","box","fn","let","mut","use","mod","loop","impl",
          "in","as","dyn","self","crate","super","where","async","await","yield","gen","try"}
# Events kept hand-written (dataclass reports; ReturnParameters embedding; base container).
SKIP={"HCI_LE_Advertising_Report_Event","HCI_LE_Extended_Advertising_Report_Event",
      "HCI_Command_Complete_Event","HCI_LE_Meta_Event"}

def fname(n): return n+"_" if n in RESERVED else n
def const(cls): return cls.upper()
def reg_variant(cls):
    toks=cls[len("HCI_"):-len("_Event")].split("_")
    return "".join(t[:1].upper()+t[1:].lower() for t in toks if t)
def meta_variant(cls):
    toks=cls[len("HCI_LE_"):-len("_Event")].split("_")
    return "".join(t[:1].upper()+t[1:].lower() for t in toks if t)
def rust_type(c):
    if c in ("u8","u16","u32","i8","i16"): return c
    if c=="u24": return "u32"
    if c in ("u16be",): return "u16"
    if c in ("u32be",): return "u32"
    if c.startswith("bytes:"): return f"[u8; {c.split(':')[1]}]"
    if c=="addr": return "Address"
    if c=="codingformat": return "CodingFormat"
    if c in ("rest","varbytes","advdata"): return "Vec<u8>"
    raise SystemExit("rust_type? "+c)

class Ctr:
    def __init__(s): s.c=0
    def nb(s,n):
        o=bytes(((s.c+i)&0xFF) for i in range(1,n+1)); s.c=(s.c+n)&0xFF; return o
def val(c, ctr, public_address=False):
    if c=="u8": b=ctr.nb(1); return str(b[0]), bytes(b)
    if c=="i8": return "5", bytes([5])
    if c in ("u16","u16be"):
        b=ctr.nb(2); v=int.from_bytes(b,'little'); return str(v),(v.to_bytes(2,'big') if c=="u16be" else bytes(b))
    if c=="i16": return "9", struct.pack('<h',9)
    if c=="u24": b=ctr.nb(3); return str(int.from_bytes(b,'little')), bytes(b)
    if c in ("u32","u32be"):
        b=ctr.nb(4); v=int.from_bytes(b,'little'); return str(v),(v.to_bytes(4,'big') if c=="u32be" else bytes(b))
    if c.startswith("bytes:"):
        n=int(c.split(':')[1]); b=ctr.nb(n); return "["+", ".join(map(str,b))+"]", bytes(b)
    if c=="addr":
        b=ctr.nb(6); address_type="PUBLIC_DEVICE" if public_address else "RANDOM_DEVICE"
        return "Address::from_bytes(["+", ".join(map(str,b))+f"], AddressType::{address_type})", bytes(b)
    if c=="codingformat":
        return "CodingFormat { coding_format: 2, company_id: 0, vendor_specific_codec_id: 0 }", bytes([2,0,0,0,0])
    if c=="rest": b=ctr.nb(4); return "vec!["+", ".join(map(str,b))+"]", bytes(b)
    if c in ("varbytes","advdata"):
        b=ctr.nb(3); return "vec!["+", ".join(map(str,b))+"]", bytes([len(b)])+bytes(b)
    raise SystemExit("val? "+c)

def ser_top(c, n):
    if c=="u8": return f"p.push(*{n});"
    if c=="i8": return f"p.push(*{n} as u8);"
    if c=="u16": return f"p.extend_from_slice(&{n}.to_le_bytes());"
    if c=="u16be": return f"p.extend_from_slice(&{n}.to_be_bytes());"
    if c=="i16": return f"p.extend_from_slice(&({n}).to_le_bytes());"
    if c=="u24": return f"p.extend_from_slice(&{n}.to_le_bytes()[..3]);"
    if c=="u32": return f"p.extend_from_slice(&{n}.to_le_bytes());"
    if c.startswith("bytes:"): return f"p.extend_from_slice({n});"
    if c=="addr": return f"p.extend_from_slice({n}.address_bytes());"
    if c=="codingformat": return f"p.extend_from_slice(&{n}.to_bytes());"
    if c=="rest": return f"p.extend_from_slice({n});"
    if c=="varbytes": return f"p.push({n}.len() as u8);\n                p.extend_from_slice({n});"
    raise SystemExit("ser_top? "+c)
def ser_elem(c, e):
    if c=="u8": return f"p.push({e});"
    if c=="i8": return f"p.push({e} as u8);"
    if c=="u16": return f"p.extend_from_slice(&{e}.to_le_bytes());"
    if c=="u24": return f"p.extend_from_slice(&{e}.to_le_bytes()[..3]);"
    if c=="u32": return f"p.extend_from_slice(&{e}.to_le_bytes());"
    if c=="i8": return f"p.push({e} as u8);"
    if c.startswith("bytes:"): return f"p.extend_from_slice(&{e});"
    if c=="addr": return f"p.extend_from_slice({e}.address_bytes());"
    if c=="varbytes": return f"p.push({e}.len() as u8);\n                    p.extend_from_slice(&{e});"
    raise SystemExit("ser_elem? "+c)
def parse_scalar(c, name=None):
    if c=="u8": return "r.u8()?"
    if c=="i8": return "r.u8()? as i8"
    if c=="u16": return "r.u16_le()?"
    if c=="u16be": return "u16::from_be_bytes(r.array::<2>()?)"
    if c=="i16": return "r.u16_le()? as i16"
    if c=="u24": return "r.u24_le()?"
    if c=="u32": return "r.u32_le()?"
    if c.startswith("bytes:"): return f"r.array::<{c.split(':')[1]}>()?"
    if c=="addr": return "addr(&mut r)?"
    if c=="codingformat": return "CodingFormat::read(&mut r)?"
    if c=="rest": return "r.rest().to_vec()"
    if c=="varbytes": return "{ let n = r.u8()? as usize; r.take(n)?.to_vec() }"
    raise SystemExit("parse? "+c)

def build(cls, e, meta):
    vn = meta_variant(cls) if meta else reg_variant(cls)
    ctr=Ctr(); expect=bytearray()
    decls=[]; binds=[]; ser=[]; parse=[]; tvals=[]; ai=0
    for fd in e["fields"]:
        if "array" in fd:
            subs=fd["array"]; names=[fname(s["name"]) for s in subs]; cnt=f"count{ai}"; ai+=1
            for s,nm in zip(subs,names): decls.append(f"        {nm}: Vec<{rust_type(s['codec'])}>,")
            binds+=names; first=names[0]
            body="\n".join("                    "+ser_elem(s["codec"], f"{fname(s['name'])}[i]") for s in subs)
            ser.append(f"                p.push({first}.len() as u8);\n                for i in 0..{first}.len() {{\n{body}\n                }}")
            inits="\n".join(f"                let mut {nm} = Vec::with_capacity({cnt});" for nm in names)
            pushes="\n".join(f"                    {nm}.push({parse_scalar(s['codec'], s['name'])});" for s,nm in zip(subs,names))
            parse.append(f"                let {cnt} = r.u8()? as usize;\n{inits}\n                for _ in 0..{cnt} {{\n{pushes}\n                }}")
            expect+=bytes([1])
            for s in subs:
                lit,wb=val(s["codec"],ctr,public_address=s["name"]=="bd_addr"); expect+=wb; tvals.append(f"            {fname(s['name'])}: vec![{lit}],")
        else:
            nm=fname(fd["name"]); c=fd["codec"]
            decls.append(f"        {nm}: {rust_type(c)},"); binds.append(nm)
            ser.append("                "+ser_top(c,nm)); parse.append(f"                let {nm} = {parse_scalar(c, fd['name'])};")
            lit,wb=val(c,ctr,public_address=fd["name"]=="bd_addr"); expect+=wb; tvals.append(f"            {nm}: {lit},")
    return dict(cls=cls,vn=vn,cn=const(cls),code=e["code"],sub=e.get("subevent_code"),
                decls=decls,binds=binds,ser="\n".join(ser),parse="\n".join(parse),tvals=tvals,
                expect=bytes(expect),body=bytes.fromhex(e["body_hex"]),noparam=(len(e["fields"])==0),meta=meta)

reg={}; meta={}; by_code={}; by_sub={}; skipped=[]
for cls,e in sorted(espec.items(), key=lambda kv:(kv[1]["code"], kv[1].get("subevent_code") or 0, kv[0])):
    if cls in SKIP: continue
    is_meta = e.get("subevent_code") is not None and e["code"]==0x3E
    b=build(cls,e,is_meta)
    if b["expect"]!=b["body"]:
        print(f"SELF-CHECK FAIL {cls}: replay={b['expect'].hex()} != oracle={b['body'].hex()}"); sys.exit(1)
    if is_meta:
        if e["subevent_code"] in by_sub: skipped.append((cls,"dup sub")); continue
        if b["vn"] in {x["vn"] for x in meta.values()}: skipped.append((cls,"dup meta variant "+b["vn"])); continue
        meta[cls]=b; by_sub[e["subevent_code"]]=cls
    else:
        if e["code"] in by_code: skipped.append((cls,"dup code")); continue
        if b["vn"] in {x["vn"] for x in reg.values()}: skipped.append((cls,"dup reg variant "+b["vn"])); continue
        reg[cls]=b; by_code[e["code"]]=cls
print(f"events: regular={len(reg)} meta={len(meta)} self-check OK skipped {len(skipped)}: {skipped}")

# ============ regenerate FULL codes.rs (commands + events + subevents) ============
def cmd_variant(cls):
    toks=cls[len("HCI_"):-len("_Command")].split("_"); return "".join(t[:1].upper()+t[1:].lower() for t in toks if t)
cmd_codes={cls.upper():e["code"] for cls,e in cspec.items()
           if cls not in ("HCI_LE_Read_All_Local_Supported_Features_Command",)}
cmd_codes["HCI_LE_SET_EXTENDED_SCAN_PARAMETERS_COMMAND"]=0x2041
cmd_codes["HCI_LE_EXTENDED_CREATE_CONNECTION_COMMAND"]=0x2043
# event/subevent codes (generated ones + the hand-written specials we skipped)
evt_codes={}   # name -> (value, is_u8)
for cls,b in reg.items(): evt_codes[b["cn"]]=b["code"]
for cls,b in meta.items(): evt_codes[b["cn"]]=b["sub"]
# specials kept hand-written still need their consts:
evt_codes["HCI_COMMAND_COMPLETE_EVENT"]=0x0E
evt_codes["HCI_LE_ADVERTISING_REPORT_EVENT"]=0x02
evt_codes["HCI_LE_EXTENDED_ADVERTISING_REPORT_EVENT"]=0x0D
evt_codes["HCI_LE_META_EVENT"]=0x3E  # container

C=['''//! HCI constants: packet type indicators, op codes, event/sub-event codes, and
//! status. GENERATED from upstream `bumble.hci` by `tools/hcigen` — do not edit
//! by hand; re-run the generator instead.

// Packet type indicators (the first byte of every HCI packet).
pub const HCI_COMMAND_PACKET: u8 = 0x01;
pub const HCI_ACL_DATA_PACKET: u8 = 0x02;
pub const HCI_SYNCHRONOUS_DATA_PACKET: u8 = 0x03;
pub const HCI_EVENT_PACKET: u8 = 0x04;
pub const HCI_ISO_DATA_PACKET: u8 = 0x05;

// ---- Command op codes (OGF << 10 | OCF) ----''']
for cn,code in sorted(cmd_codes.items(), key=lambda kv:(kv[1],kv[0])):
    C.append(f"pub const {cn}: u16 = 0x{code:04X};")
C.append("\n// ---- Event codes / LE Meta sub-event codes ----")
for cn,code in sorted(evt_codes.items(), key=lambda kv:(kv[1],kv[0])):
    C.append(f"pub const {cn}: u8 = 0x{code:02X};")
C.append('''
// Status.
pub const HCI_SUCCESS: u8 = 0x00;

/// Decompose an op code into (OGF, OCF).
pub fn ogf_ocf(op_code: u16) -> (u8, u16) {
    ((op_code >> 10) as u8, op_code & 0x03FF)
}

/// Compose an op code from OGF and OCF.
pub fn op_code(ogf: u8, ocf: u16) -> u16 {
    ((ogf as u16) << 10) | (ocf & 0x03FF)
}
''')
open(DST+"src/codes.rs","w").write("\n".join(C))
print(f"wrote codes.rs ({len(cmd_codes)} opcodes, {len(evt_codes)} event/subevent codes)")

# ============ emit event.rs ============
# embedded (hand-written) pieces preserved verbatim from the original event.rs
EMB=json.load(open(os.path.dirname(os.path.abspath(__file__))+"/event_embed.json"))
out=[EMB["head"]]
# --- Event enum ---
out.append("#[allow(clippy::large_enum_variant, clippy::enum_variant_names)]")
out.append("#[derive(Clone, Debug, PartialEq, Eq)]")
out.append("pub enum Event {")
for cls,b in reg.items():
    if b["noparam"]: out.append(f"    {b['vn']},")
    else:
        out.append(f"    {b['vn']} {{"); out+=b["decls"]; out.append("    },")
out.append(EMB["event_variants"])
out.append("}\n")
out.append(EMB["structs"])
# --- LeMetaEvent enum ---
out.append("#[allow(clippy::large_enum_variant, clippy::enum_variant_names)]")
out.append("#[derive(Clone, Debug, PartialEq, Eq)]")
out.append("pub enum LeMetaEvent {")
for cls,b in meta.items():
    if b["noparam"]: out.append(f"    {b['vn']},")
    else:
        out.append(f"    {b['vn']} {{"); out+=b["decls"]; out.append("    },")
out.append(EMB["meta_variants"])
out.append("}\n")
# --- impl Event ---
out.append("impl Event {")
out.append("    /// The 8-bit event code.\n    pub fn event_code(&self) -> u8 {\n        match self {")
for cls,b in reg.items():
    pat="" if b["noparam"] else " { .. }"
    out.append(f"            Event::{b['vn']}{pat} => {b['cn']},")
out.append(EMB["event_code_arms"])
out.append("        }\n    }\n")
out.append("    /// The serialized event parameters (without the packet/event-code header).\n    #[allow(clippy::needless_range_loop, clippy::vec_init_then_push)]\n    pub fn parameters(&self) -> Vec<u8> {\n        let mut p = Vec::new();\n        match self {")
noparam_reg=[b for b in reg.values() if b["noparam"]]
if noparam_reg:
    out.append("            "+" \n            | ".join(f"Event::{b['vn']}" for b in noparam_reg)+" => {}")
for cls,b in reg.items():
    if b["noparam"]: continue
    out.append(f"            Event::{b['vn']} {{")
    for nm in b["binds"]: out.append(f"                {nm},")
    out.append("            } => {")
    out.append(b["ser"]); out.append("            }")
out.append(EMB["event_params_arms"])
out.append("        }\n        p\n    }\n")
out.append(EMB["event_tail"])   # to_bytes, from_bytes, from_code_and_parameters head
# from_code_and_parameters generated regular arms:
fc=[]
for cls,b in reg.items():
    if b["noparam"]:
        fc.append(f"            {b['cn']} => Event::{b['vn']},")
    else:
        fc.append(f"            {b['cn']} => {{")
        fc.append(b["parse"])
        fc.append(f"                Event::{b['vn']} {{")
        for nm in b["binds"]: fc.append(f"                    {nm},")
        fc.append("                }")
        fc.append("            }")
out.append("\n".join(fc))
out.append(EMB["from_code_tail"])   # CommandComplete arm + Generic fallback + close
# --- impl LeMetaEvent ---
out.append("impl LeMetaEvent {")
out.append("    /// The LE sub-event code.\n    pub fn subevent_code(&self) -> u8 {\n        match self {")
for cls,b in meta.items():
    pat="" if b["noparam"] else " { .. }"
    out.append(f"            LeMetaEvent::{b['vn']}{pat} => {b['cn']},")
out.append(EMB["meta_subcode_arms"])
out.append("        }\n    }\n")
out.append("    /// Full LE-meta parameters: sub-event code byte followed by the fields.\n    #[allow(clippy::needless_range_loop)]\n    pub fn parameters(&self) -> Vec<u8> {\n        let mut p = vec![self.subevent_code()];\n        match self {")
noparam_meta=[b for b in meta.values() if b["noparam"]]
if noparam_meta:
    out.append("            "+" \n            | ".join(f"LeMetaEvent::{b['vn']}" for b in noparam_meta)+" => {}")
for cls,b in meta.items():
    if b["noparam"]: continue
    out.append(f"            LeMetaEvent::{b['vn']} {{")
    for nm in b["binds"]: out.append(f"                {nm},")
    out.append("            } => {")
    out.append(b["ser"]); out.append("            }")
out.append(EMB["meta_params_arms"])
out.append("        }\n        p\n    }\n")
out.append(EMB["from_subevent_head"])
fs=[]
for cls,b in meta.items():
    if b["noparam"]:
        fs.append(f"            {b['cn']} => LeMetaEvent::{b['vn']},")
    else:
        fs.append(f"            {b['cn']} => {{")
        fs.append(b["parse"])
        fs.append(f"                LeMetaEvent::{b['vn']} {{")
        for nm in b["binds"]: fs.append(f"                    {nm},")
        fs.append("                }")
        fs.append("            }")
out.append("\n".join(fs))
out.append(EMB["from_subevent_tail"])
open(DST+"src/event.rs","w").write("\n".join(out))
print(f"wrote event.rs")

# ============ emit tests/generated_events.rs ============
t=['''//! GENERATED oracle-pinned tests: every typed HCI event/LE-meta sub-event
//! round-trips byte-exact against packet bytes captured from real Python Bumble.
#![allow(clippy::redundant_clone)]

use bumble_hci::{Event, HciPacket, LeMetaEvent};
use bumble::{Address, AddressType};

fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }
fn check(ev: Event, expected: &str) {
    let packet = HciPacket::Event(ev);
    let bytes = packet.to_bytes();
    assert_eq!(hex(&bytes), expected, "serialize mismatch");
    assert_eq!(HciPacket::from_bytes(&bytes).expect("parse"), packet, "round-trip mismatch");
}
''']
for cls,b in reg.items():
    body=b["body"]; full_pkt=bytes([4, b["code"], len(body)])+body
    fn="evt_"+b["vn"].lower()
    if b["noparam"]:
        t.append(f'#[test]\nfn {fn}() {{ check(Event::{b["vn"]}, "{full_pkt.hex()}"); }}\n')
    else:
        t.append(f'#[test]\nfn {fn}() {{\n    check(Event::{b["vn"]} {{')
        t+=b["tvals"]; t.append(f'    }}, "{full_pkt.hex()}");\n}}\n')
for cls,b in meta.items():
    body=b["body"]; inner=bytes([b["sub"]])+body; full_pkt=bytes([4,0x3E,len(inner)])+inner
    fn="meta_"+b["vn"].lower()
    if b["noparam"]:
        t.append(f'#[test]\nfn {fn}() {{ check(Event::LeMeta(LeMetaEvent::{b["vn"]}), "{full_pkt.hex()}"); }}\n')
    else:
        t.append(f'#[test]\nfn {fn}() {{\n    check(Event::LeMeta(LeMetaEvent::{b["vn"]} {{')
        t+=b["tvals"]; t.append(f'    }}), "{full_pkt.hex()}");\n}}\n')
open(DST+"tests/generated_events.rs","w").write("\n".join(t))
print(f"wrote tests/generated_events.rs ({len(reg)+len(meta)} tests)")
