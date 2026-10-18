//! Native child surface: the menu uses the same MDLX/DDS renderer as flight.
use fsx_model::Model;
use fsx_render::{block_on, Appearance, Gpu, Orbit};
use fsx_vehicle::aircraft::VisualAircraft;
use std::{path::Path, sync::Arc, time::Instant};
use winit::{
    dpi::{LogicalPosition, LogicalSize},
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::ActiveEventLoop,
    raw_window_handle::HasWindowHandle,
    window::Window,
};

pub(super) struct Preview {
    // Drop the GPU and child before their parent (also retained by the shell).
    gpu: Option<Gpu>,
    pub window: Arc<Window>,
    _parent: Arc<Window>,
    orbit: Option<Orbit>,
    dragging: bool,
    cursor: Option<(f64, f64)>,
    last_frame: Instant,
}
impl Preview {
    pub fn new(parent: Arc<Window>, event_loop: &ActiveEventLoop) -> Result<Self, String> {
        let handle = parent.window_handle().map_err(|e| e.to_string())?.as_raw();
        let attributes = Window::default_attributes()
            .with_title("openFSX aircraft preview")
            .with_decorations(false)
            .with_resizable(false)
            .with_active(false)
            .with_visible(false)
            .with_position(LogicalPosition::new(228., 393.))
            .with_inner_size(LogicalSize::new(773., 277.));
        // SAFETY: the returned child retains its parent until after GPU/child drop.
        let window = Arc::new(
            event_loop
                .create_window(unsafe { attributes.with_parent_window(Some(handle)) })
                .map_err(|e| e.to_string())?,
        );
        Ok(Self {
            gpu: None,
            window,
            _parent: parent,
            orbit: None,
            dragging: false,
            cursor: None,
            last_frame: Instant::now(),
        })
    }
    pub fn load(&mut self, root: &Path, folder: &Path, variant: u32) -> Result<(), String> {
        self.window.set_visible(false);
        self.gpu = None;
        self.orbit = None;
        let visual = VisualAircraft::load(folder, variant, true)?;
        let model_path = visual.model.canonicalize().map_err(|e| e.to_string())?;
        let texture = visual.texture.canonicalize().map_err(|e| e.to_string())?;
        if !model_path.starts_with(root) || !texture.starts_with(root) {
            return Err("Preview resources must belong to the selected installation".into());
        }
        let model = Model::load(&model_path)?;
        let mut geometry = model.geometry(None)?;
        // A stationary, engine-stopped inspection state. Do not infer live
        // simulator values from throttle or execute original gauge code.
        let visibility = model.visibility_plan(|name, units| {
            if (name.eq_ignore_ascii_case("PROP MAX RPM PERCENT:1")
                && units.eq_ignore_ascii_case("percent"))
                || (["CENTER WHEEL RPM", "LEFT WHEEL RPM", "RIGHT WHEEL RPM"]
                    .iter()
                    .any(|n| name.eq_ignore_ascii_case(n))
                    && units.eq_ignore_ascii_case("grads"))
                || (name.eq_ignore_ascii_case("LIGHT LANDING")
                    && units.eq_ignore_ascii_case("bool"))
            {
                Some(0.)
            } else {
                None
            }
        });
        geometry.apply_visibility(&visibility.nodes)?;
        println!(
            "Menu visibility: {}/{} conditions evaluated; {} hidden draw batches",
            visibility
                .decisions
                .iter()
                .filter(|d| d.status == fsx_model::visibility::Status::Evaluated)
                .count(),
            visibility.decisions.len(),
            geometry.batches.iter().filter(|b| !b.visible).count()
        );
        let appearance = Appearance::load(&model, &geometry, Some(&texture))?;
        let mut orbit = Orbit::new(&geometry);
        orbit.pitch = 0.12;
        orbit.distance = orbit.radius * 1.35;
        let size = self.window.inner_size();
        let gpu = block_on(Gpu::new(
            Some(self.window.clone()),
            &geometry,
            &appearance,
            false,
            size.width.max(1),
            size.height.max(1),
        ))?;
        println!(
            "Menu 3D preview: {} | LOD {} | {} triangles",
            visual.title,
            geometry.lod,
            geometry.vertices.len() / 3
        );
        self.gpu = Some(gpu);
        self.orbit = Some(orbit);
        self.dragging = false;
        self.cursor = None;
        self.last_frame = Instant::now();
        Ok(())
    }
    pub fn ready(&self) -> bool {
        self.gpu.is_some()
    }
    pub fn visible(&mut self, visible: bool) {
        self.window.set_visible(visible && self.ready());
        if visible && self.ready() {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
            };
            if let Ok(handle) = self.window.window_handle() {
                if let winit::raw_window_handle::RawWindowHandle::Win32(handle) = handle.as_raw() {
                    // WebView2 creates its sibling asynchronously. Keep the live
                    // surface above that sibling, without taking keyboard focus.
                    unsafe {
                        SetWindowPos(
                            handle.hwnd.get() as _,
                            HWND_TOP,
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                        );
                    }
                }
            }
        }
        self.dragging = false;
        self.cursor = None;
        self.last_frame = Instant::now();
        if visible {
            self.window.request_redraw();
        }
    }
    pub fn layout(&mut self, parent: &Window) {
        let logical = parent.inner_size().to_logical::<f64>(parent.scale_factor());
        let scale = (logical.width / 1024.).min(logical.height / 738.);
        self.window
            .set_outer_position(LogicalPosition::new(228. * scale, 393. * scale));
        let _ = self
            .window
            .request_inner_size(LogicalSize::new(773. * scale, 277. * scale));
    }
    pub fn event(&mut self, event: &WindowEvent) -> Result<(), String> {
        match event {
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.dragging = *state == ElementState::Pressed;
            }
            WindowEvent::Focused(false) => {
                self.dragging = false;
                self.cursor = None;
            }
            WindowEvent::CursorMoved { position, .. } => {
                if self.dragging {
                    if let (Some((x, y)), Some(orbit)) = (self.cursor, &mut self.orbit) {
                        orbit.yaw += (position.x - x) as f32 * 0.008;
                        orbit.pitch =
                            (orbit.pitch + (position.y - y) as f32 * 0.008).clamp(-1.45, 1.45);
                    }
                }
                self.cursor = Some((position.x, position.y));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(orbit) = &mut self.orbit {
                    orbit.zoom(match delta {
                        MouseScrollDelta::LineDelta(_, y) => *y,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.,
                    });
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                if let (Some(gpu), Some(orbit)) = (&mut self.gpu, &mut self.orbit) {
                    if !self.dragging {
                        orbit.yaw +=
                            now.duration_since(self.last_frame).as_secs_f32().min(0.1) * 0.18;
                    }
                    gpu.draw(orbit)?;
                }
                self.last_frame = now;
            }
            _ => {}
        }
        Ok(())
    }
}
