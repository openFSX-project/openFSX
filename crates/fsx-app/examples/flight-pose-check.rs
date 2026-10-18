//! Local-content integration check. No OS input, window, FSX process or DLL.
#[path = "../src/desktop_flight.rs"]
// The offscreen check intentionally does not exercise this controller's UI methods.
#[allow(dead_code)]
mod desktop_flight;
#[path = "../src/flight_pose.rs"]
mod flight_pose;
use fsx_model::Model;
use fsx_render::{block_on, Appearance, Gpu, Orbit};
use fsx_vehicle::aircraft::Aircraft;
use std::path::PathBuf;
use winit::keyboard::KeyCode;

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(args.next().ok_or(
        "Usage: flight-pose-check <FSX installation> <new output directory> [--interior]",
    )?);
    let output = PathBuf::from(args.next().ok_or("Expected new output directory")?);
    let interior = match args.next() {
        None => false,
        Some(flag) if flag == "--interior" => true,
        _ => return Err("Unexpected argument".into()),
    };
    if args.next().is_some() {
        return Err("Unexpected argument".into());
    }
    let aircraft = Aircraft::load(&root.join("SimObjects/Airplanes/C172"), 0, true)?;
    let model_path = if interior {
        aircraft
            .interior
            .clone()
            .ok_or("No configured interior model")?
    } else {
        aircraft.model.clone()
    };
    let model = Model::load(&model_path)?;
    let base = model.geometry(if interior { None } else { Some(150) })?;
    let lod = base.lod;
    let appearance = Appearance::load(&model, &base, Some(&aircraft.texture))?;
    let mut orbit = Orbit::new(&base);
    orbit.distance = (orbit.distance / 1.6).clamp(orbit.radius * 1.1, orbit.radius * 30.);
    if interior {
        // Stock-C172 validation camera. Eye is loaded from CFG metadata;
        // aim is chosen to inspect the throttle, not to
        // claim FSX default-view parity or a general cockpit camera system.
        let body = aircraft
            .eye_point
            .ok_or("No configured cockpit eye point")?;
        let eye = [body[1] as f32, -body[2] as f32, body[0] as f32];
        orbit.center = [0., -0.1, 0.5];
        let delta = std::array::from_fn::<_, 3, _>(|i| eye[i] - orbit.center[i]);
        orbit.radius = 0.6;
        orbit.distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
        orbit.yaw = delta[0].atan2(delta[2]);
        orbit.pitch = (delta[1] / orbit.distance).asin();
        orbit.far = 50.;
    }
    std::fs::create_dir(&output).map_err(|e| e.to_string())?;
    for (name, key) in [
        ("center", None),
        ("right", Some(KeyCode::KeyE)),
        ("left", Some(KeyCode::KeyQ)),
        ("full-power", Some(KeyCode::F4)),
    ] {
        if interior && matches!(key, Some(KeyCode::KeyQ | KeyCode::KeyE)) {
            continue;
        }
        let mut flight = desktop_flight::FlightSession::on_ground(aircraft.parameters.clone())?;
        let mut pose = flight_pose::FlightPose::new(&model);
        let mut geometry = pose
            .update(&model, lod, &flight.simulation)?
            .ok_or("Initial pose not produced")?;
        let initial = pose.frames().to_vec();
        if let Some(key) = key {
            flight.key(key, true);
        }
        flight.advance(fsx_sim::flight::STEP)?;
        if let Some(next) = pose.update(&model, lod, &flight.simulation)? {
            geometry = next;
        }
        if key.is_some() && pose.frames() == initial {
            return Err("Control input did not change native animation frames".into());
        }
        let frames = pose.frames().to_vec();
        let time = flight.simulation.time;
        flight.paused = true;
        flight.advance(1.)?;
        if flight.simulation.time != time
            || pose.update(&model, lod, &flight.simulation)?.is_some()
            || pose.frames() != frames
        {
            return Err("Paused simulation advanced visual state".into());
        }
        let grads = flight_pose::lookup(&flight.simulation, "GEAR CENTER STEER ANGLE", "grads")
            .ok_or("No physical steering angle")?;
        println!(
            "{name}: yaw {}, steering {grads:.6} grads; frames {:?}",
            flight.simulation.controls.yaw,
            frames
                .iter()
                .enumerate()
                .filter_map(|(i, v)| v.map(|v| (i, v)))
                .collect::<Vec<_>>()
        );
        println!(
            "{name}: throttle {}, model-local position {:?}",
            flight.simulation.controls.throttle,
            pose.local_value("Engine1ThrottlePosition", "percent")
                .ok_or("Stock C172 throttle did not publish its model-local position")?
        );
        let gpu = block_on(Gpu::new(None, &geometry, &appearance, false, 960, 720))?;
        let prefix = if interior {
            "c172-cockpit-throttle"
        } else {
            "c172-flight-steer"
        };
        gpu.snapshot(&orbit, &output.join(format!("{prefix}-{name}.ppm")))?;
        let toggle = if flight.simulation.controls.throttle < 0.5 {
            KeyCode::F4
        } else {
            KeyCode::F1
        };
        flight.key(toggle, true);
        if pose.update(&model, lod, &flight.simulation)?.is_some() || pose.frames() != frames {
            return Err("Paused input mutated model animation state".into());
        }
        flight.reset()?;
        if pose.update(&model, lod, &flight.simulation)?.is_none() || pose.frames() != initial {
            return Err("Reset did not reinitialize visual state".into());
        }
    }
    println!("PASS: controller -> simulation -> native pose -> GPU; pause/reset verified. Fixed model-space camera isolates deformation from aircraft motion. No FSX parity claim.");
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Flight pose check failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
