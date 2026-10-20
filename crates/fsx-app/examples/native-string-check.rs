//! Resolve an installed native string by metadata and check original callback
//! output for explicit synthetic BCD slots. No FSX process or operational radio.
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
    if args.len() != 3 {
        return Err(
            "Usage: native-string-check <FSX root> <x86 helper.exe> <new output directory>".into(),
        );
    }
    let root = PathBuf::from(&args[0])
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let helper = PathBuf::from(&args[1])
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let output = PathBuf::from(&args[2]);
    let library = GaugeLibrary::load(&root.join("Gauges/Bendix_king_radio.dll"), true)?;
    let gauge = library.gauge("Bendix-King Radio Nav-Comm 1")?;
    let mut matching = gauge
        .elements
        .iter()
        .filter(|e| e.text.as_ref().is_some_and(|t| t.tokens == [23, 617, 0]));
    let element = matching.next().ok_or("No checked Nav-Comm string tokens")?;
    if matching.next().is_some() {
        return Err("Ambiguous Nav-Comm string binding".into());
    }
    let descriptor = element.text.as_ref().unwrap();
    if descriptor.source_callbacks.iter().any(Option::is_some)
        || descriptor.update_callback_rva.is_none()
        || descriptor.max_length != 13
    {
        return Err("Unsupported installed string profile".into());
    }
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    for (first, second, expected) in [
        (0x1825u32, 0x2050u32, "118.25 120.50"),
        (0x2250, 0x2700, "122.50 127.00"),
        (0x1985, 0x2430, "119.85 124.30"),
    ] {
        let mut command = Command::new(&helper);
        command
            .arg("--sample-string")
            .arg(&root)
            .arg("Bendix_king_radio.dll")
            .arg("Bendix-King Radio Nav-Comm 1")
            .arg(format!("0x{:x}", element.rva))
            .arg(format!("u32:{first}"))
            .arg(format!("u32:{second}"))
            .arg("f64:0")
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
            || row["library"] != "Bendix_king_radio.dll"
            || row["gauge"] != gauge.name
            || row["element_rva"] != element.rva
            || row["tokens"] != json!([23, 617, 0])
            || row["input_kinds"] != json!(["u32", "u32", "f64"])
            || row["raw_inputs"] != json!([first, second, 0])
            || row["max_length"] != descriptor.max_length
            || row["text"] != expected
            || row["guarded_buffer"] != true
            || row["module_init_called"] != true
            || row["module_deinit_called"] != true
            || row["panel_services"] != false
            || row["full_gauge_lifecycle"] != false
            || !row["callback_result"].as_f64().is_some_and(f64::is_finite)
        {
            return Err(format!("Native string metadata/echo/text mismatch: {row}"));
        }
        println!("PASS {}", row["text"]);
        rows.push(row);
    }
    let ndjson = rows.iter().map(|r| format!("{r}\n")).collect::<String>();
    fs::write(output.join("samples.ndjson"), ndjson).map_err(|e| e.to_string())?;
    fs::write(output.join("report.json"),json!({"profile":"Nav-Comm 1 explicit synthetic BCD slots","samples":rows.len(),"element_rva":element.rva,"tokens":descriptor.tokens,
        "font_name":descriptor.font_name,"font_height":descriptor.font_height,"max_length":descriptor.max_length,
        "fsx_parity_validated":false,"rendered":false,"operational_radio":false}).to_string()).map_err(|e|e.to_string())?;
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
