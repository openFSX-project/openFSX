//! BGL terrain elevation (section 0x67, TRQ1 records) indexed by QMID cell.
//! Records are decompressed by the installed terrain.dll in the separate x86
//! fsx-terrain-host process, so every FSX compression type, including PTC, is
//! decoded exactly; this module owns layout, indexing and sampling.
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const SECTION_ELEVATION: u32 = 0x67;
const TRQ1: &[u8; 4] = b"TRQ1";
const RECORD_HEADER: usize = 0x28;

/// A quad-tree cell: `level` interleaved digit pairs, u east and v south.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cell {
    pub level: u32,
    pub u: u32,
    pub v: u32,
}
impl Cell {
    pub fn from_qmid(qmid: u64) -> Option<Self> {
        let top = 63 - qmid.leading_zeros();
        if qmid == 0 || top % 2 == 0 || top < 3 {
            return None;
        }
        let level = (top - 1) / 2;
        let (mut u, mut v) = (0, 0);
        for i in 0..level {
            u |= (((qmid >> (2 * i)) & 1) as u32) << i;
            v |= (((qmid >> (2 * i + 1)) & 1) as u32) << i;
        }
        Some(Self { level, u, v })
    }
    pub fn size(level: u32) -> (f64, f64) {
        let n = f64::from(1u32 << level);
        (480. / n, 360. / n)
    }
    /// West longitude and north latitude of the cell.
    pub fn origin(&self) -> (f64, f64) {
        let (w, h) = Self::size(self.level);
        (-180. + f64::from(self.u) * w, 90. - f64::from(self.v) * h)
    }
    pub fn containing(latitude: f64, longitude: f64, level: u32) -> Self {
        let (w, h) = Self::size(level);
        Self {
            level,
            u: ((longitude + 180.) / w).floor().max(0.) as u32,
            v: ((90. - latitude) / h).floor().max(0.) as u32,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Record {
    pub file: PathBuf,
    pub offset: u64,
    pub length: u32,
    pub compression: u32,
    pub columns: u32,
    pub rows: u32,
}
fn u32_at(d: &[u8], i: usize) -> Result<u32, String> {
    d.get(i..i + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "Truncated BGL".into())
}
/// Elevation records of one BGL file, keyed by cell.
pub fn index_file(path: &Path, d: &[u8], out: &mut HashMap<Cell, Record>) -> Result<(), String> {
    index_section(path, d, SECTION_ELEVATION, 2, out)
}
/// TRQ1 records of one section type and data type (2 elevation, 3 land class,
/// 4 water class).
pub fn index_section(
    path: &Path,
    d: &[u8],
    section: u32,
    data_type: u32,
    out: &mut HashMap<Cell, Record>,
) -> Result<(), String> {
    if u32_at(d, 0)? != 0x1992_0201 {
        return Err("Not an FSX BGL file".into());
    }
    for s in 0..(u32_at(d, 0x14)? as usize).min(4096) {
        let h = 0x38 + s * 20;
        if u32_at(d, h)? != section {
            continue;
        }
        let (count, table) = (u32_at(d, h + 8)? as usize, u32_at(d, h + 12)? as usize);
        for k in 0..count.min(1 << 20) {
            let sub = table + k * 16;
            let (start, size) = (u32_at(d, sub + 8)? as usize, u32_at(d, sub + 12)? as usize);
            let r = d
                .get(start..start.saturating_add(size))
                .ok_or("Terrain record exceeds file")?;
            if r.len() < RECORD_HEADER || &r[0..4] != TRQ1 {
                continue;
            }
            let qmid = u64::from(u32_at(r, 12)?) | u64::from(u32_at(r, 16)?) << 32;
            let (Some(cell), columns, rows) = (
                Cell::from_qmid(qmid),
                u32_at(r, 24)? & 0xFFFF,
                u32_at(r, 28)? & 0xFFFF,
            ) else {
                continue;
            };
            if u32_at(r, 8)? & 0xFFFF != data_type || columns == 0 || rows == 0 || columns > 1024 {
                continue;
            }
            out.insert(
                cell,
                Record {
                    file: path.to_path_buf(),
                    offset: (start + RECORD_HEADER) as u64,
                    length: (size - RECORD_HEADER) as u32,
                    compression: u32_at(r, 8)? >> 16,
                    columns,
                    rows,
                },
            );
        }
    }
    Ok(())
}
/// Every elevation record of the stock DEM files (Scenery/*/scenery/dem*.bgl).
pub fn index(installation: &Path) -> Result<HashMap<Cell, Record>, String> {
    let mut out = HashMap::new();
    let scenery = installation.join("Scenery");
    for area in std::fs::read_dir(&scenery)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let Ok(entries) = std::fs::read_dir(area.path().join("scenery")) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.starts_with("dem") && name.ends_with(".bgl") {
                let path = entry.path();
                let data = std::fs::read(&path).map_err(|e| e.to_string())?;
                // A damaged file must not hide the rest of the world.
                let _ = index_file(&path, &data, &mut out);
            }
        }
    }
    Ok(out)
}
/// Elevation records of the scenery tiles (30 x 22.5 degree folders XXYY)
/// that overlap a square of `radius` metres around a position.
pub fn index_near(
    installation: &Path,
    latitude: f64,
    longitude: f64,
    radius: f64,
) -> Result<HashMap<Cell, Record>, String> {
    let dlat = (radius / 6_371_000.).to_degrees();
    let dlon = dlat / latitude.to_radians().cos().max(0.05);
    let mut tiles = std::collections::BTreeSet::new();
    for (la, lo) in [
        (-dlat, -dlon),
        (-dlat, dlon),
        (dlat, -dlon),
        (dlat, dlon),
        (0., 0.),
    ] {
        let u = (((longitude + lo + 180.) / 30.).floor() as i64).rem_euclid(12);
        let v = ((90. - (latitude + la)) / 22.5).floor().clamp(0., 7.) as i64;
        tiles.insert(format!("{u:02}{v:02}"));
    }
    let mut out = HashMap::new();
    for tile in tiles {
        let Ok(entries) =
            std::fs::read_dir(installation.join("Scenery").join(&tile).join("scenery"))
        else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.starts_with("dem") && name.ends_with(".bgl") {
                let path = entry.path();
                let data = std::fs::read(&path).map_err(|e| e.to_string())?;
                let _ = index_file(&path, &data, &mut out);
            }
        }
    }
    Ok(out)
}
/// Decoded elevation grid in metres, row 0 at the north edge.
#[derive(Debug, Clone)]
pub struct Grid {
    pub cell: Cell,
    pub columns: usize,
    pub rows: usize,
    pub heights: Vec<f32>,
}
impl Grid {
    /// Bilinear height at a position inside the cell (edges are shared).
    pub fn sample(&self, latitude: f64, longitude: f64) -> f32 {
        let (west, north) = self.cell.origin();
        let (w, h) = Cell::size(self.cell.level);
        let x = ((longitude - west) / w * (self.columns - 1) as f64)
            .clamp(0., (self.columns - 1) as f64);
        let y = ((north - latitude) / h * (self.rows - 1) as f64).clamp(0., (self.rows - 1) as f64);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.columns - 1), (y0 + 1).min(self.rows - 1));
        let (fx, fy) = ((x - x0 as f64) as f32, (y - y0 as f64) as f32);
        let at = |c: usize, r: usize| self.heights[r * self.columns + c];
        let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * fx;
        let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * fx;
        top + (bottom - top) * fy
    }
}
/// Land-class grid (Olson classes and FSX extensions), row 0 at the north edge.
#[derive(Debug, Clone)]
pub struct ClassGrid {
    pub cell: Cell,
    pub columns: usize,
    pub rows: usize,
    pub classes: Vec<u8>,
}
impl ClassGrid {
    /// Nearest sample: classes are categories and must not be interpolated.
    pub fn sample(&self, latitude: f64, longitude: f64) -> u8 {
        let (west, north) = self.cell.origin();
        let (w, h) = Cell::size(self.cell.level);
        let x = ((longitude - west) / w * (self.columns - 1) as f64).round();
        let y = ((north - latitude) / h * (self.rows - 1) as f64).round();
        let x = x.clamp(0., (self.columns - 1) as f64) as usize;
        let y = y.clamp(0., (self.rows - 1) as f64) as usize;
        self.classes[y * self.columns + x]
    }
}
/// Land-class records of BASE/Scenery/worldlc.bgl (section 0x68, level 7).
pub fn land_class_index(installation: &Path) -> Result<HashMap<Cell, Record>, String> {
    let path = installation.join("Scenery/BASE/Scenery/worldlc.bgl");
    let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = HashMap::new();
    index_section(&path, &data, 0x68, 3, &mut out)?;
    Ok(out)
}
/// Connection to the x86 decode helper.
pub struct Decoder {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
}
impl Decoder {
    pub fn start(helper: &Path, installation: &Path) -> Result<Self, String> {
        let mut command = Command::new(helper);
        command
            .arg("--decode")
            .arg(installation)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("{}: {e}", helper.display()))?;
        let input = child.stdin.take().ok_or("No helper stdin")?;
        let output = child.stdout.take().ok_or("No helper stdout")?;
        Ok(Self {
            child,
            input,
            output,
        })
    }
    /// Raw samples: format 0 returns f32 elevation bytes, 1 one byte per class.
    fn decode_raw(&mut self, cell: Cell, record: &Record, format: u32) -> Result<Vec<u8>, String> {
        let mut file = std::fs::File::open(&record.file).map_err(|e| e.to_string())?;
        use std::io::Seek;
        file.seek(std::io::SeekFrom::Start(record.offset))
            .map_err(|e| e.to_string())?;
        let mut payload = vec![0; record.length as usize];
        file.read_exact(&mut payload).map_err(|e| e.to_string())?;
        let mut request = Vec::with_capacity(16 + payload.len());
        for v in [
            record.compression | format << 16,
            record.columns,
            record.rows,
            record.length,
        ] {
            request.extend(v.to_le_bytes());
        }
        request.extend(payload);
        self.input
            .write_all(&request)
            .and_then(|_| self.input.flush())
            .map_err(|e| format!("Terrain helper stopped: {e}"))?;
        let mut status = [0; 4];
        self.output
            .read_exact(&mut status)
            .map_err(|e| format!("Terrain helper stopped: {e}"))?;
        if u32::from_le_bytes(status) != 1 {
            return Err(format!("terrain.dll rejected cell {cell:?}"));
        }
        let count = record.columns as usize * record.rows as usize;
        let mut bytes = vec![0; count * if format == 0 { 4 } else { 1 }];
        self.output
            .read_exact(&mut bytes)
            .map_err(|e| e.to_string())?;
        Ok(bytes)
    }
    pub fn decode_classes(&mut self, cell: Cell, record: &Record) -> Result<ClassGrid, String> {
        Ok(ClassGrid {
            cell,
            columns: record.columns as usize,
            rows: record.rows as usize,
            classes: self.decode_raw(cell, record, 1)?,
        })
    }
    pub fn decode(&mut self, cell: Cell, record: &Record) -> Result<Grid, String> {
        let bytes = self.decode_raw(cell, record, 0)?;
        let heights: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        if heights.iter().any(|h| !h.is_finite() || h.abs() > 12_000.) {
            return Err("Decoded elevation outside the plausible range".into());
        }
        Ok(Grid {
            cell,
            columns: record.columns as usize,
            rows: record.rows as usize,
            heights,
        })
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
/// Elevation sampler over the finest available cells up to `max_level`.
/// Land-class sampler at the stock world level.
pub struct LandClass {
    records: HashMap<Cell, Record>,
    cache: HashMap<Cell, Option<ClassGrid>>,
}
impl LandClass {
    pub fn new(records: HashMap<Cell, Record>) -> Self {
        Self {
            records,
            cache: HashMap::new(),
        }
    }
    pub fn class(&mut self, decoder: &mut Decoder, latitude: f64, longitude: f64) -> Option<u8> {
        let level = self.records.keys().next()?.level;
        let cell = Cell::containing(latitude, longitude, level);
        if !self.cache.contains_key(&cell) {
            let grid = self
                .records
                .get(&cell)
                .and_then(|r| decoder.decode_classes(cell, &r.clone()).ok());
            self.cache.insert(cell, grid);
        }
        self.cache
            .get(&cell)?
            .as_ref()
            .map(|g| g.sample(latitude, longitude))
    }
}
pub struct Elevation {
    records: HashMap<Cell, Record>,
    decoder: Decoder,
    cache: HashMap<Cell, Option<Grid>>,
    max_level: u32,
}
impl Elevation {
    pub fn new(records: HashMap<Cell, Record>, decoder: Decoder, max_level: u32) -> Self {
        Self {
            records,
            decoder,
            cache: HashMap::new(),
            max_level,
        }
    }
    pub fn decoder(&mut self) -> &mut Decoder {
        &mut self.decoder
    }
    pub fn height(&mut self, latitude: f64, longitude: f64) -> Option<f32> {
        for level in (4..=self.max_level).rev() {
            let cell = Cell::containing(latitude, longitude, level);
            if !self.cache.contains_key(&cell) {
                let grid = match self.records.get(&cell) {
                    Some(r) => self.decoder.decode(cell, &r.clone()).ok(),
                    None => None,
                };
                self.cache.insert(cell, grid);
            }
            if let Some(Some(grid)) = self.cache.get(&cell) {
                return Some(grid.sample(latitude, longitude));
            }
        }
        None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn qmid_matches_scenery_tile_names() {
        // Tile 0101 (lon -150..-120, lat 67.5..45) is root QMID 0x203.
        let c = Cell::from_qmid(0x203).unwrap();
        assert_eq!((c.level, c.u, c.v), (4, 1, 1));
        assert_eq!(c.origin(), (-150., 67.5));
        let child = Cell::from_qmid(0x203 * 4 + 3).unwrap();
        assert_eq!((child.level, child.u, child.v), (5, 3, 3));
        assert_eq!(Cell::containing(47.45, -122.3, 4), c);
        assert_eq!(Cell::from_qmid(0x100), None);
    }
    #[test]
    fn bilinear_sample_interpolates_corners() {
        let g = Grid {
            cell: Cell {
                level: 4,
                u: 1,
                v: 1,
            },
            columns: 2,
            rows: 2,
            heights: vec![0., 10., 20., 30.],
        };
        assert_eq!(g.sample(67.5, -150.), 0.);
        assert_eq!(g.sample(45., -120.), 30.);
        assert_eq!(g.sample(56.25, -135.), 15.);
    }
    #[test]
    fn installed_ksea_elevation_when_present() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../FSX");
        let helper =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fsx-terrain-host.exe");
        if !root.join("terrain.dll").is_file() || !helper.is_file() {
            return;
        }
        let records = index_near(&root, 47.449, -122.309, 20_000.).unwrap();
        assert!(records.len() > 1000);
        let mut e = Elevation::new(records, Decoder::start(&helper, &root).unwrap(), 12);
        let field = e.height(47.449, -122.309).unwrap();
        assert!((field - 132.).abs() < 25., "KSEA field {field}");
        // Mount Rainier summit (4392 m) appears in the coarser stock mesh.
        let rainier = e.height(46.8529, -121.7604).unwrap();
        assert!(rainier > 3500., "Rainier {rainier}");
    }
    #[test]
    fn installed_ksea_land_class_when_present() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../FSX");
        let helper =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fsx-terrain-host.exe");
        if !root.join("terrain.dll").is_file() || !helper.is_file() {
            return;
        }
        let mut lc = LandClass::new(land_class_index(&root).unwrap());
        let mut d = Decoder::start(&helper, &root).unwrap();
        // Seattle-Tacoma is urban grid; the Cascades foothills are conifer forest.
        assert_eq!(lc.class(&mut d, 47.449, -122.309), Some(101));
        assert!(matches!(
            lc.class(&mut d, 47.2, -121.6),
            Some(3 | 20..=27 | 61 | 62)
        ));
    }
}
