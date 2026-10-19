//! Ideal ISA pitot/static data for the existing local test world.
//! No installation/position errors, weather, sensor failures or FSX calibration parity.
use crate::flight::Telemetry;

const T0: f64 = 288.15;
const P0: f64 = 101325.;
const R: f64 = 287.05287;
const GAMMA: f64 = 1.4;
const LAPSE: f64 = 0.0065;
const EXPONENT: f64 = 5.2558797;

#[derive(Clone, Copy, Debug)]
pub struct AirData {
    pub temperature_kelvin: f64,
    pub static_pressure_pascal: f64,
    pub impact_pressure_pascal: f64,
    /// Ideal calibrated airspeed; used as indicated speed only without instrument errors.
    pub calibrated_speed_mps: f64,
    /// Standard-setting pressure altitude in the model's local ISA height convention.
    pub pressure_altitude_m: f64,
    pub vertical_speed_mps: f64,
}
impl AirData {
    pub fn standard(
        altitude_m: f64,
        true_speed_mps: f64,
        vertical_speed_mps: f64,
    ) -> Result<Self, String> {
        if [altitude_m, true_speed_mps, vertical_speed_mps]
            .iter()
            .any(|v| !v.is_finite())
            || !(0. ..=11000.).contains(&altitude_m)
            || true_speed_mps < 0.
            || vertical_speed_mps.abs() > 1000.
        {
            return Err("Ideal air-data model requires finite 0..11000 m altitude, nonnegative speed and bounded vertical speed".into());
        }
        let temperature = T0 - LAPSE * altitude_m;
        let pressure = P0 * (temperature / T0).powf(EXPONENT);
        let a = (GAMMA * R * temperature).sqrt();
        let mach = true_speed_mps / a;
        if mach >= 1. {
            return Err("Ideal air-data model supports subsonic pitot flow only".into());
        }
        // expm1/log1p preserve very small speeds without subtracting nearly equal pressures.
        let impact = pressure
            * ((GAMMA / (GAMMA - 1.)) * (0.5 * (GAMMA - 1.) * mach * mach).ln_1p()).exp_m1();
        let speed_squared = 2. * GAMMA * R * T0 / (GAMMA - 1.)
            * (((GAMMA - 1.) / GAMMA) * (impact / P0).ln_1p()).exp_m1();
        let calibrated = speed_squared.max(0.).sqrt();
        let pressure_altitude = T0 / LAPSE * (1. - (pressure / P0).powf(1. / EXPONENT));
        if !calibrated.is_finite() || !pressure_altitude.is_finite() {
            return Err("Non-finite air-data result".into());
        }
        Ok(Self {
            temperature_kelvin: temperature,
            static_pressure_pascal: pressure,
            impact_pressure_pascal: impact,
            calibrated_speed_mps: calibrated,
            pressure_altitude_m: pressure_altitude,
            vertical_speed_mps,
        })
    }
    pub fn from_telemetry(telemetry: Telemetry) -> Result<Self, String> {
        Self::standard(telemetry.altitude, telemetry.speed, telemetry.climb)
    }
    /// Known legacy token units only: knots, feet, and 1/256 m/s.
    pub fn native_inputs(&self) -> [f64; 3] {
        [
            self.calibrated_speed_mps / fsx_vehicle::aircraft::KNOT,
            self.pressure_altitude_m / fsx_vehicle::aircraft::FOOT,
            self.vertical_speed_mps * 256.,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sea_level_calibration_and_pressure_altitude_inversion() {
        for speed in [0., 1e-6, 50., 150., 300.] {
            let a = AirData::standard(0., speed, 0.).unwrap();
            assert!((a.calibrated_speed_mps - speed).abs() < 1e-9);
            assert_eq!(a.static_pressure_pascal, P0);
        }
        for height in [0., 300., 3000., 11000.] {
            let a = AirData::standard(height, 50., 1.).unwrap();
            assert!((a.pressure_altitude_m - height).abs() < 1e-8);
            assert!(
                (a.static_pressure_pascal / a.temperature_kelvin / R
                    - crate::flight::density(height))
                .abs()
                    < 1e-7
            );
        }
    }
    #[test]
    fn altitude_reduces_ideal_indication_and_units_preserve_climb_sign() {
        let sea = AirData::standard(0., 50., 1.).unwrap();
        let high = AirData::standard(3000., 50., -1.).unwrap();
        assert!(high.calibrated_speed_mps < sea.calibrated_speed_mps);
        assert!((high.native_inputs()[1] - 3000. / fsx_vehicle::aircraft::FOOT).abs() < 1e-7);
        assert_eq!(sea.native_inputs()[2], 256.);
        assert_eq!(high.native_inputs()[2], -256.);
        // At low Mach, ideal CAS tends to density-scaled equivalent speed.
        let slow = AirData::standard(3000., 0.01, 0.).unwrap();
        let equivalent = 0.01 * (crate::flight::density(3000.) / 1.225).sqrt();
        assert!((slow.calibrated_speed_mps - equivalent).abs() < 1e-9);
    }
    #[test]
    fn invalid_domain_is_unavailable_not_silently_clamped() {
        for (h, v, vs) in [
            (-1., 50., 0.),
            (11001., 50., 0.),
            (0., -1., 0.),
            (0., 400., 0.),
            (0., 50., f64::NAN),
            (0., 50., 1001.),
        ] {
            assert!(AirData::standard(h, v, vs).is_err());
        }
    }
}
