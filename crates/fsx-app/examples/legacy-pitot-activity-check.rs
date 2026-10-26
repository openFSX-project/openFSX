//! Compare original pitot activity with explicit input providers.
use fsx_sim::{
    legacy_consumers::ConsumerState,
    legacy_power::{self, Circuit, Context},
};
use fsx_vehicle::electrical::{Electrical, SourceDefaults};
use fsx_vehicle::pitot_static::{LegacyPitotHeat, PitotStatic};
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
        || (report["source"] != "bounded-original-pitot-activity"
            && report["source"] != "bounded-original-pitot-configuration")
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
    if report["source"] == "bounded-original-pitot-configuration" {
        for case in cases {
            let text = if case["authored"].is_null() {
                "[pitot_static]".to_string()
            } else {
                format!("[pitot_static]\npitot_heat={}", number(case, "authored")?)
            };
            let parsed = PitotStatic::parse(&text)?;
            let legacy = if case["legacy_field"].is_null() {
                LegacyPitotHeat::Missing
            } else {
                LegacyPitotHeat::Value(boolean(case, "legacy_field")?)
            };
            let resolved = parsed.resolve(legacy)?;
            if resolved.rate.map(f64::from) != case["heater_rate"].as_f64() {
                return Err("Original configured pitot rate differs".into());
            }
            let writes = case["recorded_write_back"]
                .as_array()
                .filter(|v| v.len() <= 1)
                .ok_or("Invalid write-back array")?;
            if writes.first().and_then(Value::as_f64) != resolved.write_back {
                return Err("Original requested pitot write-back differs".into());
            }
            let mut state = ConsumerState::fresh(&config);
            state.update_pitot_activity(Some(true), resolved.rate, Some(true))?;
            if state.records()[7].active != Some(u32::from(number(case, "heater_rate")? > 0.)) {
                return Err("Configured pitot activity differs".into());
            }
        }
        let unresolved = PitotStatic::default().resolve(LegacyPitotHeat::Unknown)?;
        if unresolved.rate.is_some() || unresolved.write_back.is_some() {
            return Err("Unknown legacy provider became missing/default".into());
        }
        for value in ["NaN", "inf", "1e300", "bad"] {
            if PitotStatic::parse(&format!("[pitot_static]\npitot_heat={value}")).is_ok() {
                return Err("Invalid pitot configuration accepted".into());
            }
        }
        fs::create_dir(&output).map_err(|e| e.to_string())?;
        fs::write(output.join("report.json"),serde_json::to_vec_pretty(&json!({"schema":1,"configuration_cases":cases.len(),"all_rates_and_requested_writes_equal":true,"configuration_drives_activity":true,"unknown_legacy_stays_unknown":true,"user_files_modified":false})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        println!("PASS: {} original pitot configurations/write requests equal; configured activity and unknown provider checked",cases.len());
        return Ok(());
    }
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
