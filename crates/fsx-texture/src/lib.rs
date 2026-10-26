//! Read-only DDS/bitmap decoding and texture.cfg fallback lookup.
//! Only the top mip of ordinary 2D DXT1/3/5 or masked 32-bit RGB DDS is decoded.
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    fs,
    io::Read,
    path::{Path, PathBuf},
};
mod bitmap;

type Result<T> = std::result::Result<T, String>;
const MAX_FILE: u64 = 256 * 1024 * 1024;
pub const MAX_DIMENSION: u32 = 4096;

#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn read_image(path: &Path) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    fs::File::open(path)
        .and_then(|f| f.take(MAX_FILE + 1).read_to_end(&mut data))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if data.len() as u64 > MAX_FILE {
        return Err("Image exceeds the 256 MiB file limit".into());
    }
    Ok(data)
}

fn word(b: &[u8], offset: usize) -> Result<u32> {
    b.get(offset..offset + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| "Truncated DDS header".into())
}
fn rgb565(n: u16) -> [u8; 4] {
    let (r, g, b) = ((n >> 11) as u8, ((n >> 5) & 63) as u8, (n & 31) as u8);
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
        255,
    ]
}
fn color_block(b: &[u8], four_colors: bool) -> [[u8; 4]; 16] {
    let a = u16::from_le_bytes([b[0], b[1]]);
    let z = u16::from_le_bytes([b[2], b[3]]);
    let mut colors = [rgb565(a), rgb565(z), [0, 0, 0, 255], [0, 0, 0, 255]];
    if a > z || four_colors {
        for c in 0..3 {
            colors[2][c] = ((2 * colors[0][c] as u16 + colors[1][c] as u16) / 3) as u8;
            colors[3][c] = ((colors[0][c] as u16 + 2 * colors[1][c] as u16) / 3) as u8;
        }
    } else {
        for c in 0..3 {
            colors[2][c] = ((colors[0][c] as u16 + colors[1][c] as u16) / 2) as u8;
        }
        colors[3] = [0, 0, 0, 0];
    }
    let bits = u32::from_le_bytes(b[4..8].try_into().unwrap());
    std::array::from_fn(|i| colors[((bits >> (i * 2)) & 3) as usize])
}
fn alpha_bc3(b: &[u8]) -> [u8; 16] {
    let mut palette = [b[0], b[1], 0, 0, 0, 0, 0, 255];
    if b[0] > b[1] {
        for i in 1..=6 {
            palette[i + 1] = (((7 - i) * b[0] as usize + i * b[1] as usize) / 7) as u8;
        }
    } else {
        for i in 1..=4 {
            palette[i + 1] = (((5 - i) * b[0] as usize + i * b[1] as usize) / 5) as u8;
        }
    }
    let bits = b[2..8]
        .iter()
        .enumerate()
        .fold(0u64, |n, (i, &x)| n | ((x as u64) << (i * 8)));
    std::array::from_fn(|i| palette[((bits >> (i * 3)) & 7) as usize])
}

impl Image {
    pub fn load_dds(path: &Path) -> Result<Self> {
        Self::dds(&read_image(path)?)
    }
    /// Detect an ordinary DDS or BMP file by signature, never by extension.
    pub fn load(path: &Path) -> Result<Self> {
        let data = read_image(path)?;
        if data.starts_with(b"DDS ") {
            Self::dds(&data)
        } else if data.starts_with(b"BM") && data.get(30..34).is_some_and(is_dxt) {
            Self::bmp_dxt(&data)
        } else if data.starts_with(b"BM") {
            Self::bmp(&data)
        } else {
            Err("Expected a supported DDS or BMP image".into())
        }
    }
    /// FSX terrain textures: a BITMAPINFOHEADER whose compression field is a
    /// DXT FourCC, followed by block data (top mip first) at bfOffBits.
    pub fn bmp_dxt(b: &[u8]) -> Result<Self> {
        let (offset, width, height) = (word(b, 10)? as usize, word(b, 18)?, word(b, 22)?);
        let format: [u8; 4] = b
            .get(30..34)
            .ok_or("Truncated BMP header")?
            .try_into()
            .unwrap();
        if !is_dxt(&format) || offset < 54 || offset > b.len() {
            return Err("Invalid DXT BMP header".into());
        }
        // Reuse the DDS block decoder with an equivalent legacy header.
        let mut dds = vec![0u8; 128];
        dds[..4].copy_from_slice(b"DDS ");
        for (at, v) in [(4, 124u32), (12, height), (16, width), (76, 32), (80, 4)] {
            dds[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        dds[84..88].copy_from_slice(&format);
        dds.extend_from_slice(&b[offset..]);
        Self::dds(&dds)
    }
    pub fn dds(b: &[u8]) -> Result<Self> {
        if b.len() < 128 || &b[..4] != b"DDS " || word(b, 4)? != 124 || word(b, 76)? != 32 {
            return Err("Expected a legacy DDS header".into());
        }
        let (width, height) = (word(b, 16)?, word(b, 12)?);
        if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err("DDS dimensions must be between 1 and 4096".into());
        }
        if word(b, 112)? & 0x20fe00 != 0 || word(b, 24)? > 1 {
            return Err("Cubemap and volume DDS are not supported".into());
        }
        let mut rgba = vec![0; (width * height * 4) as usize];
        let flags = word(b, 80)?;
        if flags & 4 != 0 {
            let format = &b[84..88];
            let size = match format {
                b"DXT1" => 8,
                b"DXT3" | b"DXT5" => 16,
                _ => {
                    return Err(format!(
                        "Unsupported DDS FourCC: {:?}",
                        String::from_utf8_lossy(format)
                    ))
                }
            };
            let columns = width.div_ceil(4) as usize;
            let rows = height.div_ceil(4) as usize;
            let data = b
                .get(128..128 + columns * rows * size)
                .ok_or("Truncated DDS top mip")?;
            for (i, block) in data.chunks_exact(size).enumerate() {
                let mut pixels =
                    color_block(if size == 8 { block } else { &block[8..] }, size != 8);
                if format == b"DXT3" {
                    let alpha = u64::from_le_bytes(block[..8].try_into().unwrap());
                    for (p, color) in pixels.iter_mut().enumerate() {
                        color[3] = (((alpha >> (p * 4)) & 15) * 17) as u8;
                    }
                } else if format == b"DXT5" {
                    for (color, alpha) in pixels.iter_mut().zip(alpha_bc3(&block[..8])) {
                        color[3] = alpha;
                    }
                }
                for (p, color) in pixels.iter().enumerate() {
                    let (x, y) = ((i % columns) * 4 + p % 4, (i / columns) * 4 + p / 4);
                    if x < width as usize && y < height as usize {
                        let out = (y * width as usize + x) * 4;
                        rgba[out..out + 4].copy_from_slice(color);
                    }
                }
            }
        } else if flags & 0x40 != 0 && word(b, 88)? == 32 {
            let masks = [
                word(b, 92)?,
                word(b, 96)?,
                word(b, 100)?,
                if flags & 1 != 0 { word(b, 104)? } else { 0 },
            ];
            let mut combined = 0u32;
            for (i, &mask) in masks.iter().enumerate() {
                if mask == 0 {
                    if i < 3 {
                        return Err("Missing DDS RGB mask".into());
                    }
                    continue;
                }
                let normalized = mask >> mask.trailing_zeros();
                if normalized & (normalized.wrapping_add(1)) != 0 || combined & mask != 0 {
                    return Err("Noncontiguous or overlapping DDS channel masks".into());
                }
                combined |= mask;
            }
            let pitch = if word(b, 8)? & 8 != 0 {
                word(b, 20)? as usize
            } else {
                width as usize * 4
            };
            if pitch < width as usize * 4 {
                return Err("DDS pitch is shorter than its pixels".into());
            }
            let needed = pitch
                .checked_mul(height as usize)
                .and_then(|n| 128usize.checked_add(n))
                .ok_or("DDS pitch overflow")?;
            let data = b.get(128..needed).ok_or("Truncated DDS top mip")?;
            for y in 0..height as usize {
                for x in 0..width as usize {
                    let offset = y * pitch + x * 4;
                    let pixel = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
                    let out = (y * width as usize + x) * 4;
                    for (i, &mask) in masks.iter().enumerate() {
                        rgba[out + i] = if mask == 0 {
                            255
                        } else {
                            let shift = mask.trailing_zeros();
                            (((pixel & mask) >> shift) as u64 * 255 / (mask >> shift) as u64) as u8
                        };
                    }
                }
            }
        } else {
            return Err("Only DXT1/3/5 and masked 32-bit RGB DDS are supported".into());
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }
}

fn case_file(folder: &Path, name: &str) -> Result<Option<PathBuf>> {
    let direct = folder.join(name);
    if direct.is_file() {
        return Ok(Some(direct));
    }
    if !folder.is_dir() {
        return Ok(None);
    }
    for item in fs::read_dir(folder).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        if item
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(name)
            && item.path().is_file()
        {
            return Ok(Some(item.path()));
        }
    }
    Ok(None)
}
fn fallbacks(text: &str) -> Vec<String> {
    let mut active = false;
    let mut ordered = BTreeMap::new();
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.split(';').next().unwrap_or("").trim();
        if line.starts_with('[') {
            active = line.eq_ignore_ascii_case("[fltsim]");
            continue;
        }
        if !active {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            if let Some(number) = key
                .trim()
                .to_ascii_lowercase()
                .strip_prefix("fallback.")
                .and_then(|n| n.parse::<u32>().ok())
            {
                let value = value.trim().trim_matches('"');
                if !value.is_empty() {
                    ordered.insert(number, value.to_owned());
                }
            }
        }
    }
    ordered.into_values().collect()
}

/// Search the explicitly selected livery folder, then its ordered fallback graph.
/// Cycles and absent fallback directories are tolerated; traversal is capped.
pub fn find(root: &Path, name: &str) -> Result<Option<PathBuf>> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':', '\0']) {
        return Err("Texture reference must be a single file name".into());
    }
    let mut pending = VecDeque::from([root.to_path_buf()]);
    let mut visited = HashSet::new();
    while let Some(folder) = pending.pop_front() {
        if !folder.is_dir() {
            continue;
        }
        let folder = fs::canonicalize(&folder).map_err(|e| e.to_string())?;
        if !visited.insert(folder.clone()) {
            continue;
        }
        if visited.len() > 64 {
            return Err("Texture fallback graph exceeds 64 directories".into());
        }
        if let Some(path) = case_file(&folder, name)? {
            return Ok(Some(path));
        }
        if let Some(config) = case_file(&folder, "texture.cfg")? {
            if fs::metadata(&config).map_err(|e| e.to_string())?.len() > 65536 {
                return Err("texture.cfg exceeds 64 KiB".into());
            }
            let content = fs::read_to_string(config).map_err(|e| e.to_string())?;
            let paths = fallbacks(&content);
            if paths.len() > 64 {
                return Err("Too many texture fallback entries".into());
            }
            for value in paths {
                pending.push_back(folder.join(value.replace('\\', std::path::MAIN_SEPARATOR_STR)));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dds(format: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut b = vec![0; 128];
        b[..4].copy_from_slice(b"DDS ");
        for (p, value) in [(4, 124u32), (12, 4), (16, 4), (76, 32), (80, 4)] {
            b[p..p + 4].copy_from_slice(&value.to_le_bytes());
        }
        b[84..88].copy_from_slice(format);
        b.extend_from_slice(data);
        b
    }
    #[test]
    fn bc1_endpoint_order_and_index_bits() {
        let b = dds(b"DXT1", &[0, 248, 224, 7, 0b11100100, 0, 0, 0]);
        let image = Image::dds(&b).unwrap();
        assert_eq!(
            &image.rgba[..16],
            &[255, 0, 0, 255, 0, 255, 0, 255, 170, 85, 0, 255, 85, 170, 0, 255]
        );
        let b = dds(b"DXT1", &[0, 0, 255, 255, 3, 0, 0, 0]);
        assert_eq!(&Image::dds(&b).unwrap().rgba[..4], &[0, 0, 0, 0]);
    }
    #[test]
    fn bc2_explicit_alpha_and_bc3_both_palettes() {
        let mut block = [0; 16];
        block[0] = 0xf0;
        block[8..10].copy_from_slice(&0xf800u16.to_le_bytes());
        let image = Image::dds(&dds(b"DXT3", &block)).unwrap();
        assert_eq!(image.rgba[3], 0);
        assert_eq!(image.rgba[7], 255);
        let mut alpha = [0; 8];
        alpha[0] = 255;
        alpha[1] = 0;
        alpha[2] = 2;
        assert_eq!(alpha_bc3(&alpha)[0], 218);
        alpha[0] = 0;
        alpha[1] = 255;
        alpha[2] = 7;
        assert_eq!(alpha_bc3(&alpha)[0], 255);
        block[..8].copy_from_slice(&alpha);
        assert_eq!(Image::dds(&dds(b"DXT5", &block)).unwrap().rgba[3], 255);
    }
    #[test]
    fn truncation_dimensions_and_cube_are_errors() {
        let original = dds(b"DXT5", &[0; 16]);
        for n in 0..original.len() {
            assert!(Image::dds(&original[..n]).is_err());
        }
        let mut b = original.clone();
        b[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Image::dds(&b).is_err());
        let mut b = original;
        b[112..116].copy_from_slice(&0x200u32.to_le_bytes());
        assert!(Image::dds(&b).is_err());
    }
    #[test]
    fn bgra_masks_and_padding() {
        let mut b = dds(b"NONE", &[3, 2, 1, 4, 0, 0, 0, 0]);
        for (p, value) in [
            (8, 8u32),
            (12, 1),
            (16, 1),
            (20, 8),
            (80, 65),
            (88, 32),
            (92, 0xff0000),
            (96, 0xff00),
            (100, 0xff),
            (104, 0xff000000),
        ] {
            b[p..p + 4].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(Image::dds(&b).unwrap().rgba, [1, 2, 3, 4]);
        b[96..100].copy_from_slice(&0xff0000u32.to_le_bytes());
        assert!(Image::dds(&b).is_err());
    }
    #[test]
    fn block_rows_and_partial_edge_pixels() {
        let red = [0, 248, 0, 0, 0, 0, 0, 0];
        let blue = [31, 0, 0, 0, 0, 0, 0, 0];
        let mut b = dds(b"DXT1", &[red, blue].concat());
        b[12..16].copy_from_slice(&5u32.to_le_bytes());
        b[16..20].copy_from_slice(&2u32.to_le_bytes());
        let image = Image::dds(&b).unwrap();
        assert_eq!(image.rgba.len(), 2 * 5 * 4);
        assert_eq!(&image.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&image.rgba[32..36], &[0, 0, 255, 255]);
    }
    #[test]
    fn fallback_order_case_and_cycles() {
        let root =
            std::env::temp_dir().join(format!("openfsx-texture-test-{}", std::process::id()));
        let a = root.join("a");
        let z = root.join("b");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&z).unwrap();
        fs::write(
            a.join("texture.cfg"),
            "[fltsim]\nfallback.2=..\\missing\nfallback.1=..\\b",
        )
        .unwrap();
        fs::write(z.join("texture.cfg"), "[fltsim]\nfallback.1=..\\a").unwrap();
        fs::write(z.join("Own.DDS"), b"own fixture").unwrap();
        let found = find(&a, "OWN.dds").unwrap().unwrap();
        assert_eq!(fs::read(found).unwrap(), b"own fixture");
        assert!(find(&a, "absent.dds").unwrap().is_none());
        assert!(find(&a, "../Own.DDS").is_err());
        // Only remove known files created by this test; avoid recursive deletion.
        for path in [
            a.join("texture.cfg"),
            z.join("texture.cfg"),
            z.join("Own.DDS"),
        ] {
            fs::remove_file(path).unwrap();
        }
        fs::remove_dir(a).unwrap();
        fs::remove_dir(z).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
fn is_dxt(format: &[u8]) -> bool {
    matches!(format, b"DXT1" | b"DXT3" | b"DXT5")
}
#[cfg(test)]
mod dxt_bmp_tests {
    use super::*;
    #[test]
    fn installed_terrain_texture_decodes_when_present() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../FSX/Scenery/World/texture");
        if !dir.is_dir() {
            return;
        }
        for name in [
            "001b2su1", "012b2su1", "031b2su1", "030b2su1", "035b2su1", "056b2su1", "139b2su1",
            "123b2su1",
        ] {
            let i = Image::load(&dir.join(format!("{name}.bmp"))).unwrap();
            let n = (i.width * i.height) as f64;
            let avg: Vec<f64> = (0..3)
                .map(|c| {
                    i.rgba
                        .iter()
                        .skip(c)
                        .step_by(4)
                        .map(|&v| f64::from(v))
                        .sum::<f64>()
                        / n
                })
                .collect();
            eprintln!("{name} {}x{} avg {:.0?}", i.width, i.height, avg);
            assert!(i.width >= 256 && i.width == i.height);
        }
    }
    #[test]
    fn rejects_truncated_dxt_bmp() {
        let mut b = vec![0u8; 54];
        b[..2].copy_from_slice(b"BM");
        b[10..14].copy_from_slice(&54u32.to_le_bytes());
        b[18..22].copy_from_slice(&8u32.to_le_bytes());
        b[22..26].copy_from_slice(&8u32.to_le_bytes());
        b[30..34].copy_from_slice(b"DXT1");
        assert!(Image::bmp_dxt(&b).is_err());
        b.extend([0u8; 32]);
        assert_eq!(Image::bmp_dxt(&b).unwrap().width, 8);
    }
}
