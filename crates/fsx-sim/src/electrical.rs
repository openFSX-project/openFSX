//! Legacy circuit loads from known bus observations and explicit device requests.
//! Explicit source primitives remain separate from bus ties and overload voltage
//! response: unknown supply/state never becomes normal voltage.
use crate::electrical_distribution::{self, Contact, Distribution, Feed};
use crate::electrical_sources::{Battery, Generator};
use fsx_vehicle::electrical::{Bus, Electrical};
use std::collections::BTreeMap;
#[derive(Debug, Clone, PartialEq)]
pub struct CircuitReading {
    pub name: String,
    pub bus: Bus,
    pub requested: Option<bool>,
    pub powered: Option<bool>,
    pub voltage: Option<f64>,
    pub demand_amps: Option<f64>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub circuits: Vec<CircuitReading>,
    pub authored_bus_demand_amps: [Option<f64>; 8],
}
#[derive(Debug, Clone)]
pub struct CircuitNetwork {
    pub battery: Battery,
    pub generator: Generator,
    configuration: Electrical,
    requests: BTreeMap<String, bool>,
    voltage: [Option<f64>; 8],
}
impl CircuitNetwork {
    pub fn new(configuration: Electrical) -> Result<Self, String> {
        if configuration.circuits.len() > 256 {
            return Err("Electrical network exceeds circuit budget".into());
        }
        if [
            configuration.max_battery_voltage,
            configuration.generator_voltage,
        ]
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || *v < 0. || *v > 1000.)
            || configuration
                .max_generator_amps
                .is_some_and(|v| !v.is_finite() || !(0. ..=1e6).contains(&v))
            || configuration
                .engine_generator_map
                .as_ref()
                .is_some_and(|m| m.is_empty() || m.len() > 4 || m.iter().any(|&n| n > 4))
        {
            return Err("Invalid in-memory electrical source metadata".into());
        }
        for (name, circuit) in &configuration.circuits {
            if name.is_empty()
                || name.len() > 128
                || !name.is_ascii()
                || name != &name.to_ascii_lowercase()
            {
                return Err("Electrical circuit keys must be normalized bounded ASCII".into());
            }
            circuit.bus.index()?;
            if !circuit.max_amps.is_finite()
                || !(0. ..=1e6).contains(&circuit.max_amps)
                || !circuit.min_voltage.is_finite()
                || !(0. ..=1000.).contains(&circuit.min_voltage)
            {
                return Err("Invalid in-memory electrical circuit".into());
            }
        }
        Ok(Self {
            battery: Battery::new(&configuration)?,
            generator: Generator::new(&configuration)?,
            configuration,
            requests: BTreeMap::new(),
            voltage: [None; 8],
        })
    }
    pub fn reset(&mut self) {
        // Configuration was validated by new and remains immutable.
        self.battery = Battery::new(&self.configuration).unwrap();
        self.generator = Generator::new(&self.configuration).unwrap();
        self.requests.clear();
        self.voltage = [None; 8];
    }
    pub fn set_bus_voltage(&mut self, bus: Bus, value: Option<f64>) -> Result<(), String> {
        let index = bus.index()?;
        if value.is_some_and(|v| !v.is_finite() || !(0. ..=1000.).contains(&v)) {
            return Err("Invalid bus voltage observation".into());
        }
        self.voltage[index] = value;
        Ok(())
    }
    pub fn bus_voltage(&self, bus: Bus) -> Result<Option<f64>, String> {
        Ok(self.voltage[bus.index()?])
    }
    /// Resolve an explicit complete inventory and publish all bus results
    /// atomically. Does not use authored circuit maxima as actual load or
    /// silently select aircraft source ratings/topology. Call only when a
    /// provider can supply the full inventory, including disconnected buses.
    pub fn distribute(
        &mut self,
        feeds: &[Feed],
        contacts: &[Contact],
        requested_amps: [Option<f64>; 8],
    ) -> Result<Distribution, String> {
        let result = electrical_distribution::solve(feeds, contacts, requested_amps)?;
        self.voltage = result.bus_voltage;
        Ok(result)
    }
    pub fn request(&mut self, circuit: &str, enabled: Option<bool>) -> Result<(), String> {
        if circuit.len() > 128 {
            return Err("Electrical circuit request name exceeds budget".into());
        }
        let circuit = circuit.to_ascii_lowercase();
        if !self.configuration.circuits.contains_key(&circuit) {
            return Err("Unknown/unresolved electrical circuit".into());
        }
        if let Some(enabled) = enabled {
            self.requests.insert(circuit, enabled);
        } else {
            self.requests.remove(&circuit);
        }
        Ok(())
    }
    pub fn snapshot(&self) -> Result<Snapshot, String> {
        if self.configuration.always_available == Some(true) {
            return Err(
                "Always-available electrical override needs separate supply semantics".into(),
            );
        }
        let mut circuits = Vec::new();
        let mut authored_bus_demand_amps = [Some(0.); 8];
        for (name, circuit) in &self.configuration.circuits {
            let index = circuit.bus.index()?;
            let voltage = self.voltage[index];
            let requested = self.requests.get(name).copied();
            let powered = if requested == Some(false)
                || voltage.is_some_and(|v| v == 0. || v < circuit.min_voltage)
            {
                Some(false)
            } else if requested == Some(true) && voltage.is_some() {
                Some(true)
            } else {
                None
            };
            let demand = if circuit.max_amps == 0. {
                Some(0.)
            } else {
                powered.map(|on| if on { circuit.max_amps } else { 0. })
            };
            authored_bus_demand_amps[index] = authored_bus_demand_amps[index]
                .zip(demand)
                .map(|(sum, value)| sum + value);
            circuits.push(CircuitReading {
                name: name.clone(),
                bus: circuit.bus,
                requested,
                powered,
                voltage,
                demand_amps: demand,
            });
        }
        Ok(Snapshot {
            circuits,
            authored_bus_demand_amps,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use fsx_vehicle::electrical::Circuit;
    fn network() -> CircuitNetwork {
        let mut configuration = Electrical::default();
        for (name, bus, amps, min) in [
            ("pitot_heat", Bus::Main, 2., 17.),
            ("light_nav", Bus::Main, 5., 17.),
            ("avionics", Bus::Avionics, 5., 17.),
        ] {
            configuration.circuits.insert(
                name.into(),
                Circuit {
                    bus,
                    max_amps: amps,
                    min_voltage: min,
                },
            );
        }
        CircuitNetwork::new(configuration).unwrap()
    }
    #[test]
    fn unknown_supply_or_switch_is_not_silently_off_or_normal_voltage() {
        let mut n = network();
        assert_eq!(n.bus_voltage(Bus::Main).unwrap(), None);
        assert_eq!(n.snapshot().unwrap().authored_bus_demand_amps[0], None);
        n.request("pitot_heat", Some(true)).unwrap();
        assert_eq!(
            n.snapshot()
                .unwrap()
                .circuits
                .iter()
                .find(|c| c.name == "pitot_heat")
                .unwrap()
                .powered,
            None
        );
        n.request("light_nav", Some(false)).unwrap();
        n.set_bus_voltage(Bus::Main, Some(17.)).unwrap();
        assert_eq!(n.snapshot().unwrap().authored_bus_demand_amps[0], Some(2.));
        n.request("light_nav", Some(true)).unwrap();
        assert_eq!(n.snapshot().unwrap().authored_bus_demand_amps[0], Some(7.));
        n.set_bus_voltage(Bus::Main, Some(16.99)).unwrap();
        assert_eq!(n.snapshot().unwrap().authored_bus_demand_amps[0], Some(0.));
        assert_eq!(n.snapshot().unwrap().authored_bus_demand_amps[1], None);
    }
    #[test]
    fn invalid_requests_are_atomic_and_reset_restores_unavailable_observations() {
        let mut n = network();
        n.set_bus_voltage(Bus::Main, Some(28.)).unwrap();
        let before = n.snapshot().unwrap();
        assert!(n.set_bus_voltage(Bus::Main, Some(f64::NAN)).is_err());
        assert!(n.request("not_a_circuit", Some(true)).is_err());
        assert_eq!(n.snapshot().unwrap(), before);
        n.set_bus_voltage(Bus::Main, Some(0.)).unwrap();
        assert_eq!(n.snapshot().unwrap().authored_bus_demand_amps[0], Some(0.));
        n.reset();
        assert_eq!(n.bus_voltage(Bus::Main).unwrap(), None);
        assert_eq!(n.snapshot().unwrap().authored_bus_demand_amps[0], None);
    }
    #[test]
    fn invalid_memory_configuration_and_unimplemented_power_override_are_explicit() {
        let mut config = Electrical::default();
        config.circuits.insert(
            "pitot_heat".into(),
            Circuit {
                bus: Bus::Main,
                max_amps: f64::NAN,
                min_voltage: 17.,
            },
        );
        assert!(CircuitNetwork::new(config.clone()).is_err());
        config.circuits.get_mut("pitot_heat").unwrap().max_amps = 2.;
        config.always_available = Some(true);
        assert!(CircuitNetwork::new(config).unwrap().snapshot().is_err());
    }
    #[test]
    fn source_state_and_curves_reset_without_publishing_a_bus_voltage() {
        use crate::electrical_sources::VoltageCurve;
        let mut n = CircuitNetwork::new(Electrical {
            max_battery_voltage: Some(24.),
            battery_max_capacity_amph: Some(40.),
            generator_voltage: Some(28.),
            max_generator_amps: Some(60.),
            ..Electrical::default()
        })
        .unwrap();
        n.battery.set_charge_fraction(Some(1.)).unwrap();
        n.battery
            .set_voltage_curve(Some(VoltageCurve::new(vec![(0., 18.), (1., 24.)]).unwrap()))
            .unwrap();
        n.battery.advance(3600., Some(10.)).unwrap();
        n.generator.set_online(Some(true));
        assert_eq!(n.generator.supplied_voltage(Some(7.)).unwrap(), Some(28.));
        assert_eq!(n.battery.open_circuit_voltage(), Some(22.5));
        assert_eq!(n.bus_voltage(Bus::Main).unwrap(), None);
        n.reset();
        assert_eq!(n.battery.charge_fraction(), None);
        assert_eq!(n.generator.supplied_voltage(Some(7.)).unwrap(), None);
        n.battery.set_charge_fraction(Some(0.5)).unwrap();
        assert_eq!(n.battery.open_circuit_voltage(), None);
        n.battery.set_charge_fraction(Some(1.)).unwrap();
        assert_eq!(n.battery.open_circuit_voltage(), Some(24.));
    }
    #[test]
    fn explicit_distribution_is_atomic_and_updates_authored_circuit_power() {
        let mut n = network();
        for name in ["pitot_heat", "light_nav", "avionics"] {
            n.request(name, Some(true)).unwrap();
        }
        let config = Electrical {
            generator_voltage: Some(28.),
            max_generator_amps: Some(60.),
            ..Electrical::default()
        };
        n.generator = Generator::new(&config).unwrap();
        n.generator.set_online(Some(true));
        let contact = Contact {
            from: Bus::Generator(1),
            to: Bus::Main,
            closed: Some(true),
        };
        let feeds = [n.generator.feed(Bus::Generator(1)).unwrap()];
        let mut requests = [Some(0.); 8];
        requests[0] = Some(7.);
        let d = n.distribute(&feeds, &[contact], requests).unwrap();
        assert_eq!(d.feed_amps, vec![Some(7.)]);
        assert_eq!(n.snapshot().unwrap().authored_bus_demand_amps[0], Some(7.));
        assert_eq!(n.snapshot().unwrap().authored_bus_demand_amps[1], Some(0.));
        let before = n.snapshot().unwrap();
        requests[1] = Some(f64::NAN);
        assert!(n.distribute(&feeds, &[contact], requests).is_err());
        assert_eq!(n.snapshot().unwrap(), before);
        requests[1] = Some(0.);
        let unknown = Contact {
            closed: None,
            ..contact
        };
        n.distribute(&feeds, &[unknown], requests).unwrap();
        assert_eq!(n.bus_voltage(Bus::Main).unwrap(), None);
        assert_eq!(n.snapshot().unwrap().authored_bus_demand_amps[0], None);
        n.reset();
        assert_eq!(n.bus_voltage(Bus::Main).unwrap(), None);
    }
}
