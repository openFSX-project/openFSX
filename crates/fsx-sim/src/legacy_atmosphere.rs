//! Standard-atmosphere constants of sim1.dll (FUN_20AF5D20, 0x20B992A0..0x20B992E0).
//! The original fills these doubles through a pointer table from a configuration
//! lookup (Ordinal_44 on DAT_20AE2464) and falls back to these defaults when the key
//! is missing or not positive. US customary units. The configuration source of the
//! overrides is not yet identified, so only the pinned defaults are provided.
pub const SPECIFIC_GAS_CONSTANT: f64 = 1716.0;
pub const SPECIFIC_HEAT_RATIO: f64 = 1.4;
/// Slugs per cubic foot (0x20B992B0).
pub const STANDARD_SEA_LEVEL_DENSITY: f64 = 0.0023769;
/// Rankine (0x20B992B8).
pub const STANDARD_SEA_LEVEL_TEMPERATURE: f64 = 518.69;
/// Pounds per square foot (0x20B992C0).
pub const STANDARD_SEA_LEVEL_PRESSURE: f64 = 2116.2;
/// Feet per second squared (0x20B992C8).
pub const GRAVITY_SEA_LEVEL: f64 = 32.174;
/// Feet (0x20B992D0, 0x20B992D8).
pub const POLAR_RADIUS: f64 = 20_890_130.604_584_83;
pub const EQUATORIAL_RADIUS: f64 = 20_925_637.612_886_17;
#[cfg(test)]
mod tests {
    use super::*;
    /// Raw default bit patterns read from the pinned DLL (REA, FUN_20AF5D20).
    #[test]
    fn defaults_match_the_pinned_bit_patterns() {
        for (value, bits) in [
            (SPECIFIC_GAS_CONSTANT, 0x409A_D000_0000_0000_u64),
            (SPECIFIC_HEAT_RATIO, 0x3FF6_6666_6666_6666),
            (STANDARD_SEA_LEVEL_DENSITY, 0x3F63_78B8_7881_F1DD),
            (STANDARD_SEA_LEVEL_TEMPERATURE, 0x4080_3585_1EB8_51EC),
            (STANDARD_SEA_LEVEL_PRESSURE, 0x40A0_8866_6666_6666),
            (GRAVITY_SEA_LEVEL, 0x4040_1645_A1CA_C083),
            (POLAR_RADIUS, 0x4173_EC21_29AC_6125),
            (EQUATORIAL_RADIUS, 0x4173_F4CC_59CE_61BB),
        ] {
            assert_eq!(value.to_bits(), bits);
        }
    }
}
