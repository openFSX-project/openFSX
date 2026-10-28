//! Compare the original piston thermodynamics (reference report) with the f64 port.
use fsx_sim::legacy_piston::thermodynamics_20b4ee40;
use serde_json::Value;
fn main() -> Result<(), String> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("Usage: legacy-piston-thermo-check <reference report.json>")?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("Reference exceeds byte budget".into());
    }
    let report: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if report["source"] != "bounded-original-piston-thermo-4ee40"
        || report["sha256"] != "4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e"
    {
        return Err("Unexpected piston thermo reference".into());
    }
    let cases = report["cases"].as_array().ok_or("Missing cases")?;
    let mut worst = 0f64;
    for (i, c) in cases.iter().enumerate() {
        let n = |k: &str| c[k].as_f64().ok_or_else(|| format!("Missing {k}"));
        let t = thermodynamics_20b4ee40(n("e160")?, n("e168")?, n("e170")? as u32);
        let hex = c["block"].as_str().ok_or("Missing block")?;
        let raw: Vec<u8> = (0..hex.len() / 2)
            .map(|k| u8::from_str_radix(&hex[2 * k..2 * k + 2], 16).map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?;
        if raw.len() != 0x40 {
            return Err(format!("Case {i}: block length"));
        }
        let at = |o: usize| f64::from_le_bytes(raw[o..o + 8].try_into().unwrap());
        let pairs = [
            (t.f118, at(0)),
            (t.f120, at(8)),
            (t.f128, at(16)),
            (t.f130, at(24)),
            (t.f138, at(32)),
            (t.f140, at(40)),
            (t.f148, at(48)),
            (t.f150, at(56)),
        ];
        for (k, (got, want)) in pairs.iter().enumerate() {
            if got.is_nan() && want.is_nan() {
                continue;
            }
            let error = if want == got {
                0.
            } else if *want == 0. || !want.is_finite() {
                f64::INFINITY
            } else {
                ((got - want) / want).abs()
            };
            if error > 1e-13 {
                return Err(format!("Case {i} field {k}: got {got}, original {want}"));
            }
            worst = worst.max(error);
        }
        if raw[56 + 8 - 8..].len() != 8 {
            return Err("unreachable".into());
        }
    }
    println!(
        "PASS: {} original thermo runs, worst relative error {worst:.2e}",
        cases.len()
    );
    Ok(())
}
