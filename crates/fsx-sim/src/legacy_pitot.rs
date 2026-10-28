//! Pinned original thermal prefix and icing/filter tail (sim1 0x20B458B0..0x20B45B3E);
//! environment scalar, seed and weather flags are explicit. Physical unit/provider
//! mapping of those inputs and the indicated-air consumer remain open.
#[derive(Debug, Clone, Copy, Default)]
pub struct Inputs {
    pub requested: Option<bool>,
    pub heater_rate: Option<f32>,
    pub power_available: Option<bool>,
    pub environment_scalar: Option<f64>,
}
/// Icing-tail inputs: weather +0x54 / +0x50 flags, owner +0x14C field and the
/// filtered input value (first stack argument of the original function).
#[derive(Debug, Clone, Copy)]
pub struct IcingInputs {
    pub weather_enabled: bool,
    pub weather_precipitation: bool,
    pub field_14c: f64,
    pub input: f64,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IcingStep {
    pub step: Step,
    pub ice: f32,
    pub filter: f64,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    pub active: Option<bool>,
    pub temperature: Option<f32>,
}
#[derive(Debug, Clone, Default)]
pub struct Core {
    temperature: Option<f32>,
    ice: f32,
    filter: f64,
}
impl Core {
    pub fn temperature(&self) -> Option<f32> {
        self.temperature
    }
    pub fn ice(&self) -> f32 {
        self.ice
    }
    pub fn filter(&self) -> f64 {
        self.filter
    }
    pub fn seed_icing(&mut self, ice: f32, filter: f64) -> Result<(), String> {
        if !ice.is_finite() || !filter.is_finite() {
            return Err("Nonfinite pitot icing seed".into());
        }
        self.ice = ice;
        self.filter = filter;
        Ok(())
    }
    /// Thermal prefix followed by the original icing and filter tail. Returns the
    /// filter value (the original's return and owner+0x16C). Unknown thermal
    /// inputs leave the icing state untouched and return None.
    pub fn advance_icing(
        &mut self,
        input: Inputs,
        icing: IcingInputs,
        dt: f64,
    ) -> Result<Option<IcingStep>, String> {
        if !icing.field_14c.is_finite() || !icing.input.is_finite() {
            return Err("Nonfinite pitot icing input".into());
        }
        let step = self.advance(input, dt)?;
        let Some(temperature) = step.temperature else {
            return Ok(None);
        };
        let (mut ice, mut filter) = (self.ice, self.filter);
        if !icing.weather_enabled {
            filter = icing.input;
        } else {
            if 492. > f64::from(temperature) {
                if icing.field_14c >= 20. && icing.weather_precipitation {
                    ice = (dt * f64::from(0.1_f32)) as f32 + ice;
                    if !(ice < 1.) {
                        ice = 1.;
                    }
                }
            } else {
                let rate = input.heater_rate.ok_or("Missing pitot heater rate")?;
                let delta = (f64::from(rate) * f64::from(-0.1_f32) * dt) as f32;
                ice += delta;
                if !(0. < ice) {
                    ice = 0.;
                }
            }
            let target = if ice >= 1. { 0. } else { icing.input };
            let moved = filter + 0.5 * (target - filter);
            filter = if ice >= 1. {
                if moved > 0. {
                    moved
                } else {
                    0.
                }
            } else if moved >= target {
                target
            } else {
                moved
            };
        }
        if !ice.is_finite() || !filter.is_finite() {
            return Err("Pitot icing computation overflow".into());
        }
        self.ice = ice;
        self.filter = filter;
        Ok(Some(IcingStep { step, ice, filter }))
    }
    pub fn seed(&mut self, temperature: Option<f32>) -> Result<(), String> {
        if temperature.is_some_and(|v| !v.is_finite()) {
            return Err("Nonfinite pitot temperature seed".into());
        }
        self.temperature = temperature;
        Ok(())
    }
    pub fn advance(&mut self, input: Inputs, dt: f64) -> Result<Step, String> {
        if !dt.is_finite()
            || dt <= 0.
            || dt > 1. / 60.
            || input.environment_scalar.is_some_and(|v| !v.is_finite())
        {
            return Err("Invalid pitot thermal interval/environment".into());
        }
        let active = crate::legacy_consumers::pitot_activity(
            input.requested,
            input.heater_rate,
            input.power_available,
        )?;
        let temperature = match (self.temperature, input.environment_scalar, active) {
            (Some(mut temperature), Some(environment), Some(active)) => {
                let ambient = ((environment * 1.8 + 32.) + 459.7) as f32;
                if !ambient.is_finite() {
                    return Err("Pitot ambient conversion overflow".into());
                }
                if active {
                    let rate = input.heater_rate.ok_or("Missing active heater rate")?;
                    let increment = (f64::from(rate) * dt) as f32;
                    temperature += increment;
                } else if temperature > ambient {
                    temperature += (-3. * dt) as f32;
                }
                temperature = temperature.max(ambient).min(560.);
                if !temperature.is_finite() {
                    return Err("Pitot thermal computation overflow".into());
                }
                Some(temperature)
            }
            _ => None,
        };
        self.temperature = temperature;
        Ok(Step {
            active,
            temperature,
        })
    }
}
