# Isolated native gauge update probe

This Windows x86 diagnostic process loads one explicitly selected stock gauge
library from the user's FSX installation. It resolves Linkage after Windows
relocation/import processing, locates a named gauge and enumerates its encoded
elements. Probe mode does not call ModuleInit or any element callback. Loading
the DLL does run its normal Windows loader initialization.

Sample mode calls ModuleInit, writes an explicit finite value into the selected
needle's numeric source slot, invokes its native stdcall update callback once,
and calls ModuleDeinit. It returns the callback's finite result as JSON. Current
and previous numeric values are both set to the requested raw input. No host
token lookup, unit conversion, animation integration or implicit input is added.

Only Cessna.dll, Cessna172.dll, Cessna182s.dll, Bell_206B.dll and
Bendix_king_radio.dll under the explicit installation's canonical Gauges folder
are accepted. The stock client runtime must already be installed; this tool
does not install or copy it. No FSX process is launched or required, and no
window activation, restoration, mouse/keyboard input or SimConnect events exist
in the helper.

## Build and reproduce

Use the installed MSVC x86 toolchain and Windows SDK:

```powershell
./scripts/build-gauge-host.ps1
./target/fsx-gauge-host.exe --probe ./FSX Cessna172.dll Airspeed
./target/fsx-gauge-host.exe --sample ./FSX Cessna172.dll Airspeed 1001 75
./scripts/check-native-gauges.ps1 -FsxRoot ./FSX -Output target/native-gauge-check.ndjson
```

The output path must be new. The stock check loads five libraries in separate
processes and records 18 samples across airspeed, tachometer, vertical speed and
the three main altimeter needles. Each callback must return finite values and
respond to distinct inputs. The recorded values are synthetic inspection inputs,
not measurements of a running FSX session.

## Protocol and limits

Success writes one schema-1 ASCII JSON line. Source is native-gauge-probe; mode
is probe or sample; pointer_bits is 32. Records identify the requested library,
gauge, header version, graph size, module lifecycle calls and available needles.
Sample records additionally identify resource/token IDs, raw_input and
callback_result. Errors go to stderr with a nonzero exit status.

The independently declared numeric slot uses four-byte packing and a 28-byte
x86 layout. Needle update callbacks use the SDK-declared stdcall signature.
Pointer access is bounded to committed pages of the selected DLL; callback
pages must be executable and input slots writable. Limits are 64 MiB on disk,
512 gauge entries, 2048 distinct elements, 8192 graph edges, 256-byte ASCII names
and finite input magnitudes up to 1e12. A five-second watchdog terminates this
helper only. Structured exceptions in the invoked module/element functions are
reported as failures. No native library or SDK header is distributed.

The build self-test uses an original synthetic stdcall function and a deliberately
raised structured exception. It validates the 32-bit ABI, exception reporting,
input limits and JSON escaping without loading game files. Windows CI runs that
self-test; installed-library checks run only against the user's local content.

## Isolated native string samples

```powershell
./target/fsx-gauge-host.exe --sample-string ./FSX Bendix_king_radio.dll "Bendix-King Radio Nav-Comm 1" 0x5cc8 u32:0x1825 u32:0x2050 f64:0
cargo run -p fsx-app --example native-string-check -- FSX target/fsx-gauge-host.exe target/native-string-check
```

The first command's RVA is specific to the inspected local DLL. The Rust checker
resolves its current RVA from token metadata and requires a new output directory.
Each input must explicitly choose f64 numeric bytes or a u32 union word; u32
accepts decimal/0x hexadecimal. BCD words must not be written as floating-point
numbers. These inputs bypass host token resolution and do not model radio state.

Source native-string-probe identifies token IDs, input kinds/values, maximum
length, output text and numeric callback return. String capacity is 1..256 plus
NUL; the output ends at an inaccessible page, with a checked prefix. The caller
restores the original output pointer before module deinitialization. Source
callbacks, pointer replacement, non-ASCII output, oversized/unterminated text,
faults and non-finite results fail. The buffer outlives native module teardown.
The parent Rust checker also bounds output and enforces a ten-second timeout.

Three installed Nav-Comm samples passed. Without connection notifications the isolated clock was blank and
secondary clock/transponder samples required unsupported state/services or
faulted; they are not operational. No string rendering, GDI font autosizing,
panel-service table or complete gauge lifecycle is implemented. The synthetic
self-test verifies a string callback, page-guard fault and prefix-overwrite
detection without loading installed content.

## Headless C172 clock connection notifications

```powershell
cargo run -p fsx-app --example native-string-check -- FSX target/fsx-gauge-host.exe target/native-clock-check --clock
```

This opt-in uses `--sample-string-connected`, resolves the main Clock string
from tokens [3,4,5], and delivers original CONNECT_TO_WINDOW (14) / DISCONNECT
(15) gauge callbacks with extra_data zero. Only the inspected Cessna172.dll Clock
time-string profile is allowed. The callback creates/frees its own private state;
the helper never manufactures its internal layout. It restores the public
user_data header slot to the captured initial zero after native disconnect.
No actual PANEL_WND or OS window exists, and panel_services/full_gauge_lifecycle
remain false. The notification metadata is explicit in each JSON sample.

Clock inputs must use f64 integer hours 0..23 and minutes/seconds 0..59. Samples
at midnight, 12:34:56 and 23:59:59 produced the matching original strings and
0/45296/86399 numeric results. The unconnected baseline remains blank. Six
invalid range/type/fraction cases are rejected. The native synthetic self-test
also verifies the three-argument gauge callback ABI and fault containment.

This does not implement installation, update/draw routines, service imports,
multiple instances, timer controls, font sizing or flight-mode clock updates.
Other connection handlers may require a real panel context and remain unsupported.

## Persistent frame mode

`--frames FSX_ROOT` keeps the checked Cessna/Cessna172 libraries and five needle
bindings alive for an explicit input stream. It accepts ordinary and Windows
verbatim canonical installation paths. Each request is an ASCII line:

```text
frame 1 0 90 984.251968503937 0
frame 2 0 90 984.251968503937 0
frame 3 0.01 89 985 256
quit
```

Fields are sequence, simulation seconds, airspeed knots, pressure altitude feet
and vertical speed in 1/256 m/s. Sequence starts at 1 and increments exactly;
time is nonnegative, finite and nondecreasing. Equal time requires identical
inputs and reuses callback results. A rewind requires a new process. Time is
limited to 1e9 seconds, inputs to finite magnitudes <=1e12 and airspeed must be
nonnegative. Requests and responses are bounded (1 KiB / 16 KiB).

Schema-1 source `native-gauge-stream` emits `ready`, `frame` and `closed` JSON
lines. Ready identifies PID, 32-bit pointers, two loaded modules, five bindings
and `panel_services=false`. Frames identify sequence/time, `reused`, cumulative
`frames_evaluated` and ordered binding/input/result records. The Rust adapter
validates every response and the actual child PID. Current/previous source slots
advance only with new simulation time. ModuleInit runs once per loaded module;
quit/EOF calls ModuleDeinit and acknowledges two deinitialized modules.
The watchdog is idle while waiting for input and active during native work;
the Rust parent independently bounds response and shutdown waits.

The original `--frames` schema-1 profile remains available for five-needle
inspection. The viewer uses the extended profile below. See
[the viewer guide](../../docs/VIEWER.md) and run the
`native-gauge-flight-check` example for installed-content regression evidence.

### Extended flight profile

`--flight-frames FSX_ROOT` uses schema 2 and adds Bell_206B, the Cessna heading
card and the two Bell attitude sprites used by C172's virtual cockpit. Each
request appends legacy pitch degrees (nose down positive), bank degrees (left
positive) and ideal gyro heading degrees:

```text
frame 1 0 90 984 0 -10 -30 90
frame 2 0 90 984 0 -10 -30 90
quit
```

Ready/closed report three modules. Frames contain 14 ordered values: five
needle results followed by X/Y/rotation for heading 4001, attitude outer 2004
and attitude inner 2002. Results also identify `element_kind`, `axis` (0 X,
1 Y, 2 rotation) and `callback_called`. Two additional native callbacks handle
heading rotation (token 84) and attitude pitch (token 78). Other sprite axes
preserve explicit source readings; MODULE_VAR_NONE supplies zero. Bank uses
token 80. Source-lookup callbacks are rejected. Author scales stay in the
renderer and apply once. The parent validates every axis binding, callback
presence and input echo. Pause/reset/timeouts follow the original contract.

## What this does not establish

This is an isolated update-call diagnostic, not a complete gauge host. No PANELS
import services are supplied; the protocol reports panel_services=false. Gauge
query/install/initialize/update/generate/draw/kill stages, panel service callbacks,
shared variable lifetime, failure systems, rendering and simulator events remain
unimplemented. Default flight mode has no operational native gauges; the opt-in
flight profile implements five checked needle bindings and three gyro sprites.
Gyro drift/failures, magnetic variation and adjustment controls remain absent.
Callback return values may represent a reading or an angle depending on the
native element's authored mapping; this tool does not guess that interpretation.

The checked airspeed and tachometer callbacks preserved the provided sample
values. The vertical-speed callback returned +/-196.8504 for raw inputs +/-256.
Altimeter resources 1031, 1032 and 1033 returned different results for the same
raw source inputs. Those observations demonstrate execution of the installed
functions; they do not prove unit bindings, full gauge lifecycle or FSX parity.
