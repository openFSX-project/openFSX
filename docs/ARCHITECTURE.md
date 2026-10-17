# Architecture

openFSX is an independent runtime for a user's installed FSX content. Original executable code and game resources are not distributed. The openOMSI approach is an inspiration; its implementation is not copied.

## Current code

| Component | Responsibility | State |
| --- | --- | --- |
| CLI | Installation checks and aircraft titles | Partial |
| MDLX reader | Chunk bounds, geometry, LODs, static scene transforms | Partial |
| Math | Column-major transforms and orbit camera | Implemented for the viewer |
| DDS / texture lookup | Top mip decoding and texture.cfg fallback search | Partial |
| Native viewer | winit window and wgpu diffuse textured geometry | Experimental |
| Flight simulation | Legacy AIR/CFG subset, 6DOF RK4 and keyboard-controlled airborne mode | Experimental; C172 consistency checks only |
| World | Earth coordinates, terrain, BGL and streaming | Not implemented |
| Gauges | XML expressions, simulation variables and instruments | Not implemented |
| Add-on bridge | SimConnect and research into DLL/GAU compatibility | Not implemented |

Keep useful modules together until their interfaces stabilize. Do not create empty crates for future subsystems.

## Rendering boundary

The MDLX reader produces validated world-space triangles for one LOD. It applies the static TRAN/AMAP/SCEN hierarchy. Animated nodes remain in their stored bind pose. Skinning and visibility expressions are not evaluated. Omitted skinned parts are counted; complete visual compatibility must not be claimed.

The renderer uses a depth buffer, diffuse DDS maps, material RGB fallback and face lighting. Opaque parts write depth first; ordinary source-alpha/inverse-source-alpha parts are drawn back to front by part center without depth writes. Alpha tests are evaluated separately from blending. This is approximate transparency, not parity with FSX material rendering. Normal/specular maps, reflection, effects and live aircraft state remain future work. Offscreen snapshots use the same pipeline as the window.

Texture lookup starts in the selected livery folder and follows numerically ordered texture.cfg fallback entries, with cycle detection and a directory limit. DDS is decoded on the CPU to RGBA8 before GPU upload. Only referenced diffuse textures are loaded; non-diffuse maps are not mistakenly treated as albedo. Missing or unsupported files produce diagnostics and a material-color fallback.

## Milestones and acceptance criteria

1. **Inventory.** CFG encodings, external SimObjects roots and actionable errors. Current coverage is titles in the standard Airplanes directory.
2. **Aircraft viewer.** Display one verified MDLX variant and its textures without FSX. A C172 livery has been rendered with available diffuse maps. Generated registration decals, animation, visibility and skinning remain incomplete; the result is a viewer baseline, not complete visual compatibility.
3. **First flight.** Airborne and runway C172 starts use AIR/CFG parameters, keyboard controls, RK4 integration and original flat-ground geometry. CFG payload/full usable fuel determine initial mass, CG and a full loaded inertia tensor. Three fixed-wheel spring/damper contacts allow parking, braking, steering, ground roll and departure. Speed/climb/post-peak-lift and ground/departure scenarios are repeatable; recorded FSX reference comparisons, engine tables, fuel consumption, scrape contact and landing validation remain incomplete. See the flight guide for evidence and limitations.
4. **Local scenery.** One airport, an explicit BGL subset, terrain and streaming. Record format coverage and frame-time measurements.
5. **Cockpit and APIs.** A tested subset of XML gauges and SimConnect contracts. Native DLL/GAU add-ons need a separate decision: 32-bit code cannot simply be loaded into a 64-bit process.

No deadlines or universal compatibility guarantees are stated. Each milestone needs original test data and measurable results.
