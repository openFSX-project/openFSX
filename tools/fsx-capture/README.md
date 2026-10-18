# Passive game-window capture

This independent Windows utility captures one explicitly selected, already-running
`fsx.exe` or `openfsx-viewer.exe` window through Windows Graphics Capture. It uses
`CreateForWindow` and a free-threaded frame pool, then reads the D3D11 surface into
a P6 PPM image. It never activates or restores a window, moves the cursor, sends
keyboard/mouse input, or captures the desktop as a fallback. Windows capture
indicators and security policy remain at their defaults. The cursor is omitted
where the OS supports that setting.

## Build and use

Requires Windows 10 version 1903 or later with capture support, a hardware D3D11
device, Visual Studio C++ Build Tools and a Windows SDK containing C++/WinRT.
The script uses installed tools; it does not download or install components.

```powershell
./scripts/build-capture.ps1
Get-Process fsx,openfsx-viewer -ErrorAction SilentlyContinue | Select-Object Id,ProcessName
./target/fsx-capture.exe --pid 12345 --list
./target/fsx-capture.exe --pid 12345 --output target/cockpit-background.ppm
node scripts/ppm-to-png.mjs target/cockpit-background.ppm target/cockpit-background.png
```

Replace `12345` with the selected game's current process ID. Leave the game
unminimized; it can be behind other windows. A minimized or unavailable window
produces an error without restoring it. Exactly one visible window with a client
area larger than 64 pixels on each axis must belong to the selected process.
No frame within five seconds also produces an error. Full-screen exclusive
rendering and driver-specific behavior have not been verified.

Output must be a new file and is limited to 16 megapixels and 8192 pixels per
axis. Keep original-game captures under ignored `target/`; they are reference
material, not screenshots of openFSX progress. The foreground-window comparison
printed after a capture is an observation, not a guarantee that another app or
the user did not change focus independently.

## Verification

The local MSVC build and synthetic self-test pass. The test checks BGRA-to-RGB
ordering, alpha omission and dimension limits without opening a window or
capturing the screen. Windows CI runs the same build and test.

On 18 October 2026, a passive capture of the running openFSX menu produced a
1024x738 frame containing both the HTML interface and native 3D aircraft preview.
The saved image was visually inspected and the foreground-window handle was
unchanged across capture. No activation, restoration or input was used.
The first original-FSX inspection found it minimized; no window was restored.
Original-FSX cockpit capture remains unverified. The openFSX result does not
establish capture compatibility with the original game's renderer.

The utility is outside the Cargo dependency graph and is not used by the engine.
Its implementation uses the public Windows APIs, with no game DLL injection or
copied SDK sample implementation.
