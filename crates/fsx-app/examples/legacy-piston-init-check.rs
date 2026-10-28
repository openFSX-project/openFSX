//! Compare the original piston initializer (reference report, up to its first
//! Ordinal_55 call) with the Rust port of its derived fields and constants.
use fsx_sim::legacy_piston::{
    init_derived_20b4ff10, thermodynamics_20b4ee40, INIT_DOUBLES_20B4FF10, INIT_DOUBLE_32C,
    INIT_WORDS_20B4FF10,
};
use serde_json::Value;
const REGION: usize = 0x100;
fn main() -> Result<(), String> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("Usage: legacy-piston-init-check <reference report.json>")?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Reference exceeds byte budget".into());
    }
    let report: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if report["source"] != "bounded-original-piston-init-4ff10"
        || report["sha256"] != "4bb9ef1b9669bb48676ef16ede667a2c590af27943eb34f03f09332a5417ae0e"
    {
        return Err("Unexpected piston init reference".into());
    }
    let cases = report["cases"].as_array().ok_or("Missing cases")?;
    for (i, c) in cases.iter().enumerate() {
        let n = |k: &str| c[k].as_u64().ok_or_else(|| format!("Missing {k}"));
        let hex = c["region"].as_str().ok_or("Missing region")?;
        let raw: Vec<u8> = (0..hex.len() / 2)
            .map(|k| u8::from_str_radix(&hex[2 * k..2 * k + 2], 16).map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?;
        let u32_at = |o: usize| u32::from_le_bytes(raw[o - REGION..o - REGION + 4].try_into().unwrap());
        let bits_at = |o: usize| u64::from_le_bytes(raw[o - REGION..o - REGION + 8].try_into().unwrap());
        let d = init_derived_20b4ff10(n("e110")? as u32, n("e10c")? as u16, n("e10e")? as u16);
        let checks = [
            ("f158", d.f158.to_bits(), bits_at(0x158)),
            ("f168", d.f168.to_bits(), bits_at(0x168)),
            ("f170", u64::from(d.f170), u64::from(u32_at(0x170))),
            ("f178", d.f178.to_bits(), bits_at(0x178)),
            ("f180", d.f180.to_bits(), bits_at(0x180)),
            ("c32c", INIT_DOUBLE_32C, bits_at(0x32c)),
        ];
        for (name, got, want) in checks {
            if got != want {
                return Err(format!("Case {i} {name}: got {got:#x}, original {want:#x}"));
            }
        }
        // f160 is a three-step x87 expression: f64 differs from extended precision in the last bit.
        let want160 = f64::from_bits(bits_at(0x160));
        if (d.f160 - want160).abs() > 4e-16 * want160.abs() {
            return Err(format!("Case {i} f160: got {}, original {want160}", d.f160));
        }
        for (o, b) in INIT_DOUBLES_20B4FF10 {
            if bits_at(o) != b {
                return Err(format!("Case {i} double {o:#x}: original {:#x}", bits_at(o)));
            }
        }
        for (o, w) in INIT_WORDS_20B4FF10 {
            if u32_at(o) != w {
                return Err(format!("Case {i} word {o:#x}: original {:#x}", u32_at(o)));
            }
        }
        let t = thermodynamics_20b4ee40(d.f160, d.f168, d.f170);
        let f = |o: usize| f64::from_bits(bits_at(o));
        for (name, got, want) in [
            ("f118", t.f118, f(0x118)),
            ("f120", t.f120, f(0x120)),
            ("f138", t.f138, f(0x138)),
            ("f140", t.f140, f(0x140)),
            ("f148", t.f148, f(0x148)),
            ("f150", t.f150, f(0x150)),
        ] {
            if (got - want).abs() > 1e-13 * want.abs() {
                return Err(format!("Case {i} {name}: got {got}, original {want}"));
            }
        }
    }
    println!(
        "PASS: {} original initializer runs (f160 within 4e-16, other derived fields and constants bit-exact)",
        cases.len()
    );
    Ok(())
}
