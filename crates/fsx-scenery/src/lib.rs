//! FSX BGL scenery: file/section layout and airport/runway records.
//! Only uncompressed airport data (section 0x03) is decoded; terrain
//! elevation, land class and object placement remain future work.
use std::path::{Path, PathBuf};

pub mod autogen;
pub mod catalog;
pub mod objects;
pub mod terrain;

type Result<T> = std::result::Result<T, String>;
const MAGIC: u32 = 0x1992_0201;
const SECTION_AIRPORT: u32 = 0x03;
const RECORD_AIRPORT: u16 = 0x3C;
const RECORD_AIRPORT_HEADER: usize = 0x38;
const RECORD_RUNWAY: u16 = 0x04;
const RECORD_NAME: u16 = 0x19;
const RECORD_TAXI_POINTS: u16 = 0x1A;
const RECORD_TAXI_PATHS: u16 = 0x1C;
const RECORD_APRON: u16 = 0x37;

fn u8_at(d: &[u8], i: usize) -> Result<u8> {
    d.get(i).copied().ok_or_else(|| "Truncated BGL".into())
}
fn u16_at(d: &[u8], i: usize) -> Result<u16> {
    d.get(i..i + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| "Truncated BGL".into())
}
fn u32_at(d: &[u8], i: usize) -> Result<u32> {
    d.get(i..i + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "Truncated BGL".into())
}
fn f32_at(d: &[u8], i: usize) -> Result<f64> {
    Ok(f64::from(f32::from_bits(u32_at(d, i)?)))
}
pub fn longitude(raw: u32) -> f64 {
    f64::from(raw) * 360. / f64::from(3 * 0x1000_0000u32) - 180.
}
pub fn latitude(raw: u32) -> f64 {
    90. - f64::from(raw) * 180. / f64::from(2 * 0x1000_0000u32)
}
/// Packed base-38 identifier (ICAO, runway ILS idents), low 5 bits unused.
pub fn ident(raw: u32) -> String {
    let mut v = raw >> 5;
    let mut s = Vec::new();
    while v > 0 {
        let c = (v % 38) as u8;
        v /= 38;
        s.push(match c {
            0 => b' ',
            2..=11 => b'0' + c - 2,
            12..=37 => b'A' + c - 12,
            _ => b'?',
        });
    }
    s.reverse();
    String::from_utf8_lossy(&s).trim().to_owned()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Runway {
    /// Primary end number (1..36) and designator code (0 none, 1 L, 2 R, 3 C, ...).
    pub primary: (u8, u8),
    pub secondary: (u8, u8),
    pub surface: u16,
    pub latitude: f64,
    pub longitude: f64,
    /// Metres above mean sea level.
    pub altitude: f64,
    pub length: f64,
    pub width: f64,
    /// True heading of the primary end, degrees.
    pub heading: f64,
}
impl Runway {
    pub fn name(&self) -> String {
        let end = |(n, d): (u8, u8)| {
            let suffix = match d {
                1 => "L",
                2 => "R",
                3 => "C",
                4 => "W",
                5 => "A",
                6 => "B",
                _ => "",
            };
            match n {
                1..=36 => format!("{n:02}{suffix}"),
                37 => "N".into(),
                38 => "NE".into(),
                39 => "E".into(),
                40 => "SE".into(),
                41 => "S".into(),
                42 => "SW".into(),
                43 => "W".into(),
                44 => "NW".into(),
                _ => format!("{n}{suffix}"),
            }
        };
        format!("{}/{}", end(self.primary), end(self.secondary))
    }
    /// Hard surfaces: concrete, asphalt, oil-treated, steel mats, bituminous,
    /// brick, macadam, planks and tarmac (FSX surface codes).
    pub fn hard(&self) -> bool {
        matches!(self.surface, 0 | 4 | 15 | 16 | 17 | 18 | 19 | 20 | 23)
    }
}
/// Taxiway path between two taxi points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TaxiPath {
    pub start: usize,
    pub end: usize,
    /// 1 taxi, 2 runway, 3 parking, 4 path, 5 closed, 6 vehicle.
    pub kind: u8,
    /// Metres.
    pub width: f64,
}
/// Apron outline (latitude, longitude) with a runway-style surface code.
#[derive(Debug, Clone, PartialEq)]
pub struct Apron {
    pub surface: u8,
    pub outline: Vec<(f64, f64)>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Airport {
    pub icao: String,
    pub name: Option<String>,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f64,
    pub runways: Vec<Runway>,
    /// Taxi points (latitude, longitude) referenced by `taxi_paths`.
    pub taxi_points: Vec<(f64, f64)>,
    pub taxi_paths: Vec<TaxiPath>,
    pub aprons: Vec<Apron>,
}
fn runway(d: &[u8], p: usize) -> Result<Runway> {
    Ok(Runway {
        surface: u16_at(d, p + 6)?,
        primary: (u8_at(d, p + 8)?, u8_at(d, p + 9)?),
        secondary: (u8_at(d, p + 10)?, u8_at(d, p + 11)?),
        longitude: longitude(u32_at(d, p + 20)?),
        latitude: latitude(u32_at(d, p + 24)?),
        altitude: f64::from(u32_at(d, p + 28)? as i32) / 1000.,
        length: f32_at(d, p + 32)?,
        width: f32_at(d, p + 36)?,
        heading: f32_at(d, p + 40)?,
    })
}
fn airport(d: &[u8], p: usize, size: usize) -> Result<Airport> {
    let mut a = Airport {
        longitude: longitude(u32_at(d, p + 12)?),
        latitude: latitude(u32_at(d, p + 16)?),
        altitude: f64::from(u32_at(d, p + 20)? as i32) / 1000.,
        icao: ident(u32_at(d, p + 0x28)?),
        name: None,
        runways: Vec::new(),
        taxi_points: Vec::new(),
        taxi_paths: Vec::new(),
        aprons: Vec::new(),
    };
    let end = p + size;
    let mut q = p + RECORD_AIRPORT_HEADER;
    while q + 6 <= end {
        let (id, len) = (u16_at(d, q)?, u32_at(d, q + 2)? as usize);
        if len < 6 || q + len > end {
            return Err(format!("Invalid airport subrecord at {q:#x}"));
        }
        match id {
            RECORD_RUNWAY if len >= 44 => a.runways.push(runway(d, q)?),
            RECORD_TAXI_POINTS if len >= 8 => {
                let count = usize::from(u16_at(d, q + 6)?);
                for k in 0..count.min((len - 8) / 12) {
                    let e = q + 8 + k * 12;
                    a.taxi_points
                        .push((latitude(u32_at(d, e + 8)?), longitude(u32_at(d, e + 4)?)));
                }
            }
            RECORD_TAXI_PATHS if len >= 8 => {
                let count = usize::from(u16_at(d, q + 6)?);
                for k in 0..count.min((len - 8) / 20) {
                    let e = q + 8 + k * 20;
                    let width = f32_at(d, e + 8)?;
                    a.taxi_paths.push(TaxiPath {
                        start: usize::from(u16_at(d, e)?),
                        end: usize::from(u16_at(d, e + 2)? & 0x0FFF),
                        kind: u8_at(d, e + 4)? & 0x0F,
                        width: if width.is_finite() && width > 0. {
                            width
                        } else {
                            0.
                        },
                    });
                }
            }
            RECORD_APRON if len >= 9 => {
                let count = usize::from(u16_at(d, q + 7)?);
                let outline = (0..count.min((len - 9) / 8))
                    .map(|k| {
                        let e = q + 9 + k * 8;
                        Ok((latitude(u32_at(d, e + 4)?), longitude(u32_at(d, e)?)))
                    })
                    .collect::<Result<Vec<_>>>()?;
                a.aprons.push(Apron {
                    surface: u8_at(d, q + 6)?,
                    outline,
                });
            }
            RECORD_NAME => {
                let raw = &d[q + 6..q + len];
                let text = raw.split(|&b| b == 0).next().unwrap_or(&[]);
                // Names are Windows-1252; keep ASCII exactly and map the rest.
                a.name = Some(text.iter().map(|&b| char::from(b)).collect());
            }
            _ => {}
        }
        q += len;
    }
    Ok(a)
}
/// Decode every airport record in one BGL file.
pub fn airports(d: &[u8]) -> Result<Vec<Airport>> {
    if u32_at(d, 0)? != MAGIC {
        return Err("Not an FSX BGL file".into());
    }
    let sections = u32_at(d, 0x14)? as usize;
    if sections > 4096 {
        return Err("Implausible BGL section count".into());
    }
    let mut out = Vec::new();
    for s in 0..sections {
        let h = 0x38 + s * 20;
        if u32_at(d, h)? != SECTION_AIRPORT {
            continue;
        }
        let (count, offset) = (u32_at(d, h + 8)? as usize, u32_at(d, h + 12)? as usize);
        for k in 0..count.min(65536) {
            let sub = offset + k * 16;
            let (start, size) = (u32_at(d, sub + 8)? as usize, u32_at(d, sub + 12)? as usize);
            let end = start.checked_add(size).filter(|&e| e <= d.len());
            let end = end.ok_or("BGL subsection exceeds file")?;
            let mut p = start;
            while p + 6 <= end {
                let (id, len) = (u16_at(d, p)?, u32_at(d, p + 2)? as usize);
                if len < 6 || p + len > end {
                    return Err(format!("Invalid airport record at {p:#x}"));
                }
                if id == RECORD_AIRPORT && len >= RECORD_AIRPORT_HEADER {
                    out.push(airport(d, p, len)?);
                }
                p += len;
            }
        }
    }
    Ok(out)
}
/// Airport BGLs under Scenery/*/scenery (APX files hold the stock airports).
pub fn airport_files(installation: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let scenery = installation.join("Scenery");
    for area in std::fs::read_dir(&scenery).map_err(|e| format!("{}: {e}", scenery.display()))? {
        let folder = area.map_err(|e| e.to_string())?.path().join("scenery");
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_ascii_uppercase();
            if name.starts_with("APX") && name.ends_with(".BGL") {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}
#[cfg(test)]
mod tests {
    use super::*;
    pub(crate) fn sample() -> Vec<u8> {
        let mut d = vec![0u8; 0x38];
        d[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        d[0x14..0x18].copy_from_slice(&1u32.to_le_bytes());
        // Section table, then one subsection header, then the record.
        let sub = 0x38 + 20;
        let data = sub + 16;
        let mut rec = vec![0u8; RECORD_AIRPORT_HEADER];
        rec[0..2].copy_from_slice(&RECORD_AIRPORT.to_le_bytes());
        rec[12..16].copy_from_slice(&0x0800_0000u32.to_le_bytes());
        rec[16..20].copy_from_slice(&0x1000_0000u32.to_le_bytes());
        rec[20..24].copy_from_slice(&100_000i32.to_le_bytes());
        // "KSEA" packed base-38.
        let code = "KSEA"
            .bytes()
            .fold(0u32, |v, c| v * 38 + u32::from(c - b'A' + 12));
        rec[0x28..0x2C].copy_from_slice(&(code << 5).to_le_bytes());
        let mut rwy = vec![0u8; 0x34];
        rwy[0..2].copy_from_slice(&RECORD_RUNWAY.to_le_bytes());
        rwy[2..6].copy_from_slice(&0x34u32.to_le_bytes());
        rwy[6..8].copy_from_slice(&4u16.to_le_bytes());
        rwy[8] = 16;
        rwy[9] = 1;
        rwy[10] = 34;
        rwy[11] = 2;
        rwy[32..36].copy_from_slice(&3000f32.to_le_bytes());
        rwy[36..40].copy_from_slice(&45f32.to_le_bytes());
        rwy[40..44].copy_from_slice(&180f32.to_le_bytes());
        let mut name = vec![0u8; 6];
        name[0..2].copy_from_slice(&RECORD_NAME.to_le_bytes());
        name.extend(b"Test\0\0");
        name[2..6].copy_from_slice(&12u32.to_le_bytes());
        rec.extend(rwy);
        rec.extend(name);
        let len = rec.len() as u32;
        rec[2..6].copy_from_slice(&len.to_le_bytes());
        let mut section = Vec::new();
        for v in [SECTION_AIRPORT, 1, 1, sub as u32, 16] {
            section.extend(v.to_le_bytes());
        }
        d.extend(section);
        for v in [0u32, 1, data as u32, len] {
            d.extend(v.to_le_bytes());
        }
        d.extend(rec);
        d
    }
    #[test]
    fn decodes_airport_runway_and_name() {
        let a = airports(&sample()).unwrap();
        assert_eq!(a.len(), 1);
        let a = &a[0];
        assert_eq!(a.icao, "KSEA");
        assert_eq!(a.name.as_deref(), Some("Test"));
        assert_eq!((a.latitude, a.longitude, a.altitude), (0., -120., 100.));
        assert_eq!(a.runways[0].name(), "16L/34R");
        assert!(a.runways[0].hard());
        assert_eq!((a.runways[0].length, a.runways[0].heading), (3000., 180.));
    }
    #[test]
    fn rejects_truncation_and_bad_records() {
        let d = sample();
        assert!(airports(&d[..d.len() - 3]).is_err());
        let mut bad = d.clone();
        bad[0] = 0;
        assert!(airports(&bad).is_err());
        let mut zero = d;
        let at = 0x38 + 20 + 16 + 2;
        zero[at..at + 4].copy_from_slice(&0u32.to_le_bytes());
        assert!(airports(&zero).is_err());
    }
}
