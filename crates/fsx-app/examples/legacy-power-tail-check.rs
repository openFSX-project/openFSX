//! Compare finite-input power/tail behavior against bounded original execution.
use fsx_sim::legacy_power::{self, Circuit, Context, TailInput};
use fsx_sim::legacy_voltage::{Core, Inputs, State, TailEnvironment};
use fsx_vehicle::electrical::Electrical;
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
fn number(v: &Value, k: &str) -> Result<f64, String> {
    v[k].as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("Missing finite {k}"))
}
fn boolean(v: &Value, k: &str) -> Result<bool, String> {
    v[k].as_bool().ok_or_else(|| format!("Missing bool {k}"))
}
fn array<const N: usize>(v: &Value) -> Result<[f64; N], String> {
    let values = v
        .as_array()
        .filter(|v| v.len() == N)
        .ok_or("Reference array length mismatch")?;
    let mut result = [0.; N];
    for (i, x) in values.iter().enumerate() {
        result[i] = x
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or("Nonfinite array value")?;
    }
    Ok(result)
}
fn context(v: &Value) -> Result<Context, String> {
    Ok(Context {
        voltage_mode: boolean(v, "context_flag")?
            || v["mode_query"].as_u64().ok_or("Missing mode query")? == 1,
        battery_master: boolean(v, "battery_master")?,
        max_battery_voltage: number(v, "max_battery_voltage")?,
        override_power: boolean(v, "override_power")?,
    })
}
fn circuit(v: &Value) -> Result<Option<Circuit>, String> {
    if v.is_null() {
        return Ok(None);
    }
    Ok(Some(Circuit {
        bus: u32::try_from(v["bus"].as_u64().ok_or("Missing bus")?).map_err(|e| e.to_string())?,
        min_voltage: number(v, "min_voltage")?,
        forced_off: boolean(v, "forced_off")?,
    }))
}
fn state(v: &Value) -> Result<State, String> {
    Ok(State {
        battery_voltage: number(v, "battery_voltage")?,
        filtered_net_amps: number(v, "filtered_net_amps")?,
        generator_amps: number(v, "generator_amps")?,
    })
}
fn compare(actual: f64, expected: f64, error: &mut f64) -> Result<(), String> {
    let difference = (actual - expected).abs();
    if !actual.is_finite() || difference > 1e-9 {
        return Err(format!("Complete frame mismatch {actual} vs {expected}"));
    }
    *error = error.max(difference);
    Ok(())
}
fn compare_state(actual: State, expected: State, error: &mut f64) -> Result<(), String> {
    for (a, b) in [
        (actual.battery_voltage, expected.battery_voltage),
        (actual.filtered_net_amps, expected.filtered_net_amps),
        (actual.generator_amps, expected.generator_amps),
    ] {
        compare(a, b, error)?;
    }
    Ok(())
}
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err(
            "Usage: legacy-power-tail-check <original tail report.json> <new output directory>"
                .into(),
        );
    }
    let reference = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    if output.exists() {
        return Err("Output directory must be new".into());
    }
    if fs::metadata(&reference).map_err(|e| e.to_string())?.len() > 4 * 1024 * 1024 {
        return Err("Reference exceeds 4 MiB budget".into());
    }
    let bytes = fs::read(reference).map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Reference exceeds byte budget".into());
    }
    let report: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if report["source"] != "bounded-installed-sim1-power-tail"
        || (report["schema"] != 1 && report["schema"] != 2)
        || report["sha256"] != "4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e"
    {
        return Err("Unexpected original tail profile".into());
    }
    let powers = report["power_cases"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 2000)
        .ok_or("Invalid predicate case budget")?;
    for (index, c) in powers.iter().enumerate() {
        let got = legacy_power::available(
            context(&c["context"])?,
            circuit(&c["circuit"])?,
            array(&c["bus_voltage"])?,
        )?;
        if got != boolean(c, "available")? {
            return Err(format!("Predicate mismatch at case {index}"));
        }
    }
    let tails = report["tail_cases"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 1000)
        .ok_or("Invalid tail case budget")?;
    for (index, c) in tails.iter().enumerate() {
        let got = legacy_power::finish(TailInput {
            context: context(&c["context"])?,
            bus_voltage: array(&c["incoming_bus_voltage"])?,
            generator_amps: array(&c["incoming_generator_amps"])?,
            source_sum_amps: number(c, "incoming_source_sum")?,
            battery_failed: boolean(c, "battery_failed")?,
            avionics_master: boolean(c, "avionics_master")?,
            avionics_circuit: circuit(&c["avionics_circuit"])?,
            navcom1_circuit: circuit(&c["navcom1_circuit"])?,
        })?;
        if got.bus_voltage != array::<8>(&c["bus_voltage"])?
            || got.generator_amps != array::<4>(&c["generator_amps"])?
            || got.source_sum_amps != number(c, "source_sum")?
            || got.avionics_active != boolean(c, "avionics_active")?
            || got.navcom1_active != boolean(c, "navcom1_active")?
        {
            return Err(format!("Frame-tail mismatch at case {index}"));
        }
    }
    let mut frames_checked = 0;
    let mut frame_error = 0.;
    let mut sequences = 0;
    if report["schema"] == 2 {
        let cases = report["frame_cases"]
            .as_array()
            .filter(|v| !v.is_empty() && v.len() <= 16)
            .ok_or("Invalid complete-frame sequence budget")?;
        sequences = cases.len();
        for c in cases {
            let cfg = &c["configuration"];
            let config = Electrical {
                max_battery_voltage: Some(number(cfg, "max_battery_voltage")?),
                generator_voltage: Some(number(cfg, "generator_voltage")?),
                max_generator_amps: Some(number(cfg, "max_generator_amps")?),
                ..Default::default()
            };
            let mut core = Core::default();
            core.seed(Some(state(&c["initial"])?))?;
            let mut buses = [0.; 8];
            let mut amps = [0.; 4];
            let mut av_active = false;
            let mut nav_active = false;
            let frames = c["frames"]
                .as_array()
                .filter(|v| !v.is_empty() && v.len() <= 300)
                .ok_or("Invalid complete-frame budget")?;
            for frame in frames {
                compare_state(
                    core.state().ok_or("Lost composed state")?,
                    state(&frame["before"])?,
                    &mut frame_error,
                )?;
                for (a, b) in buses
                    .into_iter()
                    .zip(array::<8>(&frame["before_bus_voltage"])?)
                {
                    compare(a, b, &mut frame_error)?;
                }
                let load = if av_active {
                    number(c, "avionics_max_amps")?
                } else {
                    0.
                } + if nav_active {
                    number(c, "navcom1_max_amps")?
                } else {
                    0.
                };
                compare(load, number(frame, "load_used_amps")?, &mut frame_error)?;
                let input = &frame["input"];
                let context = context(&frame["context"])?;
                let got = core
                    .advance_frame(
                        &config,
                        Inputs {
                            generator_online: Some(boolean(input, "generator_online")?),
                            battery_master: Some(context.battery_master),
                            load_amps: Some(load),
                            response_factor: Some(number(input, "response_factor")? as f32),
                        },
                        Some(TailEnvironment {
                            incoming_bus_voltage: buses,
                            incoming_generator_amps: amps,
                            battery_failed: boolean(input, "battery_failed")?,
                            avionics_master: boolean(input, "avionics_master")?,
                            voltage_mode: context.voltage_mode,
                            override_power: context.override_power,
                            avionics_circuit: circuit(&c["avionics_circuit"])?,
                            navcom1_circuit: circuit(&c["navcom1_circuit"])?,
                        }),
                        number(input, "dt")?,
                    )?
                    .ok_or("Unexpected unknown composed frame")?;
                compare_state(
                    got.prefix.state,
                    state(&frame["prefix"]["state"])?,
                    &mut frame_error,
                )?;
                compare_state(got.state, state(&frame["state"])?, &mut frame_error)?;
                for (a, b) in got
                    .tail
                    .bus_voltage
                    .into_iter()
                    .zip(array::<8>(&frame["bus_voltage"])?)
                {
                    compare(a, b, &mut frame_error)?;
                }
                for (a, b) in got
                    .tail
                    .generator_amps
                    .into_iter()
                    .zip(array::<4>(&frame["generator_amps"])?)
                {
                    compare(a, b, &mut frame_error)?;
                }
                compare(
                    got.tail.source_sum_amps,
                    number(frame, "source_sum")?,
                    &mut frame_error,
                )?;
                if got.tail.avionics_active != boolean(frame, "avionics_active")?
                    || got.tail.navcom1_active != boolean(frame, "navcom1_active")?
                {
                    return Err("Complete frame consumer flags differ".into());
                }
                buses = got.tail.bus_voltage;
                amps = got.tail.generator_amps;
                av_active = got.tail.avionics_active;
                nav_active = got.tail.navcom1_active;
                frames_checked += 1;
            }
        }
    }
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    fs::write(output.join("report.json"),serde_json::to_vec_pretty(&json!({"schema":2,"source":"rust-original-power-tail-comparison","predicate_cases":powers.len(),"tail_cases":tails.len(),"all_tail_values_equal":true,"frame_sequences":sequences,"frames_checked":frames_checked,"frame_max_error":frame_error,"frame_tolerance":1e-9,"scope":"known finite contexts and injected services; live mode/override provenance and equipment presence remain provider inputs"})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    println!(
        "PASS: {} predicates / {} tails equal; {sequences} composed sequences / {frames_checked} frames, max error {frame_error:e}",
        powers.len(),tails.len()
    );
    Ok(())
}
