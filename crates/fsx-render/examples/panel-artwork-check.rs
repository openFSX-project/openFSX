//! Local C172 static-artwork inspection. No OS window, input or native DLL execution.
use fsx_model::Model;
use fsx_render::{block_on, panel, Appearance, Camera, Gpu, Orbit};
use fsx_texture::Image;
use fsx_vehicle::{
    aircraft::VisualAircraft,
    gauge::{GaugeLibrary, ResourceKey},
    panel::{Kind, PanelConfig},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

fn save(image: &Image, path: &Path) -> Result<(), String> {
    let mut data = format!("P6\n{} {}\n255\n", image.width, image.height).into_bytes();
    for p in image.rgba.chunks_exact(4) {
        data.extend_from_slice(&p[..3]);
    }
    fs::write(path, data).map_err(|e| e.to_string())
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("Usage: panel-artwork-check <local FSX root> <new output directory> <bitmap language ID>".into());
    }
    let root = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let language = args[2]
        .to_str()
        .ok_or("Invalid language")?
        .parse::<u32>()
        .map_err(|_| "Invalid language ID")?;
    let aircraft = VisualAircraft::load(&root.join("SimObjects/Airplanes/C172"), 0, true)?;
    let config = PanelConfig::load(&aircraft.panel, true)?;
    let mut paths = BTreeMap::new();
    for entry in fs::read_dir(root.join("Gauges")).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("dll"))
        {
            let key = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or("Invalid gauge filename")?
                .to_ascii_lowercase();
            if paths.insert(key, path).is_some() {
                return Err("Ambiguous native gauge module filename".into());
            }
        }
    }
    let mut modules = BTreeSet::new();
    for p in &config.panels {
        if matches!(p.kind, Kind::VirtualCockpit { .. }) {
            modules.extend(p.gauges.iter().map(|g| g.module.to_ascii_lowercase()));
        }
    }
    let mut libraries = BTreeMap::new();
    let mut decoded = 0;
    for module in modules {
        let Some(path) = paths.get(&module) else {
            continue;
        };
        let library = GaugeLibrary::load(path, true)?;
        for resource in library
            .resources
            .iter()
            .filter(|r| r.kind == ResourceKey::Id(2))
        {
            Image::dib(library.resource_bytes(resource)?)?;
            decoded += 1;
        }
        libraries.insert(module, library);
    }
    println!("Decoded {decoded} native bitmap resources. Explicit language: {language}");
    let mut background_files = 0;
    for entry in fs::read_dir(aircraft.panel.parent().ok_or("No panel folder")?)
        .map_err(|e| e.to_string())?
    {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("bmp"))
        {
            Image::load(&path)?;
            background_files += 1;
        }
    }
    println!("Decoded {background_files} ordinary BMP files in the C172 panel folder; 2D window rendering remains unsupported.");
    let model = Model::load(
        aircraft
            .interior
            .as_deref()
            .ok_or("C172 has no native interior")?,
    )?;
    let geometry = model.geometry(None)?;
    let mut appearance = Appearance::load(&model, &geometry, Some(&aircraft.texture))?;
    let eye = aircraft
        .eye_point
        .ok_or("C172 has no configured eye point")?;
    let eye = [eye[1] as f32, -eye[2] as f32, eye[0] as f32];
    let mut orbit = Orbit::new(&geometry);
    orbit.camera = Some(Camera::new(
        eye,
        [eye[0], eye[1] - 0.45, eye[2] + 1.],
        [0., 1., 0.],
        0.01,
        1000.,
        55.,
    )?);
    let mut gpu = block_on(Gpu::new(None, &geometry, &appearance, false, 960, 720))?;
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    gpu.snapshot(&orbit, &output.join("c172-panel-before.ppm"))?;
    let mut total = 0;
    for p in &config.panels {
        let Kind::VirtualCockpit { texture } = &p.kind else {
            continue;
        };
        let preview = panel::static_roots(p, &libraries, language)?;
        println!(
            "{texture}: {} static roots drawn, {} elements omitted, {} diagnostics",
            preview.drawn_roots,
            preview.omitted_elements,
            preview.diagnostics.len()
        );
        for message in &preview.diagnostics {
            println!("  Omitted: {message}");
        }
        save(
            &preview.image,
            &output.join(format!("panel-{:02}-static-roots.ppm", p.index)),
        )?;
        total += preview.drawn_roots;
        appearance.bind_image(texture, preview.image)?;
    }
    if total == 0 {
        return Err("No native root artwork rendered; inspect exact resource language IDs".into());
    }
    gpu.replace_scene(&geometry, &appearance)?;
    gpu.snapshot(&orbit, &output.join("c172-panel-static-artwork.ppm"))?;
    println!("PASS: {decoded} resource images decoded, {total} static roots composed and bound to the original interior; same camera/GPU device. No needles, moving layers, digital values, XML, callbacks or runtime instrument behavior.");
    Ok(())
}
fn main() -> Result<(), String> {
    run()
}
