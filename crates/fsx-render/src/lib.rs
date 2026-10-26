//! wgpu aircraft rendering and reproducible offscreen capture.
use fsx_geometry::{self as math, Mat4};
use fsx_model::{Geometry, Material, Model};
use fsx_texture::{self as texture, Image};
use std::{
    future::Future,
    path::Path,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};
use wgpu::util::DeviceExt;
use winit::window::Window;
pub mod hud;
pub mod native_text;
pub mod panel;
pub mod picking;
pub mod xml_text;
// GPU initialization and map_async don't need a general-purpose async runtime.
pub fn block_on<T>(future: impl Future<Output = T>) -> T {
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

pub struct Appearance {
    materials: Vec<Material>,
    images: Vec<Option<Image>>,
    texture_names: Vec<String>,
}
impl Appearance {
    /// Supply a generated texture for an exact model texture slot before GPU upload.
    pub fn bind_image(&mut self, name: &str, image: Image) -> Result<(), String> {
        if image.width == 0
            || image.height == 0
            || image.width > texture::MAX_DIMENSION
            || image.height > texture::MAX_DIMENSION
            || image.rgba.len() != image.width as usize * image.height as usize * 4
        {
            return Err("Invalid generated texture dimensions or RGBA length".into());
        }
        let matches: Vec<_> = self
            .texture_names
            .iter()
            .enumerate()
            .filter(|(_, t)| t.eq_ignore_ascii_case(name))
            .map(|(i, _)| i)
            .collect();
        if matches.len() != 1 || self.images.len() != self.texture_names.len() {
            return Err(format!("Expected one model texture slot for {name}"));
        }
        let index = matches[0];
        let memory: usize = self
            .images
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .filter_map(|(_, v)| v.as_ref())
            .map(|v| v.rgba.len())
            .sum();
        if memory + image.rgba.len() > 256 * 1024 * 1024 {
            return Err("Generated texture exceeds appearance memory budget".into());
        }
        self.images[index] = Some(image);
        Ok(())
    }
    pub fn load(model: &Model, geometry: &Geometry, folder: Option<&Path>) -> Result<Self, String> {
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
                    let image = Image::load(&path)?;
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
            texture_names: model.textures.clone(),
        })
    }
}

#[derive(Clone, Copy)]
pub struct Camera {
    eye: [f32; 3],
    target: [f32; 3],
    up: [f32; 3],
    near: f32,
    far: f32,
    fov: f32,
}
impl Camera {
    pub fn new(
        eye: [f32; 3],
        target: [f32; 3],
        up: [f32; 3],
        near: f32,
        far: f32,
        fov: f32,
    ) -> Result<Self, String> {
        let forward = math::sub(target, eye);
        let cross = math::cross(up, forward);
        let norms = [
            math::dot(forward, forward),
            math::dot(up, up),
            math::dot(cross, cross),
        ];
        if eye
            .iter()
            .chain(target.iter())
            .chain(up.iter())
            .chain([near, far, fov].iter())
            .any(|v| !v.is_finite())
            || near <= 0.
            || far <= near
            || !(1. ..179.).contains(&fov)
            || math::dot(forward, forward) < 1e-10
            || math::dot(up, up) < 1e-10
            || math::dot(cross, cross) < 1e-10
            || norms.iter().any(|n| !n.is_finite())
        {
            return Err("Invalid attached camera basis or projection".into());
        }
        let camera = Self {
            eye,
            target,
            up,
            near,
            far,
            fov,
        };
        if camera.matrix(1.).iter().any(|v| !v.is_finite()) {
            return Err("Camera matrix exceeds finite projection range".into());
        }
        Ok(camera)
    }
    pub fn eye(&self) -> [f32; 3] {
        self.eye
    }
    pub fn up(&self) -> [f32; 3] {
        self.up
    }
    fn matrix(&self, aspect: f32) -> Mat4 {
        math::camera_with_up(
            self.eye,
            self.target,
            self.up,
            aspect,
            self.near,
            self.far,
            self.fov,
        )
    }
}
pub struct Orbit {
    pub camera: Option<Camera>,
    pub center: [f32; 3],
    pub radius: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub far: f32,
}
impl Orbit {
    pub fn new(g: &Geometry) -> Self {
        let center = std::array::from_fn(|i| (g.min[i] + g.max[i]) * 0.5);
        let radius = math::dot(math::sub(g.max, g.min), math::sub(g.max, g.min))
            .sqrt()
            .max(0.1)
            * 0.5;
        Self {
            camera: None,
            center,
            radius,
            yaw: 0.7,
            pitch: 0.3,
            distance: radius * 3.4,
            far: radius * 100.,
        }
    }
    fn matrix(&self, aspect: f32) -> Mat4 {
        if let Some(camera) = self.camera {
            return camera.matrix(aspect);
        }
        math::camera(
            self.eye(),
            self.center,
            aspect,
            self.radius * 0.01,
            self.far,
        )
    }
    fn eye(&self) -> [f32; 3] {
        if let Some(camera) = self.camera {
            return camera.eye();
        }
        [
            self.center[0] + self.distance * self.yaw.sin() * self.pitch.cos(),
            self.center[1] + self.distance * self.pitch.sin(),
            self.center[2] + self.distance * self.yaw.cos() * self.pitch.cos(),
        ]
    }
    pub fn zoom(&mut self, delta: f32) {
        self.distance =
            (self.distance * (-delta * 0.12).exp()).clamp(self.radius * 1.1, self.radius * 30.);
    }
}
pub struct Gpu {
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
    material_layout: wgpu::BindGroupLayout,
    material_binds: Vec<wgpu::BindGroup>,
    textures: Vec<Option<wgpu::Texture>>,
    texture_names: Vec<String>,
    draws: Vec<GpuDraw>,
    pub model_matrix: Mat4,
    flight: bool,
    ground: Option<Ground>,
    pub hud: Option<crate::hud::Readout>,
    hud_pipeline: wgpu::RenderPipeline,
    hud_vertex: wgpu::Buffer,
    presented: bool,
    aircraft_vertices: usize,
    aircraft_draws: usize,
}
/// Airport runways in world metres: x east, z north, relative to the flight origin.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ground {
    pub runways: Vec<GroundRunway>,
    pub terrain: Option<Terrain>,
}
/// Regular elevation grid in world metres around the flight origin.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Terrain {
    pub columns: usize,
    pub rows: usize,
    /// East coordinate of column 0 and north coordinate of row 0.
    pub west: f32,
    pub north: f32,
    pub spacing: f32,
    /// Heights above the flight origin plane, row-major from the north edge.
    pub heights: Vec<f32>,
    /// Mean-sea-level elevation of the origin plane.
    pub base: f32,
}
impl Terrain {
    /// Bilinear height above the origin plane; None outside the grid.
    pub fn height_at(&self, east: f32, north: f32) -> Option<f32> {
        let x = (east - self.west) / self.spacing;
        let y = (self.north - north) / self.spacing;
        if !(x >= 0. && y >= 0. && x <= (self.columns - 1) as f32 && y <= (self.rows - 1) as f32) {
            return None;
        }
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.columns - 1), (y0 + 1).min(self.rows - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |c: usize, r: usize| self.heights[r * self.columns + c];
        let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * fx;
        let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * fx;
        Some(top + (bottom - top) * fy)
    }
}
/// Elevation colours: sea, lowland green, forest, dry grass, rock, snow.
fn terrain_gradient() -> Image {
    let stops: [(f32, [f32; 3]); 6] = [
        (0., [0.30, 0.42, 0.22]),
        (300., [0.24, 0.36, 0.18]),
        (900., [0.30, 0.34, 0.20]),
        (1700., [0.42, 0.38, 0.30]),
        (2600., [0.50, 0.48, 0.46]),
        (3400., [0.93, 0.94, 0.96]),
    ];
    let mut rgba = Vec::with_capacity(256 * 4);
    for i in 0..256 {
        let c = if i == 0 {
            [0.13, 0.27, 0.42]
        } else {
            let h = (i as f32 - 1.) / 254. * 4500.;
            let k = stops.iter().rposition(|(s, _)| *s <= h).unwrap_or(0);
            let (a, b) = (stops[k], stops[(k + 1).min(stops.len() - 1)]);
            let t = if b.0 > a.0 {
                ((h - a.0) / (b.0 - a.0)).clamp(0., 1.)
            } else {
                0.
            };
            std::array::from_fn(|j| a.1[j] + (b.1[j] - a.1[j]) * t)
        };
        rgba.extend(c.map(|x| (x * 255.) as u8));
        rgba.push(255);
    }
    Image {
        width: 256,
        height: 1,
        rgba,
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroundRunway {
    pub east: f32,
    pub north: f32,
    /// True heading of the primary end, degrees.
    pub heading: f32,
    pub length: f32,
    pub width: f32,
    pub hard: bool,
}
struct UploadedScene {
    vertex: wgpu::Buffer,
    materials: Vec<wgpu::BindGroup>,
    draws: Vec<GpuDraw>,
    textures: Vec<Option<wgpu::Texture>>,
    texture_names: Vec<String>,
}
struct GpuDraw {
    visible: bool,
    range: std::ops::Range<u32>,
    material: usize,
    pub center: [f32; 3],
    blended: bool,
}
impl Gpu {
    pub async fn new(
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
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../assets/shaders/viewer.wgsl").into(),
            ),
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
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../assets/shaders/hud.wgsl").into(),
            ),
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
        let scene = Self::upload_scene(
            &device,
            &queue,
            &material_layout,
            geometry,
            appearance,
            flight,
            None,
        )?;
        let depth = Self::depth(&device, width, height);
        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            blend_pipeline,
            vertex: scene.vertex,
            uniform,
            bind,
            depth,
            material_layout,
            material_binds: scene.materials,
            textures: scene.textures,
            texture_names: scene.texture_names,
            draws: scene.draws,
            model_matrix: math::IDENTITY,
            flight,
            ground: None,
            hud: None,
            hud_pipeline,
            hud_vertex,
            presented: false,
            aircraft_vertices: geometry.vertices.len(),
            aircraft_draws: geometry.batches.len(),
        })
    }
    fn upload_scene(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        material_layout: &wgpu::BindGroupLayout,
        geometry: &Geometry,
        appearance: &Appearance,
        flight: bool,
        ground: Option<&Ground>,
    ) -> Result<UploadedScene, String> {
        if geometry.batches.iter().any(|b| {
            b.material >= appearance.materials.len()
                || b.range.start > b.range.end
                || b.range.end as usize > geometry.vertices.len()
        }) || appearance.materials.iter().any(|m| {
            m.diffuse_texture
                .is_some_and(|i| i >= appearance.images.len())
        }) {
            return Err("Scene references an unavailable material, texture or vertex range".into());
        }
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
            texture
        };
        let white_texture = upload(&Image {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        });
        let white = white_texture.create_view(&Default::default());
        let mut textures = appearance
            .images
            .iter()
            .map(|image| image.as_ref().map(upload))
            .collect::<Vec<_>>();
        let gradient = textures.len();
        textures.push(Some(upload(&terrain_gradient())));
        let views = textures
            .iter()
            .map(|texture| texture.as_ref().map(|t| t.create_view(&Default::default())))
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
                [0.30, 0.33, 0.20, 1.],
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
            materials.push(Material {
                diffuse: [1.; 4],
                diffuse_texture: Some(gradient),
                alpha_blend: false,
                alpha_function: 0,
                alpha_threshold: 0.,
                clamp_texture: true,
            });
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
                visible: batch.visible,
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
            let quad =
                |vertices: &mut Vec<[f32; 9]>, x0: f32, z0: f32, x1: f32, z1: f32, y: f32| {
                    for [x, z] in [[x0, z0], [x1, z0], [x1, z1], [x0, z0], [x1, z1], [x0, z1]] {
                        vertices.push([x, y, z, 0., 1., 0., 0., 0., 0.]);
                    }
                };
            let start = geometry.vertices.len() as u32;
            // With terrain the surrounding plane sits at sea level as water.
            let plane = ground
                .and_then(|g| g.terrain.as_ref())
                .map_or(0., |t| -t.base - 1.);
            quad(&mut vertices, -30000., -30000., 30000., 30000., plane);
            let mut count = 6;
            draws.push(GpuDraw {
                visible: true,
                range: start..start + count,
                material: ground_material,
                center: [0.; 3],
                blended: false,
            });
            if let Some(t) = ground.and_then(|g| g.terrain.as_ref()) {
                let first = vertices.len() as u32;
                let at = |c: usize, r: usize| t.heights[r * t.columns + c];
                let vertex = |c: usize, r: usize| {
                    let (x, z) = (
                        t.west + c as f32 * t.spacing,
                        t.north - r as f32 * t.spacing,
                    );
                    let h = at(c, r);
                    let dx = at((c + 1).min(t.columns - 1), r) - at(c.saturating_sub(1), r);
                    let dz = at(c, r.saturating_sub(1)) - at(c, (r + 1).min(t.rows - 1));
                    let n = [-dx, 4. * t.spacing, -dz];
                    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                    let absolute = h + t.base;
                    let u = if absolute <= 0.5 {
                        0.5 / 256.
                    } else {
                        (1.5 + absolute / 4500. * 254.) / 256.
                    };
                    [x, h, z, n[0] / l, n[1] / l, n[2] / l, u.min(1.), 0.5, 0.]
                };
                for r in 0..t.rows - 1 {
                    for c in 0..t.columns - 1 {
                        for (dc, dr) in [(0, 0), (1, 1), (1, 0), (0, 0), (0, 1), (1, 1)] {
                            vertices.push(vertex(c + dc, r + dr));
                        }
                    }
                }
                draws.push(GpuDraw {
                    visible: true,
                    range: first..vertices.len() as u32,
                    material: ground_material + 5,
                    center: [0.; 3],
                    blended: false,
                });
            }
            if let Some(ground) = ground {
                let mut draw =
                    |vertices: &mut Vec<[f32; 9]>,
                     material,
                     add: &mut dyn FnMut(&mut Vec<[f32; 9]>)| {
                        let first = vertices.len() as u32;
                        add(vertices);
                        draws.push(GpuDraw {
                            visible: true,
                            range: first..vertices.len() as u32,
                            material,
                            center: [0.; 3],
                            blended: false,
                        });
                    };
                // Oriented rectangle: offsets along/across the runway axis.
                let rect = |v: &mut Vec<[f32; 9]>,
                            r: &GroundRunway,
                            a0: f32,
                            a1: f32,
                            c0: f32,
                            c1: f32,
                            y: f32| {
                    let h = r.heading.to_radians();
                    let (along, across) = ([h.sin(), h.cos()], [h.cos(), -h.sin()]);
                    let at = |a: f32, c: f32| {
                        [
                            r.east + along[0] * a + across[0] * c,
                            r.north + along[1] * a + across[1] * c,
                        ]
                    };
                    let (p0, p1, p2, p3) = (at(a0, c0), at(a1, c0), at(a1, c1), at(a0, c1));
                    for [x, z] in [p0, p2, p1, p0, p3, p2] {
                        v.push([x, y, z, 0., 1., 0., 0., 0., 0.]);
                    }
                };
                for (hard, material) in [(true, ground_material + 1), (false, ground_material + 4)]
                {
                    draw(&mut vertices, material, &mut |v| {
                        for r in ground.runways.iter().filter(|r| r.hard == hard) {
                            let (l, w) = (r.length / 2., r.width / 2.);
                            rect(v, r, -l, l, -w, w, 0.02);
                        }
                    });
                }
                draw(&mut vertices, ground_material + 2, &mut |v| {
                    for r in ground.runways.iter().filter(|r| r.hard) {
                        let (l, w) = (r.length / 2., r.width / 2.);
                        // Centreline dashes, threshold bars and edge lines.
                        let mut a = -l + 60.;
                        while a + 30. < l - 60. {
                            rect(v, r, a, a + 30., -0.45, 0.45, 0.04);
                            a += 50.;
                        }
                        for end in [-1f32, 1.] {
                            let (a0, a1) = if end < 0. {
                                (-l + 6., -l + 36.)
                            } else {
                                (l - 36., l - 6.)
                            };
                            let bars = ((w * 2. - 6.) / 4.).floor().clamp(2., 16.) as i32;
                            for b in 0..bars {
                                let c = -w + 3. + (b as f32 + 0.25) * (w * 2. - 6.) / bars as f32;
                                rect(v, r, a0, a1, c, c + 1.8, 0.04);
                            }
                        }
                        rect(v, r, -l, l, -w, -w + 0.9, 0.04);
                        rect(v, r, -l, l, w - 0.9, w, 0.04);
                    }
                });
            } else {
                quad(&mut vertices, -15., -500., 15., 1500., 0.02);
                draws.push(GpuDraw {
                    visible: true,
                    range: start + count..start + count + 6,
                    material: ground_material + 1,
                    center: [0.; 3],
                    blended: false,
                });
                count += 6;
                for i in 0..40 {
                    let z = -480. + i as f32 * 50.;
                    quad(&mut vertices, -0.7, z, 0.7, z + 25., 0.04);
                }
                draws.push(GpuDraw {
                    visible: true,
                    range: start + count..start + count + 240,
                    material: ground_material + 2,
                    center: [0.; 3],
                    blended: false,
                });
            }
            if ground.and_then(|g| g.terrain.as_ref()).is_none() {
                let count = vertices.len() as u32 - start;
                for i in -40..=40 {
                    let p = i as f32 * 200.;
                    quad(&mut vertices, p, -8000., p + 0.8, 8000., 0.01);
                    quad(&mut vertices, -8000., p, 8000., p + 0.8, 0.01);
                }
                draws.push(GpuDraw {
                    visible: true,
                    range: start + count..vertices.len() as u32,
                    material: ground_material + 3,
                    center: [0.; 3],
                    blended: false,
                });
            }
        }
        let vertex = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("MDLX geometry"),
            contents: &bytes(vertices.iter().flatten().copied()),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        Ok(UploadedScene {
            vertex,
            materials: material_binds,
            draws,
            textures,
            texture_names: appearance.texture_names.clone(),
        })
    }
    /// Replace the test runway with airport runways (None restores it).
    pub fn set_ground(
        &mut self,
        ground: Option<Ground>,
        geometry: &Geometry,
        appearance: &Appearance,
    ) -> Result<(), String> {
        self.ground = ground;
        self.replace_scene(geometry, appearance)
    }
    /// Replace model resources without recreating the device, surface or flight state.
    pub fn replace_scene(
        &mut self,
        geometry: &Geometry,
        appearance: &Appearance,
    ) -> Result<(), String> {
        let scene = Self::upload_scene(
            &self.device,
            &self.queue,
            &self.material_layout,
            geometry,
            appearance,
            self.flight,
            self.ground.as_ref(),
        )?;
        self.vertex = scene.vertex;
        self.material_binds = scene.materials;
        self.textures = scene.textures;
        self.texture_names = scene.texture_names;
        self.draws = scene.draws;
        self.aircraft_vertices = geometry.vertices.len();
        self.aircraft_draws = geometry.batches.len();
        Ok(())
    }
    /// Replace pixels in an already-uploaded texture, retaining geometry/material bindings.
    pub fn update_image(&mut self, name: &str, image: &Image) -> Result<(), String> {
        let matches: Vec<_> = self
            .texture_names
            .iter()
            .enumerate()
            .filter(|(_, n)| n.eq_ignore_ascii_case(name))
            .map(|(i, _)| i)
            .collect();
        if matches.len() != 1 {
            return Err("Dynamic image needs one current scene texture slot".into());
        }
        let texture = self.textures[matches[0]]
            .as_ref()
            .ok_or("Dynamic texture must be initialized before GPU upload")?;
        if image.width == 0
            || image.height == 0
            || image.width > texture::MAX_DIMENSION
            || image.height > texture::MAX_DIMENSION
            || texture.width() != image.width
            || texture.height() != image.height
            || image.rgba.len() != image.width as usize * image.height as usize * 4
        {
            return Err(
                "Dynamic image dimensions or RGBA extent differ from the uploaded texture".into(),
            );
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
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
        Ok(())
    }
    pub fn update_geometry(&mut self, geometry: &Geometry) -> Result<(), String> {
        if geometry.vertices.len() != self.aircraft_vertices
            || geometry.batches.len() != self.aircraft_draws
        {
            return Err("Animation changed geometry topology".into());
        }
        for (batch, draw) in geometry.batches.iter().zip(&self.draws) {
            if batch.range != draw.range || batch.material != draw.material {
                return Err("Animation changed draw layout".into());
            }
        }
        self.queue.write_buffer(
            &self.vertex,
            0,
            &bytes(
                geometry
                    .vertices
                    .iter()
                    .flat_map(|v| [v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], 1.]),
            ),
        );
        for (batch, draw) in geometry.batches.iter().zip(&mut self.draws) {
            draw.center = batch.center;
            draw.visible = batch.visible;
        }
        Ok(())
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
    pub fn resize(&mut self, width: u32, height: u32) {
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
            for draw in self
                .draws
                .iter()
                .filter(|draw| draw.visible && !draw.blended)
            {
                pass.set_bind_group(1, &self.material_binds[draw.material], &[]);
                pass.draw(draw.range.clone(), 0..1);
            }
            let mut blended = self
                .draws
                .iter()
                .filter(|draw| draw.visible && draw.blended)
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
            let vertices = crate::hud::vertices(readout, self.config.width, self.config.height);
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
    pub fn draw(&mut self, orbit: &Orbit) -> Result<(), String> {
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
    pub fn snapshot(&self, orbit: &Orbit, path: &Path) -> Result<(), String> {
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

#[cfg(test)]
mod camera_tests {
    use super::*;
    #[test]
    fn generated_texture_binding_is_case_insensitive_checked_and_transactional() {
        let mut appearance = Appearance {
            materials: Vec::new(),
            images: vec![None],
            texture_names: vec!["$Own".into()],
        };
        let image = || Image {
            width: 1,
            height: 1,
            rgba: vec![1, 2, 3, 255],
        };
        appearance.bind_image("$OWN", image()).unwrap();
        assert_eq!(appearance.images[0].as_ref().unwrap().rgba, [1, 2, 3, 255]);
        assert!(appearance.bind_image("$Missing", image()).is_err());
        assert!(appearance
            .bind_image(
                "$Own",
                Image {
                    width: 1,
                    height: 1,
                    rgba: vec![0; 3]
                }
            )
            .is_err());
        assert_eq!(appearance.images[0].as_ref().unwrap().rgba, [1, 2, 3, 255]);
    }
    #[test]
    fn attached_up_vector_rotates_the_screen_basis_and_rejects_degenerate_views() {
        let camera = Camera::new([0.; 3], [0., 0., -1.], [1., 0., 0.], 0.01, 20000., 45.).unwrap();
        let projected = math::transform(camera.matrix(1.), [1., 0., -1.]);
        assert!(projected[0].abs() < 1e-6 && projected[1] > 2.);
        for (target, up, near, far, fov) in [
            ([0.; 3], [0., 1., 0.], 0.01, 1000., 45.),
            ([0., 1., 0.], [0., 1., 0.], 0.01, 1000., 45.),
            ([0., 0., -1.], [0., 1., 0.], 0., 1000., 45.),
            ([0., 0., -1.], [0., 1., 0.], 1., 0.1, 45.),
            ([0., 0., -1.], [0., 1., 0.], 0.01, 1000., 180.),
            ([f32::MAX, 0., 0.], [0., 1., 0.], 0.01, 1000., 45.),
        ] {
            assert!(Camera::new([0.; 3], target, up, near, far, fov).is_err());
        }
    }
}
