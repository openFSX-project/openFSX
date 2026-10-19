# Local FSX reference capture

This independent Windows x86 helper reads an **already-running original FSX**
through the SimConnect client in the user's installation. It is a measurement
tool, separate from the Rust engine. It does not implement SimConnect support
inside openFSX, distribute the original DLL, send control events, set aircraft
data, install components or launch FSX. No SDK sample implementation is copied.

## Build and run

The Windows .NET Framework C# compiler is required. No NuGet packages or SDK
headers are used by the build:

```powershell
./scripts/build-reference.ps1
./target/fsx-reference.exe --self-test
./target/fsx-reference.exe --synthetic-sample
./target/fsx-reference.exe --fsx "./FSX" --output target/fsx-controls.ndjson --seconds 30
./target/fsx-reference.exe --fsx "./FSX" --output target/fsx-instruments.ndjson --seconds 30 --instruments
```

Load a flight in FSX before capturing. For stock C172 calibration, record the
exact aircraft variant, loading, control settings, pause status and input
sequence. The output embeds the aircraft TITLE in every sample. The connection
is forced local, ignoring network client settings. Capture length is limited
to 1..300 wall-clock seconds. Output must be a new file; existing captures are
never overwritten. Original runtime requirements are handled by FSX itself.

Each NDJSON line records schema 1, source, elapsed wall time, aircraft title and
12 numeric readings: elevator percent/degrees, trim percent/degrees, left/right
aileron percent, rudder percent/degrees, yoke X/Y percent, throttle percent and
on-ground state. Samples are requested at visual-frame frequency. Pausing,
changing aircraft or minimizing FSX can affect availability and sampling rate.
Numeric fields use invariant culture; title text follows the installed native
client's ANSI encoding and is JSON escaped. No coordinates or installation
paths are included in the records. Keep raw captures under ignored `target/`.

## Instrument and flight-state profile

Add `--instruments` to request schema 2. The title, 12 controls and 32 additional
FLOAT64 datums share one definition and response packet; there is no joining
of unrelated visual frames. A schema-2 packet is 648 bytes, including the
40-byte dispatch/data header, 256-byte title and 44 doubles. The default
schema-1 capture and existing recordings retain their original layout.

| Group | Additional keys and requested units |
| --- | --- |
| Clock | sim_time_seconds (ABSOLUTE TIME, seconds), sim_rate (number) |
| Air data | ias_knots, tas_knots; indicated_altitude_ft, pressure_altitude_ft, altitude_msl_ft; vertical_speed_fpm |
| Aircraft orientation | pitch_deg, bank_deg, heading_true_deg, heading_magnetic_deg |
| Instrument orientation | gyro_pitch_deg, gyro_bank_deg, gyro_heading_deg |
| Settings/weather | baro_inhg, ambient_pressure_mbar, ambient_temp_c |
| Loading/engine | total_weight_lb, fuel_total_gallons, engine_rpm (engine 1) |
| Motion | rotation_body_x_rad_s, rotation_body_y_rad_s, rotation_body_z_rad_s, groundspeed_knots |
| Instrument state | failure_airspeed, failure_altimeter, failure_attitude, failure_heading, failure_vsi (0 normal, 1 failed, 2 blank); attitude_caged (bool), attitude_bars_ratio (percent over 100) |

Explicit SimConnect unit requests matter: attitude variables with DEGREES in
their names have radians as their documented default. This profile asks for
degrees. VERTICAL SPEED is requested in feet per minute. The absolute simulator
clock is distinct from the recorder's elapsed wall time; replay subtracts its
first value and starts a fresh native session after any backward clock jump.
Body angular-rate axes retain the names supplied by SimConnect; this tool does
not infer an openFSX pitch/roll/yaw-axis mapping.

Schema 2 adds `object_id`, a zero-based `sample_index`, and an `instruments`
object. All numbers must be finite. Captures stop with an error if they exceed
100,000 samples or 64 MiB. Partial output after an error is not a completed run.
New instrument fields do not add latitude/longitude, control events or state
writes. Record loading, weather, pressure, failure and realism settings alongside
each real capture.

The extended live request has not yet been verified against a running FSX.
No original game process was running during this stage. Synthetic data proves
encoding/decoding and unit adapters, not that every requested datum is supplied
by a particular installation or that the instruments match FSX.

## Validate and replay offline

The build also writes `target/reference-instruments-synthetic.ndjson`, containing
original invented values with source `synthetic`. Check the C# to Rust boundary,
including a generated five-row pause/rewind sequence, without loading game files:

```powershell
./scripts/build-reference.ps1
./scripts/check-reference-protocol.ps1 -OutputDirectory target/reference-protocol-local
cargo run --locked -p fsx-app --features viewer --example reference-gauge-replay -- --validate target/reference-instruments-synthetic.ndjson
```

The validator streams bounded schema-2 rows, requires a consistent source/title/
object and contiguous indices, and reports unsupported native-profile states.
Elapsed wall time must not decrease; recorded simulator time may pause/rewind.
Source labels identify declared provenance and do not authenticate a recording.
Windows CI exercises this boundary without an FSX installation.

To replay the recorded indications through the checked native C172 profile:

```powershell
./scripts/build-gauge-host.ps1
cargo run --locked -p fsx-app --features viewer --example reference-gauge-replay -- ./FSX target/fsx-instruments.ndjson target/reference-gauge-replay
```

The output directory must be new. Replay uses recorded IAS, indicated altitude,
vertical speed and gyro indications directly. It does not reconstruct them from
ISA, true speed, body attitude or magnetic/true heading. Failed/caged instruments,
adjusted reference bars and changing inputs on an unchanged simulator clock are
outside this native profile and fail explicitly. Real-source recordings must
match the selected C172 variant-0 title. The separate `--validate` mode accepts
valid unsupported states and reports their count without attempting rendering.

`frames.ndjson` records native inputs/results, clock reuse/rewind, helper PID and
diagnostics. Available stateless model bindings consume recorded controls and
instrument variables; unsupported/lagged groups remain omitted.
`first.ppm`/`last.ppm` use a fixed inspection camera, and `report.json`
is written after successful completion. ISA and body/gyro differences are
diagnostics with explicit assumptions, never a parity verdict. This does not
replay aircraft motion, instrument lag, engine dynamics or full gauge state.

Local synthetic sequence replay on 19 October 2026 passed five records, one
paused result reuse and one helper restart; the recorded 600 fpm input returned
600.0000192 from the native vertical-speed callback. This is numerical evidence
for the adapter/callback path, not measured FSX flight behavior.

Variable names and units were checked against the legacy-compatible
[simulation-variable reference](https://www.prepar3d.com/SDK/Core%20Utilities%20Kit/Variables/Simulation%20Variables.html).

## What is verified

The helper compiles for x86 and passes original synthetic-packet tests without
loading FSX: expected field offsets, JSON escaping, every truncated prefix,
invalid flags, unterminated title, non-finite numbers and object-ID binding.
The request's user-aircraft alias is object 0; a response can contain a different
real object ID. The reader binds that ID only after validating the first complete
packet, then rejects subsequent packets for a different object.

On 18 October 2026, a loaded local C172 flight returned 87 valid packets over
five seconds and 35 over a second two-second capture. The title was
`Cessna Skyhawk 172SP Paint1`, the actual object ID was 1, and the complete
392-byte packets contained the title plus all 12 finite numeric readings.
The observed baseline had neutral control readings, throttle 73.22998046875
percent and on-ground 0. Raw records remain under ignored `target/`.
This establishes live acquisition for that installation and sample state;
control sweeps, angle/percentage correlation and animation calibration remain
unverified. The tool sends no control input and does not focus the game.

Function signatures, constants and packet layout were checked against the
user-installed SimConnect SDK header. The current Microsoft
[dispatch documentation](https://docs.flightsimulator.com/html/Programming_Tools/SimConnect/API_Reference/General/SimConnect_GetNextDispatch.htm)
describes polling semantics; it is not a complete FSX compatibility contract.

The native helper and its protocol self-test are intentionally outside the
Cargo dependency graph. No engine crate depends on them or on the original
SimConnect DLL. The .NET executable is generated under ignored `target/`.

## Offline pipeline fixture

The build script also writes `target/reference-synthetic.ndjson`, using
`--synthetic-sample`. Its source label is **synthetic** and its original,
invented values must not be treated as flight measurements. PowerShell parses
the encoded JSON during the build check. The openFSX model viewer can import
one sample with `--reference-pose` and report the resolved animation groups;
see the viewer documentation for commands and limits.

Earlier Remote Desktop and menu-only attempts returned no aircraft samples.
The successful captures above required a loaded local flight. No calibration
result is inferred from the synthetic fixture or those failed attempts.
