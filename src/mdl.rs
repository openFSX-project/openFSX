//! Independently implemented, bounds-checked subset of the FSX MDLX container.
//! No animation evaluation, visibility expressions, or skin deformation yet.
use crate::math::{self, Mat4, IDENTITY};
use std::{collections::BTreeMap, fs, path::Path};

type Result<T> = std::result::Result<T, String>;
const MAX_FILE_SIZE: u64 = 256 * 1024 * 1024;
const MAX_OUTPUT_VERTICES: usize = 3_000_000;

#[derive(Debug)]
pub struct Model {
    pub name: String,
    pub textures: Vec<String>,
    pub lods: BTreeMap<u32, Vec<Part>>,
    buffers: Vec<Buffer>,
    indices: Vec<u16>,
    matrices: Vec<Mat4>,
    pub animated_nodes: usize,
    materials: Vec<[f32; 4]>,
}
#[derive(Debug)]
struct Buffer {
    positions: Vec<[f32; 3]>,
    skinned: bool,
}
#[derive(Debug)]
pub struct Part {
    pub scene: usize,
    pub material: usize,
    pub buffer: usize,
    pub vertex_offset: usize,
    pub vertex_count: usize,
    pub index_offset: usize,
    pub index_count: usize,
}
#[derive(Debug)]
pub struct Geometry {
    /// Interleaved position, face normal and diffuse RGB; one vertex per corner.
    pub vertices: Vec<[f32; 9]>,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub skipped_skinned_parts: usize,
    pub lod: u32,
}

fn u32_at(b: &[u8], i: usize) -> Result<u32> {
    b.get(i..i + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| "Truncated u32".into())
}
fn f32_at(b: &[u8], i: usize) -> Result<f32> {
    let value = f32::from_bits(u32_at(b, i)?);
    if !value.is_finite() {
        return Err("Non-finite floating-point data".into());
    }
    Ok(value)
}
fn records(b: &[u8], stride: usize) -> Result<std::slice::ChunksExact<'_, u8>> {
    if b.len() % stride != 0 {
        return Err(format!(
            "Record size {} is not divisible by {stride}",
            b.len()
        ));
    }
    Ok(b.chunks_exact(stride))
}
fn chunks(mut b: &[u8]) -> Result<Vec<([u8; 4], &[u8])>> {
    let mut out = Vec::new();
    while !b.is_empty() {
        if b.len() < 8 {
            return Err("Truncated chunk header".into());
        }
        let id = b[..4].try_into().unwrap();
        let n = u32_at(b, 4)? as usize;
        // MDLX chunks are packed: unlike generic RIFF, odd lengths have no pad byte.
        let end = 8usize.checked_add(n).ok_or("Chunk length overflow")?;
        let data = b.get(8..end).ok_or("Chunk extends beyond its parent")?;
        out.push((id, data));
        b = &b[end..];
    }
    Ok(out)
}
fn single<'a>(items: &[([u8; 4], &'a [u8])], id: &[u8; 4]) -> Result<&'a [u8]> {
    let mut matches = items.iter().filter(|(tag, _)| tag == id);
    let data = matches
        .next()
        .map(|(_, data)| *data)
        .ok_or_else(|| format!("Missing {} chunk", String::from_utf8_lossy(id)))?;
    if matches.next().is_some() {
        return Err(format!("Duplicate {} chunk", String::from_utf8_lossy(id)));
    }
    Ok(data)
}
fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b.split(|&x| x == 0).next().unwrap_or(b))
        .trim()
        .to_owned()
}

impl Model {
    pub fn load(path: &Path) -> Result<Self> {
        let len = fs::metadata(path).map_err(|e| e.to_string())?.len();
        if len > MAX_FILE_SIZE {
            return Err("MDL exceeds the 256 MiB safety limit".into());
        }
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        Self::parse(&bytes)
    }
    pub fn parse(b: &[u8]) -> Result<Self> {
        if b.len() as u64 > MAX_FILE_SIZE {
            return Err("MDL exceeds the 256 MiB safety limit".into());
        }
        if b.len() < 12 || &b[..4] != b"RIFF" || &b[8..12] != b"MDLX" {
            return Err("Expected an FSX RIFF/MDLX file (MDL8/MDL9/P3D are not supported)".into());
        }
        if u32_at(b, 4)? as usize != b.len() - 8 {
            return Err("RIFF length does not match file length".into());
        }
        let root = chunks(&b[12..])?;
        let name = root
            .iter()
            .find(|(tag, _)| tag == b"MDLN")
            .map(|(_, data)| text(data))
            .unwrap_or_default();
        let data = chunks(single(&root, b"MDLD")?)?;
        let textures = records(single(&data, b"TEXT")?, 64)?.map(text).collect();
        let materials = records(single(&data, b"MATE")?, 120)?
            .map(|r| {
                Ok([
                    f32_at(r, 36)?,
                    f32_at(r, 40)?,
                    f32_at(r, 44)?,
                    f32_at(r, 48)?,
                ])
            })
            .collect::<Result<Vec<_>>>()?;
        let indices = records(single(&data, b"INDE")?, 2)?
            .map(|r| u16::from_le_bytes(r.try_into().unwrap()))
            .collect();
        let mut buffers: Vec<Buffer> = Vec::new();
        for (id, bytes) in chunks(single(&data, b"VERB")?)? {
            if &id == b"VERT" {
                let positions = records(bytes, 32)?
                    .map(|r| Ok([f32_at(r, 0)?, f32_at(r, 4)?, f32_at(r, 8)?]))
                    .collect::<Result<Vec<_>>>()?;
                buffers.push(Buffer {
                    positions,
                    skinned: false,
                });
            } else if &id == b"SKIN" {
                if bytes.len() < 4 {
                    return Err("Truncated SKIN buffer reference".into());
                }
                let index = u32_at(bytes, 0)? as usize;
                buffers
                    .get_mut(index)
                    .ok_or("SKIN references a missing vertex buffer")?
                    .skinned = true;
            }
        }
        let transforms = records(single(&data, b"TRAN")?, 64)?
            .map(|r| {
                let mut m = [0.; 16];
                for (i, x) in m.iter_mut().enumerate() {
                    *x = f32_at(r, i * 4)?;
                }
                Ok(m)
            })
            .collect::<Result<Vec<_>>>()?;
        let amap = records(single(&data, b"AMAP")?, 8)?
            .map(|r| Ok((u32_at(r, 0)?, u32_at(r, 4)? as usize)))
            .collect::<Result<Vec<_>>>()?;
        let nodes = records(single(&data, b"SCEN")?, 8)?
            .map(|r| {
                [
                    i16::from_le_bytes([r[0], r[1]]),
                    i16::from_le_bytes([r[2], r[3]]),
                    i16::from_le_bytes([r[4], r[5]]),
                ]
            })
            .collect::<Vec<_>>();
        if nodes.is_empty() || nodes.len() > 4096 {
            return Err("Unsupported scene node count".into());
        }
        let mut matrices = vec![IDENTITY; nodes.len()];
        let mut seen = vec![false; nodes.len()];
        let mut pending = vec![(0usize, IDENTITY)];
        let mut animated_nodes = 0;
        while let Some((index, parent)) = pending.pop() {
            let &[child, peer, offset] = nodes
                .get(index)
                .ok_or("Scene node reference out of range")?;
            if seen[index] {
                return Err("Scene graph contains a cycle or multiple parents".into());
            }
            seen[index] = true;
            if offset < 0 || offset % 8 != 0 {
                return Err("Invalid AMAP byte offset".into());
            }
            let &(kind, transform) = amap
                .get(offset as usize / 8)
                .ok_or("AMAP reference out of range")?;
            if kind != 1 && kind != 2 {
                return Err("Unknown node transform type".into());
            }
            animated_nodes += usize::from(kind == 2);
            matrices[index] = math::multiply(
                parent,
                *transforms
                    .get(transform)
                    .ok_or("TRAN reference out of range")?,
            );
            for (next, matrix) in [(peer, parent), (child, matrices[index])] {
                if next < -1 {
                    return Err("Invalid negative scene reference".into());
                }
                if next >= 0 {
                    pending.push((next as usize, matrix));
                }
            }
        }
        if seen.contains(&false) {
            return Err("Unreachable scene nodes are not supported".into());
        }
        let mut lods = BTreeMap::new();
        for (tag, entry) in chunks(single(&data, b"LODT")?)? {
            if &tag != b"LODE" {
                continue;
            }
            let level = u32_at(entry, 0)?;
            let mut parts = Vec::new();
            for (tag, part) in chunks(&entry[4..])? {
                if &tag != b"PART" {
                    continue;
                }
                if part.len() != 36 || u32_at(part, 0)? != 1 {
                    return Err("Only 36-byte triangle-list PART records are supported".into());
                }
                parts.push(Part {
                    scene: u32_at(part, 4)? as usize,
                    material: u32_at(part, 8)? as usize,
                    buffer: u32_at(part, 12)? as usize,
                    vertex_offset: u32_at(part, 16)? as usize,
                    vertex_count: u32_at(part, 20)? as usize,
                    index_offset: u32_at(part, 24)? as usize,
                    index_count: u32_at(part, 28)? as usize,
                });
            }
            if lods.insert(level, parts).is_some() {
                return Err("Duplicate LOD level".into());
            }
        }
        let model = Self {
            name,
            textures,
            lods,
            buffers,
            indices,
            matrices,
            animated_nodes,
            materials,
        };
        model.validate_parts()?;
        Ok(model)
    }

    fn validate_parts(&self) -> Result<()> {
        if self.lods.is_empty() {
            return Err("No LOD geometry".into());
        }
        for parts in self.lods.values() {
            for part in parts {
                let buffer = self
                    .buffers
                    .get(part.buffer)
                    .ok_or("PART vertex buffer reference out of range")?;
                if part.scene >= self.matrices.len() || part.material >= self.materials.len() {
                    return Err("PART scene/material reference out of range".into());
                }
                let end = part
                    .vertex_offset
                    .checked_add(part.vertex_count)
                    .ok_or("PART vertex range overflow")?;
                if end > buffer.positions.len() {
                    return Err("PART vertex range out of bounds".into());
                }
                let end = part
                    .index_offset
                    .checked_add(part.index_count)
                    .ok_or("PART index range overflow")?;
                let indices = self
                    .indices
                    .get(part.index_offset..end)
                    .ok_or("PART index range out of bounds")?;
                if indices.len() % 3 != 0
                    || indices.iter().any(|&i| i as usize >= part.vertex_count)
                {
                    return Err("PART triangle index outside its local vertex range".into());
                }
            }
        }
        Ok(())
    }

    pub fn geometry(&self, requested: Option<u32>) -> Result<Geometry> {
        let lod = requested.unwrap_or(*self.lods.keys().next_back().ok_or("No LODs")?);
        let parts = self
            .lods
            .get(&lod)
            .ok_or_else(|| format!("LOD {lod} not present"))?;
        let total = parts
            .iter()
            .filter(|part| !self.buffers[part.buffer].skinned)
            .try_fold(0usize, |sum, part| {
                sum.checked_add(part.index_count)
                    .ok_or("Geometry vertex count overflow")
            })?;
        if total > MAX_OUTPUT_VERTICES {
            return Err("Decoded geometry exceeds the 3 million vertex limit".into());
        }
        let mut vertices = Vec::with_capacity(total);
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        let mut skipped_skinned_parts = 0;
        for part in parts {
            let buffer = &self.buffers[part.buffer];
            if buffer.skinned {
                skipped_skinned_parts += 1;
                continue;
            }
            let matrix = self.matrices[part.scene];
            let color = self.materials[part.material];
            for triangle in self.indices[part.index_offset..part.index_offset + part.index_count]
                .chunks_exact(3)
            {
                let points: [[f32; 3]; 3] = std::array::from_fn(|i| {
                    math::transform(
                        matrix,
                        buffer.positions[part.vertex_offset + triangle[i] as usize],
                    )
                });
                if points.iter().flatten().any(|x| !x.is_finite()) {
                    return Err("Transformed geometry is non-finite".into());
                }
                let normal = math::normalized(math::cross(
                    math::sub(points[1], points[0]),
                    math::sub(points[2], points[0]),
                ));
                for point in points {
                    for i in 0..3 {
                        min[i] = min[i].min(point[i]);
                        max[i] = max[i].max(point[i]);
                    }
                    vertices.push([
                        point[0], point[1], point[2], normal[0], normal[1], normal[2], color[0],
                        color[1], color[2],
                    ]);
                }
            }
        }
        if vertices.is_empty() {
            return Err("Selected LOD has no supported unskinned triangles".into());
        }
        Ok(Geometry {
            vertices,
            min,
            max,
            skipped_skinned_parts,
            lod,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
        [id.as_slice(), &(data.len() as u32).to_le_bytes(), data].concat()
    }
    fn model_bytes(index: u16) -> Vec<u8> {
        let floats = |values: &[f32]| {
            values
                .iter()
                .flat_map(|x| x.to_le_bytes())
                .collect::<Vec<_>>()
        };
        let positions = [
            0., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 0., 0., 1., 0., 1., 0.,
            0., 0.,
        ];
        let part = [1u32, 0, 0, 0, 0, 3, 0, 3, u32::MAX]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect::<Vec<_>>();
        let data = [
            chunk(b"TEXT", &[]),
            chunk(b"MATE", &[0; 120]),
            chunk(b"INDE", &[0, 0, 1, 0, index as u8, (index >> 8) as u8]),
            chunk(b"VERB", &chunk(b"VERT", &floats(&positions))),
            chunk(b"TRAN", &floats(&IDENTITY)),
            chunk(b"AMAP", &[1, 0, 0, 0, 0, 0, 0, 0]),
            chunk(b"SCEN", &[255, 255, 255, 255, 0, 0, 0, 0]),
            chunk(
                b"LODT",
                &chunk(
                    b"LODE",
                    &[100u32.to_le_bytes().as_slice(), &chunk(b"PART", &part)].concat(),
                ),
            ),
        ]
        .concat();
        chunk(
            b"RIFF",
            &[
                b"MDLX".as_slice(),
                &chunk(b"MDLN", b"own\0"),
                &chunk(b"MDLD", &data),
            ]
            .concat(),
        )
    }
    #[test]
    fn own_triangle_and_bad_index() {
        let model = Model::parse(&model_bytes(2)).unwrap();
        assert_eq!(model.geometry(None).unwrap().vertices.len(), 3);
        assert!(Model::parse(&model_bytes(3))
            .unwrap_err()
            .contains("local vertex"));
    }
    #[test]
    fn every_truncation_fails_without_panic() {
        let bytes = model_bytes(2);
        for n in 0..bytes.len() {
            assert!(Model::parse(&bytes[..n]).is_err());
        }
        let mut bytes = bytes;
        bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Model::parse(&bytes).is_err());
    }
    #[test]
    fn packed_odd_chunks_and_parent_bounds() {
        let bytes = [chunk(b"ODD!", &[1]), chunk(b"NEXT", &[])].concat();
        assert_eq!(chunks(&bytes).unwrap().len(), 2);
        assert!(chunks(&bytes[..bytes.len() - 1]).is_err());
    }
    #[test]
    fn cycles_and_invalid_scene_offsets_are_rejected() {
        let original = model_bytes(2);
        let scene = original.windows(4).position(|s| s == b"SCEN").unwrap() + 8;
        let mut cyclic = original.clone();
        cyclic[scene..scene + 2].copy_from_slice(&0i16.to_le_bytes());
        assert!(Model::parse(&cyclic).unwrap_err().contains("cycle"));
        let mut invalid = original;
        invalid[scene + 4..scene + 6].copy_from_slice(&1i16.to_le_bytes());
        assert!(Model::parse(&invalid).unwrap_err().contains("AMAP"));
    }
}
