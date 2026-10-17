//! Native MDLX viewer and reproducible offscreen rendering, independent of fsx.exe.
use openfsx::{
    aircraft::Aircraft,
    desktop_flight::FlightSession,
    math::{self, Mat4},
    mdl::{Geometry, Material, Model},
    texture::{self, Image},
};
use std::{
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
use wgpu::util::DeviceExt;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
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

struct Appearance {
    materials: Vec<Material>,
    images: Vec<Option<Image>>,
}
impl Appearance {
    fn load(model: &Model, geometry: &Geometry, folder: Option<&Path>) -> Result<Self, String> {
        let mut images: Vec<Option<Image>> = (0..model.textures.len()).map(|_| None).collect();
        let mut used = std::collections::BTreeSet::new();
        for batch in &geometry.batches {
            if let Some(index) = model.materials[batch.material].diffuse_texture {
                used.insert(index);
            }
        }
        let mut loaded = 0;
        let mut memory = 0usize;
        if let Some(folder) = folder {
            for &index in &used {
                let name = &model.textures[index];
                let result = texture::find(folder, name).and_then(|path| {
                    let path = path.ok_or_else(|| format!("Texture not found: {name}"))?;
                    let image = Image::load_dds(&path)?;
                    println!("Texture: {name} {}x{}", image.width, image.height);
                    Ok(image)
                });
                match result {
                    Ok(image) => {
                        memory = memory
                            .checked_add(image.rgba.len())
                            .ok_or("Texture memory overflow")?;
                        if memory > 256 * 1024 * 1024 {
                            return Err(
                                "Selected diffuse textures exceed 256 MiB decoded memory".into()
                            );
                        }
                        images[index] = Some(image);
                        loaded += 1;
                    }
                    Err(error) => eprintln!("Diffuse fallback for {name}: {error}"),
                }
            }
            println!(
                "Diffuse textures: {loaded}/{} loaded ({} MiB decoded)",
                used.len(),
                memory / (1024 * 1024)
            );
        }
        Ok(Self {
            materials: model.materials.clone(),
            images,
        })
    }
}

struct Orbit {
    center: [f32; 3],
    radius: f32,
    yaw: f32,
    pitch: f32,
    distance: f32,
    far: f32,
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
            far: radius * 100.,
        }
    }
    fn matrix(&self, aspect: f32) -> Mat4 {
        math::camera(
            self.eye(),
            self.center,
            aspect,
            self.radius * 0.01,
            self.far,
        )
    }
    fn eye(&self) -> [f32; 3] {
        [
            self.center[0] + self.distance * self.yaw.sin() * self.pitch.cos(),
            self.center[1] + self.distance * self.pitch.sin(),
            self.center[2] + self.distance * self.yaw.cos() * self.pitch.cos(),
        ]
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
    blend_pipeline: wgpu::RenderPipeline,
    vertex: wgpu::Buffer,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    depth: wgpu::TextureView,
    material_binds: Vec<wgpu::BindGroup>,
    draws: Vec<GpuDraw>,
    model_matrix: Mat4,
    flight: bool,
    hud: Option<openfsx::hud::Readout>,
    hud_pipeline: wgpu::RenderPipeline,
    hud_vertex: wgpu::Buffer,
    presented: bool,
}
struct GpuDraw {
    range: std::ops::Range<u32>,
    material: usize,
    center: [f32; 3],
    blended: bool,
}
impl Gpu {
    async fn new(
        window: Option<Arc<Window>>,
        geometry: &Geometry,
        appearance: &Appearance,
        flight: bool,
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
            size: 128,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Diffuse MDLX preview"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../viewer.wgsl").into()),
        });
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Material layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Viewer pipeline layout"),
            bind_group_layouts: &[&camera_layout, &material_layout],
            push_constant_ranges: &[],
        });
        let create_pipeline = |blended: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("MDLX triangles"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 36,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2,3=>Float32],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: if blended {Some(wgpu::BlendState::ALPHA_BLENDING)}else{None},
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: !blended,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        })
        };
        let pipeline = create_pipeline(false);
        let blend_pipeline = create_pipeline(true);
        let hud_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Original flight overlay"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../hud.wgsl").into()),
        });
        let hud_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Flight overlay layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });
        let hud_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Flight overlay"),
            layout: Some(&hud_layout),
            vertex: wgpu::VertexState {
                module: &hud_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 24,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x2,1=>Float32x4],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &hud_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let hud_vertex = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Flight overlay vertices"),
            size: 65536 * 24,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Camera"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let upload = |image: &Image| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("DDS diffuse"),
                size: wgpu::Extent3d {
                    width: image.width,
                    height: image.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: Default::default(),
                    aspect: wgpu::TextureAspect::All,
                },
                &image.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(image.width * 4),
                    rows_per_image: Some(image.height),
                },
                wgpu::Extent3d {
                    width: image.width,
                    height: image.height,
                    depth_or_array_layers: 1,
                },
            );
            texture.create_view(&Default::default())
        };
        let white = upload(&Image {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        });
        let views = appearance
            .images
            .iter()
            .map(|image| image.as_ref().map(upload))
            .collect::<Vec<_>>();
        let mut material_binds = Vec::new();
        let mut materials = appearance.materials.clone();
        let ground_material = materials.len();
        if flight {
            for diffuse in [
                [0.12, 0.18, 0.07, 1.],
                [0.06, 0.07, 0.08, 1.],
                [0.85, 0.85, 0.75, 1.],
                [0.16, 0.23, 0.10, 1.],
            ] {
                materials.push(Material {
                    diffuse,
                    diffuse_texture: None,
                    alpha_blend: false,
                    alpha_function: 0,
                    alpha_threshold: 0.,
                    clamp_texture: false,
                });
            }
        }
        for material in &materials {
            let view = material
                .diffuse_texture
                .and_then(|index| views[index].as_ref());
            let params = [
                material.diffuse[0],
                material.diffuse[1],
                material.diffuse[2],
                material.diffuse[3],
                f32::from(view.is_some()),
                f32::from(material.alpha_blend),
                material.alpha_function as f32,
                material.alpha_threshold,
            ];
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Diffuse material"),
                contents: &bytes(params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let address = if material.clamp_texture {
                wgpu::AddressMode::ClampToEdge
            } else {
                wgpu::AddressMode::Repeat
            };
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Diffuse sampler"),
                address_mode_u: address,
                address_mode_v: address,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            material_binds.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Diffuse material"),
                layout: &material_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view.unwrap_or(&white)),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform.as_entire_binding(),
                    },
                ],
            }));
        }
        let mut draws: Vec<GpuDraw> = geometry
            .batches
            .iter()
            .map(|batch| GpuDraw {
                range: batch.range.clone(),
                material: batch.material,
                center: batch.center,
                blended: appearance.materials[batch.material].alpha_blend,
            })
            .collect();
        let mut vertices = geometry
            .vertices
            .iter()
            .map(|v| [v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], 1.])
            .collect::<Vec<_>>();
        if flight {
            let mut quad = |x0: f32, z0: f32, x1: f32, z1: f32, y: f32| {
                for [x, z] in [[x0, z0], [x1, z0], [x1, z1], [x0, z0], [x1, z1], [x0, z1]] {
                    vertices.push([x, y, z, 0., 1., 0., 0., 0., 0.]);
                }
            };
            let start = geometry.vertices.len() as u32;
            quad(-8000., -8000., 8000., 8000., 0.);
            let mut count = 6;
            draws.push(GpuDraw {
                range: start..start + count,
                material: ground_material,
                center: [0.; 3],
                blended: false,
            });
            quad(-15., -500., 15., 1500., 0.02);
            draws.push(GpuDraw {
                range: start + count..start + count + 6,
                material: ground_material + 1,
                center: [0.; 3],
                blended: false,
            });
            count += 6;
            for i in 0..40 {
                let z = -480. + i as f32 * 50.;
                quad(-0.7, z, 0.7, z + 25., 0.04);
            }
            draws.push(GpuDraw {
                range: start + count..start + count + 240,
                material: ground_material + 2,
                center: [0.; 3],
                blended: false,
            });
            count += 240;
            for i in -40..=40 {
                let p = i as f32 * 200.;
                quad(p, -8000., p + 0.8, 8000., 0.01);
                quad(-8000., p, 8000., p + 0.8, 0.01);
            }
            draws.push(GpuDraw {
                range: start + count..vertices.len() as u32,
                material: ground_material + 3,
                center: [0.; 3],
                blended: false,
            });
        }
        let vertex = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("MDLX geometry"),
            contents: &bytes(vertices.iter().flatten().copied()),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let depth = Self::depth(&device, width, height);
        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            blend_pipeline,
            vertex,
            uniform,
            bind,
            depth,
            material_binds,
            draws,
            model_matrix: math::IDENTITY,
            flight,
            hud: None,
            hud_pipeline,
            hud_vertex,
            presented: false,
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
    fn encode(
        &self,
        view: &wgpu::TextureView,
        orbit: &Orbit,
    ) -> Result<wgpu::CommandEncoder, String> {
        self.queue.write_buffer(
            &self.uniform,
            0,
            &bytes(
                orbit
                    .matrix(self.config.width as f32 / self.config.height as f32)
                    .into_iter()
                    .chain(self.model_matrix),
            ),
        );
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Model preview"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(if self.flight {
                            wgpu::Color {
                                r: 0.33,
                                g: 0.56,
                                b: 0.79,
                                a: 1.,
                            }
                        } else {
                            wgpu::Color {
                                r: 0.035,
                                g: 0.055,
                                b: 0.085,
                                a: 1.,
                            }
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
            for draw in self.draws.iter().filter(|draw| !draw.blended) {
                pass.set_bind_group(1, &self.material_binds[draw.material], &[]);
                pass.draw(draw.range.clone(), 0..1);
            }
            let mut blended = self
                .draws
                .iter()
                .filter(|draw| draw.blended)
                .collect::<Vec<_>>();
            let eye = orbit.eye();
            let distance = |draw: &GpuDraw| {
                let d = math::sub(math::transform(self.model_matrix, draw.center), eye);
                math::dot(d, d)
            };
            blended.sort_by(|a, b| distance(b).total_cmp(&distance(a)));
            pass.set_pipeline(&self.blend_pipeline);
            for draw in blended {
                pass.set_bind_group(1, &self.material_binds[draw.material], &[]);
                pass.draw(draw.range.clone(), 0..1);
            }
        }
        if let Some(readout) = &self.hud {
            let vertices = openfsx::hud::vertices(readout, self.config.width, self.config.height);
            if vertices.len() > 65536 {
                return Err("Flight overlay exceeds vertex capacity".into());
            }
            self.queue.write_buffer(
                &self.hud_vertex,
                0,
                &bytes(vertices.iter().flatten().copied()),
            );
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Flight overlay"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.hud_pipeline);
            pass.set_vertex_buffer(0, self.hud_vertex.slice(..));
            pass.draw(0..vertices.len() as u32, 0..1);
        }
        Ok(encoder)
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
        self.queue.submit([self.encode(&view, orbit)?.finish()]);
        frame.present();
        if !self.presented {
            println!("First native window frame presented successfully");
            self.presented = true;
        }
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
        let mut encoder = self.encode(&texture.create_view(&Default::default()), orbit)?;
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
    appearance: Appearance,
    orbit: Orbit,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    dragging: bool,
    cursor: Option<(f64, f64)>,
    error: Option<String>,
    flight: Option<FlightSession>,
    last_frame: Instant,
    last_title: Instant,
    visible: bool,
}
fn flight_view(flight: &FlightSession, orbit: &mut Orbit) {
    orbit.center = flight.center();
    orbit.yaw = flight.heading() + std::f32::consts::PI + 0.35;
    orbit.pitch = 0.25;
    orbit.far = 20000.;
}
impl App {
    fn advance_flight(&mut self) -> Result<(), String> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_frame).as_secs_f64();
        self.last_frame = now;
        if let Some(flight) = &mut self.flight {
            flight.advance(elapsed)?;
            flight_view(flight, &mut self.orbit);
            if let Some(gpu) = &mut self.gpu {
                gpu.model_matrix = flight.model_matrix();
                gpu.hud = Some(openfsx::hud::Readout::new(
                    flight.simulation.telemetry()?,
                    flight.simulation.controls,
                    flight.paused,
                )?);
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
            let window=Arc::new(event_loop.create_window(Window::default_attributes().with_title(format!("openFSX • MDLX LOD {} • Drag: orbit | Wheel: zoom | R: reset | Esc: close",self.geometry.lod)).with_inner_size(winit::dpi::LogicalSize::new(1100.,760.))).map_err(|e|e.to_string())?);
            let size = window.inner_size();
            self.gpu = Some(block_on(Gpu::new(
                Some(window.clone()),
                &self.geometry,
                &self.appearance,
                self.flight.is_some(),
                size.width.max(1),
                size.height.max(1),
            ))?);
            window.request_redraw();
            self.window = Some(window);
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
            if let (Some(flight), Some(key)) = (
                &mut self.flight,
                openfsx::desktop_flight::flight_key(event.physical_key, &event.logical_key),
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
                if event.state == ElementState::Pressed && self.flight.is_none() =>
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
        println!("openfsx-viewer <model.mdl> [--lod N] [--texture-dir folder | --no-textures]\nopenfsx-viewer <aircraft folder> --fly [--ground] [--variant N] [--windows-1252] [--lod N]\nOptional: --snapshot output.ppm [--seconds N, flight only, 0..300]\nFlight keys: Down/Up pull/push; Left/Right bank; Q/E rudder; PageUp/PageDown throttle; F1 idle; F4 full power; B parking brake; Space wheel brakes; Home/End trim; P pause; R reset; Esc close.");
        return Ok(());
    }
    let (mut lod, mut snapshot) = (None, None);
    let (mut texture_dir, mut no_textures) = (None, false);
    let (mut fly, mut variant, mut windows_1252, mut seconds) = (false, 0u32, false, 0f64);
    let mut ground = false;
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
            Some("--texture-dir") => {
                texture_dir = Some(PathBuf::from(args.next().ok_or("Expected texture folder")?))
            }
            Some("--no-textures") => no_textures = true,
            Some("--fly") => fly = true,
            Some("--ground") => ground = true,
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
    let aircraft = if fly {
        Some(Aircraft::load(&path, variant, windows_1252)?)
    } else {
        None
    };
    if ground && !fly {
        return Err("--ground requires --fly".into());
    }
    let model_path = aircraft
        .as_ref()
        .map(|a| a.model.as_path())
        .unwrap_or(&path);
    let model = Model::load(model_path)?;
    let geometry = model.geometry(lod)?;
    if no_textures && texture_dir.is_some() {
        return Err("Use either --texture-dir or --no-textures".into());
    }
    let default_texture = model_path
        .parent()
        .and_then(Path::parent)
        .map(|path| path.join("texture"));
    let appearance = Appearance::load(
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
        "{} | LOD {} | {} triangles | {} animated nodes in bind pose | {} skinned parts omitted",
        model.name,
        geometry.lod,
        geometry.vertices.len() / 3,
        model.animated_nodes,
        geometry.skipped_skinned_parts
    );
    println!("Preview limitations: diffuse DDS only; approximate alpha blending; no skinning, animation or visibility evaluation.");
    let mut orbit = Orbit::new(&geometry);
    let mut flight = aircraft
        .map(|a| {
            if ground {
                FlightSession::on_ground(a.parameters)
            } else {
                FlightSession::new(a.parameters)
            }
        })
        .transpose()?;
    if let Some(flight) = &mut flight {
        if ground {
            println!("Experimental runway start: fixed tricycle gear, parking brake on. B releases brake; F4 full power; F1 idle; Space wheel brakes.");
        } else {
            println!("Initial airborne prototype: 90 kt, 300 m; no FSX flight-model parity.");
        }
        println!("Controls: Down/Up pull/push | Left/Right bank | Q/E rudder | PageUp/PageDown throttle | Home/End trim | P pause | R reset | Esc close");
        for _ in 0..(seconds / openfsx::flight::STEP).round() as usize {
            flight.advance(openfsx::flight::STEP)?;
        }
        println!("{}", flight.title()?);
        flight_view(flight, &mut orbit);
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
            gpu.hud = Some(openfsx::hud::Readout::new(
                flight.simulation.telemetry()?,
                flight.simulation.controls,
                flight.paused,
            )?);
        }
        return gpu.snapshot(&orbit, &path);
    }
    let mut app = App {
        geometry,
        appearance,
        orbit,
        window: None,
        gpu: None,
        dragging: false,
        cursor: None,
        error: None,
        flight,
        last_frame: Instant::now(),
        last_title: Instant::now() - Duration::from_secs(1),
        visible: true,
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
