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

fn decode(bytes: &[u8], windows_1252: bool) -> Result<String, String> {
    if windows_1252 {
        // Explicit opt-in: a legacy file's code page cannot be inferred reliably.
        const EXTENDED: [char; 32] = [
            '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
            '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ',
            '\u{9d}', 'ž', 'Ÿ',
        ];
        return Ok(bytes
            .iter()
            .map(|&b| {
                if (0x80..=0x9f).contains(&b) {
                    EXTENDED[(b - 0x80) as usize]
                } else {
                    char::from(b)
                }
            })
            .collect());
    }
    std::str::from_utf8(bytes).map(str::to_owned).map_err(|_| {
        "Unsupported encoding. For Western European legacy files, pass --windows-1252".into()
    })
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
    if args.len() == 2 && args[0] == "inspect-model" {
        return match openfsx::mdl::Model::load(Path::new(&args[1])) {
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
        eprintln!("Usage:\n  openfsx inspect <FSX folder> [--windows-1252]\n  openfsx inspect-model <model.mdl>\nFor the experimental model viewer, build openfsx-viewer with --features viewer.");
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
