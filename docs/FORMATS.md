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

Animated nodes use static matrices, not evaluated keys. Skinned parts are omitted and counted. Triangle fans/strips, unreachable scene nodes and unsupported format generations are rejected explicitly.

## DDS and texture lookup

The legacy DDS header and BC1/BC2/BC3 definitions follow Microsoft's [DDS guide](https://learn.microsoft.com/en-us/windows/win32/direct3ddds/dx-graphics-dds-pguide) and [block compression reference](https://learn.microsoft.com/en-us/windows/win32/direct3d10/d3d10-graphics-programming-guide-resources-block-compression). DXT1 selects its endpoint-order-dependent three/four-color palette. DXT3 always uses the four-color palette and explicit four-bit alpha. DXT5 uses the two endpoint-dependent alpha palettes and three-bit alpha selectors. Partial edge blocks are clipped to the declared dimensions.

Uncompressed 32-bit RGB uses validated nonoverlapping contiguous channel masks and respects row pitch. Only the top mip is decoded. DDS rows and MDLX UVs are preserved without a vertical flip; this mapping was checked against the local C172 exterior. GPU diffuse textures use an sRGB format. This does not establish full FSX lighting or material parity.

texture.cfg is read only for fltsim fallback entries, in numeric order. Relative Windows separators are normalized and canonical directory paths prevent cycles. Limits apply to file sizes, decoded pixels and directory traversal. No full content filesystem or external SimObjects resolver is claimed.

## Next work

Tested bind-pose skinning, visibility/animation evaluation, material parity and generated registration textures. Back each interpretation with synthetic fixtures and local observations; do not silently reinterpret unknown layouts.

## AIR subset

Legacy AIR files are sequences of a little-endian u32 identifier and u32 payload length followed by packed bytes. Record labels such as 1101 and 404 are hexadecimal: their numeric IDs are 0x1101 and 0x0404. Bounds, duplicate IDs and record counts are checked; input is limited to 4 MiB.

Curve payloads contain a u32 point count followed by pairs of f64 angle/value entries. Angles must be finite and strictly increasing. The current reader uses linear interpolation and clamps outside the endpoint domain. It loads lift versus body AoA from 0x0404 and pitch moment versus AoA from 0x0473, plus selected signed integer and double coefficients from the 250-byte 0x1101 layout. Integer aerodynamic derivatives use the documented 1/2048 scale. Modern override records are rejected until their precedence is implemented.

The independent reader was checked against field definitions distributed with AirEd and the local C172 data. Reference: [aircraft dynamics resources](https://www.aero.sors.fr/fsairfile.html). Source definitions and game data are not included. The [flight guide](FLIGHT.md) documents coordinate signs, units, approximations and numerical evidence; decoding these records does not establish complete FSX flight-model behavior.
