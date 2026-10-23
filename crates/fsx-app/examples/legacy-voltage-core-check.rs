//! Compare the Rust voltage-core port with bounded original-instruction traces.
use fsx_sim::legacy_voltage::{Core, Inputs, State};
use fsx_vehicle::electrical::Electrical;
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
fn number(value: &Value, key: &str) -> Result<f64, String> {
    value[key]
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("Missing/nonfinite reference {key}"))
}
fn state(value: &Value) -> Result<State, String> {
    Ok(State {
        battery_voltage: number(value, "battery_voltage")?,
        filtered_net_amps: number(value, "filtered_net_amps")?,
        generator_amps: number(value, "generator_amps")?,
    })
}
fn compare(actual: f64, expected: f64, max_error: &mut f64) -> Result<(), String> {
    let error = (actual - expected).abs();
    if !actual.is_finite() || error > 1e-9 {
        return Err(format!(
            "Rust/original voltage core mismatch: {actual} vs {expected}"
        ));
    }
    *max_error = max_error.max(error);
    Ok(())
}
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("Usage: legacy-voltage-core-check <original --voltage-core report.json> <new output directory>".into());
    }
    let reference = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    if output.exists() {
        return Err("Output directory must be new".into());
    }
    if fs::metadata(&reference).map_err(|e| e.to_string())?.len() > 2 * 1024 * 1024 {
        return Err("Reference exceeds 2 MiB budget".into());
    }
    let data = fs::read(&reference).map_err(|e| e.to_string())?;
    if data.len() > 2 * 1024 * 1024 {
        return Err("Reference exceeds byte budget".into());
    }
    let report: Value = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    if report["source"] != "bounded-installed-sim1-voltage-core"
        || report["schema"] != 1
        || report["sha256"] != "4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e"
    {
        return Err("Unexpected original voltage-core report profile".into());
    }
    let cases = report["cases"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 16)
        .ok_or("Invalid reference case budget")?;
    let mut max_error = 0.;
    let mut frames_checked = 0;
    let mut checks = Vec::new();
    for case in cases {
        let c = &case["configuration"];
        let config = Electrical {
            max_battery_voltage: Some(number(c, "max_battery_voltage")?),
            generator_voltage: Some(number(c, "generator_voltage")?),
            max_generator_amps: Some(number(c, "max_generator_amps")?),
            ..Default::default()
        };
        let mut core = Core::default();
        core.seed(Some(state(&case["initial"])?))?;
        let frames = case["frames"]
            .as_array()
            .filter(|v| !v.is_empty() && v.len() <= 300)
            .ok_or("Invalid reference frame budget")?;
        for frame in frames {
            let before = core.state().ok_or("Lost voltage-core state")?;
            let expected_before = state(&frame["before"])?;
            for (a, b) in [
                (before.battery_voltage, expected_before.battery_voltage),
                (before.filtered_net_amps, expected_before.filtered_net_amps),
                (before.generator_amps, expected_before.generator_amps),
            ] {
                compare(a, b, &mut max_error)?;
            }
            let input = &frame["input"];
            let got = core
                .advance(
                    &config,
                    Inputs {
                        generator_online: Some(
                            input["generator_online"]
                                .as_bool()
                                .ok_or("Missing online boolean")?,
                        ),
                        battery_master: Some(
                            input["battery_master"]
                                .as_bool()
                                .ok_or("Missing master boolean")?,
                        ),
                        load_amps: Some(number(input, "load_amps")?),
                        response_factor: Some(number(input, "response_factor")? as f32),
                    },
                    number(input, "dt")?,
                )?
                .ok_or("Unresolved reference inputs")?;
            let expected = state(&frame["state"])?;
            for (a, b) in [
                (got.state.battery_voltage, expected.battery_voltage),
                (got.state.filtered_net_amps, expected.filtered_net_amps),
                (got.state.generator_amps, expected.generator_amps),
                (
                    got.pre_load_main_voltage,
                    number(frame, "pre_load_main_voltage")?,
                ),
                (
                    got.pre_load_battery_voltage,
                    number(frame, "pre_load_battery_voltage")?,
                ),
                (
                    got.pre_load_hot_battery_voltage,
                    number(frame, "pre_load_hot_battery_voltage")?,
                ),
                (
                    got.source_sum_used_amps,
                    number(frame, "source_sum_used_amps")?,
                ),
            ] {
                compare(a, b, &mut max_error)?;
            }
            frames_checked += 1;
        }
        checks.push(json!({"case":case["name"],"frames":frames.len(),"passed":true}));
    }
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    fs::write(output.join("report.json"),serde_json::to_vec_pretty(&json!({"schema":1,"source":"rust-original-voltage-core-comparison","cases":checks,"frames_checked":frames_checked,"max_absolute_error":max_error,"tolerance":1e-9,
        "scope":"core prefix only; explicit response factor/load/source/seed; no battery-failure/avionics tail, loaded-bus publication or live input providers"})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    println!("PASS: {} original/Rust core sequences, {frames_checked} frames, max absolute error {max_error:e}",cases.len());
    Ok(())
}
