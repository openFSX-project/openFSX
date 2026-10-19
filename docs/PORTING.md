# Porting checkpoint

The target remains FSX 1:1 across interface, flight behavior, content and APIs.
This is a long-term acceptance target; no subsystem claims complete parity.

## Current stage — 19 October 2026

Commit `e2eceb5` completed five flight-driven C172 needles. The current stage
adds the original heading card and two attitude sprites with their authored
source scales, callback readings, stationary mask artwork and neutral reticle.
The helper's schema-2 `--flight-frames` mode maintains three modules and 14
ordered axis/needle results; original schema-1 `--frames` inspection still works.
Native code remains isolated from the x64 renderer.

The native sprite reader validates four-byte-packed x86 fields, callback RVAs
and finite scales. The renderer distinguishes transparent RGB zero, aperture
RGB (1,1,1) and opaque foreground; it rotates before nonuniform scaling and
source translation. Original bitmap dimensions are used directly. Synthetic
checks cover mask semantics, clipping, rotation direction, transform order,
bounded metadata and all 14 protocol result bindings.

The installed-content `native-gauge-flight-check` passed 121 forward frames,
three changing gyro sprites, ten paused reuses, view preservation and helper
restart with identical initial/reset pixels. Local outputs are under ignored
`target/native-sprite-flight-01/`, including `report.json` and seven PPMs.
The fixed camera/geometry comparison changed 12,161 RGB bytes during pitch/roll
input. `native-sprite-check` outputs under `target/native-sprite-03/` were
visually inspected at level, nose-up/down, right-bank and east-heading states.
Original bitmaps and derived local inspection images remain excluded from Git.

The older five-library/18-callback diagnostic also passes. Full instructions,
references and assumptions are in [the viewer guide](VIEWER.md).

Validation: 107 passing workspace test executions with all features; release
viewer build and airborne/ground snapshots; helper ABI/protocol self-test;
architecture, formatting and generated-site checks. Extended stream checks
reject missing gyro fields, changing attitude on a paused clock and rewinds.
No helper process remained after the checks.

One initial debug render exited with Windows STATUS_INVALID_HANDLE during GPU
startup; subsequent debug integration and both release snapshots passed. The
cause is not identified; investigate if it recurs rather than claiming that
all GPU startup paths have been established.

## Next acceptance work

1. Extend the reference reader to capture instrument air data and gyro values,
   then compare the live profile with recorded original-FSX flight states and
   cockpit images. No original FSX process was running during this stage.
2. Add authored temporal response only after establishing the native lag contract.
3. Validate gyro filtering/placement and add drift, failures and adjustment
   controls; extend other moving faces. Model engine/propeller state before
   treating tachometer input as telemetry.
4. Implement XML gauges, digital text, interaction and PANELS services as tested
   subsets. Do not present decoded static artwork as functional readings.
5. Continue reference flight validation, broader aircraft dynamics, airport/BGL
   scenery and menu parity. See [compatibility](COMPATIBILITY.md) for current gaps.

Keep this checkpoint current at each completed stage so interrupted work can
resume from concrete evidence. Preserve unfinished changes until verified.
