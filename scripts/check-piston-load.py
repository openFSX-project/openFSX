"""Original sim1 0x20B4F1C0 (piston engine record loader) with the chunk reader 0x20AF1FC0
replaced by a stub serving records 0x505 (0x74 bytes) / 0x500 (0x44 bytes), run up to its first
Ordinal_55 call, with the
standard-atmosphere globals set to the pinned defaults. No original process.
"""
import argparse,hashlib,json,random,struct
from pathlib import Path
import pefile
from unicorn import UcError,Uc,UC_ARCH_X86,UC_MODE_32
from unicorn.x86_const import UC_X86_REG_ECX,UC_X86_REG_ESP,UC_X86_REG_EIP,UC_X86_REG_FPCW
SHA256='4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e'
OBJ,STACK,SENT,STUB,PSTUB,D=0x300000,0x400000,0x500000,0x510000,0x520000,0x560000
D0={'e118':0x118,'e120':0x120,'e128':0x128,'e130':0x130,'e140':0x140,'e148':0x148,'e150':0x150,'e158':0x158,'e178':0x178,'e180':0x180,'e5f8':0x5F8}
FRAME_TOP=STACK+0xF000
def asm_stub(rec505,rec500):
    code=bytearray();fix={}
    def e(*b):code.extend(b)
    def imm(v):return struct.pack('<I',v)
    e(0x57,0x56,0x8B,0x7C,0x24,0x0C,0x8B,0x4C,0x24,0x10)
    e(0x81,0xFA,*imm(0x505));e(0x75,0);j1=len(code)
    e(0xBE,*imm(D+0x10));e(0x83,0x3D,*imm(D),0);e(0x74,0);f1=len(code);e(0xEB,0);c1=len(code)
    code[j1-1]=len(code)-j1
    e(0x81,0xFA,*imm(0x500));e(0x75,0);f2=len(code)
    e(0xBE,*imm(D+0x110));e(0x83,0x3D,*imm(D+0x100),0);e(0x74,0);f3=len(code)
    copy=len(code);code[c1-1]=copy-c1
    e(0xF3,0xA4,0xB8,1,0,0,0,0x5E,0x5F,0xC2,8,0)
    fail=len(code);e(0x31,0xC0,0x5E,0x5F,0xC2,8,0)
    for m in (f1-1,f2-1,f3-1):code[m]=fail-(m+1)
    return bytes(code)
def run(image,base,c):
    v=Uc(UC_ARCH_X86,UC_MODE_32);v.mem_map(base,(len(image)+4095)&~4095);v.mem_write(base,image)
    for a,s in [(OBJ,0x2000),(STACK,0x10000),(SENT,0x1000),(STUB,0x1000),(PSTUB,0x1000),(D,0x2000),(0x540000,0x1000)]:v.mem_map(a,s)
    for a,x in [(0x20B992B0,1.4),(0x20B992C0,518.69),(0x20B992C8,2116.2)]:v.mem_write(a,struct.pack('<d',x))
    v.mem_write(0x20B9A1F0,struct.pack('<I',STUB))
    v.mem_write(STUB,bytes([0x57,0x31,0xC0,0x8B,0x7C,0x24,0x08,0x8B,0x4C,0x24,0x10,0xF3,0xAA,0x5F,0xC2,0x0C,0x00]))
    v.mem_write(0x20B9A0B8,struct.pack('<I',PSTUB))
    v.mem_write(PSTUB,bytes([0xD9,0xC9,0xD9,0xF1,0xD9,0xC0,0xD9,0xFC,0xDC,0xE9,0xD9,0xC9,0xD9,0xF0,0xD9,0xE8,0xDE,0xC1,0xD9,0xFD,0xDD,0xD9,0xC3]))
    rec505=bytes.fromhex(c['rec505']) if c['rec505'] else None;rec500=bytes.fromhex(c['rec500']) if c['rec500'] else None
    v.mem_write(D,struct.pack('<I',1 if rec505 else 0));v.mem_write(D+0x10,rec505 or bytes(1))
    v.mem_write(D+0x100,struct.pack('<I',1 if rec500 else 0));v.mem_write(D+0x110,rec500 or bytes(1))
    v.mem_write(0x540000,bytes(0x1000))
    v.mem_write(0x20AF1FC0,bytes([0xE9])+struct.pack('<i',0x530000-(0x20AF1FC0+5)))
    v.mem_map(0x530000,0x1000);v.mem_write(0x530000,asm_stub(rec505,rec500))
    v.reg_write(UC_X86_REG_FPCW,0x37F)
    sp=FRAME_TOP;v.mem_write(sp,struct.pack('<I',SENT));v.reg_write(UC_X86_REG_ESP,sp);v.reg_write(UC_X86_REG_ECX,OBJ)
    v.mem_write(sp+4,struct.pack('<II',0x540000,0x541000))
    err=None
    try:v.emu_start(0x20B4F1C0,SENT,count=50000)
    except UcError as ex:err=ex
    eip=v.reg_read(UC_X86_REG_EIP)
    if eip!=SENT and eip<0x80000000:raise ValueError(f'unexpected stop: eip={eip:#x} {err}')
    r=dict(c);r['returned']=eip==SENT;r['region']=bytes(v.mem_read(OBJ+0x100,0x300)).hex();return r
def sample(r):
    def rec(n):return bytes(r.getrandbits(8) for _ in range(n))
    rec505=rec(0x74)
    # keep the doubles finite and plain: overwrite double slots with ordinary values
    b=bytearray(rec505)
    for o in (0,8,0x2C,0x38,0x40,0x48,0x50,0x5C,0x64):b[o:o+8]=struct.pack('<d',r.choice([0.,1.5,8.,100.,2400.,160.25]))
    for o in (0x14,0x1C):b[o:o+8]=struct.pack('<d',r.choice([0.,1.5,2700.,160.]))
    mode=r.choice(['505','500','none']) 
    return {'rec505':b.hex() if mode=='505' else '','rec500':rec(0x44).hex() if mode=='500' else '','mode':mode}
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('dll',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
    if a.output.exists():raise ValueError('Output must be new')
    data=a.dll.read_bytes()
    if hashlib.sha256(data).hexdigest()!=SHA256:raise ValueError('Unsupported sim1 fingerprint')
    pe=pefile.PE(data=data);image=pe.get_memory_mapped_image();base=pe.OPTIONAL_HEADER.ImageBase
    rng=random.Random(20261028);cases=[run(image,base,sample(rng)) for _ in range(200)]
    a.output.mkdir();(a.output/'report.json').write_text(json.dumps({'schema':1,'source':'bounded-original-piston-load-4f1c0','sha256':SHA256,'cases':cases},indent=1)+chr(10),encoding='utf-8')
    print(f'PASS: {len(cases)} original runs')
if __name__=='__main__':main()
