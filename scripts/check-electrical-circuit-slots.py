"""Trace original named electrical CFG triples into their internal record slots.
Pinned original code executes; CFG lookup is injected. No FSX process/bytes exported.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import pefile
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX,UC_X86_REG_EBX,UC_X86_REG_EDI,UC_X86_REG_EBP,UC_X86_REG_ESP,UC_X86_REG_EIP
_spec=importlib.util.spec_from_file_location('tail',Path(__file__).with_name('check-electrical-tail.py'))
t=importlib.util.module_from_spec(_spec);_spec.loader.exec_module(t)

def trace(image,base,maximum=24.,authored=True):
    m=t.Machine(image,base,{'context_flag':True,'mode_query':0,'battery_master':True,'max_battery_voltage':maximum,'override_power':False});v=m.vm
    m.ranges += [(0x20B253EA,0x20B26347)]
    root=0x101000;fp=t.STACK+0xF000;m.word(root+0x19B4,t.CONFIG)
    v.mem_write(fp-0x28,b'Electrical\0');m.word(fp-8,0x12345678)
    calls=[]
    def string(a):return bytes(v.mem_read(a,100)).split(b'\0')[0].decode('ascii')
    def code(vm,address,size,_data):
        if bytes(vm.mem_read(address,size))!=b'\xff\xd6':return
        sp=vm.reg_read(UC_X86_REG_ESP);section,key,out,count,handle,flags=[m.u32(sp+i*4) for i in range(6)]
        if string(section)!='Electrical' or (count,handle,flags)!=(3,0x12345678,0x24) or out!=fp-0x50:raise ValueError('Unexpected CFG triple ABI')
        index=len(calls)+1;defaults=[m.f64(out+i*8) for i in range(3)]
        values=[float(index%8),100.+index,10.+index/10.] if authored else defaults
        calls.append({'slot':index,'key':string(key),'injected_bus':int(values[0]),'injected_max_amps':values[1],'injected_min_voltage':values[2],'defaults':defaults})
        if authored:
            for i,x in enumerate(values):m.real(out+i*8,x)
        vm.reg_write(UC_X86_REG_EAX,int(authored));vm.reg_write(UC_X86_REG_ESP,sp+24);vm.reg_write(UC_X86_REG_EIP,address+2)
    v.hook_add(UC_HOOK_CODE,code)
    for reg,value in [(UC_X86_REG_EBP,fp),(UC_X86_REG_ESP,fp-0x100),(UC_X86_REG_EBX,t.CONFIG),(UC_X86_REG_EDI,root)]:v.reg_write(reg,value)
    v.emu_start(0x20B253EA,0x20B26347,count=4096)
    if v.reg_read(UC_X86_REG_EIP)!=0x20B26347 or len(calls)!=33:raise ValueError('Incomplete original circuit reads')
    for c in calls:
        ptr=t.CONFIG+0x20+c['slot']*20
        if (m.f64(ptr),m.f64(ptr+8),m.u32(ptr+16))!=(c['injected_max_amps'],c['injected_min_voltage'],c['injected_bus']):raise ValueError('Named CFG key did not reach expected slot')
    return calls


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('dll',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
    if a.output.exists():raise ValueError('Output must be new')
    data=a.dll.read_bytes()
    if hashlib.sha256(data).hexdigest()!=t.SHA256:raise ValueError('Unsupported sim1 fingerprint')
    pe=pefile.PE(data=data);base=pe.OPTIONAL_HEADER.ImageBase;image=pe.get_memory_mapped_image()
    calls=trace(image,base)
    defaults=[{'max_battery_voltage':maximum,'circuits':trace(image,base,maximum,False)} for maximum in (0.,12.,24.,28.,1000.)]
    a.output.mkdir();(a.output/'report.json').write_text(json.dumps({'schema':2,'source':'bounded-original-electrical-circuit-slots','sha256':t.SHA256,'circuits':calls,'default_cases':defaults,'scope':'original named CFG triple stores and missing-key defaults with injected lookup contract; no equipment activity or live process claim'},indent=2)+'\n',encoding='utf-8')
    print('PASS: 33 original named CFG slots and 165 original missing-key circuit defaults')


if __name__=='__main__':main()
