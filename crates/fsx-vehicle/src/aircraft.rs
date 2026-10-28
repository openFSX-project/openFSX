//! Load the initial single-engine, legacy-AIR aircraft subset into SI units.
use crate::{
    air::{Aerodynamics, AirFile},
    ground::Gear,
    mass::{body_position, vector, Loading},
};
use fsx_cfg::{decode, Config};
use std::{
    fs,
    path::{Path, PathBuf},
};
pub const FOOT: f64 = 0.3048;
pub const POUND: f64 = 0.45359237;
pub const KNOT: f64 = 1852. / 3600.;
pub const HORSEPOWER: f64 = 745.6998715822702;
pub const SLUG_FOOT_SQUARED: f64 = 1.3558179483314;

#[derive(Debug, Clone)]
pub struct Parameters {
    pub pitot_static: crate::pitot_static::PitotStatic,
    pub legacy_pitot_heat: crate::pitot_static::LegacyPitotHeat,
    pub electrical: crate::electrical::Electrical,
    pub loading: Loading,
    pub wing_area: f64,
    pub span: f64,
    pub chord: f64,
    pub efficiency: f64,
    pub power: f64,
    /// Rated engine speed, revolutions per minute ([piston_engine] max_rated_rpm).
    pub rated_rpm: f64,
    pub prop_area: f64,
    /// Steady wind in north/east/down axes, metres per second (direction the air moves toward).
    pub wind: [f64; 3],
    /// Instantaneous gust added to the steady wind, metres per second.
    pub gust: [f64; 3],
    pub tail_incidence: f64,
    pub controls: [f64; 4],
    pub elevator_up: f64,
    pub elevator_down: f64,
    pub aileron_limit: f64,
    pub rudder_limit: f64,
    pub trim_limit: f64,
    pub lift_scalar: f64,
    pub parasite_scalar: f64,
    pub induced_scalar: f64,
    pub damping: [f64; 3],
    pub cruise_speed: f64,
    pub reference_stall: f64,
    pub ground_height: f64,
    pub gear: Gear,
    pub engine_position: [f64; 3],
    pub aero: Aerodynamics,
    pub flaps: crate::flaps::Flaps,
}
#[derive(Debug)]
pub struct Aircraft {
    pub title: String,
    pub registration: Option<String>,
    pub model: PathBuf,
    pub interior: Option<PathBuf>,
    /// SI body forward/right/down coordinates, including reference datum.
    pub eye_point: Option<[f64; 3]>,
    pub texture: PathBuf,
    pub panel: PathBuf,
    pub air_path: PathBuf,
    pub parameters: Parameters,
}
/// Visual resources can be loaded without requiring a supported flight model.
#[derive(Debug)]
pub struct VisualAircraft {
    pub title: String,
    /// Authored atc_id for the selected variant; no title-derived fallback.
    pub registration: Option<String>,
    pub model: PathBuf,
    pub interior: Option<PathBuf>,
    pub eye_point: Option<[f64; 3]>,
    pub texture: PathBuf,
    /// Variant-selected panel configuration; existence is checked when loaded.
    pub panel: PathBuf,
}
fn component(value: &str) -> Result<&str, String> {
    if value.contains(['/', '\\', ':', '\0']) || value == "." || value == ".." {
        Err("Aircraft variant references must be file-name components".into())
    } else {
        Ok(value)
    }
}
fn read_config(path: &Path, windows_1252: bool) -> Result<Config, String> {
    if fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len()
        > 2 * 1024 * 1024
    {
        return Err("CFG exceeds 2 MiB limit".into());
    }
    Config::parse(&decode(
        &fs::read(path).map_err(|e| e.to_string())?,
        windows_1252,
    )?)
}
impl VisualAircraft {
    pub fn load(folder: &Path, variant: u32, windows_1252: bool) -> Result<Self, String> {
        let config = read_config(&folder.join("aircraft.cfg"), windows_1252)?;
        Self::from_config(folder, variant, windows_1252, &config)
    }
    fn from_config(
        folder: &Path,
        variant: u32,
        windows_1252: bool,
        config: &Config,
    ) -> Result<Self, String> {
        let section = format!("fltsim.{variant}");
        let title = config
            .get(&section, "title")
            .ok_or("Selected aircraft variant has no title")?
            .to_owned();
        let model = component(config.get(&section, "model").unwrap_or(""))?;
        let registration = config.get(&section, "atc_id").map(str::to_owned);
        if registration
            .as_ref()
            .is_some_and(|v| v.len() > 256 || v.chars().any(char::is_control))
        {
            return Err("Aircraft atc_id exceeds 256 bytes or contains control characters".into());
        }
        let texture = component(config.get(&section, "texture").unwrap_or(""))?;
        let panel = component(config.get(&section, "panel").unwrap_or(""))?;
        let model_dir = folder.join(if model.is_empty() {
            "model".to_owned()
        } else {
            format!("model.{model}")
        });
        let model_config = read_config(&model_dir.join("model.cfg"), windows_1252)?;
        let name = component(
            model_config
                .get("models", "normal")
                .ok_or("Missing [models] normal")?,
        )?;
        if name.is_empty() {
            return Err("Empty exterior MDL reference".into());
        }
        let model = model_dir.join(if name.to_ascii_lowercase().ends_with(".mdl") {
            name.to_owned()
        } else {
            format!("{name}.mdl")
        });
        let interior = model_config
            .get("models", "interior")
            .filter(|name| !name.is_empty())
            .map(|name| {
                let name = component(name)?;
                Ok::<_, String>(
                    model_dir.join(if name.to_ascii_lowercase().ends_with(".mdl") {
                        name.to_owned()
                    } else {
                        format!("{name}.mdl")
                    }),
                )
            })
            .transpose()?;
        let eye_point = if config.get("views", "eyepoint").is_some() {
            let datum = if config
                .get("weight_and_balance", "reference_datum_position")
                .is_some()
            {
                vector(config, "weight_and_balance", "reference_datum_position")?
            } else {
                [0.; 3]
            };
            let point = body_position(datum, vector(config, "views", "eyepoint")?);
            if point.iter().any(|v| !v.is_finite() || v.abs() > 1000.) {
                return Err(
                    "Cockpit eye point exceeds the supported 1 km body-coordinate range".into(),
                );
            }
            Some(point)
        } else {
            None
        };
        Ok(Self {
            title,
            registration,
            model,
            interior,
            eye_point,
            texture: folder.join(if texture.is_empty() {
                "texture".to_owned()
            } else {
                format!("texture.{texture}")
            }),
            panel: folder
                .join(if panel.is_empty() {
                    "panel".to_owned()
                } else {
                    format!("panel.{panel}")
                })
                .join("panel.cfg"),
        })
    }
}
impl Aircraft {
    pub fn load(folder: &Path, variant: u32, windows_1252: bool) -> Result<Self, String> {
        let config = read_config(&folder.join("aircraft.cfg"), windows_1252)?;
        let visual = VisualAircraft::from_config(folder, variant, windows_1252, &config)?;
        let section = format!("fltsim.{variant}");
        let sim = component(
            config
                .get(&section, "sim")
                .ok_or("Selected variant has no sim AIR reference")?,
        )?;
        if sim.is_empty() {
            return Err("Empty AIR reference".into());
        }
        let air_path = folder.join(format!("{sim}.air"));
        let air = AirFile::load(&air_path)?;
        Ok(Self {
            title: visual.title,
            registration: visual.registration,
            model: visual.model,
            interior: visual.interior,
            eye_point: visual.eye_point,
            texture: visual.texture,
            panel: visual.panel,
            air_path,
            parameters: {
                let mut parameters = Parameters::from_config(&config, air.dynamics()?)?;
                parameters.legacy_pitot_heat =
                    parameters.pitot_static.legacy_input_from_air(&air)?;
                parameters
            },
        })
    }
}
impl Parameters {
    pub fn from_config(c: &Config, aero: Aerodynamics) -> Result<Self, String> {
        if c.number("GeneralEngineData", "engine_type")? != 0.
            || (1..=3).any(|i| c.get("GeneralEngineData", &format!("Engine.{i}")).is_some())
        {
            return Err("Initial flight model supports a single piston engine only".into());
        }
        let required = |section: &str, key: &str, scale: f64| -> Result<f64, String> {
            let n = c.number(section, key)? * scale;
            if !n.is_finite() || n <= 0. {
                return Err(format!("[{section}] {key} must be finite and positive"));
            }
            Ok(n)
        };
        let loading = Loading::from_config(c)?;
        let gear = Gear::from_config(c, &loading)?;
        let wing_area = required("airplane_geometry", "wing_area", FOOT * FOOT)?;
        let span = required("airplane_geometry", "wing_span", FOOT)?;
        let diameter = required("propeller", "propeller_diameter", FOOT)?;
        let effectiveness = |key: &str| c.optional_number("flight_tuning", key, 1.);
        let radians = |key: &str| -> Result<f64, String> {
            Ok(required("airplane_geometry", key, 1.)?.to_radians())
        };
        let datum = vector(c, "weight_and_balance", "reference_datum_position")?;
        let engine_position = body_position(datum, vector(c, "GeneralEngineData", "Engine.0")?);
        let p = Self {
            loading,
            gear,
            wing_area,
            span,
            chord: wing_area / span,
            efficiency: required("airplane_geometry", "oswald_efficiency_factor", 1.)?,
            power: required("piston_engine", "max_rated_hp", HORSEPOWER)?
                * c.optional_number("piston_engine", "power_scalar", 1.)?,
            rated_rpm: c.optional_number("piston_engine", "max_rated_rpm", 2700.)?,
            prop_area: std::f64::consts::PI * diameter * diameter / 4.,
            wind: [0.; 3],
            gust: [0.; 3],
            tail_incidence: c
                .optional_number("airplane_geometry", "htail_incidence", 0.)?
                .to_radians(),
            controls: [
                effectiveness("elevator_effectiveness")?,
                effectiveness("aileron_effectiveness")?,
                effectiveness("rudder_effectiveness")?,
                effectiveness("elevator_trim_effectiveness")?,
            ],
            elevator_up: radians("elevator_up_limit")?,
            elevator_down: radians("elevator_down_limit")?,
            aileron_limit: radians("aileron_up_limit")?,
            rudder_limit: radians("rudder_limit")?,
            trim_limit: radians("elevator_trim_limit")?,
            lift_scalar: effectiveness("cruise_lift_scalar")?,
            parasite_scalar: effectiveness("parasite_drag_scalar")?,
            induced_scalar: effectiveness("induced_drag_scalar")?,
            damping: [
                effectiveness("roll_stability")?,
                effectiveness("pitch_stability")?,
                effectiveness("yaw_stability")?,
            ],
            cruise_speed: required("reference speeds", "cruise_speed", KNOT)?,
            reference_stall: required("reference speeds", "flaps_up_stall_speed", KNOT)?,
            ground_height: required("contact_points", "static_cg_height", FOOT)?,
            engine_position,
            electrical: crate::electrical::Electrical::from_config(c)?,
            pitot_static: crate::pitot_static::PitotStatic::from_config(c)?,
            legacy_pitot_heat: crate::pitot_static::LegacyPitotHeat::Unknown,
            flaps: crate::flaps::Flaps::from_config(
                c,
                aero.lift.peak()?.1 * effectiveness("cruise_lift_scalar")?,
            )?,
            aero,
        };
        if p.power <= 0.
            || p.efficiency > 1.
            || p.controls.iter().any(|n| *n < 0.)
            || p.damping.iter().any(|n| *n < 0.)
            || p.lift_scalar <= 0.
            || p.parasite_scalar < 0.
            || p.induced_scalar < 0.
        {
            return Err("Unsupported negative tuning or invalid power/efficiency".into());
        }
        Ok(p)
    }
}
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub fn own_parameters() -> Parameters {
    Parameters {
        loading: crate::mass::Loading {
            mass: 1000.,
            empty_mass: 1000.,
            payload_mass: 0.,
            fuel_mass: 0.,
            max_mass: 1200.,
            cg: [0.; 3],
            empty_cg: [0.; 3],
            inertia: crate::mass::Inertia::diagonal([1600., 2000., 3000.]).unwrap(),
        },
        wing_area: 16.,
        span: 10.,
        chord: 1.6,
        efficiency: 0.8,
        power: 130000.,
        rated_rpm: 2700.,
        prop_area: 3.,
        wind: [0.; 3],
        gust: [0.; 3],
        tail_incidence: 0.,
        controls: [1.; 4],
        elevator_up: 0.4,
        elevator_down: 0.3,
        aileron_limit: 0.3,
        rudder_limit: 0.4,
        trim_limit: 0.35,
        lift_scalar: 1.,
        parasite_scalar: 1.,
        induced_scalar: 1.,
        damping: [1.; 3],
        cruise_speed: 55.,
        reference_stall: 27.,
        ground_height: 1.,
        gear: crate::ground::Gear {
            static_pitch: 0.,
            omitted_contacts: 0,
            ground_level: 0.,
            wheels: vec![
                crate::ground::Wheel {
                    position: [2., 0., 1.1],
                    compression: 0.1,
                    travel: 0.3,
                    stiffness: 32688.8333333333,
                    damping: 3000.,
                    steering: 0.4,
                    braked: false,
                },
                crate::ground::Wheel {
                    position: [-1., -1.5, 1.1],
                    compression: 0.1,
                    travel: 0.3,
                    stiffness: 32688.8333333333,
                    damping: 2000.,
                    steering: 0.,
                    braked: true,
                },
                crate::ground::Wheel {
                    position: [-1., 1.5, 1.1],
                    compression: 0.1,
                    travel: 0.3,
                    stiffness: 32688.8333333333,
                    damping: 2000.,
                    steering: 0.,
                    braked: true,
                },
            ],
        },
        engine_position: [0.; 3],
        electrical: crate::electrical::Electrical::default(),
        pitot_static: crate::pitot_static::PitotStatic::default(),
        legacy_pitot_heat: crate::pitot_static::LegacyPitotHeat::Unknown,
        aero: AirFile::parse(&crate::air::own_air())
            .unwrap()
            .dynamics()
            .unwrap(),
        flaps: crate::flaps::Flaps::default(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn single_piston_profile_rejects_every_additional_engine_slot_even_with_gaps() {
        for i in 1..=3 {
            let c = Config::parse(&format!(
                "[GeneralEngineData]\nengine_type=0\nengine.0=0,0,0\nengine.{i}=1,0,0"
            ))
            .unwrap();
            let error = Parameters::from_config(&c, own_parameters().aero).unwrap_err();
            assert_eq!(
                error,
                "Initial flight model supports a single piston engine only"
            );
        }
    }
    #[test]
    fn units_and_variant_component_limits() {
        assert!((100. * KNOT - 51.4444444444).abs() < 1e-9);
        assert!((FOOT * FOOT - 0.09290304).abs() < 1e-12);
        assert!(component("../model").is_err());
        assert!(component("C:\\x").is_err());
        assert_eq!(component("g1000").unwrap(), "g1000");
    }
    #[test]
    fn visual_resources_do_not_require_an_air_file_or_piston_engine() {
        let folder = std::env::temp_dir().join(format!(
            "openfsx-visual-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(folder.join("model.jet")).unwrap();
        fs::write(folder.join("aircraft.cfg"), b"[fltsim.3]\ntitle=Original synthetic jet\nmodel=jet\ntexture=blue\npanel=ifr\n[GeneralEngineData]\nengine_type=1\n").unwrap();
        fs::write(
            folder.join("model.jet/model.cfg"),
            b"[models]\nnormal=synthetic\n",
        )
        .unwrap();
        let visual = VisualAircraft::load(&folder, 3, false).unwrap();
        assert_eq!(visual.title, "Original synthetic jet");
        assert_eq!(visual.model, folder.join("model.jet/synthetic.mdl"));
        assert_eq!(visual.texture, folder.join("texture.blue"));
        assert_eq!(visual.panel, folder.join("panel.ifr/panel.cfg"));
        assert_eq!(visual.registration, None);
        for registration in ["SYN-42", "", "ТЕСТ-7"] {
            let cfg = Config::parse(&format!("[fltsim.0]\natc_id=WRONG\n[fltsim.3]\ntitle=Own\nmodel=jet\natc_id={registration}\n")).unwrap();
            let visual = VisualAircraft::from_config(&folder, 3, false, &cfg).unwrap();
            assert_eq!(visual.registration.as_deref(), Some(registration));
        }
        for registration in ["x".repeat(257), "x\0y".into()] {
            let cfg = Config::parse(&format!(
                "[fltsim.3]\ntitle=Own\nmodel=jet\natc_id={registration}\n"
            ))
            .unwrap();
            assert!(VisualAircraft::from_config(&folder, 3, false, &cfg).is_err());
        }
        for value in ["../outside", "C:\\outside", "/outside", ".."] {
            let cfg = Config::parse(&format!("[fltsim.3]\ntitle=Own\npanel={value}\n")).unwrap();
            assert!(VisualAircraft::from_config(&folder, 3, false, &cfg).is_err());
        }
        assert!(visual.interior.is_none() && visual.eye_point.is_none());
        assert!(Aircraft::load(&folder, 3, false).is_err());
        fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn cockpit_references_and_eye_include_variant_model_folder_and_datum_units() {
        let folder = std::env::temp_dir().join(format!(
            "openfsx-cockpit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(folder.join("model.dual")).unwrap();
        let cfg="[fltsim.2]\ntitle=Original synthetic cockpit\nmodel=dual\n[weight_and_balance]\nreference_datum_position=2,3,4\n[views]\neyepoint=-1,2,3\n";
        fs::write(folder.join("aircraft.cfg"), cfg).unwrap();
        fs::write(
            folder.join("model.dual/model.cfg"),
            "[models]\nnormal=outside.MDL\ninterior=inside\n",
        )
        .unwrap();
        let visual = VisualAircraft::load(&folder, 2, false).unwrap();
        assert_eq!(visual.interior, Some(folder.join("model.dual/inside.mdl")));
        assert_eq!(visual.panel, folder.join("panel/panel.cfg"));
        assert_eq!(visual.eye_point, Some([FOOT, 5. * FOOT, -7. * FOOT]));
        for invalid in ["NaN,0,1", "1,2,3,4", "1e30,0,0"] {
            fs::write(
                folder.join("aircraft.cfg"),
                cfg.replace("eyepoint=-1,2,3", &format!("eyepoint={invalid}")),
            )
            .unwrap();
            assert!(VisualAircraft::load(&folder, 2, false).is_err());
        }
        fs::write(folder.join("aircraft.cfg"), cfg).unwrap();
        fs::write(
            folder.join("model.dual/model.cfg"),
            "[models]\nnormal=outside\ninterior=../outside\n",
        )
        .unwrap();
        assert!(VisualAircraft::load(&folder, 2, false).is_err());
        let resolved = folder.canonicalize().unwrap();
        assert!(resolved.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        fs::remove_dir_all(resolved).unwrap();
    }
}
