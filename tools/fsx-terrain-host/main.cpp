// Independent x86 terrain decode host. Loads the user's installed terrain.dll
// and calls its own record decompressor so every BGL terrain compression type
// (raw, LZ, delta, bit-pack, PTC) is decoded exactly as FSX does. No FSX
// headers, import libraries, windows or simulation state are used.
//
// Protocol on stdin/stdout (little endian), repeated until EOF:
//   request:  u32 compression | format << 16, u32 columns, u32 rows,
//             u32 length, length bytes (record payload after the TRQ1 header).
//             format 0 = elevation (f32 metres), 1 = 8-bit class grid.
//   response: u32 status (1 ok, 0 failed), then columns*rows samples when ok
//             (f32 for elevation, u8 for class grids).
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <fcntl.h>
#include <io.h>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <iostream>
#include <stdexcept>
#include <string>
#include <vector>

static_assert(sizeof(void*) == 4, "terrain.dll requires the x86 helper.");
namespace fs = std::filesystem;

// Pinned to the FSX SP2/Acceleration/Steam terrain.dll (sha256 85fd7221...).
constexpr uint32_t CONSTRUCT_RVA = 0x79090;  // decoder object constructor (this in ECX)
constexpr uint32_t DECODE_RVA = 0x79240;     // __thiscall decode(type, data, size, descriptor)
constexpr uint16_t FORMAT_ELEVATION = 0x0F;  // int16 * RCS1 scale + offset -> f32
constexpr uint16_t FORMAT_CLASS = 0x01;      // one byte per sample

using Construct = void*(__fastcall*)(void* self, void* unused);
using Decode = int(__fastcall*)(void* self, void* unused, uint32_t type, const void* data,
                                uint32_t size, void* descriptor);

#pragma pack(push, 1)
struct Descriptor {
    uint16_t format;
    uint16_t pad0;
    int32_t columns;
    int32_t rows;
    int32_t stride;
    int32_t reserved;
    uint32_t capacity;
    float* output;
    uint8_t tail[32];
};
#pragma pack(pop)
static_assert(offsetof(Descriptor, columns) == 4 && offsetof(Descriptor, stride) == 12 &&
              offsetof(Descriptor, capacity) == 20 && offsetof(Descriptor, output) == 24);

static bool read_exact(void* p, size_t n) {
    return n == 0 || fread(p, 1, n, stdin) == n;
}
static int guarded(Decode decode, void* self, uint32_t type, const void* data, uint32_t size,
                   Descriptor* d) {
    __try {
        return decode(self, nullptr, type, data, size, d);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return 0;
    }
}
static int run(const fs::path& root) {
    SetDllDirectoryW(root.c_str());
    HMODULE terrain = LoadLibraryW((root / L"terrain.dll").c_str());
    if (!terrain) throw std::runtime_error("Cannot load terrain.dll from the FSX folder.");
    auto base = reinterpret_cast<uint8_t*>(terrain);
    auto construct = reinterpret_cast<Construct>(base + CONSTRUCT_RVA);
    auto decode = reinterpret_cast<Decode>(base + DECODE_RVA);
    // Expected prologue bytes guard against a different terrain.dll build.
    const uint8_t prologue[] = {0x8B, 0xFF, 0x55, 0x8B, 0xEC};
    if (memcmp(base + CONSTRUCT_RVA, prologue, 5) || memcmp(base + DECODE_RVA, prologue, 5))
        throw std::runtime_error("Unsupported terrain.dll build.");
    alignas(16) static uint8_t object[0x200] = {};
    construct(object, nullptr);
    _setmode(_fileno(stdin), _O_BINARY);
    _setmode(_fileno(stdout), _O_BINARY);
    std::vector<uint8_t> input;
    std::vector<uint8_t> output;
    for (;;) {
        uint32_t header[4];
        if (!read_exact(header, sizeof header)) return 0;
        auto [type, columns, rows, length] = header;
        if (columns == 0 || rows == 0 || columns > 4096 || rows > 4096 || length > (64u << 20))
            throw std::runtime_error("Invalid terrain request.");
        input.resize(length);
        if (!read_exact(input.data(), length)) return 0;
        const uint32_t format = type >> 16;
        if (format > 1) throw std::runtime_error("Unsupported terrain format.");
        const size_t sample = format == 0 ? sizeof(float) : 1;
        output.assign(size_t(columns) * rows * sample, 0);
        Descriptor d{};
        d.format = format == 0 ? FORMAT_ELEVATION : FORMAT_CLASS;
        d.columns = int32_t(columns);
        d.rows = int32_t(rows);
        d.stride = int32_t(columns * sample);
        d.capacity = uint32_t(output.size());
        d.output = reinterpret_cast<float*>(output.data());
        uint32_t ok = guarded(decode, object, type & 0xFF, input.data(), length, &d) ? 1 : 0;
        fwrite(&ok, 4, 1, stdout);
        if (ok) fwrite(output.data(), 1, output.size(), stdout);
        fflush(stdout);
    }
}
int wmain(int argc, wchar_t** argv) {
    try {
        if (argc == 3 && std::wstring(argv[1]) == L"--decode") return run(argv[2]);
        std::cerr << "Usage: fsx-terrain-host --decode FSX_ROOT (binary requests on stdin)\n";
        return 2;
    } catch (const std::exception& e) {
        std::cerr << e.what() << "\n";
        return 1;
    }
}
