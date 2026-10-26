//! Original ordered consumer-load sum. Slot 0 is not visited; slots 1..=33 are.
//! Metadata/presence and prior activity need providers; CFG omissions are unknown.
pub const SLOTS: usize = 34;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Consumer {
    /// Raw original prior-frame activity word; any nonzero value is active.
    pub active: Option<u32>,
    /// Known configured maximum. None is unresolved, not a fabricated zero.
    pub max_amps: Option<f64>,
}
/// Prior equipment activity owned independently of power availability. Fresh
/// allocation zeros all activity; existing/unknown aircraft must use unknown().
#[derive(Debug, Clone)]
pub struct ConsumerState {
    records: [Consumer; SLOTS],
}
impl ConsumerState {
    /// Checked pitot activity prefix only; heater rate provenance and thermal/
    /// icing dynamics remain separate providers. Availability is not a switch.
    pub fn update_pitot_activity(
        &mut self,
        requested: Option<bool>,
        heater_rate: Option<f32>,
        power_available: Option<bool>,
    ) -> Result<(), String> {
        if heater_rate.is_some_and(|v| !v.is_finite()) {
            return Err("Nonfinite pitot heater rate".into());
        }
        let active = match requested {
            Some(false) => Some(false),
            Some(true) => match heater_rate {
                Some(v) if v <= 0. => Some(false),
                Some(_) => power_available,
                None => None,
            },
            None => None,
        };
        self.set_active(7, active)
    }
    pub fn observed(
        config: &fsx_vehicle::electrical::Electrical,
        activity: [Option<u32>; SLOTS],
    ) -> Self {
        Self {
            records: authored(config, activity),
        }
    }
    pub fn fresh(config: &fsx_vehicle::electrical::Electrical) -> Self {
        Self {
            records: authored(config, [Some(0); SLOTS]),
        }
    }
    pub fn unknown(config: &fsx_vehicle::electrical::Electrical) -> Self {
        Self {
            records: authored(config, [None; SLOTS]),
        }
    }
    pub fn records(&self) -> &[Consumer; SLOTS] {
        &self.records
    }
    /// Original equipment adapter activation/deactivation normalizes to 1/0;
    /// it does not query voltage or alter other records. None loses observation.
    pub fn set_active(&mut self, slot: usize, active: Option<bool>) -> Result<(), String> {
        let record = self
            .records
            .get_mut(slot)
            .ok_or("Legacy consumer slot must be 0..33")?;
        record.active = active.map(u32::from);
        Ok(())
    }
    /// The original electrical tail writes only these activity records. Other
    /// equipment providers retain ownership of their flags between frames.
    pub fn observe_frame(&mut self, frame: Option<crate::legacy_voltage::FrameStep>) {
        self.records[1].active = frame.map(|f| u32::from(f.tail.additional_system_active));
        self.records[5].active = frame.map(|f| u32::from(f.tail.avionics_active));
    }
}
/// Bind only authored maxima to verified slots. Activity is supplied from the
/// preceding frame; absent CFG keys remain unresolved, not zero or absent gear.
pub fn authored(
    config: &fsx_vehicle::electrical::Electrical,
    activity: [Option<u32>; SLOTS],
) -> [Consumer; SLOTS] {
    let mut result = [Consumer::default(); SLOTS];
    result[0].active = activity[0];
    for (i, key) in fsx_vehicle::electrical::LEGACY_CONSUMER_KEYS
        .iter()
        .enumerate()
    {
        result[i + 1] = Consumer {
            active: activity[i + 1],
            max_amps: config.circuits.get(*key).map(|c| c.max_amps),
        };
    }
    result
}
pub fn sum(consumers: &[Consumer; SLOTS]) -> Result<Option<f64>, String> {
    let mut total = 0.;
    let mut known = true;
    for consumer in &consumers[1..] {
        if consumer
            .max_amps
            .is_some_and(|v| !v.is_finite() || !(0. ..=1e6).contains(&v))
        {
            return Err("Invalid legacy consumer maximum".into());
        }
        match consumer.active {
            Some(0) => (),
            Some(_) => match consumer.max_amps {
                Some(amps) => total += amps,
                None => known = false,
            },
            None => known = false,
        }
    }
    Ok(known.then_some(total))
}
