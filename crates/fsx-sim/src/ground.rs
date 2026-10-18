//! Experimental fixed-wheel contact forces on a flat plane.
#[cfg(test)]
use crate::flight::Quaternion;
#[cfg(test)]
use crate::flight::GRAVITY;
use crate::flight::{Controls, State};
use fsx_vehicle::ground::{Gear, Wheel};
fn steering(wheel: &Wheel, yaw: f64) -> f64 {
    yaw * wheel.steering
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub trait GroundForces {
    fn forces(&self, s: State, cg: [f64; 3], c: Controls) -> ([f64; 3], [f64; 3], usize);
    fn center_steering_angle(&self, yaw: f64) -> Option<f64>;
}
impl GroundForces for Gear {
    fn center_steering_angle(&self, yaw: f64) -> Option<f64> {
        if !yaw.is_finite() || yaw.abs() > 1. {
            return None;
        }
        let mut centers = self.wheels.iter().filter(|w| w.position[1].abs() < 1e-6);
        let center = centers.next()?;
        if centers.next().is_some() {
            return None;
        }
        let angle = steering(center, yaw);
        angle.is_finite().then_some(angle)
    }
    /// Body force and moment about loaded CG. Contact coordinates are the
    /// uncompressed tire contact points; wheel radius is not added a second time.
    fn forces(&self, s: State, cg: [f64; 3], c: Controls) -> ([f64; 3], [f64; 3], usize) {
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
            let steer = steering(wheel, c.yaw);
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
#[cfg(test)]
mod tests {
    use super::*;
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
