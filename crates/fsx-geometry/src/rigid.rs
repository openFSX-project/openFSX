//! Double-precision rigid-body orientation, WXYZ quaternions.
type Vec3 = [f64; 3];
type Result<T> = std::result::Result<T, String>;
fn add(a: Vec3, b: Vec3) -> Vec3 {
    std::array::from_fn(|i| a[i] + b[i])
}
fn scale(a: Vec3, k: f64) -> Vec3 {
    a.map(|x| x * k)
}
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
#[derive(Debug, Clone, Copy)]
pub struct Quaternion(pub [f64; 4]);
impl Quaternion {
    pub const IDENTITY: Self = Self([1., 0., 0., 0.]);
    pub fn pitch(angle: f64) -> Self {
        Self([(angle * 0.5).cos(), 0., (angle * 0.5).sin(), 0.])
    }
    pub fn normalized(self) -> Result<Self> {
        let n = self.0.iter().map(|x| x * x).sum::<f64>().sqrt();
        if !n.is_finite() || n < 1e-12 {
            return Err("Invalid orientation quaternion".into());
        }
        Ok(Self(self.0.map(|x| x / n)))
    }
    pub fn product(self, b: Self) -> Self {
        let [w, x, y, z] = self.0;
        let [v, a, c, d] = b.0;
        Self([
            w * v - x * a - y * c - z * d,
            w * a + x * v + y * d - z * c,
            w * c - x * d + y * v + z * a,
            w * d + x * c - y * a + z * v,
        ])
    }
    pub fn inverse(self) -> Self {
        let [w, x, y, z] = self.0;
        Self([w, -x, -y, -z])
    }
    pub fn rotate(self, v: Vec3) -> Vec3 {
        let [_, x, y, z] = self.0;
        let imaginary = [x, y, z];
        let twice = scale(cross(imaginary, v), 2.);
        add(v, add(scale(twice, self.0[0]), cross(imaginary, twice)))
    }
    /// Roll right, pitch up and heading right, radians.
    pub fn euler(self) -> Vec3 {
        let [w, x, y, z] = self.0;
        [
            (2. * (w * x + y * z)).atan2(1. - 2. * (x * x + y * y)),
            (2. * (w * y - z * x)).clamp(-1., 1.).asin(),
            (2. * (w * z + x * y)).atan2(1. - 2. * (y * y + z * z)),
        ]
    }
}
