# Architecture

openFSX is an independent runtime for a user's installed FSX content. Original executable code and game resources are not distributed. The openOMSI approach is an inspiration; its implementation is not copied.

## Workspace layout

The repository follows the subsystem-workspace layout of
[openOMSI](https://github.com/openOMSI-org/openOMSI): engine packages under
`crates/`, developer tools under `tools/`, launcher frontend under `launcher/`,
shared runtime resources under `assets/`, and documentation/site/build scripts
at the root. The implementation and FSX-specific boundaries are independent.
There are no placeholder crates for unimplemented scenery, gauges or APIs.

| Package | Responsibility | Internal dependencies |
| --- | --- | --- |
| fsx-cfg | CFG parsing and explicit code-page decoding | None |
| fsx-geometry | Matrices, camera math and double-precision rigid orientation | None |
| fsx-script | Bounded numeric RPN and optional model-local data | None |
| fsx-model | MDLX geometry, materials, scene graph, authored keys and visibility | fsx-geometry, fsx-script |
| fsx-texture | DDS top-mip and BI_RGB bitmap decoding, texture.cfg fallback lookup | None |
| fsx-vehicle | CFG/AIR data, static loading, contacts, panel placements and native gauge metadata | fsx-cfg, fsx-geometry |
| fsx-sim | 6DOF integration, aerodynamics and wheel contact forces | fsx-vehicle, fsx-geometry |
| fsx-render | wgpu resources, materials, draw passes, debugging overlay and readback | model, texture, geometry, vehicle, sim |
| fsx-ui | Windows WebView2 menu host, native 3D preview and bounded installation asset access | fsx-cfg, fsx-model, fsx-vehicle, fsx-render |
| fsx-app | Native event loop, keyboard flight adapter and view orchestration | content, sim, render, UI |
| tools/fsx-check | Headless openfsx CLI and repeatable local content/flight checks | cfg, model, vehicle, sim |

The root Cargo.toml is a virtual workspace with shared versions/dependencies.
Its default member is fsx-check, preserving `cargo run -- inspect ...`.
The separate `tools/fsx-reference` Windows x86 measurement helper reads an
original running FSX through its installed client. It is outside the engine's
Cargo graph; no subsystem links the original SimConnect DLL. Protocol checks
use original synthetic packets and run in Windows CI without game resources.

The separate C++ tool under tools/fsx-gauge-host is an experimental Windows x86
process for isolated native needle-update calls. It loads only the explicitly
selected installed stock library; Rust engine packages do not link to it or to
FSX import libraries. Its own synthetic ABI/exception/protocol self-test runs in
Windows CI without game files. Local native probes do not implement the PANELS
API or a full gauge lifecycle and are not connected to flight-mode instruments.
The Rust content reader remains read-only and never executes DLLs.
Build the native application with an explicit package:

```powershell
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- --help
cargo test --workspace --all-features --locked
node scripts/check-architecture.mjs
```

`launcher/index.html` is embedded by fsx-ui. Owned WGSL shaders under
`assets/shaders/` are embedded by fsx-render. The local `FSX/` installation
remains ignored and outside all workspace packages and public site roots.
The Windows helper `scripts/build-windows.cmd` builds both release binaries
under `target/release/`; it does not package game content.

## Dependency boundaries

File readers and vehicle definitions do not depend on the simulator, windowing
or GPU. Vehicle contact configuration computes static spring parameters; the
runtime force law is a local extension trait in fsx-sim. Aerodynamic methods
likewise extend immutable vehicle parameters inside fsx-sim. Shared rigid
orientation lives in fsx-geometry, avoiding a vehicle/simulation cycle.

The renderer consumes decoded content and readout data. It owns no application
event loop or pilot input. fsx-app connects the selected aircraft, fixed-step
session, renderer and menu. fsx-sim has no renderer or UI dependency.

`scripts/architecture.json` declares allowed internal dependency directions;
`node scripts/check-architecture.mjs` checks Cargo metadata and source roots.
CI runs the guard along with workspace tests and the native build. Adding a
subsystem requires an explicit layer rule. Original synthetic aircraft fixtures
are exposed only by fsx-vehicle's test-support feature for cross-crate tests.

## Current coverage

The crates reflect implemented boundaries, not completed FSX subsystems.
Content decoding, rendering, manual key inspection and flight remain partial.
Earth coordinates, airport/BGL terrain streaming, XML gauges, SimVars,
SimConnect and native add-on bridges are still unimplemented. Compatibility
claims must follow the evidence in the [compatibility record](COMPATIBILITY.md).

## Rendering boundary

The MDLX reader produces validated world-space triangles for one LOD. It applies the static TRAN/AMAP/SCEN hierarchy. Animated nodes stay in bind pose by default; explicit manual clip/frame inspection evaluates authored rotation and translation keys. A bounded numeric RPN subset evaluates XAPC animation frame expressions and VISL conditions through an application-provided variable lookup; SGVL visibility inherits through scene parents and masks draw calls without changing topology. Unavailable or unsupported conditions keep their encoded defaults with diagnostics. Per-model temporal state limits supported animation targets by their Lag rate on the simulation clock. Visibility and static inspection use a read-only mode. Temporal XAPC evaluation can additionally use a bounded per-instance local-variable context and temporary registers; only valid expression/frame results commit changes. This is not shared XML-gauge state or an event/API bridge. The application visual adapter derives center steering from the same physical wheel-angle calculation as contact forces, and drives the native throttle expression from throttle lever state. It also exposes the on-ground flag. Geometry is rebuilt only when authored frames or inherited visibility change; pause holds time and reset clears the visual state. Skinning is not evaluated. Omitted skinned parts are counted; complete visual compatibility must not be claimed.

The renderer uses a depth buffer, diffuse DDS maps, material RGB fallback and face lighting. Opaque parts write depth first; ordinary source-alpha/inverse-source-alpha parts are drawn back to front by part center without depth writes. Alpha tests are evaluated separately from blending. This is approximate transparency, not parity with FSX material rendering. Normal/specular maps, reflection, effects and full aircraft visual state remain future work. Offscreen snapshots use the same pipeline as the window. The renderer can replace scene vertices, material bindings and textures on the existing device/surface for flight-view changes. The host caches exterior/interior content and poses independently, with one shared physics session. A validated explicit eye/target/up projection lets the cockpit camera follow pitch and bank from the loaded-CG body transform. Variant model.cfg interior and aircraft.cfg eyepoint metadata remain in the headless vehicle layer; view gestures and selection stay in the application. Camera-profile overlays, FSX zoom matching and live gauge surfaces remain incomplete.

Texture lookup starts in the selected livery folder and follows numerically ordered texture.cfg fallback entries, with cycle detection and a directory limit. DDS is decoded on the CPU to RGBA8 before GPU upload. Only referenced diffuse textures are loaded; non-diffuse maps are not mistakenly treated as albedo. Missing or unsupported files produce diagnostics and a material-color fallback.

## Native instrument boundary

The optional application adapter binds five checked C172 needles to independent
ISA air data in fsx-sim and three attitude/heading sprites to ideal gyro orientation. It communicates over bounded stdio with a hidden Windows
x86 helper; original DLLs never enter the x64 viewer. The helper validates each
native element/token/callback address, maintains module lifetime and invokes
callbacks only when simulation time advances. Pause reuses results and rewind
starts fresh modules. The app validates ordered response bindings and paints
decoded bitmap needles and authored sprite/mask pairs; fsx-render updates named, dimension-checked texture
pixels on the existing GPU. Cached interior images remain current in exterior
view. The headless simulation contains no process or gauge-host dependency.
Full PANELS services, gauge lifecycle stages and broad unit/event contracts
remain future work.

## Milestones and acceptance criteria

1. **Inventory.** CFG encodings, external SimObjects roots and actionable errors. Current coverage is titles in the standard Airplanes directory.
2. **Aircraft viewer.** Display one verified MDLX variant and its textures without FSX. A C172 livery has been rendered with available diffuse maps. Generated registration decals, animation, visibility and skinning remain incomplete; the result is a viewer baseline, not complete visual compatibility.
3. **First flight.** Airborne and runway C172 starts use AIR/CFG parameters, keyboard controls, RK4 integration and original flat-ground geometry. CFG payload/full usable fuel determine initial mass, CG and a full loaded inertia tensor. Three fixed-wheel spring/damper contacts allow parking, braking, steering, ground roll and departure. Speed/climb/post-peak-lift and ground/departure scenarios are repeatable; recorded FSX reference comparisons, engine tables, fuel consumption, scrape contact and landing validation remain incomplete. See the flight guide for evidence and limitations.
4. **Local scenery.** One airport, an explicit BGL subset, terrain and streaming. Record format coverage and frame-time measurements.
5. **Cockpit and APIs.** A tested subset of XML gauges and SimConnect contracts. Native DLL/GAU add-ons need a separate decision: 32-bit code cannot simply be loaded into a 64-bit process.

No deadlines or universal compatibility guarantees are stated. Each milestone needs original test data and measurable results.
