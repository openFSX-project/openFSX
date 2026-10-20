# Porting checkpoint

The target remains FSX 1:1 across interface, flight behavior, content and APIs.
This is a long-term acceptance target; no subsystem claims complete parity.

## Current stage — 20 October 2026

After `139f1bf`, the C172 XML registration plaque can be rasterized and composed
into its authored VC placement. The vehicle layer accepts a checked static
background/translated GaugeText subset and rejects unknown visual properties.
The renderer uses a caller-selected local font, bounded single-line ASCII,
COLORREF colors, alignment, glyph coverage, clipping and straight-alpha blending.
Neither original fonts nor artwork are added to the repository.

The viewer's opt-in `--xml-gauge-font <font.ttf>` requires `--native-gauges` and
adds the authored variant registration to the current daylight flight profile.
The installed XML omits FontFace: the caller explicitly supplies that mapping.
FontHeight is interpreted as pixels per em; the original FSX default face,
GDI metrics/filtering and rasterization parity remain unverified. Panel lights,
night-background selection, general text formatting and interaction are not
implemented by this option. Original-FSX acquisition remains deferred.

`xml-text-render-check` rendered four registration/color cases from the installed
XML using the explicitly selected Windows Arial file. Variant 0's alternate
color changed 276 cockpit pixels within (423,200)..(466,209); both variants and
their 89x25 gauge images were rendered. Local images were inspected. Reports and
ignored images are under `target/xml-text-render-02/` (initial inspection also
under `target/xml-text-render-01/`). This is a synthetic color-state comparison,
using the authored daytime background in both states, not a night-lighting model.

The actual viewer CLI snapshot passed with the new flag. The 121-frame native
flight regression with the XML plaque passed ten paused reuses, reset/helper
restart, view round trips and ground checks; reset/view pixels stayed identical
(`target/xml-text-flight-02/`). 134 all-feature workspace test executions pass,
including XML property rejection, color order, alpha and placement clipping.

## Previous stage — 19 October 2026

After `cb787c4`, GaugeString now evaluates bounded literal/string output,
read-only A:/P: conditions, nested if/else/end and default/alternate color spans.
Selected aircraft variants retain their authored atc_id, including an explicitly
empty value; missing IDs are not invented from titles. Writes, local/shared
gauge state, loops and unsupported formats fail explicitly.

`xml-gauge-check` evaluates the installed plaque's original XML with the actual
variant registration and explicitly synthetic time-of-day/panel-light inputs.
Variants 0/1/2/5 passed all six combinations each: 24 rows under
`target/xml-gauge-check-01/variants.ndjson`, with eight alternate-color results.
The interpreter never reparses returned registration text as executable markup.
130 workspace test executions with all features pass, including four new
synthetic string tests and expanded variant-selection/bounds checks.

At that checkpoint the plaque was still omitted from flight rendering. The next stage needed typed
GaugeText visual properties, an explicit font source/default-font decision,
text rasterization and VC placement with day/night/registration image checks.
No font metrics or real-FSX text-rasterization parity is established by the
successful expression evaluation. Original-FSX acquisition remains deferred.

### Previous completed CAB/XML decoding stage

Current HEAD before this stage: `3cf4387`. The goal remains active. The user
deferred original-FSX reference acquisition and asked to continue independently.
The attempted local FSX launch was canceled at Windows consent; no live capture
was obtained. Only ignored scenario/config copies were prepared, and the source
user configuration SHA-256 was unchanged (`target/reference-fsx-live-01/`).

The next XML-panel stage now has a bounded stored/MSZIP CAB reader and a
read-only ACE XML tree decoder in fsx-vehicle. It handles reserved archive fields,
cross-block DEFLATE history and contained image references. The CLI command
`inspect-cabinet` inspects these documents; ArtDirectory is never followed.
Unknown nodes remain visible in the tree rather than becoming supported elements.

Local independent evidence: all three original n_number_plaque.cab resources
match Windows Expand byte for byte (6,353 decoded bytes), checked by the new
`cabinet-verify` example against `target/cabinet-reference-01/`. Six synthetic
tests cover archive/XML truncation, checksum corruption, dictionary dependency,
declared output bounds, reserve fields, unsafe paths, unsupported compression,
entities, nesting and malformed documents. The 126 workspace test executions
with all features pass. That stage did not yet draw the plaque: GaugeString,
ATC-ID binding, fonts and daytime/nighttime text colors are the next work.

### Previous completed instrument/reference stage

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

1. Establish the FSX text/default-font contract and validate pixels when reference
   acquisition resumes. Add actual time-of-day/panel-light state and authored
   night-bitmap selection before calling this general day/night XML support.
   Extend strings beyond the current single-line ASCII/conditional subset.
2. When the user resumes original-FSX acquisition, verify schema 2 against a
   loaded C172 flight and compare indications/cockpit images with exact
   weather/loading/realism settings. Acquisition is currently deferred.
3. Add authored temporal response only after establishing the native lag contract.
4. Validate gyro filtering/placement and add drift, failures and adjustment
   controls; extend other moving faces. Model engine/propeller state before
   treating tachometer input as telemetry.
5. Extend XML gauges, digital text, interaction and PANELS services as tested
   subsets. Do not present decoded static artwork as functional readings.
6. Continue reference flight validation, broader aircraft dynamics, airport/BGL
   scenery and menu parity. See [compatibility](COMPATIBILITY.md) for current gaps.

Keep this checkpoint current at each completed stage so interrupted work can
resume from concrete evidence. Preserve unfinished changes until verified.
