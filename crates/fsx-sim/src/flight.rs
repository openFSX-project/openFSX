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
            + a.cm_pitch_rate * qhat * self.damping[1];
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
#[derive(Debug)]
pub struct Simulation {
    pub parameters: Parameters,
    pub state: State,
    pub controls: Controls,
    pub time: f64,
    pub ground_contact: bool,
}
impl Simulation {
    pub fn on_ground(parameters: Parameters) -> Result<Self> {
        let mut s = Self::trimmed(parameters, 65. * crate::aircraft::KNOT, 300.)?;
        let p = &s.parameters;
        let height = p.ground_height - (p.loading.cg[2] - p.loading.empty_cg[2]);
        s.state = State {
            position: [-400., 0., -height],
            velocity: [0.; 3],
            orientation: Quaternion::pitch(p.gear.static_pitch),
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
        })
    }
    pub fn step(&mut self, controls: Controls, dt: f64) -> Result<()> {
        if !dt.is_finite() || dt <= 0. || dt > 1. / 60. {
            return Err(
                "Flight integration step must be positive and no larger than 1/60 second".into(),
            );
        }
        let controls = controls.validated()?;
        // Contact dynamics need smaller steps than the airborne update. RK4
        // still integrates the same state and forces, including during liftoff.
        if dt > 1. / 480. && (-self.state.position[2] < 10. || self.ground_contact) {
            let count = (dt / (1. / 480.)).ceil() as usize;
            for _ in 0..count {
                self.step(controls, dt / count as f64)?;
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
        if next.position[2] >= 0. {
            return Err("Aircraft CG penetrated test ground; reset required".into());
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
            altitude: -self.state.position[2],
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
