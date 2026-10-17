# Engine screenshots

The three C172 PNG images were captured on 17 October 2026 by the openFSX renderer,
using a user-installed stock C172 Blue/Gold livery at LOD 150. No FSX process
was started. The GPU was an NVIDIA RTX 2080 using the Vulkan backend. The
flight image uses the CFG station weights and full usable Avgas loading.

These three captures are direct 960 × 720 GPU readbacks, converted losslessly from
PPM to PNG. There is no FSX scenery, AI image generation or post-production
compositing. The flight panel is drawn by openFSX from live simulation state
inside the GPU renderer. Flight ground/runway geometry is original openFSX
test content. The renderer is the same pipeline used by the window.

From the repository root, with your own installation:

```powershell
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl" --lod 150 --texture-dir "./FSX/SimObjects/Airplanes/C172/texture.1" --snapshot target/c172-viewer.ppm
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172" --fly --debug-hud --windows-1252 --lod 150 --seconds 10 --snapshot target/c172-flight.ppm
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172" --fly --ground --debug-hud --windows-1252 --lod 150 --snapshot target/c172-ground.ppm
node scripts/ppm-to-png.mjs target/c172-viewer.ppm target/c172-viewer.png
node scripts/ppm-to-png.mjs target/c172-flight.ppm target/c172-flight.png
node scripts/ppm-to-png.mjs target/c172-ground.ppm target/c172-ground.png
```

Publish selected screenshots only. Keep intermediate captures in ignored
`target/`; never copy the source MDL, DDS, AIR, CFG, SDK or executable files
into the site. Update captions and this record when refreshing captures.

Microsoft Flight Simulator X aircraft artwork remains the property of its
respective owners. The separate MIT license for the original openFSX logo
does not apply to these screenshots. Screenshots do not imply affiliation
with Microsoft or Dovetail Games, or full FSX compatibility.

## Free Flight menu capture

`free-flight-menu.png` is a direct 1024 x 738 native Windows window capture of
`openfsx-viewer ./FSX --menu`, taken on 17 October 2026. It shows the independent
WebView2 menu host, installed FSX UI bitmap artwork and a static aircraft
thumbnail. It is not a live aircraft render or a screenshot of the original FSX
process. Aircraft selection, launching C172 and returning from flight were
checked. Other menu sections are incomplete; pixel parity is not verified.
The installed artwork retains its respective owners' rights. No raw bitmap or
thumbnail asset is redistributed; the selected window screenshot is published
at the project owner's request.

## Authored elevator key inspection

`c172-elevator-0.png` and `c172-elevator-100.png` are direct 960 x 720 GPU
readbacks from the manual animation inspector, taken on 17 October 2026.
They use stock C172 animation group 7, LOD 150, the same bind-bounds rear camera
and zoom 1.6. Only the selected clip/frame differs; other clips stay in bind
pose. The two images differ within pixel bounds (258, 377) to (440, 452),
with 4,000 changed RGB pixels in the tail region. This checks local key
application, not calibrated flight response or full FSX animation parity.

```powershell
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl" --lod 150 --texture-dir "./FSX/SimObjects/Airplanes/C172/texture.1" --animation 7 --frame 0 --view rear --zoom 1.6 --snapshot target/c172-elevator-0.ppm
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl" --lod 150 --texture-dir "./FSX/SimObjects/Airplanes/C172/texture.1" --animation 7 --frame 100 --view rear --zoom 1.6 --snapshot target/c172-elevator-100.ppm
node scripts/ppm-to-png.mjs target/c172-elevator-0.ppm target/c172-elevator-0.png
node scripts/ppm-to-png.mjs target/c172-elevator-100.ppm target/c172-elevator-100.png
```

The aircraft artwork rights and publication scope above also apply to these
selected captures. No original model or animation data file is included.

## Synthetic reference-pose pipeline

`c172-reference-synthetic.png` is a direct 960 x 720 GPU readback on
18 October 2026. It imports the original invented sample written by
`scripts/build-reference.ps1` into the stock C172 LOD 150 using the rear
bind-bounds camera and zoom 1.6. The declared source is **synthetic**. Seven
of 28 animation groups resolve from available direct bindings; other groups
remain in bind pose. It is not a live FSX recording, calibrated response or
reference-flight comparison. No AI image generation or compositing is used.

```powershell
./scripts/build-reference.ps1
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl" --lod 150 --texture-dir "./FSX/SimObjects/Airplanes/C172/texture.1" --reference-pose target/reference-synthetic.ndjson --view rear --zoom 1.6 --snapshot target/c172-reference-synthetic.ppm
node scripts/ppm-to-png.mjs target/c172-reference-synthetic.ppm target/c172-reference-synthetic.png
```

Changing only the synthetic elevator_pct from -2.5 to 30 changes clip 7 from
frame 47.5 to 80, with 2,661 changed RGB pixels confined to bounds (257, 380)
to (441, 445) in this backend/camera check. This checks binding isolation, not
physical accuracy. Other content, material, visibility and skinning limits
remain. Aircraft artwork retains its respective owners' rights.
