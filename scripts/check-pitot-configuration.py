"""Pinned original pitot CFG initialization with injected lookup/legacy services.
No installed configuration is changed; original write-back calls are recorded.
"""
import argparse,hashlib,importlib.util,json,struct
from pathlib import Path
import pefile
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX,UC_X86_REG_ECX,UC_X86_REG_EDX,UC_X86_REG_ESP,UC_X86_REG_EIP
_s=importlib.util.spec_from_file_location('tail',Path(__file__).with_name('check-electrical-tail.py'));t=importlib.util.module_from_spec(_s);_s.loader.exec_module(t)
def run(image,base,authored,legacy):
    m=t.Machine(image,base,{'context_flag':True,'mode_query':0,'battery_master':False,'max_battery_voltage':24.,'override_power':False});v=m.vm
    m.ranges += [(0x20B45CD0,0x20B45DEF)]
    root=0x101000;provider=0x10E000;writes=[];reads=[]
    def string(a):return bytes(v.mem_read(a,80)).split(b'\0')[0].decode('ascii')
    def hook(vm,address,_size,_data):
        sp=vm.reg_read(UC_X86_REG_ESP)
        if address in (0x20B45D4B,0x20B45D69):
            section,key,out,handle,flags=[m.u32(sp+i*4) for i in range(5)]
            expected='vertical_speed_time_constant' if address==0x20B45D4B else 'pitot_heat'
            if string(section)!='pitot_static' or string(key)!=expected or (handle,flags)!=(0x12345678,0x24):raise ValueError('Unexpected pitot CFG ABI')
            reads.append(expected)
            if address==0x20B45D69 and authored is not None:m.real(out,authored)
            pop,result,length=20,int(address==0x20B45D69 and authored is not None),6
        elif address==0x20B45D8A:
            out,size=m.u32(sp),m.u32(sp+4)
            if vm.reg_read(UC_X86_REG_ECX)!=provider or vm.reg_read(UC_X86_REG_EDX)!=0x324 or size!=8 or not t.STACK<=out<t.STACK+0x10000-8:raise ValueError('Unexpected legacy pitot field ABI')
            if legacy is not None:m.word(out,int(legacy))
            pop,result,length=8,int(legacy is not None),5
        elif address==0x20B45DBE:
            section,key=m.u32(sp),m.u32(sp+4);value=m.f64(sp+8);handle,flags=m.u32(sp+16),m.u32(sp+20)
            if (string(section),string(key),handle,flags)!=('pitot_static','pitot_heat',0x12345678,0x24):raise ValueError('Unexpected pitot write-back ABI')
            writes.append(value);pop,result,length=24,1,6
        else:return
        vm.reg_write(UC_X86_REG_EAX,result);vm.reg_write(UC_X86_REG_ESP,sp+pop);vm.reg_write(UC_X86_REG_EIP,address+length)
    v.hook_add(UC_HOOK_CODE,hook)
    sp=t.STACK+0xF000;m.word(sp,t.SENTINEL);m.word(sp+4,0x12345678)
    v.reg_write(UC_X86_REG_ESP,sp);v.reg_write(UC_X86_REG_ECX,root);v.reg_write(UC_X86_REG_EDX,provider)
    m.execute(0x20B45CD0,sp+8)
    expected=(1. if legacy is None or legacy else 0.) if authored is None or authored<0. else authored
    expected=struct.unpack('<f',struct.pack('<f',expected))[0]
    actual=struct.unpack('<f',v.mem_read(root+0x1244,4))[0]
    if actual!=expected or writes!=([float(expected)] if authored is None or authored<0. else []):raise ValueError('Original pitot configuration differs')
    return {'authored':authored,'legacy_field':legacy,'heater_rate':actual,'recorded_write_back':writes}
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('dll',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
    if a.output.exists():raise ValueError('Output must be new')
    data=a.dll.read_bytes()
    if hashlib.sha256(data).hexdigest()!=t.SHA256:raise ValueError('Unsupported sim1 fingerprint')
    pe=pefile.PE(data=data);cases=[run(pe.get_memory_mapped_image(),pe.OPTIONAL_HEADER.ImageBase,x,y) for x in (None,-2.,-0.1,0.,0.1,1.,2.) for y in (None,False,True)]
    a.output.mkdir();(a.output/'report.json').write_text(json.dumps({'schema':1,'source':'bounded-original-pitot-configuration','sha256':t.SHA256,'cases':cases,'scope':'original initializer with injected CFG/legacy field services; write-backs recorded only, no user configuration mutation'},indent=2)+'\n',encoding='utf-8')
    print(f'PASS: {len(cases)} original pitot initializations and recorded fallback writes')
if __name__=='__main__':main()
