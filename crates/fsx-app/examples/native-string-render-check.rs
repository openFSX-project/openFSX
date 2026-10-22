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

fn session_samples(text: &str, rva: u32) -> Result<Vec<Value>, String> {
    if text.len() > 65536 {
        return Err("Native clock session record budget exceeded".into());
    }
    let records: Vec<Value> = text
        .lines()
        .map(|line| {
            if line.len() > 4096 {
                return Err("Native clock session line budget exceeded".into());
            }
            serde_json::from_str(line).map_err(|e| e.to_string())
        })
        .collect::<Result<_, _>>()?;
    if records.len() != 31 {
        return Err("Expected 30 checked session states and teardown".into());
    }
    for (index, row) in records[..30].iter().enumerate() {
        if row["schema"] != 1
            || row["source"] != "native-clock-local-stream"
            || row["kind"] != "state"
            || row["sequence"] != index + 1
            || row["element_rva"] != rva
            || row["pointer_bits"] != 32
            || row["library"] != "Cessna172.dll"
            || row["gauge"] != "Clock"
            || !row["text"]
                .as_str()
                .is_some_and(|s| s.len() <= 8 && s.is_ascii() && !s.chars().any(char::is_control))
            || !row["callback_result"].as_f64().is_some_and(f64::is_finite)
        {
            return Err("Native clock session metadata/string mismatch".into());
        }
        for key in ["guarded_context", "guarded_string", "guarded_event_words"] {
            if row[key] != true {
                return Err(format!("Missing session guard: {key}"));
            }
        }
        for key in ["panel_services", "multiplayer", "fsx_parity_verified"] {
            if row[key] != false {
                return Err(format!("Unexpected session support: {key}"));
            }
        }
    }
    let closed = &records[30];
    if closed["schema"] != 1
        || closed["source"] != "native-clock-local-stream"
        || closed["kind"] != "closed"
        || closed["sequence"] != 30
        || closed["modules_deinitialized"] != 4
    {
        return Err("Missing session teardown".into());
    }
    for key in [
        "clock_disconnected",
        "string_buffer_restored",
        "import_pointer_restored",
        "context_guards_verified",
    ] {
        if closed[key] != true {
            return Err(format!("Missing session teardown guarantee: {key}"));
        }
    }
    for (index, expected, result) in [
        (0, "12:34:56", 45296.),
        (1, " 0:00:00", 0.6),
        (6, " 0:01:01", 6125.6),
        (15, " 2:00:00", 50400.1),
        (20, "12:34:56", 45296.),
    ] {
        if records[index]["text"] != expected
            || !records[index]["callback_result"]
                .as_f64()
                .is_some_and(|v| (v - result).abs() < 1e-9)
        {
            return Err(
                "Recorded session does not match the checked stopwatch/format/reset scenario"
                    .into(),
            );
        }
    }
    Ok(records[..30].to_vec())
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if !(3..=4).contains(&args.len())
        || (args.len() == 4 && args[3] != "--selection-check" && args[3] != "--local-session")
    {
        return Err("Usage: native-string-render-check <local FSX root> <new output directory> <clock NDJSON> [--selection-check | --local-session]".into());
    }
    let selection_check = args.len() == 4 && args[3] == "--selection-check";
    let local_session = args.len() == 4 && args[3] == "--local-session";
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
    let records = if local_session {
        session_samples(text, element.rva)?
    } else {
        samples(text, element.rva)?
    };
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
    let mut rasters = Vec::new();
    let mut selection_reports = Vec::new();
    let mut cases = if local_session {
        (0..records.len())
            .map(|index| (index, 0))
            .chain([(0, 0)])
            .collect()
    } else {
        vec![(0, 0), (1, 0), (2, 0), (0, 0)]
    };
    if selection_check {
        for (state, expected) in [(24, 0..2), (25, 3..5), (26, 6..8)] {
            if style.selected_range(state, 8)? != Some(expected) {
                return Err(
                    "Installed C172 clock selection table differs from the checked profile".into(),
                );
            }
            cases.push((1, state));
        }
        cases.extend([(1, 999), (1, 0)]);
    }
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    // Repeat the first state to detect stale glyphs/background accumulation.
    for (index, (sample_index, selection_state)) in cases.into_iter().enumerate() {
        let value = records[sample_index]["text"].as_str().unwrap();
        let raster = native_text::rasterize_selected(
            &font,
            style,
            element.image_flags,
            value,
            selection_state,
        )
        .map_err(|e| {
            format!(
                "{e}; flags={} offset={:?} style={style:?}",
                element.image_flags, element.offset
            )
        })?;
        if selection_check && (4..=6).contains(&index) {
            let baseline: &Vec<u8> = &rasters[1];
            let mut changed = 0;
            let mut min = [raster.image.width as usize, raster.image.height as usize];
            let mut max = [0, 0];
            for (i, (a, b)) in baseline
                .chunks_exact(4)
                .zip(raster.image.rgba.chunks_exact(4))
                .enumerate()
            {
                if a[3] != b[3] {
                    return Err("Highlighting changed glyph coverage/layout".into());
                }
                if a != b {
                    changed += 1;
                    let p = [
                        i % raster.image.width as usize,
                        i / raster.image.width as usize,
                    ];
                    for j in 0..2 {
                        min[j] = min[j].min(p[j]);
                        max[j] = max[j].max(p[j]);
                    }
                }
            }
            if changed == 0 {
                return Err("Selected clock digits did not change raster color".into());
            }
            selection_reports.push(json!({"state":selection_state,"changed_raster_pixels":changed,"min":min,"max":max,"alpha_identical":true}));
        }
        rasters.push(raster.image.rgba.clone());
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
            "capital_w_count_width":raster.capital_w_width,"drawn_placements":drawn,"selection_state":selection_state}),
        );
    }
    let header = b"P6\n960 720\n255\n";
    if snapshots
        .iter()
        .any(|s| !s.starts_with(header) || s.len() != header.len() + 960 * 720 * 3)
    {
        return Err("Unexpected GPU snapshot format".into());
    }
    if snapshots[0] != snapshots[if local_session { records.len() } else { 3 }] {
        return Err("Restoring the first clock state changed pixels".into());
    }
    let mut comparisons = Vec::new();
    let pairs: Vec<_> = if local_session {
        for (a, b) in [
            (4, 5),
            (6, 7),
            (7, 8),
            (1, 2),
            (1, 3),
            (1, 9),
            (0, 20),
            (20, 21),
            (1, 22),
            (22, 23),
            (24, 25),
        ] {
            if snapshots[a] != snapshots[b] {
                return Err(format!("Session pause/stop/reset pixels differ: {a}/{b}"));
            }
        }
        vec![(0, 1), (4, 6), (14, 15)]
    } else if selection_check {
        vec![(0, 1), (1, 2), (1, 4), (1, 5), (1, 6)]
    } else {
        vec![(0, 1), (1, 2)]
    };
    for (from, index) in pairs {
        let mut changed = 0;
        let mut min = [960, 720];
        let mut max = [0, 0];
        for (i, (a, b)) in snapshots[from][header.len()..]
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
            .push(json!({"from":from,"to":index,"changed_pixels":changed,"min":min,"max":max}));
    }
    if selection_check {
        if rasters[7] != rasters[1]
            || rasters[8] != rasters[1]
            || snapshots[7] != snapshots[1]
            || snapshots[8] != snapshots[1]
        {
            return Err("Unrelated/deselected clock state did not restore baseline pixels".into());
        }
        for pair in selection_reports.windows(2) {
            if pair[0]["max"][0].as_u64().unwrap() >= pair[1]["min"][0].as_u64().unwrap() {
                return Err("Hours/minutes/seconds highlight regions overlap".into());
            }
        }
    }
    let selection_table: Vec<_> = style
        .selections
        .iter()
        .map(|s| json!({"state":s.state,"first":s.first,"last":s.last}))
        .collect();
    let mut inventory = Vec::new();
    for (name, library) in &libraries {
        for gauge in &library.gauges {
            for e in &gauge.elements {
                if let Some(t) = &e.text {
                    inventory.push(json!({"library":name,"gauge":gauge.name,"element_rva":e.rva,"selection_records":t.selections.len()}));
                }
            }
        }
    }
    let report = json!({"schema":1,"profile":"C172 recorded native clock string",
        "font_path":font_path,"font_name":font.name(),"element_rva":element.rva,
        "position":element.position,"size":style.size,"max_length":style.max_length,
        "font_weight":style.font_weight,"font_height":style.font_height,"font_charset":style.font_charset,
        "draw_text_flags":style.draw_text_flags,"image_flags":element.image_flags,"colors":style.colors,
        "states":reports,"gpu_comparisons":comparisons,"restored_pixels_identical":true,
        "synthetic_time_inputs":true,"authored_style_only":true,"gdi_pixel_parity_verified":false,
        "recorded_local_session":local_session,"session_pause_stop_reset_pixels_verified":local_session,
        "selection_check":selection_check,"synthetic_selection_states":selection_check,
        "selection_sequence_rva":style.selection_sequence_rva,"selection_table":selection_table,
        "selection_rasters":selection_reports,"native_string_inventory":inventory,
        "selection_events_dispatched":false,"selection_controls_operational":false,
        "operational_clock":false,"full_gauge_lifecycle":false});
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!("PASS: {} recorded native strings -> matching Quartz font -> authored C172 placement -> distinct GPU pixels; restored state identical. Report: {}",records.len(), output.join("report.json").display());
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
    #[test]
    fn session_render_records_require_guards_exact_sequence_and_native_teardown() {
        let mut rows: Vec<Value> = (1..=30)
            .map(|sequence| {
                json!({"schema":1,"source":"native-clock-local-stream",
            "kind":"state","sequence":sequence,"element_rva":100,"pointer_bits":32,
            "library":"Cessna172.dll","gauge":"Clock","text":"12:34:56","callback_result":45296,
            "guarded_context":true,"guarded_string":true,"guarded_event_words":true,
            "panel_services":false,"multiplayer":false,"fsx_parity_verified":false})
            })
            .collect();
        for (index, text, result) in [
            (1, " 0:00:00", 0.6),
            (6, " 0:01:01", 6125.6),
            (15, " 2:00:00", 50400.1),
        ] {
            rows[index]["text"] = json!(text);
            rows[index]["callback_result"] = json!(result);
        }
        rows.push(
            json!({"schema":1,"source":"native-clock-local-stream","kind":"closed","sequence":30,
            "modules_deinitialized":4,"clock_disconnected":true,"string_buffer_restored":true,
            "import_pointer_restored":true,"context_guards_verified":true}),
        );
        let encode = |rows: &[Value]| rows.iter().map(|r| format!("{r}\n")).collect::<String>();
        let valid = encode(&rows);
        assert_eq!(session_samples(&valid, 100).unwrap().len(), 30);
        assert!(session_samples(&valid, 101).is_err());
        assert!(session_samples(&encode(&rows[..30]), 100).is_err());
        for (index, key) in [
            (0, "sequence"),
            (0, "guarded_context"),
            (0, "pointer_bits"),
            (6, "text"),
            (15, "callback_result"),
            (30, "modules_deinitialized"),
            (30, "import_pointer_restored"),
        ] {
            let mut bad = rows.clone();
            bad[index][key] = Value::Null;
            assert!(
                session_samples(&encode(&bad), 100).is_err(),
                "{index} {key}"
            );
        }
    }
}
