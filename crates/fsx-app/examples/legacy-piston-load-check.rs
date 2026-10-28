//! Compare the original piston record loader (reference report) with the Rust decoding.
use fsx_sim::legacy_piston_load::{load_engine_20b4f1c0, Store};
use serde_json::Value;
const REGION: usize = 0x100;
fn bytes_of(hex: &str) -> Result<Vec<u8>, String> {
    (0..hex.len() / 2)
        .map(|k| u8::from_str_radix(&hex[2 * k..2 * k + 2], 16).map_err(|e| e.to_string()))
        .collect()
}
fn main() -> Result<(), String> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("Usage: legacy-piston-load-check <reference report.json>")?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Reference exceeds byte budget".into());
    }
    let report: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if report["source"] != "bounded-original-piston-load-4f1c0"
        || report["sha256"] != "4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e"
    {
        return Err("Unexpected piston load reference".into());
    }
    let cases = report["cases"].as_array().ok_or("Missing cases")?;
    let mut stores = 0usize;
    for (i, c) in cases.iter().enumerate() {
        let r505 = bytes_of(c["rec505"].as_str().ok_or("rec505")?)?;
        let r500 = bytes_of(c["rec500"].as_str().ok_or("rec500")?)?;
        let got = load_engine_20b4f1c0(
            (!r505.is_empty()).then_some(&r505[..]),
            (!r500.is_empty()).then_some(&r500[..]),
        );
        let returned = c["returned"].as_bool().ok_or("returned")?;
        let Some(got) = got else {
            if !returned {
                return Err(format!("Case {i}: original loaded a record, port found none"));
            }
            continue;
        };
        if returned {
            return Err(format!("Case {i}: original found no record, port did"));
        }
        let region = bytes_of(c["region"].as_str().ok_or("region")?)?;
        for s in got {
            stores += 1;
            match s {
                Store::Word(o, v) => {
                    let w = u32::from_le_bytes(region[o - REGION..o - REGION + 4].try_into().unwrap());
                    if w != v {
                        return Err(format!("Case {i} word {o:#x}: got {v:#x}, original {w:#x}"));
                    }
                }
                Store::Half(o, v) => {
                    let w = u16::from_le_bytes(region[o - REGION..o - REGION + 2].try_into().unwrap());
                    if w != v {
                        return Err(format!("Case {i} half {o:#x}: got {v:#x}, original {w:#x}"));
                    }
                }
                Store::Double(o, v) => {
                    let w = f64::from_le_bytes(region[o - REGION..o - REGION + 8].try_into().unwrap());
                    let same = v.to_bits() == w.to_bits() || (v - w).abs() <= 4e-16 * w.abs();
                    if !same {
                        return Err(format!("Case {i} double {o:#x}: got {v}, original {w}"));
                    }
                }
            }
        }
    }
    println!("PASS: {} loader runs, {stores} stores matched", cases.len());
    Ok(())
}
