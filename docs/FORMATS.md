# Format notes

## Evidence

This independent loader uses observed local data and the research published by FSDeveloper contributors: [FSX MDL format](https://www.fsdeveloper.com/wiki/index.php/MDL_file_format_%28FSX%29). Their incomplete research is a starting point for experiments, not a full specification.

No third-party importer implementation was copied. Tests create small original triangle files; original game assets stay in the local installation.

## Visual model and eye-point references

The selected fltsim.N model suffix chooses model/model.suffix; its model.cfg
provides [models] normal and optional interior. Both references are restricted
to file-name components and receive .mdl when no extension is present. No
embedded directory or absolute reference is accepted by this subset.

An optional [views] eyepoint is exactly three finite feet values relative to
reference_datum_position: longitudinal, lateral and vertical. The visual reader
uses a zero datum if omitted; the full dynamics loader still requires its mass
configuration. The sum converts to body forward/right/down SI coordinates, with
vertical sign inversion. Eye components outside 1 km are rejected. Missing eye
metadata leaves exterior viewing available. The installed C172 resolves to
[-0.10668,-0.25908,-0.57912] body metres, or
[-0.25908,0.57912,-0.10668] model-space metres before the aircraft/world transform.
Camera-definition overlays and user camera profiles are not yet interpreted.

## MDLX subset

MDLX model/view coordinates follow the left-handed Direct3D convention, with
Y up and forward depth positive. The [modeling SDK coordinate-system notes](https://www.prepar3d.com/SDK/Environment%20Kit/Modeling%20SDK/Modeling%20File%20Formats.html)
describe conversion from right-handed authoring coordinates. The projection
uses target minus eye for forward and maps near/far to depth 0/1. The earlier
right-handed camera mirrored geometry, cockpit text/control layout and the
pilot's bank direction. This is corrected in the shared renderer; physics axes
and simulation state are not changed. Synthetic projection tests verify that
right remains screen-right, up remains up, depth is bounded, and a right-bank
camera moves an eastern horizon point upward. Published GPU captures were
rerendered; the older native menu-window image is labeled historical.

Accepted files begin with RIFF, a little-endian payload length and the MDLX type. Nested chunks have a four-byte tag and a little-endian length. Observed MDLX chunks are packed without generic RIFF odd-byte padding; the C172's odd-length VISL chunk confirms this distinction.

The reader checks parent bounds, record sizes, references, local triangle indices and finite geometry values. Files are limited to 256 MiB. Unknown chunks are skipped within validated bounds.

| Chunk | Interpretation |
| --- | --- |
| MDLN | Friendly name |
| TEXT | 64-byte texture name records |
| MATE | 120-byte records; diffuse RGBA/map index, basic blend, alpha test and clamp mode |
| INDE | Little-endian u16 indices local to a part's vertex range |
| VERT | 32-byte records; position, normal and UV; face normals recomputed for rendering |
| SKIN | Buffer reference marking unsupported skinned parts |
| TRAN | 16 f32 static transform values |
| AMAP | Node type and TRAN reference |
| SCEN | Child, peer and AMAP byte offset; cycles rejected |
| LODE | Numeric LOD followed by PART chunks |
| PART | 36-byte triangle-list part and validated buffer ranges |

DirectX row-vector matrices are interpreted as transposed column-vector matrices with the same scalar storage. Child transforms compose with parents. Face normals are recomputed after transforming triangles, avoiding incorrect normal transforms under nonuniform scale.

Animated nodes use static matrices by default; explicit manual clip selection evaluates rotation/translation keys. Skinned parts are omitted and counted. Triangle fans/strips, unreachable scene nodes and unsupported format generations are rejected explicitly.

## DDS and texture lookup

The legacy DDS header and BC1/BC2/BC3 definitions follow Microsoft's [DDS guide](https://learn.microsoft.com/en-us/windows/win32/direct3ddds/dx-graphics-dds-pguide) and [block compression reference](https://learn.microsoft.com/en-us/windows/win32/direct3d10/d3d10-graphics-programming-guide-resources-block-compression). DXT1 selects its endpoint-order-dependent three/four-color palette. DXT3 always uses the four-color palette and explicit four-bit alpha. DXT5 uses the two endpoint-dependent alpha palettes and three-bit alpha selectors. Partial edge blocks are clipped to the declared dimensions.

Uncompressed 32-bit RGB uses validated nonoverlapping contiguous channel masks and respects row pitch. Only the top mip is decoded. DDS rows and MDLX UVs are preserved without a vertical flip; this mapping was checked against the local C172 exterior. GPU diffuse textures use an sRGB format. This does not establish full FSX lighting or material parity.

texture.cfg is read only for fltsim fallback entries, in numeric order. Relative Windows separators are normalized and canonical directory paths prevent cycles. Limits apply to file sizes, decoded pixels and directory traversal. No full content filesystem or external SimObjects resolver is claimed.

## Next work

Tested bind-pose skinning, simulation-driven animation and visibility evaluation, material parity and generated registration textures. Back each interpretation with synthetic fixtures and local observations; do not silently reinterpret unknown layouts.

## AIR subset

Legacy AIR files are sequences of a little-endian u32 identifier and u32 payload length followed by packed bytes. Record labels such as 1101 and 404 are hexadecimal: their numeric IDs are 0x1101 and 0x0404. Bounds, duplicate IDs and record counts are checked; input is limited to 4 MiB.

Curve payloads contain a u32 point count followed by pairs of f64 angle/value entries. Angles must be finite and strictly increasing. The current reader uses linear interpolation and clamps outside the endpoint domain. It loads lift versus body AoA from 0x0404 and pitch moment versus AoA from 0x0473, plus selected signed integer and double coefficients from the 250-byte 0x1101 layout. Integer aerodynamic derivatives use the documented 1/2048 scale. Modern override records are rejected until their precedence is implemented.

The independent reader was checked against field definitions distributed with AirEd and the local C172 data. Reference: [aircraft dynamics resources](https://www.aero.sors.fr/fsairfile.html). Source definitions and game data are not included. The [flight guide](FLIGHT.md) documents coordinate signs, units, approximations and numerical evidence; decoding these records does not establish complete FSX flight-model behavior.

## MDLX animation-key subset

ANIB in the observed C172 contains an inner packed RIFF with XANH and XANL,
followed by zero padding. XANL holds XANI groups: GUID bytes, float length,
a 16-byte type field and a terminated parameter string, then packed children.
XAPS contains terminated variable/units/scale/bias/optional-lag fields. XAPC
contains a terminated expression with an optional #lag# prefix. A bounded,
numeric RPN subset evaluates supported A-variable expressions into authored
frame positions; results clamp to the clip length. Static inspection stays
read-only. Temporal animation can additionally read/write caller-owned L: data
and use s0..s49, sp0..sp49 and l0..l49 scratch registers. Unknown syntax, missing
values and invalid results retain diagnostics. The temporal PoseState API
limits movement to Lag keyframes per simulation second for both XAPS and XAPC.
Its first available target initializes immediately; unavailable targets hold
the previous known pose and retain the failure diagnostic. Recreate state on
model changes or simulation reset. This initialization/missing-value policy is
explicit and has not been compared with FSX. Static plan_pose inspection still
defers positive lag because one sample has no time history.
XAPC source is limited to 16 KiB, each program to 4096 instructions
and the ANIB total to 262144 compiled instructions. Duplicate XAPS/XAPC drivers
are rejected. XANS begins with channel type, transform ID and float
track length. Observed channels 0 and 2 contain XANK keys with a one-byte kind,
float frame and respectively XYZW quaternion (kind 4) or XYZ translation
(kind 3). SGAL is one signed 16-bit transform ID per scene node, with -1 for
no linkage. Multiple groups can reference the same transform ID.

The reader validates parent lengths, zero padding, terminated strings, finite
values, increasing key times within track length, nonzero quaternions,
duplicate channels and SGAL references. Limits apply to clips, tracks and
keys. Other channel layouts fail explicitly. Keys clamp at their endpoints;
interior translation is linear and rotation uses normalized shortest-arc slerp.
The pose API composes selected clip deltas in file order after static local
matrices, then traverses parents before children. Arbitrary multi-clip
composition has not been compared with FSX.

The binary interpretation was checked against local bytes and original
synthetic fixtures. The [Prepar3D modeling SDK](https://www.prepar3d.com/SDKv3/LearningCenter/modeling/using_modeling_tools.html)
explains variable scaling/bias and lag semantics; reading those fields does
not establish an implemented SimVar or gauge system. See the
[viewer guide](VIEWER.md) for supported inspection and its evidence.

Model-local contexts are limited to 512 named finite numeric values. Names and
unit strings are case-insensitive ASCII, bounded to 256 and 64 characters.
First successful references initialize to zero. A variable retains its first
unit string; mismatched-unit access is unavailable rather than silently
converting. Registers start uninitialized on each evaluation; reads before a
store fail explicitly. Stores sN retain the stack value; spN and >L: consume it.
Exactly one finite result is required. Failed expressions, invalid animation
targets and failed temporal steps do not commit local writes.

The [SDK local-variable description](https://prepar3d.com/SDKv6/sdk/references/variables/local_variables.html)
describes shared L: scope. The current context is shared only between a visual
instance's Sim clips with tracks, in file order, and is cleared on simulation
reset. Shared XML-gauge/panel scope, cross-model lifetime and full unit conversion
remain unimplemented. The [Microsoft RPN operator reference](https://docs.flightsimulator.com/html/Additional_Information/Reverse_Polish_Notation.htm)
documents register stack behavior; this is not a complete FSX execution contract.
No A: writes, events, native calls, gauge DLLs, loops or strings are supported.

## Panel placement subset

`fsx-vehicle::panel` reads the selected variant's panel.cfg without loading gauge
code. Aircraft definitions select panel/panel.cfg or panel.SUFFIX/panel.cfg;
suffixes must be filename components. The `inspect-panel` command selects variant
0, reports all configured panels and checks each dynamic texture in the native
interior against the virtual-cockpit configuration.

WindowNN and VCockpitNN sections sort numerically. Gauge references retain the
module and instrument names, X/Y panel coordinates, optional positive width and
height, and the remaining parameter string including commas. Omitted dimensions
remain unresolved until the gauge's own intrinsic size is available. The loader
visits indices below 100 in numeric order and then contiguous indices from 100;
later entries after a gap are reported separately. This follows the documented
ordering in the [panel configuration reference](https://prepar3d.com/SDKv3/LearningCenter/simobjects/panels_and_gauges/panel_configuration_files.html),
but native FSX ordering at gaps has not been measured.

Panel coordinate extents, explicit pixel extents and RGB background colors are
read separately; no automatic scaling or aspect-ratio resolution is applied.
Virtual-cockpit texture names start with $, are at most 15 bytes and match the
model case-insensitively. Duplicate numeric panel/gauge aliases and duplicate VC
texture names fail explicitly. Limits are 2 MiB of decoded CFG text, 256 panels,
4096 loaded gauges, indices below 4096 and finite coordinates within +/-65536.
Other section/directive names are retained as unsupported diagnostics.

Background bitmaps, window positioning/visibility, panel aliases, render-to-texture
views, XML/CAB gauges, native DLL/GAU callbacks and panel interaction are not yet
implemented. The parser does not execute or extract installed gauge code. Tests
use original synthetic declarations; installed panel configurations remain local.

## Windows bitmap subset

`Image::dib` reads a packed native RT_BITMAP payload; `Image::bmp` additionally
reads the 14-byte file header and explicit pixel offset. Supported data uses a
40-byte BITMAPINFOHEADER, one plane, BI_RGB and 4/8/24 bits per pixel. Indexed
images use BGR-reserved palette entries, with biClrUsed or the full bit-depth
palette when zero. Reserved bytes remain opaque rather than becoming alpha.
Four-bit pixels use the high nibble first; unused nibbles and DWORD row padding
are skipped. Positive heights read bottom-up, negative heights top-down.

Dimensions are bounded to 4096, files to 256 MiB and palettes to 256 entries.
Missing palette indices, overlapping BMP offsets, truncated rows and invalid
declared extents fail. BMP file trailers outside bfSize are ignored. BI_RGB
biSizeImage may be zero; otherwise it must cover the required rows and fit in
the payload. The separate `Image::bmp_alpha` mode requires 32-bit BI_RGB file
pixels and preserves their explicit BGRA alpha, including fully transparent
nonblack pixels. It is selected only by the gauge's Alpha property; reserved
palette bytes and ordinary DIB high bytes are not guessed to be opacity.
RLE, bitfields, 1/16-bit pixels, extended headers, native DIB alpha and color
profiles remain unsupported. The [Windows bitmap reference](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-bitmapinfoheader)
documents these structures; the implementation uses no GDI calls.

`fsx-render::panel::static_roots` is an explicit artwork-inspection compositor.
It requires VC panel and gauge dimensions, exact resource language, zero-origin
static roots and supported flags. It maps CFG rectangles into panel pixels,
clips without stretching, samples source pixels by nearest neighbor and applies
RGB-black transparency when root image flag bit 0 is set. Mask, alpha, hidden,
no-stretch and no-translation roots are diagnosed instead of guessed. Other
elements are counted but omitted. Native update callbacks, conditional visibility,
day/night/luminous blending and FSX sampling/placement parity are unimplemented.
The result can be bound to one unambiguous model texture slot before GPU upload;
dimension, RGBA length and total appearance memory budgets are checked.

## Native gauge metadata subset

`fsx-vehicle::gauge` treats a native DLL/GAU file as bytes. It resolves the PE32
Linkage export and preferred-image pointers to file-backed data; it never maps
the library into the process, applies relocations, resolves imports or calls code.
Only x86 PE32 images and the shared header prefix for gauge versions 800, 900 and
1000 are supported. Other versions and PE32+ fail explicitly.

Each header references one root-pointer variable. An element's next-element
field references a null-terminated child-pointer array. The reader preserves
authored child order and records type, resource ID, position and image flags;
cycles fail, while shared elements are indexed once per gauge. Repeated Linkage
entries pointing to the same header are retained. Name lookup is ASCII
case-insensitive and rejects ambiguous names pointing to distinct headers.
Null encoded roots remain empty rather than fabricating runtime-generated data.

PE resources retain type/name/language identity, including UTF-16 named keys and
code-page metadata. Resource access returns the exact indexed file slice without
automatic locale fallback or writing extracted files. Type 2 identifies bitmap
resources, whose payloads are DIB data rather than complete .bmp files. Bitmap
decoding is implemented for the subset above. The separate static root preview
does not implement moving element transforms, callbacks, failure systems, native
variable access or operational gauge drawing.

Limits are 64 MiB per file, 96 non-overlapping sections, 8192 export entries,
512 gauge headers, 2048 distinct elements and 8192 graph edges per gauge, 65536
elements per library, 8192 resource leaves and 32768 resource-directory entries.
Name limits are 256 bytes for gauge/export names and 256 UTF-16 units for resource
keys. Truncated/overlapping ranges, unbacked virtual zero-fill, forwarded Linkage
exports, malformed ordinals, graph cycles and shared/overdeep resource directories
fail explicitly. Windows-1252 gauge-name decoding requires the explicit option.

The [Microsoft PE specification](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format)
documents export/resource structures. The installed FSX header declarations and
the [gauge API reference](https://www.prepar3d.org/SDKv4/sdk/panels_and_user_interface/gauges/references/gauges.html)
describe the gauge metadata layout. These facts do not establish native ABI or
rendering compatibility. Tests construct original non-executable PE containers;
no installed DLL, header, sample source or bitmap is included.

Reproduce the local C172 configuration/library cross-check without running FSX:

```powershell
cargo run -- inspect-gauges "./FSX/Gauges/Cessna172.dll" --windows-1252
cargo run --locked -p fsx-check --example native-panel-check -- "./FSX"
```

The example uses the explicit installation's stock C172 and Gauges folder. It is
not a general add-on module search-order implementation.

## Native needle metadata and inspection transforms

The content reader records each needle's source token, callback RVA, encoded
max-degrees-per-tick field and nonlinearity table. POS in the common element
header is the pivot in the gauge background; OFS is the pivot inside the needle
bitmap. Runtime reference fields are separate and are not substituted for OFS.
Next-list links establish drawing order, not a hierarchy of spatial transforms.

Tables contain 24-byte entries with signed X/Y points and an f64 value; the
runtime degree-cache member is ignored. Limits are 512 points per table, 65536
points per library, coordinates within +/-65536 and finite table values within
1e12 in magnitude. Invalid pointers, inconsistent pointer/count pairs and
unsupported extents fail explicitly. Reading-to-angle inspection requires at
least two strictly increasing or decreasing values, unwraps the authored
clockwise point order across +/-pi, interpolates angles and clamps outside the
table's endpoint values. Angles use screen coordinates (+Y down).

The renderer rotates the original bitmap about OFS, places it about POS and
scales it into the explicit CFG rectangle using the background image extent.
Clipping preserves the transform; nearest-neighbor sampling and RGB-black
keying match the other artwork inspection path. Hidden-tree predecessors and
unsupported masks/alpha/transform flags are rejected. A fresh root preview is
composed for each state; repeated paints are not an erase/update lifecycle.

The [gauge API reference](https://www.prepar3d.com/SDKv6/sdk/panels_and_user_interface/gauges/references/gauges_api_overview.html)
describes needle pivots, next-list order and clockwise nonlinearity points.
The implementation is an inspection subset: table clamping, sampling, temporal
lag, failures and the full FSX panel rasterization contract remain unverified.
Table-free result interpretation must be explicit; it is not inferred for
arbitrary callbacks. The checked example opts into degrees only for the three
Cessna altimeter functions. Other native layers and live token binding remain
unimplemented, and no original artwork or SDK implementation is bundled.

## Isolated native update-call protocol

The separate tools/fsx-gauge-host process can execute a selected installed
needle update function on Windows x86. It is outside the Rust content-reader
and engine dependency graph. Build and check without running FSX:

```powershell
./scripts/build-gauge-host.ps1
./target/fsx-gauge-host.exe --probe ./FSX Cessna172.dll Airspeed
./target/fsx-gauge-host.exe --sample ./FSX Cessna172.dll Airspeed 1001 75
./scripts/check-native-gauges.ps1 -FsxRoot ./FSX -Output target/native-gauge-check.ndjson
```

Probe mode loads the selected DLL but invokes no exported module/element
functions. Sample mode calls ModuleInit/ModuleDeinit around one selected needle
update call. It fills current and previous numeric source values with the same
explicit raw input. No simulator token lookup, units or native PANELS services
are supplied. Schema-1 single-line JSON identifies source native-gauge-probe,
mode, pointer_bits, library/gauge, lifecycle calls, needles and optional sample
resource/token/raw_input/callback_result. All numeric results must be finite.

Only the five checked stock library filenames under the explicit installation's
canonical Gauges folder are accepted. Names are bounded ASCII; pointers remain
within the loaded image and are checked for page access. Limits are 64 MiB on
disk, 512 headers, 2048 elements, 8192 edges and raw magnitudes up to 1e12.
Structured exceptions become errors; a five-second watchdog terminates only
the helper. Errors use stderr and a nonzero exit status. No native resources or
SDK headers are copied into the repository.

The [Windows library-loader reference](https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-loadlibraryexw)
describes native loading, and the gauge API documents the four-byte x86 slot
layout and stdcall element callbacks. The helper does not implement a full
query/install/initialize/update/generate/draw/kill sequence, panel callbacks,
shared variables, events or rendering. Its raw return values need the element's
authored mapping before they can drive a renderer. No instrument/FSX parity is
claimed by an isolated successful function call.

## CAB and ACE XML gauge documents

The read-only vehicle layer accepts single-cabinet version 1.3 archives with
stored or MSZIP folders. Compressed input and total decoded folder bytes are
each bounded to 64 MiB; file/folder tables to 8,192 entries. MSZIP resets its
DEFLATE state per block while preserving the preceding folder dictionary.
Declared block sizes, optional checksums, metadata/data ranges and overlapping
folders are checked. Header/folder/data reserve areas are supported. Offsets
are limited to the declared cabinet prefix; appended bytes are not interpreted
or trusted as a certificate. Chained/split cabinets, Quantum and LZX fail explicitly.
Names use normalized relative keys with ambiguous/case-colliding names rejected;
non-UTF-8 names require explicit Windows-1252 unless marked UTF-8 by CAB metadata.
These boundaries follow the [Microsoft cabinet format](https://download.microsoft.com/download/4/D/A/4DA14F27-B4EF-4170-A6E6-5B1EF85B1BAA/%5BMS-CAB%5D.pdf)
and [MSZIP specification](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-mci/27f0a9bf-9567-4e40-ad66-6ae9ab9d2786).

ACE XML parsing is bounded to 1 MiB, 8,192 nodes and depth 64. It retains
attributes, text and unknown nodes, resolves predefined/numeric entities and
requires an AceXML document with one SimGauge.Gauge. DTDs, external entities,
processing instructions and unsupported encodings fail. Image Name references
resolve within the XML member's directory; ArtDirectory never opens a path on
the host. Structural decoding alone does not make visual elements functional.
The typed text-gauge reader separately accepts a static background plus
translated GaugeText, parented Element Visibility, dimensions, optional FontFace, FontHeight, alignment,
transparent text and default/alternate COLORREF colors (0xBBGGRR). Unsupported
visual properties and duplicate scalar properties fail explicitly. A small
named-color subset and hexadecimal values are accepted. Root translation,
rotation, bold/weight, alternate fonts, opaque text boxes and scripted visual
properties are not applied.

The renderer requires an explicit local font file (bounded to 16 MiB), validates
an authored FontFace against the selected full face name, and reports when that
file substitutes for an absent FontFace. It bounds glyph counts/metrics and
uses single-line ASCII, fontdue coverage and pixels-per-em FontHeight. Text clips
to its authored rectangle; composed RGBA gauges scale/clip into panel coordinates
using straight-alpha blending. Fonts and installed images remain external.
Default FSX font selection, complex shaping, GDI height/kerning/rasterization
and night-art selection remain unverified or unsupported. See the
[fontdue metrics reference](https://docs.rs/fontdue/0.9.4/fontdue/struct.Font.html)
and the viewer's explicit-font inspection commands.

```powershell
cargo run -- inspect-cabinet "./FSX/Gauges/n_number_plaque.cab"
# Optional independent byte comparison; expanded files remain under ignored target/.
New-Item -ItemType Directory -Force target/cabinet-reference | Out-Null
expand.exe FSX/Gauges/n_number_plaque.cab '-F:*' target/cabinet-reference
cargo run -p fsx-vehicle --example cabinet-verify -- FSX/Gauges/n_number_plaque.cab target/cabinet-reference
```

Locally the XML and both C172 plaque BMP members (6,353 decoded bytes) matched
Windows Expand exactly. Tests use independent synthetic archives, scripts and
image names, including a second DEFLATE block that requires the first block's
dictionary. No installed XML, bitmap or compressed original member is committed.

The typed background reader also accepts explicit Image Alpha=True, selecting
the separate 32-bit BI_RGB BGRA decoder. Ordinary BMP/DIB decoding never guesses
opacity from a reserved byte. Partial coverage reaches panel composition as
straight alpha. Alpha plus black-key Transparent=True is rejected while its
precedence is unverified. Luminous is not accepted as Alpha: the SDK describes
inverse-alpha illumination with panel [Color] values, which is a separate
lighting contract still unimplemented. See the
[Image property reference](https://www.prepar3d.com/SDKv2/LearningCenter/simobjects/panels_and_gauges/creating_xml_gauges.html).

PanelConfig retains optional [Color] Day, Night and Luminous values as RGB byte
triplets, without inventing defaults for absent keys. Unknown color directives
and the unimplemented application remain diagnostic. Parsing these values does
not dim/tint existing aircraft images and does not imply automatic _night.bmp
selection. Such selection and global illumination need separate evidence.

## GaugeString evaluation subset

The independent script layer compiles literal UTF-8 text, a single read-only
A: string lookup followed by a bounded s format, numeric A:/P: output and conditions followed by
%{if}/%{else}/%{end}, percent escaping, standard newline/tab/return/backslash
escapes and the observed default/alternate color commands. Values are emitted
as colored text spans, not reparsed markup. Numeric lookups receive separate
namespace/name/units; indexed A: names cannot alias P: names. Unknown or
non-finite values fail, and inactive branches do not resolve their variables.

Source is bounded to 16 KiB, 2,048 string parts, nesting 32 and 8 KiB of output.
Numeric subexpressions retain the existing RPN stack/instruction limits. The
ordinary model RPN compiler still rejects P: reads; GaugeString does not enable
model-local writes, events, shared panel variables or host callbacks. General
string arithmetic, loops, case, alternate fonts and other
escape codes remain unsupported. Literal whitespace is preserved; full FSX
indentation/wrapping rules and rendered text metrics remain unverified.
The conditional/format syntax is described in the
[XML gauge SDK reference](https://www.prepar3d.com/SDKv2/LearningCenter/simobjects/panels_and_gauges/creating_xml_gauges.html);
the clr2/default mapping is checked against the installed plaque's expression,
not a live FSX pixel comparison.

Numeric d output supports rounding, width, sign for positive nonzero results,
leading zeros/spaces and left alignment. Negative zero is normalized for d.
Fixed f output uses requested fractional precision, ignoring width/zero padding
as described in the SDK. Width is limited to 256, precision to 16. Integer
conversions are limited to the exact f64 integer range. The s format accepts
width-formatted A: strings and whole numeric values; fractional numeric-to-string
conversion fails. String width counts characters rather than UTF-8 bytes and
never reparses returned data as markup.

Integer halves round away from zero; fixed formatting uses Rust's binary-f64
formatter, including ties-to-even behavior. These edge policies require an FSX
reference. Bare f currently uses six fractional digits, an inference from the
[Microsoft C formatting default](https://learn.microsoft.com/en-us/cpp/c-runtime-library/format-specification-syntax-printf-and-wprintf-functions),
not proof of the gauge runtime's choice. Exponential/hex formats, star widths,
precision on d/s, and floating sign/alignment flags fail explicitly.

```powershell
cargo run -p fsx-app --features viewer --example xml-numeric-render-check -- FSX/Gauges/n_number_plaque.cab n_number_plaque_background.bmp C:/Windows/Fonts/arial.ttf target/xml-numeric-check
```

This command renders independently authored synthetic COM/altitude/VSI/heading
text using an explicitly selected installed background/font. Five states check
powered/hidden/restored text with synthetic electrical/avionics input values.
It does not add
operational radio, engine or flight-variable bindings to the aircraft.

## XML element visibility subset

Element Visibility accepts a plain, bounded numeric RPN source. Parent/child
conditions remain separate nodes, with at most 8,192 nodes and 1 MiB total
condition source in the renderer. Each node is compiled before evaluation;
parents must precede children. Expressions use the shared GaugeExpression A:/P:
read-only resolver, preserving namespace/name/units and model-mode isolation.
Nonzero finite values show the element; zero hides it. A hidden parent hides all
descendants and skips their lookups, while a parent's result is shared across
its text children. Active unresolved values, malformed graphs or non-finite
results return an error. Hidden text emits an empty span list and no glyphs;
its GaugeString syntax is still checked. Authored ordering between a parent's
GaugeText and nested Elements is preserved.

The [Element SDK reference](https://www.prepar3d.com/SDKv2/LearningCenter/simobjects/panels_and_gauges/creating_xml_gauges.html)
defines Visibility as an optional numeric script. This subset does not implement
Lua, macros, shared locals, hidden update events or native lifecycle services.
Synthetic visibility/read-count/pixel tests are not an original-FSX electrical
or hidden-update comparison.

Aircraft/VisualAircraft expose optional selected-variant atc_id (at most 256
bytes, without control characters), preserving explicit emptiness and absence.
This is authored configuration, not a user-edited flight/save-game registration
or a live SimConnect reading. The integration checker below requires an
explicit expected ID and six synthetic time-of-day/light cases; it writes no
aircraft configuration and does not launch FSX.

```powershell
cargo run -p fsx-app --example xml-gauge-check -- FSX/Gauges/n_number_plaque.cab n_number_plaque.xml FSX/SimObjects/Airplanes/C172 0 G-BAFM
```

## Native string element subset

The x86, four-byte-packed ELEMENT_STRING record is 324 bytes. Authored token
slots begin at 60/88/116, source callback pointers at 168/172/176, image extent
at 192/196, COLORREFs at 208/212/216 and a 32-byte face name at 248. Weight,
charset and DrawText flags occupy 280/284/288, maximum string length is u16 at
292, selection-sequence pointer is at 300, font height at 308 and update callback
at 312. Callbacks and selection-table RVAs are bounded within the PE image.
SEQ_REC entries contain three signed 32-bit values: state, first character and
last inclusive character. A zero/zero/zero record terminates the list. At most
256 entries per sequence and 65,536 per library are accepted. Negative/duplicate
states, reversed/out-of-maximum ranges, invalid/truncated spans and malformed
terminators fail explicitly. Face names require a terminator and
explicit encoding. Zero font height remains authored automatic sizing.

The three MODULE_VAR values are unions. The separate native string probe
requires f64 or u32 payload interpretation for every slot; it does not resolve
token types, BCD units, source callbacks or live simulator state. Its guarded
Strict ASCII profiles plus an explicit printable Windows-1252 secondary-clock
profile and lifecycle checks are described in the helper README.
Successful text samples are not native font/GDI or instrument-parity validation.
The optional headless C172 clock profile calls the original GAUGE_CALLBACK
(header, service_id, extra_data) using the x86 stdcall ABI. Header callback and
user_data occupy offsets 84/88; the mutable prefix is checked through byte 92.
CONNECT_TO_WINDOW (14) and DISCONNECT (15) receive zero extra_data in this
specific inspected handler, which does not require a real panel window. The
original callback owns its private allocation; the helper only restores the
public user_data slot after disconnect. This does not provide the PANELS table,
the install/update/draw routines or general window-connection semantics.
The [published gauge header](https://prepar3d.com/SDKv3/LearningCenter/simobjects/sample_code/gauges.html)
and [C-gauge API reference](https://www.prepar3d.com/SDKv2/LearningCenter/simobjects/panels_and_gauges/programming_c_gauges.html)
describe these structures and callbacks. Implementation/tests use independent
declarations; no installed header, DLL, font or native sample source is committed.

### Native string rasterization subset

`native_text::rasterize_unselected` accepts an explicit matching local regular
font, automatic height, ANSI/default charset and bounded printable single-line
Windows-1252 text. Decoded characters, not UTF-8 bytes, count toward NUMCHARS.
It maps authored foreground/background COLORREFs and horizontal/vertical text
alignment, clips glyph coverage, and leaves transparent backgrounds clear.
The callback result is literal text, never parsed as GaugeString or RPN.
The [published gauge API](https://www.prepar3d.com/SDKv6/sdk/panels_and_user_interface/gauges/references/gauges_api_overview.html)
defines automatic sizing by fitting NUMCHARS capital Ws into SIZE_X; fontdue
advance/kerning metrics implement that width rule. GDI cell metrics, hinting
and FSX pixels remain unverified. Nonzero fixed font height and unsupported
font/effect flags are rejected. `rasterize_selected` additionally resolves an
explicit global selection state through SEQ_REC and applies HILITECOLOR to the
matching glyph range, retaining layout and alpha coverage. The same published
API defines inclusive character indices and a highlight text color. Ranges
clip to the current decoded ANSI character sequence; state zero or an unrelated state selects
nothing. `rasterize_unselected` preserves the default path. Neither API dispatches
selection/edit events or evaluates runtime user colors/lighting transitions.

The panel compositor selects strings by RVA, requires a zero-origin static
root and explicit gauge/panel extents, and scales the string position/size by
the root bitmap's intrinsic dimensions. Next-list predecessors affect drawing
order/visibility, not spatial parenting. Images use bounded source-over blending.
No DLL, font, callback result or bitmap from the installed game is distributed.

### Persistent clock stream

The opt-in --clock-frames profile uses schema 4 with the existing 14 numeric
flight bindings and one guarded clock string. Frame requests append finite
fractional local seconds in [0,86400). The PANELS lookup sees this exact value;
HMS slots use whole seconds. Ready reports clock_connected, string_bindings=1,
guarded_buffer, clock_set, clock_mouse and fractional_clock. The clock object
identifies library/gauge/element RVA, tokens [3,4,5], maximum length 8, input time,
text/result, events_delivered, local_event_bridge and guarded_event_words.

The parent tracks accepted button calls independently and requires an exact
event count. Before the first native button, the original default HH:MM:SS and
whole-second result are checked exactly. After buttons, native text must retain
the bounded eight-character clock format and the result must be finite/bounded;
private mode/timer state is not recreated by the decoder. Metadata, input echo,
event count and guards remain mandatory for all modes. Equal simulation time
requires unchanged physics and clock input. Reused frames return the already
updated native clock; a rewind restarts the complete session.

clock-click MOUSE_RVA X Y requires an initialized frame and a checked authored
left-single clock child/point. One armed capture receives the original mouse
packet, then delivers it to the original process-event callback. The response
has kind clock-click, unchanged physics sequence/time/evaluation count, exact
RVA/point, original packet word, native BOOL/acceptance, guarded context and the
updated clock object. The parent commits its event counter only after validating
all fields. Mouse or clock-source edits invalidate display state without advancing
physics. Malformed native responses close the owned helper instead of returning
stale success. Screen coordinates are not part of this protocol.

Close restores the original string pointer, disconnects the clock and deinitializes
three modules while the service table remains alive. It verifies context guards,
restores the import pointer and reports clock_events_delivered. Context/string
buffers outlive module teardown. Schemas 1/2 stay unchanged. This is a narrow
local service bridge; full PANELS, calendar and shared-cockpit behavior are absent.

### Local clock event subset

The simulation layer supports the seven local clock events in the
[published event table](https://www.prepar3d.com/SDKv6/sdk/references/variables/event_ids.html):
hour/minute increment/decrement/set and seconds zero. SET replaces only its
bounded field; increments/decrements advance local time with day wrap and ignore
payload. The whole-second state preserves the simulator's fractional phase.
Reset restores the original configured origin. Date, UTC, timezone and daylight
are not synchronized by this subset.

Schema 4 ready advertises clock_set. `clock-set SECONDS_OF_DAY` is accepted only
after an initial frame, refreshes the guarded native string and acknowledges the
unchanged sequence/time/physics frame counter plus the checked clock object.
The parent rejects edits for a different elapsed time and commits local state
only after acknowledgement. Equal-time frame requests still require unchanged
physics and the current clock value. A separate display-changed flag allows
clock/selection edits to repaint during physics reuse. Legacy schemas 1/2 are
unchanged; this is not a generic native event dispatch service.

## Native mouse metadata and candidate hits

Mouse reading is opt-in through `GaugeLibrary::mouse_rects(name)`: it requires
the 84-byte x86 gauge-header prefix and reads the pointer at 80, leaving ordinary
52-byte prefix parsing unchanged. Each 44-byte MOUSERECT has kind at 0, four
signed geometry words at 4..19, cursor/help at 20/24, flags/event at 28/32,
function slot at 36 and API data at 40. Parent/child/end-parent/EOL structure is
validated with 2048-record and 32-parent limits. Child callback RVAs must be
file-backed. USER tooltip records overload geometry/event/function slots with
help IDs, argument count and text metadata; they are never treated as actions.
Opaque API data and raw function-slot values are retained without dereferencing
runtime/argument graphs. Tooltip text pointers are bounded but text is not read.

The [published gauge reference](https://www.prepar3d.com/SDKv6/sdk/panels_and_user_interface/gauges/references/gauges_api_overview.html)
and [mouse structure reference](https://prepar3d.com/SDKv2/LearningCenter/simobjects/panels_and_gauges/programming_c_gauges.html)
describe native rectangles and their fields. Parent positions are gauge-relative;
children use the containing parent's origin. The current candidate layout clips
to gauge/ancestors and uses explicit root-bitmap dimensions for the zero-size
main box. Zero-size subregions need runtime installation and are rejected by
layout, while their metadata remains readable. Finite points and one activity
flag are required; matching children/GETALL candidates retain authored order.
Right/bottom edges are excluded by the current hit contract; original-FSX edge
and overlap-delivery semantics remain unverified. Drag capture, repeat timing,
wheel skip/flip and dispatch are not implemented by this candidate API.

`panel::native_mouse_point` converts a top-left-origin VC texel through explicit
panel/gauge extents to coordinates in the exact-language static root bitmap.
It requires a zero-origin untransformed root, rejects invalid layouts and ignores
points outside the panel/gauge. This is not a screen-ray/triangle intersection
or an event dispatch implementation.

### Isolated mouse callback probe

The public x86 MOUSECALLBACK layout is independently represented as 28 bytes:
relative point at 0/4, gauge-header user_data pointer at 8, mouse-record pointer
at 12, client/screen point at 16/20 and reserved pointer at 24. Mouse functions
use two-argument stdcall and a 32-bit BOOL return. The headless checked profile
uses explicit child-local coordinates, screen [0,0], the actual selected record
and connected gauge header; it does not invent a window or private gauge state.
Guard pages and prefix checks surround the context; the buffer remains valid
through native deinitialization. A synthetic self-test verifies field offsets,
arguments, full return width, access-violation reporting and prefix detection.

Probe schema 1/source native-mouse-probe records exact metadata/input, baseline
string, mouse_callback_completed, return or null, native exception/fault RVA and
access operation/target, context integrity and lifecycle guarantees. After-string
output is null on a mouse fault; it is not guessed or executed after partial
native failure. Exit 0 means the callback completed; exit 2 means a contained
fault with cleanup. Neither establishes operational native input. Event-size/
process-event addresses are read only in this checked FS1000 helper profile,
from the header's 236/240 offsets, with an explicit 244-byte extent check.

The [published PANELS/gauge definitions](https://www.prepar3d.com/SDKv2/LearningCenter/simobjects/sample_code/gauges.html)
and [shared-event API](https://prepar3d.com/SDKv2/LearningCenter/simobjects/panels_and_gauges/programming_c_gauges.html)
identify process_shared_event_out at x86 table offset 0x150. The sampled clock
mouse handlers access a missing PANELS pointer there; all three service-free calls
fault and are contained. A correct service bridge needs the original event-size/
process-event callbacks and supported variable lookup, with explicit delivery
semantics. No generic stub, fabricated packet or multiplayer behavior is supplied.

## Reference NDJSON

The separate x86 recorder keeps schema 1 for title/elapsed time and 12 control
readings. Optional schema 2 adds unsigned 32-bit `object_id`/`sample_index` and
32 named instrument/flight-state readings under `instruments`, all in explicitly
requested units. A single title-plus-44-double SimConnect definition produces
a 648-byte packet. Names containing DEGREES do not imply the default API unit;
the recorder requests degrees, feet, knots and feet per minute explicitly.

Readers bound files to 64 MiB, rows to 16 KiB and recordings to 100,000 samples.
Schema 2 requires every known field, finite values within magnitude 1e12,
valid failure enums/bools, and nonnegative simulator time/rate. Sequential replay
requires contiguous indices, monotonic elapsed wall time and consistent source,
aircraft title and user-object ID. Simulator time can pause or rewind. An
absolute clock rewind starts a new native session even when normalized time was
already zero. Metadata labels are not proof of a capture's authenticity.

The native replay adapter consumes measured indicated airspeed/altitude, VSI
and gyro indications directly. It does not substitute true speed, MSL altitude,
body attitude or ideal ISA values. It explicitly reports/rejects instrument states
outside the current healthy, neutral native profile. Available stateless model
bindings also consume recorded variables; unsupported/lagged groups stay omitted.
ISA/weather and body/gyro differences are diagnostic output without a parity
verdict. The extended request still needs live original-FSX acquisition checks.

See [the viewer guide](VIEWER.md) for capture, validator and replay commands.

## MDLX visibility subset

VISL contains packed VISS/VINS/VISC/VINC string chunks, bounded to 16 KiB per
condition. Strings must terminate and may have only zero bytes afterward.
SGVL is one signed i16 condition index per scene node, with -1 for no condition;
counts and references are checked. Direct Sim reads currently accept only the
observed variable#units form. Code may have a #lag# prefix; positive lag is
reported unsupported rather than silently ignored. VISS/VISC default visible;
VINS/VINC default hidden when evaluation cannot be completed. A nonzero result
selects visible. Parent visibility propagates through the scene traversal.

The C172 byte layout was inspected independently; the [FSDeveloper MDLX notes](https://www.fsdeveloper.com/wiki/index.php/MDL_file_format_%28FSX%29)
identify these chunks. The [modeling SDK](https://www.prepar3d.com/SDKv3/LearningCenter/modeling/using_modeling_tools.html)
describes visibility parameters, and the [Microsoft RPN reference](https://docs.flightsimulator.com/html/Additional_Information/Reverse_Polish_Notation.htm)
describes stack/branch syntax. These documents do not establish complete FSX
runtime equivalence. Tests use original synthetic expressions and triangle data;
no installed model expressions or SDK implementation are distributed.

### Isolated local PANELS event bridge

The separate --sample-clock-mouse-local profile preserves the checked mouse
inputs but binds the exported 16-byte ImportTable: ID 15, null function pointer,
then a zero ID/pointer terminator. Only the independent PANELS linkage prefix
and lookup_var (0xa0), process_shared_event_out (0x150), is_master (0x154) slots
are populated in a readonly table; unsupported slots are null. This table stays
alive through native ModuleDeinit; the original null import pointer is restored.

The installed C172 event-size and process-event callbacks use x86 stdcall with
two arguments. process-event/shared-event/master return C++ bool, unlike the
32-bit BOOL of mouse callbacks. These declarations are checked against the
installed binary's stack cleanup and independently tested synthetic callbacks.
The native event-size callback must report four bytes into a guarded word. The
shared-event service requires the actual clock header, four bytes and exactly
one nonrecursive packet, copying the original bytes while valid. After mouse
return, a guarded copy is delivered once to the original process-event callback;
acceptance and unchanged packet bytes are required. Native faults are contained.

lookup_var accepts only checked writable module-owned MODULE_VAR records with
CLOCK_TOTAL_SECONDS (557), supplies explicit local seconds since midnight as
FLOAT64 (type 33), and preserves the previous numeric value. Unknown variables
fail the observation. is_master returns true only for this local host profile;
none of the three checked first-click cases calls it. General variable address
binding, multiplayer and simulator-owned event routing are not implemented.

Records use source=native-mouse-local-event and add original packet word,
event size/count/delivery/result, lookup token/value/count, master query count,
guard/table/import guarantees and explicit multiplayer=false and
fsx_delivery_parity_verified=false. panel_services=false denotes the incomplete
service surface, while local_event_bridge=true identifies this narrow profile.
At 12:34:56 the three authored children produce words 1/0/2; original processing
changes the first main string to " 0:00:00" and performs one token lookup for
the second. The service-free probe remains available as a fault baseline.
Local delivery order is a host assumption requiring later reference validation;
this does not claim operational viewer buttons or original shared-cockpit parity.

### Retained local clock command stream

--clock-local-frames loads one original Cessna172 Clock and emits a
native-clock-local-stream/schema-1 ready packet with its actual PID, pointer
width and explicit narrow capabilities. It accepts newline-delimited ASCII:

```
time SEQUENCE LOCAL_SECONDS
click SEQUENCE LOCAL_SECONDS CHILD_INDEX
reset SEQUENCE LOCAL_SECONDS
quit
```

Sequences must advance exactly by one from 1 and stop at 4096. Local seconds are
finite doubles in [0,86400), allowing fractional CLOCK_TOTAL_SECONDS lookup;
whole HMS source slots are derived by truncating the nonnegative input. Child
indices 0..2 resolve the actual authored records and centers. Each click arms
exactly one shared-packet capture, invokes the original mouse handler, then
passes its guarded packet to the original event handler. Time commands do not
invoke mouse callbacks. No header-private clock object is synthesized. Native
callbacks own the mode, format and stopwatch transitions.

State responses echo sequence/generation/command/time, actual element and mouse
RVAs, text/result, cumulative delivered-event count within the generation and
per-command lookup count. time_unchanged compares explicit time within the
current generation; display_changed compares the actual text/result and is true
for the first state after a reset. Neither flag claims a physics evaluation or
live GUI repaint. Nonclick mouse fields are null. Guard flags describe context,
string and event buffers; panel_services/multiplayer/fsx_parity_verified remain
false. The PANELS time setter rejects pending undelivered events and preserves
fractional lookup values. Unsupported variable tokens still fail explicitly.

Reset closes/disconnects the old clock, deinitializes/unloads the module and
restores its import pointer before loading a fresh original module in the same
helper PID. It increments generation and clears event state. Closed packets
report the final sequence, module-deinitialization count and checked context,
string/import restoration. Native operations have a five-second watchdog;
waiting for the next command disarms only this activity deadline. Input lines
remain bounded to 512 ASCII bytes; the Rust checker bounds output lines to 4 KiB
and waits at most 5.5 seconds per response/exit, killing only its owned helper on
failure. Invalid commands fail before a state transition is emitted.

This explicit local profile checks an advancing stopwatch within one day, pauses,
stop/reset/start, format changes and native reset. Running across midnight,
changing the simulation clock while the stopwatch runs, secondary text and
original-FSX shared-event delivery still require evidence. This diagnostic stream remains separate from the schema-4 viewer flight bridge
described above and is not a full PANELS implementation.

### Screen surface picking and C172 mouse routing

fsx-render::picking is a CPU reader of current aircraft geometry/appearance, with no native
execution. It combines the exact renderer camera and supplied model matrix in
f64, inverts it with pivoted elimination and unprojects physical top-left screen
coordinates to a model-space Direct3D depth segment. Near depth is included;
far/clear depth 1 is excluded. Viewports must be nonzero and at most 32768 per
axis. Out-of-viewport points return no hit; nonfinite/singular data fails. It
bounds geometry to the model decoder's three-million-corner output budget and
requires complete visible triangle ranges and valid material/texture indices.

Two-sided segment/triangle intersection gives barycentric model-space UVs,
including perspective. The nearest surviving surface wins, preserving first-draw
order on exact ties to match the opaque Less depth test. Alpha uses the current
appearance images, normalized repeat/clamp addressing and half-texel bilinear
single-mip sampling. Alpha-test operations match viewer.wgsl; a zero-alpha blended
surface is skipped, while nonzero-alpha foregrounds block the input ray.
Nonblended alpha zero still blocks unless the shader alpha test discards it.
This is an explicit input-surface policy, not original-FSX transparency/overlap
parity or a claim that all blended compositing orders match a depth ID buffer.

A SurfaceHit includes batch/triangle/material/texture identity, raw/addressed UV,
model position and near/far segment parameter. The application resolves only the
actual picked VC texture containing its enabled C172 Clock. It maps UV using the
current preview dimensions, CFG placement and decoded native root dimensions,
then asks the bounded mouse metadata reader for left-single candidates. No hit
means no action; multiple candidates fail instead of guessing overlap priority.
Exactly one supported authored callback record is required, and child coordinates
are floored for the integer native context. The existing x86 helper checks the
record and bounds again before calling it. No picking result supplies an arbitrary
executable address.

The window routes only left press in visible cockpit flight with a known cursor
and enabled native clock; release/right/wheel never dispatch this callback path.
Window-exit/focus events clear stale cursor state. Native state/display update
uses the current simulation frame and then repaints the existing gauge texture.
No cursor icon, tooltip, drag/repeat/double-click semantics, other gauge controls,
HUD/procedural-ground hit testing or full FSX input parity is supplied by this stage.

### Connected C172 OAT/voltage text inspection

The checked secondary descriptor uses tokens [341,335,753], maximum 7 and
source-callback slots zero. The public token/units enums identify ambient Celsius,
display units (0 English, 1/2 metric) and main bus voltage. Explicit probes require
f64 union slots, temperature -273.15..1000, an integer units enum 0..2 and voltage
0..99.9. These are inspection bounds, not a weather/electrical model. Normal
connected OAT sampling needs only original gauge connect/disconnect. The voltage
profile additionally binds the narrow local PANELS bridge, invokes the original
third authored mouse child and delivers its own packet 2 before the secondary
string callback. Context, packet acceptance, string/import restoration and module
teardown are checked. No private clock flag or event packet is fabricated.

Native output is bounded in original ANSI bytes. A specific CP1252 decoder maps
it to Unicode JSON escapes while keeping the transport ASCII. C0/DEL/C1 and
undefined CP1252 bytes fail. Metadata/ordinary string profiles stay ASCII.
Records add text_encoding, secondary_mode, mouse_event_dispatched,
original_packet_word and applicable local-event/context/import guarantees.

The native painter accepts only printable CP1252 code points and an installed
matching font with every requested glyph. Character indices are converted to
UTF-8 boundaries for SEQ_REC slices, so a degree sign occupies one ANSI selection
position despite two UTF-8 bytes. XML painting continues to use its ASCII gate.
The observed OAT strings contain degree signs; actual outputs include 59 F,
-40 C and native clamp endpoints -99/999, while selected voltage samples produce
28.0/0.0/99.9 V. The renderer's --oat capture contract verifies all nine native
observations before drawing; it does not produce sensor values or activate a live
secondary flight binding. Electrical-bus behavior and FSX GDI parity remain open.

## Authored legacy electrical data and circuit observations

The [Prepar3D legacy aircraft configuration reference](https://www.prepar3d.com/SDKv5/sdk/simulation_objects/aircraft_configuration_files.html)
and [Microsoft legacy electrical parameter reference](https://docs.flightsimulator.com/html/Content_Configuration/SimObjects/Aircraft_SimO/system/electrical.htm)
identify the bus/current/minimum-voltage triples and optional source settings.
The implementation reads declarations actually present in [electrical]; omitted
SDK circuit defaults and unprofiled source defaults remain unresolved. An absent
authored source scalar is None, while authored zero stays Some(0). Effective
source resolution is separate (see the versioned profile below). Values from SDK
examples for other aircraft are not used as defaults.

fsx-vehicle::electrical models bus codes 0..7 (main, avionics, battery, hot battery,
then generator/alternator buses 1..4). Known consumer keys require exactly three
fields. Numeric values must be finite/nonnegative; implementation bounds are
1000 V and 1,000,000 A. Source settings preserve optional maximum battery voltage,
battery capacity/charging-current limit, generator voltage/current, numeric 0/1
availability and up to four engine-map
entries (counts 0..4). These are current reader bounds, not general FSX rating
limits. Unknown keys/values remain in unsupported metadata. The section budget is
256 entries, keys 128 bytes and values 4096 bytes. Invalid values do not select a
normal-voltage fallback. Flight Parameters retain this data; visual-only loading
does not require an electrical model.

Simulation owns a CircuitNetwork initialized from that schema. All bus readings
and requested device states start unavailable. Callers may explicitly supply a
finite observation or clear it with None; known 0 V denotes a deenergized bus.
Device requests are case-insensitive and must name an authored circuit. A device
is off when not requested or when a known bus is at/below its threshold. With a
known strictly sufficient bus and a true request it is powered; otherwise the
result is unknown. Original predicate checks corrected the earlier inclusive
minimum assumption: equality is off. An explicit zero maximum battery rating
bypasses voltage checks in this regular inspection profile; unknown requests stay
unknown, known true/false requests remain true/false even at zero observed volts.
This is legacy availability, not measured energy delivery. A
requested powered consumer contributes its authored maximum amps; an off consumer
contributes zero; unavailable positive-load demand propagates None to the sum.
Zero authored maximum contributes known zero even when its state is unavailable.

Snapshot.authored_bus_demand_amps covers only these declared/requested maxima.
It is not total aircraft load, measured current, source capacity or a generator
allocation. The caller must supply actual equipment activity/presence; the parser
does not activate every CFG consumer (for example a fixed-gear aircraft's unused
gear-motor entry). No implicit electrical source or bus-tie topology is created.
An explicit always-available override is retained but fails this regular-mode
circuit evaluation rather than silently pretending bypass semantics.

Invalid observations/requests are atomic. Runtime reset clears observations and
requests; new simulations start fresh. Ordinary physics stepping does not infer
voltage from throttle or fabricate battery/generator behavior.

CircuitNetwork also owns Battery and Generator primitives. Capacity and charging
limit have a 1,000,000 Ah/A implementation ceiling; explicit zero is preserved.
Battery starts with unknown charge. Its explicit advance API accepts 0..3600 s
and a signed requested terminal current with magnitude <=1,000,000 A (positive
means discharge). It integrates charge by I*seconds/(3600*capacity), limits charging
using the authored limit and clips at empty/full, reporting average accepted
current. Unknown current, missing/nonpositive capacity or a needed but missing
charge limit invalidates
charge knowledge; zero current or zero time preserves it. Zero-capacity storage
cannot be evolved. Invalid inputs leave state unchanged. These are an initial
ideal storage model, not verified FSX battery dynamics or measured current.

A separately supplied VoltageCurve requires 2..256 finite points, strict charge
ordering covering 0..1 and nondecreasing voltage in 0..1000 V. It uses linear
interpolation. If maximum battery voltage is authored, the full-charge endpoint
must match it. This API does not interpret battery_capacity_pct_to_voltage: that
CFG value remains unsupported metadata. Without a supplied curve, partial-charge
voltage stays unknown; known full charge can expose the authored maximum. This
is open-circuit voltage, never automatically loaded bus voltage.

Generator accepts explicit engine/regulator availability. Known load in 0..its
known maximum current returns authored ideal regulated voltage when online;
unknown demand/capability, overload or unavailable rating returns None. Offline
reports a disconnected source (0 V), not an observation of the shared bus.
Network reset clears charge, caller curves and availability along with circuit
observations/requests. Sources are not automatically wired. Battery charge advances
with committed physics time only when its current request is explicitly provided.

The electrical_distribution solver accepts an explicit complete inventory of up
to eight feeds, 28 unique non-self bus pairs and eight optional fixed-current
requests in 0..1,000,000 A. A feed has a bus, optional online state, voltage in
0..1000 V and capability in 0..1,000,000 A. Open contacts isolate, closed contacts
join buses bidirectionally with ideal zero resistance. No supplied feed means
explicitly disconnected; callers with incomplete aircraft inventory must not
use absence to imply a dead bus. No graph is generated by the CFG parser.

A connected group with one known online feed and known demand within known
capability receives its voltage and allocated currents. Feed current equals the
sum delivered to its connected buses. No active feed produces known zero volts
and zero delivered current even for unavailable requests. Unknown contacts can
join otherwise separate groups, so uncertainty propagates across those groups;
a redundant unknown path within a known-connected group has no additional effect.
Unknown source state/rating/load, overload and multiple active/possibly-active
feeds give unavailable voltage/current. Offline feeds remain disconnected with
zero supplied current. Parallel-source priority/sharing and battery charging are
not resolved. Zero voltage from a sole known online feed supplies no current.

Generator.feed exports existing source state on an explicitly chosen bus.
CircuitNetwork.distribute atomically publishes the solver's eight voltages;
malformed inputs leave prior observations intact. The provider must re-solve
with fresh source/contact/load inputs when they change. These current requests
are independent inputs, not automatically copied from authored circuit maxima.
They are an ideal constant-current profile, not a measurement or a nonlinear
load/threshold solver. A result can be used by authored circuit evaluation after
publication, but no feedback loop activates consumers or derives battery current.
Actual equipment activity, engine/regulator providers, aircraft topology,
battery coupling, overload response, default resolution and FSX parity remain open.


## Legacy FLT electrical switches and simulation time

The [Microsoft FLT reference](https://docs.flightsimulator.com/html/Content_Configuration/Flights_And_Missions/Flight_Definitions.htm)
identifies BatterySwitch, GeneratorSwitch and AvionicsSwitch as switch positions.
Installed FSX FLTs also establish the older Systems.0 avionics location and the
Switches.0 pitot/light spelling. These values do not establish online generation,
bus voltage, equipment presence, charge fraction or full scenario state.

fsx-vehicle::flight_electrical reads player aircraft 0: Systems.0 BatterySwitch,
AvionicsSwitch in Systems.0 or Avionics.0, GeneratorSwitch in Engine Parameters.1.0
through .4.0, and Switches.0 PitotHeat and ten light positions. PanelLights may
also occur in Systems.0. Both locations may agree; conflicting values fail because
legacy precedence is not verified. Booleans are case-insensitive True/False only;
missing fields stay None. Other aircraft/sections and unrelated keys are ignored.
File loading is bounded to 2 MiB with explicit UTF-8/Windows-1252 decoding; relevant
sections are bounded to 256 entries, 128-byte keys and 4096-byte values.

CircuitNetwork.apply_switches applies only declared aircraft circuits and replaces
all supported mapped requests (including clearing omitted values). Other explicit
manual device requests remain. Applying a snapshot clears bus observations,
generator availability, terminal-current request and last-flow record; it preserves
existing battery charge until the next actual integration input. Reset also clears
charge and supplied curves. No switch closes a guessed aircraft bus contact.

set_battery_current_request accepts optional finite signed current bounded to
+/-1,000,000 A. It is independent of authored requested consumer maxima.
Simulation.step advances the battery after physics checks and before committing
each accepted state. Ground subdivisions advance their own durations exactly once;
rejected intervals do not advance electrical state. Suspension settling clears its
source interval record before the user flight begins at time zero. The desktop
scheduler skips integration during pause and reset creates fresh electrical state.
last_battery_flow reports the last committed substep's average accepted current,
not a frame average. Unknown current makes charge unknown on a positive interval;
zero elapsed time preserves the preceding charge/record. No loaded bus voltage is
derived from this charge integration. Automatic FLT/viewer initialization, actual
current/engine providers and complete flight restoration remain unimplemented.


## Versioned FSX source scalar defaults

scripts/check-electrical-defaults.py reads the user's installed sim1.dll into
Unicorn x86 memory, not the Windows loader. It supports only SHA-256
4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e,
version 10.0.62615.0. The reference executes original source initialization and
type dispatch, with a synthetic vehicle engine count/type and stubbed allocation
and numeric CFG lookups. Its prefix stops immediately after the three source
scalar lookups; the original object zeroing constructor also executes. Instruction
ranges and mutable memory writes are whitelisted with a 4096-instruction budget.
It fails on unexpected ABI/key/order/destination, foreign fingerprints or leaving
the allowed ranges. No original implementation bytes are stored in the script or
report. It does not execute game code in the host or launch FSX.

```powershell
python scripts/check-electrical-defaults.py FSX/sim1.dll target/electrical-default-reference
```

The output directory must be new; Python requires pefile and unicorn. The report
records 23 cases (types 0..6, counts 0/1/4, zero and nonzero override fixtures),
pre-lookup initialized values, effective values and scope. This is evidence for
scalar initialization with those injected engine contexts; it is not a check of
the original numeric CFG parser, live bus voltages, RPM/regulation, total load,
charging or battery capacity/decay.

SourceDefaults::FsxSteam62615SinglePiston resolves only absent maximum battery
voltage, generator voltage and generator maximum current to 24 V, 28 V and 60 A.
Authored zero/nonzero values are preserved; capacity, charging current, curves and
omitted consumers remain unresolved. The caller must establish single-piston
eligibility. Electrical.with_source_defaults returns effective data without
modifying authored metadata. CircuitNetwork::new keeps sources unprofiled;
new_with_source_defaults selects an explicit profile and source_configuration
returns effective data. Reset reconstructs sources from the same profile.

Simulation's existing single-piston dynamics selects this versioned profile.
Its Parameters retain authored Electrical values, so missing C172 CFG entries
still report None while effective ratings resolve separately. Initial charge,
generator availability and bus voltage remain unknown. Applying a FLT switch
snapshot does not establish generator availability or override this distinction.
The broader reference engine-type matrix is diagnostic, not support for jet,
multi-engine or other flight models. Parity with other FSX versions is unverified.


## Original pre-load generator admission

The same pinned reference supports a second bounded mode:

```powershell
python scripts/check-electrical-defaults.py FSX/sim1.dll target/generator-admission-reference --availability
```

It executes the original admission routine with a synthetic one-engine state,
injected failure boolean and an auxiliary-service stub returning x87 zero. The
87 cases cover enable/switch/failure/battery-master combinations, signal 0,
0.099999, exactly 0.1, nextafter(0.1,+infinity) and 0.9, zero nominal voltage and
previously-online transitions. Original instructions and mutable writes remain
whitelisted with a 4096-instruction budget per invocation. The original state
signal at engine offset 0x5c is RPM / max_rated_rpm, established by the named trace
below. Raw RPM, throttle and percent values in 0..100 are not equivalent inputs.
A FLT Pct Engine RPM value is a saved fraction, not a live engine provider.

For a positive nominal source voltage, admission requires an enabled source-mask
bit, generator switch on, no failure and signal strictly >0.1. The reference
records current active count, raw engine flag, raw generator-voltage slot and
pre-load main/battery/hot-battery values separately. With a disabled mask bit,
old raw flag/voltage slots can remain while current active count and main-bus
contribution are zero. Admission is based on the current count, not stale slots.
These are pre-load results; current limiting, delivered current, charging and
final loaded bus voltage are not established by this mode.

LegacyGeneratorInputs separates source_enabled, switch_on, failed and the legacy
rpm_fraction, all optional. Generator.update_fsx_steam_62615_availability uses
the verified known-input gate and finite signals. Unknown inputs yield None unless
a known off condition proves false. A known zero nominal voltage is off; unknown
nominal voltage prevents a true result. Invalid signals preserve prior state.
This does not model source regulation or provide live engine RPM.

CircuitNetwork.update_generator_from_fsx_steam_signals is restricted to the
versioned single-piston profile. It uses the stored first-engine FLT switch and
explicit enable/RPM-fraction/failure inputs. Successful updates clear bus observations,
battery-current request and last-flow record. Callers must provide a fresh graph
and current allocation afterwards; failed validation preserves the old state.
Ordinary physics does not call this API without a confirmed engine provider.


## Named generator RPM fraction and FLT snapshot

A third version-pinned reference mode connects the engine signal to named inputs
and outputs using original instructions in sim1 10.0.62615.0:

```powershell
python scripts/check-electrical-defaults.py FSX/sim1.dll target/generator-rpm-trace --engine-signal
```

Thirty cases execute the original named piston_engine max_rated_rpm lookup
(destination check; numeric service injected), rated-speed copy for a piston,
signed engine RPM reconstruction and FLT save routine. The original Pct Engine
RPM descriptor identifies offset 0x18 in the save record; the original copy writes
the generator's engine-state fraction there. Runtime RPM equals that fraction
times rated RPM. The GeneratorSwitch descriptor/copy is checked independently.
The reconstructed fraction is then passed to the original admission routine.
Rated speeds 2400/2700/3000, zero, exact 0.1/nextafter, 0.9, 1.25 and forward/reverse
rotation cover mapping, overspeed, signs and the strict admission boundary.
Only guarded original ranges execute; mutable writes are bounded to synthetic
objects, stack and the original 0x48-byte save record. This is not original engine
integration, CFG numeric parsing, generic SimVar dispatch or complete FLT loading.

LegacyGeneratorInputs.rpm_fraction is RPM / max_rated_rpm; 1 means rated speed.
The FLT reader's engine_rpm_fraction array retains optional finite Pct Engine RPM
for Engine Parameters.1.0 through .4.0. Values are not multiplied by 100 or clamped
to 0..1; negative rotation and overspeed survive reading. Missing fields remain
None; nonnumeric/nonfinite values fail before application. Other FLT read bounds
and section scopes remain the same.

CircuitNetwork.update_generator_from_saved_flt explicitly uses the saved first
engine's fraction and generator switch with separately supplied enable/failure
inputs. A missing source/failure input still prevents admission unless off is
already known. Success invalidates old supply/current observations like the
ordinary versioned signal API. Call this for an explicit snapshot application,
not as a live engine provider on later physics frames. Flight stepping does not
infer RPM from throttle or repeatedly feed the initial FLT value. A real evolving
engine model/provider, current allocation and loaded-bus coupling remain open.


## Original generator-current allocation phase

```powershell
python scripts/check-electrical-defaults.py FSX/sim1.dll target/generator-current-reference --currents
```

This mode uses the same sim1 version/fingerprint and memory/instruction guards.
386 cases execute original generator-current allocation and then a separately
invoked current sum. A synthetic auxiliary service returns x87 zero. Inputs are
explicit active flags, load, current battery voltage and configured maximum
voltage/capability; no engine or consumer presence is inferred. One/two-engine
flags, zero/60 A capability, 12/24 V maximum battery voltage, current battery
voltages 0/20/24/30 V and loads 0/12/60/120 A cover the branches. Inactive slots
start with stale 123 A and must be cleared by original code. Two additional
fixtures retain stale engine-active flags with a zero admitted count; the
allocation must still clear their slots.

For each active source with a nonzero active count, original allocation is
min(capability, (load + (maximum_battery_voltage - battery_voltage))/active_count).
Inactive sources are assigned zero. No lower clamp is applied. Above-maximum
voltage fixtures can therefore produce negative allocation. These are isolated
phase semantics, not proof that all such fixtures are reachable in an ordinary
complete FSX frame. The independently invoked sum must match all allocated slots
for the zero-auxiliary profile. Nonzero auxiliary behavior is outside this check.

Generator.fsx_steam_62615_allocated_current exposes only the single-generator
profile: optional load in 0..1,000,000 A and optional battery/current maximum
voltages in 0..1000 V. Values must be finite. Known offline means zero; otherwise
all required inputs, availability and capability must be known. The signed result
preserves the original no-floor behavior. This is distinct from supplied_voltage,
which is an ideal within-capability source-voltage inspection API.

CircuitNetwork.fsx_steam_generator_current_request requires the versioned single
piston profile, obtains its effective maximum battery rating and uses independent
load/battery-voltage inputs. It is read-only and never copies authored requested
circuit maxima into load, energizes buses or updates battery charge/current.
The original frame sums prior source currents before computing the next
allocation. The reference's separate post-allocation sum tests a different
invocation boundary, not same-frame delivery. Full ordering, filtering, auxiliary
allocation, battery-voltage integration and actual load providers remain open.


## Ordered legacy voltage-core prefix

```powershell
python scripts/check-electrical-defaults.py FSX/sim1.dll target/voltage-core-reference --voltage-core
cargo run -p fsx-app --example legacy-voltage-core-check --locked -- target/voltage-core-reference/report.json target/voltage-core-comparison
```

Both output directories must be new. The reference executes the original frame
prefix, its admission, active-load summation, prior-source sum and allocation
routines in the same order, ending immediately after voltage storage and before
battery-failure/avionics processing. One generator and one explicitly active
consumer are wired in synthetic memory; failure-query and auxiliary services
return injected false/zero respectively. The native float32 response factor is explicitly provided. Its
physical origin is unverified. Original instruction/write guards remain active
with a 10,000-instruction budget per prefix invocation.

Nine sequences (380 frames) cover discharge, source start/stop, prior-current lag,
zero/0.1-float32/double response, empty/full voltage, overload, battery-master off,
and zero-generator rating with a positive battery maximum. The report records
input, incoming state, outgoing state, pre-load bus values and prior current sum
for every frame. The Rust checker validates profile/schema/size/case/frame bounds
and compares each value to those original outputs with absolute tolerance 1e-9;
observed maximum error is approximately 1.42e-14. This is not a complete FP-bit
parity claim for all possible states or a live FSX/aircraft capture.

legacy_voltage::Core holds optional State (battery_voltage, filtered_net_amps,
generator_amps). The last allocation becomes the next step's source sum. Advance
accepts positive dt <=1/60 s and optional generator-online, battery-master,
nonnegative load <=1,000,000 A and a finite nonnegative float32 response factor.
Configuration voltages are bounded to 0..1000 V and capability to 0..1,000,000 A;
maximum battery voltage must be positive. A zero maximum is not silently fixed:
the original zero-auxiliary sum has a zero-denominator singularity outside this
checked prefix. Seeds must be finite; they are explicit observations/context,
not an assumed fully charged initial battery.

For known inputs the original ordered operations are:

1. Record pre-load main/battery/hot-battery voltage from incoming state/source.
2. Read prior allocated source current I; compute Fnew = F + (I - load - F)*4*dt.
3. Compute Vintermediate = max(0, V + Fnew*0.000555*response_factor*dt).
4. Compute next allocation from Vintermediate using the checked current phase.
5. Compute Vnew = min(maximum, Vintermediate + I*0.0004*dt).

The allocation sees intermediate voltage before the upper clamp. It can therefore
be signed even when final voltage equals the maximum. A known zero generator
rating supplies no generation and leaves battery-only pre-load voltage un-clipped
by that zero rating. Unknown required inputs clear core knowledge on a positive
interval; invalid/nonfinite/overflow computation leaves prior state unchanged.
These states are distinct from the separate ideal Coulomb-counting Battery.

CircuitNetwork exposes seed_legacy_voltage_state, legacy_voltage_state and
step_legacy_voltage_core only under the versioned single-piston profile. The
step uses stored battery-master position and current source availability/ratings,
with explicit load/response inputs. Reset clears the seed; switch/source changes
preserve voltage history so prior-current lag is not discarded. Ordinary
Simulation.step does not invoke this provider-dependent prefix automatically.
Results are not published to the observed bus array or used as charge fraction.
Tail failure/avionics behavior, factor provenance, actual initial/live inputs and
loaded-bus integration remain unfinished.


## Original circuit predicate and electrical frame tail

```powershell
python scripts/check-electrical-tail.py FSX/sim1.dll target/power-tail-reference
cargo run -p fsx-app --example legacy-power-tail-check --locked -- target/power-tail-reference/report.json target/power-tail-comparison
```

Use new output directories. This reference pins the same sim1 fingerprint and
executes original predicate, bus lookup and frame-tail code with guarded CPU
instruction/write ranges and a 4096-instruction budget per call. Raw context flag,
mode-service return, battery-failure response and AVIONICS MASTER SWITCH query
(ID/name/ABI checked) are injected. Context mode remains an explicit provider;
no flight mode is assumed. Schema 3 additionally executes the original final
boolean CFG reads with an injected lookup contract: present keys write the value,
absent keys leave the destination untouched. Exact section/key/output/handle/flags
are checked. Electrical/electric_always_available writes the same global used by
the power predicate. No code in this bounded tail resets that global first.

Two sequences / 16 loads cover both observed initial values and missing/true/false
keys. Each load is followed by the original predicate at zero voltage with a
forced-off circuit, verifying that the loaded global controls the bypass. Rust
legacy_power::PowerOverride retains this process value across configuration loads;
load_configuration consumes Electrical.always_available. Default is unknown, an
absent key preserves unknown or the previous value, and an explicit key resolves
it. Carry this object across aircraft changes and supply its known value to
TailEnvironment.override_power. Do not create a new false value per aircraft or
infer the initial process value. Startup, other writers and automatic viewer
ownership are outside this check. The regular inspector's unsupported override
mode is unchanged; raw availability is separate from physical supply.

1280 predicates and 165 tails compare exactly to the Rust implementation. In
voltage mode, absent circuit metadata returns false; invalid bus mapping returns
true early (the normal CFG parser still rejects invalid bus codes). Otherwise
availability is true for the global override, zero maximum battery voltage, or
bus voltage strictly greater than minimum with no forced-off flag. Equality is
false. Outside voltage mode the original returns battery-master position before
checking circuit presence, bus, rating or override. Rust legacy_power::Context
selects this resolved mode explicitly; Option<Circuit> None means known absence,
not unresolved presence. All supplied numeric values must be finite.

Battery failure clears all eight buses, four generator current slots and source
sum. It does not clear stored battery voltage or filtered net current. The tail
then evaluates avionics circuit availability, applies avionics master and copies
main to avionics voltage only when both allow it. Navcom1 is evaluated after that
copy/clear. Override/fallback can set active flags even when failed buses remain
0 V; callers must not equate these flags to nonzero voltage. Previous avionics
voltage can matter when the configured avionics circuit points at its own bus.

The same reference runs four complete controlled sequences / 68 frames through
the original method with one generator and two declared consumers (NAVCOM and
avionics). Their requested maxima are 12/5 A and presence is explicit. Actual load
for each next frame is computed from prior active flags; there is no automatic
aircraft equipment inference. Failure/avionics/mode services, float32 response
factor and seeds are controlled. Original and composed Rust states match within
1e-9, maximum observed absolute error about 3.55e-15. This proves these complete
controlled method calls, not all aircraft loads, environments or live FSX parity.

legacy_voltage::Core.advance_frame stages prefix and tail together. The tail's
post-failure generator current becomes the next core sum; internal battery/filter
state is preserved through a failure. TailEnvironment requires known surrounding
bus/current observations, source-map generator_enabled, circuit presence,
failure/master state and mode/override. Disabled-source or zero-nominal admission
preserves the incoming generator bus voltage; an enabled but offline positive
nominal source writes zero. An online/disabled contradiction is rejected.
Unresolved context loses frame knowledge; invalid environment fails atomically.
The low-level prefix API remains separate and does not publish loaded buses.

CircuitNetwork.step_legacy_electrical_frame explicitly selects the versioned
single-piston path. It combines stored battery-master/source availability with
provider-supplied load/response and tail environment, validates resulting buses
in 0..1000 V and publishes all eight together. Failure produces known zeros;
missing providers clear prior observations to None. Invalid input preserves both
state and observations. The independent Coulomb-current request/flow is cleared;
voltage state is not converted into a charge fraction. Ordinary Simulation.step
still does not call this path, choose factor 1, infer mode/override, manufacture
initial voltage for an existing aircraft, reuse FLT RPM indefinitely or resolve
omitted equipment defaults.

### Original electrical allocation and initialization

```powershell
python scripts/check-electrical-initialization.py FSX/sim1.dll target/electrical-initialization-reference
cargo run -p fsx-app --example legacy-electrical-initialization-check --locked -- target/electrical-initialization-reference/report.json target/electrical-initialization-comparison
```

Use new output directories. The reference uses the same version/hash as the
electrical tail checker and reuses its guarded emulation harness. It executes
the complete original state constructor against poisoned memory and the complete
initializer, with context service, failure-object factories and generator naming
injected at checked ABIs. It exports only numeric observations and metadata.
40 cases cover fresh/reused states, max battery 0/12/24/28/1000 V, generator
nominal 0/28 V and generator-map enabled/disabled. The constructor clears scalar
voltages/currents/filter/load/master/count and consumer records. Initialization
sets battery voltage and every bus voltage to the effective maximum, preserving
existing filter/current/load/master/count state. Generator bus pointers alias
the four internal voltage/current records.

32 fresh/reused positive-battery cases execute a first full frame with known
factor 1, zero auxiliary, retained battery-master position, generator/avionics
switches off and explicitly zero circuit metadata. These are
controlled fixtures, not inferred omitted aircraft equipment. Rust matches all
compared initialization/first-frame values exactly. This exposed a prior full-frame
port bug: disabled or zero-nominal generation leaves its previous bus observation
intact. TailEnvironment.generator_enabled now distinguishes this from an enabled
but offline source, which writes zero for a positive nominal rating. Generator
bus readings may thus retain initial battery voltage even when not admitted.

legacy_voltage::Core.constructed establishes the original fresh zero history;
Default still means unknown. initialize_rated_voltage updates battery and bus
voltages from the effective rating and retains known filter/current history.
An unknown history stays unknown even when initial bus voltages are known.
Missing ratings invalidate knowledge, invalid ratings fail atomically. Zero is
valid for initialization; the dynamic voltage core still excludes zero maximum.

CircuitNetwork.new_fresh_legacy creates this fresh state using the pinned
single-piston source profile. All buses initially equal max battery voltage,
battery master and generator admission are off, and Coulomb charge is unknown.
Call only for a genuinely fresh allocation, before applying flight state. This
is not an assumption about an already running aircraft or a saved-flight restore.
The installed checker proves C172 fresh 24 V buses, followed by Lesson 1 switches
and RPM plus explicit load/response/context/failure/NAVCOM inputs reaching 28 V
main/avionics. Existing construction/reset still leaves observations unknown;
ordinary physics has no automatic new initialization or legacy frame invocation.

### Electrical response-factor provenance

```powershell
python scripts/check-electrical-response.py FSX target/electrical-response-reference
```

The bounded trace pins sim1 10.0.62615.0 and main.dll SHA256
7b01a1b3a53e0779309aaf0144519cabb6b339a82c8490449dabd129c5e5883f.
main ordinal 57 returns a global settings record; its original descriptor names
[REALISM] General at double offset 0x20. The original sim1 settings-copy method
converts that double into the float32 context field consumed by the electrical
voltage calculation. Two context queries are injected active for this copy;
with the first query inactive, the previous context factor is retained. Ten
cases cover 0/0.1/0.5/1/1.25 and active/inactive queries, then execute the actual
filter/voltage fragment with the copied value. Instruction/write guards and a
4096-instruction budget constrain each phase. No original code bytes are emitted.
This establishes named settings provenance, not battery temperature/capacity.
File parsing, defaults, the live query meaning, other writers and automatic
viewer ownership remain unverified. Callers still supply response_factor
explicitly; absence does not become 1.

The reference also supports independent update/copy gates (20 cases). Compare
the ported provider with:

```powershell
cargo run -p fsx-app --example legacy-electrical-response-check --locked -- target/electrical-response-reference/report.json target/electrical-response-comparison
```

fsx_vehicle::realism::Realism::parse/from_config reads authored General; missing
stays None. The supported electrical domain is finite nonnegative f64 that
converts to finite f32, retaining values above 1. This domain is not evidence of
the original settings parser's rejection behavior. legacy_response::Response
starts unknown or with an observed factor. copy_settings retains history when
update is false or update=true/copy=false; both true copies with float32 rounding.
Unresolved gates or admitted missing settings invalidate knowledge; invalid
numeric input is atomic. factor() is suitable for the explicit voltage-core
input. This does not implement automatic settings discovery, gate semantics or
flight timing.

The response checker additionally passes each copied factor into Core.advance
with the same 22 V/zero-filter/zero-prior-current, 12 A load and 1/120 s inputs
as the original fragment. Both voltage and filtered current are compared at
1e-12 tolerance and the maximum error is recorded. Admitted missing settings
must invalidate evolving core knowledge; invalid provider updates are checked
for atomic retention of the preceding factor. These are explicit-input checks,
not evidence of automatic flight context selection.

Schema 2 of the response reference executes the concrete context getter from
its actual sim1 vtable slot 0x50: it returns raw +0xB4. The primary field is the
same one tested directly by the original electrical availability predicate.
The linked receiver (primary+0x34) is assigned the same concrete vtable explicitly
in this fixture; its live runtime type and ownership remain unverified. 80 cases
vary both raw words over 0/1/2/0xFFFFFFFF, confirming nonzero truth semantics,
and compare the factor plus provider-driven voltage/filter output.
Response.copy_concrete_context_settings accepts optional raw words only when
both receiver types are established by the caller. It converts nonzero to true
and preserves unknown inputs. This removes the injected query-result stub from
the reference without claiming automatic context discovery or flight-mode mapping.

The fresh base-context constructor is now checked separately: its zeroing prefix
sets response +0x38 to zero, its ownership fragment unconditionally assigns
+0x34 to self, then both original virtual queries read the same +0xB4 field.
The pinned main getter supplies General; nonzero flags copy it, zero flags retain
the constructor zero. 20 such cases extend the reference/comparison to 100 total.
Response.constructed_self_context implements this explicit fresh path. Unknown
flags or admitted missing settings do not imply a nominal value. This proves
initial self ownership only; later pointer changes and the flag's semantic
flight-mode identity remain unverified. The reference runs bounded constructor
fragments, not every unrelated constructor allocation/service.

Fresh reference cases also poison the flag field and execute its original
constructor argument-copy fragment. The third raw argument is stored unchanged
at +0xB4 before the self-link/query/copy/voltage phases. The first pointer argument
is independently checked at +0x64 to guard stack layout. This proves argument
provenance and nonzero truth handling; it does not identify the argument as a
player/AI flag or any named flight mode.

The response reference additionally exercises base/general-aircraft/airplane/
helicopter context vtables, whose original query slot resolves to the same
+0xB4 getter. This guards against a category-specific query override being missed
by the general-context fixture. 340 bounded cases include the 20 fresh constructor
cases and all factor/filter/voltage outputs match the port. This is query
inheritance evidence only: helicopter electrical dynamics and full category
factory execution remain outside the supported single-piston model. The response
comparator accepts only those four pinned vtable identities, at most 400 cases.
