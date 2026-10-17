# Compatibility

This page records observed behavior, not a promise that existing add-ons will work in a future simulator.

| Area | Current support | Limits |
| --- | --- | --- |
| Installation | Directory structure check | fsx.exe and standard Airplanes folder |
| aircraft.cfg | Partial | Aircraft title, variant references, geometry, mass/inertia, engine/control parameters; explicit encoding |
| MDLX | Partial | Packed chunks, VERT, INDE, PART, LOD, static TRAN/AMAP/SCEN |
| Native viewer | Experimental | Diffuse textured geometry, approximate alpha blending; no animation |
| Skinned MDLX parts | Omitted with a count | No bone deformation |
| MDL8, MDL9, P3D | Unsupported | Explicit format error |
| DDS diffuse maps | Partial | DXT1/3/5 and masked 32-bit RGB, top mip only; up to 4096 pixels per dimension |
| Texture lookup | Partial | Selected folder and ordered texture.cfg fallback graph; ASCII case-insensitive file lookup |
| BMP / advanced materials | Not implemented | No BMP, bump, specular, reflection or emissive maps |
| Flight dynamics / AIR | Initial airborne subset | Legacy 1101/0404/0473; modern overrides explicitly rejected; approximate engine |
| Keyboard flight mode | Experimental | C172 starts at 90 kt / 300 m; flat ground-contact stop, no takeoff/landing |
| BGL / scenery | Not implemented | No airport or terrain runtime |
| XML gauges | Not implemented | No expression engine |
| DLL / GAU / SimConnect | Not implemented | Native ABI/API research required |
| Audio, AI, multiplayer | Not implemented | Future subsystems |

## Local evidence

On 17 October 2026, the CFG inspector found **117 aircraft variants with no read errors** in the local FSX folder using Windows-1252. Variants are CFG sections, not unique aircraft models.

The C172 exterior MDLX file has six LODs (5, 15, 35, 75, 150 and 400), nine texture references and 148 animated scene nodes. LOD 150 yields 11,314 solid triangles without skinned omissions. LOD 400 yields 13,527 triangles after omitting one skinned part. These counts establish geometry decoding, not correct animation or textured appearance.

An offscreen C172 LOD 150 render was produced using the same wgpu pipeline as the native window, on an NVIDIA RTX 2080 through Vulkan. The Blue/Gold livery in texture.1 loads four of five referenced diffuse DDS files, including shared interior and pilot maps resolved through fallback. The missing DECAL_NNUMBER.DDS is reported; dynamic registration generation is not implemented. Visual inspection confirmed the blue/gold exterior mapping after preserving DDS row order and the original MDLX UV coordinates. Animated states, interactive camera input and other backends are not established by this check.

The game installation and rendered snapshots are excluded from Git. CI uses original synthetic fixtures and does not contain or download FSX resources.

The airborne prototype loads the local C172 variant, AIR and MDL without starting FSX. Reproducible checks hold 90 kt and 300 m for 60 seconds, verify initial control signs, report a full-power transient and enter the post-peak lift region during a power-off pull. Derived sea-level clean stall is 53.57 kt versus the CFG's 53.00 kt. These are consistency checks, not validated flight performance. A native-window frame was presented successfully; physical keyboard injection remains unverified because the desktop helper failed to initialize. Full details are in the [flight guide](FLIGHT.md).
