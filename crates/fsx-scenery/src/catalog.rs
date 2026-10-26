//! Installation-wide airport index and runway start positions.
use crate::{airport_files, airports, Airport, Runway};
use std::path::Path;

pub struct Catalog {
    pub airports: Vec<Airport>,
}
impl Catalog {
    /// Read every stock airport BGL. Files that fail to decode are skipped and
    /// counted so one damaged add-on cannot block the whole world.
    pub fn load(installation: &Path) -> Result<(Self, usize), String> {
        let mut all = Vec::new();
        let mut failed = 0;
        for file in airport_files(installation)? {
            match std::fs::read(&file)
                .map_err(|e| e.to_string())
                .and_then(|d| airports(&d))
            {
                Ok(list) => all.extend(list),
                Err(_) => failed += 1,
            }
        }
        all.sort_by(|a, b| a.icao.cmp(&b.icao));
        Ok((Self { airports: all }, failed))
    }
    pub fn find(&self, icao: &str) -> Option<&Airport> {
        let icao = icao.trim().to_ascii_uppercase();
        // Later scenery areas override earlier ones; prefer the most complete.
        self.airports
            .iter()
            .filter(|a| a.icao == icao)
            .max_by_key(|a| a.runways.len())
    }
}
/// A takeoff position: threshold of the chosen runway end, facing along it.
#[derive(Debug, Clone, PartialEq)]
pub struct Start {
    pub runway: String,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f64,
    pub heading: f64,
    pub length: f64,
    pub width: f64,
}
const EARTH_RADIUS: f64 = 6_371_000.;
/// Offset a position by metres north/east on a local sphere approximation.
pub fn offset(latitude: f64, longitude: f64, north: f64, east: f64) -> (f64, f64) {
    let lat = latitude + (north / EARTH_RADIUS).to_degrees();
    let lon = longitude + (east / (EARTH_RADIUS * latitude.to_radians().cos())).to_degrees();
    (lat, lon)
}
/// Metres north/east of `origin` (local tangent approximation).
pub fn local(origin: (f64, f64), point: (f64, f64)) -> (f64, f64) {
    let north = (point.0 - origin.0).to_radians() * EARTH_RADIUS;
    let east = (point.1 - origin.1).to_radians() * EARTH_RADIUS * origin.0.to_radians().cos();
    (north, east)
}
impl Airport {
    /// Longest hard runway (any runway if none is hard), primary end.
    pub fn default_start(&self) -> Option<Start> {
        let r = self
            .runways
            .iter()
            .filter(|r| r.hard())
            .chain(self.runways.iter())
            .max_by(|a, b| {
                (a.hard(), a.length)
                    .partial_cmp(&(b.hard(), b.length))
                    .unwrap()
            })?;
        Some(r.start(false))
    }
}
impl Runway {
    /// Threshold start; `reciprocal` selects the secondary end.
    pub fn start(&self, reciprocal: bool) -> Start {
        let heading = if reciprocal {
            (self.heading + 180.) % 360.
        } else {
            self.heading
        };
        // Leave a few metres behind the threshold for the aircraft's length.
        let back = self.length / 2. - 30f64.min(self.length / 4.);
        let h = heading.to_radians();
        let (latitude, longitude) = offset(
            self.latitude,
            self.longitude,
            -back * h.cos(),
            -back * h.sin(),
        );
        let name = self.name();
        let mut ends = name.split('/');
        let (first, second) = (ends.next().unwrap_or(""), ends.next().unwrap_or(""));
        Start {
            runway: if reciprocal { second } else { first }.to_owned(),
            latitude,
            longitude,
            altitude: self.altitude,
            heading,
            length: self.length,
            width: self.width,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn threshold_is_behind_center_along_heading() {
        let r = Runway {
            primary: (36, 0),
            secondary: (18, 0),
            surface: 0,
            latitude: 0.,
            longitude: 0.,
            altitude: 10.,
            length: 2000.,
            width: 45.,
            heading: 0.,
        };
        let s = r.start(false);
        let (n, e) = local((0., 0.), (s.latitude, s.longitude));
        assert!((n + 970.).abs() < 0.5 && e.abs() < 1e-6);
        assert_eq!(s.runway, "36");
        let s = r.start(true);
        let (n, _) = local((0., 0.), (s.latitude, s.longitude));
        assert!((n - 970.).abs() < 0.5 && s.heading == 180. && s.runway == "18");
    }
    #[test]
    fn installed_catalog_finds_ksea_when_present() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../FSX");
        if !root.join("Scenery").is_dir() {
            return;
        }
        let (c, failed) = Catalog::load(&root).unwrap();
        assert_eq!(failed, 0);
        assert!(c.airports.len() > 20000);
        let ksea = c.find("ksea").unwrap();
        assert!((ksea.latitude - 47.449).abs() < 0.01 && ksea.runways.len() >= 2);
        let s = ksea.default_start().unwrap();
        assert!(s.length > 3000. && (s.heading - 180.).abs() < 1.);
    }
}
