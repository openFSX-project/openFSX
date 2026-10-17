//! Native MDLX viewer and reproducible offscreen rendering, independent of fsx.exe.
use openfsx::{
    math::{self, Mat4},
    mdl::{Geometry, Model},
};
use std::{
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};
use wgpu::util::DeviceExt;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::Window,
};

// GPU initialization and map_async don't need a general-purpose async runtime.
fn block_on<T>(future: impl Future<Output = T>) -> T {
    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}
fn bytes(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    values.into_iter().flat_map(f32::to_le_bytes).collect()
}

struct Orbit {
    center: [f32; 3],
    radius: f32,
    yaw: f32,
    pitch: f32,
    distance: f32,
}
impl Orbit {
    fn new(g: &Geometry) -> Self {
        let center = std::array::from_fn(|i| (g.min[i] + g.max[i]) * 0.5);
        let radius = math::dot(math::sub(g.max, g.min), math::sub(g.max, g.min))
            .sqrt()
            .max(0.1)
            * 0.5;
        Self {
            center,
            radius,
            yaw: 0.7,
            pitch: 0.3,
            distance: radius * 3.4,
        }
    }
    fn matrix(&self, aspect: f32) -> Mat4 {
        let eye = [
            self.center[0] + self.distance * self.yaw.sin() * self.pitch.cos(),
            self.center[1] + self.distance * self.pitch.sin(),
            self.center[2] + self.distance * self.yaw.cos() * self.pitch.cos(),
        ];
        math::camera(
            eye,
            self.center,
            aspect,
            self.radius * 0.01,
            self.radius * 100.,
        )
    }
    fn zoom(&mut self, delta: f32) {
        self.distance =
            (self.distance * (-delta * 0.12).exp()).clamp(self.radius * 1.1, self.radius * 30.);
    }
}
struct Gpu {
    surface: Option<wgpu::Surface<'static>>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    vertex: wgpu::Buffer,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    depth: wgpu::TextureView,
    count: u32,
}
impl Gpu {
    async fn new(
        window: Option<Arc<Window>>,
        geometry: &Geometry,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = window
            .map(|window| instance.create_surface(window))
            .transpose()
            .map_err(|e| e.to_string())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: surface.as_ref(),
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
            })
            .await
            .ok_or("No compatible graphics adapter")?;
        println!(
            "Adapter: {} ({:?})",
            adapter.get_info().name,
            adapter.get_info().backend
        );
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("openFSX viewer"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: Default::default(),
                },
                None,
            )
            .await
            .map_err(|e| e.to_string())?;
        let format = surface
            .as_ref()
            .and_then(|s| {
                s.get_capabilities(&adapter)
                    .formats
                    .into_iter()
                    .find(|f| f.is_srgb())
            })
            .unwrap_or(wgpu::TextureFormat::Rgba8UnormSrgb);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        if let Some(s) = &surface {
            s.configure(&device, &config);
        }
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Camera matrix"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Solid MDLX preview"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../viewer.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("MDLX triangles"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 36,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x3],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Camera"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let vertex = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("MDLX geometry"),
            contents: &bytes(geometry.vertices.iter().flatten().copied()),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let depth = Self::depth(&device, width, height);
        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            vertex,
            uniform,
            bind,
            depth,
            count: geometry
                .vertices
                .len()
                .try_into()
                .map_err(|_| "Too many vertices")?,
        })
    }
    fn depth(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("Depth"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }
    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.depth = Self::depth(&self.device, width, height);
        if let Some(surface) = &self.surface {
            surface.configure(&self.device, &self.config);
        }
    }
    fn encode(&self, view: &wgpu::TextureView, orbit: &Orbit) -> wgpu::CommandEncoder {
        self.queue.write_buffer(
            &self.uniform,
            0,
            &bytes(orbit.matrix(self.config.width as f32 / self.config.height as f32)),
        );
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Model preview"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.035,
                            g: 0.055,
                            b: 0.085,
                            a: 1.,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.set_vertex_buffer(0, self.vertex.slice(..));
            pass.draw(0..self.count, 0..1);
        }
        encoder
    }
    fn draw(&mut self, orbit: &Orbit) -> Result<(), String> {
        let surface = self.surface.as_ref().ok_or("No window surface")?;
        let frame = match surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                surface.configure(&self.device, &self.config);
                return Ok(());
            }
            Err(wgpu::SurfaceError::Timeout) => return Ok(()),
            Err(e) => return Err(e.to_string()),
        };
        let view = frame.texture.create_view(&Default::default());
        self.queue.submit([self.encode(&view, orbit).finish()]);
        frame.present();
        Ok(())
    }
    fn snapshot(&self, orbit: &Orbit, path: &Path) -> Result<(), String> {
        let (width, height) = (self.config.width, self.config.height);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Offscreen preview"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let row = (width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Readback"),
            size: u64::from(row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.encode(&texture.create_view(&Default::default()), orbit);
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: Default::default(),
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let (send, recv) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = send.send(result);
            });
        self.device.poll(wgpu::Maintain::Wait);
        recv.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let mut ppm = format!("P6\n{width} {height}\n255\n").into_bytes();
        {
            let data = buffer.slice(..).get_mapped_range();
            for y in 0..height {
                for x in 0..width {
                    let i = (y * row + x * 4) as usize;
                    ppm.extend_from_slice(&data[i..i + 3]);
                }
            }
        }
        buffer.unmap();
        std::fs::write(path, ppm).map_err(|e| e.to_string())?;
        println!("Snapshot: {} ({}x{})", path.display(), width, height);
        Ok(())
    }
}
struct App {
    geometry: Geometry,
    orbit: Orbit,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    dragging: bool,
    cursor: Option<(f64, f64)>,
    error: Option<String>,
}
impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let result = (|| {
            let window=Arc::new(event_loop.create_window(Window::default_attributes().with_title(format!("openFSX • MDLX LOD {} • Drag: orbit | Wheel: zoom | R: reset | Esc: close",self.geometry.lod)).with_inner_size(winit::dpi::LogicalSize::new(1100.,760.))).map_err(|e|e.to_string())?);
            let size = window.inner_size();
            self.gpu = Some(block_on(Gpu::new(
                Some(window.clone()),
                &self.geometry,
                size.width.max(1),
                size.height.max(1),
            ))?);
            window.request_redraw();
            self.window = Some(window);
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
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => self.dragging = state == ElementState::Pressed,
            WindowEvent::Focused(false) => {
                self.dragging = false;
                self.cursor = None;
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some((x, y)) = self.cursor {
                    if self.dragging {
                        self.orbit.yaw += (position.x - x) as f32 * 0.008;
                        self.orbit.pitch =
                            (self.orbit.pitch + (position.y - y) as f32 * 0.008).clamp(-1.45, 1.45);
                    }
                }
                self.cursor = Some((position.x, position.y));
            }
            WindowEvent::MouseWheel { delta, .. } => self.orbit.zoom(match delta {
                MouseScrollDelta::LineDelta(_, y) => y,
                MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.,
            }),
            WindowEvent::KeyboardInput { ref event, .. }
                if event.state == ElementState::Pressed =>
            {
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) => event_loop.exit(),
                    PhysicalKey::Code(KeyCode::KeyR) => self.orbit = Orbit::new(&self.geometry),
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
                if let Some(gpu) = &mut self.gpu {
                    if let Err(error) = gpu.draw(&self.orbit) {
                        self.error = Some(error);
                        event_loop.exit();
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
            .ok_or("Usage: openfsx-viewer <model.mdl> [--lod N] [--snapshot output.ppm]")?,
    );
    let (mut lod, mut snapshot) = (None, None);
    while let Some(arg) = args.next() {
        match arg.to_str() {
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
            _ => return Err("Unknown viewer argument".into()),
        }
    }
    let model = Model::load(&path)?;
    let geometry = model.geometry(lod)?;
    println!(
        "{} | LOD {} | {} triangles | {} animated nodes in bind pose | {} skinned parts omitted",
        model.name,
        geometry.lod,
        geometry.vertices.len() / 3,
        model.animated_nodes,
        geometry.skipped_skinned_parts
    );
    println!("Preview limitations: no textures, skinning, animation or visibility evaluation; all unskinned parts are opaque.");
    let orbit = Orbit::new(&geometry);
    if let Some(path) = snapshot {
        return block_on(Gpu::new(None, &geometry, 960, 720))?.snapshot(&orbit, &path);
    }
    let mut app = App {
        geometry,
        orbit,
        window: None,
        gpu: None,
        dragging: false,
        cursor: None,
        error: None,
    };
    EventLoop::new()
        .map_err(|e| e.to_string())?
        .run_app(&mut app)
        .map_err(|e| e.to_string())?;
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
