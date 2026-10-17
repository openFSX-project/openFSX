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
invalid flags, unterminated title and non-finite numbers. On the local machine,
the installed DLL exports resolved and SimConnect_Open returned 0x80004005
without a running FSX server. That failure produces no reference samples and
does not establish live ABI/data compatibility. A successful connection and
valid live packets remain necessary before treating output as FSX evidence.

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

The original FSX refused to run in the observed Remote Desktop session on
18 October 2026. A later local launch reached the menu and SimConnect_Open succeeded, but no
aircraft samples were returned before a flight was loaded. A loaded local FSX
flight is still needed to establish live acquisition. No data or calibration result is inferred
from the synthetic fixture or a failed connection.
