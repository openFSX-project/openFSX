//! RIFF WAVE decoding for FSX sound content: PCM 8/16-bit and Microsoft ADPCM.
//! Output is mono f32 in -1..1; multichannel input is averaged.

#[derive(Debug, Clone, PartialEq)]
pub struct Wave {
    pub rate: u32,
    pub samples: Vec<f32>,
}
const MAX_SAMPLES: usize = 64 << 20;
fn u16_at(d: &[u8], i: usize) -> Result<u16, String> {
    d.get(i..i + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| "Truncated WAVE data".into())
}
fn u32_at(d: &[u8], i: usize) -> Result<u32, String> {
    d.get(i..i + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "Truncated WAVE data".into())
}
impl Wave {
    pub fn parse(d: &[u8]) -> Result<Self, String> {
        if d.get(0..4) != Some(b"RIFF") || d.get(8..12) != Some(b"WAVE") {
            return Err("Not a RIFF WAVE file".into());
        }
        let (mut fmt, mut data) = (None, None);
        let mut i = 12;
        while i + 8 <= d.len() {
            let size = u32_at(d, i + 4)? as usize;
            let body = &d[i + 8..(i + 8).saturating_add(size).min(d.len())];
            match &d[i..i + 4] {
                b"fmt " => fmt = Some(body),
                b"data" => data = Some(body),
                _ => {}
            }
            i = i + 8 + size + (size & 1);
        }
        let fmt = fmt.ok_or("WAVE has no fmt chunk")?;
        let data = data.ok_or("WAVE has no data chunk")?;
        let (tag, channels, rate) = (u16_at(fmt, 0)?, u16_at(fmt, 2)? as usize, u32_at(fmt, 4)?);
        let (align, bits) = (u16_at(fmt, 12)? as usize, u16_at(fmt, 14)?);
        if channels == 0 || channels > 8 || rate == 0 || rate > 192_000 {
            return Err("Unsupported WAVE channel count or rate".into());
        }
        let interleaved: Vec<f32> = match (tag, bits) {
            (1, 8) => data.iter().map(|&b| (f32::from(b) - 128.) / 128.).collect(),
            (1, 16) => data
                .chunks_exact(2)
                .map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.)
                .collect(),
            (2, 4) => adpcm(data, channels, align, fmt)?,
            _ => return Err(format!("Unsupported WAVE format {tag} with {bits} bits")),
        };
        if interleaved.len() > MAX_SAMPLES {
            return Err("WAVE exceeds the sample limit".into());
        }
        let samples = interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect();
        Ok(Self { rate, samples })
    }
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        Self::parse(&std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
            .map_err(|e| format!("{}: {e}", path.display()))
    }
}
const ADAPTATION: [i32; 16] = [
    230, 230, 230, 230, 307, 409, 512, 614, 768, 614, 512, 409, 307, 230, 230, 230,
];
const DEFAULT_COEFFICIENTS: [(i32, i32); 7] = [
    (256, 0),
    (512, -256),
    (0, 0),
    (192, 64),
    (240, 0),
    (460, -208),
    (392, -232),
];
fn adpcm(data: &[u8], channels: usize, align: usize, fmt: &[u8]) -> Result<Vec<f32>, String> {
    let mut coefficients = DEFAULT_COEFFICIENTS.to_vec();
    if fmt.len() >= 22 {
        let count = u16_at(fmt, 20)? as usize;
        if count > 0 && fmt.len() >= 22 + count * 4 {
            coefficients = (0..count)
                .map(|k| {
                    Ok((
                        i32::from(u16_at(fmt, 22 + k * 4)? as i16),
                        i32::from(u16_at(fmt, 24 + k * 4)? as i16),
                    ))
                })
                .collect::<Result<_, String>>()?;
        }
    }
    let header = 7 * channels;
    if align <= header {
        return Err("Invalid ADPCM block alignment".into());
    }
    let mut out = Vec::new();
    for block in data.chunks(align) {
        if block.len() < header {
            break;
        }
        let mut state = Vec::with_capacity(channels);
        for c in 0..channels {
            let predictor = block[c] as usize;
            let &(c1, c2) = coefficients
                .get(predictor)
                .ok_or("Invalid ADPCM predictor")?;
            let delta = i32::from(u16_at(block, channels + 2 * c)? as i16);
            let s1 = i32::from(u16_at(block, 3 * channels + 2 * c)? as i16);
            let s2 = i32::from(u16_at(block, 5 * channels + 2 * c)? as i16);
            state.push([c1, c2, delta, s1, s2]);
        }
        for s in &state {
            out.push(s[4] as f32 / 32768.);
        }
        for s in &state {
            out.push(s[3] as f32 / 32768.);
        }
        let mut channel = 0;
        for &byte in &block[header..] {
            for nibble in [byte >> 4, byte & 15] {
                let [c1, c2, delta, s1, s2] = &mut state[channel];
                let signed = i32::from(nibble as i8) - if nibble & 8 != 0 { 16 } else { 0 };
                let predicted = (*s1 * *c1 + *s2 * *c2) / 256 + signed * *delta;
                let sample = predicted.clamp(-32768, 32767);
                *s2 = *s1;
                *s1 = sample;
                *delta = (ADAPTATION[nibble as usize] * *delta / 256).max(16);
                out.push(sample as f32 / 32768.);
                channel = (channel + 1) % channels;
            }
        }
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn riff(fmt: &[u8], data: &[u8]) -> Vec<u8> {
        let mut v = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        v.extend((fmt.len() as u32).to_le_bytes());
        v.extend(fmt);
        v.extend(b"data");
        v.extend((data.len() as u32).to_le_bytes());
        v.extend(data);
        v
    }
    fn fmt(tag: u16, channels: u16, align: u16, bits: u16) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend(tag.to_le_bytes());
        f.extend(channels.to_le_bytes());
        f.extend(22050u32.to_le_bytes());
        f.extend(0u32.to_le_bytes());
        f.extend(align.to_le_bytes());
        f.extend(bits.to_le_bytes());
        f
    }
    #[test]
    fn decodes_pcm_and_downmixes_stereo() {
        let w = Wave::parse(&riff(&fmt(1, 2, 4, 16), &[0, 64, 0, 0, 0, 192, 0, 0])).unwrap();
        assert_eq!(w.samples, vec![0.25, -0.25]);
        let w = Wave::parse(&riff(&fmt(1, 1, 1, 8), &[128, 0])).unwrap();
        assert_eq!(w.samples, vec![0., -1.]);
        assert!(Wave::parse(b"RIFF").is_err());
        assert!(Wave::parse(&riff(&fmt(3, 1, 4, 32), &[0; 4])).is_err());
    }
    #[test]
    fn decodes_adpcm_block_header_and_nibbles() {
        // Predictor 0 (256, 0): sample = s1 + nibble * delta.
        let block = [0u8, 16, 0, 100, 0, 50, 0, 0x10];
        let w = Wave::parse(&riff(&fmt(2, 1, 8, 4), &block)).unwrap();
        let expect = [50., 100., 116., 116.].map(|x: f32| x / 32768.);
        assert_eq!(w.samples, expect);
    }
}
