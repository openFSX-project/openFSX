//! Compare decoded CAB bytes with an independently expanded local directory.
//! Original resources stay on the user's machine; this command writes nothing.
use fsx_vehicle::cabinet::{resource_key, Cabinet};
use std::{env, fs, io::Read, path::Path, process::ExitCode};

fn run() -> Result<(), String> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err(
            "Usage: cabinet-verify <cabinet.cab> <independently expanded directory>".into(),
        );
    }
    let cabinet = Cabinet::load(Path::new(&args[0]), true)?;
    let directory = Path::new(&args[1])
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let mut total = 0;
    for entry in cabinet.entries() {
        resource_key(&entry.name)?;
        let file = directory
            .join(entry.name.replace('\\', "/"))
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !file.starts_with(&directory) || !file.is_file() {
            return Err("Reference member is not a contained regular file".into());
        }
        let mut expected = Vec::new();
        fs::File::open(&file)
            .map_err(|e| e.to_string())?
            .take(entry.size() as u64 + 1)
            .read_to_end(&mut expected)
            .map_err(|e| e.to_string())?;
        if cabinet.resource(&entry.name)? != expected {
            return Err(format!("Decoded bytes differ: {}", entry.name));
        }
        total += expected.len();
        println!("MATCH {} | {} bytes", entry.name, expected.len());
    }
    println!(
        "Verified {} resources, {total} bytes; decoder is read-only.",
        cabinet.entries().len()
    );
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
