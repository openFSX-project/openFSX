//! Compare fresh/reused electrical initialization and the first original frame.
use fsx_sim::electrical::CircuitNetwork;
use fsx_sim::legacy_power::Circuit;
use fsx_sim::legacy_voltage::{Core, Inputs, State, TailEnvironment};
use fsx_vehicle::electrical::{Bus, Electrical};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

fn number(v: &Value, key: &str) -> Result<f64, String> {
    v[key]
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("Missing finite {key}"))
}
fn boolean(v: &Value, key: &str) -> Result<bool, String> {
    v[key]
        .as_bool()
        .ok_or_else(|| format!("Missing bool {key}"))
}
fn state(v: &Value) -> Result<State, String> {
    Ok(State {
        battery_voltage: number(v, "battery_voltage")?,
        filtered_net_amps: number(v, "filtered_net_amps")?,
        generator_amps: number(v, "generator_amps")?,
    })
}
fn compare_array<const N: usize>(got: [f64; N], reference: &Value) -> Result<(), String> {
    let expected = reference
        .as_array()
        .filter(|v| v.len() == N)
        .ok_or("Invalid reference array")?;
    for (index, (got, expected)) in got.into_iter().zip(expected).enumerate() {
        if Some(got) != expected.as_f64() {
            return Err(format!(
                "Original array[{index}] differs: {got} vs {expected}"
            ));
        }
    }
    Ok(())
}
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("Usage: legacy-electrical-initialization-check <reference report.json> <new output directory>".into());
    }
    let input = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    if output.exists() {
        return Err("Output directory must be new".into());
    }
    if fs::metadata(&input).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
        return Err("Reference exceeds 1 MiB budget".into());
    }
    let bytes = fs::read(&input).map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("Reference exceeds byte budget".into());
    }
    let report: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if report["schema"] != 2
        || report["source"] != "bounded-installed-sim1-electrical-initialization"
        || report["sha256"] != "4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e"
    {
        return Err("Unexpected initialization reference profile".into());
    }
    let cases = report["cases"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 100)
        .ok_or("Invalid reference case budget")?;
    let mut first_frames = 0;
    for case in cases {
        let cfg = Electrical {
            max_battery_voltage: Some(number(case, "max_battery_voltage")?),
            generator_voltage: Some(number(case, "generator_voltage")?),
            max_generator_amps: Some(60.),
            ..Default::default()
        };
        let mut core = Core::constructed();
        if core.state() != Some(state(&case["constructed"]["state"])?) {
            return Err("Constructor state differs".into());
        }
        if boolean(case, "reused")? {
            core.seed(Some(state(&case["before"])?))?;
        }
        let buses = core.initialize_rated_voltage(&cfg)?;
        if core.state() != Some(state(&case["result"]["state"])?) {
            return Err("Initializer state differs".into());
        }
        let mut known_buses = [0.; 8];
        for (dst, src) in known_buses.iter_mut().zip(buses) {
            *dst = src.ok_or("Missing initialized bus")?;
        }
        compare_array(known_buses, &case["result"]["bus_voltage"])?;
        if !boolean(case, "reused")? {
            let n = CircuitNetwork::new_fresh_legacy(cfg.clone())?;
            if n.legacy_voltage_state() != core.state()
                || n.switches().battery != Some(false)
                || n.generator.feed(Bus::Generator(1))?.online != Some(false)
                || n.battery.charge_fraction().is_some()
            {
                return Err("Fresh network initialization differs or invented charge".into());
            }
            for (i, expected) in known_buses.into_iter().enumerate() {
                let bus = match i {
                    0 => Bus::Main,
                    1 => Bus::Avionics,
                    2 => Bus::Battery,
                    3 => Bus::HotBattery,
                    n => Bus::Generator((n - 3) as u8),
                };
                if n.bus_voltage(bus)? != Some(expected) {
                    return Err("Fresh network bus differs".into());
                }
            }
        }
        if !case["first_frame"].is_null() {
            let got = core
                .advance_frame(
                    &cfg,
                    Inputs {
                        generator_online: Some(false),
                        battery_master: Some(boolean(case, "reused")?),
                        load_amps: Some(0.),
                        response_factor: Some(1.),
                    },
                    Some(TailEnvironment {
                        generator_enabled: boolean(case, "generator_enabled")?,
                        incoming_bus_voltage: known_buses,
                        incoming_generator_amps: [0.; 4],
                        battery_failed: false,
                        avionics_master: false,
                        voltage_mode: true,
                        override_power: false,
                        avionics_circuit: Some(Circuit {
                            bus: 0,
                            min_voltage: 0.,
                            forced_off: false,
                        }),
                        additional_system_circuit: Some(Circuit {
                            bus: 0,
                            min_voltage: 0.,
                            forced_off: false,
                        }),
                    }),
                    1. / 120.,
                )?
                .ok_or("Unknown first frame")?;
            let expected = &case["first_frame"];
            if got.state != state(&expected["state"])?
                || got.tail.source_sum_amps != number(expected, "source_sum")?
                || got.tail.avionics_active != boolean(expected, "avionics_active")?
                || got.tail.additional_system_active
                    != boolean(expected, "additional_system_active")?
            {
                return Err("First initialized frame state differs".into());
            }
            compare_array(got.tail.bus_voltage, &expected["bus_voltage"])?;
            compare_array(got.tail.generator_amps, &expected["generator_amps"])?;
            first_frames += 1;
        }
    }
    // Do not convert missing history into fresh zeros; invalid input is atomic.
    let mut unknown = Core::default();
    let cfg = Electrical {
        max_battery_voltage: Some(24.),
        ..Default::default()
    };
    if unknown.initialize_rated_voltage(&cfg)? != [Some(24.); 8] || unknown.state().is_some() {
        return Err("Unknown history became fresh".into());
    }
    let mut fresh = Core::constructed();
    let before = fresh.state();
    if fresh
        .initialize_rated_voltage(&Electrical {
            max_battery_voltage: Some(f64::NAN),
            ..Default::default()
        })
        .is_ok()
        || fresh.state() != before
    {
        return Err("Invalid initialization is not atomic".into());
    }
    if fresh.initialize_rated_voltage(&Electrical::default())? != [None; 8]
        || fresh.state().is_some()
    {
        return Err("Missing maximum became a default".into());
    }
    let defaults = CircuitNetwork::new_fresh_legacy(Electrical::default())?;
    if defaults.bus_voltage(Bus::Main)? != Some(24.)
        || defaults.source_configuration().max_battery_voltage != Some(24.)
    {
        return Err("Pinned fresh source defaults missing".into());
    }
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    fs::write(output.join("report.json"),serde_json::to_vec_pretty(&json!({"schema":1,"source":"rust-original-electrical-initialization-comparison","initialization_cases":cases.len(),"first_frames":first_frames,"all_compared_values_equal":true,"scope":"fresh/reused original initialization and injected first frames; no automatic flight/FLT restore or live providers"})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    println!("PASS: {} original initializations and {first_frames} first frames equal; fresh network, unknown history and atomic validation checked",cases.len());
    Ok(())
}
