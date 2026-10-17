# Model viewer

The native viewer reads FSX MDLX geometry and diffuse DDS textures directly. It is a development tool, not a simulator or a converter.

The same executable also has an experimental --fly mode with an aircraft-folder argument. Its keyboard bindings and simulation behavior differ from orbit inspection: see the [flight guide](FLIGHT.md).

## Build and run

Use stable Rust, the Windows MSVC toolchain and a GPU with current Vulkan or DirectX 12 drivers. wgpu chooses an available backend:

```powershell
cargo run --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl" --lod 150 --texture-dir "./FSX/SimObjects/Airplanes/C172/texture.1"
```

For orbit inspection pass an explicit MDL path from your own installation. Flight mode resolves exterior model and texture paths through the selected aircraft.cfg/model.cfg variant.

## Texture selection

Use --texture-dir to choose a livery directory. Without it the viewer looks for the aircraft's sibling texture folder. Use --no-textures for a material-color preview; it cannot be combined with --texture-dir.

The selected directory takes precedence over its texture.cfg fallbacks, visited in numeric order. Case-insensitive ASCII file lookup handles MDLX uppercase names on case-sensitive filesystems. Cycles are ignored and search is limited to 64 directories. No file in the installation is modified.

The decoder supports the top mip of ordinary 2D DXT1, DXT3, DXT5 and masked 32-bit RGB DDS. Dimensions are limited to 4096, input files to 256 MiB, and the total selected diffuse textures to 256 MiB decoded. Cubemaps, volumes, DX10 formats, BMP and other compression variants are rejected with a diagnostic. Missing textures use material-color fallback. Generated registration decals are not available as ordinary DDS files and remain unsupported.

## Controls

| Input | Action |
| --- | --- |
| Left mouse drag | Orbit around model bounds |
| Mouse wheel | Zoom within distance limits |
| Arrow keys | Rotate camera |
| R | Reset framing |
| Esc | Close viewer |

Rendering is event driven and does not redraw continuously while idle. Resizing rebuilds the depth buffer. Zero-size minimized windows are not configured as render surfaces.

## LOD selection

```powershell
cargo run -- inspect-model "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl"
```

The inspector prints each LOD's parts, triangle count, skipped skinned parts and transformed bounds. Use `--lod N` to choose a level; the default is the highest numeric LOD. Malformed data produces an error instead of unchecked indexing.

## Offscreen validation

```powershell
cargo run --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl" --lod 150 --texture-dir "./FSX/SimObjects/Airplanes/C172/texture.1" --snapshot target/c172.ppm
```

The same depth-tested pipeline renders without a window and writes a binary PPM. Keep working snapshots under ignored `target/`. Selected PNG engine screenshots are published in the website gallery at the project owner's request; original game resource files remain excluded. Use `node scripts/ppm-to-png.mjs target/c172.ppm target/c172.png` for lossless conversion.

## Known limits

Diffuse textures and flat face lighting only, with approximate ordinary alpha blending and MDLX alpha tests. Transparent parts are sorted by their centers; intersecting transparent triangles are not sorted individually. Other blend modes, accurate FSX material lighting, specular/bump/reflection maps and texture mip chains are absent. No visibility expressions, animation playback, skinning, attachments or effects. Bind pose can expose normally hidden alternatives, including multiple propeller states. Startup diagnostics report these limits.

Windows x64 is the initial target. The first offscreen C172 render was verified on an NVIDIA RTX 2080 through Vulkan. Building on another platform or backend does not establish rendering support there.

## FSX-style menu on Windows

```powershell
cargo run --features viewer --bin openfsx-viewer -- "./FSX" --menu
```

The menu host requires the Microsoft Edge WebView2 Runtime. It reads the local
installation's `uires` bitmap skin and aircraft thumbnails; these assets are not
included in openFSX. No original executable code is loaded. The independent
HTML/CSS layout is not yet pixel-verified against FSX.

**CHANGE...** under Current Aircraft opens the installation catalog, with a
manufacturer filter and variation selection. **FLY NOW!** starts the selected
variant on the experimental test runway. Unsupported aircraft report an error;
being listed does not establish flight support. **Esc** closes the flight window
and restores the menu. Native C172 launch and return were checked on Windows.

Location, weather, time/season, fuel/payload editing, flight plans, saved flights,
failures and other sidebar sections remain unimplemented. The aircraft image is
an installed thumbnail, not a live model preview. Only windowed launch is
supported. The menu catalog currently explicitly decodes CFG files as
Windows-1252. Flight telemetry is hidden by default; use **--debug-hud** when
launching flight directly to enable the independent debugging overlay.

The host serves only its own HTML/catalog, an exact bitmap allowlist and indexed
thumbnail paths canonicalized inside the installation. It does not serve the
FSX directory as a general web root or expose filesystem paths in the catalog.
