//! C172 English-unit ideal-ISA binding to the isolated native update profile.
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
}
impl FlightGauges {
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
        for (library, gauge, ..) in BINDINGS {
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
        })
    }
    pub fn pid(&self) -> u32 {
        self.native.pid()
    }
    pub fn update(&mut self, simulation: &Simulation) -> Result<Frame, String> {
        let inputs = AirData::from_telemetry(simulation.telemetry()?)?.native_inputs();
        self.native.update(simulation.time, inputs)
    }
    pub fn apply(
        &self,
        frame: &Frame,
        model: &Model,
        appearance: &mut Appearance,
        mut gpu: Option<&mut Gpu>,
    ) -> Result<(), String> {
        let mut drawn = 0;
        for (index, base) in &self.bases {
            let p = &self.config.panels[*index];
            let Kind::VirtualCockpit { texture } = &p.kind else {
                unreachable!()
            };
            let mut preview = base.clone();
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
        if drawn != 5 {
            return Err("Native flight profile did not draw all five bound needles".into());
        }
        Ok(())
    }
    pub fn close(&mut self) -> Result<(), String> {
        self.native.close()
    }
}
