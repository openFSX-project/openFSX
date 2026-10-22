// Checked authored mouse targets shared by one-shot and persistent profiles.
struct ClockMouseTarget {uintptr_t record,callback;uint32_t flags;int32_t width,height;};
class ClockMouse {
    const Module& module;
    std::vector<uintptr_t> children;
public:
    ClockMouse(const Module& module_,uintptr_t header):module(module_) {
        module.check(header,84);auto list=module.word(header+80);
        if(!list)throw std::runtime_error("No native C172 clock mouse list.");
        size_t depth=0;bool ended=false;
        for(size_t i=0;i<=2048;++i){
            auto record=static_cast<uintptr_t>(list)+i*44;module.check(record,44);auto kind=module.word(record);
            if(!kind){if(depth)throw std::runtime_error("Unclosed native mouse parent.");ended=true;break;}
            if(i==2048 || kind>4)throw std::runtime_error("Invalid or excessive native mouse list.");
            if(kind==1){if(++depth>32)throw std::runtime_error("Excessive native mouse nesting.");}
            if(kind==3){if(!depth)throw std::runtime_error("Orphan native mouse end-parent.");--depth;}
            if(kind==2){if(!depth)throw std::runtime_error("Orphan native mouse child.");children.push_back(record);}
        }
        if(!ended || children.size()!=3)throw std::runtime_error("Expected the checked three-child C172 clock mouse list.");
    }
    uint32_t rva(size_t index) const {
        if(index>=children.size())throw std::runtime_error("Clock mouse child index exceeds 2.");
        return static_cast<uint32_t>(children[index]-module.base);
    }
    ClockMouseTarget target(uint32_t rva,int32_t x,int32_t y) const {
        uintptr_t record=0;for(auto child:children)if(child-module.base==rva)record=child;
        if(!record)throw std::runtime_error("Selected mouse RVA is not an authored clock child.");
        auto flags=module.word(record+28);auto callback=module.word(record+36);
        auto width=static_cast<int32_t>(module.word(record+12)),height=static_cast<int32_t>(module.word(record+16));
        if(flags!=(1u<<29) || module.word(record+32)!=0 || module.word(record+40)!=0
            || width<=0 || height<=0 || width>65536 || height>65536 || x<0 || y<0 || x>=width || y>=height)
            throw std::runtime_error("Only in-bounds left-single callbacks in the checked clock profile are supported.");
        module.check(callback,1,false,true);return {record,callback,flags,width,height};
    }
};
