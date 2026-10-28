//! Piston engine helper sim1 0x20B4D7E0 (used by the default-curve builder 0x20B4D8A0).
//! The original evaluates in x87 extended precision; this f64 port matches it to a
//! relative error of about 5e-16 (see scripts/check-piston-helper.py), not bit-exactly.
use crate::legacy_atmosphere::{
    SPECIFIC_HEAT_RATIO, STANDARD_SEA_LEVEL_PRESSURE, STANDARD_SEA_LEVEL_TEMPERATURE,
};
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
    let a = f.f140 * STANDARD_SEA_LEVEL_TEMPERATURE;
    let ratio = if a > 0. {
        (PRESSURE_OFFSET + a) / a
    } else {
        0.
    };
    let divisor = 1. - SPECIFIC_HEAT_RATIO;
    let inner = ratio * f.f148 * (f.f150 * f.f130 - f.f128) - f.f118 + f.f148 * f.f120;
    inner * SCALE * 0.5 / TWO_PI / divisor * f.f5f8
}

/// Piecewise-linear table lookup, sim1 0x20AF1710: clamps to the end values and
/// interpolates linearly between rows. An empty table yields 0.
pub fn lookup(table: &[(f64, f64)], x: f64) -> f64 {
    let (Some(first), Some(last)) = (table.first(), table.last()) else {
        return 0.;
    };
    if !(first.0 < x) {
        return first.1;
    }
    if x > last.0 {
        return last.1;
    }
    let mut i = 0;
    while i + 2 < table.len() && !(x <= table[i + 1].0) {
        i += 1;
    }
    let ((x0, y0), (x1, y1)) = (table[i], table[i + 1]);
    y0 + (y1 - y0) * ((x - x0) / (x1 - x0))
}
// Tables captured from the stack frame of 0x20B4D8A0 (x, y rows).
const TABLE_1: [(f64, f64); 6] = [
    (0., 0.),
    (500., 4.2),
    (1000., 2.1),
    (2100., 1.2),
    (2400., 1.),
    (3000., 1.),
];
const TABLE_2: [(f64, f64); 7] = [
    (0., 0.),
    (50., 1.),
    (100., 1.),
    (230., 1.),
    (300., 1.),
    (400., 1.),
    (1200., 1.),
];
const TABLE_3: [(f64, f64); 6] = [
    (0., 0.),
    (500., 1.),
    (1000., 1.),
    (2400., 1.1),
    (2800., 1.15),
    (3200., 1.35),
];
const TABLE_4: [(f64, f64); 7] = [
    (0., 0.),
    (50., 1.),
    (100., 1.),
    (230., 1.),
    (300., 1.),
    (400., 1.1),
    (1200., 1.2),
];
/// Extra fields read by the default-curve builder.
#[derive(Debug, Clone, Copy)]
pub struct CurveInputs {
    pub helper: Fields,
    pub f158: f64,
    pub f178: f64,
    pub f180: f64,
}
/// Output of 0x20B4D8A0: the arrays copied to +0x5CC (5 rows) and +0x620 (4 rows);
/// the four scalars at +0x2B0/+0x2B8/+0x2C0/+0x2C8 are all set to 1.0.
#[derive(Debug, Clone, PartialEq)]
pub struct Curves {
    pub array_5cc: [(f64, f64); 5],
    pub array_620: [(f64, f64); 4],
}
pub fn default_curves(i: &CurveInputs) -> Curves {
    let f = &i.helper;
    let (rpm, f158) = (i.f178, i.f158);
    let pressure = f.f140 * STANDARD_SEA_LEVEL_TEMPERATURE;
    let gravity_term = f.f148 * STANDARD_SEA_LEVEL_PRESSURE;
    let ratio_term = if pressure > 0. {
        ((pressure + PRESSURE_OFFSET) / pressure) * gravity_term
    } else {
        0.
    };
    let loss = if rpm > 0. {
        i.f180 / (rpm * 0.1047197551 * 0.001818182)
    } else {
        0.
    };
    let divisor = 1. - SPECIFIC_HEAT_RATIO;
    let mut c = if f.f170 == 0 {
        0.
    } else {
        let g = STANDARD_SEA_LEVEL_PRESSURE;
        ((((f.f150 * ratio_term * f.f130 - f.f128 * ratio_term) / divisor
            - (f.f118 * g - f.f120 * gravity_term) / divisor)
            * 0.5)
            / TWO_PI)
            * f158
            - loss / f64::from(f.f170)
    };
    // The builder first copies its first array over +0x5CC..+0x620, which overwrites the
    // helper's +0x5F8 input with that array's third y value (f158 * 1.05).
    let mut v = helper_20b4d7e0(&Fields {
        f5f8: f158 * 1.05,
        ..*f
    });
    if c < 18. {
        if 16.2 <= v {
            c = v * 1.1;
        } else {
            c = 18.;
            v = 16.2;
        }
    }
    c *= lookup(&TABLE_1, rpm);
    c *= lookup(&TABLE_2, i.f180);
    v *= lookup(&TABLE_3, rpm);
    let mut w = lookup(&TABLE_4, i.f180) * v;
    if c < w {
        w = 0.9 * c;
    }
    Curves {
        array_5cc: [
            (0., f158 * 1.2),
            (rpm * 0.25, f158 * 1.2),
            (rpm * 0.75, f158 * 1.05),
            (rpm * 0.8, f158),
            (rpm, f158),
        ],
        array_620: [
            (rpm * -0.11, -(c + c)),
            (rpm * 0.11, c + c),
            (rpm * 0.185, w),
            (rpm, c),
        ],
    }
}

/// Thermodynamic fields written by sim1 0x20B4EE40 at +0x118..+0x150 of the engine
/// object (the remaining bytes of the zeroed 0x40-byte block stay zero).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thermodynamics {
    pub f118: f64,
    pub f120: f64,
    pub f128: f64,
    pub f130: f64,
    pub f138: f64,
    pub f140: f64,
    pub f148: f64,
    pub f150: f64,
}

/// sim1 0x20B4EE40 from the displacement `volume` (+0x160, ft^3), compression ratio
/// `ratio` (+0x168) and cylinder count `cylinders` (u32 at +0x170). The original calls
/// its x87 pow (0x20B8BF10); this port uses f64 `powf`, so it is not bit-exact.
pub fn thermodynamics_20b4ee40(volume: f64, ratio: f64, cylinders: u32) -> Thermodynamics {
    let gamma = SPECIFIC_HEAT_RATIO;
    let f118 = ratio / (ratio - 1.) * volume;
    let f120 = f118 / ratio;
    Thermodynamics {
        f118,
        f120,
        f128: f120,
        f130: f118,
        f138: f64::from(cylinders) * volume,
        f140: ratio.powf(gamma - 1.),
        f148: ratio.powf(gamma),
        f150: if ratio > 0. { (1. / ratio).powf(gamma) } else { 0. },
    }
}
