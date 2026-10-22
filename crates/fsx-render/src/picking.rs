//! CPU surface picking using the current renderer camera, posed triangles and UVs.
//! No gauge semantics or native execution belong here.
use crate::{Appearance, Orbit};
use fsx_geometry::Mat4;
use fsx_model::Geometry;

#[derive(Debug, Clone)]
pub struct SurfaceHit {
    pub batch: usize,
    pub triangle_vertex: u32,
    pub material: usize,
    pub texture: Option<usize>,
    pub uv: [f64; 2],
    pub sample_uv: [f64; 2],
    pub position: [f64; 3],
    /// Fraction along the model-space near-to-far segment, not depth-buffer Z.
    pub segment: f64,
}
type Matrix = [[f64; 4]; 4];
fn matrix(orbit: &Orbit, model: Mat4, viewport: [u32; 2]) -> Result<Matrix, String> {
    if viewport.contains(&0)
        || viewport.iter().any(|&v| v > 32768)
        || model.iter().any(|v| !v.is_finite())
    {
        return Err("Invalid picking viewport/model transform".into());
    }
    let camera = orbit.matrix(viewport[0] as f32 / viewport[1] as f32);
    if camera.iter().any(|v| !v.is_finite()) {
        return Err("Invalid picking camera".into());
    }
    Ok(std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            (0..4)
                .map(|k| f64::from(camera[k * 4 + r]) * f64::from(model[c * 4 + k]))
                .sum()
        })
    }))
}
fn multiply(m: Matrix, p: [f64; 4]) -> [f64; 4] {
    std::array::from_fn(|r| (0..4).map(|c| m[r][c] * p[c]).sum())
}
fn inverse(m: Matrix) -> Result<Matrix, String> {
    let mut rows: [[f64; 8]; 4] = std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            if c < 4 {
                m[r][c]
            } else {
                f64::from(c - 4 == r)
            }
        })
    });
    for c in 0..4 {
        let pivot = (c..4)
            .max_by(|&a, &b| rows[a][c].abs().total_cmp(&rows[b][c].abs()))
            .unwrap();
        if rows[pivot][c].abs() < 1e-15 {
            return Err("Singular picking transform".into());
        }
        rows.swap(c, pivot);
        let scale = rows[c][c];
        for v in &mut rows[c] {
            *v /= scale;
        }
        for r in 0..4 {
            if r != c {
                let scale = rows[r][c];
                for k in 0..8 {
                    rows[r][k] -= scale * rows[c][k];
                }
            }
        }
    }
    let result = std::array::from_fn(|r| std::array::from_fn(|c| rows[r][c + 4]));
    if result.iter().flatten().any(|v| !v.is_finite()) {
        return Err("Picking inverse overflow".into());
    }
    Ok(result)
}
fn divide(p: [f64; 4]) -> Result<[f64; 3], String> {
    if p.iter().any(|v| !v.is_finite()) || p[3].abs() < 1e-15 {
        return Err("Picking homogeneous coordinate overflow".into());
    }
    let out = std::array::from_fn(|i| p[i] / p[3]);
    if out.iter().any(|v| !v.is_finite()) {
        return Err("Picking coordinate overflow".into());
    }
    Ok(out)
}
/// Project a model point into physical viewport coordinates (top-left origin).
/// Returns None outside the viewport or the renderer's Direct3D 0..1 depth clip.
pub fn project(
    orbit: &Orbit,
    model: Mat4,
    viewport: [u32; 2],
    point: [f64; 3],
) -> Result<Option<[f64; 2]>, String> {
    if point.iter().any(|v| !v.is_finite()) {
        return Err("Nonfinite picking model point".into());
    }
    let p = multiply(
        matrix(orbit, model, viewport)?,
        [point[0], point[1], point[2], 1.],
    );
    if p[3] <= 0. {
        return Ok(None);
    }
    let ndc = divide(p)?;
    if ndc[0] < -1. || ndc[0] >= 1. || ndc[1] <= -1. || ndc[1] > 1. || !(0. ..1.).contains(&ndc[2])
    {
        return Ok(None);
    }
    Ok(Some([
        (ndc[0] + 1.) * 0.5 * f64::from(viewport[0]),
        (1. - ndc[1]) * 0.5 * f64::from(viewport[1]),
    ]))
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn triangle(origin: [f64; 3], direction: [f64; 3], p: [[f64; 3]; 3]) -> Option<[f64; 3]> {
    let e1 = sub(p[1], p[0]);
    let e2 = sub(p[2], p[0]);
    let h = cross(direction, e2);
    let det = dot(e1, h);
    let scale = (dot(e1, e1) * dot(e2, e2) * dot(direction, direction)).sqrt();
    if !scale.is_finite() || det.abs() <= scale * 1e-12 {
        return None;
    }
    let s = sub(origin, p[0]);
    let u = dot(s, h) / det;
    if !(0. ..=1.).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(direction, q) / det;
    if v < 0. || u + v > 1. {
        return None;
    }
    let t = dot(e2, q) / det;
    if !(0. ..1.).contains(&t) {
        return None;
    }
    Some([t, u, v])
}
fn address(uv: [f64; 2], clamp: bool) -> [f64; 2] {
    uv.map(|v| {
        if clamp {
            v.clamp(0., 1.)
        } else {
            v.rem_euclid(1.)
        }
    })
}
fn alpha(image: &fsx_texture::Image, uv: [f64; 2], clamp: bool) -> Result<f64, String> {
    if image.width == 0
        || image.height == 0
        || image.rgba.len() != image.width as usize * image.height as usize * 4
    {
        return Err("Invalid picking texture image".into());
    }
    let p = [
        uv[0] * f64::from(image.width) - 0.5,
        uv[1] * f64::from(image.height) - 0.5,
    ];
    let floor = p.map(f64::floor);
    let fraction = [p[0] - floor[0], p[1] - floor[1]];
    let index = |v: f64, size: u32| {
        if clamp {
            (v as i64).clamp(0, i64::from(size) - 1) as usize
        } else {
            (v as i64).rem_euclid(i64::from(size)) as usize
        }
    };
    let mut result = 0.;
    for y in 0..2 {
        for x in 0..2 {
            let px = index(floor[0] + x as f64, image.width);
            let py = index(floor[1] + y as f64, image.height);
            let w = if x == 0 {
                1. - fraction[0]
            } else {
                fraction[0]
            } * if y == 0 {
                1. - fraction[1]
            } else {
                fraction[1]
            };
            result += w * f64::from(image.rgba[(py * image.width as usize + px) * 4 + 3]) / 255.;
        }
    }
    Ok(result)
}
fn survives(function: u32, value: f64, reference: f64) -> bool {
    match function {
        1 => false,
        2 => value < reference,
        3 => (value - reference).abs() < 0.001,
        4 => value <= reference,
        5 => value > reference,
        6 => (value - reference).abs() >= 0.001,
        7 => value >= reference,
        _ => true,
    }
}
/// Nearest rendered surface; opaque/nonzero-alpha foreground geometry blocks
/// input. Hidden batches, alpha-test discards and zero-alpha blends are ignored.
/// Matches the current renderer's two-sided triangles and single-mip sampling.
pub fn surface(
    geometry: &Geometry,
    appearance: &Appearance,
    orbit: &Orbit,
    model: Mat4,
    viewport: [u32; 2],
    point: [f64; 2],
) -> Result<Option<SurfaceHit>, String> {
    let m = matrix(orbit, model, viewport)?;
    if point.iter().any(|v| !v.is_finite()) {
        return Err("Nonfinite screen picking point".into());
    }
    if point[0] < 0.
        || point[1] < 0.
        || point[0] >= f64::from(viewport[0])
        || point[1] >= f64::from(viewport[1])
    {
        return Ok(None);
    }
    if geometry.vertices.len() > 3_000_000 {
        return Err("Picking geometry exceeds vertex budget".into());
    }
    let inverse = inverse(m)?;
    let xy = [
        point[0] * 2. / f64::from(viewport[0]) - 1.,
        1. - point[1] * 2. / f64::from(viewport[1]),
    ];
    let start = divide(multiply(inverse, [xy[0], xy[1], 0., 1.]))?;
    let end = divide(multiply(inverse, [xy[0], xy[1], 1., 1.]))?;
    let direction = sub(end, start);
    let mut nearest: Option<SurfaceHit> = None;
    for (batch_index, batch) in geometry
        .batches
        .iter()
        .enumerate()
        .filter(|(_, b)| b.visible)
    {
        let vertices = geometry
            .vertices
            .get(batch.range.start as usize..batch.range.end as usize)
            .ok_or("Picking draw range exceeds geometry")?;
        if vertices.len() % 3 != 0 {
            return Err("Picking draw does not contain complete triangles".into());
        }
        let material = appearance
            .materials
            .get(batch.material)
            .ok_or("Picking material index out of range")?;
        let image = match material.diffuse_texture {
            Some(index) => appearance
                .images
                .get(index)
                .ok_or("Picking texture index out of range")?
                .as_ref(),
            None => None,
        };
        for (triangle_index, vertices) in vertices.chunks_exact(3).enumerate() {
            if vertices.iter().flatten().any(|v| !v.is_finite()) {
                return Err("Nonfinite picking geometry".into());
            }
            let p = std::array::from_fn(|i| std::array::from_fn(|j| f64::from(vertices[i][j])));
            let Some([t, u, v]) = triangle(start, direction, p) else {
                continue;
            };
            if nearest.as_ref().is_some_and(|hit| {
                hit.segment < t
                    || (hit.segment == t
                        && (!appearance.materials[hit.material].alpha_blend
                            || material.alpha_blend))
            }) {
                continue;
            }
            let uv = std::array::from_fn(|j| {
                (1. - u - v) * f64::from(vertices[0][j + 6])
                    + u * f64::from(vertices[1][j + 6])
                    + v * f64::from(vertices[2][j + 6])
            });
            let sample_uv = address(uv, material.clamp_texture);
            let a = image
                .map(|image| alpha(image, sample_uv, material.clamp_texture))
                .transpose()?;
            let test = a.unwrap_or(f64::from(material.diffuse[3]));
            if !survives(
                material.alpha_function,
                test,
                f64::from(material.alpha_threshold),
            ) || (material.alpha_blend && a.unwrap_or(1.) * f64::from(material.diffuse[3]) <= 0.)
            {
                continue;
            }
            nearest = Some(SurfaceHit {
                batch: batch_index,
                triangle_vertex: batch.range.start + triangle_index as u32 * 3,
                material: batch.material,
                texture: material.diffuse_texture,
                uv,
                sample_uv,
                position: std::array::from_fn(|i| start[i] + t * direction[i]),
                segment: t,
            });
        }
    }
    Ok(nearest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Camera;
    use fsx_geometry::IDENTITY;
    use fsx_model::{DrawBatch, Material};
    fn scene() -> (Geometry, Appearance, Orbit) {
        let vertices = vec![
            [-1., -1., 2., 0., 0., 1., 0., 1.],
            [1., -1., 2., 0., 0., 1., 1., 1.],
            [0., 1., 2., 0., 0., 1., 0.5, 0.],
        ];
        let geometry = Geometry {
            vertices,
            batches: vec![DrawBatch {
                scene: 0,
                visible: true,
                range: 0..3,
                material: 0,
                center: [0., 0., 2.],
            }],
            min: [-1., -1., 2.],
            max: [1., 1., 2.],
            skipped_skinned_parts: 0,
            lod: 0,
        };
        let appearance = Appearance {
            materials: vec![Material {
                diffuse: [1.; 4],
                diffuse_texture: Some(0),
                alpha_blend: false,
                alpha_function: 0,
                alpha_threshold: 0.5,
                clamp_texture: false,
            }],
            images: vec![Some(fsx_texture::Image {
                width: 1,
                height: 1,
                rgba: vec![255; 4],
            })],
            texture_names: vec!["panel".into()],
        };
        let mut orbit = Orbit::new(&geometry);
        orbit.camera =
            Some(Camera::new([0.; 3], [0., 0., 1.], [0., 1., 0.], 0.1, 10., 60.).unwrap());
        (geometry, appearance, orbit)
    }
    #[test]
    fn screen_ray_preserves_perspective_uv_and_model_camera_transforms() {
        let (geometry, appearance, mut orbit) = scene();
        let mut transform = IDENTITY;
        transform[12] = 30.;
        transform[13] = 7.;
        transform[14] = 20.;
        orbit.camera =
            Some(Camera::new([30., 7., 20.], [30., 7., 21.], [0., 1., 0.], 0.1, 10., 60.).unwrap());
        for size in [[800, 600], [1600, 1200], [1280, 720]] {
            let screen = project(&orbit, transform, size, [0.2, -0.1, 2.])
                .unwrap()
                .unwrap();
            let hit = surface(&geometry, &appearance, &orbit, transform, size, screen)
                .unwrap()
                .unwrap();
            assert!((hit.uv[0] - 0.6).abs() < 1e-6 && (hit.uv[1] - 0.55).abs() < 1e-6);
            for (a, b) in hit.position.into_iter().zip([0.2, -0.1, 2.]) {
                assert!((a - b).abs() < 1e-6);
            }
        }
        assert!(project(&orbit, transform, [800, 600], [0., 0., 0.01])
            .unwrap()
            .is_none());
        assert!(project(&orbit, transform, [800, 600], [0., 0., 11.])
            .unwrap()
            .is_none());
        assert!(project(&orbit, transform, [800, 600], [0., 0., -2.])
            .unwrap()
            .is_none());
        let mut tilted = geometry;
        tilted.vertices[1][2] = 3.;
        tilted.vertices[2][2] = 4.;
        let screen = project(&orbit, transform, [800, 600], [0.1, 0., 3.3])
            .unwrap()
            .unwrap();
        let hit = surface(&tilted, &appearance, &orbit, transform, [800, 600], screen)
            .unwrap()
            .unwrap();
        assert!((hit.uv[0] - 0.55).abs() < 1e-6 && (hit.uv[1] - 0.5).abs() < 1e-6);
        tilted.vertices.swap(0, 1);
        let reverse = surface(&tilted, &appearance, &orbit, transform, [800, 600], screen)
            .unwrap()
            .unwrap();
        assert!(
            (reverse.uv[0] - hit.uv[0]).abs() < 1e-6 && (reverse.uv[1] - hit.uv[1]).abs() < 1e-6
        );
    }
    #[test]
    fn nearer_geometry_blocks_and_hidden_discarded_transparent_geometry_does_not() {
        let (mut geometry, mut appearance, orbit) = scene();
        let front: Vec<_> = geometry
            .vertices
            .iter()
            .map(|v| {
                let mut v = *v;
                v[2] = 1.;
                v
            })
            .collect();
        geometry.vertices.extend(front);
        geometry.batches.push(DrawBatch {
            scene: 1,
            visible: true,
            range: 3..6,
            material: 1,
            center: [0., 0., 1.],
        });
        appearance.materials.push(appearance.materials[0].clone());
        appearance.materials[1].diffuse_texture = None;
        let pick = |g: &Geometry, a: &Appearance| {
            surface(g, a, &orbit, IDENTITY, [800, 600], [400., 300.])
                .unwrap()
                .unwrap()
                .batch
        };
        assert_eq!(pick(&geometry, &appearance), 1);
        geometry.batches[1].visible = false;
        assert_eq!(pick(&geometry, &appearance), 0);
        geometry.batches[1].visible = true;
        appearance.materials[1].alpha_function = 1;
        assert_eq!(pick(&geometry, &appearance), 0);
        appearance.materials[1].alpha_function = 0;
        appearance.materials[1].alpha_blend = true;
        appearance.materials[1].diffuse[3] = 0.;
        assert_eq!(pick(&geometry, &appearance), 0);
        appearance.materials[1].diffuse[3] = 0.5;
        assert_eq!(pick(&geometry, &appearance), 1);
        appearance.materials[1].alpha_blend = false;
        appearance.materials[1].diffuse[3] = 0.;
        assert_eq!(pick(&geometry, &appearance), 1);
        for vertex in &mut geometry.vertices[3..] {
            vertex[2] = 2.;
        }
        appearance.materials[1].alpha_blend = true;
        appearance.materials[1].diffuse[3] = 0.5;
        geometry.batches.swap(0, 1);
        assert_eq!(pick(&geometry,&appearance),1,"Opaque pass must win equal-depth ties even when blended geometry precedes it in model order");
    }
    #[test]
    fn alpha_sampling_matches_linear_addressing_and_shader_tests() {
        let image = fsx_texture::Image {
            width: 2,
            height: 1,
            rgba: vec![0, 0, 0, 0, 0, 0, 0, 255],
        };
        assert_eq!(alpha(&image, [0.5, 0.5], false).unwrap(), 0.5);
        assert_eq!(alpha(&image, [0., 0.5], true).unwrap(), 0.);
        assert_eq!(alpha(&image, [1., 0.5], true).unwrap(), 1.);
        assert_eq!(alpha(&image, [0., 0.5], false).unwrap(), 0.5);
        assert_eq!(address([-0.25, 1.25], false), [0.75, 0.25]);
        assert_eq!(address([-0.25, 1.25], true), [0., 1.]);
        for (function, pass) in [
            (0, true),
            (1, false),
            (2, false),
            (3, true),
            (4, true),
            (5, false),
            (6, false),
            (7, true),
            (8, true),
        ] {
            assert_eq!(survives(function, 0.5, 0.5), pass);
        }
    }
    #[test]
    fn picker_rejects_malformed_ranges_nonfinite_values_and_singular_projection() {
        let (mut geometry, appearance, orbit) = scene();
        assert!(surface(&geometry, &appearance, &orbit, IDENTITY, [0, 600], [0., 0.]).is_err());
        assert!(surface(
            &geometry,
            &appearance,
            &orbit,
            IDENTITY,
            [800, 600],
            [f64::NAN, 0.]
        )
        .is_err());
        assert!(surface(
            &geometry,
            &appearance,
            &orbit,
            IDENTITY,
            [800, 600],
            [-1., 0.]
        )
        .unwrap()
        .is_none());
        assert!(surface(
            &geometry,
            &appearance,
            &orbit,
            [0.; 16],
            [800, 600],
            [400., 300.]
        )
        .is_err());
        geometry.batches[0].range = 0..4;
        assert!(surface(
            &geometry,
            &appearance,
            &orbit,
            IDENTITY,
            [800, 600],
            [400., 300.]
        )
        .is_err());
        geometry.batches[0].range = 0..3;
        geometry.vertices[0][0] = f32::NAN;
        assert!(surface(
            &geometry,
            &appearance,
            &orbit,
            IDENTITY,
            [800, 600],
            [400., 300.]
        )
        .is_err());
    }
}
