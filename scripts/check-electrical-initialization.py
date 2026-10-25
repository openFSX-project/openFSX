"""Pinned original electrical constructor and complete initializer in emulation.
Requires pefile/unicorn; failure-object factories and context service are injected.
No FSX process, binary loading, original bytes or assets are exported.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import pefile
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP, UC_X86_REG_EIP
_spec=importlib.util.spec_from_file_location('electrical_tail_reference',Path(__file__).with_name('check-electrical-tail.py'))
_tail=importlib.util.module_from_spec(_spec);_spec.loader.exec_module(_tail)
Machine, SHA256, CONTEXT, CONFIG, STATE, STACK, SENTINEL, FAILURE=(_tail.Machine,_tail.SHA256,_tail.CONTEXT,_tail.CONFIG,_tail.STATE,_tail.STACK,_tail.SENTINEL,_tail.FAILURE)


def case(image, base, maximum, nominal, reused, enabled):
    context={'context_flag':True,'mode_query':0,'battery_master':False,'max_battery_voltage':maximum,'override_power':False}
    m=Machine(image,base,context);v=m.vm
    m.ranges += [(0x20B0DA90,0x20B0DB2B),(0x20B26410,0x20B26645)]
    owner,root,engine,services=0x109000,0x101000,0x10B000,0x10A000
    m.word(CONTEXT+0x270,owner);m.word(owner,root);m.word(owner+0x43C,engine)
    m.word(root+0x19B4,CONFIG);m.word(root+0x454,1)
    m.real(CONFIG+0x10,nominal);m.real(CONFIG+0x18,60.);m.word(CONFIG,int(enabled))
    guid_battery=struct.unpack_from('<4I',image,0x20AE3318-base)
    guid_generator=struct.unpack_from('<4I',image,0x20AE3308-base)
    calls=[]
    def service(vm,address,_size,_data):
        sp=vm.reg_read(UC_X86_REG_ESP)
        if address==0x20B2642F:
            if m.u32(sp)!=CONTEXT:raise ValueError('Unexpected initializer context-service receiver')
            result,pop=services,4;name='context'
        elif address in (0x20B2655B,0x20B26600):
            args=[m.u32(sp+i*4) for i in range(8)]
            if address==0x20B2655B:
                if args != [services,*guid_battery,1,0xD017,0]:raise ValueError('Unexpected battery failure-factory ABI')
                result,name=FAILURE,'battery_failure'
            else:
                if args[:6]!=[services,*guid_generator,1] or args[7]!=0 or not STACK<=args[6]<STACK+0x10000-256:raise ValueError('Unexpected generator failure-factory ABI')
                result,name=0x10D000,'generator_failure'
            pop=32
        elif address==0x20B265B0:
            args=[m.u32(sp+i*4) for i in range(3)]
            if vm.reg_read(UC_X86_REG_ECX)!=0xD021 or vm.reg_read(UC_X86_REG_EDX)!=0 or (args[0],args[2])!=(1,CONFIG) or not STACK<=args[1]<STACK+0x10000-256:raise ValueError('Unexpected generator naming ABI')
            result,pop,name=0,12,'generator_name'
        else:return
        calls.append(name);vm.reg_write(UC_X86_REG_EAX,result)
        vm.reg_write(UC_X86_REG_ESP,sp+pop)
        vm.reg_write(UC_X86_REG_EIP,address+(5 if address==0x20B265B0 else 3))
    v.hook_add(UC_HOOK_CODE,service)
    # Poison allocation memory: constructor zeros must come from original code.
    v.mem_write(STATE,bytes([0xA5])*0x250)
    sp=STACK+0xF000;m.word(sp,SENTINEL)
    v.reg_write(UC_X86_REG_ESP,sp);v.reg_write(UC_X86_REG_ECX,STATE)
    m.execute(0x20B0DA90,sp+4)
    if v.reg_read(UC_X86_REG_EAX)!=STATE:raise ValueError('Constructor did not return state')
    constructed={'state':m.state(),'bus_voltage':[m.f64(STATE+4+i*16) for i in range(8)],'bus_amps':[m.f64(STATE+12+i*16) for i in range(8)],'load_amps':m.f64(STATE+0xA4),'battery_master':bool(m.u32(STATE+0x244)),'active_count':m.u32(STATE+0x248)}
    if any(constructed['state'].values()) or any(constructed['bus_voltage']) or any(constructed['bus_amps']) or constructed['load_amps'] or constructed['battery_master'] or constructed['active_count']:raise ValueError('Original constructor did not zero electrical scalars')
    if any(m.u32(STATE+0xAC+i*4) for i in range(34*3)):raise ValueError('Original constructor did not clear consumer records')
    if reused:
        m.real(STATE+0x94,19.);m.real(STATE+0x9C,-3.);m.real(STATE+0x4C,7.)
        m.real(STATE+12,9.);m.real(STATE+0xA4,12.)
        m.word(STATE+0x244,1);m.word(STATE+0x248,1)
    before=m.state()
    m.word(sp,SENTINEL);m.word(sp+4,CONTEXT)
    v.reg_write(UC_X86_REG_ESP,sp);v.reg_write(UC_X86_REG_ECX,STATE)
    m.execute(0x20B26410,sp+8)
    result={'state':m.state(),'bus_voltage':[m.f64(STATE+4+i*16) for i in range(8)],'generator_amps':[m.f64(STATE+0x4C+i*16) for i in range(4)],'source_sum':m.f64(STATE+12),'load_amps':m.f64(STATE+0xA4),'battery_master':bool(m.u32(STATE+0x244)),'active_count':m.u32(STATE+0x248)}
    expected=dict(before,battery_voltage=maximum)
    if result['state']!=expected or result['bus_voltage']!=[maximum]*8 or result['generator_amps']!=[7. if reused else 0.,0.,0.,0.] or result['source_sum']!=(9. if reused else 0.) or result['load_amps']!=(12. if reused else 0.) or result['battery_master']!=reused or result['active_count']!=int(reused):raise ValueError('Original initializer changed unexpected state')
    if [m.u32(STATE+0x84+i*4)-STATE for i in range(4)]!=[0x44,0x54,0x64,0x74]:raise ValueError('Unexpected generator bus aliases')
    first_frame=None
    if maximum>0:
        m.ranges += [(0x20B24C60,0x20B24D78),(0x20B24D80,0x20B24EC9),(0x20B24ED0,0x20B24F3C),(0x20B24F50,0x20B24FB3),(0x20B24FC0,0x20B2507A),(0x20B251D0,0x20B25242),(SENTINEL+0x100,SENTINEL+0x103)]
        aux,factor_owner,factor_data=0x10C000,0x105000,0x106000
        m.word(owner+0x440,aux);m.word(aux,aux+0x100);m.word(aux+0x118,SENTINEL+0x100)
        v.mem_write(SENTINEL+0x100,b'\xd9\xee\xc3')
        m.word(CONTEXT+0x2D8,factor_owner);m.word(factor_owner,factor_data)
        v.mem_write(factor_data+0x38,struct.pack('<f',1.))
        m.word(sp,SENTINEL);m.word(sp+4,CONTEXT);m.real(sp+8,1./120.)
        v.reg_write(UC_X86_REG_ESP,sp);v.reg_write(UC_X86_REG_ECX,STATE)
        m.execute(0x20B24C60,sp+16)
        first_frame={'state':m.state(),'bus_voltage':[m.f64(STATE+4+i*16) for i in range(8)],'generator_amps':[m.f64(STATE+0x4C+i*16) for i in range(4)],'source_sum':m.f64(STATE+12),'load_amps':m.f64(STATE+0xA4),'avionics_active':bool(m.u32(STATE+0xE8)),'additional_system_active':bool(m.u32(STATE+0xB8))}
    return {'max_battery_voltage':maximum,'generator_voltage':nominal,'generator_enabled':enabled,'reused':reused,'constructed':constructed,'before':before,'result':result,'services':calls,'first_frame':first_frame}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('dll',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
    if a.output.exists():raise ValueError('Output directory must be new')
    data=a.dll.read_bytes()
    if hashlib.sha256(data).hexdigest()!=SHA256:raise ValueError('Unsupported sim1 fingerprint')
    pe=pefile.PE(data=data);base=pe.OPTIONAL_HEADER.ImageBase
    if base!=0x20AD0000 or pe.FILE_HEADER.Machine!=0x14C:raise ValueError('Unexpected PE profile')
    image=pe.get_memory_mapped_image()
    cases=[case(image,base,maximum,nominal,reused,enabled) for maximum in (0.,12.,24.,28.,1000.) for nominal in (0.,28.) for reused in (False,True) for enabled in (False,True)]
    a.output.mkdir()
    report={'schema':2,'source':'bounded-installed-sim1-electrical-initialization','sha256':SHA256,'file_version':'10.0.62615.0','cases':cases,'game_launched':False,'scope':'original constructor/full initializer and first frames with one configured engine, enabled/disabled generator and zero/positive rating; injected context/failure/naming/response/zero-auxiliary services and zero circuit metadata; no FLT restore or live provider claim','instruction_budget_per_call':4096,'guarded_writes':True,'guarded_instruction_ranges':True}
    (a.output/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    print(f'PASS: {len(cases)} original constructor/initializer cases / {sum(c["first_frame"] is not None for c in cases)} first frames; fresh zeros, rated buses, reused-state retention and skipped generator voltage writes')


if __name__=='__main__':main()
