//! Explicit root/needle artwork inspection, not an operational gauge runtime.
use fsx_texture::{Image, MAX_DIMENSION};
use fsx_vehicle::{
    gauge::GaugeLibrary,
    panel::{Kind, Panel},
};
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct Preview {
    pub image: Image,
    pub drawn_roots: usize,
    pub drawn_needles: usize,
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
        drawn_needles: 0,
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

/// Paint one needle at an explicit clockwise screen angle in gauge-background coordinates.
/// The caller supplies the callback/table interpretation; no token values are invented.
pub fn paint_needle(
    preview: &mut Preview,
    panel: &Panel,
    placement: &fsx_vehicle::panel::Gauge,
    library: &GaugeLibrary,
    resource_id: i32,
    language: u32,
    angle: f64,
) -> Result<(), String> {
    let mm = extent(panel.size_mm)?;
    let pixels = extent(panel.pixel_size)?;
    if !matches!(panel.kind, Kind::VirtualCockpit { .. })
        || pixels != [preview.image.width as f64, preview.image.height as f64]
        || preview.image.width == 0
        || preview.image.height == 0
        || preview.image.width > MAX_DIMENSION
        || preview.image.height > MAX_DIMENSION
        || preview.image.rgba.len()
            != preview.image.width as usize * preview.image.height as usize * 4
    {
        return Err("Needle overlay does not match the panel pixel extent".into());
    }
    let gauge = library.gauge(&placement.name)?;
    let root = gauge
        .roots
        .first()
        .and_then(|r| gauge.elements.iter().find(|e| e.rva == *r))
        .ok_or("No encoded gauge root")?;
    let matches: Vec<_> = gauge
        .elements
        .iter()
        .filter(|e| e.kind == 1 && e.resource_id == resource_id)
        .collect();
    if matches.len() != 1 {
        return Err("Expected one needle with the selected resource ID".into());
    }
    let element = matches[0];
    if root.kind != 0 || root.position != [0, 0] {
        return Err("Needle inspection requires a zero-origin static root".into());
    }
    // NEXT_LIST links establish drawing order, not spatial parenting. For example,
    // a barometer card can precede altimeter needles using the same background origin.
    let mut pending = vec![element.rva];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(rva) = pending.pop() {
        if !seen.insert(rva) {
            continue;
        }
        for predecessor in gauge.elements.iter().filter(|e| e.children.contains(&rva)) {
            if predecessor.image_flags & (1 << 25) != 0 {
                return Err("A preceding element hides the needle's next-list tree".into());
            }
            pending.push(predecessor.rva);
        }
    }
    if element
        .position
        .iter()
        .chain(element.offset.iter())
        .any(|v| v.unsigned_abs() > 65536)
        || element.image_flags
            & ((1 << 12) | (1 << 13) | (1 << 25) | (1 << 27) | (1 << 29) | (1 << 30))
            != 0
    {
        return Err("Unsupported needle pivot or mask/alpha/visibility/transform flags".into());
    }
    let bitmap = |id: i32| -> Result<Image, String> {
        let id = u32::try_from(id)
            .ok()
            .filter(|v| *v != 0)
            .ok_or("Invalid needle/root bitmap ID")?;
        let resource = library
            .resource(2, id, language)
            .ok_or("Missing exact-language needle/root bitmap")?;
        Image::dib(library.resource_bytes(resource)?)
    };
    let background = bitmap(root.resource_id)?;
    let source = bitmap(resource_id)?;
    let [Some(w), Some(h)] = placement.size else {
        return Err("Needle inspection requires explicit gauge size".into());
    };
    let rect = [
        placement.position[0] / mm[0] * pixels[0],
        placement.position[1] / mm[1] * pixels[1],
        w / mm[0] * pixels[0],
        h / mm[1] * pixels[1],
    ];
    rotate(
        &mut preview.image,
        &source,
        rect,
        [background.width, background.height],
        element.position,
        element.offset,
        angle,
        element.image_flags & 1 != 0,
    )?;
    preview.drawn_needles += 1;
    preview.omitted_elements = preview.omitted_elements.saturating_sub(1);
    Ok(())
}

fn rotate(
    target: &mut Image,
    source: &Image,
    rect: [f64; 4],
    background: [u32; 2],
    pivot: [i32; 2],
    offset: [i32; 2],
    angle: f64,
    black_key: bool,
) -> Result<(), String> {
    if rect.iter().any(|v| !v.is_finite() || v.abs() > 1e9)
        || rect[2] <= 0.
        || rect[3] <= 0.
        || !angle.is_finite()
        || angle.abs() > 1e12
        || background.contains(&0)
    {
        return Err("Invalid needle inspection transform".into());
    }
    let (s, c) = angle.rem_euclid(std::f64::consts::TAU).sin_cos();
    let scale = [
        rect[2] / background[0] as f64,
        rect[3] / background[1] as f64,
    ];
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for xy in [
        [0., 0.],
        [source.width as f64, 0.],
        [0., source.height as f64],
        [source.width as f64, source.height as f64],
    ] {
        let x = xy[0] - offset[0] as f64;
        let y = xy[1] - offset[1] as f64;
        let p = [
            rect[0] + (pivot[0] as f64 + c * x - s * y) * scale[0],
            rect[1] + (pivot[1] as f64 + s * x + c * y) * scale[1],
        ];
        for i in 0..2 {
            min[i] = min[i].min(p[i]);
            max[i] = max[i].max(p[i]);
        }
    }
    let left = min[0]
        .floor()
        .max(rect[0].floor())
        .clamp(0., target.width as f64) as u32;
    let top = min[1]
        .floor()
        .max(rect[1].floor())
        .clamp(0., target.height as f64) as u32;
    let right = max[0]
        .ceil()
        .min((rect[0] + rect[2]).ceil())
        .clamp(0., target.width as f64) as u32;
    let bottom = max[1]
        .ceil()
        .min((rect[1] + rect[3]).ceil())
        .clamp(0., target.height as f64) as u32;
    for y in top..bottom {
        for x in left..right {
            let gauge = [
                (x as f64 + 0.5 - rect[0]) / scale[0],
                (y as f64 + 0.5 - rect[1]) / scale[1],
            ];
            if gauge[0] < 0.
                || gauge[1] < 0.
                || gauge[0] >= background[0] as f64
                || gauge[1] >= background[1] as f64
            {
                continue;
            }
            let dx = gauge[0] - pivot[0] as f64;
            let dy = gauge[1] - pivot[1] as f64;
            let sx = (c * dx + s * dy + offset[0] as f64).floor();
            let sy = (-s * dx + c * dy + offset[1] as f64).floor();
            if sx < 0. || sy < 0. || sx >= source.width as f64 || sy >= source.height as f64 {
                continue;
            }
            let p = (sy as usize * source.width as usize + sx as usize) * 4;
            if black_key && source.rgba[p..p + 3] == [0, 0, 0] {
                continue;
            }
            let t = (y as usize * target.width as usize + x as usize) * 4;
            target.rgba[t..t + 4].copy_from_slice(&source.rgba[p..p + 4]);
        }
    }
    Ok(())
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
    fn authored_bitmap_pivot_rotates_clockwise_and_stays_in_gauge_bounds() {
        let source = Image {
            width: 3,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 0, 0, 255, 0, 255, 0, 255],
        };
        let mut target = Image {
            width: 5,
            height: 5,
            rgba: vec![255; 100],
        };
        rotate(
            &mut target,
            &source,
            [0., 0., 5., 5.],
            [5, 5],
            [2, 2],
            [0, 0],
            0.,
            true,
        )
        .unwrap();
        assert_eq!(
            &target.rgba[(2 * 5 + 4) * 4..(2 * 5 + 5) * 4],
            &[0, 255, 0, 255]
        );
        let mut turned = Image {
            width: 5,
            height: 5,
            rgba: vec![255; 100],
        };
        rotate(
            &mut turned,
            &source,
            [0., 0., 5., 5.],
            [5, 5],
            [2, 2],
            [0, 0],
            std::f64::consts::FRAC_PI_2,
            true,
        )
        .unwrap();
        assert_eq!(
            &turned.rgba[(4 * 5 + 1) * 4..(4 * 5 + 2) * 4],
            &[0, 255, 0, 255]
        );
        assert!(rotate(
            &mut turned,
            &source,
            [0., 0., 5., 5.],
            [5, 5],
            [2, 2],
            [0, 0],
            f64::NAN,
            true
        )
        .is_err());
    }
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
