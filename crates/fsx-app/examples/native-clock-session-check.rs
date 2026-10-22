//! Retained original C172 clock state, explicit local tokens and native mouse packets.
use fsx_vehicle::gauge::{GaugeLibrary, MouseKind};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
const WAIT: Duration = Duration::from_millis(5500);
fn read_line(reader: &mut impl BufRead) -> Result<String, String> {
    let mut bytes = Vec::new();
    reader
        .take(4097)
        .read_until(b'\n', &mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.is_empty() {
        return Err("EOF".into());
    }
    if bytes.len() > 4096 || bytes.last() != Some(&b'\n') {
        return Err("Unbounded or incomplete clock response".into());
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}
fn packet(line: &str, kind: &str) -> Result<Value, String> {
    let row: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
    if row["schema"] != 1 || row["source"] != "native-clock-local-stream" || row["kind"] != kind {
        return Err("Unexpected clock stream protocol".into());
    }
    Ok(row)
}
struct Process {
    child: Child,
    input: Option<ChildStdin>,
    rx: Option<Receiver<Result<String, String>>>,
    reader: Option<JoinHandle<()>>,
}
impl Process {
    fn open(helper: &Path, root: &Path) -> Result<Self, String> {
        let mut command = Command::new(helper);
        command
            .args(["--clock-local-frames"])
            .arg(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        let input = child.stdin.take();
        let output = child.stdout.take().ok_or("Missing clock stdout")?;
        let (tx, rx) = mpsc::sync_channel(2);
        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let line = read_line(&mut reader);
                let terminal = line.is_err();
                if tx.send(line).is_err() || terminal {
                    break;
                }
            }
        });
        let mut process = Self {
            child,
            input,
            rx: Some(rx),
            reader: Some(reader),
        };
        let ready = packet(&process.receive()?, "ready")?;
        if ready["pid"] != process.child.id()
            || ready["pointer_bits"] != 32
            || ready["local_event_bridge"] != true
            || ready["string_bindings"] != 1
            || ready["mouse_children"] != 3
            || ready["panel_services"] != false
            || ready["multiplayer"] != false
            || ready["fsx_parity_verified"] != false
        {
            return Err("Clock capabilities/PID mismatch".into());
        }
        Ok(process)
    }
    fn receive(&mut self) -> Result<String, String> {
        self.rx
            .as_ref()
            .ok_or("Clock responses closed")?
            .recv_timeout(WAIT)
            .map_err(|e| format!("Clock helper response timeout/failure: {e}"))?
    }
    fn send(&mut self, line: &str) -> Result<(), String> {
        let input = self.input.as_mut().ok_or("Clock input closed")?;
        writeln!(input, "{line}")
            .and_then(|_| input.flush())
            .map_err(|e| e.to_string())
    }
    fn finish(&mut self, sequence: usize, generation: u32) -> Result<Value, String> {
        self.send("quit")?;
        let row = packet(&self.receive()?, "closed")?;
        if row["sequence"] != sequence || row["modules_deinitialized"] != generation {
            return Err("Clock teardown count mismatch".into());
        }
        for key in [
            "clock_disconnected",
            "string_buffer_restored",
            "import_pointer_restored",
            "context_guards_verified",
        ] {
            if row[key] != true {
                return Err(format!("Missing teardown guarantee: {key}"));
            }
        }
        self.input.take();
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().map_err(|e| e.to_string())? {
                let mut stderr = String::new();
                if let Some(stream) = self.child.stderr.take() {
                    stream
                        .take(8193)
                        .read_to_string(&mut stderr)
                        .map_err(|e| e.to_string())?;
                }
                if !status.success() || !stderr.is_empty() {
                    return Err(format!("Clock exit {status}: {stderr}"));
                }
                if self.receive() != Err("EOF".into()) {
                    return Err("Unexpected trailing clock output".into());
                }
                return Ok(row);
            }
            if start.elapsed() > WAIT {
                return Err("Clock helper did not exit".into());
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.rx.take();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
struct Step {
    command: &'static str,
    seconds: f64,
    child: Option<usize>,
    text: &'static str,
    result: f64,
    lookups: u32,
}
fn steps() -> Vec<Step> {
    let time = |seconds, text, result, lookups| Step {
        command: "time",
        seconds,
        child: None,
        text,
        result,
        lookups,
    };
    let click = |seconds, child, text, result, lookups| Step {
        command: "click",
        seconds,
        child: Some(child),
        text,
        result,
        lookups,
    };
    let reset = |seconds, text, result| Step {
        command: "reset",
        seconds,
        child: None,
        text,
        result,
        lookups: 0,
    };
    vec![
        time(45296., "12:34:56", 45296., 0),
        click(45296., 0, " 0:00:00", 0.6, 0),
        time(45296.5, " 0:00:00", 0.6, 0),
        click(45296.5, 1, " 0:00:00", 0.6, 2),
        time(45297.75, " 0:00:01", 125.6, 1),
        time(45297.75, " 0:00:01", 125.6, 1),
        time(45357.75, " 0:01:01", 6125.6, 1),
        click(45357.75, 1, " 0:01:01", 6125.6, 1),
        time(45417.75, " 0:01:01", 6125.6, 0),
        click(45417.75, 1, " 0:00:00", 0.6, 1),
        time(45418.75, " 0:00:00", 0.6, 0),
        click(45418.75, 1, " 0:00:00", 0.6, 2),
        time(45419.75, " 0:00:01", 100.6, 1),
        click(45419.75, 0, "12:36:59", 45419., 0),
        time(50400., "14:00:00", 50400., 0),
        click(50400., 1, " 2:00:00", 50400.1, 1),
        click(50400., 2, " 2:00:00", 50400.1, 0),
        click(50400., 1, "14:00:00", 50400., 1),
        time(0.25, "00:00:00", 0., 0),
        time(86399.75, "23:59:59", 86399., 0),
        reset(45296., "12:34:56", 45296.),
        time(45296., "12:34:56", 45296., 0),
        click(45296., 0, " 0:00:00", 0.6, 0),
        time(45299.25, " 0:00:00", 0.6, 0),
        reset(0., "00:00:00", 0.),
        time(0., "00:00:00", 0., 0),
        click(0., 2, "00:00:00", 0., 0),
        reset(50400., "14:00:00", 50400.),
        click(50400., 1, " 2:00:00", 50400.1, 1),
        time(50400.25, " 2:00:00", 50400.1, 0),
    ]
}
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(
            "Usage: native-clock-session-check <FSX root> <helper.exe> <new output directory>"
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
    if output.exists() {
        return Err("Output directory must be new".into());
    }
    let library = GaugeLibrary::load(&root.join("Gauges/Cessna172.dll"), true)?;
    let main: Vec<_> = library
        .gauge("Clock")?
        .elements
        .iter()
        .filter(|e| e.kind == 2 && e.text.as_ref().is_some_and(|s| s.tokens == [3, 4, 5]))
        .collect();
    if main.len() != 1 {
        return Err("Expected one native main clock string".into());
    }
    let element_rva = main[0].rva;
    let list = library
        .mouse_rects("Clock")?
        .ok_or("Missing native clock mouse records")?;
    let children: Vec<_> = list
        .records
        .iter()
        .filter(|r| r.kind == MouseKind::Child)
        .map(|r| r.rva)
        .collect();
    if children.len() != 3 {
        return Err("Expected three authored clock mouse children".into());
    }
    let mut process = Process::open(&helper, &root)?;
    let pid = process.child.id();
    let steps = steps();
    let mut generation = 1;
    let mut events = 0;
    let mut previous: Option<&Step> = None;
    let mut rows = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        if step.command == "reset" {
            generation += 1;
            events = 0;
            previous = None;
        }
        let suffix = step.child.map(|c| format!(" {c}")).unwrap_or_default();
        process.send(&format!(
            "{} {} {}{suffix}",
            step.command,
            i + 1,
            step.seconds
        ))?;
        let row = packet(&process.receive()?, "state")?;
        let unchanged = previous.is_some_and(|p| p.seconds == step.seconds);
        let changed = previous.is_none_or(|p| p.text != step.text || p.result != step.result);
        if step.child.is_some() {
            events += 1;
        }
        if row["sequence"] != i + 1
            || row["element_rva"] != element_rva
            || row["pointer_bits"] != 32
            || row["library"] != "Cessna172.dll"
            || row["gauge"] != "Clock"
            || row["generation"] != generation
            || row["command"] != step.command
            || row["local_seconds"] != step.seconds
            || row["lookup_seconds"] != step.seconds
            || row["text"] != step.text
            || !row["callback_result"]
                .as_f64()
                .is_some_and(|v| (v - step.result).abs() < 1e-9)
            || row["lookup_count"] != step.lookups
            || row["lookup_token"] != 557
            || row["events_delivered"] != events
            || row["time_unchanged"] != unchanged
            || row["display_changed"] != changed
        {
            return Err(format!(
                "Native retained state mismatch at step {}: {row}",
                i + 1
            ));
        }
        for key in ["guarded_context", "guarded_string", "guarded_event_words"] {
            if row[key] != true {
                return Err(format!("Missing guard {key}"));
            }
        }
        for key in ["panel_services", "multiplayer", "fsx_parity_verified"] {
            if row[key] != false {
                return Err(format!("Unexpected support claim {key}"));
            }
        }
        if let Some(child) = step.child {
            if row["child"] != child
                || row["mouse_rva"] != children[child]
                || row["original_packet_word"] != [1, 0, 2][child]
                || row["mouse_result"] != 1
                || row["process_event_accepted"] != true
            {
                return Err("Original mouse packet/input mismatch".into());
            }
        } else if [
            "child",
            "mouse_rva",
            "original_packet_word",
            "mouse_result",
            "process_event_accepted",
        ]
        .iter()
        .any(|k| !row[k].is_null())
        {
            return Err("Time/reset command unexpectedly invoked a mouse callback".into());
        }
        previous = Some(step);
        println!("CHECKED {} {} -> {}", i + 1, step.command, step.text);
        rows.push(row);
    }
    let closed = process.finish(steps.len(), generation)?;
    rows.push(closed);
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    fs::write(
        output.join("samples.ndjson"),
        rows.iter().map(|r| format!("{r}\n")).collect::<String>(),
    )
    .map_err(|e| e.to_string())?;
    let report = json!({"schema":1,"commands":steps.len(),"pid":pid,"module_generations":generation,
        "retained_stopwatch":true,"fractional_seconds":true,"pause_preserves_value":true,"stop_freezes_value":true,
        "stopwatch_reset":true,"repeat_start":true,"format_12_24":true,"native_reinitialization":true,"clean_exit":true,
        "viewer_input":false,"secondary_string":false,"fsx_parity_verified":false});
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!("PASS: {report}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clock_reader_bounds_packets_preserves_following_lines_and_requires_termination() {
        let mut reader = std::io::Cursor::new(b"first\nsecond\n");
        assert_eq!(read_line(&mut reader).unwrap(), "first\n");
        assert_eq!(read_line(&mut reader).unwrap(), "second\n");
        assert_eq!(read_line(&mut reader), Err("EOF".into()));
        assert!(read_line(&mut std::io::Cursor::new(b"partial")).is_err());
        assert!(read_line(&mut std::io::Cursor::new(vec![b'x'; 4097])).is_err());
        assert!(packet(
            r#"{"schema":1,"source":"native-clock-local-stream","kind":"state"}"#,
            "state"
        )
        .is_ok());
        assert!(packet(
            r#"{"schema":1,"source":"native-clock-local-stream","kind":"closed"}"#,
            "state"
        )
        .is_err());
    }
}
