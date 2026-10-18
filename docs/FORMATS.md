# Format notes

## Evidence

This independent loader uses observed local data and the research published by FSDeveloper contributors: [FSX MDL format](https://www.fsdeveloper.com/wiki/index.php/MDL_file_format_%28FSX%29). Their incomplete research is a starting point for experiments, not a full specification.

No third-party importer implementation was copied. Tests create small original triangle files; original game assets stay in the local installation.

## MDLX subset

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
read-only numeric RPN subset evaluates supported A-variable expressions into
authored frame positions; results clamp to the clip length. Unknown syntax,
local variables, writes, missing values and invalid results keep the clip in
bind pose with diagnostics in static inspection. The temporal PoseState API
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
