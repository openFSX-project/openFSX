# Compatibility

This page records observed behavior, not a promise that existing add-ons will work in a future simulator.

| Area | Current support | Limits |
| --- | --- | --- |
| Installation | Directory structure check | fsx.exe and standard Airplanes folder |
| aircraft.cfg | Partial | title in fltsim.N; ASCII/UTF-8 or explicit Windows-1252 |
| MDLX | Partial | Packed chunks, VERT, INDE, PART, LOD, static TRAN/AMAP/SCEN |
| Native viewer | Experimental | Solid opaque geometry; no textures or animation |
| Skinned MDLX parts | Omitted with a count | No bone deformation |
| MDL8, MDL9, P3D | Unsupported | Explicit format error |
| DDS / BMP materials | Not loaded | Texture names cataloged only |
| Flight dynamics / AIR | Not implemented | No controllable flight |
| BGL / scenery | Not implemented | No airport or terrain runtime |
| XML gauges | Not implemented | No expression engine |
| DLL / GAU / SimConnect | Not implemented | Native ABI/API research required |
| Audio, AI, multiplayer | Not implemented | Future subsystems |

## Local evidence

On 17 October 2026, the CFG inspector found **117 aircraft variants with no read errors** in the local FSX folder using Windows-1252. Variants are CFG sections, not unique aircraft models.

The C172 exterior MDLX file has six LODs (5, 15, 35, 75, 150 and 400), nine texture references and 148 animated scene nodes. LOD 150 yields 11,314 solid triangles without skinned omissions. LOD 400 yields 13,527 triangles after omitting one skinned part. These counts establish geometry decoding, not correct animation or textured appearance.

An offscreen C172 LOD 150 render was produced using the same wgpu pipeline as the native window, on an NVIDIA RTX 2080 through Vulkan. Visual inspection confirmed recognizable aircraft geometry. Textures, animated states, interactive camera input and other backends are not established by this check.

The game installation and rendered snapshots are excluded from Git. CI uses original synthetic fixtures and does not contain or download FSX resources.
