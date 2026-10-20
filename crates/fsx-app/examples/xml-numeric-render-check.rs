//! Synthetic numeric ACE text through the actual XML/script/font pipeline.
//! Uses an explicitly supplied installed CAB background; no aircraft gauge is
//! replaced or claimed operational by these synthetic readings.
use fsx_render::xml_text::GaugeFont;
use fsx_vehicle::{
    cabinet::{resource_key, Cabinet},
    xml_gauge::GaugeDocument,
    xml_text::TextGauge,
};
use serde_json::json;
use std::{fs, path::PathBuf, process::ExitCode};

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 4 {
        return Err("Usage: xml-numeric-render-check <cabinet.cab> <background member.bmp> <font.ttf> <new output directory>".into());
    }
    let member = args[1].to_str().ok_or("Invalid background resource")?;
    resource_key(member)?;
    let escaped = member
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;");
    let xml = format!(
        r#"<SimBase.Document Type="AceXML"><SimGauge.Gauge id="synthetic numeric check"><Size>400,68</Size><Image Name="{escaped}"/>
<Element><Visibility>(A:ELECTRICAL MASTER BATTERY, bool)</Visibility>
<Element><FloatPosition>8,8</FloatPosition><Visibility>(A:AVIONICS MASTER SWITCH, bool)</Visibility><GaugeText><Size>384,24</Size><FontHeight>18</FontHeight><FontColor>White</FontColor><Transparent>True</Transparent><GaugeString>COM %((A:COM ACTIVE FREQUENCY:1, MHz))%!6.3f! ALT %((A:INDICATED ALTITUDE, feet))%!+06d!</GaugeString></GaugeText></Element>
<Element><FloatPosition>8,34</FloatPosition><GaugeText><Size>384,24</Size><FontHeight>18</FontHeight><FontColor>White</FontColor><Transparent>True</Transparent><GaugeString>VSI %((A:VERTICAL SPEED, feet per minute))%!+06d! HDG %((A:PLANE HEADING DEGREES MAGNETIC, degrees))%!03d!</GaugeString></GaugeText></Element>
</Element></SimGauge.Gauge></SimBase.Document>"#
    );
    let doc = GaugeDocument::parse(xml.as_bytes(), false)?;
    let gauge = TextGauge::from_document(&doc)?;
    let cabinet = Cabinet::load(&PathBuf::from(&args[0]), true)?;
    let font = GaugeFont::load(&PathBuf::from(&args[2]))?;
    let output = PathBuf::from(&args[3]);
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    let mut reports = Vec::new();
    let mut previous = None;
    let mut restored_reference = None;
    for (index, (values, power, expected)) in [
        (
            [118.025, 2540.6, -450.3, 359.4],
            [true, true],
            ["COM 118.025 ALT +02541", "VSI -00450 HDG 359"],
        ),
        (
            [121.5, -72.4, 700.9, 0.],
            [true, true],
            ["COM 121.500 ALT -00072", "VSI +00701 HDG 000"],
        ),
        (
            [121.5, -72.4, 700.9, 0.],
            [true, false],
            ["", "VSI +00701 HDG 000"],
        ),
        ([121.5, -72.4, 700.9, 0.], [false, false], ["", ""]),
        (
            [121.5, -72.4, 700.9, 0.],
            [true, true],
            ["COM 121.500 ALT -00072", "VSI +00701 HDG 000"],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut reads = Vec::new();
        let rendered = font.render(
            &gauge,
            &cabinet,
            "synthetic.xml",
            |namespace, name, units| {
                reads.push(name.to_owned());
                if namespace != 'A' {
                    return None;
                }
                match (name, units) {
                    ("ELECTRICAL MASTER BATTERY", "bool") => Some(u8::from(power[0]) as f64),
                    ("AVIONICS MASTER SWITCH", "bool") if power[0] => {
                        Some(u8::from(power[1]) as f64)
                    }
                    _ if !power[0] => None,
                    ("COM ACTIVE FREQUENCY:1", _) | ("INDICATED ALTITUDE", _) if !power[1] => None,
                    ("COM ACTIVE FREQUENCY:1", "MHz") => Some(values[0]),
                    ("INDICATED ALTITUDE", "feet") => Some(values[1]),
                    ("VERTICAL SPEED", "feet per minute") => Some(values[2]),
                    ("PLANE HEADING DEGREES MAGNETIC", "degrees") => Some(values[3]),
                    _ => None,
                }
            },
            |_| None,
        )?;
        let actual: Vec<String> = rendered
            .spans
            .iter()
            .map(|s| s.iter().map(|s| s.text.as_str()).collect())
            .collect();
        if actual != expected {
            return Err(format!("Numeric XML output differs: {actual:?}"));
        }
        if reads
            .iter()
            .filter(|s| s.as_str() == "ELECTRICAL MASTER BATTERY")
            .count()
            != 1
            || (!power[0] && reads.len() != 1)
            || rendered.visible != [power[0] && power[1], power[0]]
        {
            return Err(format!("Visibility inheritance/cache failed: {reads:?}"));
        }
        if index == 1 {
            restored_reference = Some(rendered.image.rgba.clone());
        }
        if index == 4 && restored_reference.as_ref() != Some(&rendered.image.rgba) {
            return Err("Restored visibility did not restore identical pixels".into());
        }
        if previous.as_ref() == Some(&rendered.image.rgba) {
            return Err("Changed numeric readings did not change raster pixels".into());
        }
        let changed = previous.as_ref().map(|p: &Vec<u8>| {
            p.chunks_exact(4)
                .zip(rendered.image.rgba.chunks_exact(4))
                .filter(|(a, b)| a != b)
                .count()
        });
        let mut ppm = format!(
            "P6\n{} {}\n255\n",
            rendered.image.width, rendered.image.height
        )
        .into_bytes();
        for pixel in rendered.image.rgba.chunks_exact(4) {
            ppm.extend_from_slice(&pixel[..3]);
        }
        fs::write(output.join(format!("numeric-{index}.ppm")), ppm).map_err(|e| e.to_string())?;
        previous = Some(rendered.image.rgba);
        reports.push(json!({"source":"original synthetic numeric ACE check", "inputs":values, "text":actual, "changed_pixels":changed,
            "synthetic_master_power":power[0], "synthetic_avionics_power":power[1], "visible":rendered.visible, "variable_reads":reads,
            "integer_half_rounding":"away from zero", "float_rounding":"Rust fixed-format binary f64, ties to even",
            "font_name":font.name(), "font_height_interpretation":"pixels per em", "installed_background":member,
            "fsx_parity_validated":false, "operational_radio_or_altimeter":false, "electrical_simulation":false}));
    }
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&reports).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!(
        "PASS: five synthetic numeric/visibility XML cases rendered with {:?}",
        font.name()
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
