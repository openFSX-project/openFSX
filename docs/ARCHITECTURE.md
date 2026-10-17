# Architecture

openFSX is an independent runtime for a user's installed FSX content. Original executable code and game resources are not distributed. The openOMSI approach is an inspiration; its implementation is not copied.

## Current code

| Component | Responsibility | State |
| --- | --- | --- |
| CLI | Installation checks and aircraft titles | Partial |
| MDLX reader | Chunk bounds, geometry, LODs, static scene transforms | Partial |
| Math | Column-major transforms and orbit camera | Implemented for the viewer |
| Native viewer | winit window and wgpu solid geometry rendering | Experimental |
| Flight simulation | Time step, atmosphere, aerodynamics and systems | Not implemented |
| World | Earth coordinates, terrain, BGL and streaming | Not implemented |
| Gauges | XML expressions, simulation variables and instruments | Not implemented |
| Add-on bridge | SimConnect and research into DLL/GAU compatibility | Not implemented |

Keep useful modules together until their interfaces stabilize. Do not create empty crates for future subsystems.

## Rendering boundary

The MDLX reader produces validated world-space triangles for one LOD. It applies the static TRAN/AMAP/SCEN hierarchy. Animated nodes remain in their stored bind pose. Skinning and visibility expressions are not evaluated. Omitted skinned parts are counted; complete visual compatibility must not be claimed.

The renderer uses a depth buffer, diffuse material RGB and face lighting. Textures, transparency, normal maps, effects and live aircraft state remain future work. Offscreen snapshots use the same pipeline as the window to check GPU initialization, shaders and geometry without launching FSX.

## Milestones and acceptance criteria

1. **Inventory.** CFG encodings, external SimObjects roots and actionable errors. Current coverage is titles in the standard Airplanes directory.
2. **Aircraft viewer.** Display one verified MDLX variant and its textures without FSX. Solid geometry is a first step; this milestone remains incomplete.
3. **First flight.** One aircraft's AIR/CFG parameters, controls, an integrator and a simple ground surface. Compare speed, climb and stall in repeatable scenarios.
4. **Local scenery.** One airport, an explicit BGL subset, terrain and streaming. Record format coverage and frame-time measurements.
5. **Cockpit and APIs.** A tested subset of XML gauges and SimConnect contracts. Native DLL/GAU add-ons need a separate decision: 32-bit code cannot simply be loaded into a 64-bit process.

No deadlines or universal compatibility guarantees are stated. Each milestone needs original test data and measurable results.
