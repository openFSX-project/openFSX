//! C172 ideal ISA/gyro binding to the isolated native update profile.
use crate::native_gauges::{Frame, NativeGauges, BINDINGS};
use fsx_model::Model;
use fsx_render::{panel, Appearance, Gpu};
use fsx_sim::{air_data::AirData, flight::Simulation};
use fsx_vehicle::{
    gauge::GaugeLibrary,
    panel::{Kind, PanelConfig},
};
use std::{collections::BTreeMap, path::Path};

pub struct FlightGauges {
    native: NativeGauges,
    config: PanelConfig,
    libraries: BTreeMap<String, GaugeLibrary>,
    bases: Vec<(usize, panel::Preview)>,
    clock: Option<(fsx_sim::clock::LocalClock, fsx_render::xml_text::GaugeFont)>,
}
impl FlightGauges {
    /// Explicit local-time origin, unselected authored daylight clock styling.
    pub fn with_day_native_clock(
        mut self,
        root: &Path,
        clock: fsx_sim::clock::LocalClock,
    ) -> Result<Self, String> {
        let font = fsx_render::xml_text::GaugeFont::load(&root.join("Fonts/quartz.ttf"))?;
        let gauge = self.libraries["cessna172"].gauge("Clock")?;
        let candidates: Vec<_> = gauge
            .elements
            .iter()
            .filter(|e| e.kind == 2 && e.text.as_ref().is_some_and(|t| t.tokens == [3, 4, 5]))
            .collect();
        if candidates.len() != 1 {
            return Err("Expected one C172 clock string".into());
        }
        let e = candidates[0];
        let style = e.text.as_ref().unwrap();
        if style.source_callbacks.iter().any(Option::is_some)
            || style.update_callback_rva.is_none()
            || style.max_length != 8
        {
            return Err("Unsupported native clock descriptor".into());
        }
        fsx_render::native_text::rasterize_unselected(&font, style, e.image_flags, "00:00:00")?;
        let count = self
            .bases
            .iter()
            .flat_map(|(i, _)| &self.config.panels[*i].gauges)
            .filter(|g| {
                g.module.eq_ignore_ascii_case("Cessna172")
                    && g.name.eq_ignore_ascii_case("Clock")
                    && g.parameters.is_none()
            })
            .count();
        if count != 1 {
            return Err("Expected one parameter-free C172 clock placement".into());
        }
        self.native.enable_clock()?;
        self.clock = Some((clock, font));
        println!("Native clock: explicit local-time origin, installed Quartz, unselected authored daylight style; no timer buttons or FSX pixel parity");
        Ok(self)
    }

    /// Explicit-font XML plaque in the current daylight-only flight environment.
    /// This does not infer the FSX default font or simulate panel-light controls.
    pub fn with_day_xml_plaque(
        mut self,
        root: &Path,
        font: &Path,
        registration: &str,
    ) -> Result<Self, String> {
        use fsx_vehicle::{cabinet::Cabinet, xml_gauge::GaugeDocument, xml_text::TextGauge};
        let cabinet = Cabinet::load(&root.join("Gauges/n_number_plaque.cab"), true)?;
        let document = GaugeDocument::from_cabinet(&cabinet, "n_number_plaque.xml", false)?;
        let text = TextGauge::from_document(&document)?;
        let font = fsx_render::xml_text::GaugeFont::load(font)?;
        let rendered = font.render(
            &text,
            &cabinet,
            "n_number_plaque.xml",
            |namespace, name, units| {
                if namespace == 'P'
                    && name.eq_ignore_ascii_case("Time of Day")
                    && units.eq_ignore_ascii_case("enum")
                {
                    Some(1.)
                } else if namespace == 'A'
                    && name.eq_ignore_ascii_case("Light Panel")
                    && units.eq_ignore_ascii_case("bool")
                {
                    Some(0.)
                } else {
                    None
                }
            },
            |name| {
                name.eq_ignore_ascii_case("ATC ID")
                    .then(|| registration.to_owned())
            },
        )?;
        let mut count = 0;
        for (index, preview) in &mut self.bases {
            let p = &self.config.panels[*index];
            for placement in p.gauges.iter().filter(|g| {
                g.module.eq_ignore_ascii_case("n_number_plaque")
                    && g.name.eq_ignore_ascii_case("n_number_plaque")
            }) {
                if placement.parameters.is_some() {
                    return Err("XML plaque parameters are unsupported".into());
                }
                panel::paint_xml_image(preview, p, placement, &rendered.image)?;
                preview.diagnostics.retain(|d| {
                    !d.to_ascii_lowercase()
                        .starts_with("n_number_plaque!n_number_plaque:")
                });
                count += 1;
            }
        }
        if count != 1 {
            return Err("XML flight profile requires exactly one C172 VC plaque".into());
        }
        println!("XML plaque: {registration:?}, explicit font {:?}, default-font mapping {}, daylight profile; FSX rasterization parity unverified", font.name(), rendered.explicit_default_font);
        Ok(self)
    }
    pub fn new(root: &Path, panel_path: &Path, helper: &Path) -> Result<Self, String> {
        let config = PanelConfig::load(panel_path, true)?;
        let mut libraries = BTreeMap::new();
        for name in [
            "Cessna.dll",
            "Cessna172.dll",
            "Cessna182s.dll",
            "Bell_206B.dll",
            "Bendix_king_radio.dll",
        ] {
            libraries.insert(
                name.strip_suffix(".dll").unwrap().to_ascii_lowercase(),
                GaugeLibrary::load(&root.join("Gauges").join(name), true)?,
            );
        }
        let mut bases = Vec::new();
        for (i, p) in config.panels.iter().enumerate() {
            if matches!(p.kind, Kind::VirtualCockpit { .. }) {
                let preview = panel::static_roots(p, &libraries, 1033)?;
                println!(
                    "Native panel {:?}: {} roots, {} diagnostic omissions",
                    p.kind,
                    preview.drawn_roots,
                    preview.diagnostics.len()
                );
                bases.push((i, preview));
            }
        }
        if bases.len() != 2 {
            return Err("Native flight profile requires the two checked C172 VC panels".into());
        }
        for (library, gauge) in BINDINGS.iter().map(|b| (b.0, b.1)).chain([
            ("Cessna.dll", "Heading_Indicator"),
            ("Bell_206B.dll", "Attitude"),
        ]) {
            let module = library.strip_suffix(".dll").unwrap();
            let matches = bases
                .iter()
                .flat_map(|(i, _)| &config.panels[*i].gauges)
                .filter(|g| {
                    g.module.eq_ignore_ascii_case(module) && g.name.eq_ignore_ascii_case(gauge)
                })
                .count();
            if matches != 1 {
                return Err(format!("Native C172 flight profile requires exactly one VC placement of {module}!{gauge}"));
            }
        }
        Ok(Self {
            native: NativeGauges::new(helper, root)?,
            config,
            libraries,
            bases,
            clock: None,
        })
    }
    pub fn pid(&self) -> u32 {
        self.native.pid()
    }
    #[allow(dead_code)] // Explicit clock epochs are used by the offline recording consumer.
    pub fn restart(&mut self) -> Result<(), String> {
        self.native.restart()
    }
    pub fn update_inputs(&mut self, time: f64, inputs: [f64; 6]) -> Result<Frame, String> {
        if let Some((clock, _)) = &self.clock {
            self.native
                .update_with_clock(time, inputs, Some(clock.seconds_at(time)?))
        } else {
            self.native.update(time, inputs)
        }
    }
    pub fn update(&mut self, simulation: &Simulation) -> Result<Frame, String> {
        let telemetry = simulation.telemetry()?;
        let air = AirData::from_telemetry(telemetry)?.native_inputs();
        let attitude = attitude_inputs(telemetry.attitude)?;
        self.update_inputs(
            simulation.time,
            [
                air[0],
                air[1],
                air[2],
                attitude[0],
                attitude[1],
                attitude[2],
            ],
        )
    }
    pub fn apply(
        &self,
        frame: &Frame,
        model: &Model,
        appearance: &mut Appearance,
        mut gpu: Option<&mut Gpu>,
    ) -> Result<(), String> {
        let mut drawn = 0;
        let mut sprites_drawn = 0;
        let mut strings_drawn = 0;
        if self.clock.is_some() != frame.clock.is_some() {
            return Err("Native clock result/profile mismatch".into());
        }
        for (index, base) in &self.bases {
            let p = &self.config.panels[*index];
            let Kind::VirtualCockpit { texture } = &p.kind else {
                unreachable!()
            };
            let mut preview = base.clone();
            for value in &frame.sprites {
                let module = value
                    .library
                    .strip_suffix(".dll")
                    .ok_or("Invalid sprite library")?
                    .to_ascii_lowercase();
                let Some(placement) = p.gauges.iter().find(|g| {
                    g.module.eq_ignore_ascii_case(&module)
                        && g.name.eq_ignore_ascii_case(&value.gauge)
                }) else {
                    continue;
                };
                let library = self
                    .libraries
                    .get(&module)
                    .ok_or("Missing sprite library")?;
                let gauge = library.gauge(&value.gauge)?;
                let descriptor = gauge
                    .elements
                    .iter()
                    .find(|e| e.kind == 6 && e.resource_id == value.resource)
                    .and_then(|e| e.sprite.as_ref())
                    .ok_or("Missing sprite descriptor")?;
                for i in 0..3 {
                    if descriptor.axes[i].token != value.tokens[i]
                        || descriptor.axes[i].update_callback_rva.is_some() != value.callbacks[i]
                        || descriptor.axes[i].source_callback_rva.is_some()
                    {
                        return Err("Sprite binding differs from installed metadata".into());
                    }
                }
                panel::paint_sprite(
                    &mut preview,
                    p,
                    placement,
                    library,
                    value.resource,
                    1033,
                    value.results,
                )?;
                sprites_drawn += 1;
            }
            // The C172 attitude foreground uses neutral bars plus two fixed images.
            // Its adjustable bars, failure state and pitch knob are not simulated.
            for placement in &p.gauges {
                if placement.module.eq_ignore_ascii_case("bell_206b")
                    && placement.name.eq_ignore_ascii_case("Attitude")
                {
                    for resource in [2013, 2001, 12007] {
                        panel::paint_fixed_layer(
                            &mut preview,
                            p,
                            placement,
                            &self.libraries["bell_206b"],
                            resource,
                            1033,
                        )?;
                    }
                }
            }
            for value in &frame.values {
                let module = value
                    .library
                    .strip_suffix(".dll")
                    .ok_or("Invalid native module suffix")?
                    .to_ascii_lowercase();
                let Some(placement) = p.gauges.iter().find(|g| {
                    g.module.eq_ignore_ascii_case(&module)
                        && g.name.eq_ignore_ascii_case(&value.gauge)
                }) else {
                    continue;
                };
                let library = &self.libraries[&module];
                let gauge = library.gauge(&value.gauge)?;
                let element = gauge
                    .elements
                    .iter()
                    .find(|e| e.kind == 1 && e.resource_id == value.resource)
                    .ok_or("Native flight needle metadata missing")?;
                let needle = element
                    .needle
                    .as_ref()
                    .ok_or("Native flight needle descriptor missing")?;
                if needle.token != value.token || needle.update_callback_rva.is_none() {
                    return Err("Native flight binding differs from loaded metadata".into());
                }
                let angle = if needle.curve.is_empty() {
                    if module != "cessna" || value.gauge != "Altimeter" {
                        return Err("Unsupported direct-angle native flight binding".into());
                    }
                    value.result.to_radians()
                } else {
                    needle.reading_angle(element.position, value.result)?
                };
                panel::paint_needle(
                    &mut preview,
                    p,
                    placement,
                    library,
                    value.resource,
                    1033,
                    angle,
                )?;
                drawn += 1;
            }
            if let (Some((_, font)), Some(value)) = (&self.clock, &frame.clock) {
                for placement in p.gauges.iter().filter(|g| {
                    g.module.eq_ignore_ascii_case("Cessna172")
                        && g.name.eq_ignore_ascii_case("Clock")
                }) {
                    let library = &self.libraries["cessna172"];
                    let gauge = library.gauge("Clock")?;
                    let e = gauge
                        .elements
                        .iter()
                        .find(|e| e.kind == 2 && e.rva == value.element_rva)
                        .ok_or("Native clock RVA does not match installed metadata")?;
                    let style = e
                        .text
                        .as_ref()
                        .filter(|t| t.tokens == [3, 4, 5] && t.max_length == 8)
                        .ok_or("Native clock style mismatch")?;
                    let raster = fsx_render::native_text::rasterize_unselected(
                        font,
                        style,
                        e.image_flags,
                        &value.text,
                    )?;
                    panel::paint_native_text(
                        &mut preview,
                        p,
                        placement,
                        library,
                        e.rva,
                        1033,
                        &raster.image,
                    )?;
                    strings_drawn += 1;
                }
            }
            if !model
                .textures
                .iter()
                .any(|n| n.eq_ignore_ascii_case(texture))
            {
                return Err(
                    "Native flight panel does not match the selected interior model".into(),
                );
            }
            if let Some(gpu) = gpu.as_deref_mut() {
                gpu.update_image(texture, &preview.image)?;
            }
            appearance.bind_image(texture, preview.image)?;
        }
        if drawn != 5 || sprites_drawn != 3 || strings_drawn != usize::from(self.clock.is_some()) {
            return Err(
                "Native flight profile did not draw all configured needles, sprites and strings"
                    .into(),
            );
        }
        Ok(())
    }
    pub fn close(&mut self) -> Result<(), String> {
        self.native.close()
    }
}

/// Ideal gyro adapter: simulator body angles are nose-up/right-bank positive;
/// legacy gauge pitch/bank use nose-down/left-bank positive. No drift/magnetism.
fn attitude_inputs(attitude: [f64; 3]) -> Result<[f64; 3], String> {
    if attitude.iter().any(|v| !v.is_finite()) {
        return Err("Invalid instrument attitude".into());
    }
    Ok([
        -attitude[1].to_degrees(),
        -attitude[0].to_degrees(),
        attitude[2].to_degrees().rem_euclid(360.),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ideal_gyro_units_signs_and_heading_wrap() {
        let v = attitude_inputs([
            30f64.to_radians(),
            10f64.to_radians(),
            (-90f64).to_radians(),
        ])
        .unwrap();
        for (actual, expected) in v.into_iter().zip([-10., -30., 270.]) {
            assert!((actual - expected).abs() < 1e-10);
        }
        assert!(attitude_inputs([0., f64::NAN, 0.]).is_err());
    }
}
