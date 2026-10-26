//! Authored pitot heater input and pinned legacy-field fallback.
//! Configuration write-back is reported as data, never applied implicitly.
use fsx_cfg::Config;
#[derive(Debug, Clone, Copy, Default)]
pub struct PitotStatic {
    pub pitot_heat: Option<f64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyPitotHeat {
    Unknown,
    Missing,
    Value(bool),
}
impl LegacyPitotHeat {
    /// Pinned original record lookup, first 32-bit word of AIR record 0x0324.
    /// Short malformed records are outside the deterministic supported domain.
    pub fn from_air(air: &crate::air::AirFile) -> Result<Self, String> {
        let Some(bytes) = air.records.get(&0x324) else {
            return Ok(Self::Missing);
        };
        let word = bytes
            .get(..4)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
            .ok_or("AIR pitot-heat record 0x0324 must contain at least 4 bytes")?;
        Ok(Self::Value(word != 0))
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeaterConfiguration {
    pub rate: Option<f32>,
    /// Original initializer's requested CFG write; caller controls persistence.
    pub write_back: Option<f64>,
}
impl PitotStatic {
    /// Avoid consulting legacy data when an explicit nonnegative CFG value won.
    pub fn legacy_input_from_air(
        self,
        air: &crate::air::AirFile,
    ) -> Result<LegacyPitotHeat, String> {
        self.validate()?;
        if self.pitot_heat.is_some_and(|v| v >= 0.) {
            Ok(LegacyPitotHeat::Unknown)
        } else {
            LegacyPitotHeat::from_air(air)
        }
    }
    pub fn parse(text: &str) -> Result<Self, String> {
        Self::from_config(&Config::parse(text)?)
    }
    pub fn from_config(config: &Config) -> Result<Self, String> {
        let pitot_heat = config
            .get("pitot_static", "pitot_heat")
            .map(|v| {
                v.parse::<f64>()
                    .map_err(|_| "Invalid [pitot_static] pitot_heat".to_string())
            })
            .transpose()?;
        let result = Self { pitot_heat };
        result.validate()?;
        Ok(result)
    }
    fn validate(self) -> Result<(), String> {
        if self
            .pitot_heat
            .is_some_and(|v| !v.is_finite() || (v > 0. && !(v as f32).is_finite()))
        {
            return Err("Unsupported nonfinite/overflowing pitot heater input".into());
        }
        Ok(())
    }
    pub fn resolve(self, legacy: LegacyPitotHeat) -> Result<HeaterConfiguration, String> {
        self.validate()?;
        if let Some(value) = self.pitot_heat.filter(|v| *v >= 0.) {
            return Ok(HeaterConfiguration {
                rate: Some(if value == 0. { 0. } else { value as f32 }),
                write_back: None,
            });
        }
        let rate = match legacy {
            LegacyPitotHeat::Unknown => None,
            LegacyPitotHeat::Missing | LegacyPitotHeat::Value(true) => Some(1.),
            LegacyPitotHeat::Value(false) => Some(0.),
        };
        Ok(HeaterConfiguration {
            rate,
            write_back: rate.map(f64::from),
        })
    }
}
