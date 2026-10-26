"""Original AIR record lookup helper, with synthetic bounded file services.
No FSX process or filesystem writes; streams are independently authored fixtures.
"""
import argparse,hashlib,importlib.util,json,struct
from pathlib import Path
import pefile
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_EAX,UC_X86_REG_ECX,UC_X86_REG_EDX,UC_X86_REG_ESP,UC_X86_REG_EIP
_s=importlib.util.spec_from_file_location('tail',Path(__file__).with_name('check-electrical-tail.py'));t=importlib.util.module_from_spec(_s);_s.loader.exec_module(t)
def run(image,base,payload):
    m=t.Machine(image,base,{'context_flag':True,'mode_query':0,'battery_master':False,'max_battery_voltage':24.,'override_power':False});v=m.vm
    m.ranges += [(0x20AF1FC0,0x20AF20AC)]
    data=struct.pack('<II',0x123,3)+b'own'
    if payload is not None:data+=struct.pack('<II',0x324,len(payload))+payload
    cursor=0;closed=0;handle=0x1234;out=t.CONTEXT+0xF000
    def bounded(a,n):
        if not any(lo<=a and a+n<=hi for lo,hi in [(t.CONTEXT,t.CONTEXT+0x10000),(t.STACK,t.STACK+0x10000)]):raise ValueError('File service output outside bounded buffers')
    def hook(vm,address,_size,_data):
        nonlocal cursor,closed
        sp=vm.reg_read(UC_X86_REG_ESP)
        if address==0x20AF1FDB:
            dst,provider,mode,flags=[m.u32(sp+i*4) for i in range(4)]
            if (provider,mode,flags)!=(0x10E000,15,0):raise ValueError('Unexpected AIR open ABI')
            bounded(dst,4);m.word(dst,handle);pop,result,length=16,0,6
        elif address in (0x20AF200D,0x20AF2026,0x20AF2059,0x20AF207F):
            h,dst,size,actual=[m.u32(sp+i*4) for i in range(4)]
            if h!=handle or size>8:raise ValueError('Unexpected AIR read ABI/budget')
            bounded(dst,size);bounded(actual,4);chunk=data[cursor:cursor+size];cursor+=len(chunk)
            if chunk:vm.mem_write(dst,chunk)
            m.word(actual,len(chunk));pop,result,length=16,0,2
        elif address==0x20AF203B:
            h,delta,origin=[m.u32(sp+i*4) for i in range(3)]
            if h!=handle or origin!=1 or cursor+delta>len(data):raise ValueError('Unexpected AIR seek')
            cursor+=delta;pop,result,length=12,0,2
        elif address in (0x20AF2070,0x20AF2093):
            dst,value,size=[m.u32(sp+i*4) for i in range(3)]
            if value!=0 or size>8:raise ValueError('Unexpected AIR clear')
            bounded(dst,size);vm.mem_write(dst,bytes(size));pop,result,length=12,dst,6
        elif address==0x20AF209C:
            if m.u32(sp)!=handle:raise ValueError('Unexpected AIR close')
            closed+=1;pop,result,length=4,0,6
        else:return
        vm.reg_write(UC_X86_REG_EAX,result);vm.reg_write(UC_X86_REG_ESP,sp+pop);vm.reg_write(UC_X86_REG_EIP,address+length)
    v.hook_add(UC_HOOK_CODE,hook);v.mem_write(out,bytes([0xA5])*8)
    sp=t.STACK+0xF000;m.word(sp,t.SENTINEL);m.word(sp+4,out);m.word(sp+8,8)
    v.reg_write(UC_X86_REG_ESP,sp);v.reg_write(UC_X86_REG_ECX,0x10E000);v.reg_write(UC_X86_REG_EDX,0x324)
    m.execute(0x20AF1FC0,sp+12)
    found=bool(v.reg_read(UC_X86_REG_EAX))
    if found!=(payload is not None) or closed!=1:raise ValueError('Original record presence/close differs')
    value=bool(m.u32(out)) if found else None
    if found and value!=(struct.unpack_from('<I',payload)[0]!=0):raise ValueError('Original pitot AIR word differs')
    return {'payload_hex':None if payload is None else payload.hex(),'found':found,'enabled':value}
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('dll',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
    if a.output.exists():raise ValueError('Output must be new')
    data=a.dll.read_bytes()
    if hashlib.sha256(data).hexdigest()!=t.SHA256:raise ValueError('Unsupported sim1 fingerprint')
    pe=pefile.PE(data=data);fixtures=[None]+[struct.pack('<I',v)+bytes(size-4) for size in (4,5,7,8,12) for v in (0,1,2,0xFFFFFFFF)]
    cases=[run(pe.get_memory_mapped_image(),pe.OPTIONAL_HEADER.ImageBase,payload) for payload in fixtures]
    a.output.mkdir();(a.output/'report.json').write_text(json.dumps({'schema':1,'source':'bounded-original-pitot-air-record','sha256':t.SHA256,'cases':cases,'scope':'original record helper with synthetic open/read/seek/close/clear services; first matching record, valid 4+ byte payloads; no actual FSX process'},indent=2)+'\n',encoding='utf-8')
    print(f'PASS: {len(cases)} original AIR 0x324 record lookups')
if __name__=='__main__':main()
