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
        "Usage: native-gauge-flight-check <FSX root> <new output directory> [x86 helper] [explicit XML font.ttf] [local clock HH:MM:SS]",
    )?)
    .canonicalize()
    .map_err(|e| e.to_string())?;
    let output = PathBuf::from(args.next().ok_or("Expected new output directory")?);
    let helper = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fsx-gauge-host.exe")
    });
    let xml_font = args.next().map(PathBuf::from);
    let local_clock = args
        .next()
        .map(|s| fsx_sim::clock::LocalClock::parse(s.to_str().ok_or("Invalid clock text")?))
        .transpose()?;
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
    if let Some(clock) = local_clock {
        gauges = gauges.with_day_native_clock(&root, clock)?;
    }
    let first_pid = gauges.pid();
    let mut button_reports = Vec::new();
    let first = gauges.update(&flight.simulation)?;
    ensure(
        first.clock.is_some() == local_clock.is_some(),
        "Clock profile/result mismatch",
    )?;
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
    let mut clock_comparisons = Vec::new();
    let mut clock_event_reports = Vec::new();
    if local_clock.is_some() {
        ensure(
            first.clock.as_ref().unwrap().text == reset.clock.as_ref().unwrap().text,
            "Reset did not restore local clock origin",
        )?;
        let inputs = [75., 500., 256., 0., 0., 90.];
        let mut clock_pixels = Vec::new();
        let mut fixed_results: Option<native_gauges::Frame> = None;
        for (i, time) in [0.75, 1.25, 61.25, 86401.25].into_iter().enumerate() {
            let frame = gauges.update_inputs(time, inputs)?;
            if let Some(previous) = &fixed_results {
                ensure(
                    previous
                        .values
                        .iter()
                        .zip(&frame.values)
                        .all(|(a, b)| a.result == b.result)
                        && previous
                            .sprites
                            .iter()
                            .zip(&frame.sprites)
                            .all(|(a, b)| a.results == b.results),
                    "Clock-only GPU comparison changed another instrument",
                )?;
            } else {
                fixed_results = Some(frame.clone());
            }
            gauges.apply(
                &frame,
                &current.model,
                &mut current.appearance,
                Some(&mut gpu),
            )?;
            let path = output.join(format!("c172-clock-boundary-{i}.ppm"));
            gpu.snapshot(&orbit, &path)?;
            let pixels = std::fs::read(path).map_err(|e| e.to_string())?;
            if i > 0 {
                ensure(
                    pixels != clock_pixels[i - 1],
                    "Clock boundary produced unchanged cockpit pixels",
                )?;
            }
            let paused = gauges.update_inputs(time, inputs)?;
            ensure(
                paused.reused
                    && paused.clock.as_ref().unwrap().text == frame.clock.as_ref().unwrap().text,
                "Clock pause did not reuse the string",
            )?;
            gauges.apply(
                &paused,
                &current.model,
                &mut current.appearance,
                Some(&mut gpu),
            )?;
            let path = output.join(format!("c172-clock-boundary-{i}-paused.ppm"));
            gpu.snapshot(&orbit, &path)?;
            ensure(
                std::fs::read(path).map_err(|e| e.to_string())? == pixels,
                "Paused clock changed cockpit pixels",
            )?;
            clock_comparisons.push(serde_json::json!({"elapsed":time,"text":frame.clock.as_ref().unwrap().text,"paused_reused":true,"paused_pixels_identical":true}));
            clock_pixels.push(pixels);
        }
        ensure(
            clock_pixels[1] == clock_pixels[3],
            "A whole day changed the same time-of-day pixels",
        )?;
        let pid = gauges.pid();
        let rewound = gauges.update_inputs(0.75, inputs)?;
        ensure(
            gauges.pid() != pid && rewound.sequence == 1,
            "Clock rewind did not restart the connected native session",
        )?;
        gauges.apply(
            &rewound,
            &current.model,
            &mut current.appearance,
            Some(&mut gpu),
        )?;
        let path = output.join("c172-clock-boundary-rewound.ppm");
        gpu.snapshot(&orbit, &path)?;
        ensure(
            std::fs::read(path).map_err(|e| e.to_string())? == clock_pixels[0],
            "Clock rewind did not restore the same pixels",
        )?;
        let edit_time = 0.75;
        let edit_pid = gauges.pid();
        for (name, data) in [
            ("CLOCK_HOURS_SET", 13),
            ("CLOCK_MINUTES_SET", 34),
            ("CLOCK_SECONDS_ZERO", 0),
            ("CLOCK_HOURS_INC", 123),
            ("CLOCK_MINUTES_INC", 123),
            ("CLOCK_MINUTES_DEC", 0),
            ("CLOCK_HOURS_DEC", 0),
        ] {
            gauges.clock_event(
                fsx_sim::clock::ClockEvent::from_name(name)?,
                data,
                edit_time,
            )?;
            let frame = gauges.update_inputs(edit_time, inputs)?;
            ensure(
                frame.reused
                    && frame.display_changed
                    && frame.evaluated == 1
                    && gauges.pid() == edit_pid,
                "Paused clock edit advanced/restarted physics or failed to invalidate pixels",
            )?;
            clock_event_reports.push(serde_json::json!({"event":name,"data":data,"text":frame.clock.as_ref().unwrap().text,"physics_reused":true}));
        }
        let edited = gauges.update_inputs(edit_time, inputs)?;
        ensure(
            !edited.display_changed && edited.clock.as_ref().unwrap().text == "13:34:00",
            "Clock events produced the wrong final time",
        )?;
        for (event, data) in [
            (fsx_sim::clock::ClockEvent::HoursSet, 24),
            (fsx_sim::clock::ClockEvent::MinutesSet, 60),
        ] {
            ensure(
                gauges.clock_event(event, data, edit_time).is_err(),
                "Invalid clock SET was accepted",
            )?;
        }
        ensure(
            gauges
                .clock_event(fsx_sim::clock::ClockEvent::HoursInc, 0, 1.25)
                .is_err(),
            "Clock event at a stale physics time was accepted",
        )?;
        let unchanged = gauges.update_inputs(edit_time, inputs)?;
        ensure(
            unchanged.clock.as_ref().unwrap().text == "13:34:00" && !unchanged.display_changed,
            "Rejected clock event mutated state",
        )?;
        gauges.apply(
            &edited,
            &current.model,
            &mut current.appearance,
            Some(&mut gpu),
        )?;
        let path = output.join("c172-clock-edited.ppm");
        gpu.snapshot(&orbit, &path)?;
        let edited_pixels = std::fs::read(path).map_err(|e| e.to_string())?;
        ensure(
            edited_pixels != clock_pixels[0],
            "Clock edit did not change GPU pixels",
        )?;
        gauges.select_clock(25)?;
        let selected = gauges.update_inputs(edit_time, inputs)?;
        ensure(
            selected.reused
                && selected.display_changed
                && selected.evaluated == 1
                && gauges.pid() == edit_pid,
            "Paused selection changed physics/session",
        )?;
        gauges.apply(
            &selected,
            &current.model,
            &mut current.appearance,
            Some(&mut gpu),
        )?;
        let path = output.join("c172-clock-edited-selected.ppm");
        gpu.snapshot(&orbit, &path)?;
        ensure(
            std::fs::read(path).map_err(|e| e.to_string())? != edited_pixels,
            "Paused selection did not reach GPU pixels",
        )?;
        gauges.select_clock(0)?;
        let unselected = gauges.update_inputs(edit_time, inputs)?;
        gauges.apply(
            &unselected,
            &current.model,
            &mut current.appearance,
            Some(&mut gpu),
        )?;
        let path = output.join("c172-clock-edited-unselected.ppm");
        gpu.snapshot(&orbit, &path)?;
        ensure(
            std::fs::read(path).map_err(|e| e.to_string())? == edited_pixels,
            "Deselection did not restore edited clock pixels",
        )?;
        gauges.restart()?;
        let restored = gauges.update_inputs(edit_time, inputs)?;
        gauges.apply(
            &restored,
            &current.model,
            &mut current.appearance,
            Some(&mut gpu),
        )?;
        let path = output.join("c172-clock-edit-reset.ppm");
        gpu.snapshot(&orbit, &path)?;
        ensure(
            std::fs::read(path).map_err(|e| e.to_string())? == clock_pixels[0],
            "Reset did not restore the configured clock origin",
        )?;
        // Reset must also work before simulation time has ever advanced.
        gauges.restart()?;
        gauges.update_inputs(0., inputs)?;
        gauges.clock_event(fsx_sim::clock::ClockEvent::HoursSet, 13, 0.)?;
        gauges.select_clock(25)?;
        gauges.update_inputs(0., inputs)?;
        gauges.restart()?;
        let zero_reset = gauges.update_inputs(0., inputs)?;
        gauges.apply(
            &zero_reset,
            &current.model,
            &mut current.appearance,
            Some(&mut gpu),
        )?;
        let path = output.join("c172-clock-edit-zero-reset.ppm");
        gpu.snapshot(&orbit, &path)?;
        ensure(
            std::fs::read(path).map_err(|e| e.to_string())? == clock_pixels[0],
            "Reset at elapsed zero did not restore origin and clear selection",
        )?;
        // Native button callbacks share the same three-library flight process.
        for (event, data) in [
            (fsx_sim::clock::ClockEvent::HoursSet, 12),
            (fsx_sim::clock::ClockEvent::MinutesSet, 34),
            (fsx_sim::clock::ClockEvent::SecondsZero, 0),
        ] {
            gauges.clock_event(event, data, 0.)?;
        }
        let before = gauges.update_inputs(0.5, inputs)?;
        let button_pid = gauges.pid();
        gauges.apply(
            &before,
            &current.model,
            &mut current.appearance,
            Some(&mut gpu),
        )?;
        let path = output.join("c172-clock-button-before.ppm");
        gpu.snapshot(&orbit, &path)?;
        let button_baseline = std::fs::read(path).map_err(|e| e.to_string())?;
        ensure(
            gauges.click_clock(3, 0.5).is_err() && gauges.click_clock(0, 0.75).is_err(),
            "Invalid/stale mouse input was accepted",
        )?;
        let invalid = gauges.update_inputs(0.5, inputs)?;
        ensure(
            invalid.clock.as_ref().unwrap().events == 0
                && !invalid.display_changed
                && gauges.pid() == button_pid,
            "Rejected mouse input changed native state",
        )?;
        let mut previous_pixels = None;
        for (i, (time, child, expected, result, events)) in [
            (0.5, Some(0), " 0:00:00", 0.6, 1),
            (0.5, Some(1), " 0:00:00", 0.6, 2),
            (1.75, None, " 0:00:01", 125.6, 2),
            (1.75, None, " 0:00:01", 125.6, 2),
            (61.75, None, " 0:01:01", 6125.6, 2),
            (61.75, Some(1), " 0:01:01", 6125.6, 3),
            (121.75, None, " 0:01:01", 6125.6, 3),
            (121.75, Some(1), " 0:00:00", 0.6, 4),
            (121.75, Some(1), " 0:00:00", 0.6, 5),
            (123., None, " 0:00:01", 125.6, 5),
            (123., Some(2), " 0:00:01", 125.6, 6),
        ]
        .into_iter()
        .enumerate()
        {
            let context = gauges.update_inputs(time, inputs)?;
            let frame = if let Some(child) = child {
                gauges.click_clock(child, time)?;
                let frame = gauges.update_inputs(time, inputs)?;
                ensure(
                    frame.reused && frame.display_changed && frame.evaluated == context.evaluated,
                    "Mouse event advanced physics or failed to invalidate the display",
                )?;
                frame
            } else {
                context
            };
            let value = frame.clock.as_ref().unwrap();
            ensure(
                value.text == expected
                    && (value.result - result).abs() < 1e-9
                    && value.events == events
                    && gauges.pid() == button_pid,
                "Retained flight stopwatch state/input mismatch",
            )?;
            gauges.apply(
                &frame,
                &current.model,
                &mut current.appearance,
                Some(&mut gpu),
            )?;
            let path = output.join(format!("c172-clock-button-{i}.ppm"));
            gpu.snapshot(&orbit, &path)?;
            let pixels = std::fs::read(path).map_err(|e| e.to_string())?;
            if i == 0 {
                ensure(
                    pixels != button_baseline,
                    "Native mode button did not change live GPU texture",
                )?;
            }
            if [1, 3, 5, 6, 8, 10].contains(&i) {
                ensure(
                    previous_pixels.as_ref() == Some(&pixels),
                    "Pause/start/stop changed identical display pixels",
                )?;
            }
            if [2, 4, 7, 9].contains(&i) {
                ensure(
                    previous_pixels.as_ref() != Some(&pixels),
                    "Stopwatch state change did not reach live GPU texture",
                )?;
            }
            previous_pixels = Some(pixels);
            button_reports.push(serde_json::json!({"time":time,"child":child,"text":value.text,"callback_result":value.result,"events":events,"evaluated":frame.evaluated,"physics_reused":frame.reused,"display_changed":frame.display_changed}));
        }
        gauges.restart()?;
        let reset_pid = gauges.pid();
        let reset = gauges.update_inputs(0.5, inputs)?;
        ensure(
            reset_pid != button_pid
                && reset.clock.as_ref().unwrap().events == 0
                && reset.clock.as_ref().unwrap().text == zero_reset.clock.as_ref().unwrap().text,
            "Flight reset did not discard native button state",
        )?;
        gauges.apply(
            &reset,
            &current.model,
            &mut current.appearance,
            Some(&mut gpu),
        )?;
        let path = output.join("c172-clock-button-reset.ppm");
        gpu.snapshot(&orbit, &path)?;
        ensure(
            std::fs::read(path).map_err(|e| e.to_string())? == clock_pixels[0],
            "Native button reset failed to restore original configured-clock pixels",
        )?;
    }
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
    if let Some(clock) = local_clock {
        ground_gauges = ground_gauges.with_day_native_clock(&root, clock)?;
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
        "flight_driven_sprites":3,"native_local_clock":local_clock.is_some(),"clock_boundaries":clock_comparisons,"clock_events":clock_event_reports,"native_clock_buttons":button_reports,
        "clock_style":"authored daylight with explicit selection events","gdi_pixel_parity_verified":false,
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
