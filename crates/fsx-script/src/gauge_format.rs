//! Bounded GaugeString field formatting, independent of OS printf/locales.
use crate::EvalError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Integer,
    Fixed,
    String,
}
#[derive(Debug)]
pub(crate) struct Format {
    pub kind: Kind,
    width: usize,
    precision: Option<usize>,
    left: bool,
    plus: bool,
    zero: bool,
}
impl Format {
    pub fn parse(source: &str) -> Result<Self, String> {
        if !source.is_ascii() {
            return Err("GaugeString format must be ASCII".into());
        }
        let (body, last) = source.split_at(
            source
                .len()
                .checked_sub(1)
                .ok_or("Empty GaugeString format")?,
        );
        let kind = match last {
            "d" => Kind::Integer,
            "f" => Kind::Fixed,
            "s" => Kind::String,
            _ => return Err("Unsupported GaugeString format type".into()),
        };
        let mut body = body;
        let mut left = false;
        let mut plus = false;
        let mut seen_space = false;
        while let Some(flag) = body.as_bytes().first() {
            match flag {
                b'-' if !left => left = true,
                b'+' if !plus => plus = true,
                b' ' if !seen_space => seen_space = true,
                b'-' | b'+' | b' ' => return Err("Duplicate GaugeString format flag".into()),
                _ => break,
            }
            body = &body[1..];
        }
        let (digits, precision) = match body.split_once('.') {
            Some((digits, precision)) => {
                if kind != Kind::Fixed
                    || precision.is_empty()
                    || !precision.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err("Invalid GaugeString precision".into());
                }
                let precision = precision
                    .parse::<usize>()
                    .map_err(|_| "GaugeString precision overflow")?;
                if precision > 16 {
                    return Err("GaugeString precision exceeds 16".into());
                }
                (digits, Some(precision))
            }
            None => (body, None),
        };
        if !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err("Invalid GaugeString width".into());
        }
        let width = if digits.is_empty() {
            0
        } else {
            digits
                .parse::<usize>()
                .map_err(|_| "GaugeString width overflow")?
        };
        if width > 256 {
            return Err("GaugeString field width exceeds 256".into());
        }
        let zero = digits.starts_with('0');
        if kind == Kind::String && (plus || zero) {
            return Err("String formats do not support sign/zero flags".into());
        }
        if kind == Kind::Fixed && (plus || left || seen_space) {
            return Err("Floating sign/alignment flags require reference validation".into());
        }
        Ok(Self {
            kind,
            width,
            precision,
            left,
            plus,
            zero,
        })
    }

    pub fn text(&self, text: String) -> Result<String, EvalError> {
        if text.len() > 8192 || text.contains('\0') {
            return Err(EvalError::Invalid("GaugeString value limit or NUL".into()));
        }
        let pad = self.width.saturating_sub(text.chars().count());
        Ok(if self.left {
            format!("{text}{}", " ".repeat(pad))
        } else {
            format!("{}{text}", " ".repeat(pad))
        })
    }

    pub fn number(&self, number: f64) -> Result<String, EvalError> {
        if !number.is_finite() {
            return Err(EvalError::Invalid("Non-finite GaugeString number".into()));
        }
        match self.kind {
            Kind::Fixed => {
                // SDK examples ignore the minimum width and zero flag for f.
                // Bare f currently uses six fractional digits, explicitly documented.
                Ok(format!("{:.*}", self.precision.unwrap_or(6), number))
            }
            Kind::Integer | Kind::String => {
                if number.abs() > 9_007_199_254_740_991. {
                    return Err(EvalError::Invalid(
                        "GaugeString integer exceeds exact f64 range".into(),
                    ));
                }
                if self.kind == Kind::String && number.fract() != 0. {
                    return Err(EvalError::Invalid(
                        "Fractional numeric-to-string formatting is unverified".into(),
                    ));
                }
                let rounded = number.round();
                let sign = if rounded < 0. {
                    "-"
                } else if self.plus && rounded > 0. {
                    "+"
                } else {
                    ""
                };
                let digits = format!("{:.0}", rounded.abs());
                let pad = self.width.saturating_sub(sign.len() + digits.len());
                let text = if self.left {
                    format!("{sign}{digits}{}", " ".repeat(pad))
                } else if self.zero {
                    format!("{sign}{}{digits}", "0".repeat(pad))
                } else {
                    format!("{}{sign}{digits}", " ".repeat(pad))
                };
                Ok(text)
            }
        }
    }
}
