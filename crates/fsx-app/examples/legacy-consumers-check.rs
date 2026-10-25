//! Compare original consumer sums without inventing aircraft equipment.
use fsx_sim::legacy_consumers::{self, Consumer, ConsumerState, SLOTS};
use fsx_vehicle::electrical::{Bus, Circuit, Electrical, LEGACY_CONSUMER_KEYS};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err(
            "Usage: legacy-consumers-check <reference report.json> <new output directory>".into(),
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
    if !matches!(report["schema"].as_u64(), Some(1..=2))
        || report["source"] != "bounded-original-electrical-consumers"
        || report["sha256"] != "4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e"
    {
        return Err("Unexpected consumer reference".into());
    }
    let cases = report["cases"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 500)
        .ok_or("Invalid case budget")?;
    for case in cases {
        let records = case["consumers"]
            .as_array()
            .filter(|v| v.len() == SLOTS)
            .ok_or("Wrong slot count")?;
        let mut inputs = [Consumer::default(); SLOTS];
        for (dst, src) in inputs.iter_mut().zip(records) {
            dst.active = Some(
                u32::try_from(src["active"].as_u64().ok_or("Missing raw activity")?)
                    .map_err(|e| e.to_string())?,
            );
            dst.max_amps = if src["max_amps"].is_null() {
                None
            } else {
                Some(
                    src["max_amps"]
                        .as_f64()
                        .filter(|v| v.is_finite())
                        .ok_or("Invalid maximum")?,
                )
            };
        }
        if legacy_consumers::sum(&inputs)? != case["load_amps"].as_f64() {
            return Err("Original ordered consumer sum differs".into());
        }
    }
    let mut inputs = [Consumer {
        active: Some(0),
        max_amps: None,
    }; SLOTS];
    inputs[0] = Consumer {
        active: None,
        max_amps: Some(f64::NAN),
    };
    if legacy_consumers::sum(&inputs)? != Some(0.) {
        return Err("Ignored slot or inactive metadata was consumed".into());
    }
    inputs[33].active = Some(1);
    if legacy_consumers::sum(&inputs)?.is_some() {
        return Err("Active unresolved metadata became zero".into());
    }
    inputs[33] = Consumer {
        active: None,
        max_amps: Some(7.),
    };
    if legacy_consumers::sum(&inputs)?.is_some() {
        return Err("Unknown activity became known".into());
    }
    inputs[33] = Consumer {
        active: Some(0),
        max_amps: Some(f64::NAN),
    };
    if legacy_consumers::sum(&inputs).is_ok() {
        return Err("Invalid supported-slot metadata accepted".into());
    }
    let mut setters_checked = 0;
    if report["schema"] == 2 {
        let cases = report["activity_cases"]
            .as_array()
            .filter(|v| !v.is_empty() && v.len() <= 300)
            .ok_or("Invalid setter case budget")?;
        let mut config = Electrical::default();
        for key in LEGACY_CONSUMER_KEYS {
            config.circuits.insert(
                key.into(),
                Circuit {
                    bus: Bus::Main,
                    max_amps: 7.25,
                    min_voltage: 17.,
                },
            );
        }
        for case in cases {
            let slot = usize::try_from(case["slot"].as_u64().ok_or("Missing setter slot")?)
                .map_err(|e| e.to_string())?;
            if slot >= SLOTS {
                return Err("Setter slot outside budget".into());
            }
            let previous = u32::try_from(case["previous"].as_u64().ok_or("Missing previous word")?)
                .map_err(|e| e.to_string())?;
            let mut words = [Some(0); SLOTS];
            words[slot] = Some(previous);
            let mut state = ConsumerState::observed(&config, words);
            state.set_active(
                slot,
                Some(case["enabled"].as_bool().ok_or("Missing enabled")?),
            )?;
            if state.records()[slot].active.map(u64::from) != case["active"].as_u64()
                || legacy_consumers::sum(state.records())? != case["load_amps"].as_f64()
            {
                return Err("Original setter/load differs".into());
            }
            for (i, record) in state.records().iter().enumerate() {
                if i != slot && record.active != Some(0) {
                    return Err("Setter changed another slot".into());
                }
            }
            setters_checked += 1;
        }
        let mut state = ConsumerState::fresh(&config);
        if state.set_active(SLOTS, Some(true)).is_ok()
            || legacy_consumers::sum(state.records())? != Some(0.)
        {
            return Err("Invalid setter mutated state".into());
        }
        state.set_active(7, None)?;
        if legacy_consumers::sum(state.records())?.is_some()
            || legacy_consumers::sum(ConsumerState::unknown(&config).records())?.is_some()
        {
            return Err("Unknown equipment became active/off".into());
        }
    }
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    fs::write(output.join("report.json"),serde_json::to_vec_pretty(&json!({"schema":2,"original_cases":cases.len(),"activity_setters_checked":setters_checked,"all_loads_equal":true,"unknown_and_invalid_inputs_checked":true,"scope":"original 34-slot equipment activity setters and ordered sum; no live activation scheduler"})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    println!("PASS: {} original sums / {setters_checked} setters equal; ignored slot, unknown metadata/activity and invalid input checked",cases.len());
    Ok(())
}
