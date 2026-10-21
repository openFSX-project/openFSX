//! Bounded persistent stdio bridge to the owned x86 gauge helper.
use serde_json::Value;
use std::{
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::Duration,
};

const WAIT: Duration = Duration::from_millis(5500);
pub const BINDINGS: [(&str, &str, i32, u32, usize); 5] = [
    ("Cessna172.dll", "Airspeed", 1001, 61, 0),
    ("Cessna.dll", "Altimeter", 1033, 74, 1),
    ("Cessna.dll", "Altimeter", 1032, 74, 1),
    ("Cessna.dll", "Altimeter", 1031, 74, 1),
    ("Cessna.dll", "Vertical_Speed", 5047, 65, 2),
];
// library, gauge, bitmap, token, input index (6 = MODULE_VAR_NONE), axis, callback
const SPRITE_BINDINGS: [(&str, &str, i32, u32, usize, u32, bool); 9] = [
    ("Cessna.dll", "Heading_Indicator", 4001, 0, 6, 0, false),
    ("Cessna.dll", "Heading_Indicator", 4001, 0, 6, 1, false),
    ("Cessna.dll", "Heading_Indicator", 4001, 84, 5, 2, true),
    ("Bell_206B.dll", "Attitude", 2004, 0, 6, 0, false),
    ("Bell_206B.dll", "Attitude", 2004, 0, 6, 1, false),
    ("Bell_206B.dll", "Attitude", 2004, 80, 4, 2, false),
    ("Bell_206B.dll", "Attitude", 2002, 0, 6, 0, false),
    ("Bell_206B.dll", "Attitude", 2002, 78, 3, 1, true),
    ("Bell_206B.dll", "Attitude", 2002, 80, 4, 2, false),
];

#[derive(Debug, Clone)]
pub struct NeedleValue {
    pub library: String,
    pub gauge: String,
    pub resource: i32,
    pub token: u32,
    pub result: f64,
}
#[derive(Debug, Clone)]
pub struct Frame {
    pub sequence: u64,
    pub time: f64,
    pub reused: bool,
    pub display_changed: bool,
    pub evaluated: u64,
    pub values: Vec<NeedleValue>,
    pub sprites: Vec<SpriteValue>,
    pub clock: Option<ClockValue>,
}
#[derive(Debug, Clone)]
pub struct ClockValue {
    pub element_rva: u32,
    pub text: String,
}
#[derive(Debug, Clone)]
pub struct SpriteValue {
    pub library: String,
    pub gauge: String,
    pub resource: i32,
    pub tokens: [u32; 3],
    pub results: [f64; 3],
    pub callbacks: [bool; 3],
}
fn close_number(a: f64, b: f64) -> bool {
    (a - b).abs() <= 8. * f64::EPSILON * a.abs().max(b.abs()).max(1.)
}
fn packet(line: &str, kind: &str, clock: bool) -> Result<Value, String> {
    if line.len() > 16384 {
        return Err("Native gauge response exceeds 16 KiB".into());
    }
    let v: Value =
        serde_json::from_str(line).map_err(|e| format!("Invalid native gauge JSON: {e}"))?;
    if v["schema"] != if clock { 3 } else { 2 }
        || v["source"] != "native-gauge-stream"
        || v["kind"] != kind
    {
        return Err("Unexpected native gauge response schema/source/kind".into());
    }
    Ok(v)
}
fn decode_clock(c: &Value, seconds: u32) -> Result<ClockValue, String> {
    if seconds >= 86400 {
        return Err("Invalid local clock seconds".into());
    }

    let expected = format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    );
    let element_rva = c["element_rva"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|&n| n != 0)
        .ok_or("Invalid native clock element RVA")?;
    if c["library"] != "Cessna172.dll"
        || c["gauge"] != "Clock"
        || c["tokens"] != serde_json::json!([3, 4, 5])
        || c["max_length"] != 8
        || c["local_seconds"].as_u64() != Some(u64::from(seconds))
        || c["guarded_buffer"] != true
        || c["text"].as_str() != Some(expected.as_str())
        || !c["callback_result"]
            .as_f64()
            .is_some_and(|n| close_number(n, f64::from(seconds)))
    {
        return Err("Native clock binding, text, guard or input echo mismatch".into());
    }
    Ok(ClockValue {
        element_rva,
        text: expected,
    })
}
fn decode_frame(
    line: &str,
    sequence: u64,
    time: f64,
    inputs: [f64; 6],
    reused: bool,
    evaluated: u64,
    clock_seconds: Option<u32>,
) -> Result<Frame, String> {
    let v = packet(line, "frame", clock_seconds.is_some())?;
    let returned_time = v["time"]
        .as_f64()
        .filter(|n| n.is_finite())
        .ok_or("Invalid native frame time")?;
    if v["sequence"].as_u64() != Some(sequence)
        || !close_number(returned_time, time)
        || v["reused"] != reused
        || v["frames_evaluated"].as_u64() != Some(evaluated)
    {
        return Err("Native gauge frame context does not match the request".into());
    }
    let values = v["values"]
        .as_array()
        .filter(|a| a.len() == BINDINGS.len() + SPRITE_BINDINGS.len())
        .ok_or("Expected five needle and nine sprite-axis results")?;
    let mut out = Vec::new();
    for (value, (library, gauge, resource, token, input)) in values.iter().zip(BINDINGS) {
        if value["library"] != library
            || value["gauge"] != gauge
            || value["resource"] != resource
            || value["token"] != token
            || value["element_kind"] != 1
            || value["axis"] != 0
            || value["callback_called"] != true
        {
            return Err("Native gauge result binding mismatch".into());
        }
        let raw = value["raw_input"]
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or("Invalid returned native input")?;
        let result = value["callback_result"]
            .as_f64()
            .filter(|n| n.is_finite() && n.abs() <= 1e12)
            .ok_or("Invalid returned native reading")?;
        if !close_number(raw, inputs[input]) {
            return Err("Native gauge input echo mismatch".into());
        }
        out.push(NeedleValue {
            library: library.to_owned(),
            gauge: gauge.to_owned(),
            resource,
            token,
            result,
        });
    }
    let mut sprites = Vec::new();
    for group in 0..3 {
        let start = group * 3;
        let (library, gauge, resource, ..) = SPRITE_BINDINGS[start];
        let mut sprite = SpriteValue {
            library: library.into(),
            gauge: gauge.into(),
            resource,
            tokens: [0; 3],
            results: [0.; 3],
            callbacks: [false; 3],
        };
        for i in 0..3 {
            let (l, g, r, token, input, axis, callback) = SPRITE_BINDINGS[start + i];
            let value = &values[BINDINGS.len() + start + i];
            let raw = if input == 6 { 0. } else { inputs[input] };
            if value["library"] != l
                || value["gauge"] != g
                || value["resource"] != r
                || value["token"] != token
                || value["element_kind"] != 6
                || value["axis"] != axis
                || value["callback_called"] != callback
                || !value["raw_input"]
                    .as_f64()
                    .is_some_and(|v| v.is_finite() && close_number(v, raw))
            {
                return Err("Native sprite binding/input mismatch".into());
            }
            let result = value["callback_result"]
                .as_f64()
                .filter(|v| v.is_finite() && v.abs() <= 1e12)
                .ok_or("Invalid sprite result")?;
            if !callback && !close_number(result, raw) {
                return Err("Callback-free sprite axis changed its source value".into());
            }
            sprite.tokens[i] = token;
            sprite.results[i] = result;
            sprite.callbacks[i] = callback;
        }
        sprites.push(sprite);
    }
    let clock = if let Some(seconds) = clock_seconds {
        Some(decode_clock(&v["clock"], seconds)?)
    } else {
        if !v["clock"].is_null() {
            return Err("Unexpected native clock result".into());
        }
        None
    };
    Ok(Frame {
        sequence,
        time,
        reused,
        display_changed: !reused,
        evaluated,
        values: out,
        sprites,
        clock,
    })
}

struct Process {
    child: Child,
    stdin: Option<ChildStdin>,
    responses: Option<Receiver<Result<String, String>>>,
    reader: Option<JoinHandle<()>>,
    pid: u32,
    clock: bool,
}
impl Process {
    fn open(helper: &Path, root: &Path, clock: bool) -> Result<Self, String> {
        let mut command = Command::new(helper);
        command
            .arg(if clock {
                "--clock-frames"
            } else {
                "--flight-frames"
            })
            .arg(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW, including release GUI launch paths.
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("{}: {e}", helper.display()))?;
        let stdin = child.stdin.take().ok_or("Missing native gauge stdin")?;
        let stdout = child.stdout.take().ok_or("Missing native gauge stdout")?;
        let (tx, rx) = mpsc::sync_channel(1);
        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let result = (|| -> Result<Option<String>, String> {
                    let mut line = Vec::new();
                    let mut byte = [0];
                    loop {
                        let n = reader.read(&mut byte).map_err(|e| e.to_string())?;
                        if n == 0 {
                            return if line.is_empty() {
                                Ok(None)
                            } else {
                                Err("Truncated native gauge response".into())
                            };
                        }
                        if byte[0] == b'\n' {
                            break;
                        }
                        if line.len() >= 16384 {
                            return Err("Native gauge response exceeds 16 KiB".into());
                        }
                        line.push(byte[0]);
                    }
                    Ok(Some(
                        String::from_utf8(line)
                            .map_err(|_| "Native gauge response is not UTF-8")?,
                    ))
                })();
                match result {
                    Ok(Some(line)) => {
                        if tx.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
            }
        });
        let mut process = Self {
            child,
            stdin: Some(stdin),
            responses: Some(rx),
            reader: Some(reader),
            pid: 0,
            clock,
        };
        let ready = packet(&process.receive()?, "ready", clock)?;
        let pid = ready["pid"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or("Invalid native helper PID")?;
        if ready["pointer_bits"] != 32
            || ready["libraries_loaded"] != 3
            || ready["bindings"] != 14
            || ready["panel_services"] != false
            || (clock
                && (ready["clock_connected"] != true
                    || ready["string_bindings"] != 1
                    || ready["guarded_buffer"] != true
                    || ready["clock_set"] != true))
            || pid != process.child.id()
        {
            return Err("Unexpected native gauge helper capabilities/PID".into());
        }
        process.pid = pid;
        Ok(process)
    }
    fn receive(&mut self) -> Result<String, String> {
        let response = self
            .responses
            .as_ref()
            .ok_or("Native gauge response channel closed")?
            .recv_timeout(WAIT);
        match response {
            Ok(Ok(line)) => Ok(line),
            other => {
                let _ = self.child.kill();
                let _ = self.child.wait();
                let mut diagnostic = String::new();
                if let Some(stderr) = self.child.stderr.take() {
                    let _ = stderr.take(8192).read_to_string(&mut diagnostic);
                }
                Err(format!(
                    "Native gauge helper failed ({other:?}): {}",
                    diagnostic.trim()
                ))
            }
        }
    }
    fn write(&mut self, line: &str) -> Result<(), String> {
        let pipe = self.stdin.as_mut().ok_or("Native gauge input closed")?;
        pipe.write_all(line.as_bytes())
            .and_then(|_| pipe.flush())
            .map_err(|e| format!("Native gauge input failed: {e}"))
    }
    fn close(&mut self, evaluated: u64) -> Result<(), String> {
        self.write("quit\n")?;
        let v = packet(&self.receive()?, "closed", self.clock)?;
        if v["modules_deinitialized"] != 3
            || v["frames_evaluated"].as_u64() != Some(evaluated)
            || (self.clock
                && (v["clock_disconnected"] != true || v["string_buffer_restored"] != true))
        {
            return Err("Invalid native module teardown acknowledgement".into());
        }
        let deadline = std::time::Instant::now() + WAIT;
        loop {
            if let Some(status) = self.child.try_wait().map_err(|e| e.to_string())? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("Native gauge teardown exited with {status}"))
                };
            }
            if std::time::Instant::now() >= deadline {
                return Err("Native gauge teardown did not exit".into());
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.responses.take();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

pub struct NativeGauges {
    helper: PathBuf,
    root: PathBuf,
    process: Option<Process>,
    sequence: u64,
    evaluated: u64,
    last: Option<(f64, [f64; 6], Option<u32>)>,
    clock: bool,
    display_dirty: bool,
}
impl NativeGauges {
    pub fn new(helper: &Path, root: &Path) -> Result<Self, String> {
        Ok(Self {
            helper: helper.to_owned(),
            root: root.to_owned(),
            process: Some(Process::open(helper, root, false)?),
            sequence: 0,
            evaluated: 0,
            last: None,
            clock: false,
            display_dirty: false,
        })
    }
    pub fn enable_clock(&mut self) -> Result<(), String> {
        if self.sequence != 0 {
            return Err("Enable native clock before evaluating frames".into());
        }
        self.clock = true;
        self.restart()
    }
    pub fn pid(&self) -> u32 {
        self.process.as_ref().map(|p| p.pid).unwrap_or(0)
    }
    pub fn restart(&mut self) -> Result<(), String> {
        self.close()?;
        self.process = Some(Process::open(&self.helper, &self.root, self.clock)?);
        self.sequence = 0;
        self.evaluated = 0;
        self.last = None;
        self.display_dirty = false;
        Ok(())
    }
    pub fn set_clock_seconds(&mut self, seconds: u32, elapsed: f64) -> Result<(), String> {
        let (time, _, _) = self
            .last
            .ok_or("Clock events require an initialized native frame")?;
        if !self.clock || seconds >= 86400 || elapsed != time {
            return Err("Invalid native clock edit/profile".into());
        }
        let process = self
            .process
            .as_mut()
            .ok_or("Native gauge session is closed")?;
        let result = (|| -> Result<(), String> {
            process.write(&format!("clock-set {seconds}\n"))?;
            let v = packet(&process.receive()?, "clock-set", true)?;
            if v["sequence"].as_u64() != Some(self.sequence)
                || v["frames_evaluated"].as_u64() != Some(self.evaluated)
                || !v["time"].as_f64().is_some_and(|n| close_number(n, time))
            {
                return Err("Native clock edit changed physics frame context".into());
            }
            decode_clock(&v["clock"], seconds)?;
            Ok(())
        })();
        if let Err(error) = result {
            self.process.take();
            return Err(error);
        }
        self.last.as_mut().unwrap().2 = Some(seconds);
        self.display_dirty = true;
        Ok(())
    }
    pub fn update(&mut self, time: f64, inputs: [f64; 6]) -> Result<Frame, String> {
        self.update_with_clock(time, inputs, None)
    }
    pub fn update_with_clock(
        &mut self,
        time: f64,
        inputs: [f64; 6],
        clock_seconds: Option<u32>,
    ) -> Result<Frame, String> {
        if self.clock != clock_seconds.is_some() || clock_seconds.is_some_and(|s| s >= 86400) {
            return Err("Native clock request does not match the session profile".into());
        }
        if !time.is_finite()
            || !(0. ..=1e9).contains(&time)
            || inputs.iter().any(|v| !v.is_finite() || v.abs() > 1e12)
            || inputs[0] < 0.
        {
            return Err("Invalid simulation time or native gauge inputs".into());
        }
        if self.last.is_some_and(|(t, _, _)| time < t) {
            self.restart()?;
        }
        let reused = self.last.is_some_and(|(t, _, _)| t == time);
        if reused && (self.last.unwrap().1 != inputs || self.last.unwrap().2 != clock_seconds) {
            return Err("Changed gauge physics inputs without advancing simulation time".into());
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Native gauge sequence exhausted")?;
        let evaluated = self.evaluated + u64::from(!reused);
        let process = self
            .process
            .as_mut()
            .ok_or("Native gauge session is closed")?;
        let result = (|| -> Result<Frame, String> {
            let clock_suffix = clock_seconds.map(|s| format!(" {s}")).unwrap_or_default();
            process.write(&format!(
                "frame {sequence} {time} {} {} {} {} {} {}{clock_suffix}\n",
                inputs[0], inputs[1], inputs[2], inputs[3], inputs[4], inputs[5]
            ))?;
            decode_frame(
                &process.receive()?,
                sequence,
                time,
                inputs,
                reused,
                evaluated,
                clock_seconds,
            )
        })();
        let mut frame = match result {
            Ok(frame) => frame,
            Err(error) => {
                self.process.take();
                return Err(error);
            }
        };
        frame.display_changed |= self.display_dirty;
        self.display_dirty = false;
        self.sequence = sequence;
        self.evaluated = evaluated;
        self.last = Some((time, inputs, clock_seconds));
        Ok(frame)
    }
    pub fn close(&mut self) -> Result<(), String> {
        if let Some(mut process) = self.process.take() {
            process.close(self.evaluated)?;
        }
        Ok(())
    }
}
impl Drop for NativeGauges {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clock_protocol_requires_guarded_matching_default_mode_output() {
        let mut v = fixture();
        v["schema"] = serde_json::json!(3);
        v["clock"] = serde_json::json!({"library":"Cessna172.dll","gauge":"Clock",
            "element_rva":100,"tokens":[3,4,5],"max_length":8,"local_seconds":45296,
            "guarded_buffer":true,"text":"12:34:56","callback_result":45296});
        let decode = |v: &Value, seconds| {
            decode_frame(
                &v.to_string(),
                1,
                0.5,
                [75., 500., 256., -10., -30., 90.],
                false,
                1,
                seconds,
            )
        };
        assert_eq!(
            decode(&v, Some(45296)).unwrap().clock.unwrap().text,
            "12:34:56"
        );
        assert!(decode(&v, None).is_err());
        assert!(decode(&v, Some(45297)).is_err());
        assert!(decode(&v, Some(86400)).is_err());
        for field in [
            "library",
            "gauge",
            "element_rva",
            "tokens",
            "max_length",
            "local_seconds",
            "guarded_buffer",
            "text",
            "callback_result",
        ] {
            let mut bad = v.clone();
            bad["clock"][field] = Value::Null;
            assert!(decode(&bad, Some(45296)).is_err(), "{field}");
        }
        v["clock"]["text"] = serde_json::json!("12:34:55");
        assert!(decode(&v, Some(45296)).is_err());
    }
    fn fixture() -> Value {
        let inputs = [75., 500., 256., -10., -30., 90.];
        let mut values: Vec<_> = BINDINGS.iter().map(|(l,g,r,t,i)|serde_json::json!({"library":l,"gauge":g,"resource":r,"token":t,"raw_input":inputs[*i],"callback_result":42.,"element_kind":1,"axis":0,"callback_called":true})).collect();
        values.extend(SPRITE_BINDINGS.iter().map(|(l,g,r,t,i,axis,called)| {
            let raw = if *i == 6 {0.} else {inputs[*i]};
            serde_json::json!({"library":l,"gauge":g,"resource":r,"token":t,"raw_input":raw,"callback_result":raw,"element_kind":6,"axis":axis,"callback_called":called})
        }));
        serde_json::json!({"schema":2,"source":"native-gauge-stream","kind":"frame","sequence":1,"time":0.5,"reused":false,"frames_evaluated":1,"values":values})
    }
    #[test]
    fn original_synthetic_protocol_fixture_checks_context_and_each_binding() {
        let v = fixture();
        let f = decode_frame(
            &v.to_string(),
            1,
            0.5,
            [75., 500., 256., -10., -30., 90.],
            false,
            1,
            None,
        )
        .unwrap();
        assert_eq!(f.values.len(), 5);
        assert_eq!(f.values[4].token, 65);
        assert_eq!(f.sprites.len(), 3);
        assert_eq!(f.sprites[2].tokens, [0, 78, 80]);
        assert_eq!(f.sprites[2].results, [0., -10., -30.]);
        for field in ["sequence", "time", "frames_evaluated", "reused"] {
            let mut bad = v.clone();
            bad[field] = Value::Null;
            assert!(decode_frame(
                &bad.to_string(),
                1,
                0.5,
                [75., 500., 256., -10., -30., 90.],
                false,
                1,
                None
            )
            .is_err());
        }
        for field in [
            "library",
            "gauge",
            "resource",
            "token",
            "raw_input",
            "callback_result",
            "element_kind",
            "axis",
            "callback_called",
        ] {
            for i in 0..14 {
                let mut bad = v.clone();
                bad["values"][i][field] = Value::Null;
                assert!(decode_frame(
                    &bad.to_string(),
                    1,
                    0.5,
                    [75., 500., 256., -10., -30., 90.],
                    false,
                    1,
                    None
                )
                .is_err());
            }
        }
    }
}
