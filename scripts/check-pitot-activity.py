"""Pinned original pitot activity prefix; stops before thermal/icing arithmetic.
Uses explicit switch/heater/context metadata; no live FSX process or bytes emitted.
"""
import argparse
import hashlib
import importlib.util
import itertools
import json
import math
from pathlib import Path
import struct
import pefile
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX,UC_X86_REG_ESP,UC_X86_REG_EIP
_spec=importlib.util.spec_from_file_location('tail',Path(__file__).with_name('check-electrical-tail.py'))
t=importlib.util.module_from_spec(_spec);_spec.loader.exec_module(t)

def case(image,base,requested,rate,voltage,voltage_mode,master,override,forced,present):
    context={'context_flag':voltage_mode,'mode_query':0,'battery_master':master,'max_battery_voltage':24.,'override_power':override}
    m=t.Machine(image,base,context);v=m.vm
    m.ranges += [(0x20B458B0,0x20B4590A),(0x20B4596F,0x20B45983)]
    owner,root=0x109000,0x101000
    m.word(t.CONTEXT+0x270,owner);m.word(owner,root);m.word(owner+0x534,t.STATE)
    m.word(owner+0x20,int(requested));v.mem_write(root+0x1244,struct.pack('<f',rate))
    circuit={'bus':0,'min_voltage':17.,'forced_off':forced} if present else None
    m.circuit(7,circuit);m.buses([voltage]*8);m.word(t.STATE+0x100,0xFFFFFFFF)
    def stop(vm,address,_size,_data):
        if address in (0x20B45909,0x20B45982):vm.emu_stop()
    v.hook_add(UC_HOOK_CODE,stop)
    sp=t.STACK+0xF000;m.word(sp,t.SENTINEL);m.real(sp+16,1./120.)
    v.reg_write(UC_X86_REG_ESP,sp);v.reg_write(UC_X86_REG_ECX,t.CONTEXT)
    v.emu_start(0x20B458B0,t.SENTINEL,count=4096)
    if v.reg_read(UC_X86_REG_EIP) not in (0x20B45909,0x20B45982):raise ValueError('Pitot prefix did not reach activity boundary')
    power=t.expected_power(context,circuit,[voltage]*8)
    active=bool(m.u32(t.STATE+0x100))
    if active!=(requested and rate>0. and power):raise ValueError('Original pitot activity differs')
    return {'requested':requested,'heater_rate':rate,'context':context,'circuit':circuit,'bus_voltage':[voltage]*8,'power_available':power,'active':active}

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('dll',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
    if a.output.exists():raise ValueError('Output must be new')
    data=a.dll.read_bytes()
    if hashlib.sha256(data).hexdigest()!=t.SHA256:raise ValueError('Unsupported sim1 fingerprint')
    pe=pefile.PE(data=data);image=pe.get_memory_mapped_image();base=pe.OPTIONAL_HEADER.ImageBase
    cases=[case(image,base,*inputs) for inputs in itertools.product((False,True),(-1.,0.,1.),(0.,17.,math.nextafter(17.,math.inf),28.),(False,True),(False,True),(False,True),(False,True),(False,True))]
    a.output.mkdir();(a.output/'report.json').write_text(json.dumps({'schema':1,'source':'bounded-original-pitot-activity','sha256':t.SHA256,'cases':cases,'scope':'original slot7 activity prefix only; injected switch/heater/context/circuit values; no thermal/icing/provider provenance or live process'},indent=2)+'\n',encoding='utf-8')
    print(f'PASS: {len(cases)} original pitot activity prefixes; request/rate/power gating, no thermal model claim')

if __name__=='__main__':main()
