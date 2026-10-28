# Flight prototype

openFSX now has an initial keyboard-controlled airborne mode for the local stock C172. It loads the aircraft's CFG and legacy AIR data, simulates six degrees of freedom, and renders the installed MDLX/DDS content independently of fsx.exe.

This is an experimental flight-dynamics subset with an optional fixed-wheel
runway start. Ground roll and departure are demonstrated; FSX behavior parity,
a complete aircraft system model and validated landing behavior are absent.

The visual model now follows the physical center-wheel steering angle through
its own animation binding. Q/E therefore changes both contact-force direction
and the nose-wheel pose. Supported XAPS/XAPC targets can use model Lag limits
on the simulation clock; pause holds the pose and reset initializes it again.
The current visual adapter also drives the C172 throttle expression using
instance-local numeric data and scratch registers, and exposes the on-ground
state. A local interior-model check verified throttle movement. Cockpit flight
selection now reads the native interior reference and configured eye point;
gauges and panel interaction remain unimplemented. It does not invent RPM or control-surface percent mappings. C172 steering
was checked from the shared keyboard controller through physics to offscreen GPU
output; original-FSX motion and timing comparisons remain unverified. See the
[viewer guide](VIEWER.md) for the reproducible local-content check.

## Start a flight

From the repository root, with your own FSX installation under FSX:

```powershell
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172" --fly --variant 0 --windows-1252 --lod 150
```

Add --cockpit to start in the installed interior model, or press F9 during flight. F11 returns to the exterior. Model selection and eye position follow the selected variant and its CFG datum; dynamic gauge surfaces remain blank.

In flight mode the positional path is an aircraft folder, not an MDL file. The selected fltsim.N entry resolves the AIR file, exterior model.cfg entry and texture folder. Variant 0 selects the Blue/Gold C172 in the verified installation. Encoding selection is explicit; omit --windows-1252 for UTF-8 CFG files.

The flight starts airborne at 90 knots true airspeed and 300 metres above a local flat plane. Initial throttle and elevator trim are solved for level flight once. There is no autopilot keeping it level after pilot input.

Add --ground for a runway start:

```powershell
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172" --fly --ground --windows-1252 --lod 150
```

This settles the suspension with power off, positions the aircraft near the
south end of the original test runway, and applies the parking brake. B releases
the brake; F4 selects full power and F1 idle. Rudder also steers the nosewheel.
Use pitch input and trim to control departure; sustained full power with the
initial trim can produce a large nose-up transient and loss of speed. Initial
runway trim comes from a 65 kt / 300 m level-trim solve; it is not a C172 POH
takeoff-trim specification. R resets to the selected ground or airborne start.

Add **--airport ICAO** (or pick an airport in the menu's CURRENT LOCATION) to
start on the threshold of that airport's longest hard runway, parking brake set.
Runways of every airport within 25 km are drawn from the installed BGL files with
centreline, edge and threshold markings. The world is a flat plane at field
elevation; reported and instrument altitude include that elevation, while air
density still uses height above the plane. When `target/fsx-terrain-host.exe`
is built, a 257 x 257 elevation mesh of +-40 km (about 310 m spacing) is decoded
from the installed DEM files; every airport within range is flattened to its
own field elevation within 3 km, blending to real terrain by 6 km, and its
runways are drawn at that height. Each frame the wheel contact plane is moved to
the terrain height under the aircraft (locally level), so landings at other
airports and on open terrain use the same spring/damper gear. A CG penetration
or numeric blow-up is a crash: the flight stops until R.

Terrain streams with the flight: when the aircraft is 20 km from the centre of
the loaded area, a background thread decodes a new +-40 km area (runways,
flattening, elevation, land class) around it and swaps it in; the local origin
stays at the start, so very long flights accumulate flat-earth error. The swap
re-uploads scene textures and may cause a short hitch.

The sky is a dome textured with the installed `Texture/sky_noon_5.bmp` gradient
(32-bit BGRA strip, zenith at the top). Fair-weather cumulus are built from the
installed `cumulus01.bmp` 4 x 4 puff sheet (DXT5 alpha): 45 clusters of 6-14
puffs per area, each puff three crossed translucent quads, sorted back to front
per cloud. Cloud base is 500 m above the highest terrain in the area (at least
1500 m). Time of day, weather themes, cloud lighting and flying through cloud
are not simulated yet. Distance haze uses the sky gradient's horizon colour
(1 - exp(-(d/24 km)^2)); the dome itself is unhazed. Terrain tapers to the origin
plane over its outer 6 km so mountains crossing the grid edge do not end in a
vertical cut.

Trees use the installed `Texture/treesSU.bmp` 4 x 4 sheet (DXT1 alpha, rows:
deciduous, two conifer rows, bushes/palms) as crossed alpha-tested quads 11-23 m
tall, within 8 km of the area centre and at least 1.8 km from airports. Species
and density come from the land class (e.g. 36 per 310 m quad in forest, 3 in
suburbs); this is an openFSX density model, not FSX autogen annotation.

Runways use installed surface textures by FSX surface code (concrete
`NewRunwayLayout.DDS`, asphalt/bituminous/tarmac `runway12.dds`, grass, dirt,
sand and gravel variants), tiled every 1.5 runway widths. openFSX paints
threshold bars, edge lines, centreline dashes, aiming-point blocks 300 m in
(runways over 1200 m) and both designators (e.g. 16L / 34R) from a 5 x 7 pixel
font, oriented for the landing pilot. FSX's own marking geometry, lights, edge
and aprons are not reproduced yet; water runways are not drawn.

Aprons (airport subrecord 0x37 outlines, ear-clipped) and taxiways (0x1A points,
0x1C taxi/parking/path segments as strips of the authored width with round
joints) use the asphalt surface texture with planar UVs; taxi and path segments
get a yellow centreline. The pavement stack sits below the wheel contact plane:
flattened field -0.2 m, aprons -0.14, taxiways -0.10, centrelines -0.08,
runway -0.05, runway paint 0. Depth uses reversed-Z on a 32-bit float buffer so
these thin layers stay separate at distance. Taxiway edge lines, hold-short
markings, signs and lights are not drawn yet.

Placed scenery objects within 6 km (BGL section 0x25 LibraryObject records)
are resolved through a GUID index of every installed model library (section
0x2B) and parsed with the same MDLX reader as aircraft. Each model is baked
into world space with its heading and scale, standing on the terrain when
placed above ground level; textures come from the stock texture folders,
downscaled to 512 px. Nearest placements first, up to 4000 objects / 3 M
vertices. Pitch/bank, animation, LOD switching, night textures and autogen are not applied yet.
Generic buildings (record 0x0A) are drawn as solid-colour boxes of the authored
footprint and band heights, pitched for non-flat roof types; their FSX texture
bands are not decoded.

Engine speed is simulated for a fixed-pitch propeller: equilibrium RPM grows
with the square root of power (idle fuel 6 %) and with forward speed, about
85 % of `[piston_engine] max_rated_rpm` static at full throttle and 100 % at
cruise speed, followed with a 0.6 s lag. It drives the engine sound layers and
the native C172 tachometer needle (GENERAL ENG RPM, token 390, helper input 7).
Thrust still uses the momentum-disk model; FSX propeller tables are not used.

Fuel burns from the CFG usable load at 0.45 lb/hp/h (7.6e-8 kg/J) of the power
produced (idle fuel 6 %, scaled by throttle and RPM), about 35 L/h for the C172
at cruise power; aircraft mass falls with it. When the tanks run dry the engine
stops (throttle ineffective, propeller windmilling with airspeed) and the title
shows ENGINE OUT; R restores full fuel. Tank selection, fuel CG shift, mixture
and altitude power loss are not modelled.

Autogen uses the stock annotations: each land texture `NNN?2su1.bmp` has an
`NNN?2an1.agn` covering the same tile (4 x 4 terrain quads here). Rectangular
buildings become boxes with the annotated footprint and orientation, coloured
from varied wall/roof palettes, with heights estimated from footprint (4-30 m,
pitched roofs on small houses); vegetation polygons are filled with trees from
the stock sheet (about one per 30 m square). Within 5 km of the area centre and
2 km clear of airports. Building classes, heights, textures, rows of houses and
the older region-X annotation layout are not decoded.

Start-up reads only BGL section tables for the model/object index (built on
a background thread while terrain decodes) and then reads just the object
subsections whose QMID cell overlaps the area; the log prints per-stage times.
A KSEA/LOWI start takes about 5 s in a release build.

`--time HH:MM` (or CURRENT TIME in the menu) sets a fixed local time: the sky
uses the matching installed `sky_<period>_5.bmp` (midnight, predawn, dawn,
morning, noon, afternoon, presunset, sunset, postsunset), haze takes its horizon
colour, and lit surfaces get a tint from full daylight through warm low sun to
dark blue night; the aircraft keeps 30 % panel lighting. From 18:00 to 07:00
hard runways get self-luminous edge lights every 60 m, green thresholds and red
ends. Sun position, date/latitude, seasons, landing/taxi lights and night
textures are not simulated.

`--airborne` with `--airport` starts trimmed at 90 kt and 300 m over the runway
threshold, along the runway heading; R returns there.
The ground uses the stock summer land-class textures: worldlc.bgl classes are
sampled per mesh quad and the matching `NNN?2su1.bmp` textures (DXT1 inside a
BMP header) are packed into a 2048-pixel atlas, one texture per 4 x 4 quads
(about 1.25 km). Natural Olson classes use their own texture number; urban,
suburban and water classes are approximated (031, 030, 123) because the
lclookup.bgl remapping, regions and seasons are not decoded yet. All textures,
including the atlas and live panel images, carry box-filtered mip chains with
trilinear 16x anisotropic sampling. Without the
helper or land-class data the mesh falls back to a height colour gradient.
Water polygons, buildings, autogen and taxiways are not loaded yet.

## Controls

| Key | Action |
| --- | --- |
| Down / Up | Pull / push elevator |
| Left / Right | Bank left / right |
| Q / E | Rudder left / right |
| Page Up / Page Down | Increase / decrease throttle |
| F1 / F4 | Idle / full throttle |
| F9 / F11 | Virtual cockpit / exterior chase; F9 also recenters cockpit look |
| B | Toggle parking brake |
| Space | Hold wheel brakes |
| Home / End | Nose-up / nose-down elevator trim |
| F5 / F6 / F7 / F8 | Flaps fully up / one detent up / one detent down / fully down |
| P | Pause / resume |
| R | Reset the selected ground or airborne start |
| Left drag in cockpit | Look around relative to the aircraft |
| Mouse wheel | Chase distance or cockpit vertical field of view |
| Esc | Close window |

Releasing a directional key centers that control. Throttle and trim remain at their last positions. Opposing keys cancel each other. Keyboard deflections use half the configured control range. Losing window focus or minimizing pauses the flight and clears held controls; press P to resume.

A tap completed between physics steps is sampled for one 1/120-second step,
then released. Dedicated navigation keys and NumLock-off numpad navigation
events share the same controls. Number input with NumLock on is not treated
as a directional command. Letter bindings use physical key positions.

A connected game controller (XInput or any device gilrs recognizes) flies
alongside the keyboard: left stick pitch/roll, right stick X rudder, right/left
triggers increase/decrease throttle, D-pad up/down move flaps one detent, A holds
brakes, X toggles the parking brake, Start pauses and Back resets. Sticks use a
dead zone and a mild response curve; a held key overrides the matching axis.

Flap detents come from `[flaps.0] flaps-position.N`; the surface moves at
`extending-time`. The full-flap lift increment is derived from the authored
`flaps_up_stall_speed`/`full_flaps_stall_speed` ratio and the clean AIR lift
peak. Drag (0.06 at full, quadratic in deflection) and nose-down pitch (-0.08,
linear) are generic estimates scaled by `drag_scalar`/`pitch_scalar`. AIR flap
tables are not decoded yet, so this is a playable approximation, not FSX parity.

Flight plays the aircraft's `sound/sound.cfg`: every engine combustion, propeller
and non-combustion layer with its rparams pitch line and vparams volume curve,
cockpit (viewpoint 1) or exterior (viewpoint 2) set by the current view, plus
stall horn, wind, ground roll, flap motor and touchdown sounds (shared files
come from the installation `Sound` folder). PCM 8/16-bit and MS ADPCM WAVE are
decoded. The flight model has no RPM state yet, so normalized RPM is estimated
from throttle and airspeed (idle 0.25, about 0.85 static full power). The stall
horn sounds from 4 degrees below the AIR lift peak. Missing audio hardware or
sound files only disable sound.

**--wind DEGREES/KNOTS** (also the Wind field of the menu time dialog) sets a steady wind (meteorological direction it blows from, 0 to 100 kt, for example `--wind 270/15`). Aerodynamics, propeller thrust, RPM and the airspeed readout use air-relative velocity; ground contact and climb rate stay ground-referenced, so a headwind shortens the takeoff roll. **--gusts KNOTS** (0 to 30) adds smooth deterministic gusts of that strength on top of it, about half as strong vertically. No turbulence, shear or altitude variation.

The window title reports live flight state, including the sink rate at the last touchdown; a touchdown faster than 3 m/s (about 590 fpm) shows HARD LANDING while rolling. This is only a report: no gear damage is modelled. Add **--debug-hud** for the optional original telemetry overlay; it is hidden by default.
The camera follows the aircraft. The runway and grid are original test geometry,
not FSX scenery. Wheel contact continues the rigid-body simulation, including
compression, braking and liftoff. Scrape points, damage and validated landing
handling are not implemented. CG penetration or an invalid numeric state
terminates the run with a diagnostic.

## On-screen flight panel

- TAS is true airspeed in knots, not indicated airspeed.
- ALT is height in feet above the flat test plane, not pressure altitude.
- V/S is vertical speed in feet per minute, positive upwards.
- HDG is heading in degrees clockwise from the local test world's north. No magnetic model or geographic navigation is implemented.
- THR and TRIM are normalized throttle and elevator-trim commands in percent.
- BRK reports the wheel-brake command in percent; it includes parking brake or held Space.
- The attitude display uses body roll and pitch in degrees. Pitch marks are spaced by 10 degrees; right bank rotates the horizon counterclockwise, and pitch-up moves it below the fixed aircraft symbol.
- AIRBORNE, PAUSED, POST-PEAK LIFT and ON WHEELS status come from the simulation. The post-peak message is a curve-region diagnostic, not a validated stall warning system.

The panel scales with the window and is rendered by the same GPU path in
offscreen captures. Its original geometry and pixel letterforms require no
external fonts, gauge files or textures. It is a diagnostic flight overlay;
FSX cockpit instruments and XML gauges remain unimplemented. The panel updates
per rendered frame; the window title updates four times per second.

## Reproducible checks

```powershell
cargo run -- inspect-aircraft "./FSX/SimObjects/Airplanes/C172" --windows-1252
cargo run -- flight-check "./FSX/SimObjects/Airplanes/C172" --windows-1252
cargo run -- takeoff-check "./FSX/SimObjects/Airplanes/C172" --windows-1252
cargo test --workspace --all-features --locked
```

flight-check loads variant 0, reports converted parameters, then runs fixed-control trim, initial control-sign, full-power response and power-off pull scenarios. Its acceptance gates check internal numerical consistency and signs. They are not validation against recorded FSX trajectories.

An offscreen flight frame uses the same geometry and GPU pipeline as the window:

```powershell
cargo run -p fsx-app --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172" --fly --debug-hud --windows-1252 --lod 150 --seconds 10 --snapshot target/c172-flight.ppm
```

The snapshot advances the trimmed state at 120 Hz before rendering. Duration is limited to 300 seconds and requires --fly and --snapshot. Keep working captures under ignored target. Selected PNG engine screenshots are published in the website gallery at the project owner's request; source game resources are never published. Convert captures losslessly with `node scripts/ppm-to-png.mjs target/c172-flight.ppm target/c172-flight.png`.

## Model and units

State uses metres, seconds, kilograms and radians. World axes are north/east/down; body axes are forward/right/down. Orientation is a normalized quaternion. A fourth-order Runge-Kutta integrator uses a 1/120-second outer step, with smaller contact steps near the ground. Desktop scheduling accumulates frame time and caps any one frame at 0.25 seconds; long stalls do not trigger unbounded catch-up.

Rendering maps NED to east/up/north and uses a left-handed Direct3D projection,
matching the native MDLX convention. The corrected camera preserves left/right
and pilot-view bank direction; a former right-handed projection mirrored them.
This visual correction does not change the physics integration or its control
signs. Exterior and cockpit cameras share the same loaded-CG transform.

CFG supplies maximum gross mass, empty mass and inertias, loading stations,
tank positions and capacities, wing geometry, engine position/power, control
limits, datum and tuning scalars. Initial loading uses the configured station
weights and full usable Avgas at an explicit fixed 6 lb/US gallon. Unusable
fuel is assumed included in the configured empty weight and is not added twice.
This is a deterministic loading choice, not an import of the user's saved FSX
flight or fuel state. Overweight loading and other fuel types are rejected.

The mass-weighted CG includes passengers and fuel. Empty-aircraft inertia is
translated to the loaded CG with the parallel-axis theorem; stations and tanks
are point masses. The full symmetric 3-by-3 tensor includes their products of
inertia. Cholesky factorization rejects non-positive tensors and solves the
rigid-body Euler equation, including gyroscopic coupling. A nonzero configured
empty_weight_coupled_moi remains unsupported; loading-induced coupling is
supported. Distributed tank shape, fuel slosh and fuel consumption are absent.

As an explicit initial approximation, AIR aerodynamic moments are treated as
referenced to the empty CG. The aerodynamic wrench is translated to the loaded
CG; propeller thrust acts at Engine.0. Initial pitch trim uses those same moment
equations. Laterally unbalanced loading/engine placement is rejected by the
initial level-trim solver, which does not yet solve aileron/rudder trim.
CFG static height/pitch initialize ground settling; they do not constrain the
aircraft once integration starts.

## Fixed-wheel ground subset

Only three fixed wheels are supported. Wheel positions, static compression,
travel ratio, damping ratio, brake mapping and steering angle are read from
contact_points. Non-wheel contacts are omitted and counted; retracting gear,
invalid compression/damping and other wheel counts are rejected. Impact damage
and wheel-extension airspeed fields are not implemented. Contact locations are treated as
uncompressed tire contact points, without adding wheel radius again.

Empty-weight support fractions balance vertical load and roll/pitch moments
over the wheel triangle. Spring stiffness comes from that support load and
configured static compression; damping uses the configured ratio. Normal force
cannot pull a tire onto the plane. Travel beyond the configured stroke uses an
explicit stiffer stop. Tire forces use regularized friction: 0.02 rolling,
up to 0.8 braking/lateral, with a combined friction limit. These coefficients
and the stop are openFSX choices, not decoded FSX surface/material behavior.
No wheel angular dynamics, anti-skid, distributed tire patch, scrape response,
ground effect, runway slope or weather model is implemented.

The outer control update remains 120 Hz; within 10 m of the test plane, RK4
subdivides integration to at most 1/480 second for contact stability. Runway
initialization settles for five simulated seconds with idle power and brakes,
then resets the public scenario clock. There is no hidden position constraint
or controller during the subsequent run.

takeoff-check verifies 20 seconds of power-off parking, braking from 10 m/s,
and a departure sequence: full power, brake release, normalized pull 0.12 for
two seconds when speed first reaches 55 kt, then neutral elevator. The gate
requires height above 30 m within 60 seconds. The 55 kt trigger is a test input,
not an established rotation-speed recommendation. On the local C172:

| Check | Observed result |
| --- | --- |
| Settled ground start | CG height 1.2116 m; pitch 1.655 degrees |
| 20 s idle parking | North drift -0.0003 m; speed below displayed precision |
| Braking from 10 m/s | 14.45 m; final speed 0.0005 m/s after 10 s |
| First airborne clearance above 3 m | 18.65 s; 480.91 m north travel; 82.07 kt |
| Departure height gate | 30.07 m after 22.02 s; 75.74 kt; transient climb 11.97 m/s |

Clearance above 3 m is not the instant of first wheel separation. These are
internal scenario results, not validated C172 takeoff performance. The large
transient climb and speed exchange require recorded reference comparison.
In the native window, F4 and B changed the live power/brake readouts and the
aircraft departed the runway; one observed frame showed about 81 kt and 32 ft
with AIRBORNE status. P paused and R restored the braked runway start. A longer
unchanged full-power run developed high pitch and loss of speed; sustained
departure control and landing behavior remain unverified.

AIR records 0x0404 and 0x0473 supply piecewise-linear lift and pitch-moment curves versus body angle of attack. Selected coefficients from the 250-byte 0x1101 record supply parasite/gear drag, control derivatives, damping, sideslip and cross-axis terms. Integer coefficients use their documented 1/2048 scale; double coefficients are not rescaled. Legacy pitch/yaw and control signs are converted to the right-handed body convention. Modern override records 0x1539 through 0x1547 are rejected rather than silently ignored.

Forces include lift, drag, side force, propeller thrust and gravity. Moments include the loaded static/control/damping derivatives and rigid-body gyroscopic coupling. Induced drag uses the conventional lift-squared/wing-aspect-ratio relation. Density follows the ISA troposphere, limited to 0–11 km.

The propeller is an explicit approximation: an ideal momentum-theory disk with a fixed effective efficiency of 0.8, sea-level rated shaft power, throttle and density scaling. It remains finite at zero airspeed. FSX engine/propeller lookup tables, mixture, RPM dynamics, propwash moments, torque and fuel consumption are not implemented. This is not a claim of an accurate C172 engine model.

## Observed results

On 17 October 2026, with the local C172 at loaded mass **1,046.89 kg**:
748.43 kg empty, 154.22 kg configured payload, and 144.24 kg full usable fuel.
Maximum gross mass is 1,156.66 kg. Body CG is [0.145084, 0, -0.104990] metres.
These results supersede the earlier maximum-gross/empty-CG approximation.

| Check | Result |
| --- | --- |
| Derived clean sea-level stall | 50.97 kt; CFG reference 53.00 kt, with no loading condition established for that reference |
| 90 kt / 300 m initial trim | Throttle 0.50830; normalized trim -0.19759; AoA 1.972 degrees |
| 60 s fixed-control level flight | 90.00000 kt; 300.00000 m; no controller corrections |
| 0.5 s pull input 0.1 | Positive pitch rate, 6.1515 degrees/s |
| 0.5 s right-bank input 0.1 | Positive roll rate, 3.3736 degrees/s |
| 0.5 s right-rudder input 0.1 | Positive yaw rate, 2.4879 degrees/s |
| 30 s full power from 90 kt / 1500 m | 87.16 kt; height gain 111.73 m; final climb 1.66 m/s |
| 20 s power-off pull from 65 kt / 1500 m | Reached angles beyond the lift-curve peak |

The full-power values describe a transient scenario, not a steady-state climb specification. The post-peak flag establishes curve entry, not validated stall, spin or recovery behavior.

The NVIDIA RTX 2080 / Vulkan path produced an offscreen flight frame and presented a frame in the native window. Automated tests exercise keyboard mapping, short taps, key release, pause/reset and frame partitioning.

End-to-end window input was rechecked on 17 October 2026 with the loaded-mass
model and on-screen panel using Windows Computer Use keyboard injection.
A Right tap changed displayed bank from +0.0 to +0.1 degrees. A subsequent
Down tap was followed by pitch +2.1 degrees and approximately +20 ft/min climb,
compared with the initial +2.0 degrees and zero climb. These values are
observations after short impulses, not controlled performance benchmarks.
P displayed PAUSED on the panel; R restored 90.0 kt, 984 ft, zero bank and
initial trim; Esc closed the process successfully. Earlier event tracing
identified an injected arrow as physical Numpad6 and logical ArrowRight;
both dedicated and numpad navigation are handled. This check establishes
delivered window input, simulation response and visible telemetry changes.
Sustained physical keyboard holds, controller hardware, other aircraft,
graphics backends and platforms remain unverified.

## Experimental native instruments

Add `--native-gauges --cockpit` on Windows after building the separate x86
helper with `scripts/build-gauge-host.ps1`. Five native C172 needles consume
ideal ISA pitot/static air data from the existing simulation: calibrated
airspeed, standard-setting pressure altitude and vertical speed. Three native
attitude/heading sprites also consume ideal gyro orientation. Pause freezes
callback evaluation; reset restarts the helper and clears native state. The
local integration check covers flight-driven GPU pixels and lifecycle behavior.
See [the viewer guide](VIEWER.md) for
commands, assumptions and verification limits. Other instruments remain static
or blank; gyro drift/failures and adjustment knobs are absent; FSX instrument accuracy, weather, pressure adjustment, response lag,
panel interaction and engine RPM remain unimplemented or unverified.

## Remaining work

Recorded reference flights and quantitative comparison, fuel consumption and configurable loading, engine/propeller tables, broader ground handling and landing validation, control-surface animation, cockpit instruments, audio and controller hardware. MDLX skinning/visibility and generated registration decals remain viewer limitations. No BGL terrain, geographic navigation or add-on API compatibility is implied by this local flight test.

AIR field research was checked against the AirEd definitions linked from [Herve Sors' aircraft dynamics resources](https://www.aero.sors.fr/fsairfile.html). The reference archive was read locally and is excluded from Git. No third-party implementation or game resource is redistributed.

Legacy loading-field meanings were checked against the [Prepar3D SDK aircraft configuration reference](https://www.prepar3d.com/SDKv3/LearningCenter/simobjects/aircraft_configuration_files.html). Point-mass inertia, fixed fuel density and aerodynamic moment reference choices above are openFSX approximations, not claims about FSX's internal implementation.

