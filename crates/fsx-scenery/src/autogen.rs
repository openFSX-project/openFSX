//! FSX autogen annotations (`*.agn`, RIFF "AGNX"), one per land texture tile.
//! Coordinates are fractions of the tile in -0.5..0.5 (x east, z north).
//! Decoded: rectangular buildings (GBLD/GBLR) and vegetation polygons
//! (VGRG/VGRE). Rows of houses, building class GUIDs and heights are not.

/// A rectangular building footprint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Building {
    pub center: [f32; 2],
    /// Unit direction of the building's x axis.
    pub direction: [f32; 2],
    pub half_size: [f32; 2],
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Annotation {
    pub buildings: Vec<Building>,
    pub vegetation: Vec<Vec<[f32; 2]>>,
}
fn f32_at(d: &[u8], i: usize) -> f32 {
    d.get(i..i + 4)
        .map_or(0., |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn walk(d: &[u8], mut o: usize, end: usize, out: &mut Annotation, depth: u32) {
    while o + 8 <= end && depth < 8 {
        let tag = &d[o..o + 4];
        let size = u32::from_le_bytes([d[o + 4], d[o + 5], d[o + 6], d[o + 7]]) as usize;
        let body_end = (o + 8).saturating_add(size).min(end);
        let body = &d[o + 8..body_end];
        match tag {
            b"GBLD" | b"VGRG" => walk(d, o + 8, body_end, out, depth + 1),
            b"GBLR" if body.len() >= 16 => {
                for e in body[16..].chunks_exact(24) {
                    let v: [f32; 6] = std::array::from_fn(|i| f32_at(e, i * 4));
                    let len = (v[2] * v[2] + v[3] * v[3]).sqrt();
                    if v.iter().all(|x| x.is_finite())
                        && len > 0.5
                        && v[0].abs() <= 0.6
                        && v[1].abs() <= 0.6
                    {
                        out.buildings.push(Building {
                            center: [v[0], v[1]],
                            direction: [v[2] / len, v[3] / len],
                            half_size: [v[4].abs().max(0.002), v[5].abs().max(0.002)],
                        });
                    }
                }
            }
            b"VGRE" if body.len() >= 16 + 24 => {
                let points: Vec<[f32; 2]> = body[16..]
                    .chunks_exact(8)
                    .map(|p| [f32_at(p, 0), f32_at(p, 4)])
                    .filter(|p| p.iter().all(|x| x.is_finite() && x.abs() <= 1.))
                    .collect();
                if points.len() >= 3 {
                    out.vegetation.push(points);
                }
            }
            _ => {}
        }
        o = body_end + (size & 1);
    }
}
pub fn parse(d: &[u8]) -> Result<Annotation, String> {
    if d.get(0..4) != Some(b"RIFF") || d.get(8..12) != Some(b"AGNX") {
        return Err("Not an AGNX autogen annotation".into());
    }
    let mut out = Annotation::default();
    walk(d, 12, d.len(), &mut out, 0);
    Ok(out)
}
/// Point-in-polygon (even-odd rule).
pub fn contains(polygon: &[[f32; 2]], p: [f32; 2]) -> bool {
    let mut inside = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let (a, b) = (polygon[i], polygon[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}
#[cfg(test)]
mod tests {
    use super::*;
    fn chunk(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut v = tag.to_vec();
        v.extend((body.len() as u32).to_le_bytes());
        v.extend(body);
        v
    }
    #[test]
    fn decodes_buildings_and_vegetation_polygons() {
        let mut gblr = vec![0u8; 16];
        for x in [0.1f32, -0.2, 0., 1., 0.01, -0.02] {
            gblr.extend(x.to_le_bytes());
        }
        let mut vgre = vec![0u8; 16];
        for x in [-0.5f32, -0.5, 0.5, -0.5, 0.5, 0.5] {
            vgre.extend(x.to_le_bytes());
        }
        let mut body = b"AGNX".to_vec();
        body.extend(chunk(b"GBLD", &chunk(b"GBLR", &gblr)));
        body.extend(chunk(b"VGRG", &chunk(b"VGRE", &vgre)));
        let d = chunk(b"RIFF", &body);
        let a = parse(&d).unwrap();
        assert_eq!(a.buildings[0].half_size, [0.01, 0.02]);
        assert_eq!(a.buildings[0].direction, [0., 1.]);
        assert_eq!(a.vegetation[0].len(), 3);
        assert!(
            contains(&a.vegetation[0], [0.4, -0.3]) && !contains(&a.vegetation[0], [-0.4, 0.3])
        );
        assert!(parse(b"RIFF\0\0\0\0WAVE").is_err());
    }
    #[test]
    fn installed_annotations_parse_when_present() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../FSX/Scenery/World/texture");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        let (mut files, mut buildings, mut vegetation) = (0, 0, 0);
        for e in entries.flatten() {
            if e.file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with(".agn")
            {
                // A few region-X files use an older non-RIFF layout and are skipped.
                let Ok(a) = parse(&std::fs::read(e.path()).unwrap()) else {
                    continue;
                };
                files += 1;
                buildings += a.buildings.len();
                vegetation += a.vegetation.len();
            }
        }
        assert!(
            files > 800 && buildings > 50_000 && vegetation > 500,
            "{files} {buildings} {vegetation}"
        );
    }
}
