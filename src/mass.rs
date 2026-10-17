//! Static aircraft loading and a body-axis inertia tensor, in SI units.
use crate::{
    aircraft::{FOOT, POUND, SLUG_FOOT_SQUARED},
    cfg::Config,
};
type Vec3 = [f64; 3];
type Matrix = [[f64; 3]; 3];

#[derive(Debug, Clone)]
pub struct Inertia {
    matrix: Matrix,
    cholesky: Matrix,
}
impl Inertia {
    pub fn new(matrix: Matrix) -> Result<Self, String> {
        let mut l = [[0.; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                if !matrix[i][j].is_finite() || (matrix[i][j] - matrix[j][i]).abs() > 1e-9 {
                    return Err("Inertia tensor must be finite and symmetric".into());
                }
            }
            for j in 0..=i {
                let residual = matrix[i][j] - (0..j).map(|k| l[i][k] * l[j][k]).sum::<f64>();
                l[i][j] = if i == j {
                    if residual <= 1e-9 || !residual.is_finite() {
                        return Err("Inertia tensor must be positive definite".into());
                    }
                    residual.sqrt()
                } else {
                    residual / l[j][j]
                };
            }
        }
        Ok(Self {
            matrix,
            cholesky: l,
        })
    }
    pub fn diagonal(values: Vec3) -> Result<Self, String> {
        Self::new(std::array::from_fn(|i| {
            std::array::from_fn(|j| if i == j { values[i] } else { 0. })
        }))
    }
    pub fn matrix(&self) -> Matrix {
        self.matrix
    }
    pub fn apply(&self, v: Vec3) -> Vec3 {
        std::array::from_fn(|i| (0..3).map(|j| self.matrix[i][j] * v[j]).sum())
    }
    pub fn solve(&self, v: Vec3) -> Vec3 {
        let l = &self.cholesky;
        let mut y = [0.; 3];
        for i in 0..3 {
            y[i] = (v[i] - (0..i).map(|j| l[i][j] * y[j]).sum::<f64>()) / l[i][i];
        }
        let mut x = [0.; 3];
        for i in (0..3).rev() {
            x[i] = (y[i] - (i + 1..3).map(|j| l[j][i] * x[j]).sum::<f64>()) / l[i][i];
        }
        x
    }
}

pub fn numbers(text: &str, count: usize, label: &str) -> Result<Vec<f64>, String> {
    let values = text
        .split(',')
        .take(count)
        .map(|s| s.trim().parse::<f64>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| format!("Invalid {label}"))?;
    if values.len() != count || values.iter().any(|x| !x.is_finite()) {
        return Err(format!("Invalid {label}"));
    }
    Ok(values)
}
pub fn vector(c: &Config, section: &str, key: &str) -> Result<Vec3, String> {
    let text = c
        .get(section, key)
        .ok_or_else(|| format!("Missing [{section}] {key}"))?;
    let v = numbers(text, 3, key)?;
    if text.split(',').count() != 3 {
        return Err(format!("Invalid vector {key}"));
    }
    Ok([v[0], v[1], v[2]])
}
// CFG longitudinal/lateral/vertical -> body forward/right/down, including datum.
pub fn body_position(datum: Vec3, position: Vec3) -> Vec3 {
    [
        (datum[0] + position[0]) * FOOT,
        (datum[1] + position[1]) * FOOT,
        -(datum[2] + position[2]) * FOOT,
    ]
}
#[derive(Debug, Clone)]
pub struct Loading {
    pub mass: f64,
    pub empty_mass: f64,
    pub payload_mass: f64,
    pub fuel_mass: f64,
    pub max_mass: f64,
    pub cg: Vec3,
    pub empty_cg: Vec3,
    pub inertia: Inertia,
}
impl Loading {
    /// CFG station weights plus all usable Avgas at an explicit 6 lb/US gal.
    /// Unusable fuel is already part of empty weight and is not added twice.
    pub fn from_config(c: &Config) -> Result<Self, String> {
        let positive = |key| -> Result<f64, String> {
            let n = c.number("weight_and_balance", key)?;
            if n <= 0. {
                return Err(format!("{key} must be positive"));
            }
            Ok(n)
        };
        let empty_mass = positive("empty_weight")? * POUND;
        let max_mass = positive("max_gross_weight")? * POUND;
        let datum = vector(c, "weight_and_balance", "reference_datum_position")?;
        let empty_cg = body_position(
            datum,
            vector(c, "weight_and_balance", "empty_weight_cg_position")?,
        );
        if c.optional_number("weight_and_balance", "empty_weight_coupled_moi", 0.)?
            .abs()
            > 1e-9
        {
            return Err("Nonzero empty_weight_coupled_moi remains unsupported".into());
        }
        let diagonal = [
            positive("empty_weight_roll_moi")?,
            positive("empty_weight_pitch_moi")?,
            positive("empty_weight_yaw_moi")?,
        ]
        .map(|x| x * SLUG_FOOT_SQUARED);
        let mut points = vec![(empty_mass, empty_cg)];
        let mut payload_mass = 0.;
        let mut stations = std::collections::HashSet::new();
        for (key, text) in c.section("weight_and_balance").into_iter().flatten() {
            if let Some(index) = key.strip_prefix("station_load.") {
                let index = index
                    .parse::<u32>()
                    .map_err(|_| format!("Invalid station index {key}"))?;
                if !stations.insert(index) {
                    return Err(format!("Duplicate numeric station index {key}"));
                }
                if points.len() > 1024 {
                    return Err("More than 1024 loading stations".into());
                }
                let v = numbers(text, 4, key)?;
                if v[0] < 0. {
                    return Err(format!("Negative weight at {key}"));
                }
                let mass = v[0] * POUND;
                payload_mass += mass;
                points.push((mass, body_position(datum, [v[1], v[2], v[3]])));
            }
        }
        const TANKS: [&str; 11] = [
            "center1",
            "center2",
            "center3",
            "leftmain",
            "leftaux",
            "lefttip",
            "rightmain",
            "rightaux",
            "righttip",
            "external1",
            "external2",
        ];
        if c.number("fuel", "fuel_type")? != 1. {
            return Err("Static fuel loading currently requires Avgas (fuel_type=1)".into());
        }
        let mut fuel_mass = 0.;
        for key in TANKS {
            if let Some(text) = c.get("fuel", key) {
                let v = numbers(text, 5, key)?;
                if text.split(',').count() != 5 || v[3] < 0. || v[4] < 0. {
                    return Err(format!("Invalid tank capacities at {key}"));
                }
                let mass = v[3] * 6. * POUND;
                fuel_mass += mass;
                points.push((mass, body_position(datum, [v[0], v[1], v[2]])));
            }
        }
        let mass = empty_mass + payload_mass + fuel_mass;
        if !mass.is_finite() || mass > max_mass + 1e-6 {
            return Err("Configured full-fuel loading exceeds maximum gross weight".into());
        }
        let cg = std::array::from_fn(|i| points.iter().map(|(m, p)| m * p[i]).sum::<f64>() / mass);
        let mut tensor =
            std::array::from_fn(|i| std::array::from_fn(|j| if i == j { diagonal[i] } else { 0. }));
        for (m, p) in points {
            let r: Vec3 = std::array::from_fn(|i| p[i] - cg[i]);
            let squared = r.iter().map(|x| x * x).sum::<f64>();
            for i in 0..3 {
                for j in 0..3 {
                    tensor[i][j] += m * ((if i == j { squared } else { 0. }) - r[i] * r[j]);
                }
            }
        }
        Ok(Self {
            mass,
            empty_mass,
            payload_mass,
            fuel_mass,
            max_mass,
            cg,
            empty_cg,
            inertia: Inertia::new(tensor)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Invented weights and geometry, independent of installed game resources.
    const OWN: &str = "[weight_and_balance]\nempty_weight=100\nmax_gross_weight=500\nreference_datum_position=0,0,0\nempty_weight_cg_position=0,0,0\nempty_weight_roll_moi=10\nempty_weight_pitch_moi=20\nempty_weight_yaw_moi=30\nstation_load.0=100,2,0,2,\"Test load\"\n[fuel]\nfuel_type=1\nleftmain=0,0,0,0,2\n";
    fn load(text: &str) -> Result<Loading, String> {
        Loading::from_config(&Config::parse(text)?)
    }
    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }
    #[test]
    fn loading_cg_parallel_axis_and_cross_products() {
        let l = load(OWN).unwrap();
        close(l.mass, 200. * POUND);
        close(l.payload_mass, 100. * POUND);
        close(l.fuel_mass, 0.); // Unusable volume is already in empty weight.
        close(l.cg[0], FOOT);
        close(l.cg[1], 0.);
        close(l.cg[2], -FOOT);
        let d = 200. * POUND * FOOT * FOOT;
        let m = l.inertia.matrix();
        close(m[0][0], 10. * SLUG_FOOT_SQUARED + d);
        close(m[1][1], 20. * SLUG_FOOT_SQUARED + 2. * d);
        close(m[2][2], 30. * SLUG_FOOT_SQUARED + d);
        close(m[0][2], d);
        close(m[2][0], d);
        let translated = load(&OWN.replace(
            "reference_datum_position=0,0,0",
            "reference_datum_position=12,4,7",
        ))
        .unwrap();
        for i in 0..3 {
            for j in 0..3 {
                close(m[i][j], translated.inertia.matrix()[i][j]);
            }
        }
        close(translated.cg[0], 13. * FOOT);
        close(translated.cg[1], 4. * FOOT);
        close(translated.cg[2], -8. * FOOT);
    }
    #[test]
    fn full_usable_fuel_is_added_once() {
        let l = load(&OWN.replace("leftmain=0,0,0,0,2", "leftmain=0,0,0,10,2")).unwrap();
        close(l.fuel_mass, 60. * POUND);
        close(l.mass, 260. * POUND);
        close(l.cg[0], 200. / 260. * FOOT);
    }
    #[test]
    fn invalid_and_overweight_loading_are_errors() {
        for (from, to) in [
            ("max_gross_weight=500", "max_gross_weight=150"),
            ("station_load.0=100", "station_load.0=-1"),
            ("station_load.0=100", "station_load.0=NaN"),
            ("station_load.0=100", "station_load.bad=100"),
            ("fuel_type=1", "fuel_type=2"),
            ("leftmain=0,0,0,0,2", "leftmain=0,0,0,-1,2"),
            ("empty_weight_roll_moi=10", "empty_weight_roll_moi=0"),
            (
                "empty_weight_cg_position=0,0,0",
                "empty_weight_cg_position=0,0,0,1",
            ),
        ] {
            assert!(load(&OWN.replace(from, to)).is_err(), "{to}");
        }
        assert!(load(&OWN.replace("[fuel]", "station_load.00=1,0,0,0\n[fuel]")).is_err());
        assert!(load(&OWN.replace("[fuel]", "empty_weight_coupled_moi=2\n[fuel]")).is_err());
    }
    #[test]
    fn coupled_inertia_solves_and_rejects_invalid_tensors() {
        let inertia = Inertia::new([[4., 1., -0.5], [1., 5., 0.2], [-0.5, 0.2, 6.]]).unwrap();
        let v = [2., -3., 4.];
        let back = inertia.solve(inertia.apply(v));
        for i in 0..3 {
            close(v[i], back[i]);
        }
        assert!(Inertia::diagonal([1., 0., 2.]).is_err());
        assert!(Inertia::diagonal([1., f64::NAN, 2.]).is_err());
        assert!(Inertia::new([[1., 2., 0.], [2., 1., 0.], [0., 0., 1.]]).is_err());
        assert!(Inertia::new([[1., 2., 0.], [0., 3., 0.], [0., 0., 1.]]).is_err());
    }
}
