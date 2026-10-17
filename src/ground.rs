//! Experimental fixed tricycle gear on a horizontal plane, not FSX contact parity.
use crate::{
    aircraft::FOOT,
    cfg::Config,
    flight::{Controls, Quaternion, State, GRAVITY},
    mass::{body_position, numbers, vector, Loading},
};
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
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
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
    #[test]
    fn springs_support_weight_and_tires_dissipate_motion() {
        let p = crate::aircraft::own_parameters();
        let mut s = State {
            position: [0., 0., -1.],
            velocity: [0.; 3],
            orientation: Quaternion::IDENTITY,
            rates: [0.; 3],
        };
        let (f, m, count) = p.gear.forces(s, p.loading.cg, Controls::default());
        assert_eq!(count, 3);
        assert!((f[2] + p.loading.mass * GRAVITY).abs() < 1e-8);
        assert!(m.iter().all(|x| x.abs() < 1e-9));
        for speed in [-10., 10.] {
            s.velocity = [speed, 0., 0.];
            let (rolling, _, _) = p.gear.forces(s, p.loading.cg, Controls::default());
            let (braking, _, _) = p.gear.forces(
                s,
                p.loading.cg,
                Controls {
                    brake: 1.,
                    ..Default::default()
                },
            );
            assert!(braking[0] * speed < rolling[0] * speed && rolling[0] * speed < 0.);
        }
        s.position[2] = -100.;
        assert_eq!(
            p.gear.forces(s, p.loading.cg, Controls::default()).0,
            [0.; 3]
        );
        s.position[2] = -1.;
        s.velocity = [0., 0., -100.];
        assert_eq!(
            p.gear.forces(s, p.loading.cg, Controls::default()).0,
            [0.; 3]
        );
    }
    #[test]
    fn nosewheel_steers_with_rudder_direction() {
        let p = crate::aircraft::own_parameters();
        let s = State {
            position: [0., 0., -1.],
            velocity: [5., 0., 0.],
            orientation: Quaternion::IDENTITY,
            rates: [0.; 3],
        };
        for yaw in [-0.3, 0.3] {
            let (force, moment, _) = p.gear.forces(
                s,
                p.loading.cg,
                Controls {
                    yaw,
                    ..Default::default()
                },
            );
            assert!(force[1] * yaw > 0. && moment[2] * yaw > 0.);
        }
    }
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
    /// Body force and moment about loaded CG. Contact coordinates are the
    /// uncompressed tire contact points; wheel radius is not added a second time.
    pub fn forces(&self, s: State, cg: [f64; 3], c: Controls) -> ([f64; 3], [f64; 3], usize) {
        let q = s.orientation;
        let mut force = [0.; 3];
        let mut moment = [0.; 3];
        let mut count = 0;
        for wheel in &self.wheels {
            let r = std::array::from_fn(|i| wheel.position[i] - cg[i]);
            let world = q.rotate(r);
            let compression = s.position[2] + world[2];
            if compression <= 0. {
                continue;
            }
            count += 1;
            let rotation = q.rotate(cross(s.rates, r));
            let velocity: [f64; 3] = std::array::from_fn(|i| s.velocity[i] + rotation[i]);
            let spring =
                wheel.stiffness * (compression + 19. * (compression - wheel.travel).max(0.));
            let normal = (spring + wheel.damping * velocity[2]).max(0.);
            let steer = c.yaw * wheel.steering;
            let direction = q.rotate([steer.cos(), steer.sin(), 0.]);
            let horizontal = direction[0].hypot(direction[1]).max(1e-9);
            let forward = [direction[0] / horizontal, direction[1] / horizontal];
            let side = [-forward[1], forward[0]];
            let longitudinal = velocity[0] * forward[0] + velocity[1] * forward[1];
            let lateral = velocity[0] * side[0] + velocity[1] * side[1];
            let brake = if wheel.braked { c.brake } else { 0. };
            let mut fx = -normal * (0.02 + 0.78 * brake) * (longitudinal / 0.03).tanh();
            let mut fy = -normal * 0.8 * (lateral / 0.15).tanh();
            let ratio = fx.hypot(fy) / (0.8 * normal).max(1e-9);
            if ratio > 1. {
                fx /= ratio;
                fy /= ratio;
            }
            let body = q.inverse().rotate([
                fx * forward[0] + fy * side[0],
                fx * forward[1] + fy * side[1],
                -normal,
            ]);
            let torque = cross(r, body);
            for i in 0..3 {
                force[i] += body[i];
                moment[i] += torque[i];
            }
        }
        (force, moment, count)
    }
}
