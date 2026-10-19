//! Installed-resource sprite inspection. No native library execution.
use fsx_render::panel;
use fsx_texture::Image;
use fsx_vehicle::{
    gauge::GaugeLibrary,
    panel::{Kind, PanelConfig},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

fn save(image: &Image, path: &Path) -> Result<(), String> {
    let mut bytes = format!("P6\n{} {}\n255\n", image.width, image.height).into_bytes();
    for p in image.rgba.chunks_exact(4) {
        bytes.extend_from_slice(&p[..3]);
    }
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}
fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(args.next().ok_or("Expected FSX root")?);
    let output = PathBuf::from(args.next().ok_or("Expected new output directory")?);
    if args.next().is_some() {
        return Err("Unexpected argument".into());
    }
    std::fs::create_dir(&output).map_err(|e| e.to_string())?;
    let config = PanelConfig::load(
        &root.join("SimObjects/Airplanes/C172/panel/panel.cfg"),
        true,
    )?;
    let mut libraries = BTreeMap::new();
    for name in ["Cessna", "Bell_206B"] {
        libraries.insert(
            name.to_ascii_lowercase(),
            GaugeLibrary::load(&root.join(format!("Gauges/{name}.dll")), true)?,
        );
    }
    for (module, gauge_name) in [("cessna", "Heading_Indicator"), ("bell_206b", "Attitude")] {
        let library = &libraries[module];
        let gauge = library.gauge(gauge_name)?;
        for e in &gauge.elements {
            println!(
                "{module}!{gauge_name} resource {} kind {} position {:?} center {:?} sprite {:?}",
                e.resource_id, e.kind, e.position, e.offset, e.sprite
            );
            for id in [
                e.resource_id,
                if e.sprite.is_some() {
                    e.resource_id + 1
                } else {
                    e.resource_id
                },
            ] {
                let r = library
                    .resource(2, id as u32, 1033)
                    .ok_or("Missing bitmap")?;
                let image = Image::dib(library.resource_bytes(r)?)?;
                println!("  resource {id}: {}x{}", image.width, image.height);
                save(&image, &output.join(format!("{module}-{id}.ppm")))?;
            }
        }
        let panel = config
            .panels
            .iter()
            .find(|p| {
                matches!(p.kind, Kind::VirtualCockpit { .. })
                    && p.gauges
                        .iter()
                        .any(|g| g.module.eq_ignore_ascii_case(module) && g.name == gauge_name)
            })
            .ok_or("No VC placement")?;
        let preview = panel::static_roots(panel, &libraries, 1033)?;
        save(&preview.image, &output.join(format!("{module}-roots.ppm")))?;
        let placement = panel
            .gauges
            .iter()
            .find(|g| g.module.eq_ignore_ascii_case(module) && g.name == gauge_name)
            .unwrap();
        let mut isolated = panel.clone();
        let mut placement = placement.clone();
        placement.position = [0., 0.];
        placement.size = [Some(200.), Some(200.)];
        isolated.size_mm = Some([Some(200.), Some(200.)]);
        isolated.pixel_size = Some([Some(200.), Some(200.)]);
        isolated.gauges = vec![placement.clone()];
        let mut states = Vec::new();
        for (name, pitch, bank, heading) in [
            ("level", 0., 0., 0.),
            ("pitch-up", 10., 0., 0.),
            ("pitch-down", -10., 0., 0.),
            ("bank-right", 0., 30., 0.),
            ("east", 0., 0., 90.),
        ] {
            let mut preview = panel::static_roots(&isolated, &libraries, 1033)?;
            for e in &gauge.elements {
                if let Some(sprite) = &e.sprite {
                    let values = std::array::from_fn(|i| match sprite.axes[i].token {
                        0 => 0.,
                        78 => -pitch,
                        80 => -bank,
                        84 => heading,
                        _ => f64::NAN,
                    });
                    panel::paint_sprite(
                        &mut preview,
                        &isolated,
                        &placement,
                        library,
                        e.resource_id,
                        1033,
                        values,
                    )?;
                } else if module == "bell_206b" && [2013, 2001, 12007].contains(&e.resource_id) {
                    panel::paint_fixed_layer(
                        &mut preview,
                        &isolated,
                        &placement,
                        library,
                        e.resource_id,
                        1033,
                    )?;
                }
            }
            save(&preview.image, &output.join(format!("{module}-{name}.ppm")))?;
            states.push(preview.image.rgba);
        }
        let moving = if module == "cessna" { 4 } else { 1 };
        if states[0] == states[moving] {
            return Err("Sprite artwork did not respond to its explicit inputs".into());
        }
    }
    println!("PASS: original sprites/masks, authored metadata and five explicit orientation states; no native callbacks or FSX reference comparison in this inspector.");
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Sprite check failed: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
