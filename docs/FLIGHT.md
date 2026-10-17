# Airborne flight prototype

openFSX now has an initial keyboard-controlled airborne mode for the local stock C172. It loads the aircraft's CFG and legacy AIR data, simulates six degrees of freedom, and renders the installed MDLX/DDS content independently of fsx.exe.

This is an experimental flight-dynamics subset. It does not establish FSX behavior parity, a complete aircraft system model, takeoff or landing support.

## Start a flight

From the repository root, with your own FSX installation under FSX:

```powershell
cargo run --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172" --fly --variant 0 --windows-1252 --lod 150
```

In flight mode the positional path is an aircraft folder, not an MDL file. The selected fltsim.N entry resolves the AIR file, exterior model.cfg entry and texture folder. Variant 0 selects the Blue/Gold C172 in the verified installation. Encoding selection is explicit; omit --windows-1252 for UTF-8 CFG files.

The flight starts airborne at 90 knots true airspeed and 300 metres above a local flat plane. Initial throttle and elevator trim are solved for level flight once. There is no autopilot keeping it level after pilot input.

## Controls

| Key | Action |
| --- | --- |
| Down / Up | Pull / push elevator |
| Left / Right | Bank left / right |
| Q / E | Rudder left / right |
| Page Up / Page Down | Increase / decrease throttle |
| Home / End | Nose-up / nose-down elevator trim |
| P | Pause / resume |
| R | Reset the initial airborne state |
| Mouse wheel | Change chase-camera distance |
| Esc | Close window |

Releasing a directional key centers that control. Throttle and trim remain at their last positions. Opposing keys cancel each other. Keyboard deflections use half the configured control range. Losing window focus or minimizing pauses the flight and clears held controls; press P to resume.

The window title reports airspeed, altitude, vertical speed, pitch, bank and throttle. The camera follows the aircraft. The runway and grid are original test geometry, not FSX scenery. First ground contact stops the simulation until reset: there is no landing-gear suspension, taxiing, bounce or damage model yet.

## Reproducible checks

```powershell
cargo run -- inspect-aircraft "./FSX/SimObjects/Airplanes/C172" --windows-1252
cargo run -- flight-check "./FSX/SimObjects/Airplanes/C172" --windows-1252
cargo test --features viewer --locked
```

flight-check loads variant 0, reports converted parameters, then runs fixed-control trim, initial control-sign, full-power response and power-off pull scenarios. Its acceptance gates check internal numerical consistency and signs. They are not validation against recorded FSX trajectories.

An offscreen flight frame uses the same geometry and GPU pipeline as the window:

```powershell
cargo run --features viewer --bin openfsx-viewer -- "./FSX/SimObjects/Airplanes/C172" --fly --windows-1252 --lod 150 --seconds 10 --snapshot target/c172-flight.ppm
```

The snapshot advances the trimmed state at 120 Hz before rendering. Duration is limited to 300 seconds and requires --fly and --snapshot. Snapshots of installed content stay under ignored target and are not published with the website.

## Model and units

State uses metres, seconds, kilograms and radians. World axes are north/east/down; body axes are forward/right/down. Orientation is a normalized quaternion. A fourth-order Runge-Kutta integrator advances at 1/120 second. Desktop scheduling accumulates frame time and caps any one frame at 0.25 seconds; long stalls do not trigger unbounded catch-up.

CFG supplies maximum gross mass, wing area/span, Oswald efficiency, empty-aircraft inertias, engine power, propeller diameter, control limits, CG/datum and tuning scalars. The initial approximation holds mass at maximum gross weight, scales the empty diagonal inertias by gross/empty mass, and retains the empty-aircraft CG. It does not compute a loaded inertia tensor from fuel and payload distribution. Nonzero coupled inertia is explicitly unsupported.

AIR records 0x0404 and 0x0473 supply piecewise-linear lift and pitch-moment curves versus body angle of attack. Selected coefficients from the 250-byte 0x1101 record supply parasite/gear drag, control derivatives, damping, sideslip and cross-axis terms. Integer coefficients use their documented 1/2048 scale; double coefficients are not rescaled. Legacy pitch/yaw and control signs are converted to the right-handed body convention. Modern override records 0x1539 through 0x1547 are rejected rather than silently ignored.

Forces include lift, drag, side force, propeller thrust and gravity. Moments include the loaded static/control/damping derivatives and rigid-body gyroscopic coupling. Induced drag uses the conventional lift-squared/wing-aspect-ratio relation. Density follows the ISA troposphere, limited to 0–11 km.

The propeller is an explicit approximation: an ideal momentum-theory disk with a fixed effective efficiency of 0.8, sea-level rated shaft power, throttle and density scaling. It remains finite at zero airspeed. FSX engine/propeller lookup tables, mixture, RPM dynamics, propwash moments, torque and fuel consumption are not implemented. This is not a claim of an accurate C172 engine model.

## Observed results

On 17 October 2026, with the local C172 at configured maximum gross mass of 1,156.66 kg:

| Check | Result |
| --- | --- |
| Derived clean sea-level stall | 53.57 kt; CFG reference 53.00 kt |
| 90 kt / 300 m initial trim | Throttle 0.54328; normalized trim -0.07218; AoA 2.570 degrees |
| 60 s fixed-control level flight | 90.00000 kt; 300.00000 m; no controller corrections |
| 0.5 s pull input 0.1 | Positive pitch rate, 5.2203 degrees/s |
| 0.5 s right-bank input 0.1 | Positive roll rate, 3.4471 degrees/s |
| 0.5 s right-rudder input 0.1 | Positive yaw rate, 2.0225 degrees/s |
| 30 s full power from 90 kt / 1500 m | 91.83 kt; height gain 77.34 m; final climb 1.52 m/s |
| 20 s power-off pull from 65 kt / 1500 m | Reached angles beyond the lift-curve peak |

The full-power values describe a transient scenario, not a steady-state climb specification. The post-peak flag establishes curve entry, not validated stall, spin or recovery behavior.

The NVIDIA RTX 2080 / Vulkan path produced an offscreen flight frame and presented a frame in the native window. Automated tests exercise keyboard mapping, key release, pause/reset and frame partitioning. End-to-end physical keyboard injection was not verified: the desktop automation helper could not initialize. Other aircraft, graphics backends and platforms remain unverified.

## Remaining work

Recorded reference flights and quantitative comparison, loaded CG/inertia and fuel, engine/propeller tables, ground handling, control-surface animation, cockpit instruments, audio and controller hardware. MDLX skinning/visibility and generated registration decals remain viewer limitations. No BGL terrain, geographic navigation or add-on API compatibility is implied by this local flight test.

AIR field research was checked against the AirEd definitions linked from [Herve Sors' aircraft dynamics resources](https://www.aero.sors.fr/fsairfile.html). The reference archive was read locally and is excluded from Git. No third-party implementation or game resource is redistributed.
