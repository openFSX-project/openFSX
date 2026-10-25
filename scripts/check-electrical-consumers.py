"""Original ordered load sum, including raw activity words and ignored slot 0.
Reads pinned sim1 in guarded x86 emulation; no original process or bytes exported.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import pefile
from unicorn.x86_const import UC_X86_REG_ECX,UC_X86_REG_ESP
_spec=importlib.util.spec_from_file_location('tail',Path(__file__).with_name('check-electrical-tail.py'))
_tail=importlib.util.module_from_spec(_spec);_spec.loader.exec_module(_tail)

def run(image,base,records):
    t=_tail;m=t.Machine(image,base,{'context_flag':True,'mode_query':0,'battery_master':True,'max_battery_voltage':24.,'override_power':False})
    m.ranges += [(0x20B24ED0,0x20B24F3C)]
    for i,c in enumerate(records):
        if c['max_amps'] is not None:
            ptr=t.CONFIG+0x20+i*20;m.real(ptr,c['max_amps']);m.word(t.STATE+0xB4+i*12,ptr)
        else:m.word(t.STATE+0xB4+i*12,0)
        m.word(t.STATE+0xAC+i*12,c['active'])
    m.real(t.STATE+0xA4,999.)
    sp=t.STACK+0xF000;m.word(sp,t.SENTINEL);m.vm.reg_write(UC_X86_REG_ESP,sp);m.vm.reg_write(UC_X86_REG_ECX,t.STATE)
    m.execute(0x20B24ED0,sp+4)
    expected=0.
    for c in records[1:]:
        if c['active']:expected+=c['max_amps']
    actual=m.f64(t.STATE+0xA4)
    if actual!=expected:raise ValueError('Original ordered sum differs')
    return {'consumers':records,'load_amps':actual}

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('dll',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
    if a.output.exists():raise ValueError('Output must be new')
    data=a.dll.read_bytes()
    if hashlib.sha256(data).hexdigest()!=_tail.SHA256:raise ValueError('Unsupported sim1 fingerprint')
    pe=pefile.PE(data=data);image=pe.get_memory_mapped_image();base=pe.OPTIONAL_HEADER.ImageBase
    cases=[]
    for slot in range(34):
        for flag in (0,1,2,0xFFFFFFFF):
            records=[{'active':0,'max_amps':None} for _ in range(34)]
            records[slot]={'active':flag,'max_amps':7.25}
            cases.append(run(image,base,records))
    for seed in range(20):
        records=[{'active':0 if (i+seed)%3==0 else (2 if i%2 else 1),'max_amps':(1e6 if i%5==0 else (1e-10 if i%5==1 else (i+seed)/8.))} for i in range(34)]
        cases.append(run(image,base,records))
    a.output.mkdir();(a.output/'report.json').write_text(json.dumps({'schema':1,'source':'bounded-original-electrical-consumers','sha256':_tail.SHA256,'cases':cases,'scope':'original ordered slots 1..33 with explicit prior activity/metadata; slot0 ignored; no aircraft presence/default or activity-generation inference'},indent=2)+'\n',encoding='utf-8')
    print(f'PASS: {len(cases)} original ordered consumer sums; slot 0 ignored, raw nonzero activity and sequential rounding')

if __name__=='__main__':main()
