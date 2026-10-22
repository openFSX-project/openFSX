//! Experimental single-line ACE text rasterization with an explicit local font.
//! FontHeight is currently interpreted as pixels per em; FSX/GDI metrics and
//! default-font selection require reference validation.
use fontdue::{Font, FontSettings};
use fsx_script::gauge_expression::GaugeExpression;
use fsx_script::gauge_string::{Color, GaugeString, Span};
use fsx_texture::Image;
use fsx_vehicle::{
    cabinet::Cabinet,
    xml_text::{Align, TextGauge},
};
use std::{fs::File, io::Read, path::Path};

pub struct GaugeFont {
    pub(crate) font: Font,
}
pub struct RenderedText {
    pub image: Image,
    pub spans: Vec<Vec<Span>>,
    pub explicit_default_font: bool,
    pub visible: Vec<bool>,
}

fn visible_elements(
    gauge: &TextGauge,
    mut numeric: impl FnMut(char, &str, &str) -> Option<f64>,
) -> Result<Vec<bool>, String> {
    if gauge.visibility.len() > 8192
        || gauge
            .visibility
            .iter()
            .fold(0usize, |sum, v| sum.saturating_add(v.source.len()))
            > 1024 * 1024
    {
        return Err("XML visibility exceeds node/source budget".into());
    }
    let expressions = gauge
        .visibility
        .iter()
        .map(|v| GaugeExpression::compile(&v.source))
        .collect::<Result<Vec<_>, _>>()?;
    let mut values = Vec::new();
    for (index, (node, expression)) in gauge.visibility.iter().zip(&expressions).enumerate() {
        let parent = match node.parent {
            Some(parent) if parent < index => values[parent],
            Some(_) => return Err("XML visibility graph must be parent-before-child".into()),
            None => true,
        };
        values.push(
            parent
                && expression
                    .evaluate(&mut numeric)
                    .map_err(|e| format!("XML Visibility: {e:?}"))?
                    != 0.,
        );
    }
    gauge
        .texts
        .iter()
        .map(|t| match t.visibility {
            Some(index) => values
                .get(index)
                .copied()
                .ok_or_else(|| "XML text has an invalid visibility reference".into()),
            None => Ok(true),
        })
        .collect()
}
fn background_image(gauge: &TextGauge, cabinet: &Cabinet, member: &str) -> Result<Image, String> {
    if gauge.alpha && gauge.transparent {
        return Err(
            "Combined XML Alpha and black transparency require reference validation".into(),
        );
    }
    if gauge.size.iter().any(|&v| v == 0 || v > 4096) {
        return Err("Invalid XML gauge size".into());
    }
    let key = fsx_vehicle::cabinet::resource_key(member)?;
    let background = fsx_vehicle::cabinet::resource_key(&gauge.background)?;
    let background = match key.rsplit_once('/') {
        Some((d, _)) => format!("{d}/{background}"),
        None => background,
    };
    let bytes = cabinet.resource(&background)?;
    let background = if gauge.alpha {
        Image::bmp_alpha(bytes)?
    } else {
        Image::bmp(bytes)?
    };
    let mut image = Image {
        width: gauge.size[0],
        height: gauge.size[1],
        rgba: vec![0; gauge.size[0] as usize * gauge.size[1] as usize * 4],
    };
    for y in 0..image.height {
        for x in 0..image.width {
            let sx =
                ((x as f64 + 0.5) / image.width as f64 * background.width as f64).floor() as usize;
            let sy = ((y as f64 + 0.5) / image.height as f64 * background.height as f64).floor()
                as usize;
            let source = &background.rgba[(sy * background.width as usize + sx) * 4..][..4];
            if gauge.transparent && source[..3] == [0; 3] {
                continue;
            }
            image.rgba[(y as usize * image.width as usize + x as usize) * 4..][..4]
                .copy_from_slice(source);
        }
    }
    Ok(image)
}

impl GaugeFont {
    pub fn load(path: &Path) -> Result<Self, String> {
        let mut bytes = Vec::new();
        File::open(path)
            .and_then(|f| f.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("Gauge font exceeds 16 MiB".into());
        }
        Ok(Self {
            font: Font::from_bytes(bytes, FontSettings::default()).map_err(str::to_owned)?,
        })
    }
    pub fn name(&self) -> Option<&str> {
        self.font.name()
    }

    pub(crate) fn paint_spans(
        &self,
        image: &mut Image,
        text: &fsx_vehicle::xml_text::TextElement,
        spans: &[Span],
    ) -> Result<(), String> {
        self.paint_spans_profile(image, text, spans, false)
    }
    pub(crate) fn paint_ansi_spans(
        &self,
        image: &mut Image,
        text: &fsx_vehicle::xml_text::TextElement,
        spans: &[Span],
    ) -> Result<(), String> {
        self.paint_spans_profile(image, text, spans, true)
    }
    fn paint_spans_profile(
        &self,
        image: &mut Image,
        text: &fsx_vehicle::xml_text::TextElement,
        spans: &[Span],
        ansi: bool,
    ) -> Result<(), String> {
        let mut glyphs = Vec::new();
        let mut width = 0f32;
        let mut previous = None;
        for span in spans {
            let color = match span.color {
                Color::Default => text.color,
                Color::Alternate => text
                    .alternate
                    .ok_or("Text selects an absent alternate color")?,
            };
            for ch in span.text.chars() {
                if !(if ansi {
                    ansi_printable(ch)
                } else {
                    ch.is_ascii() && !ch.is_control()
                }) {
                    return Err(
                        "Rasterizer character is outside the selected ASCII/CP1252 profile".into(),
                    );
                }
                if !self.font.has_glyph(ch) {
                    return Err(format!("Selected font lacks glyph {ch:?}"));
                }
                if glyphs.len() >= 256 {
                    return Err("XML text exceeds 256 glyphs".into());
                }
                if let Some(previous) = previous {
                    width += self
                        .font
                        .horizontal_kern(previous, ch, text.font_height)
                        .unwrap_or(0.);
                }
                let metrics = self.font.metrics(ch, text.font_height);
                if metrics.width > 1024
                    || metrics.height > 1024
                    || !metrics.advance_width.is_finite()
                    || metrics.advance_width.abs() > 4096.
                {
                    return Err("Font glyph metrics exceed rasterization bounds".into());
                }
                glyphs.push((ch, width, color));
                width += metrics.advance_width;
                previous = Some(ch);
            }
        }
        let line = self
            .font
            .horizontal_line_metrics(text.font_height)
            .ok_or("Font lacks horizontal line metrics")?;
        let x = text.position[0] + aligned(text.horizontal, text.size[0] as f32 - width);
        let baseline = text.position[1]
            + aligned(
                text.vertical,
                text.size[1] as f32 - (line.ascent - line.descent),
            )
            + line.ascent;
        let clip = [
            text.position[0],
            text.position[1],
            text.position[0] + text.size[0] as f32,
            text.position[1] + text.size[1] as f32,
        ];
        for (ch, advance, color) in glyphs {
            let (metrics, coverage) = self.font.rasterize(ch, text.font_height);
            let left = (x + advance + metrics.xmin as f32).floor() as i32;
            let top = (baseline - metrics.ymin as f32 - metrics.height as f32).floor() as i32;
            for gy in 0..metrics.height {
                for gx in 0..metrics.width {
                    let px = left + gx as i32;
                    let py = top + gy as i32;
                    if px < 0
                        || py < 0
                        || px >= image.width as i32
                        || py >= image.height as i32
                        || (px as f32 + 0.5) < clip[0]
                        || (py as f32 + 0.5) < clip[1]
                        || (px as f32 + 0.5) >= clip[2]
                        || (py as f32 + 0.5) >= clip[3]
                    {
                        continue;
                    }
                    let at = (py as usize * image.width as usize + px as usize) * 4;
                    source_over(
                        &mut image.rgba[at..at + 4],
                        color,
                        coverage[gy * metrics.width + gx],
                    );
                }
            }
        }
        Ok(())
    }

    pub fn render(
        &self,
        gauge: &TextGauge,
        cabinet: &Cabinet,
        member: &str,
        mut numeric: impl FnMut(char, &str, &str) -> Option<f64>,
        mut string: impl FnMut(&str) -> Option<String>,
    ) -> Result<RenderedText, String> {
        let mut image = background_image(gauge, cabinet, member)?;
        let programs = gauge
            .texts
            .iter()
            .map(|text| GaugeString::compile(&text.source))
            .collect::<Result<Vec<_>, _>>()?;
        let visible = visible_elements(gauge, &mut numeric)?;
        let mut all_spans = Vec::new();
        let mut explicit_default_font = false;
        for ((text, program), &is_visible) in gauge.texts.iter().zip(&programs).zip(&visible) {
            if !is_visible {
                all_spans.push(Vec::new());
                continue;
            }
            if let Some(face) = &text.font_face {
                if !self.name().is_some_and(|n| n.eq_ignore_ascii_case(face)) {
                    return Err(format!(
                        "Selected font {:?} does not match authored face {face}",
                        self.name()
                    ));
                }
            } else {
                explicit_default_font = true;
            }
            if !text.font_height.is_finite()
                || !(1. ..=256.).contains(&text.font_height)
                || text
                    .position
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 4096.)
                || text.size.iter().any(|&v| v == 0 || v > 4096)
            {
                return Err("Invalid XML text layout bounds".into());
            }
            let spans = program
                .evaluate(&mut numeric, &mut string)
                .map_err(|e| format!("{e:?}"))?;
            self.paint_spans(&mut image, text, &spans)?;
            all_spans.push(spans);
        }
        Ok(RenderedText {
            image,
            spans: all_spans,
            explicit_default_font,
            visible,
        })
    }
}
fn aligned(align: Align, space: f32) -> f32 {
    match align {
        Align::Start => 0.,
        Align::Center => space / 2.,
        Align::End => space,
    }
}
pub(crate) fn ansi_printable(ch: char) -> bool {
    (ch.is_ascii() && !ch.is_control())
        || matches!(
            ch,
            '\u{a0}'
                ..='\u{ff}'
                    | '€'
                    | '‚'
                    | 'ƒ'
                    | '„'
                    | '…'
                    | '†'
                    | '‡'
                    | 'ˆ'
                    | '‰'
                    | 'Š'
                    | '‹'
                    | 'Œ'
                    | 'Ž'
                    | '‘'
                    | '’'
                    | '“'
                    | '”'
                    | '•'
                    | '–'
                    | '—'
                    | '˜'
                    | '™'
                    | 'š'
                    | '›'
                    | 'œ'
                    | 'ž'
                    | 'Ÿ'
        )
}
pub(crate) fn source_over(target: &mut [u8], color: [u8; 3], coverage: u8) {
    let a = u32::from(coverage);
    let old_a = u32::from(target[3]);
    let alpha = a * 255 + old_a * (255 - a);
    if alpha == 0 {
        return;
    }
    for i in 0..3 {
        target[i] =
            ((u32::from(color[i]) * a * 255 + u32::from(target[i]) * old_a * (255 - a) + alpha / 2)
                / alpha) as u8;
    }
    target[3] = ((alpha + 127) / 255) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visibility_caches_parents_short_circuits_descendants_and_rejects_bad_graphs() {
        let text = |visibility| fsx_vehicle::xml_text::TextElement {
            position: [0.; 2],
            size: [1; 2],
            font_face: None,
            font_height: 1.,
            horizontal: Align::Start,
            vertical: Align::Start,
            color: [255; 3],
            alternate: None,
            source: "unused".into(),
            visibility,
        };
        let mut gauge = TextGauge {
            size: [1; 2],
            background: "unused.bmp".into(),
            transparent: false,
            alpha: false,
            texts: vec![text(Some(1)), text(Some(0)), text(Some(0)), text(None)],
            visibility: vec![
                fsx_vehicle::xml_text::Visibility {
                    parent: None,
                    source: "(A:power, bool)".into(),
                },
                fsx_vehicle::xml_text::Visibility {
                    parent: Some(0),
                    source: "(P:mode, enum) 1 ==".into(),
                },
            ],
        };
        let mut reads = 0;
        let hidden = visible_elements(&gauge, |ns, name, units| {
            reads += 1;
            assert_eq!((ns, name, units), ('A', "power", "bool"));
            Some(0.)
        })
        .unwrap();
        assert_eq!(hidden, [false, false, false, true]);
        assert_eq!(reads, 1);
        let mut power_reads = 0;
        assert_eq!(
            visible_elements(&gauge, |ns, name, units| match (ns, name, units) {
                ('A', "power", "bool") => {
                    power_reads += 1;
                    Some(1.)
                }
                ('P', "mode", "enum") => Some(0.),
                _ => None,
            })
            .unwrap(),
            [false, true, true, true]
        );
        assert_eq!(power_reads, 1);
        assert!(visible_elements(&gauge, |_, _, _| None).is_err());
        assert!(visible_elements(&gauge, |_, _, _| Some(f64::NAN)).is_err());
        gauge.visibility[0].parent = Some(1);
        assert!(visible_elements(&gauge, |_, _, _| Some(1.)).is_err());
        gauge.visibility[0].parent = None;
        gauge.texts[0].visibility = Some(999);
        assert!(visible_elements(&gauge, |_, _, _| Some(1.)).is_err());
        gauge.texts[0].visibility = Some(1);
        gauge.visibility[1].source = "(>A:write, number)".into();
        assert!(visible_elements(&gauge, |_, _, _| Some(0.)).is_err());
    }
    #[test]
    fn xml_alpha_background_preserves_channels_through_cabinet_and_panel() {
        fn put(b: &mut [u8], at: usize, n: u32) {
            b[at..at + 4].copy_from_slice(&n.to_le_bytes());
        }
        let mut bmp = vec![0u8; 54];
        bmp[..2].copy_from_slice(b"BM");
        put(&mut bmp, 2, 70);
        put(&mut bmp, 10, 54);
        put(&mut bmp, 14, 40);
        put(&mut bmp, 18, 4);
        put(&mut bmp, 22, 1);
        bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
        bmp[28..30].copy_from_slice(&32u16.to_le_bytes());
        for alpha in [0, 64, 128, 255] {
            bmp.extend_from_slice(&[50, 100, 200, alpha]);
        }
        let mut cab = vec![0u8; 44];
        cab[..4].copy_from_slice(b"MSCF");
        put(&mut cab, 16, 44);
        cab[24..26].copy_from_slice(&[3, 1]);
        cab[26..28].copy_from_slice(&1u16.to_le_bytes());
        cab[28..30].copy_from_slice(&1u16.to_le_bytes());
        cab[40..42].copy_from_slice(&1u16.to_le_bytes());
        cab.extend_from_slice(&(bmp.len() as u32).to_le_bytes());
        cab.extend_from_slice(&[0; 12]);
        cab.extend_from_slice(b"own.bmp\0");
        let data_at = cab.len() as u32;
        put(&mut cab, 36, data_at);
        cab.extend_from_slice(&[0; 4]);
        cab.extend_from_slice(&(bmp.len() as u16).to_le_bytes());
        cab.extend_from_slice(&(bmp.len() as u16).to_le_bytes());
        cab.extend_from_slice(&bmp);
        let length = cab.len() as u32;
        put(&mut cab, 8, length);
        let cabinet = Cabinet::parse(&cab, false).unwrap();
        let xml = r#"<SimBase.Document Type="AceXML"><SimGauge.Gauge><Size>4,1</Size><Image Name="own.bmp"><Alpha>True</Alpha></Image><Element><GaugeText><Size>1,1</Size><FontHeight>1</FontHeight><FontColor>White</FontColor><GaugeString/><Transparent>True</Transparent></GaugeText></Element></SimGauge.Gauge></SimBase.Document>"#;
        let doc = fsx_vehicle::xml_gauge::GaugeDocument::parse(xml.as_bytes(), false).unwrap();
        let mut gauge = TextGauge::from_document(&doc).unwrap();
        let image = background_image(&gauge, &cabinet, "own.xml").unwrap();
        assert_eq!(
            image.rgba,
            [200, 100, 50, 0, 200, 100, 50, 64, 200, 100, 50, 128, 200, 100, 50, 255]
        );
        let cfg = fsx_vehicle::panel::PanelConfig::parse(
            "[VCockpit01]\ntexture=$Own\nsize_mm=4,1\npixel_size=4,1\ngauge00=Own!Text,0,0,4,1\n",
        )
        .unwrap();
        let p = &cfg.panels[0];
        let mut preview = crate::panel::static_roots(p, &Default::default(), 1033).unwrap();
        crate::panel::paint_xml_image(&mut preview, p, &p.gauges[0], &image).unwrap();
        assert_eq!(
            preview.image.rgba,
            [0, 0, 0, 255, 50, 25, 13, 255, 100, 50, 25, 255, 200, 100, 50, 255]
        );
        gauge.alpha = false;
        assert!(background_image(&gauge, &cabinet, "own.xml").is_err());
        gauge.alpha = true;
        gauge.transparent = true;
        assert!(background_image(&gauge, &cabinet, "own.xml").is_err());
    }
    #[test]
    fn coverage_preserves_transparency_and_blends_straight_alpha() {
        let mut transparent = [0; 4];
        source_over(&mut transparent, [240, 120, 60], 128);
        assert_eq!(transparent, [240, 120, 60, 128]);
        source_over(&mut transparent, [0; 3], 0);
        assert_eq!(transparent, [240, 120, 60, 128]);
        let mut opaque = [0, 0, 0, 255];
        source_over(&mut opaque, [200, 100, 50], 128);
        assert_eq!(opaque, [100, 50, 25, 255]);
        assert_eq!(aligned(Align::Center, 10.), 5.);
    }
}
