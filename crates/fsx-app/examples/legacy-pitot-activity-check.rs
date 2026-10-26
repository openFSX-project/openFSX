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
            && report["source"] != "bounded-original-pitot-configuration"
            && report["source"] != "bounded-original-pitot-air-record"
            && report["source"] != "bounded-original-pitot-thermal")
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
    if report["source"] == "bounded-original-pitot-thermal" {
        let mut histories =
            std::collections::BTreeMap::<String, fsx_sim::legacy_pitot::Core>::new();
        for case in cases {
            let mut isolated = fsx_sim::legacy_pitot::Core::default();
            let core = if let Some(name) = case["sequence"].as_str() {
                let core = histories.entry(name.into()).or_default();
                if case["frame"] == 0 {
                    core.seed(Some(number(case, "initial_temperature")? as f32))?;
                }
                if core.temperature() != Some(number(case, "initial_temperature")? as f32) {
                    return Err("Thermal sequence lost preceding state".into());
                }
                core
            } else {
                isolated.seed(Some(number(case, "initial_temperature")? as f32))?;
                &mut isolated
            };
            let step = core.advance(
                fsx_sim::legacy_pitot::Inputs {
                    requested: Some(boolean(case, "requested")?),
                    heater_rate: Some(number(case, "heater_rate")? as f32),
                    power_available: Some(boolean(case, "power_available")?),
                    environment_scalar: Some(number(case, "environment_scalar")?),
                },
                number(case, "dt")?,
            )?;
            if step.active != Some(boolean(case, "active")?)
                || step.temperature.map(f64::from) != case["temperature"].as_f64()
            {
                return Err(format!(
                    "Original thermal prefix differs: {step:?} vs {case}"
                ));
            }
        }
        let mut core = fsx_sim::legacy_pitot::Core::default();
        core.seed(Some(500.))?;
        if core
            .advance(fsx_sim::legacy_pitot::Inputs::default(), 0.)
            .is_ok()
            || core.temperature() != Some(500.)
        {
            return Err("Invalid thermal interval changed state".into());
        }
        if core
            .advance(fsx_sim::legacy_pitot::Inputs::default(), 1. / 120.)?
            .temperature
            .is_some()
        {
            return Err("Missing thermal providers retained state".into());
        }
        fs::create_dir(&output).map_err(|e| e.to_string())?;
        fs::write(output.join("report.json"),serde_json::to_vec_pretty(&json!({"schema":1,"thermal_cases":cases.len(),"all_temperatures_equal":true,"unknown_and_atomic_errors_checked":true,"scope":"thermal prefix only; explicit environment/seed, no icing or live flight binding"})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        println!(
            "PASS: {} original thermal prefix temperatures/activity equal",
            cases.len()
        );
        return Ok(());
    }
    if report["source"] == "bounded-original-pitot-air-record" {
        for case in cases {
            let mut bytes = Vec::new();
            bytes.extend(0x123u32.to_le_bytes());
            bytes.extend(3u32.to_le_bytes());
            bytes.extend(b"own");
            if let Some(hex) = case["payload_hex"].as_str() {
                if !hex.is_ascii() || hex.len() > 64 || hex.len() % 2 != 0 {
                    return Err("Invalid synthetic AIR payload budget".into());
                }
                let payload = (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| e.to_string()))
                    .collect::<Result<Vec<_>, _>>()?;
                bytes.extend(0x324u32.to_le_bytes());
                bytes.extend((payload.len() as u32).to_le_bytes());
                bytes.extend(payload);
            }
            let air = fsx_vehicle::air::AirFile::parse(&bytes)?;
            let actual = LegacyPitotHeat::from_air(&air)?;
            let expected = if boolean(case, "found")? {
                LegacyPitotHeat::Value(boolean(case, "enabled")?)
            } else {
                LegacyPitotHeat::Missing
            };
            if actual != expected {
                return Err("Original AIR heater fallback differs".into());
            }
            let resolved = PitotStatic::default().resolve(actual)?;
            let enabled = case["enabled"].as_bool().unwrap_or(true);
            if resolved.rate != Some(if enabled { 1. } else { 0. }) {
                return Err("AIR record did not drive configured rate".into());
            }
        }
        for size in 0..4 {
            let air = fsx_vehicle::air::AirFile {
                records: std::collections::BTreeMap::from([(0x324, vec![0; size])]),
            };
            if LegacyPitotHeat::from_air(&air).is_ok() {
                return Err("Malformed short pitot AIR record accepted".into());
            }
            if PitotStatic::parse("[pitot_static]\npitot_heat=1")?.legacy_input_from_air(&air)?
                != LegacyPitotHeat::Unknown
            {
                return Err(
                    "Authored pitot coefficient consulted malformed unused AIR data".into(),
                );
            }
        }
        fs::create_dir(&output).map_err(|e| e.to_string())?;
        fs::write(output.join("report.json"),serde_json::to_vec_pretty(&json!({"schema":1,"air_record_cases":cases.len(),"all_record_values_equal":true,"fallback_rate_checked":true,"short_records_rejected":true,"scope":"original AIR helper with synthetic files; no original live process"})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        println!("PASS: {} original AIR record lookups and fallback rates equal; malformed short payloads rejected",cases.len());
        return Ok(());
    }
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
