# Porting checkpoint

The target remains FSX 1:1 across interface, flight behavior, content and APIs.
This is a long-term acceptance target; no subsystem claims complete parity.

## Current stage — 19 October 2026

The baseline commit `6a2b462` rendered authored needles from recorded native
callback results. The current stage connects five C172 native needles to flight
through a persistent isolated x86 helper, with bounded stdio, pause reuse,
module reset/teardown and dynamic GPU texture updates. Windows verbatim paths
from Rust canonicalization now work in the helper's installation validation.

The installed-content `native-gauge-flight-check` example verifies 121 forward
frames, ten pause reuses, moving needle pixels, exterior/interior preservation,
module restart on rewind, identical initial/reset images and settled ground
input. Full details and commands are in [the viewer guide](VIEWER.md).

Local outputs live under ignored `target/native-gauge-flight-02/`, including
`report.json` and seven PPM snapshots. Original resources remain excluded.

Validation: 103 passing workspace test executions with all features; release
viewer build and airborne/ground native-instrument snapshots; helper self-test;
architecture, formatting and generated-site checks. Direct stream checks also
reject changed inputs on a paused clock, rewind and skipped sequence numbers.
One initial debug render exited with Windows STATUS_INVALID_HANDLE during GPU
startup; subsequent debug integration and both release snapshots passed. The
cause is not identified; investigate if it recurs rather than claiming that
all GPU startup paths have been established.

## Next acceptance work

1. Compare these airspeed/altitude/vertical-speed bindings with recorded original
   FSX flight states and cockpit images; document unit and pressure assumptions.
2. Add authored temporal response only after establishing the native lag contract.
3. Extend the explicitly checked profile to attitude/heading and moving faces;
   model engine/propeller state before treating tachometer input as telemetry.
4. Implement XML gauges, digital text, interaction and PANELS services as tested
   subsets. Do not present decoded static artwork as functional readings.
5. Continue reference flight validation, broader aircraft dynamics, airport/BGL
   scenery and menu parity. See [compatibility](COMPATIBILITY.md) for current gaps.

Keep this checkpoint current at each completed stage so interrupted work can
resume from concrete evidence. Preserve unfinished changes until verified.
