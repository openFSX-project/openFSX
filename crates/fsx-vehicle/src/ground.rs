//! Fixed-wheel contact configuration; runtime forces live in fsx-sim.
use crate::{
    aircraft::FOOT,
    mass::{body_position, numbers, vector, Loading},
    GRAVITY,
};
use fsx_cfg::Config;
use fsx_geometry::Quaternion;
#[derive(Debug, Clone)]
pub struct Wheel {
    pub position: [f64; 3],
    pub compression: f64,
    pub travel: f64,
    pub stiffness: f64,
    pub damping: f64,
    pub steering: f64,
    pub braked: bool,
}
#[derive(Debug, Clone)]
pub struct Gear {
    pub wheels: Vec<Wheel>,
    pub static_pitch: f64,
    pub omitted_contacts: usize,
}
impl Gear {
    pub fn from_config(c: &Config, loading: &Loading) -> Result<Self, String> {
        let datum = vector(c, "weight_and_balance", "reference_datum_position")?;
        let static_pitch = c
            .optional_number("contact_points", "static_pitch", 0.)?
            .to_radians();
        if static_pitch.abs() > 20_f64.to_radians() {
            return Err("Unsupported static gear pitch".into());
        }
        let mut wheels = Vec::new();
        let mut omitted_contacts = 0;
        let mut indices = std::collections::HashSet::new();
        for (key, text) in c.section("contact_points").into_iter().flatten() {
            if let Some(index) = key.strip_prefix("point.") {
                let index = index
                    .parse::<u32>()
                    .map_err(|_| format!("Invalid contact index {key}"))?;
                if !indices.insert(index) || indices.len() > 128 {
                    return Err("Duplicate contact index or more than 128 contacts".into());
                }
                let class = text
                    .split(',')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| format!("Invalid contact class {key}"))?;
                if class != 1 {
                    omitted_contacts += 1;
                    continue;
                }
                if !(14..=16).contains(&text.split(',').count()) {
                    return Err(format!("Expected 14 to 16 wheel fields at {key}"));
                }
                let v = numbers(text, 14, key)?;
                if v[6] <= 0.
                    || v[8] <= 0.
                    || v[9] < 1.
                    || !(0. ..=1.).contains(&v[10])
                    || v[7].abs() > 90.
                    || v[11] != 0.
                    || v[12] != 0.
                    || ![0., 1., 2.].contains(&v[5])
                {
                    return Err(format!("Unsupported fixed-wheel parameters at {key}"));
                }
                wheels.push(Wheel {
                    position: body_position(datum, [v[1], v[2], v[3]]),
                    compression: v[8] * FOOT,
                    travel: v[8] * v[9] * FOOT,
                    stiffness: 0.,
                    damping: v[10],
                    steering: v[7].to_radians(),
                    braked: v[5] != 0.,
                });
            }
        }
        if wheels.len() != 3 {
            return Err("Initial ground model requires three fixed wheels".into());
        }
        // Empty-aircraft support fractions: sum N = mg; zero roll/pitch moments.
        let projected = wheels
            .iter()
            .map(|w| {
                let r = std::array::from_fn(|i| w.position[i] - loading.empty_cg[i]);
                Quaternion::pitch(static_pitch).rotate(r)
            })
            .collect::<Vec<_>>();
        let [a, b, d] = [projected[0], projected[1], projected[2]];
        let denominator = (b[1] - d[1]) * (a[0] - d[0]) + (d[0] - b[0]) * (a[1] - d[1]);
        if denominator.abs() < 1e-9 {
            return Err("Degenerate wheel support triangle".into());
        }
        let f0 = ((b[1] - d[1]) * (-d[0]) + (d[0] - b[0]) * (-d[1])) / denominator;
        let f1 = ((d[1] - a[1]) * (-d[0]) + (a[0] - d[0]) * (-d[1])) / denominator;
        for (wheel, fraction) in wheels.iter_mut().zip([f0, f1, 1. - f0 - f1]) {
            if !fraction.is_finite() || fraction <= 0. {
                return Err("Empty CG lies outside wheel support triangle".into());
            }
            let supported_mass = loading.empty_mass * fraction;
            wheel.stiffness = supported_mass * GRAVITY / wheel.compression;
            wheel.damping *= 2. * (wheel.stiffness * supported_mass).sqrt();
            if !wheel.stiffness.is_finite() || !wheel.damping.is_finite() {
                return Err("Non-finite wheel spring/damper".into());
            }
        }
        Ok(Self {
            wheels,
            static_pitch,
            omitted_contacts,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    // Invented contact geometry, not extracted aircraft data.
    const OWN:&str="[weight_and_balance]\nreference_datum_position=0,0,0\n[contact_points]\nstatic_pitch=0\npoint.0=1,6,0,-4,1000,0,0.5,20,0.3,3,0.7,0,0,0\npoint.1=1,-3,-4,-4,1000,1,0.5,0,0.3,3,0.7,0,0,0\npoint.2=1,-3,4,-4,1000,2,0.5,0,0.3,3,0.7,0,0,0\npoint.3=2,unimplemented scrape\n";
    #[test]
    fn cfg_wheel_units_support_shares_and_rejections() {
        let loading = crate::aircraft::own_parameters().loading;
        let gear = Gear::from_config(&Config::parse(OWN).unwrap(), &loading).unwrap();
        assert_eq!(gear.wheels.len(), 3);
        assert_eq!(gear.omitted_contacts, 1);
        assert!((gear.wheels[0].position[2] - 4. * FOOT).abs() < 1e-12);
        let force = gear
            .wheels
            .iter()
            .map(|w| w.stiffness * w.compression)
            .sum::<f64>();
        assert!((force - loading.empty_mass * GRAVITY).abs() < 1e-9);
        for (from, to) in [
            ("0.3,3,0.7", "0,3,0.7"),
            ("0.3,3,0.7", "0.3,0.5,0.7"),
            ("0.3,3,0.7", "0.3,3,1.5"),
            ("0.7,0,0,0", "0.7,1,1,0"),
            ("1,6,0,-4", "1,NaN,0,-4"),
        ] {
            assert!(
                Gear::from_config(&Config::parse(&OWN.replace(from, to)).unwrap(), &loading)
                    .is_err(),
                "{to}"
            );
        }
    }
}
