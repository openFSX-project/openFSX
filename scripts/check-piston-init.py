"""Original sim1 0x20B4FF10 (piston engine initializer) run up to its first Ordinal_55 call, with the
standard-atmosphere globals set to the pinned defaults. No original process.
"""
import argparse,hashlib,json,random,struct
from pathlib import Path
import pefile
from unicorn import UcError,Uc,UC_ARCH_X86,UC_MODE_32
from unicorn.x86_const import UC_X86_REG_ECX,UC_X86_REG_ESP,UC_X86_REG_EIP,UC_X86_REG_FPCW
SHA256='4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e'
OBJ,STACK,SENT,STUB,PSTUB=0x300000,0x400000,0x500000,0x510000,0x520000
D0={'e118':0x118,'e120':0x120,'e128':0x128,'e130':0x130,'e140':0x140,'e148':0x148,'e150':0x150,'e158':0x158,'e178':0x178,'e180':0x180,'e5f8':0x5F8}
FRAME_TOP=STACK+0xF000
def run(image,base,c,tables=False):
    v=Uc(UC_ARCH_X86,UC_MODE_32);v.mem_map(base,(len(image)+4095)&~4095);v.mem_write(base,image)
    for a,s in [(OBJ,0x2000),(STACK,0x10000),(SENT,0x1000),(STUB,0x1000),(PSTUB,0x1000)]:v.mem_map(a,s)
    for a,x in [(0x20B992B0,1.4),(0x20B992C0,518.69),(0x20B992C8,2116.2)]:v.mem_write(a,struct.pack('<d',x))
    v.mem_write(OBJ+0x110,struct.pack('<I',c['e110']));v.mem_write(OBJ+0x10C,struct.pack('<HH',c['e10c'],c['e10e']))
    v.mem_write(0x20B9A1F0,struct.pack('<I',STUB))
    # stdcall fill(dest, value, length), preserving EDI like the real import: push edi; zero; pop edi; ret 12
    v.mem_write(STUB,bytes([0x57,0x31,0xC0,0x8B,0x7C,0x24,0x08,0x8B,0x4C,0x24,0x10,0xF3,0xAA,0x5F,0xC2,0x0C,0x00]))
    # 0x20B8BF10 is a jump through the imported CRT pow (_CIpow: base ST1, exponent ST0).
    # Stand-in in extended precision for base > 0: fxch; fyl2x; fld st0; frndint; fsub st1,st0;
    # fxch; f2xm1; fld1; faddp; fscale; fstp st1; ret
    v.mem_write(0x20B9A0B8,struct.pack('<I',PSTUB))
    v.mem_write(PSTUB,bytes([0xD9,0xC9,0xD9,0xF1,0xD9,0xC0,0xD9,0xFC,0xDC,0xE9,0xD9,0xC9,0xD9,0xF0,0xD9,0xE8,0xDE,0xC1,0xD9,0xFD,0xDD,0xD9,0xC3]))
    v.reg_write(UC_X86_REG_FPCW,0x37F)
    sp=FRAME_TOP;v.mem_write(sp,struct.pack('<I',SENT));v.reg_write(UC_X86_REG_ESP,sp);v.reg_write(UC_X86_REG_ECX,OBJ)
    try:v.emu_start(0x20B4FF10,SENT,count=20000)
    except UcError:pass
    # the first Ordinal_55 import is unresolved in the file image: its call faults at 0x80000000|ordinal
    if v.reg_read(UC_X86_REG_EIP)<0x80000000:raise ValueError('did not reach the first Ordinal_55 call')
    r=dict(c)
    r['region']=bytes(v.mem_read(OBJ+0x100,0x300)).hex()
    if tables:
        ebp=sp-4-4  # after push ebp: ebp = esp_at_entry - 4
        ebp=sp-4
        r['frame']=bytes(v.mem_read(ebp-0x274,0x274)).hex();r['ebp']=ebp
    return r
def sample(r):
    return {'e110':r.choice([0,1,199,200,299,300,301,1000,160000,0x7FFFFFFF,0x80000000,0xFFFFFFFF]),'e10c':r.choice([0,1,2400,0xFFFF]),'e10e':r.choice([0,5,100,0xFFFF])}
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('dll',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
    if a.output.exists():raise ValueError('Output must be new')
    data=a.dll.read_bytes()
    if hashlib.sha256(data).hexdigest()!=SHA256:raise ValueError('Unsupported sim1 fingerprint')
    pe=pefile.PE(data=data);image=pe.get_memory_mapped_image();base=pe.OPTIONAL_HEADER.ImageBase
    rng=random.Random(20261028);cases=[run(image,base,sample(rng)) for i in range(200)]
    a.output.mkdir();(a.output/'report.json').write_text(json.dumps({'schema':1,'source':'bounded-original-piston-init-4ff10','sha256':SHA256,'cases':cases},indent=1)+'\n',encoding='utf-8')
    print(f'PASS: {len(cases)} original runs')
if __name__=='__main__':main()
