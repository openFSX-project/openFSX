# Porting checkpoint

The target remains FSX 1:1 across interface, flight behavior, content and APIs.
This is a long-term acceptance target; no subsystem claims complete parity.

## Current stage — 19 October 2026

Commits `e2eceb5` and `09b44c5` completed the current opt-in C172 panel:
five live air-data needles plus the native heading card/two attitude sprites.
The current stage adds optional reference schema 2: 12 control readings and
32 instrument/clock/loading/state readings in one 648-byte SimConnect packet.
Default schema-1 capture and pose import remain supported.

The recorder's extended live request is not yet verified against a running
original FSX. The build emits original synthetic fixtures; schema-2 C#/Rust
interoperability is checked without game files in the Windows CI workflow.
The sequential reader validates bounds, field set, source/title/object,
contiguous indices and elapsed wall time. Absolute simulator time is kept
separate and can pause/rewind.

`reference-gauge-replay` feeds recorded IAS, indicated altitude, VSI and gyro
readings directly to the native C172 profile. Available stateless model bindings
also use recorded controls/variables. It normalizes simulator time and explicitly
restarts native state after rewinds, including rewinds from normalized time zero.
Failed/caged/adjusted instruments or changed native inputs on a paused clock are
outside the checked replay profile. ISA/body/gyro differences are diagnostic
output only; aircraft trajectory and full instrument behavior are not replayed.

Local evidence: `target/reference-protocol-check-03/` contains a C#-derived
five-row synthetic sequence and malformed-case checks. Native replay under
`target/reference-instrument-replay-sequence-02/` passed five rows, one paused
reuse and one clock rewind with a new helper PID. The native VSI returned
600.0000192 for the recorded 600 fpm input. First/last fixed-camera images were
rendered and local images inspected. Raw resources/recordings remain ignored.

The existing live-flight gauge check remains available; instructions and known
limits are in [the viewer guide](VIEWER.md) and [format notes](FORMATS.md).

Validation: 120 passing workspace test executions with all features; both C#
schema self-tests and C#/Rust protocol regressions; release viewer/replay builds,
release schema-2 pose import and five-row native replay. The existing 121-frame
live-flight gauge check also passed pause/reset/view preservation unchanged.
Architecture, formatting and generated documentation checks pass locally.
The CI workflow is configured for the synthetic protocol gate; no remote CI
run or extended real-FSX acquisition is claimed by this local checkpoint.

One initial debug render exited with Windows STATUS_INVALID_HANDLE during GPU
startup; subsequent debug integration and both release snapshots passed. The
cause is not identified; investigate if it recurs rather than claiming that
all GPU startup paths have been established.

## Next acceptance work

1. Verify the schema-2 request against a loaded original-FSX C172 flight, then
   compare recorded indications and cockpit images. No original FSX process
   was running during this stage. Record exact weather/loading/realism settings.
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
