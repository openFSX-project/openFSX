//! Scenery object placements (section 0x25 LibraryObject records) and model
//! libraries (section 0x2B: GUID index followed by embedded RIFF MDLX data).
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const SECTION_OBJECTS: u32 = 0x25;
const SECTION_MODELS: u32 = 0x2B;
const RECORD_LIBRARY_OBJECT: u16 = 0x0B;
const RECORD_GENERIC_BUILDING: u16 = 0x0A;

pub type Guid = [u8; 16];

#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub latitude: f64,
    pub longitude: f64,
    /// Metres; above ground level when `agl`, else above mean sea level.
    pub altitude: f64,
    pub agl: bool,
    /// Degrees.
    pub pitch: f64,
    pub bank: f64,
    pub heading: f64,
    pub model: Guid,
    pub scale: f64,
}
/// Generic building (record 0x0A): a box of `size` metres (x across, y up,
/// z along the heading), optionally with a pitched roof.
#[derive(Debug, Clone, PartialEq)]
pub struct GenericBuilding {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f64,
    pub agl: bool,
    pub heading: f64,
    pub size: [f64; 3],
    pub pitched_roof: bool,
}
fn u16_at(d: &[u8], i: usize) -> Option<u16> {
    d.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}
fn u32_at(d: &[u8], i: usize) -> Option<u32> {
    d.get(i..i + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn angle(raw: u16) -> f64 {
    f64::from(raw) * 360. / 65536.
}
/// Library object placements in one BGL file.
pub fn placements(d: &[u8]) -> Vec<Placement> {
    objects(d).0
}
/// Library object placements and generic buildings in one BGL file.
pub fn objects(d: &[u8]) -> (Vec<Placement>, Vec<GenericBuilding>) {
    let mut out = Vec::new();
    let mut out_buildings = Vec::new();
    if u32_at(d, 0) != Some(0x1992_0201) {
        return (out, out_buildings);
    }
    let sections = u32_at(d, 0x14).unwrap_or(0).min(4096) as usize;
    for s in 0..sections {
        let h = 0x38 + s * 20;
        if u32_at(d, h) != Some(SECTION_OBJECTS) {
            continue;
        }
        let (Some(count), Some(table)) = (u32_at(d, h + 8), u32_at(d, h + 12)) else {
            continue;
        };
        for k in 0..(count as usize).min(1 << 16) {
            let sub = table as usize + k * 16;
            let (Some(start), Some(size)) = (u32_at(d, sub + 8), u32_at(d, sub + 12)) else {
                break;
            };
            let end = (start as usize).saturating_add(size as usize).min(d.len());
            let mut p = start as usize;
            while p + 4 <= end {
                let (id, len) = (
                    u16_at(d, p).unwrap_or(0),
                    usize::from(u16_at(d, p + 2).unwrap_or(0)),
                );
                if len < 4 || p + len > end {
                    break;
                }
                if id == RECORD_GENERIC_BUILDING && len >= 52 + 28 {
                    let r = &d[p..p + len];
                    // Building section at 48: u16 code, u16 size, then the
                    // type, footprint and the base/window/top band heights.
                    let w = |i: usize| f64::from(u16_at(r, 52 + 2 * i).unwrap_or(0));
                    let scale = f64::from(f32::from_bits(u32_at(r, 44).unwrap_or(0)));
                    let scale = if scale.is_finite() && scale > 0. {
                        scale
                    } else {
                        1.
                    };
                    let height = w(4) + w(8) + w(13);
                    if w(1) > 0. && w(2) > 0. && height > 0. && height < 500. {
                        out_buildings.push(GenericBuilding {
                            longitude: crate::longitude(u32_at(r, 4).unwrap_or(0)),
                            latitude: crate::latitude(u32_at(r, 8).unwrap_or(0)),
                            altitude: f64::from(u32_at(r, 12).unwrap_or(0) as i32) / 1000.,
                            agl: u16_at(r, 16).unwrap_or(0) & 1 != 0,
                            heading: angle(u16_at(r, 22).unwrap_or(0)),
                            size: [w(1) * scale, height * scale, w(2) * scale],
                            pitched_roof: w(0) != 4.,
                        });
                    }
                }
                if id == RECORD_LIBRARY_OBJECT && len >= 64 {
                    let r = &d[p..p + len];
                    let scale = f64::from(f32::from_bits(u32_at(r, 60).unwrap_or(0)));
                    out.push(Placement {
                        longitude: crate::longitude(u32_at(r, 4).unwrap_or(0)),
                        latitude: crate::latitude(u32_at(r, 8).unwrap_or(0)),
                        altitude: f64::from(u32_at(r, 12).unwrap_or(0) as i32) / 1000.,
                        agl: u16_at(r, 16).unwrap_or(0) & 1 != 0,
                        pitch: angle(u16_at(r, 18).unwrap_or(0)),
                        bank: angle(u16_at(r, 20).unwrap_or(0)),
                        heading: angle(u16_at(r, 22).unwrap_or(0)),
                        model: r[44..60].try_into().unwrap(),
                        scale: if scale.is_finite() && scale > 0. {
                            scale
                        } else {
                            1.
                        },
                    });
                }
                p += len;
            }
        }
    }
    (out, out_buildings)
}
#[derive(Debug, Clone)]
pub struct ModelEntry {
    pub file: PathBuf,
    pub offset: u64,
    pub size: u32,
}
/// GUID index of every model library under Scenery, reading only headers and
/// index tables (models are read on demand).
pub fn model_index(installation: &Path) -> HashMap<Guid, ModelEntry> {
    let mut out = HashMap::new();
    let mut stack = vec![installation.join("Scenery")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("bgl"))
            {
                let _ = index_models(&path, &mut out);
            }
        }
    }
    out
}
fn read_at(file: &mut std::fs::File, offset: u64, len: usize) -> std::io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut b = vec![0; len];
    file.read_exact(&mut b)?;
    Ok(b)
}
fn index_models(path: &Path, out: &mut HashMap<Guid, ModelEntry>) -> std::io::Result<()> {
    let mut f = std::fs::File::open(path)?;
    let header = read_at(&mut f, 0, 0x38)?;
    if u32_at(&header, 0) != Some(0x1992_0201) {
        return Ok(());
    }
    let sections = u32_at(&header, 0x14).unwrap_or(0).min(4096) as usize;
    let table = read_at(&mut f, 0x38, sections * 20)?;
    for s in 0..sections {
        let h = s * 20;
        if u32_at(&table, h) != Some(SECTION_MODELS) {
            continue;
        }
        let count = u32_at(&table, h + 8).unwrap_or(0).min(4096) as usize;
        let subs = read_at(
            &mut f,
            u64::from(u32_at(&table, h + 12).unwrap_or(0)),
            count * 16,
        )?;
        for k in 0..count {
            let entries = u32_at(&subs, k * 16 + 4).unwrap_or(0).min(65536) as usize;
            let start = u64::from(u32_at(&subs, k * 16 + 8).unwrap_or(0));
            let index = read_at(&mut f, start, entries * 24)?;
            for e in index.chunks_exact(24) {
                out.insert(
                    e[..16].try_into().unwrap(),
                    ModelEntry {
                        file: path.to_path_buf(),
                        offset: start + u64::from(u32_at(e, 16).unwrap_or(0)),
                        size: u32_at(e, 20).unwrap_or(0),
                    },
                );
            }
        }
    }
    Ok(())
}
impl ModelEntry {
    /// The embedded RIFF MDLX bytes.
    pub fn read(&self) -> Result<Vec<u8>, String> {
        if self.size > 64 << 20 {
            return Err("Embedded model exceeds 64 MiB".into());
        }
        let mut f = std::fs::File::open(&self.file).map_err(|e| e.to_string())?;
        let b = read_at(&mut f, self.offset, self.size as usize).map_err(|e| e.to_string())?;
        if b.get(..4) != Some(b"RIFF") {
            return Err("Library entry is not RIFF model data".into());
        }
        Ok(b)
    }
}
/// Placements in the scenery tiles (and named scenery areas) around a point.
pub fn placements_near(
    installation: &Path,
    latitude: f64,
    longitude: f64,
    radius: f64,
) -> Vec<Placement> {
    objects_near(installation, latitude, longitude, radius).0
}
/// Library placements and generic buildings around a point.
pub fn objects_near(
    installation: &Path,
    latitude: f64,
    longitude: f64,
    radius: f64,
) -> (Vec<Placement>, Vec<GenericBuilding>) {
    let mut buildings = Vec::new();
    let dlat = (radius / 6_371_000.).to_degrees();
    let dlon = dlat / latitude.to_radians().cos().max(0.05);
    let mut out = Vec::new();
    let mut stack = vec![installation.join("Scenery")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            // Elevation, land/water class and vector files never hold objects.
            if !name.ends_with(".bgl")
                || name.starts_with("dem")
                || name.starts_with("cvx")
                || name.starts_with("world")
            {
                continue;
            }
            let Ok(data) = std::fs::read(&path) else {
                continue;
            };
            let near = |lat: f64, lon: f64| {
                (lat - latitude).abs() < dlat && (lon - longitude).abs() < dlon
            };
            let (library, generic) = objects(&data);
            out.extend(
                library
                    .into_iter()
                    .filter(|p| near(p.latitude, p.longitude)),
            );
            buildings.extend(
                generic
                    .into_iter()
                    .filter(|b| near(b.latitude, b.longitude)),
            );
        }
    }
    (out, buildings)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_library_object_record() {
        let mut d = vec![0u8; 0x38];
        d[0..4].copy_from_slice(&0x1992_0201u32.to_le_bytes());
        d[0x14..0x18].copy_from_slice(&1u32.to_le_bytes());
        for v in [SECTION_OBJECTS, 1, 1, 0x4C, 16] {
            d.extend(v.to_le_bytes());
        }
        for v in [0u32, 1, 0x5C, 64] {
            d.extend(v.to_le_bytes());
        }
        let mut r = vec![0u8; 64];
        r[0..2].copy_from_slice(&RECORD_LIBRARY_OBJECT.to_le_bytes());
        r[2..4].copy_from_slice(&64u16.to_le_bytes());
        r[4..8].copy_from_slice(&0x0800_0000u32.to_le_bytes());
        r[8..12].copy_from_slice(&0x1000_0000u32.to_le_bytes());
        r[12..16].copy_from_slice(&5000i32.to_le_bytes());
        r[16..18].copy_from_slice(&1u16.to_le_bytes());
        r[22..24].copy_from_slice(&16384u16.to_le_bytes());
        r[44] = 7;
        r[60..64].copy_from_slice(&2f32.to_le_bytes());
        d.extend(r);
        let p = &placements(&d)[0];
        assert_eq!(
            (p.latitude, p.longitude, p.altitude, p.agl),
            (0., -120., 5., true)
        );
        assert_eq!((p.heading, p.scale, p.model[0]), (90., 2., 7));
        assert!(placements(&d[..d.len() - 10]).is_empty());
    }
    #[test]
    fn installed_ksea_objects_resolve_to_models_when_present() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../FSX");
        if !root.join("Scenery").is_dir() {
            return;
        }
        let index = model_index(&root);
        assert!(index.len() > 10_000);
        let near = placements_near(&root, 47.449, -122.309, 3000.);
        let resolved = near.iter().filter(|p| index.contains_key(&p.model)).count();
        assert!(
            near.len() > 20 && resolved * 2 > near.len(),
            "{resolved}/{}",
            near.len()
        );
        let model = index[&near
            .iter()
            .find(|p| index.contains_key(&p.model))
            .unwrap()
            .model]
            .read();
        assert!(model.is_ok());
    }
}
