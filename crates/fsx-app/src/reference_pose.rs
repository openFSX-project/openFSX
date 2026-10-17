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
#[derive(Debug)]
pub struct Sample {
    pub source: String,
    pub title: String,
    pub elapsed: f64,
    values: BTreeMap<String, f64>,
}
impl Sample {
    fn parse(bytes: &[u8]) -> Result<Self, String> {
        let data: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| format!("Invalid reference JSON: {e}"))?;
        if data.get("schema").and_then(|v| v.as_u64()) != Some(1) {
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
        Ok(Self {
            source: source.into(),
            title: title.into(),
            elapsed,
            values,
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
        let &(key, _, native_unit) = FIELDS
            .iter()
            .find(|(_, name, _)| name.eq_ignore_ascii_case(variable.trim()))?;
        let value = self.values.get(key)?;
        let unit = units.trim().to_ascii_lowercase();
        let value = match (native_unit, unit.as_str()) {
            ("percent", "percent") | ("bool", "bool") => *value,
            ("percent", "percent over 100") => *value / 100.,
            ("degrees", "degrees" | "degree") => *value,
            ("degrees", "radians" | "radian") => value.to_radians(),
            _ => return None,
        };
        (value.is_finite() && value.abs() <= f32::MAX as f64).then_some(value as f32)
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
