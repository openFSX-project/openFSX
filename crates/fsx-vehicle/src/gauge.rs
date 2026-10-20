//! Read-only PE32 / FS800-1000 gauge metadata. Never maps or executes a DLL.
use fsx_cfg::decode;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    ops::Range,
    path::Path,
};

type Result<T> = std::result::Result<T, String>;
const MAX_FILE: usize = 64 * 1024 * 1024;
const MAX_TABLE: u32 = 8192;
const MAX_GAUGES: usize = 512;
const MAX_ELEMENTS: usize = 2048;
const MAX_TOTAL_ELEMENTS: usize = 65536;
const MAX_RESOURCES: usize = 8192;
const MAX_NEEDLE_POINTS: usize = 65536;

fn bytes(b: &[u8], offset: usize, size: usize) -> Result<&[u8]> {
    let end = offset.checked_add(size).ok_or("PE offset overflow")?;
    b.get(offset..end).ok_or_else(|| "Truncated PE data".into())
}
fn u16_at(b: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(bytes(b, offset, 2)?.try_into().unwrap()))
}
fn u32_at(b: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(bytes(b, offset, 4)?.try_into().unwrap()))
}
fn f64_at(b: &[u8], offset: usize) -> Result<f64> {
    let value = f64::from_le_bytes(bytes(b, offset, 8)?.try_into().unwrap());
    if !value.is_finite() {
        return Err("Non-finite gauge numeric field".into());
    }
    Ok(value)
}
fn plus(a: u32, b: u32) -> Result<u32> {
    a.checked_add(b).ok_or_else(|| "PE address overflow".into())
}

#[derive(Debug)]
struct Section {
    rva: u32,
    raw: Range<usize>,
    mapped: u32,
}
struct Pe<'a> {
    data: &'a [u8],
    base: u32,
    headers: u32,
    sections: Vec<Section>,
    exports: (u32, u32),
    resources: (u32, u32),
}
impl<'a> Pe<'a> {
    fn parse(data: &'a [u8]) -> Result<Self> {
        if data.len() > MAX_FILE || bytes(data, 0, 2)? != b"MZ" {
            return Err("Expected a PE32 gauge file within 64 MiB".into());
        }
        let pe = u32_at(data, 0x3c)? as usize;
        if bytes(data, pe, 4)? != b"PE\0\0" || u16_at(data, pe + 4)? != 0x14c {
            return Err("Only x86 PE32 gauge images are supported".into());
        }
        let count = u16_at(data, pe + 6)? as usize;
        let optional_size = u16_at(data, pe + 20)? as usize;
        let opt = pe.checked_add(24).ok_or("PE offset overflow")?;
        let optional = bytes(data, opt, optional_size)?;
        if count == 0
            || count > 96
            || optional_size < 120
            || u16_at(optional, 0)? != 0x10b
            || u32_at(optional, 92)? < 3
        {
            return Err("Invalid or unsupported PE32 optional header / section count".into());
        }
        let headers = u32_at(optional, 60)?;
        let image_size = u32_at(optional, 56)?;
        let base = u32_at(optional, 28)?;
        plus(base, image_size)?;
        let table = opt + optional_size;
        bytes(data, 0, headers as usize)?;
        if (headers as usize) < table + count * 40 || headers > image_size {
            return Err("Invalid PE header extent".into());
        }
        let mut sections: Vec<Section> = Vec::new();
        for i in 0..count {
            let p = table + i * 40;
            let virtual_size = u32_at(data, p + 8)?;
            let rva = u32_at(data, p + 12)?;
            let raw_size = u32_at(data, p + 16)?;
            let raw_start = u32_at(data, p + 20)? as usize;
            let mapped = virtual_size.max(raw_size);
            let end = plus(rva, mapped)?;
            let raw_end = raw_start
                .checked_add(raw_size as usize)
                .ok_or("PE offset overflow")?;
            bytes(data, raw_start, raw_size as usize)?;
            if mapped != 0 && (rva < headers || end > image_size)
                || raw_size != 0 && raw_start < headers as usize
                || sections.iter().any(|s| {
                    mapped != 0 && s.mapped != 0 && rva < s.rva + s.mapped && s.rva < end
                        || raw_size != 0
                            && !s.raw.is_empty()
                            && raw_start < s.raw.end
                            && s.raw.start < raw_end
                })
            {
                return Err("Overlapping or out-of-image PE sections".into());
            }
            sections.push(Section {
                rva,
                raw: raw_start..raw_end,
                mapped,
            });
        }
        Ok(Self {
            data,
            base,
            headers,
            sections,
            exports: (u32_at(optional, 96)?, u32_at(optional, 100)?),
            resources: (u32_at(optional, 112)?, u32_at(optional, 116)?),
        })
    }
    fn range(&self, rva: u32, size: usize) -> Result<Range<usize>> {
        let size_u32 = u32::try_from(size).map_err(|_| "PE range too large")?;
        let end = plus(rva, size_u32)?;
        if end <= self.headers {
            return Ok(rva as usize..end as usize);
        }
        for section in &self.sections {
            if rva >= section.rva {
                let delta = (rva - section.rva) as usize;
                if delta
                    .checked_add(size)
                    .is_some_and(|end| end <= section.raw.len())
                {
                    let start = section.raw.start + delta;
                    return Ok(start..start + size);
                }
            }
        }
        Err("PE RVA is not wholly backed by one file section".into())
    }
    fn slice(&self, rva: u32, size: usize) -> Result<&'a [u8]> {
        Ok(&self.data[self.range(rva, size)?])
    }
    fn word(&self, rva: u32) -> Result<u32> {
        u32_at(self.slice(rva, 4)?, 0)
    }
    fn pointer_rva(&self, pointer: u32) -> Result<u32> {
        pointer
            .checked_sub(self.base)
            .filter(|r| *r != 0)
            .ok_or_else(|| {
                format!(
                    "Invalid gauge image pointer {pointer:#x} (base {:#x})",
                    self.base
                )
            })
    }
    fn string_bytes(&self, rva: u32) -> Result<Vec<u8>> {
        let mut result = Vec::new();
        for i in 0..=256 {
            let value = self.slice(plus(rva, i)?, 1)?[0];
            if value == 0 {
                return Ok(result);
            }
            if value < 32 || value == 127 {
                return Err("Control byte in gauge name".into());
            }
            result.push(value);
        }
        Err("Gauge name exceeds 256 bytes or is unterminated".into())
    }
    fn linkage(&self) -> Result<u32> {
        let (rva, size) = self.exports;
        if rva == 0 || size < 40 {
            return Err("Gauge library has no export directory".into());
        }
        let directory = self.slice(rva, 40)?;
        let end = plus(rva, size)?;
        let functions = u32_at(directory, 20)?;
        let names = u32_at(directory, 24)?;
        if functions == 0 || functions > MAX_TABLE || names > MAX_TABLE {
            return Err("PE export table count exceeds limit".into());
        }
        let addresses = self.slice(u32_at(directory, 28)?, functions as usize * 4)?;
        let pointers = self.slice(u32_at(directory, 32)?, names as usize * 4)?;
        let ordinals = self.slice(u32_at(directory, 36)?, names as usize * 2)?;
        let mut found = None;
        for i in 0..names as usize {
            let ordinal = u16_at(ordinals, i * 2)? as usize;
            if ordinal >= functions as usize {
                return Err("PE export ordinal exceeds address table".into());
            }
            if self.string_bytes(u32_at(pointers, i * 4)?)? == b"Linkage" {
                let address = u32_at(addresses, ordinal * 4)?;
                if address == 0
                    || (address >= rva && address < end)
                    || found.replace(address).is_some()
                {
                    return Err("Null, forwarded or duplicate Linkage export".into());
                }
            }
        }
        found.ok_or_else(|| "Gauge library does not export Linkage".into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResourceKey {
    Id(u32),
    Name(String),
}
#[derive(Debug, Clone)]
pub struct Resource {
    pub kind: ResourceKey,
    pub name: ResourceKey,
    pub language: ResourceKey,
    pub code_page: u32,
    range: Range<usize>,
}

#[derive(Debug, Clone)]
pub struct Element {
    /// Address within the preferred image, expressed as an RVA, never callable.
    pub rva: u32,
    pub kind: u32,
    pub resource_id: i32,
    pub position: [i32; 2],
    /// Authored OFS: needle bitmap pivot, or sprite texture-center position in the gauge.
    pub offset: [i32; 2],
    pub image_flags: u32,
    /// Successor RVAs in the authored next-list order, not spatial transform parenting.
    pub children: Vec<u32>,
    pub needle: Option<Needle>,
    pub sprite: Option<Sprite>,
    pub text: Option<NativeText>,
}
#[derive(Debug, Clone)]
pub struct NativeText {
    pub tokens: [u32; 3],
    pub source_callbacks: [Option<u32>; 3],
    pub update_callback_rva: Option<u32>,
    /// Authored string rectangle, not measured font glyph metrics.
    pub size: [u32; 2],
    pub font_name: String,
    pub font_weight: i32,
    pub font_charset: u8,
    pub font_height: i32,
    pub draw_text_flags: u32,
    pub max_length: u16,
    /// COLORREF values remain in their original 0xBBGGRR representation.
    pub colors: [u32; 3],
    /// Only the first record is bounded; sequence traversal is unsupported.
    pub selection_sequence_rva: Option<u32>,
}

fn native_text(pe: &Pe<'_>, rva: u32, windows_1252: bool) -> Result<NativeText> {
    // x86 pack(4), independently derived offsets; runtime string/GDI handles
    // and source values are not followed or interpreted from file bytes.
    let b = pe.slice(rva, 324)?;
    let name = &b[248..280];
    let end = name
        .iter()
        .position(|&c| c == 0)
        .ok_or("Unterminated native string font name")?;
    let font_name = decode(&name[..end], windows_1252)?;
    if font_name.chars().any(char::is_control) {
        return Err("Control character in native string font name".into());
    }
    let size = [u32_at(b, 192)?, u32_at(b, 196)?];
    let font_height = u32_at(b, 308)? as i32;
    if size.iter().any(|&v| v > 65536) || font_height.unsigned_abs() > 65536 {
        return Err("Native string layout exceeds bounds".into());
    }
    let selection_sequence_rva = match u32_at(b, 300)? {
        0 => None,
        pointer => {
            let rva = pe.pointer_rva(pointer)?;
            pe.slice(rva, 12)?;
            Some(rva)
        }
    };
    Ok(NativeText {
        tokens: [u32_at(b, 60)?, u32_at(b, 88)?, u32_at(b, 116)?],
        source_callbacks: [
            callback_rva(pe, u32_at(b, 168)?)?,
            callback_rva(pe, u32_at(b, 172)?)?,
            callback_rva(pe, u32_at(b, 176)?)?,
        ],
        update_callback_rva: callback_rva(pe, u32_at(b, 312)?)?,
        size,
        font_name,
        font_height,
        font_weight: u32_at(b, 280)? as i32,
        font_charset: b[284],
        draw_text_flags: u32_at(b, 288)?,
        max_length: u16_at(b, 292)?,
        colors: [u32_at(b, 208)?, u32_at(b, 212)?, u32_at(b, 216)?],
        selection_sequence_rva,
    })
}
#[derive(Debug, Clone)]
pub struct SpriteAxis {
    pub token: u32,
    pub scale: f64,
    pub source_callback_rva: Option<u32>,
    pub update_callback_rva: Option<u32>,
}
#[derive(Debug, Clone)]
pub struct Sprite {
    /// X, Y, rotation; stored numeric source slots are deliberately not interpreted.
    pub axes: [SpriteAxis; 3],
    pub texture_scale: [f64; 2],
}

fn callback_rva(pe: &Pe<'_>, pointer: u32) -> Result<Option<u32>> {
    if pointer == 0 {
        return Ok(None);
    }
    let rva = pe.pointer_rva(pointer)?;
    pe.slice(rva, 1)?;
    Ok(Some(rva))
}
fn sprite(pe: &Pe<'_>, rva: u32) -> Result<Sprite> {
    // Independently read x86, four-byte-packed ABI fields; no SDK structures linked.
    let b = pe.slice(rva, 352)?;
    let axis = |i: usize| -> Result<SpriteAxis> {
        let start = 60 + i * 40;
        let scale = f64_at(b, start + 32)?;
        if scale.abs() > 65536. {
            return Err("Sprite source scale exceeds bounds".into());
        }
        Ok(SpriteAxis {
            token: u32_at(b, start)?,
            scale,
            source_callback_rva: callback_rva(pe, u32_at(b, start + 28)?)?,
            update_callback_rva: callback_rva(pe, u32_at(b, 316 + i * 4)?)?,
        })
    };
    let texture_scale = [180, 184]
        .map(|offset| f32::from_le_bytes(b[offset..offset + 4].try_into().unwrap()) as f64);
    if texture_scale
        .iter()
        .any(|s| !s.is_finite() || s.abs() > 65536.)
    {
        return Err("Non-finite or oversized sprite texture scale".into());
    }
    Ok(Sprite {
        axes: [axis(0)?, axis(1)?, axis(2)?],
        texture_scale,
    })
}
#[derive(Debug, Clone)]
pub struct NeedlePoint {
    pub position: [i32; 2],
    pub value: f64,
}
#[derive(Debug, Clone)]
pub struct Needle {
    pub token: u32,
    pub update_callback_rva: Option<u32>,
    pub max_degrees_per_tick: i32,
    pub curve: Vec<NeedlePoint>,
}
impl Needle {
    /// Stateless table interpolation for explicit inspection values. No timing/lag.
    /// Screen coordinates use +X right, +Y down; angles increase clockwise.
    pub fn reading_angle(&self, pivot: [i32; 2], reading: f64) -> Result<f64> {
        if !reading.is_finite()
            || reading.abs() > 1e12
            || self.curve.len() < 2
            || self.curve.len() > 512
        {
            return Err("Needle reading needs a finite value and 2..512 table points".into());
        }
        let increasing = self.curve[1].value > self.curve[0].value;
        let mut mapped = Vec::new();
        for (i, point) in self.curve.iter().enumerate() {
            if !point.value.is_finite()
                || point.value.abs() > 1e12
                || (i > 0
                    && if increasing {
                        point.value <= self.curve[i - 1].value
                    } else {
                        point.value >= self.curve[i - 1].value
                    })
            {
                return Err("Needle table values must be finite and strictly monotonic".into());
            }
            let dx = point.position[0] as f64 - pivot[0] as f64;
            let dy = point.position[1] as f64 - pivot[1] as f64;
            if dx == 0. && dy == 0. {
                return Err("Needle table point coincides with its pivot".into());
            }
            let mut angle = dy.atan2(dx);
            if let Some((_, previous)) = mapped.last() {
                // Authored points are ordered clockwise, including across +/-pi.
                while angle <= *previous {
                    angle += std::f64::consts::TAU;
                }
            }
            mapped.push((point.value, angle));
        }
        let first = mapped[0];
        let last = mapped[mapped.len() - 1];
        if if increasing {
            reading <= first.0
        } else {
            reading >= first.0
        } {
            return Ok(first.1);
        }
        if if increasing {
            reading >= last.0
        } else {
            reading <= last.0
        } {
            return Ok(last.1);
        }
        for pair in mapped.windows(2) {
            if if increasing {
                reading <= pair[1].0
            } else {
                reading >= pair[1].0
            } {
                let t = (reading - pair[0].0) / (pair[1].0 - pair[0].0);
                return Ok(pair[0].1 + t * (pair[1].1 - pair[0].1));
            }
        }
        Err("Needle reading has no table interval".into())
    }
}

fn needle(pe: &Pe<'_>, rva: u32) -> Result<Needle> {
    let b = pe.slice(rva, 236)?;
    let count = u32_at(b, 116)? as usize;
    if count > 512 {
        return Err("Native needle table exceeds 512 points".into());
    }
    let pointer = u32_at(b, 112)?;
    if (pointer == 0) != (count == 0) {
        return Err("Inconsistent native needle table pointer/count".into());
    }
    let mut curve = Vec::new();
    if count != 0 {
        let table = pe.slice(pe.pointer_rva(pointer)?, count * 24)?;
        for i in 0..count {
            let position = [
                u32_at(table, i * 24)? as i32,
                u32_at(table, i * 24 + 4)? as i32,
            ];
            if position.iter().any(|v| v.unsigned_abs() > 65536) {
                return Err("Native needle table coordinate exceeds bounds".into());
            }
            let value = f64_at(table, i * 24 + 8)?;
            if value.abs() > 1e12 {
                return Err("Native needle table value exceeds numeric bounds".into());
            }
            curve.push(NeedlePoint { position, value });
        }
    }
    let callback = u32_at(b, 224)?;
    let update_callback_rva = if callback == 0 {
        None
    } else {
        let rva = pe.pointer_rva(callback)?;
        pe.slice(rva, 1)?;
        Some(rva)
    };
    Ok(Needle {
        token: u32_at(b, 60)?,
        update_callback_rva,
        max_degrees_per_tick: u32_at(b, 120)? as i32,
        curve,
    })
}
#[derive(Debug, Clone)]
pub struct Gauge {
    pub rva: u32,
    pub name: String,
    pub version: u32,
    pub size_mm: [u32; 2],
    pub roots: Vec<u32>,
    pub elements: Vec<Element>,
}
#[derive(Debug)]
pub struct GaugeLibrary {
    data: Vec<u8>,
    pub gauges: Vec<Gauge>,
    pub resources: Vec<Resource>,
}

fn pointer_list(pe: &Pe<'_>, mut rva: u32, limit: usize) -> Result<Vec<u32>> {
    let mut result = Vec::new();
    for _ in 0..=limit {
        let pointer = pe.word(rva)?;
        if pointer == 0 {
            return Ok(result);
        }
        if result.len() == limit {
            break;
        }
        result.push(pe.pointer_rva(pointer)?);
        rva = plus(rva, 4)?;
    }
    Err("Unterminated or oversized gauge pointer list".into())
}

fn elements(pe: &Pe<'_>, roots: &[u32], windows_1252: bool) -> Result<Vec<Element>> {
    let mut output = Vec::new();
    let mut active = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut stack: Vec<_> = roots.iter().rev().map(|r| (*r, false)).collect();
    let mut edges = roots.len();
    while let Some((rva, leave)) = stack.pop() {
        if leave {
            active.remove(&rva);
            continue;
        }
        if active.contains(&rva) {
            return Err("Cycle in gauge element graph".into());
        }
        if !seen.insert(rva) {
            continue;
        }
        if output.len() >= MAX_ELEMENTS {
            return Err("Gauge exceeds 2048 distinct elements".into());
        }
        let b = pe.slice(rva, 60)?;
        let next = u32_at(b, 40)?;
        let children = if next == 0 {
            Vec::new()
        } else {
            pointer_list(pe, pe.pointer_rva(next)?, MAX_ELEMENTS)?
        };
        edges += children.len();
        if edges > MAX_ELEMENTS * 4 {
            return Err("Gauge element edge budget exceeded".into());
        }
        active.insert(rva);
        stack.push((rva, true));
        stack.extend(children.iter().rev().map(|child| (*child, false)));
        output.push(Element {
            rva,
            kind: u32_at(b, 0)?,
            resource_id: u32_at(b, 4)? as i32,
            position: [u32_at(b, 8)? as i32, u32_at(b, 12)? as i32],
            offset: [u32_at(b, 24)? as i32, u32_at(b, 28)? as i32],
            image_flags: u32_at(b, 48)?,
            children,
            needle: if u32_at(b, 0)? == 1 {
                Some(needle(pe, rva)?)
            } else {
                None
            },
            sprite: if u32_at(b, 0)? == 6 {
                Some(sprite(pe, rva)?)
            } else {
                None
            },
            text: if u32_at(b, 0)? == 2 {
                Some(native_text(pe, rva, windows_1252)?)
            } else {
                None
            },
        });
    }
    Ok(output)
}

fn resources(pe: &Pe<'_>) -> Result<Vec<Resource>> {
    let (base, size) = pe.resources;
    if base == 0 && size == 0 {
        return Ok(Vec::new());
    }
    if base == 0 || size < 16 {
        return Err("Invalid PE resource directory extent".into());
    }
    let directory = pe.slice(base, size as usize)?;
    let mut output = Vec::new();
    let mut stack = vec![(0usize, Vec::<ResourceKey>::new())];
    let mut directories = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut entry_budget = 0;
    while let Some((offset, path)) = stack.pop() {
        if path.len() >= 3 || !directories.insert(offset) {
            return Err("Cyclic, shared or overdeep PE resource directory".into());
        }
        let header = bytes(directory, offset, 16)?;
        let count = u16_at(header, 12)? as usize + u16_at(header, 14)? as usize;
        entry_budget += count;
        if entry_budget > MAX_RESOURCES * 4 {
            return Err("PE resource entry budget exceeded".into());
        }
        let entries = bytes(directory, offset + 16, count * 8)?;
        for i in 0..count {
            let name = u32_at(entries, i * 8)?;
            let key = if name & 0x80000000 == 0 {
                ResourceKey::Id(name)
            } else {
                let p = (name & 0x7fffffff) as usize;
                let count = u16_at(directory, p)? as usize;
                if count > 256 {
                    return Err("PE resource name exceeds 256 UTF-16 units".into());
                }
                let units = bytes(directory, p + 2, count * 2)?
                    .chunks_exact(2)
                    .map(|s| u16::from_le_bytes([s[0], s[1]]))
                    .collect::<Vec<_>>();
                ResourceKey::Name(
                    String::from_utf16(&units).map_err(|_| "Invalid resource UTF-16")?,
                )
            };
            let mut path = path.clone();
            path.push(key);
            let target = u32_at(entries, i * 8 + 4)?;
            if target & 0x80000000 != 0 {
                stack.push(((target & 0x7fffffff) as usize, path));
                if stack.len() > MAX_RESOURCES {
                    return Err("PE resource directory budget exceeded".into());
                }
            } else {
                if path.len() != 3 || output.len() >= MAX_RESOURCES || !paths.insert(path.clone()) {
                    return Err("Invalid depth, duplicate or excessive PE resource leaf".into());
                }
                let leaf = bytes(directory, target as usize, 16)?;
                let range = pe.range(u32_at(leaf, 0)?, u32_at(leaf, 4)? as usize)?;
                output.push(Resource {
                    kind: path[0].clone(),
                    name: path[1].clone(),
                    language: path[2].clone(),
                    code_page: u32_at(leaf, 8)?,
                    range,
                });
            }
        }
    }
    output.sort_by(|a, b| (&a.kind, &a.name, &a.language).cmp(&(&b.kind, &b.name, &b.language)));
    Ok(output)
}

impl GaugeLibrary {
    pub fn load(path: &Path, windows_1252: bool) -> Result<Self> {
        let mut data = Vec::new();
        fs::File::open(path)
            .and_then(|f| f.take((MAX_FILE + 1) as u64).read_to_end(&mut data))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(data, windows_1252)
    }
    pub fn parse(data: Vec<u8>, windows_1252: bool) -> Result<Self> {
        let pe = Pe::parse(&data)?;
        let linkage = pe.linkage()?;
        pe.slice(linkage, 24)?;
        let headers = pointer_list(&pe, plus(linkage, 24)?, MAX_GAUGES)?;
        let mut gauges = Vec::new();
        let mut total_elements = 0;
        let mut total_needle_points = 0;
        for rva in headers {
            let header = pe.slice(rva, 52)?;
            let version = u32_at(header, 0)?;
            if ![800, 900, 1000].contains(&version) {
                return Err(format!(
                    "Unsupported gauge header version {version}; expected FS800, FS900 or FS1000"
                ));
            }
            let name = decode(
                &pe.string_bytes(pe.pointer_rva(u32_at(header, 4)?)?)?,
                windows_1252,
            )?;
            if name.is_empty() {
                return Err(format!("Empty gauge name at RVA {rva:#x}"));
            }
            let list = u32_at(header, 8)?;
            let roots = if list == 0 {
                Vec::new()
            } else {
                // GAUGEHDR points to one root-pointer variable, not a terminated array.
                let root = pe.word(pe.pointer_rva(list)?)?;
                if root == 0 {
                    Vec::new()
                } else {
                    vec![pe.pointer_rva(root)?]
                }
            };
            let elements =
                elements(&pe, &roots, windows_1252).map_err(|e| format!("Gauge {name}: {e}"))?;
            total_elements += elements.len();
            total_needle_points += elements
                .iter()
                .filter_map(|e| e.needle.as_ref())
                .map(|n| n.curve.len())
                .sum::<usize>();
            if total_needle_points > MAX_NEEDLE_POINTS {
                return Err("Library exceeds 65536 native needle table points".into());
            }
            if total_elements > MAX_TOTAL_ELEMENTS {
                return Err("Library exceeds total element budget".into());
            }
            gauges.push(Gauge {
                rva,
                name,
                version,
                size_mm: [u32_at(header, 44)?, u32_at(header, 48)?],
                roots,
                elements,
            });
        }
        let resources = resources(&pe)?;
        Ok(Self {
            data,
            gauges,
            resources,
        })
    }
    pub fn gauge(&self, name: &str) -> Result<&Gauge> {
        let mut matches = self
            .gauges
            .iter()
            .filter(|g| g.name.eq_ignore_ascii_case(name));
        let first = matches
            .next()
            .ok_or_else(|| format!("No native gauge named {name}"))?;
        if matches.any(|g| g.rva != first.rva) {
            return Err(format!(
                "Ambiguous native gauge name {name}: distinct headers"
            ));
        }
        Ok(first)
    }
    /// Exact resource identity, including language. No implicit locale fallback.
    pub fn resource(&self, kind: u32, id: u32, language: u32) -> Option<&Resource> {
        self.resources.iter().find(|r| {
            r.kind == ResourceKey::Id(kind)
                && r.name == ResourceKey::Id(id)
                && r.language == ResourceKey::Id(language)
        })
    }
    pub fn resource_bytes(&self, resource: &Resource) -> Result<&[u8]> {
        // Require an indexed descriptor with matching identity and range, not bounds alone.
        let own = self.resources.iter().any(|r| {
            r.kind == resource.kind
                && r.name == resource.name
                && r.language == resource.language
                && r.range == resource.range
                && r.code_page == resource.code_page
        });
        if !own {
            return Err("Resource descriptor is not indexed by this library".into());
        }
        self.data
            .get(resource.range.clone())
            .ok_or_else(|| "Resource range is invalid".into())
    }
    pub fn resource_counts(&self) -> BTreeMap<ResourceKey, usize> {
        let mut counts = BTreeMap::new();
        for r in &self.resources {
            *counts.entry(r.kind.clone()).or_default() += 1;
        }
        counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_string_fields_callbacks_and_runtime_pointers_are_distinct() {
        let mut b = fixture();
        rword(&mut b, 0x1500, 2);
        for (offset, value) in [
            (60, 23),
            (88, 617),
            (116, 0),
            (168, BASE + 0x1804),
            (192, 64),
            (196, 18),
            (208, 0x123456),
            (212, 0x234567),
            (216, 0x345678),
            (280, 400),
            (288, 0x25),
            (300, BASE + 0x1660),
            (308, 0),
            (312, BASE + 0x1800),
            (180, u32::MAX),
            (184, u32::MAX),
            (188, u32::MAX),
            (220, u32::MAX),
        ] {
            rword(&mut b, 0x1500 + offset, value);
        }
        let name = at(0x1500 + 248);
        b[name..name + 15].copy_from_slice(b"Synthetic Face\0");
        b[at(0x1500 + 284)] = 1;
        put16(&mut b, at(0x1500 + 292), 13);
        let library = GaugeLibrary::parse(b.clone(), false).unwrap();
        let element = &library.gauges[0].elements[1];
        let text = element.text.as_ref().unwrap();
        assert!(element.needle.is_none() && element.sprite.is_none());
        assert_eq!(text.tokens, [23, 617, 0]);
        assert_eq!(text.source_callbacks, [Some(0x1804), None, None]);
        assert_eq!(text.update_callback_rva, Some(0x1800));
        assert_eq!(text.size, [64, 18]);
        assert_eq!(text.font_name, "Synthetic Face");
        assert_eq!(
            (
                text.font_weight,
                text.font_charset,
                text.font_height,
                text.max_length,
                text.draw_text_flags
            ),
            (400, 1, 0, 13, 0x25)
        );
        assert_eq!(text.colors, [0x123456, 0x234567, 0x345678]);
        assert_eq!(text.selection_sequence_rva, Some(0x1660));
        for (offset, value) in [
            (168, u32::MAX),
            (312, u32::MAX),
            (300, u32::MAX),
            (192, 65537),
            (308, i32::MIN as u32),
        ] {
            let mut bad = b.clone();
            rword(&mut bad, 0x1500 + offset, value);
            assert!(GaugeLibrary::parse(bad, false).is_err(), "offset {offset}");
        }
        let mut unterminated = b.clone();
        unterminated[name..name + 32].fill(b'X');
        assert!(GaugeLibrary::parse(unterminated, false).is_err());
        b[name] = 0xe9;
        assert!(GaugeLibrary::parse(b.clone(), false).is_err());
        assert!(GaugeLibrary::parse(b, true).unwrap().gauges[0].elements[1]
            .text
            .as_ref()
            .unwrap()
            .font_name
            .starts_with('é'));
    }
    const BASE: u32 = 0x400000;
    #[test]
    fn sprite_fields_and_callback_addresses_are_bounded() {
        let mut b = fixture();
        rword(&mut b, 0x1500, 6);
        for (offset, token, scale) in [(60, 0, 1.), (100, 78, 1.1764705882352942), (140, 80, -1.)] {
            rword(&mut b, 0x1500 + offset, token);
            let p = at(0x1500 + offset + 32);
            b[p..p + 8].copy_from_slice(&f64::to_le_bytes(scale));
        }
        rword(&mut b, 0x1500 + 180, 1f32.to_bits());
        rword(&mut b, 0x1500 + 184, 0.5f32.to_bits());
        rword(&mut b, 0x1500 + 320, BASE + 0x1800);
        let lib = GaugeLibrary::parse(b.clone(), false).unwrap();
        let e = &lib.gauges[0].elements[1];
        assert!(e.needle.is_none());
        let s = e.sprite.as_ref().unwrap();
        assert_eq!(s.axes[1].token, 78);
        assert_eq!(s.axes[2].scale, -1.);
        assert_eq!(s.axes[1].update_callback_rva, Some(0x1800));
        assert_eq!(s.texture_scale, [1., 0.5]);
        for (offset, value) in [
            (180, f32::NAN.to_bits()),
            (184, f32::INFINITY.to_bits()),
            (88, u32::MAX),
            (320, u32::MAX),
        ] {
            let mut bad = b.clone();
            rword(&mut bad, 0x1500 + offset, value);
            assert!(GaugeLibrary::parse(bad, false).is_err());
        }
        let p = at(0x1500 + 132);
        b[p..p + 8].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(GaugeLibrary::parse(b, false).is_err());
    }
    #[test]
    fn native_needle_fields_and_table_pointer_bounds() {
        let mut b = fixture();
        for (r, v) in [
            (0x1508, 0),
            (0x150c, 0),
            (0x1518, 2),
            (0x151c, 3),
            (0x153c, 61),
            (0x1570, BASE + 0x1600),
            (0x1574, 2),
            (0x1578, 12),
            (0x15e0, BASE + 0x1800),
            (0x1600, 10),
            (0x1604, 0),
            (0x1618, 0),
            (0x161c, 10),
        ] {
            rword(&mut b, r, v);
        }
        b[at(0x1620)..at(0x1620) + 8].copy_from_slice(&100f64.to_le_bytes());
        let lib = GaugeLibrary::parse(b.clone(), false).unwrap();
        let e = &lib.gauges[0].elements[1];
        let n = e.needle.as_ref().unwrap();
        assert_eq!(e.offset, [2, 3]);
        assert_eq!(n.token, 61);
        assert_eq!(n.max_degrees_per_tick, 12);
        assert_eq!(n.update_callback_rva, Some(0x1800));
        assert!(
            (n.reading_angle(e.position, 50.).unwrap() - std::f64::consts::FRAC_PI_4).abs() < 1e-12
        );
        for (r, v) in [
            (0x1574, 513),
            (0x1570, 0),
            (0x15e0, u32::MAX),
            (0x1600, 65537),
        ] {
            let mut bad = b.clone();
            rword(&mut bad, r, v);
            assert!(GaugeLibrary::parse(bad, false).is_err());
        }
        b[at(0x1608)..at(0x1608) + 8].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(GaugeLibrary::parse(b, false).is_err());
    }
    #[test]
    fn needle_interpolation_wraps_clockwise_and_supports_decreasing_values() {
        let mut n = Needle {
            token: 0,
            update_callback_rva: None,
            max_degrees_per_tick: 0,
            curve: vec![
                NeedlePoint {
                    position: [-10, 2],
                    value: 0.,
                },
                NeedlePoint {
                    position: [-10, -2],
                    value: 100.,
                },
            ],
        };
        assert!((n.reading_angle([0, 0], 50.).unwrap() - std::f64::consts::PI).abs() < 1e-12);
        n.curve = vec![
            NeedlePoint {
                position: [10, 0],
                value: 100.,
            },
            NeedlePoint {
                position: [0, 10],
                value: 0.,
            },
        ];
        assert!(
            (n.reading_angle([0, 0], 75.).unwrap() - std::f64::consts::FRAC_PI_4 / 2.).abs()
                < 1e-12
        );
        assert_eq!(n.reading_angle([0, 0], 101.).unwrap(), 0.);
        assert!(
            (n.reading_angle([0, 0], -1.).unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-12
        );
        assert!(n.reading_angle([0, 0], f64::NAN).is_err());
        n.curve[1].value = 100.;
        assert!(n.reading_angle([0, 0], 50.).is_err());
        n.curve[1].value = 0.;
        n.curve[0].position = [0, 0];
        assert!(n.reading_angle([0, 0], 50.).is_err());
        n.curve.clear();
        assert!(n.reading_angle([0, 0], 50.).is_err());
    }
    fn put16(b: &mut [u8], p: usize, v: u16) {
        b[p..p + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn put32(b: &mut [u8], p: usize, v: u32) {
        b[p..p + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn at(rva: u32) -> usize {
        (rva - 0x1000 + 0x200) as usize
    }
    fn rword(b: &mut [u8], rva: u32, v: u32) {
        put32(b, at(rva), v);
    }
    // An original, non-executable synthetic PE container. No compiler/SDK/game fixture.
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 0x1200];
        b[..2].copy_from_slice(b"MZ");
        put32(&mut b, 0x3c, 0x80);
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        put16(&mut b, 0x84, 0x14c);
        put16(&mut b, 0x86, 1);
        put16(&mut b, 0x94, 224);
        let opt = 0x98;
        put16(&mut b, opt, 0x10b);
        put32(&mut b, opt + 28, BASE);
        put32(&mut b, opt + 56, 0x2000);
        put32(&mut b, opt + 60, 0x200);
        put32(&mut b, opt + 92, 16);
        put32(&mut b, opt + 96, 0x1000);
        put32(&mut b, opt + 100, 0x100);
        put32(&mut b, opt + 112, 0x1700);
        put32(&mut b, opt + 116, 0x200);
        let section = opt + 224;
        for (p, v) in [(8, 0x1000), (12, 0x1000), (16, 0x1000), (20, 0x200)] {
            put32(&mut b, section + p, v);
        }
        for (rva, v) in [
            (0x1014, 1),
            (0x1018, 1),
            (0x101c, 0x1060),
            (0x1020, 0x1040),
            (0x1024, 0x1050),
            (0x1040, 0x1070),
            (0x1060, 0x1100),
            (0x1118, BASE + 0x1200),
            (0x1200, 800),
            (0x1204, BASE + 0x1300),
            (0x1208, BASE + 0x1320),
            (0x122c, 100),
            (0x1320, BASE + 0x1400),
            (0x1324, 1), // adjacent data is not a second root or array terminator
            (0x1404, 101),
            (0x1408, 2),
            (0x140c, 3),
            (0x1428, BASE + 0x1450),
            (0x1430, 1),
            (0x1450, BASE + 0x1500),
            (0x1500, 1),
            (0x1504, 102),
            (0x1508, u32::MAX - 1),
            (0x150c, 4),
            (0x1710, 2),
            (0x1714, 0x80000020),
            (0x1730, 101),
            (0x1734, 0x80000040),
            (0x1750, 1033),
            (0x1754, 0x60),
            (0x1760, 0x1900),
            (0x1764, 4),
            (0x1768, 1252),
        ] {
            rword(&mut b, rva, v);
        }
        for rva in [0x170e, 0x172e, 0x174e] {
            put16(&mut b, at(rva), 1);
        }
        b[at(0x1070)..at(0x1070) + 8].copy_from_slice(b"Linkage\0");
        b[at(0x1300)..at(0x1300) + 9].copy_from_slice(b"Own Dial\0");
        b[at(0x1900)..at(0x1900) + 4].copy_from_slice(b"OWN!");
        b
    }
    #[test]
    fn catalog_graph_and_exact_localized_resource_data() {
        let lib = GaugeLibrary::parse(fixture(), false).unwrap();
        assert_eq!(lib.gauges.len(), 1);
        let gauge = lib.gauge("oWn dIaL").unwrap();
        assert_eq!(gauge.version, 800);
        assert_eq!(gauge.size_mm, [100, 0]);
        assert_eq!(gauge.roots, [0x1400]);
        assert_eq!(gauge.elements.len(), 2);
        assert_eq!(gauge.elements[0].children, [0x1500]);
        assert_eq!(gauge.elements[0].position, [2, 3]);
        assert_eq!(gauge.elements[0].image_flags, 1);
        assert_eq!(gauge.elements[1].position, [-2, 4]);
        assert_eq!(gauge.elements[1].kind, 1);
        assert_eq!(gauge.elements[1].resource_id, 102);
        let bitmap = lib.resource(2, 101, 1033).unwrap();
        assert_eq!(bitmap.code_page, 1252);
        assert_eq!(lib.resource_bytes(bitmap).unwrap(), b"OWN!");
        assert!(lib.resource(2, 101, 0).is_none());
        assert_eq!(lib.resource_counts().get(&ResourceKey::Id(2)), Some(&1));
    }
    #[test]
    fn every_truncation_is_an_error_without_a_panic() {
        let b = fixture();
        for length in 0..b.len() {
            assert!(
                GaugeLibrary::parse(b[..length].to_vec(), false).is_err(),
                "length {length}"
            );
        }
        assert!(GaugeLibrary::parse(vec![0; MAX_FILE + 1], false).is_err());
    }
    #[test]
    fn checked_addresses_versions_graphs_exports_and_resource_depth() {
        for (rva, value) in [
            (0x1014, MAX_TABLE + 1),
            (0x1040, u32::MAX),
            (0x1060, 0x1070),
            (0x1118, BASE - 1),
            (0x1200, 1234),
            (0x1208, u32::MAX),
            (0x1450, BASE + 0x1400), // cycle through the static image's child list
            (0x1714, 0x80000000),    // recursive resource root
            (0x1734, 0x60),          // leaf at name level rather than language level
            (0x1754, 0x80000060),
            (0x1760, u32::MAX),
            (0x1764, u32::MAX),
        ] {
            let mut b = fixture();
            rword(&mut b, rva, value);
            assert!(
                GaugeLibrary::parse(b, false).is_err(),
                "RVA {rva:x}, value {value:x}"
            );
        }
        let mut b = fixture();
        put16(&mut b, at(0x1050), 1);
        assert!(GaugeLibrary::parse(b, false).is_err());
        let mut b = fixture();
        put16(&mut b, 0x84, 0x8664);
        assert!(GaugeLibrary::parse(b, false).is_err());
        let mut b = fixture();
        put16(&mut b, 0x86, 2);
        let section = b[0x178..0x1a0].to_vec();
        b[0x1a0..0x1c8].copy_from_slice(&section);
        assert!(GaugeLibrary::parse(b, false).is_err());
    }
    #[test]
    fn named_resource_keys_and_explicit_legacy_gauge_encoding() {
        let mut b = fixture();
        rword(&mut b, 0x1730, 0x80000080);
        put16(&mut b, at(0x1780), 3);
        for (i, c) in "Own".encode_utf16().enumerate() {
            put16(&mut b, at(0x1782) + i * 2, c);
        }
        b[at(0x1300)] = 0xe9;
        assert!(GaugeLibrary::parse(b.clone(), false).is_err());
        let lib = GaugeLibrary::parse(b, true).unwrap();
        assert_eq!(lib.gauges[0].name, "éwn Dial");
        assert_eq!(lib.resources[0].name, ResourceKey::Name("Own".into()));
        assert_eq!(lib.resource_bytes(&lib.resources[0]).unwrap(), b"OWN!");
    }
    #[test]
    fn common_header_prefixes_and_runtime_only_data_are_distinguished() {
        for version in [800, 900, 1000] {
            let mut b = fixture();
            rword(&mut b, 0x1200, version);
            let lib = GaugeLibrary::parse(b, false).unwrap();
            assert_eq!(lib.gauges[0].version, version);
            assert_eq!(lib.gauges[0].elements.len(), 2);
        }
        let mut b = fixture();
        rword(&mut b, 0x1208, 0); // owner-drawn/runtime-generated gauge, no encoded root
        assert!(GaugeLibrary::parse(b, false).unwrap().gauges[0]
            .elements
            .is_empty());
        let mut b = fixture();
        put32(&mut b, 0x98 + 56, 0x3000);
        put32(&mut b, 0x178 + 8, 0x2000); // virtual zero-fill beyond the file-backed range
        rword(&mut b, 0x1208, BASE + 0x2100);
        assert!(GaugeLibrary::parse(b, false).is_err());
    }
    #[test]
    fn repeated_header_aliases_resolve_but_distinct_same_name_headers_are_ambiguous() {
        let mut b = fixture();
        rword(&mut b, 0x111c, BASE + 0x1200);
        let library = GaugeLibrary::parse(b.clone(), false).unwrap();
        assert_eq!(library.gauges.len(), 2);
        assert_eq!(library.gauge("Own Dial").unwrap().rva, 0x1200);
        assert!(library.gauge("Missing").is_err());
        let header = b[at(0x1200)..at(0x1200) + 52].to_vec();
        b[at(0x1260)..at(0x1260) + 52].copy_from_slice(&header);
        rword(&mut b, 0x111c, BASE + 0x1260);
        let library = GaugeLibrary::parse(b, false).unwrap();
        assert!(library.gauge("Own Dial").is_err());
    }
}
