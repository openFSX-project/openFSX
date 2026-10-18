//! Read the user's installed C172 panel and resolve its native gauge metadata.
//! No DLL execution, window creation, input or installation writes.
use fsx_vehicle::{
    aircraft::VisualAircraft,
    gauge::{GaugeLibrary, ResourceKey},
    panel::{Kind, PanelConfig},
};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

fn unique_file(folder: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    let mut matches = Vec::new();
    for entry in fs::read_dir(folder).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(name)
            && entry.file_type().map_err(|e| e.to_string())?.is_file()
        {
            matches.push(entry.path());
        }
    }
    if matches.len() > 1 {
        return Err(format!("Ambiguous file {name}"));
    }
    Ok(matches.pop())
}

fn run(root: &Path) -> Result<(), String> {
    let aircraft = VisualAircraft::load(&root.join("SimObjects/Airplanes/C172"), 0, true)?;
    let panel = PanelConfig::load(&aircraft.panel, true)?;
    let model = fsx_model::Model::load(aircraft.interior.as_ref().ok_or("C172 has no interior")?)?;
    let folder = root.join("Gauges");
    let mut libraries = BTreeMap::<String, GaugeLibrary>::new();
    let mut native = 0;
    let mut xml = 0;
    for p in &panel.panels {
        let Kind::VirtualCockpit { texture } = &p.kind else {
            continue;
        };
        if !model
            .textures
            .iter()
            .any(|t| t.eq_ignore_ascii_case(texture))
        {
            return Err(format!("No interior texture {texture}"));
        }
        for placement in &p.gauges {
            let key = placement.module.to_ascii_lowercase();
            if !libraries.contains_key(&key) {
                if let Some(path) = unique_file(&folder, &format!("{}.dll", placement.module))? {
                    libraries.insert(key.clone(), GaugeLibrary::load(&path, true)?);
                } else if unique_file(&folder, &format!("{}.cab", placement.module))?.is_some() {
                    xml += 1;
                    println!(
                        "{texture}: {}!{} -> CAB present; XML gauge remains unsupported",
                        placement.module, placement.name
                    );
                    continue;
                } else {
                    return Err(format!("No stock DLL/CAB for {}", placement.module));
                }
            }
            let library = &libraries[&key];
            let gauge = library
                .gauge(&placement.name)
                .map_err(|e| format!("{}: {e}", placement.module))?;
            let referenced = gauge.elements.iter().filter(|e| e.resource_id > 0).count();
            let bitmap_elements = gauge
                .elements
                .iter()
                .filter(|e| {
                    e.resource_id > 0
                        && library.resources.iter().any(|r| {
                            r.kind == ResourceKey::Id(2)
                                && r.name == ResourceKey::Id(e.resource_id as u32)
                        })
                })
                .count();
            println!("{texture}: {}!{} -> v{}, {} elements, {bitmap_elements}/{referenced} positive element IDs have bitmap resources", placement.module, placement.name, gauge.version, gauge.elements.len());
            if bitmap_elements != referenced {
                return Err(format!(
                    "Unresolved bitmap resource in {}!{}",
                    placement.module, placement.name
                ));
            }
            native += 1;
        }
    }
    println!("Resolved {native} native placements in {} libraries; {xml} CAB placements require XML support.", libraries.len());
    let mut bitmap_count = 0;
    let mut bitmap_bytes = 0;
    for library in libraries.values() {
        for resource in library
            .resources
            .iter()
            .filter(|r| r.kind == ResourceKey::Id(2))
        {
            bitmap_bytes += library.resource_bytes(resource)?.len();
            bitmap_count += 1;
        }
    }
    println!("Indexed {bitmap_count} bitmap resource leaves, {bitmap_bytes} bytes accessible without DLL loading.");
    println!("Exact stock C172 scope only. This does not validate gauge execution or instrument rendering.");
    Ok(())
}
fn main() -> Result<(), String> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 1 {
        return Err(
            "Usage: cargo run -p fsx-check --example native-panel-check -- <local FSX root>".into(),
        );
    }
    run(Path::new(&args[0]))
}
