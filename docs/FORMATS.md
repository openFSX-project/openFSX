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
at 312. Callbacks and the first sequence record are bounded within the PE image;
the selection sequence is not traversed. Face names require a terminator and
explicit encoding. Zero font height remains authored automatic sizing.

The three MODULE_VAR values are unions. The separate native string probe
requires f64 or u32 payload interpretation for every slot; it does not resolve
token types, BCD units, source callbacks or live simulator state. Its guarded
ASCII output and explicit lifecycle checks are described in the helper README.
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
font, automatic height, ANSI/default charset and bounded single-line ASCII.
It maps authored foreground/background COLORREFs and horizontal/vertical text
alignment, clips glyph coverage, and leaves transparent backgrounds clear.
The callback result is literal text, never parsed as GaugeString or RPN.
The [published gauge API](https://www.prepar3d.com/SDKv6/sdk/panels_and_user_interface/gauges/references/gauges_api_overview.html)
defines automatic sizing by fitting NUMCHARS capital Ws into SIZE_X; fontdue
advance/kerning metrics implement that width rule. GDI cell metrics, hinting
and FSX pixels remain unverified. Nonzero fixed font height and unsupported
font/effect flags are rejected. Selection tables may be present, but this API
explicitly renders the unselected authored style; it does not run selection
sequences, highlight events, runtime user colors or lighting transitions.

The panel compositor selects strings by RVA, requires a zero-origin static
root and explicit gauge/panel extents, and scales the string position/size by
the root bitmap's intrinsic dimensions. Next-list predecessors affect drawing
order/visibility, not spatial parenting. Images use bounded source-over blending.
No DLL, font, callback result or bitmap from the installed game is distributed.

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
