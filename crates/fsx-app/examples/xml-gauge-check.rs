//! Evaluate installed ACE GaugeText expressions with explicit synthetic panel
//! conditions and the selected aircraft's authored registration. No FSX launch.
use fsx_script::gauge_string::{Color, GaugeString};
use fsx_vehicle::{
    aircraft::VisualAircraft,
    cabinet::Cabinet,
    xml_gauge::{Element, GaugeDocument},
};
use serde_json::json;
use std::{env, path::PathBuf, process::ExitCode};

fn gauge_texts<'a>(node: &'a Element, result: &mut Vec<&'a Element>) {
    if node.name == "GaugeText" {
        result.push(node);
    }
    for child in &node.children {
        gauge_texts(child, result);
    }
}
fn run() -> Result<(), String> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 5 {
        return Err("Usage: xml-gauge-check <cabinet.cab> <member.xml> <aircraft folder> <variant> <expected registration>".into());
    }
    let member = args[1].to_str().ok_or("Invalid member name")?;
    let variant = args[3]
        .to_str()
        .ok_or("Invalid variant")?
        .parse()
        .map_err(|_| "Invalid variant")?;
    let expected = args[4].to_str().ok_or("Invalid expected registration")?;
    let aircraft = VisualAircraft::load(&PathBuf::from(&args[2]), variant, true)?;
    let registration = aircraft
        .registration
        .as_deref()
        .ok_or("Selected aircraft has no authored atc_id")?;
    if registration != expected {
        return Err("Authored registration differs from expected variant registration".into());
    }
    let cabinet = Cabinet::load(&PathBuf::from(&args[0]), true)?;
    let document = GaugeDocument::from_cabinet(&cabinet, member, false)?;
    let mut texts = Vec::new();
    gauge_texts(document.gauge(), &mut texts);
    if texts.is_empty() {
        return Err("No GaugeText elements found".into());
    }
    for (index, text) in texts.iter().enumerate() {
        let source = &text
            .child("GaugeString")?
            .ok_or("GaugeText lacks GaugeString")?
            .text;
        let program = GaugeString::compile(source)?;
        for time_of_day in 1..=3 {
            for panel_light in [false, true] {
                let spans = program
                    .evaluate(
                        |namespace, name, units| {
                            if namespace == 'P'
                                && name.eq_ignore_ascii_case("Time of Day")
                                && units.eq_ignore_ascii_case("enum")
                            {
                                Some(time_of_day as f64)
                            } else if namespace == 'A'
                                && name.eq_ignore_ascii_case("Light Panel")
                                && units.eq_ignore_ascii_case("bool")
                            {
                                Some(u8::from(panel_light) as f64)
                            } else {
                                None
                            }
                        },
                        |name| {
                            name.eq_ignore_ascii_case("ATC ID")
                                .then(|| registration.to_owned())
                        },
                    )
                    .map_err(|e| format!("{e:?}"))?;
                let actual = spans.iter().map(|s| s.text.as_str()).collect::<String>();
                let expected_color = if time_of_day >= 2 && panel_light {
                    Color::Alternate
                } else {
                    Color::Default
                };
                // This checker is for the registration-plaque profile; it cannot
                // treat arbitrary successfully evaluated text as that profile.
                if actual != expected || spans.len() != 1 || spans[0].color != expected_color {
                    return Err(
                        "Plaque text/color differs from explicit day/night/light expectation"
                            .into(),
                    );
                }
                println!(
                    "{}",
                    json!({"gauge_text": index, "title": aircraft.title, "variant": variant,
                    "time_of_day": time_of_day, "panel_light": panel_light, "text": actual,
                    "color": if expected_color == Color::Alternate { "alternate" } else { "default" },
                    "inputs": "authored atc_id plus synthetic panel conditions", "fsx_parity_validated": false})
                );
            }
        }
    }
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
