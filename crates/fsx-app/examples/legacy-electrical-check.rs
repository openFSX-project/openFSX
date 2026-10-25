//! Installed authored circuit schema and explicit bus-observation runtime check.
//! Versioned source ratings are separate from unavailable live bus voltage.
use fsx_sim::electrical_distribution::Contact;
use fsx_sim::flight::Simulation;
use fsx_sim::{electrical::CircuitNetwork, electrical_sources::VoltageCurve};
use fsx_vehicle::{
    aircraft::{Aircraft, KNOT},
    electrical::{Bus, Electrical},
    flight_electrical::FlightElectrical,
};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
fn ensure(value: bool, message: &str) -> Result<(), String> {
    value.then_some(()).ok_or_else(|| message.into())
}
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("Usage: legacy-electrical-check <FSX root> <new output directory>".into());
    }
    let root = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    if output.exists() {
        return Err("Output directory must be new".into());
    }
    let aircraft = Aircraft::load(&root.join("SimObjects/Airplanes/C172"), 0, true)?;
    let configuration = &aircraft.parameters.electrical;
    ensure(
        configuration.circuits.len() == 17,
        "Expected 17 authored C172 circuits",
    )?;
    ensure(
        configuration.max_battery_voltage.is_none()
            && configuration.generator_voltage.is_none()
            && configuration.max_generator_amps.is_none()
            && configuration.battery_max_capacity_amph.is_none()
            && configuration.battery_charging_current.is_none(),
        "Unexpected authored C172 source parameters",
    )?;
    ensure(
        configuration.circuits["pitot_heat"].bus == Bus::Main
            && configuration.circuits["pitot_heat"].max_amps == 2.
            && configuration.circuits["pitot_heat"].min_voltage == 17.,
        "C172 pitot circuit metadata mismatch",
    )?;
    let mut simulation = Simulation::trimmed(aircraft.parameters.clone(), 90. * KNOT, 300.)?;
    let effective_sources = simulation.electrical.source_configuration();
    ensure(
        effective_sources.max_battery_voltage == Some(24.)
            && effective_sources.generator_voltage == Some(28.)
            && effective_sources.max_generator_amps == Some(60.)
            && effective_sources.battery_max_capacity_amph.is_none()
            && effective_sources.battery_charging_current.is_none(),
        "Versioned single-piston source-default profile mismatch",
    )?;
    let mut supplied_source = simulation.electrical.generator.clone();
    supplied_source.set_online(Some(true));
    ensure(
        supplied_source.supplied_voltage(Some(60.))? == Some(28.)
            && supplied_source.supplied_voltage(Some(60.01))?.is_none(),
        "Resolved source rating is not used by the generator model",
    )?;
    ensure(
        simulation.electrical.bus_voltage(Bus::Main)?.is_none(),
        "Initial voltage was fabricated",
    )?;
    let snapshot = simulation.electrical.snapshot()?;
    ensure(
        snapshot.authored_bus_demand_amps[0].is_none(),
        "Unknown requests/supply became zero current",
    )?;
    let serialize = |snapshot: &fsx_sim::electrical::Snapshot| -> Result<Value, String> {
        Ok(
            json!({"authored_bus_demand_amps":snapshot.authored_bus_demand_amps,
        "circuits":snapshot.circuits.iter().map(|c|Ok(json!({"name":c.name,"bus":c.bus.index()?,"requested":c.requested,"powered":c.powered,"voltage":c.voltage,"demand_amps":c.demand_amps}))).collect::<Result<Vec<_>,String>>()?}),
        )
    };
    let mut cases = vec![json!({"case":"unresolved","snapshot":serialize(&snapshot)?})];
    for name in configuration.circuits.keys() {
        simulation.electrical.request(name, Some(false))?;
    }
    simulation.electrical.request("PITOT_HEAT", Some(true))?;
    simulation.electrical.request("light_nav", Some(true))?;
    for (name, voltage, expected) in [
        ("below_minimum", 16.99, 0.),
        ("at_minimum", 17., 0.),
        ("above_minimum", f64::from_bits(17f64.to_bits() + 1), 7.),
        ("inspection_28v", 28., 7.),
        ("deenergized", 0., 0.),
    ] {
        simulation
            .electrical
            .set_bus_voltage(Bus::Main, Some(voltage))?;
        let snapshot = simulation.electrical.snapshot()?;
        ensure(
            snapshot.authored_bus_demand_amps[0] == Some(expected),
            "Authored maximum-load demand mismatch",
        )?;
        cases.push(
            json!({"case":name,"input_bus_voltage":voltage,"snapshot":serialize(&snapshot)?}),
        );
    }
    simulation.electrical.set_bus_voltage(Bus::Main, None)?;
    let unknown = simulation.electrical.snapshot()?;
    ensure(
        unknown.authored_bus_demand_amps[0].is_none(),
        "Lost voltage observation left stale current",
    )?;
    let before = unknown.clone();
    ensure(
        simulation
            .electrical
            .set_bus_voltage(Bus::Main, Some(f64::NAN))
            .is_err()
            && simulation
                .electrical
                .request("unknown", Some(true))
                .is_err(),
        "Invalid electrical request accepted",
    )?;
    ensure(
        simulation.electrical.snapshot()? == before,
        "Rejected electrical input changed state",
    )?;
    simulation.step(simulation.controls, 1. / 120.)?;
    ensure(
        simulation.electrical.snapshot()? == before,
        "Physics step fabricated electrical source observations",
    )?;
    let reset = Simulation::trimmed(aircraft.parameters.clone(), 90. * KNOT, 300.)?;
    ensure(
        reset.electrical.bus_voltage(Bus::Main)?.is_none(),
        "New simulation inherited an observed bus voltage",
    )?;
    cases.push(
        json!({"case":"fresh_simulation","snapshot":serialize(&reset.electrical.snapshot()?)?}),
    );
    let circuits=configuration.circuits.iter().map(|(name,c)|Ok(json!({"name":name,"bus":c.bus.index()?,"max_amps":c.max_amps,"min_voltage":c.min_voltage}))).collect::<Result<Vec<_>,String>>()?;
    ensure(
        simulation.electrical.battery.charge_fraction().is_none()
            && simulation
                .electrical
                .battery
                .open_circuit_voltage()
                .is_none()
            && simulation
                .electrical
                .generator
                .supplied_voltage(Some(7.))?
                .is_none(),
        "Installed sources acquired synthetic defaults",
    )?;
    // Independent fixture, never the installed C172's ratings or live state.
    let mut sources = CircuitNetwork::new(Electrical {
        battery_max_capacity_amph: Some(40.),
        battery_charging_current: Some(4.),
        max_battery_voltage: Some(24.),
        generator_voltage: Some(28.),
        max_generator_amps: Some(60.),
        ..Electrical::default()
    })?;
    sources
        .battery
        .set_voltage_curve(Some(VoltageCurve::new(vec![
            (0., 18.),
            (0.5, 22.),
            (1., 24.),
        ])?))?;
    sources.battery.set_charge_fraction(Some(1.))?;
    let discharge = sources.battery.advance(3600., Some(10.))?;
    ensure(
        sources.battery.charge_fraction() == Some(0.75)
            && sources.battery.open_circuit_voltage() == Some(23.),
        "Synthetic battery amp-hour/curve mismatch",
    )?;
    let discharged_fraction = sources.battery.charge_fraction();
    let charge = sources.battery.advance(3600., Some(-20.))?;
    ensure(
        charge
            .average_terminal_amps
            .is_some_and(|i| (i + 4.).abs() < 1e-10)
            && sources.battery.charge_fraction() == Some(0.85),
        "Synthetic battery charge limit mismatch",
    )?;
    let charged_fraction = sources.battery.charge_fraction();
    sources.generator.set_online(Some(true));
    let regulated = sources.generator.supplied_voltage(Some(60.))?;
    let overloaded = sources.generator.supplied_voltage(Some(61.))?;
    ensure(
        regulated == Some(28.) && overloaded.is_none() && sources.bus_voltage(Bus::Main)?.is_none(),
        "Synthetic generator inferred a bus or overload voltage",
    )?;
    sources.reset();
    ensure(
        sources.battery.charge_fraction().is_none()
            && sources.generator.supplied_voltage(Some(7.))?.is_none(),
        "Source reset retained charge/availability",
    )?;
    let synthetic_sources = json!({"inputs":"independent synthetic ratings; not C172 defaults or FSX reference data", "capacity_amph":40,"charge_limit_amps":4,
        "requested_discharge_amps":10,"discharge_seconds":3600,"average_discharge_amps":discharge.average_terminal_amps,"discharged_fraction":discharged_fraction,
        "requested_charge_amps":-20,"charge_seconds":3600,"average_charge_amps":charge.average_terminal_amps,"charged_fraction":charged_fraction,
        "generator_capacity_amps":60,"regulated_voltage_at_capacity":regulated,"overload_voltage":overloaded,"main_bus_voltage":sources.bus_voltage(Bus::Main)?,"reset_clears_state":true});
    let mut distribution_cases = Vec::new();
    for (
        name,
        online,
        main,
        avionics,
        main_load,
        expected_main,
        expected_avionics,
        expected_feed,
    ) in [
        (
            "closed",
            Some(true),
            Some(true),
            Some(true),
            Some(7.),
            Some(28.),
            Some(28.),
            Some(12.),
        ),
        (
            "avionics_open",
            Some(true),
            Some(true),
            Some(false),
            Some(7.),
            Some(28.),
            Some(0.),
            Some(7.),
        ),
        (
            "main_open",
            Some(true),
            Some(false),
            Some(true),
            Some(7.),
            Some(0.),
            Some(0.),
            Some(0.),
        ),
        (
            "main_unknown",
            Some(true),
            None,
            Some(true),
            Some(7.),
            None,
            None,
            None,
        ),
        (
            "overload",
            Some(true),
            Some(true),
            Some(true),
            Some(60.),
            None,
            None,
            None,
        ),
        (
            "load_unknown",
            Some(true),
            Some(true),
            Some(true),
            None,
            None,
            None,
            None,
        ),
        (
            "offline",
            Some(false),
            Some(true),
            Some(true),
            Some(7.),
            Some(0.),
            Some(0.),
            Some(0.),
        ),
    ] {
        sources.generator.set_online(online);
        let feeds = [sources.generator.feed(Bus::Generator(1))?];
        let contacts = [
            Contact {
                from: Bus::Generator(1),
                to: Bus::Main,
                closed: main,
            },
            Contact {
                from: Bus::Main,
                to: Bus::Avionics,
                closed: avionics,
            },
        ];
        let mut requested = [Some(0.); 8];
        requested[0] = main_load;
        requested[1] = Some(5.);
        let d = sources.distribute(&feeds, &contacts, requested)?;
        ensure(
            d.bus_voltage[0] == expected_main
                && d.bus_voltage[1] == expected_avionics
                && d.feed_amps == vec![expected_feed],
            "Explicit distribution voltage/current mismatch",
        )?;
        ensure(
            sources.bus_voltage(Bus::Main)? == expected_main,
            "Distribution was not published",
        )?;
        distribution_cases.push(json!({"case":name,"generator_online":online,"main_contact_closed":main,"avionics_contact_closed":avionics,"requested_amps":requested,"bus_voltage":d.bus_voltage,"delivered_amps":d.delivered_amps,"feed_amps":d.feed_amps}));
    }
    sources.reset();
    ensure(
        sources.bus_voltage(Bus::Main)?.is_none(),
        "Distribution survived reset",
    )?;
    let mut flight_switch_cases = Vec::new();
    for (path, expected_fraction) in [
        ("lessons/student/Lesson 1.FLT", 0.8870350550460396),
        ("flights/other/FLTSIM.FLT", 0.795762346073706),
    ] {
        let switches = FlightElectrical::load(&root.join(path), true)?;
        ensure(
            switches.battery == Some(true)
                && switches.avionics == Some(true)
                && switches.generators[0] == Some(true)
                && switches.engine_rpm_fraction[0] == Some(expected_fraction)
                && switches.requests().get("pitot_heat") == Some(&false),
            "Installed FLT switch profile mismatch",
        )?;
        simulation.electrical.apply_switches(&switches);
        ensure(
            simulation.electrical.switches() == &switches
                && simulation.electrical.bus_voltage(Bus::Main)?.is_none()
                && simulation
                    .electrical
                    .generator
                    .supplied_voltage(Some(7.))?
                    .is_none(),
            "FLT switch position invented source availability/voltage",
        )?;
        let snapshot = simulation.electrical.snapshot()?;
        ensure(
            snapshot
                .circuits
                .iter()
                .find(|c| c.name == "pitot_heat")
                .unwrap()
                .requested
                == Some(false),
            "FLT pitot request was not applied",
        )?;
        ensure(
            simulation
                .electrical
                .update_generator_from_saved_flt(None, Some(false))?
                .is_none(),
            "Saved RPM invented a source-enable input",
        )?;
        let saved_admitted = simulation
            .electrical
            .update_generator_from_saved_flt(Some(true), Some(false))?;
        ensure(
            saved_admitted == Some(true) && simulation.electrical.bus_voltage(Bus::Main)?.is_none(),
            "Saved FLT RPM admission mismatch or invented loaded bus voltage",
        )?;
        flight_switch_cases.push(json!({"file":path,"scope":"switch positions and saved RPM only; no aircraft/flight restoration",
            "battery":switches.battery,"avionics":switches.avionics,"generators":switches.generators,"engine_rpm_fraction":switches.engine_rpm_fraction,"requests":switches.requests(),"snapshot":serialize(&snapshot)?,
            "saved_state_admitted":saved_admitted,"explicit_source_enabled_input":true,"explicit_failed_input":false,"loaded_bus_voltage":null}));
    }
    let mut admission_cases = Vec::new();
    for (name, enabled, signal, failed, expected) in [
        (
            "at_boundary",
            Some(true),
            Some(0.1),
            Some(false),
            Some(false),
        ),
        (
            "above_boundary",
            Some(true),
            Some(f64::from_bits(0.1f64.to_bits() + 1)),
            Some(false),
            Some(true),
        ),
        ("failed", Some(true), Some(0.9), Some(true), Some(false)),
        ("failure_unknown", Some(true), Some(0.9), None, None),
        ("signal_unknown", Some(true), None, Some(false), None),
        (
            "source_disabled",
            Some(false),
            Some(0.9),
            Some(false),
            Some(false),
        ),
    ] {
        let online = simulation
            .electrical
            .update_generator_from_fsx_steam_signals(enabled, signal, failed)?;
        ensure(
            online == expected && simulation.electrical.bus_voltage(Bus::Main)?.is_none(),
            "Versioned admission fabricated online state or loaded bus voltage",
        )?;
        admission_cases.push(json!({"case":name,"source_enabled":enabled,"rpm_fraction":signal,"failed":failed,"stored_generator_switch":simulation.electrical.switches().generators[0],"online":online,"loaded_main_bus_voltage":null}));
    }
    let mut current_cases = Vec::new();
    for (name, enabled, load, battery_voltage, expected) in [
        ("balanced", true, Some(12.), Some(24.), Some(12.)),
        ("voltage_gap", true, Some(12.), Some(22.), Some(14.)),
        ("at_capacity", true, Some(60.), Some(24.), Some(60.)),
        ("overload", true, Some(120.), Some(22.), Some(60.)),
        (
            "above_max_voltage_fixture",
            true,
            Some(0.),
            Some(30.),
            Some(-6.),
        ),
        ("unknown_load", true, None, Some(22.), None),
        ("unknown_battery_voltage", true, Some(12.), None, None),
        ("offline", false, None, None, Some(0.)),
    ] {
        simulation
            .electrical
            .update_generator_from_fsx_steam_signals(Some(enabled), Some(0.9), Some(false))?;
        let allocated = simulation
            .electrical
            .fsx_steam_generator_current_request(load, battery_voltage)?;
        ensure(
            allocated == expected
                && simulation.electrical.bus_voltage(Bus::Main)?.is_none()
                && simulation.electrical.last_battery_flow().is_none(),
            "Current allocation phase mismatch or accidental bus/battery coupling",
        )?;
        current_cases.push(json!({"case":name,"source_enabled_input":enabled,"load_amps_input":load,"battery_voltage_input":battery_voltage,
            "allocated_generator_amps":allocated,"scope":"explicit allocation-phase fixture; not measured current or solved battery current","loaded_main_bus_voltage":null}));
    }
    simulation
        .electrical
        .apply_switches(&FlightElectrical::default());
    ensure(
        simulation
            .electrical
            .snapshot()?
            .circuits
            .iter()
            .find(|c| c.name == "pitot_heat")
            .unwrap()
            .requested
            .is_none(),
        "Missing FLT request inherited a prior switch",
    )?;
    // Independent configured fixture: installed C172 capacity remains unknown.
    let mut timed_parameters = aircraft.parameters.clone();
    timed_parameters.electrical.battery_max_capacity_amph = Some(40.);
    let mut timed = Simulation::trimmed(timed_parameters, 90. * KNOT, 300.)?;
    timed.electrical.battery.set_charge_fraction(Some(1.))?;
    timed.electrical.set_battery_current_request(Some(36.))?;
    for _ in 0..120 {
        timed.step(timed.controls, 1. / 120.)?;
    }
    ensure(
        (timed.electrical.battery.charge_fraction().unwrap() - 0.99975).abs() < 1e-12
            && timed.electrical.bus_voltage(Bus::Main)?.is_none(),
        "Synthetic flight-time battery conservation/unknown-bus mismatch",
    )?;
    let timed_fraction = timed.electrical.battery.charge_fraction();
    timed.electrical.set_battery_current_request(None)?;
    timed.step(timed.controls, 1. / 120.)?;
    ensure(
        timed.electrical.battery.charge_fraction().is_none(),
        "Lost terminal-current knowledge retained a charge estimate",
    )?;
    let mut frame_fixture = simulation.electrical.clone();
    let saved = FlightElectrical::load(&root.join("lessons/student/Lesson 1.FLT"), true)?;
    frame_fixture.apply_switches(&saved);
    frame_fixture.update_generator_from_saved_flt(Some(true), Some(false))?;
    frame_fixture.seed_legacy_voltage_state(Some(fsx_sim::legacy_voltage::State {
        battery_voltage: 22.,
        filtered_net_amps: 0.,
        generator_amps: 0.,
    }))?;
    let authored_avionics = &configuration.circuits["avionics_bus"];
    let tail = fsx_sim::legacy_voltage::TailEnvironment {
        generator_enabled: true,
        incoming_bus_voltage: [0.; 8],
        incoming_generator_amps: [0.; 4],
        battery_failed: false,
        avionics_master: true,
        voltage_mode: true,
        override_power: false,
        avionics_circuit: Some(fsx_sim::legacy_power::Circuit {
            bus: authored_avionics.bus.index()? as u32,
            min_voltage: authored_avionics.min_voltage,
            forced_off: false,
        }),
        navcom1_circuit: Some(fsx_sim::legacy_power::Circuit {
            bus: 1,
            min_voltage: 17.,
            forced_off: false,
        }),
    };
    let mut fresh = fsx_sim::electrical::CircuitNetwork::new_fresh_legacy(configuration.clone())?;
    let initial_voltage = effective_sources
        .max_battery_voltage
        .ok_or("Missing fresh rating")?;
    ensure(
        fresh.legacy_voltage_state()
            == Some(fsx_sim::legacy_voltage::State {
                battery_voltage: initial_voltage,
                filtered_net_amps: 0.,
                generator_amps: 0.,
            })
            && fresh.switches().battery == Some(false)
            && fresh.battery.charge_fraction().is_none(),
        "Fresh installed configuration did not initialize original state",
    )?;
    for bus in [
        Bus::Main,
        Bus::Avionics,
        Bus::Battery,
        Bus::HotBattery,
        Bus::Generator(1),
        Bus::Generator(2),
        Bus::Generator(3),
        Bus::Generator(4),
    ] {
        ensure(
            fresh.bus_voltage(bus)? == Some(initial_voltage),
            "Fresh original bus voltage differs",
        )?;
    }
    fresh.apply_switches(&saved);
    fresh.update_generator_from_saved_flt(Some(true), Some(false))?;
    let fresh_frame = fresh
        .step_legacy_electrical_frame(
            1. / 120.,
            Some(12.),
            Some(1.),
            Some(fsx_sim::legacy_voltage::TailEnvironment {
                incoming_bus_voltage: [initial_voltage; 8],
                ..tail
            }),
        )?
        .ok_or("Unknown fresh fixture frame")?;
    ensure(
        fresh_frame.tail.bus_voltage[0] == 28. && fresh_frame.tail.bus_voltage[1] == 28.,
        "Fresh fixture did not reach powered frame",
    )?;
    let healthy = frame_fixture
        .step_legacy_electrical_frame(1. / 120., Some(12.), Some(1.), Some(tail))?
        .ok_or("Unknown explicit frame fixture")?;
    ensure(
        frame_fixture.bus_voltage(Bus::Main)? == Some(28.)
            && frame_fixture.bus_voltage(Bus::Avionics)? == Some(28.)
            && healthy.tail.navcom1_active,
        "Known frame fixture did not publish calculated buses",
    )?;
    let failed = frame_fixture
        .step_legacy_electrical_frame(
            1. / 120.,
            Some(12.),
            Some(1.),
            Some(fsx_sim::legacy_voltage::TailEnvironment {
                battery_failed: true,
                ..tail
            }),
        )?
        .ok_or("Unknown failed frame fixture")?;
    ensure(
        failed.tail.bus_voltage == [0.; 8]
            && failed.state.generator_amps == 0.
            && failed.state.battery_voltage == failed.prefix.state.battery_voltage,
        "Failure tail lost state or retained stale source current",
    )?;
    ensure(
        frame_fixture
            .step_legacy_electrical_frame(1. / 120., Some(12.), Some(1.), None)?
            .is_none()
            && frame_fixture.bus_voltage(Bus::Main)?.is_none(),
        "Missing frame context retained stale published voltage",
    )?;
    let controlled_frame = json!({"scope":"explicit seed/load/response/mode/failure/bus observations and synthetic NAVCOM presence; not default flight providers",
        "healthy_main_voltage":healthy.tail.bus_voltage[0],"healthy_avionics_voltage":healthy.tail.bus_voltage[1],"failed_bus_voltage":failed.tail.bus_voltage,"failure_preserves_battery_state":true,"unknown_context_clears_bus_observation":true});
    let report = json!({"schema":10,"source":"legacy-electrical-authored-circuit-check","authored_circuits":circuits,"max_battery_voltage":configuration.max_battery_voltage,
        "fresh_allocation_fixture":{"initial_all_bus_voltage":initial_voltage,"initial_battery_master":false,"initial_filter_and_current":0.,"charge_fraction":null,"first_main_voltage":fresh_frame.tail.bus_voltage[0],"first_avionics_voltage":fresh_frame.tail.bus_voltage[1],"scope":"original fresh initialization, then saved FLT switches/RPM and explicit load/response/failure/context/NAVCOM inputs; not automatic flight restoration"},
        "effective_source_ratings":{"profile":"FsxSteam62615SinglePiston","max_battery_voltage":effective_sources.max_battery_voltage,"generator_voltage":effective_sources.generator_voltage,"max_generator_amps":effective_sources.max_generator_amps,
            "reference":"bounded original scalar initialization; not live generation or measured bus voltage","capacity_and_charge_limit_resolved":false},
        "battery_max_capacity_amph":configuration.battery_max_capacity_amph,"battery_charging_current":configuration.battery_charging_current,
        "generator_voltage":configuration.generator_voltage,"max_generator_amps":configuration.max_generator_amps,"engine_generator_map":configuration.engine_generator_map,"electric_always_available":configuration.always_available,
        "unsupported":configuration.unsupported,"cases":cases,"inputs":"explicit synthetic bus observations and requested authored loads",
        "synthetic_source_primitives":synthetic_sources,"synthetic_distribution_cases":distribution_cases,
        "distribution_inputs":"complete synthetic graph and independent constant-current requests; not installed C172 wiring or automatic CFG consumer activation",
        "installed_flight_switches":flight_switch_cases,"synthetic_simulation_time_battery":{"capacity_amph":40,"current_request_amps":36,"elapsed_seconds":1,"charge_fraction":timed_fraction,"unknown_request_clears_charge":true,"bus_voltage":null},
        "explicit_generator_admission_cases":admission_cases,"rpm_fraction_physical_mapping_verified":true,
        "explicit_generator_current_cases":current_cases,
        "known_input_frame_fixture":controlled_frame,
        "installed_source_generation":false,"installed_battery_dynamics":false,"sdk_omitted_circuit_defaults_resolved":false,"total_aircraft_current":false,"fsx_parity_verified":false});
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!("PASS: C172 circuit/FLT/RPM, versioned source/admission/current phases; provider-driven bus/battery dynamics remain unresolved.");
    Ok(())
}
