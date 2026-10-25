//! Versioned General-realism copy into the electrical response context.
//! Query meanings and automatic context ownership are still unresolved.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Response {
    factor: Option<f32>,
}
impl Response {
    pub fn observed(factor: Option<f32>) -> Result<Self, String> {
        if factor.is_some_and(|v| !v.is_finite() || v < 0.) {
            return Err("Invalid observed electrical response".into());
        }
        Ok(Self { factor })
    }
    pub fn factor(self) -> Option<f32> {
        self.factor
    }
    /// The first original context query gates the whole settings update. The
    /// second gates the General copy. False retains history; unresolved queries
    /// or an admitted missing value lose knowledge. Inputs fail atomically.
    pub fn copy_settings(
        &mut self,
        settings: fsx_vehicle::realism::Realism,
        update_allowed: Option<bool>,
        general_copy_allowed: Option<bool>,
    ) -> Result<(), String> {
        let value = settings.general.map(|v| v as f32);
        if settings.general.is_some_and(|v| !v.is_finite() || v < 0.)
            || value.is_some_and(|v| !v.is_finite())
        {
            return Err("Invalid General realism response input".into());
        }
        self.factor = match (update_allowed, general_copy_allowed) {
            (Some(false), _) | (Some(true), Some(false)) => self.factor,
            (Some(true), Some(true)) => value,
            _ => None,
        };
        Ok(())
    }
}
