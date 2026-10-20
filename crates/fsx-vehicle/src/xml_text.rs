//! Typed static-background / translated GaugeText subset of ACE documents.
//! Unsupported visual behavior is rejected before a renderer can claim support.
use crate::xml_gauge::{Element, GaugeDocument};

#[derive(Debug, Clone, Copy)]
pub enum Align {
    Start,
    Center,
    End,
}
#[derive(Debug)]
pub struct TextElement {
    pub position: [f32; 2],
    pub size: [u32; 2],
    pub font_face: Option<String>,
    pub font_height: f32,
    pub horizontal: Align,
    pub vertical: Align,
    pub color: [u8; 3],
    pub alternate: Option<[u8; 3]>,
    pub source: String,
}
#[derive(Debug)]
pub struct TextGauge {
    pub size: [u32; 2],
    pub background: String,
    pub transparent: bool,
    pub alpha: bool,
    pub texts: Vec<TextElement>,
}
type Result<T> = std::result::Result<T, String>;

fn allowed(node: &Element, fields: &[&str], attributes: &[&str]) -> Result<()> {
    if !node.text.trim().is_empty()
        || node
            .attributes
            .keys()
            .any(|k| !attributes.contains(&k.as_str()))
        || node
            .children
            .iter()
            .any(|n| !fields.contains(&n.name.as_str()))
    {
        return Err(format!(
            "Unsupported XML text-gauge content in {}",
            node.name
        ));
    }
    Ok(())
}
fn value<'a>(node: &'a Element, key: &str) -> Result<Option<&'a str>> {
    node.child(key)?
        .map(|n| {
            if !n.attributes.is_empty() || !n.children.is_empty() {
                return Err(format!("Expected plain XML property {key}"));
            }
            Ok(n.text.trim())
        })
        .transpose()
}
fn pair(text: &str) -> Result<[f32; 2]> {
    let (x, y) = text.split_once(',').ok_or("Expected XML coordinate pair")?;
    let result = [
        x.trim()
            .parse::<f32>()
            .map_err(|_| "Invalid XML coordinate")?,
        y.trim()
            .parse::<f32>()
            .map_err(|_| "Invalid XML coordinate")?,
    ];
    if result.iter().any(|v| !v.is_finite() || v.abs() > 4096.) {
        return Err("XML coordinate outside supported bounds".into());
    }
    Ok(result)
}
fn size(node: &Element) -> Result<[u32; 2]> {
    let result = pair(value(node, "Size")?.ok_or("XML text gauge needs Size")?)?;
    if result.iter().any(|v| *v <= 0. || v.fract() != 0.) {
        return Err("XML text dimensions must be positive integers".into());
    }
    Ok(result.map(|v| v as u32))
}
fn boolean(node: &Element, key: &str) -> Result<bool> {
    match value(node, key)? {
        Some(v) if v.eq_ignore_ascii_case("true") => Ok(true),
        Some(v) if v.eq_ignore_ascii_case("false") => Ok(false),
        None => Ok(false),
        _ => Err(format!("Invalid XML boolean property {key}")),
    }
}
fn align(node: &Element, field: &str) -> Result<Align> {
    match value(node, field)?.unwrap_or(if field == "HorizontalAlign" {
        "LEFT"
    } else {
        "TOP"
    }) {
        "LEFT" if field == "HorizontalAlign" => Ok(Align::Start),
        "TOP" if field == "VerticalAlign" => Ok(Align::Start),
        "CENTER" => Ok(Align::Center),
        "RIGHT" if field == "HorizontalAlign" => Ok(Align::End),
        "BOTTOM" if field == "VerticalAlign" => Ok(Align::End),
        _ => Err("Unsupported XML text alignment".into()),
    }
}
/// XML colors are COLORREF 0xBBGGRR, not CSS RGB order.
fn color(text: &str) -> Result<[u8; 3]> {
    match text.to_ascii_lowercase().as_str() {
        "white" => Ok([255; 3]),
        "black" => Ok([0; 3]),
        "red" => Ok([255, 0, 0]),
        "green" => Ok([0, 128, 0]),
        "blue" => Ok([0, 0, 255]),
        _ => {
            let hex = text
                .strip_prefix("0x")
                .or_else(|| text.strip_prefix("0X"))
                .ok_or("Unsupported named XML color")?;
            if hex.is_empty() || hex.len() > 6 {
                return Err("Invalid XML COLORREF".into());
            }
            let value = u32::from_str_radix(hex, 16).map_err(|_| "Invalid XML COLORREF")?;
            Ok([value as u8, (value >> 8) as u8, (value >> 16) as u8])
        }
    }
}
impl TextGauge {
    pub fn from_document(document: &GaugeDocument) -> Result<Self> {
        let gauge = document.gauge();
        allowed(
            gauge,
            &["FloatPosition", "Size", "Image", "Element"],
            &["id", "ArtDirectory"],
        )?;
        if pair(value(gauge, "FloatPosition")?.unwrap_or("0,0"))? != [0.; 2] {
            return Err("Translated XML gauge root is unsupported".into());
        }
        let image = gauge
            .child("Image")?
            .ok_or("XML text gauge needs a background Image")?;
        allowed(image, &["Transparent", "Alpha"], &["id", "Name"])?;
        let alpha = boolean(image, "Alpha")?;
        let transparent = boolean(image, "Transparent")?;
        if alpha && transparent {
            return Err(
                "Combined XML Alpha and black transparency require reference validation".into(),
            );
        }
        let background = image
            .attributes
            .get("Name")
            .ok_or("XML background lacks Name")?
            .to_owned();
        crate::cabinet::resource_key(&background)?;
        let mut result = Self {
            size: size(gauge)?,
            background,
            transparent,
            alpha,
            texts: Vec::new(),
        };
        fn element(node: &Element, origin: [f32; 2], output: &mut Vec<TextElement>) -> Result<()> {
            allowed(node, &["FloatPosition", "GaugeText", "Element"], &["id"])?;
            let offset = pair(value(node, "FloatPosition")?.unwrap_or("0,0"))?;
            let position = [origin[0] + offset[0], origin[1] + offset[1]];
            if position.iter().any(|v| v.abs() > 4096.) {
                return Err("XML accumulated translation exceeds bounds".into());
            }
            if let Some(text) = node.child("GaugeText")? {
                allowed(
                    text,
                    &[
                        "FontFace",
                        "FontColor",
                        "FontHeight",
                        "GaugeString",
                        "HorizontalAlign",
                        "VerticalAlign",
                        "Size",
                        "Transparent",
                        "AlternateColor",
                    ],
                    &["id"],
                )?;
                if !boolean(text, "Transparent")? {
                    return Err("Opaque GaugeText background is unsupported".into());
                }
                let font_height = value(text, "FontHeight")?
                    .ok_or("GaugeText needs FontHeight")?
                    .parse::<f32>()
                    .map_err(|_| "Invalid FontHeight")?;
                if !font_height.is_finite() || !(1. ..=256.).contains(&font_height) {
                    return Err("FontHeight outside 1..256".into());
                }
                let alternate = text
                    .child("AlternateColor")?
                    .map(|alt| {
                        allowed(alt, &["FontColor"], &["id"])?;
                        color(value(alt, "FontColor")?.ok_or("AlternateColor lacks FontColor")?)
                    })
                    .transpose()?;
                let font_face = value(text, "FontFace")?.map(str::to_owned);
                if font_face.as_ref().is_some_and(|v| {
                    v.is_empty() || v.len() > 128 || v.chars().any(char::is_control)
                }) {
                    return Err("Invalid XML font face".into());
                }
                // Preserve authored string whitespace; visual-property values trim.
                let source = text
                    .child("GaugeString")?
                    .ok_or("GaugeText lacks GaugeString")?;
                if !source.attributes.is_empty() || !source.children.is_empty() {
                    return Err("GaugeString must be plain text".into());
                }
                output.push(TextElement {
                    position,
                    size: size(text)?,
                    font_face,
                    font_height,
                    horizontal: align(text, "HorizontalAlign")?,
                    vertical: align(text, "VerticalAlign")?,
                    color: color(value(text, "FontColor")?.ok_or("GaugeText needs FontColor")?)?,
                    alternate,
                    source: source.text.clone(),
                });
            }
            for child in node.children.iter().filter(|c| c.name == "Element") {
                element(child, position, output)?;
            }
            Ok(())
        }
        for child in gauge.children.iter().filter(|n| n.name == "Element") {
            element(child, [0.; 2], &mut result.texts)?;
        }
        if result.texts.is_empty() {
            return Err("XML text gauge has no GaugeText".into());
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_image_alpha_is_separate_from_black_key_and_luminosity() {
        for (properties, valid, alpha) in [
            ("", true, false),
            ("<Alpha>True</Alpha>", true, true),
            (
                "<Alpha>False</Alpha><Transparent>True</Transparent>",
                true,
                false,
            ),
            (
                "<Alpha>True</Alpha><Transparent>True</Transparent>",
                false,
                false,
            ),
            ("<Alpha>maybe</Alpha>", false, false),
            ("<Luminous>True</Luminous>", false, false),
        ] {
            let xml = format!(
                r#"<SimBase.Document Type="AceXML"><SimGauge.Gauge><Size>10,10</Size><Image Name="own.bmp">{properties}</Image><Element><GaugeText><Size>8,8</Size><FontHeight>6</FontHeight><FontColor>White</FontColor><GaugeString>X</GaugeString><Transparent>True</Transparent></GaugeText></Element></SimGauge.Gauge></SimBase.Document>"#
            );
            let doc = GaugeDocument::parse(xml.as_bytes(), false).unwrap();
            let typed = TextGauge::from_document(&doc);
            assert_eq!(typed.is_ok(), valid, "{properties}");
            if valid {
                assert_eq!(typed.unwrap().alpha, alpha);
            }
        }
    }
    fn document(properties: &str) -> GaugeDocument {
        GaugeDocument::parse(format!(r#"<SimBase.Document Type="AceXML"><SimGauge.Gauge><Size>100,30</Size><Image Name="face.bmp"/><Element><FloatPosition>2,3</FloatPosition><Element><FloatPosition>4,5</FloatPosition><GaugeText><Size>90,20</Size><FontHeight>16</FontHeight><FontColor>0x123456</FontColor><GaugeString> text </GaugeString><Transparent>True</Transparent>{properties}</GaugeText></Element></Element></SimGauge.Gauge></SimBase.Document>"#).as_bytes(), false).unwrap()
    }
    #[test]
    fn typed_text_preserves_units_translation_color_and_optional_font() {
        let gauge =
            TextGauge::from_document(&document("<HorizontalAlign>CENTER</HorizontalAlign>"))
                .unwrap();
        assert_eq!(gauge.size, [100, 30]);
        assert_eq!(gauge.texts[0].position, [6., 8.]);
        assert_eq!(gauge.texts[0].color, [0x56, 0x34, 0x12]);
        assert_eq!(gauge.texts[0].source, " text ");
        assert!(gauge.texts[0].font_face.is_none());
    }
    #[test]
    fn unknown_behavior_and_ambiguous_properties_fail() {
        for property in [
            "<Rotation>1</Rotation>",
            "<FontHeight>18</FontHeight>",
            "<FontColorScript>1</FontColorScript>",
            "<Bold>True</Bold>",
            "<FontFace/>",
        ] {
            assert!(
                TextGauge::from_document(&document(property)).is_err(),
                "{property}"
            );
        }
        assert_eq!(color("0x709090").unwrap(), [0x90, 0x90, 0x70]);
    }
}
