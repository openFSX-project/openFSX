# Compatibility

The project target is FSX 1:1 across interface, behavior, content and APIs. No subsystem currently has a complete recorded parity comparison.

This page records observed behavior, not a promise that existing add-ons will work in a future simulator.

| Area | Current support | Limits |
| --- | --- | --- |
| FSX-style menu | Initial Windows host | Local UI skin, aircraft catalog, live native MDLX/DDS preview, launch and return; independent layout, no pixel-parity verification; other sections unimplemented |
| Installation | Directory structure check | fsx.exe and standard Airplanes folder |
| aircraft.cfg | Partial | Variant references, geometry, station payload, full usable Avgas loading, loaded CG/inertia tensor and engine/control parameters; explicit encoding |
| MDLX | Partial | Packed chunks, VERT, INDE, PART, LOD, static TRAN/AMAP/SCEN |
| Native viewer | Experimental | Diffuse textured geometry, approximate alpha blending; manual native-key inspection |
| MDLX animation keys | Initial runtime binding subset | ANIB/XANI/XANS/XANK and SGAL; XAPS/XAPC, bounded instance-local data/registers, simulation-clock Lag and automatic center steering/throttle; control-surface percent mappings, RPM, Standard autoplay and full FSX timing remain unverified or unimplemented |
| Cockpit flight view | Initial native-interior subset | Variant model.cfg interior and CFG eyepoint, attached pitch/bank camera, F9/F11 selection and existing-device resource exchange; offscreen ground/air/bank and release CLI checked; native key input, FSX camera definitions/FOV parity, dynamic gauges and panel interaction remain unverified or unimplemented |
| panel.cfg | Read-only placement subset | Variant-selected panel path, Window/VCockpit indices, titles, gauge references/coordinates/optional dimensions/parameters and case-insensitive model texture bindings; backgrounds, aliases, layout behavior and gauge execution remain unimplemented |
| MDLX visibility | Initial read-only subset | VISL VISS/VINS/VISC/VINC and SGVL; numeric RPN, A-variable lookup and inherited draw masks; unsupported or unavailable conditions use encoded defaults |
| Skinned MDLX parts | Omitted with a count | No bone deformation |
| MDL8, MDL9, P3D | Unsupported | Explicit format error |
| DDS diffuse maps | Partial | DXT1/3/5 and masked 32-bit RGB, top mip only; up to 4096 pixels per dimension |
| Texture lookup | Partial | Selected folder and ordered texture.cfg fallback graph; ASCII case-insensitive file lookup |
| BMP / advanced materials | Not implemented | No BMP, bump, specular, reflection or emissive maps |
| Flight dynamics / AIR | Initial airborne subset | Legacy 1101/0404/0473; modern overrides explicitly rejected; approximate engine |
| Keyboard flight mode | Experimental | Airborne C172 start or fixed-wheel runway start; power, brakes, steering and departure checked |
| Wheel contact | Initial subset | Three fixed wheels, flat-plane springs/dampers and regularized friction; non-wheel contacts omitted, no landing-performance validation |
| Flight overlay | Optional --debug-hud | Live TAS, local height/heading, vertical speed, commands, attitude and status; independent of FSX gauges |
| BGL / scenery | Not implemented | No airport or terrain runtime |
| XML gauges | Not implemented | Numeric model expressions support bounded local animation data, not a gauge XML loader or shared panel-variable contract; no host events, SimVar writes or native gauge execution |
| DLL / GAU / SimConnect | Not implemented | Native ABI/API research required |
| Original-FSX measurement tool | Initial live acquisition | Separate x86 reader under tools/fsx-reference; loaded local C172 returned 87 valid packets over five seconds and 35 over two seconds; actual user-object ID bound after validation; control calibration and an openFSX SimConnect server remain unimplemented |
| Passive reference screenshots | openFSX menu verified | Independent Windows Graphics Capture utility under tools/fsx-capture; 1024x738 menu and native 3D preview captured with foreground unchanged and no activation, restoration or input; original-FSX cockpit still unverified |
| Recorded pose inspection | Initial static subset | One schema-1 NDJSON sample drives available XAPS/read-only XAPC bindings; synthetic and locally captured neutral C172 samples rendered; no live calibration or temporal lag evaluation |
| Audio, AI, multiplayer | Not implemented | Future subsystems |

## Local evidence

On 18 October 2026, `inspect-panel` read the installed C172 variant-0 configuration: six 2D windows with 147 placements and two virtual-cockpit panels with 29 placements. Both native interior texture names matched their configured panels case-insensitively. A321 (17 VC placements) and B747 (28 VC placements) also matched both configured textures. These are configuration/model checks, not instrument rendering or behavior validation. C208B inspection rejects its installed `pixel_size=512,512s` value; FSX's handling of that malformed value has not been measured. No installation file was corrected or included in the repository.

The native Windows menu loaded the local aircraft catalog and live C172 exterior preview. On 18 October 2026, native capture and input verified camera movement, zoom, hiding the preview for the aircraft picker and replacing C172 with the selected A321 model (29,888 LOD-400 triangles). Visual resource loading does not require the single-piston flight model; this does not establish A321 flight support. Skinned parts, registration decals and automatic model animation remain incomplete. Aircraft chooser opening, FLY NOW launching the C172 runway renderer and Esc restoring the menu were checked. This establishes the launch path only; it does not establish FSX menu or simulation parity.

The C172 ground path loads three wheel contacts and reports six omitted
non-wheel contacts. Parking and braking scenarios pass, and explicit fixed
inputs reach height above 30 m from the runway in 22.02 s. Native F4/B input
also produced departure and visible AIRBORNE telemetry. This establishes a
ground-to-air transition, not FSX takeoff fidelity or validated landing behavior.

On 17 October 2026, the CFG inspector found **117 aircraft variants with no read errors** in the local FSX folder using Windows-1252. Variants are CFG sections, not unique aircraft models.

The C172 exterior MDLX file has six LODs (5, 15, 35, 75, 150 and 400), nine texture references and 148 animated scene nodes. LOD 150 yields 11,314 solid triangles without skinned omissions. LOD 400 yields 13,527 triangles after omitting one skinned part. These counts establish geometry decoding, not correct animation or textured appearance.

An offscreen C172 LOD 150 render was produced using the same wgpu pipeline as the native window, on an NVIDIA RTX 2080 through Vulkan. The Blue/Gold livery in texture.1 loads four of five referenced diffuse DDS files, including shared interior and pilot maps resolved through fallback. The missing DECAL_NNUMBER.DDS is reported; dynamic registration generation is not implemented. Visual inspection confirmed the blue/gold exterior mapping after preserving DDS row order and the original MDLX UV coordinates. Automatic animation state, interactive camera input and other backends are not established by this offscreen check. Manual native-key inspection is recorded separately in the [viewer guide](VIEWER.md).

The game installation and source resources are excluded from Git. Selected PNG engine screenshots are published in the website gallery at the project owner's request; intermediate captures remain under ignored target. CI uses original synthetic fixtures and does not contain or download FSX resources.

The airborne prototype loads the local C172 variant, AIR and MDL without starting FSX. CFG payload and full usable Avgas yield 1,046.89 kg, a loaded CG and coupled inertia tensor. Reproducible checks hold 90 kt and 300 m for 60 seconds, verify initial control signs, report a full-power transient and enter the post-peak lift region during a power-off pull. Derived sea-level clean stall is 50.97 kt at this loading; the CFG lists 53.00 kt without an established loading condition. These are consistency checks, not validated flight performance. Native-window keyboard injection with this loading and the flight overlay verified short Right/Down impulses changing bank/pitch and vertical speed, plus P pause, R reset and Esc exit. Dedicated and NumLock-off navigation events are mapped consistently; sustained hardware holds remain unverified. Full details are in the [flight guide](FLIGHT.md).

Visibility evidence: the local C172 has 61 conditions. Explicit prop-RPM-percent values 0, 8 and 20 each resolve 30 conditions and hide four LOD-150 draw batches, selecting the still, intermediate and blurred propeller alternatives. Identical-camera GPU readbacks differ only in the propeller region. These are explicit inspection inputs, not recorded FSX RPM or calibrated engine dynamics. The menu supplies an engine-stopped, stationary preview state and resolves all 61 C172 conditions; flight mode does not yet provide propeller or wheel RPM.
