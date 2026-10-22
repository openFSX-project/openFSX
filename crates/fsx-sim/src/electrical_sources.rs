//! Explicit source primitives, not an inferred FSX bus topology or default model.
//! Positive terminal current discharges the battery; negative current charges it.
use fsx_vehicle::electrical::Electrical;

fn bounded(value: Option<f64>, max: f64) -> bool {
    value.is_none_or(|v| v.is_finite() && (0. ..=max).contains(&v))
}

/// Caller-supplied charge-fraction/voltage observations. This does not decode the
/// unresolved legacy battery_capacity_pct_to_voltage CFG grammar.
#[derive(Debug, Clone, PartialEq)]
pub struct VoltageCurve(Vec<(f64, f64)>);
impl VoltageCurve {
    pub fn new(points: Vec<(f64, f64)>) -> Result<Self, String> {
        if !(2..=256).contains(&points.len())
            || points.first().map(|p| p.0) != Some(0.)
            || points.last().map(|p| p.0) != Some(1.)
            || points
                .iter()
                .any(|&(x, y)| !bounded(Some(x), 1.) || !bounded(Some(y), 1000.))
            || points
                .windows(2)
                .any(|p| p[0].0 >= p[1].0 || p[0].1 > p[1].1)
        {
            return Err(
                "Voltage curve requires 2..256 ordered finite points covering charge 0..1".into(),
            );
        }
        Ok(Self(points))
    }
    fn voltage(&self, fraction: f64) -> f64 {
        for pair in self.0.windows(2) {
            let [(x0, y0), (x1, y1)] = [pair[0], pair[1]];
            if fraction <= x1 {
                return y0 + (y1 - y0) * (fraction - x0) / (x1 - x0);
            }
        }
        self.0.last().unwrap().1
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Battery {
    capacity_amph: Option<f64>,
    max_voltage: Option<f64>,
    charging_limit_amps: Option<f64>,
    charge_fraction: Option<f64>,
    curve: Option<VoltageCurve>,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BatteryFlow {
    /// Average actual current after charge-limit and empty/full clipping. None
    /// means the supplied metadata/state is insufficient to calculate it.
    pub average_terminal_amps: Option<f64>,
}
impl Battery {
    pub fn new(configuration: &Electrical) -> Result<Self, String> {
        if !bounded(configuration.battery_max_capacity_amph, 1e6)
            || !bounded(configuration.max_battery_voltage, 1000.)
            || !bounded(configuration.battery_charging_current, 1e6)
        {
            return Err("Invalid battery metadata".into());
        }
        Ok(Self {
            capacity_amph: configuration.battery_max_capacity_amph,
            max_voltage: configuration.max_battery_voltage,
            charging_limit_amps: configuration.battery_charging_current,
            charge_fraction: None,
            curve: None,
        })
    }
    pub fn set_charge_fraction(&mut self, fraction: Option<f64>) -> Result<(), String> {
        if !bounded(fraction, 1.) {
            return Err("Battery charge fraction must be finite and in 0..1".into());
        }
        self.charge_fraction = fraction;
        Ok(())
    }
    pub fn charge_fraction(&self) -> Option<f64> {
        self.charge_fraction
    }
    pub fn set_voltage_curve(&mut self, curve: Option<VoltageCurve>) -> Result<(), String> {
        if let (Some(max), Some(curve)) = (self.max_voltage, &curve) {
            if curve.0.last().unwrap().1 != max {
                return Err("Full-charge curve voltage conflicts with authored maximum".into());
            }
        }
        self.curve = curve;
        Ok(())
    }
    /// Open-circuit voltage only: this is not loaded bus voltage. Partial charge
    /// without a supplied curve remains unknown; no linear/default curve is made.
    pub fn open_circuit_voltage(&self) -> Option<f64> {
        let fraction = self.charge_fraction?;
        if let Some(curve) = &self.curve {
            Some(curve.voltage(fraction))
        } else if fraction == 1. {
            self.max_voltage
        } else {
            None
        }
    }
    /// Integrate an explicit requested terminal current, not authored consumer
    /// maxima. Charging needs an explicit limit. Unknown current invalidates
    /// charge knowledge for positive elapsed time; zero time never changes it.
    /// Zero authored capacity cannot store charge and is outside this model.
    pub fn advance(
        &mut self,
        seconds: f64,
        requested_amps: Option<f64>,
    ) -> Result<BatteryFlow, String> {
        if !seconds.is_finite()
            || !(0. ..=3600.).contains(&seconds)
            || requested_amps.is_some_and(|i| !i.is_finite() || i.abs() > 1e6)
        {
            return Err("Invalid bounded battery integration input".into());
        }
        if seconds == 0. {
            return Ok(BatteryFlow {
                average_terminal_amps: None,
            });
        }
        let current = requested_amps.and_then(|i| {
            if i < 0. {
                self.charging_limit_amps.map(|limit| i.max(-limit))
            } else {
                Some(i)
            }
        });
        if current == Some(0.) {
            return Ok(BatteryFlow {
                average_terminal_amps: Some(0.),
            });
        }
        let (Some(current), Some(fraction), Some(capacity)) = (
            current,
            self.charge_fraction,
            self.capacity_amph.filter(|c| *c > 0.),
        ) else {
            self.charge_fraction = None;
            return Ok(BatteryFlow {
                average_terminal_amps: None,
            });
        };
        let next = (fraction - current * seconds / (3600. * capacity)).clamp(0., 1.);
        self.charge_fraction = Some(next);
        Ok(BatteryFlow {
            average_terminal_amps: Some((fraction - next) * capacity * 3600. / seconds),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Generator {
    voltage: Option<f64>,
    capacity_amps: Option<f64>,
    online: Option<bool>,
}
impl Generator {
    pub fn new(configuration: &Electrical) -> Result<Self, String> {
        if !bounded(configuration.generator_voltage, 1000.)
            || !bounded(configuration.max_generator_amps, 1e6)
        {
            return Err("Invalid generator metadata".into());
        }
        Ok(Self {
            voltage: configuration.generator_voltage,
            capacity_amps: configuration.max_generator_amps,
            online: None,
        })
    }
    /// Availability must come from an engine/regulator provider, not throttle.
    pub fn set_online(&mut self, online: Option<bool>) {
        self.online = online;
    }
    /// Ideal regulated source for known demand within capability. Overload has
    /// no guessed droop curve. Offline means disconnected, not a dead bus.
    pub fn supplied_voltage(&self, demand_amps: Option<f64>) -> Result<Option<f64>, String> {
        if !bounded(demand_amps, 1e6) {
            return Err("Invalid generator demand".into());
        }
        Ok(match (self.online, demand_amps, self.capacity_amps) {
            (Some(false), _, _) => Some(0.),
            (Some(true), Some(demand), Some(capacity)) if demand <= capacity => self.voltage,
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Electrical {
        Electrical {
            battery_max_capacity_amph: Some(40.),
            battery_charging_current: Some(4.),
            max_battery_voltage: Some(24.),
            generator_voltage: Some(28.),
            max_generator_amps: Some(60.),
            ..Electrical::default()
        }
    }
    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-10, "{actual} != {expected}");
    }
    #[test]
    fn amp_hours_charge_limit_and_boundary_flow_are_conserved() {
        let mut b = Battery::new(&config()).unwrap();
        assert_eq!(b.charge_fraction(), None);
        b.set_charge_fraction(Some(1.)).unwrap();
        assert_eq!(b.open_circuit_voltage(), Some(24.));
        near(
            b.advance(3600., Some(10.))
                .unwrap()
                .average_terminal_amps
                .unwrap(),
            10.,
        );
        near(b.charge_fraction().unwrap(), 0.75);
        assert_eq!(b.open_circuit_voltage(), None);
        near(
            b.advance(3600., Some(-20.))
                .unwrap()
                .average_terminal_amps
                .unwrap(),
            -4.,
        );
        near(b.charge_fraction().unwrap(), 0.85);
        // Only 6 Ah remained; requested 40 A empties the battery within the hour.
        b.set_charge_fraction(Some(0.15)).unwrap();
        near(
            b.advance(3600., Some(40.))
                .unwrap()
                .average_terminal_amps
                .unwrap(),
            6.,
        );
        assert_eq!(b.charge_fraction(), Some(0.));
        near(
            b.advance(3600., Some(40.))
                .unwrap()
                .average_terminal_amps
                .unwrap(),
            0.,
        );
        b.set_charge_fraction(Some(0.99)).unwrap();
        near(
            b.advance(3600., Some(-20.))
                .unwrap()
                .average_terminal_amps
                .unwrap(),
            -0.4,
        );
        assert_eq!(b.charge_fraction(), Some(1.));
    }
    #[test]
    fn pause_unknown_inputs_missing_limits_and_zero_capacity_do_not_invent_charge() {
        let mut b = Battery::new(&Electrical::default()).unwrap();
        b.set_charge_fraction(Some(0.5)).unwrap();
        b.advance(0., None).unwrap();
        b.advance(12., Some(0.)).unwrap();
        assert_eq!(b.charge_fraction(), Some(0.5));
        b.advance(1., Some(1.)).unwrap();
        assert_eq!(b.charge_fraction(), None);
        let mut c = config();
        c.battery_charging_current = None;
        let mut b = Battery::new(&c).unwrap();
        b.set_charge_fraction(Some(0.5)).unwrap();
        b.advance(1., Some(-1.)).unwrap();
        assert_eq!(b.charge_fraction(), None);
        c.battery_max_capacity_amph = Some(0.);
        let mut b = Battery::new(&c).unwrap();
        b.set_charge_fraction(Some(0.5)).unwrap();
        b.advance(1., Some(1.)).unwrap();
        assert_eq!(b.charge_fraction(), None);
        let mut b = Battery::new(&config()).unwrap();
        b.set_charge_fraction(Some(1.)).unwrap();
        b.advance(1., None).unwrap();
        assert_eq!(b.charge_fraction(), None);
    }
    #[test]
    fn explicit_curve_interpolates_but_is_not_a_bus_voltage_model() {
        let mut b = Battery::new(&config()).unwrap();
        b.set_voltage_curve(Some(
            VoltageCurve::new(vec![(0., 18.), (0.5, 22.), (1., 24.)]).unwrap(),
        ))
        .unwrap();
        for (fraction, voltage) in [(0., 18.), (0.25, 20.), (0.75, 23.), (1., 24.)] {
            b.set_charge_fraction(Some(fraction)).unwrap();
            assert_eq!(b.open_circuit_voltage(), Some(voltage));
        }
        b.set_charge_fraction(None).unwrap();
        assert_eq!(b.open_circuit_voltage(), None);
        for points in [
            vec![],
            vec![(0., 0.)],
            vec![(0., 0.), (0., 24.)],
            vec![(0., 24.), (1., 18.)],
            vec![(0., 0.), (1., f64::NAN)],
            vec![(0.1, 0.), (1., 24.)],
        ] {
            assert!(VoltageCurve::new(points).is_err());
        }
    }
    #[test]
    fn invalid_source_mutations_are_atomic_and_metadata_is_validated() {
        let mut b = Battery::new(&config()).unwrap();
        b.set_charge_fraction(Some(0.5)).unwrap();
        let before = b.clone();
        for (dt, amps) in [
            (f64::NAN, Some(1.)),
            (-1., Some(1.)),
            (3601., Some(1.)),
            (1., Some(f64::INFINITY)),
        ] {
            assert!(b.advance(dt, amps).is_err());
            assert_eq!(b, before);
        }
        assert!(b.set_charge_fraction(Some(1.01)).is_err());
        assert!(b
            .set_voltage_curve(Some(VoltageCurve::new(vec![(0., 18.), (1., 28.)]).unwrap()))
            .is_err());
        assert_eq!(b, before);
        let mut c = config();
        c.battery_max_capacity_amph = Some(f64::INFINITY);
        assert!(Battery::new(&c).is_err());
        c.max_generator_amps = Some(-1.);
        assert!(Generator::new(&c).is_err());
    }
    #[test]
    fn generator_needs_explicit_availability_and_known_capacity_and_demand() {
        let mut g = Generator::new(&config()).unwrap();
        assert_eq!(g.supplied_voltage(Some(7.)).unwrap(), None);
        g.set_online(Some(true));
        assert_eq!(g.supplied_voltage(Some(60.)).unwrap(), Some(28.));
        assert_eq!(g.supplied_voltage(Some(60.01)).unwrap(), None);
        assert_eq!(g.supplied_voltage(None).unwrap(), None);
        assert!(g.supplied_voltage(Some(-1.)).is_err());
        g.set_online(Some(false));
        assert_eq!(g.supplied_voltage(None).unwrap(), Some(0.));
        let mut g = Generator::new(&Electrical::default()).unwrap();
        g.set_online(Some(true));
        assert_eq!(g.supplied_voltage(Some(0.)).unwrap(), None);
    }
}
