//! Authored pitot heater input and pinned legacy-field fallback.
//! Configuration write-back is reported as data, never applied implicitly.
use fsx_cfg::Config;
#[derive(Debug, Clone, Copy, Default)]
pub struct PitotStatic {
    pub pitot_heat: Option<f64>,
}
#[derive(Debug, Clone, Copy)]
pub enum LegacyPitotHeat {
    Unknown,
    Missing,
    Value(bool),
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeaterConfiguration {
    pub rate: Option<f32>,
    /// Original initializer's requested CFG write; caller controls persistence.
    pub write_back: Option<f64>,
}
impl PitotStatic {
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
