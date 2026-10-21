//! Recorded native clock strings -> authored Quartz layout -> cockpit GPU texture.
//! Inputs are synthetic; this check does not implement a live simulator clock.
use fsx_model::Model;
use fsx_render::{
    block_on, native_text, panel, xml_text::GaugeFont, Appearance, Camera, Gpu, Orbit,
};
use fsx_vehicle::{
    aircraft::VisualAircraft,
    gauge::GaugeLibrary,
    panel::{Kind, PanelConfig},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

fn samples(text: &str, rva: u32) -> Result<Vec<Value>, String> {
    if text.len() > 65536 {
        return Err("Native string record budget exceeded".into());
    }
    let lines: Vec<_> = text.lines().collect();
    if lines.len() != 3 {
        return Err("Expected exactly three clock sample records".into());
    }
    let mut records = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.len() > 16384 {
            return Err("Native string line budget exceeded".into());
        }
        let v: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let (inputs, expected, result) = [
            ([0, 0, 0], "00:00:00", 0),
            ([12, 34, 56], "12:34:56", 45296),
            ([23, 59, 59], "23:59:59", 86399),
        ][index];
        if v["schema"] != 1
            || v["source"] != "native-string-probe"
            || v["pointer_bits"] != 32
            || v["library"] != "Cessna172.dll"
            || v["gauge"] != "Clock"
            || v["element_rva"] != rva
            || v["tokens"] != json!([3, 4, 5])
            || v["max_length"] != 8
            || v["input_kinds"] != json!(["f64", "f64", "f64"])
            || v["raw_inputs"] != json!(inputs)
            || v["text"] != expected
            || v["callback_result"] != result
        {
            return Err(
                "Native clock sample does not match the checked profile/inputs/output".into(),
            );
        }
        for key in [
            "module_init_called",
            "module_deinit_called",
            "guarded_buffer",
            "gauge_connection_notifications",
            "private_state_created_by_original",
        ] {
            if v[key] != true {
                return Err(format!("Missing native sample guarantee: {key}"));
            }
        }
        for key in ["panel_window", "panel_services", "full_gauge_lifecycle"] {
            if v[key] != false {
                return Err(format!("Unsupported native sample context: {key}"));
            }
        }
        records.push(v);
    }
    Ok(records)
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("Usage: native-string-render-check <local FSX root> <new output directory> <clock NDJSON>".into());
    }
    let root = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let mut bytes = Vec::new();
    fs::File::open(Path::new(&args[2]))
        .and_then(|f| f.take(65537).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "Clock records must be UTF-8")?;
    let aircraft = VisualAircraft::load(&root.join("SimObjects/Airplanes/C172"), 0, true)?;
    let config = PanelConfig::load(&aircraft.panel, true)?;
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
            GaugeLibrary::load(&root.join(format!("Gauges/{name}.dll")), true)?,
        );
    }
    let library = &libraries["cessna172"];
    let gauge = library.gauge("Clock")?;
    let candidates: Vec<_> = gauge
        .elements
        .iter()
        .filter(|e| e.kind == 2 && e.text.as_ref().is_some_and(|s| s.tokens == [3, 4, 5]))
        .collect();
    if candidates.len() != 1 {
        return Err("Expected a unique C172 main clock string".into());
    }
    let element = candidates[0];
    let style = element.text.as_ref().unwrap();
    if style.max_length != 8
        || style.source_callbacks.iter().any(Option::is_some)
        || style.update_callback_rva.is_none()
    {
        return Err("Unsupported C172 clock descriptor".into());
    }
    let records = samples(text, element.rva)?;
    let font_path = root.join("Fonts/quartz.ttf");
    let font = GaugeFont::load(&font_path)?;
    let model = Model::load(aircraft.interior.as_deref().ok_or("No C172 interior")?)?;
    let geometry = model.geometry(None)?;
    let eye = aircraft.eye_point.ok_or("No C172 eye point")?;
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
    let mut gpu: Option<Gpu> = None;
    let mut reports = Vec::new();
    let mut snapshots = Vec::new();
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    // Repeat the first state to detect stale glyphs/background accumulation.
    for (index, sample_index) in [0, 1, 2, 0].into_iter().enumerate() {
        let value = records[sample_index]["text"].as_str().unwrap();
        let raster = native_text::rasterize_unselected(&font, style, element.image_flags, value)
            .map_err(|e| {
                format!(
                    "{e}; flags={} offset={:?} style={style:?}",
                    element.image_flags, element.offset
                )
            })?;
        let mut ppm =
            format!("P6\n{} {}\n255\n", raster.image.width, raster.image.height).into_bytes();
        for p in raster.image.rgba.chunks_exact(4) {
            ppm.extend_from_slice(&p[..3]);
        }
        fs::write(output.join(format!("clock-text-{index}.ppm")), ppm)
            .map_err(|e| e.to_string())?;
        let mut appearance = Appearance::load(&model, &geometry, Some(&aircraft.texture))?;
        let mut drawn = 0;
        for p in &config.panels {
            let Kind::VirtualCockpit { texture } = &p.kind else {
                continue;
            };
            let mut preview = panel::static_roots(p, &libraries, 1033)?;
            for placement in &p.gauges {
                if placement.module.eq_ignore_ascii_case("Cessna172")
                    && placement.name.eq_ignore_ascii_case("Clock")
                {
                    panel::paint_native_text(
                        &mut preview,
                        p,
                        placement,
                        library,
                        element.rva,
                        1033,
                        &raster.image,
                    )?;
                    drawn += 1;
                }
            }
            appearance.bind_image(texture, preview.image)?;
        }
        if drawn != 1 {
            return Err("Expected exactly one C172 cockpit clock placement".into());
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
        let path = output.join(format!("c172-clock-{index}.ppm"));
        gpu.as_ref().unwrap().snapshot(&orbit, &path)?;
        snapshots.push(fs::read(path).map_err(|e| e.to_string())?);
        reports.push(
            json!({"text":value,"sample_index":sample_index,"em_pixels":raster.em_pixels,
            "capital_w_count_width":raster.capital_w_width,"drawn_placements":drawn}),
        );
    }
    let header = b"P6\n960 720\n255\n";
    if snapshots
        .iter()
        .any(|s| !s.starts_with(header) || s.len() != header.len() + 960 * 720 * 3)
    {
        return Err("Unexpected GPU snapshot format".into());
    }
    if snapshots[0] != snapshots[3] {
        return Err("Restoring the first clock state changed pixels".into());
    }
    let mut comparisons = Vec::new();
    for index in 1..3 {
        let mut changed = 0;
        let mut min = [960, 720];
        let mut max = [0, 0];
        for (i, (a, b)) in snapshots[index - 1][header.len()..]
            .chunks_exact(3)
            .zip(snapshots[index][header.len()..].chunks_exact(3))
            .enumerate()
        {
            if a != b {
                changed += 1;
                let p = [i % 960, i / 960];
                for j in 0..2 {
                    min[j] = min[j].min(p[j]);
                    max[j] = max[j].max(p[j]);
                }
            }
        }
        if changed == 0 {
            return Err("Native clock changes did not reach cockpit GPU pixels".into());
        }
        comparisons
            .push(json!({"from":index-1,"to":index,"changed_pixels":changed,"min":min,"max":max}));
    }
    let report = json!({"schema":1,"profile":"C172 recorded native clock string",
        "font_path":font_path,"font_name":font.name(),"element_rva":element.rva,
        "position":element.position,"size":style.size,"max_length":style.max_length,
        "font_weight":style.font_weight,"font_height":style.font_height,"font_charset":style.font_charset,
        "draw_text_flags":style.draw_text_flags,"image_flags":element.image_flags,"colors":style.colors,
        "states":reports,"gpu_comparisons":comparisons,"restored_pixels_identical":true,
        "synthetic_time_inputs":true,"authored_style_only":true,"gdi_pixel_parity_verified":false,
        "selection_state":"explicitly unselected","selection_sequence_rva":style.selection_sequence_rva,
        "operational_clock":false,"full_gauge_lifecycle":false});
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!("PASS: three recorded native strings -> matching Quartz font -> authored C172 placement -> distinct GPU pixels; restored state identical. Report: {}", output.join("report.json").display());
    Ok(())
}
fn main() -> Result<(), String> {
    run()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> String {
        [([0,0,0],"00:00:00",0),([12,34,56],"12:34:56",45296),([23,59,59],"23:59:59",86399)]
            .into_iter().map(|(input,text,result)| json!({"schema":1,"source":"native-string-probe",
                "pointer_bits":32,"library":"Cessna172.dll","gauge":"Clock","element_rva":100,
                "tokens":[3,4,5],"max_length":8,"input_kinds":["f64","f64","f64"],"raw_inputs":input,
                "callback_result":result,"text":text,"module_init_called":true,"module_deinit_called":true,
                "guarded_buffer":true,"gauge_connection_notifications":true,"private_state_created_by_original":true,
                "panel_window":false,"panel_services":false,"full_gauge_lifecycle":false}).to_string()).collect::<Vec<_>>().join("\n")
    }
    #[test]
    fn recorded_clock_contract_rejects_wrong_context_and_stale_metadata() {
        let data = fixture();
        assert_eq!(samples(&data, 100).unwrap().len(), 3);
        assert!(samples(&data, 101).is_err());
        for (from, to) in [
            ("\"guarded_buffer\":true", "\"guarded_buffer\":false"),
            (
                "\"full_gauge_lifecycle\":false",
                "\"full_gauge_lifecycle\":true",
            ),
            ("12:34:56", "12:34:57"),
            ("[3,4,5]", "[3,4,6]"),
            ("[\"f64\",\"f64\",\"f64\"]", "[\"u32\",\"f64\",\"f64\"]"),
        ] {
            assert!(data.contains(from));
            assert!(samples(&data.replace(from, to), 100).is_err());
        }
        assert!(samples(&format!("{data}\n{data}"), 100).is_err());
        assert!(samples(&" ".repeat(65537), 100).is_err());
    }
}
