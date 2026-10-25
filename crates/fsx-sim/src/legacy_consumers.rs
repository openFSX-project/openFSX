//! Original ordered consumer-load sum. Slot 0 is not visited; slots 1..=33 are.
//! Metadata/presence and prior activity need providers; CFG omissions are unknown.
pub const SLOTS: usize = 34;
#[derive(Debug, Clone, Copy, Default)]
pub struct Consumer {
    /// Raw original prior-frame activity word; any nonzero value is active.
    pub active: Option<u32>,
    /// Known configured maximum. None is unresolved, not a fabricated zero.
    pub max_amps: Option<f64>,
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
