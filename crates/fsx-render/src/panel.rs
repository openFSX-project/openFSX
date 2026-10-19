//! Native bitmap panel composition for checked roots, needles and masked sprites.
//! Variable lookup and gauge lifecycle belong to the application/isolated host.
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
    pub drawn_sprites: usize,
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
        drawn_sprites: 0,
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

/// Paint an authored sprite and its stationary RES_ID+1 mask/foreground.
/// Values are final X/Y/rotation callback readings before authored source scales.
pub fn paint_sprite(
    preview: &mut Preview,
    panel: &Panel,
    placement: &fsx_vehicle::panel::Gauge,
    library: &GaugeLibrary,
    resource_id: i32,
    language: u32,
    values: [f64; 3],
) -> Result<(), String> {
    let gauge = library.gauge(&placement.name)?;
    let root = gauge
        .roots
        .first()
        .and_then(|r| gauge.elements.iter().find(|e| e.rva == *r))
        .ok_or("Missing sprite gauge root")?;
    let elements: Vec<_> = gauge
        .elements
        .iter()
        .filter(|e| e.kind == 6 && e.resource_id == resource_id)
        .collect();
    if elements.len() != 1 || root.kind != 0 || root.position != [0, 0] {
        return Err("Sprite needs one selected element and a zero-origin static root".into());
    }
    let e = elements[0];
    if e.position
        .iter()
        .chain(e.offset.iter())
        .any(|v| v.unsigned_abs() > 65536)
        || e.image_flags & ((1 << 12) | (1 << 13) | (1 << 25) | (1 << 27) | (1 << 29) | (1 << 30))
            != 0
    {
        return Err("Unsupported sprite placement or mask/alpha/visibility flags".into());
    }
    let sprite = e.sprite.as_ref().ok_or("Missing sprite descriptor")?;
    let bitmap = |id: i32| -> Result<Image, String> {
        let id = u32::try_from(id)
            .ok()
            .filter(|i| *i != 0)
            .ok_or("Invalid sprite resource ID")?;
        let r = library
            .resource(2, id, language)
            .ok_or("Missing exact-language sprite bitmap")?;
        Image::dib(library.resource_bytes(r)?)
    };
    let background = bitmap(root.resource_id)?;
    let source = bitmap(resource_id)?;
    let mask = bitmap(
        resource_id
            .checked_add(1)
            .ok_or("Sprite mask ID overflow")?,
    )?;
    let rect = panel_rectangle(preview, panel, placement)?;
    let scaled = std::array::from_fn(|i| values[i] * sprite.axes[i].scale);
    sprite_blit(
        &mut preview.image,
        &source,
        &mask,
        rect,
        [background.width, background.height],
        e.position,
        e.offset,
        sprite.texture_scale,
        scaled,
        e.image_flags & 1 != 0,
    )?;
    preview.drawn_sprites += 1;
    preview.omitted_elements = preview.omitted_elements.saturating_sub(1);
    Ok(())
}

fn panel_rectangle(
    preview: &Preview,
    panel: &Panel,
    placement: &fsx_vehicle::panel::Gauge,
) -> Result<[f64; 4], String> {
    let mm = extent(panel.size_mm)?;
    let pixels = extent(panel.pixel_size)?;
    if !matches!(panel.kind, Kind::VirtualCockpit { .. })
        || pixels != [preview.image.width as f64, preview.image.height as f64]
    {
        return Err("Sprite overlay does not match VC pixel extents".into());
    }
    validate_image(&preview.image)?;
    let [Some(w), Some(h)] = placement.size else {
        return Err("Sprite requires explicit gauge size".into());
    };
    let rect = [
        placement.position[0] / mm[0] * pixels[0],
        placement.position[1] / mm[1] * pixels[1],
        w / mm[0] * pixels[0],
        h / mm[1] * pixels[1],
    ];
    if rect.iter().any(|v| !v.is_finite() || v.abs() > 1e9) || rect[2] <= 0. || rect[3] <= 0. {
        return Err("Invalid sprite drawing rectangle".into());
    }
    Ok(rect)
}

fn validate_image(image: &Image) -> Result<(), String> {
    if image.width == 0
        || image.height == 0
        || image.width > MAX_DIMENSION
        || image.height > MAX_DIMENSION
        || image.rgba.len() != image.width as usize * image.height as usize * 4
    {
        return Err("Invalid sprite RGBA extent".into());
    }
    Ok(())
}

// The SDK sprite rotation is counterclockwise in screen coordinates. Source
// translation samples an offset texture; rotation precedes nonuniform scaling.
fn sprite_blit(
    target: &mut Image,
    source: &Image,
    mask: &Image,
    rect: [f64; 4],
    background: [u32; 2],
    position: [i32; 2],
    center: [i32; 2],
    texture_scale: [f64; 2],
    values: [f64; 3],
    black_key: bool,
) -> Result<(), String> {
    for image in [&*target, source, mask] {
        validate_image(image)?;
    }
    if rect.iter().any(|v| !v.is_finite() || v.abs() > 1e9)
        || rect[2] <= 0.
        || rect[3] <= 0.
        || background.contains(&0)
        || background.iter().any(|v| *v > MAX_DIMENSION)
        || texture_scale
            .iter()
            .any(|v| !v.is_finite() || v.abs() < 1e-6 || v.abs() > 65536.)
        || values.iter().any(|v| !v.is_finite() || v.abs() > 1e9)
        || position
            .iter()
            .chain(center.iter())
            .any(|v| v.unsigned_abs() > 65536)
    {
        return Err("Invalid sprite transform".into());
    }
    let scale = [
        rect[2] / background[0] as f64,
        rect[3] / background[1] as f64,
    ];
    let left = (rect[0] + position[0] as f64 * scale[0])
        .floor()
        .max(rect[0].floor())
        .clamp(0., target.width as f64) as u32;
    let top = (rect[1] + position[1] as f64 * scale[1])
        .floor()
        .max(rect[1].floor())
        .clamp(0., target.height as f64) as u32;
    let right = (rect[0] + (position[0] as f64 + mask.width as f64) * scale[0])
        .ceil()
        .min((rect[0] + rect[2]).ceil())
        .clamp(0., target.width as f64) as u32;
    let bottom = (rect[1] + (position[1] as f64 + mask.height as f64) * scale[1])
        .ceil()
        .min((rect[1] + rect[3]).ceil())
        .clamp(0., target.height as f64) as u32;
    let (s, c) = values[2].rem_euclid(360.).to_radians().sin_cos();
    for y in top..bottom {
        for x in left..right {
            let p = [
                (x as f64 + 0.5 - rect[0]) / scale[0],
                (y as f64 + 0.5 - rect[1]) / scale[1],
            ];
            if p[0] < 0.
                || p[1] < 0.
                || p[0] >= background[0] as f64
                || p[1] >= background[1] as f64
            {
                continue;
            }
            let mx = (p[0] - position[0] as f64).floor();
            let my = (p[1] - position[1] as f64).floor();
            if mx < 0. || my < 0. || mx >= mask.width as f64 || my >= mask.height as f64 {
                continue;
            }
            let mi = (my as usize * mask.width as usize + mx as usize) * 4;
            let ti = (y as usize * target.width as usize + x as usize) * 4;
            if mask.rgba[mi..mi + 3] == [0, 0, 0] {
                continue; // Outside the aperture: retain the preceding gauge layers.
            }
            if mask.rgba[mi..mi + 3] != [1, 1, 1] {
                target.rgba[ti..ti + 4].copy_from_slice(&mask.rgba[mi..mi + 4]);
                continue;
            }
            let dx = (p[0] - (center[0] as f64 + 0.5) + values[0]) / texture_scale[0];
            let dy = (p[1] - (center[1] as f64 + 0.5) + values[1]) / texture_scale[1];
            let sx = (c * dx - s * dy + (source.width / 2) as f64 + 0.5).floor();
            let sy = (s * dx + c * dy + (source.height / 2) as f64 + 0.5).floor();
            if sx < 0. || sy < 0. || sx >= source.width as f64 || sy >= source.height as f64 {
                continue;
            }
            let si = (sy as usize * source.width as usize + sx as usize) * 4;
            if black_key && source.rgba[si..si + 3] == [0, 0, 0] {
                continue;
            }
            target.rgba[ti..ti + 4].copy_from_slice(&source.rgba[si..si + 4]);
        }
    }
    Ok(())
}

/// Draw an explicitly selected fixed layer at its authored background coordinates.
/// Slider support here is a zero-adjustment inspection only, not token evaluation.
pub fn paint_fixed_layer(
    preview: &mut Preview,
    panel: &Panel,
    placement: &fsx_vehicle::panel::Gauge,
    library: &GaugeLibrary,
    resource_id: i32,
    language: u32,
) -> Result<(), String> {
    let gauge = library.gauge(&placement.name)?;
    let selected: Vec<_> = gauge
        .elements
        .iter()
        .filter(|e| e.resource_id == resource_id)
        .collect();
    if selected.len() != 1 {
        return Err("Expected one fixed gauge layer".into());
    }
    let e = selected[0];
    if ![0, 3].contains(&e.kind)
        || e.image_flags & ((1 << 12) | (1 << 13) | (1 << 25) | (1 << 27) | (1 << 29) | (1 << 30))
            != 0
    {
        return Err("Unsupported fixed layer type/flags".into());
    }
    let bitmap = |id: i32| -> Result<Image, String> {
        let r = library
            .resource(
                2,
                u32::try_from(id).map_err(|_| "Invalid fixed-layer resource")?,
                language,
            )
            .ok_or("Missing fixed-layer resource")?;
        Image::dib(library.resource_bytes(r)?)
    };
    let root = gauge
        .roots
        .first()
        .and_then(|r| gauge.elements.iter().find(|e| e.rva == *r))
        .ok_or("Missing root")?;
    if root.kind != 0 || root.position != [0, 0] {
        return Err("Fixed layer requires a zero-origin static root".into());
    }
    let background = bitmap(root.resource_id)?;
    let source = bitmap(resource_id)?;
    let rect = panel_rectangle(preview, panel, placement)?;
    // Use the same gauge clipping as the needle transform for fixed image placement.
    rotate(
        &mut preview.image,
        &source,
        rect,
        [background.width, background.height],
        e.position,
        [0, 0],
        0.,
        e.image_flags & 1 != 0,
    )?;
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
    fn sprite_mask_foreground_rotation_and_clipping_are_independent() {
        let mut source = Image {
            width: 5,
            height: 5,
            rgba: vec![0; 100],
        };
        let marker = (2 * 5 + 4) * 4;
        source.rgba[marker..marker + 4].copy_from_slice(&[255, 0, 0, 255]);
        let mut mask = Image {
            width: 5,
            height: 5,
            rgba: [1, 1, 1, 255].repeat(25),
        };
        mask.rgba[..4].copy_from_slice(&[0, 255, 0, 255]);
        let mut target = Image {
            width: 7,
            height: 7,
            rgba: vec![255; 196],
        };
        sprite_blit(
            &mut target,
            &source,
            &mask,
            [1., 1., 5., 5.],
            [5, 5],
            [0, 0],
            [2, 2],
            [1., 1.],
            [0., 0., 90.],
            true,
        )
        .unwrap();
        // A positive sprite angle moves a marker from right to top (CCW).
        assert_eq!(
            &target.rgba[(1 * 7 + 3) * 4..(1 * 7 + 4) * 4],
            &[255, 0, 0, 255]
        );
        // Stationary mask foreground and untouched outside-gauge sentinel.
        assert_eq!(
            &target.rgba[(1 * 7 + 1) * 4..(1 * 7 + 2) * 4],
            &[0, 255, 0, 255]
        );
        assert_eq!(&target.rgba[..4], &[255; 4]);
        mask.rgba[2 * 4..3 * 4].copy_from_slice(&[0, 0, 0, 255]);
        let mut clipped = Image {
            width: 7,
            height: 7,
            rgba: vec![255; 196],
        };
        sprite_blit(
            &mut clipped,
            &source,
            &mask,
            [1., 1., 5., 5.],
            [5, 5],
            [0, 0],
            [2, 2],
            [1., 1.],
            [0., 0., 90.],
            false,
        )
        .unwrap();
        assert_eq!(&clipped.rgba[(1 * 7 + 3) * 4..(1 * 7 + 4) * 4], &[255; 4]);
        let mut shifted = Image {
            width: 7,
            height: 7,
            rgba: vec![255; 196],
        };
        sprite_blit(
            &mut shifted,
            &source,
            &mask,
            [1., 1., 5., 5.],
            [5, 5],
            [0, 0],
            [2, 2],
            [1., 1.],
            [1., 0., 0.],
            true,
        )
        .unwrap();
        assert_eq!(
            &shifted.rgba[(3 * 7 + 4) * 4..(3 * 7 + 5) * 4],
            &[255, 0, 0, 255]
        );
        for scale in [[0., 1.], [f64::NAN, 1.]] {
            assert!(sprite_blit(
                &mut target,
                &source,
                &mask,
                [1., 1., 5., 5.],
                [5, 5],
                [0, 0],
                [2, 2],
                scale,
                [0.; 3],
                true
            )
            .is_err());
        }
        source.rgba.pop();
        assert!(sprite_blit(
            &mut target,
            &source,
            &mask,
            [1., 1., 5., 5.],
            [5, 5],
            [0, 0],
            [2, 2],
            [1., 1.],
            [0.; 3],
            true
        )
        .is_err());
    }
    #[test]
    fn sprite_rotation_precedes_nonuniform_texture_scaling() {
        let mut source = Image {
            width: 5,
            height: 5,
            rgba: vec![0; 100],
        };
        source.rgba[(4 * 5 + 2) * 4..(4 * 5 + 3) * 4].copy_from_slice(&[255, 0, 0, 255]);
        let mask = Image {
            width: 9,
            height: 9,
            rgba: [1, 1, 1, 255].repeat(81),
        };
        let mut target = Image {
            width: 9,
            height: 9,
            rgba: vec![255; 324],
        };
        sprite_blit(
            &mut target,
            &source,
            &mask,
            [0., 0., 9., 9.],
            [9, 9],
            [0, 0],
            [4, 4],
            [2., 1.],
            [0., 0., 90.],
            true,
        )
        .unwrap();
        // Below-center source -> right, then horizontal scale doubles that displacement.
        assert_eq!(
            &target.rgba[(4 * 9 + 8) * 4..(4 * 9 + 9) * 4],
            &[255, 0, 0, 255]
        );
        assert_eq!(&target.rgba[(4 * 9 + 6) * 4..(4 * 9 + 7) * 4], &[255; 4]);
    }
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
