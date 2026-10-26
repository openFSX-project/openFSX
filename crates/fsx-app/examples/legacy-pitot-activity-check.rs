//! Compare original pitot activity with explicit input providers.
use fsx_sim::{
    legacy_consumers::ConsumerState,
    legacy_power::{self, Circuit, Context},
};
use fsx_vehicle::electrical::{Electrical, SourceDefaults};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
fn boolean(v: &Value, key: &str) -> Result<bool, String> {
    v[key].as_bool().ok_or_else(|| format!("Missing {key}"))
}
fn number(v: &Value, key: &str) -> Result<f64, String> {
    v[key]
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("Missing finite {key}"))
}
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err(
            "Usage: legacy-pitot-activity-check <reference report.json> <new output directory>"
                .into(),
        );
    }
    let input = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    if output.exists() || fs::metadata(&input).map_err(|e| e.to_string())?.len() > 2 * 1024 * 1024 {
        return Err("Output exists or reference exceeds budget".into());
    }
    let bytes = fs::read(input).map_err(|e| e.to_string())?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("Reference exceeds byte budget".into());
    }
    let report: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if report["schema"] != 1
        || report["source"] != "bounded-original-pitot-activity"
        || report["sha256"] != "4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e"
    {
        return Err("Unexpected pitot reference".into());
    }
    let cases = report["cases"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 1000)
        .ok_or("Invalid case budget")?;
    let config = Electrical::default()
        .with_legacy_circuit_defaults(SourceDefaults::FsxSteam62615SinglePiston)?;
    for case in cases {
        let c = &case["context"];
        let context = Context {
            voltage_mode: boolean(c, "context_flag")?,
            battery_master: boolean(c, "battery_master")?,
            max_battery_voltage: number(c, "max_battery_voltage")?,
            override_power: boolean(c, "override_power")?,
        };
        let circuit = if case["circuit"].is_null() {
            None
        } else {
            let c = &case["circuit"];
            Some(Circuit {
                bus: u32::try_from(c["bus"].as_u64().ok_or("Missing bus")?)
                    .map_err(|e| e.to_string())?,
                min_voltage: number(c, "min_voltage")?,
                forced_off: boolean(c, "forced_off")?,
            })
        };
        let values = case["bus_voltage"]
            .as_array()
            .filter(|v| v.len() == 8)
            .ok_or("Invalid bus array")?;
        let mut buses = [0.; 8];
        for (dst, src) in buses.iter_mut().zip(values) {
            *dst = src
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or("Invalid bus voltage")?;
        }
        let power = legacy_power::available(context, circuit, buses)?;
        if power != boolean(case, "power_available")? {
            return Err("Original pitot power differs".into());
        }
        let mut state = ConsumerState::fresh(&config);
        state.set_active(7, Some(!boolean(case, "active")?))?;
        state.update_pitot_activity(
            Some(boolean(case, "requested")?),
            Some(number(case, "heater_rate")? as f32),
            Some(power),
        )?;
        if state.records()[7].active != Some(u32::from(boolean(case, "active")?)) {
            return Err("Original pitot activity differs".into());
        }
        if state
            .records()
            .iter()
            .enumerate()
            .any(|(i, r)| i != 7 && r.active != Some(0))
        {
            return Err("Pitot changed another consumer".into());
        }
    }
    let mut state = ConsumerState::fresh(&config);
    state.set_active(7, Some(true))?;
    if state
        .update_pitot_activity(Some(false), Some(f32::NAN), None)
        .is_ok()
        || state.records()[7].active != Some(1)
    {
        return Err("Invalid pitot update not atomic".into());
    }
    state.update_pitot_activity(Some(false), None, None)?;
    if state.records()[7].active != Some(0) {
        return Err("Off request retained unknown power".into());
    }
    state.update_pitot_activity(Some(true), Some(1.), None)?;
    if state.records()[7].active.is_some() {
        return Err("Unknown pitot supply became known".into());
    }
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    fs::write(output.join("report.json"),serde_json::to_vec_pretty(&json!({"schema":1,"cases":cases.len(),"all_activity_equal":true,"unknown_and_invalid_checked":true,"scope":"original pitot activity prefix with explicit providers; no thermal/icing model"})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    println!(
        "PASS: {} original pitot activity prefixes equal; unknown inputs and atomic errors checked",
        cases.len()
    );
    Ok(())
}
