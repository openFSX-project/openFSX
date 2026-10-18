# Model viewer

The native viewer reads FSX MDLX geometry and supported diffuse DDS/BMP textures directly. It is a development tool, not a simulator or a converter.

The same executable also has an experimental --fly mode with an aircraft-folder argument. Its keyboard bindings and simulation behavior differ from orbit inspection: see the [flight guide](FLIGHT.md).

## Build and run

Use stable Rust, the Windows MSVC toolchain and a GPU with current Vulkan or DirectX 12 drivers. wgpu chooses an available backend:

```powershell
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl" --lod 150 --texture-dir "./FSX/SimObjects/Airplanes/C172/texture.1"
```

For orbit inspection pass an explicit MDL path from your own installation. Flight mode resolves exterior model and texture paths through the selected aircraft.cfg/model.cfg variant.

## Texture selection

Use --texture-dir to choose a livery directory. Without it the viewer looks for the aircraft's sibling texture folder. Use --no-textures for a material-color preview; it cannot be combined with --texture-dir.

The selected directory takes precedence over its texture.cfg fallbacks, visited in numeric order. Case-insensitive ASCII file lookup handles MDLX uppercase names on case-sensitive filesystems. Cycles are ignored and search is limited to 64 directories. No file in the installation is modified.

The decoder supports the top mip of ordinary 2D DXT1, DXT3, DXT5 and masked 32-bit RGB DDS, plus 4/8/24-bit uncompressed BITMAPINFOHEADER BMP files. DDS/BMP selection uses the file signature. Dimensions are limited to 4096, input files to 256 MiB, and the total selected diffuse textures to 256 MiB decoded. Cubemaps, volumes, DX10 formats, compressed/extended BMP variants and other compression variants are rejected with a diagnostic. Missing textures use material-color fallback. Generated registration decals are not available as ordinary DDS files and remain unsupported.

## Static panel artwork inspection

This separate offscreen example decodes the stock C172 gauge-library bitmap
resources and binds the static root images to the configured interior textures.
It does not run native callbacks, draw needles/moving layers/digital values or
change the default flight view's blank gauges. Use a new output directory and
the exact resource language ID (1033 in the checked installation):

```powershell
cargo run --locked -p fsx-render --example panel-artwork-check -- "./FSX" target/panel-artwork-01 1033
```

The check decodes all 1,022 bitmap resources from the five native libraries used
by the C172 VC, plus seven BMP files in its panel folder. It renders 28 static
root images through the two 512x512 panel textures, with the XML registration
plaque reported unsupported. Before/after 960x720 snapshots use the same camera,
geometry and GPU device. This is unlit source-artwork composition followed by
the viewer's approximate material lighting, not operational instruments or
verified FSX panel rasterization. Raw panel atlases stay under ignored target.

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
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl" --lod 150 --texture-dir "./FSX/SimObjects/Airplanes/C172/texture.1" --snapshot target/c172.ppm
```

The same depth-tested pipeline renders without a window and writes a binary PPM. Keep working snapshots under ignored `target/`. Selected PNG engine screenshots are published in the website gallery at the project owner's request; original game resource files remain excluded. Use `node scripts/ppm-to-png.mjs target/c172.ppm target/c172.png` for lossless conversion.

## Known limits

Diffuse textures and flat face lighting only, with approximate ordinary alpha blending and MDLX alpha tests. Transparent parts are sorted by their centers; intersecting transparent triangles are not sorted individually. Other blend modes, accurate FSX material lighting, specular/bump/reflection maps and texture mip chains are absent. Visibility supports a bounded read-only numeric subset, described below. Automatic animation drivers, skinning, attachments and effects remain incomplete. Manual native-key inspection is described below. Unavailable visibility variables can expose normally hidden alternatives, including multiple propeller states. Startup diagnostics report these limits.

Windows x64 is the initial target. The first offscreen C172 render was verified on an NVIDIA RTX 2080 through Vulkan. Building on another platform or backend does not establish rendering support there.

## FSX-style menu on Windows

```powershell
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX" --menu
```

The menu host requires the Microsoft Edge WebView2 Runtime. It reads the local
installation's `uires` bitmap skin and aircraft thumbnails; these assets are not
included in openFSX. No original executable or gauge DLL is loaded. The independent
HTML/CSS layout is not yet pixel-verified against FSX.

**CHANGE...** under Current Aircraft opens the installation catalog, with a
manufacturer filter and variation selection. **FLY NOW!** starts the selected
variant on the experimental test runway. Unsupported aircraft report an error;
being listed does not establish flight support. **Esc** closes the flight window
and restores the menu. Native C172 launch and return were checked on Windows.

Location, weather, time/season, fuel/payload editing, flight plans, saved flights,
failures and other sidebar sections remain unimplemented. The aircraft image is
a live native MDLX/DDS preview. It slowly rotates; drag with the left mouse button to change the view and use the wheel to zoom. Selection reloads the exterior model and livery without requiring a supported AIR flight model. The renderer hides behind menu dialogs and while flight runs. A failed model load clears the previous aircraft and reports an error. Only windowed launch is
supported. The menu catalog currently explicitly decodes CFG files as
Windows-1252. Flight telemetry is hidden by default; use **--debug-hud** when
launching flight directly to enable the independent debugging overlay.

The host serves only its own HTML/catalog, an exact bitmap allowlist and indexed
thumbnail paths canonicalized inside the installation. It does not serve the
FSX directory as a general web root or expose filesystem paths in the catalog.

## Inspect authored animation keys

List the model's animation catalog first:

```powershell
cargo run -- inspect-model "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl"
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl" --lod 150 --texture-dir "./FSX/SimObjects/Airplanes/C172/texture.1" --animation 7 --frame 50 --view rear --zoom 1.6
```

For the verified stock C172, animation 7 is the elevator group. Indexes are
file-local, not a universal aircraft API. **[** and **]** scrub five authored
keyframes at a time. The title displays the selected clip and frame. Rotation
uses normalized XYZW quaternions and shortest-arc spherical interpolation;
translation is linear. Native key deltas compose after the static local node
matrix, then inherit parent transforms. Draw topology, texture coordinates and
material selection stay fixed while positions, face normals and draw centers
update. No original animation DLL or native implementation is loaded.

`--view front|rear` and `--zoom N` set a reproducible initial orbit camera.
These options and `--animation` are for model inspection, not flight mode.
The camera initially uses bind-pose bounds so two frame captures have identical
framing. R resets the orbit around the current pose. Add `--snapshot path.ppm`
to capture a chosen pose without opening a window.

Manual clip/frame selection is separate from flight animation. Direct Sim
bindings expose variable, units, scale, bias and optional lag. Supported XAPC
expressions use the bounded interpreter. Static inspection remains read-only;
temporal animation also supports model-local numeric data and scratch registers.
Flight currently supplies
physical center-wheel steering, throttle lever position and on-ground state;
unknown values remain unavailable. Control-surface percent mappings, RPM,
Standard autoplay timing/randomization and skin deformation still require work.
Other clips remain in bind pose; mutually exclusive propeller
and wheel alternatives can still be visible together. A clip with no geometry
tracks is reported as such and has no visual effect.

Local C172 evidence: 28 groups decode, 27 with rotation/translation tracks.
Elevator frames 0 and 100 rendered through the same Vulkan path with identical
camera framing; differences were confined to the tail region. Native **]**
input advanced frame 50 to 55 and presented the updated geometry. This proves
the inspected path, not FSX animation parity for all aircraft or channels.

In flight mode, Q/E steering now updates C172 clip 4 automatically from the same
CFG-derived angle used in wheel contact forces. The temporal evaluator uses
simulation elapsed time, so pause holds visual state and R reinitializes it.
Positive Lag bounds motion in authored keyframes per second. Unknown targets
hold the last available pose; the first available target initializes immediately.
These policies and the steering direction still lack an original-FSX comparison.

Run the local-content controller-to-GPU check without opening a window:

```powershell
cargo run --locked -p fsx-app --features viewer --example flight-pose-check -- ./FSX target/flight-pose-check
```

The output directory must be new. The check drives the shared flight keyboard
controller directly, advances physics, evaluates the native animation, saves
center/right/left/full-power P6 renders, and verifies pause/reset. It sends no OS input.
Its fixed model-space camera isolates deformation from aircraft translation.
The tested C172 CFG has 22 degrees maximum center steering: controller values
0 and +/-0.5 produce angles 0 and +/-11 degrees, or +/-12.222222 grads.
Its native XAPC selects frames 200, 6.111111 and 193.88889. The two turning
captures differ at 498 pixels within (591,469)..(617,502). This verifies the
independent physics-to-rendering path, not FSX steering fidelity.

## Model-local throttle expression and cockpit check

The C172 throttle XAPC uses L: variables, s0/l0 scratch storage and a local
write. Temporal evaluation now resolves it from the independent throttle state:
idle selects native frame 50 and full power selects frame 0, with the same value
stored in Engine1ThrottlePosition (percent). The exterior resolves two of 28
groups; an exterior LOD-150 check showed no visible throttle pixel difference
from the external camera. That result alone does not prove visible movement.

For a direct view of the control, use the same controller-to-GPU check with the
installed stock C172 interior model:

```powershell
cargo run --locked -p fsx-app --features viewer --example flight-pose-check -- ./FSX target/cockpit-throttle-check --interior
```

This local validation option selects cessna172sp_interior.mdl, its LOD 100
(24,842 triangles, 55 scene nodes, 42 animation groups), and a fixed camera aimed
at the throttle. Its eye [-0.25908, 0.57912, -0.10668] metres is derived from the
observed stock C172 CFG datum/eyepoint; target [0, -0.1, 0.5] is chosen for this
inspection, not FSX default-camera parity. One of 42 groups resolves. Seven of
nine diffuse maps load (18 MiB decoded); dynamic $C172S_1/$C172S_2 panel surfaces
are unavailable, so gauges remain blank. This control-inspection example is
separate from the flight camera below; it does not implement live gauges or
panel interaction.

The verified idle/full-power renders differ at 1,908 pixels confined to the
throttle region (397,166)..(447,222). Pose and local values follow model code;
pause holds state even if a power key changes the pending control, and reset
reinitializes the local context. Program writes commit only on valid results.
Registers are temporary; local values are shared between this instance's clips,
with matching units required. Other-aircraft/gauge sharing and original-FSX
timing, scope and control-position comparison remain unverified.

## Cockpit flight camera

```powershell
cargo run --locked -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172" --fly --ground --cockpit --windows-1252
```

Omit --ground for the 90 kt / 300 m airborne start. F9 selects/recenters the
virtual cockpit and F11 returns to the exterior chase view. The default FSX
Standard.xml maps F9/F11 to VIEW_CAMERA_SELECT_1/3; the installed cameras.cfg
assigns those slots to Virtual Cockpit/Locked Spot. Those associations were
checked locally; original files are not copied into openFSX. The prototype's
exterior camera remains its own chase view, not FSX Locked Spot behavior.

The loader reads [models] interior from the selected variant's model.cfg, and
[views] eyepoint plus reference_datum_position from aircraft.cfg. Feet convert
to body forward/right/down metres, then to the same loaded-CG/world transform
as the aircraft mesh. Camera up follows aircraft attitude, including bank.
Missing interior/eyepoint disables cockpit selection; --cockpit reports an
error. Invalid vectors and non-component model references fail explicitly.

In cockpit view, left drag looks around and the wheel changes vertical FOV
between 15 and 90 degrees; F9 recenters at a nominal 45 degrees. This free-look
gesture, initial FOV, camera definitions, InitialZoom/InitialPbh, head movement
and clip behavior have not been matched to FSX. The near plane is 0.01 m.
The [camera SDK](https://www.prepar3d.com/SDKv3/LearningCenter/utilities/camera_configuration/camera_configuration.html)
describes the broader system; the current camera implements only this subset.
Dynamic gauge surfaces are still blank and panel clicks are not implemented.
Use --debug-hud if experimental telemetry is needed. The outside world remains
the original test plane/runway, with no FSX scenery loaded.

Both visual models retain and update their animation states. Selection exchanges
mesh/material resources on the existing GPU device and surface; it does not
recreate the window or reset physics. --lod applies to the starting model; the
alternate model initially selects its highest available LOD. A failed optional
alternate load leaves the starting view usable, with a diagnostic.

For a reproducible check without a window or OS input:

```powershell
cargo run --locked -p fsx-app --features viewer --example flight-view-check -- ./FSX target/flight-view-check
```

The output directory must be new. The shared camera/resource-exchange code was
checked at ground start, level flight and after one second of right-bank input.
The last state reports 90.1 kt, 300 m and 15.7 degrees right bank. Its horizon
tilts while cockpit geometry stays attached to the aircraft. For all three
states, exterior -> cockpit -> exterior produces byte-identical exterior GPU
readbacks and leaves simulation time, position, orientation and throttle unchanged.
The common projection now uses the native Direct3D/MDLX handedness, correcting
previously mirrored left/right geometry and bank direction. All published GPU
captures were rerendered after this correction. Release CLI --cockpit snapshots passed for one-second ground
and ten-second airborne runs. These checks verify the shared rendering path;
native F9/F11 input has been wired and compiled but was not injected into a
desktop window during this verification.

## Reference calibration

`tools/fsx-reference` contains a separate Windows x86 reader for the original
FSX process. Build it with `./scripts/build-reference.ps1`, load a stock C172
flight in FSX and capture with:

```powershell
./target/fsx-reference.exe --fsx "./FSX" --output target/fsx-controls.ndjson --seconds 30
```

This reads surface angles, percentages, trim, yoke, throttle, on-ground state
and aircraft title. It sends no control events. Record exact loading/settings
and the input sequence alongside each local capture. Synthetic protocol checks
pass. A loaded local C172 flight returned 87 valid packets over five seconds
and another 35 over two seconds on 18 October 2026. User-aircraft request alias
0 resolved to actual object ID 1, which the reader validates and binds.
This establishes baseline acquisition; control sweeps and angle/percentage
calibration remain unverified. A failed connection or an empty output is not
calibration evidence. Flight mode has an initial center-steering binding; control-surface mapping remains uncalibrated. Offline pose import is
available for supported recorded bindings, as described below.

## Inspect one captured pose

```powershell
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl" --lod 150 --texture-dir "./FSX/SimObjects/Airplanes/C172/texture.1" --reference-pose target/fsx-controls.ndjson --reference-sample 0 --view rear --zoom 1.6
```

The sample index is zero-based. Import validates schema 1, the selected row's
12 finite values, aircraft title, time and source label. Files are limited to
64 MiB, rows to 16 KiB and indexes to fewer than 100,000. The window title and
console identify the declared source and sample. Use the matching aircraft MDL;
the tool prints the recorded title but cannot prove that it matches an arbitrary
model file. Source labels are metadata, not proof of a capture's authenticity.

Sample 0 from the locally recorded 87-packet C172 baseline was imported and
rendered offscreen. Its seven available XAPS control/trim bindings resolve to
frame 50. All recorded control readings were neutral, so this verifies the live
acquisition-to-pose pipeline without establishing motion, signs or calibration.

The adapter resolves known variables in their requested units, then uses the
model's own XAPS scale and bias to choose keyframes. Percent and percent-over-100
conversion and degree/radian conversion are explicit. Unknown variables/units,
unsupported code, positive lag and standard autoplay are skipped with diagnostics.
Shared transform IDs compose in file order; full multi-clip FSX equivalence
remains unverified. This is one static pose, not temporal replay or flight input.
It cannot be combined with --fly or manual --animation/--frame. Add --snapshot
for offscreen output through the same renderer.

For variables outside the recorded schema, repeat `--pose-var NAME#units#value`.
This supplies explicit inspection values, not FSX measurements, and resolves
both XAPS and supported read-only XAPC expressions. The source code chooses
authored frames; no manual frame mapping is added for individual aircraft.
The option cannot be combined with flight, a recorded pose or manual animation.
Up to 128 distinct variable/unit pairs are allowed, with finite f32-range values.

```powershell
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl" --lod 150 --texture-dir "./FSX/SimObjects/Airplanes/C172/texture.1" --pose-var "GEAR CENTER STEER ANGLE#grads#40" --visibility-var "PROP MAX RPM PERCENT:1#percent#0" --view front --zoom 1.6 --snapshot target/c172-steer.ppm
```

On the local C172, explicit steering values +40 and -40 in the model-requested
grads unit resolve XAPC clip 4 to frames 20 and 180. Offscreen GPU snapshots
from the same camera differ at 516 pixels, confined to the nose-wheel region
(593,470)..(617,502). This verifies the native expression-to-geometry path;
it does not establish steering calibration or a match to an FSX reference frame.
The menu now supplies zero steering to this same pose evaluator for its
stationary preview. That menu wiring is compiled; the changed native menu has
not been reopened for visual verification during this check.

For a reproducible pipeline check without an FSX server:

```powershell
./scripts/build-reference.ps1
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl" --lod 150 --texture-dir "./FSX/SimObjects/Airplanes/C172/texture.1" --reference-pose target/reference-synthetic.ndjson --view rear --zoom 1.6 --snapshot target/c172-reference-synthetic.ppm
```

The helper generates this original test sample with source **synthetic**; it
contains invented values, not FSX measurements. On the local C172 it resolves
seven of 28 animation groups and renders the posed aircraft. This checks the
C# encoder, JSON import, unit adapter, authored key evaluation and GPU path.
It does not establish physical calibration or FSX animation parity.

Earlier Remote Desktop and menu-only attempts returned no aircraft samples.
The later successful captures above required a loaded local C172 flight; the
offline openFSX pipeline does not launch FSX.

## Passive reference screenshots

The independent `tools/fsx-capture` Windows utility captures only the selected
game window through Windows Graphics Capture. It never activates or restores
the window or sends mouse/keyboard input. Leave FSX unminimized behind other
windows; minimized windows are rejected without restoring them. Windows capture
indicators and security policy keep their defaults; there is no desktop fallback.

```powershell
./scripts/build-capture.ps1
./target/fsx-capture.exe --pid 12345 --list
./target/fsx-capture.exe --pid 12345 --output target/fsx-reference.ppm
```

Replace `12345` with the running game's process ID. The build needs installed
Visual Studio C++ tools and a Windows SDK with C++/WinRT headers. Output must
be a new file. Local compilation and synthetic pixel/dimension checks pass.
A 1024x738 passive capture of the openFSX menu was visually verified, including
the native 3D preview; the foreground-window handle was unchanged. Original-FSX
cockpit capture remains unverified: it was minimized during the first inspection,
and the later available game window belonged to openFSX. Keep original-game
images under ignored `target/` as reference material. They are not screenshots
of openFSX progress.

## Visibility inspection

VISL conditions link to scene nodes through SGVL. The reader supports direct
VISS/VINS variable reads and a numeric, read-only VISC/VINC RPN subset: finite
numbers, A-variable reads with explicit units, arithmetic, comparisons, boolean
operators and lazy nested if/else branches. Defaults encoded by the file apply
to unavailable variables, unsupported operations and invalid evaluation, with
diagnostics. Hiding a parent hides its descendants. Draw masks preserve vertex
buffers, material ranges and framing; both opaque and blended draws honor them.

Supply exact variable/unit values for inspection, for example:

```powershell
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl" --lod 150 --texture-dir "./FSX/SimObjects/Airplanes/C172/texture.1" --visibility-var "PROP MAX RPM PERCENT:1#percent#0" --view front --zoom 1.6 --snapshot target/c172-visibility-stopped.ppm
```

Repeat --visibility-var for different variables; at most 128 unique name/unit
pairs are accepted. Matching is ASCII case-insensitive, without inferred unit
conversion. Values must be finite. Set the example value to 8 or 20 to inspect
the intermediate or blurred propeller alternative. These are explicit inputs,
not captured RPM. The option cannot override --fly or --menu; a selected mask
is preserved while manually scrubbing animation keys.

The menu supplies a stationary, engine-stopped inspection state: propeller
percent and wheel rotation rates zero, landing lights off. On the local C172,
all 61 conditions resolve and seven LOD-400 draw batches are hidden. This is a
presentation state, not a simulation of propeller mechanics or FSX menu parity.
Flight still lacks engine/wheel RPM state, so unknown conditions retain their
encoded defaults. Model visibility does not implement gauge XML, local variables,
assignment/events, registers, loops, strings, native gauge DLLs or temporal visibility lag. Evaluation must leave exactly one finite scalar result.
The RPN interpreter bounds source size (16 KiB), instructions (4096), nesting
(32) and stack entries (64), and never executes native or original gauge code. A model visibility block is limited to 4 MiB and 262,144 compiled instructions in total.
