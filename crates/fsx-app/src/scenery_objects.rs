//! Placed FSX library objects around an area, baked into world-space static
//! geometry. Models come from the installed BGL model libraries and are parsed
//! by fsx-model; pitch/bank, animation, LOD switching and night textures are
//! not applied.
use fsx_render::{StaticScene, TextureImage};
use fsx_scenery::objects::{Guid, ModelEntry, Placement};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const MAX_PLACEMENTS: usize = 4000;
const MAX_VERTICES: usize = 3_000_000;
const MAX_TEXTURE: u32 = 512;

/// Lower-case texture file name to path across the stock scenery texture folders.
pub fn texture_index(root: &Path) -> HashMap<String, PathBuf> {
    let mut out = HashMap::new();
    let mut folders = vec![root.join("Texture")];
    let mut stack = vec![root.join("Scenery")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("texture")
                {
                    folders.push(path.clone());
                }
                stack.push(path);
            }
        }
    }
    // Earlier folders win: global texture folders before area-specific ones.
    folders.sort_by_key(|f| !f.to_string_lossy().to_ascii_lowercase().contains("global"));
    for folder in folders {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            out.entry(entry.file_name().to_string_lossy().to_ascii_lowercase())
                .or_insert_with(|| entry.path());
        }
    }
    out
}
/// Box-filter an image down so neither side exceeds `max`.
fn shrink(mut image: TextureImage, max: u32) -> TextureImage {
    while image.width > max || image.height > max {
        let (w, h) = ((image.width / 2).max(1), (image.height / 2).max(1));
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                for k in 0..4 {
                    let at = |sx: u32, sy: u32| {
                        let (sx, sy) = (sx.min(image.width - 1), sy.min(image.height - 1));
                        u32::from(image.rgba[((sy * image.width + sx) * 4 + k) as usize])
                    };
                    let sum = at(2 * x, 2 * y)
                        + at(2 * x + 1, 2 * y)
                        + at(2 * x, 2 * y + 1)
                        + at(2 * x + 1, 2 * y + 1);
                    rgba.push(((sum + 2) / 4) as u8);
                }
            }
        }
        image = TextureImage {
            width: w,
            height: h,
            rgba,
        };
    }
    image
}
struct Loaded {
    vertices: Vec<[f32; 8]>,
    /// Vertex range and global material index.
    batches: Vec<(std::ops::Range<u32>, usize)>,
}
/// Bake placements into one static scene. `place` maps a placement to its
/// world position (east, up, north) or None to skip it.
pub fn build(
    placements: &[Placement],
    models: &HashMap<Guid, ModelEntry>,
    textures: &HashMap<String, PathBuf>,
    mut place: impl FnMut(&Placement) -> Option<[f32; 3]>,
) -> Option<StaticScene> {
    let mut scene = StaticScene::default();
    let mut loaded: HashMap<Guid, Option<Loaded>> = HashMap::new();
    let mut images: HashMap<String, Option<usize>> = HashMap::new();
    for p in placements.iter().take(MAX_PLACEMENTS) {
        let Some(position) = place(p) else { continue };
        let model = loaded.entry(p.model).or_insert_with(|| {
            let bytes = models.get(&p.model)?.read().ok()?;
            let model = fsx_model::Model::parse(&bytes).ok()?;
            let geometry = model.geometry(None).ok()?;
            let base = scene.materials.len();
            for m in &model.materials {
                let texture = m.diffuse_texture.and_then(|t| {
                    let name = model.textures.get(t)?.to_ascii_lowercase();
                    *images.entry(name.clone()).or_insert_with(|| {
                        let Some(path) = textures.get(&name) else {
                            eprintln!("Scenery texture not found: {name}");
                            return None;
                        };
                        let image = TextureImage::load(path)
                            .map_err(|e| eprintln!("Scenery texture {name}: {e}"))
                            .ok()?;
                        scene.images.push(Some(shrink(image, MAX_TEXTURE)));
                        Some(scene.images.len() - 1)
                    })
                });
                scene.materials.push(fsx_render::ModelMaterial {
                    diffuse_texture: texture,
                    ..m.clone()
                });
            }
            Some(Loaded {
                batches: geometry
                    .batches
                    .iter()
                    .filter(|b| b.visible)
                    .map(|b| (b.range.clone(), base + b.material))
                    .collect(),
                vertices: geometry.vertices,
            })
        });
        let Some(model) = model else { continue };
        if scene.vertices.len() + model.vertices.len() > MAX_VERTICES {
            break;
        }
        // Heading turns clockwise from north about the vertical axis.
        let (sin, cos) = (p.heading.to_radians() as f32).sin_cos();
        let s = p.scale as f32;
        let first = scene.vertices.len() as u32;
        for v in &model.vertices {
            let (x, z) = (v[0] * s, v[2] * s);
            let (nx, nz) = (v[3], v[5]);
            scene.vertices.push([
                position[0] + x * cos + z * sin,
                position[1] + v[1] * s,
                position[2] - x * sin + z * cos,
                nx * cos + nz * sin,
                v[4],
                -nx * sin + nz * cos,
                v[6],
                v[7],
            ]);
        }
        for (range, material) in &model.batches {
            scene
                .batches
                .push((first + range.start..first + range.end, *material));
        }
    }
    let textured = scene.images.iter().filter(|i| i.is_some()).count();
    println!(
        "Scenery objects: {} placements, {}/{} models parsed, {} textures, {} vertices",
        placements.len().min(MAX_PLACEMENTS),
        loaded.values().filter(|m| m.is_some()).count(),
        loaded.len(),
        textured,
        scene.vertices.len()
    );
    (!scene.batches.is_empty()).then_some(scene)
}
