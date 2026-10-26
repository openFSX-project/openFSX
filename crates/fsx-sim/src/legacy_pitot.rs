//! Pinned original thermal prefix only; environment scalar and seed are explicit.
//! Physical unit/provider mapping and icing/indicated-air effects remain open.
#[derive(Debug, Clone, Copy, Default)]
pub struct Inputs {
    pub requested: Option<bool>,
    pub heater_rate: Option<f32>,
    pub power_available: Option<bool>,
    pub environment_scalar: Option<f64>,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    pub active: Option<bool>,
    pub temperature: Option<f32>,
}
#[derive(Debug, Clone, Default)]
pub struct Core {
    temperature: Option<f32>,
}
impl Core {
    pub fn temperature(&self) -> Option<f32> {
        self.temperature
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
