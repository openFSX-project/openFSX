"""Bounded, version-pinned x86 reference check; never launches or loads FSX.

Requires pefile and unicorn. Reads the user's installed sim1.dll into emulated
memory. The default mode stops after three scalar CFG reads, stubbing allocation
and lookup. --availability executes pre-load admission with failure/auxiliary
services stubbed; original x87 branches execute unchanged in either mode.
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
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW

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
    """Execute original pre-load generator admission. The signal's physical
    mapping is deliberately unresolved; this does not claim an RPM provider.
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
    return {"enabled":enabled,"switch":switch,"failed":failed,"engine_signal":signal,"battery_master":master,"nominal_voltage":nominal,
        "previously_online":previously_online,"online":bool(readword(state+0x248)),"engine_active_flag":readword(engine+0x94),"active_count":readword(state+0x248),"generator_voltage":readreal(generator),
        "pre_load_main_voltage":readreal(state+4),"battery_bus_voltage":readreal(state+0x24),"hot_battery_voltage":readreal(state+0x34)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dll", type=Path)
    parser.add_argument("output", type=Path, help="new directory")
    parser.add_argument("--availability", action="store_true", help="check original pre-load generator admission instead of scalar defaults")
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
        report={"schema":1,"source":"bounded-installed-sim1-generator-admission","sha256":SHA256,"file_version":"10.0.62615.0","cases":cases,
            "scope":"pre-load admission only; synthetic engine signal/failure query; auxiliary source fixed zero; RPM mapping, generation current and loaded bus voltage unverified",
            "game_launched":False,"engine_signal_physical_mapping_verified":False,"guarded_instruction_ranges":True,"guarded_writes":True,"instruction_budget_per_case":4096}
        (args.output/"report.json").write_text(json.dumps(report,indent=2)+"\n",encoding="utf-8")
        print("PASS: 87 original generator-admission cases; strict signal > 0.1, source/switch/failure gates and prior-online transitions; physical signal mapping remains unresolved")
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
