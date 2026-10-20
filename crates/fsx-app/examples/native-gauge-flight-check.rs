//! Local installed-content check: persistent callbacks, flight clock and GPU textures.
//! No original FSX process, OS window or input injection.
#[path = "../src/desktop_flight.rs"]
#[allow(dead_code)]
mod desktop_flight;
#[path = "../src/flight_gauges.rs"]
mod flight_gauges;
#[path = "../src/flight_pose.rs"]
mod flight_pose;
#[path = "../src/flight_view.rs"]
#[allow(dead_code)]
mod flight_view;
#[path = "../src/native_gauges.rs"]
#[allow(dead_code)]
mod native_gauges;
use fsx_render::{block_on, Gpu, Orbit};
use fsx_vehicle::aircraft::Aircraft;
use std::path::PathBuf;
use winit::keyboard::KeyCode;

fn ensure(condition: bool, message: &str) -> Result<(), String> {
    condition.then_some(()).ok_or_else(|| message.into())
}
fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(args.next().ok_or(
        "Usage: native-gauge-flight-check <FSX root> <new output directory> [x86 helper] [explicit XML font.ttf]",
    )?)
    .canonicalize()
    .map_err(|e| e.to_string())?;
    let output = PathBuf::from(args.next().ok_or("Expected new output directory")?);
    let helper = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fsx-gauge-host.exe")
    });
    let xml_font = args.next().map(PathBuf::from);
    ensure(args.next().is_none(), "Unexpected argument")?;
    let aircraft = Aircraft::load(&root.join("SimObjects/Airplanes/C172"), 0, true)?;
    let interior = aircraft
        .interior
        .as_deref()
        .ok_or("Missing native interior")?;
    let eye = aircraft.eye_point.ok_or("Missing eye point")?;
    std::fs::create_dir(&output).map_err(|e| e.to_string())?;
    let mut flight = desktop_flight::FlightSession::new(aircraft.parameters.clone())?;
    let mut current = flight_view::Visual::load(interior, Some(&aircraft.texture))?;
    let mut alternate = flight_view::Visual::load(&aircraft.model, Some(&aircraft.texture))?;
    let mut gauges = flight_gauges::FlightGauges::new(&root, &aircraft.panel, &helper)?;
    if let Some(font) = &xml_font {
        gauges = gauges.with_day_xml_plaque(
            &root,
            font,
            aircraft
                .registration
                .as_deref()
                .ok_or("Missing authored registration")?,
        )?;
    }
    let first_pid = gauges.pid();
    let first = gauges.update(&flight.simulation)?;
    ensure(
        first.sequence == 1 && first.evaluated == 1 && !first.reused,
        "Invalid first frame",
    )?;
    gauges.apply(&first, &current.model, &mut current.appearance, None)?;
    current.update(&flight)?;
    alternate.update(&flight)?;
    let mut orbit = Orbit::new(&current.geometry);
    flight_view::camera(&flight, &mut orbit, Some(eye), Default::default())?;
    let mut gpu = block_on(Gpu::new(
        None,
        &current.geometry,
        &current.appearance,
        true,
        960,
        720,
    ))?;
    gpu.model_matrix = flight.model_matrix();
    let baseline = output.join("c172-native-initial.ppm");
    gpu.snapshot(&orbit, &baseline)?;

    // Exercise the GPU COPY_DST path while keeping geometry and camera fixed.
    flight.key(KeyCode::ArrowDown, true);
    flight.key(KeyCode::ArrowRight, true);
    flight.key(KeyCode::F4, true);
    let mut changed = false;
    let mut sprite_changed = [false; 3];
    for _ in 0..120 {
        flight.advance(fsx_sim::flight::STEP)?;
        let frame = gauges.update(&flight.simulation)?;
        ensure(
            !frame.reused && gauges.pid() == first_pid,
            "Helper restarted during forward flight",
        )?;
        changed |= first
            .values
            .iter()
            .zip(&frame.values)
            .any(|(a, b)| (a.result - b.result).abs() > 1e-6);
        for (i, (a, b)) in first.sprites.iter().zip(&frame.sprites).enumerate() {
            sprite_changed[i] |= a
                .results
                .iter()
                .zip(b.results)
                .any(|(a, b)| (a - b).abs() > 1e-6);
        }
        gauges.apply(
            &frame,
            &current.model,
            &mut current.appearance,
            Some(&mut gpu),
        )?;
    }
    flight.key(KeyCode::ArrowDown, false);
    flight.key(KeyCode::ArrowRight, false);
    flight.key(KeyCode::F4, false);
    ensure(changed, "Native readings did not respond to flight state")?;
    ensure(
        sprite_changed.into_iter().all(|v| v),
        "Not all three gyro sprites responded to flight",
    )?;
    let active = output.join("c172-native-live-fixed-camera.ppm");
    gpu.snapshot(&orbit, &active)?;
    let initial_pixels = std::fs::read(&baseline).map_err(|e| e.to_string())?;
    let active_pixels = std::fs::read(&active).map_err(|e| e.to_string())?;
    let changed_bytes = initial_pixels
        .iter()
        .zip(&active_pixels)
        .filter(|(a, b)| a != b)
        .count();
    ensure(changed_bytes > 0, "GPU instrument pixels did not change")?;

    flight.paused = true;
    let paused_time = flight.simulation.time;
    for _ in 0..10 {
        flight.advance(0.1)?;
        let frame = gauges.update(&flight.simulation)?;
        ensure(
            frame.reused && frame.evaluated == 121 && frame.time == paused_time,
            "Pause evaluated callbacks or advanced the clock",
        )?;
    }
    let paused = output.join("c172-native-paused.ppm");
    gpu.snapshot(&orbit, &paused)?;
    ensure(
        std::fs::read(&paused).map_err(|e| e.to_string())? == active_pixels,
        "Pause changed rendered pixels",
    )?;

    current.exchange(&mut alternate, Some(&mut gpu))?;
    current.exchange(&mut alternate, Some(&mut gpu))?;
    let returned = output.join("c172-native-view-return.ppm");
    gpu.snapshot(&orbit, &returned)?;
    ensure(
        std::fs::read(returned).map_err(|e| e.to_string())? == active_pixels,
        "View round trip lost dynamic cockpit pixels",
    )?;

    // Update while the exterior is active, then upload the latest interior on return.
    current.exchange(&mut alternate, Some(&mut gpu))?;
    flight.paused = false;
    flight.advance(fsx_sim::flight::STEP)?;
    let frame = gauges.update(&flight.simulation)?;
    gauges.apply(&frame, &alternate.model, &mut alternate.appearance, None)?;
    current.exchange(&mut alternate, Some(&mut gpu))?;
    let exterior_updated = output.join("c172-native-exterior-updated.ppm");
    gpu.snapshot(&orbit, &exterior_updated)?;
    gauges.apply(
        &frame,
        &current.model,
        &mut current.appearance,
        Some(&mut gpu),
    )?;
    ensure(
        {
            let direct = output.join("c172-native-direct-updated.ppm");
            gpu.snapshot(&orbit, &direct)?;
            std::fs::read(direct).map_err(|e| e.to_string())?
                == std::fs::read(exterior_updated).map_err(|e| e.to_string())?
        },
        "Exterior flight lost the latest instrument image",
    )?;

    flight.reset()?;
    let reset = gauges.update(&flight.simulation)?;
    ensure(
        gauges.pid() != first_pid && reset.sequence == 1 && reset.evaluated == 1 && !reset.reused,
        "Reset did not start a fresh native module session",
    )?;
    ensure(
        first
            .values
            .iter()
            .zip(&reset.values)
            .all(|(a, b)| a.result == b.result),
        "Reset changed initial native results",
    )?;
    ensure(
        first
            .sprites
            .iter()
            .zip(&reset.sprites)
            .all(|(a, b)| a.results == b.results),
        "Reset changed initial gyro sprite results",
    )?;
    current.update(&flight)?;
    gpu.update_geometry(&current.geometry)?;
    gpu.model_matrix = flight.model_matrix();
    flight_view::camera(&flight, &mut orbit, Some(eye), Default::default())?;
    gauges.apply(
        &reset,
        &current.model,
        &mut current.appearance,
        Some(&mut gpu),
    )?;
    let reset_path = output.join("c172-native-reset.ppm");
    gpu.snapshot(&orbit, &reset_path)?;
    ensure(
        std::fs::read(reset_path).map_err(|e| e.to_string())? == initial_pixels,
        "Reset did not restore initial cockpit pixels",
    )?;
    gauges.close()?;
    ensure(gauges.pid() == 0, "Helper remained attached after close")?;

    // Ground air data must also work with the actual loaded contact geometry.
    let ground = desktop_flight::FlightSession::on_ground(aircraft.parameters)?;
    let mut ground_gauges = flight_gauges::FlightGauges::new(&root, &aircraft.panel, &helper)?;
    if let Some(font) = &xml_font {
        ground_gauges = ground_gauges.with_day_xml_plaque(
            &root,
            font,
            aircraft
                .registration
                .as_deref()
                .ok_or("Missing authored registration")?,
        )?;
    }
    let ground_frame = ground_gauges.update(&ground.simulation)?;
    let ground_airspeed =
        fsx_sim::air_data::AirData::from_telemetry(ground.simulation.telemetry()?)?.native_inputs()
            [0];
    ensure(
        (ground_frame.values[0].result - ground_airspeed).abs() < 1e-9 && ground_airspeed < 0.1,
        "Settled ground airspeed does not match the small residual physics velocity",
    )?;
    ground_gauges.close()?;
    let report = serde_json::json!({"schema":1,"source":"openfsx-native-gauge-flight-check",
        "forward_frames":121,"paused_reuses":10,"changed_rgb_bytes":changed_bytes,
        "flight_driven_sprites":3,
        "xml_day_plaque":xml_font.is_some(),"xml_font_path":xml_font,"registration":aircraft.registration,
        "reset_restarted_helper":true,"reset_pixels_identical":true,"view_pixels_identical":true,
        "ground_airspeed_knots":ground_airspeed,"full_panel_services":false,"fsx_parity_validated":false});
    std::fs::write(output.join("report.json"), report.to_string()).map_err(|e| e.to_string())?;
    println!("PASS: {report}");
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Native flight check failed: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
