//! Keyboard input and fixed-step scheduling shared with the native flight window.
use fsx_geometry::Mat4;
use fsx_sim::flight::{Simulation, STEP};
use fsx_vehicle::aircraft::{Parameters, FOOT, KNOT};
use std::collections::HashSet;
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};

/// Honor navigation semantics for both dedicated keys and NumLock-off numpad
/// events. Keep the existing physical bindings for letter controls.
pub fn flight_key(physical: PhysicalKey, logical: &Key) -> Option<KeyCode> {
    Some(match logical {
        Key::Named(NamedKey::ArrowUp) => KeyCode::ArrowUp,
        Key::Named(NamedKey::ArrowDown) => KeyCode::ArrowDown,
        Key::Named(NamedKey::ArrowLeft) => KeyCode::ArrowLeft,
        Key::Named(NamedKey::ArrowRight) => KeyCode::ArrowRight,
        Key::Named(NamedKey::PageUp) => KeyCode::PageUp,
        Key::Named(NamedKey::PageDown) => KeyCode::PageDown,
        Key::Named(NamedKey::Home) => KeyCode::Home,
        Key::Named(NamedKey::End) => KeyCode::End,
        _ => match physical {
            PhysicalKey::Code(code) => code,
            PhysicalKey::Unidentified(_) => return None,
        },
    })
}

/// Normalized analog inputs: pitch positive pull, roll right, yaw right.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Analog {
    pub pitch: f64,
    pub roll: f64,
    pub yaw: f64,
    /// Absolute lever position when the device has one.
    pub throttle: Option<f64>,
    /// Throttle change per second for devices without a lever, -1..1.
    pub throttle_rate: f64,
    pub brake: f64,
}

pub struct FlightSession {
    pub simulation: Simulation,
    pub paused: bool,
    pub parking_brake: bool,
    /// Selected flap detent; the surface follows at the authored extension rate.
    pub flap_handle: usize,
    /// Latest analog stick/pedal/lever positions; None keeps keyboard control.
    pub analog: Option<Analog>,
    keys: HashSet<KeyCode>,
    // A complete tap can arrive between two fixed steps. Sample it once rather
    // than dropping the press when its release precedes the next redraw.
    pending_presses: HashSet<KeyCode>,
    accumulator: f64,
    initial: Parameters,
    initial_ground: bool,
}
impl FlightSession {
    pub fn new(parameters: Parameters) -> Result<Self, String> {
        Self::with_start(parameters, false)
    }
    pub fn on_ground(parameters: Parameters) -> Result<Self, String> {
        Self::with_start(parameters, true)
    }
    fn with_start(parameters: Parameters, ground: bool) -> Result<Self, String> {
        Ok(Self {
            simulation: if ground {
                Simulation::on_ground(parameters.clone())?
            } else {
                Simulation::trimmed(parameters.clone(), 90. * KNOT, 300.)?
            },
            paused: false,
            parking_brake: ground,
            flap_handle: 0,
            analog: None,
            keys: HashSet::new(),
            pending_presses: HashSet::new(),
            accumulator: 0.,
            initial: parameters,
            initial_ground: ground,
        })
    }
    pub fn reset(&mut self) -> Result<(), String> {
        *self = Self::with_start(self.initial.clone(), self.initial_ground)?;
        Ok(())
    }
    pub fn key(&mut self, key: KeyCode, pressed: bool) {
        if pressed {
            if self.keys.insert(key) {
                self.pending_presses.insert(key);
                match key {
                    KeyCode::F1 => self.simulation.controls.throttle = 0.,
                    KeyCode::F4 => self.simulation.controls.throttle = 1.,
                    KeyCode::KeyB => self.parking_brake = !self.parking_brake,
                    KeyCode::F5 => self.flap_handle = 0,
                    KeyCode::F6 => self.flap_handle = self.flap_handle.saturating_sub(1),
                    KeyCode::F7 => {
                        self.flap_handle = (self.flap_handle + 1)
                            .min(self.simulation.parameters.flaps.detents() - 1)
                    }
                    KeyCode::F8 => {
                        self.flap_handle = self.simulation.parameters.flaps.detents() - 1
                    }
                    _ => {}
                }
            }
        } else {
            self.keys.remove(&key);
        }
    }
    pub fn clear_keys(&mut self) {
        self.keys.clear();
        self.pending_presses.clear();
        self.accumulator = 0.;
    }
    pub fn advance(&mut self, elapsed: f64) -> Result<(), String> {
        if !elapsed.is_finite() || elapsed < 0. {
            return Err("Invalid frame time".into());
        }
        if self.paused {
            self.accumulator = 0.;
            return Ok(());
        }
        self.accumulator += elapsed.min(0.25);
        while self.accumulator + 1e-12 >= STEP {
            let axis = |positive, negative| {
                let active = |key| self.keys.contains(&key) || self.pending_presses.contains(&key);
                f64::from(active(positive)) - f64::from(active(negative))
            };
            let mut c = self.simulation.controls;
            c.pitch = axis(KeyCode::ArrowDown, KeyCode::ArrowUp) * 0.5;
            c.roll = axis(KeyCode::ArrowRight, KeyCode::ArrowLeft) * 0.5;
            c.yaw = axis(KeyCode::KeyE, KeyCode::KeyQ) * 0.5;
            if let Some(analog) = self.analog {
                // Keys still win while held so either device can be used.
                for (value, stick) in [
                    (&mut c.pitch, analog.pitch),
                    (&mut c.roll, analog.roll),
                    (&mut c.yaw, analog.yaw),
                ] {
                    if *value == 0. {
                        *value = stick;
                    }
                }
            }
            c.brake = if self.parking_brake
                || self.keys.contains(&KeyCode::Space)
                || self.pending_presses.contains(&KeyCode::Space)
            {
                1.
            } else {
                0.
            };
            c.throttle =
                (c.throttle + axis(KeyCode::PageUp, KeyCode::PageDown) * STEP * 0.25).clamp(0., 1.);
            if let Some(analog) = self.analog {
                c.brake = c.brake.max(analog.brake);
                c.throttle = match analog.throttle {
                    Some(lever) => lever,
                    None => c.throttle + analog.throttle_rate * STEP * 0.5,
                }
                .clamp(0., 1.);
            }
            let flaps = &self.simulation.parameters.flaps;
            let target = flaps.detent_fraction(self.flap_handle);
            let travel = STEP / flaps.extending_time;
            c.flaps = if target > c.flaps {
                (c.flaps + travel).min(target)
            } else {
                (c.flaps - travel).max(target)
            };
            c.trim = (c.trim + axis(KeyCode::Home, KeyCode::End) * STEP * 0.15).clamp(-1., 1.);
            self.simulation.step(c, STEP)?;
            self.pending_presses.clear();
            self.accumulator = (self.accumulator - STEP).max(0.);
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
        let cg = to_world(self.simulation.parameters.loading.cg);
        let center = std::array::from_fn::<_, 3, _>(|i| self.center()[i] - cg[i]);
        [
            right[0], right[1], right[2], 0., up[0], up[1], up[2], 0., forward[0], forward[1],
            forward[2], 0., center[0], center[1], center[2], 1.,
        ]
    }
    pub fn title(&self) -> Result<String, String> {
        let t = self.simulation.telemetry()?;
        let status = if self.paused {
            "PAUSED — P resume"
        } else if t.ground_contact {
            if self.parking_brake {
                "GROUND — PARKING BRAKE"
            } else {
                "GROUND"
            }
        } else if t.stalled {
            "POST-PEAK LIFT"
        } else {
            "AIRBORNE"
        };
        Ok(format!("openFSX Flight | {:.1} kt | {:.0} ft | {:+.0} fpm | bank {:+.1}° | pitch {:+.1}° | throttle {:.0}% | flaps {:.0}° | {status}",t.speed/KNOT,t.altitude/FOOT,t.climb/FOOT*60.,t.attitude[0].to_degrees(),t.attitude[1].to_degrees(),self.simulation.controls.throttle*100.,self.simulation.controls.flaps*self.simulation.parameters.flaps.max_deflection()))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn battery_tracks_simulated_time_pause_frame_partition_and_reset() {
        let mut p = fsx_vehicle::aircraft::own_parameters();
        p.electrical.battery_max_capacity_amph = Some(40.);
        let mut a = FlightSession::new(p.clone()).unwrap();
        let mut b = FlightSession::new(p).unwrap();
        for session in [&mut a, &mut b] {
            session
                .simulation
                .electrical
                .battery
                .set_charge_fraction(Some(1.))
                .unwrap();
            session
                .simulation
                .electrical
                .set_battery_current_request(Some(36.))
                .unwrap();
        }
        for _ in 0..60 {
            a.advance(1. / 60.).unwrap();
        }
        for _ in 0..120 {
            b.advance(1. / 120.).unwrap();
        }
        assert_eq!(
            a.simulation.electrical.battery.charge_fraction(),
            b.simulation.electrical.battery.charge_fraction()
        );
        assert!(
            (a.simulation.electrical.battery.charge_fraction().unwrap() - 0.99975).abs() < 1e-12
        );
        let fraction = a.simulation.electrical.battery.charge_fraction();
        let flow = a.simulation.electrical.last_battery_flow();
        a.paused = true;
        a.advance(30.).unwrap();
        assert_eq!(a.simulation.electrical.battery.charge_fraction(), fraction);
        assert_eq!(a.simulation.electrical.last_battery_flow(), flow);
        a.paused = false;
        a.advance(1. / 120.).unwrap();
        assert!(a.simulation.electrical.battery.charge_fraction().unwrap() < fraction.unwrap());
        a.reset().unwrap();
        assert_eq!(a.simulation.electrical.battery.charge_fraction(), None);
        assert_eq!(a.simulation.electrical.last_battery_flow(), None);
        // Reinitializing charge without a source request must not inherit 36 A.
        a.simulation
            .electrical
            .battery
            .set_charge_fraction(Some(1.))
            .unwrap();
        a.advance(1. / 120.).unwrap();
        assert_eq!(a.simulation.electrical.battery.charge_fraction(), None);
    }
    #[test]
    fn runway_reset_parking_brake_and_power_keys() {
        let mut session =
            FlightSession::on_ground(fsx_vehicle::aircraft::own_parameters()).unwrap();
        assert!(session.parking_brake && session.simulation.ground_contact);
        session.key(KeyCode::F4, true);
        session.key(KeyCode::F4, false);
        assert_eq!(session.simulation.controls.throttle, 1.);
        session.key(KeyCode::KeyB, true);
        session.key(KeyCode::KeyB, true);
        session.key(KeyCode::KeyB, false);
        assert!(!session.parking_brake);
        session.advance(STEP).unwrap();
        assert_eq!(session.simulation.controls.brake, 0.);
        session.key(KeyCode::Space, true);
        session.advance(STEP).unwrap();
        assert_eq!(session.simulation.controls.brake, 1.);
        session.key(KeyCode::Space, false);
        session.key(KeyCode::F1, true);
        session.key(KeyCode::F1, false);
        assert_eq!(session.simulation.controls.throttle, 0.);
        session.reset().unwrap();
        assert!(session.parking_brake && session.simulation.ground_contact);
        assert_eq!(session.simulation.time, 0.);
    }
    #[test]
    fn flap_detents_follow_extension_rate_and_analog_inputs_apply() {
        let mut p = fsx_vehicle::aircraft::own_parameters();
        p.flaps = fsx_vehicle::flaps::Flaps {
            positions: vec![0., 10., 20.],
            extending_time: 2.,
            lift: 0.5,
            drag: 0.05,
            pitch: -0.05,
        };
        let mut session = FlightSession::new(p).unwrap();
        session.key(KeyCode::F7, true);
        session.key(KeyCode::F7, false);
        session.key(KeyCode::F7, true);
        session.key(KeyCode::F7, false);
        session.key(KeyCode::F7, true);
        assert_eq!(session.flap_handle, 2);
        for _ in 0..4 {
            session.advance(0.25).unwrap();
        }
        assert!((session.simulation.controls.flaps - 0.5).abs() < 1e-6);
        for _ in 0..5 {
            session.advance(0.25).unwrap();
        }
        assert_eq!(session.simulation.controls.flaps, 1.);
        session.key(KeyCode::F5, true);
        assert_eq!(session.flap_handle, 0);
        session.analog = Some(Analog {
            roll: 0.3,
            throttle: Some(0.2),
            ..Default::default()
        });
        session.advance(STEP).unwrap();
        assert_eq!(session.simulation.controls.roll, 0.3);
        assert_eq!(session.simulation.controls.throttle, 0.2);
        assert!(session.simulation.controls.flaps < 1.);
    }
    #[test]
    fn navigation_uses_logical_key_with_numlock_off() {
        assert_eq!(
            flight_key(
                PhysicalKey::Code(KeyCode::Numpad6),
                &Key::Named(NamedKey::ArrowRight)
            ),
            Some(KeyCode::ArrowRight)
        );
        assert_eq!(
            flight_key(
                PhysicalKey::Code(KeyCode::Numpad6),
                &Key::Character("6".into())
            ),
            Some(KeyCode::Numpad6)
        );
        assert_eq!(
            flight_key(
                PhysicalKey::Code(KeyCode::KeyQ),
                &Key::Character("a".into())
            ),
            Some(KeyCode::KeyQ)
        );
    }
    #[test]
    fn tap_between_physics_steps_is_not_lost() {
        let mut flight = FlightSession::new(fsx_vehicle::aircraft::own_parameters()).unwrap();
        flight.key(KeyCode::ArrowRight, true);
        flight.key(KeyCode::ArrowRight, false);
        flight.advance(STEP * 0.4).unwrap();
        assert_eq!(flight.simulation.time, 0.);
        flight.advance(STEP * 0.6).unwrap();
        assert_eq!(flight.simulation.controls.roll, 0.5);
        assert!(flight.simulation.state.rates[0] > 0.);
        flight.advance(STEP).unwrap();
        assert_eq!(flight.simulation.controls.roll, 0.);
        flight.key(KeyCode::ArrowLeft, true);
        flight.key(KeyCode::ArrowLeft, false);
        flight.clear_keys();
        flight.advance(STEP).unwrap();
        assert_eq!(flight.simulation.controls.roll, 0.);
    }
    #[test]
    fn keyboard_press_release_pause_and_frame_partition() {
        let mut a = FlightSession::new(fsx_vehicle::aircraft::own_parameters()).unwrap();
        let mut b = FlightSession::new(fsx_vehicle::aircraft::own_parameters()).unwrap();
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
