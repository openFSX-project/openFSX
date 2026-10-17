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
| MATE | 120-byte records; diffuse RGB for solid rendering |
| INDE | Little-endian u16 indices local to a part's vertex range |
| VERT | 32-byte records; position in first three f32 values |
| SKIN | Buffer reference marking unsupported skinned parts |
| TRAN | 16 f32 static transform values |
| AMAP | Node type and TRAN reference |
| SCEN | Child, peer and AMAP byte offset; cycles rejected |
| LODE | Numeric LOD followed by PART chunks |
| PART | 36-byte triangle-list part and validated buffer ranges |

DirectX row-vector matrices are interpreted as transposed column-vector matrices with the same scalar storage. Child transforms compose with parents. Face normals are recomputed after transforming triangles, avoiding incorrect normal transforms under nonuniform scale.

Animated nodes use static matrices, not evaluated keys. Skinned parts are omitted and counted. Triangle fans/strips, unreachable scene nodes and unsupported format generations are rejected explicitly.

## Next work

Diffuse texture decoding and lookup, material transparency and tested bind-pose skinning. Back each interpretation with synthetic fixtures and local observations; do not silently reinterpret unknown layouts.
