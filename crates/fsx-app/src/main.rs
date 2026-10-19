//! Native application and event loop; rendering lives in fsx-render.
use desktop_flight::FlightSession;
use fsx_model::Model;
use fsx_render::{block_on, Appearance, Gpu, Orbit};
use fsx_vehicle::aircraft::Aircraft;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::Window,
};
mod desktop_flight;
mod flight_gauges;
mod flight_pose;
mod flight_view;
mod native_gauges;
mod reference_pose;
struct App {
    visual: flight_view::Visual,
    animation: Option<usize>,
    frame: f32,
    orbit: Orbit,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    dragging: bool,
    cursor: Option<(f64, f64)>,
    error: Option<String>,
    flight: Option<FlightSession>,
    alternate: Option<flight_view::Visual>,
    cockpit: bool,
    eye_point: Option<[f64; 3]>,
    look: flight_view::Look,
    last_frame: Instant,
    last_title: Instant,
    visible: bool,
    debug_hud: bool,
    reference_label: Option<String>,
    visibility_nodes: Option<Vec<bool>>,
    gauges: Option<flight_gauges::FlightGauges>,
}
impl App {
    fn select_view(&mut self, cockpit: bool) -> Result<(), String> {
        let Some(flight) = &self.flight else {
            return Ok(());
        };
        if cockpit && self.eye_point.is_none() {
            return Err("No configured cockpit eye point".into());
        }
        if cockpit != self.cockpit {
            let alternate = self
                .alternate
                .as_mut()
                .ok_or("No available alternate visual model")?;
            self.visual.exchange(alternate, self.gpu.as_mut())?;
            self.cockpit = cockpit;
            self.orbit = Orbit::new(&self.visual.geometry);
        }
        self.look = flight_view::Look::default();
        flight_view::camera(
            flight,
            &mut self.orbit,
            if self.cockpit { self.eye_point } else { None },
            self.look,
        )?;
        self.last_frame = Instant::now();
        println!(
            "Flight view: {}",
            if self.cockpit {
                "virtual cockpit (dynamic gauges unavailable)"
            } else {
                "exterior"
            }
        );
        Ok(())
    }
    fn scrub(&mut self, delta: f32) -> Result<(), String> {
        let Some(index) = self.animation else {
            return Ok(());
        };
        self.frame = (self.frame + delta).clamp(0., self.visual.model.animations[index].length);
        let mut frames = vec![None; self.visual.model.animations.len()];
        frames[index] = Some(self.frame);
        let mut geometry = self
            .visual
            .model
            .posed_geometry(Some(self.visual.geometry.lod), &frames)?;
        if let Some(nodes) = &self.visibility_nodes {
            geometry.apply_visibility(nodes)?;
        }
        if let Some(gpu) = &mut self.gpu {
            gpu.update_geometry(&geometry)?;
        }
        self.visual.geometry = geometry;
        if let Some(w) = &self.window {
            w.set_title(&format!(
                "openFSX Animation {index} | frame {:.2} / {:.2} | [ / ] scrub | Esc close",
                self.frame, self.visual.model.animations[index].length
            ));
        }
        Ok(())
    }
    fn advance_flight(&mut self) -> Result<(), String> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_frame).as_secs_f64();
        self.last_frame = now;
        if let Some(flight) = &mut self.flight {
            flight.advance(elapsed)?;
            if let Some(alternate) = &mut self.alternate {
                alternate.update(flight)?;
            }
            if let Some(geometry) = self.visual.pose.update(
                &self.visual.model,
                self.visual.geometry.lod,
                &flight.simulation,
            )? {
                if let Some(gpu) = &mut self.gpu {
                    gpu.update_geometry(&geometry)?;
                }
                self.visual.geometry = geometry;
            }
            if let Some(gauges) = &mut self.gauges {
                let frame = gauges.update(&flight.simulation)?;
                if !frame.reused {
                    if self.cockpit {
                        gauges.apply(
                            &frame,
                            &self.visual.model,
                            &mut self.visual.appearance,
                            self.gpu.as_mut(),
                        )?;
                    } else if let Some(interior) = &mut self.alternate {
                        gauges.apply(&frame, &interior.model, &mut interior.appearance, None)?;
                    }
                }
            }
            flight_view::camera(
                flight,
                &mut self.orbit,
                if self.cockpit { self.eye_point } else { None },
                self.look,
            )?;
            if let Some(gpu) = &mut self.gpu {
                gpu.model_matrix = flight.model_matrix();
                gpu.hud = if self.debug_hud {
                    Some(fsx_render::hud::Readout::new(
                        flight.simulation.telemetry()?,
                        flight.simulation.controls,
                        flight.paused,
                    )?)
                } else {
                    None
                };
            }
            if now.duration_since(self.last_title) >= Duration::from_millis(250) {
                if let Some(window) = &self.window {
                    window.set_title(&flight.title()?);
                }
                self.last_title = now;
            }
        }
        Ok(())
    }
}
impl ApplicationHandler for App {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.visible && self.flight.as_ref().is_some_and(|f| !f.paused) {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(16),
            ));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let result = (|| {
            let title = self.reference_label.clone().unwrap_or_else(|| {
                format!(
                    "openFSX • MDLX LOD {} • Drag: orbit | Wheel: zoom | R: reset | Esc: close",
                    self.visual.geometry.lod
                )
            });
            let window = Arc::new(
                event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_title(title)
                            .with_inner_size(winit::dpi::LogicalSize::new(1100., 760.)),
                    )
                    .map_err(|e| e.to_string())?,
            );
            let size = window.inner_size();
            self.gpu = Some(block_on(Gpu::new(
                Some(window.clone()),
                &self.visual.geometry,
                &self.visual.appearance,
                self.flight.is_some(),
                size.width.max(1),
                size.height.max(1),
            ))?);
            window.request_redraw();
            self.window = Some(window);
            self.scrub(0.)?;
            self.last_frame = Instant::now();
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            self.error = Some(error);
            event_loop.exit();
        }
    }
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        if let WindowEvent::KeyboardInput { event, .. } = &event {
            if self.flight.is_some() && event.state == ElementState::Pressed && !event.repeat {
                let desired = match event.physical_key {
                    PhysicalKey::Code(KeyCode::F9) => Some(true),
                    PhysicalKey::Code(KeyCode::F11) => Some(false),
                    _ => None,
                };
                if let Some(cockpit) = desired {
                    if let Err(error) = self.select_view(cockpit) {
                        eprintln!("View unavailable: {error}");
                    }
                }
            }
            if let (Some(flight), Some(key)) = (
                &mut self.flight,
                desktop_flight::flight_key(event.physical_key, &event.logical_key),
            ) {
                let pressed = event.state == ElementState::Pressed;
                flight.key(key, pressed);
                if pressed && !event.repeat {
                    match key {
                        KeyCode::Escape => event_loop.exit(),
                        KeyCode::KeyP => {
                            flight.paused = !flight.paused;
                            flight.clear_keys();
                            self.last_frame = Instant::now();
                        }
                        KeyCode::KeyR => {
                            if let Err(error) = flight.reset() {
                                self.error = Some(error);
                                event_loop.exit();
                            }
                            self.last_frame = Instant::now();
                        }
                        _ => {}
                    }
                    self.last_title = Instant::now() - Duration::from_secs(1);
                }
            }
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.visible = size.width > 0 && size.height > 0;
                if !self.visible {
                    if let Some(flight) = &mut self.flight {
                        flight.paused = true;
                        flight.clear_keys();
                    }
                }
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => self.dragging = state == ElementState::Pressed && self.flight.is_none(),
            WindowEvent::Focused(false) => {
                self.dragging = false;
                self.cursor = None;
                if let Some(flight) = &mut self.flight {
                    flight.paused = true;
                    flight.clear_keys();
                    self.last_title = Instant::now() - Duration::from_secs(1);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some((x, y)) = self.cursor {
                    if self.dragging {
                        if self.cockpit {
                            self.look.drag(position.x - x, position.y - y);
                        } else {
                            self.orbit.yaw += (position.x - x) as f32 * 0.008;
                            self.orbit.pitch = (self.orbit.pitch + (position.y - y) as f32 * 0.008)
                                .clamp(-1.45, 1.45);
                        }
                    }
                }
                self.cursor = Some((position.x, position.y));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let amount = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.,
                };
                if self.cockpit {
                    self.look.zoom(amount);
                } else {
                    self.orbit.zoom(amount);
                }
            }
            WindowEvent::KeyboardInput { ref event, .. }
                if event.state == ElementState::Pressed && self.flight.is_none() =>
            {
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::BracketLeft | KeyCode::BracketRight) => {
                        let delta = if event.physical_key == PhysicalKey::Code(KeyCode::BracketLeft)
                        {
                            -5.
                        } else {
                            5.
                        };
                        if let Err(error) = self.scrub(delta) {
                            self.error = Some(error);
                            event_loop.exit();
                        }
                    }
                    PhysicalKey::Code(KeyCode::Escape) => event_loop.exit(),
                    PhysicalKey::Code(KeyCode::KeyR) => {
                        self.orbit = Orbit::new(&self.visual.geometry)
                    }
                    PhysicalKey::Code(KeyCode::ArrowLeft) => self.orbit.yaw -= 0.12,
                    PhysicalKey::Code(KeyCode::ArrowRight) => self.orbit.yaw += 0.12,
                    PhysicalKey::Code(KeyCode::ArrowUp) => {
                        self.orbit.pitch = (self.orbit.pitch + 0.1).min(1.45)
                    }
                    PhysicalKey::Code(KeyCode::ArrowDown) => {
                        self.orbit.pitch = (self.orbit.pitch - 0.1).max(-1.45)
                    }
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.advance_flight() {
                    self.error = Some(error);
                    event_loop.exit();
                    return;
                }
                if let Some(gpu) = &mut self.gpu {
                    if self.visible {
                        if let Err(error) = gpu.draw(&self.orbit) {
                            self.error = Some(error);
                            event_loop.exit();
                        }
                    }
                }
            }
            _ => {}
        }
        // Event-driven rendering: no busy redraw loop while the view is idle.
        if let Some(window) = &self.window {
            if !matches!(event, WindowEvent::RedrawRequested) {
                window.request_redraw();
            }
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let path = PathBuf::from(
        args.next()
            .ok_or("Usage: openfsx-viewer <model.mdl> [options]\nFlight: openfsx-viewer <aircraft folder> --fly [--variant N] [--windows-1252]\nOptions: --lod N --texture-dir folder --no-textures --snapshot output.ppm --seconds N")?,
    );
    if path == Path::new("--help") || path == Path::new("-h") {
        println!("openfsx-viewer <model.mdl> [--lod N] [--texture-dir folder | --no-textures]\nopenfsx-viewer <aircraft folder> --fly [--ground] [--variant N] [--windows-1252] [--lod N]\nWindows menu: openfsx-viewer <FSX installation> --menu\nAnimation inspector: --animation N [--frame N] (use inspect-model for IDs; [ / ] scrub)\nRecorded pose: --reference-pose sample.ndjson [--reference-sample N] (inspection only)\nOrbit camera: --view front|rear --zoom N (0.1..10)\nOptional: --debug-hud (experimental telemetry); --snapshot output.ppm [--seconds N, flight only, 0..300]\nFlight keys: Down/Up pull/push; Left/Right bank; Q/E rudder; PageUp/PageDown throttle; F1 idle; F4 full power; B parking brake; Space wheel brakes; Home/End trim; P pause; R reset; Esc close.");
        println!("Visibility inspector: repeat --visibility-var NAME#units#value (explicit values; no flight override)");
        println!("Pose inspector: repeat --pose-var NAME#units#value (XAPS/read-only XAPC; no flight override)");
        println!("Flight views: --cockpit starts inside; F9 virtual cockpit / recenter, F11 exterior; cockpit drag looks around, wheel changes FOV.");
        println!("Experimental Windows C172 instruments: --native-gauges [--gauge-host path/to/fsx-gauge-host.exe]. Five needles plus native attitude/heading sprites, ideal ISA air data and ideal gyros; no RPM, full panel services or needle lag.");
        return Ok(());
    }
    let (mut lod, mut snapshot) = (None, None);
    let (mut texture_dir, mut no_textures) = (None, false);
    let (mut fly, mut variant, mut windows_1252, mut seconds) = (false, 0u32, false, 0f64);
    let mut ground = false;
    let mut cockpit = false;
    let mut native_gauges_enabled = false;
    let mut gauge_host = None;
    let mut menu = false;
    let mut debug_hud = false;
    let mut animation = None;
    let mut frame = None;
    let mut rear_view = false;
    let mut zoom: f32 = 1.;
    let mut reference_path = None;
    let mut reference_index = None;
    let mut visibility_values: Vec<(String, String, f64)> = Vec::new();
    let mut pose_values: Vec<(String, String, f64)> = Vec::new();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--native-gauges") => native_gauges_enabled = true,
            Some("--gauge-host") => {
                gauge_host = Some(PathBuf::from(
                    args.next().ok_or("Expected native gauge helper path")?,
                ))
            }
            Some("--visibility-var" | "--pose-var") => {
                let values = if arg == "--pose-var" {
                    &mut pose_values
                } else {
                    &mut visibility_values
                };
                if values.len() >= 128 {
                    return Err("At most 128 variables per inspection option are accepted".into());
                }
                let text = args
                    .next()
                    .and_then(|s| s.to_str().map(str::to_owned))
                    .ok_or("Expected NAME#units#value")?;
                let fields = text.split('#').collect::<Vec<_>>();
                if fields.len() != 3 || fields[0].trim().is_empty() || fields[1].trim().is_empty() {
                    return Err("Inspection variable expects NAME#units#value".into());
                }
                let value: f64 = fields[2]
                    .trim()
                    .parse()
                    .map_err(|_| "Invalid inspection number")?;
                if !value.is_finite() || (arg == "--pose-var" && value.abs() > f32::MAX as f64) {
                    return Err(
                        "Inspection values must be finite and within the supported range".into(),
                    );
                }
                let (name, units) = (fields[0].trim().to_owned(), fields[1].trim().to_owned());
                if values.iter().any(|(n, u, _)| {
                    n.eq_ignore_ascii_case(&name) && u.eq_ignore_ascii_case(&units)
                }) {
                    return Err("Duplicate inspection variable and units".into());
                }
                values.push((name, units, value));
            }
            Some("--reference-pose") => {
                reference_path = Some(PathBuf::from(
                    args.next().ok_or("Expected reference NDJSON path")?,
                ))
            }
            Some("--reference-sample") => {
                reference_index = Some(
                    args.next()
                        .and_then(|s| s.to_str().and_then(|s| s.parse::<usize>().ok()))
                        .ok_or("Expected zero-based reference sample index")?,
                )
            }
            Some("--view") => {
                rear_view = match args
                    .next()
                    .and_then(|s| s.to_str().map(str::to_owned))
                    .as_deref()
                {
                    Some("front") => false,
                    Some("rear") => true,
                    _ => return Err("--view expects front or rear".into()),
                }
            }
            Some("--zoom") => {
                zoom = args
                    .next()
                    .and_then(|s| s.to_str().and_then(|s| s.parse().ok()))
                    .ok_or("Expected zoom factor")?;
                if !zoom.is_finite() || !(0.1..=10.).contains(&zoom) {
                    return Err("Zoom must be finite and within 0.1..10".into());
                }
            }
            Some("--lod") => {
                lod = Some(
                    args.next()
                        .and_then(|s| s.to_str().and_then(|s| s.parse().ok()))
                        .ok_or("Expected an integer LOD")?,
                )
            }
            Some("--snapshot") => {
                snapshot = Some(PathBuf::from(args.next().ok_or("Expected snapshot path")?))
            }
            Some("--texture-dir") => {
                texture_dir = Some(PathBuf::from(args.next().ok_or("Expected texture folder")?))
            }
            Some("--no-textures") => no_textures = true,
            Some("--fly") => fly = true,
            Some("--ground") => ground = true,
            Some("--cockpit") => cockpit = true,
            Some("--menu") => menu = true,
            Some("--debug-hud") => debug_hud = true,
            Some("--animation") => {
                animation = Some(
                    args.next()
                        .and_then(|s| s.to_str().and_then(|s| s.parse::<usize>().ok()))
                        .ok_or("Expected animation index")?,
                )
            }
            Some("--frame") => {
                frame = Some(
                    args.next()
                        .and_then(|s| s.to_str().and_then(|s| s.parse::<f32>().ok()))
                        .ok_or("Expected animation frame")?,
                )
            }
            Some("--windows-1252") => windows_1252 = true,
            Some("--variant") => {
                variant = args
                    .next()
                    .and_then(|s| s.to_str().and_then(|s| s.parse().ok()))
                    .ok_or("Expected a numeric aircraft variant")?
            }
            Some("--seconds") => {
                seconds = args
                    .next()
                    .and_then(|s| s.to_str().and_then(|s| s.parse().ok()))
                    .ok_or("Expected snapshot flight duration")?
            }
            _ => return Err("Unknown viewer argument".into()),
        }
    }
    if !seconds.is_finite()
        || !(0. ..=300.).contains(&seconds)
        || (seconds > 0. && (!fly || snapshot.is_none()))
    {
        return Err("--seconds requires --fly and --snapshot, between 0 and 300 seconds".into());
    }
    if menu {
        if fly
            || native_gauges_enabled
            || gauge_host.is_some()
            || cockpit
            || ground
            || snapshot.is_some()
            || debug_hud
            || lod.is_some()
            || variant != 0
            || no_textures
            || texture_dir.is_some()
            || animation.is_some()
            || frame.is_some()
            || rear_view
            || zoom != 1.
            || reference_path.is_some()
            || reference_index.is_some()
            || !visibility_values.is_empty()
            || !pose_values.is_empty()
        {
            return Err("--menu takes an FSX installation path and cannot be combined with renderer options".into());
        }
        #[cfg(target_os = "windows")]
        {
            return fsx_ui::run(&path);
        }
        #[cfg(not(target_os = "windows"))]
        {
            return Err("The FSX menu host currently requires Windows".into());
        }
    }
    if (native_gauges_enabled && (!fly || !cfg!(windows) || no_textures))
        || (gauge_host.is_some() && !native_gauges_enabled)
    {
        return Err("--native-gauges requires Windows --fly with textures; --gauge-host requires --native-gauges".into());
    }
    let aircraft = if fly {
        Some(Aircraft::load(&path, variant, windows_1252)?)
    } else {
        None
    };
    if cockpit && !fly {
        return Err("--cockpit requires --fly".into());
    }
    let eye_point = aircraft.as_ref().and_then(|a| a.eye_point);
    if cockpit && eye_point.is_none() {
        return Err("--cockpit requires a configured [views] eyepoint".into());
    }
    if ground && !fly {
        return Err("--ground requires --fly".into());
    }
    if fly && !visibility_values.is_empty() {
        return Err("--visibility-var is an inspection option, not a flight-state override".into());
    }
    if !pose_values.is_empty()
        && (fly || reference_path.is_some() || animation.is_some() || frame.is_some())
    {
        return Err(
            "--pose-var cannot be combined with flight, recorded poses or manual animation".into(),
        );
    }
    let model_path = if let Some(aircraft) = &aircraft {
        if cockpit {
            aircraft
                .interior
                .as_deref()
                .ok_or("No [models] interior reference")?
        } else {
            aircraft.model.as_path()
        }
    } else {
        &path
    };
    let model = Model::load(model_path)?;
    if reference_index.is_some() && reference_path.is_none() {
        return Err("--reference-sample requires --reference-pose".into());
    }
    if reference_path.is_some() && (fly || animation.is_some() || frame.is_some()) {
        return Err(
            "Reference pose inspection cannot be combined with --fly or manual --animation/--frame"
                .into(),
        );
    }
    let mut reference_label = None;
    let reference_plan = if !pose_values.is_empty() {
        let plan = fsx_model::animation::plan_pose(&model.animations, |name, units| {
            pose_values
                .iter()
                .find(|(n, u, _)| n.eq_ignore_ascii_case(name) && u.eq_ignore_ascii_case(units))
                .map(|(_, _, v)| *v as f32)
        })?;
        let applied = plan.frames.iter().filter(|f| f.is_some()).count();
        if applied == 0 {
            return Err("No supplied variables drive supported stateless animation tracks".into());
        }
        println!("Explicit inspection pose: {applied}/{} animation groups; values are user-supplied, not simulator measurements", plan.frames.len());
        for (i, status) in plan.status.iter().enumerate() {
            println!("  Clip {i}: {status:?}");
        }
        reference_label = Some(format!(
            "openFSX Explicit inspection pose | {applied} groups | Esc close"
        ));
        Some(plan)
    } else if let Some(path) = &reference_path {
        let index = reference_index.unwrap_or(0);
        let sample = reference_pose::Sample::load(path, index)?;
        let plan = fsx_model::animation::plan_pose(&model.animations, |name, units| {
            sample.lookup(name, units)
        })?;
        let applied = plan.frames.iter().filter(|f| f.is_some()).count();
        if applied == 0 {
            return Err("No available reference variables drive this model's supported stateless animation tracks".into());
        }
        println!(
            "Reference pose: declared source {} | sample {} | elapsed {:.3} s | aircraft {:?}",
            sample.source, index, sample.elapsed, sample.title
        );
        println!("Applied {applied}/{} animation groups. Other clips remain in bind pose; unsupported code, visibility and temporal lag are not evaluated.",plan.frames.len());
        for (i, status) in plan.status.iter().enumerate() {
            println!("  Clip {i}: {status:?}");
        }
        reference_label = Some(format!(
            "openFSX Reference pose | {} | sample {index} | {applied} groups | Esc close",
            sample.source
        ));
        Some(plan)
    } else {
        None
    };
    if frame.is_some() && animation.is_none() {
        return Err("--frame requires --animation".into());
    }
    if animation.is_some() && fly {
        return Err("Manual animation inspection cannot be combined with --fly".into());
    }
    if fly && (rear_view || zoom != 1.) {
        return Err("--view and --zoom are orbit-viewer options".into());
    }
    let frame = frame.unwrap_or(0.);
    let mut geometry = if let Some(plan) = &reference_plan {
        model.posed_geometry(lod, &plan.frames)?
    } else if let Some(index) = animation {
        let clip = model
            .animations
            .get(index)
            .ok_or("Animation index outside model catalog; use inspect-model to list animations")?;
        if !frame.is_finite() || frame < 0. || frame > clip.length {
            return Err("Animation frame must be finite and within clip length".into());
        }
        let mut frames = vec![None; model.animations.len()];
        frames[index] = Some(frame);
        model.posed_geometry(lod, &frames)?
    } else {
        model.geometry(lod)?
    };
    let visibility_nodes = if !visibility_values.is_empty() {
        let plan = model.visibility_plan(|name, units| {
            visibility_values
                .iter()
                .find(|(n, u, _)| n.eq_ignore_ascii_case(name) && u.eq_ignore_ascii_case(units))
                .map(|(_, _, v)| *v)
        });
        geometry.apply_visibility(&plan.nodes)?;
        println!("Visibility: {}/{} conditions evaluated; {} hidden draw batches; unknown/unsupported conditions use their encoded defaults",plan.decisions.iter().filter(|d|d.status==fsx_model::visibility::Status::Evaluated).count(),plan.decisions.len(),geometry.batches.iter().filter(|b|!b.visible).count());
        for (index, decision) in plan.decisions.iter().enumerate() {
            println!(
                "  Visibility {index}: {} | {:?}",
                if decision.visible {
                    "visible"
                } else {
                    "hidden"
                },
                decision.status
            );
        }
        Some(plan.nodes)
    } else {
        None
    };
    if no_textures && texture_dir.is_some() {
        return Err("Use either --texture-dir or --no-textures".into());
    }
    let default_texture = model_path
        .parent()
        .and_then(Path::parent)
        .map(|path| path.join("texture"));
    let mut appearance = Appearance::load(
        &model,
        &geometry,
        if no_textures {
            None
        } else {
            texture_dir
                .as_deref()
                .or(aircraft.as_ref().map(|a| a.texture.as_path()))
                .or(default_texture.as_deref())
        },
    )?;
    println!(
        "{} | LOD {} | {} triangles | {} animated scene nodes | {} skinned parts omitted",
        model.name,
        geometry.lod,
        geometry.vertices.len() / 3,
        model.animated_nodes,
        geometry.skipped_skinned_parts
    );
    println!("Preview limitations: diffuse DDS only; approximate alpha blending; no skinning; read-only visibility subset with encoded defaults for unavailable conditions. Keys are evaluated for explicit clips, supported inspection variables or recorded poses.");
    if let Some(index) = animation {
        println!(
            "Manual animation {index}: frame {frame} / {} | use [ / ] to scrub by five frames",
            model.animations[index].length
        );
    }
    let mut orbit = Orbit::new(&model.geometry(lod)?);
    if rear_view {
        orbit.yaw += std::f32::consts::PI;
    }
    orbit.distance = (orbit.distance / zoom).clamp(orbit.radius * 1.1, orbit.radius * 30.);
    let mut alternate = if let Some(a) = &aircraft {
        let other = if cockpit {
            Some(a.model.as_path())
        } else if a.eye_point.is_some() {
            a.interior.as_deref()
        } else {
            None
        };
        other.and_then(|path| {
            match flight_view::Visual::load(
                path,
                if no_textures {
                    None
                } else {
                    texture_dir.as_deref().or(Some(a.texture.as_path()))
                },
            ) {
                Ok(view) => Some(view),
                Err(error) => {
                    eprintln!("Alternate flight view unavailable: {error}");
                    None
                }
            }
        })
    } else {
        None
    };
    let mut gauges = if native_gauges_enabled {
        let aircraft = aircraft
            .as_ref()
            .ok_or("Native gauges require an aircraft")?;
        let canonical = path.canonicalize().map_err(|e| e.to_string())?;
        let root = canonical
            .ancestors()
            .find(|p| p.join("fsx.exe").is_file() && p.join("Gauges").is_dir())
            .ok_or("Native gauges require an aircraft inside the explicit FSX installation")?;
        let helper = if let Some(helper) = gauge_host {
            helper
        } else {
            let executable = std::env::current_exe().map_err(|e| e.to_string())?;
            let folder = executable.parent().ok_or("No executable directory")?;
            let sibling = folder.join("fsx-gauge-host.exe");
            if sibling.is_file() {
                sibling
            } else {
                folder
                    .parent()
                    .ok_or("No build directory")?
                    .join("fsx-gauge-host.exe")
            }
        };
        Some(flight_gauges::FlightGauges::new(
            root,
            &aircraft.panel,
            &helper,
        )?)
    } else {
        None
    };
    let mut flight = aircraft
        .map(|a| {
            if ground {
                FlightSession::on_ground(a.parameters)
            } else {
                FlightSession::new(a.parameters)
            }
        })
        .transpose()?;
    let mut flight_pose = flight_pose::FlightPose::new(&model);
    if let Some(flight) = &mut flight {
        if ground {
            println!("Experimental runway start: fixed tricycle gear, parking brake on. B releases brake; F4 full power; F1 idle; Space wheel brakes.");
        } else {
            println!("Initial airborne prototype: 90 kt, 300 m; no FSX flight-model parity.");
        }
        println!("Controls: Down/Up pull/push | Left/Right bank | Q/E rudder | PageUp/PageDown throttle | Home/End trim | P pause | R reset | Esc close");
        for _ in 0..(seconds / fsx_sim::flight::STEP).round() as usize {
            flight.advance(fsx_sim::flight::STEP)?;
        }
        println!("{}", flight.title()?);
        flight_view::camera(
            flight,
            &mut orbit,
            if cockpit { eye_point } else { None },
            flight_view::Look::default(),
        )?;
        if let Some(posed) = flight_pose.update(&model, geometry.lod, &flight.simulation)? {
            geometry = posed;
        }
        if let Some(alternate) = &mut alternate {
            alternate.update(flight)?;
        }
        if let Some(gauges) = &mut gauges {
            let frame = gauges.update(&flight.simulation)?;
            if cockpit {
                gauges.apply(&frame, &model, &mut appearance, None)?;
            } else {
                let interior = alternate
                    .as_mut()
                    .ok_or("Native gauges need the configured interior model")?;
                gauges.apply(&frame, &interior.model, &mut interior.appearance, None)?;
            }
            println!("Native C172 gauges: helper PID {}, sequence {}, time {}, evaluated {}; five needles and three attitude/heading sprites, ideal ISA/English units and ideal gyros; RPM, needle lag and full panel lifecycle remain unsupported.",gauges.pid(),frame.sequence,frame.time,frame.evaluated);
        }
    }
    if let Some(path) = snapshot {
        let mut gpu = block_on(Gpu::new(
            None,
            &geometry,
            &appearance,
            flight.is_some(),
            960,
            720,
        ))?;
        if let Some(flight) = &flight {
            gpu.model_matrix = flight.model_matrix();
            gpu.hud = if debug_hud {
                Some(fsx_render::hud::Readout::new(
                    flight.simulation.telemetry()?,
                    flight.simulation.controls,
                    flight.paused,
                )?)
            } else {
                None
            };
        }
        gpu.snapshot(&orbit, &path)?;
        if let Some(gauges) = &mut gauges {
            gauges.close()?;
        }
        return Ok(());
    }
    let mut app = App {
        visual: flight_view::Visual {
            model,
            geometry,
            appearance,
            pose: flight_pose,
        },
        animation,
        frame,
        orbit,
        window: None,
        gpu: None,
        dragging: false,
        cursor: None,
        error: None,
        flight,
        alternate,
        cockpit,
        eye_point,
        look: flight_view::Look::default(),
        last_frame: Instant::now(),
        last_title: Instant::now() - Duration::from_secs(1),
        visible: true,
        debug_hud,
        reference_label,
        visibility_nodes,
        gauges,
    };
    EventLoop::new()
        .map_err(|e| e.to_string())?
        .run_app(&mut app)
        .map_err(|e| e.to_string())?;
    if let Some(gauges) = &mut app.gauges {
        gauges.close()?;
    }
    app.error.map_or(Ok(()), Err)
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
