//! Authored native string layout using an explicit matching installed font.
//! Automatic width follows the documented NUMCHARS-capital-W rule. GDI font
//! metrics, hinting and FSX pixel equivalence still require reference validation.
use crate::xml_text::GaugeFont;
use fsx_script::gauge_string::{Color, Span};
use fsx_texture::Image;
use fsx_vehicle::{
    gauge::NativeText,
    xml_text::{Align, TextElement},
};

pub struct Rasterized {
    pub image: Image,
    pub em_pixels: f32,
    pub capital_w_width: f32,
}

fn automatic_em(width: u32, count: u16, advance: f32, kern: f32) -> Result<f32, String> {
    if width == 0
        || width > 4096
        || count == 0
        || count > 256
        || !advance.is_finite()
        || advance <= 0.
        || !kern.is_finite()
    {
        return Err("Invalid native automatic-font width/count/metrics".into());
    }
    let denominator = advance * f32::from(count) + kern * f32::from(count - 1);
    let size = width as f32 / denominator;
    if !denominator.is_finite()
        || denominator <= 0.
        || !size.is_finite()
        || !(1. ..=256.).contains(&size)
    {
        return Err("Native automatic-font size is outside 1..256 pixels per em".into());
    }
    Ok(size)
}

/// Render explicitly unselected text with authored foreground/background colors.
/// Runtime color overrides are not evaluated here.
pub fn rasterize_unselected(
    font: &GaugeFont,
    style: &NativeText,
    image_flags: u32,
    value: &str,
) -> Result<Rasterized, String> {
    rasterize_selected(font, style, image_flags, value, 0)
}

/// Resolve one global SEQ_REC state and paint selected glyphs with HILITECOLOR.
/// An unrelated state leaves the string unselected. This does not dispatch events.
pub fn rasterize_selected(
    font: &GaugeFont,
    style: &NativeText,
    image_flags: u32,
    value: &str,
    selection_state: i32,
) -> Result<Rasterized, String> {
    let selected = style.selected_range(selection_state, value.len())?;
    if style.font_height != 0 {
        return Err("Native fixed-height/GDI text sizing is not yet validated".into());
    }
    if style.size.iter().any(|&v| v == 0 || v > 4096)
        || style.max_length == 0
        || style.max_length > 256
        || value.len() > style.max_length as usize
        || !value.is_ascii()
        || value.chars().any(char::is_control)
    {
        return Err("Native string rasterization bounds or ASCII contract failed".into());
    }
    if !font
        .name()
        .is_some_and(|name| name.eq_ignore_ascii_case(&style.font_name))
        || style.font_weight != 400
        || ![0, 1].contains(&style.font_charset)
    {
        return Err(
            "Native text requires the matching regular font and ANSI/default charset".into(),
        );
    }
    if image_flags & !7 != 0
        || style.draw_text_flags & !0x2f != 0
        || style.draw_text_flags & 0x20 == 0
    {
        return Err("Native text drawing flags or effect is unsupported".into());
    }
    let horizontal = match style.draw_text_flags & 3 {
        0 => Align::Start,
        1 => Align::Center,
        2 => Align::End,
        _ => return Err("Invalid native horizontal alignment".into()),
    };
    let vertical = match style.draw_text_flags & 0xc {
        0 => Align::Start,
        4 => Align::Center,
        8 => Align::End,
        _ => return Err("Invalid native vertical alignment".into()),
    };
    if !font.font.has_glyph('W') {
        return Err("Native auto-fit font lacks capital W".into());
    }
    let advance = font.font.metrics('W', 1.).advance_width;
    let kern = font.font.horizontal_kern('W', 'W', 1.).unwrap_or(0.);
    let em = automatic_em(style.size[0], style.max_length, advance, kern)?;
    let color = |raw: u32| -> Result<[u8; 3], String> {
        if raw & 0xff00_0000 != 0 {
            return Err("Palette/system native COLORREF is unsupported".into());
        }
        Ok([raw as u8, (raw >> 8) as u8, (raw >> 16) as u8])
    };
    let foreground = color(style.colors[0])?;
    let background = color(style.colors[1])?;
    let clear = if image_flags & 1 != 0 {
        [0; 4]
    } else {
        [background[0], background[1], background[2], 255]
    };
    let mut image = Image {
        width: style.size[0],
        height: style.size[1],
        rgba: clear.repeat(style.size[0] as usize * style.size[1] as usize),
    };
    // Reuse geometric font layout only; callback output is data, never XML/RPN.
    let layout = TextElement {
        position: [0.; 2],
        size: style.size,
        font_face: Some(style.font_name.clone()),
        font_height: em,
        horizontal,
        vertical,
        color: foreground,
        alternate: selected
            .as_ref()
            .map(|_| color(style.colors[2]))
            .transpose()?,
        source: String::new(),
        visibility: None,
    };
    let spans = if let Some(range) = selected {
        vec![
            Span {
                text: value[..range.start].to_owned(),
                color: Color::Default,
            },
            Span {
                text: value[range.clone()].to_owned(),
                color: Color::Alternate,
            },
            Span {
                text: value[range.end..].to_owned(),
                color: Color::Default,
            },
        ]
    } else {
        vec![Span {
            text: value.to_owned(),
            color: Color::Default,
        }]
    };
    font.paint_spans(&mut image, &layout, &spans)?;
    Ok(Rasterized {
        image,
        em_pixels: em,
        capital_w_width: advance * em * f32::from(style.max_length)
            + kern * em * f32::from(style.max_length - 1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auto_font_uses_declared_w_count_and_rejects_unbounded_inputs() {
        assert_eq!(automatic_em(48, 8, 0.5, 0.).unwrap(), 12.);
        assert_eq!(automatic_em(44, 8, 0.5, -0.1).unwrap(), 44. / 3.3);
        for (width, count, advance, kern) in [
            (0, 8, 0.5, 0.),
            (4097, 8, 0.5, 0.),
            (48, 0, 0.5, 0.),
            (48, 257, 0.5, 0.),
            (48, 8, 0., 0.),
            (48, 8, f32::NAN, 0.),
            (48, 8, 0.5, f32::INFINITY),
            (48, 8, 0.5, -2.),
            (4096, 1, 0.01, 0.),
        ] {
            assert!(automatic_em(width, count, advance, kern).is_err());
        }
    }
}
