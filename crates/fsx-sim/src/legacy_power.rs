//! Versioned finite-input circuit predicate and battery-failure/avionics tail.
//! Context mode requires a provider; configuration overrides retain process state.
/// Original process-global override, updated only by a present CFG key. Keep
/// this across aircraft loads; a new unresolved process has no assumed value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PowerOverride {
    value: Option<bool>,
}
impl PowerOverride {
    /// Supply an observed process value, or None when startup is unverified.
    pub fn observed(value: Option<bool>) -> Self {
        Self { value }
    }
    pub fn value(self) -> Option<bool> {
        self.value
    }
    /// Versioned sim1 10.0.62615 configuration-load behavior. Missing keys leave
    /// the previous process value intact, including unknown state.
    pub fn load_configuration(&mut self, configuration: &fsx_vehicle::electrical::Electrical) {
        if let Some(value) = configuration.always_available {
            self.value = Some(value);
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Context {
    /// Original context flag OR mode-service result == 1.
    pub voltage_mode: bool,
    pub battery_master: bool,
    pub max_battery_voltage: f64,
    pub override_power: bool,
}
#[derive(Debug, Clone, Copy)]
pub struct Circuit {
    /// Raw legacy bus code. Invalid codes bypass the predicate in the original;
    /// normal CFG loading still rejects them before simulation.
    pub bus: u32,
    pub min_voltage: f64,
    pub forced_off: bool,
}
fn valid_context(context: Context) -> bool {
    context.max_battery_voltage.is_finite() && (0. ..=1000.).contains(&context.max_battery_voltage)
}
pub(crate) fn valid_circuit(circuit: Option<Circuit>) -> bool {
    circuit.is_none_or(|c| c.min_voltage.is_finite() && (0. ..=1000.).contains(&c.min_voltage))
}
/// None explicitly means absent equipment, not unresolved presence. All inputs
/// must already be known; a caller with unknown providers must defer evaluation.
pub fn available(
    context: Context,
    circuit: Option<Circuit>,
    bus_voltage: [f64; 8],
) -> Result<bool, String> {
    if !valid_context(context)
        || !valid_circuit(circuit)
        || bus_voltage.iter().any(|v| !v.is_finite())
    {
        return Err("Invalid original circuit predicate input".into());
    }
    if !context.voltage_mode {
        return Ok(context.battery_master);
    }
    let Some(circuit) = circuit else {
        return Ok(false);
    };
    if circuit.bus > 7 {
        return Ok(true);
    }
    Ok(context.override_power
        || context.max_battery_voltage == 0.
        || (bus_voltage[circuit.bus as usize] > circuit.min_voltage && !circuit.forced_off))
}
#[derive(Debug, Clone, Copy)]
pub struct TailInput {
    pub context: Context,
    pub bus_voltage: [f64; 8],
    pub generator_amps: [f64; 4],
    pub source_sum_amps: f64,
    pub battery_failed: bool,
    pub avionics_master: bool,
    pub avionics_circuit: Option<Circuit>,
    pub additional_system_circuit: Option<Circuit>,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TailStep {
    pub bus_voltage: [f64; 8],
    pub generator_amps: [f64; 4],
    pub source_sum_amps: f64,
    pub avionics_active: bool,
    pub additional_system_active: bool,
}
/// Original frame tail, given the preceding core outputs and known services.
/// Active flags are legacy availability, not a proof of nonzero bus voltage:
/// bypass/fallback paths can report availability with a failed battery.
pub fn finish(input: TailInput) -> Result<TailStep, String> {
    if !valid_context(input.context)
        || !valid_circuit(input.avionics_circuit)
        || !valid_circuit(input.additional_system_circuit)
        || input
            .bus_voltage
            .iter()
            .chain(input.generator_amps.iter())
            .any(|v| !v.is_finite())
        || !input.source_sum_amps.is_finite()
    {
        return Err("Invalid original electrical-tail input".into());
    }
    let mut output = TailStep {
        bus_voltage: input.bus_voltage,
        generator_amps: input.generator_amps,
        source_sum_amps: input.source_sum_amps,
        avionics_active: false,
        additional_system_active: false,
    };
    if input.battery_failed {
        output.bus_voltage = [0.; 8];
        output.generator_amps = [0.; 4];
        output.source_sum_amps = 0.;
    }
    output.avionics_active = available(input.context, input.avionics_circuit, output.bus_voltage)?
        && input.avionics_master;
    output.bus_voltage[1] = if output.avionics_active {
        output.bus_voltage[0]
    } else {
        0.
    };
    output.additional_system_active = available(
        input.context,
        input.additional_system_circuit,
        output.bus_voltage,
    )?;
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> Context {
        Context {
            voltage_mode: true,
            battery_master: true,
            max_battery_voltage: 24.,
            override_power: false,
        }
    }
    fn circuit(bus: u32) -> Option<Circuit> {
        Some(Circuit {
            bus,
            min_voltage: 17.,
            forced_off: false,
        })
    }
    #[test]
    fn strict_boundary_and_early_fallbacks_match_original_predicate() {
        assert!(!available(context(), circuit(0), [17.; 8]).unwrap());
        assert!(available(
            context(),
            circuit(0),
            [f64::from_bits(17f64.to_bits() + 1); 8]
        )
        .unwrap());
        assert!(!available(
            Context {
                override_power: true,
                ..context()
            },
            None,
            [0.; 8]
        )
        .unwrap());
        assert!(available(context(), circuit(8), [0.; 8]).unwrap());
        assert!(available(
            Context {
                voltage_mode: false,
                ..context()
            },
            None,
            [0.; 8]
        )
        .unwrap());
        assert!(!available(
            Context {
                voltage_mode: false,
                battery_master: false,
                override_power: true,
                ..context()
            },
            circuit(0),
            [28.; 8]
        )
        .unwrap());
        assert!(available(
            Context {
                max_battery_voltage: 0.,
                ..context()
            },
            Some(Circuit {
                forced_off: true,
                ..circuit(0).unwrap()
            }),
            [0.; 8]
        )
        .unwrap());
    }
    #[test]
    fn failure_clears_all_buses_and_current_slots_before_avionics_and_additional_system_checks() {
        let input = TailInput {
            context: context(),
            bus_voltage: [22., 27., 22., 22., 28., 0., 0., 0.],
            generator_amps: [14., 3., 2., 1.],
            source_sum_amps: 12.,
            battery_failed: false,
            avionics_master: true,
            avionics_circuit: circuit(0),
            additional_system_circuit: circuit(1),
        };
        let on = finish(input).unwrap();
        assert_eq!(on.bus_voltage[1], 22.);
        assert!(on.additional_system_active);
        let off = finish(TailInput {
            avionics_master: false,
            ..input
        })
        .unwrap();
        assert_eq!(off.bus_voltage[1], 0.);
        assert!(!off.additional_system_active);
        let failed = finish(TailInput {
            battery_failed: true,
            ..input
        })
        .unwrap();
        assert_eq!(failed.bus_voltage, [0.; 8]);
        assert_eq!(failed.generator_amps, [0.; 4]);
        assert_eq!(failed.source_sum_amps, 0.);
        let bypass = finish(TailInput {
            battery_failed: true,
            context: Context {
                override_power: true,
                ..context()
            },
            ..input
        })
        .unwrap();
        assert!(bypass.avionics_active && bypass.additional_system_active);
        assert_eq!(bypass.bus_voltage, [0.; 8]);
    }
}
