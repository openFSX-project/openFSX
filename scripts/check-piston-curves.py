"""Original sim1 0x20B4D8A0 (piston default performance curves) run to its return.
Engine fields are explicit; standard-atmosphere globals use the pinned defaults; the
imported zero-fill is replaced by a local stub. Dumps the output arrays and, with
--tables, the four embedded lookup tables captured from the stack frame.
"""
import argparse,hashlib,json,random,struct
from pathlib import Path
import pefile
from unicorn import Uc,UC_ARCH_X86,UC_MODE_32
from unicorn.x86_const import UC_X86_REG_ECX,UC_X86_REG_ESP,UC_X86_REG_EIP,UC_X86_REG_FPCW
SHA256='4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e'
OBJ,STACK,SENT,STUB=0x300000,0x400000,0x500000,0x510000
D={'e118':0x118,'e120':0x120,'e128':0x128,'e130':0x130,'e140':0x140,'e148':0x148,'e150':0x150,'e158':0x158,'e178':0x178,'e180':0x180,'e5f8':0x5F8}
FRAME_TOP=STACK+0xF000
def run(image,base,c,tables=False):
    v=Uc(UC_ARCH_X86,UC_MODE_32);v.mem_map(base,(len(image)+4095)&~4095);v.mem_write(base,image)
    for a,s in [(OBJ,0x2000),(STACK,0x10000),(SENT,0x1000),(STUB,0x1000)]:v.mem_map(a,s)
    for a,x in [(0x20B992B0,0.0023769),(0x20B992C0,2116.2),(0x20B992C8,32.174)]:v.mem_write(a,struct.pack('<d',x))
    for k,o in D.items():v.mem_write(OBJ+o,struct.pack('<d',c[k]))
    v.mem_write(OBJ+0x170,struct.pack('<I',c['e170']))
    v.mem_write(0x20B9A1F0,struct.pack('<I',STUB))
    # stdcall fill(dest, value, length): xor eax,eax; mov edi,[esp+4]; mov ecx,[esp+12]; rep stosb; ret 12
    v.mem_write(STUB,bytes([0x31,0xC0,0x8B,0x7C,0x24,0x04,0x8B,0x4C,0x24,0x0C,0xF3,0xAA,0xC2,0x0C,0x00]))
    v.reg_write(UC_X86_REG_FPCW,0x37F)
    sp=FRAME_TOP;v.mem_write(sp,struct.pack('<I',SENT));v.reg_write(UC_X86_REG_ESP,sp);v.reg_write(UC_X86_REG_ECX,OBJ)
    v.emu_start(0x20B4D8A0,SENT,count=20000)
    if v.reg_read(UC_X86_REG_EIP)!=SENT:raise ValueError('did not return')
    r=dict(c)
    r['array_5cc']=bytes(v.mem_read(OBJ+0x5CC,84)).hex();r['array_620']=bytes(v.mem_read(OBJ+0x620,68)).hex()
    r['scalars']=bytes(v.mem_read(OBJ+0x2B0,32)).hex()
    if tables:
        ebp=sp-4-4  # after push ebp: ebp = esp_at_entry - 4
        ebp=sp-4
        r['frame']=bytes(v.mem_read(ebp-0x274,0x274)).hex();r['ebp']=ebp
    return r
def sample(r):
    return {'e118':r.choice([0.,1.5,-2.]),'e120':r.choice([0.,.5,3.]),'e128':r.choice([0.,2.,-1.]),'e130':r.choice([0.,.7,1.2]),
     'e140':r.choice([-1.,0.,.5,2.,10.]),'e148':r.choice([0.,.3,1.,2.5]),'e150':r.choice([0.,1.,2.]),'e158':r.choice([1.,2.5,100.]),
     'e178':r.choice([0.,1000.,2400.,2700.,3500.]),'e180':r.choice([0.,50.,160000.,500000.]),'e5f8':r.choice([1.,2.5,.4]),
     'e170':r.choice([0,1,2,4,0x80000000])}
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('dll',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
    if a.output.exists():raise ValueError('Output must be new')
    data=a.dll.read_bytes()
    if hashlib.sha256(data).hexdigest()!=SHA256:raise ValueError('Unsupported sim1 fingerprint')
    pe=pefile.PE(data=data);image=pe.get_memory_mapped_image();base=pe.OPTIONAL_HEADER.ImageBase
    rng=random.Random(20261028);cases=[run(image,base,sample(rng),tables=(i==0)) for i in range(200)]
    a.output.mkdir();(a.output/'report.json').write_text(json.dumps({'schema':1,'source':'bounded-original-piston-curves-4d8a0','sha256':SHA256,'cases':cases},indent=1)+'\n',encoding='utf-8')
    print(f'PASS: {len(cases)} original runs')
if __name__=='__main__':main()
