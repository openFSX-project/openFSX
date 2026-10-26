//! Aircraft sound.cfg: layered engine loops and named event sounds.
//! Engine layers map normalized RPM to pitch (rparams, two points) and to
//! volume (vparams, up to eight points, 0..100). viewpoint 1 is the cockpit,
//! 2 the exterior; layers without one play in both.
use fsx_cfg::Config;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Viewpoint {
    Both,
    Inside,
    Outside,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Combustion,
    Propeller,
    NonCombustion,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub file: PathBuf,
    pub source: Source,
    pub viewpoint: Viewpoint,
    pub pitch: [(f64, f64); 2],
    pub volume: Vec<(f64, f64)>,
}
impl Layer {
    /// Linear pitch between the two rparams points, held constant outside.
    pub fn pitch_at(&self, rpm: f64) -> f64 {
        let [(x0, p0), (x1, p1)] = self.pitch;
        if x1 <= x0 {
            return p0;
        }
        p0 + (p1 - p0) * ((rpm - x0) / (x1 - x0)).clamp(0., 1.)
    }
    /// Piecewise-linear volume in 0..1.
    pub fn volume_at(&self, rpm: f64) -> f64 {
        let v = &self.volume;
        let Some(first) = v.first() else { return 0. };
        if rpm <= first.0 {
            return first.1 / 100.;
        }
        for w in v.windows(2) {
            let ((x0, y0), (x1, y1)) = (w[0], w[1]);
            if rpm <= x1 {
                let t = if x1 > x0 { (rpm - x0) / (x1 - x0) } else { 1. };
                return (y0 + (y1 - y0) * t) / 100.;
            }
        }
        v.last().map_or(0., |l| l.1 / 100.)
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sounds {
    pub engine: Vec<Layer>,
    pub stall_warning: Option<PathBuf>,
    pub touchdown: Vec<PathBuf>,
    pub ground_roll: Option<PathBuf>,
    pub flaps: Option<PathBuf>,
    pub wind: Option<PathBuf>,
}
fn numbers(text: &str) -> Result<Vec<f64>, String> {
    text.split(',')
        .map(|s| {
            s.trim()
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .ok_or_else(|| format!("Invalid sound parameter {s:?}"))
        })
        .collect()
}
impl Sounds {
    pub fn load(sound_folder: &Path, windows_1252: bool) -> Result<Self, String> {
        let bytes = std::fs::read(sound_folder.join("sound.cfg")).map_err(|e| e.to_string())?;
        let text = fsx_cfg::decode(&bytes, windows_1252)?;
        Self::from_config(&Config::parse(&text)?, sound_folder)
    }
    pub fn from_config(c: &Config, folder: &Path) -> Result<Self, String> {
        let file = |section: &str| -> Option<PathBuf> {
            let name = c.get(section, "filename")?.split(',').next()?.trim();
            existing(folder, name)
        };
        let mut engine = Vec::new();
        for (key, source) in [
            ("eng1_combustion", Source::Combustion),
            ("eng1_prop", Source::Propeller),
            ("eng1_non_combustion", Source::NonCombustion),
        ] {
            let mut section = c.get("sound_engine", key).map(str::to_owned);
            let mut guard = 0;
            while let Some(name) = section.take() {
                guard += 1;
                if guard > 64 {
                    return Err("Sound layer links do not terminate".into());
                }
                let (Some(rp), Some(vp)) = (c.get(&name, "rparams"), c.get(&name, "vparams"))
                else {
                    break;
                };
                let (rp, vp) = (numbers(rp)?, numbers(vp)?);
                if rp.len() != 4 || vp.len() < 2 || vp.len() % 2 != 0 {
                    return Err(format!("[{name}] has malformed rparams/vparams"));
                }
                if let Some(path) = file(&name) {
                    engine.push(Layer {
                        file: path,
                        source,
                        viewpoint: match c.get(&name, "viewpoint").map(str::trim) {
                            Some("1") => Viewpoint::Inside,
                            Some("2") => Viewpoint::Outside,
                            _ => Viewpoint::Both,
                        },
                        pitch: [(rp[0], rp[1]), (rp[2], rp[3])],
                        volume: vp.chunks(2).map(|p| (p[0], p[1])).collect(),
                    });
                }
                section = c.get(&name, "link").map(str::to_owned);
            }
        }
        let touchdown = ["left_touchdown", "right_touchdown", "center_touchdown"]
            .iter()
            .filter_map(|s| c.get(s, "filename"))
            .flat_map(|list| list.split(','))
            .filter_map(|n| existing(folder, n.trim()))
            .collect();
        Ok(Self {
            engine,
            stall_warning: file("stall_warning"),
            touchdown,
            ground_roll: file("ground_roll"),
            flaps: existing(folder, "c172_flaps").or_else(|| file("flaps")),
            wind: existing(folder, "wind_general"),
        })
    }
}
/// Sound files are referenced without extension; search the aircraft sound
/// folder, then the installation-wide Sound folder (case-insensitive on Windows).
fn existing(folder: &Path, name: &str) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    let file = format!("{name}.wav");
    let shared = folder
        .ancestors()
        .find(|p| p.join("fsx.exe").is_file())
        .map(|root| root.join("Sound"));
    [Some(folder.to_path_buf()), shared]
        .into_iter()
        .flatten()
        .map(|d| d.join(&file))
        .find(|p| p.is_file())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn curves_interpolate_and_clamp() {
        let l = Layer {
            file: PathBuf::new(),
            source: Source::Combustion,
            viewpoint: Viewpoint::Inside,
            pitch: [(0.2, 1.), (0.6, 1.4)],
            volume: vec![(0., 0.), (0.5, 100.), (1., 50.)],
        };
        assert_eq!(l.pitch_at(0.), 1.);
        assert!((l.pitch_at(0.4) - 1.2).abs() < 1e-12);
        assert_eq!(l.pitch_at(2.), 1.4);
        assert_eq!(l.volume_at(0.25), 0.5);
        assert_eq!(l.volume_at(0.75), 0.75);
        assert_eq!(l.volume_at(5.), 0.5);
    }
    #[test]
    fn parses_linked_layers_and_rejects_cycles() {
        let c = Config::parse("[sound_engine]\neng1_prop=a\n[a]\nfilename=missing\nviewpoint=2\nrparams=0,1,1,2\nvparams=0,0,1,100\nlink=b\n[b]\nrparams=0,1\nvparams=0,0\n").unwrap();
        // Missing files are skipped; malformed linked layers are rejected.
        assert!(Sounds::from_config(&c, Path::new(".")).is_err());
        let cycle = Config::parse(
            "[sound_engine]\neng1_prop=a\n[a]\nrparams=0,1,1,2\nvparams=0,0,1,100\nlink=a\n",
        )
        .unwrap();
        assert!(Sounds::from_config(&cycle, Path::new(".")).is_err());
    }
    #[test]
    fn installed_c172_sounds_decode_when_present() {
        let folder =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../FSX/SimObjects/Airplanes/C172/sound");
        if !folder.is_dir() {
            return;
        }
        let s = Sounds::load(&folder, true).unwrap();
        assert!(s.engine.len() >= 20 && s.stall_warning.is_some() && !s.touchdown.is_empty());
        for layer in &s.engine {
            let w = crate::wav::Wave::load(&layer.file).unwrap();
            assert!(w.samples.len() > 1000 && w.samples.iter().all(|x| x.abs() <= 1.));
        }
    }
}
