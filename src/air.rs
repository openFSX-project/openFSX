//! Independent legacy AIR record reader. Numeric AIR record labels are hexadecimal.
//! The supported dynamics subset uses 0x1101 plus 0x0404 and 0x0473 curves.
use std::{collections::BTreeMap, fs, path::Path};
type Result<T> = std::result::Result<T, String>;
fn u32_at(b: &[u8], i: usize) -> Result<u32> {
    b.get(i..i + 4)
        .and_then(|x| x.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| "Truncated AIR word".into())
}
fn double(b: &[u8], i: usize) -> Result<f64> {
    let n = b
        .get(i..i + 8)
        .and_then(|x| x.try_into().ok())
        .map(f64::from_le_bytes)
        .ok_or("Truncated AIR double")?;
    if !n.is_finite() {
        return Err("Non-finite AIR value".into());
    }
    Ok(n)
}
#[derive(Clone, Debug)]
pub struct Curve {
    points: Vec<(f64, f64)>,
}
impl Curve {
    pub fn parse(b: &[u8]) -> Result<Self> {
        let count = u32_at(b, 0)? as usize;
        if !(2..=256).contains(&count) || b.len() != 4 + count * 16 {
            return Err("AIR curve count/size mismatch (2..256 points required)".into());
        }
        let mut points = Vec::with_capacity(count);
        for i in 0..count {
            points.push((double(b, 4 + i * 16)?, double(b, 12 + i * 16)?));
        }
        if points.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
            return Err("AIR curve angles must increase strictly".into());
        }
        Ok(Self { points })
    }
    pub fn sample(&self, x: f64) -> Result<f64> {
        if !x.is_finite() {
            return Err("Non-finite AIR curve query".into());
        }
        if x <= self.points[0].0 {
            return Ok(self.points[0].1);
        }
        for pair in self.points.windows(2) {
            let [(a, va), (b, vb)] = pair else {
                unreachable!()
            };
            if x <= *b {
                return Ok(va + (vb - va) * (x - a) / (b - a));
            }
        }
        Ok(self.points.last().unwrap().1)
    }
    pub fn peak(&self) -> Result<(f64, f64)> {
        self.points
            .iter()
            .copied()
            .filter(|(a, _)| *a >= 0. && *a <= 0.7)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .ok_or_else(|| "Lift curve has no positive-angle operating region".into())
    }
}
#[derive(Debug)]
pub struct AirFile {
    pub records: BTreeMap<u32, Vec<u8>>,
}
impl AirFile {
    pub fn load(path: &Path) -> Result<Self> {
        if fs::metadata(path).map_err(|e| e.to_string())?.len() > 4 * 1024 * 1024 {
            return Err("AIR exceeds 4 MiB limit".into());
        }
        Self::parse(&fs::read(path).map_err(|e| e.to_string())?)
    }
    pub fn parse(mut b: &[u8]) -> Result<Self> {
        if b.is_empty() || b.len() > 4 * 1024 * 1024 {
            return Err("Empty or oversized AIR file".into());
        }
        let mut records = BTreeMap::new();
        while !b.is_empty() {
            let id = u32_at(b, 0)?;
            let size = u32_at(b, 4)? as usize;
            if id > 0xffff || records.len() >= 4096 {
                return Err("Unsupported AIR record ID/count".into());
            }
            let end = 8usize
                .checked_add(size)
                .ok_or("AIR record length overflow")?;
            let data = b.get(8..end).ok_or("AIR record extends past end of file")?;
            if records.insert(id, data.to_vec()).is_some() {
                return Err(format!("Duplicate AIR record 0x{id:04x}"));
            }
            b = &b[end..];
        }
        Ok(Self { records })
    }
    fn record(&self, id: u32) -> Result<&[u8]> {
        self.records
            .get(&id)
            .map(Vec::as_slice)
            .ok_or_else(|| format!("Missing AIR record 0x{id:04x}"))
    }
    pub fn dynamics(&self) -> Result<Aerodynamics> {
        if self.records.keys().any(|id| (0x1539..=0x1547).contains(id)) {
            return Err("Modern AIR override records 0x1539..0x1547 are not supported yet".into());
        }
        let b = self.record(0x1101)?;
        if b.len() != 250 {
            return Err("Expected 250-byte legacy 0x1101 aerodynamic record".into());
        }
        let i16_at = |offset: usize| -> f64 {
            i16::from_le_bytes(b[offset..offset + 2].try_into().unwrap()) as f64 / 2048.
        };
        let i32_at = |offset: usize| -> f64 {
            i32::from_le_bytes(b[offset..offset + 4].try_into().unwrap()) as f64 / 2048.
        };
        Ok(Aerodynamics {
            lift: Curve::parse(self.record(0x0404)?)?,
            pitch: Curve::parse(self.record(0x0473)?)?,
            cd0: i16_at(0x30),
            cd_gear: i16_at(0x34),
            cl_elevator: i16_at(0x5a),
            cl_stabilizer: i16_at(0x5c),
            cl_pitch_rate: i32_at(0x56),
            roll_beta: i16_at(0x6e),
            roll_aileron: i16_at(0x70),
            roll_rudder: i32_at(0x72),
            roll_rate: i32_at(0x76),
            roll_yaw_rate: i32_at(0x7a),
            cm0: i32_at(0x86),
            cm_elevator: i32_at(0x8e),
            cm_stabilizer: i32_at(0x92),
            cm_pitch_rate: i32_at(0x96),
            cm_trim: double(b, 0xaa)?,
            cm_gear: double(b, 0xba)?,
            yaw_beta: i32_at(0xca),
            yaw_aileron: i16_at(0xce),
            yaw_rudder: i32_at(0xd0),
            yaw_roll_rate: i32_at(0xe4),
            yaw_rate: i16_at(0xe8),
            side_beta: i16_at(0xee),
            side_rudder: i16_at(0xf0),
            side_roll_rate: i32_at(0xf2),
            side_yaw_rate: i32_at(0xf6),
        })
    }
}
#[derive(Debug, Clone)]
pub struct Aerodynamics {
    pub lift: Curve,
    pub pitch: Curve,
    pub cd0: f64,
    pub cd_gear: f64,
    pub cl_elevator: f64,
    pub cl_stabilizer: f64,
    pub cl_pitch_rate: f64,
    pub roll_beta: f64,
    pub roll_aileron: f64,
    pub roll_rudder: f64,
    pub roll_rate: f64,
    pub roll_yaw_rate: f64,
    pub cm0: f64,
    pub cm_elevator: f64,
    pub cm_stabilizer: f64,
    pub cm_pitch_rate: f64,
    pub cm_trim: f64,
    pub cm_gear: f64,
    pub yaw_beta: f64,
    pub yaw_aileron: f64,
    pub yaw_rudder: f64,
    pub yaw_roll_rate: f64,
    pub yaw_rate: f64,
    pub side_beta: f64,
    pub side_rudder: f64,
    pub side_roll_rate: f64,
    pub side_yaw_rate: f64,
}
#[cfg(test)]
pub(crate) fn own_air() -> Vec<u8> {
    let curve = |points: &[(f64, f64)]| {
        let mut b = (points.len() as u32).to_le_bytes().to_vec();
        for (x, y) in points {
            b.extend(x.to_le_bytes());
            b.extend(y.to_le_bytes());
        }
        b
    };
    let mut constants = vec![0; 250];
    for (offset, value) in [
        (0x30, 0.035),
        (0x5a, -0.2),
        (0x5c, 0.),
        (0x6e, 0.08),
        (0x70, -0.25),
        (0xe8, -0.3),
        (0xee, -0.5),
        (0xf0, -0.2),
    ] {
        constants[offset..offset + 2].copy_from_slice(&((value * 2048.) as i16).to_le_bytes());
    }
    for (offset, value) in [
        (0x76, -0.8),
        (0x8e, -1.2),
        (0x96, -18.),
        (0xca, 0.1),
        (0xd0, 0.08),
    ] {
        constants[offset..offset + 4].copy_from_slice(&((value * 2048.) as i32).to_le_bytes());
    }
    constants[0xaa..0xb2].copy_from_slice(&(-0.5f64).to_le_bytes());
    let mut bytes = Vec::new();
    for (id, data) in [
        (0x1101u32, constants),
        (
            0x404,
            curve(&[
                (-3.142, 0.),
                (-0.1, -0.3),
                (0., 0.2),
                (0.25, 1.4),
                (0.4, 0.7),
                (3.142, 0.),
            ]),
        ),
        (
            0x473,
            curve(&[
                (-3.142, 0.),
                (-0.3, -0.2),
                (0., 0.),
                (0.3, 0.2),
                (3.142, 0.),
            ]),
        ),
    ] {
        bytes.extend(id.to_le_bytes());
        bytes.extend((data.len() as u32).to_le_bytes());
        bytes.extend(data);
    }
    bytes
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn own_records_curve_interpolation_and_scaled_coefficients() {
        let air = AirFile::parse(&own_air()).unwrap();
        let a = air.dynamics().unwrap();
        assert!((a.lift.sample(0.125).unwrap() - 0.8).abs() < 1e-12);
        assert_eq!(a.cm_trim, -0.5);
        assert!((a.cm_pitch_rate + 18.).abs() < 1e-12);
        assert!(a.lift.sample(f64::NAN).is_err());
        assert_eq!(a.lift.peak().unwrap(), (0.25, 1.4));
    }
    #[test]
    fn truncation_duplicate_and_bad_curve_are_errors() {
        let bytes = own_air();
        for n in 0..bytes.len() {
            if let Ok(file) = AirFile::parse(&bytes[..n]) {
                assert!(file.dynamics().is_err());
            }
        }
        assert!(AirFile::parse(&[bytes.clone(), bytes].concat()).is_err());
        assert!(Curve::parse(&[2, 0, 0, 0]).is_err());
        let mut file = AirFile::parse(&own_air()).unwrap();
        file.records.insert(0x1539, vec![]);
        assert!(file.dynamics().unwrap_err().contains("override"));
    }
}
