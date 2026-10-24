"""Pinned original circuit predicate and frame-tail checks in x86 emulation.
No FSX process or Windows DLL loading. Requires pefile and unicorn.
Context-mode, failure and avionics services are explicitly injected. A bounded
CFG lookup separately checks the global override's configuration provenance.
"""
import argparse
import hashlib
import itertools
import json
import math
from pathlib import Path
import struct
import pefile
from unicorn import Uc,UC_ARCH_X86,UC_MODE_32,UC_HOOK_CODE,UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX,UC_X86_REG_EBX,UC_X86_REG_ECX,UC_X86_REG_ESI,UC_X86_REG_EDI,UC_X86_REG_EBP,UC_X86_REG_ESP,UC_X86_REG_EIP,UC_X86_REG_FPCW

SHA256='4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e'
CONTEXT,CONFIG,FAILURE,MODE,STATE,STACK,SENTINEL=0x100000,0x102000,0x104000,0x108000,0x110000,0x200000,0x220000


class Machine:
    def __init__(self,image,base,context):
        self.vm=Uc(UC_ARCH_X86,UC_MODE_32);v=self.vm
        v.mem_map(base,(len(image)+4095)&~4095);v.mem_write(base,image)
        for a,s in [(CONTEXT,0x10000),(STATE,0x1000),(STACK,0x10000),(SENTINEL,0x1000)]:v.mem_map(a,s)
        self.context=context;self.failure=False;self.avionics=False
        self.prefix=None
        self.word(CONTEXT,0x10A000);self.word(CONTEXT+0x64,MODE);self.word(MODE,MODE+0x100)
        self.word(CONTEXT+0xB4,int(context['context_flag']));self.word(STATE,CONFIG)
        self.real(CONFIG+8,context['max_battery_voltage']);self.word(STATE+0x244,int(context['battery_master']))
        self.word(0x20B989B8,int(context['override_power']))
        self.word(FAILURE,FAILURE+0x100);self.word(STATE+0x24C,FAILURE)
        v.reg_write(UC_X86_REG_FPCW,0x37F)
        self.ranges=[(0x20B25120,0x20B251C9),(0x20B25250,0x20B252A0)]
        self.vm.hook_add(UC_HOOK_CODE,self.instruction);self.vm.hook_add(UC_HOOK_MEM_WRITE,self.writes)
    def word(self,a,x):self.vm.mem_write(a,struct.pack('<I',x))
    def real(self,a,x):self.vm.mem_write(a,struct.pack('<d',x))
    def u32(self,a):return struct.unpack('<I',self.vm.mem_read(a,4))[0]
    def f64(self,a):return struct.unpack('<d',self.vm.mem_read(a,8))[0]
    def circuit(self,index,circuit):
        if circuit is None:self.word(STATE+0xB4+index*12,0);return
        pointer=CONFIG+0x20+index*20
        self.real(pointer,5.);self.real(pointer+8,circuit['min_voltage']);self.word(pointer+16,circuit['bus'])
        self.word(STATE+0xB4+index*12,pointer);self.word(STATE+0xB0+index*12,int(circuit['forced_off']))
    def buses(self,values):
        for i,x in enumerate(values):self.real(STATE+4+i*16,x)
    def instruction(self,vm,address,_size,_data):
        if not any(a<=address<b for a,b in self.ranges):raise ValueError('Tail reference left instruction whitelist')
        sp=vm.reg_read(UC_X86_REG_ESP)
        if address==0x20B2513A:
            if vm.reg_read(UC_X86_REG_ECX)!=MODE:raise ValueError('Unexpected mode-query receiver')
            vm.reg_write(UC_X86_REG_EAX,self.context['mode_query']);vm.reg_write(UC_X86_REG_EIP,address+6)
        elif address==0x20B24E16:
            if self.u32(sp)!=0x10D000:raise ValueError('Unexpected full-frame generator failure ABI')
            vm.reg_write(UC_X86_REG_EAX,0);vm.reg_write(UC_X86_REG_ESP,sp+4);vm.reg_write(UC_X86_REG_EIP,address+3)
        elif address==0x20B24D1F:
            self.prefix={'state':self.state(),'bus_voltage':[self.f64(STATE+4+i*16) for i in range(8)],'source_sum':self.f64(STATE+12)}
        elif address==0x20B24D26:
            if self.u32(sp)!=FAILURE:raise ValueError('Unexpected battery-failure ABI')
            vm.reg_write(UC_X86_REG_EAX,int(self.failure));vm.reg_write(UC_X86_REG_ESP,sp+4);vm.reg_write(UC_X86_REG_EIP,address+3)
        elif address==0x20B251F7:
            receiver,variable,unit,out,flags=[self.u32(sp+i*4) for i in range(5)]
            if (receiver,variable,unit,flags)!=(CONTEXT,172,143,0) or not STACK<=out<STACK+0x10000-8:raise ValueError('Unexpected AVIONICS MASTER SWITCH query ABI')
            self.real(out,float(self.avionics));vm.reg_write(UC_X86_REG_EAX,1);vm.reg_write(UC_X86_REG_ESP,sp+20);vm.reg_write(UC_X86_REG_EIP,address+3)
    def writes(self,_vm,_access,address,size,_value,_data):
        if not any(a<=address and address+size<=b for a,b in [(CONTEXT,CONTEXT+0x10000),(STATE,STATE+0x1000),(STACK,STACK+0x10000)]):raise ValueError('Tail reference wrote outside guarded mutable regions')
    def execute(self,start,expected_sp):
        self.vm.emu_start(start,SENTINEL,count=4096)
        if self.vm.reg_read(UC_X86_REG_EIP)!=SENTINEL or self.vm.reg_read(UC_X86_REG_ESP)!=expected_sp:raise ValueError('Tail reference did not return with expected ABI')
    def state(self):return {'battery_voltage':self.f64(STATE+0x94),'filtered_net_amps':self.f64(STATE+0x9C),'generator_amps':self.f64(STATE+0x4C)}


def expected_power(context,circuit,buses):
    if not context['context_flag'] and context['mode_query']!=1:return context['battery_master']
    if circuit is None:return False
    if circuit['bus']>7:return True
    return context['override_power'] or context['max_battery_voltage']==0. or (buses[circuit['bus']]>circuit['min_voltage'] and not circuit['forced_off'])


def power_case(image,base,context,circuit,voltage):
    m=Machine(image,base,context);buses=[voltage]*8;m.buses(buses);m.circuit(5,circuit)
    sp=STACK+0xF000;m.word(sp,SENTINEL);m.word(sp+4,5);m.word(sp+8,CONTEXT)
    m.vm.reg_write(UC_X86_REG_ESP,sp);m.vm.reg_write(UC_X86_REG_ECX,STATE)
    m.execute(0x20B25120,sp+12);result=bool(m.vm.reg_read(UC_X86_REG_EAX))
    if result!=expected_power(context,circuit,buses):raise ValueError('Original power predicate mismatch')
    return {'context':context,'circuit':circuit,'bus_voltage':buses,'available':result}


def tail_case(image,base,context,failed,avionics,main,circuit_av=None,circuit_nav=None,missing_av=False,missing_nav=False):
    m=Machine(image,base,context);m.ranges += [(0x20B24D1F,0x20B24D78),(0x20B251D0,0x20B25242)]
    m.failure=failed;m.avionics=avionics
    av=None if missing_av else circuit_av or {'bus':0,'min_voltage':17.,'forced_off':False}
    nav=None if missing_nav else circuit_nav or {'bus':1,'min_voltage':17.,'forced_off':False}
    m.circuit(5,av);m.circuit(1,nav)
    incoming=[main,27.,main,main,28.,0.,0.,0.];currents=[14.,3.,2.,1.]
    m.buses(incoming)
    for i,x in enumerate(currents):m.real(STATE+0x4C+i*16,x)
    m.real(STATE+12,12.);m.real(STATE+0x94,23.5);m.real(STATE+0x9C,3.)
    sp=STACK+0xF000;m.word(sp,SENTINEL);m.word(sp+4,CONTEXT);m.real(sp+8,1./120.)
    m.vm.reg_write(UC_X86_REG_ESP,sp-16);m.vm.reg_write(UC_X86_REG_EBP,sp-4)
    m.vm.reg_write(UC_X86_REG_ESI,STATE);m.vm.reg_write(UC_X86_REG_EDI,CONTEXT);m.vm.reg_write(UC_X86_REG_ECX,FAILURE)
    m.execute(0x20B24D1F,sp+16)
    buses=[0.]*8 if failed else incoming.copy()
    av_active=expected_power(context,av,buses) and avionics;buses[1]=buses[0] if av_active else 0.
    nav_active=expected_power(context,nav,buses)
    actual=[m.f64(STATE+4+i*16) for i in range(8)];amps=[m.f64(STATE+0x4C+i*16) for i in range(4)]
    if actual!=buses or amps!=([0.]*4 if failed else currents) or m.f64(STATE+12)!=(0. if failed else 12.) or m.u32(STATE+0xE8)!=int(av_active) or m.u32(STATE+0xB8)!=int(nav_active) or m.f64(STATE+0x94)!=23.5 or m.f64(STATE+0x9C)!=3.:
        raise ValueError('Original battery-failure/avionics tail mismatch')
    return {'context':context,'battery_failed':failed,'avionics_master':avionics,'avionics_circuit':av,'navcom1_circuit':nav,'incoming_bus_voltage':incoming,'incoming_generator_amps':currents,'incoming_source_sum':12.,
        'bus_voltage':actual,'generator_amps':amps,'source_sum':m.f64(STATE+12),'avionics_active':av_active,'navcom1_active':nav_active,'battery_voltage_unchanged':m.f64(STATE+0x94),'filtered_net_unchanged':m.f64(STATE+0x9C)}


def frame_sequence(image,base,name,voltage,frames):
    context={'context_flag':True,'mode_query':0,'battery_master':True,'max_battery_voltage':24.,'override_power':False}
    m=Machine(image,base,context)
    m.ranges += [(0x20B24C60,0x20B24D78),(0x20B24D80,0x20B24EC9),(0x20B24ED0,0x20B24F3C),(0x20B24F50,0x20B24FB3),(0x20B24FC0,0x20B2507A),(0x20B251D0,0x20B25242),(SENTINEL+0x100,SENTINEL+0x103)]
    owner,root,engine,aux,factor_owner,factor_data=0x109000,0x101000,0x10B000,0x10C000,0x105000,0x106000
    m.word(CONTEXT+0x270,owner);m.word(owner,root);m.word(root+0x454,1);m.word(root+0x19B4,CONFIG)
    m.word(owner+0x43C,engine);m.word(engine+0x90,0x10D000);m.word(0x10D000,0x10D100)
    m.word(owner+0x440,aux);m.word(aux,aux+0x100);m.word(aux+0x118,SENTINEL+0x100);m.vm.mem_write(SENTINEL+0x100,b'\xd9\xee\xc3')
    m.word(CONTEXT+0x2D8,factor_owner);m.word(factor_owner,factor_data)
    m.word(CONFIG,1);m.real(CONFIG+0x10,28.);m.real(CONFIG+0x18,60.);m.word(STATE+0x84,STATE+0x44)
    av={'bus':0,'min_voltage':17.,'forced_off':False};nav={'bus':1,'min_voltage':17.,'forced_off':False}
    m.circuit(1,nav);m.real(CONFIG+0x34,12.);m.circuit(5,av)
    m.real(STATE+0x94,voltage);records=[]
    for online,avionics,failed,override in frames:
        context=dict(context,override_power=override);m.context=context;m.failure=failed;m.avionics=avionics
        m.word(0x20B989B8,int(override));m.word(engine+0x8C,int(online));m.real(engine+0x5C,0.9 if online else 0.)
        m.vm.mem_write(factor_data+0x38,struct.pack('<f',1.))
        before=m.state();before_buses=[m.f64(STATE+4+i*16) for i in range(8)];before_amps=[m.f64(STATE+0x4C+i*16) for i in range(4)]
        load=12.*m.u32(STATE+0xB8)+5.*m.u32(STATE+0xE8)
        sp=STACK+0xF000;m.word(sp,SENTINEL);m.word(sp+4,CONTEXT);m.real(sp+8,1./120.)
        m.vm.reg_write(UC_X86_REG_ESP,sp);m.vm.reg_write(UC_X86_REG_ECX,STATE)
        m.execute(0x20B24C60,sp+16)
        if m.f64(STATE+0xA4)!=load:raise ValueError('Full-frame consumer-load lag mismatch')
        if m.prefix is None:raise ValueError('Missing original prefix boundary')
        records.append({'context':context,'input':{'generator_online':online,'avionics_master':avionics,'battery_failed':failed,'dt':1./120.,'response_factor':1.},
            'before':before,'before_bus_voltage':before_buses,'before_generator_amps':before_amps,'load_used_amps':load,'prefix':m.prefix,
            'state':m.state(),'bus_voltage':[m.f64(STATE+4+i*16) for i in range(8)],'generator_amps':[m.f64(STATE+0x4C+i*16) for i in range(4)],'source_sum':m.f64(STATE+12),
            'avionics_active':bool(m.u32(STATE+0xE8)),'navcom1_active':bool(m.u32(STATE+0xB8))})
    return {'name':name,'configuration':{'max_battery_voltage':24.,'generator_voltage':28.,'max_generator_amps':60.},'initial':{'battery_voltage':voltage,'filtered_net_amps':0.,'generator_amps':0.},'avionics_circuit':av,'navcom1_circuit':nav,'avionics_max_amps':5.,'navcom1_max_amps':12.,'frames':records}


def override_sequence(image,base,initial,authored_values):
    """Execute the final two original boolean CFG reads, with exact ABI checks.
    Missing lookup deliberately writes nothing, as the injected CFG contract.
    No process-start default, engine setup or full configuration load is claimed.
    """
    context={'context_flag':True,'mode_query':0,'battery_master':False,'max_battery_voltage':24.,'override_power':initial}
    m=Machine(image,base,context);v=m.vm
    m.ranges += [(0x20B263A4,0x20B263EC)]
    root=0x101000;m.word(root+0x46C,0)
    current=None;calls=[]
    def lookup(vm,address,_size,_data):
        if address not in (0x20B263C8,0x20B263E3):return
        sp=vm.reg_read(UC_X86_REG_ESP)
        section,key,out,handle,flags=[m.u32(sp+i*4) for i in range(5)]
        def string(a):return bytes(vm.mem_read(a,64)).split(b'\0')[0]
        expected=b'alternators' if address==0x20B263C8 else b'electric_always_available'
        expected_out=STACK+0xF000-0xC if address==0x20B263C8 else 0x20B989B8
        if string(section)!=b'Electrical' or string(key)!=expected or (out,handle,flags)!=(expected_out,0x12345678,0x24):raise ValueError('Unexpected original boolean CFG lookup ABI')
        before=m.u32(out)
        if address==0x20B263E3:
            if current is not None:m.word(out,int(current))
            calls.append({'authored':current,'before':bool(before),'after':bool(m.u32(out))})
        vm.reg_write(UC_X86_REG_EAX,int(current is not None));vm.reg_write(UC_X86_REG_ESP,sp+20);vm.reg_write(UC_X86_REG_EIP,address+2)
    v.hook_add(UC_HOOK_CODE,lookup)
    expected=initial
    for current in authored_values:
        fp=STACK+0xF000;m.word(fp-8,0x12345678)
        v.mem_write(fp-0x28,b'Electrical\0')
        # The entry is inside the original CFG initializer, after four saves.
        for reg,value in [(UC_X86_REG_EBP,fp),(UC_X86_REG_ESP,fp-0x60),(UC_X86_REG_EDI,root),(UC_X86_REG_EBX,CONFIG),(UC_X86_REG_ESI,0x12345678)]:v.reg_write(reg,value)
        m.word(fp,0);m.word(fp+4,SENTINEL)
        m.execute(0x20B263A4,fp+8)
        expected=expected if current is None else current
        if bool(m.u32(0x20B989B8))!=expected:raise ValueError('Original missing CFG key did not retain override')
        m.circuit(5,{'bus':0,'min_voltage':17.,'forced_off':True});m.buses([0.]*8)
        sp=STACK+0xF000;m.word(sp,SENTINEL);m.word(sp+4,5);m.word(sp+8,CONTEXT)
        v.reg_write(UC_X86_REG_ESP,sp);v.reg_write(UC_X86_REG_ECX,STATE)
        m.execute(0x20B25120,sp+12)
        available=bool(v.reg_read(UC_X86_REG_EAX))
        if available!=expected:raise ValueError('CFG global did not control original power bypass')
        calls[-1]['available_with_zero_voltage_and_forced_off']=available
    return {'initial':initial,'loads':calls}


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('dll',type=Path);parser.add_argument('output',type=Path);args=parser.parse_args()
    if args.output.exists():raise ValueError('Output directory must be new')
    data=args.dll.read_bytes()
    if hashlib.sha256(data).hexdigest()!=SHA256:raise ValueError('Unsupported sim1 fingerprint')
    pe=pefile.PE(data=data);base=pe.OPTIONAL_HEADER.ImageBase
    if base!=0x20AD0000 or pe.FILE_HEADER.Machine!=0x14C:raise ValueError('Unexpected PE profile')
    image=pe.get_memory_mapped_image()
    ptr=struct.unpack('<I',pe.get_data(0x20AD5220+172*4-base,4))[0]
    if pe.get_data(ptr-base,64).split(b'\0')[0]!=b'AVIONICS MASTER SWITCH':raise ValueError('Unexpected original SimVar identity')
    powers=[];tails=[]
    for raw,mode in [(True,0),(False,0),(False,1),(False,2)]:
        for voltage,maximum,forced,override,present,bus,master in itertools.product([0.,16.999,17.,math.nextafter(17.,math.inf),28.],[0.,24.],[False,True],[False,True],[False,True],[0,8],[False,True]):
            context={'context_flag':raw,'mode_query':mode,'battery_master':master,'max_battery_voltage':maximum,'override_power':override}
            powers.append(power_case(image,base,context,{'bus':bus,'min_voltage':17.,'forced_off':forced} if present else None,voltage))
    for main_voltage,failed,avionics,voltage_mode,master,override in itertools.product([0.,16.999,17.,math.nextafter(17.,math.inf),28.],[False,True],[False,True],[False,True],[False,True],[False,True]):
        context={'context_flag':voltage_mode,'mode_query':0,'battery_master':master,'max_battery_voltage':24.,'override_power':override}
        tails.append(tail_case(image,base,context,failed,avionics,main_voltage))
    context={'context_flag':True,'mode_query':0,'battery_master':True,'max_battery_voltage':24.,'override_power':False}
    for missing_av,missing_nav in itertools.product([False,True],repeat=2):tails.append(tail_case(image,base,context,False,True,28.,missing_av=missing_av,missing_nav=missing_nav))
    tails.append(tail_case(image,base,context,False,True,17.,circuit_av={'bus':1,'min_voltage':17.,'forced_off':False}))
    frames=[frame_sequence(image,base,'avionics_toggle',22.,[(True,True,False,False)]*12+[(True,False,False,False)]*10+[(True,True,False,False)]*12),
        frame_sequence(image,base,'battery_failure_recovery',22.,[(True,True,False,False)]*6+[(True,True,True,False)]*6+[(True,True,False,False)]*8),
        frame_sequence(image,base,'strict_threshold',17.,[(False,True,False,False)]*6),
        frame_sequence(image,base,'override_with_empty_battery',0.,[(False,True,False,True)]*8)]
    overrides=[override_sequence(image,base,initial,[None,True,None,False,None,False,True,None]) for initial in (False,True)]
    args.output.mkdir()
    report={'schema':3,'source':'bounded-installed-sim1-power-tail','sha256':SHA256,'file_version':'10.0.62615.0','power_cases':powers,'tail_cases':tails,'frame_cases':frames,'override_sequences':overrides,
        'scope':'original predicate/tail and controlled one-generator/two-consumer frames plus CFG global-override lookup/retention; context/failure/avionics/response and CFG service injected; no startup override or equipment-presence inference','game_launched':False,'guarded_instruction_ranges':True,'guarded_writes':True,'instruction_budget_per_case':4096}
    (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    print(f'PASS: {len(powers)} original predicates, {len(tails)} tails, {sum(len(c["frames"]) for c in frames)} controlled frames, {sum(len(c["loads"]) for c in overrides)} CFG override loads; strict threshold, bypasses, failure/current/load lag verified')


if __name__=='__main__':main()
