"""Original pitot thermal + icing function sim1 0x20B458B0..0x20B45B3E run to its return.
Explicit weather flags (+0x50/+0x54), owner field +0x14C, float32 temperature/ice state and
double filter state; no original process. Seeded sampling keeps the report reproducible.
"""
import argparse,hashlib,importlib.util,json,random,struct
from pathlib import Path
import pefile
from unicorn.x86_const import UC_X86_REG_ECX,UC_X86_REG_ESP,UC_X86_REG_EIP
_s=importlib.util.spec_from_file_location('tail',Path(__file__).with_name('check-electrical-tail.py'));t=importlib.util.module_from_spec(_s);_s.loader.exec_module(t)
F=lambda x:struct.unpack('<f',struct.pack('<f',x))[0]
def run(image,base,c):
    m=t.Machine(image,base,{'context_flag':True,'mode_query':0,'battery_master':True,'max_battery_voltage':24.,'override_power':False});v=m.vm
    m.ranges += [(0x20B458B0,0x20B45B3F)]
    owner,root,weather=0x109000,0x101000,0x10B000
    m.word(t.CONTEXT+0x270,owner);m.word(owner,root);m.word(owner+0x534,t.STATE)
    m.word(t.CONTEXT+0x18,weather);m.real(weather+0x40,c['environment_scalar'])
    m.word(weather+0x54,int(c['weather_54']));m.word(weather+0x50,int(c['weather_50']))
    m.real(owner+0x14C,c['field_14c']);m.real(owner+0x16C,c['filter_initial'])
    m.word(owner+0x20,int(c['requested']));v.mem_write(root+0x1244,struct.pack('<f',c['heater_rate']))
    v.mem_write(owner+0x4E4,struct.pack('<f',c['temperature_initial']));v.mem_write(owner+0x4E8,struct.pack('<f',c['ice_initial']))
    m.circuit(7,{'bus':0,'min_voltage':17.,'forced_off':False});m.buses([28. if c['power_available'] else 0.]*8)
    sp=t.STACK+0xF000;m.word(sp,t.SENTINEL);m.real(sp+4,c['input']);m.real(sp+12,c['dt'])
    v.reg_write(UC_X86_REG_ESP,sp);v.reg_write(UC_X86_REG_ECX,t.CONTEXT)
    v.emu_start(0x20B458B0,t.SENTINEL,count=4096)
    if v.reg_read(UC_X86_REG_EIP)!=t.SENTINEL:raise ValueError('Icing function did not return')
    r=dict(c);r['temperature']=struct.unpack('<f',v.mem_read(owner+0x4E4,4))[0];r['ice']=struct.unpack('<f',v.mem_read(owner+0x4E8,4))[0]
    r['filter']=m.f64(owner+0x16C);r['active']=bool(m.u32(t.STATE+0x100));return r
def sample(r):
    return {'environment_scalar':r.choice([-40.,-20.,0.,20.,100.]),'weather_54':r.random()<.8,'weather_50':r.random()<.7,
     'field_14c':r.choice([0.,19.99,20.,35.,100.]),'filter_initial':r.choice([0.,5.,50.,123.5]),'requested':r.random()<.5,
     'heater_rate':r.choice([0.,1.,2.]),'temperature_initial':F(r.choice([300.,455.7,491.9,492.,492.1,520.,559.])),
     'ice_initial':F(r.choice([0.,.3,.99,1.])),'power_available':r.random()<.8,'input':r.choice([0.,10.,50.,200.]),'dt':r.choice([1./120.,1./60.])}
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('dll',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
    if a.output.exists():raise ValueError('Output must be new')
    data=a.dll.read_bytes()
    if hashlib.sha256(data).hexdigest()!=t.SHA256:raise ValueError('Unsupported sim1 fingerprint')
    pe=pefile.PE(data=data);image=pe.get_memory_mapped_image();base=pe.OPTIONAL_HEADER.ImageBase
    rng=random.Random(20261028);cases=[run(image,base,sample(rng)) for _ in range(500)]
    for name,env,rate,req in [('freeze_then_heat',-40.,2.,False),('heat',-40.,2.,True)]:
        c=sample(rng);c.update(environment_scalar=env,weather_54=True,weather_50=True,field_14c=50.,heater_rate=rate,requested=req,power_available=True,temperature_initial=F(400.),ice_initial=0.,filter_initial=0.,input=60.)
        for frame in range(120):
            c['requested']=req if name=='heat' else frame>=60;c['input']=60.
            r=run(image,base,c);r['sequence']=name;r['frame']=frame;cases.append(r)
            c['temperature_initial']=r['temperature'];c['ice_initial']=r['ice'];c['filter_initial']=r['filter']
    a.output.mkdir();(a.output/'report.json').write_text(json.dumps({'schema':1,'source':'bounded-original-pitot-icing','sha256':t.SHA256,'cases':cases,'scope':'original function through return; explicit weather flags and +0x14C owner field; providers not established'},indent=1)+'\n',encoding='utf-8')
    print(f'PASS: {len(cases)} original icing runs')
if __name__=='__main__':main()
