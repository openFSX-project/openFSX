//! Resolve an installed native string by metadata and check original callback
//! output for explicit BCD, connected time or OAT/voltage slots. No FSX process.
use fsx_vehicle::gauge::GaugeLibrary;
use serde_json::{json, Value};
use std::{
    env, fs,
    io::Read,
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
    time::{Duration, Instant},
};

fn run() -> Result<(), String> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 3 && !(args.len() == 4 && (args[3] == "--clock" || args[3] == "--oat")) {
        return Err(
            "Usage: native-string-check <FSX root> <x86 helper.exe> <new output directory> [--clock | --oat]".into(),
        );
    }
    let root = PathBuf::from(&args[0])
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let helper = PathBuf::from(&args[1])
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let output = PathBuf::from(&args[2]);
    let connected = args.len() == 4;
    let oat = connected && args[3] == "--oat";
    let (library_name, gauge_name, tokens, maximum) = if oat {
        ("Cessna172.dll", "Clock", [341, 335, 753], 7)
    } else if connected {
        ("Cessna172.dll", "Clock", [3, 4, 5], 8)
    } else {
        (
            "Bendix_king_radio.dll",
            "Bendix-King Radio Nav-Comm 1",
            [23, 617, 0],
            13,
        )
    };
    let library = GaugeLibrary::load(&root.join("Gauges").join(library_name), true)?;
    let gauge = library.gauge(gauge_name)?;
    let mut matching = gauge
        .elements
        .iter()
        .filter(|e| e.text.as_ref().is_some_and(|t| t.tokens == tokens));
    let element = matching.next().ok_or("No checked native string tokens")?;
    if matching.next().is_some() {
        return Err("Ambiguous native string binding".into());
    }
    let descriptor = element.text.as_ref().unwrap();
    if descriptor.source_callbacks.iter().any(Option::is_some)
        || descriptor.update_callback_rva.is_none()
        || descriptor.max_length != maximum
    {
        return Err("Unsupported installed string profile".into());
    }
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    let mut cases: Vec<(f64, f64, f64, &str, Option<f64>)> = if oat {
        vec![
            (15., 0., 28., " 59° F", Some(59.)),
            (-40., 1., 28., "-40° C", Some(-40.)),
            (15., 2., 0., " 15° C", Some(15.)),
            (37.8, 0., 24., "100° F", Some(100.04)),
            (-273.15, 1., 28., "-99° C", Some(-99.)),
            (1000., 0., 28., "999° F", Some(999.)),
        ]
    } else if connected {
        vec![
            (0., 0., 0., "00:00:00", Some(0.)),
            (12., 34., 56., "12:34:56", Some(45296.)),
            (23., 59., 59., "23:59:59", Some(86399.)),
        ]
    } else {
        vec![
            (0x1825 as f64, 0x2050 as f64, 0., "118.25 120.50", None),
            (0x2250 as f64, 0x2700 as f64, 0., "122.50 127.00", None),
            (0x1985 as f64, 0x2430 as f64, 0., "119.85 124.30", None),
        ]
    };
    let default_cases = cases.len();
    if oat {
        cases.extend([
            (15., 0., 28., " 28.0 V", Some(28.)),
            (15., 0., 0., " 0.0 V", Some(0.)),
            (15., 0., 99.9, " 99.9 V", Some(99.9)),
        ]);
    }
    let kinds = if connected {
        ["f64", "f64", "f64"]
    } else {
        ["u32", "u32", "f64"]
    };
    for (index, (first, second, third, expected, expected_result)) in cases.into_iter().enumerate()
    {
        let volts = oat && index >= default_cases;
        let mut command = Command::new(&helper);
        command
            .arg(if volts {
                "--sample-string-connected-volts"
            } else if connected {
                "--sample-string-connected"
            } else {
                "--sample-string"
            })
            .arg(&root)
            .arg(library_name)
            .arg(gauge_name)
            .arg(format!("0x{:x}", element.rva))
            .arg(format!("{}:{first}", kinds[0]))
            .arg(format!("{}:{second}", kinds[1]))
            .arg(format!("{}:{third}", kinds[2]))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
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
                return Err("Native string helper exceeded parent timeout".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        child
            .stdout
            .take()
            .ok_or("Missing helper output")?
            .take(16385)
            .read_to_end(&mut stdout)
            .map_err(|e| e.to_string())?;
        child
            .stderr
            .take()
            .ok_or("Missing helper errors")?
            .take(16385)
            .read_to_end(&mut stderr)
            .map_err(|e| e.to_string())?;
        if !status.success() || stdout.len() > 16384 || stderr.len() > 16384 {
            return Err(format!(
                "Native string sample failed: {}",
                String::from_utf8_lossy(&stderr)
            ));
        }
        let row: Value = serde_json::from_slice(&stdout).map_err(|e| e.to_string())?;
        if row["schema"] != 1
            || row["source"] != "native-string-probe"
            || row["pointer_bits"] != 32
            || row["library"] != library_name
            || row["gauge"] != gauge.name
            || row["element_rva"] != element.rva
            || row["tokens"] != json!(tokens)
            || row["input_kinds"] != json!(kinds)
            || !row["raw_inputs"].as_array().is_some_and(|a| {
                a.len() == 3
                    && a.iter()
                        .zip([first, second, third])
                        .all(|(v, n)| v.as_f64() == Some(n))
            })
            || row["max_length"] != descriptor.max_length
            || row["text"] != expected
            || row["guarded_buffer"] != true
            || row["module_init_called"] != true
            || row["module_deinit_called"] != true
            || row["panel_services"] != false
            || row["full_gauge_lifecycle"] != false
            || row["gauge_connection_notifications"] != connected
            || row["private_state_created_by_original"] != connected
            || row["panel_window"] != false
            || row["text_encoding"] != if oat { "windows-1252" } else { "ascii" }
            || row["string_buffer_restored"] != true
            || row["mouse_event_dispatched"] != volts
            || row["local_event_bridge"] != volts
            || (oat && row["secondary_mode"] != if volts { "volts" } else { "oat" })
            || (volts
                && (row["original_packet_word"] != 2
                    || row["mouse_context_verified"] != true
                    || row["import_pointer_restored"] != true))
            || expected_result.is_some_and(|expected| {
                !row["callback_result"]
                    .as_f64()
                    .is_some_and(|n| (n - expected).abs() < 1e-9)
            })
            || !row["callback_result"].as_f64().is_some_and(f64::is_finite)
        {
            return Err(format!("Native string metadata/echo/text mismatch: {row}"));
        }
        println!("PASS {}", row["text"]);
        rows.push(row);
    }
    let ndjson = rows.iter().map(|r| format!("{r}\n")).collect::<String>();
    fs::write(output.join("samples.ndjson"), ndjson).map_err(|e| e.to_string())?;
    fs::write(output.join("report.json"),json!({"profile":if oat {"C172 connected OAT with explicit synthetic ambient temperature, unit enum and voltage"}else if connected {"C172 Clock headless original connect/disconnect with synthetic local time"}else{"Nav-Comm 1 explicit synthetic BCD slots"},"samples":rows.len(),"element_rva":element.rva,"tokens":descriptor.tokens,
        "font_name":descriptor.font_name,"font_height":descriptor.font_height,"max_length":descriptor.max_length,
        "fsx_parity_validated":false,"rendered":false,"operational_radio":false,"operational_clock":false,"full_gauge_lifecycle":false}).to_string()).map_err(|e|e.to_string())?;
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
