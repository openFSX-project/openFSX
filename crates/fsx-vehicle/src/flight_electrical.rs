//! Authored switches and saved engine RPM fractions for aircraft 0 in legacy FLT.
//! Switches are not proof of generation, equipment presence, bus voltage or charge.
use fsx_cfg::{decode, Config};
use std::{collections::BTreeMap, fs, path::Path};
const SWITCHES: &[(&str, &str)] = &[
    ("pitotheat", "pitot_heat"),
    ("beaconlights", "light_beacon"),
    ("landinglights", "light_landing"),
    ("logolights", "light_logo"),
    ("navlights", "light_nav"),
    ("panellights", "light_panel"),
    ("recognitionlights", "light_recognition"),
    ("strobelights", "light_strobe"),
    ("taxilights", "light_taxi"),
    ("winglights", "light_wing"),
    ("cabinlights", "light_cabin"),
];
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FlightElectrical {
    pub battery: Option<bool>,
    pub avionics: Option<bool>,
    /// Engine sections 1..4, aircraft 0. These are switch positions, not online.
    pub generators: [Option<bool>; 4],
    /// Saved Pct Engine RPM, a fraction (1 = rated RPM) in the checked piston
    /// profile. This snapshot is not evolving/live engine state. Overspeed and
    /// signed rotation are retained; finite values are not clamped to 0..1.
    pub engine_rpm_fraction: [Option<f64>; 4],
    requests: BTreeMap<String, bool>,
}
fn boolean(config: &Config, section: &str, key: &str) -> Result<Option<bool>, String> {
    config
        .get(section, key)
        .map(|v| {
            if v.eq_ignore_ascii_case("true") {
                Ok(true)
            } else if v.eq_ignore_ascii_case("false") {
                Ok(false)
            } else {
                Err(format!("[{section}] {key} must be True or False"))
            }
        })
        .transpose()
}
fn consistent(a: Option<bool>, b: Option<bool>, name: &str) -> Result<Option<bool>, String> {
    if a.is_some() && b.is_some() && a != b {
        return Err(format!(
            "Conflicting legacy FLT {name} locations; precedence unresolved"
        ));
    }
    Ok(a.or(b))
}
impl FlightElectrical {
    pub fn load(path: &Path, windows_1252: bool) -> Result<Self, String> {
        if fs::metadata(path).map_err(|e| e.to_string())?.len() > 2 * 1024 * 1024 {
            return Err("FLT exceeds 2 MiB switch-reader budget".into());
        }
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err("FLT exceeds switch-reader budget".into());
        }
        Self::from_config(&Config::parse(&decode(&bytes, windows_1252)?)?)
    }
    pub fn from_config(config: &Config) -> Result<Self, String> {
        let sections = [
            "systems.0",
            "avionics.0",
            "switches.0",
            "engine parameters.1.0",
            "engine parameters.2.0",
            "engine parameters.3.0",
            "engine parameters.4.0",
        ];
        for section in sections {
            if config.section(section).is_some_and(|s| {
                s.len() > 256 || s.iter().any(|(k, v)| k.len() > 128 || v.len() > 4096)
            }) {
                return Err("FLT electrical section exceeds reader budget".into());
            }
        }
        let mut result = Self {
            battery: boolean(config, "systems.0", "batteryswitch")?,
            avionics: consistent(
                boolean(config, "systems.0", "avionicsswitch")?,
                boolean(config, "avionics.0", "avionicsswitch")?,
                "AvionicsSwitch",
            )?,
            ..Self::default()
        };
        for (i, value) in result.generators.iter_mut().enumerate() {
            let section = format!("engine parameters.{}.0", i + 1);
            *value = boolean(config, &section, "generatorswitch")?;
            result.engine_rpm_fraction[i] = config
                .get(&section, "pct engine rpm")
                .map(|_| config.number(&section, "pct engine rpm"))
                .transpose()?;
        }
        for &(key, circuit) in SWITCHES {
            let mut value = boolean(config, "switches.0", key)?;
            if key == "panellights" {
                value = consistent(value, boolean(config, "systems.0", key)?, "PanelLights")?;
            }
            if let Some(value) = value {
                result.requests.insert(circuit.into(), value);
            }
        }
        Ok(result)
    }
    pub fn requests(&self) -> &BTreeMap<String, bool> {
        &self.requests
    }
    pub fn mapped_circuits() -> impl Iterator<Item = &'static str> {
        SWITCHES.iter().map(|(_, circuit)| *circuit)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_positions_and_both_avionics_locations_are_read_without_defaults() {
        let c=Config::parse("[Systems.0]\nBatterySwitch=True\nAvionicsSwitch=False\n[Switches.0]\nPitotHeat=True\nNavLights=False\n[Engine Parameters.1.0]\nGeneratorSwitch=True\n[Engine Parameters.2.0]\nGeneratorSwitch=False\n[Systems.1]\nBatterySwitch=False").unwrap();
        let s = FlightElectrical::from_config(&c).unwrap();
        assert_eq!(s.battery, Some(true));
        assert_eq!(s.avionics, Some(false));
        assert_eq!(s.generators, [Some(true), Some(false), None, None]);
        assert_eq!(s.requests()["pitot_heat"], true);
        assert_eq!(s.requests()["light_nav"], false);
        assert!(!s.requests().contains_key("light_taxi"));
        assert_eq!(
            FlightElectrical::from_config(&Config::default()).unwrap(),
            FlightElectrical::default()
        );
        let c=Config::parse("[Avionics.0]\nAvionicsSwitch=tRuE\n[Systems.0]\nPanelLights=False\n[Switches.0]\nPanelLights=False").unwrap();
        let s = FlightElectrical::from_config(&c).unwrap();
        assert_eq!(s.avionics, Some(true));
        assert_eq!(s.requests()["light_panel"], false);
    }
    #[test]
    fn invalid_booleans_conflicts_and_oversized_sections_fail() {
        for text in [
            "[Systems.0]\nBatterySwitch=1",
            "[Switches.0]\nNavLights=yes",
            "[Engine Parameters.1.0]\nGeneratorSwitch=on",
            "[Systems.0]\nAvionicsSwitch=True\n[Avionics.0]\nAvionicsSwitch=False",
            "[Systems.0]\nPanelLights=True\n[Switches.0]\nPanelLights=False",
        ] {
            assert!(
                FlightElectrical::from_config(&Config::parse(text).unwrap()).is_err(),
                "{text}"
            );
        }
        let text = format!(
            "[Systems.0]\n{}",
            (0..257)
                .map(|i| format!("unknown{i}=False\n"))
                .collect::<String>()
        );
        assert!(FlightElectrical::from_config(&Config::parse(&text).unwrap()).is_err());
    }
    #[test]
    fn saved_rpm_fraction_is_optional_finite_signed_and_not_clamped_at_rated_speed() {
        let c=Config::parse("[Engine Parameters.1.0]\nPct Engine RPM=0.887\n[Engine Parameters.2.0]\nPct Engine RPM=1.25\n[Engine Parameters.3.0]\nPct Engine RPM=-0.5").unwrap();
        let s = FlightElectrical::from_config(&c).unwrap();
        assert_eq!(
            s.engine_rpm_fraction,
            [Some(0.887), Some(1.25), Some(-0.5), None]
        );
        for value in ["NaN", "inf", "bad"] {
            let c =
                Config::parse(&format!("[Engine Parameters.1.0]\nPct Engine RPM={value}")).unwrap();
            assert!(FlightElectrical::from_config(&c).is_err());
        }
    }
}
