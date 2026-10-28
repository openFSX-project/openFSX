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
mod audio;
mod desktop_flight;
mod flight_gauges;
mod flight_pose;
mod flight_view;
mod gamepad;
mod native_gauges;
mod pavement;
#[allow(dead_code)] // Shared with the bounded offline reference-gauge replay example.
mod reference_pose;
mod scenery_objects;
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
    gamepad: Option<gamepad::Gamepad>,
    audio: Option<audio::Audio>,
    ground: Option<fsx_render::Ground>,
    /// Scenery source for rebuilding ground as the aircraft travels.
    world: Option<World>,
    terrain_center: (f64, f64),
    pending_ground: Option<std::thread::JoinHandle<((f64, f64), fsx_render::Ground)>>,
    /// Previous wheel contact and flap deflection, for sound events.
    sound_state: (bool, f64),
}
impl App {
    fn click_gauge(&mut self) -> Result<bool, String> {
        if !self.cockpit || !self.visible {
            return Ok(false);
        }
        let (Some(flight), Some(gauges), Some(window), Some((x, y))) =
            (&self.flight, &mut self.gauges, &self.window, self.cursor)
        else {
            return Ok(false);
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(false);
        }
        gauges.update(&flight.simulation)?;
        if gauges
            .click_clock_at(
                &self.visual.model,
                &self.visual.geometry,
                &self.visual.appearance,
                &self.orbit,
                flight.model_matrix(),
                [size.width, size.height],
                [x, y],
                flight.simulation.time,
            )?
            .is_none()
        {
            return Ok(false);
        }
        let frame = gauges.update(&flight.simulation)?;
        gauges.apply(
            &frame,
            &self.visual.model,
            &mut self.visual.appearance,
            self.gpu.as_mut(),
        )?;
        Ok(true)
    }
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
                "virtual cockpit"
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
            if let Some(pad) = &mut self.gamepad {
                let (keys, analog) = pad.poll();
                for (key, pressed) in keys {
                    match key {
                        KeyCode::KeyP if pressed => flight.paused = !flight.paused,
                        KeyCode::KeyR if pressed => flight.reset()?,
                        KeyCode::KeyP | KeyCode::KeyR => {}
                        _ => flight.key(key, pressed),
                    }
                }
                flight.analog = analog;
            }
            let sink = flight.simulation.state.velocity[2];
            if let Some(terrain) = self.ground.as_ref().and_then(|g| g.terrain.as_ref()) {
                // Wheels meet the terrain directly under the aircraft, treated
                // as locally level for this frame.
                let [north, east, _] = flight.simulation.state.position;
                flight.simulation.parameters.gear.ground_level =
                    f64::from(terrain.height_at(east as f32, north as f32).unwrap_or(0.));
            }
            match flight.advance(elapsed) {
                Err(e) if e.contains("penetrated") || e.contains("numeric envelope") => {
                    flight.crashed = true;
                    eprintln!("Crash: {e}. Press R to reset");
                }
                other => other?,
            }
            // Stream terrain: rebuild around the aircraft in the background
            // once it is 20 km from the loaded centre, then swap it in.
            if let Some(world) = &self.world {
                let [north, east, _] = flight.simulation.state.position;
                let finished = self
                    .pending_ground
                    .as_ref()
                    .is_some_and(|h| h.is_finished());
                if finished {
                    if let Some(Ok((center, ground))) = self.pending_ground.take().map(|h| h.join())
                    {
                        self.terrain_center = center;
                        if let Some(gpu) = &mut self.gpu {
                            gpu.set_ground(
                                Some(ground.clone()),
                                &self.visual.geometry,
                                &self.visual.appearance,
                            )?;
                        }
                        self.ground = Some(ground);
                    }
                } else if self.pending_ground.is_none()
                    && (north - self.terrain_center.0).hypot(east - self.terrain_center.1) > 20_000.
                {
                    let (world, center) = (world.clone(), (north, east));
                    self.pending_ground =
                        Some(std::thread::spawn(move || (center, world.ground(center))));
                }
            }
            if let Some(audio) = &mut self.audio {
                let t = flight.simulation.telemetry()?;
                let c = flight.simulation.controls;
                let [n, e, _] = flight.simulation.state.velocity;
                let (was_down, flaps) = self.sound_state;
                audio.update(
                    audio::Cue {
                        rpm: flight.simulation.rpm / flight.simulation.parameters.rated_rpm,
                        throttle: c.throttle,
                        airspeed: t.speed,
                        cruise_speed: flight.simulation.parameters.cruise_speed,
                        on_ground: t.ground_contact,
                        ground_speed: n.hypot(e),
                        // The horn leads the lift peak by about 4 degrees, like the
                        // C172 reed warning a few knots above the stall.
                        stalled: t.stalled
                            || flight
                                .simulation
                                .parameters
                                .aero
                                .lift
                                .peak()
                                .is_ok_and(|(alpha, _)| t.alpha > alpha - 4f64.to_radians()),
                        flaps_moving: (c.flaps - flaps).abs() > 1e-9,
                        cockpit: self.cockpit,
                        paused: flight.paused,
                        touchdown: (t.ground_contact && !was_down && flight.simulation.time > 1.)
                            .then_some(sink.max(0.)),
                    },
                    elapsed,
                );
                self.sound_state = (t.ground_contact, c.flaps);
            }
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
                if !frame.reused || frame.display_changed {
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
            if let (Some(gpu), Some(ground)) = (&mut self.gpu, &self.ground) {
                gpu.set_ground(
                    Some(ground.clone()),
                    &self.visual.geometry,
                    &self.visual.appearance,
                )?;
            }
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
                            if let Some(gauges) = &mut self.gauges {
                                if let Err(error) = gauges.restart() {
                                    self.error = Some(error);
                                    event_loop.exit();
                                }
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
            } => {
                self.dragging = state == ElementState::Pressed && self.flight.is_none();
                if state == ElementState::Pressed && self.flight.is_some() {
                    if let Err(error) = self.click_gauge() {
                        self.error = Some(error);
                        event_loop.exit();
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                self.dragging = false;
            }
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

/// Runways of the chosen airport and its neighbours, relative to the takeoff
/// threshold, which becomes the local flight origin.
/// Installed scenery around a fixed flight origin; ground can be rebuilt
/// around any local centre as the aircraft travels.
#[derive(Clone)]
struct World {
    root: PathBuf,
    catalog: Arc<fsx_scenery::catalog::Catalog>,
    /// Model library GUID index and texture file index for placed objects.
    scenery: Arc<LazyIndex>,
    textures: Arc<std::collections::HashMap<String, PathBuf>>,
    origin: (f64, f64),
    /// Mean-sea-level elevation of the origin plane.
    base: f64,
    /// Local time of day in hours (0..24), fixed for the flight.
    hours: f64,
}
impl World {
    /// Runways within 25 km and terrain within +-40 km of a local north/east centre.
    fn ground(&self, center: (f64, f64)) -> fsx_render::Ground {
        use fsx_scenery::catalog::local;
        let started = Instant::now();
        let mut runways = Vec::new();
        let (mut paved, mut taxi_lines) = (Vec::new(), Vec::new());
        let mut fields = Vec::new();
        for a in &self.catalog.airports {
            let (n, e) = local(self.origin, (a.latitude, a.longitude));
            let d = (n - center.0).hypot(e - center.1);
            if d < 60_000. {
                // Every airport in range is flattened to its own field elevation.
                // The flattened field sits 0.2 m below the pavement stack.
                fields.push((n, e, a.altitude - self.base - 0.2));
            }
            if d > 25_000. {
                continue;
            }
            // Water runways (seaplane bases) have no painted surface.
            for r in a.runways.iter().filter(|r| r.surface != 2) {
                let (north, east) = local(self.origin, (r.latitude, r.longitude));
                let name = r.name();
                let mut ends = [[b' '; 3]; 2];
                for (end, text) in ends.iter_mut().zip(name.split('/')) {
                    for (slot, byte) in end.iter_mut().zip(text.bytes()) {
                        *slot = byte;
                    }
                }
                runways.push(fsx_render::GroundRunway {
                    east: east as f32,
                    north: north as f32,
                    heading: r.heading as f32,
                    length: r.length as f32,
                    width: r.width as f32,
                    hard: r.hard(),
                    height: (r.altitude - self.base) as f32,
                    surface: runway_surface(r.surface),
                    ends,
                });
            }
            // Aprons below taxiways below runways, all on the flattened field.
            let field = (a.altitude - self.base) as f32;
            let to_local = |&(lat, lon): &(f64, f64)| {
                let (n, e) = local(self.origin, (lat, lon));
                [e as f32, n as f32]
            };
            for apron in a.aprons.iter().filter(|ap| ap.surface != 2) {
                let outline: Vec<[f32; 2]> = apron.outline.iter().map(to_local).collect();
                for t in pavement::triangulate(&outline) {
                    for i in t {
                        paved.push([outline[i][0], field - 0.14, outline[i][1]]);
                    }
                }
            }
            let points: Vec<[f32; 2]> = a.taxi_points.iter().map(to_local).collect();
            for path in a.taxi_paths.iter().filter(|p| matches!(p.kind, 1 | 3 | 4)) {
                let (Some(&from), Some(&to)) = (points.get(path.start), points.get(path.end))
                else {
                    continue;
                };
                let width = path.width.clamp(6., 60.) as f32;
                pavement::strip(&mut paved, from, to, width, field - 0.1);
                pavement::disc(&mut paved, from, width / 2., field - 0.1);
                pavement::disc(&mut paved, to, width / 2., field - 0.1);
                if path.kind != 3 {
                    pavement::strip(&mut taxi_lines, from, to, 0.3, field - 0.08);
                }
            }
        }
        let airports = started.elapsed();
        let (terrain, classes) = match self.terrain(center, &fields) {
            Ok((t, c)) => (Some(t), c),
            Err(e) => {
                eprintln!("Terrain elevation unavailable, using a flat field: {e}");
                (None, Vec::new())
            }
        };
        let autogen = terrain
            .as_ref()
            .map(|t| self.autogen(t, &classes, center, &fields))
            .unwrap_or_default();
        let trees = terrain
            .as_ref()
            .and_then(|t| self.trees(t, &classes, center, &fields, &autogen));
        let top = terrain
            .as_ref()
            .map_or(0., |t| t.heights.iter().fold(0f32, |a, &h| a.max(h)));
        let sky = self.sky(center, top);
        let surfaces = RUNWAY_TEXTURES
            .iter()
            .map(|name| fsx_render::TextureImage::load(&self.root.join("Texture").join(name)))
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_else(|e| {
                eprintln!("Runway textures unavailable: {e}");
                Vec::new()
            });
        if surfaces.is_empty() {
            runways.iter_mut().for_each(|r| r.surface = None);
        }
        let surroundings = started.elapsed();
        let objects = self.objects(center, terrain.as_ref(), autogen.houses);
        println!(
            "Scenery build: airports {:.1} s, terrain/land class/trees/autogen {:.1} s, objects {:.1} s",
            airports.as_secs_f64(),
            (surroundings - airports).as_secs_f64(),
            (started.elapsed() - surroundings).as_secs_f64()
        );
        let lights = if night_lights(self.hours) {
            runway_lights(&runways)
        } else {
            Vec::new()
        };
        fsx_render::Ground {
            lights,
            tint: day_tint(self.hours),
            runways,
            terrain,
            sky,
            trees,
            surfaces,
            objects,
            pavement: paved,
            taxi_lines,
        }
    }
    /// Library objects within 6 km of a local centre, standing on the terrain
    /// when placed above ground level.
    fn objects(
        &self,
        center: (f64, f64),
        terrain: Option<&fsx_render::Terrain>,
        houses: Vec<fsx_scenery::objects::GenericBuilding>,
    ) -> Option<fsx_render::StaticScene> {
        use fsx_scenery::catalog::{local, offset};
        let (lat, lon) = offset(self.origin.0, self.origin.1, center.0, center.1);
        let (mut placements, buildings) = self.scenery.get().objects_near(lat, lon, 6000.);
        // Nearest first, so the placement budget favours what the pilot sees.
        let distance = |p: &fsx_scenery::objects::Placement| {
            let (n, e) = local((lat, lon), (p.latitude, p.longitude));
            n.hypot(e)
        };
        placements.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
        let position = |latitude: f64, longitude: f64, altitude: f64, agl: bool| {
            let (north, east) = local(self.origin, (latitude, longitude));
            let ground = terrain
                .and_then(|t| t.height_at(east as f32, north as f32))
                .unwrap_or(0.);
            let up = if agl {
                ground + altitude as f32
            } else {
                (altitude - self.base) as f32
            };
            Some([east as f32, up, north as f32])
        };
        let mut scene = scenery_objects::build(
            &placements,
            &self.scenery.get().models,
            &self.textures,
            |p| position(p.latitude, p.longitude, p.altitude, p.agl),
        )
        .unwrap_or_default();
        scenery_objects::add_generic_buildings(&mut scene, &buildings, |b| {
            position(b.latitude, b.longitude, b.altitude, b.agl)
        });
        scenery_objects::add_generic_buildings(&mut scene, &houses, |b| {
            position(b.latitude, b.longitude, b.altitude, b.agl)
        });
        (!scene.batches.is_empty()).then_some(scene)
    }
    /// Trees from the installed sheet within 8 km of the area centre, chosen
    /// and spaced by land class and kept clear of airports. Placement is an
    /// openFSX density model, not FSX autogen annotation.
    fn trees(
        &self,
        terrain: &fsx_render::Terrain,
        classes: &[u8],
        center: (f64, f64),
        fields: &[(f64, f64, f64)],
        autogen: &Autogen,
    ) -> Option<fsx_render::Trees> {
        let quads = terrain.columns - 1;
        if classes.len() != quads * (terrain.rows - 1) {
            return None;
        }
        let sprites =
            fsx_render::TextureImage::load(&self.root.join("Texture/treesSU.bmp")).ok()?;
        // (trees per quad, candidate sheet cells): row 0 deciduous, rows 1-2
        // conifers, row 3 palms.
        let kind = |class: u8| -> (u32, &'static [u8]) {
            match class {
                3 | 4 | 20 | 21 | 22 | 27 | 61 | 62 | 63 => (36, &[4, 5, 6, 7, 8, 9, 10, 11]),
                5 | 23 | 24 | 25 | 26 | 54 | 60 | 78 | 89 => (36, &[0, 1, 2, 3, 4, 5]),
                6 | 28 | 29 | 32 | 33 | 34 | 72 | 90 => (30, &[0, 1, 14, 15]),
                13 | 43 | 48 | 91 => (14, &[0, 1, 2, 3]),
                19 | 55 | 56 | 57 | 58 | 144 | 145 => (10, &[0, 1, 2, 4, 5]),
                30 | 31 | 35 | 38 | 92 | 93 | 109..=120 | 134 => (3, &[0, 1, 2, 3]),
                _ => (0, &[]),
            }
        };
        let mut seed = 0x9E37_79B9_7F4A_7C15u64
            ^ (center.0 as i64 as u64).rotate_left(17)
            ^ center.1 as i64 as u64;
        let mut random = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f32) / (1u64 << 31) as f32
        };
        let mut instances = Vec::new();
        for r in 0..terrain.rows - 1 {
            for c in 0..quads {
                let (count, cells) = kind(classes[r * quads + c]);
                let (qe, qn) = (
                    terrain.west + (c as f32 + 0.5) * terrain.spacing,
                    terrain.north - (r as f32 + 0.5) * terrain.spacing,
                );
                if count == 0
                    || autogen.covered.contains(&(r / 4, c / 4))
                    || (f64::from(qn) - center.0).hypot(f64::from(qe) - center.1) > 8_000.
                    || fields
                        .iter()
                        .any(|&(n, e, _)| (f64::from(qn) - n).hypot(f64::from(qe) - e) < 1_800.)
                {
                    continue;
                }
                for _ in 0..count {
                    let east = qe + (random() - 0.5) * terrain.spacing;
                    let north = qn + (random() - 0.5) * terrain.spacing;
                    let Some(y) = terrain.height_at(east, north) else {
                        continue;
                    };
                    let cell = cells[(random() * cells.len() as f32) as usize % cells.len()];
                    instances.push(([east, y - 0.5, north], 11. + random() * 12., cell));
                }
            }
        }
        instances.extend(autogen.trees.iter().copied());
        Some(fsx_render::Trees { sprites, instances })
    }
    /// FSX noon sky gradient and a fair-weather cumulus layer 500 m above the
    /// highest terrain in the area (at least 1500 m above the origin plane).
    /// The layout is deterministic per area; weather themes are not read yet.
    fn sky(&self, center: (f64, f64), terrain_top: f32) -> Option<fsx_render::Sky> {
        let texture = self.root.join("Texture");
        let load = |name: &str| fsx_render::TextureImage::load(&texture.join(name)).ok();
        // 32-bit BI_RGB sky strips use the explicit BGRA path; the sky is opaque.
        let mut gradient =
            std::fs::read(texture.join(format!("sky_{}_5.bmp", sky_period(self.hours))))
                .ok()
                .and_then(|b| fsx_render::TextureImage::bmp_alpha(&b).ok())?;
        gradient.rgba.chunks_mut(4).for_each(|p| p[3] = 255);
        let sprites = load("cumulus01.bmp")?;
        let base = (terrain_top + 500.).max(1500.);
        let mut seed = (center.0 / 1000.).round() as i64 as u64 * 31
            + (center.1 / 1000.).round() as i64 as u64;
        let mut random = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f32) / (1u64 << 31) as f32
        };
        let cells = [0u8, 1, 10, 11, 12, 13, 14, 15];
        let mut clouds = Vec::new();
        for _ in 0..45 {
            let (east, north) = (
                center.1 as f32 + (random() - 0.5) * 70_000.,
                center.0 as f32 + (random() - 0.5) * 70_000.,
            );
            let puffs = (0..6 + (random() * 9.) as usize)
                .map(|_| {
                    let p = [
                        east + (random() - 0.5) * 1600.,
                        base + random() * 450.,
                        north + (random() - 0.5) * 1600.,
                    ];
                    let cell = cells[(random() * cells.len() as f32) as usize % cells.len()];
                    (p, 250. + random() * 300., cell)
                })
                .collect();
            clouds.push(fsx_render::Cloud {
                center: [east, base + 200., north],
                puffs,
            });
        }
        Some(fsx_render::Sky {
            gradient,
            center: [center.1 as f32, center.0 as f32],
            // Inside the 90 km far plane even after 20 km of travel before re-centring.
            radius: 60_000.,
            sprites,
            clouds,
        })
    }
}
fn airport_scenery(
    aircraft_folder: &Path,
    icao: &str,
    hours: f64,
) -> Result<(fsx_scenery::catalog::Start, fsx_render::Ground, World), String> {
    let canonical = aircraft_folder.canonicalize().map_err(|e| e.to_string())?;
    let root = canonical
        .ancestors()
        .find(|p| p.join("fsx.exe").is_file() && p.join("Scenery").is_dir())
        .ok_or("--airport needs an aircraft inside the FSX installation")?;
    let started = Instant::now();
    let (catalog, failed) = fsx_scenery::catalog::Catalog::load(root)?;
    println!("Airport catalog: {:.1} s", started.elapsed().as_secs_f64());
    if failed > 0 {
        eprintln!("{failed} airport BGL files could not be decoded");
    }
    let airport = catalog
        .find(icao)
        .ok_or_else(|| format!("Airport {icao} not found in installed scenery"))?;
    let start = airport
        .default_start()
        .ok_or_else(|| format!("Airport {icao} has no runways"))?;
    println!(
        "Airport {} {}: runway {} heading {:.0}°, field elevation {:.0} m.",
        airport.icao,
        airport.name.as_deref().unwrap_or(""),
        start.runway,
        start.heading,
        start.altitude,
    );
    let world = World {
        root: root.to_path_buf(),
        origin: (start.latitude, start.longitude),
        base: start.altitude,
        hours,
        catalog: Arc::new(catalog),
        scenery: Arc::new(LazyIndex::spawn(root.to_path_buf())),
        textures: Arc::new(scenery_objects::texture_index(root)),
    };
    let ground = world.ground((0., 0.));
    println!(
        "{} runways within 25 km; {}.",
        ground.runways.len(),
        if ground.terrain.is_some() {
            "installed FSX elevation mesh"
        } else {
            "flat terrain"
        }
    );
    Ok((start, ground, world))
}
/// Autogen houses and trees from the stock annotations of the land textures.
#[derive(Default)]
struct Autogen {
    houses: Vec<fsx_scenery::objects::GenericBuilding>,
    trees: Vec<([f32; 3], f32, u8)>,
    /// Texture tiles (row, column in 4 x 4 quads) whose vegetation came from
    /// annotations, so the random density model leaves them alone.
    covered: std::collections::HashSet<(usize, usize)>,
}
impl World {
    /// Annotated houses and vegetation within 5 km of a local centre. Each
    /// annotation spans one land texture repetition (4 x 4 terrain quads),
    /// with x east and z north in -0.5..0.5. Heights are an openFSX estimate
    /// from footprint; annotation building classes are not decoded.
    fn autogen(
        &self,
        terrain: &fsx_render::Terrain,
        classes: &[u8],
        center: (f64, f64),
        fields: &[(f64, f64, f64)],
    ) -> Autogen {
        use fsx_scenery::{autogen, catalog::offset};
        let mut out = Autogen::default();
        let quads = terrain.columns - 1;
        if classes.len() != quads * (terrain.rows - 1) {
            return out;
        }
        let mut cache: std::collections::HashMap<u16, Option<autogen::Annotation>> =
            std::collections::HashMap::new();
        let size = 4. * terrain.spacing;
        let mut seed = 0x2545_F491_4F6C_DD1Du64
            ^ (center.0 as i64 as u64).rotate_left(23)
            ^ center.1 as i64 as u64;
        let mut random = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f32) / (1u64 << 31) as f32
        };
        for tr in 0..(terrain.rows - 1) / 4 {
            for tc in 0..quads / 4 {
                let (ce, cn) = (
                    terrain.west + (tc as f32 * 4. + 2.) * terrain.spacing,
                    terrain.north - (tr as f32 * 4. + 2.) * terrain.spacing,
                );
                let (dn, de) = (f64::from(cn) - center.0, f64::from(ce) - center.1);
                if dn.hypot(de) > 5_000.
                    || fields
                        .iter()
                        .any(|&(n, e, _)| (f64::from(cn) - n).hypot(f64::from(ce) - e) < 2_000.)
                {
                    continue;
                }
                let class = classes[(tr * 4 + 2) * quads + tc * 4 + 2];
                let texture = land_texture(class);
                let annotation = cache.entry(texture).or_insert_with(|| {
                    let path = "bacdefg".chars().find_map(|region| {
                        self.textures.get(&format!("{texture:03}{region}2an1.agn"))
                    })?;
                    autogen::parse(&std::fs::read(path).ok()?).ok()
                });
                let Some(annotation) = annotation else {
                    continue;
                };
                let local = |p: [f32; 2]| (ce + p[0] * size, cn + p[1] * size);
                for b in &annotation.buildings {
                    let (e, n) = local(b.center);
                    let (lat, lon) =
                        offset(self.origin.0, self.origin.1, f64::from(n), f64::from(e));
                    let (w, d) = (b.half_size[0] * 2. * size, b.half_size[1] * 2. * size);
                    let footprint = (w * d).sqrt();
                    let height = (3.5 + footprint * 0.18).clamp(4., 30.) * (0.8 + 0.4 * random());
                    out.houses.push(fsx_scenery::objects::GenericBuilding {
                        latitude: lat,
                        longitude: lon,
                        altitude: 0.,
                        agl: true,
                        // The building x axis points along `direction`.
                        heading: f64::from((-b.direction[1]).atan2(b.direction[0])).to_degrees(),
                        size: [f64::from(w), f64::from(height), f64::from(d)],
                        pitched_roof: footprint < 22.,
                    });
                }
                if !annotation.vegetation.is_empty() {
                    out.covered.insert((tr, tc));
                }
                let cells: &[u8] = match class {
                    3 | 4 | 20 | 21 | 22 | 27 | 61 | 62 | 63 => &[4, 5, 6, 7, 8, 9, 10, 11],
                    6 | 28 | 29 | 32 | 33 | 34 | 72 | 90 => &[0, 1, 14, 15],
                    _ => &[0, 1, 2, 3, 4, 5],
                };
                for polygon in &annotation.vegetation {
                    // About one tree per 30 x 30 m, at most 120 per polygon.
                    let (min, max) = polygon.iter().fold(([1f32; 2], [-1f32; 2]), |(lo, hi), p| {
                        (
                            [lo[0].min(p[0]), lo[1].min(p[1])],
                            [hi[0].max(p[0]), hi[1].max(p[1])],
                        )
                    });
                    let area = (max[0] - min[0]) * (max[1] - min[1]) * size * size;
                    let wanted = ((area / 900.) as usize).min(120);
                    for _ in 0..wanted * 2 {
                        let p = [
                            min[0] + random() * (max[0] - min[0]),
                            min[1] + random() * (max[1] - min[1]),
                        ];
                        if !autogen::contains(polygon, p) {
                            continue;
                        }
                        let (e, n) = local(p);
                        if let Some(y) = terrain.height_at(e, n) {
                            let cell =
                                cells[(random() * cells.len() as f32) as usize % cells.len()];
                            out.trees
                                .push(([e, y - 0.5, n], 10. + random() * 12., cell));
                        }
                    }
                }
            }
        }
        println!(
            "Autogen: {} houses, {} trees from {} annotations",
            out.houses.len(),
            out.trees.len(),
            cache.values().filter(|a| a.is_some()).count()
        );
        out
    }
}
/// Scenery object/model index built on a background thread at start-up, so
/// it overlaps terrain decoding; the first object build waits for it.
struct LazyIndex {
    index: std::sync::OnceLock<fsx_scenery::objects::SceneryIndex>,
    builder: std::sync::Mutex<Option<std::thread::JoinHandle<fsx_scenery::objects::SceneryIndex>>>,
}
impl LazyIndex {
    fn spawn(root: PathBuf) -> Self {
        Self {
            index: std::sync::OnceLock::new(),
            builder: std::sync::Mutex::new(Some(std::thread::spawn(move || {
                let started = Instant::now();
                let index = fsx_scenery::objects::SceneryIndex::build(&root);
                println!(
                    "Scenery object/model index: {:.1} s",
                    started.elapsed().as_secs_f64()
                );
                index
            }))),
        }
    }
    fn get(&self) -> &fsx_scenery::objects::SceneryIndex {
        self.index.get_or_init(|| {
            let handle = self.builder.lock().ok().and_then(|mut h| h.take());
            handle
                .and_then(|h| h.join().ok())
                .unwrap_or_else(|| fsx_scenery::objects::SceneryIndex::build(Path::new("")))
        })
    }
}
/// FSX sky texture period for a local hour (Texture/sky_<period>_N.bmp).
fn sky_period(hours: f64) -> &'static str {
    match hours.rem_euclid(24.) {
        h if h < 4.5 => "midnight",
        h if h < 5. => "predawn1",
        h if h < 5.5 => "predawn2",
        h if h < 6. => "dawn",
        h if h < 6.5 => "postdawn1",
        h if h < 7. => "postdawn2",
        h if h < 10. => "morning",
        h if h < 14. => "noon",
        h if h < 17. => "afternoon",
        h if h < 17.5 => "presunset1",
        h if h < 18. => "presunset2",
        h if h < 18.5 => "sunset",
        h if h < 19. => "postsunset1",
        h if h < 19.5 => "postsunset2",
        _ => "midnight",
    }
}
/// Light tint on lit surfaces: full daylight, warm low sun, dark blue night.
/// Fixed hours, not computed from date, latitude or the sun position.
fn day_tint(hours: f64) -> [f32; 3] {
    let h = hours.rem_euclid(24.) as f32;
    // Linear values: about 15 % displayed brightness after sRGB encoding.
    let night = [0.018, 0.02, 0.035];
    let warm = [0.95, 0.70, 0.52];
    let day = [1., 1., 1.];
    let mix = |a: [f32; 3], b: [f32; 3], t: f32| {
        std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t.clamp(0., 1.))
    };
    match h {
        h if h < 5. => night,
        h if h < 6.5 => mix(night, warm, (h - 5.) / 1.5),
        h if h < 8. => mix(warm, day, (h - 6.5) / 1.5),
        h if h < 17. => day,
        h if h < 18.5 => mix(day, warm, (h - 17.) / 1.5),
        h if h < 20. => mix(warm, night, (h - 18.5) / 1.5),
        _ => night,
    }
}
fn night_lights(hours: f64) -> bool {
    let h = hours.rem_euclid(24.);
    !(7. ..18.).contains(&h)
}
/// Runway edge lights every 60 m (white), threshold green, far end red,
/// for both directions on hard runways.
fn runway_lights(runways: &[fsx_render::GroundRunway]) -> Vec<([f32; 3], u8)> {
    let mut out = Vec::new();
    for r in runways.iter().filter(|r| r.hard) {
        let h = r.heading.to_radians();
        let (along, across) = ([h.sin(), h.cos()], [h.cos(), -h.sin()]);
        let at = |a: f32, c: f32| {
            [
                r.east + along[0] * a + across[0] * c,
                r.height,
                r.north + along[1] * a + across[1] * c,
            ]
        };
        let (l, w) = (r.length / 2., r.width / 2. + 1.5);
        let mut a = -l;
        while a <= l {
            out.push((at(a, -w), 0));
            out.push((at(a, w), 0));
            a += 60.;
        }
        let mut c = -w;
        while c <= w {
            // Green facing arrivals at both thresholds, red just inside the ends.
            out.push((at(-l - 2., c), 1));
            out.push((at(l + 2., c), 1));
            out.push((at(-l + 2., c), 2));
            out.push((at(l - 2., c), 2));
            c += 4.;
        }
    }
    out
}
/// Installed runway surface textures, indexed by runway_surface().
const RUNWAY_TEXTURES: [&str; 6] = [
    "NewRunwayLayout.DDS",
    "runway12.dds",
    "runway13_su.bmp",
    "runway10_su.bmp",
    "runway15.bmp",
    "runway14.bmp",
];
/// FSX runway surface code to a RUNWAY_TEXTURES index.
fn runway_surface(code: u16) -> Option<u8> {
    Some(match code {
        0 | 16 | 18 | 20 => 0,
        4 | 15 | 17 | 19 | 23 => 1,
        1 | 3 | 5 | 6 | 7 | 11 => 2,
        12 | 13 | 22 => 3,
        8 | 9 | 21 => 4,
        14 => 5,
        _ => return None,
    })
}
const TERRAIN_SIZE: usize = 257;
const TERRAIN_HALF: f64 = 40_000.;
impl World {
    /// Elevation grid of +-40 km around a local centre, decoded by the installed
    /// terrain.dll through the x86 helper, with airport flattening.
    fn terrain(
        &self,
        center: (f64, f64),
        fields: &[(f64, f64, f64)],
    ) -> Result<(fsx_render::Terrain, Vec<u8>), String> {
        use fsx_scenery::{catalog, terrain};
        const SIZE: usize = TERRAIN_SIZE;
        const HALF: f64 = TERRAIN_HALF;
        let root = self.root.as_path();
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let helper = executable
            .ancestors()
            .skip(1)
            .take(3)
            .map(|d| d.join("fsx-terrain-host.exe"))
            .find(|p| p.is_file())
            .ok_or("target/fsx-terrain-host.exe not built (scripts/build-terrain-host.ps1)")?;
        let origin = self.origin;
        let (lat0, lon0) = catalog::offset(origin.0, origin.1, center.0, center.1);
        let records = terrain::index_near(root, lat0, lon0, HALF * 1.5)?;
        let mut elevation =
            terrain::Elevation::new(records, terrain::Decoder::start(&helper, root)?, 12);
        let spacing = 2. * HALF / (SIZE - 1) as f64;
        let (top, left) = (center.0 + HALF, center.1 - HALF);
        let mut heights = Vec::with_capacity(SIZE * SIZE);
        let mut found = 0;
        for r in 0..SIZE {
            for c in 0..SIZE {
                let (north, east) = (top - r as f64 * spacing, left + c as f64 * spacing);
                let (lat, lon) = catalog::offset(origin.0, origin.1, north, east);
                let absolute = elevation.height(lat, lon).map(f64::from);
                found += usize::from(absolute.is_some());
                let relative = absolute.unwrap_or(self.base) - self.base;
                // Blend from the nearest flat field (3 km) to raw terrain (6 km).
                let (d, field) = fields
                    .iter()
                    .map(|&(n, e, h)| ((north - n).hypot(east - e), h))
                    .fold((f64::INFINITY, 0.), |a, b| if b.0 < a.0 { b } else { a });
                let k = ((d - 3000.) / 3000.).clamp(0., 1.);
                heights.push((field + (relative - field) * k * k * (3. - 2. * k)) as f32);
            }
        }
        if found < SIZE * SIZE / 2 {
            return Err("No elevation records cover this area".into());
        }
        // Taper to the origin plane over the outer 6 km so mountains crossing
        // the grid edge slope away instead of ending in a vertical cut.
        for r in 0..SIZE {
            for c in 0..SIZE {
                let edge = r.min(c).min(SIZE - 1 - r).min(SIZE - 1 - c) as f64 * spacing;
                let k = (edge / 6000.).clamp(0., 1.);
                heights[r * SIZE + c] *= (k * k * (3. - 2. * k)) as f32;
            }
        }
        let mut quad_classes = Vec::new();
        let (atlas, quad_slots) = match terrain::land_class_index(root) {
            Ok(index) => {
                let mut classes = terrain::LandClass::new(index);
                let mut quads = Vec::with_capacity((SIZE - 1) * (SIZE - 1));
                for r in 0..SIZE - 1 {
                    for c in 0..SIZE - 1 {
                        let north = top - (r as f64 + 0.5) * spacing;
                        let east = left + (c as f64 + 0.5) * spacing;
                        let (lat, lon) = catalog::offset(origin.0, origin.1, north, east);
                        quads.push(classes.class(elevation.decoder(), lat, lon).unwrap_or(0));
                    }
                }
                let packed = land_atlas(root, &quads);
                quad_classes = quads;
                packed
            }
            Err(e) => {
                eprintln!("Land class unavailable: {e}");
                (None, Vec::new())
            }
        };
        Ok((
            fsx_render::Terrain {
                atlas,
                atlas_slots: ATLAS_SLOTS,
                quad_slots,
                texture_quads: 4,
                columns: SIZE,
                rows: SIZE,
                west: left as f32,
                north: top as f32,
                spacing: spacing as f32,
                heights,
                base: self.base as f32,
            },
            quad_classes,
        ))
    }
}
const ATLAS_SLOTS: u32 = 8;
const SLOT_PIXELS: u32 = 256;
/// Stock texture number for a land class. Natural Olson classes use their own
/// number; urban, suburban and water classes use approximations until the
/// lclookup.bgl remapping is decoded.
fn land_texture(class: u8) -> u16 {
    match class {
        0 | 100 | 123..=126 => 123,
        // Texture 001 is snow, so urban grids use the grey crops-and-town set.
        1 | 101..=108 | 121 => 31,
        109..=120 => 30,
        139 | 140 => 139,
        c => u16::from(c),
    }
}
/// Summer texture `NNN?2su1.bmp`, preferring region b, box-filtered to a slot.
fn land_image(root: &Path, texture: u16) -> Option<fsx_render::TextureImage> {
    let folder = root.join("Scenery/World/texture");
    let path = "bacdefg".chars().find_map(|region| {
        let p = folder.join(format!("{texture:03}{region}2su1.bmp"));
        p.is_file().then_some(p)
    })?;
    let image = fsx_render::TextureImage::load(&path).ok()?;
    let step = (image.width / SLOT_PIXELS).max(1);
    let mut rgba = Vec::with_capacity((SLOT_PIXELS * SLOT_PIXELS * 4) as usize);
    for y in 0..SLOT_PIXELS {
        for x in 0..SLOT_PIXELS {
            let mut sum = [0u32; 4];
            for dy in 0..step {
                for dx in 0..step {
                    let sx =
                        ((x * step + dx) * image.width / (SLOT_PIXELS * step)).min(image.width - 1);
                    let sy = ((y * step + dy) * image.height / (SLOT_PIXELS * step))
                        .min(image.height - 1);
                    let i = ((sy * image.width + sx) * 4) as usize;
                    for k in 0..4 {
                        sum[k] += u32::from(image.rgba[i + k]);
                    }
                }
            }
            rgba.extend(sum.map(|v| (v / (step * step)) as u8));
        }
    }
    Some(fsx_render::TextureImage {
        width: SLOT_PIXELS,
        height: SLOT_PIXELS,
        rgba,
    })
}
/// Pack the textures of the land classes in view into one atlas.
fn land_atlas(root: &Path, quads: &[u8]) -> (Option<fsx_render::TextureImage>, Vec<u16>) {
    let side = ATLAS_SLOTS * SLOT_PIXELS;
    let mut atlas = vec![0u8; (side * side * 4) as usize];
    let mut slots: std::collections::HashMap<u16, u16> = std::collections::HashMap::new();
    let mut used = 0u32;
    let mut place = |texture: u16, atlas: &mut Vec<u8>| -> Option<u16> {
        if let Some(s) = slots.get(&texture) {
            return Some(*s);
        }
        if used >= ATLAS_SLOTS * ATLAS_SLOTS {
            return None;
        }
        let image = land_image(root, texture)?;
        let (ox, oy) = (
            (used % ATLAS_SLOTS) * SLOT_PIXELS,
            (used / ATLAS_SLOTS) * SLOT_PIXELS,
        );
        for y in 0..SLOT_PIXELS {
            let src = (y * SLOT_PIXELS * 4) as usize;
            let dst = (((oy + y) * side + ox) * 4) as usize;
            atlas[dst..dst + (SLOT_PIXELS * 4) as usize]
                .copy_from_slice(&image.rgba[src..src + (SLOT_PIXELS * 4) as usize]);
        }
        let slot = used as u16;
        used += 1;
        slots.insert(texture, slot);
        Some(slot)
    };
    let fallback = place(40, &mut atlas).or_else(|| place(31, &mut atlas));
    let Some(fallback) = fallback else {
        return (None, Vec::new());
    };
    let quad_slots = quads
        .iter()
        .map(|&c| place(land_texture(c), &mut atlas).unwrap_or(fallback))
        .collect();
    (
        Some(fsx_render::TextureImage {
            width: side,
            height: side,
            rgba: atlas,
        }),
        quad_slots,
    )
}
fn run_menu(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        fsx_ui::run(path)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        Err("The FSX menu host currently requires Windows".into())
    }
}
/// FSX folders beside or above the executable, then the working directory.
fn find_installation() -> Option<PathBuf> {
    let is_root = |p: &Path| p.join("fsx.exe").is_file() && p.join("SimObjects").is_dir();
    let exe = std::env::current_exe().ok()?;
    let cwd = std::env::current_dir().ok();
    exe.ancestors()
        .skip(1)
        .chain(cwd.as_deref().into_iter())
        .flat_map(|d| [d.to_path_buf(), d.join("FSX")])
        .find(|p| is_root(p))
}
fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1).peekable();
    if args.peek().is_none() {
        // Double-click start: open the menu for the nearest FSX installation.
        return run_menu(&find_installation().ok_or(
            "No FSX installation found. Place openFSX next to an FSX folder or pass its path with --menu",
        )?);
    }
    let path = PathBuf::from(
        args.next()
            .ok_or("Usage: openfsx-viewer <model.mdl> [options]\nFlight: openfsx-viewer <aircraft folder> --fly [--variant N] [--windows-1252]\nOptions: --lod N --texture-dir folder --no-textures --snapshot output.ppm --seconds N")?,
    );
    if path == Path::new("--help") || path == Path::new("-h") {
        println!("openfsx-viewer <model.mdl> [--lod N] [--texture-dir folder | --no-textures]\nopenfsx-viewer <aircraft folder> --fly [--ground] [--variant N] [--windows-1252] [--lod N]\nWindows menu: openfsx-viewer <FSX installation> --menu\nAnimation inspector: --animation N [--frame N] (use inspect-model for IDs; [ / ] scrub)\nRecorded pose: --reference-pose sample.ndjson [--reference-sample N] (inspection only)\nOrbit camera: --view front|rear --zoom N (0.1..10)\nOptional: --debug-hud (experimental telemetry); --snapshot output.ppm [--seconds N, flight only, 0..300]\nFlight keys: Down/Up pull/push; Left/Right bank; Q/E rudder; PageUp/PageDown throttle; F1 idle; F4 full power; B parking brake; Space wheel brakes; Home/End trim; F5-F8 flaps up/raise/lower/full; P pause; R reset; Esc close.
Game controller: left stick pitch/roll, right stick rudder, triggers throttle, D-pad flaps, A brakes, X parking brake, Start pause, Back reset.
With no arguments, the menu opens for an FSX folder beside or above openfsx-viewer.");
        println!("Visibility inspector: repeat --visibility-var NAME#units#value (explicit values; no flight override)");
        println!("Pose inspector: repeat --pose-var NAME#units#value (XAPS/read-only XAPC; no flight override)");
        println!("Flight views: --cockpit starts inside; F9 virtual cockpit / recenter, F11 exterior; cockpit drag looks around, wheel changes FOV.");
        println!("Experimental Windows C172 instruments: --native-gauges [--gauge-host path/to/fsx-gauge-host.exe]. Five needles plus native attitude/heading sprites, ideal ISA air data and ideal gyros; no RPM, full panel services or needle lag.");
        println!("Optional native C172 clock: --native-clock-start HH:MM:SS with --native-gauges. Explicit local time advanced by simulation seconds; Quartz daylight styling; native timer buttons and mouse/keyboard routing unavailable.");
        println!("Clock controls: repeat --clock-event CLOCK_EVENT[:u32] and --native-clock-select none|hours|minutes|seconds with --native-clock-start. Startup event injection. With a native clock in cockpit view, left press picks its three buttons; other gauge mouse/keyboard bindings remain unavailable.");
        println!("Optional native clock button injection: --native-clock-click 0|1|2 with --native-clock-start.\nOptional daylight XML registration plaque: --xml-gauge-font path/to/font.ttf with --native-gauges. Explicit font mapping and pixels-per-em metrics; FSX default font/rasterization parity unverified.");
        return Ok(());
    }
    let (mut lod, mut snapshot) = (None, None);
    let (mut texture_dir, mut no_textures) = (None, false);
    let (mut fly, mut variant, mut windows_1252, mut seconds) = (false, 0u32, false, 0f64);
    let mut ground = false;
    let mut airport = None;
    let mut hours = 12.;
    let mut airborne = false;
    let mut wind = [0.0_f64; 3];
    let mut gusts = 0.0_f64;
    let mut cockpit = false;
    let mut native_gauges_enabled = false;
    let mut gauge_host = None;
    let mut xml_gauge_font = None;
    let mut native_clock = None;
    let mut clock_events = Vec::new();
    let mut clock_clicks = Vec::new();
    let mut clock_selection = None;
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
            Some("--native-clock-click") => {
                let child = args
                    .next()
                    .ok_or("Expected authored clock mouse child 0..2")?
                    .to_str()
                    .ok_or("Invalid clock child")?
                    .parse::<usize>()
                    .map_err(|_| "Expected clock mouse child 0..2")?;
                if child > 2 || clock_clicks.len() >= 128 {
                    return Err("Clock child must be 0..2; at most 128 startup clicks".into());
                }
                clock_clicks.push(child);
            }
            Some("--native-gauges") => native_gauges_enabled = true,
            Some("--native-clock-select") => {
                let value = args.next().ok_or("Expected none/hours/minutes/seconds")?;
                clock_selection = Some(match value.to_str() {
                    Some("none") => 0,
                    Some("hours") => 24,
                    Some("minutes") => 25,
                    Some("seconds") => 26,
                    _ => return Err("Expected none/hours/minutes/seconds".into()),
                });
            }
            Some("--clock-event") => {
                if clock_events.len() >= 128 {
                    return Err("Too many startup clock events".into());
                }
                let value = args.next().ok_or("Expected CLOCK_EVENT[:u32 data]")?;
                let value = value.to_str().ok_or("Invalid clock event text")?;
                let (name, data) = value.split_once(':').unwrap_or((value, "0"));
                let event = fsx_sim::clock::ClockEvent::from_name(name)?;
                let data = data
                    .parse::<u32>()
                    .map_err(|_| "Invalid clock event u32 payload")?;
                clock_events.push((event, data));
            }
            Some("--native-clock-start") => {
                let value = args.next().ok_or("Expected local clock HH:MM:SS")?;
                native_clock = Some(fsx_sim::clock::LocalClock::parse(
                    value.to_str().ok_or("Invalid local clock text")?,
                )?);
            }
            Some("--xml-gauge-font") => {
                xml_gauge_font = Some(PathBuf::from(
                    args.next().ok_or("Expected explicit XML gauge font path")?,
                ))
            }
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
            Some("--airborne") => airborne = true,
            Some("--gusts") => {
                let k: f64 = args
                    .next()
                    .and_then(|a| a.to_str().and_then(|s| s.parse().ok()))
                    .ok_or("Expected --gusts KNOTS")?;
                if !(0. ..=30.).contains(&k) {
                    return Err("--gusts must be 0 to 30 knots".into());
                }
                gusts = k * 1852. / 3600.;
            }
            Some("--wind") => {
                let text = args
                    .next()
                    .and_then(|a| a.into_string().ok())
                    .ok_or("Expected --wind DEGREES/KNOTS")?;
                let (d, k) = text
                    .split_once('/')
                    .ok_or("Expected --wind DEGREES/KNOTS")?;
                let (d, k): (f64, f64) = (
                    d.parse().map_err(|_| "Invalid --wind direction")?,
                    k.parse().map_err(|_| "Invalid --wind speed")?,
                );
                if !d.is_finite() || !k.is_finite() || !(0. ..=100.).contains(&k) {
                    return Err("--wind speed must be 0 to 100 knots".into());
                }
                // Meteorological convention: direction the wind blows from.
                let toward = (d + 180.).to_radians();
                let m = k * 1852. / 3600.;
                wind = [m * toward.cos(), m * toward.sin(), 0.];
            }
            Some("--time") => {
                let text = args
                    .next()
                    .and_then(|a| a.into_string().ok())
                    .ok_or("Expected --time HH:MM")?;
                let (h, m) = text.split_once(':').ok_or("Expected --time HH:MM")?;
                let (h, m): (f64, f64) = (
                    h.parse().map_err(|_| "Invalid --time hour")?,
                    m.parse().map_err(|_| "Invalid --time minute")?,
                );
                if !(0. ..24.).contains(&h) || !(0. ..60.).contains(&m) {
                    return Err("--time must be between 00:00 and 23:59".into());
                }
                hours = h + m / 60.;
            }
            Some("--airport") => {
                airport = Some(
                    args.next()
                        .and_then(|a| a.into_string().ok())
                        .ok_or("Expected an airport ICAO code")?,
                );
                ground = true;
            }
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
            || xml_gauge_font.is_some()
            || native_clock.is_some()
            || !clock_events.is_empty()
            || !clock_clicks.is_empty()
            || clock_selection.is_some()
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
        return run_menu(&path);
    }
    if (native_gauges_enabled && (!fly || !cfg!(windows) || no_textures))
        || (gauge_host.is_some() && !native_gauges_enabled)
        || (xml_gauge_font.is_some() && !native_gauges_enabled)
        || (native_clock.is_some() && !native_gauges_enabled)
    {
        return Err("--native-gauges requires Windows --fly with textures; --gauge-host, --xml-gauge-font and --native-clock-start require --native-gauges".into());
    }
    if (!clock_events.is_empty() || !clock_clicks.is_empty() || clock_selection.is_some())
        && native_clock.is_none()
    {
        return Err("--clock-event, --native-clock-click and --native-clock-select require --native-clock-start".into());
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
        // Aircraft without an authored eyepoint start in the exterior view.
        eprintln!("No configured [views] eyepoint; starting in exterior view");
        cockpit = false;
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
        'gauges: {
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
            let gauges = match flight_gauges::FlightGauges::new(root, &aircraft.panel, &helper) {
                Ok(gauges) => gauges,
                // Plain --native-gauges degrades to the static panel for aircraft
                // whose instruments are not supported yet.
                Err(error) if native_clock.is_none() && xml_gauge_font.is_none() => {
                    eprintln!("Native gauges unavailable: {error}");
                    break 'gauges None;
                }
                Err(error) => return Err(error),
            };
            let gauges = if let Some(clock) = native_clock {
                gauges.with_day_native_clock(root, clock)?
            } else {
                gauges
            };
            Some(if let Some(font) = &xml_gauge_font {
                gauges.with_day_xml_plaque(
                    root,
                    font,
                    aircraft
                        .registration
                        .as_deref()
                        .ok_or("XML plaque requires authored aircraft atc_id")?,
                )?
            } else {
                gauges
            })
        }
    } else {
        None
    };
    let scenery = match (&airport, &aircraft) {
        (Some(icao), Some(_)) => Some(airport_scenery(&path, icao, hours)?),
        (Some(_), None) => return Err("--airport requires --fly".into()),
        _ => None,
    };
    let mut flight = aircraft
        .map(|mut a| {
            a.parameters.wind = wind;
            if let Some((start, _, _)) = scenery.as_ref().filter(|_| airborne) {
                FlightSession::airborne(a.parameters, start.heading.to_radians()).map(|mut f| {
                    f.set_field_elevation(start.altitude);
                    f.simulation.state.velocity =
                        std::array::from_fn(|i| f.simulation.state.velocity[i] + wind[i]);
                    f
                })
            } else if let Some((start, _, _)) = &scenery {
                FlightSession::on_runway(a.parameters, [0., 0.], start.heading.to_radians()).map(
                    |mut f| {
                        f.set_field_elevation(start.altitude);
                        f
                    },
                )
            } else if ground {
                FlightSession::on_ground(a.parameters)
            } else {
                FlightSession::new(a.parameters)
            }
        })
        .transpose()?;
    let mut flight_pose = flight_pose::FlightPose::new(&model);
    if let Some(flight) = &mut flight {
        flight.gust_amplitude = gusts;
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
            let mut frame = gauges.update(&flight.simulation)?;
            if !clock_events.is_empty() || !clock_clicks.is_empty() || clock_selection.is_some() {
                for (event, data) in clock_events {
                    gauges.clock_event(event, data, flight.simulation.time)?;
                }
                for child in clock_clicks {
                    gauges.click_clock(child, flight.simulation.time)?;
                }
                if let Some(state) = clock_selection {
                    gauges.select_clock(state)?;
                }
                frame = gauges.update(&flight.simulation)?;
            }
            if cockpit {
                gauges.apply(&frame, &model, &mut appearance, None)?;
            } else {
                let interior = alternate
                    .as_mut()
                    .ok_or("Native gauges need the configured interior model")?;
                gauges.apply(&frame, &interior.model, &mut interior.appearance, None)?;
            }
            println!("Native C172 gauges: helper PID {}, sequence {}, time {}, evaluated {}; six needles (including the tachometer from simulated engine RPM) and three attitude/heading sprites, ideal ISA/English units and ideal gyros; needle lag and full panel lifecycle remain unsupported.",gauges.pid(),frame.sequence,frame.time,frame.evaluated);
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
        if let Some((_, ground, _)) = &scenery {
            gpu.set_ground(Some(ground.clone()), &geometry, &appearance)?;
        }
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
    let gamepad = flight.is_some().then(gamepad::Gamepad::new);
    let audio = if flight.is_some() {
        audio::Audio::new(&path.join("sound"), windows_1252)
            .map_err(|e| eprintln!("Aircraft sound unavailable: {e}"))
            .ok()
    } else {
        None
    };
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
        gamepad,
        audio,
        sound_state: (true, 0.),
        ground: scenery.as_ref().map(|(_, g, _)| g.clone()),
        world: scenery.map(|(_, _, w)| w),
        terrain_center: (0., 0.),
        pending_ground: None,
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
#[cfg(test)]
mod world_tests {
    use super::*;
    #[test]
    fn recentred_ground_moves_with_the_aircraft_when_installed() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../FSX");
        let helper =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fsx-terrain-host.exe");
        if !root.join("terrain.dll").is_file() || !helper.is_file() {
            return;
        }
        let aircraft = root.join("SimObjects/Airplanes/C172");
        let (_, first, world) = airport_scenery(&aircraft, "KSEA", 12.).unwrap();
        let moved = world.ground((30_000., 0.));
        let (a, b) = (first.terrain.unwrap(), moved.terrain.unwrap());
        assert!((b.north - a.north - 30_000.).abs() < 1.);
        // Seattle Boeing Field and Paine Field sit north of KSEA.
        assert!(!moved.runways.is_empty());
        assert!(b.height_at(0., 30_000.).is_some() && a.height_at(0., 45_000.).is_none());
    }
}
