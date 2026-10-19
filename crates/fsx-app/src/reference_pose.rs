//! Offline import of one explicitly selected measurement sample; no native DLL.
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
};

const MAX_FILE: u64 = 64 * 1024 * 1024;
const MAX_LINE: u64 = 16 * 1024;
const FIELDS: &[(&str, &str, &str)] = &[
    ("elevator_pct", "ELEVATOR DEFLECTION PCT", "percent"),
    ("elevator_deg", "ELEVATOR DEFLECTION", "degrees"),
    ("trim_pct", "ELEVATOR TRIM PCT", "percent"),
    ("trim_deg", "ELEVATOR TRIM POSITION", "degrees"),
    ("left_aileron_pct", "AILERON LEFT DEFLECTION PCT", "percent"),
    (
        "right_aileron_pct",
        "AILERON RIGHT DEFLECTION PCT",
        "percent",
    ),
    ("rudder_pct", "RUDDER DEFLECTION PCT", "percent"),
    ("rudder_deg", "RUDDER DEFLECTION", "degrees"),
    ("yoke_x_pct", "YOKE X POSITION", "percent"),
    ("yoke_y_pct", "YOKE Y POSITION", "percent"),
    (
        "throttle_pct",
        "GENERAL ENG THROTTLE LEVER POSITION:1",
        "percent",
    ),
    ("on_ground", "SIM ON GROUND", "bool"),
];
const INSTRUMENT_FIELDS: &[(&str, &str, &str)] = &[
    ("sim_time_seconds", "ABSOLUTE TIME", "seconds"),
    ("sim_rate", "SIMULATION RATE", "number"),
    ("ias_knots", "AIRSPEED INDICATED", "knots"),
    ("tas_knots", "AIRSPEED TRUE", "knots"),
    ("indicated_altitude_ft", "INDICATED ALTITUDE", "feet"),
    ("pressure_altitude_ft", "PRESSURE ALTITUDE", "feet"),
    ("altitude_msl_ft", "PLANE ALTITUDE", "feet"),
    ("vertical_speed_fpm", "VERTICAL SPEED", "feet per minute"),
    ("pitch_deg", "PLANE PITCH DEGREES", "degrees"),
    ("bank_deg", "PLANE BANK DEGREES", "degrees"),
    ("heading_true_deg", "PLANE HEADING DEGREES TRUE", "degrees"),
    (
        "heading_magnetic_deg",
        "PLANE HEADING DEGREES MAGNETIC",
        "degrees",
    ),
    (
        "gyro_pitch_deg",
        "ATTITUDE INDICATOR PITCH DEGREES",
        "degrees",
    ),
    (
        "gyro_bank_deg",
        "ATTITUDE INDICATOR BANK DEGREES",
        "degrees",
    ),
    ("gyro_heading_deg", "HEADING INDICATOR", "degrees"),
    ("baro_inhg", "KOHLSMAN SETTING HG", "inHg"),
    ("ambient_pressure_mbar", "AMBIENT PRESSURE", "millibars"),
    ("ambient_temp_c", "AMBIENT TEMPERATURE", "celsius"),
    ("total_weight_lb", "TOTAL WEIGHT", "pounds"),
    ("fuel_total_gallons", "FUEL TOTAL QUANTITY", "gallons"),
    ("engine_rpm", "GENERAL ENG RPM:1", "rpm"),
    (
        "rotation_body_x_rad_s",
        "ROTATION VELOCITY BODY X",
        "radians per second",
    ),
    (
        "rotation_body_y_rad_s",
        "ROTATION VELOCITY BODY Y",
        "radians per second",
    ),
    (
        "rotation_body_z_rad_s",
        "ROTATION VELOCITY BODY Z",
        "radians per second",
    ),
    ("groundspeed_knots", "GROUND VELOCITY", "knots"),
    ("failure_airspeed", "PARTIAL PANEL AIRSPEED", "enum"),
    ("failure_altimeter", "PARTIAL PANEL ALTIMETER", "enum"),
    ("failure_attitude", "PARTIAL PANEL ATTITUDE", "enum"),
    ("failure_heading", "PARTIAL PANEL HEADING", "enum"),
    ("failure_vsi", "PARTIAL PANEL VERTICAL VELOCITY", "enum"),
    ("attitude_caged", "ATTITUDE CAGE", "bool"),
    (
        "attitude_bars_ratio",
        "ATTITUDE BARS POSITION",
        "percent over 100",
    ),
];
#[derive(Debug)]
pub struct Instruments {
    values: BTreeMap<String, f64>,
}
impl Instruments {
    fn parse(data: &serde_json::Value) -> Result<Self, String> {
        let object = data.as_object().ok_or("Missing schema-2 instruments")?;
        if object.len() != INSTRUMENT_FIELDS.len() {
            return Err("Instrument field count does not match schema 2".into());
        }
        let mut values = BTreeMap::new();
        for &(key, _, unit) in INSTRUMENT_FIELDS {
            let value = object
                .get(key)
                .and_then(|v| v.as_f64())
                .filter(|v| v.is_finite() && v.abs() <= 1e12)
                .ok_or_else(|| format!("Invalid instrument value: {key}"))?;
            if unit == "bool" && value != 0. && value != 1. {
                return Err(format!("Instrument bool {key} must be 0 or 1"));
            }
            if unit == "enum" && ![0., 1., 2.].contains(&value) {
                return Err(format!("Unsupported instrument failure state: {key}"));
            }
            values.insert(key.into(), value);
        }
        if values["sim_time_seconds"] < 0. || values["sim_rate"] < 0. {
            return Err("Negative recorded simulation time/rate".into());
        }
        Ok(Self { values })
    }
    pub fn get(&self, key: &str) -> Option<f64> {
        self.values.get(key).copied()
    }
    pub fn time(&self) -> f64 {
        self.values["sim_time_seconds"]
    }
    /// Actual recorded instrument indications, without ideal-ISA reconstruction.
    pub fn native_inputs(&self) -> Result<[f64; 6], String> {
        if [
            "failure_airspeed",
            "failure_altimeter",
            "failure_attitude",
            "failure_heading",
            "failure_vsi",
            "attitude_caged",
            "attitude_bars_ratio",
        ]
        .iter()
        .any(|key| self.values[*key] != 0.)
        {
            return Err(
                "Recorded failed/caged/adjusted instruments exceed the checked native profile"
                    .into(),
            );
        }
        let input = [
            self.values["ias_knots"],
            self.values["indicated_altitude_ft"],
            self.values["vertical_speed_fpm"] * fsx_vehicle::aircraft::FOOT / 60. * 256.,
            self.values["gyro_pitch_deg"],
            self.values["gyro_bank_deg"],
            self.values["gyro_heading_deg"],
        ];
        if input[0] < 0. || input.iter().any(|v| !v.is_finite() || v.abs() > 1e12) {
            return Err("Recorded inputs exceed the native profile's numeric domain".into());
        }
        Ok(input)
    }
    fn lookup(&self, variable: &str, units: &str) -> Option<f32> {
        let &(key, _, unit) = INSTRUMENT_FIELDS
            .iter()
            .find(|(_, v, _)| v.eq_ignore_ascii_case(variable.trim()))?;
        converted(self.values[key], unit, units)
    }
}
fn converted(value: f64, native: &str, units: &str) -> Option<f32> {
    let requested = units.trim().to_ascii_lowercase();
    let value = if native.eq_ignore_ascii_case(&requested) {
        value
    } else {
        match (native, requested.as_str()) {
            ("percent", "percent over 100") => value / 100.,
            ("degrees", "degree") => value,
            ("degrees", "radian" | "radians") => value.to_radians(),
            ("feet", "meter" | "meters") => value * fsx_vehicle::aircraft::FOOT,
            ("knots", "meters per second") => value * fsx_vehicle::aircraft::KNOT,
            ("feet per minute", "meters per second") => value * fsx_vehicle::aircraft::FOOT / 60.,
            _ => return None,
        }
    };
    (value.is_finite() && value.abs() <= f32::MAX as f64).then_some(value as f32)
}
#[derive(Debug)]
pub struct Sample {
    pub source: String,
    pub title: String,
    pub elapsed: f64,
    pub instruments: Option<Instruments>,
    pub object_id: Option<u32>,
    pub index: Option<u32>,
    values: BTreeMap<String, f64>,
}
impl Sample {
    fn parse(bytes: &[u8]) -> Result<Self, String> {
        let data: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| format!("Invalid reference JSON: {e}"))?;
        let schema = data.get("schema").and_then(|v| v.as_u64());
        if !matches!(schema, Some(1 | 2)) {
            return Err("Unsupported reference schema".into());
        }
        let source = data
            .get("source")
            .and_then(|v| v.as_str())
            .ok_or("Missing reference source")?;
        if !["FSX SimConnect", "synthetic"].contains(&source) {
            return Err("Unsupported reference source label".into());
        }
        let title = data
            .get("title")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty() && s.len() <= 1024 && !s.contains('\0'))
            .ok_or("Invalid reference aircraft title")?;
        let elapsed = data
            .get("elapsed_seconds")
            .and_then(|v| v.as_f64())
            .filter(|v| v.is_finite() && *v >= 0.)
            .ok_or("Invalid reference elapsed time")?;
        let object = data
            .get("values")
            .and_then(|v| v.as_object())
            .ok_or("Missing reference values")?;
        if object.len() != FIELDS.len() {
            return Err("Reference field count does not match schema 1".into());
        }
        let mut values = BTreeMap::new();
        for &(key, _, _) in FIELDS {
            let value = object
                .get(key)
                .and_then(|v| v.as_f64())
                .filter(|v| v.is_finite() && v.abs() <= f32::MAX as f64)
                .ok_or_else(|| format!("Missing or non-finite reference value: {key}"))?;
            if key == "on_ground" && value != 0. && value != 1. {
                return Err("Reference on_ground must be 0 or 1".into());
            }
            values.insert(key.into(), value);
        }
        let (instruments, object_id, index) = if schema == Some(2) {
            let integer = |name: &str| {
                data.get(name)
                    .and_then(|v| v.as_u64())
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| format!("Invalid schema-2 {name}"))
            };
            (
                Some(Instruments::parse(&data["instruments"])?),
                Some(integer("object_id")?),
                Some(integer("sample_index")?),
            )
        } else {
            if data.get("instruments").is_some() {
                return Err("Instrument data requires reference schema 2".into());
            }
            (None, None, None)
        };
        Ok(Self {
            source: source.into(),
            title: title.into(),
            elapsed,
            values,
            instruments,
            object_id,
            index,
        })
    }
    pub fn load(path: &Path, index: usize) -> Result<Self, String> {
        if index >= 100_000 {
            return Err("Reference sample index exceeds limit".into());
        }
        let file = File::open(path).map_err(|e| e.to_string())?;
        if file.metadata().map_err(|e| e.to_string())?.len() > MAX_FILE {
            return Err("Reference file exceeds 64 MiB".into());
        }
        let mut reader = BufReader::new(file.take(MAX_FILE + 1));
        for row in 0..=index {
            let mut line = Vec::new();
            (&mut reader)
                .take(MAX_LINE + 1)
                .read_until(b'\n', &mut line)
                .map_err(|e| e.to_string())?;
            if line.len() as u64 > MAX_LINE {
                return Err("Reference row exceeds 16 KiB".into());
            }
            if line.is_empty() {
                return Err("Reference sample index is beyond end of file".into());
            }
            if row == index {
                return Self::parse(&line);
            }
        }
        unreachable!()
    }
    pub fn lookup(&self, variable: &str, units: &str) -> Option<f32> {
        let field = FIELDS
            .iter()
            .find(|(_, name, _)| name.eq_ignore_ascii_case(variable.trim()));
        let Some(&(key, _, native_unit)) = field else {
            return self.instruments.as_ref()?.lookup(variable, units);
        };
        let value = self.values.get(key)?;
        converted(*value, native_unit, units)
    }
}

/// Bounded sequential schema-2 reader. One aircraft/object/source per recording.
pub struct Recording {
    reader: BufReader<std::io::Take<File>>,
    rows: u32,
    bytes: u64,
    identity: Option<(String, String, u32)>,
    last_elapsed: f64,
}

/// Normalize the recorded absolute simulator clock, including paused frames and rewinds.
#[derive(Default)]
pub struct ReplayClock {
    last: Option<f64>,
    origin: f64,
}
impl ReplayClock {
    pub fn observe(&mut self, time: f64) -> Result<(f64, bool), String> {
        if !time.is_finite() || time < 0. {
            return Err("Invalid absolute reference clock".into());
        }
        let rewind = self.last.is_some_and(|previous| time < previous);
        let origin = if self.last.is_none() || rewind {
            time
        } else {
            self.origin
        };
        let relative = time - origin;
        if relative > 1e9 {
            return Err("Reference clock span exceeds the native profile".into());
        }
        self.last = Some(time);
        self.origin = origin;
        Ok((relative, rewind))
    }
}
impl Recording {
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| e.to_string())?;
        if file.metadata().map_err(|e| e.to_string())?.len() > MAX_FILE {
            return Err("Reference file exceeds 64 MiB".into());
        }
        Ok(Self {
            reader: BufReader::new(file.take(MAX_FILE + 1)),
            rows: 0,
            bytes: 0,
            identity: None,
            last_elapsed: 0.,
        })
    }
    pub fn next(&mut self) -> Result<Option<Sample>, String> {
        let mut line = Vec::new();
        (&mut self.reader)
            .take(MAX_LINE + 1)
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?;
        if line.is_empty() {
            return Ok(None);
        }
        self.bytes += line.len() as u64;
        if self.bytes > MAX_FILE {
            return Err("Reference stream exceeds 64 MiB".into());
        }
        if line.len() as u64 > MAX_LINE || self.rows >= 100_000 {
            return Err("Reference row/count budget exceeded".into());
        }
        let sample =
            Sample::parse(&line).map_err(|e| format!("Reference row {}: {e}", self.rows + 1))?;
        let object = sample
            .object_id
            .ok_or("Instrument replay requires schema 2")?;
        if sample.index != Some(self.rows) || sample.elapsed < self.last_elapsed {
            return Err("Recording sequence/wall time is not contiguous and monotonic".into());
        }
        let identity = (sample.source.clone(), sample.title.clone(), object);
        if self.identity.as_ref().is_some_and(|v| v != &identity) {
            return Err("Recording changes source, aircraft title or user object".into());
        }
        self.identity = Some(identity);
        self.last_elapsed = sample.elapsed;
        self.rows += 1;
        Ok(Some(sample))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> serde_json::Value {
        let values = FIELDS
            .iter()
            .map(|(key, _, _)| (key.to_string(), serde_json::json!(0)))
            .collect::<serde_json::Map<_, _>>();
        serde_json::json!({"schema":1,"source":"synthetic","title":"Original synthetic aircraft","elapsed_seconds":0.25,"values":values})
    }
    #[test]
    fn explicit_native_units_convert_without_assuming_control_sign_or_range() {
        let mut data = fixture();
        data["values"]["elevator_pct"] = serde_json::json!(-25.);
        data["values"]["elevator_deg"] = serde_json::json!(90.);
        let sample = Sample::parse(&serde_json::to_vec(&data).unwrap()).unwrap();
        assert_eq!(
            sample.lookup("elevator deflection pct", "percent"),
            Some(-25.)
        );
        assert_eq!(
            sample.lookup("ELEVATOR DEFLECTION PCT", "Percent Over 100"),
            Some(-0.25)
        );
        assert!(
            (sample.lookup("ELEVATOR DEFLECTION", "radians").unwrap()
                - std::f32::consts::FRAC_PI_2)
                .abs()
                < 1e-6
        );
        assert_eq!(sample.lookup("UNKNOWN", "percent"), None);
        assert_eq!(sample.lookup("ELEVATOR DEFLECTION PCT", "radians"), None);
    }
    fn instrument_fixture() -> serde_json::Value {
        let mut data = fixture();
        data["schema"] = serde_json::json!(2);
        data["object_id"] = serde_json::json!(7);
        data["sample_index"] = serde_json::json!(0);
        data["instruments"] = serde_json::Value::Object(
            INSTRUMENT_FIELDS
                .iter()
                .map(|(key, _, _)| (key.to_string(), serde_json::json!(0)))
                .collect(),
        );
        for (key, value) in [
            ("sim_time_seconds", 63700000000.),
            ("ias_knots", 90.),
            ("tas_knots", 92.),
            ("indicated_altitude_ft", 1000.),
            ("pressure_altitude_ft", 980.),
            ("altitude_msl_ft", 1020.),
            ("vertical_speed_fpm", 600.),
            ("gyro_pitch_deg", -10.),
            ("gyro_bank_deg", -30.),
            ("gyro_heading_deg", 90.),
        ] {
            data["instruments"][key] = serde_json::json!(value);
        }
        data
    }
    #[test]
    fn measured_indications_use_requested_units_without_isa_or_body_substitution() {
        let sample = Sample::parse(&serde_json::to_vec(&instrument_fixture()).unwrap()).unwrap();
        let instrument = sample.instruments.as_ref().unwrap();
        let inputs = instrument.native_inputs().unwrap();
        assert_eq!(
            [inputs[0], inputs[1], inputs[3], inputs[4], inputs[5]],
            [90., 1000., -10., -30., 90.]
        );
        assert!((inputs[2] - 780.288).abs() < 1e-10);
        assert_eq!(sample.object_id, Some(7));
        assert_eq!(instrument.time(), 63700000000.);
        assert!(
            (sample
                .lookup("VERTICAL SPEED", "meters per second")
                .unwrap()
                - 3.048)
                .abs()
                < 1e-6
        );
        assert!(
            (sample
                .lookup("ATTITUDE INDICATOR BANK DEGREES", "radians")
                .unwrap()
                + std::f32::consts::PI / 6.)
                .abs()
                < 1e-6
        );
        assert_eq!(
            sample.lookup("ELEVATOR DEFLECTION PCT", "percent"),
            Some(0.)
        );
    }
    #[test]
    fn schema_two_requires_every_field_and_preserves_unsupported_instrument_states() {
        let data = instrument_fixture();
        for &(key, _, _) in INSTRUMENT_FIELDS {
            let mut bad = data.clone();
            bad["instruments"].as_object_mut().unwrap().remove(key);
            assert!(Sample::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
        }
        for (key, value) in [
            ("failure_attitude", 2.),
            ("attitude_caged", 1.),
            ("attitude_bars_ratio", 0.2),
        ] {
            let mut state = data.clone();
            state["instruments"][key] = serde_json::json!(value);
            let sample = Sample::parse(&serde_json::to_vec(&state).unwrap()).unwrap();
            assert!(sample.instruments.unwrap().native_inputs().is_err());
        }
        for key in ["object_id", "sample_index"] {
            let mut bad = data.clone();
            bad[key] = serde_json::json!(u64::MAX);
            assert!(Sample::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
        }
        let mut bad = data.clone();
        bad["instruments"]["attitude_caged"] = serde_json::json!(2);
        assert!(Sample::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
        let mut bad = data.clone();
        bad["schema"] = serde_json::json!(1);
        assert!(Sample::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    #[test]
    fn reference_clock_distinguishes_pauses_and_rewinds_including_zero_elapsed_reset() {
        let mut clock = ReplayClock::default();
        let origin = 63700000000.;
        for (time, expected) in [
            (origin, (0., false)),
            (origin, (0., false)),
            (origin + 1., (1., false)),
            (origin - 10., (0., true)),
            (origin - 20., (0., true)),
        ] {
            assert_eq!(clock.observe(time).unwrap(), expected);
        }
        assert!(clock.observe(f64::NAN).is_err());
        assert!(clock.observe(-1.).is_err());
        assert_eq!(clock.observe(origin - 20.).unwrap(), (0., false));
    }
    #[test]
    fn sequential_recording_checks_identity_indices_and_wall_clock() {
        let path = std::env::temp_dir().join(format!(
            "openfsx-instrument-sequence-{}-{}.ndjson",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first = instrument_fixture();
        let mut second = first.clone();
        second["sample_index"] = serde_json::json!(1);
        second["elapsed_seconds"] = serde_json::json!(1.5);
        std::fs::write(&path, format!("{first}\n{second}\n")).unwrap();
        let mut recording = Recording::open(&path).unwrap();
        assert_eq!(recording.next().unwrap().unwrap().index, Some(0));
        assert_eq!(recording.next().unwrap().unwrap().index, Some(1));
        assert!(recording.next().unwrap().is_none());
        for (key, value) in [
            ("title", serde_json::json!("different aircraft")),
            ("source", serde_json::json!("FSX SimConnect")),
            ("object_id", serde_json::json!(8)),
            ("sample_index", serde_json::json!(3)),
            ("elapsed_seconds", serde_json::json!(0.1)),
        ] {
            let mut bad = second.clone();
            bad[key] = value;
            std::fs::write(&path, format!("{first}\n{bad}\n")).unwrap();
            let mut recording = Recording::open(&path).unwrap();
            recording.next().unwrap();
            assert!(recording.next().is_err());
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn malformed_schema_values_and_json_are_rejected() {
        let data = fixture();
        for (key, value) in [
            ("schema", serde_json::json!(2)),
            ("title", serde_json::json!("")),
            ("elapsed_seconds", serde_json::json!(-1)),
            ("source", serde_json::json!("unknown")),
        ] {
            let mut bad = data.clone();
            bad[key] = value;
            assert!(Sample::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
        }
        let mut bad = data.clone();
        bad["values"]["on_ground"] = serde_json::json!(2);
        assert!(Sample::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
        let bytes = serde_json::to_vec(&data).unwrap();
        for end in 0..bytes.len() {
            assert!(Sample::parse(&bytes[..end]).is_err());
        }
    }
    #[test]
    fn ndjson_selection_checks_index_and_line_limits() {
        let file = std::env::temp_dir().join(format!(
            "openfsx-reference-{}-{}.ndjson",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let a = serde_json::to_string(&fixture()).unwrap();
        let mut b = fixture();
        b["elapsed_seconds"] = serde_json::json!(1.5);
        std::fs::write(&file, format!("{a}\n{b}\n")).unwrap();
        assert_eq!(Sample::load(&file, 1).unwrap().elapsed, 1.5);
        assert!(Sample::load(&file, 2).is_err());
        assert!(Sample::load(&file, 100_000).is_err());
        std::fs::write(&file, vec![b' '; MAX_LINE as usize + 1]).unwrap();
        assert!(Sample::load(&file, 0).is_err());
        std::fs::remove_file(file).unwrap();
    }
}
