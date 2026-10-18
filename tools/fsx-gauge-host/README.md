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

## What this does not establish

This is an isolated update-call diagnostic, not a complete gauge host. No PANELS
import services are supplied; the protocol reports panel_services=false. Gauge
query/install/initialize/update/generate/draw/kill stages, panel service callbacks,
shared variable lifetime, failure systems, rendering and simulator events remain
unimplemented. The normal flight mode still has no operational native gauges.
Callback return values may represent a reading or an angle depending on the
native element's authored mapping; this tool does not guess that interpretation.

The checked airspeed and tachometer callbacks preserved the provided sample
values. The vertical-speed callback returned +/-196.8504 for raw inputs +/-256.
Altimeter resources 1031, 1032 and 1033 returned different results for the same
raw source inputs. Those observations demonstrate execution of the installed
functions; they do not prove unit bindings, full gauge lifecycle or FSX parity.
