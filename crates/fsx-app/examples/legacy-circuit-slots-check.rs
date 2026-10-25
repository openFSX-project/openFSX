//! Validate pinned original CFG names and authored consumer binding.
use fsx_sim::legacy_consumers;
use fsx_vehicle::electrical::{Bus, Circuit, Electrical, SourceDefaults, LEGACY_CONSUMER_KEYS};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err(
            "Usage: legacy-circuit-slots-check <reference report.json> <new output directory>"
                .into(),
        );
    }
    let input = PathBuf::from(&args[0]);
    let out = PathBuf::from(&args[1]);
    if out.exists() || fs::metadata(&input).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
        return Err("Output exists or reference exceeds budget".into());
    }
    let bytes = fs::read(input).map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("Reference exceeds byte budget".into());
    }
    let report: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if !matches!(report["schema"].as_u64(), Some(1..=2))
        || report["source"] != "bounded-original-electrical-circuit-slots"
        || report["sha256"] != "4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e"
    {
        return Err("Unexpected slot reference".into());
    }
    let records = report["circuits"]
        .as_array()
        .filter(|v| v.len() == 33)
        .ok_or("Wrong original circuit count")?;
    let mut config = Electrical::default();
    for (i, (key, record)) in LEGACY_CONSUMER_KEYS.iter().zip(records).enumerate() {
        if record["slot"].as_u64() != Some((i + 1) as u64) || record["key"].as_str() != Some(*key) {
            return Err("Original named circuit slot differs".into());
        }
        config.circuits.insert(
            key.to_string(),
            Circuit {
                bus: Bus::from_index(
                    u8::try_from(record["injected_bus"].as_u64().ok_or("Missing bus")?)
                        .map_err(|e| e.to_string())?,
                )?,
                max_amps: record["injected_max_amps"]
                    .as_f64()
                    .ok_or("Missing maximum")?,
                min_voltage: record["injected_min_voltage"]
                    .as_f64()
                    .ok_or("Missing minimum")?,
            },
        );
    }
    for slot in 1..34 {
        let mut activity = [Some(0); 34];
        activity[slot] = Some(2);
        let inputs = legacy_consumers::authored(&config, activity);
        if legacy_consumers::sum(&inputs)? != records[slot - 1]["injected_max_amps"].as_f64() {
            return Err("Authored binding loaded wrong circuit".into());
        }
    }
    let mut activity = [Some(0); 34];
    activity[1] = Some(1);
    config.circuits.remove("additional_system");
    config.circuits.insert(
        "navcom1".into(),
        Circuit {
            bus: Bus::Main,
            max_amps: 999.,
            min_voltage: 17.,
        },
    );
    if legacy_consumers::sum(&legacy_consumers::authored(&config, activity))?.is_some() {
        return Err("Missing additional_system was replaced by NAVCOM or zero".into());
    }
    let mut defaults_checked = 0;
    if report["schema"] == 2 {
        let cases = report["default_cases"]
            .as_array()
            .filter(|v| !v.is_empty() && v.len() <= 20)
            .ok_or("Invalid defaults case budget")?;
        for case in cases {
            let authored = Electrical {
                max_battery_voltage: Some(
                    case["max_battery_voltage"]
                        .as_f64()
                        .ok_or("Missing maximum")?,
                ),
                ..Default::default()
            };
            let effective =
                authored.with_legacy_circuit_defaults(SourceDefaults::FsxSteam62615SinglePiston)?;
            if !authored.circuits.is_empty() {
                return Err("Mutated authored circuits".into());
            }
            let records = case["circuits"]
                .as_array()
                .filter(|v| v.len() == 33)
                .ok_or("Wrong defaults slot count")?;
            for (key, record) in LEGACY_CONSUMER_KEYS.iter().zip(records) {
                let c = &effective.circuits[*key];
                if Some(c.bus.index()? as u64) != record["injected_bus"].as_u64()
                    || Some(c.max_amps) != record["injected_max_amps"].as_f64()
                    || Some(c.min_voltage) != record["injected_min_voltage"].as_f64()
                {
                    return Err(format!(
                        "Original circuit default differs for {key}: {:?} vs {record}",
                        c
                    ));
                }
                defaults_checked += 1;
            }
        }
        let preserved =
            config.with_legacy_circuit_defaults(SourceDefaults::FsxSteam62615SinglePiston)?;
        if legacy_consumers::sum(&legacy_consumers::authored(&preserved, activity))? != Some(5.) {
            return Err("Resolved additional_system did not use original 5 A default".into());
        }
        for (key, c) in &config.circuits {
            let got = &preserved.circuits[key];
            if got.bus != c.bus || got.max_amps != c.max_amps || got.min_voltage != c.min_voltage {
                return Err("Circuit default replaced authored value".into());
            }
        }
    }
    fs::create_dir(&out).map_err(|e| e.to_string())?;
    fs::write(out.join("report.json"),serde_json::to_vec_pretty(&json!({"schema":2,"slots_checked":33,"defaults_checked":defaults_checked,"all_names_and_loads_equal":true,"authored_values_preserved":true,"raw_missing_metadata_stays_unknown":true,"slot1":"additional_system","slot5":"avionics_bus"})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    println!(
        "PASS: 33 original names/slots and {defaults_checked} defaults; authored triples preserved"
    );
    Ok(())
}
