"""Original sim1 0x20B4D7E0 (piston default-curve helper) run to its return.
Engine fields are explicit doubles; the standard-atmosphere globals are set to the
pinned FUN_20AF5D20 defaults. No original process.
"""
import argparse,hashlib,json,random,struct
from pathlib import Path
import pefile
from unicorn import Uc,UC_ARCH_X86,UC_MODE_32,UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX,UC_X86_REG_ESP,UC_X86_REG_EIP,UC_X86_REG_FPCW
SHA256='4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e'
OBJ,STACK,SENT=0x300000,0x400000,0x500000
FIELDS={'e118':0x118,'e120':0x120,'e128':0x128,'e130':0x130,'e140':0x140,'e148':0x148,'e150':0x150,'e5f8':0x5F8}
def run(image,base,c):
    v=Uc(UC_ARCH_X86,UC_MODE_32);v.mem_map(base,(len(image)+4095)&~4095);v.mem_write(base,image)
    for a,s in [(OBJ,0x2000),(STACK,0x10000),(SENT,0x1000)]:v.mem_map(a,s)
    v.mem_write(0x20B992B0,struct.pack('<d',1.4));v.mem_write(0x20B992C0,struct.pack('<d',518.69));v.mem_write(0x20B992C8,struct.pack('<d',2116.2))
    for k,o in FIELDS.items():v.mem_write(OBJ+o,struct.pack('<d',c[k]))
    v.mem_write(OBJ+0x170,struct.pack('<I',c['e170']))
    v.reg_write(UC_X86_REG_FPCW,0x37F)
    sp=STACK+0xF000;v.mem_write(sp,struct.pack('<I',SENT));v.reg_write(UC_X86_REG_ESP,sp);v.reg_write(UC_X86_REG_ECX,OBJ)
    v.mem_write(SENT,bytes([0xDD,0x1D])+struct.pack('<I',OBJ+0x1000)+bytes([0x90]))
    v.emu_start(0x20B4D7E0,SENT+7,count=4096)
    if v.reg_read(UC_X86_REG_EIP)!=SENT+7:raise ValueError('did not return')
    val=struct.unpack('<d',v.mem_read(OBJ+0x1000,8))[0]
    r=dict(c);r['result']=val;return r
def sample(r):
    return {'e118':r.choice([0.,1.5,-2.]),'e120':r.choice([0.,.5,3.]),'e128':r.choice([0.,2.,-1.]),'e130':r.choice([0.,.7,1.2]),
     'e140':r.choice([-1.,0.,.5,2.,10.]),'e148':r.choice([0.,.3,1.,2.5]),'e150':r.choice([0.,1.,2.]),'e5f8':r.choice([1.,2.5,.4]),
     'e170':r.choice([0,1,2,0x80000000,0xFFFFFFFF])}
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('dll',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
    if a.output.exists():raise ValueError('Output must be new')
    data=a.dll.read_bytes()
    if hashlib.sha256(data).hexdigest()!=SHA256:raise ValueError('Unsupported sim1 fingerprint')
    pe=pefile.PE(data=data);image=pe.get_memory_mapped_image();base=pe.OPTIONAL_HEADER.ImageBase
    rng=random.Random(20261028);cases=[run(image,base,sample(rng)) for _ in range(300)]
    a.output.mkdir();(a.output/'report.json').write_text(json.dumps({'schema':1,'source':'bounded-original-piston-helper-4d7e0','sha256':SHA256,'cases':cases},indent=1)+'\n',encoding='utf-8')
    print(f'PASS: {len(cases)} original runs')
if __name__=='__main__':main()
