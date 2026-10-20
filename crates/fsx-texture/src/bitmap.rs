//! Windows BITMAPINFOHEADER / BI_RGB, independent of GDI and gauge execution.
use super::{Image, Result, MAX_DIMENSION, MAX_FILE};

fn range(b: &[u8], offset: usize, size: usize) -> Result<&[u8]> {
    b.get(offset..offset.checked_add(size).ok_or("Bitmap offset overflow")?)
        .ok_or_else(|| "Truncated bitmap data".into())
}
fn word(b: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(range(b, offset, 4)?.try_into().unwrap()))
}
fn short(b: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(range(b, offset, 2)?.try_into().unwrap()))
}

fn decode(b: &[u8], pixel_offset: Option<usize>, explicit_alpha: bool) -> Result<Image> {
    if b.len() as u64 > MAX_FILE || word(b, 0)? != 40 || short(b, 12)? != 1 {
        return Err("Expected a bounded 40-byte BITMAPINFOHEADER with one plane".into());
    }
    let width = word(b, 4)? as i32;
    let height = word(b, 8)? as i32;
    let bits = short(b, 14)?;
    if width <= 0
        || height == 0
        || width as u32 > MAX_DIMENSION
        || height.unsigned_abs() > MAX_DIMENSION
    {
        return Err("Bitmap dimensions must be between 1 and 4096".into());
    }
    if (explicit_alpha && bits != 32)
        || (!explicit_alpha && ![4, 8, 24].contains(&bits))
        || word(b, 16)? != 0
    {
        return Err(if explicit_alpha {
            "Explicit BMP alpha needs uncompressed 32-bit BI_RGB"
        } else {
            "Ordinary BMP needs uncompressed BI_RGB 4/8/24-bit data"
        }
        .into());
    }
    let colors = word(b, 32)? as usize;
    let colors = if colors == 0 && bits <= 8 {
        1usize << bits
    } else {
        colors
    };
    if colors > 256 || (bits <= 8 && colors > 1usize << bits) {
        return Err("Invalid or oversized bitmap color table".into());
    }
    let palette = range(b, 40, colors * 4)?;
    let minimum_offset = 40 + colors * 4;
    let offset = pixel_offset.unwrap_or(minimum_offset);
    if offset < minimum_offset {
        return Err("BMP pixel offset overlaps the header or palette".into());
    }
    let width = width as usize;
    let rows = height.unsigned_abs() as usize;
    let stride = (width * bits as usize).div_ceil(32) * 4;
    let required = stride * rows;
    let declared = word(b, 20)? as usize;
    if declared != 0 && (declared < required || range(b, offset, declared).is_err()) {
        return Err("Invalid bitmap image-size field".into());
    }
    let source = range(b, offset, required)?;
    let mut rgba = vec![0; width * rows * 4];
    for y in 0..rows {
        let source_y = if height > 0 { rows - 1 - y } else { y };
        let row = &source[source_y * stride..(source_y + 1) * stride];
        for x in 0..width {
            let color = if bits == 32 {
                let p = x * 4;
                [row[p + 2], row[p + 1], row[p], row[p + 3]]
            } else if bits == 24 {
                let p = x * 3;
                [row[p + 2], row[p + 1], row[p], 255]
            } else {
                let index = if bits == 8 {
                    row[x] as usize
                } else {
                    ((row[x / 2] >> if x % 2 == 0 { 4 } else { 0 }) & 15) as usize
                };
                if index >= colors {
                    return Err("Bitmap pixel references a missing palette entry".into());
                }
                let p = index * 4;
                [palette[p + 2], palette[p + 1], palette[p], 255]
            };
            rgba[(y * width + x) * 4..(y * width + x + 1) * 4].copy_from_slice(&color);
        }
    }
    Ok(Image {
        width: width as u32,
        height: rows as u32,
        rgba,
    })
}

impl Image {
    /// Packed native RT_BITMAP resource: no BITMAPFILEHEADER and no implicit transparency.
    pub fn dib(b: &[u8]) -> Result<Self> {
        decode(b, None, false)
    }
    /// Ordinary BMP file. Reserved palette bytes are not an alpha channel.
    pub fn bmp(b: &[u8]) -> Result<Self> {
        Self::bmp_mode(b, false)
    }
    /// Explicit gauge Alpha mode: 32-bit BI_RGB BGRA, straight alpha.
    /// The high byte is not treated as alpha by ordinary bmp()/dib().
    pub fn bmp_alpha(b: &[u8]) -> Result<Self> {
        Self::bmp_mode(b, true)
    }
    fn bmp_mode(b: &[u8], explicit_alpha: bool) -> Result<Self> {
        if b.len() as u64 > MAX_FILE || range(b, 0, 2)? != b"BM" {
            return Err("Expected a bounded BMP file".into());
        }
        let size = word(b, 2)? as usize;
        let offset = word(b, 10)? as usize;
        if size < 14 || offset < 14 {
            return Err("Invalid BMP file size / pixel offset".into());
        }
        let b = range(b, 0, size)?;
        decode(&b[14..], Some(offset - 14), explicit_alpha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_bgra_alpha_preserves_zero_partial_and_opaque_values() {
        let dib = fixture(
            32,
            2,
            -2,
            &[],
            &[
                30, 20, 10, 0, 60, 50, 40, 128, 90, 80, 70, 255, 120, 110, 100, 1,
            ],
        );
        let mut bmp = vec![0; 14];
        bmp[..2].copy_from_slice(b"BM");
        bmp.extend_from_slice(&dib);
        let length = bmp.len() as u32;
        put(&mut bmp, 2, length);
        put(&mut bmp, 10, 54);
        assert_eq!(
            Image::bmp_alpha(&bmp).unwrap().rgba,
            [10, 20, 30, 0, 40, 50, 60, 128, 70, 80, 90, 255, 100, 110, 120, 1]
        );
        assert!(Image::bmp(&bmp).is_err());
        assert!(Image::dib(&dib).is_err());
        for n in 0..bmp.len() {
            assert!(Image::bmp_alpha(&bmp[..n]).is_err(), "truncated {n}");
        }
        let mut bottom = bmp.clone();
        put(&mut bottom, 22, 2);
        assert_eq!(
            Image::bmp_alpha(&bottom).unwrap().rgba[..8],
            [70, 80, 90, 255, 100, 110, 120, 1]
        );
        for bits in [4u16, 8, 24] {
            let mut bad = bmp.clone();
            bad[28..30].copy_from_slice(&bits.to_le_bytes());
            assert!(Image::bmp_alpha(&bad).is_err());
        }
    }
    fn put(b: &mut [u8], p: usize, value: u32) {
        b[p..p + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn fixture(bits: u16, width: i32, height: i32, palette: &[[u8; 4]], pixels: &[u8]) -> Vec<u8> {
        let mut b = vec![0; 40];
        put(&mut b, 0, 40);
        put(&mut b, 4, width as u32);
        put(&mut b, 8, height as u32);
        b[12..14].copy_from_slice(&1u16.to_le_bytes());
        b[14..16].copy_from_slice(&bits.to_le_bytes());
        put(&mut b, 32, palette.len() as u32);
        b.extend(palette.iter().flatten());
        b.extend_from_slice(pixels);
        b
    }
    const COLORS: [[u8; 4]; 3] = [[0, 0, 255, 0], [0, 255, 0, 254], [255, 0, 0, 1]];
    #[test]
    fn palette_order_nibbles_odd_width_and_padded_bottom_up_rows() {
        let b = fixture(
            4,
            3,
            2,
            &COLORS,
            &[0x21, 0x0f, 0xee, 0xee, 0x01, 0x2f, 0xee, 0xee],
        );
        let image = Image::dib(&b).unwrap();
        assert_eq!((image.width, image.height), (3, 2));
        assert_eq!(
            image.rgba,
            [
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 255, 0, 255,
                255, 0, 0, 255
            ]
        );
        // Low nibble and row padding are not pixels; reserved palette bytes stay opaque.
    }
    #[test]
    fn eight_bit_default_palette_and_top_down_orientation() {
        let mut palette = [[0; 4]; 256];
        palette[0] = COLORS[0];
        palette[255] = COLORS[2];
        let mut b = fixture(8, 1, -2, &palette, &[0, 99, 99, 99, 255, 99, 99, 99]);
        put(&mut b, 32, 0);
        assert_eq!(
            Image::dib(&b).unwrap().rgba,
            [255, 0, 0, 255, 0, 0, 255, 255]
        );
    }
    #[test]
    fn bgr24_optional_device_palette_file_offset_gap_and_declared_extent() {
        let dib = fixture(24, 1, 2, &COLORS[..1], &[255, 0, 0, 99, 0, 0, 255, 99]);
        assert_eq!(
            Image::dib(&dib).unwrap().rgba,
            [255, 0, 0, 255, 0, 0, 255, 255]
        );
        let mut bmp = vec![0; 14];
        bmp[..2].copy_from_slice(b"BM");
        bmp.extend_from_slice(&dib[..44]);
        bmp.extend_from_slice(&[77; 3]);
        bmp.extend_from_slice(&dib[44..]);
        let size = bmp.len() as u32;
        put(&mut bmp, 2, size);
        put(&mut bmp, 10, 61);
        assert_eq!(
            Image::bmp(&bmp).unwrap().rgba,
            Image::dib(&dib).unwrap().rgba
        );
        bmp.extend_from_slice(&[9; 6]); // file trailer outside bfSize is not image data
        assert!(Image::bmp(&bmp).is_ok());
        put(&mut bmp, 10, 54);
        assert!(Image::bmp(&bmp).is_err());
    }
    #[test]
    fn truncation_bad_indices_sizes_compression_and_unsupported_formats_are_errors() {
        let dib = fixture(8, 1, 1, &COLORS, &[2, 0, 0, 0]);
        for len in 0..dib.len() {
            assert!(Image::dib(&dib[..len]).is_err(), "{len}");
        }
        for (p, v) in [
            (0, 108),
            (4, 0),
            (4, u32::MAX),
            (8, 0),
            (8, 0x80000000),
            (4, 4097),
            (16, 1),
            (20, 1),
            (20, u32::MAX),
            (32, 257),
        ] {
            let mut b = dib.clone();
            put(&mut b, p, v);
            assert!(Image::dib(&b).is_err(), "{p}={v}");
        }
        let mut b = dib.clone();
        b[52] = 3;
        assert!(Image::dib(&b).is_err());
        for bits in [1u16, 16, 32] {
            let mut b = dib.clone();
            b[14..16].copy_from_slice(&bits.to_le_bytes());
            assert!(Image::dib(&b).is_err());
        }
    }
}
