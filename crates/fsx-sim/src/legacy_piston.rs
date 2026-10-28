//! Piston engine helper sim1 0x20B4D7E0 (used by the default-curve builder 0x20B4D8A0).
//! The original evaluates in x87 extended precision; this f64 port matches it to a
//! relative error of about 5e-16 (see scripts/check-piston-helper.py), not bit-exactly.
use crate::legacy_atmosphere::{STANDARD_SEA_LEVEL_DENSITY, STANDARD_SEA_LEVEL_PRESSURE};
/// Engine object fields read by the helper (byte offsets in the original object).
#[derive(Debug, Clone, Copy)]
pub struct Fields {
    pub f118: f64,
    pub f120: f64,
    pub f128: f64,
    pub f130: f64,
    pub f140: f64,
    pub f148: f64,
    pub f150: f64,
    pub f5f8: f64,
    /// Unsigned 32-bit value at +0x170; zero disables the helper.
    pub f170: u32,
}
const PRESSURE_OFFSET: f64 = 8224.513023991623; // 0x20AE9448
const SCALE: f64 = 350.0; // 0x20AE9198
const TWO_PI: f64 = 6.283185307179586; // 0x20AE8DC0
pub fn helper_20b4d7e0(f: &Fields) -> f64 {
    if f.f170 == 0 {
        return 0.;
    }
    let a = f.f140 * STANDARD_SEA_LEVEL_PRESSURE;
    let ratio = if a > 0. {
        (PRESSURE_OFFSET + a) / a
    } else {
        0.
    };
    let divisor = 1. - STANDARD_SEA_LEVEL_DENSITY;
    let inner = ratio * f.f148 * (f.f150 * f.f130 - f.f128) - f.f118 + f.f148 * f.f120;
    inner * SCALE * 0.5 / TWO_PI / divisor * f.f5f8
}
