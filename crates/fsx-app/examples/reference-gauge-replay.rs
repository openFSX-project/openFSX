//! Bounded recorded-indication replay; no FSX launch, control events or physics replay.
#[path = "../src/flight_gauges.rs"]
#[allow(dead_code)]
mod flight_gauges;
#[path = "../src/native_gauges.rs"]
#[allow(dead_code)]
mod native_gauges;
#[path = "../src/reference_pose.rs"]
#[allow(dead_code)]
mod reference_pose;
use fsx_model::Model;
use fsx_render::{block_on, Appearance, Camera, Gpu, Orbit};
use fsx_sim::air_data::AirData;
use fsx_vehicle::aircraft::{VisualAircraft, FOOT, KNOT};
use reference_pose::{Instruments, Recording, ReplayClock};
use serde_json::{json, Value};
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::PathBuf,
};

fn diagnostics(data: &Instruments) -> Value {
    let get = |key| data.get(key).unwrap(); // Schema validation requires every named field.
    let ideal = match AirData::standard(
        get("altitude_msl_ft") * FOOT,
        get("tas_knots") * KNOT,
        get("vertical_speed_fpm") * FOOT / 60.,
    ) {
        Ok(air) => {
            json!({"available":true,"assumption":"ideal ISA at recorded geometric MSL altitude, no instrument errors",
            "predicted_cas_knots":air.calibrated_speed_mps/KNOT,
            "recorded_ias_minus_ideal_cas_knots":get("ias_knots")-air.calibrated_speed_mps/KNOT,
            "recorded_pressure_altitude_minus_ideal_ft":get("pressure_altitude_ft")-air.pressure_altitude_m/FOOT,
            "ambient_pressure_minus_isa_pa":get("ambient_pressure_mbar")*100.-air.static_pressure_pascal,
            "ambient_temp_minus_isa_c":get("ambient_temp_c")-(air.temperature_kelvin-273.15)})
        }
        Err(error) => json!({"available":false,"reason":error}),
    };
    let heading_difference =
        (get("gyro_heading_deg") - get("heading_magnetic_deg") + 180.).rem_euclid(360.) - 180.;
    json!({"ideal_air_data":ideal,
        "gyro_minus_body_pitch_deg":get("gyro_pitch_deg")-get("pitch_deg"),
        "gyro_minus_body_bank_deg":get("gyro_bank_deg")-get("bank_deg"),
        "gyro_minus_magnetic_heading_deg":heading_difference,
        "baro_inhg":get("baro_inhg"),"sim_rate":get("sim_rate"),
        "parity_verdict":"not established; diagnostics are not a recorded-FSX flight trajectory comparison"})
}
fn validate(path: &std::path::Path) -> Result<Value, String> {
    let mut recording = Recording::open(path)?;
    let mut clock = ReplayClock::default();
    let (mut count, mut rewinds, mut same_clock, mut unsupported) = (0u32, 0u32, 0u32, 0u32);
    let mut last_time = None;
    let mut last_inputs = None;
    let mut paused_input_changes = 0u32;
    let mut first_inputs = None;
    let mut source = None;
    while let Some(row) = recording.next()? {
        let instruments = row
            .instruments
            .as_ref()
            .ok_or("Schema-2 instrument data required")?;
        let (_, rewind) = clock.observe(instruments.time())?;
        rewinds += u32::from(rewind);
        same_clock += u32::from(last_time == Some(instruments.time()));
        match instruments.native_inputs() {
            Ok(inputs) => {
                if last_time == Some(instruments.time())
                    && last_inputs.is_some_and(|previous| previous != inputs)
                {
                    paused_input_changes += 1;
                    unsupported += 1;
                }
                if count == 0 {
                    first_inputs = Some(inputs);
                }
                last_inputs = Some(inputs);
            }
            Err(_) => {
                unsupported += 1;
                last_inputs = None;
            }
        }
        last_time = Some(instruments.time());
        source = Some(row.source);
        count += 1;
    }
    if count == 0 {
        return Err("Empty reference recording".into());
    }
    Ok(
        json!({"schema":1,"source":"openfsx-reference-protocol-check","recorded_source":source,
        "samples":count,"unchanged_sim_clock_rows":same_clock,"clock_rewinds":rewinds,
        "unsupported_native_profile_rows":unsupported,"first_native_inputs":first_inputs,
        "paused_input_changes":paused_input_changes,
        "game_loaded":false,"fsx_parity_validated":false}),
    )
}
fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let first_argument = args
        .next()
        .ok_or("Expected FSX root or --validate recording")?;
    if first_argument == "--validate" {
        let path = PathBuf::from(args.next().ok_or("Expected schema-2 recording")?);
        if args.next().is_some() {
            return Err("Unexpected validator argument".into());
        }
        println!("{}", validate(&path)?);
        return Ok(());
    }
    let root = PathBuf::from(first_argument)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let recording = PathBuf::from(args.next().ok_or("Expected recording")?);
    let output = PathBuf::from(args.next().ok_or("Expected new output directory")?);
    let helper = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fsx-gauge-host.exe")
    });
    if args.next().is_some() {
        return Err("Unexpected argument".into());
    }
    let mut reader = Recording::open(&recording)?;
    let first = reader.next()?.ok_or("Empty reference recording")?;
    let aircraft = VisualAircraft::load(&root.join("SimObjects/Airplanes/C172"), 0, true)?;
    if first.source == "FSX SimConnect" && first.title != aircraft.title {
        return Err("Recorded title does not match the checked C172 variant 0".into());
    }
    // Validate profile eligibility before starting a helper or creating outputs.
    first
        .instruments
        .as_ref()
        .ok_or("Expected schema-2 instruments")?
        .native_inputs()?;
    std::fs::create_dir(&output).map_err(|e| e.to_string())?;
    let mut log = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join("frames.ndjson"))
            .map_err(|e| e.to_string())?,
    );
    let model = Model::load(
        aircraft
            .interior
            .as_deref()
            .ok_or("Missing configured C172 interior")?,
    )?;
    let geometry = model.geometry(None)?;
    let mut appearance = Appearance::load(&model, &geometry, Some(&aircraft.texture))?;
    let eye = aircraft.eye_point.ok_or("No configured C172 eye point")?;
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
    let mut gauges = flight_gauges::FlightGauges::new(&root, &aircraft.panel, &helper)?;
    let source = first.source.clone();
    let title = first.title.clone();
    let object = first.object_id;
    let mut sample = Some(first);
    let mut clock = ReplayClock::default();
    let mut gpu: Option<Gpu> = None;
    let (mut frames, mut paused, mut rewinds) = (0u32, 0u32, 0u32);
    while let Some(row) = sample {
        let instruments = row.instruments.as_ref().ok_or("Missing instruments")?;
        let inputs = instruments.native_inputs()?;
        let (time, rewind) = clock.observe(instruments.time())?;
        if rewind {
            gauges.restart()?;
            rewinds += 1;
        }
        let frame = gauges.update_inputs(time, inputs)?;
        let pose = fsx_model::animation::plan_pose(&model.animations, |name, units| {
            row.lookup(name, units)
        })?;
        let frame_geometry = model.posed_geometry(Some(geometry.lod), &pose.frames)?;
        let posed_groups = pose.frames.iter().filter(|f| f.is_some()).count();
        if let Some(renderer) = &mut gpu {
            renderer.update_geometry(&frame_geometry)?;
        }
        if frame.reused {
            paused += 1;
        } else {
            gauges.apply(&frame, &model, &mut appearance, gpu.as_mut())?;
        }
        if gpu.is_none() {
            let renderer = block_on(Gpu::new(
                None,
                &frame_geometry,
                &appearance,
                false,
                960,
                720,
            ))?;
            renderer.snapshot(&orbit, &output.join("first.ppm"))?;
            gpu = Some(renderer);
        }
        let log_row = json!({"schema":1,"source":"openfsx-recorded-instrument-replay","recorded_source":row.source,
            "title":row.title,"object_id":row.object_id,"sample_index":row.index,
            "recorded_sim_time_seconds":instruments.time(),"relative_sim_time_seconds":time,"rewind":rewind,
            "helper_pid":gauges.pid(),"reused":frame.reused,"frames_evaluated":frame.evaluated,"native_inputs":inputs,
            "recorded_animation_groups":posed_groups,"model_animation_groups":pose.frames.len(),
            "needles":frame.values.iter().map(|v|json!({"library":v.library,"gauge":v.gauge,"resource":v.resource,"result":v.result})).collect::<Vec<_>>(),
            "sprites":frame.sprites.iter().map(|v|json!({"library":v.library,"gauge":v.gauge,"resource":v.resource,"results":v.results})).collect::<Vec<_>>(),
            "diagnostics":diagnostics(instruments)});
        writeln!(log, "{log_row}").map_err(|e| e.to_string())?;
        frames += 1;
        sample = reader.next()?;
    }
    gpu.ok_or("No rendered replay frame")?
        .snapshot(&orbit, &output.join("last.ppm"))?;
    gauges.close()?;
    log.flush().map_err(|e| e.to_string())?;
    let report = json!({"schema":1,"source":"openfsx-recorded-instrument-replay","recorded_source":source,"title":title,"object_id":object,
        "samples":frames,"paused_reuses":paused,"clock_rewinds":rewinds,"fixed_camera":true,"physics_replayed":false,
        "control_animation":"available stateless authored bindings; positive lag and unsupported expressions omitted",
        "native_panel_profile":"C172 six needles (tachometer at 0 without recorded RPM) and three sprites; neutral healthy instruments only",
        "fsx_parity_validated":false});
    std::fs::write(output.join("report.json"), report.to_string()).map_err(|e| e.to_string())?;
    println!("PASS recorded indication replay: {report}");
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Reference gauge replay failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
