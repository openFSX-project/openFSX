//! Explicit local time of day advanced by simulator elapsed seconds.
//! Calendar, timezone, UTC and host wall-clock synchronization are separate concerns.
#[derive(Debug, Clone, Copy)]
pub struct LocalClock {
    start: u32,
}
impl LocalClock {
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
