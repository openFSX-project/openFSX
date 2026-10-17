# Engine screenshots

These PNG images were captured on 17 October 2026 by the openFSX renderer,
using a user-installed stock C172 Blue/Gold livery at LOD 150. No FSX process
was started. The GPU was an NVIDIA RTX 2080 using the Vulkan backend. The
flight image uses the CFG station weights and full usable Avgas loading.

Both captures are direct 960 × 720 GPU readbacks, converted losslessly from
PPM to PNG. There is no FSX scenery, AI image generation, compositing,
or added instrument overlay. Flight ground/runway geometry is original
openFSX test content. The renderer is the same pipeline used by the window.

From the repository root, with your own installation:

```powershell
cargo run --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl" --lod 150 --texture-dir "./FSX/SimObjects/Airplanes/C172/texture.1" --snapshot target/c172-viewer.ppm
cargo run --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172" --fly --windows-1252 --lod 150 --seconds 10 --snapshot target/c172-flight.ppm
node scripts/ppm-to-png.mjs target/c172-viewer.ppm target/c172-viewer.png
node scripts/ppm-to-png.mjs target/c172-flight.ppm target/c172-flight.png
```

Publish selected screenshots only. Keep intermediate captures in ignored
`target/`; never copy the source MDL, DDS, AIR, CFG, SDK or executable files
into the site. Update captions and this record when refreshing captures.

Microsoft Flight Simulator X aircraft artwork remains the property of its
respective owners. The separate MIT license for the original openFSX logo
does not apply to these screenshots. Screenshots do not imply affiliation
with Microsoft or Dovetail Games, or full FSX compatibility.
