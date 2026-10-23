//! Versioned single-piston, zero-auxiliary voltage-core prefix. This ends before
//! battery-failure/avionics processing and does not establish loaded bus state.
use fsx_vehicle::electrical::Electrical;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct State {
    pub battery_voltage: f64,
    pub filtered_net_amps: f64,
    /// Last allocation, consumed by the next invocation's current sum.
    pub generator_amps: f64,
}
impl State {
    fn finite(self) -> bool {
        [
            self.battery_voltage,
            self.filtered_net_amps,
            self.generator_amps,
        ]
        .iter()
        .all(|v| v.is_finite())
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct Inputs {
    pub generator_online: Option<bool>,
    pub battery_master: Option<bool>,
    pub load_amps: Option<f64>,
    /// Native float32 input. Physical mapping remains unresolved; do not infer
    /// this from battery capacity, temperature or a nominal default.
    pub response_factor: Option<f32>,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    pub state: State,
    pub source_sum_used_amps: f64,
    pub pre_load_main_voltage: f64,
    pub pre_load_battery_voltage: f64,
    pub pre_load_hot_battery_voltage: f64,
}
#[derive(Debug, Clone, Default)]
pub struct Core {
    state: Option<State>,
}
impl Core {
    pub fn state(&self) -> Option<State> {
        self.state
    }
    pub fn seed(&mut self, state: Option<State>) -> Result<(), String> {
        if state.is_some_and(|s| !s.finite()) {
            return Err("Nonfinite legacy voltage-core state".into());
        }
        self.state = state;
        Ok(())
    }
    /// Explicit initialization/provider path, not an automatic flight update.
    /// Missing required input loses knowledge over a positive interval. Invalid
    /// input/computation leaves preceding state intact. Pause skips this API.
    pub fn advance(
        &mut self,
        config: &Electrical,
        input: Inputs,
        dt: f64,
    ) -> Result<Option<Step>, String> {
        if !dt.is_finite()
            || dt <= 0.
            || dt > 1. / 60.
            || input
                .load_amps
                .is_some_and(|v| !v.is_finite() || !(0. ..=1e6).contains(&v))
            || input
                .response_factor
                .is_some_and(|v| !v.is_finite() || v < 0.)
        {
            return Err("Invalid legacy voltage-core interval/load/response input".into());
        }
        for value in [config.max_battery_voltage, config.generator_voltage] {
            if value.is_some_and(|v| !v.is_finite() || !(0. ..=1000.).contains(&v)) {
                return Err("Invalid voltage-core rating".into());
            }
        }
        if config
            .max_generator_amps
            .is_some_and(|v| !v.is_finite() || !(0. ..=1e6).contains(&v))
        {
            return Err("Invalid voltage-core current rating".into());
        }
        // The original auxiliary sum divides by maximum battery voltage. Its
        // zero-rating singularity is outside this checked zero-auxiliary core.
        if config.max_battery_voltage == Some(0.) {
            return Err("Zero maximum battery voltage is outside the checked voltage core".into());
        }
        let (
            Some(before),
            Some(maximum),
            Some(nominal),
            Some(capacity),
            Some(online),
            Some(master),
            Some(load),
            Some(factor),
        ) = (
            self.state,
            config.max_battery_voltage,
            config.generator_voltage,
            config.max_generator_amps,
            input.generator_online,
            input.battery_master,
            input.load_amps,
            input.response_factor,
        )
        else {
            self.state = None;
            return Ok(None);
        };
        let online = online && nominal > 0.;
        let battery_bus = if master { before.battery_voltage } else { 0. };
        let main = if nominal > 0. {
            (battery_bus + if online { nominal } else { 0. }).min(nominal)
        } else {
            battery_bus
        };
        let filtered = before.filtered_net_amps
            + (before.generator_amps - load - before.filtered_net_amps) * 4. * dt;
        let intermediate =
            (before.battery_voltage + filtered * 0.000555 * f64::from(factor) * dt).max(0.);
        let allocated = if online {
            (load + (maximum - intermediate)).min(capacity)
        } else {
            0.
        };
        let voltage = (intermediate + before.generator_amps * 0.0004 * dt).min(maximum);
        let state = State {
            battery_voltage: voltage,
            filtered_net_amps: filtered,
            generator_amps: allocated,
        };
        if !state.finite() || !main.is_finite() || !intermediate.is_finite() {
            return Err("Legacy voltage core left finite numeric envelope".into());
        }
        self.state = Some(state);
        Ok(Some(Step {
            state,
            source_sum_used_amps: before.generator_amps,
            pre_load_main_voltage: main,
            pre_load_battery_voltage: battery_bus,
            pre_load_hot_battery_voltage: before.battery_voltage,
        }))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Electrical {
        Electrical {
            max_battery_voltage: Some(24.),
            generator_voltage: Some(28.),
            max_generator_amps: Some(60.),
            ..Default::default()
        }
    }
    fn input() -> Inputs {
        Inputs {
            generator_online: Some(true),
            battery_master: Some(true),
            load_amps: Some(12.),
            response_factor: Some(1.),
        }
    }
    fn seed() -> State {
        State {
            battery_voltage: 22.,
            filtered_net_amps: 0.,
            generator_amps: 0.,
        }
    }
    #[test]
    fn prior_current_lag_pre_load_values_and_reset_are_distinct() {
        let mut c = Core::default();
        c.seed(Some(seed())).unwrap();
        let first = c.advance(&config(), input(), 1. / 120.).unwrap().unwrap();
        assert_eq!(first.source_sum_used_amps, 0.);
        assert_eq!(first.pre_load_main_voltage, 28.);
        assert_eq!(first.pre_load_battery_voltage, 22.);
        assert!((first.state.filtered_net_amps + 0.4).abs() < 1e-12);
        assert!(first.state.battery_voltage < 22.);
        assert!(first.state.generator_amps > 14.);
        let off = Inputs {
            generator_online: Some(false),
            ..input()
        };
        let second = c.advance(&config(), off, 1. / 120.).unwrap().unwrap();
        assert_eq!(second.source_sum_used_amps, first.state.generator_amps);
        assert_eq!(second.state.generator_amps, 0.);
        c.seed(None).unwrap();
        assert_eq!(c.state(), None);
    }
    #[test]
    fn unknown_response_invalid_numbers_and_overflow_never_leave_stale_estimates() {
        let mut c = Core::default();
        c.seed(Some(seed())).unwrap();
        for dt in [0., -1., f64::NAN, 1.] {
            assert!(c.advance(&config(), input(), dt).is_err());
            assert_eq!(c.state(), Some(seed()));
        }
        assert!(c
            .advance(
                &config(),
                Inputs {
                    response_factor: Some(f32::INFINITY),
                    ..input()
                },
                1. / 120.
            )
            .is_err());
        assert_eq!(c.state(), Some(seed()));
        assert_eq!(
            c.advance(
                &config(),
                Inputs {
                    response_factor: None,
                    ..input()
                },
                1. / 120.
            )
            .unwrap(),
            None
        );
        assert_eq!(c.state(), None);
        assert!(c
            .seed(Some(State {
                battery_voltage: f64::NAN,
                ..seed()
            }))
            .is_err());
        let huge = State {
            filtered_net_amps: f64::MAX,
            generator_amps: -f64::MAX,
            ..seed()
        };
        c.seed(Some(huge)).unwrap();
        assert!(c.advance(&config(), input(), 1. / 120.).is_err());
        assert_eq!(c.state(), Some(huge));
    }
    #[test]
    fn allocation_uses_intermediate_voltage_before_the_upper_battery_clamp() {
        let mut c = Core::default();
        c.seed(Some(State {
            battery_voltage: 24.,
            filtered_net_amps: 0.,
            generator_amps: 60.,
        }))
        .unwrap();
        let s = c
            .advance(
                &config(),
                Inputs {
                    load_amps: Some(0.),
                    ..input()
                },
                1. / 120.,
            )
            .unwrap()
            .unwrap();
        assert_eq!(s.state.battery_voltage, 24.);
        assert!(s.state.generator_amps < 0.);
        let mut zero = config();
        zero.max_battery_voltage = Some(0.);
        assert!(c.advance(&zero, input(), 1. / 120.).is_err());
        assert_eq!(c.state(), Some(s.state));
    }
}
