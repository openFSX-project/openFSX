//! Read-only visual state derived from the independent simulation.
use fsx_model::{animation::PoseState, Geometry, Model};
use fsx_sim::{flight::Simulation, ground::GroundForces};

pub fn lookup(sim: &Simulation, name: &str, units: &str) -> Option<f32> {
    let units = units.trim().to_ascii_lowercase();
    let value = if name.eq_ignore_ascii_case("GEAR CENTER STEER ANGLE") {
        let radians = sim
            .parameters
            .gear
            .center_steering_angle(sim.controls.yaw)?;
        match units.as_str() {
            "radian" | "radians" => radians,
            "degree" | "degrees" => radians.to_degrees(),
            "grad" | "grads" => radians * 200. / std::f64::consts::PI,
            _ => return None,
        }
    } else if name.eq_ignore_ascii_case("GENERAL ENG THROTTLE LEVER POSITION:1") {
        if !(0. ..=1.).contains(&sim.controls.throttle) {
            return None;
        }
        match units.as_str() {
            "percent" => sim.controls.throttle * 100.,
            "part" | "percent over 100" => sim.controls.throttle,
            _ => return None,
        }
    } else if name.eq_ignore_ascii_case("SIM ON GROUND") && units == "bool" {
        f64::from(sim.ground_contact)
    } else {
        return None;
    };
    (value.is_finite() && value.abs() <= f32::MAX as f64).then_some(value as f32)
}

pub struct FlightPose {
    state: PoseState,
    time: Option<f64>,
    visibility: Option<Vec<bool>>,
}
impl FlightPose {
    pub fn frames(&self) -> &[Option<f32>] {
        self.state.frames()
    }
    pub fn new(model: &Model) -> Self {
        Self {
            state: PoseState::new(&model.animations),
            time: None,
            visibility: None,
        }
    }
    pub fn update(
        &mut self,
        model: &Model,
        lod: u32,
        sim: &Simulation,
    ) -> Result<Option<Geometry>, String> {
        if !sim.time.is_finite() || sim.time < 0. {
            return Err("Invalid animation simulation time".into());
        }
        if self.time.is_some_and(|time| sim.time < time) {
            self.state = PoseState::new(&model.animations);
            self.visibility = None;
            self.time = None;
        }
        let elapsed = self.time.map_or(0., |time| sim.time - time);
        let initializing = self.time.is_none();
        let previous = self.frames().to_vec();
        let plan = self
            .state
            .advance(&model.animations, elapsed, |name, units| {
                lookup(sim, name, units)
            })?;
        let visibility =
            model.visibility_plan(|name, units| lookup(sim, name, units).map(f64::from));
        let changed =
            previous != plan.frames || self.visibility.as_ref() != Some(&visibility.nodes);
        if initializing {
            println!(
                "Flight pose: {}/{} animation groups resolved from independent simulation state",
                plan.frames.iter().filter(|f| f.is_some()).count(),
                plan.frames.len()
            );
            for (index, frame) in plan.frames.iter().enumerate() {
                if let Some(frame) = frame {
                    println!("  Flight clip {index}: frame {frame:.4}");
                }
            }
        }
        self.time = Some(sim.time);
        if !changed {
            return Ok(None);
        }
        let mut geometry = model.posed_geometry(Some(lod), &plan.frames)?;
        geometry.apply_visibility(&visibility.nodes)?;
        self.visibility = Some(visibility.nodes);
        Ok(Some(geometry))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn read_only_visual_values_follow_physical_steering_in_explicit_units() {
        let mut sim = Simulation::on_ground(fsx_vehicle::aircraft::own_parameters()).unwrap();
        for yaw in [-0.5, 0., 0.5] {
            let mut controls = sim.controls;
            controls.yaw = yaw;
            sim.step(controls, fsx_sim::flight::STEP).unwrap();
            let radians = sim.parameters.gear.center_steering_angle(yaw).unwrap();
            assert!(
                (lookup(&sim, "GEAR CENTER STEER ANGLE", "radians").unwrap() as f64 - radians)
                    .abs()
                    < 1e-6
            );
            assert!(
                (lookup(&sim, "GEAR CENTER STEER ANGLE", "grads").unwrap() as f64
                    - radians * 200. / std::f64::consts::PI)
                    .abs()
                    < 1e-5
            );
        }
        assert_eq!(lookup(&sim, "ELEVATOR DEFLECTION PCT", "percent"), None);
        assert_eq!(lookup(&sim, "PROP MAX RPM PERCENT:1", "percent"), None);
        assert_eq!(lookup(&sim, "GEAR CENTER STEER ANGLE", "percent"), None);
        sim.parameters
            .gear
            .wheels
            .push(sim.parameters.gear.wheels[0].clone());
        assert_eq!(lookup(&sim, "GEAR CENTER STEER ANGLE", "grads"), None);
    }
}
