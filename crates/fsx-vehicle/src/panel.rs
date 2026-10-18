//! Declarative panel placement only. No gauge code is loaded or executed.
use fsx_cfg::{decode, Config, Section};
use std::{collections::BTreeSet, fs, path::Path};

const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_PANELS: usize = 256;
const MAX_GAUGES: usize = 4096;
const MAX_COORD: f64 = 65536.;

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Window,
    VirtualCockpit { texture: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Gauge {
    pub index: u32,
    pub module: String,
    pub name: String,
    pub position: [f64; 2],
    /// Omitted dimensions require the gauge's own intrinsic size.
    pub size: [Option<f64>; 2],
    pub parameters: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    pub index: u32,
    pub kind: Kind,
    pub title: Option<String>,
    /// Panel-coordinate extent; omitted height requires background aspect ratio.
    pub size_mm: Option<[Option<f64>; 2]>,
    /// Explicit pixel extent; zero/omitted height requires aspect-ratio resolution.
    pub pixel_size: Option<[Option<f64>; 2]>,
    pub background_color: [u8; 3],
    pub gauges: Vec<Gauge>,
    /// Entries beyond the documented extended-index break, retained for inspection.
    pub ignored_gauges: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PanelConfig {
    pub panels: Vec<Panel>,
    /// Unsupported directives are retained as diagnostics rather than interpreted.
    pub unsupported: Vec<String>,
}

fn indexed(name: &str, prefix: &str) -> Option<u32> {
    let suffix = name.strip_prefix(prefix)?;
    if suffix.is_empty() || !suffix.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    suffix.parse().ok()
}

fn scalar(text: &str, positive: bool) -> Result<f64, String> {
    let value = text
        .trim()
        .parse::<f64>()
        .map_err(|_| "Invalid panel coordinate")?;
    if !value.is_finite() || value.abs() > MAX_COORD || (positive && value <= 0.) {
        return Err("Panel coordinates must be finite, bounded, and sizes positive".into());
    }
    Ok(value)
}

fn dimensions(text: &str, zero_height: bool) -> Result<[Option<f64>; 2], String> {
    let fields: Vec<_> = text.split(',').collect();
    if fields.is_empty() || fields.len() > 2 {
        return Err("Expected panel width[,height]".into());
    }
    let width = scalar(fields[0], true)?;
    let height = fields.get(1).map(|v| scalar(v, false)).transpose()?;
    if height.is_some_and(|v| v < 0. || (v == 0. && !zero_height)) {
        return Err("Invalid panel height".into());
    }
    Ok([Some(width), height.filter(|v| *v != 0.)])
}

fn gauge(index: u32, text: &str) -> Result<Gauge, String> {
    // The final field belongs to the gauge; preserve embedded commas verbatim.
    let fields: Vec<_> = text.splitn(6, ',').map(str::trim).collect();
    if fields.len() < 3 {
        return Err("Gauge requires module!name,x,y[,width,height,parameters]".into());
    }
    let (module, name) = fields[0]
        .split_once('!')
        .ok_or("Gauge requires module!name")?;
    let module = module.trim();
    let name = name.trim();
    if module.is_empty()
        || name.is_empty()
        || module.len() > 256
        || name.len() > 256
        || module.contains(['/', '\\', ':', '\0', '!'])
        || module == "."
        || module == ".."
        || name.contains(['\0', '!'])
    {
        return Err("Invalid gauge module or name".into());
    }
    let optional_size = |i: usize| {
        fields
            .get(i)
            .filter(|v| !v.is_empty())
            .map(|v| scalar(v, true))
            .transpose()
    };
    Ok(Gauge {
        index,
        module: module.to_owned(),
        name: name.to_owned(),
        position: [scalar(fields[1], false)?, scalar(fields[2], false)?],
        size: [optional_size(3)?, optional_size(4)?],
        parameters: fields
            .get(5)
            .filter(|v| !v.is_empty())
            .map(|v| (*v).to_owned()),
    })
}

fn parse_panel(
    index: u32,
    kind: Kind,
    title: Option<String>,
    values: &Section,
) -> Result<Panel, String> {
    let mut all = std::collections::BTreeMap::new();
    for (key, value) in values {
        if let Some(i) = indexed(key, "gauge") {
            if i as usize >= MAX_GAUGES || all.insert(i, value).is_some() {
                return Err("Gauge index exceeds limit or has duplicate numeric spelling".into());
            }
        }
    }
    let mut gauges = Vec::new();
    let mut ignored_gauges = Vec::new();
    let mut next = 100;
    for (i, value) in all {
        if i >= 100 {
            if i != next {
                ignored_gauges.push(i);
                continue;
            }
            next += 1;
        }
        gauges.push(gauge(i, value).map_err(|e| format!("gauge{i:02}: {e}"))?);
    }
    let mut background_color = [0; 3];
    if let Some(color) = values.get("background_color") {
        let fields: Vec<_> = color.split(',').map(str::trim).collect();
        if fields.len() != 3 {
            return Err("background_color requires three bytes".into());
        }
        for (i, field) in fields.iter().enumerate() {
            background_color[i] = field.parse().map_err(|_| "Invalid background_color byte")?;
        }
    }
    Ok(Panel {
        index,
        kind,
        title,
        size_mm: values
            .get("size_mm")
            .map(|v| dimensions(v, false))
            .transpose()
            .map_err(|e| format!("size_mm: {e}"))?,
        pixel_size: values
            .get("pixel_size")
            .map(|v| dimensions(v, true))
            .transpose()
            .map_err(|e| format!("pixel_size: {e}"))?,
        background_color,
        gauges,
        ignored_gauges,
    })
}

impl PanelConfig {
    /// Match the model's dynamic texture name without altering its placement units.
    pub fn virtual_cockpit(&self, texture_name: &str) -> Option<&Panel> {
        self.panels.iter().find(|panel| match &panel.kind {
            Kind::VirtualCockpit { texture } => texture.eq_ignore_ascii_case(texture_name),
            Kind::Window => false,
        })
    }

    pub fn load(path: &Path, windows_1252: bool) -> Result<Self, String> {
        // A bounded read also handles files changed between metadata and reading.
        use std::io::Read;
        let mut bytes = Vec::new();
        fs::File::open(path)
            .and_then(|f| f.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&decode(&bytes, windows_1252)?)
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > MAX_BYTES || text.contains('\0') {
            return Err("Panel CFG exceeds 2 MiB or contains NUL".into());
        }
        let cfg = Config::parse(text)?;
        if cfg.get("fltsim", "alias").is_some() {
            return Err("Panel alias resolution is not implemented".into());
        }
        let mut result = Self {
            panels: Vec::new(),
            unsupported: Vec::new(),
        };
        let mut seen = BTreeSet::new();
        let mut textures = BTreeSet::new();
        let mut gauge_count = 0;
        for (section, values) in cfg.sections() {
            let (virtual_cockpit, index) = if let Some(index) = indexed(section, "vcockpit") {
                (true, index)
            } else if let Some(index) = indexed(section, "window") {
                (false, index)
            } else {
                if section != "window titles" {
                    result.unsupported.push(format!("[{section}]"));
                }
                continue;
            };
            if result.panels.len() >= MAX_PANELS || !seen.insert((virtual_cockpit, index)) {
                return Err("Too many panels or duplicate numeric panel section".into());
            }
            let kind = if virtual_cockpit {
                let texture = values
                    .get("texture")
                    .ok_or_else(|| format!("[{section}] missing texture"))?;
                if !texture.starts_with('$')
                    || texture.len() < 2
                    || texture.len() > 15
                    || texture.contains(['/', '\\', ':', ','])
                    || texture.chars().any(char::is_whitespace)
                    || !textures.insert(texture.to_ascii_lowercase())
                {
                    return Err(format!("[{section}] invalid or duplicate VC texture"));
                }
                Kind::VirtualCockpit {
                    texture: texture.to_owned(),
                }
            } else {
                Kind::Window
            };
            // Find titles numerically, including differently padded section keys.
            let title = cfg
                .section("window titles")
                .and_then(|titles| {
                    titles
                        .iter()
                        .find(|(k, _)| indexed(k, "window") == Some(index))
                        .map(|(_, v)| v.clone())
                })
                .filter(|_| !virtual_cockpit);
            let panel =
                parse_panel(index, kind, title, values).map_err(|e| format!("[{section}] {e}"))?;
            gauge_count += panel.gauges.len();
            if gauge_count > MAX_GAUGES {
                return Err("Panel CFG exceeds 4096 loaded gauges".into());
            }
            for key in values.keys() {
                if indexed(key, "gauge").is_none()
                    && !["texture", "size_mm", "pixel_size", "background_color"]
                        .contains(&key.as_str())
                {
                    result.unsupported.push(format!("[{section}] {key}"));
                }
            }
            result.panels.push(panel);
        }
        result
            .panels
            .sort_by_key(|p| (matches!(p.kind, Kind::VirtualCockpit { .. }), p.index));
        if result.panels.is_empty() {
            return Err("Panel CFG has no WindowNN or VCockpitNN sections".into());
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_layout_and_parameters_preserve_units_case_order_and_optional_sizes() {
        let c = PanelConfig::parse("[Window Titles]\nWindow02=Original synthetic panel\n[Window02]\nsize_mm=640\ngauge09=Test!Lamp, -2,4\ngauge01=Test!Dial,2,3,40,,alpha,beta\n[VCockpit01]\ntexture=$Own\nsize_mm=512,256\npixel_size=1024,0\nbackground_color=1,2,255\ngauge00=Test!Needle,8,9,20,30\n").unwrap();
        assert_eq!(
            c.panels[0].title.as_deref(),
            Some("Original synthetic panel")
        );
        assert_eq!(c.panels[0].size_mm, Some([Some(640.), None]));
        let gauges = &c.panels[0].gauges;
        assert_eq!(gauges.iter().map(|g| g.index).collect::<Vec<_>>(), [1, 9]);
        assert_eq!(gauges[0].size, [Some(40.), None]);
        assert_eq!(gauges[0].parameters.as_deref(), Some("alpha,beta"));
        assert_eq!(gauges[1].position, [-2., 4.]);
        assert_eq!(gauges[1].size, [None, None]);
        let vc = &c.panels[1];
        assert_eq!(
            vc.kind,
            Kind::VirtualCockpit {
                texture: "$Own".into()
            }
        );
        assert_eq!(vc.pixel_size, Some([Some(1024.), None]));
        assert_eq!(vc.background_color, [1, 2, 255]);
        assert_eq!(c.virtual_cockpit("$OWN"), Some(vc));
        assert!(c.virtual_cockpit("Own").is_none());
        assert!(c.virtual_cockpit("$Missing").is_none());
    }
    #[test]
    fn extended_gauges_stop_at_first_gap_and_indices_are_numeric() {
        let c = PanelConfig::parse("[Window00]\ngauge99=Own!A,0,0\ngauge100=Own!B,0,0\ngauge101=Own!C,0,0\ngauge103=invalid ignored value\ngauge104=invalid ignored value").unwrap();
        assert_eq!(
            c.panels[0]
                .gauges
                .iter()
                .map(|g| g.index)
                .collect::<Vec<_>>(),
            [99, 100, 101]
        );
        assert_eq!(c.panels[0].ignored_gauges, [103, 104]);
        assert!(PanelConfig::parse("[Window00]\ngauge00=Own!A,0,0\ngauge0=Own!B,0,0").is_err());
        assert!(PanelConfig::parse("[Window00]\nsize_mm=10\n[Window0]\nsize_mm=20").is_err());
    }
    #[test]
    fn malformed_placements_textures_and_aliases_fail_explicitly() {
        for text in [
            "[Window00]\ngauge00=Own!A,NaN,1",
            "[Window00]\ngauge00=Own!A,0,1,-2,5",
            "[Window00]\ngauge00=../Own!A,0,1",
            "[Window00]\ngauge00=Own!A,0,1e99",
            "[Window00]\nsize_mm=1,2,3",
            "[Window00]\npixel_size=512,512s",
            "[Window00]\nbackground_color=0,0,256",
            "[VCockpit01]\nsize_mm=512,512",
            "[VCockpit01]\ntexture=Own",
            "[VCockpit01]\ntexture=$Own\n[VCockpit02]\ntexture=$own",
            "[fltsim]\nalias=Other\\panel",
            "[Views]\nview_forward_dir=1,2,3",
            "[Window00]\ngauge4096=Own!A,0,0",
            "[Window00]\nsize_mm=2\0",
        ] {
            assert!(PanelConfig::parse(text).is_err(), "Accepted: {text}");
        }
        assert!(PanelConfig::parse(&" ".repeat(MAX_BYTES + 1)).is_err());
    }
    #[test]
    fn unsupported_layout_and_background_directives_are_not_silently_applied() {
        let c = PanelConfig::parse(
            "[Window00]\nfile=own.bmp\nvisible=1\n[Views]\nview_forward_dir=8,0,0",
        )
        .unwrap();
        assert_eq!(
            c.unsupported,
            ["[views]", "[window00] file", "[window00] visible"]
        );
    }
}
