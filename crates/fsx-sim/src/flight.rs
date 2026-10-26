//! Initial six-degree-of-freedom airborne dynamics, not FSX flight-model parity.
//! SI units; right-handed body forward/right/down and world north/east/down.
use crate::ground::GroundForces;
use fsx_vehicle::aircraft::Parameters;
type Result<T> = std::result::Result<T, String>;
type Vec3 = [f64; 3];
pub const STEP: f64 = 1. / 120.;
pub use fsx_vehicle::GRAVITY;
pub const PROPELLER_EFFICIENCY: f64 = 0.8;
fn add(a: Vec3, b: Vec3) -> Vec3 {
    std::array::from_fn(|i| a[i] + b[i])
}
fn scale(a: Vec3, k: f64) -> Vec3 {
    a.map(|x| x * k)
}
fn dot(a: Vec3, b: Vec3) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn length(v: Vec3) -> f64 {
    dot(v, v).sqrt()
}
fn unit(v: Vec3) -> Vec3 {
    let n = length(v);
    if n < 1e-12 {
        [0.; 3]
    } else {
        scale(v, 1. / n)
    }
}

pub use fsx_geometry::Quaternion;
#[derive(Debug, Clone, Copy, Default)]
pub struct Controls {
    /// Positive pull, bank right, yaw right; normalized controls and trim in -1..1.
    pub pitch: f64,
    pub roll: f64,
    pub yaw: f64,
    pub trim: f64,
    pub throttle: f64,
    pub brake: f64,
    /// Actual flap deflection, normalized 0 (retracted) .. 1 (last detent).
    pub flaps: f64,
}
impl Controls {
    fn validated(self) -> Result<Self> {
        if [
            self.pitch,
            self.roll,
            self.yaw,
            self.trim,
            self.throttle,
            self.brake,
            self.flaps,
        ]
        .iter()
        .any(|x| !x.is_finite())
        {
            return Err("Non-finite pilot control".into());
        }
        Ok(Self {
            pitch: self.pitch.clamp(-1., 1.),
            roll: self.roll.clamp(-1., 1.),
            yaw: self.yaw.clamp(-1., 1.),
            trim: self.trim.clamp(-1., 1.),
            throttle: self.throttle.clamp(0., 1.),
            brake: self.brake.clamp(0., 1.),
            flaps: self.flaps.clamp(0., 1.),
        })
    }
}
#[derive(Debug, Clone, Copy)]
pub struct State {
    pub position: Vec3,
    pub velocity: Vec3,
    pub orientation: Quaternion,
    pub rates: Vec3,
}
impl State {
    fn plus(self, d: Self, k: f64) -> Self {
        Self {
            position: add(self.position, scale(d.position, k)),
            velocity: add(self.velocity, scale(d.velocity, k)),
            orientation: Quaternion(std::array::from_fn(|i| {
                self.orientation.0[i] + d.orientation.0[i] * k
            })),
            rates: add(self.rates, scale(d.rates, k)),
        }
    }
    fn finite(&self) -> bool {
        self.position
            .iter()
            .chain(self.velocity.iter())
            .chain(self.orientation.0.iter())
            .chain(self.rates.iter())
            .all(|x| x.is_finite())
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Telemetry {
    pub speed: f64,
    pub altitude: f64,
    pub alpha: f64,
    pub beta: f64,
    pub attitude: Vec3,
    pub climb: f64,
    pub stalled: bool,
    pub ground_contact: bool,
}
pub fn density(altitude: f64) -> f64 {
    // ISA troposphere, limited to the initial validated altitude interval.
    let temperature = 288.15 - 0.0065 * altitude.clamp(0., 11000.);
    1.225 * (temperature / 288.15).powf(4.2558797)
}
fn induced_velocity(thrust: f64, forward: f64, rho: f64, area: f64) -> f64 {
    0.5 * ((forward * forward + 2. * thrust / (rho * area)).sqrt() - forward)
}
fn propeller_thrust(power: f64, forward: f64, rho: f64, area: f64) -> f64 {
    if power <= 0. {
        return 0.;
    }
    // Momentum-theory disk: effective power = T(V + induced velocity).
    // Unlike P/V, this remains finite at zero airspeed.
    let mut low = 0.;
    let mut high = (power * (2. * rho * area).sqrt()).powf(2. / 3.);
    for _ in 0..60 {
        let t = (low + high) * 0.5;
        if t * (forward + induced_velocity(t, forward, rho, area)) > power {
            high = t;
        } else {
            low = t;
        }
    }
    (low + high) * 0.5
}
pub trait AircraftDynamics {
    fn predicted_stall(&self, altitude: f64) -> Result<f64>;
    fn lift_drag(&self, alpha: f64, pitch_control: f64, qhat: f64) -> Result<(f64, f64)>;
    fn derivative(&self, s: State, c: Controls) -> Result<State>;
}
impl AircraftDynamics for Parameters {
    fn predicted_stall(&self, altitude: f64) -> Result<f64> {
        let (_, peak) = self.aero.lift.peak()?;
        let cl = peak * self.lift_scalar + self.aero.cl_stabilizer * self.tail_incidence;
        if cl <= 0. {
            return Err("No positive maximum lift".into());
        }
        Ok((2. * self.loading.mass * GRAVITY / (density(altitude) * self.wing_area * cl)).sqrt())
    }
    fn lift_drag(&self, alpha: f64, pitch_control: f64, qhat: f64) -> Result<(f64, f64)> {
        let cl = self.aero.lift.sample(alpha)? * self.lift_scalar
            + self.aero.cl_stabilizer * self.tail_incidence
            + self.aero.cl_elevator * pitch_control
            - self.aero.cl_pitch_rate * qhat;
        let aspect = self.span * self.span / self.wing_area;
        let cd = (self.aero.cd0 + self.aero.cd_gear) * self.parasite_scalar
            + cl * cl / (std::f64::consts::PI * aspect * self.efficiency) * self.induced_scalar;
        if cd < 0. {
            return Err("Negative total drag coefficient".into());
        }
        Ok((cl, cd))
    }
    fn derivative(&self, s: State, c: Controls) -> Result<State> {
        let orientation = s.orientation.normalized()?;
        let velocity = orientation.inverse().rotate(s.velocity);
        let speed = length(velocity);
        let alpha = velocity[2].atan2(velocity[0]);
        let beta = if speed > 1e-9 {
            (velocity[1] / speed).clamp(-1., 1.).asin()
        } else {
            0.
        };
        let rho = density(-s.position[2]);
        let pressure = 0.5 * rho * speed * speed;
        let [p, q, r] = s.rates;
        let phat = p * self.span / (2. * speed.max(1.));
        let qhat = q * self.chord / (2. * speed.max(1.));
        let rhat = r * self.span / (2. * speed.max(1.));
        let de = c.pitch
            * if c.pitch >= 0. {
                self.elevator_up
            } else {
                self.elevator_down
            }
            * self.controls[0];
        let da = c.roll * self.aileron_limit * self.controls[1];
        let dr = c.yaw * self.rudder_limit * self.controls[2];
        let trim = c.trim * self.trim_limit * self.controls[3];
        let (cl, cd) = self.lift_drag(alpha, de, qhat)?;
        let (flap_lift, flap_drag, flap_pitch) = self.flaps.increments(c.flaps);
        let (cl, cd) = (cl + flap_lift, cd + flap_drag);
        let a = &self.aero;
        // Legacy AIR pitch/yaw are nose-down/left positive; body q/r are up/right.
        // Legacy aileron and rudder deflections have opposite signs to pilot input.
        let cy = a.side_beta * beta - a.side_roll_rate * phat
            + a.side_yaw_rate * rhat
            + a.side_rudder * dr;
        let roll = -a.roll_beta * beta + a.roll_rate * phat * self.damping[0]
            - a.roll_yaw_rate * rhat
            - a.roll_aileron * da
            - a.roll_rudder * dr;
        let pitch = -(a.cm0
            + a.pitch.sample(alpha)?
            + a.cm_stabilizer * self.tail_incidence
            + a.cm_elevator * de
            + a.cm_trim * trim
            + a.cm_gear)
            + a.cm_pitch_rate * qhat * self.damping[1]
            + flap_pitch;
        let yaw = a.yaw_beta * beta - a.yaw_roll_rate * phat
            + a.yaw_rate * rhat * self.damping[2]
            + a.yaw_aileron * da
            + a.yaw_rudder * dr;
        let moments = scale(
            [self.span * roll, self.chord * pitch, self.span * yaw],
            pressure * self.wing_area,
        );
        let drag = scale(unit(velocity), -pressure * self.wing_area * cd);
        let lift_direction = unit([velocity[2], 0., -velocity[0]]);
        let lift = scale(lift_direction, pressure * self.wing_area * cl);
        let side = scale(
            unit(cross(unit(velocity), lift_direction)),
            pressure * self.wing_area * cy,
        );
        let power = self.power * c.throttle * PROPELLER_EFFICIENCY * rho / 1.225;
        let thrust = propeller_thrust(power, velocity[0].max(0.), rho, self.prop_area);
        let aero_force = add(add(lift, drag), side);
        let thrust_force = [thrust, 0., 0.];
        let (ground_force, ground_moment, _) =
            self.gear
                .forces(State { orientation, ..s }, self.loading.cg, c);
        let force = add(add(aero_force, thrust_force), ground_force);
        // Initial model: AIR moments are referenced to the empty CG. Translate
        // that wrench to the loaded CG; thrust acts at the configured engine.
        let offset = |point: Vec3| std::array::from_fn(|i| point[i] - self.loading.cg[i]);
        let moments = add(
            ground_moment,
            add(
                moments,
                add(
                    cross(offset(self.loading.empty_cg), aero_force),
                    cross(offset(self.engine_position), thrust_force),
                ),
            ),
        );
        let acceleration = add(
            scale(orientation.rotate(force), 1. / self.loading.mass),
            [0., 0., GRAVITY],
        );
        let angular_momentum = self.loading.inertia.apply(s.rates);
        let gyroscopic = cross(s.rates, angular_momentum);
        let angular_acceleration = self
            .loading
            .inertia
            .solve(add(moments, scale(gyroscopic, -1.)));
        let qdot = s
            .orientation
            .product(Quaternion([0., p, q, r]))
            .0
            .map(|x| x * 0.5);
        Ok(State {
            position: s.velocity,
            velocity: acceleration,
            orientation: Quaternion(qdot),
            rates: angular_acceleration,
        })
    }
}
#[derive(Debug, Clone)]
pub struct Simulation {
    pub electrical: crate::electrical::CircuitNetwork,
    pub parameters: Parameters,
    pub state: State,
    pub controls: Controls,
    pub time: f64,
    pub ground_contact: bool,
    /// Height of the local ground plane above mean sea level, metres. Reported
    /// altitude includes it; aerodynamic density still uses height above ground.
    pub field_elevation: f64,
    /// Engine speed in revolutions per minute (fixed-pitch propeller model).
    pub rpm: f64,
}
impl Simulation {
    /// Commit physics and equipment history together. Load uses prior records;
    /// only successful frame outputs replace additional-system/avionics flags.
    /// Other equipment still needs its real activation provider.
    pub fn step_with_legacy_consumer_state(
        &mut self,
        controls: Controls,
        dt: f64,
        consumers: &mut crate::legacy_consumers::ConsumerState,
        response: crate::legacy_response::Response,
        tail: Option<crate::legacy_voltage::TailEnvironment>,
    ) -> Result<Option<crate::legacy_voltage::FrameStep>> {
        let frame =
            self.step_with_legacy_consumers(controls, dt, consumers.records(), response, tail)?;
        consumers.observe_frame(frame);
        Ok(frame)
    }
    /// Sum original slots 1..=33 from prior activity before this frame. Providers
    /// own activity/presence and must carry the preceding frame's flags; this
    /// method does not infer omitted circuits or update all equipment activity.
    pub fn step_with_legacy_consumers(
        &mut self,
        controls: Controls,
        dt: f64,
        consumers: &[crate::legacy_consumers::Consumer; crate::legacy_consumers::SLOTS],
        response: crate::legacy_response::Response,
        tail: Option<crate::legacy_voltage::TailEnvironment>,
    ) -> Result<Option<crate::legacy_voltage::FrameStep>> {
        let load = crate::legacy_consumers::sum(consumers)?;
        self.step_with_legacy_electrical(controls, dt, load, response, tail)
    }
    /// Commit one provider-driven legacy electrical frame with one caller flight
    /// interval. Ground RK4 subdivision does not repeat the electrical frame.
    /// Pause is represented by making no call. Providers describe this interval;
    /// no stored FLT RPM, load, context or response default is selected here.
    /// All physics/electrical mutations roll back together on any error.
    pub fn step_with_legacy_electrical(
        &mut self,
        controls: Controls,
        dt: f64,
        load_amps: Option<f64>,
        response: crate::legacy_response::Response,
        tail: Option<crate::legacy_voltage::TailEnvironment>,
    ) -> Result<Option<crate::legacy_voltage::FrameStep>> {
        let mut staged = self.clone();
        let mut electrical = self.electrical.clone();
        let frame =
            electrical.step_legacy_electrical_frame(dt, load_amps, response.factor(), tail)?;
        staged.step_physics(controls, dt, false)?;
        // The chosen original voltage/current path supersedes independent
        // Coulomb integration; never run both for this interval.
        staged.electrical = electrical;
        *self = staged;
        Ok(frame)
    }
    pub fn on_ground(parameters: Parameters) -> Result<Self> {
        Self::on_ground_at(parameters, [-400., 0.], 0.)
    }
    /// Runway start at a local north/east position facing a true heading (radians).
    pub fn on_ground_at(parameters: Parameters, position: [f64; 2], heading: f64) -> Result<Self> {
        if !heading.is_finite() || position.iter().any(|x| !x.is_finite()) {
            return Err("Invalid runway start position or heading".into());
        }
        let mut s = Self::trimmed(parameters, 65. * crate::aircraft::KNOT, 300.)?;
        let p = &s.parameters;
        let height = p.ground_height - (p.loading.cg[2] - p.loading.empty_cg[2]);
        let yaw = Quaternion([(heading * 0.5).cos(), 0., 0., (heading * 0.5).sin()]);
        s.state = State {
            position: [position[0], position[1], -height],
            velocity: [0.; 3],
            orientation: yaw.product(Quaternion::pitch(p.gear.static_pitch)),
            rates: [0.; 3],
        };
        s.controls.throttle = 0.;
        s.controls.brake = 1.;
        // Settle the suspension with power off; this does not constrain motion
        // once the user releases the brake or moves the controls.
        for _ in 0..2400 {
            s.step(s.controls, 1. / 480.)?;
        }
        if !s.ground_contact {
            return Err("Runway start did not settle on the configured wheels".into());
        }
        s.time = 0.;
        // Suspension settling precedes the flight epoch; discard its source
        // interval record just as its elapsed physics time is discarded.
        s.electrical.reset();
        Ok(s)
    }
    /// Trimmed level flight turned to a true heading (radians) about the vertical.
    pub fn trimmed_heading(
        parameters: Parameters,
        speed: f64,
        altitude: f64,
        heading: f64,
    ) -> Result<Self> {
        let mut s = Self::trimmed(parameters, speed, altitude)?;
        let yaw = Quaternion([(heading * 0.5).cos(), 0., 0., (heading * 0.5).sin()]);
        s.state.orientation = yaw.product(s.state.orientation);
        s.state.velocity = yaw.rotate(s.state.velocity);
        Ok(s)
    }
    pub fn trimmed(parameters: Parameters, speed: f64, altitude: f64) -> Result<Self> {
        if !speed.is_finite()
            || speed <= 0.
            || !altitude.is_finite()
            || !(10. ..=11000.).contains(&altitude)
        {
            return Err(
                "Trim requires positive airspeed and altitude between 10 and 11000 metres".into(),
            );
        }
        let rho = density(altitude);
        let pressure = 0.5 * rho * speed * speed * parameters.wing_area;
        let balance = |alpha: f64| -> Result<f64> {
            let (cl, cd) = parameters.lift_drag(alpha, 0., 0.)?;
            Ok(pressure * (cl + cd * alpha.tan()) - parameters.loading.mass * GRAVITY)
        };
        let mut low = -0.15;
        let mut high = parameters.aero.lift.peak()?.0;
        if balance(low)? > 0. || balance(high)? < 0. {
            return Err("No pre-stall level-flight trim at the requested speed/mass".into());
        }
        for _ in 0..60 {
            let alpha = (low + high) * 0.5;
            if balance(alpha)? > 0. {
                high = alpha;
            } else {
                low = alpha;
            }
        }
        let alpha = (low + high) * 0.5;
        let (_, cd) = parameters.lift_drag(alpha, 0., 0.)?;
        let thrust = pressure * cd / alpha.cos();
        let forward = speed * alpha.cos();
        let power =
            thrust * (forward + induced_velocity(thrust, forward, rho, parameters.prop_area));
        let throttle = power / (parameters.power * PROPELLER_EFFICIENCY * rho / 1.225);
        let a = &parameters.aero;
        let divisor = a.cm_trim * parameters.trim_limit * parameters.controls[3];
        if divisor.abs() < 1e-9 {
            return Err("Aircraft has no usable trim moment coefficient".into());
        }
        let mut trim = -(a.cm0
            + a.pitch.sample(alpha)?
            + a.cm_stabilizer * parameters.tail_incidence
            + a.cm_gear)
            / divisor;
        // Correct the initial pitch balance for the configured loading and
        // engine arm using the very same force/moment equations as integration.
        let state = State {
            position: [0., 0., -altitude],
            velocity: [speed, 0., 0.],
            orientation: Quaternion::pitch(alpha),
            rates: [0.; 3],
        };
        let derivative = parameters.derivative(
            state,
            Controls {
                throttle,
                trim,
                ..Default::default()
            },
        )?;
        let moment = parameters.loading.inertia.apply(derivative.rates);
        trim += moment[1] / (pressure * parameters.chord * divisor);
        if moment[0].abs() > 1e-6 || moment[2].abs() > 1e-6 {
            return Err(
                "Initial level trim requires laterally balanced loading and engine placement"
                    .into(),
            );
        }
        if !(0. ..=1.).contains(&throttle) || trim.abs() > 1. {
            return Err(format!(
                "Trim exceeds available power/control: throttle {throttle:.3}, trim {trim:.3}"
            ));
        }
        Ok(Self {
            // This dynamics profile accepts one piston engine. Preserve authored
            // CFG data separately from this versioned FSX source initialization.
            electrical: crate::electrical::CircuitNetwork::new_with_source_defaults(
                parameters.electrical.clone(),
                fsx_vehicle::electrical::SourceDefaults::FsxSteam62615SinglePiston,
            )?,
            parameters,
            state: State {
                position: [0., 0., -altitude],
                velocity: [speed, 0., 0.],
                orientation: Quaternion::pitch(alpha),
                rates: [0.; 3],
            },
            controls: Controls {
                throttle,
                trim,
                ..Default::default()
            },
            time: 0.,
            ground_contact: false,
            field_elevation: 0.,
            rpm: 0.,
        })
        .map(|mut s: Self| {
            s.rpm = s.equilibrium_rpm(s.controls.throttle);
            s
        })
    }
    /// Steady fixed-pitch propeller speed: absorbed power grows with RPM cubed
    /// and falls as forward speed unloads the blades, so RPM rises with the
    /// square root of power and with airspeed. Calibrated to about 85 % of
    /// rated RPM static at full power and 100 % at cruise speed; idle fuel flow
    /// keeps about 21 %. A playable model, not the FSX propeller tables.
    pub fn equilibrium_rpm(&self, throttle: f64) -> f64 {
        let p = &self.parameters;
        let forward = self.state.orientation.inverse().rotate(self.state.velocity)[0];
        let unload = (forward / p.cruise_speed.max(1.)).clamp(0., 1.4);
        let power = 0.06 + 0.94 * throttle.clamp(0., 1.);
        p.rated_rpm * (0.85 * power.sqrt() * (1. + 0.176 * unload)).min(1.12)
    }
    pub fn step(&mut self, controls: Controls, dt: f64) -> Result<()> {
        self.step_physics(controls, dt, true)
    }
    fn step_physics(
        &mut self,
        controls: Controls,
        dt: f64,
        independent_electrical: bool,
    ) -> Result<()> {
        if !dt.is_finite() || dt <= 0. || dt > 1. / 60. {
            return Err(
                "Flight integration step must be positive and no larger than 1/60 second".into(),
            );
        }
        let controls = controls.validated()?;
        // Contact dynamics need smaller steps than the airborne update. RK4
        // still integrates the same state and forces, including during liftoff.
        let level = self.parameters.gear.ground_level;
        if dt > 1. / 480. && (-self.state.position[2] - level < 10. || self.ground_contact) {
            let count = (dt / (1. / 480.)).ceil() as usize;
            for _ in 0..count {
                self.step_physics(controls, dt / count as f64, independent_electrical)?;
            }
            return Ok(());
        }
        let s = self.state;
        let a = self.parameters.derivative(s, controls)?;
        let b = self.parameters.derivative(s.plus(a, dt * 0.5), controls)?;
        let c = self.parameters.derivative(s.plus(b, dt * 0.5), controls)?;
        let d = self.parameters.derivative(s.plus(c, dt), controls)?;
        let mut next = s
            .plus(a, dt / 6.)
            .plus(b, dt / 3.)
            .plus(c, dt / 3.)
            .plus(d, dt / 6.);
        next.orientation = next.orientation.normalized()?;
        if !next.finite() || length(next.velocity) > 500. || length(next.rates) > 50. {
            return Err("Flight state left the supported numeric envelope".into());
        }
        if next.position[2] + level >= 0. {
            return Err("Aircraft CG penetrated the ground; reset required".into());
        }
        // Engine speed follows its equilibrium with propeller inertia.
        let target = {
            let previous = self.state;
            self.state = next;
            let target = self.equilibrium_rpm(controls.throttle);
            self.state = previous;
            target
        };
        self.rpm += (target - self.rpm) * (dt / 0.6).min(1.);
        // No electrical mutation until this physics substep has passed all
        // fallible calculations. Contact subdivision advances each interval once.
        if independent_electrical {
            self.electrical.advance_time(dt)?;
        }
        self.ground_contact = self
            .parameters
            .gear
            .forces(next, self.parameters.loading.cg, controls)
            .2
            > 0;
        self.state = next;
        self.controls = controls;
        self.time += dt;
        Ok(())
    }
    pub fn telemetry(&self) -> Result<Telemetry> {
        let velocity = self.state.orientation.inverse().rotate(self.state.velocity);
        let speed = length(velocity);
        let alpha = velocity[2].atan2(velocity[0]);
        Ok(Telemetry {
            speed,
            altitude: -self.state.position[2] + self.field_elevation,
            alpha,
            beta: if speed > 1e-9 {
                (velocity[1] / speed).clamp(-1., 1.).asin()
            } else {
                0.
            },
            attitude: self.state.orientation.euler(),
            climb: -self.state.velocity[2],
            stalled: alpha > self.parameters.aero.lift.peak()?.0,
            ground_contact: self.ground_contact,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::aircraft::own_parameters;
    #[test]
    fn rpm_settles_at_idle_static_power_and_rises_with_airspeed() {
        let mut s = Simulation::on_ground(own_parameters()).unwrap();
        assert!(
            (s.rpm / 2700. - 0.85 * 0.06f64.sqrt()).abs() < 0.01,
            "idle {}",
            s.rpm
        );
        let full = Controls {
            throttle: 1.,
            brake: 1.,
            ..s.controls
        };
        for _ in 0..600 {
            s.step(full, 1. / 120.).unwrap();
        }
        assert!((s.rpm / 2700. - 0.85).abs() < 0.01, "static {}", s.rpm);
        let cruise = Simulation::trimmed(own_parameters(), 55., 300.).unwrap();
        assert!(cruise.equilibrium_rpm(1.) > 2650. && cruise.equilibrium_rpm(1.) <= 2700. * 1.12);
    }
    #[test]
    fn flaps_add_lift_drag_and_nose_down_moment() {
        let mut p = fsx_vehicle::aircraft::own_parameters();
        p.flaps = fsx_vehicle::flaps::Flaps {
            positions: vec![0., 40.],
            extending_time: 5.,
            lift: 0.6,
            drag: 0.06,
            pitch: -0.08,
        };
        let s = State {
            position: [0., 0., -300.],
            velocity: [40., 0., 0.],
            orientation: Quaternion::IDENTITY,
            rates: [0.; 3],
        };
        let clean = p.derivative(s, Controls::default()).unwrap();
        let full = p
            .derivative(
                s,
                Controls {
                    flaps: 1.,
                    ..Default::default()
                },
            )
            .unwrap();
        // More upward (negative down) and backward acceleration, less pitch-up.
        assert!(full.velocity[2] < clean.velocity[2]);
        assert!(full.velocity[0] < clean.velocity[0]);
        assert!(full.rates[1] < clean.rates[1]);
    }
    #[test]
    fn consumer_history_commits_tail_flags_after_load_and_preserves_other_equipment() {
        use crate::legacy_consumers::{self, ConsumerState};
        use crate::legacy_power::Circuit;
        use crate::legacy_response::Response;
        use crate::legacy_voltage::TailEnvironment;
        use fsx_vehicle::flight_electrical::FlightElectrical;
        let mut s = Simulation::trimmed(own_parameters(), 50., 300.).unwrap();
        s.electrical =
            crate::electrical::CircuitNetwork::new_fresh_legacy(s.parameters.electrical.clone())
                .unwrap();
        let config = s.electrical.source_configuration();
        let mut consumers = ConsumerState::fresh(&config);
        consumers
            .update_pitot_activity(Some(true), Some(1.), Some(true))
            .unwrap();
        let mut switches = FlightElectrical::default();
        switches.battery = Some(true);
        switches.generators[0] = Some(false);
        s.electrical.apply_switches(&switches);
        s.electrical
            .update_generator_from_fsx_steam_signals(Some(true), Some(0.), Some(false))
            .unwrap();
        let response = Response::observed(Some(1.)).unwrap();
        let mut tail = TailEnvironment {
            generator_enabled: true,
            incoming_bus_voltage: [24.; 8],
            incoming_generator_amps: [0.; 4],
            battery_failed: false,
            avionics_master: true,
            voltage_mode: true,
            override_power: false,
            additional_system_circuit: Some(Circuit {
                bus: 0,
                min_voltage: 16.8,
                forced_off: false,
            }),
            avionics_circuit: Some(Circuit {
                bus: 0,
                min_voltage: 16.8,
                forced_off: false,
            }),
        };
        let mut direct = s.electrical.clone();
        for expected_load in [5., 25., 25.] {
            assert_eq!(
                legacy_consumers::sum(consumers.records()).unwrap(),
                Some(expected_load)
            );
            let expected = direct
                .step_legacy_electrical_frame(
                    STEP,
                    Some(expected_load),
                    response.factor(),
                    Some(tail),
                )
                .unwrap()
                .unwrap();
            let got = s
                .step_with_legacy_consumer_state(
                    s.controls,
                    STEP,
                    &mut consumers,
                    response,
                    Some(tail),
                )
                .unwrap()
                .unwrap();
            assert_eq!(got, expected);
            assert_eq!(consumers.records()[1].active, Some(1));
            assert_eq!(consumers.records()[5].active, Some(1));
            assert_eq!(consumers.records()[7].active, Some(1));
            tail.incoming_bus_voltage = got.tail.bus_voltage;
            tail.incoming_generator_amps = got.tail.generator_amps;
        }
        let before = *consumers.records();
        let time = s.time;
        let mut invalid = s.controls;
        invalid.pitch = f64::NAN;
        assert!(s
            .step_with_legacy_consumer_state(invalid, STEP, &mut consumers, response, Some(tail))
            .is_err());
        assert_eq!(consumers.records(), &before);
        assert_eq!(s.time, time);
        assert!(s
            .step_with_legacy_consumer_state(
                s.controls,
                STEP,
                &mut consumers,
                Response::default(),
                Some(tail)
            )
            .unwrap()
            .is_none());
        assert_eq!(consumers.records()[1].active, None);
        assert_eq!(consumers.records()[5].active, None);
        assert_eq!(consumers.records()[7].active, Some(1));
        consumers
            .update_pitot_activity(Some(false), None, None)
            .unwrap();
        assert_eq!(consumers.records()[7].active, Some(0));
    }
    #[test]
    fn provider_electrical_frame_commits_once_with_flight_and_rolls_back_on_errors() {
        use crate::legacy_response::Response;
        use crate::legacy_voltage::TailEnvironment;
        use fsx_vehicle::electrical::Bus;
        use fsx_vehicle::flight_electrical::FlightElectrical;
        for ground in [false, true] {
            let p = own_parameters();
            let mut s = if ground {
                Simulation::on_ground(p).unwrap()
            } else {
                Simulation::trimmed(p, 50., 300.).unwrap()
            };
            s.electrical = crate::electrical::CircuitNetwork::new_fresh_legacy(
                s.parameters.electrical.clone(),
            )
            .unwrap();
            let mut switches = FlightElectrical::default();
            switches.battery = Some(true);
            switches.generators[0] = Some(false);
            s.electrical.apply_switches(&switches);
            s.electrical
                .update_generator_from_fsx_steam_signals(Some(true), Some(0.), Some(false))
                .unwrap();
            let response = Response::observed(Some(0.5)).unwrap();
            let mut tail = TailEnvironment {
                generator_enabled: true,
                incoming_bus_voltage: [24.; 8],
                incoming_generator_amps: [0.; 4],
                battery_failed: false,
                avionics_master: false,
                voltage_mode: true,
                override_power: false,
                avionics_circuit: None,
                additional_system_circuit: None,
            };
            let mut direct = s.electrical.clone();
            let mut consumers = [crate::legacy_consumers::Consumer {
                active: Some(0),
                max_amps: None,
            }; crate::legacy_consumers::SLOTS];
            consumers[0] = crate::legacy_consumers::Consumer {
                active: Some(1),
                max_amps: Some(500.),
            };
            consumers[1] = crate::legacy_consumers::Consumer {
                active: Some(2),
                max_amps: Some(12.),
            };
            for _ in 0..120 {
                let expected = direct
                    .step_legacy_electrical_frame(STEP, Some(12.), response.factor(), Some(tail))
                    .unwrap()
                    .unwrap();
                let got = s
                    .step_with_legacy_consumers(s.controls, STEP, &consumers, response, Some(tail))
                    .unwrap()
                    .unwrap();
                assert_eq!(got, expected);
                assert_eq!(s.electrical.last_battery_flow(), None);
                tail.incoming_bus_voltage = got.tail.bus_voltage;
                tail.incoming_generator_amps = got.tail.generator_amps;
            }
            assert!((s.time - 1.).abs() < 1e-12);
            let before = s.clone();
            let mut bad = s.controls;
            bad.pitch = f64::NAN;
            assert!(s
                .step_with_legacy_electrical(bad, STEP, Some(12.), response, Some(tail))
                .is_err());
            assert_eq!(s.state.position, before.state.position);
            assert_eq!(s.time, before.time);
            assert_eq!(
                s.electrical.legacy_voltage_state(),
                before.electrical.legacy_voltage_state()
            );
            assert_eq!(
                s.electrical.bus_voltage(Bus::Main).unwrap(),
                before.electrical.bus_voltage(Bus::Main).unwrap()
            );
            // Force a physical failure after electrical staging, not just invalid controls.
            s.state.velocity = [1000., 0., 0.];
            let failed_state = s.state;
            assert!(s
                .step_with_legacy_electrical(s.controls, STEP, Some(12.), response, Some(tail))
                .is_err());
            assert_eq!(s.state.velocity, failed_state.velocity);
            assert_eq!(s.time, before.time);
            assert_eq!(
                s.electrical.legacy_voltage_state(),
                before.electrical.legacy_voltage_state()
            );
            s.state = before.state;
            assert!(s
                .step_with_legacy_electrical(
                    s.controls,
                    STEP,
                    Some(12.),
                    Response::default(),
                    Some(tail)
                )
                .unwrap()
                .is_none());
            assert_eq!(s.electrical.legacy_voltage_state(), None);
            assert_eq!(s.electrical.bus_voltage(Bus::Main).unwrap(), None);
            s.electrical.reset();
            assert_eq!(s.electrical.legacy_voltage_state(), None);
        }
    }
    #[test]
    fn battery_uses_committed_airborne_and_contact_substep_time_without_double_counting() {
        let mut p = own_parameters();
        p.electrical.battery_max_capacity_amph = Some(40.);
        for ground in [false, true] {
            let mut s = if ground {
                Simulation::on_ground(p.clone()).unwrap()
            } else {
                Simulation::trimmed(p.clone(), 50., 300.).unwrap()
            };
            s.electrical.battery.set_charge_fraction(Some(1.)).unwrap();
            s.electrical.set_battery_current_request(Some(36.)).unwrap();
            let before = s.state.position;
            let mut invalid = s.controls;
            invalid.pitch = f64::NAN;
            assert!(s.step(invalid, STEP).is_err());
            assert_eq!(s.electrical.battery.charge_fraction(), Some(1.));
            assert_eq!(s.electrical.last_battery_flow(), None);
            assert_eq!(s.state.position, before);
            for _ in 0..120 {
                s.step(s.controls, STEP).unwrap();
            }
            assert!((s.time - 1.).abs() < 1e-12);
            assert!((s.electrical.battery.charge_fraction().unwrap() - 0.99975).abs() < 1e-12);
            assert!(
                (s.electrical
                    .last_battery_flow()
                    .unwrap()
                    .average_terminal_amps
                    .unwrap()
                    - 36.)
                    .abs()
                    < 1e-7
            );
            s.electrical.set_battery_current_request(None).unwrap();
            s.step(s.controls, STEP).unwrap();
            assert_eq!(s.electrical.battery.charge_fraction(), None);
        }
    }
    #[test]
    fn shifted_loading_and_engine_arm_are_in_initial_trim() {
        let mut p = own_parameters();
        p.loading.cg = [0.12, 0., -0.08];
        p.engine_position = [2., 0., 0.05];
        p.loading.inertia =
            crate::mass::Inertia::new([[1600., 0., -50.], [0., 2000., 0.], [-50., 0., 3000.]])
                .unwrap();
        let mut s = Simulation::trimmed(p.clone(), 50., 300.).unwrap();
        for _ in 0..7200 {
            s.step(s.controls, STEP).unwrap();
        }
        let t = s.telemetry().unwrap();
        assert!((t.altitude - 300.).abs() < 1e-5);
        assert!((t.speed - 50.).abs() < 1e-6);
        p.loading.cg[1] = 0.2;
        assert!(Simulation::trimmed(p, 50., 300.).is_err());
    }
    #[test]
    fn coupled_free_rotation_obeys_euler_equation() {
        let mut p = own_parameters();
        p.loading.inertia =
            crate::mass::Inertia::new([[1600., 20., -50.], [20., 2000., 30.], [-50., 30., 3000.]])
                .unwrap();
        let state = State {
            position: [0., 0., -300.],
            velocity: [0.; 3],
            orientation: Quaternion::IDENTITY,
            rates: [0.3, -0.2, 0.5],
        };
        let d = p.derivative(state, Controls::default()).unwrap();
        let torque = p.loading.inertia.apply(d.rates);
        let coupling = cross(state.rates, p.loading.inertia.apply(state.rates));
        for i in 0..3 {
            assert!((torque[i] + coupling[i]).abs() < 1e-9);
        }
        assert!(dot(state.rates, torque).abs() < 1e-9);
    }
    #[test]
    fn quaternion_pitch_and_inverse_preserve_vectors() {
        let q = Quaternion::pitch(0.2);
        let rotated = q.rotate([1., 0., 0.]);
        assert!(rotated[2] < 0.);
        let back = q.inverse().rotate(rotated);
        for (a, b) in back.into_iter().zip([1., 0., 0.]) {
            assert!((a - b).abs() < 1e-12);
        }
        assert!((q.euler()[1] - 0.2).abs() < 1e-12);
    }
    #[test]
    fn trim_stays_in_level_flight_without_hidden_controller() {
        let mut s = Simulation::trimmed(own_parameters(), 50., 300.).unwrap();
        let controls = s.controls;
        for _ in 0..7200 {
            s.step(controls, STEP).unwrap();
        }
        let t = s.telemetry().unwrap();
        assert!((t.altitude - 300.).abs() < 1e-5);
        assert!((t.speed - 50.).abs() < 1e-6);
        assert!(t.climb.abs() < 1e-6);
    }
    #[test]
    fn pilot_input_has_expected_pitch_roll_and_yaw_signs() {
        for axis in 0..3 {
            let mut s = Simulation::trimmed(own_parameters(), 50., 300.).unwrap();
            let mut c = s.controls;
            match axis {
                0 => c.roll = 0.1,
                1 => c.pitch = 0.1,
                _ => c.yaw = 0.1,
            };
            for _ in 0..12 {
                s.step(c, STEP).unwrap();
            }
            assert!(s.state.rates[axis] > 0., "axis {axis}: {:?}", s.state.rates);
        }
    }
    #[test]
    fn thrust_is_finite_at_rest_and_power_off_conserves_gravity() {
        assert!(propeller_thrust(100000., 0., 1.225, 3.).is_finite());
        assert_eq!(propeller_thrust(0., 50., 1.225, 3.), 0.);
        assert!((density(0.) - 1.225).abs() < 1e-12);
        assert!(density(3000.) < density(0.));
        let p = own_parameters();
        let state = State {
            position: [0., 0., -300.],
            velocity: [0.; 3],
            orientation: Quaternion::IDENTITY,
            rates: [0.; 3],
        };
        let d = p.derivative(state, Controls::default()).unwrap();
        assert_eq!(d.velocity, [0., 0., GRAVITY]);
    }
    #[test]
    fn stall_curve_and_invalid_trim_are_observable() {
        let p = own_parameters();
        let speed = p.predicted_stall(0.).unwrap();
        assert!(speed > 20. && speed < 35.);
        assert!(p.aero.lift.sample(0.4).unwrap() < p.aero.lift.sample(0.25).unwrap());
        assert!(Simulation::trimmed(p, 10., 300.).is_err());
    }
    #[test]
    fn bad_controls_and_step_do_not_modify_state() {
        let mut s = Simulation::trimmed(own_parameters(), 50., 300.).unwrap();
        let initial = s.state.position;
        assert!(s.step(s.controls, 1.).is_err());
        let mut c = s.controls;
        c.pitch = f64::NAN;
        assert!(s.step(c, STEP).is_err());
        assert_eq!(s.state.position, initial);
        assert_eq!(s.time, 0.);
    }
    #[test]
    fn ground_contact_supports_weight_without_freezing_time() {
        let mut s = Simulation::on_ground(own_parameters()).unwrap();
        assert!(s.ground_contact);
        let position = s.state.position;
        for _ in 0..1200 {
            s.step(s.controls, STEP).unwrap();
        }
        assert!(s.time > 9.9);
        assert!((s.state.position[2] - position[2]).abs() < 0.01);
        assert!(s.telemetry().unwrap().speed < 0.01);
        s.controls.throttle = 1.;
        s.controls.brake = 0.;
        for _ in 0..120 {
            s.step(s.controls, STEP).unwrap();
        }
        assert!(s.state.velocity[0] > 0.5);
    }
    #[test]
    fn controlled_motion_converges_with_smaller_rk4_steps() {
        let run = |dt: f64| {
            let mut s = Simulation::trimmed(own_parameters(), 50., 300.).unwrap();
            let mut c = s.controls;
            c.roll = 0.15;
            c.pitch = 0.05;
            for _ in 0..(1. / dt).round() as usize {
                s.step(c, dt).unwrap();
            }
            s.state
        };
        let coarse = run(1. / 60.);
        let fine = run(1. / 120.);
        let reference = run(1. / 480.);
        let error = |s: State| {
            length(add(s.position, scale(reference.position, -1.)))
                + length(add(s.rates, scale(reference.rates, -1.)))
        };
        assert!(error(fine) < error(coarse));
        assert!(error(fine) < 1e-5);
    }
}
