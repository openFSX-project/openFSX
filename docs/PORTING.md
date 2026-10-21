# Porting checkpoint

The target remains FSX 1:1 across interface, flight behavior, content and APIs.
This is a long-term acceptance target; no subsystem claims complete parity.

## Current stage — 21 October 2026

After `109bbb7`, the bounded local-time state accepts seven published CLOCK_*
events: hour/minute increment/decrement, hour/minute set and seconds zero.
Increment/decrement ignore the payload; SET validates its field range and
preserves other fields. Events preserve simulator fractional-second phase,
wrap within the local day and leave the configured reset origin intact.
Invalid events/payloads/times leave the prior local-time state unchanged.

The persistent schema-3 helper advertises clock_set and accepts `clock-set`
only after a physics frame exists. This updates just the guarded native clock
callback at unchanged simulator time, acknowledging the same sequence/time and
frames_evaluated count. The Rust bridge validates the acknowledgement before
committing the new local-time origin; stale-time edits fail without mutation.
Display invalidation is separate from physics reuse, so paused edits and
SEQ_REC selection changes update cockpit pixels without advancing flight or
restarting DLLs. Reset explicitly restores origin and deselects even at time zero.

The viewer exposes startup event injection through repeated
`--clock-event CLOCK_EVENT[:u32]` plus `--native-clock-select
none|hours|minutes|seconds`, requiring the opt-in local clock. Installed
Standard.xml contains no direct CLOCK_* binding; no default keyboard shortcut
has been invented. This is the event/state/render path, not native mouse/input
routing or general SimConnect event support.

154 all-feature Rust test executions pass, with the x86 synthetic self-test.
The connected flight check exercises all seven events at paused elapsed 0.75,
checks native PID/physics reuse and pixel invalidation, rejects invalid SET and
stale-time edits atomically, checks selected/deselected GPU output and reset at elapsed zero to
the configured origin (`target/native-clock-events-flight-02/`). Five malformed
native mutations are rejected (`target/native-clock-events-negative-01.json`).
The actual viewer startup event/selection command renders a checked cockpit
snapshot (`target/native-clock-events-viewer-01.ppm`). Existing flight, midnight,
minute/day wrap, pause, rewind, view, ground and helper-close checks still pass.

Calendar/UTC/timezone and environment lighting are not driven by these local
time events. Timer buttons, native keyboard/mouse routing, runtime user colors,
full panel services/lifecycle and FSX pixel equivalence remain absent or
unverified. Next work is native mouse rectangles/hit testing and input event
routing. Original-FSX acquisition stays deferred; the full 1:1 goal stays active.

### Previous completed native string selection stage

After `7089893`, native ELEMENT_STRING selection tables are decoded and can
select authored character ranges for highlighting. Each SEQ_REC has three
32-bit signed fields (state, first character, last inclusive character); a
zero/zero/zero record terminates the list. The decoder bounds each sequence to
256 records and a library to 65,536, checks file-backed RVA spans, and rejects
negative states, duplicate states, inverted/out-of-maximum ranges and malformed
terminators. State zero and states belonging to other gauges leave text alone;
valid ranges clip to the current shorter ASCII string.

`native_text::rasterize_selected` paints the selected glyphs with authored
HILITECOLOR using the existing shared layout/coverage. Unselected callers retain
their prior API/output. The installed five-library check found 20 string entries
(including aliases) and 22 selection records. C172's clock maps states 24/25/26
to hours/minutes/seconds at inclusive character ranges 0..1 / 3..4 / 6..7.

`native-string-render-check --selection-check` renders nine states: the existing
four baselines, three selected digit pairs, an unrelated state and deselection.
Selected rasters preserve identical alpha/coverage, with separate horizontal
regions; GPU differences are 54 / 69 / 68 pixels in the corresponding digit
pairs. Unrelated state/deselection restore exact baseline pixels. All four
unselected GPU frames remain byte-identical to the previous checkpoint.
Rasters and cockpit images were inspected (`target/native-selection-render-01/`).

153 all-feature Rust test executions pass, including malformed tables, duplicate
states, range clipping, truncated tables and traversal budgets. The first full
build collided with a running example's Windows executable lock; the sequential
rerun passed (`target/native-selection-workspace-tests-02.log`). The connected
clock/121-frame flight regression retains pause/reset/view/ground/helper-close,
midnight/minute/day and rewind checks (`target/native-selection-flight-regression-01/`).

Selection inputs in this checker are explicit synthetic states; no keyboard or
mouse selection/edit events are dispatched. Live flight remains unselected.
Next work is event routing/selection state and clock edit controls, followed by
remaining panel services and instrument systems. GDI/FSX pixel equivalence is
unverified, original-FSX acquisition remains deferred, and the full 1:1 goal
remains active.

### Previous completed persistent clock/time stage

After `32131cd`, opt-in native C172 local-time text runs in the same persistent
x86 session as the five needles and three gyro sprites. `--clock-frames` schema 3
adds explicit whole seconds of local time of day, guarded string results, native
connect/disconnect and buffer-restoration acknowledgements. Schemas 1/2 remain
unchanged. Source old/current values advance only with new simulator time;
unchanged time must keep both physics and clock inputs unchanged.

The headless simulation layer has a bounded deterministic LocalClock with an
explicit HH:MM:SS origin. It advances on elapsed simulator seconds, wraps at
midnight, freezes on pause and rewinds deterministically. The viewer enables it
with `--native-clock-start HH:MM:SS` alongside `--native-gauges`, loads the matching
installed Quartz font and draws the native callback output into the authored
C172 placement. Ordinary flight and reference replay keep their schema-2 profile.

151 all-feature Rust test executions pass, including clock boundaries and strict
schema-3 response validation. The x86 synthetic self-test passes. Six malformed
or inconsistent clock requests were rejected
(`target/native-clock-stream-negative-01.json`). The clock-enabled 121-frame
flight regression passes pause/reset/view/ground/helper-close checks, plus
23:59:59 -> 00:00:00 -> 00:01:00, a whole-day wrap, paused pixel equality and
rewind pixel equality (`target/native-clock-live-flight-02/`). Boundary comparisons
hold all other needle/sprite results fixed. The legacy profile also passes
(`target/native-clock-live-legacy-regression-01/`). Actual viewer CLI output and
its 1.25-second cockpit snapshot were checked
(`target/native-clock-live-viewer-01.png`).

This is a default-mode local-time display using unselected authored daylight
styling. Calendar/timezone/UTC synchronization, clock timer/selection buttons,
electrical/lighting state, runtime color overrides, full gauge services/lifecycle
and FSX pixel equivalence are still absent or unverified. Next work should extend
the actual instrument state/services and controls, retaining the isolated ABI
and pause/reset contracts. Original-FSX acquisition remains deferred; the full
1:1 goal stays active.

### Previous completed recorded clock rasterization stage

After `62dbd3f`, recorded native C172 clock strings can be rasterized with the
matching installed Quartz font and composed into the authored cockpit texture.
The automatic font size follows the published NUMCHARS-capital-W width rule;
this descriptor fits eight Ws into 43 pixels at about 10.899 pixels per em.
The rasterizer bounds input, verifies the face/weight/charset, decodes COLORREF,
handles single-line alignment and clips to the declared 43x13 extent. Fixed
font heights and unsupported effects fail explicitly. XML text reuses the same
bounded glyph painter without changing its evaluation or visibility behavior.

`native-string-render-check` validates three bounded connected-probe records,
resolves the current string RVA from tokens [3,4,5], and places each raster using
the root bitmap dimensions, native [6,23] position and panel.cfg rectangle.
The installed HILITE_LIST is present; this check explicitly requests unselected
text and does not evaluate selection events or runtime color overrides.

`target/native-clock-render-02/` contains 00:00:00 / 12:34:56 / 23:59:59 and a
repeat of the first state. GPU comparisons change 212 and 172 pixels within
[322,266]..[362,273]; restoring the first state restores identical pixels.
The small raster and cockpit snapshot were visually inspected. The report
labels synthetic time inputs, authored styling and unverified GDI pixel parity.

148 all-feature Rust test executions pass. All five XML numeric/visibility
rasters remain byte-identical to the previous checkpoint
(`target/native-clock-xml-regression-01/`). The 121-frame native flight/plaque
regression still passes pause/reset/view/ground/helper-close checks
(`target/native-clock-render-flight-regression-01/`). No operational clock,
flight-mode native string binding, selection controls, complete gauge lifecycle
or original-FSX pixel comparison is claimed. Next work is a bounded persistent
clock/time binding and pause/reset validation. Original-FSX acquisition remains
deferred and the complete 1:1 goal remains active.

### Previous completed native-clock connection stage

After `8b3922c`, the isolated C172 clock profile can deliver original gauge
CONNECT_TO_WINDOW/DISCONNECT notifications. Local callback inspection showed
that connect creates the gauge-private state used by the main time string;
ModuleInit alone left that state absent. The helper invokes the original
three-argument stdcall callback, with extra_data=0 for this checked headless
profile. It does not create a panel window or fabricate the private structure.

`--sample-string-connected` is limited to the C172 Clock [3,4,5] string and
integer f64 hour/minute/second inputs. Native connect creates state; the string
callback fills the guarded buffer; native disconnect runs before ModuleDeinit.
The public user_data slot is restored to its captured initial zero after native
disconnect, preventing a dangling slot in the shared template. Exceptional paths
also attempt disconnect while the module and output buffer are alive.

The Rust checker's `--clock` mode resolves the string RVA from metadata and
validates 00:00:00 / 12:34:56 / 23:59:59, including numeric returns 0 / 45296 /
86399. `target/native-clock-connected-02/` records these plus the unconnected
blank baseline. Six invalid type/range/fractional inputs were rejected. The
enhanced synthetic host self-test checks connect/disconnect argument passing
and exception containment; its build gate requires notification verification.

146 all-feature Rust test executions pass. Three Nav-Comm samples and the
121-frame native flight/XML-plaque regression remain passing
(`target/native-clock-radio-regression-01/`, `target/native-clock-flight-regression-01/`).
This is still an isolated, default-mode clock sample: no panel-service table,
install/update/draw lifecycle, timer buttons, simulated local-time service,
font sizing/rendering or cockpit clock binding is implemented. Original-FSX
acquisition remains deferred and the complete 1:1 goal remains active.

### Previous completed native-string sampling stage

After `b60cad2`, the read-only PE decoder recognizes x86 ELEMENT_STRING
metadata: three tokens/source callbacks, update callback, authored dimensions,
font/weight/charset/height, text flags, maximum length, COLORREFs and a bounded
selection-sequence pointer. Runtime string/GDI/source-value pointers are not
followed. All five checked libraries decoded; 20 reported string entries include
aliases. The C172 clock declares Quartz, automatic font height and eight chars.

The x86 helper adds `--sample-string` with explicit f64/u32 union payloads,
ModuleInit/Deinit, executable-pointer checks, source-callback rejection and a
guarded output buffer sized to the authored maximum (1..256). Its synthetic
self-test contains a deliberate overrun and detects a prefix overwrite; the
build/CI self-test gate now requires the string guard result.

`native-string-check` resolves the Nav-Comm 1 string from tokens [23,617,0], not
a hardcoded RVA, and validates three native outputs from synthetic BCD words:
118.25/120.50, 122.50/127.00 and 119.85/124.30. Metadata, type/value echoes,
text, guards and lifecycle fields pass (`target/native-string-check-01/`).
Numeric n-slot injection alone is incorrect for these BCD union fields; input
interpretation is now mandatory rather than inferred. Radio state/band logic
and native text rendering remain unimplemented.

The isolated C172/C182 clock callbacks returned blank text; a secondary clock
and transponder sample faulted inside the callback, contained by the helper.
No operational clock or complete gauge lifecycle is claimed, and private gauge
state/services were not fabricated to force a result. Next work should establish
the required lifecycle and native font sizing before integrating these strings.

146 all-feature Rust test executions pass, plus the enhanced x86 self-test and
three installed string samples. The existing 121-frame native flight check with
the XML plaque still passes pause/reset/view/ground and helper-close checks
(`target/native-string-flight-regression-01/`). Original-FSX acquisition remains
deferred; the broad 1:1 goal remains active.

### Previous completed XML visibility stage

After `93233bb`, translated XML text elements accept bounded read-only Visibility
expressions. Decoding retains parent-before-child condition nodes and authored
text/child drawing order. Rendering evaluates each condition once per state;
a hidden ancestor skips descendant lookups and text evaluation. Invalid,
unavailable or non-finite active conditions fail explicitly. String syntax is
compiled even for hidden text, so unsupported markup is not silently accepted.

The numeric raster checker now covers five synthetic states: two powered
baselines, an avionics-hidden row, all rows hidden by the parent, and restored
power. Hidden providers intentionally withhold unused values, and the report
records read names/visibility. Restoring power restores identical pixels;
the two original powered baselines remain byte-identical. Images were inspected
under `target/xml-visibility-render-01/`. These conditions use synthetic power
inputs and do not implement the aircraft's electrical system.

145 all-feature workspace test executions pass, including shared A:/P: numeric
expression boundaries, parent/child graph checks, lazy lookups and drawing-order
coverage. Actual FSX hidden-gauge update/lifecycle semantics and visibility
side effects remain unverified; writes/shared locals/events are unsupported.
Original-FSX acquisition remains deferred and the full 1:1 goal stays active.

### Previous completed explicit-alpha stage

After `2f5045b`, XML Image Alpha=True selects explicit 32-bit BI_RGB BGRA
decoding. Ordinary BMP/DIB decoding does not reinterpret unused high bytes as
opacity. Zero/partial/full alpha survive CAB -> typed XML -> image -> authored
panel composition, checked using independent synthetic resources and exact
expected pixels. Combined Alpha/black-key transparency fails until validated;
Luminous remains distinct and unsupported rather than becoming opacity.

PanelConfig now retains optional authored [Color] Day/Night/Luminous RGB values,
with byte-range checks and diagnostics for unimplemented lighting application.
The local C172 inspector reports the installed values directly. These metadata
are prerequisites for lighting, not a lighting model. The SDK documents an
inverse-alpha luminous mask and panel colors; it does not establish automatic
_night.bmp selection for this loader. No suffix substitution was guessed.

142 all-feature workspace test executions pass, including four new cases for
BGRA/truncation/orientation, typed Alpha behavior, exact composition and authored
color metadata. Actual FSX night selection, blending/dimming, Bright/Luminous
behavior and time-of-day/panel-light services still need implementation and
reference evidence. The current flight plaque stays in its daylight profile.
The existing two numeric/daytime raster cases also remained byte-identical
after the alpha change (`target/xml-alpha-day-regression-01/`).

### Previous completed numeric-text stage

After `6d0a1c9`, GaugeString additionally supports bounded numeric d/f output
and width-formatted A: strings. Numeric expressions retain explicit A:/P:
namespace and units. Integer output rounds, preserves sign and supports space/
zero padding and left alignment; fixed output uses explicit fractional precision
and the SDK's width-ignored behavior. Limits are width 256, precision 16 and
the existing 8 KiB total text budget. Unknown formats and unsafe widths fail.

The synthetic `xml-numeric-render-check` passes two complete XML -> lookup ->
format -> font rasterization cases for COM/altitude/VSI/heading text. Changing
inputs changed 2,824 pixels in the 400x68 diagnostic image; both local images
were inspected (`target/xml-numeric-render-01/`). These are synthetic diagnostic
readings, not operational radios or extra telemetry in the flight cockpit.
138 all-feature workspace test executions pass, including four new formatting
tests and the existing registration/conditional/renderer checks.

Integer half ties currently round away from zero; fixed formatting follows
Rust's binary-f64 rounding. Bare f uses a six-digit precision policy inferred
from C formatting, without proof that FSX uses that default. These choices,
zero-sign behavior and fractional numeric-to-string conversion still need
original-FSX validation. The latter currently fails explicitly. Original-FSX
acquisition remains deferred; full compatibility is not claimed.

### Previous completed XML rasterization stage

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
night-background selection, broader text formatting and interaction are not
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
