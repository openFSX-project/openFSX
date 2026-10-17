//! Read-only first step: inspect aircraft definitions without loading executable add-ons.
use std::{env, fs, path::Path, process::ExitCode};

fn titles(text: &str) -> Vec<String> {
    let mut aircraft = false;
    let mut result = Vec::new();
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.starts_with(';') || line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            aircraft = line
                .find(']')
                .map(|end| {
                    let name = line[1..end].to_ascii_lowercase();
                    name.strip_prefix("fltsim.")
                        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
                })
                .unwrap_or(false);
        } else if aircraft {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim().eq_ignore_ascii_case("title") {
                    let value = value
                        .split(';')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .trim_matches('"');
                    if !value.is_empty() {
                        result.push(value.to_owned());
                    }
                }
            }
        }
    }
    result
}

use fsx_cfg::decode;
use fsx_sim::AircraftDynamics;

fn takeoff_report(folder: &Path, windows_1252: bool) -> Result<(), String> {
    use fsx_sim::flight::{Simulation, STEP};
    use fsx_vehicle::aircraft::{Aircraft, KNOT};
    let aircraft = Aircraft::load(folder, 0, windows_1252)?;
    let mut s = Simulation::on_ground(aircraft.parameters)?;
    println!(
        "{} | {} fixed wheels | {} non-wheel contacts omitted",
        aircraft.title,
        s.parameters.gear.wheels.len(),
        s.parameters.gear.omitted_contacts
    );
    println!(
        "Settled: height {:.4} m | pitch {:.3} deg | speed {:.6} m/s | trim {:.5}",
        s.telemetry()?.altitude,
        s.telemetry()?.attitude[1].to_degrees(),
        s.telemetry()?.speed,
        s.controls.trim
    );
    let start = s.state.position;
    for _ in 0..2400 {
        s.step(s.controls, STEP)?;
    }
    let t = s.telemetry()?;
    println!(
        "20 s power-off parking: speed {:.6} m/s | height {:.4} m | north drift {:.4} m",
        t.speed,
        t.altitude,
        s.state.position[0] - start[0]
    );
    if t.speed > 0.2
        || (s.state.position[0] - start[0]).abs() > 1.
        || (s.state.position[2] - start[2]).abs() > 0.1
    {
        return Err("Ground parking consistency failed".into());
    }
    let mut controls = s.controls;
    let mut braking = Simulation::on_ground(s.parameters.clone())?;
    braking.state.velocity[0] = 10.;
    let brake_start = braking.state.position[0];
    for _ in 0..1200 {
        braking.step(braking.controls, STEP)?;
    }
    println!(
        "10 m/s braking: final speed {:.4} m/s | distance {:.2} m",
        braking.telemetry()?.speed,
        braking.state.position[0] - brake_start
    );
    if braking.telemetry()?.speed > 0.3 || braking.state.position[0] - brake_start > 30. {
        return Err("Braking consistency failed".into());
    }
    controls.throttle = 1.;
    controls.brake = 0.;
    let start = s.state.position;
    let time = s.time;
    let mut rotation = None;
    let mut liftoff = None;
    for _ in 0..7200 {
        if rotation.is_none() && s.telemetry()?.speed >= 55. * KNOT {
            rotation = Some(s.time);
            println!("Rotation input at {:.2} s", s.time - time);
        }
        controls.pitch = if rotation.is_some_and(|r| s.time - r < 2.) {
            0.12
        } else {
            0.
        };
        s.step(controls, STEP)?;
        if !s.ground_contact && liftoff.is_none() && s.telemetry()?.altitude > 3. {
            liftoff = Some((
                s.time - time,
                s.state.position[0] - start[0],
                s.telemetry()?.speed / KNOT,
            ));
        }
        if s.telemetry()?.altitude > 30. && !s.ground_contact {
            println!("First airborne clearance above 3 m: {liftoff:?} (seconds, north distance metres, knots)");
            println!(
                "Climb gate: {:.2} s | {:.2} kt | {:.2} m | {:.2} m/s",
                s.time - time,
                s.telemetry()?.speed / KNOT,
                s.telemetry()?.altitude,
                s.telemetry()?.climb
            );
            println!("PASS: ground roll and departure under explicit fixed pilot inputs. Not an FSX performance comparison.");
            return Ok(());
        }
    }
    Err(format!(
        "Takeoff did not reach 30 m in 60 s: {:?}",
        s.telemetry()?
    ))
}

fn aircraft_report(folder: &Path, windows_1252: bool, check_flight: bool) -> Result<(), String> {
    use fsx_sim::flight::{Simulation, STEP};
    use fsx_vehicle::aircraft::{Aircraft, KNOT};
    let aircraft = Aircraft::load(folder, 0, windows_1252)?;
    let p = &aircraft.parameters;
    println!(
        "{}\nModel: {}\nLivery: {}\nAIR: {}",
        aircraft.title,
        aircraft.model.display(),
        aircraft.texture.display(),
        aircraft.air_path.display()
    );
    println!(
        "Loaded mass: {:.2} kg | Wing: {:.3} m2 | Span: {:.3} m | Power: {:.2} kW",
        p.loading.mass,
        p.wing_area,
        p.span,
        p.power / 1000.
    );
    println!(
        "Loading: empty {:.2}, payload {:.2}, full usable Avgas {:.2}, maximum {:.2} kg",
        p.loading.empty_mass, p.loading.payload_mass, p.loading.fuel_mass, p.loading.max_mass
    );
    println!(
        "Body CG (forward/right/down metres): {:?}\nLoaded inertia (kg m2): {:?}",
        p.loading.cg,
        p.loading.inertia.matrix()
    );
    let stall = p.predicted_stall(0.)? / KNOT;
    println!(
        "Derived clean stall (sea level): {stall:.2} kt | CFG reference: {:.2} kt",
        p.reference_stall / KNOT
    );
    println!("Subset: legacy AIR 1101/0404/0473; fixed-mass single piston; approximate propeller/drag; not FSX parity.");
    if check_flight {
        let mut simulation = Simulation::trimmed(p.clone(), 90. * KNOT, 300.)?;
        let controls = simulation.controls;
        println!(
            "90 kt / 300 m trim: throttle {:.5}, trim {:.5}, alpha {:.3} deg",
            controls.throttle,
            controls.trim,
            simulation.telemetry()?.alpha.to_degrees()
        );
        for _ in 0..7200 {
            simulation.step(controls, STEP)?;
        }
        let t = simulation.telemetry()?;
        println!(
            "60 s fixed-control level check: speed {:.5} kt, altitude {:.5} m, climb {:.6} m/s",
            t.speed / KNOT,
            t.altitude,
            t.climb
        );
        if (t.speed / KNOT - 90.).abs() > 0.1 || (t.altitude - 300.).abs() > 0.1 {
            return Err("Level trim drift exceeds initial consistency gate".into());
        }
        for (name, axis) in [("pitch pull", 1), ("bank right", 0), ("rudder right", 2)] {
            let mut s = Simulation::trimmed(p.clone(), 90. * KNOT, 300.)?;
            let mut c = s.controls;
            match axis {
                0 => c.roll = 0.1,
                1 => c.pitch = 0.1,
                _ => c.yaw = 0.1,
            };
            for _ in 0..60 {
                s.step(c, STEP)?;
            }
            println!(
                "0.5 s {name}: body rate {:.4} deg/s",
                s.state.rates[axis].to_degrees()
            );
            if s.state.rates[axis] <= 0. {
                return Err(format!("Incorrect initial {name} control sign"));
            }
        }
        let mut s = Simulation::trimmed(p.clone(), 90. * KNOT, 1500.)?;
        let mut c = s.controls;
        c.throttle = 1.;
        for _ in 0..3600 {
            s.step(c, STEP)?;
        }
        let t = s.telemetry()?;
        println!(
            "30 s full-power response: speed {:.2} kt, height change {:.2} m, climb {:.2} m/s",
            t.speed / KNOT,
            t.altitude - 1500.,
            t.climb
        );
        let mut s = Simulation::trimmed(p.clone(), 65. * KNOT, 1500.)?;
        let mut c = s.controls;
        c.throttle = 0.;
        c.pitch = 0.35;
        let mut stalled = false;
        for _ in 0..2400 {
            s.step(c, STEP)?;
            stalled |= s.telemetry()?.stalled;
        }
        println!("20 s power-off pull: post-peak lift angle observed = {stalled}");
        if !stalled {
            return Err("Stall scenario did not reach the post-peak lift curve".into());
        }
        println!("PASS: deterministic trim/control/stall consistency. No comparison against recorded FSX flight data yet.");
    }
    Ok(())
}

fn inspect(root: &Path, windows_1252: bool) -> Result<(), String> {
    if !root.join("fsx.exe").is_file() {
        return Err("Expected an FSX installation folder containing fsx.exe".into());
    }
    let folder = root.join("SimObjects").join("Airplanes");
    let mut entries = fs::read_dir(&folder)
        .map_err(|e| format!("{}: {e}", folder.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|e| e.file_name());
    let mut count = 0;
    let mut failed = 0;
    for entry in entries {
        let path = entry.path().join("aircraft.cfg");
        if !path.is_file() {
            continue;
        }
        match fs::read(&path) {
            Ok(bytes) => match decode(&bytes, windows_1252) {
                Ok(text) => {
                    for title in titles(&text) {
                        println!("{title}  [{}]", entry.file_name().to_string_lossy());
                        count += 1;
                    }
                }
                Err(e) => {
                    eprintln!("{}: {e}", path.display());
                    failed += 1;
                }
            },
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                failed += 1;
            }
        }
    }
    println!("\n{count} aircraft variants found; {failed} files could not be inspected.");
    if failed > 0 {
        return Err("Inspection incomplete; see diagnostics above".into());
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if (args.len() == 2 || (args.len() == 3 && args[2] == "--windows-1252"))
        && (args[0] == "inspect-aircraft"
            || args[0] == "flight-check"
            || args[0] == "takeoff-check")
    {
        if args[0] == "takeoff-check" {
            return match takeoff_report(Path::new(&args[1]), args.len() == 3) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("Error: {error}");
                    ExitCode::FAILURE
                }
            };
        }
        return match aircraft_report(
            Path::new(&args[1]),
            args.len() == 3,
            args[0] == "flight-check",
        ) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Error: {error}");
                ExitCode::FAILURE
            }
        };
    }
    if args.len() == 2 && args[0] == "inspect-model" {
        return match fsx_model::Model::load(Path::new(&args[1])) {
            Ok(model) => {
                println!(
                    "MDLX: {} | {} textures | {} animated nodes",
                    model.name,
                    model.textures.len(),
                    model.animated_nodes
                );
                for (&lod, parts) in &model.lods {
                    match model.geometry(Some(lod)) {
                        Ok(mesh) => println!("LOD {lod}: {} parts, {} triangles, {} skinned parts omitted; bounds {:?} .. {:?}", parts.len(),mesh.vertices.len()/3,mesh.skipped_skinned_parts,mesh.min,mesh.max),
                        Err(e) => { eprintln!("LOD {lod}: {e}"); return ExitCode::FAILURE; }
                    }
                }
                for (index, clip) in model.animations.iter().enumerate() {
                    let driver = clip
                        .driver
                        .as_ref()
                        .map(|d| {
                            format!(
                                "{} # {} (scale {}, bias {}, lag {:?})",
                                d.variable, d.units, d.scale, d.bias, d.lag
                            )
                        })
                        .unwrap_or_else(|| "code-driven or standard; no direct Sim binding".into());
                    println!(
                        "Animation {index}: {} | {} frames | {} tracks | {driver}",
                        clip.kind,
                        clip.length,
                        clip.track_count()
                    );
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Error: {e}");
                ExitCode::FAILURE
            }
        };
    }
    if !(args.len() == 2 || (args.len() == 3 && args[2] == "--windows-1252"))
        || args[0] != "inspect"
    {
        eprintln!("Usage:\n  openfsx inspect <FSX folder> [--windows-1252]\n  openfsx inspect-model <model.mdl>\n  openfsx inspect-aircraft <aircraft folder> [--windows-1252]\n  openfsx flight-check <aircraft folder> [--windows-1252]\n  openfsx takeoff-check <aircraft folder> [--windows-1252]\nFor the experimental model viewer, build package fsx-app with --features viewer (binary openfsx-viewer).");
        return ExitCode::from(2);
    }
    match inspect(Path::new(&args[1]), args.len() == 3) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_encoding_requires_explicit_choice() {
        let bytes = b"[fltsim.0]\ntitle=Caf\xe9 \x96 Trainer";
        assert!(decode(bytes, false).is_err());
        assert_eq!(
            titles(&decode(bytes, true).unwrap()),
            vec!["Café – Trainer"]
        );
    }
    #[test]
    fn variants_ignore_other_sections_and_comments() {
        let cfg = "\u{feff}; test\n[FLTSIM.0]\n TITLE = \"Trainer\" ; comment\n[General]\ntitle=ignored\n[fltsim.1]\ntitle=Floatplane\n[fltsim.bad]\ntitle=ignored";
        assert_eq!(titles(cfg), vec!["Trainer", "Floatplane"]);
    }
    #[test]
    fn malformed_and_empty_sections_do_not_leak() {
        assert!(
            titles("[fltsim.]\ntitle=x\n[fltsim.0\ntitle=y\n[fltsim.1]\ntitle= ;empty").is_empty()
        );
    }
}
