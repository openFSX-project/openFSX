"""Original pitot thermal prefix through temperature storage, before icing.
Environment scalar/seed/rate/switch/power are explicit; no original process.
"""
import argparse,hashlib,importlib.util,itertools,json,struct
from pathlib import Path
import pefile
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ECX,UC_X86_REG_ESP,UC_X86_REG_EIP
_s=importlib.util.spec_from_file_location('tail',Path(__file__).with_name('check-electrical-tail.py'));t=importlib.util.module_from_spec(_s);_s.loader.exec_module(t)
def run(image,base,environment,initial,rate,requested,power,dt=1./120.):
    m=t.Machine(image,base,{'context_flag':True,'mode_query':0,'battery_master':True,'max_battery_voltage':24.,'override_power':False});v=m.vm
    m.ranges += [(0x20B458B0,0x20B45A0B)]
    owner,root,weather=0x109000,0x101000,0x10B000
    m.word(t.CONTEXT+0x270,owner);m.word(owner,root);m.word(owner+0x534,t.STATE)
    m.word(t.CONTEXT+0x18,weather);m.real(weather+0x40,environment)
    m.word(owner+0x20,int(requested));v.mem_write(root+0x1244,struct.pack('<f',rate));v.mem_write(owner+0x4E4,struct.pack('<f',initial))
    m.circuit(7,{'bus':0,'min_voltage':17.,'forced_off':False});m.buses([28. if power else 0.]*8)
    def stop(vm,address,_size,_data):
        if address==0x20B45A0A:vm.emu_stop()
    v.hook_add(UC_HOOK_CODE,stop)
    sp=t.STACK+0xF000;m.word(sp,t.SENTINEL);m.real(sp+12,dt)
    v.reg_write(UC_X86_REG_ESP,sp);v.reg_write(UC_X86_REG_ECX,t.CONTEXT)
    v.emu_start(0x20B458B0,t.SENTINEL,count=4096)
    if v.reg_read(UC_X86_REG_EIP)!=0x20B45A0A:raise ValueError('Thermal prefix did not reach boundary')
    active=bool(m.u32(t.STATE+0x100));temperature=struct.unpack('<f',v.mem_read(owner+0x4E4,4))[0]
    if active!=(requested and rate>0. and power):raise ValueError('Thermal activity differs')
    return {'environment_scalar':environment,'initial_temperature':initial,'heater_rate':rate,'requested':requested,'power_available':power,'dt':dt,'active':active,'temperature':temperature}
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('dll',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
    if a.output.exists():raise ValueError('Output must be new')
    data=a.dll.read_bytes()
    if hashlib.sha256(data).hexdigest()!=t.SHA256:raise ValueError('Unsupported sim1 fingerprint')
    pe=pefile.PE(data=data);image=pe.get_memory_mapped_image();base=pe.OPTIONAL_HEADER.ImageBase
    cases=[run(image,base,*args) for args in itertools.product((-100.,0.,20.,100.),(0.,491.7,559.99,600.),(0.,1.,2.),(False,True),(False,True))]
    for name,temperature,environment in [('heat_cool',550.,0.),('cold_start',0.,20.),('hot_environment',560.,100.),('supply_loss',500.,-20.)]:
        for frame in range(60):
            c=run(image,base,environment,temperature,2.,frame<40,frame<20 or frame>=40,(1./120. if frame%2 else 1./60.))
            c['sequence']=name;c['frame']=frame;cases.append(c);temperature=c['temperature']
    a.output.mkdir();(a.output/'report.json').write_text(json.dumps({'schema':1,'source':'bounded-original-pitot-thermal','sha256':t.SHA256,'cases':cases,'scope':'original thermal prefix before icing; explicit environment scalar, seeded float32 state, rate and supply; physical environment mapping/initial state provider not established'},indent=2)+'\n',encoding='utf-8')
    print(f'PASS: {len(cases)} original thermal prefixes; stops before icing')
if __name__=='__main__':main()
