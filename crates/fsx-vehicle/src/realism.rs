//! Explicit authored [REALISM] inputs; no installation/user-profile discovery.
use fsx_cfg::Config;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Realism {
    /// Original double setting. Missing remains unknown; no nominal 1 default.
    pub general: Option<f64>,
}
impl Realism {
    pub fn parse(text: &str) -> Result<Self, String> {
        Self::from_config(&Config::parse(text)?)
    }
    pub fn from_config(config: &Config) -> Result<Self, String> {
        let general = config
            .get("realism", "general")
            .map(|value| {
                let value: f64 = value.parse().map_err(|_| "Invalid [REALISM] General")?;
                if !value.is_finite() || value < 0. || !(value as f32).is_finite() {
                    return Err("Unsupported nonfinite/negative/overflowing [REALISM] General");
                }
                Ok(value)
            })
            .transpose()
            .map_err(str::to_owned)?;
        Ok(Self { general })
    }
}
