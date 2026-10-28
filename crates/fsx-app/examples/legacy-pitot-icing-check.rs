//! Compare the original pitot icing function (reference report) with the Rust port.
use fsx_sim::legacy_pitot::{Core, IcingInputs, Inputs};
use serde_json::Value;
use std::fs;
fn num(c: &Value, k: &str) -> Result<f64, String> {
    c[k].as_f64().ok_or_else(|| format!("Missing {k}"))
}
fn flag(c: &Value, k: &str) -> Result<bool, String> {
    c[k].as_bool().ok_or_else(|| format!("Missing {k}"))
}
fn main() -> Result<(), String> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("Usage: legacy-pitot-icing-check <reference report.json>")?;
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("Reference exceeds byte budget".into());
    }
    let report: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if report["source"] != "bounded-original-pitot-icing"
        || report["sha256"] != "4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e"
    {
        return Err("Unexpected icing reference".into());
    }
    let cases = report["cases"].as_array().ok_or("Missing cases")?;
    for (i, c) in cases.iter().enumerate() {
        let mut core = Core::default();
        core.seed(Some(num(c, "temperature_initial")? as f32))?;
        core.seed_icing(num(c, "ice_initial")? as f32, num(c, "filter_initial")?)?;
        let step = core
            .advance_icing(
                Inputs {
                    requested: Some(flag(c, "requested")?),
                    heater_rate: Some(num(c, "heater_rate")? as f32),
                    power_available: Some(flag(c, "power_available")?),
                    environment_scalar: Some(num(c, "environment_scalar")?),
                },
                IcingInputs {
                    weather_enabled: flag(c, "weather_54")?,
                    weather_precipitation: flag(c, "weather_50")?,
                    field_14c: num(c, "field_14c")?,
                    input: num(c, "input")?,
                },
                num(c, "dt")?,
            )?
            .ok_or("Unexpected unknown result")?;
        let ok = step.step.temperature == Some(num(c, "temperature")? as f32)
            && step.step.active == Some(flag(c, "active")?)
            && step.ice == num(c, "ice")? as f32
            && step.filter == num(c, "filter")?;
        if !ok {
            return Err(format!(
                "Case {i} differs from the original: {c} vs {step:?}"
            ));
        }
    }
    println!(
        "PASS: {} original pitot icing runs match bit-exactly",
        cases.len()
    );
    Ok(())
}
