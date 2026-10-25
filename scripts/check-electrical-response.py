"""Bounded original REALISM/General -> context float -> battery voltage trace.
Requires pefile/unicorn. No FSX process or original bytes are exported.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import pefile
from unicorn import Uc,UC_ARCH_X86,UC_MODE_32,UC_HOOK_CODE,UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EBX,UC_X86_REG_EAX,UC_X86_REG_ECX,UC_X86_REG_EDI,UC_X86_REG_ESI,UC_X86_REG_EBP,UC_X86_REG_ESP,UC_X86_REG_EIP,UC_X86_REG_FPCW

SIM='4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e'
MAIN='7b01a1b3a53e0779309aaf0144519cabb6b339a82c8490449dabd129c5e5883f'

def run(sim,main,value,active,copy_active,self_link=False):
    v=Uc(UC_ARCH_X86,UC_MODE_32)
    for pe in (sim,main):
        image=pe.get_memory_mapped_image();base=pe.OPTIONAL_HEADER.ImageBase
        v.mem_map(base,(len(image)+4095)&~4095);v.mem_write(base,image)
    obj,ctx,settings,other,state,stack,sentinel=0x100000,0x101000,0x102000,0x103000,0x104000,0x200000,0x220000
    v.mem_map(obj,0x10000);v.mem_map(stack,0x10000);v.mem_map(sentinel,0x1000)
    def word(a,x):v.mem_write(a,struct.pack('<I',x))
    def real(a,x):v.mem_write(a,struct.pack('<d',x))
    def u32(a):return struct.unpack('<I',v.mem_read(a,4))[0]
    def f64(a):return struct.unpack('<d',v.mem_read(a,8))[0]
    def f32(a):return struct.unpack('<f',v.mem_read(a,4))[0]
    word(obj,ctx);word(ctx,0x20AE364C);word(ctx+0x34,other)
    word(other,0x20AE364C);word(ctx+0xB4,int(active));word(other+0xB4,int(copy_active))
    query=struct.unpack("<I",sim.get_data(0x20AE364C+0x50-sim.OPTIONAL_HEADER.ImageBase,4))[0]
    if query!=0x20AF6380:raise ValueError("Unexpected concrete context query")
    word(ctx+0x2D8,obj);real(settings+0x20,value);word(settings+0x2C,1)
    v.mem_write(ctx+0x38,struct.pack('<f',0.75))
    # Validate actual named settings descriptor and getter export identity.
    descriptor=main.get_data(0x209BE2F8-main.OPTIONAL_HEADER.ImageBase,24)
    name,offset,_,kind,_,_=struct.unpack('<6I',descriptor)
    if main.get_data(name-main.OPTIONAL_HEADER.ImageBase,32).split(b'\0')[0]!=b'General' or offset!=0x20 or kind!=0x13:raise ValueError('Unexpected General descriptor')
    section=main.get_data(0x20910F68-main.OPTIONAL_HEADER.ImageBase,16).split(b'\0')[0]
    if section!=b'REALISM':raise ValueError('Unexpected settings section')
    export=next(e for e in main.DIRECTORY_ENTRY_EXPORT.symbols if e.ordinal==57)
    if main.OPTIONAL_HEADER.ImageBase+export.address!=0x209A2EC0:raise ValueError('Unexpected settings getter')
    ranges=[(0x20AF668F,0x20AF66AB),(0x20AF68C7,0x20AF6905),(0x209A2EC0,0x209A2EC6),(0x20B1C310,0x20B1C430),(0x20B24C86,0x20B24CE8),(0x20AF6380,0x20AF6387)]
    def code(vm,address,_size,_data):
        if not any(a<=address<b for a,b in ranges):raise ValueError('Response trace left instruction whitelist')
    def writes(_vm,_access,a,size,_value,_data):
        if not any(lo<=a and a+size<=hi for lo,hi in [(obj,obj+0x10000),(stack,stack+0x10000)]):raise ValueError('Response trace wrote outside guards')
    v.hook_add(UC_HOOK_CODE,code);v.hook_add(UC_HOOK_MEM_WRITE,writes)
    v.reg_write(UC_X86_REG_FPCW,0x37F)
    sp=stack+0xF000;word(sp,sentinel);v.reg_write(UC_X86_REG_ESP,sp)
    v.emu_start(0x209A2EC0,sentinel,count=4096)
    if v.reg_read(UC_X86_REG_EAX)!=0x209C2330:raise ValueError('Getter did not return descriptor-backed settings')
    prior=0.75
    if self_link:
        # Original base construction zeros these fields, then points +0x34
        # at itself before querying/copying General from the actual main getter.
        v.reg_write(UC_X86_REG_EBX,ctx)
        v.emu_start(0x20AF668F,0x20AF66AB,count=4096)
        if v.reg_read(UC_X86_REG_EIP)!=0x20AF66AB or f32(ctx+0x38)!=0.:raise ValueError('Constructor zero prefix failed')
        real(0x209C2330+0x20,value)
        # Resolve this identified import to the pinned original export.
        word(0x20B9A320,0x209A2EC0)
        v.reg_write(UC_X86_REG_ESP,sp-0x100)
        v.emu_start(0x20AF68C7,0x20AF6905,count=4096)
        if v.reg_read(UC_X86_REG_EIP)!=0x20AF6905 or u32(ctx+0x34)!=ctx:raise ValueError('Original constructor self-link failed')
        prior=struct.unpack('<f',struct.pack('<f',value))[0] if active else 0.
        if f32(ctx+0x38)!=prior:raise ValueError('Constructor General copy differs')
    # Copy method receives the same settings layout, supplied explicitly here.
    word(sp,sentinel);word(sp+4,settings);v.reg_write(UC_X86_REG_ESP,sp);v.reg_write(UC_X86_REG_ECX,obj)
    v.emu_start(0x20B1C310,sentinel,count=4096)
    if v.reg_read(UC_X86_REG_EIP)!=sentinel or v.reg_read(UC_X86_REG_ESP)!=sp+8:raise ValueError('Settings copy did not return')
    expected=struct.unpack('<f',struct.pack('<f',value))[0] if active and (active if self_link else copy_active) else prior
    if f32(ctx+0x38)!=expected:raise ValueError('General did not copy to battery response context')
    # Execute actual core filter/voltage fragment with the copied context.
    real(state+0x94,22.);real(state+0x9C,0.);real(state+12,0.);real(state+0xA4,12.)
    fp=sp;real(fp+12,1./120.)
    v.reg_write(UC_X86_REG_EBP,fp);v.reg_write(UC_X86_REG_ESP,sp-0x100)
    v.reg_write(UC_X86_REG_EDI,ctx);v.reg_write(UC_X86_REG_ESI,state)
    v.emu_start(0x20B24C86,0x20B24CE8,count=4096)
    if v.reg_read(UC_X86_REG_EIP)!=0x20B24CE8:raise ValueError('Core fragment did not reach endpoint')
    filtered=-12.*4./120.;voltage=max(0.,22.+filtered*0.000555*expected/120.)
    if abs(f64(state+0x94)-voltage)>1e-12 or abs(f64(state+0x9C)-filtered)>1e-12:raise ValueError('Copied General did not drive voltage update')
    return {'general':value,'context_queries_active':bool(active),'general_copy_active':bool(active if self_link else copy_active),'context_flag_raw':int(active),'copy_context_flag_raw':int(active if self_link else copy_active),'constructor_self_link':self_link,'prior_response_factor':prior,'response_factor':expected,'filtered_net_amps':f64(state+0x9C),'battery_voltage':f64(state+0x94)}

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('root',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
    if a.output.exists():raise ValueError('Output must be new')
    binaries=[]
    for name,digest in [('sim1.dll',SIM),('main.dll',MAIN)]:
        data=(a.root/name).read_bytes()
        if hashlib.sha256(data).hexdigest()!=digest:raise ValueError('Unsupported '+name+' fingerprint')
        binaries.append(pefile.PE(data=data))
    cases=[run(*binaries,x,active,copy_active) for x in (0.,0.1,0.5,1.,1.25) for active in (0,1,2,0xFFFFFFFF) for copy_active in (0,1,2,0xFFFFFFFF)]
    cases += [run(*binaries,x,flag,flag,True) for x in (0.,0.1,0.5,1.,1.25) for flag in (0,1,2,0xFFFFFFFF)]
    a.output.mkdir();(a.output/'report.json').write_text(json.dumps({'schema':2,'source':'bounded-original-realism-electrical-response','sim1_sha256':SIM,'main_sha256':MAIN,'cases':cases,'scope':'named descriptor/getter, guarded context copy and core fragment with original concrete-context queries and explicit settings/raw flags; linked context uses the same concrete vtable as an explicit fixture; no settings-file parsing/default or automatic viewer provider claim'},indent=2)+'\n',encoding='utf-8')
    print(f'PASS: {len(cases)} General settings -> float32 context -> original electrical voltage traces')

if __name__=='__main__':main()
