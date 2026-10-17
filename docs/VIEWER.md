# Model viewer

The native viewer reads FSX MDLX geometry and diffuse DDS textures directly. It is a development tool, not a simulator or a converter.

## Build and run

Use stable Rust, the Windows MSVC toolchain and a GPU with current Vulkan or DirectX 12 drivers. wgpu chooses an available backend:

```powershell
cargo run --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl" --lod 150 --texture-dir "./FSX/SimObjects/Airplanes/C172/texture.1"
```

Pass a model from your own installation. An explicit MDL path is required; aircraft.cfg/model.cfg variants are not yet resolved automatically.

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

The same depth-tested pipeline renders without a window and writes a binary PPM. Keep snapshots of proprietary content local under ignored `target/`; do not add them to the site or repository.

## Known limits

Diffuse textures and flat face lighting only, with approximate ordinary alpha blending and MDLX alpha tests. Transparent parts are sorted by their centers; intersecting transparent triangles are not sorted individually. Other blend modes, accurate FSX material lighting, specular/bump/reflection maps and texture mip chains are absent. No visibility expressions, animation playback, skinning, attachments or effects. Bind pose can expose normally hidden alternatives, including multiple propeller states. Startup diagnostics report these limits.

Windows x64 is the initial target. The first offscreen C172 render was verified on an NVIDIA RTX 2080 through Vulkan. Building on another platform or backend does not establish rendering support there.
