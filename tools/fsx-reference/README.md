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
