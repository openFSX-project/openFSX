//! Explicit static root-artwork inspection, not an operational gauge runtime.
use fsx_texture::{Image, MAX_DIMENSION};
use fsx_vehicle::{
    gauge::GaugeLibrary,
    panel::{Kind, Panel},
};
use std::collections::BTreeMap;

pub struct Preview {
    pub image: Image,
    pub drawn_roots: usize,
    pub omitted_elements: usize,
    pub diagnostics: Vec<String>,
}

fn extent(values: Option<[Option<f64>; 2]>) -> Result<[f64; 2], String> {
    let [Some(w), Some(h)] = values.ok_or("Static panel preview requires explicit extents")? else {
        return Err("Static panel preview requires both extent dimensions".into());
    };
    if !w.is_finite() || !h.is_finite() || w <= 0. || h <= 0. {
        return Err("Invalid panel preview extent".into());
    }
    Ok([w, h])
}

/// Draw only zero-origin static root images into their explicit CFG rectangles.
/// The caller supplies libraries and exact resource language; there is no add-on search.
pub fn static_roots(
    panel: &Panel,
    libraries: &BTreeMap<String, GaugeLibrary>,
    language: u32,
) -> Result<Preview, String> {
    if !matches!(panel.kind, Kind::VirtualCockpit { .. }) {
        return Err("Expected a virtual-cockpit panel".into());
    }
    let mm = extent(panel.size_mm)?;
    let pixels = extent(panel.pixel_size)?;
    if pixels
        .iter()
        .any(|v| v.fract() != 0. || *v > MAX_DIMENSION as f64)
    {
        return Err("Panel preview pixel dimensions must be integers within 4096".into());
    }
    if panel.gauges.len() > 4096 {
        return Err("Too many panel preview placements".into());
    }
    let (width, height) = (pixels[0] as u32, pixels[1] as u32);
    let mut image = Image {
        width,
        height,
        rgba: vec![0; width as usize * height as usize * 4],
    };
    for pixel in image.rgba.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[
            panel.background_color[0],
            panel.background_color[1],
            panel.background_color[2],
            255,
        ]);
    }
    let mut result = Preview {
        image,
        drawn_roots: 0,
        omitted_elements: 0,
        diagnostics: Vec::new(),
    };
    for placement in &panel.gauges {
        let draw = (|| -> Result<(), String> {
            let library = libraries
                .get(&placement.module.to_ascii_lowercase())
                .ok_or("No native library supplied (XML/CAB is unsupported)")?;
            let gauge = library.gauge(&placement.name)?;
            result.omitted_elements += gauge.elements.len();
            let root = gauge
                .roots
                .first()
                .and_then(|r| gauge.elements.iter().find(|e| e.rva == *r))
                .ok_or("No encoded root element")?;
            if root.kind != 0 || root.position != [0, 0] {
                return Err(
                    "Only zero-origin static roots are supported by the artwork preview".into(),
                );
            }
            if root.image_flags
                & ((1 << 12) | (1 << 13) | (1 << 25) | (1 << 27) | (1 << 29) | (1 << 30))
                != 0
            {
                return Err(
                    "Root needs unsupported masks, alpha, visibility or transform flags".into(),
                );
            }
            let id = u32::try_from(root.resource_id)
                .ok()
                .filter(|v| *v != 0)
                .ok_or("No root bitmap resource ID")?;
            let resource = library
                .resource(2, id, language)
                .ok_or("No root bitmap in the explicitly selected language")?;
            let source = Image::dib(library.resource_bytes(resource)?)?;
            let [Some(w), Some(h)] = placement.size else {
                return Err("Preview requires explicit gauge width and height".into());
            };
            let rectangle = [
                placement.position[0] / mm[0] * pixels[0],
                placement.position[1] / mm[1] * pixels[1],
                w / mm[0] * pixels[0],
                h / mm[1] * pixels[1],
            ];
            blit(
                &mut result.image,
                &source,
                rectangle,
                root.image_flags & 1 != 0,
            )?;
            result.drawn_roots += 1;
            result.omitted_elements -= 1;
            Ok(())
        })();
        if let Err(e) = draw {
            result
                .diagnostics
                .push(format!("{}!{}: {e}", placement.module, placement.name));
        }
    }
    Ok(result)
}

// Nearest-neighbor rectangle inspection. Coordinates describe pixel edges;
// destination centers select source pixels. Clipping does not stretch the source.
fn blit(target: &mut Image, source: &Image, rect: [f64; 4], black_key: bool) -> Result<(), String> {
    if rect.iter().any(|v| !v.is_finite() || v.abs() > 1e9) || rect[2] <= 0. || rect[3] <= 0. {
        return Err("Invalid panel drawing rectangle".into());
    }
    let left = rect[0].floor().clamp(0., target.width as f64) as u32;
    let top = rect[1].floor().clamp(0., target.height as f64) as u32;
    let right = (rect[0] + rect[2]).ceil().clamp(0., target.width as f64) as u32;
    let bottom = (rect[1] + rect[3]).ceil().clamp(0., target.height as f64) as u32;
    for y in top..bottom {
        for x in left..right {
            let u = (x as f64 + 0.5 - rect[0]) / rect[2];
            let v = (y as f64 + 0.5 - rect[1]) / rect[3];
            if !(0. ..1.).contains(&u) || !(0. ..1.).contains(&v) {
                continue;
            }
            let sx = (u * source.width as f64).floor() as usize;
            let sy = (v * source.height as f64).floor() as usize;
            let p = (sy * source.width as usize + sx) * 4;
            if black_key && source.rgba[p..p + 3] == [0, 0, 0] {
                continue;
            }
            let t = (y as usize * target.width as usize + x as usize) * 4;
            target.rgba[t..t + 4].copy_from_slice(&source.rgba[p..p + 4]);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pixel_centers_clipping_and_black_key_preserve_placement_order() {
        let mut target = Image {
            width: 4,
            height: 2,
            rgba: vec![255; 32],
        };
        let source = Image {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 0, 0, 255],
        };
        blit(&mut target, &source, [-1., 0., 4., 2.], true).unwrap();
        assert_eq!(
            &target.rgba[..16],
            &[255, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255]
        );
        blit(&mut target, &source, [0., 0., 2., 1.], false).unwrap();
        assert_eq!(&target.rgba[..8], &[255, 0, 0, 255, 0, 0, 0, 255]);
        for rect in [
            [0., 0., 0., 1.],
            [f64::NAN, 0., 1., 1.],
            [0., 0., 1e300, 1.],
        ] {
            assert!(blit(&mut target, &source, rect, false).is_err());
        }
    }
    #[test]
    fn unsupported_gauges_are_diagnostic_and_bad_extents_fail() {
        let config=fsx_vehicle::panel::PanelConfig::parse("[VCockpit01]\ntexture=$Own\nsize_mm=4,2\npixel_size=4,2\nbackground_color=1,2,3\ngauge00=Own!Unknown,0,0,2,2").unwrap();
        let mut panel = config.panels[0].clone();
        let p = static_roots(&panel, &BTreeMap::new(), 1033).unwrap();
        assert_eq!(p.drawn_roots, 0);
        assert_eq!(p.diagnostics.len(), 1);
        assert_eq!(&p.image.rgba[..4], &[1, 2, 3, 255]);
        panel.pixel_size = Some([Some(4.5), Some(2.)]);
        assert!(static_roots(&panel, &BTreeMap::new(), 1033).is_err());
        panel.pixel_size = Some([Some(4097.), Some(2.)]);
        assert!(static_roots(&panel, &BTreeMap::new(), 1033).is_err());
    }
}
