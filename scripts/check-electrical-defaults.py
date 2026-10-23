"""Bounded, version-pinned x86 reference check; never launches or loads FSX.

Requires pefile and unicorn. Reads the user's installed sim1.dll into emulated
memory. The default mode stops after three scalar CFG reads, stubbing allocation
and lookup. --availability executes pre-load admission with failure/auxiliary
services stubbed. --engine-signal traces named rated RPM through reconstruction
and FLT save. --currents checks allocation and a separate current sum. Original
x87 branches execute unchanged in each mode.
No original instructions or assets are embedded or written to the repository.
"""
import argparse
import hashlib
import json
import itertools
import math
from pathlib import Path
import struct

import pefile
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW, UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_EBX

SHA256 = "4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e"
START, STOP = 0x20B252E0, 0x20B253EA
CTOR, CTOR_END = 0x20B24C20, 0x20B24C59
OBJECT, ALLOCATION, STACK = 0x100000, 0x110000, 0x200000
KEYS = ["max_battery_voltage", "generator_alternator_voltage", "max_generator_alternator_amps"]


def run(image, base, engine_type, engine_count, overrides):
    vm = Uc(UC_ARCH_X86, UC_MODE_32)
    vm.mem_map(base, (len(image) + 4095) & ~4095)
    vm.mem_write(base, image)
    for address, size in [(OBJECT, 0x3000), (ALLOCATION, 0x1000), (STACK, 0x10000)]:
        vm.mem_map(address, size)
    vm.mem_write(OBJECT + 0x454, struct.pack("<I", engine_count))
    vm.mem_write(OBJECT + 0x46C, bytes([engine_type]))
    vm.reg_write(UC_X86_REG_ECX, OBJECT)
    vm.reg_write(UC_X86_REG_EDX, 0x12345678)  # opaque CFG handle; never dereferenced
    vm.reg_write(UC_X86_REG_ESP, STACK + 0xF000)
    vm.reg_write(UC_X86_REG_FPCW, 0x37F)
    calls = []
    allocation_calls = 0

    def u32(address):
        return struct.unpack("<I", vm.mem_read(address, 4))[0]

    def string(address):
        data = bytes(vm.mem_read(address, 128))
        if b"\0" not in data:
            raise ValueError("Unbounded reference string")
        return data.split(b"\0", 1)[0].decode("ascii")

    def writes(_vm, _access, address, size, _value, _data):
        if not any(lo <= address and address + size <= hi for lo, hi in [
            (OBJECT, OBJECT+0x3000), (ALLOCATION, ALLOCATION+0x1000), (STACK, STACK+0x10000)
        ]):
            raise ValueError("Reference wrote outside guarded mutable regions")

    def instruction(_vm, address, _size, _data):
        nonlocal allocation_calls
        if not (START <= address < STOP or CTOR <= address < CTOR_END):
            raise ValueError("Reference left whitelisted instruction ranges")
        sp = vm.reg_read(UC_X86_REG_ESP)
        if address == 0x20B25328:
            args = [u32(sp+i*4) for i in range(4)]
            if allocation_calls or args[:2] != [1, 0x2C8] or not STACK <= args[2] < STACK+0x10000:
                raise ValueError("Unexpected allocator ABI")
            allocation_calls += 1
            vm.mem_write(args[2], struct.pack("<I", ALLOCATION))
            vm.reg_write(UC_X86_REG_EAX, 0)
            vm.reg_write(UC_X86_REG_ESP, sp+16)
            vm.reg_write(UC_X86_REG_EIP, address+6)
        elif address in [0x20B253C0, 0x20B253D4, 0x20B253E8]:
            section, key, out, handle, flags = [u32(sp+i*4) for i in range(5)]
            name = string(key)
            if string(section) != "Electrical" or name != KEYS[len(calls)] or handle != 0x12345678 or flags != 0x24:
                raise ValueError(f"Unexpected CFG scalar ABI/key/order: {string(section)!r}, {name!r}, {handle:#x}, {flags:#x}")
            if out != ALLOCATION + [8, 16, 24][len(calls)]:
                raise ValueError("Unexpected scalar destination")
            initial = struct.unpack("<d", vm.mem_read(out, 8))[0]
            if name in overrides:
                vm.mem_write(out, struct.pack("<d", overrides[name]))
            calls.append({"key": name, "initialized": initial, "authored": overrides.get(name)})
            vm.reg_write(UC_X86_REG_EAX, int(name in overrides))
            vm.reg_write(UC_X86_REG_ESP, sp+20)
            vm.reg_write(UC_X86_REG_EIP, address+2)

    vm.hook_add(UC_HOOK_CODE, instruction)
    vm.hook_add(UC_HOOK_MEM_WRITE, writes)
    vm.emu_start(START, STOP, count=4096)
    if vm.reg_read(UC_X86_REG_EIP) != STOP or len(calls) != 3 or allocation_calls != 1:
        raise ValueError("Reference did not reach the bounded scalar-read endpoint")
    values = dict(zip(KEYS, struct.unpack("<3d", vm.mem_read(ALLOCATION+8,24))))
    return {"engine_type": engine_type, "engine_count": engine_count, "lookups": calls, "effective": values}


def run_availability(image, base, enabled, switch, failed, signal, master, nominal=28., previously_online=False):
    """Execute original pre-load generator admission. The RPM fraction mapping
    is verified separately by run_engine_signal; this is not an RPM provider.
    Failure query returns the injected boolean; auxiliary source returns zero.
    """
    start, end, sentinel, stub = 0x20B24D80, 0x20B24EC9, 0x220100, 0x220000
    state, root, config, engine, failure, auxiliary, generator = 0x110000, 0x101000, 0x102000, 0x103000, 0x104000, 0x105000, 0x106000
    vm = Uc(UC_ARCH_X86, UC_MODE_32)
    vm.mem_map(base, (len(image)+4095) & ~4095); vm.mem_write(base,image)
    for address,size in [(OBJECT,0x10000),(state,0x1000),(STACK,0x10000),(stub,0x1000)]: vm.mem_map(address,size)
    # Original CPU instructions are read only from the pinned image. This tiny
    # synthetic auxiliary-service stub returns x87 zero, then returns normally.
    vm.mem_write(stub,b"\xd9\xee\xc3")
    def word(address,value): vm.mem_write(address,struct.pack("<I",value))
    def real(address,value): vm.mem_write(address,struct.pack("<d",value))
    def readword(address): return struct.unpack("<I",vm.mem_read(address,4))[0]
    def readreal(address): return struct.unpack("<d",vm.mem_read(address,8))[0]
    word(OBJECT,root); word(root+0x454,1)
    word(OBJECT+0x43C,engine); word(OBJECT+0x440,auxiliary)
    word(state,config); word(config,int(enabled)); real(config+0x10,nominal)
    real(state+0x94,24.); word(state+0x244,int(master)); word(state+0x84,generator)
    word(engine+0x8C,int(switch)); word(engine+0x90,failure); real(engine+0x5C,signal)
    if previously_online:
        word(engine+0x94,1);word(state+0x248,1);real(generator,28.)
    word(failure,failure+0x100); word(auxiliary,auxiliary+0x100); word(auxiliary+0x118,stub)
    sp=STACK+0xF000; word(sp,sentinel); word(sp+4,OBJECT)
    vm.reg_write(UC_X86_REG_ESP,sp); vm.reg_write(UC_X86_REG_ECX,state); vm.reg_write(UC_X86_REG_FPCW,0x37F)
    failures=0
    def instruction(_vm,address,_size,_data):
        nonlocal failures
        if not (start<=address<end or stub<=address<stub+3): raise ValueError("Availability reference left instruction whitelist")
        if address==0x20B24E16:
            sp=vm.reg_read(UC_X86_REG_ESP)
            if readword(sp)!=failure: raise ValueError("Unexpected failure-service ABI")
            failures+=1
            vm.reg_write(UC_X86_REG_EAX,int(failed)); vm.reg_write(UC_X86_REG_ESP,sp+4); vm.reg_write(UC_X86_REG_EIP,address+3)
    def writes(_vm,_access,address,size,_value,_data):
        if not any(lo<=address and address+size<=hi for lo,hi in [(OBJECT,OBJECT+0x10000),(state,state+0x1000),(STACK,STACK+0x10000)]):
            raise ValueError("Availability reference wrote outside mutable regions")
    vm.hook_add(UC_HOOK_CODE,instruction); vm.hook_add(UC_HOOK_MEM_WRITE,writes)
    vm.emu_start(start,sentinel,count=4096)
    if vm.reg_read(UC_X86_REG_EIP)!=sentinel or vm.reg_read(UC_X86_REG_ESP)!=sp+8:
        raise ValueError("Availability reference did not return with expected ABI")
    if failures!=int(nominal>0. and enabled and switch): raise ValueError("Unexpected failure-service query count")
    return {"enabled":enabled,"switch":switch,"failed":failed,"rpm_fraction":signal,"battery_master":master,"nominal_voltage":nominal,
        "previously_online":previously_online,"online":bool(readword(state+0x248)),"engine_active_flag":readword(engine+0x94),"active_count":readword(state+0x248),"generator_voltage":readreal(generator),
        "pre_load_main_voltage":readreal(state+4),"battery_bus_voltage":readreal(state+0x24),"hot_battery_voltage":readreal(state+0x34)}


def run_engine_signal(image, base, rated_rpm, fraction, rotation):
    """Trace original named CFG input -> RPM reconstruction -> named FLT field.
    No engine dynamics or general SimVar-dispatch path is substituted/claimed.
    """
    vm=Uc(UC_ARCH_X86,UC_MODE_32)
    vm.mem_map(base,(len(image)+4095)&~4095);vm.mem_write(base,image)
    vm.mem_map(OBJECT,0x10000);vm.mem_map(STACK,0x10000);vm.mem_map(0x220000,0x1000)
    root,engine,piston,propeller=0x101000,0x103000,0x104000,0x105000
    record,sentinel=0x20B98E50,0x220100
    def word(a,v):vm.mem_write(a,struct.pack('<I',v))
    def real(a,v):vm.mem_write(a,struct.pack('<d',v))
    def readword(a):return struct.unpack('<I',vm.mem_read(a,4))[0]
    def readreal(a):return struct.unpack('<d',vm.mem_read(a,8))[0]
    def string(a):return bytes(vm.mem_read(a,128)).split(b'\0',1)[0].decode('ascii')
    # Validate the original FLT descriptor, not a reconstructed name/offset.
    name,offset=struct.unpack('<2I',vm.mem_read(0x20B94E88,8))
    if string(name)!='Pct Engine RPM' or offset!=0x18:raise ValueError('Unexpected original FLT RPM descriptor')
    switch_name,switch_offset=struct.unpack('<2I',vm.mem_read(0x20B94EE8,8))
    if string(switch_name)!='GeneratorSwitch' or switch_offset!=0x30:raise ValueError('Unexpected original FLT switch descriptor')
    word(OBJECT,root);word(root+0x454,1);word(root+0x46C,0)
    word(OBJECT+0x43C,engine);word(OBJECT+0x430,piston);word(OBJECT+0x438,propeller)
    real(propeller+8,fraction);real(propeller+0x44,rotation);word(engine+0x8C,1)
    sp=STACK+0xF000;word(sp,sentinel);word(sp-0x70,0)
    vm.reg_write(UC_X86_REG_ESP,sp-0x200);vm.reg_write(UC_X86_REG_EBP,sp);vm.reg_write(UC_X86_REG_FPCW,0x37F)
    lookups=[]
    ranges=[(0x20B4E406,0x20B4E444),(0x20B4C45E,0x20B4C476),(0x20B6C19C,0x20B6C1FC),(0x20B3E6B0,0x20B3E799)]
    def instruction(_vm,address,_size,_data):
        if not any(lo<=address<hi for lo,hi in ranges):raise ValueError('Engine trace left instruction whitelist')
        if address==0x20B4E420:
            esp=vm.reg_read(UC_X86_REG_ESP)
            section,key,out,handle,flags=[readword(esp+i*4) for i in range(5)]
            if string(section).lower()!='piston_engine' or string(key)!='max_rated_rpm' or out!=sp-12 or handle!=0x12345678 or flags!=0x24:
                raise ValueError(f'Unexpected rated RPM CFG lookup ABI: {string(section)!r}, {string(key)!r}, {out:#x}, {handle:#x}, {flags:#x}; expected out {sp-12:#x}')
            real(out,rated_rpm);lookups.append(string(key))
            vm.reg_write(UC_X86_REG_EAX,1);vm.reg_write(UC_X86_REG_ESP,esp+20);vm.reg_write(UC_X86_REG_EIP,address+2)
    def writes(_vm,_access,address,size,_value,_data):
        if not any(lo<=address and address+size<=hi for lo,hi in [(OBJECT,OBJECT+0x10000),(STACK,STACK+0x10000),(record,record+0x48)]):
            raise ValueError('Engine trace wrote outside guarded buffers')
    vm.hook_add(UC_HOOK_CODE,instruction);vm.hook_add(UC_HOOK_MEM_WRITE,writes)
    def phase(start,stop):
        vm.emu_start(start,stop,count=4096)
        if vm.reg_read(UC_X86_REG_EIP)!=stop:raise ValueError('Engine trace instruction budget exhausted')
    vm.reg_write(UC_X86_REG_EDI,root);vm.reg_write(UC_X86_REG_EBX,0x12345678)
    phase(0x20B4E406,0x20B4E444)
    vm.reg_write(UC_X86_REG_ESI,OBJECT)
    phase(0x20B4C45E,0x20B4C476)
    vm.reg_write(UC_X86_REG_EDI,OBJECT)
    phase(0x20B6C19C,0x20B6C1FC)
    vm.reg_write(UC_X86_REG_ECX,OBJECT);vm.reg_write(UC_X86_REG_ESP,sp);word(sp,sentinel)
    phase(0x20B3E6B0,sentinel)
    runtime_fraction=readreal(engine+0x5C)
    expected=math.copysign(fraction,rotation) if fraction else 0.
    if lookups!=['max_rated_rpm'] or readreal(root+0x178)!=rated_rpm or readreal(OBJECT+0x61C)!=rated_rpm or runtime_fraction!=expected or readreal(engine+0x4C)!=expected*rated_rpm or readreal(record+offset)!=expected or readword(record+switch_offset)!=1:
        raise ValueError('Named original engine/FLT ratio trace mismatch')
    admission=run_availability(image,base,True,True,False,runtime_fraction,False)
    if admission['online']!=(expected>0.1):raise ValueError('Reconstructed ratio did not match original admission')
    return {'max_rated_rpm':rated_rpm,'input_fraction':fraction,'rotation':rotation,'runtime_rpm':readreal(engine+0x4C),
        'runtime_rpm_fraction':runtime_fraction,'flt_field':string(name),'flt_fraction':readreal(record+offset),'saved_generator_switch':True,'generator_admitted':admission['online']}


def run_currents(image,base,active,capacity,max_battery,battery_voltage,load,active_count=None):
    """Original allocation and a separate current-sum invocation. This does not
    change the original frame's ordering or emulate battery integration.
    """
    vm=Uc(UC_ARCH_X86,UC_MODE_32)
    vm.mem_map(base,(len(image)+4095)&~4095);vm.mem_write(base,image)
    state,root,config,engine,auxiliary,slots=0x110000,0x101000,0x102000,0x103000,0x105000,0x106000
    stub,sentinel=0x220000,0x220100
    for address,size in [(OBJECT,0x10000),(state,0x1000),(STACK,0x10000),(stub,0x1000)]:vm.mem_map(address,size)
    vm.mem_write(stub,b'\xd9\xee\xc3')  # own auxiliary service: x87 zero, ret
    def word(a,v):vm.mem_write(a,struct.pack('<I',v))
    def real(a,v):vm.mem_write(a,struct.pack('<d',v))
    def readreal(a):return struct.unpack('<d',vm.mem_read(a,8))[0]
    word(OBJECT,root);word(root+0x454,len(active));word(root+0x19B4,config)
    word(OBJECT+0x43C,engine);word(OBJECT+0x440,auxiliary)
    word(auxiliary,auxiliary+0x100);word(auxiliary+0x118,stub)
    word(state,config);real(config+8,max_battery);real(config+0x18,capacity)
    count=sum(active) if active_count is None else active_count
    real(state+0x94,battery_voltage);real(state+0xA4,load);word(state+0x248,count)
    for i,on in enumerate(active):
        word(engine+i*0xEC+0x94,int(on));word(state+0x84+i*4,slots+i*16)
        real(slots+i*16+8,123.)  # proves inactive slots are explicitly cleared
    sp=STACK+0xF000
    vm.reg_write(UC_X86_REG_FPCW,0x37F)
    ranges=[(0x20B24FC0,0x20B2507A),(0x20B24F50,0x20B24FB3),(stub,stub+3)]
    def instruction(_vm,address,_size,_data):
        if not any(lo<=address<hi for lo,hi in ranges):raise ValueError('Current reference left instruction whitelist')
    def writes(_vm,_access,address,size,_value,_data):
        if not any(lo<=address and address+size<=hi for lo,hi in [(OBJECT,OBJECT+0x10000),(state,state+0x1000),(STACK,STACK+0x10000)]):
            raise ValueError('Current reference wrote outside mutable regions')
    vm.hook_add(UC_HOOK_CODE,instruction);vm.hook_add(UC_HOOK_MEM_WRITE,writes)
    for start in [0x20B24FC0,0x20B24F50]:
        word(sp,sentinel);word(sp+4,OBJECT)
        vm.reg_write(UC_X86_REG_ESP,sp);vm.reg_write(UC_X86_REG_ECX,state)
        vm.emu_start(start,sentinel,count=4096)
        if vm.reg_read(UC_X86_REG_EIP)!=sentinel or vm.reg_read(UC_X86_REG_ESP)!=sp+8:
            raise ValueError('Current reference did not return with expected ABI')
    currents=[readreal(slots+i*16+8) for i in range(len(active))]
    expected=[min(capacity,(load+(max_battery-battery_voltage))/count) if on and count else 0. for on in active]
    if currents!=expected or readreal(state+12)!=sum(expected):raise ValueError('Unexpected original current allocation/sum')
    return {'active':active,'active_count_input':count,'capacity_amps':capacity,'max_battery_voltage':max_battery,'battery_voltage':battery_voltage,'load_amps':load,
        'allocated_generator_amps':currents,'sum_after_explicit_resum':readreal(state+12)}


def run_voltage_core(image,base,scenario):
    """Original frame prefix through voltage storage, before failure/avionics.
    One explicit consumer is active; no equipment-presence model is invented.
    The response factor's source/physical meaning is not established here.
    """
    vm=Uc(UC_ARCH_X86,UC_MODE_32)
    vm.mem_map(base,(len(image)+4095)&~4095);vm.mem_write(base,image)
    state,root,config,engine,failure,auxiliary=0x110000,0x101000,0x102000,0x103000,0x104000,0x105000
    context,factor_owner,factor_data,stub=0x107000,0x108000,0x109000,0x220000
    for address,size in [(OBJECT,0x10000),(state,0x1000),(STACK,0x10000),(stub,0x1000)]:vm.mem_map(address,size)
    vm.mem_write(stub,b'\xd9\xee\xc3')
    def word(a,v):vm.mem_write(a,struct.pack('<I',v))
    def real(a,v):vm.mem_write(a,struct.pack('<d',v))
    def readword(a):return struct.unpack('<I',vm.mem_read(a,4))[0]
    def readreal(a):return struct.unpack('<d',vm.mem_read(a,8))[0]
    word(OBJECT,root);word(root+0x454,1);word(root+0x19B4,config)
    word(OBJECT+0x43C,engine);word(OBJECT+0x440,auxiliary)
    word(auxiliary,auxiliary+0x100);word(auxiliary+0x118,stub)
    word(context+0x270,OBJECT);word(context+0x2D8,factor_owner);word(factor_owner,factor_data)
    cfg=scenario.get('configuration',{'max_battery_voltage':24.,'generator_voltage':28.,'max_generator_amps':60.})
    maximum,nominal,capacity=cfg['max_battery_voltage'],cfg['generator_voltage'],cfg['max_generator_amps']
    word(state,config);word(config,1);real(config+8,maximum);real(config+0x10,nominal);real(config+0x18,capacity)
    word(state+0x84,state+0x44);word(engine+0x90,failure);word(failure,failure+0x100)
    word(state+0xB8,1);word(state+0xC0,config+0x20)
    initial=scenario['initial'];real(state+0x94,initial['battery_voltage']);real(state+0x9C,initial['filtered_net_amps']);real(state+0x4C,initial['generator_amps'])
    ranges=[(0x20B24C60,0x20B24D1F),(0x20B24D80,0x20B24EC9),(0x20B24ED0,0x20B24F3C),(0x20B24F50,0x20B24FB3),(0x20B24FC0,0x20B2507A),(stub,stub+3)]
    def instruction(_vm,address,_size,_data):
        if not any(lo<=address<hi for lo,hi in ranges):raise ValueError('Voltage core left instruction whitelist')
        if address==0x20B24E16:
            sp=vm.reg_read(UC_X86_REG_ESP)
            if readword(sp)!=failure:raise ValueError('Unexpected voltage-core failure ABI')
            vm.reg_write(UC_X86_REG_EAX,0);vm.reg_write(UC_X86_REG_ESP,sp+4);vm.reg_write(UC_X86_REG_EIP,address+3)
    def writes(_vm,_access,address,size,_value,_data):
        if not any(lo<=address and address+size<=hi for lo,hi in [(OBJECT,OBJECT+0x10000),(state,state+0x1000),(STACK,STACK+0x10000)]):raise ValueError('Voltage core wrote outside guarded objects')
    vm.hook_add(UC_HOOK_CODE,instruction);vm.hook_add(UC_HOOK_MEM_WRITE,writes)
    vm.reg_write(UC_X86_REG_FPCW,0x37F)
    records=[]
    for frame in scenario['inputs']:
        before={'battery_voltage':readreal(state+0x94),'filtered_net_amps':readreal(state+0x9C),'generator_amps':readreal(state+0x4C)}
        online=frame['generator_online'] and nominal>0.;dt=frame['dt'];load=frame['load_amps']
        factor=struct.unpack('<f',struct.pack('<f',frame['response_factor']))[0]
        word(engine+0x8C,int(online));real(engine+0x5C,0.9 if online else 0.)
        word(state+0x244,int(frame['battery_master']));real(config+0x20,load)
        vm.mem_write(factor_data+0x38,struct.pack('<f',factor))
        sp=STACK+0xF000;word(sp,0);word(sp+4,context);real(sp+8,dt)
        vm.reg_write(UC_X86_REG_ESP,sp);vm.reg_write(UC_X86_REG_ECX,state)
        vm.emu_start(0x20B24C60,0x20B24D1F,count=10000)
        if vm.reg_read(UC_X86_REG_EIP)!=0x20B24D1F or vm.reg_read(UC_X86_REG_ESP)!=sp-16:raise ValueError('Voltage prefix did not reach expected boundary')
        filtered=before['filtered_net_amps']+(before['generator_amps']-load-before['filtered_net_amps'])*4.*dt
        intermediate=max(0.,before['battery_voltage']+filtered*0.000555*factor*dt)
        allocated=min(capacity,load+(maximum-intermediate)) if online else 0.
        voltage=min(maximum,intermediate+before['generator_amps']*0.0004*dt)
        actual={'battery_voltage':readreal(state+0x94),'filtered_net_amps':readreal(state+0x9C),'generator_amps':readreal(state+0x4C)}
        for key,expected in [('battery_voltage',voltage),('filtered_net_amps',filtered),('generator_amps',allocated)]:
            if abs(actual[key]-expected)>1e-10:raise ValueError(f'Original voltage core mismatch: {key}, {actual[key]}, {expected}')
        battery_bus=before['battery_voltage'] if frame['battery_master'] else 0.
        main=min(nominal,battery_bus+(nominal if online else 0.)) if nominal>0. else battery_bus
        if readreal(state+4)!=main or readreal(state+0x24)!=battery_bus or readreal(state+0x34)!=before['battery_voltage'] or readreal(state+12)!=before['generator_amps']:raise ValueError('Original pre-load/prior-current ordering mismatch')
        records.append({'input':dict(frame,response_factor=factor),'before':before,'state':actual,'pre_load_main_voltage':readreal(state+4),
            'pre_load_battery_voltage':readreal(state+0x24),'pre_load_hot_battery_voltage':readreal(state+0x34),'source_sum_used_amps':readreal(state+12)})
    return {'name':scenario['name'],'configuration':cfg,'initial':initial,'frames':records}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dll", type=Path)
    parser.add_argument("output", type=Path, help="new directory")
    modes=parser.add_mutually_exclusive_group()
    modes.add_argument("--availability", action="store_true", help="check original pre-load generator admission instead of scalar defaults")
    modes.add_argument("--engine-signal", action="store_true", help="trace named rated RPM through original engine reconstruction and FLT save")
    modes.add_argument("--currents", action="store_true", help="check original generator current allocation and a separate zero-auxiliary current sum")
    modes.add_argument("--voltage-core", action="store_true", help="check original frame prefix through voltage storage, excluding failure/avionics tail")
    args = parser.parse_args()
    if args.output.exists():
        raise ValueError("Output directory must be new")
    data = args.dll.read_bytes()
    if hashlib.sha256(data).hexdigest() != SHA256:
        raise ValueError("Unsupported sim1.dll fingerprint; no offsets/defaults assumed")
    pe = pefile.PE(data=data)
    base = pe.OPTIONAL_HEADER.ImageBase
    if base != 0x20AD0000 or pe.FILE_HEADER.Machine != 0x14C:
        raise ValueError("Unexpected reference PE profile")
    image = pe.get_memory_mapped_image()
    cases = []
    if args.voltage_core:
        def frame(online=False,load=12.,factor=1.,master=True,dt=1./120.):return {'generator_online':online,'load_amps':load,'response_factor':factor,'battery_master':master,'dt':dt}
        def scenario(name,voltage,filtered,current,inputs):return {'name':name,'initial':{'battery_voltage':voltage,'filtered_net_amps':filtered,'generator_amps':current},'inputs':inputs}
        scenarios=[scenario('discharge',24.,0.,0.,[frame()]*120),
            scenario('start_stop_lag',22.,0.,0.,[frame()]*10+[frame(True)]*30+[frame()]*10),
            scenario('zero_response',22.,0.,14.,[frame(True,factor=0.)]*20),
            scenario('empty_battery',0.,-12.,0.,[frame()]*20),
            scenario('full_charge_signed_allocation',24.,0.,60.,[frame(True,load=0.)]*30),
            scenario('overload',22.,0.,0.,[frame(True,load=120.)]*60),
            scenario('float32_response_master_off',22.,0.,12.,[frame(True,factor=0.1,master=False)]*30),
            scenario('double_response',20.,-6.,0.,[frame(factor=2.,dt=1./60.)]*30)]
        battery_only=scenario('zero_generator_rating',24.,0.,0.,[frame()]*20)
        battery_only['configuration']={'max_battery_voltage':24.,'generator_voltage':0.,'max_generator_amps':0.}
        scenarios.append(battery_only)
        cases=[run_voltage_core(image,base,s) for s in scenarios]
        args.output.mkdir()
        report={'schema':1,'source':'bounded-installed-sim1-voltage-core','sha256':SHA256,'file_version':'10.0.62615.0','cases':cases,
            'scope':'original core prefix through voltage storage, before battery-failure and avionics tail; one injected active consumer, zero auxiliary, no generator failure, explicit float32 response factor; not complete aircraft electrical simulation',
            'response_factor_physical_mapping_verified':False,'game_launched':False,'guarded_instruction_ranges':True,'guarded_writes':True,'instruction_budget_per_frame':10000}
        (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
        print(f'PASS: {len(cases)} original voltage-core sequences / {sum(len(c["frames"]) for c in cases)} frames; prior-current lag, filter, clamps and pre-load ordering; failure/avionics tail excluded')
        return
    if args.currents:
        for active in [(False,),(True,),(False,False),(True,False),(False,True),(True,True)]:
            for capacity,max_battery,battery,load in itertools.product([0.,60.],[12.,24.],[0.,20.,24.,30.],[0.,12.,60.,120.]):
                cases.append(run_currents(image,base,active,capacity,max_battery,battery,load))
        for stale_flags in [(True,),(True,False)]:
            cases.append(run_currents(image,base,stale_flags,60.,24.,22.,12.,active_count=0))
        args.output.mkdir()
        report={'schema':1,'source':'bounded-installed-sim1-generator-current-allocation','sha256':SHA256,'file_version':'10.0.62615.0','cases':cases,
            'scope':'allocation plus separately invoked current sum with zero auxiliary service; input load/active flags/battery voltage supplied; no full-frame timing or battery/load/voltage solution',
            'game_launched':False,'guarded_instruction_ranges':True,'guarded_writes':True,'instruction_budget_per_invocation':4096}
        (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
        print('PASS: 386 original allocation/sum cases; cap/gap, stale-slot and zero-active-count clearing, no lower clamp; no full-frame dynamics claim')
        return
    if args.engine_signal:
        for rated in [2400.,2700.,3000.]:
            for fraction in [0.,0.1,math.nextafter(0.1,math.inf),0.9,1.25]:
                for rotation in [-1.,1.]:cases.append(run_engine_signal(image,base,rated,fraction,rotation))
        args.output.mkdir()
        report={'schema':1,'source':'bounded-installed-sim1-engine-rpm-fraction-trace','sha256':SHA256,'file_version':'10.0.62615.0','cases':cases,
            'engine_signal_physical_mapping_verified':True,'mapping':'runtime RPM / piston_engine max_rated_rpm; saved as FLT Pct Engine RPM',
            'scope':'original named CFG lookup destination, rated-RPM copy, engine reconstruction and FLT save; numeric CFG service injected; no engine dynamics or full scenario restore',
            'game_launched':False,'guarded_instruction_ranges':True,'guarded_writes':True,'instruction_budget_per_phase':4096}
        (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
        print('PASS: 30 original engine reconstruction/FLT traces; generator signal is RPM / max_rated_rpm, with strict > 0.1 admission')
        return
    if args.availability:
        for enabled,switch,failed,master in itertools.product([False,True],repeat=4):
            for signal in [0.,0.099999,0.1,math.nextafter(0.1,math.inf),0.9]:
                case=run_availability(image,base,enabled,switch,failed,signal,master)
                online=enabled and switch and not failed and signal>0.1
                expected_voltage=28. if online else (24. if master else 0.)
                if case["online"]!=online or case["active_count"]!=int(online) or case["generator_voltage"]!=(28. if online else 0.) or case["pre_load_main_voltage"]!=expected_voltage or case["hot_battery_voltage"]!=24.:
                    raise ValueError(f"Unexpected availability result: {case}")
                cases.append(case)
        for master in [False,True]:
            case=run_availability(image,base,True,True,False,0.9,master,0.)
            if case["online"] or case["pre_load_main_voltage"]!=(24. if master else 0.): raise ValueError("Unexpected zero-rated source result")
            cases.append(case)
        for enabled,switch,failed,signal in [(True,True,False,0.9),(False,True,False,0.9),(True,False,False,0.9),(True,True,True,0.9),(True,True,False,0.1)]:
            case=run_availability(image,base,enabled,switch,failed,signal,False,previously_online=True)
            admitted=enabled and switch and not failed and signal>0.1
            if case["online"]!=admitted or case["pre_load_main_voltage"]!=(28. if admitted else 0.): raise ValueError("Unexpected previously-online admission result")
            cases.append(case)
        args.output.mkdir()
        report={"schema":2,"source":"bounded-installed-sim1-generator-admission","sha256":SHA256,"file_version":"10.0.62615.0","cases":cases,
            "scope":"pre-load admission only; synthetic RPM fraction/failure query; auxiliary source fixed zero; named RPM mapping exercised separately by --engine-signal; generation current and loaded voltage unverified",
            "game_launched":False,"rpm_mapping_exercised":False,"guarded_instruction_ranges":True,"guarded_writes":True,"instruction_budget_per_case":4096}
        (args.output/"report.json").write_text(json.dumps(report,indent=2)+"\n",encoding="utf-8")
        print("PASS: 87 original generator-admission cases; strict RPM fraction > 0.1, source/switch/failure gates and prior-online transitions; named mapping checked separately by --engine-signal")
        return
    # Broader matrix detects engine-count/type branch dependence. Runtime port
    # currently resolves only the single-piston profile, not these other engines.
    for kind, current in enumerate([60.,400.,0.,250.,60.,250.,60.]):
        for count in [0,1,4]:
            case = run(image, base, kind, count, {})
            expected = dict(zip(KEYS, [24., 28. if count else 0., current]))
            if case["effective"] != expected:
                raise ValueError(f"Unexpected reference defaults: {case}")
            cases.append(case)
    for overrides in [dict(zip(KEYS,[0.,0.,0.])), dict(zip(KEYS,[12.,13.8,45.]))]:
        case = run(image, base, 0, 1, overrides)
        if case["effective"] != overrides:
            raise ValueError("Authored CFG overrides were not preserved")
        cases.append(case)
    args.output.mkdir()
    report = {"schema":1,"source":"bounded-installed-sim1-scalar-initialization", "sha256":SHA256,
        "file_version":"10.0.62615.0", "cases":cases,"game_launched":False,
        "scope":"original initialization/dispatch only; stubbed allocator and numeric CFG lookup; no generator/battery dynamics or live bus voltage",
        "guarded_instruction_ranges":True,"guarded_writes":True,"instruction_budget_per_case":4096}
    (args.output/"report.json").write_text(json.dumps(report,indent=2)+"\n",encoding="utf-8")
    print("PASS: 23 bounded original scalar-initialization cases; single piston defaults 24 V / 28 V / 60 A; no live voltage claim")


if __name__ == "__main__":
    main()
