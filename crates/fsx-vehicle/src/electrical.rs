//! Authored legacy [electrical] data. Absent parameters/defaults stay unresolved.
use fsx_cfg::Config;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bus {
    Main,
    Avionics,
    Battery,
    HotBattery,
    Generator(u8),
}
impl Bus {
    pub fn from_index(index: u8) -> Result<Self, String> {
        Ok(match index {
            0 => Self::Main,
            1 => Self::Avionics,
            2 => Self::Battery,
            3 => Self::HotBattery,
            4..=7 => Self::Generator(index - 3),
            _ => return Err("Legacy electrical bus index must be 0..7".into()),
        })
    }
    pub fn index(self) -> Result<usize, String> {
        Ok(match self {
            Self::Main => 0,
            Self::Avionics => 1,
            Self::Battery => 2,
            Self::HotBattery => 3,
            Self::Generator(index) if (1..=4).contains(&index) => usize::from(index) + 3,
            _ => return Err("Generator bus number must be 1..4".into()),
        })
    }
}
#[derive(Debug, Clone)]
pub struct Circuit {
    pub bus: Bus,
    pub max_amps: f64,
    pub min_voltage: f64,
}
#[derive(Debug, Clone, Default)]
pub struct Electrical {
    pub circuits: BTreeMap<String, Circuit>,
    pub max_battery_voltage: Option<f64>,
    pub battery_max_capacity_amph: Option<f64>,
    pub battery_charging_current: Option<f64>,
    pub generator_voltage: Option<f64>,
    pub max_generator_amps: Option<f64>,
    pub always_available: Option<bool>,
    pub engine_generator_map: Option<Vec<u8>>,
    /// Preserved keys are not silently interpreted as circuits or scalar defaults.
    pub unsupported: BTreeMap<String, String>,
}
/// Version-specific source initialization, separate from authored CFG data.
/// The reference executes installed sim1 scalar initialization with guarded
/// emulation (scripts/check-electrical-defaults.py); no live supply is implied.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SourceDefaults {
    #[default]
    Unresolved,
    FsxSteam62615SinglePiston,
}
const CIRCUITS: &[&str] = &[
    "navcom1",
    "navcom2",
    "navcom3",
    "flap_motor",
    "gear_motor",
    "autopilot",
    "avionics_bus",
    "avionics",
    "pitot_heat",
    "additional_system",
    "marker_beacon",
    "gear_warning",
    "fuel_pump",
    "starter1",
    "starter2",
    "starter3",
    "starter4",
    "light_nav",
    "light_beacon",
    "light_landing",
    "light_taxi",
    "light_strobe",
    "light_panel",
    "light_cabin",
    "prop_sync",
    "auto_feather",
    "auto_brakes",
    "standby_vacuum",
    "hydraulic_pump",
    "fuel_transfer_pump",
    "propeller_deice",
    "light_recognition",
    "light_wing",
    "light_logo",
    "directional_gyro",
    "directional_gyro_slaving",
];
/// Pinned sim1 10.0.62615 CFG triple destinations, in slots 1..=33.
/// Slot 0 is separate/ignored by load summation. Omitted defaults are unresolved.
pub const LEGACY_CONSUMER_KEYS: [&str; 33] = [
    "additional_system",
    "flap_motor",
    "gear_motor",
    "autopilot",
    "avionics_bus",
    "avionics",
    "pitot_heat",
    "prop_sync",
    "auto_feather",
    "auto_brakes",
    "standby_vacuum",
    "marker_beacon",
    "gear_warning",
    "hydraulic_pump",
    "fuel_pump",
    "fuel_transfer_pump",
    "propeller_deice",
    "starter1",
    "starter2",
    "starter3",
    "starter4",
    "light_nav",
    "light_beacon",
    "light_landing",
    "light_taxi",
    "light_strobe",
    "light_panel",
    "light_recognition",
    "light_wing",
    "light_logo",
    "light_cabin",
    "directional_gyro",
    "directional_gyro_slaving",
];
fn number(value: &str, maximum: f64) -> Result<f64, String> {
    let number = value
        .trim()
        .parse::<f64>()
        .map_err(|_| "Invalid electrical number")?;
    if !number.is_finite() || number < 0. || number > maximum {
        return Err("Electrical number outside finite nonnegative bounds".into());
    }
    Ok(number)
}
impl Electrical {
    /// Apply only source defaults verified for the specified engine/version
    /// profile. Does not expand omitted circuit defaults or battery capacity.
    /// The caller must establish single-piston engine eligibility separately.
    pub fn with_source_defaults(&self, profile: SourceDefaults) -> Self {
        let mut effective = self.clone();
        if profile == SourceDefaults::FsxSteam62615SinglePiston {
            effective.max_battery_voltage.get_or_insert(24.);
            effective.generator_voltage.get_or_insert(28.);
            effective.max_generator_amps.get_or_insert(60.);
        }
        effective
    }
    pub fn from_config(config: &Config) -> Result<Self, String> {
        let mut result = Self::default();
        let Some(section) = config.section("electrical") else {
            return Ok(result);
        };
        if section.len() > 256 {
            return Err("Electrical section exceeds 256 entries".into());
        }
        for (key, value) in section {
            if key.len() > 128 || value.len() > 4096 {
                return Err("Electrical key/value budget exceeded".into());
            }
            let parsed = (|| -> Result<(), String> {
                if CIRCUITS.contains(&key.as_str()) {
                    let fields: Vec<_> = value.split(',').map(str::trim).collect();
                    if fields.len() != 3 {
                        return Err(
                            "Electrical circuit requires bus, amps and minimum voltage".into()
                        );
                    }
                    let bus = Bus::from_index(
                        fields[0]
                            .parse::<u8>()
                            .map_err(|_| "Invalid electrical bus integer")?,
                    )?;
                    result.circuits.insert(
                        key.clone(),
                        Circuit {
                            bus,
                            max_amps: number(fields[1], 1e6)?,
                            min_voltage: number(fields[2], 1000.)?,
                        },
                    );
                } else {
                    match key.as_str() {
                        "max_battery_voltage" => {
                            let value = number(value, 1000.)?;
                            result.max_battery_voltage = Some(value);
                        }
                        "battery_max_capacity_amph" => {
                            result.battery_max_capacity_amph = Some(number(value, 1e6)?);
                        }
                        "battery_charging_current" => {
                            result.battery_charging_current = Some(number(value, 1e6)?);
                        }
                        "generator_alternator_voltage" => {
                            let value = number(value, 1000.)?;
                            result.generator_voltage = Some(value);
                        }
                        "max_generator_alternator_amps" => {
                            result.max_generator_amps = Some(number(value, 1e6)?)
                        }
                        "electric_always_available" => {
                            result.always_available = Some(match value.trim() {
                                "0" => false,
                                "1" => true,
                                _ => {
                                    return Err("Electrical availability flag must be 0 or 1".into())
                                }
                            })
                        }
                        "engine_generator_map" => {
                            let values = value
                                .split(',')
                                .map(|v| {
                                    v.trim()
                                        .parse::<u8>()
                                        .map_err(|_| "Invalid generator map integer")
                                })
                                .collect::<Result<Vec<_>, _>>()?;
                            if values.is_empty()
                                || values.len() > 4
                                || values.iter().any(|&v| v > 4)
                            {
                                return Err(
                                    "Generator map requires 1..4 engine entries, each 0..4".into(),
                                );
                            }
                            result.engine_generator_map = Some(values);
                        }
                        _ => {
                            result.unsupported.insert(key.clone(), value.clone());
                        }
                    }
                }
                Ok(())
            })();
            parsed.map_err(|error| format!("[electrical] {key}: {error}"))?;
        }
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verified_source_profile_resolves_only_missing_ratings_without_altering_authored_data() {
        let authored = Electrical::default();
        let effective = authored.with_source_defaults(SourceDefaults::FsxSteam62615SinglePiston);
        assert_eq!(authored.max_battery_voltage, None);
        assert_eq!(effective.max_battery_voltage, Some(24.));
        assert_eq!(effective.generator_voltage, Some(28.));
        assert_eq!(effective.max_generator_amps, Some(60.));
        assert_eq!(effective.battery_max_capacity_amph, None);
        assert_eq!(effective.battery_charging_current, None);
        assert!(effective.circuits.is_empty());
        assert_eq!(
            authored
                .with_source_defaults(SourceDefaults::Unresolved)
                .generator_voltage,
            None
        );
        for (battery, generator, amps) in [(0., 0., 0.), (12., 13.8, 45.)] {
            let authored = Electrical {
                max_battery_voltage: Some(battery),
                generator_voltage: Some(generator),
                max_generator_amps: Some(amps),
                ..Electrical::default()
            };
            let e = authored.with_source_defaults(SourceDefaults::FsxSteam62615SinglePiston);
            assert_eq!(
                (
                    e.max_battery_voltage,
                    e.generator_voltage,
                    e.max_generator_amps
                ),
                (Some(battery), Some(generator), Some(amps))
            );
        }
    }
    #[test]
    fn electrical_parser_retains_authored_values_unknown_keys_and_missing_source_data() {
        let config=Config::parse("[electrical]\nPITOT_HEAT=0,2,17\navionics=1,5,17\ncustom_wire=original\nmax_battery_voltage=24\nelectric_always_available=0\nengine_generator_map=1,0,2").unwrap();
        let e = Electrical::from_config(&config).unwrap();
        assert_eq!(e.circuits.len(), 2);
        assert_eq!(e.circuits["pitot_heat"].max_amps, 2.);
        assert_eq!(e.circuits["avionics"].bus, Bus::Avionics);
        assert_eq!(e.max_battery_voltage, Some(24.));
        assert_eq!(e.generator_voltage, None);
        assert_eq!(e.max_generator_amps, None);
        assert_eq!(e.battery_max_capacity_amph, None);
        assert_eq!(e.battery_charging_current, None);
        assert_eq!(e.engine_generator_map, Some(vec![1, 0, 2]));
        assert_eq!(e.unsupported["custom_wire"], "original");
        assert!(Electrical::from_config(&Config::default())
            .unwrap()
            .circuits
            .is_empty());
        let zero = Electrical::from_config(
            &Config::parse("[electrical]\nmax_battery_voltage=0\ngenerator_alternator_voltage=0")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(zero.max_battery_voltage, Some(0.));
        assert_eq!(zero.generator_voltage, Some(0.));
        let battery = Electrical::from_config(&Config::parse("[electrical]\nbattery_max_capacity_amph=40\nbattery_charging_current=4\nbattery_capacity_pct_to_voltage=unresolved").unwrap()).unwrap();
        assert_eq!(battery.battery_max_capacity_amph, Some(40.));
        assert_eq!(battery.battery_charging_current, Some(4.));
        assert_eq!(
            battery.unsupported["battery_capacity_pct_to_voltage"],
            "unresolved"
        );
    }
    #[test]
    fn electrical_parser_rejects_invalid_circuits_and_sources_instead_of_defaulting() {
        for line in [
            "pitot_heat=8,2,17",
            "pitot_heat=0.5,2,17",
            "pitot_heat=0,-2,17",
            "pitot_heat=0,NaN,17",
            "pitot_heat=0,2,-1",
            "pitot_heat=0,2",
            "pitot_heat=0,2,17,4",
            "max_battery_voltage=-1",
            "generator_alternator_voltage=inf",
            "max_generator_alternator_amps=-1",
            "battery_max_capacity_amph=NaN",
            "battery_max_capacity_amph=-1",
            "battery_charging_current=1000001",
            "electric_always_available=2",
            "engine_generator_map=1,1,1,1,1",
            "engine_generator_map=5",
        ] {
            assert!(
                Electrical::from_config(&Config::parse(&format!("[electrical]\n{line}")).unwrap())
                    .is_err(),
                "{line}"
            );
        }
        for index in 0..8 {
            assert_eq!(
                Bus::from_index(index).unwrap().index().unwrap(),
                usize::from(index)
            );
        }
        assert!(Bus::Generator(0).index().is_err());
        assert!(Bus::Generator(5).index().is_err());
    }
}
