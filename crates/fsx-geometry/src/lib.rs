//! Column-major matrices, shared by content transforms and the renderer.
pub type Mat4 = [f32; 16];
pub const IDENTITY: Mat4 = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

pub fn multiply(a: Mat4, b: Mat4) -> Mat4 {
    let mut out = [0.; 16];
    for c in 0..4 {
        for r in 0..4 {
            for k in 0..4 {
                out[c * 4 + r] += a[k * 4 + r] * b[c * 4 + k];
            }
        }
    }
    out
}

pub fn transform(m: Mat4, p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r])
}

pub fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
pub fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
pub fn normalized(v: [f32; 3]) -> [f32; 3] {
    let n = dot(v, v).sqrt();
    if n < 1e-10 {
        [0., 1., 0.]
    } else {
        v.map(|x| x / n)
    }
}

pub fn camera(eye: [f32; 3], target: [f32; 3], aspect: f32, near: f32, far: f32) -> Mat4 {
    let z = normalized(sub(eye, target));
    let x = normalized(cross([0., 1., 0.], z));
    let y = cross(z, x);
    let view = [
        x[0],
        y[0],
        z[0],
        0.,
        x[1],
        y[1],
        z[1],
        0.,
        x[2],
        y[2],
        z[2],
        0.,
        -dot(x, eye),
        -dot(y, eye),
        -dot(z, eye),
        1.,
    ];
    let f = 1. / (45_f32.to_radians() / 2.).tan();
    let projection = [
        f / aspect,
        0.,
        0.,
        0.,
        0.,
        f,
        0.,
        0.,
        0.,
        0.,
        far / (near - far),
        -1.,
        0.,
        0.,
        far * near / (near - far),
        0.,
    ];
    multiply(projection, view)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hierarchical_transform_order() {
        let mut scale = IDENTITY;
        scale[0] = 2.;
        let mut translation = IDENTITY;
        translation[12] = 3.;
        assert_eq!(
            transform(multiply(translation, scale), [1., 0., 0.]),
            [5., 0., 0.]
        );
    }
}

mod rigid;
pub use rigid::Quaternion;
