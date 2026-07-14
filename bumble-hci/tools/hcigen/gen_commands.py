#!/usr/bin/env python3
"""Generate bumble-hci command.rs + command opcodes (codes.rs) + oracle tests
from the introspected upstream spec. Self-checks every class's replayed wire
bytes against the upstream-captured body_hex before emitting a line of Rust."""
import os, json, struct, sys

BASE=os.environ.get("HCIGEN_OUT", os.path.dirname(os.path.abspath(__file__)))+"/"
DST=os.path.abspath(os.path.dirname(os.path.abspath(__file__))+"/../..")+"/"
spec=json.load(open(BASE+"spec.json"))["commands"]

RESERVED={"type","ref","match","move","box","fn","let","mut","use","mod","loop","impl",
          "in","as","dyn","self","crate","super","where","async","await","yield","gen","try","fn_"}
EMBED={"HCI_LE_Set_Extended_Scan_Parameters_Command","HCI_LE_Extended_Create_Connection_Command"}
SKIP=set()
ADV={"HCI_LE_Set_Advertising_Data_Command","HCI_LE_Set_Scan_Response_Data_Command"}

def variant(cls):
    toks=cls[len("HCI_"):-len("_Command")].split("_")
    return "".join(t[:1].upper()+t[1:].lower() for t in toks if t)
def const(cls): return cls.upper()
def fname(n): return n+"_" if n in RESERVED else n
def rust_type(c):
    if c in ("u8","u16","u32","i8","i16"): return c
    if c=="u24": return "u32"
    if c=="u16be": return "u16"
    if c=="u32be": return "u32"
    if c.startswith("bytes:"): return f"[u8; {c.split(':')[1]}]"
    if c=="addr": return "Address"
    if c=="codingformat": return "CodingFormat"
    if c in ("rest","varbytes","advdata"): return "Vec<u8>"
    raise SystemExit("rust_type? "+c)

# ---- value replay + expected bytes (mirrors extract.py) ----
class Ctr:
    def __init__(s): s.c=0
    def nb(s,n):
        o=bytes(((s.c+i)&0xFF) for i in range(1,n+1)); s.c=(s.c+n)&0xFF; return o
def val(c, ctr, adv=False, public_address=False):
    if c=="u8": b=ctr.nb(1); return str(b[0]), bytes(b)
    if c=="i8": return "5", bytes([5])
    if c in ("u16","u16be"):
        b=ctr.nb(2); v=int.from_bytes(b,'little'); return str(v), (v.to_bytes(2,'big') if c=="u16be" else bytes(b))
    if c=="i16": return "9", struct.pack('<h',9)
    if c=="u24": b=ctr.nb(3); return str(int.from_bytes(b,'little')), bytes(b)
    if c in ("u32","u32be"):
        b=ctr.nb(4); v=int.from_bytes(b,'little'); return str(v), (v.to_bytes(4,'big') if c=="u32be" else bytes(b))
    if c.startswith("bytes:"):
        n=int(c.split(':')[1]); b=ctr.nb(n); return "["+", ".join(map(str,b))+"]", bytes(b)
    if c=="addr":
        b=ctr.nb(6); address_type="PUBLIC_DEVICE" if public_address else "RANDOM_DEVICE"
        return "Address::from_bytes(["+", ".join(map(str,b))+f"], AddressType::{address_type})", bytes(b)
    if c=="codingformat":
        return "CodingFormat { coding_format: 2, company_id: 0, vendor_specific_codec_id: 0 }", bytes([2,0,0,0,0])
    if c=="rest": b=ctr.nb(4); return "vec!["+", ".join(map(str,b))+"]", bytes(b)
    if c in ("varbytes","advdata"):
        b=ctr.nb(3); w=bytes([len(b)])+bytes(b)
        if c=="advdata": w=w+bytes(31-len(b))
        return "vec!["+", ".join(map(str,b))+"]", w
    raise SystemExit("val? "+c)

# ---- serialize (top-level binding is &T) ----
def ser_top(c, n):
    if c=="u8": return f"p.push(*{n});"
    if c=="i8": return f"p.push(*{n} as u8);"
    if c=="u16": return f"push_u16(&mut p, *{n});"
    if c=="u16be": return f"p.extend_from_slice(&{n}.to_be_bytes());"
    if c=="i16": return f"push_u16(&mut p, *{n} as u16);"
    if c=="u24": return f"push_u24(&mut p, *{n});"
    if c=="u32": return f"p.extend_from_slice(&{n}.to_le_bytes());"
    if c=="u32be": return f"p.extend_from_slice(&{n}.to_be_bytes());"
    if c.startswith("bytes:"): return f"p.extend_from_slice({n});"
    if c=="addr": return f"p.extend_from_slice({n}.address_bytes());"
    if c=="codingformat": return f"p.extend_from_slice(&{n}.to_bytes());"
    if c=="rest": return f"p.extend_from_slice({n});"
    if c=="varbytes": return f"p.push({n}.len() as u8);\n                p.extend_from_slice({n});"
    if c=="advdata": return f"p.push({n}.len() as u8);\n                p.extend_from_slice({n});\n                p.resize(1 + 31, 0);"
    raise SystemExit("ser_top? "+c)
# ---- serialize array element (place expr like name[i]) ----
def ser_elem(c, e):
    if c=="u8": return f"p.push({e});"
    if c=="i8": return f"p.push({e} as u8);"
    if c=="u16": return f"push_u16(&mut p, {e});"
    if c=="i16": return f"push_u16(&mut p, {e} as u16);"
    if c=="u24": return f"push_u24(&mut p, {e});"
    if c=="u32": return f"p.extend_from_slice(&{e}.to_le_bytes());"
    if c.startswith("bytes:"): return f"p.extend_from_slice(&{e});"
    if c=="addr": return f"p.extend_from_slice({e}.address_bytes());"
    if c=="codingformat": return f"p.extend_from_slice(&{e}.to_bytes());"
    raise SystemExit("ser_elem? "+c)
# ---- parse scalar ----
def parse_scalar(c, name=None):
    if c=="u8": return "r.u8()?"
    if c=="i8": return "r.u8()? as i8"
    if c=="u16": return "r.u16_le()?"
    if c=="u16be": return "u16::from_be_bytes(r.array::<2>()?)"
    if c=="i16": return "r.u16_le()? as i16"
    if c=="u24": return "r.u24_le()?"
    if c=="u32": return "r.u32_le()?"
    if c=="u32be": return "u32::from_be_bytes(r.array::<4>()?)"
    if c.startswith("bytes:"): return f"r.array::<{c.split(':')[1]}>()?"
    if c=="addr": return ("addr" if name=="bd_addr" else "random_addr")+"(&mut r)?"
    if c=="codingformat": return "CodingFormat::read(&mut r)?"
    if c=="rest": return "r.rest().to_vec()"
    if c=="varbytes": return "{ let n = r.u8()? as usize; r.take(n)?.to_vec() }"
    if c=="advdata": return "{ let n = r.u8()? as usize; let f = r.array::<31>()?; f[..n.min(31)].to_vec() }"
    raise SystemExit("parse? "+c)

def build(cls, e):
    vn=variant(cls); is_adv=cls in ADV; ctr=Ctr(); expect=bytearray()
    decls=[]; binds=[]; ser_lines=[]; parse_lines=[]; tvals=[]; arr_idx=0
    for fd in e["fields"]:
        if "array" in fd:
            subs=fd["array"]; names=[fname(s["name"]) for s in subs]; cnt=f"count{arr_idx}"; arr_idx+=1
            for s,nm in zip(subs,names):
                decls.append(f"        {nm}: Vec<{rust_type(s['codec'])}>,")
            binds+=names
            first=names[0]
            body="\n".join("                    "+ser_elem(s["codec"], f"{fname(s['name'])}[i]") for s in subs)
            ser_lines.append(f"                p.push({first}.len() as u8);\n"
                             f"                for i in 0..{first}.len() {{\n{body}\n                }}")
            inits="\n".join(f"                let mut {nm} = Vec::with_capacity({cnt});" for nm in names)
            pushes="\n".join(f"                    {nm}.push({parse_scalar(s['codec'], s['name'])});" for s,nm in zip(subs,names))
            parse_lines.append(f"                let {cnt} = r.u8()? as usize;\n{inits}\n"
                               f"                for _ in 0..{cnt} {{\n{pushes}\n                }}")
            expect+=bytes([1])
            for s in subs:
                lit,wb=val(s["codec"], ctr, public_address=s["name"]=="bd_addr"); expect+=wb
                tvals.append(f"            {fname(s['name'])}: vec![{lit}],")
        else:
            nm=fname(fd["name"]); c=fd["codec"]
            if is_adv and c=="varbytes": c="advdata"
            decls.append(f"        {nm}: {rust_type(c)},")
            binds.append(nm)
            ser_lines.append("                "+ser_top(c, nm))
            parse_lines.append(f"                let {nm} = {parse_scalar(c, fd['name'])};")
            lit,wb=val(c, ctr, adv=is_adv, public_address=fd["name"]=="bd_addr"); expect+=wb
            tvals.append(f"            {nm}: {lit},")
    return dict(cls=cls, vn=vn, cn=const(cls), code=e["code"], decls=decls, binds=binds,
                ser="\n".join(ser_lines), parse="\n".join(parse_lines), tvals=tvals,
                expect=bytes(expect), body=bytes.fromhex(e["body_hex"]), noparam=(len(e["fields"])==0))

built={}; by_code={}; skipped=[]
for cls,e in sorted(spec.items(), key=lambda kv:(kv[1]["code"], kv[0])):
    if cls in EMBED or cls in SKIP: continue
    if e["code"] in by_code: skipped.append((cls,"dup opcode "+by_code[e["code"]])); continue
    b=build(cls,e)
    if b["vn"] in {x["vn"] for x in built.values()}: skipped.append((cls,"dup variant "+b["vn"])); continue
    if b["expect"]!=b["body"]:
        print(f"SELF-CHECK FAIL {cls}: replay={b['expect'].hex()} != oracle={b['body'].hex()}"); sys.exit(1)
    built[cls]=b; by_code[e["code"]]=cls
print(f"commands generated: {len(built)}  self-check OK  skipped {len(skipped)}: {skipped}")

# ================= EMBEDDED custom commands (phys-derived arrays) =================
EMB_ENUM='''    /// Per-PHY arrays; the count is `scanning_phys.count_ones()`.
    LeSetExtendedScanParameters {
        own_address_type: u8,
        scanning_filter_policy: u8,
        scanning_phys: u8,
        scan_types: Vec<u8>,
        scan_intervals: Vec<u16>,
        scan_windows: Vec<u16>,
    },
    /// Per-PHY arrays; the count is `initiating_phys.count_ones()`.
    LeExtendedCreateConnection {
        initiator_filter_policy: u8,
        own_address_type: u8,
        peer_address_type: u8,
        peer_address: Address,
        initiating_phys: u8,
        scan_intervals: Vec<u16>,
        scan_windows: Vec<u16>,
        connection_interval_mins: Vec<u16>,
        connection_interval_maxs: Vec<u16>,
        max_latencies: Vec<u16>,
        supervision_timeouts: Vec<u16>,
        min_ce_lengths: Vec<u16>,
        max_ce_lengths: Vec<u16>,
    },
'''
EMB_OPCODE='''            Command::LeSetExtendedScanParameters { .. } => {
                HCI_LE_SET_EXTENDED_SCAN_PARAMETERS_COMMAND
            }
            Command::LeExtendedCreateConnection { .. } => HCI_LE_EXTENDED_CREATE_CONNECTION_COMMAND,
'''
EMB_PARAMS='''            Command::LeSetExtendedScanParameters {
                own_address_type,
                scanning_filter_policy,
                scanning_phys,
                scan_types,
                scan_intervals,
                scan_windows,
            } => {
                p.push(*own_address_type);
                p.push(*scanning_filter_policy);
                p.push(*scanning_phys);
                for i in 0..scan_types.len() {
                    p.push(scan_types[i]);
                    push_u16(&mut p, scan_intervals[i]);
                    push_u16(&mut p, scan_windows[i]);
                }
            }
            Command::LeExtendedCreateConnection {
                initiator_filter_policy,
                own_address_type,
                peer_address_type,
                peer_address,
                initiating_phys,
                scan_intervals,
                scan_windows,
                connection_interval_mins,
                connection_interval_maxs,
                max_latencies,
                supervision_timeouts,
                min_ce_lengths,
                max_ce_lengths,
            } => {
                p.push(*initiator_filter_policy);
                p.push(*own_address_type);
                p.push(*peer_address_type);
                p.extend_from_slice(peer_address.address_bytes());
                p.push(*initiating_phys);
                for i in 0..scan_intervals.len() {
                    push_u16(&mut p, scan_intervals[i]);
                    push_u16(&mut p, scan_windows[i]);
                    push_u16(&mut p, connection_interval_mins[i]);
                    push_u16(&mut p, connection_interval_maxs[i]);
                    push_u16(&mut p, max_latencies[i]);
                    push_u16(&mut p, supervision_timeouts[i]);
                    push_u16(&mut p, min_ce_lengths[i]);
                    push_u16(&mut p, max_ce_lengths[i]);
                }
            }
'''
EMB_PARSE='''            HCI_LE_SET_EXTENDED_SCAN_PARAMETERS_COMMAND => {
                let own_address_type = r.u8()?;
                let scanning_filter_policy = r.u8()?;
                let scanning_phys = r.u8()?;
                let n = scanning_phys.count_ones() as usize;
                let mut scan_types = Vec::with_capacity(n);
                let mut scan_intervals = Vec::with_capacity(n);
                let mut scan_windows = Vec::with_capacity(n);
                for _ in 0..n {
                    scan_types.push(r.u8()?);
                    scan_intervals.push(r.u16_le()?);
                    scan_windows.push(r.u16_le()?);
                }
                Command::LeSetExtendedScanParameters {
                    own_address_type,
                    scanning_filter_policy,
                    scanning_phys,
                    scan_types,
                    scan_intervals,
                    scan_windows,
                }
            }
            HCI_LE_EXTENDED_CREATE_CONNECTION_COMMAND => {
                let initiator_filter_policy = r.u8()?;
                let own_address_type = r.u8()?;
                let peer_address_type = r.u8()?;
                let peer_address = random_addr(&mut r)?;
                let initiating_phys = r.u8()?;
                let n = initiating_phys.count_ones() as usize;
                let mut scan_intervals = Vec::with_capacity(n);
                let mut scan_windows = Vec::with_capacity(n);
                let mut connection_interval_mins = Vec::with_capacity(n);
                let mut connection_interval_maxs = Vec::with_capacity(n);
                let mut max_latencies = Vec::with_capacity(n);
                let mut supervision_timeouts = Vec::with_capacity(n);
                let mut min_ce_lengths = Vec::with_capacity(n);
                let mut max_ce_lengths = Vec::with_capacity(n);
                for _ in 0..n {
                    scan_intervals.push(r.u16_le()?);
                    scan_windows.push(r.u16_le()?);
                    connection_interval_mins.push(r.u16_le()?);
                    connection_interval_maxs.push(r.u16_le()?);
                    max_latencies.push(r.u16_le()?);
                    supervision_timeouts.push(r.u16_le()?);
                    min_ce_lengths.push(r.u16_le()?);
                    max_ce_lengths.push(r.u16_le()?);
                }
                Command::LeExtendedCreateConnection {
                    initiator_filter_policy,
                    own_address_type,
                    peer_address_type,
                    peer_address,
                    initiating_phys,
                    scan_intervals,
                    scan_windows,
                    connection_interval_mins,
                    connection_interval_maxs,
                    max_latencies,
                    supervision_timeouts,
                    min_ce_lengths,
                    max_ce_lengths,
                }
            }
'''

# ================= emit codes.rs (command opcodes + preserved event/status tail) =================
CODES_HEAD='''//! HCI constants: packet type indicators, op codes, event/subevent codes, and
//! status. Command op codes are GENERATED from upstream `bumble.hci`; the event
//! and status section is maintained by hand (regenerated with the event port).

// Packet type indicators (the first byte of every HCI packet).
pub const HCI_COMMAND_PACKET: u8 = 0x01;
pub const HCI_ACL_DATA_PACKET: u8 = 0x02;
pub const HCI_SYNCHRONOUS_DATA_PACKET: u8 = 0x03;
pub const HCI_EVENT_PACKET: u8 = 0x04;
pub const HCI_ISO_DATA_PACKET: u8 = 0x05;

// ---- Command op codes (GENERATED: OGF << 10 | OCF) ----
'''
CODES_TAIL='''
// Event codes.
pub const HCI_DISCONNECTION_COMPLETE_EVENT: u8 = 0x05;
pub const HCI_ENCRYPTION_CHANGE_EVENT: u8 = 0x08;
pub const HCI_READ_REMOTE_VERSION_INFORMATION_COMPLETE_EVENT: u8 = 0x0C;
pub const HCI_COMMAND_COMPLETE_EVENT: u8 = 0x0E;
pub const HCI_COMMAND_STATUS_EVENT: u8 = 0x0F;
pub const HCI_NUMBER_OF_COMPLETED_PACKETS_EVENT: u8 = 0x13;
pub const HCI_LE_META_EVENT: u8 = 0x3E;

// LE Meta sub-event codes.
pub const HCI_LE_CONNECTION_COMPLETE_EVENT: u8 = 0x01;
pub const HCI_LE_ADVERTISING_REPORT_EVENT: u8 = 0x02;
pub const HCI_LE_CONNECTION_UPDATE_COMPLETE_EVENT: u8 = 0x03;
pub const HCI_LE_READ_REMOTE_FEATURES_COMPLETE_EVENT: u8 = 0x04;
pub const HCI_LE_LONG_TERM_KEY_REQUEST_EVENT: u8 = 0x05;
pub const HCI_LE_DATA_LENGTH_CHANGE_EVENT: u8 = 0x07;
pub const HCI_LE_ENHANCED_CONNECTION_COMPLETE_EVENT: u8 = 0x0A;
pub const HCI_LE_PHY_UPDATE_COMPLETE_EVENT: u8 = 0x0C;
pub const HCI_LE_EXTENDED_ADVERTISING_REPORT_EVENT: u8 = 0x0D;
pub const HCI_LE_CHANNEL_SELECTION_ALGORITHM_EVENT: u8 = 0x14;

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
'''
allcodes={b["cn"]:b["code"] for b in built.values()}
allcodes["HCI_LE_SET_EXTENDED_SCAN_PARAMETERS_COMMAND"]=0x2041
allcodes["HCI_LE_EXTENDED_CREATE_CONNECTION_COMMAND"]=0x2043
codes=[CODES_HEAD]
for cn,code in sorted(allcodes.items(), key=lambda kv:(kv[1],kv[0])):
    codes.append(f"pub const {cn}: u16 = 0x{code:04X};")
codes.append(CODES_TAIL)
open(DST+"src/codes.rs","w").write("\n".join(codes))
print(f"wrote codes.rs ({len(allcodes)} command opcodes)")

# ================= emit command.rs =================
out=[]
out.append('''//! HCI Command packets (Vol 2, Part E - 5.4.1).
//!
//! Wire form: `[0x01, op_code_lo, op_code_hi, param_len, parameters…]`.
//!
//! The [`Command`] enum is GENERATED from upstream `bumble.hci` (see
//! `tools/hcigen`): one typed variant per command class, plus [`Command::Generic`]
//! for op codes with no typed model. Two phys-derived array commands
//! (`LE_Set_Extended_Scan_Parameters`, `LE_Extended_Create_Connection`) are
//! hand-written because their array count comes from a PHY bitmask, not a
//! leading count byte.

use crate::codes::*;
use crate::{Error, Reader, Result};
use bumble::{Address, AddressType};

/// A codec identifier (Coding Format), 5 bytes on the wire:
/// `coding_format(1) + company_id(2 LE) + vendor_specific_codec_id(2 LE)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodingFormat {
    pub coding_format: u8,
    pub company_id: u16,
    pub vendor_specific_codec_id: u16,
}

impl CodingFormat {
    /// `CodecID::TRANSPARENT` (0x03) with no company/vendor id.
    pub const TRANSPARENT: CodingFormat = CodingFormat {
        coding_format: 0x03,
        company_id: 0,
        vendor_specific_codec_id: 0,
    };

    /// `CodecID::LC3` (0x06) with no company/vendor id.
    pub const LC3: CodingFormat = CodingFormat {
        coding_format: 0x06,
        company_id: 0,
        vendor_specific_codec_id: 0,
    };

    pub fn to_bytes(self) -> [u8; 5] {
        let mut out = [0u8; 5];
        out[0] = self.coding_format;
        out[1..3].copy_from_slice(&self.company_id.to_le_bytes());
        out[3..5].copy_from_slice(&self.vendor_specific_codec_id.to_le_bytes());
        out
    }

    /// Parse the exact five-byte HCI Coding Format representation.
    pub fn from_bytes(data: &[u8]) -> Result<CodingFormat> {
        if data.len() != 5 {
            return Err(Error::InvalidPacket(format!(
                "coding format has length {}, expected 5",
                data.len()
            )));
        }
        Self::read(&mut Reader::new(data, 0))
    }

    fn read(r: &mut Reader) -> Result<CodingFormat> {
        Ok(CodingFormat {
            coding_format: r.u8()?,
            company_id: r.u16_le()?,
            vendor_specific_codec_id: r.u16_le()?,
        })
    }
}

/// An HCI command. Typed variants carry decoded fields; [`Command::Generic`]
/// preserves the raw parameters for op codes with no typed model.
#[allow(clippy::large_enum_variant, clippy::enum_variant_names)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {''')

for cls,b in built.items():
    if b["noparam"]:
        out.append(f"    {b['vn']},")
    else:
        out.append(f"    {b['vn']} {{")
        out+=b["decls"]
        out.append("    }, ".rstrip())
out.append(EMB_ENUM.rstrip("\n"))
out.append('''    /// Any command with no typed model: raw op code + parameters.
    Generic {
        op_code: u16,
        parameters: Vec<u8>,
    },
}

// Small serialization helpers.
fn push_u16(p: &mut Vec<u8>, v: u16) {
    p.extend_from_slice(&v.to_le_bytes());
}
fn push_u24(p: &mut Vec<u8>, v: u32) {
    p.extend_from_slice(&v.to_le_bytes()[..3]);
}

impl Command {
    /// The 16-bit op code for this command.
    pub fn op_code(&self) -> u16 {
        match self {''')
for cls,b in built.items():
    pat="" if b["noparam"] else " { .. }"
    out.append(f"            Command::{b['vn']}{pat} => {b['cn']},")
out.append(EMB_OPCODE.rstrip("\n"))
out.append('''            Command::Generic { op_code, .. } => *op_code,
        }
    }

    /// The serialized command parameters (without the packet/op-code header).
    #[allow(clippy::needless_range_loop)]
    pub fn parameters(&self) -> Vec<u8> {
        let mut p = Vec::new();
        match self {''')
# group no-param commands into one arm
noparams=[b for b in built.values() if b["noparam"]]
if noparams:
    pat=" \n            | ".join(f"Command::{b['vn']}" for b in noparams)
    out.append(f"            {pat} => {{}}")
for cls,b in built.items():
    if b["noparam"]: continue
    out.append(f"            Command::{b['vn']} {{")
    for nm in b["binds"]:
        out.append(f"                {nm},")
    out.append("            } => {")
    out.append(b["ser"])
    out.append("            }")
out.append(EMB_PARAMS.rstrip("\n"))
out.append('''            Command::Generic { parameters, .. } => p.extend_from_slice(parameters),
        }
        p
    }

    /// Serialize to the full wire packet.
    pub fn to_bytes(&self) -> Vec<u8> {
        let params = self.parameters();
        let mut out = Vec::with_capacity(4 + params.len());
        out.push(HCI_COMMAND_PACKET);
        out.extend_from_slice(&self.op_code().to_le_bytes());
        out.push(params.len() as u8);
        out.extend_from_slice(&params);
        out
    }

    /// Parse a complete command packet (including the leading type byte).
    pub fn from_bytes(packet: &[u8]) -> Result<Command> {
        let mut r = Reader::new(packet, 1);
        let op_code = r.u16_le()?;
        let length = r.u8()? as usize;
        let parameters = r
            .take(length)
            .map_err(|_| Error::InvalidPacket("invalid packet length".into()))?;
        Command::from_parameters(op_code, parameters)
    }

    /// Build a typed command from its op code and raw parameters.
    #[allow(clippy::redundant_closure_call)]
    pub fn from_parameters(op_code: u16, parameters: &[u8]) -> Result<Command> {
        // Classic HCI BD_ADDR fields do not carry an address type and therefore
        // denote public device addresses. LE fields that are paired with a
        // separate type byte retain the historical random-device reconstruction
        // below until the enclosing command applies that type.
        let addr = |r: &mut Reader| -> Result<Address> {
            Ok(Address::from_bytes(
                r.array::<6>()?,
                AddressType::PUBLIC_DEVICE,
            ))
        };
        let random_addr = |r: &mut Reader| -> Result<Address> {
            Ok(Address::from_bytes(
                r.array::<6>()?,
                AddressType::RANDOM_DEVICE,
            ))
        };

        let mut r = Reader::new(parameters, 0);
        let _ = &mut r;
        Ok(match op_code {''')
for cls,b in built.items():
    if b["noparam"]:
        out.append(f"            {b['cn']} => Command::{b['vn']},")
    else:
        out.append(f"            {b['cn']} => {{")
        out.append(b["parse"])
        out.append(f"                Command::{b['vn']} {{")
        for nm in b["binds"]:
            out.append(f"                    {nm},")
        out.append("                }")
        out.append("            }")
out.append(EMB_PARSE.rstrip("\n"))
out.append('''            _ => Command::Generic {
                op_code,
                parameters: parameters.to_vec(),
            },
        })
    }
}
''')
open(DST+"src/command.rs","w").write("\n".join(out))
print("wrote command.rs")

# ================= emit tests/generated_commands.rs =================
t=['''//! GENERATED oracle-pinned tests: every typed HCI command round-trips
//! byte-exact against packet bytes captured from real Python Bumble
//! (`bumble.hci`), and re-parses to the same variant. Values are distinct and
//! position-revealing so the layout — not just the length — is pinned.
#![allow(clippy::redundant_clone)]

use bumble_hci::{CodingFormat, Command, HciPacket};
use bumble::{Address, AddressType};

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn check(cmd: Command, expected: &str) {
    let packet = HciPacket::Command(cmd);
    let bytes = packet.to_bytes();
    assert_eq!(hex(&bytes), expected, "serialize mismatch");
    let back = HciPacket::from_bytes(&bytes).expect("parse");
    assert_eq!(back, packet, "round-trip mismatch");
}
''']
for cls,b in built.items():
    body=b["body"]
    full=bytes([1])+bytes([b["code"]&0xFF, b["code"]>>8, len(body)])+body
    fn="cmd_"+b["vn"].lower()
    if b["noparam"]:
        t.append(f'#[test]\nfn {fn}() {{\n    check(Command::{b["vn"]}, "{full.hex()}");\n}}\n')
    else:
        t.append(f'#[test]\nfn {fn}() {{\n    check(Command::{b["vn"]} {{')
        t+=b["tvals"]
        t.append(f'    }}, "{full.hex()}");\n}}\n')
open(DST+"tests/generated_commands.rs","w").write("\n".join(t))
print(f"wrote tests/generated_commands.rs ({len(built)} tests)")
