//! Keyboard input and fixed-step scheduling shared with the native flight window.
use crate::{
    aircraft::{Parameters, FOOT, KNOT},
    flight::{Simulation, STEP},
    math::Mat4,
};
use std::collections::HashSet;
use winit::keyboard::KeyCode;

pub struct FlightSession {
    pub simulation: Simulation,
    pub paused: bool,
    keys: HashSet<KeyCode>,
    accumulator: f64,
    initial: Parameters,
}
impl FlightSession {
    pub fn new(parameters: Parameters) -> Result<Self, String> {
        Ok(Self {
            simulation: Simulation::trimmed(parameters.clone(), 90. * KNOT, 300.)?,
            paused: false,
            keys: HashSet::new(),
            accumulator: 0.,
            initial: parameters,
        })
    }
    pub fn reset(&mut self) -> Result<(), String> {
        *self = Self::new(self.initial.clone())?;
        Ok(())
    }
    pub fn key(&mut self, key: KeyCode, pressed: bool) {
        if pressed {
            self.keys.insert(key);
        } else {
            self.keys.remove(&key);
        }
    }
    pub fn clear_keys(&mut self) {
        self.keys.clear();
        self.accumulator = 0.;
    }
    pub fn advance(&mut self, elapsed: f64) -> Result<(), String> {
        if !elapsed.is_finite() || elapsed < 0. {
            return Err("Invalid frame time".into());
        }
        if self.paused || self.simulation.ground_contact {
            self.accumulator = 0.;
            return Ok(());
        }
        self.accumulator += elapsed.min(0.25);
        while self.accumulator + 1e-12 >= STEP {
            let axis = |positive, negative| {
                f64::from(self.keys.contains(&positive)) - f64::from(self.keys.contains(&negative))
            };
            let mut c = self.simulation.controls;
            c.pitch = axis(KeyCode::ArrowDown, KeyCode::ArrowUp) * 0.5;
            c.roll = axis(KeyCode::ArrowRight, KeyCode::ArrowLeft) * 0.5;
            c.yaw = axis(KeyCode::KeyE, KeyCode::KeyQ) * 0.5;
            c.throttle =
                (c.throttle + axis(KeyCode::PageUp, KeyCode::PageDown) * STEP * 0.25).clamp(0., 1.);
            c.trim = (c.trim + axis(KeyCode::Home, KeyCode::End) * STEP * 0.15).clamp(-1., 1.);
            self.simulation.step(c, STEP)?;
            self.accumulator = (self.accumulator - STEP).max(0.);
            if self.simulation.ground_contact {
                self.accumulator = 0.;
                break;
            }
        }
        Ok(())
    }
    pub fn center(&self) -> [f32; 3] {
        let [n, e, d] = self.simulation.state.position;
        [e as f32, -d as f32, n as f32]
    }
    pub fn heading(&self) -> f32 {
        self.simulation.state.orientation.euler()[2] as f32
    }
    pub fn model_matrix(&self) -> Mat4 {
        let rotation = self.simulation.state.orientation;
        let to_world = |body| {
            let [n, e, d] = rotation.rotate(body);
            [e as f32, -d as f32, n as f32]
        };
        let right = to_world([0., 1., 0.]);
        let up = to_world([0., 0., -1.]);
        let forward = to_world([1., 0., 0.]);
        let cg = to_world(self.simulation.parameters.body_cg);
        let center = std::array::from_fn::<_, 3, _>(|i| self.center()[i] - cg[i]);
        [
            right[0], right[1], right[2], 0., up[0], up[1], up[2], 0., forward[0], forward[1],
            forward[2], 0., center[0], center[1], center[2], 1.,
        ]
    }
    pub fn title(&self) -> Result<String, String> {
        let t = self.simulation.telemetry()?;
        let status = if t.ground_contact {
            "GROUND CONTACT — R reset"
        } else if self.paused {
            "PAUSED — P resume"
        } else if t.stalled {
            "POST-PEAK LIFT"
        } else {
            "AIRBORNE"
        };
        Ok(format!("openFSX Flight | {:.1} kt | {:.0} ft | {:+.0} fpm | bank {:+.1}° | pitch {:+.1}° | throttle {:.0}% | {status}",t.speed/KNOT,t.altitude/FOOT,t.climb/FOOT*60.,t.attitude[0].to_degrees(),t.attitude[1].to_degrees(),self.simulation.controls.throttle*100.))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keyboard_press_release_pause_and_frame_partition() {
        let mut a = FlightSession::new(crate::aircraft::own_parameters()).unwrap();
        let mut b = FlightSession::new(crate::aircraft::own_parameters()).unwrap();
        a.key(KeyCode::ArrowRight, true);
        b.key(KeyCode::ArrowRight, true);
        for _ in 0..30 {
            a.advance(1. / 60.).unwrap();
        }
        for _ in 0..60 {
            b.advance(1. / 120.).unwrap();
        }
        assert!(a.simulation.state.rates[0] > 0.);
        assert_eq!(a.simulation.state.position, b.simulation.state.position);
        a.key(KeyCode::ArrowRight, false);
        a.advance(STEP).unwrap();
        assert_eq!(a.simulation.controls.roll, 0.);
        let time = a.simulation.time;
        a.paused = true;
        a.advance(10.).unwrap();
        assert_eq!(a.simulation.time, time);
        a.reset().unwrap();
        assert_eq!(a.simulation.time, 0.);
        assert!(!a.paused);
        assert_eq!(a.model_matrix()[13], 300.);
    }
}
