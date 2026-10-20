//! Installed XML text -> selected font -> authored VC placement -> GPU snapshot.
//! All panel conditions are synthetic; this is not a running-FSX comparison.
use fsx_model::Model;
use fsx_render::{block_on, panel, xml_text::GaugeFont, Appearance, Camera, Gpu, Orbit};
use fsx_vehicle::{
    aircraft::VisualAircraft,
    cabinet::Cabinet,
    gauge::GaugeLibrary,
    panel::{Kind, PanelConfig},
    xml_gauge::GaugeDocument,
    xml_text::TextGauge,
};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::PathBuf, process::ExitCode};

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(
            "Usage: xml-text-render-check <FSX root> <new output directory> <explicit font.ttf>"
                .into(),
        );
    }
    let root = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let font_path = PathBuf::from(&args[2]);
    let font = GaugeFont::load(&font_path)?;
    let cabinet = Cabinet::load(&root.join("Gauges/n_number_plaque.cab"), true)?;
    let document = GaugeDocument::from_cabinet(&cabinet, "n_number_plaque.xml", false)?;
    let typed = TextGauge::from_document(&document)?;
    let mut libraries = BTreeMap::new();
    for name in [
        "Cessna",
        "Cessna172",
        "Cessna182s",
        "Bell_206B",
        "Bendix_king_radio",
    ] {
        libraries.insert(
            name.to_ascii_lowercase(),
            GaugeLibrary::load(&root.join("Gauges").join(format!("{name}.dll")), true)?,
        );
    }
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    let mut reports = Vec::new();
    let mut gpu: Option<Gpu> = None;
    let mut previous_registration = None;
    for variant in [0, 1] {
        let aircraft =
            VisualAircraft::load(&root.join("SimObjects/Airplanes/C172"), variant, true)?;
        let registration = aircraft
            .registration
            .as_deref()
            .ok_or("No authored registration")?;
        let config = PanelConfig::load(&aircraft.panel, true)?;
        let model = Model::load(aircraft.interior.as_deref().ok_or("No interior")?)?;
        let geometry = model.geometry(None)?;
        let eye = aircraft.eye_point.ok_or("No configured eye")?;
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
        let mut previous_day = None;
        let mut day_snapshot = None;
        for (state, time, light) in [("day", 1., false), ("night-lit", 3., true)] {
            let rendered = font.render(
                &typed,
                &cabinet,
                "n_number_plaque.xml",
                |namespace, name, units| match (
                    namespace,
                    name.to_ascii_lowercase().as_str(),
                    units.to_ascii_lowercase().as_str(),
                ) {
                    ('P', "time of day", "enum") => Some(time),
                    ('A', "light panel", "bool") => Some(u8::from(light) as f64),
                    _ => None,
                },
                |name| {
                    name.eq_ignore_ascii_case("ATC ID")
                        .then(|| registration.to_owned())
                },
            )?;
            if state == "day" {
                if previous_registration
                    .as_ref()
                    .is_some_and(|p| *p == rendered.image.rgba)
                {
                    return Err(
                        "Different variant registrations produced identical gauge images".into(),
                    );
                }
                previous_registration = Some(rendered.image.rgba.clone());
                previous_day = Some(rendered.image.rgba.clone());
            } else if previous_day.as_ref() == Some(&rendered.image.rgba) {
                return Err("Day and lit-night gauge images are identical".into());
            }
            let name = format!("variant-{variant}-{state}");
            let mut gauge_ppm = format!(
                "P6\n{} {}\n255\n",
                rendered.image.width, rendered.image.height
            )
            .into_bytes();
            for p in rendered.image.rgba.chunks_exact(4) {
                gauge_ppm.extend_from_slice(&p[..3]);
            }
            fs::write(output.join(format!("{name}-gauge.ppm")), gauge_ppm)
                .map_err(|e| e.to_string())?;
            let mut appearance = Appearance::load(&model, &geometry, Some(&aircraft.texture))?;
            let mut placements = 0;
            for p in config
                .panels
                .iter()
                .filter(|p| matches!(p.kind, Kind::VirtualCockpit { .. }))
            {
                let Kind::VirtualCockpit { texture } = &p.kind else {
                    unreachable!()
                };
                let mut preview = panel::static_roots(p, &libraries, 1033)?;
                for placement in p.gauges.iter().filter(|g| {
                    g.module.eq_ignore_ascii_case("n_number_plaque")
                        && g.name.eq_ignore_ascii_case("n_number_plaque")
                }) {
                    panel::paint_xml_image(&mut preview, p, placement, &rendered.image)?;
                    placements += 1;
                }
                appearance.bind_image(texture, preview.image)?;
            }
            if placements != 1 {
                return Err("Expected one C172 VC plaque placement".into());
            }
            if let Some(gpu) = &mut gpu {
                gpu.replace_scene(&geometry, &appearance)?;
            } else {
                gpu = Some(block_on(Gpu::new(
                    None,
                    &geometry,
                    &appearance,
                    false,
                    960,
                    720,
                ))?);
            }
            let snapshot = output.join(format!("{name}-cockpit.ppm"));
            gpu.as_ref().unwrap().snapshot(&orbit, &snapshot)?;
            let snapshot = fs::read(snapshot).map_err(|e| e.to_string())?;
            let header = b"P6\n960 720\n255\n";
            if !snapshot.starts_with(header) || snapshot.len() != header.len() + 960 * 720 * 3 {
                return Err("Unexpected XML snapshot layout".into());
            }
            let mut changed = 0;
            let mut low = [960usize, 720];
            let mut high = [0usize; 2];
            if let Some(day) = &day_snapshot {
                let day: &Vec<u8> = day;
                for (i, (a, b)) in day[header.len()..]
                    .chunks_exact(3)
                    .zip(snapshot[header.len()..].chunks_exact(3))
                    .enumerate()
                {
                    if a != b {
                        changed += 1;
                        let xy = [i % 960, i / 960];
                        for axis in 0..2 {
                            low[axis] = low[axis].min(xy[axis]);
                            high[axis] = high[axis].max(xy[axis]);
                        }
                    }
                }
                if changed == 0 {
                    return Err("XML color change did not reach the GPU cockpit snapshot".into());
                }
            } else {
                day_snapshot = Some(snapshot);
            }
            reports.push(json!({"variant":variant,"registration":registration,"state":state,"font_path":font_path,"font_name":font.name(),
                "changed_cockpit_pixels_from_day":changed,"changed_bounds_from_day":if changed > 0 {Some([low,high])}else{None},
                "explicit_font_for_missing_face":rendered.explicit_default_font,"font_height_interpretation":"pixels per em",
                "gauge_size":typed.size,"vc_placements":placements,"gauge_bitmap":"authored day background in both states",
                "conditions":"synthetic", "fsx_parity_validated":false}));
        }
    }
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&reports).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!(
        "Rendered {} XML/VC cases with explicit font {:?}",
        reports.len(),
        font.name()
    );
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
