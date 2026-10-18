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
    let root = PathBuf::from(
        args.next()
            .ok_or("Usage: flight-pose-check <FSX installation> <new output directory>")?,
    );
    let output = PathBuf::from(args.next().ok_or("Expected new output directory")?);
    if args.next().is_some() {
        return Err("Unexpected argument".into());
    }
    let aircraft = Aircraft::load(&root.join("SimObjects/Airplanes/C172"), 0, true)?;
    let model = Model::load(&aircraft.model)?;
    let base = model.geometry(Some(150))?;
    let appearance = Appearance::load(&model, &base, Some(&aircraft.texture))?;
    let mut orbit = Orbit::new(&base);
    orbit.distance = (orbit.distance / 1.6).clamp(orbit.radius * 1.1, orbit.radius * 30.);
    std::fs::create_dir(&output).map_err(|e| e.to_string())?;
    for (name, key) in [
        ("center", None),
        ("right", Some(KeyCode::KeyE)),
        ("left", Some(KeyCode::KeyQ)),
    ] {
        let mut flight = desktop_flight::FlightSession::on_ground(aircraft.parameters.clone())?;
        let mut pose = flight_pose::FlightPose::new(&model);
        let mut geometry = pose
            .update(&model, 150, &flight.simulation)?
            .ok_or("Initial pose not produced")?;
        let initial = pose.frames().to_vec();
        if let Some(key) = key {
            flight.key(key, true);
        }
        flight.advance(fsx_sim::flight::STEP)?;
        if let Some(next) = pose.update(&model, 150, &flight.simulation)? {
            geometry = next;
        }
        if key.is_some() && pose.frames() == initial {
            return Err("Steering input did not change native animation frames".into());
        }
        let frames = pose.frames().to_vec();
        let time = flight.simulation.time;
        flight.paused = true;
        flight.advance(1.)?;
        if flight.simulation.time != time
            || pose.update(&model, 150, &flight.simulation)?.is_some()
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
        let gpu = block_on(Gpu::new(None, &geometry, &appearance, false, 960, 720))?;
        gpu.snapshot(
            &orbit,
            &output.join(format!("c172-flight-steer-{name}.ppm")),
        )?;
        flight.reset()?;
        if pose.update(&model, 150, &flight.simulation)?.is_none() || pose.frames() != initial {
            return Err("Reset did not reinitialize visual state".into());
        }
    }
    println!("PASS: physical steering -> native pose -> GPU; pause/reset verified. Fixed model-space camera isolates deformation from aircraft motion. No FSX parity claim.");
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
