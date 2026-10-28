//! Compare the original piston helper (reference report) with the f64 port.
use fsx_sim::legacy_piston::{helper_20b4d7e0, Fields};
use serde_json::Value;
fn main() -> Result<(), String> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("Usage: legacy-piston-helper-check <reference report.json>")?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("Reference exceeds byte budget".into());
    }
    let report: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if report["source"] != "bounded-original-piston-helper-4d7e0"
        || report["sha256"] != "4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e"
    {
        return Err("Unexpected piston helper reference".into());
    }
    let cases = report["cases"].as_array().ok_or("Missing cases")?;
    let mut worst = 0f64;
    for (i, c) in cases.iter().enumerate() {
        let n = |k: &str| c[k].as_f64().ok_or_else(|| format!("Missing {k}"));
        let got = helper_20b4d7e0(&Fields {
            f118: n("e118")?,
            f120: n("e120")?,
            f128: n("e128")?,
            f130: n("e130")?,
            f140: n("e140")?,
            f148: n("e148")?,
            f150: n("e150")?,
            f5f8: n("e5f8")?,
            f170: n("e170")? as u32,
        });
        let want = n("result")?;
        let error = if want == 0. {
            got.abs()
        } else {
            ((got - want) / want).abs()
        };
        if error > 1e-14 {
            return Err(format!("Case {i}: got {got}, original {want}"));
        }
        worst = worst.max(error);
    }
    println!(
        "PASS: {} original helper runs, worst relative error {worst:.2e}",
        cases.len()
    );
    Ok(())
}
