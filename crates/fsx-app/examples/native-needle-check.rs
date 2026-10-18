//! Offscreen native callback-result -> authored needle artwork integration.
//! Consumes an explicit recorded probe file; never starts FSX or an OS window.
use fsx_model::Model;
use fsx_render::{block_on, panel, Appearance, Camera, Gpu, Orbit};
use fsx_vehicle::{
    aircraft::VisualAircraft,
    gauge::GaugeLibrary,
    panel::{Kind, PanelConfig},
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
struct Sample {
    input: f64,
    result: f64,
    token: u32,
    version: u32,
}
type Key = (String, String, i32);

fn samples(path: &Path) -> Result<BTreeMap<Key, Vec<Sample>>, String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|f| f.take(1024 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("Native sample file exceeds 1 MiB".into());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "Native sample file is not UTF-8")?;
    parse_samples(text)
}
fn parse_samples(text: &str) -> Result<BTreeMap<Key, Vec<Sample>>, String> {
    let mut result = BTreeMap::<Key, Vec<Sample>>::new();
    for (i, line) in text.lines().enumerate() {
        if i >= 128 || line.len() > 16384 {
            return Err("Native sample record budget exceeded".into());
        }
        let v: Value =
            serde_json::from_str(line).map_err(|e| format!("Native sample line {}: {e}", i + 1))?;
        if v["schema"] != 1
            || v["source"] != "native-gauge-probe"
            || v["pointer_bits"] != 32
            || v["panel_services"] != false
        {
            return Err("Unsupported native probe schema/source/context".into());
        }
        if v["mode"] == "probe" {
            continue;
        }
        if v["mode"] != "sample"
            || v["module_init_called"] != true
            || v["module_deinit_called"] != true
        {
            return Err("Incomplete native sample lifecycle".into());
        }
        let name = |key: &str| {
            v[key]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 256 && s.is_ascii())
                .map(str::to_ascii_lowercase)
                .ok_or("Invalid sample name")
        };
        let module = name("library")?;
        let module = module
            .strip_suffix(".dll")
            .ok_or("Expected a native DLL sample")?
            .to_owned();
        let gauge = name("gauge")?;
        let integer = |key: &str| {
            v["sample"][key]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or("Invalid native sample integer")
        };
        let resource =
            i32::try_from(integer("resource")?).map_err(|_| "Invalid native sample resource ID")?;
        let number = |key: &str| {
            v["sample"][key]
                .as_f64()
                .filter(|n| n.is_finite() && n.abs() <= 1e12)
                .ok_or("Invalid native sample numeric value")
        };
        let sample = Sample {
            input: number("raw_input")?,
            result: number("callback_result")?,
            token: integer("token")?,
            version: v["version"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or("Invalid native header version")?,
        };
        let key = (module, gauge, resource);
        if !matches!(
            (key.0.as_str(), key.1.as_str(), key.2),
            ("cessna172", "airspeed", 1001)
                | ("cessna172", "tachometer", 5023)
                | ("cessna", "vertical_speed", 5047)
                | ("cessna", "altimeter", 1031..=1033)
        ) {
            return Err(
                "This check requires the six explicitly supported C172 needle bindings".into(),
            );
        }
        let group = result.entry(key).or_default();
        if group.len() >= 3 {
            return Err("Too many samples for an inspection needle".into());
        }
        group.push(sample);
    }
    if result.len() != 6
        || result
            .values()
            .any(|v| v.len() != 3 || v.iter().all(|s| s.result == v[0].result))
    {
        return Err("Expected three native samples for each of six needles".into());
    }
    Ok(result)
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("Usage: native-needle-check <local FSX root> <new output directory> <native probe NDJSON>".into());
    }
    let root = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let samples = samples(Path::new(&args[2]))?;
    let aircraft = VisualAircraft::load(&root.join("SimObjects/Airplanes/C172"), 0, true)?;
    let config = PanelConfig::load(&aircraft.panel, true)?;
    let mut libraries = BTreeMap::new();
    for name in [
        "Cessna.dll",
        "Cessna172.dll",
        "Cessna182s.dll",
        "Bell_206B.dll",
        "Bendix_king_radio.dll",
    ] {
        libraries.insert(
            name.strip_suffix(".dll").unwrap().to_ascii_lowercase(),
            GaugeLibrary::load(&root.join("Gauges").join(name), true)?,
        );
    }
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
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    for index in 0..3 {
        let mut appearance = Appearance::load(&model, &geometry, Some(&aircraft.texture))?;
        let mut drawn = 0;
        for p in &config.panels {
            let Kind::VirtualCockpit { texture } = &p.kind else {
                continue;
            };
            let mut preview = panel::static_roots(p, &libraries, 1033)?;
            for placement in &p.gauges {
                let module = placement.module.to_ascii_lowercase();
                let Some(library) = libraries.get(&module) else {
                    continue;
                };
                let gauge = library.gauge(&placement.name)?;
                // Preserve the authored graph order for overlapping altimeter needles.
                for element in &gauge.elements {
                    let Some(needle) = &element.needle else {
                        continue;
                    };
                    let key = (
                        module.clone(),
                        placement.name.to_ascii_lowercase(),
                        element.resource_id,
                    );
                    let Some(values) = samples.get(&key) else {
                        continue;
                    };
                    let value = &values[index];
                    if value.token != needle.token
                        || value.version != gauge.version
                        || needle.update_callback_rva.is_none()
                    {
                        return Err("Native sample and loaded needle metadata do not match".into());
                    }
                    let angle = if needle.curve.is_empty() {
                        // Explicit direct-degree interpretation only for these checked altimeter callbacks.
                        if key.0 != "cessna" || key.1 != "altimeter" {
                            return Err(
                                "Unverified table-free native callback interpretation".into()
                            );
                        }
                        value.result.to_radians()
                    } else {
                        needle.reading_angle(element.position, value.result)?
                    };
                    println!("Sample {index}: {}!{} resource {} token {} | raw {} -> native {} -> {:.6} degrees | POS {:?} OFS {:?}",placement.module,placement.name,element.resource_id,needle.token,value.input,value.result,angle.to_degrees(),element.position,element.offset);
                    panel::paint_needle(
                        &mut preview,
                        p,
                        placement,
                        library,
                        element.resource_id,
                        1033,
                        angle,
                    )
                    .map_err(|e| {
                        format!(
                            "{e}; predecessor element kinds: {:?}",
                            gauge
                                .elements
                                .iter()
                                .filter(|p| p.children.contains(&element.rva))
                                .map(|p| (p.kind, p.resource_id))
                                .collect::<Vec<_>>()
                        )
                    })?;
                    drawn += 1;
                }
            }
            appearance.bind_image(texture, preview.image)?;
        }
        if drawn != 6 {
            return Err("Not all six native sample needles were rendered".into());
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
        gpu.as_ref().unwrap().snapshot(
            &orbit,
            &output.join(format!("c172-native-needle-{index}.ppm")),
        )?;
    }
    let first = fs::read(output.join("c172-native-needle-1.ppm")).map_err(|e| e.to_string())?;
    let second = fs::read(output.join("c172-native-needle-2.ppm")).map_err(|e| e.to_string())?;
    let header = b"P6\n960 720\n255\n";
    if !first.starts_with(header)
        || !second.starts_with(header)
        || first.len() != header.len() + 960 * 720 * 3
        || second.len() != first.len()
    {
        return Err("Unexpected needle snapshot layout".into());
    }
    let mut changed = 0;
    let mut min = [960, 720];
    let mut max = [0, 0];
    for (i, (a, b)) in first[header.len()..]
        .chunks_exact(3)
        .zip(second[header.len()..].chunks_exact(3))
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
        return Err("Distinct native inspection samples produced identical final pixels".into());
    }
    println!("GPU comparison states 1 -> 2: {changed} changed pixels within {min:?}..{max:?}");
    println!("PASS: 18 recorded native update results -> authored needle pivots/tables -> three GPU renders. Explicit inspection values, no temporal lag, FSX measurements or flight integration. Other instrument layers remain omitted.");
    Ok(())
}
fn main() -> Result<(), String> {
    run()
}

#[cfg(test)]
mod tests {
    use super::*;
    // Original synthetic protocol records, not captured/native callback results.
    fn fixture() -> String {
        let mut lines = Vec::new();
        for (module, gauge, id) in [
            ("Cessna172.dll", "Airspeed", 1001),
            ("Cessna172.dll", "Tachometer", 5023),
            ("Cessna.dll", "Vertical_Speed", 5047),
            ("Cessna.dll", "Altimeter", 1031),
            ("Cessna.dll", "Altimeter", 1032),
            ("Cessna.dll", "Altimeter", 1033),
        ] {
            for n in 0..3 {
                lines.push(serde_json::json!({"schema":1,"source":"native-gauge-probe","mode":"sample","pointer_bits":32,
                    "panel_services":false,"module_init_called":true,"module_deinit_called":true,"library":module,"gauge":gauge,"version":800,
                    "sample":{"resource":id,"token":0,"raw_input":n,"callback_result":n}}).to_string());
            }
        }
        lines.join("\n")
    }
    #[test]
    fn strict_protocol_groups_and_input_order() {
        let records = parse_samples(&fixture()).unwrap();
        let group = &records[&("cessna172".into(), "airspeed".into(), 1001)];
        assert_eq!(records.len(), 6);
        assert_eq!(
            group.iter().map(|s| s.input).collect::<Vec<_>>(),
            [0., 1., 2.]
        );
        assert_eq!(group[2].result, 2.);
    }
    #[test]
    fn incomplete_unmatched_and_unbounded_samples_are_errors() {
        let data = fixture();
        for (from, to) in [
            ("\"schema\":1", "\"schema\":2"),
            ("\"pointer_bits\":32", "\"pointer_bits\":64"),
            (
                "\"module_deinit_called\":true",
                "\"module_deinit_called\":false",
            ),
            ("\"panel_services\":false", "\"panel_services\":true"),
            ("\"callback_result\":2", "\"callback_result\":1e99"),
            ("\"Airspeed\"", "\"Unknown\""),
        ] {
            assert!(parse_samples(&data.replace(from, to)).is_err(), "{from}");
        }
        let mut lines: Vec<_> = data.lines().collect();
        lines.pop();
        assert!(parse_samples(&lines.join("\n")).is_err());
        assert!(parse_samples(&(data.clone() + "\n" + data.lines().next().unwrap())).is_err());
        assert!(parse_samples(&" ".repeat(16385)).is_err());
    }
}
