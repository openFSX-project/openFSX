// Independent x86 diagnostic host. No FSX headers/import libraries or window/input APIs.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <psapi.h>
#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <iomanip>
#include <iostream>
#include <limits>
#include <locale>
#include <memory>
#include <set>
#include <sstream>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

namespace fs = std::filesystem;
static_assert(sizeof(void*) == 4, "Native FSX gauges require the x86 helper.");
using Update = double(__stdcall*)(void*);
using ModuleCall = void(__stdcall*)();

// The public x86 MODULE_VAR layout, independently declared from ABI facts.
#pragma pack(push, 4)
struct NumericSlot {
    uint32_t token;
    void* address;
    int32_t type;
    double value;
    double previous;
};
#pragma pack(pop)
static_assert(sizeof(NumericSlot) == 28 && offsetof(NumericSlot, value) == 12);

static bool invoke(Update fn, void* element, double* result, DWORD* exception) {
    __try { *result = fn(element); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { *exception = GetExceptionCode(); return false; }
}
static bool invoke_module(ModuleCall fn, DWORD* exception) {
    __try { fn(); return true; }
    __except (EXCEPTION_EXECUTE_HANDLER) { *exception = GetExceptionCode(); return false; }
}
static std::string hex(uintptr_t value) {
    std::ostringstream s; s << "0x" << std::hex << value; return s.str();
}
static std::string ascii(const std::wstring& value) {
    if (value.empty() || value.size() > 256) throw std::runtime_error("Empty or oversized argument.");
    std::string out;
    for (auto c : value) {
        if (c < 32 || c > 126) throw std::runtime_error("This probe accepts ASCII gauge/library names only.");
        out.push_back(static_cast<char>(c));
    }
    return out;
}
static std::string json(const std::string& value) {
    std::string out = "\"";
    for (unsigned char c : value) {
        if (c < 32 || c >= 127) throw std::runtime_error("Non-ASCII metadata is unsupported by this diagnostic protocol.");
        if (c == '"' || c == '\\') out += '\\';
        out += static_cast<char>(c);
    }
    return out + '"';
}
static bool equal_ascii(std::string a, std::string b) {
    auto lower = [](unsigned char c) -> char { return static_cast<char>(c >= 'A' && c <= 'Z' ? c + ('a'-'A') : c); };
    std::transform(a.begin(), a.end(), a.begin(), lower);
    std::transform(b.begin(), b.end(), b.begin(), lower);
    return a == b;
}
static double number(const wchar_t* text) {
    size_t end = 0;
    auto value = std::stod(text, &end);
    if (text[end] || !std::isfinite(value) || std::abs(value) > 1e12)
        throw std::runtime_error("Expected a finite raw token value within +/-1e12.");
    return value;
}

class Deadline {
    HANDLE done = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    HANDLE started = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    std::thread thread;
    std::atomic<ULONGLONG> expires{GetTickCount64()+5000};
public:
    Deadline() {
        if (!done || !started) throw std::runtime_error("Cannot create the helper timeout events.");
        thread = std::thread([this] {
            SetEvent(started);
            while (WaitForSingleObject(done, 25) == WAIT_TIMEOUT) {
                if (GetTickCount64() >= expires.load()) {
                    std::cerr << "Native gauge helper timed out; terminating this helper only.\n";
                    std::cerr.flush(); TerminateProcess(GetCurrentProcess(), 124);
                }
            }
        });
        // Start the watchdog before any vendor DLL can hold the Windows loader lock.
        if (WaitForSingleObject(started, 5000) != WAIT_OBJECT_0) TerminateProcess(GetCurrentProcess(), 124);
    }
    void active() {expires.store(GetTickCount64()+5000);}
    void idle() {expires.store(std::numeric_limits<ULONGLONG>::max());}
    ~Deadline() { SetEvent(done); thread.join(); CloseHandle(started); CloseHandle(done); }
};
struct Module {
    HMODULE handle = nullptr;
    uintptr_t base = 0;
    size_t size = 0;
    explicit Module(const fs::path& path) {
        handle = LoadLibraryExW(path.c_str(), nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
        if (!handle) throw std::runtime_error("Stock DLL load failed, Win32 error " + std::to_string(GetLastError()) + ". No runtime installation is attempted.");
        MODULEINFO info{};
        if (!GetModuleInformation(GetCurrentProcess(), handle, &info, sizeof(info)))
            throw std::runtime_error("Cannot inspect the loaded DLL extent.");
        base = reinterpret_cast<uintptr_t>(info.lpBaseOfDll); size = info.SizeOfImage;
        wchar_t actual[32768]{};
        if (!GetModuleFileNameW(handle, actual, 32768) || fs::canonical(actual) != path)
            throw std::runtime_error("Windows loaded a different DLL path.");
    }
    ~Module() { if (handle) FreeLibrary(handle); }
    Module(const Module&) = delete;
    void check(uintptr_t address, size_t length, bool writable = false, bool executable = false) const {
        if (address < base || length > size || address - base > size - length)
            throw std::runtime_error("Native pointer leaves the selected DLL image.");
        auto end = address + length;
        while (address < end) {
            MEMORY_BASIC_INFORMATION page{};
            if (!VirtualQuery(reinterpret_cast<void*>(address), &page, sizeof(page)) || page.State != MEM_COMMIT
                || page.AllocationBase != handle || (page.Protect & (PAGE_GUARD | PAGE_NOACCESS)))
                throw std::runtime_error("Unavailable native memory page.");
            DWORD kind = page.Protect & 255;
            if (!executable && kind != PAGE_READONLY && kind != PAGE_READWRITE && kind != PAGE_WRITECOPY
                && kind != PAGE_EXECUTE_READ && kind != PAGE_EXECUTE_READWRITE && kind != PAGE_EXECUTE_WRITECOPY)
                throw std::runtime_error("Native metadata page is not readable.");
            if (writable && kind != PAGE_READWRITE && kind != PAGE_WRITECOPY && kind != PAGE_EXECUTE_READWRITE && kind != PAGE_EXECUTE_WRITECOPY)
                throw std::runtime_error("Native input slot is not writable.");
            if (executable && kind != PAGE_EXECUTE && kind != PAGE_EXECUTE_READ && kind != PAGE_EXECUTE_READWRITE && kind != PAGE_EXECUTE_WRITECOPY)
                throw std::runtime_error("Native callback is not executable.");
            auto next = reinterpret_cast<uintptr_t>(page.BaseAddress) + page.RegionSize;
            if (next <= address) throw std::runtime_error("Native memory range overflow.");
            address = next;
        }
    }
    uint32_t word(uintptr_t address) const {
        check(address, 4); uint32_t result; std::memcpy(&result, reinterpret_cast<void*>(address), 4); return result;
    }
    std::string string(uintptr_t address) const {
        std::string out;
        for (size_t i = 0; i <= 256; ++i) {
            check(address + i, 1); auto c = *reinterpret_cast<const char*>(address + i);
            if (!c) return out;
            if (static_cast<unsigned char>(c) < 32 || static_cast<unsigned char>(c) >= 127)
                throw std::runtime_error("Unsupported native gauge-name encoding.");
            out += c;
        }
        throw std::runtime_error("Unterminated native gauge name.");
    }
    uintptr_t symbol(const char* name) const {
        auto pointer = reinterpret_cast<uintptr_t>(GetProcAddress(handle, name));
        if (!pointer) throw std::runtime_error(std::string("Missing native export ") + name);
        check(pointer, 4); return pointer;
    }
};
static fs::path library_path(const fs::path& root, const std::string& library) {
    bool allowed = false;
    for (const auto* name : {"Cessna.dll", "Cessna172.dll", "Cessna182s.dll", "Bell_206B.dll", "Bendix_king_radio.dll"})
        allowed |= equal_ascii(library, name);
    if (!allowed) throw std::runtime_error("This probe is limited to the five checked stock C172 gauge DLLs.");
    // Rust canonicalize supplies a verbatim (\\?\) Windows path. In that
    // namespace forward slashes are literal, so append each component separately.
    if (!fs::is_regular_file(root / "fsx.exe") || !fs::is_directory(root / "SimObjects" / "Airplanes"))
        throw std::runtime_error("Expected an explicit local FSX installation root.");
    auto folder = fs::canonical(root / "Gauges");
    auto path = fs::canonical(folder / library);
    if (path.parent_path() != folder || fs::file_size(path) > 64 * 1024 * 1024)
        throw std::runtime_error("Gauge file leaves the explicit folder or exceeds 64 MiB.");
    return path;
}
static uintptr_t gauge(const Module& module, uintptr_t linkage, const std::string& name) {
    uintptr_t found = 0;
    for (size_t i = 0; i <= 512; ++i) {
        auto header = module.word(linkage + 24 + i * 4);
        if (!header) {
            if (!found) throw std::runtime_error("No native gauge matches the selected name.");
            return found;
        }
        if (i == 512) break;
        module.check(header, 52);
        auto version = module.word(header);
        if (version != 800 && version != 900 && version != 1000) throw std::runtime_error("Unsupported native gauge header version.");
        if (equal_ascii(module.string(module.word(header + 4)), name)) {
            if (found && found != header) throw std::runtime_error("Ambiguous native gauge name.");
            found = header;
        }
    }
    throw std::runtime_error("Unterminated native gauge header list.");
}
static std::vector<uintptr_t> elements(const Module& module, uintptr_t header) {
    auto root_slot = module.word(header + 8);
    std::vector<uintptr_t> queue;
    if (root_slot) { auto root = module.word(root_slot); if (root) queue.push_back(root); }
    std::set<uintptr_t> seen;
    std::vector<uintptr_t> out;
    size_t edges = queue.size();
    while (!queue.empty()) {
        auto element = queue.back(); queue.pop_back();
        if (!seen.insert(element).second) continue;
        if (seen.size() > 2048) throw std::runtime_error("Native element budget exceeded.");
        module.check(element, 60); out.push_back(element);
        auto list = module.word(element + 40);
        if (!list) continue;
        bool ended = false;
        std::vector<uintptr_t> children;
        for (size_t i = 0; i <= 2048; ++i) {
            auto child = module.word(list + i * 4);
            if (!child) { ended = true; break; }
            if (i == 2048 || ++edges > 8192) break;
            children.push_back(child);
        }
        if (!ended) throw std::runtime_error("Unterminated or excessive native child list.");
        queue.insert(queue.end(), children.rbegin(), children.rend());
    }
    return out;
}

struct FrameInput {uint64_t sequence;double time;std::array<double,6> inputs;};
static FrameInput parse_frame(const std::string& line, bool flight=false) {
    std::istringstream stream(line);stream.imbue(std::locale::classic());
    std::string command;FrameInput frame{};
    if (!(stream>>command>>frame.sequence>>frame.time>>frame.inputs[0]>>frame.inputs[1]>>frame.inputs[2]) || command!="frame")
        throw std::runtime_error("Expected: frame SEQUENCE SECONDS IAS_KNOTS BARO_FEET VERTICAL_SPEED_RAW");
    if (flight && !(stream>>frame.inputs[3]>>frame.inputs[4]>>frame.inputs[5]))
        throw std::runtime_error("Flight frames also require PITCH_DEG BANK_DEG GYRO_HEADING_DEG");
    std::string extra;if (stream>>extra) throw std::runtime_error("Trailing frame data.");
    if (!frame.sequence || !std::isfinite(frame.time) || frame.time<0. || frame.time>1e9
        || std::any_of(frame.inputs.begin(),frame.inputs.end(),[](double v){return !std::isfinite(v)||std::abs(v)>1e12;}) || frame.inputs[0]<0.)
        throw std::runtime_error("Invalid native frame sequence, time or numeric inputs.");
    return frame;
}
static bool line_bounded(std::string& line) {
    line.clear();char c;
    while (std::cin.get(c)) {
        if (c=='\n') return true;
        if (c=='\r') continue;
        if (c<32 || static_cast<unsigned char>(c)>=127 || line.size()>=512) throw std::runtime_error("Frame input must be bounded ASCII.");
        line+=c;
    }
    return !line.empty();
}
class InitializedModule {
public:
    Module module;
    uintptr_t linkage;
    bool initialized=false;
    explicit InitializedModule(const fs::path& path):module(path),linkage(module.symbol("Linkage")) {
        module.check(linkage,24);
        auto init=module.word(linkage+4);
        if (!init || !module.word(linkage+8)) throw std::runtime_error("Stream profile requires native ModuleInit/ModuleDeinit callbacks.");
        if (init) {
            module.check(init,1,false,true);DWORD fault=0;
            if (!invoke_module(reinterpret_cast<ModuleCall>(init),&fault)) throw std::runtime_error("Stream ModuleInit failed: "+hex(fault));
            initialized=true;
        }
    }
    void close() {
        if (!initialized) return;
        initialized=false;auto deinit=module.word(linkage+8);
        if (deinit) {
            module.check(deinit,1,false,true);DWORD fault=0;
            if (!invoke_module(reinterpret_cast<ModuleCall>(deinit),&fault)) throw std::runtime_error("Stream ModuleDeinit failed: "+hex(fault));
        }
    }
    ~InitializedModule() {try {close();} catch (const std::exception& e) {std::cerr<<e.what()<<'\n';}}
};
class GuardedTextBuffer {
    unsigned char* allocation=nullptr;
    size_t page=0,capacity=0;
public:
    char* data=nullptr;
    explicit GuardedTextBuffer(size_t maximum) {
        if (!maximum || maximum>256) throw std::runtime_error("String probe length must be 1..256.");
        SYSTEM_INFO info{};GetSystemInfo(&info);page=info.dwPageSize;capacity=maximum+1;
        allocation=static_cast<unsigned char*>(VirtualAlloc(nullptr,page*3,MEM_RESERVE,PAGE_NOACCESS));
        if (!allocation) throw std::runtime_error("Cannot reserve guarded string output.");
        if(!VirtualAlloc(allocation+page,page,MEM_COMMIT,PAGE_READWRITE)){VirtualFree(allocation,0,MEM_RELEASE);allocation=nullptr;throw std::runtime_error("Cannot commit guarded string output.");}
        reset(maximum);
    }
    void reset(size_t maximum) {
        if(!maximum || maximum>256)throw std::runtime_error("String probe length must be 1..256.");
        capacity=maximum+1;
        std::memset(allocation+page,0xa5,page);
        data=reinterpret_cast<char*>(allocation+page*2-capacity);std::memset(data,0,capacity);
    }
    ~GuardedTextBuffer(){if(allocation)VirtualFree(allocation,0,MEM_RELEASE);}
    GuardedTextBuffer(const GuardedTextBuffer&)=delete;
    GuardedTextBuffer& operator=(const GuardedTextBuffer&)=delete;
    std::string value() const {
        for(size_t i=0;i<page-capacity;++i)if(allocation[page+i]!=0xa5)throw std::runtime_error("Native string callback wrote before its output buffer.");
        auto end=static_cast<const char*>(std::memchr(data,0,capacity));
        if(!end)throw std::runtime_error("Native string output exceeds declared length or lacks NUL.");
        auto result=std::string(data,static_cast<size_t>(end-data));json(result);return result;
    }
};
struct StringInput { double value;uint64_t payload;const char* kind; };
static StringInput string_input(const wchar_t* raw) {
    std::wstring text(raw);
    if(text.rfind(L"f64:",0)==0){auto value=number(text.c_str()+4);uint64_t payload=0;std::memcpy(&payload,&value,8);return {value,payload,"f64"};}
    if(text.rfind(L"u32:",0)==0){auto value_text=text.substr(4);if(value_text.empty() || value_text[0]==L'-' || value_text[0]==L'+')throw std::runtime_error("Invalid unsigned string slot.");size_t end=0;int base=value_text.rfind(L"0x",0)==0 || value_text.rfind(L"0X",0)==0?16:10;auto value=std::stoull(value_text,&end,base);if(end!=value_text.size() || value>UINT32_MAX)throw std::runtime_error("String integer slot exceeds u32.");return {static_cast<double>(value),value,"u32"};}
    throw std::runtime_error("String inputs require explicit f64:VALUE or u32:VALUE union interpretation.");
}
static int string_sample(const fs::path& root,const std::string& library,const std::string& name,uint32_t rva,const std::array<StringInput,3>& inputs) {
    // Buffer precedes module lifetime so native deinitialization cannot outlive it.
    GuardedTextBuffer buffer(256);
    InitializedModule owner(library_path(root,library));auto& module=owner.module;
    auto header=gauge(module,owner.linkage,name);uintptr_t selected=0;
    for(auto element:elements(module,header))if(element-module.base==rva && module.word(element)==2)selected=element;
    if(!selected)throw std::runtime_error("No selected string RVA in the gauge graph.");
    module.check(selected,324,true);
    auto maximum=module.word(selected+292)&0xffff;
    if(!maximum || maximum>256)throw std::runtime_error("Selected native string length is outside 1..256.");
    buffer.reset(maximum);
    auto callback=module.word(selected+312);module.check(callback,1,false,true);
    for(size_t i=0;i<3;++i)if(module.word(selected+168+i*4))throw std::runtime_error("String source callbacks are unsupported by explicit-input sampling.");
    std::array<uint32_t,3> tokens{};
    for(size_t i=0;i<3;++i){tokens[i]=module.word(selected+60+i*28);module.check(selected+72+i*28,16,true);std::memcpy(reinterpret_cast<void*>(selected+72+i*28),&inputs[i].payload,8);std::memcpy(reinterpret_cast<void*>(selected+80+i*28),&inputs[i].payload,8);}
    auto original=module.word(selected+184);auto pointer=buffer.data;std::memcpy(reinterpret_cast<void*>(selected+184),&pointer,4);
    double result=0.;DWORD exception=0;
    const bool success=invoke(reinterpret_cast<Update>(callback),reinterpret_cast<void*>(selected),&result,&exception);
    const bool pointer_unchanged=module.word(selected+184)==reinterpret_cast<uintptr_t>(pointer);
    std::memcpy(reinterpret_cast<void*>(selected+184),&original,4);
    if(!success)throw std::runtime_error("Native string callback failed or needs unsupported services: "+hex(exception));
    if(!pointer_unchanged || !std::isfinite(result))throw std::runtime_error("Native string callback changed its buffer or returned a non-finite value.");
    auto text=buffer.value();owner.close();
    std::cout<<std::setprecision(17)<<"{\"schema\":1,\"source\":\"native-string-probe\",\"pointer_bits\":32,\"library\":"<<json(library)<<",\"gauge\":"<<json(name)<<",\"element_rva\":"<<rva<<",\"tokens\":["<<tokens[0]<<','<<tokens[1]<<','<<tokens[2]<<"],\"raw_inputs\":["<<inputs[0].value<<','<<inputs[1].value<<','<<inputs[2].value<<"],\"input_kinds\":["<<json(inputs[0].kind)<<','<<json(inputs[1].kind)<<','<<json(inputs[2].kind)<<"],\"max_length\":"<<maximum<<",\"callback_result\":"<<result<<",\"text\":"<<json(text)<<",\"module_init_called\":true,\"module_deinit_called\":true,\"guarded_buffer\":true,\"panel_services\":false,\"full_gauge_lifecycle\":false}\n";
    return 0;
}
struct Binding {
    InitializedModule* owner;
    const char* library;
    const char* name;
    uint32_t resource;
    uint32_t token;
    size_t input;
    uintptr_t element=0;
    Update callback=nullptr;
    bool previous_available=false;
    double previous=0.;
    uint32_t kind=1,axis=0;
    Binding(InitializedModule* owner_,const char* library_,const char* name_,uint32_t resource_,uint32_t token_,size_t input_,uint32_t kind_=1,uint32_t axis_=0)
        :owner(owner_),library(library_),name(name_),resource(resource_),token(token_),input(input_),kind(kind_),axis(axis_) {}
    void locate() {
        const auto& module=owner->module;
        auto header=gauge(module,owner->linkage,name);
        for (auto e:elements(module,header)) {
            if (module.word(e)==kind && module.word(e+4)==resource) {
                if (element) throw std::runtime_error("Ambiguous stream needle.");
                module.check(e,kind==6?352:236);element=e;
            }
        }
        auto slot=60+(kind==6?axis*40:0);
        if (!element || module.word(element+slot)!=token) throw std::runtime_error("Native stream token binding mismatch.");
        auto address=module.word(element+(kind==6?316+axis*4:224));
        if (kind==6 && module.word(element+slot+28)) throw std::runtime_error("Sprite source lookup callback is unsupported.");
        if (address) module.check(address,1,false,true);
        else if (kind!=6) throw std::runtime_error("Native needle callback is required.");
        module.check(element+slot+12,16,true);callback=reinterpret_cast<Update>(address);
    }
    double update(double raw) {
        double old=previous_available?previous:raw;
        auto slot=60+(kind==6?axis*40:0);
        std::memcpy(reinterpret_cast<void*>(element+slot+12),&raw,8);
        std::memcpy(reinterpret_cast<void*>(element+slot+20),&old,8);
        double result=raw;DWORD fault=0;
        if (callback && !invoke(callback,reinterpret_cast<void*>(element),&result,&fault)) throw std::runtime_error("Stream element callback failed: "+hex(fault));
        if (!std::isfinite(result) || std::abs(result)>1e12) throw std::runtime_error("Invalid stream needle result.");
        previous=raw;previous_available=true;return result;
    }
};
static int frame_stream(const fs::path& root,Deadline& deadline,bool flight=false) {
    InitializedModule cessna(library_path(root,"Cessna.dll"));
    InitializedModule c172(library_path(root,"Cessna172.dll"));
    auto bell=flight?std::make_unique<InitializedModule>(library_path(root,"Bell_206B.dll")):nullptr;
    std::array<Binding,14> bindings{{
        {&c172,"Cessna172.dll","Airspeed",1001,61,0},
        {&cessna,"Cessna.dll","Altimeter",1033,74,1},
        {&cessna,"Cessna.dll","Altimeter",1032,74,1},
        {&cessna,"Cessna.dll","Altimeter",1031,74,1},
        {&cessna,"Cessna.dll","Vertical_Speed",5047,65,2},
        {&cessna,"Cessna.dll","Heading_Indicator",4001,0,6,6,0},
        {&cessna,"Cessna.dll","Heading_Indicator",4001,0,6,6,1},
        {&cessna,"Cessna.dll","Heading_Indicator",4001,84,5,6,2},
        {bell.get(),"Bell_206B.dll","Attitude",2004,0,6,6,0},
        {bell.get(),"Bell_206B.dll","Attitude",2004,0,6,6,1},
        {bell.get(),"Bell_206B.dll","Attitude",2004,80,4,6,2},
        {bell.get(),"Bell_206B.dll","Attitude",2002,0,6,6,0},
        {bell.get(),"Bell_206B.dll","Attitude",2002,78,3,6,1},
        {bell.get(),"Bell_206B.dll","Attitude",2002,80,4,6,2}
    }};
    const size_t count=flight?14:5;const int schema=flight?2:1;
    for (size_t i=0;i<count;++i) bindings[i].locate();
    std::cout<<"{\"schema\":"<<schema<<",\"source\":\"native-gauge-stream\",\"kind\":\"ready\",\"pointer_bits\":32,\"pid\":"<<GetCurrentProcessId()
        <<",\"libraries_loaded\":"<<(flight?3:2)<<",\"bindings\":"<<count<<",\"panel_services\":false}"<<std::endl;
    uint64_t sequence=0,evaluated=0;double time=-1.;std::array<double,6> inputs{};std::array<double,14> results{};
    std::string line;
    while (true) {
        deadline.idle();bool available=line_bounded(line);deadline.active();
        if (!available || line=="quit") break;
        auto frame=parse_frame(line,flight);
        if (frame.sequence!=sequence+1 || frame.time<time) throw std::runtime_error("Stream sequence/time must advance; reset requires a new session.");
        bool reused=frame.time==time;
        if (reused && frame.inputs!=inputs) throw std::runtime_error("Changed physics inputs at an unchanged simulation time.");
        if (!reused) {
            for (size_t i=0;i<count;++i) results[i]=bindings[i].update(bindings[i].input<6?frame.inputs[bindings[i].input]:0.);
            ++evaluated;inputs=frame.inputs;time=frame.time;
        }
        sequence=frame.sequence;
        std::cout<<std::setprecision(17)<<"{\"schema\":"<<schema<<",\"source\":\"native-gauge-stream\",\"kind\":\"frame\",\"sequence\":"<<sequence
            <<",\"time\":"<<time<<",\"reused\":"<<(reused?"true":"false")<<",\"frames_evaluated\":"<<evaluated<<",\"values\":[";
        for (size_t i=0;i<count;++i) {
            auto& b=bindings[i];if (i) std::cout<<',';
            std::cout<<"{\"library\":"<<json(b.library)<<",\"gauge\":"<<json(b.name)<<",\"resource\":"<<b.resource<<",\"token\":"<<b.token
                <<",\"raw_input\":"<<(b.input<6?inputs[b.input]:0.)<<",\"callback_result\":"<<results[i];
            if (flight) std::cout<<",\"element_kind\":"<<b.kind<<",\"axis\":"<<b.axis<<",\"callback_called\":"<<(b.callback?"true":"false");
            std::cout<<'}';
        }
        std::cout<<"]}"<<std::endl;
    }
    if (bell) bell->close();c172.close();cessna.close();
    std::cout<<"{\"schema\":"<<schema<<",\"source\":\"native-gauge-stream\",\"kind\":\"closed\",\"frames_evaluated\":"<<evaluated<<",\"modules_deinitialized\":"<<(flight?3:2)<<"}"<<std::endl;
    return 0;
}
static double __stdcall synthetic(void* value) { return *static_cast<double*>(value) * 2. + 3.; }
static double __stdcall synthetic_fault(void*) { RaiseException(0xe0420001, 0, 0, nullptr); return 0.; }
static double __stdcall synthetic_string(void* element) {
    char* output=nullptr;std::memcpy(&output,static_cast<unsigned char*>(element)+184,4);
    std::memcpy(output,"09:08:07",9);return 7.;
}
static double __stdcall synthetic_string_overflow(void* element) {
    char* output=nullptr;std::memcpy(&output,static_cast<unsigned char*>(element)+184,4);
    output[9]='X';return 0.;
}
static double __stdcall synthetic_string_underflow(void* element) {
    char* output=nullptr;std::memcpy(&output,static_cast<unsigned char*>(element)+184,4);
    output[-1]='X';return 0.;
}
static int self_test() {
    double input = 4., result = 0.; DWORD exception = 0;
    if (!invoke(synthetic, &input, &result, &exception) || result != 11.) throw std::runtime_error("Synthetic x86 stdcall result failed.");
    if (invoke(synthetic_fault, &input, &result, &exception) || exception != 0xe0420001) throw std::runtime_error("Structured exception containment failed.");
    GuardedTextBuffer text(8);std::array<unsigned char,324> text_element{};auto text_pointer=text.data;
    std::memcpy(text_element.data()+184,&text_pointer,4);
    if(!invoke(synthetic_string,text_element.data(),&result,&exception)||result!=7.||text.value()!="09:08:07")throw std::runtime_error("Synthetic string callback failed.");
    if(invoke(synthetic_string_overflow,text_element.data(),&result,&exception)||exception!=EXCEPTION_ACCESS_VIOLATION)throw std::runtime_error("String output guard did not contain an overflow.");
    if(!invoke(synthetic_string_underflow,text_element.data(),&result,&exception))throw std::runtime_error("Synthetic string prefix test failed.");
    bool underrun=false;try{text.value();}catch(...){underrun=true;}if(!underrun)throw std::runtime_error("String output prefix overwrite was not detected.");
    if(string_input(L"u32:0x1825").payload!=0x1825 || string_input(L"f64:2.5").value!=2.5)throw std::runtime_error("String union interpretation failed.");
    for(const wchar_t* bad:{L"1",L"u32:-1",L"u32:4294967296",L"f64:NaN"}){bool rejected=false;try{string_input(bad);}catch(...){rejected=true;}if(!rejected)throw std::runtime_error("Invalid string union input accepted.");}
    if (json("a\\b\"c") != "\"a\\\\b\\\"c\"" || number(L"-2.5") != -2.5) throw std::runtime_error("Protocol scalar/escaping check failed.");
    for (const wchar_t* bad : {L"NaN",L"1e99",L"2tail"}) {
        bool rejected = false; try { number(bad); } catch (...) { rejected = true; }
        if (!rejected) throw std::runtime_error("Invalid raw input accepted.");
    }
    auto frame=parse_frame("frame 1 0.25 75 500 -256");
    if (frame.sequence!=1 || frame.time!=0.25 || frame.inputs!=std::array<double,6>{75.,500.,-256.,0.,0.,0.}) throw std::runtime_error("Synthetic frame protocol failed.");
    auto flight=parse_frame("frame 1 0.25 75 500 -256 -10 -30 90",true);
    if (flight.inputs!=std::array<double,6>{75.,500.,-256.,-10.,-30.,90.}) throw std::runtime_error("Synthetic flight protocol failed.");
    for (const char* bad:{"frame 1 0 75 500 0","frame 1 0 75 500 0 1 2 NaN","frame 1 0 75 500 0 1 2 3 extra"}) {
        bool rejected=false;try {parse_frame(bad,true);} catch (...) {rejected=true;}
        if (!rejected) throw std::runtime_error("Invalid flight frame accepted.");
    }
    for (const char* bad:{"frame 0 0 1 2 3","frame 1 -1 1 2 3","frame 1 0 -1 2 3","frame 1 0 1 2 1e99","frame 1 0 1 2 3 extra"}) {
        bool rejected=false;try {parse_frame(bad);} catch (...) {rejected=true;}
        if (!rejected) throw std::runtime_error("Invalid frame input accepted.");
    }
    std::cout << "{\"schema\":1,\"source\":\"synthetic\",\"pointer_bits\":32,\"stdcall_result\":11,\"exception_contained\":true,\"string_guard_verified\":true,\"game_loaded\":false}\n";
    return 0;
}
int wmain(int argc, wchar_t** argv) {
    SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
    _set_abort_behavior(0, _WRITE_ABORT_MSG | _CALL_REPORTFAULT);
    try {
        Deadline deadline;
        if (argc == 2 && std::wstring(argv[1]) == L"--self-test") return self_test();
        if (argc==3 && std::wstring(argv[1])==L"--frames") return frame_stream(argv[2],deadline);
        if (argc==3 && std::wstring(argv[1])==L"--flight-frames") return frame_stream(argv[2],deadline,true);
        if(argc==9 && std::wstring(argv[1])==L"--sample-string") {
            size_t end=0;auto rva=std::stoul(argv[5],&end,0);if(!rva || argv[5][end] || argv[5][0]==L'-' || argv[5][0]==L'+')throw std::runtime_error("Invalid string RVA.");
            return string_sample(argv[2],ascii(argv[3]),ascii(argv[4]),rva,{string_input(argv[6]),string_input(argv[7]),string_input(argv[8])});
        }
        const bool sample = argc == 7 && std::wstring(argv[1]) == L"--sample";
        if (!sample && !(argc == 5 && std::wstring(argv[1]) == L"--probe"))
            throw std::runtime_error("Usage: fsx-gauge-host --probe FSX_ROOT LIBRARY GAUGE | --sample FSX_ROOT LIBRARY GAUGE NEEDLE_RESOURCE_ID RAW_VALUE | --sample-string FSX_ROOT LIBRARY GAUGE ELEMENT_RVA INPUT1 INPUT2 INPUT3 (f64:VALUE or u32:VALUE) | --frames FSX_ROOT | --flight-frames FSX_ROOT | --self-test");
        auto library = ascii(argv[3]); auto name = ascii(argv[4]);
        uint32_t selected_id = 0; double raw = 0.;
        if (sample) {
            size_t end = 0; auto value = std::stoul(argv[5], &end);
            if (argv[5][end] || value == 0 || value > INT32_MAX) throw std::runtime_error("Invalid needle resource ID.");
            selected_id = static_cast<uint32_t>(value); raw = number(argv[6]);
        }
        auto path = library_path(argv[2], library);
        Module module(path);
        auto linkage = module.symbol("Linkage"); module.check(linkage, 24);
        auto header = gauge(module, linkage, name);
        bool initialized = false;
        if (sample) {
            auto init = module.word(linkage + 4);
            if (init) {
                module.check(init, 1, false, true); DWORD exception = 0;
                if (!invoke_module(reinterpret_cast<ModuleCall>(init), &exception))
                    throw std::runtime_error("Native ModuleInit requires unsupported host services or failed: " + hex(exception));
                initialized = true;
            }
        }
        auto graph = elements(module, header);
        std::ostringstream needles; needles << '[';
        size_t count = 0; uintptr_t selected = 0;
        for (auto element : graph) {
            if (module.word(element) != 1) continue;
            module.check(element, 236);
            if (count++) needles << ',';
            auto callback = module.word(element + 224);
            needles << "{\"resource\":" << module.word(element + 4) << ",\"token\":" << module.word(element + 60)
                << ",\"update_callback\":" << (callback ? "true" : "false") << '}';
            if (sample && module.word(element + 4) == selected_id) {
                if (selected) throw std::runtime_error("Ambiguous needle resource ID.");
                selected = element;
            }
        }
        needles << ']';
        double value = 0.;
        if (sample) {
            if (!selected) throw std::runtime_error("No selected needle resource in this gauge.");
            auto callback = module.word(selected + 224);
            if (!callback) throw std::runtime_error("Selected needle has no native update callback.");
            module.check(callback, 1, false, true); module.check(selected + 72, 16, true);
            std::memcpy(reinterpret_cast<void*>(selected + 72), &raw, 8);
            std::memcpy(reinterpret_cast<void*>(selected + 80), &raw, 8);
            DWORD exception = 0;
            if (!invoke(reinterpret_cast<Update>(callback), reinterpret_cast<void*>(selected), &value, &exception))
                throw std::runtime_error("Native element callback requires unsupported services or failed: " + hex(exception));
            if (!std::isfinite(value)) throw std::runtime_error("Native element callback returned a non-finite result.");
        }
        bool deinitialized = false;
        if (initialized) {
            auto deinit = module.word(linkage + 8);
            if (deinit) {
                module.check(deinit, 1, false, true); DWORD exception = 0;
                if (!invoke_module(reinterpret_cast<ModuleCall>(deinit), &exception))
                    throw std::runtime_error("Native ModuleDeinit failed: " + hex(exception));
                deinitialized = true;
            }
        }
        std::cout << std::setprecision(17) << "{\"schema\":1,\"source\":\"native-gauge-probe\",\"mode\":" << json(sample ? "sample" : "probe") << ",\"pointer_bits\":32,\"library\":" << json(library)
            << ",\"gauge\":" << json(name) << ",\"version\":" << module.word(header) << ",\"elements\":" << graph.size()
            << ",\"module_init_called\":" << (initialized ? "true" : "false") << ",\"module_deinit_called\":" << (deinitialized ? "true" : "false") << ",\"panel_services\":false,\"needles\":" << needles.str();
        if (sample) std::cout << ",\"sample\":{\"resource\":" << selected_id << ",\"token\":" << module.word(selected + 60) << ",\"raw_input\":" << raw << ",\"callback_result\":" << value << '}';
        std::cout << "}\n";
        // No full gauge lifecycle is claimed. Process exit discards the isolated sample state.
        return 0;
    } catch (const std::exception& error) { std::cerr << "Native gauge probe failed: " << error.what() << '\n'; return 1; }
}
