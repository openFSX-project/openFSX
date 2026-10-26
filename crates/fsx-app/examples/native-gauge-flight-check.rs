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
// Independently locate authored button centers in the model's UV triangles,
// then use the production screen picker to check the entire inverse path.
fn clock_screen_points(
    root: &std::path::Path,
    panel_path: &std::path::Path,
    model: &fsx_model::Model,
    geometry: &fsx_model::Geometry,
    appearance: &fsx_render::Appearance,
    gauges: &flight_gauges::FlightGauges,
    orbit: &Orbit,
    transform: fsx_geometry::Mat4,
    viewport: [u32; 2],
) -> Result<Vec<[f64; 2]>, String> {
    use fsx_vehicle::{
        gauge::GaugeLibrary,
        panel::{Kind, PanelConfig},
    };
    let config = PanelConfig::load(panel_path, true)?;
    let library = GaugeLibrary::load(&root.join("Gauges/Cessna172.dll"), true)?;
    let mut placements = Vec::new();
    for p in &config.panels {
        if let Kind::VirtualCockpit { texture } = &p.kind {
            for g in &p.gauges {
                if g.module.eq_ignore_ascii_case("Cessna172")
                    && g.name.eq_ignore_ascii_case("Clock")
                {
                    placements.push((p, g, texture));
                }
            }
        }
    }
    ensure(
        placements.len() == 1,
        "Expected one clock placement for screen picking",
    )?;
    let (p, g, texture) = placements[0];
    let [Some(mw), Some(mh)] = p.size_mm.ok_or("No panel mm extent")? else {
        return Err("Incomplete mm extent".into());
    };
    let [Some(gw), Some(gh)] = g.size else {
        return Err("No clock extent".into());
    };
    let [Some(pw), Some(ph)] = p.pixel_size.ok_or("No panel pixel extent")? else {
        return Err("Incomplete pixel extent".into());
    };
    let origin = [g.position[0] / mw * pw, g.position[1] / mh * ph];
    let size = [gw / mw * pw, gh / mh * ph];
    let native = fsx_render::panel::native_mouse_point(
        p,
        g,
        &library,
        1033,
        [origin[0] + size[0] / 2., origin[1] + size[1] / 2.],
    )?
    .ok_or("No native clock mapping")?;
    let regions = library
        .mouse_rects("Clock")?
        .ok_or("No native mouse list")?
        .regions(native.native_size)?;
    ensure(regions.len() == 3, "Expected three clock regions")?;
    let mut result = Vec::new();
    let native_points = regions
        .iter()
        .map(|region| {
            let r = region.rectangle;
            [r[0] + r[2] * 0.5, r[1] + r[3] * 0.5]
        })
        .chain([[27., 29.]]);
    for (child, xy) in native_points.enumerate() {
        let uv = [
            (origin[0] + xy[0] / f64::from(native.native_size[0]) * size[0]) / pw,
            (origin[1] + xy[1] / f64::from(native.native_size[1]) * size[1]) / ph,
        ];
        let mut selected = None;
        let mut diagnostics = Vec::new();
        for batch in geometry.batches.iter().filter(|b| {
            b.visible
                && model.materials[b.material]
                    .diffuse_texture
                    .is_some_and(|i| model.textures[i].eq_ignore_ascii_case(texture))
        }) {
            for tri in geometry.vertices[batch.range.start as usize..batch.range.end as usize]
                .chunks_exact(3)
            {
                let q = std::array::from_fn::<_, 3, _>(|i| {
                    [f64::from(tri[i][6]), f64::from(tri[i][7])]
                });
                let a = [q[1][0] - q[0][0], q[1][1] - q[0][1]];
                let b = [q[2][0] - q[0][0], q[2][1] - q[0][1]];
                let d = [uv[0] - q[0][0], uv[1] - q[0][1]];
                let det = a[0] * b[1] - a[1] * b[0];
                if det.abs() < 1e-12 {
                    continue;
                }
                let u = (d[0] * b[1] - d[1] * b[0]) / det;
                let v = (a[0] * d[1] - a[1] * d[0]) / det;
                if u < 0. || v < 0. || u + v > 1. {
                    continue;
                }
                let point = std::array::from_fn(|j| {
                    (1. - u - v) * f64::from(tri[0][j])
                        + u * f64::from(tri[1][j])
                        + v * f64::from(tri[2][j])
                });
                let Some(screen) = fsx_render::picking::project(orbit, transform, viewport, point)?
                else {
                    continue;
                };
                let target = gauges.clock_target(
                    model, geometry, appearance, orbit, transform, viewport, screen,
                )?;
                if let Some(hit) = target {
                    if hit.child == child
                        && (hit.surface.uv[0] - uv[0]).abs() < 1e-5
                        && (hit.surface.uv[1] - uv[1]).abs() < 1e-5
                    {
                        selected = Some(screen);
                        break;
                    }
                } else {
                    let front = fsx_render::picking::surface(
                        geometry, appearance, orbit, transform, viewport, screen,
                    )?;
                    if child == 3
                        && front.as_ref().is_some_and(|hit| {
                            hit.texture
                                .is_some_and(|i| model.textures[i].eq_ignore_ascii_case(texture))
                                && (hit.uv[0] - uv[0]).abs() < 1e-5
                                && (hit.uv[1] - uv[1]).abs() < 1e-5
                        })
                    {
                        selected = Some(screen);
                        break;
                    }
                    diagnostics.push(format!("screen={screen:?} surface={:?}", front));
                }
            }
            if selected.is_some() {
                break;
            }
        }
        result.push(selected.ok_or_else(|| {
            format!("Clock child {child} has no visible screen hit: {diagnostics:?}")
        })?);
    }
    Ok(result)
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
    let mut picking_reports = Vec::new();
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
        let inputs = [75., 500., 256., 0., 0., 90., 0., 2400.];
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
        let screen_points = clock_screen_points(
            &root,
            &aircraft.panel,
            &current.model,
            &current.geometry,
            &current.appearance,
            &gauges,
            &orbit,
            gpu.model_matrix,
            [960, 720],
        )?;
        for size in [[1920, 1440], [1280, 720]] {
            let points = clock_screen_points(
                &root,
                &aircraft.panel,
                &current.model,
                &current.geometry,
                &current.appearance,
                &gauges,
                &orbit,
                gpu.model_matrix,
                size,
            )?;
            if size == [1920, 1440] {
                ensure(
                    points.iter().zip(&screen_points).all(|(a, b)| {
                        (a[0] - b[0] * 2.).abs() < 1e-5 && (a[1] - b[1] * 2.).abs() < 1e-5
                    }),
                    "Physical viewport scaling changed mouse mapping",
                )?;
            }
            picking_reports.push(serde_json::json!({"viewport":size,"screen_points":points,"targets":3,"display_nonactionable":true}));
        }
        ensure(
            gauges
                .click_clock_at(
                    &current.model,
                    &current.geometry,
                    &current.appearance,
                    &orbit,
                    gpu.model_matrix,
                    [960, 720],
                    screen_points[3],
                    0.5,
                )?
                .is_none(),
            "Clock display/help region invoked a mouse action",
        )?;
        let visible_hit = gauges
            .clock_target(
                &current.model,
                &current.geometry,
                &current.appearance,
                &orbit,
                gpu.model_matrix,
                [960, 720],
                screen_points[0],
            )?
            .ok_or("No visible clock hit")?;
        current.geometry.batches[visible_hit.surface.batch].visible = false;
        let hidden = gauges.clock_target(
            &current.model,
            &current.geometry,
            &current.appearance,
            &orbit,
            gpu.model_matrix,
            [960, 720],
            screen_points[0],
        );
        current.geometry.batches[visible_hit.surface.batch].visible = true;
        ensure(
            hidden?.is_none(),
            "Hidden cockpit batch still received input",
        )?;
        let mut rolled = desktop_flight::FlightSession::new(aircraft.parameters.clone())?;
        rolled.simulation.state.position = [1200., 400., -350.];
        rolled.simulation.state.orientation =
            fsx_geometry::Quaternion([0.2_f64.cos(), 0.2_f64.sin(), 0., 0.]);
        let mut rolled_orbit = Orbit::new(&current.geometry);
        flight_view::camera(
            &rolled,
            &mut rolled_orbit,
            Some(eye),
            flight_view::Look {
                pitch: -0.3,
                ..Default::default()
            },
        )?;
        let rolled_points = clock_screen_points(
            &root,
            &aircraft.panel,
            &current.model,
            &current.geometry,
            &current.appearance,
            &gauges,
            &rolled_orbit,
            rolled.model_matrix(),
            [960, 720],
        )?;
        picking_reports.push(serde_json::json!({"viewport":[960,720],"screen_points":rolled_points,"rolled_translated_camera":true,"targets":3}));
        let transform = gpu.model_matrix;
        gpu.model_matrix = rolled.model_matrix();
        gpu.snapshot(&rolled_orbit, &output.join("c172-clock-picking-rolled.ppm"))?;
        gpu.model_matrix = transform;
        ensure(
            gauges
                .clock_target(
                    &current.model,
                    &current.geometry,
                    &current.appearance,
                    &orbit,
                    gpu.model_matrix,
                    [960, 720],
                    [-1., 0.],
                )?
                .is_none(),
            "Outside viewport produced a clock hit",
        )?;
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
            let mut picked = None;
            let frame = if let Some(child) = child {
                let hit = gauges
                    .click_clock_at(
                        &current.model,
                        &current.geometry,
                        &current.appearance,
                        &orbit,
                        gpu.model_matrix,
                        [960, 720],
                        screen_points[child],
                        time,
                    )?
                    .ok_or("Clock screen input missed authored child")?;
                ensure(
                    hit.child == child && hit.child_local.iter().all(|&v| (0..6).contains(&v)),
                    "Screen mouse mapped to the wrong child/local point",
                )?;
                picked = Some(
                    serde_json::json!({"screen":screen_points[child],"mouse_rva":hit.mouse_rva,"child_local":hit.child_local,"texture":hit.texture,"triangle_vertex":hit.surface.triangle_vertex}),
                );
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
            button_reports.push(serde_json::json!({"time":time,"child":child,"picked":picked,"text":value.text,"callback_result":value.result,"events":events,"evaluated":frame.evaluated,"physics_reused":frame.reused,"display_changed":frame.display_changed}));
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
        "flight_driven_sprites":3,"native_local_clock":local_clock.is_some(),"clock_boundaries":clock_comparisons,"clock_events":clock_event_reports,"native_clock_buttons":button_reports,"clock_picking":picking_reports,
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
