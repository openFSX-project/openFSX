//! Offscreen flight-view integration; never creates an OS window or sends input.
#[path = "../src/desktop_flight.rs"]
#[allow(dead_code)]
mod desktop_flight;
#[path = "../src/flight_pose.rs"]
mod flight_pose;
#[path = "../src/flight_view.rs"]
#[allow(dead_code)]
mod flight_view;
use fsx_render::{block_on, Gpu, Orbit};
use fsx_vehicle::aircraft::Aircraft;
use std::path::PathBuf;
use winit::keyboard::KeyCode;

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(
        args.next()
            .ok_or("Usage: flight-view-check <FSX installation> <new output directory>")?,
    );
    let output = PathBuf::from(args.next().ok_or("Expected new output directory")?);
    if args.next().is_some() {
        return Err("Unexpected argument".into());
    }
    let aircraft = Aircraft::load(&root.join("SimObjects/Airplanes/C172"), 0, true)?;
    let interior = aircraft
        .interior
        .as_deref()
        .ok_or("No configured interior")?;
    let eye = aircraft.eye_point.ok_or("No configured eye point")?;
    println!("CFG eye point, body metres: {eye:?}");
    std::fs::create_dir(&output).map_err(|e| e.to_string())?;
    for name in ["ground", "level", "bank-right"] {
        let mut flight = if name == "ground" {
            desktop_flight::FlightSession::on_ground(aircraft.parameters.clone())?
        } else {
            desktop_flight::FlightSession::new(aircraft.parameters.clone())?
        };
        let mut current = flight_view::Visual::load(&aircraft.model, Some(&aircraft.texture))?;
        let mut alternate = flight_view::Visual::load(interior, Some(&aircraft.texture))?;
        current.update(&flight)?;
        alternate.update(&flight)?;
        if name == "bank-right" {
            flight.key(KeyCode::ArrowRight, true);
            for _ in 0..120 {
                flight.advance(fsx_sim::flight::STEP)?;
                current.update(&flight)?;
                alternate.update(&flight)?;
            }
            flight.key(KeyCode::ArrowRight, false);
        }
        let state = flight.simulation.state;
        let time = flight.simulation.time;
        let throttle = flight.simulation.controls.throttle;
        let mut orbit = Orbit::new(&current.geometry);
        flight_view::camera(&flight, &mut orbit, None, flight_view::Look::default())?;
        let mut gpu = block_on(Gpu::new(
            None,
            &current.geometry,
            &current.appearance,
            true,
            960,
            720,
        ))?;
        gpu.model_matrix = flight.model_matrix();
        let before = output.join(format!("{name}-exterior-before.ppm"));
        gpu.snapshot(&orbit, &before)?;
        current.exchange(&mut alternate, Some(&mut gpu))?;
        flight_view::camera(&flight, &mut orbit, Some(eye), flight_view::Look::default())?;
        println!(
            "{name}: {} | world eye {:?} | {}",
            current.model.name,
            orbit.camera.unwrap().eye(),
            flight.title()?
        );
        gpu.snapshot(&orbit, &output.join(format!("c172-cockpit-{name}.ppm")))?;
        current.exchange(&mut alternate, Some(&mut gpu))?;
        orbit = Orbit::new(&current.geometry);
        flight_view::camera(&flight, &mut orbit, None, flight_view::Look::default())?;
        let after = output.join(format!("{name}-exterior-after.ppm"));
        gpu.snapshot(&orbit, &after)?;
        if std::fs::read(before).map_err(|e| e.to_string())?
            != std::fs::read(after).map_err(|e| e.to_string())?
        {
            return Err(
                "Exterior pixels changed after a view round trip with unchanged flight state"
                    .into(),
            );
        }
        if time != flight.simulation.time
            || state.position != flight.simulation.state.position
            || state.orientation.0 != flight.simulation.state.orientation.0
            || throttle != flight.simulation.controls.throttle
        {
            return Err("View selection changed simulation state".into());
        }
        current.exchange(&mut alternate, Some(&mut gpu))?;
        flight.key(KeyCode::F4, true);
        flight.advance(fsx_sim::flight::STEP)?;
        current.update(&flight)?;
        alternate.update(&flight)?;
        gpu.update_geometry(&current.geometry)?;
        gpu.model_matrix = flight.model_matrix();
        let look = flight_view::Look {
            pitch: -0.65,
            ..Default::default()
        };
        flight_view::camera(&flight, &mut orbit, Some(eye), look)?;
        gpu.snapshot(
            &orbit,
            &output.join(format!("{name}-cockpit-look-down.ppm")),
        )?;
        flight.paused = true;
        flight.advance(1.)?;
        if current
            .pose
            .update(&current.model, current.geometry.lod, &flight.simulation)?
            .is_some()
        {
            return Err("Paused view advanced animation".into());
        }
    }
    println!("PASS: configured interior/eye, ground/air/bank cameras, unchanged physics and identical exterior pixels after view round trip; one GPU device, no OS window/input. FSX camera/gauge parity remains unverified.");
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Flight view check failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
