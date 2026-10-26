//! Trailing-edge flap detents from aircraft.cfg [flaps.0].
//! The lift increment is derived from the authored clean/full-flap stall
//! speeds; drag and pitch increments are generic estimates, not FSX parity.
use fsx_cfg::Config;

#[derive(Debug, Clone, PartialEq)]
pub struct Flaps {
    /// Detent deflections in degrees, starting at the retracted position.
    pub positions: Vec<f64>,
    /// Seconds for full travel from retracted to the last detent.
    pub extending_time: f64,
    /// Coefficient increments at the last detent, scaled linearly by deflection.
    pub lift: f64,
    pub drag: f64,
    pub pitch: f64,
}
impl Default for Flaps {
    fn default() -> Self {
        Self {
            positions: vec![0.],
            extending_time: 1.,
            lift: 0.,
            drag: 0.,
            pitch: 0.,
        }
    }
}
/// Generic full-deflection estimates used where AIR flap tables are not decoded.
const FULL_DRAG: f64 = 0.06;
const FULL_PITCH: f64 = -0.08;
impl Flaps {
    pub fn from_config(c: &Config, clean_max_lift: f64) -> Result<Self, String> {
        if c.get("flaps.0", "flaps-position.0").is_none() {
            return Ok(Self::default());
        }
        let mut positions = Vec::new();
        while let Some(_) = c.get("flaps.0", &format!("flaps-position.{}", positions.len())) {
            let degrees = c.number("flaps.0", &format!("flaps-position.{}", positions.len()))?;
            if !degrees.is_finite()
                || degrees < 0.
                || positions.last().is_some_and(|p| degrees < *p)
            {
                return Err(
                    "[flaps.0] positions must be finite, nonnegative and increasing".into(),
                );
            }
            positions.push(degrees);
        }
        let extending_time = c.optional_number("flaps.0", "extending-time", 5.)?;
        let scalar = |key: &str| c.optional_number("flaps.0", key, 1.);
        let clean = c.optional_number("reference speeds", "flaps_up_stall_speed", 0.)?;
        let full = c.optional_number("reference speeds", "full_flaps_stall_speed", 0.)?;
        // CLmax scales with (Vclean/Vfull)^2 at equal weight and density.
        let lift = if clean > 0. && full > 0. && full < clean {
            clean_max_lift * ((clean / full).powi(2) - 1.)
        } else {
            0.
        };
        let flaps = Self {
            positions,
            extending_time,
            lift: lift * scalar("lift_scalar")?,
            drag: FULL_DRAG * scalar("drag_scalar")?,
            pitch: FULL_PITCH * scalar("pitch_scalar")?,
        };
        if !flaps.extending_time.is_finite()
            || flaps.extending_time <= 0.
            || [flaps.lift, flaps.drag, flaps.pitch]
                .iter()
                .any(|x| !x.is_finite())
        {
            return Err("[flaps.0] has invalid timing or scalars".into());
        }
        Ok(flaps)
    }
    pub fn max_deflection(&self) -> f64 {
        *self.positions.last().unwrap_or(&0.)
    }
    pub fn detents(&self) -> usize {
        self.positions.len()
    }
    /// Normalized deflection 0..1 of a detent index (clamped).
    pub fn detent_fraction(&self, index: usize) -> f64 {
        let max = self.max_deflection();
        if max <= 0. {
            return 0.;
        }
        self.positions[index.min(self.positions.len() - 1)] / max
    }
    /// Lift, drag and pitch coefficient increments at normalized deflection.
    pub fn increments(&self, fraction: f64) -> (f64, f64, f64) {
        let f = fraction.clamp(0., 1.);
        (self.lift * f, self.drag * f * f, self.pitch * f)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn derives_lift_from_stall_speeds_and_rejects_bad_detents() {
        let c = Config::parse("[flaps.0]\nflaps-position.0=0\nflaps-position.1=10\nflaps-position.2=40\nextending-time=8\n[reference speeds]\nflaps_up_stall_speed=50\nfull_flaps_stall_speed=40\n").unwrap();
        let f = Flaps::from_config(&c, 1.6).unwrap();
        assert_eq!(f.detents(), 3);
        assert!((f.lift - 1.6 * (1.5625 - 1.)).abs() < 1e-12);
        assert_eq!(f.detent_fraction(1), 0.25);
        assert_eq!(f.increments(0.), (0., 0., 0.));
        let bad = Config::parse("[flaps.0]\nflaps-position.0=10\nflaps-position.1=5\n").unwrap();
        assert!(Flaps::from_config(&bad, 1.6).is_err());
        assert_eq!(
            Flaps::from_config(&Config::parse("").unwrap(), 1.6).unwrap(),
            Flaps::default()
        );
    }
}
