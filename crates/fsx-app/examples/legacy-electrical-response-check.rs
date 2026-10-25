//! Compare explicit settings provider to guarded original response traces.
use fsx_sim::legacy_response::Response;
use fsx_sim::legacy_voltage::{Core, Inputs, State};
use fsx_vehicle::electrical::Electrical;
use fsx_vehicle::realism::Realism;
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("Usage: legacy-electrical-response-check <reference report.json> <new output directory>".into());
    }
    let input = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    if output.exists() || fs::metadata(&input).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
        return Err("Output exists or reference exceeds budget".into());
    }
    let bytes = fs::read(input).map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("Reference exceeds byte budget".into());
    }
    let report: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if !matches!(report["schema"].as_u64(), Some(1..=2))
        || report["source"] != "bounded-original-realism-electrical-response"
        || report["sim1_sha256"]
            != "4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e"
        || report["main_sha256"]
            != "7b01a1b3a53e0779309aaf0144519cabb6b339a82c8490449dabd129c5e5883f"
    {
        return Err("Unexpected response reference".into());
    }
    let cases = report["cases"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 100)
        .ok_or("Invalid case budget")?;
    let mut voltage_max_error: f64 = 0.;
    let cfg = Electrical {
        max_battery_voltage: Some(24.),
        generator_voltage: Some(28.),
        max_generator_amps: Some(60.),
        ..Default::default()
    };
    for case in cases {
        let general = case["general"].as_f64().ok_or("Missing General")?;
        let settings = Realism::parse(&format!("[REALISM]\nGeneral={general}"))?;
        let mut response = Response::observed(Some(
            case["prior_response_factor"]
                .as_f64()
                .ok_or("Missing prior")? as f32,
        ))?;
        if report["schema"] == 2 {
            let raw = |key: &str| -> Result<u32, String> {
                u32::try_from(case[key].as_u64().ok_or("Missing raw context flag")?)
                    .map_err(|e| e.to_string())
            };
            response.copy_concrete_context_settings(
                settings,
                Some(raw("context_flag_raw")?),
                Some(raw("copy_context_flag_raw")?),
            )?;
        } else {
            response.copy_settings(
                settings,
                Some(
                    case["context_queries_active"]
                        .as_bool()
                        .ok_or("Missing gate")?,
                ),
                Some(
                    case["general_copy_active"]
                        .as_bool()
                        .ok_or("Missing copy gate")?,
                ),
            )?;
        }
        let factor = response.factor().ok_or("Unknown copied factor")?;
        if Some(factor as f64) != case["response_factor"].as_f64() {
            return Err("Original copied factor differs".into());
        }
        let mut core = Core::default();
        core.seed(Some(State {
            battery_voltage: 22.,
            filtered_net_amps: 0.,
            generator_amps: 0.,
        }))?;
        let step = core
            .advance(
                &cfg,
                Inputs {
                    generator_online: Some(false),
                    battery_master: Some(true),
                    load_amps: Some(12.),
                    response_factor: response.factor(),
                },
                1. / 120.,
            )?
            .ok_or("Unknown provider-driven core")?;
        for (actual, key) in [
            (step.state.battery_voltage, "battery_voltage"),
            (step.state.filtered_net_amps, "filtered_net_amps"),
        ] {
            let expected = case[key]
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or("Missing original core output")?;
            let error = (actual - expected).abs();
            if error > 1e-12 {
                return Err(format!(
                    "Provider-driven {key} differs: {actual} vs {expected}"
                ));
            }
            voltage_max_error = voltage_max_error.max(error);
        }
    }
    let mut response = Response::observed(Some(0.75))?;
    response.copy_settings(Realism::default(), Some(false), None)?;
    if response.factor() != Some(0.75) {
        return Err("Inactive gate lost history".into());
    }
    response.copy_settings(Realism::default(), Some(true), Some(true))?;
    if response.factor().is_some() {
        return Err("Missing General invented a default".into());
    }
    // Once an admitted update loses the setting, the core must lose state
    // knowledge over elapsed time, rather than continue with a stale factor.
    let mut core = Core::constructed();
    core.initialize_rated_voltage(&cfg)?;
    if core
        .advance(
            &cfg,
            Inputs {
                generator_online: Some(false),
                battery_master: Some(true),
                load_amps: Some(12.),
                response_factor: response.factor(),
            },
            1. / 120.,
        )?
        .is_some()
        || core.state().is_some()
    {
        return Err("Missing response retained evolving voltage".into());
    }
    // Invalid updates are atomic even when a gate would otherwise skip copying.
    let mut observed = Response::observed(Some(0.75))?;
    for general in [f64::NAN, -1., f64::MAX] {
        if observed
            .copy_settings(
                Realism {
                    general: Some(general),
                },
                Some(false),
                Some(false),
            )
            .is_ok()
            || observed.factor() != Some(0.75)
        {
            return Err("Invalid response update changed prior observation".into());
        }
    }
    for value in ["NaN", "inf", "-1", "1e300", "bad"] {
        if Realism::parse(&format!("[Realism]\nGeneral={value}")).is_ok() {
            return Err("Invalid authored General accepted".into());
        }
    }
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    fs::write(output.join("report.json"),serde_json::to_vec_pretty(&json!({"schema":2,"cases":cases.len(),"all_factors_equal":true,"provider_driven_voltage_max_error":voltage_max_error,"voltage_tolerance":1e-12,"missing_factor_invalidates_core":true,"invalid_update_atomic":true,"scope":"explicit authored settings and observed context gates driving voltage core; no automatic flight binding"})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    println!(
        "PASS: {} original response factors equal; missing and invalid settings checked",
        cases.len()
    );
    Ok(())
}
