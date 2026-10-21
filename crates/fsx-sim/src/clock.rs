//! Explicit local time of day advanced by simulator elapsed seconds.
//! Calendar, timezone, UTC and host wall-clock synchronization are separate concerns.
#[derive(Debug, Clone, Copy)]
pub struct LocalClock {
    start: u32,
    initial: u32,
}
/// Published legacy simulator clock events; payload is used only by SET.
#[derive(Debug, Clone, Copy)]
pub enum ClockEvent {
    HoursDec,
    HoursInc,
    MinutesDec,
    MinutesInc,
    SecondsZero,
    HoursSet,
    MinutesSet,
}
impl ClockEvent {
    pub fn from_name(name: &str) -> Result<Self, String> {
        match name.to_ascii_uppercase().as_str() {
            "CLOCK_HOURS_DEC" => Ok(Self::HoursDec),
            "CLOCK_HOURS_INC" => Ok(Self::HoursInc),
            "CLOCK_MINUTES_DEC" => Ok(Self::MinutesDec),
            "CLOCK_MINUTES_INC" => Ok(Self::MinutesInc),
            "CLOCK_SECONDS_ZERO" => Ok(Self::SecondsZero),
            "CLOCK_HOURS_SET" => Ok(Self::HoursSet),
            "CLOCK_MINUTES_SET" => Ok(Self::MinutesSet),
            _ => Err("Unsupported local clock event".into()),
        }
    }
}
impl LocalClock {
    pub fn reset(&mut self) {
        self.start = self.initial;
    }
    pub fn apply(&mut self, event: ClockEvent, data: u32, elapsed: f64) -> Result<(), String> {
        let current = self.seconds_at(elapsed)?;
        let seconds = match event {
            ClockEvent::HoursDec => (current + 86400 - 3600) % 86400,
            ClockEvent::HoursInc => (current + 3600) % 86400,
            ClockEvent::MinutesDec => (current + 86400 - 60) % 86400,
            ClockEvent::MinutesInc => (current + 60) % 86400,
            ClockEvent::SecondsZero => current - current % 60,
            ClockEvent::HoursSet if data < 24 => data * 3600 + current % 3600,
            ClockEvent::MinutesSet if data < 60 => current / 3600 * 3600 + data * 60 + current % 60,
            _ => return Err("Clock SET payload is outside the hour/minute range".into()),
        };
        self.start = (seconds + 86400 - (elapsed.floor() as u64 % 86400) as u32) % 86400;
        Ok(())
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        let b = value.as_bytes();
        if b.len() != 8
            || b[2] != b':'
            || b[5] != b':'
            || [0, 1, 3, 4, 6, 7].iter().any(|&i| !b[i].is_ascii_digit())
        {
            return Err("Local clock requires HH:MM:SS".into());
        }
        let pair = |i| u32::from(b[i] - b'0') * 10 + u32::from(b[i + 1] - b'0');
        let (h, m, s) = (pair(0), pair(3), pair(6));
        if h >= 24 || m >= 60 || s >= 60 {
            return Err("Local clock is outside 00:00:00..23:59:59".into());
        }
        Ok(Self {
            start: h * 3600 + m * 60 + s,
            initial: h * 3600 + m * 60 + s,
        })
    }
    pub fn seconds_at(self, elapsed: f64) -> Result<u32, String> {
        if !elapsed.is_finite() || !(0. ..=1e9).contains(&elapsed) {
            return Err("Invalid simulator elapsed time for local clock".into());
        }
        Ok(((u64::from(self.start) + elapsed.floor() as u64) % 86400) as u32)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clock_events_wrap_preserve_other_fields_and_reject_invalid_sets_atomically() {
        let mut clock = LocalClock::parse("23:59:59").unwrap();
        clock.apply(ClockEvent::HoursInc, u32::MAX, 0.75).unwrap();
        assert_eq!(clock.seconds_at(0.75).unwrap(), 3599);
        clock.apply(ClockEvent::MinutesInc, 999, 0.75).unwrap();
        assert_eq!(clock.seconds_at(0.75).unwrap(), 3659);
        clock.apply(ClockEvent::MinutesDec, 0, 0.75).unwrap();
        clock.apply(ClockEvent::HoursDec, 0, 0.75).unwrap();
        assert_eq!(clock.seconds_at(0.75).unwrap(), 86399);
        clock.apply(ClockEvent::HoursSet, 13, 0.75).unwrap();
        clock.apply(ClockEvent::MinutesSet, 34, 0.75).unwrap();
        assert_eq!(clock.seconds_at(0.75).unwrap(), 13 * 3600 + 34 * 60 + 59);
        clock.apply(ClockEvent::SecondsZero, 0, 0.75).unwrap();
        let saved = clock.seconds_at(0.75).unwrap();
        assert_eq!(saved, 13 * 3600 + 34 * 60);
        assert_eq!(clock.seconds_at(1.).unwrap(), saved + 1);
        for (event, data) in [
            (ClockEvent::HoursSet, 24),
            (ClockEvent::MinutesSet, 60),
            (ClockEvent::HoursSet, u32::MAX),
        ] {
            assert!(clock.apply(event, data, 0.75).is_err());
            assert_eq!(clock.seconds_at(0.75).unwrap(), saved);
        }
        assert!(clock.apply(ClockEvent::HoursInc, 0, f64::NAN).is_err());
        assert_eq!(clock.seconds_at(0.75).unwrap(), saved);
        assert!(ClockEvent::from_name("clock_hours_inc").is_ok());
        assert!(ClockEvent::from_name("CLOCK_SECONDS_SET").is_err());
        clock.reset();
        assert_eq!(clock.seconds_at(0.75).unwrap(), 86399);
    }
    #[test]
    fn clock_rolls_over_at_whole_simulator_seconds_and_rewinds_deterministically() {
        let clock = LocalClock::parse("23:59:59").unwrap();
        for (elapsed, expected) in [
            (0., 86399),
            (0.999, 86399),
            (1., 0),
            (61., 60),
            (86401., 0),
            (0., 86399),
        ] {
            assert_eq!(clock.seconds_at(elapsed).unwrap(), expected);
        }
        for bad in [
            "24:00:00",
            "12:60:00",
            "12:00:60",
            "1:00:00",
            "-1:00:00",
            "12:00:00x",
            "１２:00:00",
        ] {
            assert!(LocalClock::parse(bad).is_err());
        }
        for bad in [-1., f64::NAN, f64::INFINITY, 1e9 + 1.] {
            assert!(clock.seconds_at(bad).is_err());
        }
    }
}
