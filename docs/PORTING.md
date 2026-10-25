# Porting checkpoint

The target remains FSX 1:1 across interface, flight behavior, content and APIs.
This is a long-term acceptance target; no subsystem claims complete parity.

## Current stage — 26 October 2026

After `7112c78`, the original base-context constructor resolves initial ownership:
it writes self into +0x34 unconditionally, then queries its own +0xB4 through
both virtual calls before copying General from the actual main.dll getter.
The preceding constructor stores zero into response +0x38. A nonzero flag copies
General with float32 rounding; zero leaves the initial zero. This establishes
fresh construction, not later pointer changes or the flag's flight-mode meaning.

The guarded response reference now executes the original zeroing/self-link/copy
fragments and resolves the identified main import to its original pinned export.
20 constructor cases extend the prior 80 independent-context traces to 100.
Response.constructed_self_context models the fresh initialization, with unknown
flag/settings remaining unknown when required. All factors and provider-driven
filter/voltage results match with zero observed error in
target/legacy-electrical-self-context-comparison-01/report.json, using
target/legacy-electrical-self-context-reference-01/report.json. Workspace tests
pass in target/legacy-self-context-tests-01.log. Formatting, architecture and
generated docs checks pass. Full Goal remains active; automatic flight context
selection, live RPM/load activity and later ownership transitions remain open.

### Previous context-query stage — 25 October 2026

After `c2e1477`, the primary realism-update query is resolved through the
original concrete context vtable: slot 0x50 returns raw context+0xB4. This is the
same primary field already consumed by the electrical availability predicate.
The response reference now executes that original getter instead of intercepting
its calls. Its linked receiver at primary+0x34 is explicitly instantiated with
the same concrete vtable for this fixture; the actual linked object's runtime
type/ownership is not yet established.

80 cases cover General 0/0.1/0.5/1/1.25 and both raw flags 0/1/2/0xFFFFFFFF.
Every nonzero flag admits the relevant branch. The provider's new
copy_concrete_context_settings path resolves known raw words, preserving missing
flags as unknown, and feeds the existing voltage core. All 80 factors/filter/
voltage results match original instructions with zero observed error in
target/legacy-electrical-response-context-comparison-01/report.json, using
target/legacy-electrical-response-context-reference-01/report.json. Workspace
tests pass in target/legacy-response-context-tests-01.log. No automatic mode,
linked-context type or flight provider is inferred. Full Goal remains active.

### Previous provider-to-core stage — 25 October 2026

After `a37fb0b`, the response comparison now feeds the parsed/copied factor into
the Rust voltage core and compares both filtered current and battery voltage
against all 20 original filter/voltage traces. Tolerance is 1e-12; observed
maximum error is recorded in target/legacy-electrical-response-comparison-02/report.json.
It also checks that an admitted missing setting invalidates evolving voltage
knowledge and that invalid settings updates leave the preceding factor intact,
including when copying would otherwise be skipped. This closes the explicit
provider-to-core comparison; live context-gate semantics and automatic flight
ownership remain unfinished. Formatting and diff checks pass.

### Previous response-provider stage — 25 October 2026

After `4eb07f2`, explicit General realism inputs are available through
fsx_vehicle::realism::Realism and fsx_sim::legacy_response::Response. The parser
reads [REALISM] General without a missing-key default. Supported electrical
inputs are finite nonnegative doubles representable as finite float32; values
above 1 are retained, not clamped. This is a supported input domain, not a claim
about the original UI/parser rejection policy.

Response retains its observed factor when the update gate is false or when the
update gate is true and the copy gate is false. Both true copies General with
float32 rounding. Missing admitted values or unresolved gates lose knowledge;
invalid values fail atomically. Gate meanings and automatic ownership remain
unresolved. factor() supplies the existing explicit voltage-core response input;
ordinary flight does not select settings or gates automatically.

The original reference now varies both gates independently: 20 original traces
match the Rust provider exactly in
target/legacy-electrical-response-comparison-01/report.json, using
target/legacy-electrical-response-reference-02/report.json. Missing/invalid
settings checks pass. Workspace tests pass in target/legacy-response-provider-tests-01.log.
Formatting, architecture and generated documentation checks pass.

### Previous response-provenance stage — 25 October 2026

After `0cb9669`, response-factor provenance is narrowed by an original
cross-module trace. main.dll ordinal 57 returns the global settings record whose
descriptor names [REALISM] General at double offset 0x20. The sim1 settings-copy
method converts that double to float32 at context+0x38 when its two injected
context queries are active. The electrical filter/voltage fragment reads that
same context field via its settings owner. When the first query is inactive,
the preceding context factor is retained. This is a general-realism setting,
not an inferred battery capacity or temperature factor.

scripts/check-electrical-response.py pins both original binaries and executes
the getter, complete settings-copy method and electrical filter/voltage fragment
under instruction/write guards. Ten cases include General 0/0.1/0.5/1/1.25 and
active/inactive queries with prior factor 0.75. All copied factors and voltage
updates match the checked arithmetic. Evidence:
target/legacy-electrical-response-reference-01/report.json. No process was
launched or original bytes exported. Settings-file parsing, defaults, query
meaning, other writers and automatic viewer ownership are still unverified;
there is no implicit factor 1 or new automatic physics binding.

### Previous fresh-initialization stage — 25 October 2026

After `0ed7c4b`, fresh electrical allocation and complete initialization are
checked against the pinned original. The original constructor runs on poisoned
memory and clears bus/current/filter/load/master/count scalars and consumer
records. The initializer sets stored battery voltage and all eight bus voltages
to effective max_battery_voltage. It preserves current/filter/load/master/count
history when called on an existing object; initialization is not a fresh reset.

40 original cases cover fresh/reused state, 0/12/24/28/1000 V battery ratings,
0/28 V generator ratings and enabled/disabled generator configuration. Context,
failure-object factories and generator naming are guarded injected services.
32 positive-rating fresh/reused cases then execute one full electrical frame
using explicit factor 1, zero auxiliary, retained battery-master state and zero
circuit metadata. All
compared Rust initialization and first-frame values match exactly.

The first-frame comparison found a port bug hidden by earlier zero incoming
generator-bus fixtures: original admission skips writing generator bus voltage
when nominal generation is zero or the source-map bit is disabled. The port
now preserves its previous observation in those branches. TailEnvironment
requires generator_enabled explicitly; an enabled offline positive-rated source
still writes zero. An online/disabled contradiction is rejected atomically.
These initialized or retained voltages are legacy observations, not energy
delivery inferred from generator admission.

Core.constructed creates the checked zero state. initialize_rated_voltage
replaces voltage, preserves known filter/current history and leaves missing
history unknown. Missing ratings clear knowledge; invalid ratings are atomic.
CircuitNetwork.new_fresh_legacy selects the pinned single-piston source profile,
publishes initialized buses and establishes battery master/source admission off.
Coulomb charge remains unknown. Existing unknown construction, generic reset and
ordinary Simulation.step are unchanged. This explicit fresh-allocation API is
not FLT restoration or a live state provider for an already running aircraft.

The installed C172 checker confirms fresh 24 V initialization, then applies
Lesson 1 switches/RPM with explicit load/response/context/failure/NAVCOM inputs
and reaches 28 V main/avionics. Existing 1280 predicate / 165 tail / 68 frame /
16 override-load regression passes. 214 all-feature test executions pass in
target/legacy-electrical-initialization-workspace-tests-02.log. Original evidence:
target/legacy-electrical-initialization-reference-05/report.json; direct Rust
comparison: target/legacy-electrical-initialization-comparison-03/report.json;
installed checks: target/legacy-electrical-initialization-installed-check-01/;
tail regression: target/legacy-electrical-initialization-tail-regression-01.log.
Formatting, architecture and generated documentation checks pass.

Next: response-factor/context providers, actual evolving RPM and equipment load
activity, process override ownership, then automatic flight and OAT/V binding.
Original-FSX live capture remains deferred. REA MCP is requested but unavailable
in this session's tool inventory/configuration; connection details are pending.
The full FSX 1:1 Goal remains active.

### Previous global-override stage — 24 October 2026

After `7202ab2`, the original global power override is traced to the final
Electrical/electric_always_available CFG lookup. Two bounded sequences execute
16 original configuration-read tails, including absent, true and false keys
with both observed initial values. The injected CFG lookup writes only for a
present key. The original code does not initialize this destination before the
lookup: a missing key retains the prior process value. Each resulting global is
then consumed by the original power predicate at zero voltage with forced-off
equipment, proving the bypass connection. All 16 transitions match Rust exactly;
the existing 1280 predicates, 165 tails and 68 complete controlled frames still
pass (maximum frame error about 3.55e-15).

legacy_power::PowerOverride models this versioned process state separately from
an aircraft network. Call load_configuration with each parsed Electrical, and
carry the object across aircraft changes. Its initial value is unknown unless
observed; an omitted key cannot turn unknown into false. An explicit key resolves
the value. The resolved value can supply TailEnvironment.override_power. Process
startup, other writers, automatic viewer ownership and live context mode remain
unverified. The regular circuit inspector still rejects its existing override
mode; these raw bypass flags are not evidence of delivered electrical energy.

Reference evidence is in target/legacy-power-override-reference-03/report.json;
Rust comparison is in target/legacy-power-override-comparison-01/report.json.
All-feature workspace tests pass in
target/legacy-power-override-workspace-tests-01.log. Formatting, architecture and
generated documentation checks pass. Next: confirm full initialization and the
response/context providers, then real RPM, equipment activity and flight binding.
Original-FSX live capture remains deferred; the full FSX 1:1 Goal is active.

### Previous electrical-tail stage — 24 October 2026

After `a162101`, original circuit availability and the battery-failure/avionics
tail are ported and checked. 1280 original predicates cover exact/nextafter voltage
thresholds, absent equipment, invalid bus mapping, forced-off, zero maximum battery
voltage, global override and context fallback. 165 original tails cover failure
clearing, avionics-master control, sequential navcom checks and unusual prior-bus
inputs. Rust matches every tested predicate and tail output exactly.

This exposed and corrected the earlier regular inspection threshold: voltage must
be strictly above the declared minimum, not equal. At 17 V the C172 17 V consumers
are off; nextafter(17,+infinity) admits the requested 7 A pitot/nav maxima. An
explicit zero maximum battery rating disables regular voltage checks; device
requests still govern the inspection subset. The separate raw predicate also
preserves original early fallback/bypass ordering. The global override's origin
and context mode's live mapping remain unverified; they are explicit inputs.

A battery failure clears all eight bus voltages, four generator-current slots and
reported source sum, while retaining stored battery voltage and filtered current.
Avionics availability is then evaluated, its bus copies main voltage only when
master and availability permit, and navcom1 is checked against the updated bus.
Availability flags can remain true at zero voltage under bypass/fallback; they
are not a claim of physical energy delivery.

The full original electrical method also runs in four controlled sequences / 68
frames with one generator and two explicit consumers. Load is summed from prior
consumer flags, then prefix and tail execute in original order. Toggle, failure/
recovery, exact threshold and empty-battery override cases match Rust within
1e-9 (maximum observed error about 3.55e-15). This does not infer actual aircraft
equipment presence, source/response/mode providers or all 33 consumers.

Core.advance_frame composes both phases atomically and stores post-failure source
current for the next step. Network.step_legacy_electrical_frame requires the
versioned profile and complete explicit inputs, publishes validated final bus
values, and clears stale observations when providers are unavailable. Invalid
input leaves state and observed buses intact. The installed C172 checker proves
known-input 28 V main/avionics publication, failure-to-zero and loss-to-unknown
using labeled synthetic seed/load/response/context/NAVCOM data. Ordinary physics
still does not select those inputs or run this path automatically.

Next is confirming response-factor, context/override and initial-state providers,
live engine RPM and complete equipment load activity, followed by automatic flight
and secondary OAT/V binding. Full FSX 1:1 remains unfinished; user-deferred live
original-FSX acquisition has not been attempted.

213 all-feature Rust test executions pass in
`target/legacy-power-tail-workspace-tests-02.log`. Original predicate/tail/frame
evidence is in `target/legacy-power-tail-reference-03/report.json`; comparison
passes in `target/legacy-power-tail-comparison-03/`. Installed C172 and explicit
frame publication checks pass in `target/legacy-power-tail-installed-check-02/`.
Native clock/buttons/picking/GPU/121-frame flight/pause/reset/view/ground regression
passes in `target/legacy-power-tail-flight-regression-01.log`. Formatting,
architecture and generated documentation checks pass.

### Previous voltage-core stage — 23 October 2026

After `a89f7ae`, original electrical frame ordering is checked through battery
voltage storage, before the battery-failure and avionics tail. Nine bounded
sequences execute 380 original core invocations with persistent voltage/filter/
allocation state. They include discharge, start/stop lag, zero/float32/double
response factors, empty/full battery, overload, battery-master isolation and a
zero-generator-rating battery-only path. Failure service returns false, auxiliary
returns zero and a single explicitly active consumer supplies the requested load.
This is a checked original prefix, not a complete aircraft electrical frame.

The core reads prior allocated generator current before calculating the next
allocation. It filters net current with coefficient 4*dt, changes battery voltage
using the filtered value, a native float32 response factor and coefficient
0.000555, clips below zero, allocates the next current from that intermediate
voltage, adds the prior-current 0.0004*dt term and clips above the configured
maximum. Allocation precedes the upper clamp, so switching off a large load near
full voltage can retain the original small negative current request. Pre-load
main/battery/hot-battery readings describe the preceding voltage, not final loaded
bus state. The response factor's physical source remains unverified; no capacity,
temperature or nominal value is substituted.

fsx-sim::legacy_voltage owns explicit optional seeded state and implements these
ordered operations. Missing providers lose state knowledge over a positive step;
invalid input/computation fails atomically. Response factor is Option<f32>, and
zero maximum battery voltage is explicitly outside this checked core because the
original auxiliary sum divides by it. Network APIs require the versioned profile,
use stored battery-master/source availability, retain state across control changes
and clear it on reset. They do not run automatically in ordinary physics, publish
loaded bus observations or translate voltage into Coulomb charge fraction.

legacy-voltage-core-check compares the Rust implementation directly against the
original JSON traces, including each incoming state, outgoing state, pre-load
readings and prior source sum. All 380 frames match within 1e-9; maximum observed
absolute error is about 1.42e-14. Unit checks cover lag, intermediate allocation
before the upper clamp, signed requests, unknown response, invalid/overflow input,
profile gating, reset and absence of automatic bus/charge publication.

Next is battery-failure/avionics tail processing and confirming the response
factor's source, followed by actual seed/live engine/load providers and loaded
bus/secondary OAT/V binding. Full FSX 1:1 remains unfinished; original-FSX live
capture stays deferred by the user.

208 all-feature Rust test executions pass in
`target/legacy-voltage-core-workspace-tests-01.log`. Original prefix traces pass
in `target/legacy-voltage-core-reference-02/report.json`; the direct 380-frame
Rust/original comparison passes in `target/legacy-voltage-core-comparison-02/`.
Installed C172/FLT/source-phase regression passes in
`target/legacy-voltage-core-installed-regression-01.log`. Formatting, architecture
and generated documentation checks pass.

### Previous current-allocation stage — 23 October 2026

After `50c2826`, the original generator-current allocation phase is checked in
386 bounded sim1 10.0.62615.0 cases. The reference runs original allocation and
then separately re-sums current with a synthetic zero auxiliary service. It
covers one/two generator flags, no/one/two active sources, zero/60 A capability,
12/24 V maximum battery ratings, varied battery voltage and load, upper clipping,
inactive stale-slot clearing and above-maximum voltage fixtures. This explicit
re-sum is a probe operation, not a reordering of the original electrical frame.

For one active generator, original allocation is
min(max_generator_amps, load_amps + (max_battery_voltage - battery_voltage)).
There is no zero floor: an above-maximum battery voltage fixture can yield a
negative allocation. The port preserves this isolated original arithmetic rather
than adding an unverified physical charging model. The two-generator reference
also verifies original division by active count; supported flight remains one
piston engine, so no general parallel-source solver is enabled by this evidence.

Generator.fsx_steam_62615_allocated_current implements the single-source phase.
Network access requires the explicit versioned profile and uses its effective
maximum battery rating. Load and battery voltage are separate provider inputs;
CFG consumer maxima are not substituted. Unknown required inputs give None;
a known offline source gives zero and invalid values fail. The result is not
measured/delivered current or a solved battery terminal current and is not applied
to charge, bus observations or ordinary physics automatically.

The installed checker adds eight explicit cases through C172's effective source
profile and known saved switch: balanced 12 A load gives 12 A, battery voltage
22 V against a 24 V maximum gives 14 A, excess demand caps at 60 A, and above-max
voltage gives the original signed result. Missing load/voltage and offline supply
remain distinct. Unit checks also cover invalid inputs, profile eligibility and
absence of accidental bus/battery state publication.

Next is original full-frame ordering, prior-current summation/filtering and
battery-voltage dynamics, plus a live engine/load provider. Loaded bus state,
live OAT/V and full FSX 1:1 parity remain unfinished. Original-FSX live capture
stays deferred by the user.

204 all-feature Rust test executions pass in
`target/generator-current-workspace-tests-01.log`. Original allocation/sum evidence
is in `target/generator-current-reference-02/report.json` (386 cases, including
stale-active flags with zero admitted count). Installed C172 and eight explicit
allocation probes pass in `target/generator-current-runtime-check-01/report.json`.
Formatting, architecture and generated documentation checks pass.

### Previous named RPM-fraction stage — 23 October 2026

After `82d69b8`, the generator admission signal is now identified as engine RPM
divided by piston_engine max_rated_rpm. Thirty bounded original traces connect
the named CFG lookup destination, original rated-RPM copy and signed engine RPM
reconstruction to the original FLT save routine and its Pct Engine RPM descriptor.
The same runtime fraction feeds original generator admission. Cases cover three
rated speeds, zero/boundary/nextafter/running/overspeed fractions and both rotation
directions. Numeric CFG lookup is injected; the original reconstruction and FLT
save instructions run in guarded emulation. Engine dynamics are not substituted.

LegacyGeneratorInputs now names rpm_fraction explicitly: 1 means rated RPM,
0.1 means ten percent and admission is strictly above that boundary. Raw RPM,
throttle and values expressed as percent in 0..100 are not equivalent inputs.
FLT reading retains four optional finite Pct Engine RPM fractions, preserving
overspeed and signed rotation. Missing data remain unknown. The explicit
update_generator_from_saved_flt API combines the saved first-engine RPM fraction
and generator switch with independent source-enable/failure inputs. It is a
point-in-time initialization/probe operation, not a continuous engine provider.

Installed Lesson 1 and FLTSIM snapshots provide their actual stored fractions
(about 0.887 and 0.796). The checker verifies exact parsing without percent
rescaling, demonstrates admission only under explicitly supplied enabled/no-fault
conditions, and asserts that loaded bus voltage stays unknown. Unit checks cover
signed/overspeed/missing/nonfinite values, unknown independent gates and the
saved-fraction boundary. Ordinary physics does not reuse a saved RPM snapshot as
live engine state and does not infer RPM from the throttle/power model.

Next is a live engine RPM provider and original source-current/battery dynamics,
followed by loaded bus state and live OAT/V. Full engine simulation, scenario
restoration and FSX 1:1 parity remain unfinished; live original-FSX capture stays
deferred by the user.

202 all-feature Rust test executions pass in
`target/generator-rpm-workspace-tests-01.log`. Named original mapping evidence is
in `target/generator-rpm-trace-reference-01/report.json` (30 cases); preceding
admission/scalar modes pass in `target/generator-rpm-admission-regression-02/`
(87 cases) and `target/generator-rpm-scalar-regression-01/` (23 cases). Installed
C172/FLT and explicit snapshot runtime checks pass in
`target/generator-rpm-runtime-check-01/report.json`. Formatting, architecture and
generated documentation checks pass.

### Previous generator-admission stage — 23 October 2026

After `d99b762`, bounded reference execution now covers original pre-load
generator admission in sim1 10.0.62615.0. The additional mode runs 87 cases:
source-enable mask, switch, failure and battery-master combinations, the exact
0.1 engine-signal boundary and the next representable value, a zero-rated source,
and previously-online transitions. Failure service responses are injected and
the auxiliary source returns zero. This is not a complete electrical frame or a
live FSX observation. The engine signal's physical mapping remains unverified;
it is not supplied from throttle, raw RPM or the FLT Pct Engine RPM value.

Original admission requires the enabled source, on switch, no failure, positive
nominal voltage and engine signal strictly greater than 0.1. At exactly 0.1 it is
off. The count of sources admitted during this invocation is authoritative for
this result: the original routine can retain an old engine-active flag and old
source-voltage slot when a source-mask bit is disabled, while excluding that
source from pre-load main voltage. The report retains these distinct fields;
stale raw slots are not treated as a newly admitted source.

Generator.update_fsx_steam_62615_availability implements this known-input gate.
Missing inputs remain unknown unless a known condition proves off; nonfinite
signals fail atomically. CircuitNetwork's versioned signal API combines explicit
provider inputs with its stored first-engine FLT generator switch. It refuses an
unprofiled network and clears prior bus/current observations only after a valid
update so distribution and battery-current allocation must be recomputed. It does
not run automatically in physics, infer an engine signal, or energize a bus.

The installed checker exercises six explicit admission cases after applying real
FLT switch positions, including boundary, failure and unknown inputs. Default
flight state stays unresolved without engine/failure/source providers. Unit tests
cover all known gate combinations, exact boundary, unknown/off propagation,
profile eligibility, stale-state invalidation, rejected input and switch binding.
The earlier 23-case scalar reference mode remains passing.

Next is confirming the engine signal's meaning and binding actual engine state,
then original current allocation, loaded bus/battery dynamics and live OAT/V.
These and full FSX 1:1 parity remain unfinished. Original-FSX live capture stays
deferred by the user.

200 all-feature Rust test executions pass in
`target/generator-admission-workspace-tests-01.log`. The 87-case original reference
passes in `target/electrical-generator-admission-reference-02/report.json`; the
23-case scalar regression passes in `target/generator-admission-scalar-regression-01/`.
Installed C172/FLT, six admission cases and preceding synthetic runtime checks
pass in `target/generator-admission-check-01/report.json`. Formatting, architecture
and generated documentation checks pass.

### Previous versioned scalar-default stage — 22 October 2026

After `f0662ae`, three previously unresolved source ratings now have a versioned
reference basis. A bounded Unicorn x86 check reads the user's installed sim1.dll
(version 10.0.62615.0, SHA-256 pinned) and executes its original source scalar
initialization/engine dispatch. It stops after three numeric CFG lookups. Only
allocation and numeric CFG services are stubbed; instruction ranges, writes and
instruction count are guarded. No FSX process starts, no DLL is loaded by Windows,
and no original instructions/assets are embedded or exported.

23 cases cover engine types 0..6, engine counts 0/1/4 and two override fixtures.
The single-piston/one-engine path initializes maximum battery voltage to 24 V,
generator voltage to 28 V and generator capability to 60 A. These are original
initializer results, not borrowed aircraft examples or live measurements. A
foreign binary is rejected before emulation and creates no report directory.
This does not verify the real CFG parser, generator regulation or battery decay.

Electrical.with_source_defaults applies only these three missing ratings under
FsxSteam62615SinglePiston, preserving authored values including zero. Authored CFG
metadata remains unchanged. The generic CircuitNetwork constructor remains
unresolved; the single-piston Simulation explicitly selects this reference
profile. Reset reuses the profile but clears charge, availability, curves and bus
observations. Battery capacity, charging current, voltage curve and omitted
circuit defaults remain unknown. No source is made online and no bus is energized
from a rating alone; the default flight display still receives no bus voltage.

The installed C172 checker now distinguishes absent authored settings from the
24/28/60 effective source profile, verifies its ideal generator capacity boundary
under explicitly supplied online state, and retains the unknown live-supply,
FLT, circuit/distribution and flight-time checks. Unit checks cover authored zero,
profile selection, missing battery metadata and profile-preserving reset. The
broader reference type/count matrix does not expand supported flight engines.
The existing single-piston eligibility check now rejects every additional engine
slot 1..3, including gaps that previously slipped past the engine.1-only check.

Next is actual engine/regulator state, terminal-current allocation, confirmed bus
topology and battery behavior, followed by live OAT/V. Electrical dynamics and
complete FSX 1:1 parity remain unfinished. User-deferred original-FSX live capture
has not been attempted.

197 all-feature Rust test executions pass in
`target/electrical-defaults-workspace-tests-02.log`. Original initializer evidence
is in `target/electrical-default-reference-01/report.json`; unsupported-binary
rejection is recorded in `target/electrical-default-reference-rejection.log`.
Installed C172/FLT and synthetic runtime checks pass in
`target/electrical-defaults-check-02/`. Native clock/buttons/picking/GPU/121-frame
flight/pause/reset/view/ground regression passes in
`target/electrical-defaults-flight-regression-01.log`. Formatting, architecture
and generated documentation checks pass.

### Previous FLT switches and simulation-time stage

After `0066b95`, legacy FLT electrical switch positions can be read and applied
explicitly to the simulation-owned circuit runtime. Installed Lesson 1 and
FLTSIM flights exercise both legacy Systems.0 and Avionics.0 locations for the
avionics switch. Battery and four engine generator switches remain distinct
from source availability; eleven known pitot/light switches map only to authored
aircraft circuits. Missing fields remain unknown and conflicting duplicate
locations fail rather than choosing an unverified precedence. True/False parsing,
Windows-1252 loading and bounded file/section sizes are checked. This is a switch
reader/application API, not full flight restoration or automatic viewer startup.

Applying a switch snapshot replaces supported mapped requests, preserves unrelated
manual requests, clears stale bus/source-availability/current observations and
retains the battery's existing charge estimate. It does not manufacture aircraft
topology, charge or a running generator from a switch position. An empty snapshot
clears old supported switch requests instead of inheriting another flight's state.

Battery integration now runs once for each committed physics interval, including
the smaller ground-contact substeps. Its current request must come explicitly
from a source provider; CFG consumer maxima are never used automatically. A
missing request invalidates known charge on positive elapsed time. Invalid physics
input does not advance charge, and the desktop scheduler's pause does not consume
wall time. Reset clears current/flow/switch state; suspension settling precedes
the flight epoch and its interval record is discarded when time returns to zero.
The most recent flow describes the last committed substep, not a whole frame.

Tests cover airborne/contact timing without double counting, frame partition,
pause/resume, rejected inputs, unavailable current and reset. The installed checker
also applies two real FLT switch snapshots to the C172 circuit runtime while
asserting that generator availability and voltage remain unknown. A separately
labeled 40 Ah / 36 A fixture coupled to C172 flight physics loses 0.00025 charge
fraction over one simulation second; this is not the installed aircraft's rating
or an original-FSX measurement. Physical distribution remains provider-driven.

Next is actual engine/regulator availability, terminal-current allocation and
confirmed aircraft source/default/topology data, followed by live secondary OAT/V.
Source sharing/charging, FSX voltage behavior, complete FLT restoration and full
FSX 1:1 parity remain unfinished. Original-FSX acquisition stays deferred.

194 all-feature Rust test executions pass in
`target/electrical-flight-time-workspace-tests-02.log`. Installed C172/FLT and
synthetic electrical checks pass in `target/electrical-flight-time-check-01/`.
The native clock/buttons/picking/GPU/121-frame flight/pause/reset/view/ground
regression passes in `target/electrical-flight-time-regression-01.log`.
Formatting, architecture and generated documentation checks pass.

### Previous explicit distribution stage

After `fac489c`, explicit source distribution is available for a caller-supplied
complete inventory of up to eight feeds and 28 bus contacts. Closed contacts
are ideal bidirectional conductors; open contacts isolate groups. A sole known
online source supplies connected fixed-current requests within its rating, with
current conservation across the group. All groups without a connected source
are deenergized only because the caller explicitly declares the full inventory.
No aircraft graph is synthesized from bus names or engine-generator counts.

Unknown contacts propagate uncertainty through all possibly connected groups;
redundant unknown contacts within an already connected group do not invalidate
known paths. Unknown availability/load/rating, overload and parallel sources
remain unresolved. No source priority, equal sharing, charging path, voltage
droop, battery internal resistance or automatic consumer activation is invented.
The solver takes independent constant-current requests, never silently treating
CFG requested maxima as actual current. This remains an ideal distribution
profile, not verified FSX electrical behavior.

Generator.feed exports its existing configured ratings and explicit availability
for the caller-selected bus. CircuitNetwork.distribute solves and publishes all
eight bus results atomically; invalid input preserves the preceding readings.
Authored circuit evaluation then observes those bus results normally. Clearing a
contact observation or demand removes stale resolved voltage; reset clears all
published distribution. Providers must supply a fresh inventory on each solve.
Ordinary flight stepping still has no source/default/activity providers and does
not call the solver or infer C172's missing source parameters/topology.

The installed checker adds seven separately labeled synthetic distribution cases:
closed main/avionics paths carry 7 A + 5 A from one 28 V / 60 A source; opening
avionics drops source current to 7 A; opening the main feed isolates both loads.
Unknown contact/load, overload and an offline generator have distinct expected
results. These values and wiring are not installed C172 defaults or original-FSX
measurements. Unit checks additionally cover reverse contact orientation, chains
of unknown contacts, redundant paths, parallel-source refusal, malformed inputs,
current conservation, circuit power publication, atomicity and reset.

The next work remains real source/default/activity providers, engine/regulator
availability and integration with flight time, then live secondary OAT/V binding.
C172 live electrical generation, battery/load coupling, source sharing/charging,
FSX voltage behavior and the full 1:1 objective remain unfinished. Original-FSX
acquisition stays deferred by the user.

189 all-feature Rust test executions pass in
`target/electrical-distribution-workspace-tests-01.log`. The installed C172 and
synthetic source/distribution checks pass in
`target/electrical-distribution-check-01/report.json`. Formatting, architecture
and generated documentation checks pass. Default flight behavior remains outside
this explicit solver path; this milestone does not claim new native/GPU parity.

### Previous explicit battery and generator stage

After `2d982d7`, the simulation-owned electrical network also owns explicit
battery/generator primitives. Authored battery capacity (Ah) and charging-current
limit are parsed as optional finite nonnegative values, including explicit zero.
Missing legacy defaults and the battery CFG voltage-curve grammar stay unresolved.
C172 still supplies none of these source ratings: no nominal voltage/capacity or
initial charge has been added to its flight state.

The battery integrates explicitly requested terminal current in amp-hours, caps
charging at an explicitly configured limit, clamps charge at empty/full and
reports the average current actually accepted over the interval. Unknown current,
required capacity or charging limit makes evolution unknown. Zero elapsed time
preserves state; invalid inputs fail atomically. Zero capacity is retained but
cannot be evolved by this storage model. A caller can supply a bounded monotonic
open-circuit voltage curve; this is a separate API, not a decoder for the legacy
CFG curve. Without a curve, only known full charge plus authored maximum voltage
can resolve open-circuit voltage. It never becomes loaded main-bus voltage.

The generator accepts explicit upstream availability and known requested load.
Within a known current capability it returns the authored ideal regulated voltage;
overload/unknown capability remains unknown without a droop model. Reset clears
charge, curves and generator availability while preserving configured ratings.
Neither source is automatically connected to a bus. Physics does not yet advance
these sources: actual terminal currents, engine/regulator state and distribution
must be integrated before source state can supply the secondary OAT/V display.
Authored requested consumer maxima are not used as actual battery current.

Unit checks cover conservation, empty/full boundaries, charging limits, missing
inputs, zero capacity, explicit-curve interpolation, invalid-input atomicity,
generator capacity/overload and network reset. The installed checker retains the
17 C172 circuit/threshold/lifecycle checks and adds a separately labeled synthetic
40 Ah / 4 A battery and 28 V / 60 A generator fixture. These values are not C172
ratings or original-FSX reference measurements. The full FSX 1:1 target remains
unfinished; original-FSX acquisition remains deferred by the user.

183 all-feature Rust test executions pass in
`target/electrical-sources-workspace-tests-01.log`. Installed and synthetic checks
pass in `target/electrical-sources-check-02/report.json`. The existing native
clock/buttons/picking/GPU/121-frame flight/pause/reset/view/ground regression
passes in `target/electrical-sources-flight-regression-01.log`. Architecture and
generated documentation checks pass. Next is explicit source-to-bus distribution
and terminal-current allocation, then engine/regulator providers and live OAT/V.

### Previous authored electrical circuit stage

After `59e362f`, authored legacy electrical data is loaded with flight parameters
and a circuit-observation runtime is owned by Simulation. The checked C172 CFG
contains 17 consumer declarations but omits battery voltage, generator voltage
and generator current capability. They remain None rather than borrowing another
aircraft's example voltage or treating an absent value as an authored zero.
The source meanings/bus codes were cross-checked against the published legacy
SDK references linked in FORMATS.md. Unlisted circuit defaults are not expanded.

The new vehicle parser reads documented bus/amps/minimum-voltage triples, optional
source scalars, the availability flag and engine-generator counts. It rejects
malformed/nonfinite/out-of-profile fields and retains unsupported keys. Explicit
zero scalars stay Some(0). Visual-only aircraft loading remains independent.
The simulation circuit runtime starts with unknown bus observations and device
requests. It distinguishes off/deenergized/below-minimum/unknown state, sums only
requested authored maxima by bus, clears unavailable observations without stale
values and rejects invalid writes atomically. A known 0 V is distinct from None.
No source-generation, bus-tie, battery-charge, current-limit/droop or always-power
bypass algorithm is invented. The latter explicitly fails circuit evaluation.

`legacy-electrical-check` loads the installed C172, checks its 17 authored
circuits and absent source parameters, then drives explicit inspection bus inputs
through Simulation. At 17 V the requested pitot/nav-light maxima sum to 7 A;
below threshold they contribute zero. Losing an observation returns unknown,
invalid input leaves state unchanged, physics steps do not fabricate a supply,
and a new simulation starts without old observations. These sums describe only
the declared requested consumers, not total aircraft current or actual delivered
power; unmodeled systems are not automatically switched on.

177 all-feature Rust test executions pass. Installed evidence is in
`target/legacy-electrical-check-02/`; the existing native clock/picking/button/
GPU/121-frame flight/pause/reset/view/ground regression passes under
`target/legacy-electrical-flight-regression-01/`. The default native flight stream
still has only the main clock string; no live bus voltage is claimed.

Next work is source/default resolution and battery/generator state, physical bus
distribution and actual consumer requests, followed by live OAT/V input binding.
Electrical generation/dynamics, omitted circuit defaults, source-driven voltage,
actual-window mouse capture and the full FSX 1:1 objective remain unfinished.
Original-FSX acquisition remains deferred by the user.

### Previous completed native OAT/V and CP1252 stage

After `5ceed9e`, the secondary C172 Clock element is identified and exercised:
tokens 341/335/753 are AMBIENT_TEMP_DEGREES_C, DISPLAY_UNITS and MAIN_BUS_VOLTAGE
in the installed public SDK. The element has a seven-byte maximum, a 33x9
regular Quartz descriptor and its own original callback. Native connect supplies
the real private state. Six explicit OAT cases prove English/metric display and
native formatting/clamping; three voltage cases use the original third mouse
handler and its own packet 2, delivered through the existing checked local event
bridge. No private state or event packet is synthesized.

A missing encoding path was exposed: the original temperature strings contain
ANSI byte 0xb0. The helper now explicitly decodes printable Windows-1252 through
the Windows code-page API and emits JSON Unicode escapes on the ASCII wire.
Undefined/control bytes still fail; ordinary ASCII profiles remain strict.
Native rasterization counts decoded ANSI characters rather than UTF-8 bytes and
uses character-safe selection boundaries. Only the native font-paint path gains
printable CP1252; XML text remains ASCII. Missing font glyphs still fail. The
build gate adds ansi_text_verified with degree/euro, undefined/control rejection
and guarded ANSI-buffer checks.

`native-string-check --oat` records nine guarded original callbacks under
`target/native-oat-check-02/`: 59 F, -40 C, 15 C, 100 F, native endpoint clamps
-99/999 and 28.0/0.0/99.9 V. Temperature strings retain their degree glyph.
Voltage records additionally require the original packet, native context and
import restoration. `native-string-render-check --oat` paints all nine captured
strings through authored placement/Quartz into cockpit GPU pixels and restores
the first state identically (`target/native-oat-render-02/`). The degree-bearing
render was visually inspected. All sensor values in these probes are explicit
inspection inputs, not simulated electrical measurements.

172 all-feature Rust test executions pass. Eight invalid OAT requests are
rejected without a sample (`target/native-oat-negative-01.json`). Existing main
clock and Nav-Comm probes, the SEQ_REC selection/GPU regression and the complete
flight/picking/button/pause/reset/ground/clock-event checker pass under the
native-oat-* evidence directories. No installed font/DLL/asset is committed.

Next work is wiring secondary environmental inputs into live flight with honest
variable availability: ISA temperature already exists, but MAIN_BUS_VOLTAGE
requires electrical state (or an explicitly declared inspection input), not an
invented normal voltage. The default flight stream still emits/renders only the
main clock string. Actual-window input capture, further gauge services, native
color/lifecycle behavior and original-FSX parity remain unfinished. The broad
1:1 objective stays intact; original-game acquisition remains deferred.

### Previous completed screen picking stage

After `53f9d2d`, left-button press in the native-clock cockpit is routed through
current screen/geometry picking to the original native clock mouse handler.
The renderer's new CPU picker uses its actual camera and model matrices,
Direct3D near-inclusive/far-exclusive depth, a model-space near/far ray and
current visible posed triangles. It intersects two-sided geometry, interpolates
UVs in model space, applies repeat/clamp and linear single-mip alpha sampling,
and follows the current shader's alpha-test rules. The nearest surviving surface
blocks input, including nonpanel geometry and nonzero-alpha blended foregrounds.
No fixed screen rectangles or arbitrary native callback addresses are used.

FlightGauges resolves the picked model texture against its actual VC panel,
converts UV through preview dimensions and existing CFG/native bitmap mapping,
then requires one authored left-single clock child. Child-local coordinates are
floored for the x86 POINT ABI. The helper rechecks the actual mouse record/point.
Viewer routing requires flight/cockpit/visible-window/known-cursor context; only
left press sends the callback. Release/right/wheel do not enter this native path,
CursorLeft clears the point, and focus loss retains existing flight pause/key
cleanup. An accepted click updates the native clock and GPU texture at the same
simulation time, including physics pause. Exterior/nonflight orbit behavior is
retained. The helper ABI/protocol is unchanged by this stage.

170 all-feature Rust test executions pass. Picker tests cover perspective UVs,
translation, viewport scaling, both windings, near/far clipping, nearest/hidden/
alpha-discard/transparent occlusion, alpha addressing and invalid ranges/values.
`target/native-screen-picking-flight-04/` drives the entire screen -> triangle ->
UV -> panel -> native child -> original mouse/event callbacks -> GPU path for all
three clock buttons. Their actual child-local centers resolve to [3,3]. It also
checks 2x physical viewport scale, a different aspect, a rolled/translated camera,
a hidden real cockpit batch, a nonactionable display/help point and an outside
viewport miss. Mode/start/progress/stop/reset/restart still preserve helper PID,
physics counters and expected changed/equal pixels; the existing 121-frame flight,
view/ground, clock edit/day-boundary and selection regressions pass. The rolled
cockpit render was visually inspected. Viewer compilation succeeds. The checker
calls the production screen handler path without injecting OS mouse input; an
actual window-event/manual interaction capture remains to be done.

Next work is actual-window input validation, secondary clock text/state and
broader native control/lifecycle coverage. Only the three C172 left-single clock
handlers are routed: cursor feedback/tooltips, drag/repeat/double-click/wheel,
generic gauge actions, complex transparent overlay parity and original-FSX
input/pixel parity remain unfinished. Running-stopwatch midnight/time edits and
the broad FSX 1:1 objective still need work. Original-game acquisition remains
deferred by the user.

### Previous completed flight clock button integration

After `4e10fcf`, the real flight gauge stream now hosts the narrow PANELS bridge
and original clock button state in the same x86 process as the five air-data
needles and three gyro sprites. Clock-enabled flight uses schema 4; ordinary
schemas 1/2 and the isolated clock diagnostic stream remain unchanged. Local
seconds carry a fractional phase into native CLOCK_TOTAL_SECONDS lookup while
HMS display sources retain their established whole-second boundaries.

clock-click accepts an actual authored mouse RVA and child-local coordinates,
validates the target in the bounded clock list, calls the original mouse handler
and delivers its original guarded packet. The acknowledgement preserves physics
sequence/time/evaluation count. The Rust parent verifies those fields, input
coordinates, native acceptance/guards and its independently tracked delivered
event count. Before any mouse event, exact default HH:MM:SS output/result remains
mandatory; after events, bounded native clock strings/results are accepted with
strict metadata/counter/guard checks, without predicting private gauge state.
Pause reuses physics and marks the changed display dirty after a button call.
Native disconnect/string/import/context restoration and event count are required
at close; reset/rewind restarts the complete helper and clears native button state.

A fractional-clock boundary issue was found by the existing flight regression:
adding a fraction just below 1 could round 23:59:59 upward to invalid 86400. The
simulation clock now caps a fractional value below the next representable whole
second, preserving seconds_at boundaries and edited-clock phase. It does not
change the host wall clock, UTC or calendar.

FlightGauges exposes checked authored child-center calls. Startup CLI
--native-clock-click 0/1/2 applies after CLOCK_* edits and before explicit
selection; repeated button calls retain native state. This is inspection input,
not a substitute FSX keyboard binding or screen mouse picker. A viewer snapshot
with mode/start injection succeeds (`target/native-flight-clock-cli-02.ppm`).

166 all-feature Rust test executions pass. The integrated checker in
`target/native-flight-clock-buttons-03/` covers all three original handlers,
start, 1.25/61.25-second progress, paused input, stop/freeze/reset/restart,
unchanged helper PID during button use, invalid child/stale-time rejection,
unchanged physics evaluation counts, GPU dirty updates/pixel equality and full
helper reset. Its original 121-frame flight/pause/view/ground, day-boundary,
CLOCK_* edit and selection checks still pass. Twelve invalid flight-clock protocol
requests are rejected without a response transition
(`target/native-flight-clock-negative-01.json`). The rendered integrated
stopwatch was visually inspected. No original code/assets are added.

Next work is screen-ray/texture picking and child-local mouse routing in the
cockpit, plus secondary clock text and further runtime service coverage. Running
stopwatch behavior across midnight/simulation time edits still needs evidence.
Full PANELS/gauge lifecycle, original-FSX input/pixel parity and the broad 1:1
objective remain unfinished; original-game acquisition remains deferred.

### Previous completed retained isolated clock stage

After `671a5b3`, --clock-local-frames retains one connected original C172 Clock,
its guarded main string/context, checked mouse metadata and narrow PANELS bridge
across time/click commands. A bounded explicit local-seconds input now preserves
fractional seconds for CLOCK_TOTAL_SECONDS lookup; the displayed HMS source
slots use whole seconds. Original mouse records/callbacks and their own packets
remain the source of all mode, format and stopwatch state transitions. There is
no replacement private timer algorithm.

The local stream uses exact sequence numbers (1..4096), explicit seconds in
[0,86400), and metadata child indices 0..2. Each click uses the authored child's
center and one left-single activity. Packet capture is armed only for that
mouse call; unsolicited, duplicate, wrong-header/size and recursive traffic
fails. Repeated events reset packet bookkeeping without replacing native gauge
state. Reset disconnects/deinitializes/unloads the old module, restores the
import pointer and loads/connects a fresh original module in the same helper
process. Context/string buffers outlive teardown. State packets include dirty/time-unchanged
flags, actual string/result, lookup/event counters and actual child/element RVAs.

`native-clock-session-check` drives 30 commands in one verified PID with four
module generations (`target/native-clock-session-check-03/`). Native behavior
covers mode change, start, 1.25/61.25 seconds of stopwatch progress, identical-time
pause, stop/freeze, reset, repeat start, 24/12-hour format and native module reset.
The running stopwatch returns 6125.6 and displays " 0:01:01" at 61.25 seconds;
stopping preserves both as explicit local time advances. These are observations
of the original DLL under host-supplied time, not FSX flight captures. Secondary
clock text and running-stopwatch day wrap/time edits remain unverified.

`native-string-render-check --local-session` renders all 30 captured states plus
a restored baseline through installed Quartz, actual panel placement and GPU
(`target/native-clock-session-render-01/`). Mode/progress/format changes affect
190/45/56 cockpit pixels respectively, confined to the clock display. Paused,
stopped and reset pairs match exactly, including native reinitialization. The
rendered stopwatch image was visually inspected. This is recorded-state render
evidence; live viewer mouse input/invalidation is not connected yet.

163 all-feature Rust test executions pass, along with the guarded helper ABI/
command self-test. Eighteen malformed/invalid stream commands are rejected
without emitting a state (`target/native-clock-session-negative-01.json`). The
previous one-shot local-event probe still completes all three original callbacks.
The existing 121-frame flight/clock-event/selection/pause/reset/view regression
passes under `target/native-clock-session-flight-regression-01/`. No original
assets or routine implementations are committed.

Next work is integrating persistent native button state and fractional clock
lookup into the flight gauge bridge/renderer, then screen mouse routing. Broad
PANELS services, secondary text, multiplayer, full gauge lifecycle, original-FSX
pixel/input parity and the wider 1:1 objective remain unfinished. Original-FSX
acquisition remains deferred by the user.

### Previous completed isolated local event bridge

After `4ec6d7c`, an opt-in isolated C172 local event bridge binds the checked
exported PANELS import entry. It resolves ImportTable by export, requires the
PANELS ID 15 plus a null terminator and an unbound pointer, and installs a
read-only independent table with only shared-event capture, CLOCK_TOTAL_SECONDS
lookup and a local-master query. All remaining function slots stay unavailable.
The bridge remains installed through original ModuleDeinit and restores the
import pointer before releasing its table. It is not enabled in the viewer.

The original event-size callback must write exactly four bytes into a guarded
word. The original mouse callback creates its own packet; the bridge copies it
while the caller's stack is alive, then delivers a guarded copy once to the
original process-event callback after mouse return. No private gauge object or
native packet is fabricated. Header/length/count/recursion, executable pointers,
packet immutability, native acceptance and cleanup are checked. Event words end
at NOACCESS pages and have checked prefixes. The installed DLL uses stdcall for
both event callbacks (ret 8), even though the public typedef omits FSAPI; the
process-event result and PANELS shared/master results are C++ bool. A repeated
synthetic ABI/true/false/overflow/underrun check gates helper builds.

`native-mouse-callback-check --local-events` passes all three original callbacks
with no native exceptions in `target/native-local-event-check-02/`. Their actual
packets are 1, 0, 2 in metadata child order. The first changes the main string
from 12:34:56 to " 0:00:00" (result 0.6); the second preserves the visible time
with result 45296.1 and one real token-557 lookup; the third preserves the main
string/result. Lookup supplies explicit local seconds since midnight (45296),
not an invented all-zero variable service. The secondary clock string and its
state are not rendered by this probe. Local delivery after callback return is
an explicit host profile, not verified original-FSX shared-cockpit behavior.

161 all-feature Rust test executions pass, including strict local delivery
protocol checks. The guarded native helper self-test and existing 121-frame
flight/pause/reset/view/clock-event/selection regression pass under
`target/native-local-event-flight-regression-01/`. Baseline --sample-clock-mouse
still omits services and reports contained faults, preserving the diagnostic
comparison. No original code or installed asset is committed.

Next work is persistent local button/event delivery with advancing clock tokens,
stopwatch/pause/reset checks and renderer invalidation, before screen input
routing. Multiplayer, broad PANELS services, secondary text and full gauge
lifecycle remain unsupported. Original-FSX acquisition remains deferred; the
full 1:1 objective is unfinished.

### Previous completed guarded mouse diagnostic stage

After `57eed45`, the isolated x86 helper can probe a checked C172 Clock mouse
callback with the independently declared two-argument stdcall/32-bit BOOL ABI.
The 28-byte MOUSECALLBACK context carries child-local coordinates, the actual
gauge header and mouse-record pointers, explicit synthetic screen [0,0] and a
null reserved field. Its body ends at a NOACCESS page; prefix guards detect
underruns. Context and guarded string buffers outlive native module teardown.
The synthetic self-test verifies argument fields/flags/full BOOL return,
contains a deliberate overflow with exception address/access diagnostics and
detects prefix damage. The helper build gate requires mouse_context_verified.

`--sample-clock-mouse` accepts only the three encoded left-single C172 Clock
children, in-bounds integer coordinates and explicit local seconds. It validates
the complete bounded mouse list/executable callback, initializes/connects the
original clock, records its baseline string, invokes the native mouse callback,
and only updates the string afterward if that call completed. Both success and
contained faults require original disconnect, pointer/header restoration and
ModuleDeinit before a diagnostic record is emitted. Exit 2 denotes a contained
native fault, not successful button execution.

`native-mouse-callback-check` resolves children/callbacks from metadata, bounds
process I/O/time, validates context/cleanup and explicitly distinguishes completed
calls from contained faults. Three observations under
`target/native-mouse-callback-check-02/` all contain 0xC0000005 at native RVAs
0x226e / 0x237e / 0x23be, reading address 0x150. Local callback inspection and the
published PANELS layout identify a null table access to process_shared_event_out
(offset 0x150); the original clock also exposes event-size/process-event callbacks
at RVAs 0x1fd0 / 0x1ff0. Original disassembly stays ignored; no original routine
implementation is copied. This establishes a missing service dependency, not
working clock buttons. No service table or private gauge object was fabricated.

160 all-feature Rust test executions pass. Eight invalid mouse probe requests
are rejected (`target/native-mouse-callback-negative-01.json`), the enhanced x86
self-test passes, and the existing connected clock/event flight checks retain
pause/reset/view/ground/helper-close and edited-clock pixel invariants
(`target/native-mouse-callback-flight-regression-01/`).

Next work is a checked PANELS import/service bridge and original shared-event
callback delivery, including the required variable lookup semantics, followed by
screen input routing. Shared-cockpit/local delivery semantics still need careful
validation. Original-FSX acquisition is deferred; the full 1:1 goal stays active.

### Previous completed mouse metadata/coordinate stage

After `affcc16`, read-only native mouse metadata is available through an explicit
GaugeLibrary::mouse_rects lookup. Ordinary gauge parsing retains its 52-byte
header contract; mouse lookup separately requires the 84-byte prefix and reads
the mouse-list pointer at offset 80. The independently decoded x86 MOUSERECT
record is 44 bytes, with parent/child/end-parent/tooltip kinds, relative geometry,
cursor/help IDs, activity flags, event/callback identity and opaque API data.
Tooltip function slots are data/text addresses, never executable callbacks.
Original function-slot bits are retained for every kind. No argument or runtime
pointer graph is followed.

Lists are file-span checked, limited to 2048 records/32 nested parents and must
terminate with balanced parents. Unknown kinds, orphan children/end markers,
invalid geometry, bad callback/text RVAs and unbounded lists fail explicitly.
Action regions use gauge-relative parent positions, parent-local child positions
and ancestor/gauge clipping. Candidate hit tests retain authored overlap order,
filter activity flags and exclude right/bottom edges. Runtime-installed zero-size
subregions fail explicitly. No overlap delivery, drag/repeat/wheel routing,
callback execution or screen-ray picking is implied.

`native-mouse-check` decoded all 88 gauge-header entries (including aliases) in
the five checked libraries: 208 parents, 208 matching end markers, 205 children
and 38 tooltip records. The installed C172 clock has a 54x54 root bitmap and
three callback regions. The renderer maps VC texture pixels through actual
panel.cfg placement/extents into these bitmap coordinates. All three centers
resolve uniquely with child-local [3,3] positions, while right-clicks, exclusive
edges, the display/help region and points outside the texture produce no false
action hits (`target/native-mouse-check-02/`).

158 all-feature Rust test executions pass, including overloaded tooltip slots,
list structure, truncation, bounds/nesting/record budgets, clipping, overlapping
candidate order and activity filtering. Existing connected clock/event flight
regression passes (`target/native-mouse-flight-regression-01/`). This stage establishes passive metadata and texture-space
hit candidates; native installation/callback ABI and actual screen input remain
next. Original-FSX mouse behavior/pixel parity is unverified, acquisition remains
deferred, and the complete 1:1 goal remains active.

### Previous completed local-clock event stage

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
