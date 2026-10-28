//! Compare the original piston default-curve builder (reference report) with the port.
use fsx_sim::legacy_piston::{default_curves, CurveInputs, Fields};
use serde_json::Value;
fn rows<const N: usize>(hex_text: &str) -> Result<[(f64, f64); N], String> {
    let bytes: Vec<u8> = (0..hex_text.len() / 2)
        .map(|i| u8::from_str_radix(&hex_text[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    if bytes.len() != 4 + 16 * N
        || u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize != N
    {
        return Err("Unexpected array layout".into());
    }
    let d = |o: usize| f64::from_le_bytes(bytes[o..o + 8].try_into().unwrap());
    Ok(std::array::from_fn(|i| (d(4 + 16 * i), d(12 + 16 * i))))
}
fn close(a: f64, b: f64) -> bool {
    a == b || (a - b).abs() <= 1e-13 * a.abs().max(b.abs())
}
fn main() -> Result<(), String> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("Usage: legacy-piston-curves-check <report.json>")?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Reference exceeds byte budget".into());
    }
    let report: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if report["source"] != "bounded-original-piston-curves-4d8a0"
        || report["sha256"] != "4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e"
    {
        return Err("Unexpected piston curves reference".into());
    }
    let cases = report["cases"].as_array().ok_or("Missing cases")?;
    for (i, c) in cases.iter().enumerate() {
        let n = |k: &str| c[k].as_f64().ok_or_else(|| format!("Missing {k}"));
        let got = default_curves(&CurveInputs {
            helper: Fields {
                f118: n("e118")?,
                f120: n("e120")?,
                f128: n("e128")?,
                f130: n("e130")?,
                f140: n("e140")?,
                f148: n("e148")?,
                f150: n("e150")?,
                f5f8: n("e5f8")?,
                f170: n("e170")? as u32,
            },
            f158: n("e158")?,
            f178: n("e178")?,
            f180: n("e180")?,
        });
        let a1: [(f64, f64); 5] = rows(c["array_5cc"].as_str().ok_or("array_5cc")?)?;
        let a2: [(f64, f64); 4] = rows(c["array_620"].as_str().ok_or("array_620")?)?;
        let ok = got
            .array_5cc
            .iter()
            .zip(&a1)
            .all(|(g, w)| close(g.0, w.0) && close(g.1, w.1))
            && got
                .array_620
                .iter()
                .zip(&a2)
                .all(|(g, w)| close(g.0, w.0) && close(g.1, w.1));
        if !ok {
            return Err(format!(
                "Case {i} differs: {c}\ngot {got:?}\nwant {a1:?} {a2:?}"
            ));
        }
    }
    println!(
        "PASS: {} original curve builds match within 1e-13",
        cases.len()
    );
    Ok(())
}
