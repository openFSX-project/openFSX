// A single connected native Clock, retained across bounded local input commands.
class ClockSession {
    GuardedMouseContext context;
    GuardedTextBuffer buffer{8};
    InitializedModule owner;
    const uintptr_t header;
    ClockMouse mouse;
    ClockPanels panels;
    ClockBinding clock;
    bool closed=false,has_time=false;
    double last_time=0;
    uint32_t total_events=0;
public:
    explicit ClockSession(const fs::path& root):owner(library_path(root,"Cessna172.dll")),
        header(gauge(owner.module,owner.linkage,"Clock")),mouse(owner.module,header),
        panels(owner,header,0),clock(owner,buffer) {}
    void state(uint64_t sequence,uint32_t generation,const std::string& command,double seconds,int32_t child=-1){
        if(closed)throw std::runtime_error("Clock session already closed.");
        auto previous_text=clock.text;auto previous_result=clock.result;
        bool unchanged=has_time && seconds==last_time;
        uint32_t clicked_rva=0;int32_t mouse_result=0;
        if(child>=0){
            // Centers are synthetic child-local coordinates, resolved from metadata.
            clicked_rva=mouse.rva(static_cast<size_t>(child));auto initial=mouse.target(clicked_rva,0,0);
            auto target=mouse.target(clicked_rva,initial.width/2,initial.height/2);
            panels.begin_event(seconds);
            *context.data=MouseContext{{target.width/2,target.height/2},reinterpret_cast<void*>(header),reinterpret_cast<void*>(target.record),{0,0},nullptr};
            const auto original=*context.data;MouseFault fault{};
            bool completed=invoke_mouse(reinterpret_cast<MouseCall>(target.callback),context.data,target.flags,&mouse_result,&fault);
            context.check();
            if(!completed)throw std::runtime_error("Persistent native mouse callback failed: "+hex(fault.code));
            if(mouse_result!=1 || std::memcmp(&original,context.data,sizeof(original)))throw std::runtime_error("Native mouse callback rejected input or changed its context.");
            panels.deliver();++total_events;
        }else panels.set_time(seconds);
        clock.update(static_cast<uint32_t>(seconds));panels.check();context.check();
        auto changed=!has_time || previous_text!=clock.text || previous_result!=clock.result;
        std::cout<<std::setprecision(17)<<R"({"schema":1,"source":"native-clock-local-stream","kind":"state","pointer_bits":32,"library":"Cessna172.dll","gauge":"Clock","sequence":)"<<sequence
            <<R"(,"generation":)"<<generation<<R"(,"command":)"<<json(command)<<R"(,"local_seconds":)"<<seconds
            <<R"(,"time_unchanged":)"<<(unchanged?"true":"false")<<R"(,"display_changed":)"<<(changed?"true":"false")
            <<R"(,"text":)"<<json(clock.text)<<R"(,"callback_result":)"<<clock.result<<R"(,"element_rva":)"<<clock.rva
            <<R"(,"events_delivered":)"<<total_events<<R"(,"lookup_count":)"<<panels.lookups<<R"(,"lookup_token":557,"lookup_seconds":)"<<seconds
            <<R"(,"child":)"<<(child>=0?std::to_string(child):"null")<<R"(,"mouse_rva":)"<<(child>=0?std::to_string(clicked_rva):"null")
            <<R"(,"original_packet_word":)"<<(child>=0?std::to_string(panels.packet_word):"null")
            <<R"(,"mouse_result":)"<<(child>=0?std::to_string(mouse_result):"null")
            <<R"(,"process_event_accepted":)"<<(child>=0?"true":"null")
            <<R"(,"guarded_context":true,"guarded_string":true,"guarded_event_words":true,"panel_services":false,"multiplayer":false,"fsx_parity_verified":false})"<<std::endl;
        last_time=seconds;has_time=true;
    }
    void close(){if(!closed){clock.close();context.check();if(owner.module.word(header+88))throw std::runtime_error("Clock user_data was not restored.");panels.close();closed=true;}}
    ~ClockSession(){try{close();}catch(const std::exception& e){std::cerr<<e.what()<<'\n';}}
};
struct ClockCommand {std::string name;uint64_t sequence;double seconds;int32_t child=-1;};
static ClockCommand parse_clock_command(const std::string& line){
    std::istringstream input(line);input.imbue(std::locale::classic());ClockCommand result{};std::string sequence,seconds,extra;
    if(!(input>>result.name>>sequence>>seconds) || sequence.empty() || sequence.find_first_not_of("0123456789")!=std::string::npos)
        throw std::runtime_error("Expected command, integer sequence and local seconds.");
    result.sequence=std::stoull(sequence);size_t end=0;result.seconds=std::stod(seconds,&end);
    if(!result.sequence || result.sequence>4096 || end!=seconds.size() || !std::isfinite(result.seconds) || result.seconds<0 || result.seconds>=86400)
        throw std::runtime_error("Expected local clock command, positive sequence and seconds in [0,86400).");
    if(result.name=="click"){
        std::string child;if(!(input>>child) || child.size()!=1 || child[0]<'0' || child[0]>'2')throw std::runtime_error("Clock click requires child index 0..2.");
        result.child=child[0]-'0';
    }else if(result.name!="time" && result.name!="reset")throw std::runtime_error("Unknown local clock command.");
    if(input>>extra)throw std::runtime_error("Unexpected local clock command argument.");
    return result;
}
static int clock_local_stream(const fs::path& root,Deadline& deadline){
    auto session=std::make_unique<ClockSession>(root);uint32_t generation=1;uint64_t sequence=0;
    std::cout<<R"({"schema":1,"source":"native-clock-local-stream","kind":"ready","pointer_bits":32,"pid":)"<<GetCurrentProcessId()
        <<R"(,"local_event_bridge":true,"string_bindings":1,"mouse_children":3,"panel_services":false,"multiplayer":false,"fsx_parity_verified":false})"<<std::endl;
    std::string line;
    while(true){
        deadline.idle();bool available=line_bounded(line);deadline.active();if(!available || line=="quit")break;
        auto command=parse_clock_command(line);
        if(command.sequence!=sequence+1 || command.sequence>4096)throw std::runtime_error("Local clock sequence must advance exactly once, at most 4096 commands.");
        if(command.name=="reset"){session->close();session.reset();session=std::make_unique<ClockSession>(root);++generation;}
        session->state(command.sequence,generation,command.name,command.seconds,command.child);sequence=command.sequence;
    }
    session->close();session.reset();
    std::cout<<R"({"schema":1,"source":"native-clock-local-stream","kind":"closed","sequence":)"<<sequence
        <<R"(,"modules_deinitialized":)"<<generation<<R"(,"clock_disconnected":true,"string_buffer_restored":true,"import_pointer_restored":true,"context_guards_verified":true})"<<std::endl;
    return 0;
}
