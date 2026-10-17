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
    pub loading: Loading,
    pub wing_area: f64,
    pub span: f64,
    pub chord: f64,
    pub efficiency: f64,
    pub power: f64,
    pub prop_area: f64,
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
}
#[derive(Debug)]
pub struct Aircraft {
    pub title: String,
    pub model: PathBuf,
    pub texture: PathBuf,
    pub air_path: PathBuf,
    pub parameters: Parameters,
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
impl Aircraft {
    pub fn load(folder: &Path, variant: u32, windows_1252: bool) -> Result<Self, String> {
        let config = read_config(&folder.join("aircraft.cfg"), windows_1252)?;
        let section = format!("fltsim.{variant}");
        let title = config
            .get(&section, "title")
            .ok_or("Selected aircraft variant has no title")?
            .to_owned();
        let sim = component(
            config
                .get(&section, "sim")
                .ok_or("Selected variant has no sim AIR reference")?,
        )?;
        if sim.is_empty() {
            return Err("Empty AIR reference".into());
        }
        let model = component(config.get(&section, "model").unwrap_or(""))?;
        let texture = component(config.get(&section, "texture").unwrap_or(""))?;
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
        let air_path = folder.join(format!("{sim}.air"));
        let air = AirFile::load(&air_path)?;
        let parameters = Parameters::from_config(&config, air.dynamics()?)?;
        Ok(Self {
            title,
            model,
            texture: folder.join(if texture.is_empty() {
                "texture".to_owned()
            } else {
                format!("texture.{texture}")
            }),
            air_path,
            parameters,
        })
    }
}
impl Parameters {
    pub fn from_config(c: &Config, aero: Aerodynamics) -> Result<Self, String> {
        if c.number("GeneralEngineData", "engine_type")? != 0.
            || c.get("GeneralEngineData", "Engine.1").is_some()
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
            prop_area: std::f64::consts::PI * diameter * diameter / 4.,
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
        prop_area: 3.,
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
        aero: AirFile::parse(&crate::air::own_air())
            .unwrap()
            .dynamics()
            .unwrap(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn units_and_variant_component_limits() {
        assert!((100. * KNOT - 51.4444444444).abs() < 1e-9);
        assert!((FOOT * FOOT - 0.09290304).abs() < 1e-12);
        assert!(component("../model").is_err());
        assert!(component("C:\\x").is_err());
        assert_eq!(component("g1000").unwrap(), "g1000");
    }
}
