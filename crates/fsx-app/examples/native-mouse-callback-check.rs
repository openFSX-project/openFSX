//! Bounded isolated C172 mouse callback observations, including contained faults.
//! A completed probe is not proof that the button or full panel services work.
use fsx_vehicle::gauge::{GaugeLibrary, MouseKind};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn validate(
    row: &Value,
    status: i32,
    header: u32,
    mouse: u32,
    callback: u32,
    point: [i32; 2],
) -> Result<bool, String> {
    if row["schema"] != 1
        || !["native-mouse-probe", "native-mouse-local-event"]
            .iter()
            .any(|s| row["source"] == *s)
        || row["pointer_bits"] != 32
        || row["library"] != "Cessna172.dll"
        || row["gauge"] != "Clock"
        || row["header_rva"] != header
        || row["mouse_rva"] != mouse
        || row["callback_rva"] != callback
        || row["mouse_flags"] != 1u32 << 29
        || row["child_local_point"] != json!(point)
        || row["screen_point"] != json!([0, 0])
        || row["local_seconds"] != 45296
        || row["before_text"] != "12:34:56"
        || row["before_callback_result"] != 45296
    {
        return Err("Mouse probe metadata/input/baseline mismatch".into());
    }
    for key in [
        "synthetic_pointer_coordinates",
        "context_unchanged",
        "guarded_context",
        "guarded_string",
        "module_init_called",
        "module_deinit_called",
        "gauge_connection_notifications",
        "private_header_slot_restored",
        "string_buffer_restored",
    ] {
        if row[key] != true {
            return Err(format!("Missing mouse probe guarantee: {key}"));
        }
    }
    for key in ["panel_window", "panel_services", "full_gauge_lifecycle"] {
        if row[key] != false {
            return Err(format!("Unsupported mouse probe context: {key}"));
        }
    }
    let completed = row["mouse_callback_completed"]
        .as_bool()
        .ok_or("Missing native mouse completion result")?;
    if completed {
        if status != 0
            || row["native_exception"] != 0
            || row["mouse_callback_result"]
                .as_i64()
                .and_then(|v| i32::try_from(v).ok())
                .is_none()
            || !row["after_text"]
                .as_str()
                .is_some_and(|s| s.len() <= 8 && s.is_ascii() && !s.chars().any(char::is_control))
            || !row["after_callback_result"]
                .as_f64()
                .is_some_and(f64::is_finite)
        {
            return Err("Incomplete or inconsistent successful mouse observation".into());
        }
    } else if status != 2
        || !row["mouse_callback_result"].is_null()
        || !row["after_text"].is_null()
        || !row["after_callback_result"].is_null()
        || !row["native_exception"]
            .as_u64()
            .is_some_and(|n| n > 0 && n <= u32::MAX as u64)
        || row["fault_rva"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .is_none()
        || !row["access_operation"]
            .as_u64()
            .is_some_and(|n| [0, 1, 8].contains(&n))
        || row["access_target"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .is_none()
    {
        return Err("Incomplete or inconsistent contained mouse fault observation".into());
    }
    Ok(completed)
}

fn validate_local(row: &Value, packet: u32) -> Result<(), String> {
    for key in [
        "local_event_bridge",
        "process_event_delivered",
        "process_event_result",
        "guarded_event_words",
        "service_table_readonly",
        "import_pointer_restored",
    ] {
        if row[key] != true {
            return Err(format!("Missing local event guarantee: {key}"));
        }
    }
    let (text, result, lookups) = match packet {
        1 => (" 0:00:00", 0.6, 0),
        0 => ("12:34:56", 45296.1, 1),
        2 => ("12:34:56", 45296., 0),
        _ => return Err("Unexpected C172 mouse packet".into()),
    };
    if row["source"] != "native-mouse-local-event"
        || row["mouse_callback_completed"] != true
        || row["mouse_callback_result"] != 1
        || row["shared_event_count"] != 1
        || row["event_size"] != 4
        || row["original_packet_word"] != packet
        || row["lookup_count"] != lookups
        || row["lookup_token"] != 557
        || row["lookup_value"] != 45296
        || row["master_query_count"] != 0
        || row["after_text"] != text
        || !row["after_callback_result"]
            .as_f64()
            .is_some_and(|v| (v - result).abs() < 1e-9)
        || row["multiplayer"] != false
        || row["fsx_delivery_parity_verified"] != false
        || !["fault_rva", "access_operation", "access_target"]
            .iter()
            .all(|k| row[k].is_null())
    {
        return Err("Original local event packet/delivery/result mismatch".into());
    }
    Ok(())
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let local = args.len() == 4 && args[3] == "--local-events";
    if args.len() != 3 && !local {
        return Err(
            "Usage: native-mouse-callback-check <FSX root> <x86 helper.exe> <new output directory> [--local-events]"
                .into(),
        );
    }
    let root = PathBuf::from(&args[0])
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let helper = PathBuf::from(&args[1])
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let output = PathBuf::from(&args[2]);
    let library = GaugeLibrary::load(&root.join("Gauges/Cessna172.dll"), true)?;
    let gauge = library.gauge("Clock")?;
    let list = library
        .mouse_rects("Clock")?
        .ok_or("No C172 clock mouse metadata")?;
    let children: Vec<_> = list
        .records
        .iter()
        .filter(|r| r.kind == MouseKind::Child)
        .collect();
    if children.len() != 3 {
        return Err("Expected exactly three C172 clock mouse callbacks".into());
    }
    let mut rows = Vec::new();
    let mut completed = 0;
    for (index, r) in children.into_iter().enumerate() {
        let callback = r.callback_rva.ok_or("Missing native mouse callback")?;
        let size = [r.relative_box[2], r.relative_box[3]];
        if r.mouse_flags != 1 << 29 || r.event_id != 0 || size.iter().any(|&v| v <= 0 || v > 65536)
        {
            return Err("Unsupported clock mouse profile".into());
        }
        let point = [size[0] / 2, size[1] / 2];
        let mut command = Command::new(&helper);
        command
            .arg(if local {
                "--sample-clock-mouse-local"
            } else {
                "--sample-clock-mouse"
            })
            .arg(&root)
            .arg(r.rva.to_string())
            .arg("45296")
            .arg(point[0].to_string())
            .arg(point[1].to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                break status;
            }
            if start.elapsed() > Duration::from_secs(10) {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Native mouse probe exceeded parent timeout".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        child
            .stdout
            .take()
            .ok_or("Missing mouse probe stdout")?
            .take(16385)
            .read_to_end(&mut stdout)
            .map_err(|e| e.to_string())?;
        child
            .stderr
            .take()
            .ok_or("Missing mouse probe stderr")?
            .take(16385)
            .read_to_end(&mut stderr)
            .map_err(|e| e.to_string())?;
        if stdout.len() > 16384 || stderr.len() > 16384 || !stderr.is_empty() {
            return Err(format!(
                "Native mouse probe failed: {}",
                String::from_utf8_lossy(&stderr)
            ));
        }
        let row: Value = serde_json::from_slice(&stdout).map_err(|e| e.to_string())?;
        let did_complete = validate(
            &row,
            status.code().ok_or("Mouse helper exited without a code")?,
            gauge.rva,
            r.rva,
            callback,
            point,
        )?;
        if local {
            validate_local(&row, [1, 0, 2][index])?;
        } else if row["source"] != "native-mouse-probe" {
            return Err("Unexpected mouse probe source".into());
        }
        completed += usize::from(did_complete);
        println!(
            "{} mouse RVA {} callback {}: {}",
            if did_complete {
                "OBSERVED"
            } else {
                "CONTAINED FAULT"
            },
            r.rva,
            callback,
            row["native_exception"]
        );
        rows.push(row);
    }
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    fs::write(
        output.join("samples.ndjson"),
        rows.iter().map(|r| format!("{r}\n")).collect::<String>(),
    )
    .map_err(|e| e.to_string())?;
    fs::write(output.join("report.json"),serde_json::to_vec_pretty(&json!({"schema":1,"observations":rows.len(),"callbacks_completed":completed,
        "contained_native_faults":rows.len()-completed,"headless_context":true,"all_modules_deinitialized":true,
        "panel_services":false,"local_event_bridge":local,"original_events_delivered":if local {3}else{0},
        "operational_buttons":false,"fsx_input_parity_verified":false})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    println!("CHECKED: {completed}/3 callbacks completed; {} contained native faults. This is diagnostic evidence, not operational button support.",3-completed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        json!({"schema":1,"source":"native-mouse-probe","pointer_bits":32,"library":"Cessna172.dll","gauge":"Clock",
        "header_rva":10,"mouse_rva":20,"callback_rva":30,"mouse_flags":1u32<<29,"child_local_point":[3,3],"screen_point":[0,0],
        "synthetic_pointer_coordinates":true,"local_seconds":45296,"before_text":"12:34:56","before_callback_result":45296,
        "context_unchanged":true,"guarded_context":true,"guarded_string":true,"module_init_called":true,"module_deinit_called":true,
        "gauge_connection_notifications":true,"private_header_slot_restored":true,"string_buffer_restored":true,"panel_window":false,
        "panel_services":false,"full_gauge_lifecycle":false,"mouse_callback_completed":false,"mouse_callback_result":null,
        "native_exception":0xc0000005u32,"fault_rva":32,"access_operation":0,"access_target":336,"after_text":null,"after_callback_result":null})
    }
    #[test]
    fn mouse_observations_distinguish_completed_calls_from_contained_faults() {
        let fault = fixture();
        assert!(!validate(&fault, 2, 10, 20, 30, [3, 3]).unwrap());
        assert!(validate(&fault, 0, 10, 20, 30, [3, 3]).is_err());
        let mut done = fault;
        done["mouse_callback_completed"] = json!(true);
        done["mouse_callback_result"] = json!(1);
        done["native_exception"] = json!(0);
        done["after_text"] = json!("12:34:56");
        done["after_callback_result"] = json!(45296);
        assert!(validate(&done, 0, 10, 20, 30, [3, 3]).unwrap());
        assert!(validate(&done, 2, 10, 20, 30, [3, 3]).is_err());
    }
    #[test]
    fn mouse_observations_require_context_guards_cleanup_and_exact_metadata() {
        let v = fixture();
        for key in [
            "header_rva",
            "mouse_rva",
            "callback_rva",
            "mouse_flags",
            "child_local_point",
            "screen_point",
            "before_text",
            "before_callback_result",
            "guarded_context",
            "guarded_string",
            "context_unchanged",
            "module_deinit_called",
            "panel_services",
            "private_header_slot_restored",
            "string_buffer_restored",
            "native_exception",
            "fault_rva",
            "access_operation",
            "access_target",
        ] {
            let mut bad = v.clone();
            bad[key] = Value::Null;
            assert!(validate(&bad, 2, 10, 20, 30, [3, 3]).is_err(), "{key}");
        }
    }
    #[test]
    fn local_delivery_requires_original_packet_acknowledgment_lookup_and_cleanup() {
        let mut row = fixture();
        let fields = json!({"source":"native-mouse-local-event", "mouse_callback_completed":true,
            "mouse_callback_result":1,"native_exception":0,"fault_rva":null,"access_operation":null,"access_target":null,
            "after_text":"12:34:56","after_callback_result":45296.1,"local_event_bridge":true,
            "process_event_delivered":true,"process_event_result":true,"guarded_event_words":true,
            "service_table_readonly":true,"import_pointer_restored":true,"shared_event_count":1,
            "event_size":4,"original_packet_word":0,"lookup_count":1,"lookup_token":557,
            "lookup_value":45296,"master_query_count":0,"multiplayer":false,"fsx_delivery_parity_verified":false});
        row.as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        assert!(validate(&row, 0, 10, 20, 30, [3, 3]).unwrap());
        validate_local(&row, 0).unwrap();
        assert!(validate_local(&row, 1).is_err());
        for key in [
            "source",
            "shared_event_count",
            "event_size",
            "original_packet_word",
            "lookup_count",
            "lookup_token",
            "lookup_value",
            "process_event_delivered",
            "process_event_result",
            "guarded_event_words",
            "service_table_readonly",
            "import_pointer_restored",
            "multiplayer",
            "fsx_delivery_parity_verified",
            "after_text",
            "after_callback_result",
        ] {
            let mut bad = row.clone();
            bad[key] = Value::Null;
            assert!(validate_local(&bad, 0).is_err(), "{key}");
        }
    }
}
