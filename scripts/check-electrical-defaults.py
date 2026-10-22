"""Bounded, version-pinned x86 reference check; never launches or loads FSX.

Requires pefile and unicorn. Reads the user's installed sim1.dll into emulated
memory and stops after its three scalar CFG reads. Only allocation and scalar
CFG lookup are stubbed; original x87 initialization/dispatch executes unchanged.
No original instructions or assets are embedded or written to the repository.
"""
import argparse
import hashlib
import json
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dll", type=Path)
    parser.add_argument("output", type=Path, help="new directory")
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
