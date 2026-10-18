// Independent passive window capture. No activation, restoration or input APIs.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <windows.graphics.capture.interop.h>
#include <windows.graphics.directx.direct3d11.interop.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Foundation.Metadata.h>
#include <winrt/Windows.Graphics.Capture.h>
#include <winrt/Windows.Graphics.DirectX.h>
#include <winrt/Windows.Graphics.DirectX.Direct3D11.h>
#include <algorithm>
#include <chrono>
#include <cwctype>
#include <filesystem>
#include <iostream>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

using namespace winrt;
using namespace winrt::Windows::Graphics::Capture;
using namespace winrt::Windows::Graphics::DirectX;
using namespace winrt::Windows::Graphics::DirectX::Direct3D11;

static void dimensions(int width, int height) {
    if (width <= 0 || height <= 0 || width > 8192 || height > 8192 ||
        static_cast<uint64_t>(width) * height > 16 * 1024 * 1024)
        throw std::runtime_error("Capture dimensions exceed the 16-megapixel / 8192-axis limit.");
}
static std::vector<uint8_t> rgb_row(const uint8_t* bgra, size_t width) {
    std::vector<uint8_t> rgb(width * 3);
    for (size_t x = 0; x < width; ++x) {
        rgb[x * 3] = bgra[x * 4 + 2];
        rgb[x * 3 + 1] = bgra[x * 4 + 1];
        rgb[x * 3 + 2] = bgra[x * 4];
    }
    return rgb;
}
static void write_bytes(HANDLE file, const void* data, DWORD size) {
    DWORD written = 0;
    if (!WriteFile(file, data, size, &written, nullptr) || written != size) throw_last_error();
}
static void save_ppm(const wchar_t* path, const D3D11_MAPPED_SUBRESOURCE& map, int width, int height) {
    if (!map.pData || map.RowPitch < static_cast<UINT>(width) * 4)
        throw std::runtime_error("Invalid captured row pitch.");
    HANDLE raw = CreateFileW(path, GENERIC_WRITE, 0, nullptr, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (raw == INVALID_HANDLE_VALUE) throw_last_error();
    handle file{raw};
    std::string header = "P6\n" + std::to_string(width) + " " + std::to_string(height) + "\n255\n";
    write_bytes(file.get(), header.data(), static_cast<DWORD>(header.size()));
    for (int y = 0; y < height; ++y) {
        auto row = rgb_row(static_cast<const uint8_t*>(map.pData) + static_cast<size_t>(y) * map.RowPitch, width);
        write_bytes(file.get(), row.data(), static_cast<DWORD>(row.size()));
    }
}
struct Selection { DWORD pid; std::vector<HWND> windows; };
static BOOL CALLBACK enumerate(HWND window, LPARAM context) {
    auto& selection = *reinterpret_cast<Selection*>(context);
    DWORD pid = 0; GetWindowThreadProcessId(window, &pid);
    if (pid == selection.pid) selection.windows.push_back(window);
    return TRUE;
}
static handle verify_process(DWORD pid) {
    handle process{OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid)};
    if (!process) throw_last_error();
    std::wstring path(32768, L'\0'); DWORD size = static_cast<DWORD>(path.size());
    if (!QueryFullProcessImageNameW(process.get(), 0, path.data(), &size)) throw_last_error();
    path.resize(size);
    auto name = std::filesystem::path(path).filename().wstring();
    std::transform(name.begin(), name.end(), name.begin(), [](wchar_t c) { return static_cast<wchar_t>(std::towlower(c)); });
    if (name != L"fsx.exe" && name != L"openfsx-viewer.exe")
        throw std::runtime_error("Only the explicitly selected FSX/openFSX process can be captured.");
    return process;
}
static void capture(HWND window, DWORD pid, const wchar_t* output) {
    DWORD current_pid = 0; GetWindowThreadProcessId(window, &current_pid);
    if (current_pid != pid || IsIconic(window))
        throw std::runtime_error("The selected game window is unavailable or minimized; it will not be restored.");
    if (!GraphicsCaptureSession::IsSupported()) throw std::runtime_error("Windows Graphics Capture is unavailable.");
    auto foreground = GetForegroundWindow();
    auto factory = get_activation_factory<GraphicsCaptureItem, IGraphicsCaptureItemInterop>();
    GraphicsCaptureItem item{nullptr};
    check_hresult(factory->CreateForWindow(window, guid_of<GraphicsCaptureItem>(), put_abi(item)));
    auto size = item.Size(); dimensions(size.Width, size.Height);
    com_ptr<ID3D11Device> device; com_ptr<ID3D11DeviceContext> context;
    check_hresult(D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        nullptr, 0, D3D11_SDK_VERSION, device.put(), nullptr, context.put()));
    auto dxgi = device.as<IDXGIDevice>();
    com_ptr<IInspectable> bridge;
    check_hresult(CreateDirect3D11DeviceFromDXGIDevice(dxgi.get(), bridge.put()));
    auto pool = Direct3D11CaptureFramePool::CreateFreeThreaded(bridge.as<IDirect3DDevice>(),
        DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size);
    auto session = pool.CreateCaptureSession(item);
    if (winrt::Windows::Foundation::Metadata::ApiInformation::IsPropertyPresent(
        L"Windows.Graphics.Capture.GraphicsCaptureSession", L"IsCursorCaptureEnabled")) session.IsCursorCaptureEnabled(false);
    // Keep the OS capture indicator and all capture/security policy defaults.
    session.StartCapture();
    Direct3D11CaptureFrame frame{nullptr};
    auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(5);
    while (!frame && std::chrono::steady_clock::now() < deadline) {
        frame = pool.TryGetNextFrame();
        if (!frame) std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    if (!frame) throw std::runtime_error("No window frame arrived within five seconds; no desktop fallback is used.");
    auto content = frame.ContentSize(); dimensions(content.Width, content.Height);
    auto access = frame.Surface().as<::Windows::Graphics::DirectX::Direct3D11::IDirect3DDxgiInterfaceAccess>();
    com_ptr<ID3D11Texture2D> texture;
    check_hresult(access->GetInterface(__uuidof(ID3D11Texture2D), texture.put_void()));
    D3D11_TEXTURE2D_DESC description{}; texture->GetDesc(&description);
    dimensions(static_cast<int>(description.Width), static_cast<int>(description.Height));
    if (content.Width > static_cast<int>(description.Width) || content.Height > static_cast<int>(description.Height) ||
        description.Format != DXGI_FORMAT_B8G8R8A8_UNORM || description.ArraySize != 1 || description.MipLevels != 1)
        throw std::runtime_error("Unexpected capture surface layout.");
    description.Usage = D3D11_USAGE_STAGING; description.BindFlags = 0;
    description.CPUAccessFlags = D3D11_CPU_ACCESS_READ; description.MiscFlags = 0;
    com_ptr<ID3D11Texture2D> staging;
    check_hresult(device->CreateTexture2D(&description, nullptr, staging.put()));
    context->CopyResource(staging.get(), texture.get());
    D3D11_MAPPED_SUBRESOURCE map{};
    check_hresult(context->Map(staging.get(), 0, D3D11_MAP_READ, 0, &map));
    try { save_ppm(output, map, content.Width, content.Height); }
    catch (...) { context->Unmap(staging.get(), 0); throw; }
    context->Unmap(staging.get(), 0);
    frame.Close(); session.Close(); pool.Close();
    std::cout << "Captured selected game window: " << content.Width << "x" << content.Height
        << ". Foreground unchanged during capture: " << (foreground == GetForegroundWindow() ? "yes" : "no (observation only)")
        << ". No activation, restoration, mouse or keyboard input is used.\n";
}
static int self_test() {
    uint8_t pixels[]{0,0,255,255, 0,255,0,0, 255,0,0,127};
    if (rgb_row(pixels,3) != std::vector<uint8_t>{255,0,0,0,255,0,0,0,255}) throw std::runtime_error("RGB conversion failed.");
    for (auto [width,height] : std::vector<std::pair<int,int>>{{0,1},{1,-1},{8193,1},{8192,8192}}) {
        bool rejected = false; try { dimensions(width,height); } catch (const std::runtime_error&) { rejected = true; }
        if (!rejected) throw std::runtime_error("Invalid dimensions accepted.");
    }
    dimensions(1024,768);
    std::cout << "PASS: original synthetic BGRA/RGB ordering, alpha omission and bounded dimensions. No capture or game needed.\n";
    return 0;
}
int wmain(int argc, wchar_t** argv) {
    try {
        if (argc == 2 && std::wstring(argv[1]) == L"--self-test") return self_test();
        if ((argc != 4 && argc != 5) || std::wstring(argv[1]) != L"--pid" ||
            (argc == 4 ? std::wstring(argv[3]) != L"--list" : std::wstring(argv[3]) != L"--output"))
            throw std::runtime_error("Usage: fsx-capture --pid PID --list | --pid PID --output NEW.ppm | --self-test");
        size_t consumed = 0; auto number = std::stoull(argv[2], &consumed);
        if (consumed != std::wstring(argv[2]).size() || number == 0 || number > MAXDWORD) throw std::runtime_error("Invalid PID.");
        auto pid = static_cast<DWORD>(number);
        auto process = verify_process(pid); // Retain the process handle through capture.
        Selection selection{pid,{}};
        if (!EnumWindows(enumerate, reinterpret_cast<LPARAM>(&selection))) throw_last_error();
        if (argc == 4) {
            for (auto window : selection.windows) {
                RECT bounds{}; GetClientRect(window,&bounds);
                std::cout << "Game window " << window << " | " << bounds.right << "x" << bounds.bottom
                    << " | visible " << (IsWindowVisible(window) ? "yes" : "no")
                    << " | minimized " << (IsIconic(window) ? "yes" : "no") << "\n";
            }
            return 0;
        }
        bool minimized = std::any_of(selection.windows.begin(), selection.windows.end(), [](HWND window) {
            return IsWindowVisible(window) && IsIconic(window);
        });
        selection.windows.erase(std::remove_if(selection.windows.begin(),selection.windows.end(),[](HWND window) {
            RECT bounds{};
            return !IsWindowVisible(window) || !GetClientRect(window,&bounds) || bounds.right<=64 || bounds.bottom<=64;
        }),selection.windows.end());
        if (selection.windows.empty() && minimized)
            throw std::runtime_error("The game window is minimized; no restore or activation was performed. Leave it open behind other windows to capture.");
        if (selection.windows.size() != 1) throw std::runtime_error("Expected exactly one visible game window; inspect --list first.");
        init_apartment(apartment_type::multi_threaded);
        capture(selection.windows[0], pid, argv[4]);
        return 0;
    } catch (const hresult_error& error) {
        std::wcerr << L"Window capture failed: 0x" << std::hex << static_cast<uint32_t>(error.code().value)
            << L" " << error.message().c_str() << L"\n";
    } catch (const std::exception& error) { std::cerr << "Window capture failed: " << error.what() << "\n"; }
    return 1;
}
