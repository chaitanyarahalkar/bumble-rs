import os, bumble.hci as h, json, collections, struct
from bumble.hci import Address, HCI_Object

OUT=os.environ.get("HCIGEN_OUT", os.path.dirname(os.path.abspath(__file__)))+"/"

def real(suffix,attr):
    return {nm:getattr(h,nm) for nm in dir(h) if isinstance(getattr(h,nm),type) and nm.startswith("HCI_") and nm.endswith(suffix) and hasattr(getattr(h,nm),attr)}
cmds=real("_Command","op_code"); evts=real("_Event","event_code")

def probe_width(spec):
    """Width in bytes of a dict-serializer (enum) field."""
    ser=spec.get('serializer'); mapper=spec.get('mapper')
    for sample in ([mapper(0)] if mapper else []) + [0]:
        try: return len(ser(sample))
        except Exception: pass
    return 1

def codec_of(spec):
    if isinstance(spec,int):
        return {1:"u8",-1:"i8",2:"u16",-2:"i16",3:"u24",4:"u32"}.get(spec) or (f"bytes:{spec}" if 4<spec<=256 else f"int:{spec}")
    if isinstance(spec,str):
        return {'*':"rest",'v':"varbytes",'>2':"u16be",'>4':"u32be"}.get(spec,f"str:{spec}")
    if isinstance(spec,dict):
        if 'size' in spec: return codec_of(spec['size'])
        q=getattr(spec.get('parser'),'__qualname__','')
        if 'length_prefixed' in q: return "varbytes"
        return {1:"u8",2:"u16",3:"u24",4:"u32"}.get(probe_width(spec),f"bytes:{probe_width(spec)}")
    if callable(spec):
        q=getattr(spec,'__qualname__','')
        if 'address' in q.lower() or 'Address' in q or 'Random_Address' in q: return "addr"
        if 'CodingFormat' in q: return "codingformat"
        return "call:"+q
    return "other"

CTR=[0]
def nb(n):
    out=bytes(((CTR[0]+i)&0xFF) for i in range(1,n+1)); CTR[0]=(CTR[0]+n)&0xFF; return out
def val_for(spec):
    c=codec_of(spec)
    if c=="u8": return int.from_bytes(nb(1),'little')
    if c=="i8": return 5
    if c=="u16" or c=="u16be": return int.from_bytes(nb(2),'little')
    if c=="i16": return 9
    if c=="u24": return int.from_bytes(nb(3),'little')
    if c=="u32" or c=="u32be": return int.from_bytes(nb(4),'little')
    if c.startswith("bytes:"): return nb(int(c.split(':')[1]))
    if c=="rest": return nb(4)
    if c=="varbytes": return nb(3)
    if c=="addr": return Address(nb(6), Address.RANDOM_DEVICE_ADDRESS)
    if c=="codingformat": return h.CodingFormat(h.CodecID.CVSD)
    return nb(1)

def fieldspecs(fields):
    out=[]
    for item in fields:
        if isinstance(item,list):
            out.append({"array":[{"name":n,"codec":codec_of(s)} for (n,s) in item]})
        else:
            n,s=item; out.append({"name":n,"codec":codec_of(s)})
    return out

def body_bytes(cls):
    """Serialize params body via upstream dict_to_bytes with distinct values."""
    CTR[0]=0
    fields=cls.fields
    vals={}
    for item in fields:
        if isinstance(item,list):
            for (n,s) in item: vals[n]=[val_for(s)]
        else:
            n,s=item; vals[n]=val_for(s)
    return HCI_Object.dict_to_bytes(vals, fields)

CUSTOM={'HCI_LE_Extended_Create_Connection_Command','HCI_LE_Set_Extended_Scan_Parameters_Command',
        'HCI_LE_Read_All_Local_Supported_Features_Command','HCI_LE_Meta_Event'}

def build(reg, is_cmd):
    specs={}; oracle={}; errs=[]
    for nm,c in reg.items():
        if nm in CUSTOM: continue
        f=getattr(c,'fields',None)
        if not isinstance(f,(list,tuple)): f=[]
        entry={"class":nm,"code":(c.op_code if is_cmd else c.event_code),"fields":fieldspecs(f)}
        if not is_cmd and hasattr(c,'subevent_code'): entry["subevent_code"]=c.subevent_code
        try:
            body=body_bytes(c)
            entry["body_hex"]=body.hex()
            oracle[nm]=body.hex()
        except Exception as e:
            errs.append((nm,str(e)[:90])); continue
        specs[nm]=entry
    return specs,oracle,errs

cspec,coracle,cerr=build(cmds,True)
espec,eoracle,eerr=build(evts,False)
json.dump({"commands":cspec,"events":espec}, open(OUT+"spec.json","w"), indent=1)
print(f"commands: spec={len(cspec)} err={len(cerr)}   events: spec={len(espec)} err={len(eerr)}")
for nm,e in (cerr+eerr): print("  ERR",nm,"->",e)
# distinct codec vocab in final spec
voc=collections.Counter()
def walk(fs):
    for fd in fs:
        if "array" in fd: walk(fd["array"])
        else: voc[fd["codec"]]+=1
for e in cspec.values(): walk(e["fields"])
for e in espec.values(): walk(e["fields"])
print("\nFINAL codec vocab:", dict(voc.most_common()))
