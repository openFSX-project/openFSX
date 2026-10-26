//! Flight visual resources and an eye point attached to the aircraft body.
use crate::{desktop_flight::FlightSession, flight_pose::FlightPose};
use fsx_model::{Geometry, Model};
use fsx_render::{Appearance, Camera, Gpu, Orbit};
use std::path::Path;

pub struct Visual {
    pub model: Model,
    pub geometry: Geometry,
    pub appearance: Appearance,
    pub pose: FlightPose,
}
impl Visual {
    pub fn exchange(&mut self, other: &mut Self, gpu: Option<&mut Gpu>) -> Result<(), String> {
        if let Some(gpu) = gpu {
            gpu.replace_scene(&other.geometry, &other.appearance)?;
        }
        std::mem::swap(self, other);
        Ok(())
    }
    pub fn load(path: &Path, textures: Option<&Path>) -> Result<Self, String> {
        let model = Model::load(path)?;
        let geometry = model.geometry(None)?;
        let appearance = Appearance::load(&model, &geometry, textures)?;
        let pose = FlightPose::new(&model);
        Ok(Self {
            model,
            geometry,
            appearance,
            pose,
        })
    }
    pub fn update(&mut self, flight: &FlightSession) -> Result<(), String> {
        if let Some(geometry) =
            self.pose
                .update(&self.model, self.geometry.lod, &flight.simulation)?
        {
            self.geometry = geometry;
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
pub struct Look {
    pub yaw: f32,
    pub pitch: f32,
    pub fov: f32,
}
impl Default for Look {
    fn default() -> Self {
        Self {
            yaw: 0.,
            pitch: 0.,
            fov: 45.,
        }
    }
}
impl Look {
    pub fn drag(&mut self, dx: f64, dy: f64) {
        self.yaw = (self.yaw + dx as f32 * 0.008).rem_euclid(std::f32::consts::TAU);
        self.pitch = (self.pitch - dy as f32 * 0.008).clamp(-1.45, 1.45);
    }
    pub fn zoom(&mut self, amount: f32) {
        self.fov = (self.fov * (-amount * 0.12).exp()).clamp(15., 90.);
    }
}
pub fn camera(
    flight: &FlightSession,
    orbit: &mut Orbit,
    eye_point: Option<[f64; 3]>,
    look: Look,
) -> Result<(), String> {
    let Some(eye_point) = eye_point else {
        orbit.camera = None;
        orbit.center = flight.center();
        orbit.yaw = flight.heading() + std::f32::consts::PI + 0.35;
        orbit.pitch = 0.25;
        orbit.far = 90000.;
        return Ok(());
    };
    let eye_model = [
        eye_point[1] as f32,
        -eye_point[2] as f32,
        eye_point[0] as f32,
    ];
    let eye = fsx_geometry::transform(flight.model_matrix(), eye_model);
    let q = flight.simulation.state.orientation;
    let heading = look.yaw as f64;
    let pitch = look.pitch as f64;
    let to_world = |v| {
        let [n, e, d] = q.rotate(v);
        [e as f32, -d as f32, n as f32]
    };
    let forward = to_world([
        pitch.cos() * heading.cos(),
        pitch.cos() * heading.sin(),
        -pitch.sin(),
    ]);
    let up = to_world([0., 0., -1.]);
    orbit.camera = Some(Camera::new(
        eye,
        std::array::from_fn(|i| eye[i] + forward[i]),
        up,
        0.01,
        20000.,
        look.fov,
    )?);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eye_tracks_loaded_cg_translation_and_roll_without_resetting_flight() {
        let mut flight = FlightSession::on_ground(fsx_vehicle::aircraft::own_parameters()).unwrap();
        flight.simulation.parameters.loading.cg = [0.3, -0.2, 0.1];
        flight.simulation.state.position = [30., 50., -100.];
        let half = 0.4f64 / 2.;
        flight.simulation.state.orientation =
            fsx_geometry::Quaternion([half.cos(), half.sin(), 0., 0.]);
        let eye = [-0.5, -0.25, -0.7];
        let mut orbit = Orbit {
            camera: None,
            center: [0.; 3],
            radius: 1.,
            yaw: 0.,
            pitch: 0.,
            distance: 10.,
            far: 1000.,
        };
        camera(&flight, &mut orbit, Some(eye), Look::default()).unwrap();
        let delta = std::array::from_fn(|i| eye[i] - flight.simulation.parameters.loading.cg[i]);
        let [n, e, d] = flight.simulation.state.orientation.rotate(delta);
        let expected = [50. + e, 100. - d, 30. + n];
        for (actual, expected) in orbit.camera.unwrap().eye().into_iter().zip(expected) {
            assert!((actual as f64 - expected).abs() < 1e-4);
        }
        let [n, e, d] = flight.simulation.state.orientation.rotate([0., 0., -1.]);
        for (actual, expected) in orbit.camera.unwrap().up().into_iter().zip([e, -d, n]) {
            assert!((actual as f64 - expected).abs() < 1e-6);
        }
        camera(&flight, &mut orbit, None, Look::default()).unwrap();
        assert!(orbit.camera.is_none());
        assert_eq!(flight.simulation.time, 0.);
        assert_eq!(flight.simulation.state.position, [30., 50., -100.]);
    }
}
