//! Versioned single-piston initialization and zero-auxiliary voltage core.
//! advance() is the prefix; advance_frame() composes the failure/avionics tail.
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
/// Known tail context and surrounding bus observations. None circuit entries
/// mean explicitly absent equipment; unresolved presence needs no frame call.
#[derive(Debug, Clone, Copy)]
pub struct TailEnvironment {
    /// Original generator-map bit. Disabled equipment skips the voltage write;
    /// an enabled but offline source writes zero when its nominal rating > 0.
    pub generator_enabled: bool,
    pub incoming_bus_voltage: [f64; 8],
    pub incoming_generator_amps: [f64; 4],
    pub battery_failed: bool,
    pub avionics_master: bool,
    pub voltage_mode: bool,
    pub override_power: bool,
    pub avionics_circuit: Option<crate::legacy_power::Circuit>,
    pub additional_system_circuit: Option<crate::legacy_power::Circuit>,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameStep {
    pub prefix: Step,
    pub tail: crate::legacy_power::TailStep,
    pub state: State,
}
#[derive(Debug, Clone, Default)]
pub struct Core {
    state: Option<State>,
}
impl Core {
    /// Original state constructor, before aircraft initialization. Default
    /// remains unknown for callers that have not observed a fresh allocation.
    pub fn constructed() -> Self {
        Self {
            state: Some(State {
                battery_voltage: 0.,
                filtered_net_amps: 0.,
                generator_amps: 0.,
            }),
        }
    }
    /// Original initializer replaces battery voltage and all eight bus voltages
    /// with the effective maximum. It preserves prior filter/allocation history;
    /// only constructed() establishes fresh zeros. Unknown history stays unknown.
    /// Zero rating is valid here, although the dynamic core rejects that case.
    pub fn initialize_rated_voltage(
        &mut self,
        config: &Electrical,
    ) -> Result<[Option<f64>; 8], String> {
        if config
            .max_battery_voltage
            .is_some_and(|v| !v.is_finite() || !(0. ..=1000.).contains(&v))
        {
            return Err("Invalid legacy initialization voltage".into());
        }
        match config.max_battery_voltage {
            Some(voltage) => {
                if let Some(state) = &mut self.state {
                    state.battery_voltage = voltage;
                }
                Ok([Some(voltage); 8])
            }
            None => {
                self.state = None;
                Ok([None; 8])
            }
        }
    }
    /// Compose the checked prefix and tail atomically for known providers. A
    /// failure clears the stored allocated current before the next prefix sum.
    pub fn advance_frame(
        &mut self,
        config: &Electrical,
        input: Inputs,
        tail: Option<TailEnvironment>,
        dt: f64,
    ) -> Result<Option<FrameStep>, String> {
        if tail.is_some_and(|t| {
            (!t.generator_enabled && input.generator_online == Some(true))
                || t.incoming_bus_voltage
                    .iter()
                    .chain(t.incoming_generator_amps.iter())
                    .any(|v| !v.is_finite())
                || !crate::legacy_power::valid_circuit(t.avionics_circuit)
                || !crate::legacy_power::valid_circuit(t.additional_system_circuit)
        }) {
            return Err("Invalid legacy frame-tail environment".into());
        }
        let mut staged = self.clone();
        let prefix = staged.advance(config, input, dt)?;
        let (Some(prefix), Some(tail)) = (prefix, tail) else {
            self.state = None;
            return Ok(None);
        };
        let mut buses = tail.incoming_bus_voltage;
        buses[0] = prefix.pre_load_main_voltage;
        buses[2] = prefix.pre_load_battery_voltage;
        buses[3] = prefix.pre_load_hot_battery_voltage;
        let nominal = config
            .generator_voltage
            .ok_or("Missing checked generator rating")?;
        if tail.generator_enabled && nominal > 0. {
            buses[4] = if input.generator_online == Some(true) {
                nominal
            } else {
                0.
            };
        }
        let mut amps = tail.incoming_generator_amps;
        amps[0] = prefix.state.generator_amps;
        let tail = crate::legacy_power::finish(crate::legacy_power::TailInput {
            context: crate::legacy_power::Context {
                voltage_mode: tail.voltage_mode,
                battery_master: input
                    .battery_master
                    .ok_or("Missing checked master position")?,
                max_battery_voltage: config
                    .max_battery_voltage
                    .ok_or("Missing checked battery rating")?,
                override_power: tail.override_power,
            },
            bus_voltage: buses,
            generator_amps: amps,
            source_sum_amps: prefix.source_sum_used_amps,
            battery_failed: tail.battery_failed,
            avionics_master: tail.avionics_master,
            avionics_circuit: tail.avionics_circuit,
            additional_system_circuit: tail.additional_system_circuit,
        })?;
        let mut state = prefix.state;
        state.generator_amps = tail.generator_amps[0];
        self.state = Some(state);
        Ok(Some(FrameStep {
            prefix,
            tail,
            state,
        }))
    }
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
    #[test]
    fn first_frame_preserves_generator_voltage_only_when_original_skips_the_write() {
        for (nominal, enabled, expected) in [(0., true, 24.), (28., false, 24.), (28., true, 0.)] {
            let cfg = Electrical {
                max_battery_voltage: Some(24.),
                generator_voltage: Some(nominal),
                max_generator_amps: Some(60.),
                ..Default::default()
            };
            let mut core = Core::constructed();
            let buses = core
                .initialize_rated_voltage(&cfg)
                .unwrap()
                .map(Option::unwrap);
            let tail = TailEnvironment {
                generator_enabled: enabled,
                incoming_bus_voltage: buses,
                incoming_generator_amps: [0.; 4],
                battery_failed: false,
                avionics_master: false,
                voltage_mode: true,
                override_power: false,
                avionics_circuit: None,
                additional_system_circuit: None,
            };
            let input = Inputs {
                generator_online: Some(false),
                battery_master: Some(false),
                load_amps: Some(0.),
                response_factor: Some(1.),
            };
            let frame = core
                .advance_frame(&cfg, input, Some(tail), 1. / 120.)
                .unwrap()
                .unwrap();
            assert_eq!(
                frame.tail.bus_voltage,
                [0., 0., 0., 24., expected, 24., 24., 24.]
            );
            let before = core.state();
            assert!(core
                .advance_frame(
                    &cfg,
                    Inputs {
                        generator_online: Some(true),
                        ..input
                    },
                    Some(TailEnvironment {
                        generator_enabled: false,
                        ..tail
                    }),
                    1. / 120.
                )
                .is_err());
            assert_eq!(core.state(), before);
        }
    }
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
    #[test]
    fn failure_tail_updates_next_prefix_current_and_invalid_tail_is_atomic() {
        let mut core = Core::default();
        core.seed(Some(seed())).unwrap();
        let circuit = crate::legacy_power::Circuit {
            bus: 0,
            min_voltage: 17.,
            forced_off: false,
        };
        let tail = TailEnvironment {
            generator_enabled: true,
            incoming_bus_voltage: [0.; 8],
            incoming_generator_amps: [0.; 4],
            battery_failed: true,
            avionics_master: true,
            voltage_mode: true,
            override_power: false,
            avionics_circuit: Some(circuit),
            additional_system_circuit: Some(crate::legacy_power::Circuit { bus: 1, ..circuit }),
        };
        let first = core
            .advance_frame(&config(), input(), Some(tail), 1. / 120.)
            .unwrap()
            .unwrap();
        assert!(first.prefix.state.generator_amps > 14.);
        assert_eq!(first.state.generator_amps, 0.);
        assert_eq!(first.tail.bus_voltage, [0.; 8]);
        let saved = core.state();
        let mut invalid = tail;
        invalid.incoming_generator_amps[2] = f64::NAN;
        assert!(core
            .advance_frame(&config(), input(), Some(invalid), 1. / 120.)
            .is_err());
        assert_eq!(core.state(), saved);
        assert!(core
            .advance_frame(
                &config(),
                Inputs {
                    response_factor: None,
                    ..input()
                },
                Some(invalid),
                1. / 120.
            )
            .is_err());
        assert_eq!(core.state(), saved);
        let next = core
            .advance_frame(
                &config(),
                input(),
                Some(TailEnvironment {
                    battery_failed: false,
                    ..tail
                }),
                1. / 120.,
            )
            .unwrap()
            .unwrap();
        assert_eq!(next.prefix.source_sum_used_amps, 0.);
        assert_eq!(next.tail.bus_voltage[1], 28.);
        assert!(next.tail.additional_system_active);
    }
}
