//! Legacy circuit loads from known bus observations and explicit device requests.
//! Explicit source primitives remain separate from bus ties and overload voltage
//! response: unknown supply/state never becomes normal voltage.
use crate::electrical_distribution::{self, Contact, Distribution, Feed};
use crate::electrical_sources::{Battery, BatteryFlow, Generator, LegacyGeneratorInputs};
use fsx_vehicle::electrical::{Bus, Electrical, SourceDefaults};
use fsx_vehicle::flight_electrical::FlightElectrical;
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
    source_defaults: SourceDefaults,
    requests: BTreeMap<String, bool>,
    voltage: [Option<f64>; 8],
    switches: FlightElectrical,
    battery_current_request: Option<f64>,
    last_battery_flow: Option<BatteryFlow>,
    voltage_core: crate::legacy_voltage::Core,
}
impl CircuitNetwork {
    /// Explicit fresh original allocation + initialization, using the pinned
    /// FSX Steam single-piston source profile. This is not FLT restoration or a
    /// guess about an already running aircraft. Initial bus readings precede
    /// admission/failure/avionics: even offline generator buses contain the
    /// battery maximum until the first frame. No Coulomb charge is inferred.
    pub fn new_fresh_legacy(configuration: Electrical) -> Result<Self, String> {
        let mut network = Self::new_with_source_defaults(
            configuration,
            SourceDefaults::FsxSteam62615SinglePiston,
        )?;
        let mut core = crate::legacy_voltage::Core::constructed();
        network.voltage = core.initialize_rated_voltage(&network.source_configuration())?;
        network.voltage_core = core;
        network.switches.battery = Some(false);
        network.generator.set_online(Some(false));
        Ok(network)
    }
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
            source_defaults: SourceDefaults::Unresolved,
            requests: BTreeMap::new(),
            voltage: [None; 8],
            switches: FlightElectrical::default(),
            battery_current_request: None,
            last_battery_flow: None,
            voltage_core: crate::legacy_voltage::Core::default(),
        })
    }
    pub fn new_with_source_defaults(
        configuration: Electrical,
        profile: SourceDefaults,
    ) -> Result<Self, String> {
        let mut network = Self::new(configuration)?;
        network.source_defaults = profile;
        let effective = network.source_configuration();
        network.battery = Battery::new(&effective)?;
        network.generator = Generator::new(&effective)?;
        Ok(network)
    }
    /// Effective source configuration; the original authored data stays intact.
    pub fn source_configuration(&self) -> Electrical {
        self.configuration
            .with_source_defaults(self.source_defaults)
    }
    pub fn reset(&mut self) {
        // Configuration was validated by new and remains immutable.
        let effective = self.source_configuration();
        self.battery = Battery::new(&effective).unwrap();
        self.generator = Generator::new(&effective).unwrap();
        self.requests.clear();
        self.voltage = [None; 8];
        self.switches = FlightElectrical::default();
        self.battery_current_request = None;
        self.last_battery_flow = None;
        self.voltage_core = crate::legacy_voltage::Core::default();
    }
    pub fn legacy_voltage_state(&self) -> Option<crate::legacy_voltage::State> {
        self.voltage_core.state()
    }
    pub fn seed_legacy_voltage_state(
        &mut self,
        state: Option<crate::legacy_voltage::State>,
    ) -> Result<(), String> {
        if self.source_defaults != SourceDefaults::FsxSteam62615SinglePiston {
            return Err(
                "Voltage core requires the explicit FSX Steam single-piston profile".into(),
            );
        }
        self.voltage_core.seed(state)
    }
    /// Advance the checked pre-failure/avionics core only. The response factor
    /// and load require providers; outputs are not published as loaded buses or
    /// converted into Coulomb charge. Ordinary Simulation.step does not call it.
    pub fn step_legacy_voltage_core(
        &mut self,
        dt: f64,
        load_amps: Option<f64>,
        response_factor: Option<f32>,
    ) -> Result<Option<crate::legacy_voltage::Step>, String> {
        if self.source_defaults != SourceDefaults::FsxSteam62615SinglePiston {
            return Err(
                "Voltage core requires the explicit FSX Steam single-piston profile".into(),
            );
        }
        let feed = self.generator.feed(Bus::Generator(1))?;
        let mut config = self.source_configuration();
        config.generator_voltage = feed.voltage;
        config.max_generator_amps = feed.capacity_amps;
        self.voltage_core.advance(
            &config,
            crate::legacy_voltage::Inputs {
                generator_online: feed.online,
                battery_master: self.switches.battery,
                load_amps,
                response_factor,
            },
            dt,
        )
    }
    /// Explicit known-input frame path. Mode/override provenance, complete load,
    /// response factor, circuit presence and surrounding bus inputs must come
    /// from providers. No ordinary physics call selects these values implicitly.
    pub fn step_legacy_electrical_frame(
        &mut self,
        dt: f64,
        load_amps: Option<f64>,
        response_factor: Option<f32>,
        tail: Option<crate::legacy_voltage::TailEnvironment>,
    ) -> Result<Option<crate::legacy_voltage::FrameStep>, String> {
        if self.source_defaults != SourceDefaults::FsxSteam62615SinglePiston {
            return Err("Electrical frame requires the versioned single-piston profile".into());
        }
        let feed = self.generator.feed(Bus::Generator(1))?;
        let mut config = self.source_configuration();
        config.generator_voltage = feed.voltage;
        config.max_generator_amps = feed.capacity_amps;
        let mut staged = self.voltage_core.clone();
        let output = staged.advance_frame(
            &config,
            crate::legacy_voltage::Inputs {
                generator_online: feed.online,
                battery_master: self.switches.battery,
                load_amps,
                response_factor,
            },
            tail,
            dt,
        )?;
        let voltages = if let Some(frame) = output {
            if frame
                .tail
                .bus_voltage
                .iter()
                .any(|v| !(0. ..=1000.).contains(v))
            {
                return Err("Computed electrical frame is outside observed bus envelope".into());
            }
            frame.tail.bus_voltage.map(Some)
        } else {
            [None; 8]
        };
        self.voltage_core = staged;
        self.voltage = voltages;
        self.battery_current_request = None;
        self.last_battery_flow = None;
        Ok(output)
    }
    pub fn switches(&self) -> &FlightElectrical {
        &self.switches
    }
    /// Bridge the stored first-engine FLT switch to versioned admission inputs.
    /// Providers must supply RPM / max_rated_rpm, not throttle or raw RPM.
    /// Recompute bus distribution and current allocation afterwards.
    pub fn update_generator_from_fsx_steam_signals(
        &mut self,
        source_enabled: Option<bool>,
        rpm_fraction: Option<f64>,
        failed: Option<bool>,
    ) -> Result<Option<bool>, String> {
        if self.source_defaults != SourceDefaults::FsxSteam62615SinglePiston {
            return Err(
                "Generator admission requires the explicit FSX Steam single-piston profile".into(),
            );
        }
        let online = self
            .generator
            .update_fsx_steam_62615_availability(LegacyGeneratorInputs {
                source_enabled,
                rpm_fraction,
                failed,
                switch_on: self.switches.generators[0],
            })?;
        self.voltage = [None; 8];
        self.battery_current_request = None;
        self.last_battery_flow = None;
        Ok(online)
    }
    /// Explicit one-time use of a saved FLT RPM fraction. This is not a live
    /// engine provider; never call automatically on subsequent physics frames.
    pub fn update_generator_from_saved_flt(
        &mut self,
        source_enabled: Option<bool>,
        failed: Option<bool>,
    ) -> Result<Option<bool>, String> {
        self.update_generator_from_fsx_steam_signals(
            source_enabled,
            self.switches.engine_rpm_fraction[0],
            failed,
        )
    }
    /// Versioned single-piston allocation phase with independent actual-load
    /// input. Does not use authored CFG maxima or apply the result to a battery,
    /// bus observation or physics step. Full original frame timing remains open.
    pub fn fsx_steam_generator_current_request(
        &self,
        load_amps: Option<f64>,
        battery_voltage: Option<f64>,
    ) -> Result<Option<f64>, String> {
        if self.source_defaults != SourceDefaults::FsxSteam62615SinglePiston {
            return Err("Generator current allocation requires the explicit FSX Steam single-piston profile".into());
        }
        self.generator.fsx_steam_62615_allocated_current(
            load_amps,
            battery_voltage,
            self.source_configuration().max_battery_voltage,
        )
    }
    /// Replace FLT switch positions and saved RPM; missing mapped requests become
    /// unknown, absent aircraft circuits stay absent. Switches alone never
    /// energize buses or establish generator availability/terminal current.
    pub fn apply_switches(&mut self, switches: &FlightElectrical) {
        for name in FlightElectrical::mapped_circuits() {
            self.requests.remove(name);
        }
        for (name, &requested) in switches.requests() {
            if self.configuration.circuits.contains_key(name) {
                self.requests.insert(name.clone(), requested);
            }
        }
        self.switches = switches.clone();
        self.voltage = [None; 8];
        self.generator.set_online(None);
        self.battery_current_request = None;
        self.last_battery_flow = None;
    }
    /// Explicit source-provider request, independent of CFG consumer maxima.
    /// None clears knowledge on the next positive simulation step.
    pub fn set_battery_current_request(&mut self, amps: Option<f64>) -> Result<(), String> {
        if amps.is_some_and(|a| !a.is_finite() || a.abs() > 1e6) {
            return Err("Invalid battery current request".into());
        }
        self.battery_current_request = amps;
        Ok(())
    }
    pub fn last_battery_flow(&self) -> Option<BatteryFlow> {
        self.last_battery_flow
    }
    /// Called only for committed simulation intervals, not wall time or pause.
    pub fn advance_time(&mut self, seconds: f64) -> Result<(), String> {
        let flow = self
            .battery
            .advance(seconds, self.battery_current_request)?;
        if seconds > 0. {
            self.last_battery_flow = Some(flow);
        }
        Ok(())
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
        let voltage_checks_disabled = self.configuration.max_battery_voltage == Some(0.);
        for (name, circuit) in &self.configuration.circuits {
            let index = circuit.bus.index()?;
            let voltage = self.voltage[index];
            let requested = self.requests.get(name).copied();
            let powered = if requested == Some(false) {
                Some(false)
            } else if voltage_checks_disabled {
                requested
            } else if voltage.is_some_and(|v| v <= circuit.min_voltage) {
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
        assert_eq!(n.snapshot().unwrap().authored_bus_demand_amps[0], Some(0.));
        n.set_bus_voltage(Bus::Main, Some(f64::from_bits(17f64.to_bits() + 1)))
            .unwrap();
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
    #[test]
    fn clearing_terminal_current_and_invalid_requests_have_explicit_lifecycle() {
        let config = Electrical {
            battery_max_capacity_amph: Some(40.),
            ..Electrical::default()
        };
        let mut n = CircuitNetwork::new(config).unwrap();
        n.battery.set_charge_fraction(Some(1.)).unwrap();
        n.set_battery_current_request(Some(36.)).unwrap();
        assert!(n.set_battery_current_request(Some(f64::NAN)).is_err());
        assert!(n.advance_time(-1.).is_err());
        assert_eq!(n.battery.charge_fraction(), Some(1.));
        assert_eq!(n.last_battery_flow(), None);
        n.advance_time(1.).unwrap();
        assert_eq!(n.battery.charge_fraction(), Some(0.99975));
        let flow = n.last_battery_flow();
        n.advance_time(0.).unwrap();
        assert_eq!(n.last_battery_flow(), flow);
        n.set_battery_current_request(None).unwrap();
        n.advance_time(1.).unwrap();
        assert_eq!(n.battery.charge_fraction(), None);
        n.reset();
        assert_eq!(n.last_battery_flow(), None);
        n.battery.set_charge_fraction(Some(1.)).unwrap();
        n.set_battery_current_request(Some(36.)).unwrap();
        let mut switches = FlightElectrical::default();
        switches.battery = Some(false);
        n.apply_switches(&switches);
        assert_eq!(n.switches().battery, Some(false));
        n.advance_time(1.).unwrap();
        assert_eq!(n.battery.charge_fraction(), None);
    }
    #[test]
    fn source_profile_survives_reset_without_inventing_charge_or_online_generation() {
        let mut n = CircuitNetwork::new_with_source_defaults(
            Electrical::default(),
            SourceDefaults::FsxSteam62615SinglePiston,
        )
        .unwrap();
        assert_eq!(n.configuration.generator_voltage, None);
        assert_eq!(n.source_configuration().generator_voltage, Some(28.));
        assert_eq!(n.generator.supplied_voltage(Some(7.)).unwrap(), None);
        assert_eq!(n.battery.open_circuit_voltage(), None);
        n.generator.set_online(Some(true));
        assert_eq!(n.generator.supplied_voltage(Some(60.)).unwrap(), Some(28.));
        assert_eq!(n.generator.supplied_voltage(Some(60.01)).unwrap(), None);
        n.reset();
        assert_eq!(n.generator.supplied_voltage(Some(7.)).unwrap(), None);
        assert_eq!(n.source_configuration().max_generator_amps, Some(60.));
        assert_eq!(n.bus_voltage(Bus::Main).unwrap(), None);
        n.battery.set_charge_fraction(Some(1.)).unwrap();
        assert_eq!(n.battery.open_circuit_voltage(), Some(24.));
    }
    #[test]
    fn stored_switch_and_versioned_signals_invalidate_stale_supply_only_after_valid_update() {
        let mut n = CircuitNetwork::new_with_source_defaults(
            Electrical {
                battery_max_capacity_amph: Some(40.),
                ..Default::default()
            },
            SourceDefaults::FsxSteam62615SinglePiston,
        )
        .unwrap();
        let mut switches = FlightElectrical::default();
        switches.generators[0] = Some(true);
        n.apply_switches(&switches);
        assert_eq!(
            n.update_generator_from_fsx_steam_signals(Some(true), Some(0.1), Some(false))
                .unwrap(),
            Some(false)
        );
        n.set_bus_voltage(Bus::Main, Some(24.)).unwrap();
        n.battery.set_charge_fraction(Some(1.)).unwrap();
        n.set_battery_current_request(Some(36.)).unwrap();
        assert!(n
            .update_generator_from_fsx_steam_signals(Some(true), Some(f64::NAN), Some(false))
            .is_err());
        assert_eq!(n.bus_voltage(Bus::Main).unwrap(), Some(24.));
        n.advance_time(1.).unwrap();
        assert_eq!(n.battery.charge_fraction(), Some(0.99975));
        assert_eq!(
            n.update_generator_from_fsx_steam_signals(Some(true), Some(0.9), Some(false))
                .unwrap(),
            Some(true)
        );
        assert_eq!(n.bus_voltage(Bus::Main).unwrap(), None);
        assert_eq!(n.last_battery_flow(), None);
        n.advance_time(1.).unwrap();
        assert_eq!(n.battery.charge_fraction(), None);
        switches.generators[0] = Some(false);
        n.apply_switches(&switches);
        assert_eq!(
            n.update_generator_from_fsx_steam_signals(Some(true), Some(0.9), Some(false))
                .unwrap(),
            Some(false)
        );
        let mut unresolved = CircuitNetwork::new(Electrical::default()).unwrap();
        assert!(unresolved
            .update_generator_from_fsx_steam_signals(Some(true), Some(0.9), Some(false))
            .is_err());
    }
    #[test]
    fn saved_flt_rpm_and_switch_require_independent_source_and_failure_inputs() {
        let mut n = CircuitNetwork::new_with_source_defaults(
            Electrical::default(),
            SourceDefaults::FsxSteam62615SinglePiston,
        )
        .unwrap();
        let mut saved = FlightElectrical::default();
        saved.generators[0] = Some(true);
        saved.engine_rpm_fraction[0] = Some(0.887);
        n.apply_switches(&saved);
        assert_eq!(
            n.update_generator_from_saved_flt(None, Some(false))
                .unwrap(),
            None
        );
        assert_eq!(
            n.update_generator_from_saved_flt(Some(true), None).unwrap(),
            None
        );
        assert_eq!(
            n.update_generator_from_saved_flt(Some(true), Some(false))
                .unwrap(),
            Some(true)
        );
        assert_eq!(n.bus_voltage(Bus::Main).unwrap(), None);
        saved.engine_rpm_fraction[0] = Some(0.1);
        n.apply_switches(&saved);
        assert_eq!(
            n.update_generator_from_saved_flt(Some(true), Some(false))
                .unwrap(),
            Some(false)
        );
        saved.engine_rpm_fraction[0] = None;
        n.apply_switches(&saved);
        assert_eq!(
            n.update_generator_from_saved_flt(Some(true), Some(false))
                .unwrap(),
            None
        );
    }
    #[test]
    fn current_request_uses_effective_profile_and_never_publishes_supply_or_charge() {
        let mut n = CircuitNetwork::new_with_source_defaults(
            Electrical::default(),
            SourceDefaults::FsxSteam62615SinglePiston,
        )
        .unwrap();
        let mut saved = FlightElectrical::default();
        saved.generators[0] = Some(true);
        saved.engine_rpm_fraction[0] = Some(0.9);
        n.apply_switches(&saved);
        assert_eq!(
            n.fsx_steam_generator_current_request(Some(12.), Some(22.))
                .unwrap(),
            None
        );
        n.update_generator_from_saved_flt(Some(true), Some(false))
            .unwrap();
        assert_eq!(
            n.fsx_steam_generator_current_request(Some(12.), Some(22.))
                .unwrap(),
            Some(14.)
        );
        assert_eq!(n.bus_voltage(Bus::Main).unwrap(), None);
        assert_eq!(n.battery.charge_fraction(), None);
        assert_eq!(n.last_battery_flow(), None);
        let unresolved = CircuitNetwork::new(Electrical::default()).unwrap();
        assert!(unresolved
            .fsx_steam_generator_current_request(Some(12.), Some(22.))
            .is_err());
    }
    #[test]
    fn explicit_voltage_core_uses_stored_switches_and_resets_without_publishing_buses() {
        let mut n = CircuitNetwork::new_with_source_defaults(
            Electrical::default(),
            SourceDefaults::FsxSteam62615SinglePiston,
        )
        .unwrap();
        let mut saved = FlightElectrical::default();
        saved.battery = Some(true);
        saved.generators[0] = Some(true);
        saved.engine_rpm_fraction[0] = Some(0.9);
        n.apply_switches(&saved);
        n.update_generator_from_saved_flt(Some(true), Some(false))
            .unwrap();
        n.seed_legacy_voltage_state(Some(crate::legacy_voltage::State {
            battery_voltage: 22.,
            filtered_net_amps: 0.,
            generator_amps: 0.,
        }))
        .unwrap();
        let s = n
            .step_legacy_voltage_core(1. / 120., Some(12.), Some(1.))
            .unwrap()
            .unwrap();
        assert!(s.state.battery_voltage < 22.);
        assert_eq!(n.bus_voltage(Bus::Main).unwrap(), None);
        assert_eq!(n.battery.charge_fraction(), None);
        assert_eq!(n.last_battery_flow(), None);
        assert!(n
            .step_legacy_voltage_core(1. / 120., Some(12.), Some(f32::NAN))
            .is_err());
        assert_eq!(n.legacy_voltage_state(), Some(s.state));
        assert_eq!(
            n.step_legacy_voltage_core(1. / 120., Some(12.), None)
                .unwrap(),
            None
        );
        n.reset();
        assert_eq!(n.legacy_voltage_state(), None);
        let mut unresolved = CircuitNetwork::new(Electrical::default()).unwrap();
        assert!(unresolved.seed_legacy_voltage_state(None).is_err());
    }
    #[test]
    fn complete_known_frame_publishes_tail_outputs_and_missing_context_clears_stale_buses() {
        let mut n = CircuitNetwork::new_with_source_defaults(
            Electrical::default(),
            SourceDefaults::FsxSteam62615SinglePiston,
        )
        .unwrap();
        let mut saved = FlightElectrical::default();
        saved.battery = Some(true);
        saved.generators[0] = Some(true);
        saved.engine_rpm_fraction[0] = Some(0.9);
        n.apply_switches(&saved);
        n.update_generator_from_saved_flt(Some(true), Some(false))
            .unwrap();
        n.seed_legacy_voltage_state(Some(crate::legacy_voltage::State {
            battery_voltage: 22.,
            filtered_net_amps: 0.,
            generator_amps: 0.,
        }))
        .unwrap();
        let tail = crate::legacy_voltage::TailEnvironment {
            generator_enabled: true,
            incoming_bus_voltage: [0.; 8],
            incoming_generator_amps: [0.; 4],
            battery_failed: false,
            avionics_master: true,
            voltage_mode: true,
            override_power: false,
            avionics_circuit: Some(crate::legacy_power::Circuit {
                bus: 0,
                min_voltage: 17.,
                forced_off: false,
            }),
            additional_system_circuit: None,
        };
        n.step_legacy_electrical_frame(1. / 120., Some(12.), Some(1.), Some(tail))
            .unwrap();
        assert_eq!(n.bus_voltage(Bus::Main).unwrap(), Some(28.));
        assert_eq!(n.bus_voltage(Bus::Avionics).unwrap(), Some(28.));
        let state = n.legacy_voltage_state();
        assert!(n
            .step_legacy_electrical_frame(0., Some(12.), Some(1.), Some(tail))
            .is_err());
        assert_eq!(n.legacy_voltage_state(), state);
        n.step_legacy_electrical_frame(
            1. / 120.,
            Some(12.),
            Some(1.),
            Some(crate::legacy_voltage::TailEnvironment {
                battery_failed: true,
                ..tail
            }),
        )
        .unwrap();
        assert_eq!(n.bus_voltage(Bus::Main).unwrap(), Some(0.));
        assert_eq!(
            n.step_legacy_electrical_frame(1. / 120., Some(12.), Some(1.), None)
                .unwrap(),
            None
        );
        assert_eq!(n.bus_voltage(Bus::Main).unwrap(), None);
    }
    #[test]
    fn authored_zero_battery_rating_bypasses_voltage_checks_but_not_unknown_device_requests() {
        let mut cfg = Electrical {
            max_battery_voltage: Some(0.),
            ..Default::default()
        };
        cfg.circuits.insert(
            "pitot_heat".into(),
            Circuit {
                bus: Bus::Main,
                max_amps: 2.,
                min_voltage: 17.,
            },
        );
        let mut n = CircuitNetwork::new(cfg).unwrap();
        assert_eq!(n.snapshot().unwrap().circuits[0].powered, None);
        n.request("pitot_heat", Some(true)).unwrap();
        n.set_bus_voltage(Bus::Main, Some(0.)).unwrap();
        assert_eq!(n.snapshot().unwrap().circuits[0].powered, Some(true));
        n.request("pitot_heat", Some(false)).unwrap();
        assert_eq!(n.snapshot().unwrap().circuits[0].powered, Some(false));
    }
}
