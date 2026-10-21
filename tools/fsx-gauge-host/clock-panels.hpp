// Independent, deliberately narrow local C172 event service profile.
// Included after the helper's checked module, context and clock declarations.
using EventSizeCall = void(__stdcall*)(void*,uint32_t*);
using ProcessEventCall = bool(__stdcall*)(void*,unsigned char*);
static bool invoke_event_size(EventSizeCall fn,void* header,uint32_t* size,DWORD* fault) {
    __try {fn(header,size);return true;}
    __except(EXCEPTION_EXECUTE_HANDLER){*fault=GetExceptionCode();return false;}
}
static bool invoke_process_event(ProcessEventCall fn,void* header,unsigned char* packet,bool* result,DWORD* fault) {
    __try {*result=fn(header,packet);return true;}
    __except(EXCEPTION_EXECUTE_HANDLER){*fault=GetExceptionCode();return false;}
}
class GuardedWord {
    GuardedMouseContext storage;
public:
    GuardedWord(){std::memset(storage.data,0xa5,sizeof(MouseContext));set(0);}
    unsigned char* data(){return reinterpret_cast<unsigned char*>(storage.data)+24;}
    void set(uint32_t value){std::memcpy(data(),&value,4);}
    uint32_t get(){uint32_t value;std::memcpy(&value,data(),4);return value;}
    void check(){storage.check();auto prefix=reinterpret_cast<unsigned char*>(storage.data);
        for(size_t i=0;i<24;++i)if(prefix[i]!=0xa5)throw std::runtime_error("Event word prefix guard damaged.");}
};
class ClockPanels {
    InitializedModule& owner;
    const Module& module;
    uintptr_t header,import_slot=0;
    unsigned char* table=nullptr;
    size_t table_bytes=0;
    bool installed=false,delivering=false;
    uint32_t seconds;
    std::string error;
    inline static thread_local ClockPanels* active=nullptr;
    void put(size_t offset,uintptr_t value){uint32_t word=static_cast<uint32_t>(value);std::memcpy(table+offset,&word,4);}
    static bool __stdcall shared(void* gauge_header,unsigned char* packet,uint32_t size) {
        auto self=active;if(!self)return false;
        if(gauge_header!=reinterpret_cast<void*>(self->header) || !packet || size!=4 || self->events || self->delivering){self->error="Unsupported shared-event context, size or recursion.";return false;}
        // The caller's packet is consumed synchronously while its stack is alive.
        std::memcpy(&self->packet_word,packet,4);++self->events;return true;
    }
    static void __stdcall lookup(NumericSlot* slot) {
        auto self=active;if(!self)return;
        try {
            self->module.check(reinterpret_cast<uintptr_t>(slot),sizeof(NumericSlot),true);
            if(slot->token!=557){self->error="Unsupported PANELS variable token.";return;}
            if(++self->lookups>32){self->error="Excessive PANELS variable lookup.";return;}
            slot->previous=slot->value;slot->value=static_cast<double>(self->seconds);slot->type=33;
        }catch(const std::exception& e){self->error=e.what();}
    }
    static bool __stdcall master(){auto self=active;if(!self)return false;++self->master_calls;return true;}
public:
    uint32_t events=0,packet_word=0,lookups=0,master_calls=0;
    bool delivered=false,process_result=false;
    ClockPanels(InitializedModule& owner_,uintptr_t header_,uint32_t seconds_)
        :owner(owner_),module(owner_.module),header(header_),seconds(seconds_) {
        if(active || seconds>=86400)throw std::runtime_error("Invalid local PANELS profile context.");
        auto imports=module.symbol("ImportTable");module.check(imports,16,true);
        if(module.word(imports)!=15 || module.word(imports+4) || module.word(imports+8) || module.word(imports+12))
            throw std::runtime_error("Expected an unbound PANELS-only import table.");
        module.check(header,244);auto size_fn=module.word(header+236);auto event_fn=module.word(header+240);
        module.check(size_fn,1,false,true);module.check(event_fn,1,false,true);
        GuardedWord size;DWORD fault=0;
        if(!invoke_event_size(reinterpret_cast<EventSizeCall>(size_fn),reinterpret_cast<void*>(header),reinterpret_cast<uint32_t*>(size.data()),&fault))
            throw std::runtime_error("Native event-size callback failed: "+hex(fault));
        size.check();if(size.get()!=4)throw std::runtime_error("Unsupported native event packet size.");
        SYSTEM_INFO info{};GetSystemInfo(&info);table_bytes=info.dwPageSize;
        table=static_cast<unsigned char*>(VirtualAlloc(nullptr,table_bytes,MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE));
        if(!table)throw std::runtime_error("Cannot allocate PANELS service table.");
        // Public PANELS prefix: 24-byte linkage followed by 4-byte function slots.
        put(0,15);put(20,0x900);put(0xa0,reinterpret_cast<uintptr_t>(&lookup));
        put(0x150,reinterpret_cast<uintptr_t>(&shared));put(0x154,reinterpret_cast<uintptr_t>(&master));
        DWORD previous=0;if(!VirtualProtect(table,table_bytes,PAGE_READONLY,&previous)){VirtualFree(table,0,MEM_RELEASE);table=nullptr;throw std::runtime_error("Cannot protect PANELS table.");}
        import_slot=imports+4;auto address=reinterpret_cast<uintptr_t>(table);
        std::memcpy(reinterpret_cast<void*>(import_slot),&address,4);installed=true;active=this;
    }
    void check(){if(!error.empty())throw std::runtime_error(error);if(module.word(import_slot)!=reinterpret_cast<uintptr_t>(table))throw std::runtime_error("Native PANELS import pointer changed.");}
    void deliver() {
        check();if(events!=1 || delivered)throw std::runtime_error("Expected exactly one original mouse packet.");
        GuardedWord packet;packet.set(packet_word);DWORD fault=0;delivering=true;
        auto callback=module.word(header+240);module.check(callback,1,false,true);
        const bool completed=invoke_process_event(reinterpret_cast<ProcessEventCall>(callback),reinterpret_cast<void*>(header),packet.data(),&process_result,&fault);
        delivering=false;packet.check();check();
        if(!completed)throw std::runtime_error("Original process-event callback failed: "+hex(fault));
        if(packet.get()!=packet_word || !process_result)throw std::runtime_error("Original event rejected or mutated its packet.");
        delivered=true;
    }
    void close(){if(installed){
        std::exception_ptr failure;
        try{owner.close();check();}catch(...){failure=std::current_exception();}
        uint32_t zero=0;std::memcpy(reinterpret_cast<void*>(import_slot),&zero,4);installed=false;active=nullptr;
        if(module.word(import_slot))throw std::runtime_error("PANELS import pointer was not restored.");
        if(failure)std::rethrow_exception(failure);
    }}
    ~ClockPanels(){try{close();}catch(const std::exception& e){std::cerr<<e.what()<<'\n';}if(table)VirtualFree(table,0,MEM_RELEASE);if(active==this)active=nullptr;}
    ClockPanels(const ClockPanels&)=delete;
    ClockPanels& operator=(const ClockPanels&)=delete;
};
