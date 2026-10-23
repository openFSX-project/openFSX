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

## Native needle result inspection

The separate native-needle-check example consumes the explicit output of the
x86 update probe. It draws six original bitmap needles using native callback
results, authored POS/OFS pivots and nonlinearity tables. The three selected
altimeter callbacks are explicitly interpreted as degrees when no table exists;
the example does not generalize that interpretation to other table-free gauges.

```powershell
./scripts/build-gauge-host.ps1
./scripts/check-native-gauges.ps1 -FsxRoot ./FSX -Output target/needle-inputs.ndjson
cargo run --locked -p fsx-app --features viewer --example native-needle-check -- ./FSX target/needle-inspection target/needle-inputs.ndjson
```

Use new output paths. The check renders three 960x720 snapshots on one GPU
device, with a fixed camera and 28 static root images. It preserves the authored
next-list order for overlapping needles and reads exact language 1033 resources.
Six bindings are included: airspeed, tachometer, vertical speed and three
altimeter needles. The input states are explicit raw probe values, not a flight
recording. Schema, context, module lifecycle, resource/token IDs, header versions,
finite values and sample counts are checked before use.

This path does not execute DLLs from Rust, run temporal needle lag, supply
pressure/air-data/calibration behavior or integrate instruments into --fly.
Other moving/static layers, digital values, masks and panel interaction remain
omitted. Original-FSX visual/behavioral parity has not been measured.

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

## Recorded instrument indications

The separate reference reader accepts `--instruments` to add 32 air-data,
gyro, clock, loading, engine and instrument-state readings to the existing
12 controls in one response packet. This schema-2 live request is not yet
verified against a running FSX. Default schema-1 capture remains available.

```powershell
./scripts/build-reference.ps1
./target/fsx-reference.exe --fsx ./FSX --output target/fsx-instruments.ndjson --seconds 30 --instruments
```

Load the matching C172 variant-0 flight first and record its settings. Source
labels alone do not prove a file's authenticity. To validate or replay a schema-2
recording through the checked C172 native panel profile:

```powershell
cargo run --locked -p fsx-app --features viewer --example reference-gauge-replay -- --validate target/fsx-instruments.ndjson
./scripts/build-gauge-host.ps1
cargo run --locked -p fsx-app --features viewer --example reference-gauge-replay -- ./FSX target/fsx-instruments.ndjson target/reference-gauge-replay
```

Replay streams bounded records and preserves the absolute simulator clock's
pause/rewind behavior independently of wall time. It uses actual recorded IAS,
indicated altitude, VSI and gyro readings as native inputs. Failed/caged or
adjusted instruments and changing values on an unchanged simulator clock are
not supported by this profile. Real-source titles must match the loaded variant.
Schema-2 files can also supply the existing `--reference-pose` inspector, with
explicit unit conversions and their original source/title labels.

Available stateless model-animation bindings also consume recorded control and
instrument variables; unsupported expressions and positive lag remain omitted.
The new output directory contains fixed-camera first/last images, per-frame
native callback results and an explicitly non-parity diagnostic report. ISA,
body/gyro and pressure differences describe assumptions to check against future
real data. Aircraft trajectory, lag and complete instrument state are not replayed.

The C# synthetic fixture passed the Rust validator and native render path. A
five-record synthetic sequence exercised one pause reuse and one rewind/helper
restart; its 600 fpm reading returned 600.0000192 through the native VSI callback.
No original FSX process was running during this check. See the measurement
tool's repository README for the complete schema and native-profile limits.

## Inspect one captured pose

```powershell
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172/model/cessna172sp.mdl" --lod 150 --texture-dir "./FSX/SimObjects/Airplanes/C172/texture.1" --reference-pose target/fsx-controls.ndjson --reference-sample 0 --view rear --zoom 1.6
```

The sample index is zero-based. Import validates schema 1, the selected row's
12 finite control values, aircraft title, time and source label. Schema 2 additionally
requires its 32 finite instrument fields and object/index metadata. Files are limited to
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

## Experimental live native C172 needles

Windows flight mode can opt into the five checked native needle bindings:

```powershell
./scripts/build-gauge-host.ps1
cargo run --locked -p fsx-app --features viewer --bin openfsx-viewer -- ./FSX/SimObjects/Airplanes/C172 --fly --cockpit --native-gauges --windows-1252
```

Add `--xml-gauge-font C:/Windows/Fonts/arial.ttf` to draw the XML registration
plaque using that explicitly selected installed font. No font is bundled or
silently chosen for the XML's missing FontFace. This option uses authored atc_id,
the current daylight environment and panel-light value false. FontHeight uses
pixels per em; FSX default-font/GDI metrics and filtering remain unverified.
The normal native profile keeps its XML omission unless a font is supplied.

```powershell
cargo run -p fsx-app --features viewer --example xml-text-render-check -- FSX target/xml-text-check C:/Windows/Fonts/arial.ttf
cargo run -p fsx-app --features viewer --example native-gauge-flight-check -- FSX target/xml-flight-check target/fsx-gauge-host.exe C:/Windows/Fonts/arial.ttf
```

Both output directories must be new. The first example checks two authored
registrations and two synthetic text-color conditions through GPU snapshots;
both use the authored day background and do not simulate cockpit night lighting.
The second checks XML persistence with live native needles/sprites, pause/reset,
view changes and helper closure. Neither launches original FSX.

The helper defaults to `fsx-gauge-host.exe` beside the viewer or in its parent
build directory. Use `--gauge-host PATH` to select it explicitly. This option
requires Windows, flight mode, textures and the checked C172 VC placements.
It does not enable instruments in the menu's FLY NOW launch path.

A persistent, hidden x86 process loads the installed Cessna/Cessna172/Bell_206B DLLs,
calls ModuleInit once and updates the airspeed, vertical-speed and three
altimeter needles. The independent flight model supplies ideal ISA calibrated
airspeed in knots, pressure altitude in feet at the standard pressure setting,
and vertical speed in the legacy 1/256 m/s token units. This assumes subsonic
flow and local ISA height between 0 and 11,000 m; unsupported data fails
explicitly. Instrument errors, weather and adjustable pressure settings are
not modeled. The renderer paints native needle bitmaps over decoded static
panel artwork using authored pivots and curves, and updates the existing GPU
textures without replacing scene geometry.

The same option draws the native heading card and both attitude sprites with
the original RES_ID+1 masks and foreground artwork. Ideal gyro pitch/bank and
local heading come from the aircraft orientation; legacy inputs are nose-down
and left-bank positive. Original token-78 pitch and token-84 heading callbacks
run in the helper; bank axes use their encoded direct source. Authored scales
apply once, with rotation before texture scaling and source translation.
Pixels use nearest-neighbor sampling of the installed source bitmap. RGB zero
in a mask preserves the preceding layer, RGB (1,1,1) opens the moving texture,
and other colors draw stationary foreground. Reticle and adjustment artwork
use neutral authored placement. This ideal gyro profile has no magnetic
variation, drift, failures, erection dynamics or knob adjustment.

Callbacks run only when simulation time advances. Pause reuses results; a
backward clock reset closes/deinitializes the old modules and starts a fresh
helper. F9/F11 preserve the latest interior textures, including updates while
viewing the exterior. Normal exit deinitializes all three modules; protocol errors,
disconnects and callback timeouts terminate the helper and report failure.

Run the installed-content integration check in a new output directory:

```powershell
cargo run --locked -p fsx-app --features viewer --example native-gauge-flight-check -- ./FSX target/native-gauge-flight-local
```

On 19 October 2026 the local RTX 2080/Vulkan check passed 121 forward frames,
ten paused result reuses, dynamic GPU texture updates, exterior/interior round
trips, helper restart and identical initial/reset pixels. With the camera and
geometry fixed, flight-driven needles changed 1,706 RGB bytes. The settled
ground airspeed was 0.00136 kt, matching residual physics velocity. These are
openFSX consistency checks, not recorded FSX instrument comparisons. Native
window input with this option has not been rechecked.

The extended integration check subsequently passed all three flight-driven
sprites, 121 forward frames and ten pause reuses. Frozen camera/geometry pixels
changed in 12,161 RGB bytes during combined pitch/roll input; reset and view
round trips retained identical expected pixels. A separate explicit-artwork
check saves level, nose-up/down, right-bank and east-heading images:

```powershell
cargo run --locked -p fsx-render --example native-sprite-check -- ./FSX target/native-sprite-local
```

Visual inspection confirmed that nose-up lowers the horizon, right bank tilts
it left, and heading 90 places E under the top index. Local source art is 84/93
pixels wide; openFSX samples those bitmaps directly, without a 256-pixel runtime
texture conversion. FSX filtering and subpixel placement remain unverified.

RPM, other moving faces, broader digital/XML gauges, needle lag, interaction and
the full PANELS/gauge lifecycle remain unsupported.
Static root artwork can display fixed or empty regions for those instruments;
it must not be treated as operational telemetry. Native DLL code remains in
the separate helper; the x64 renderer does not load it.

Research references: [gauge API](https://prepar3d.com/SDKv4/sdk/panels_and_user_interface/gauges/references/gauges_api_overview.html),
[published ABI declarations](https://prepar3d.com/SDKv4/sdk/panels_and_user_interface/gauges/references/gauges.html)
and [gauge artwork conventions](https://www.prepar3d.org/SDKv4/sdk/panels_and_user_interface/panels/panel_and_gauge_artwork.html).
These later SDK references informed independent readers and rendering;
local FSX resources and synthetic tests establish the checked subset.

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

## Native local-time display in flight

Add an explicit local time origin to the C172 native profile:

```powershell
cargo run -p fsx-app --features viewer --bin openfsx-viewer --locked -- FSX/SimObjects/Airplanes/C172 --fly --cockpit --native-gauges --native-clock-start 23:59:59 --windows-1252
```

HH:MM:SS must be within 00:00:00..23:59:59. Time advances with fractional elapsed
simulator seconds (the HMS display uses whole seconds), wraps at midnight, pauses with flight and returns to the
origin on reset. The helper maintains one connected C172 clock, writes its
callback output into a guarded buffer and restores that pointer before native
disconnect/module teardown. The renderer loads the matching installed Quartz
font and uses authored daylight styling, unselected by default. The option
requires `--native-gauges`; it does not read or set the host's wall clock.

For reproducible flight/GPU boundaries, include the explicit XML plaque font
and clock origin in the installed-content checker:

```powershell
cargo run -p fsx-app --features viewer --example native-gauge-flight-check --locked -- FSX target/native-clock-flight target/fsx-gauge-host.exe C:/Windows/Fonts/arial.ttf 23:59:59
```

The check covers native session lifetime, flight/pause/reset/view/ground behavior,
clock second/minute/day transitions and paused/rewound pixel equality while
holding other instrument results fixed. Calendar, timezone, UTC synchronization,
screen routing, secondary clock text, running-stopwatch midnight/time edits,
electrical/lighting changes, full panel services and
GDI/FSX pixel parity remain unsupported or unverified. Omitting the clock option
preserves the existing schema-2 flight/replay profile.

### Clock event injection and selection

Repeated `--clock-event CLOCK_EVENT[:u32]` applies supported events after the
initial native frame, in order. `--native-clock-select` selects none, hours,
minutes or seconds through the installed SEQ_REC table. Both require
`--native-clock-start`. For example:

```powershell
cargo run -p fsx-app --features viewer --bin openfsx-viewer --locked -- FSX/SimObjects/Airplanes/C172 --fly --cockpit --native-gauges --native-clock-start 23:59:59 --clock-event CLOCK_HOURS_SET:13 --clock-event CLOCK_MINUTES_SET:34 --clock-event CLOCK_SECONDS_ZERO --native-clock-select minutes --windows-1252 --seconds 0.1 --snapshot target/clock-events.ppm
```

Supported names are CLOCK_HOURS_INC/DEC/SET, CLOCK_MINUTES_INC/DEC/SET and
CLOCK_SECONDS_ZERO. SET uses an unsigned hour 0..23 or minute 0..59; other events
ignore their payload. Unsupported names/invalid values fail explicitly; at most
128 startup events are accepted. This is startup injection, with no automatic
keyboard or mouse binding and no SimConnect service exposed.

The underlying clock event and selection APIs also work while physics is paused:
only the native clock callback is updated, other instrument results remain
cached, and dirty display state reaches the GPU. Invalid/stale-time edits leave
local time unchanged. Reset restores the CLI origin and clears selection.
Date/UTC/timezone and environment lighting remain outside this local-time subset.

### Native button injection in the flight profile

Repeated --native-clock-click 0/1/2 invokes the corresponding authored Clock
mouse child at its center, after the initial flight frame and CLOCK_* edits and
before explicit selection. At most 128 startup clicks are accepted; these
require --native-clock-start. For a local stopwatch mode/start inspection:

```powershell
cargo run -p fsx-app --features viewer --bin openfsx-viewer --locked -- FSX/SimObjects/Airplanes/C172 --fly --cockpit --native-gauges --native-clock-start 12:34:56 --native-clock-click 0 --native-clock-click 1 --windows-1252
```

Child 0 switches the original mode. Child 1 uses the original current-mode action
(format switching or stopwatch start/stop/reset). Child 2's original handler is
also invoked, but its secondary display state is not yet rendered. Native
handlers and their own packets control private state; no replacement stopwatch
is used. The same helper owns the needles/sprites and clock. Clicks at a paused
frame preserve physics time/evaluation counts and invalidate the clock texture.
The running stopwatch receives fractional local time; reset restores the
configured time origin and discards button state by restarting the helper.

The installed-content flight checker above now exercises all three handlers,
1.25/61.25-second progress, pause/stop/reset/restart and live GPU texture updates.
With the clock enabled in cockpit view, left press now picks the current visible
clock button from the rendered geometry and sends its actual child-local point
to the native handler. This works while physics is paused. Release, right mouse
and wheel do not invoke native clock handlers. Exterior view and nonflight orbit
retain their existing controls. Cursor exit/focus loss clears stale pointer state.
The nearest surviving surface blocks input, so hidden/covered buttons do not
receive a click through opaque foreground geometry.

The checker independently projects authored UV centers and sends those screen
points through production picking/native/GPU paths at multiple physical viewport
sizes and a rolled/translated camera. It also checks the nonactionable clock
display and hidden geometry. It does not inject OS input; manual window-event
interaction capture and original-FSX input parity remain unverified. Cursor
feedback/tooltips, drag/repeat/double-click/wheel, secondary text and other gauge
controls remain unfinished. If --seconds is used,
startup clicks happen at that initialized simulation time, before subsequent
interactive frames; they are not backdated through the offline advance.

## Recorded native clock text

Produce three guarded native strings in the isolated x86 helper, then render
those records offscreen with the matching font from the same local installation:

```powershell
cargo run -p fsx-app --example native-string-check --locked -- FSX target/fsx-gauge-host.exe target/native-clock-samples --clock
cargo run -p fsx-app --features viewer --example native-string-render-check --locked -- FSX target/native-clock-render target/native-clock-samples/samples.ndjson
```

Both output directories must be new. The renderer reads at most 64 KiB/three
records, validates the checked C172 profile and resolves metadata from the DLL.
It loads `FSX/Fonts/quartz.ttf`, fits the authored eight-character width, uses
the native position/colors/alignment, and composites exactly one cockpit clock.
Four GPU snapshots cover three times plus a restored first state. The check
requires distinct GPU pixels and identical restored pixels; report.json records
font metrics, style, placement, comparisons and limitations. It opens no game
window and starts no native helper during rendering.

Add `--selection-check` as a fourth argument to the renderer to exercise the
installed clock's hours/minutes/seconds selection records. It produces nine
states, checks separate selected digit regions with identical glyph coverage,
requires selected GPU pixels to change, and verifies exact restoration for an
unrelated state and deselection. The report includes selection tables, raster
comparisons and an inventory of native strings across the five libraries.
These are explicit synthetic selection states; the checker dispatches no key
or mouse events and implements no clock edit controls. Live flight defaults
to the unselected path; the explicit clock selection option is described above.

Times are explicit synthetic inputs. Baseline styling is explicitly unselected and
uses authored colors; HILITE_LIST events, runtime color overrides, timer buttons,
electrical/night states and full panel services are absent from these isolated
recorded-string checks. Live flight time and native button injection are described above.
The width rule is implemented with fontdue; GDI/FSX pixel equivalence still
requires original-game reference images. Installed fonts and generated images
remain local, ignored evidence.

## Native mouse metadata and coordinate check

```powershell
cargo run -p fsx-app --features viewer --example native-mouse-check --locked -- FSX target/native-mouse-check
```

The new output directory receives a read-only report of mouse lists in the five
checked DLLs, including parent indices, boxes, flags, cursor/help/event IDs and
callback/tooltip addresses. The command never loads a DLL, executes a callback
or dispatches an event. The viewer feature supplies texture/panel coordinate
helpers; no GPU or window is opened by this checker.

The C172 clock check reads its actual root bitmap size and panel.cfg rectangle,
maps all three button centers from VC texels to native gauge coordinates, and
checks unique left-click candidates, right-click filtering, right/bottom edges,
help-only display area and outside-panel rejection. No screen-ray picking,
repeat/drag/capture behavior, installed zero-size subregions or callback services
are provided. These checks establish coordinates/candidates; original-FSX mouse
behavior and native button execution still require separate validation.

### Isolated native mouse callback observations

```powershell
cargo run -p fsx-app --example native-mouse-callback-check --locked -- FSX target/fsx-gauge-host.exe target/native-mouse-callbacks
```

This separate checker needs the x86 helper built above; its output directory
must be new. It selects the three C172 Clock callbacks from read-only metadata,
uses explicit center coordinates and local time 12:34:56, and records bounded
native observations plus a summary. The tool opens no FSX window and injects
no OS input. Each call has native connect/disconnect/ModuleInit/ModuleDeinit,
a guarded mouse context/string buffer and independent parent timeout.

The checker accepts and distinguishes a completed callback and a contained
native fault; success of the diagnostic command does not mean buttons work.
Without services all three calls fault on the absent PANELS shared-event table.
That baseline reports callbacks_completed=0 and contained_native_faults=3.
To exercise the separate, opt-in local event bridge:

```powershell
cargo run -p fsx-app --example native-mouse-callback-check --locked -- FSX target/fsx-gauge-host.exe target/native-local-events --local-events
```

This profile binds only shared-event capture, CLOCK_TOTAL_SECONDS lookup and a
local-master query. It copies each original four-byte mouse packet, delivers it
once to the original event callback after mouse return, then updates the main
string. All three original callbacks complete and accept delivery. The first
switches to " 0:00:00"; the second performs a local-time lookup. Guarded event
words, readonly table and import restoration are checked. This is isolated
local delivery evidence: operational_buttons=false describes this isolated
probe, not the aggregate viewer state. The flight profile now connects clock
screen presses as described above. Secondary text, generic gauge routing and
original-FSX delivery parity remain unfinished. Failed calls
never produce a fabricated after-string result.

## Retained native clock state and recorded rendering

The separate local clock stream retains the original C172 gauge across time and
button commands. This checker verifies actual mode/stopwatch/12–24-hour changes,
fractional local-time lookup, paused/stopped values and module reinitialization:

```powershell
cargo run -p fsx-app --example native-clock-session-check --locked -- FSX target/fsx-gauge-host.exe target/native-clock-session
cargo run -p fsx-app --features viewer --example native-string-render-check --locked -- FSX target/native-clock-session-render target/native-clock-session/samples.ndjson --local-session
```

Use new output directories. The first command runs 30 steps in one hidden helper
PID, checks four native module generations and requires clean disconnect,
ModuleDeinit, context/string guards and import-pointer restoration. Original
mouse packets drive the native clock state; no substitute timer is used. The
second command renders all captured states and a restored baseline with installed
Quartz and the actual cockpit placement. Mode/progress/format must change GPU
pixels; paused, stopped and reset pairs must match exactly. Capture parsing
requires the checked element metadata, guards and completed teardown.

This is a retained native session plus recorded rendering, not mouse input in
the live viewer. Secondary clock text and
stopwatch behavior across midnight or clock edits are still unfinished. The flight profile now integrates persistent native buttons through startup/local
API input as described above; direct left-press screen routing is now connected as described above. No original-FSX pixel/input parity is claimed.

## Recorded secondary OAT/voltage display

The original C172 secondary text can now be inspected with explicit temperature,
units and voltage slots, including its original third-button switch to volts:

```powershell
cargo run -p fsx-app --example native-string-check --locked -- FSX target/fsx-gauge-host.exe target/native-oat --oat
cargo run -p fsx-app --features viewer --example native-string-render-check --locked -- FSX target/native-oat-render target/native-oat/samples.ndjson --oat
```

Use new output directories. Nine observations cover Fahrenheit/Celsius, native
clamping and three voltages. The helper preserves original degree bytes through
explicit Windows-1252 JSON conversion; the painter uses actual Quartz glyphs and
authored secondary placement. Voltage mode is reached with the original mouse
handler/packet, with guarded context and verified import/string restoration.
All rendered states must differ as expected and returning to the first must
restore identical pixels.

This is explicit-input/recorded rendering. The default live flight profile still
renders only the main clock text: ISA temperature is available, while bus voltage
has no electrical simulation yet. No normal bus voltage is silently supplied.
Native character/selection limits count ANSI characters; XML text keeps its ASCII
contract. Full native lifecycle and original-FSX pixel equivalence are unverified.

## Legacy electrical circuit inspection

The installed circuit schema and simulation-owned observation/load state can be
checked without a window, renderer or original FSX process:

```powershell
cargo run -p fsx-app --example legacy-electrical-check --locked -- FSX target/legacy-electrical
```

Use a new output directory. The checked C172 has 17 authored consumer entries;
authored source voltage/current parameters are absent. The checker
supplies explicitly labeled inspection bus values to test thresholds, two
requested loads, invalid-input atomicity and lifecycle. Its current sums are
requested maxima of authored consumers, not measured/total aircraft current.
It never fills MAIN_BUS_VOLTAGE with a nominal example value. Simulation starts
with unknown electrical observations and holds them until a provider updates or
clears them. A separate synthetic fixture checks explicit source primitives:
40 Ah discharge conservation, a 4 A charge limit, a supplied open-circuit curve,
generator capacity/overload and reset. These are not installed C172 ratings.
Seven more synthetic cases check explicit distribution from one generator through
main/avionics contacts: 12 A combined load, 7 A with avionics isolated, main-feed
isolation, unknown contact/load, overload and offline supply. The solver publishes
all bus results atomically; it does not derive the aircraft graph or actual loads.
The checker also reads two installed FLT switch profiles, applies the supported
positions without inventing source availability and verifies that absent requests
clear prior switch state. A separate 40 Ah / 36 A fixture follows one second of
C172 flight simulation time; its charge changes by 0.00025. The desktop scheduler
has battery pause/frame-partition/reset tests. Battery integration now follows
committed physics substeps, with an explicitly supplied current request; it does
not derive that current from CFG maxima or distribute power automatically.
A separate version-pinned reference command executes bounded original source
initialization in x86 emulation (requires pefile/unicorn; no original FSX process):

```powershell
python scripts/check-electrical-defaults.py FSX/sim1.dll target/electrical-default-reference
```

For installed sim1 version 10.0.62615.0, its single-piston path initializes source
ratings to 24 V battery / 28 V generator / 60 A. Simulation explicitly applies that
profile to missing ratings, preserving authored zero. The checker reports authored
and effective settings separately. These ratings do not make the generator online,
set charge or energize a bus. Other versions are not established by this check.
The reference also checks 87 original pre-load generator-admission cases:

```powershell
python scripts/check-electrical-defaults.py FSX/sim1.dll target/generator-admission-reference --availability
```

The port's explicit signal API combines source enable, failure and legacy engine
signal with the stored first-engine FLT switch. The checker exercises six such
cases and verifies that admission still leaves loaded bus voltage unknown. The
signal is now verified as RPM / max_rated_rpm by a separate original reconstruction
and FLT save trace:

```powershell
python scripts/check-electrical-defaults.py FSX/sim1.dll target/generator-rpm-trace --engine-signal
```

The checker reads actual saved Pct Engine RPM fractions from both FLTs and probes
admission under separately supplied enabled/no-fault inputs. These are saved
snapshots; the viewer does not reuse them as evolving RPM or infer RPM from
throttle. Pre-load voltages in the reference report are not final loaded bus
observations, and stale raw generator slots are not admitted supply.
Actual engine/load providers, C172 topology and remaining legacy defaults still
need implementation before live OAT/V binding. FLT loading is an explicit API and
checker path; it does not yet restore a flight into the viewer.

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
