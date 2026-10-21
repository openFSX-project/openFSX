//! Read-only installed mouse record inventory; no DLL loading or event execution.
use fsx_vehicle::gauge::GaugeLibrary;
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};
fn clock_hits(root: &Path) -> Result<serde_json::Value, String> {
    use fsx_vehicle::{
        aircraft::VisualAircraft,
        panel::{Kind, PanelConfig},
    };
    let aircraft = VisualAircraft::load(&root.join("SimObjects/Airplanes/C172"), 0, true)?;
    let config = PanelConfig::load(&aircraft.panel, true)?;
    let mut placements = Vec::new();
    for panel in &config.panels {
        if matches!(panel.kind, Kind::VirtualCockpit { .. }) {
            for g in &panel.gauges {
                if g.module.eq_ignore_ascii_case("Cessna172")
                    && g.name.eq_ignore_ascii_case("Clock")
                {
                    placements.push((panel, g));
                }
            }
        }
    }
    if placements.len() != 1 {
        return Err("Expected one C172 VC clock placement".into());
    }
    let (p, g) = placements[0];
    let [Some(mw), Some(mh)] = p.size_mm.ok_or("No panel size")? else {
        return Err("No panel mm size".into());
    };
    let [Some(pw), Some(ph)] = p.pixel_size.ok_or("No panel pixels")? else {
        return Err("No panel pixel size".into());
    };
    let [Some(gw), Some(gh)] = g.size else {
        return Err("No clock placement size".into());
    };
    let origin = [g.position[0] / mw * pw, g.position[1] / mh * ph];
    let size = [gw / mw * pw, gh / mh * ph];
    let library = GaugeLibrary::load(&root.join("Gauges/Cessna172.dll"), true)?;
    let center = fsx_render::panel::native_mouse_point(
        p,
        g,
        &library,
        1033,
        [origin[0] + size[0] * 0.5, origin[1] + size[1] * 0.5],
    )?
    .ok_or("Clock placement center mapped outside the gauge")?;
    let list = library
        .mouse_rects("Clock")?
        .ok_or("No native clock mouse list")?;
    let regions = list.regions(center.native_size)?;
    if regions.len() != 3 {
        return Err("Expected three authored native clock child regions".into());
    }
    let mut checks = Vec::new();
    for region in regions {
        let r = region.rectangle;
        let native = [r[0] + r[2] * 0.5, r[1] + r[3] * 0.5];
        let pixel = [
            origin[0] + native[0] / center.native_size[0] as f64 * size[0],
            origin[1] + native[1] / center.native_size[1] as f64 * size[1],
        ];
        let mapped = fsx_render::panel::native_mouse_point(p, g, &library, 1033, pixel)?
            .ok_or("Native button center mapped outside the gauge")?;
        if mapped
            .point
            .iter()
            .zip(native)
            .any(|(a, b)| (a - b).abs() > 1e-9)
        {
            return Err("Panel-to-native mouse round trip changed coordinates".into());
        }
        let hits = list.hit_candidates(mapped.native_size, mapped.point, 1 << 29)?;
        if hits.len() != 1 || hits[0].record_index != region.record_index {
            return Err("Clock button center did not resolve uniquely to its native child".into());
        }
        let record = &list.records[region.record_index];
        if record.callback_rva.is_none() || record.event_id != 0 || record.cursor != 5 {
            return Err("Unsupported native clock mouse action/cursor profile".into());
        }
        if !list
            .hit_candidates(mapped.native_size, mapped.point, 1 << 31)?
            .is_empty()
            || list
                .hit_candidates(mapped.native_size, [r[0] + r[2], native[1]], 1 << 29)?
                .iter()
                .any(|h| h.record_index == region.record_index)
            || list
                .hit_candidates(mapped.native_size, [native[0], r[1] + r[3]], 1 << 29)?
                .iter()
                .any(|h| h.record_index == region.record_index)
        {
            return Err("Native clock hit flags or exclusive right/bottom edges failed".into());
        }
        checks.push(json!({"child_rva":record.rva,"callback_rva":record.callback_rva,"native_rectangle":r,"panel_pixel":pixel,
            "mapped_point":mapped.point,"child_local_point":hits[0].child_local_point,"right_button_ignored":true,"right_bottom_edges_excluded":true}));
    }
    if !list
        .hit_candidates(center.native_size, [27., 29.], 1 << 29)?
        .is_empty()
    {
        return Err("Clock display/help region generated an actionable child hit".into());
    }
    if fsx_render::panel::native_mouse_point(p, g, &library, 1033, [-1., 0.])?.is_some() {
        return Err("Point outside the VC texture generated a native hit".into());
    }
    Ok(
        json!({"native_size":center.native_size,"checks":checks,"display_help_area_not_actionable":true,"outside_texture_ignored":true,
        "screen_ray_picking":false,"callbacks_executed":false}),
    )
}
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("Usage: native-mouse-check <FSX root> <new output directory>".into());
    }
    let root = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let mut reports = Vec::new();
    for name in [
        "Cessna.dll",
        "Cessna172.dll",
        "Cessna182s.dll",
        "Bell_206B.dll",
        "Bendix_king_radio.dll",
    ] {
        let library = GaugeLibrary::load(&root.join("Gauges").join(name), true)?;
        for gauge in &library.gauges {
            let list = library
                .mouse_rects(&gauge.name)
                .map_err(|e| format!("{name}!{}: {e}", gauge.name))?;
            let records:Vec<_>=list.as_ref().map(|l|l.records.iter().map(|r|json!({"rva":r.rva,"kind":format!("{:?}",r.kind),
                "parent":r.parent,"relative_box":r.relative_box,"cursor":r.cursor,"help_id":r.help_id,"mouse_flags":r.mouse_flags,
                "event_id_or_arg_count":r.event_id,"callback_rva":r.callback_rva,"raw_function_slot":r.raw_function_slot,"tooltip_text_rva":r.tooltip_text_rva,"opaque_api_data":r.api_data})).collect()).unwrap_or_default();
            reports.push(json!({"library":name,"gauge":gauge.name,"header_rva":gauge.rva,"version":gauge.version,
                "mouse_list_rva":list.as_ref().map(|l|l.rva),"terminator_rva":list.as_ref().map(|l|l.terminator_rva),"records":records}));
        }
    }
    let hit_checks = clock_hits(&root)?;
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    fs::write(output.join("report.json"),serde_json::to_vec_pretty(&json!({"schema":1,"gauges":reports,
            "clock_hit_checks":hit_checks,"fsx_hit_parity_verified":false,"source":"installed PE metadata","dll_loaded":false,"callbacks_executed":false,"events_dispatched":false})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    println!(
        "PASS: five installed libraries' mouse records decoded read-only; report {}",
        output.join("report.json").display()
    );
    Ok(())
}
