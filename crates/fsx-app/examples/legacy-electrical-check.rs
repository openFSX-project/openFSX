//! Installed authored circuit schema and explicit bus-observation runtime check.
//! No implicit battery/generator voltage or FSX process is used.
use fsx_sim::flight::Simulation;
use fsx_vehicle::{
    aircraft::{Aircraft, KNOT},
    electrical::Bus,
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
            && configuration.max_generator_amps.is_none(),
        "Unexpected authored C172 source parameters",
    )?;
    ensure(
        configuration.circuits["pitot_heat"].bus == Bus::Main
            && configuration.circuits["pitot_heat"].max_amps == 2.
            && configuration.circuits["pitot_heat"].min_voltage == 17.,
        "C172 pitot circuit metadata mismatch",
    )?;
    let mut simulation = Simulation::trimmed(aircraft.parameters.clone(), 90. * KNOT, 300.)?;
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
        ("at_minimum", 17., 7.),
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
    let report = json!({"schema":1,"source":"legacy-electrical-authored-circuit-check","authored_circuits":circuits,"max_battery_voltage":configuration.max_battery_voltage,
        "generator_voltage":configuration.generator_voltage,"max_generator_amps":configuration.max_generator_amps,"engine_generator_map":configuration.engine_generator_map,"electric_always_available":configuration.always_available,
        "unsupported":configuration.unsupported,"cases":cases,"inputs":"explicit synthetic bus observations and requested authored loads",
        "source_generation":false,"battery_dynamics":false,"sdk_omitted_circuit_defaults_resolved":false,"total_aircraft_current":false,"fsx_parity_verified":false});
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!("PASS: 17 authored circuits, unknown-state propagation, 17 V threshold, explicit-load totals and simulation lifecycle; source generation remains unimplemented.");
    Ok(())
}
