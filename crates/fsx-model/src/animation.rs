//! Bounds-checked MDLX animation keys. No executable code or gauge VM is run.
use fsx_geometry::{Mat4, IDENTITY};
use std::collections::BTreeMap;

type Result<T> = std::result::Result<T, String>;
#[derive(Debug)]
pub struct Clip {
    pub length: f32,
    pub kind: String,
    pub parameters: String,
    pub driver: Option<Driver>,
    tracks: BTreeMap<u32, Channels>,
}
#[derive(Debug)]
pub struct Driver {
    pub variable: String,
    pub units: String,
    pub scale: f32,
    pub bias: f32,
    pub lag: Option<f32>,
}
impl Driver {
    pub fn frame(&self, value: f32, length: f32) -> Result<f32> {
        let frame = value * self.scale + self.bias;
        if !frame.is_finite() || !length.is_finite() || length < 0. {
            return Err("Non-finite animation driver result".into());
        }
        Ok(frame.clamp(0., length))
    }
}
#[derive(Debug, Default)]
struct Channels {
    rotation: Option<Vec<Key>>,
    translation: Option<Vec<Key>>,
}
#[derive(Debug)]
struct Key {
    time: f32,
    value: [f32; 4],
}
fn u32_at(b: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        b.get(offset..offset + 4)
            .ok_or("Truncated animation integer")?
            .try_into()
            .unwrap(),
    ))
}
fn float(b: &[u8], offset: usize) -> Result<f32> {
    let v = f32::from_bits(u32_at(b, offset)?);
    if !v.is_finite() {
        return Err("Non-finite animation key".into());
    }
    Ok(v)
}
fn chunks(mut b: &[u8]) -> Result<Vec<([u8; 4], &[u8])>> {
    let mut result = Vec::new();
    while !b.is_empty() {
        if b.len() < 8 {
            return Err("Truncated animation chunk".into());
        }
        let end = 8usize
            .checked_add(u32_at(b, 4)? as usize)
            .ok_or("Animation chunk overflow")?;
        let body = b.get(8..end).ok_or("Animation chunk outside parent")?;
        result.push((b[..4].try_into().unwrap(), body));
        if result.len() > 200_000 {
            return Err("Animation chunk count limit".into());
        }
        b = &b[end..];
    }
    Ok(result)
}
fn string(b: &[u8]) -> Result<String> {
    let end = b
        .iter()
        .position(|&c| c == 0)
        .ok_or("Unterminated animation string")?;
    if b[end..].iter().any(|&c| c != 0) {
        return Err("Data after animation string terminator".into());
    }
    Ok(String::from_utf8_lossy(&b[..end]).into_owned())
}
fn driver(b: &[u8]) -> Result<Driver> {
    let text = string(b)?;
    let fields: Vec<_> = text.split('#').collect();
    if !(2..=5).contains(&fields.len()) || fields[0].is_empty() || fields[1].is_empty() {
        return Err("Invalid XAPS parameter fields".into());
    }
    let number = |i: usize, fallback: f32| -> Result<f32> {
        let Some(s) = fields.get(i) else {
            return Ok(fallback);
        };
        let value: f32 = s.parse().map_err(|_| "Invalid XAPS numeric field")?;
        if !value.is_finite() {
            return Err("Non-finite XAPS numeric field".into());
        }
        Ok(value)
    };
    let lag = if fields.len() == 5 {
        Some(number(4, 0.)?)
    } else {
        None
    };
    if lag.is_some_and(|v| v < 0.) {
        return Err("Negative animation lag".into());
    }
    Ok(Driver {
        variable: fields[0].into(),
        units: fields[1].into(),
        scale: number(2, 1.)?,
        bias: number(3, 0.)?,
        lag,
    })
}

/// ANIB wraps a packed RIFF with XANH and XANL chunks, followed by zero padding.
/// Rotation keys are byte 4 + time + XYZW; translation keys byte 3 + time + XYZ.
pub fn parse(b: &[u8]) -> Result<Vec<Clip>> {
    if b.len() < 8 || &b[..4] != b"RIFF" {
        return Err("Expected RIFF in ANIB".into());
    }
    let end = 8usize
        .checked_add(u32_at(b, 4)? as usize)
        .ok_or("ANIB overflow")?;
    let payload = b.get(8..end).ok_or("ANIB RIFF outside parent")?;
    if b[end..].iter().any(|&c| c != 0) {
        return Err("Nonzero ANIB padding".into());
    }
    let mut list = None;
    let mut header = false;
    for (tag, body) in chunks(payload)? {
        match &tag {
            b"XANH" => {
                if header || body.len() != 4 || float(body, 0)? <= 0. {
                    return Err("Invalid XANH header".into());
                }
                header = true;
            }
            b"XANL" => {
                if list.replace(body).is_some() {
                    return Err("Duplicate XANL".into());
                }
            }
            _ => {}
        }
    }
    if !header {
        return Err("Missing XANH".into());
    }
    let mut clips = Vec::new();
    let mut key_count = 0usize;
    for (tag, body) in chunks(list.ok_or("Missing XANL")?)? {
        if &tag != b"XANI" {
            continue;
        }
        if body.len() < 37 {
            return Err("Truncated XANI header".into());
        }
        let length = float(body, 16)?;
        if length < 0. {
            return Err("Negative animation length".into());
        }
        let kind = string(&body[20..36])?;
        let param_end = 36
            + body[36..]
                .iter()
                .position(|&c| c == 0)
                .ok_or("Unterminated XANI parameters")?
            + 1;
        let parameters = string(&body[36..param_end])?;
        let mut clip = Clip {
            length,
            kind,
            parameters,
            driver: None,
            tracks: BTreeMap::new(),
        };
        for (tag, track) in chunks(&body[param_end..])? {
            if &tag == b"XAPS" {
                if clip.driver.is_some() {
                    return Err("Duplicate XAPS driver".into());
                }
                clip.driver = Some(driver(track)?);
                continue;
            }
            if &tag != b"XANS" {
                continue;
            }
            if track.len() < 12 {
                return Err("Truncated XANS header".into());
            }
            let channel = u32_at(track, 0)?;
            if channel != 0 && channel != 2 {
                return Err("Unsupported XANS channel type".into());
            }
            let id = u32_at(track, 4)?;
            if id > i16::MAX as u32 {
                return Err("XANS ID outside SGAL range".into());
            }
            let track_length = float(track, 8)?;
            if track_length < 0. {
                return Err("Negative XANS length".into());
            }
            let mut keys: Vec<Key> = Vec::new();
            for (tag, bytes) in chunks(&track[12..])? {
                if &tag != b"XANK" {
                    return Err("Unsupported chunk in XANS".into());
                }
                let rotation = channel == 0;
                if bytes.len() != if rotation { 21 } else { 17 }
                    || bytes[0] != if rotation { 4 } else { 3 }
                {
                    return Err("XANK type/size disagrees with channel".into());
                }
                let time = float(bytes, 1)?;
                if time < 0. || time > track_length || keys.last().is_some_and(|k| time <= k.time) {
                    return Err("Animation key times must increase within track length".into());
                }
                let mut value = [0.; 4];
                for (i, v) in value
                    .iter_mut()
                    .enumerate()
                    .take(if rotation { 4 } else { 3 })
                {
                    *v = float(bytes, 5 + i * 4)?;
                }
                if rotation {
                    value = normalize(value)?;
                }
                keys.push(Key { time, value });
                key_count += 1;
                if key_count > 200_000 {
                    return Err("Animation key count limit".into());
                }
            }
            if keys.is_empty() {
                return Err("Empty animation channel".into());
            }
            let channels = clip.tracks.entry(id).or_default();
            let slot = if channel == 0 {
                &mut channels.rotation
            } else {
                &mut channels.translation
            };
            if slot.replace(keys).is_some() {
                return Err("Duplicate animation channel".into());
            }
            if clip.tracks.len() > 4096 {
                return Err("Animation track count limit".into());
            }
        }
        clips.push(clip);
        if clips.len() > 4096 {
            return Err("Animation clip count limit".into());
        }
    }
    Ok(clips)
}

fn normalize(q: [f32; 4]) -> Result<[f32; 4]> {
    let norm = q.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    if !norm.is_finite() || norm < 1e-8 {
        return Err("Invalid animation quaternion".into());
    }
    Ok(q.map(|x| (x as f64 / norm) as f32))
}
fn sample(keys: &[Key], frame: f32, rotation: bool) -> [f32; 4] {
    let next = keys.partition_point(|k| k.time < frame);
    if next == 0 {
        return keys[0].value;
    }
    if next == keys.len() {
        return keys[next - 1].value;
    }
    let a = keys[next - 1].value;
    let mut b = keys[next].value;
    let t = (frame - keys[next - 1].time) / (keys[next].time - keys[next - 1].time);
    let mut wa = 1. - t;
    let mut wb = t;
    if rotation {
        let mut dot = a.iter().zip(b).map(|(a, b)| a * b).sum::<f32>();
        if dot < 0. {
            b = b.map(|x| -x);
            dot = -dot;
        }
        if dot < 0.9995 {
            let theta = dot.clamp(-1., 1.).acos();
            wa = ((1. - t) * theta).sin() / theta.sin();
            wb = (t * theta).sin() / theta.sin();
        }
    }
    let value = std::array::from_fn(|i| a[i] * wa + b[i] * wb);
    if rotation {
        normalize(value).expect("interpolated normalized quaternion")
    } else {
        value
    }
}
impl Clip {
    pub fn transform(&self, id: u32, frame: f32) -> Result<Option<Mat4>> {
        if !frame.is_finite() {
            return Err("Non-finite animation frame".into());
        }
        let Some(channels) = self.tracks.get(&id) else {
            return Ok(None);
        };
        let frame = frame.clamp(0., self.length);
        let mut matrix = IDENTITY;
        if let Some(keys) = &channels.rotation {
            let [x, y, z, w] = sample(keys, frame, true);
            matrix = [
                1. - 2. * (y * y + z * z),
                2. * (x * y + z * w),
                2. * (x * z - y * w),
                0.,
                2. * (x * y - z * w),
                1. - 2. * (x * x + z * z),
                2. * (y * z + x * w),
                0.,
                2. * (x * z + y * w),
                2. * (y * z - x * w),
                1. - 2. * (x * x + y * y),
                0.,
                0.,
                0.,
                0.,
                1.,
            ];
        }
        if let Some(keys) = &channels.translation {
            matrix[12..15].copy_from_slice(&sample(keys, frame, false)[..3]);
        }
        Ok(Some(matrix))
    }
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }
    pub fn contains(&self, id: u32) -> bool {
        self.tracks.contains_key(&id)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    fn chunk(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
        [tag.as_slice(), &(body.len() as u32).to_le_bytes(), body].concat()
    }
    fn key(kind: u8, time: f32, values: &[f32]) -> Vec<u8> {
        let mut body = vec![kind];
        body.extend(time.to_le_bytes());
        for value in values {
            body.extend(value.to_le_bytes());
        }
        chunk(b"XANK", &body)
    }
    pub(crate) fn fixture() -> Vec<u8> {
        let mut rotation = [
            0u32.to_le_bytes().as_slice(),
            &0u32.to_le_bytes(),
            &10f32.to_le_bytes(),
        ]
        .concat();
        rotation.extend(key(4, 0., &[0., 0., 0., 1.]));
        rotation.extend(key(
            4,
            10.,
            &[
                0.,
                0.,
                std::f32::consts::FRAC_1_SQRT_2,
                std::f32::consts::FRAC_1_SQRT_2,
            ],
        ));
        let mut translation = [
            2u32.to_le_bytes().as_slice(),
            &0u32.to_le_bytes(),
            &10f32.to_le_bytes(),
        ]
        .concat();
        translation.extend(key(3, 0., &[0., 0., 0.]));
        translation.extend(key(3, 10., &[10., 0., 0.]));
        let mut clip = vec![0; 16];
        clip.extend(10f32.to_le_bytes());
        clip.extend(b"Sim\0\0\0\0\0\0\0\0\0\0\0\0\0AutoPlay\0");
        clip.extend(chunk(b"XAPS", b"TEST VALUE#percent#2#1\0"));
        clip.extend(chunk(b"XANS", &rotation));
        clip.extend(chunk(b"XANS", &translation));
        chunk(
            b"RIFF",
            &[
                chunk(b"XANH", &9.2f32.to_le_bytes()),
                chunk(b"XANL", &chunk(b"XANI", &clip)),
            ]
            .concat(),
        )
    }
    #[test]
    fn native_keys_interpolate_rotation_translation_and_clamp() {
        let clips = parse(&fixture()).unwrap();
        let clip = &clips[0];
        assert_eq!(clip.kind, "Sim");
        assert_eq!(clip.parameters, "AutoPlay");
        assert_eq!(clip.track_count(), 1);
        assert_eq!(clip.driver.as_ref().unwrap().frame(2., 10.).unwrap(), 5.);
        let p = fsx_geometry::transform(clip.transform(0, 5.).unwrap().unwrap(), [1., 0., 0.]);
        assert!((p[0] - (5. + std::f32::consts::FRAC_1_SQRT_2)).abs() < 1e-5);
        assert!((p[1] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5);
        assert_eq!(clip.transform(0, -5.).unwrap().unwrap(), IDENTITY);
        assert_eq!(
            clip.transform(0, 15.).unwrap(),
            clip.transform(0, 10.).unwrap()
        );
        assert!(clip.transform(1, 5.).unwrap().is_none());
        assert!(clip.transform(0, f32::NAN).is_err());
    }
    #[test]
    fn quaternion_sign_equivalence_uses_shortest_arc() {
        let keys = vec![
            Key {
                time: 0.,
                value: [0., 0., 0., 1.],
            },
            Key {
                time: 1.,
                value: [0., 0., 0., -1.],
            },
        ];
        assert_eq!(sample(&keys, 0.5, true), [0., 0., 0., 1.]);
        assert!(normalize([0.; 4]).is_err());
        assert!(normalize([f32::INFINITY, 0., 0., 1.]).is_err());
    }
    #[test]
    fn truncation_padding_bad_key_order_and_quaternion_are_rejected() {
        let bytes = fixture();
        for end in 0..bytes.len() {
            assert!(parse(&bytes[..end]).is_err(), "prefix {end}");
        }
        let mut padded = bytes.clone();
        padded.extend([0; 17]);
        assert!(parse(&padded).is_ok());
        *padded.last_mut().unwrap() = 1;
        assert!(parse(&padded).is_err());
        let mut bad = bytes.clone();
        let positions: Vec<_> = bad
            .windows(4)
            .enumerate()
            .filter(|(_, w)| *w == b"XANK")
            .map(|(i, _)| i)
            .collect();
        bad[positions[1] + 9..positions[1] + 13].copy_from_slice(&0f32.to_le_bytes());
        assert!(parse(&bad).unwrap_err().contains("times"));
        bad = bytes.clone();
        bad[positions[0] + 13..positions[0] + 29].fill(0);
        assert!(parse(&bad).unwrap_err().contains("quaternion"));
        bad = bytes;
        bad[positions[0] + 8] = 3;
        assert!(parse(&bad).is_err());
    }
}
