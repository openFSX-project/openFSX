//! Bounded, read-only GaugeString subset: literal text, A: strings, numeric
//! A:/P: conditions, if/else/end, percent escaping and default/alternate colors.
use crate::gauge_expression::GaugeExpression;
use crate::gauge_format::{Format, Kind};
use crate::{EvalError, MAX_DEPTH, MAX_SOURCE};

const MAX_OUTPUT: usize = 8192;
const MAX_PARTS: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Default,
    Alternate,
}
#[derive(Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub color: Color,
}
#[derive(Debug)]
pub struct GaugeString {
    parts: Vec<Part>,
}
#[derive(Debug)]
enum Part {
    Literal(String),
    StringVariable(String, Format),
    Numeric(GaugeExpression, Format),
    Color(Color),
    If(GaugeExpression, Vec<Part>, Vec<Part>),
}
struct Parser<'a> {
    source: &'a str,
    offset: usize,
    count: usize,
}

impl Parser<'_> {
    fn expression(&mut self) -> Result<&str, String> {
        let start = self.offset + 2;
        let mut depth = 1;
        for (i, c) in self.source[start..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => (),
            }
            if depth > MAX_DEPTH {
                return Err("GaugeString expression nesting limit".into());
            }
            if depth == 0 {
                self.offset = start + i + 1;
                return Ok(&self.source[start..start + i]);
            }
        }
        Err("Unclosed GaugeString expression".into())
    }

    fn block(&mut self, depth: usize) -> Result<(Vec<Part>, Option<&'static str>), String> {
        if depth > MAX_DEPTH {
            return Err("GaugeString branch depth limit".into());
        }
        let mut parts = Vec::new();
        while self.offset < self.source.len() {
            let rest = &self.source[self.offset..];
            for (marker, terminator) in [("%{else}", "else"), ("%{end}", "end")] {
                if rest.starts_with(marker) {
                    self.offset += marker.len();
                    return Ok((parts, Some(terminator)));
                }
            }
            self.count += 1;
            if self.count > MAX_PARTS {
                return Err("GaugeString part limit".into());
            }
            let part = if rest.starts_with("%(") {
                let expression = self.expression()?.trim().to_owned();
                let tail = &self.source[self.offset..];
                if let Some(tail) = tail.strip_prefix("%!") {
                    let end = tail.find('!').ok_or("Unclosed GaugeString field format")?;
                    let format = Format::parse(&tail[..end])?;
                    self.offset += end + 3;
                    let string_variable = expression
                        .strip_prefix('(')
                        .and_then(|v| v.strip_suffix(')'))
                        .and_then(|v| v.trim().split_once(':'))
                        .and_then(|(namespace, v)| {
                            v.split_once(',')
                                .map(|(name, units)| (namespace, name.trim(), units.trim()))
                        })
                        .filter(|(_, _, units)| units.eq_ignore_ascii_case("string"));
                    if let Some((namespace, name, _)) = string_variable {
                        if format.kind != Kind::String
                            || !namespace.eq_ignore_ascii_case("A")
                            || name.is_empty()
                            || name.len() > 256
                            || !name.is_ascii()
                            || name.contains(['(', ')', ','])
                        {
                            return Err(
                                "String output requires a bounded A: string read and s format"
                                    .into(),
                            );
                        }
                        Part::StringVariable(name.to_owned(), format)
                    } else {
                        Part::Numeric(GaugeExpression::compile(&expression)?, format)
                    }
                } else if tail.starts_with("%{if}") {
                    self.offset += 5;
                    let program = GaugeExpression::compile(&expression)?;
                    let (yes, terminator) = self.block(depth + 1)?;
                    let no = if terminator == Some("else") {
                        let (no, end) = self.block(depth + 1)?;
                        if end != Some("end") {
                            return Err("GaugeString else lacks end".into());
                        }
                        no
                    } else if terminator == Some("end") {
                        Vec::new()
                    } else {
                        return Err("GaugeString if lacks end".into());
                    };
                    Part::If(program, yes, no)
                } else {
                    return Err("Unsupported GaugeString expression format".into());
                }
            } else if rest.starts_with("%%") {
                self.offset += 2;
                Part::Literal("%".into())
            } else if rest.starts_with(r"\{clr2}") {
                self.offset += 7;
                Part::Color(Color::Alternate)
            } else if rest.starts_with(r"\{clr}") {
                self.offset += 6;
                Part::Color(Color::Default)
            } else if rest.starts_with('\\') {
                let escape = rest
                    .as_bytes()
                    .get(1)
                    .ok_or("Unclosed GaugeString escape")?;
                let value = match escape {
                    b'n' => "\n",
                    b't' => "\t",
                    b'r' => "\r",
                    b'\\' => "\\",
                    _ => return Err("Unsupported GaugeString escape".into()),
                };
                self.offset += 2;
                Part::Literal(value.into())
            } else if rest.starts_with('%') {
                return Err("Unsupported GaugeString directive".into());
            } else {
                let length = rest
                    .char_indices()
                    .skip(1)
                    .find(|(_, c)| *c == '%' || *c == '\\')
                    .map_or(rest.len(), |(i, _)| i);
                self.offset += length;
                Part::Literal(rest[..length].to_owned())
            };
            parts.push(part);
        }
        Ok((parts, None))
    }
}

impl GaugeString {
    pub fn compile(source: &str) -> Result<Self, String> {
        if source.len() > MAX_SOURCE || source.contains('\0') {
            return Err("GaugeString source limit or NUL".into());
        }
        let mut parser = Parser {
            source,
            offset: 0,
            count: 0,
        };
        let (parts, terminator) = parser.block(0)?;
        if terminator.is_some() {
            return Err("Unexpected GaugeString branch terminator".into());
        }
        Ok(Self { parts })
    }

    /// Caller supplies namespace, name and units explicitly. Unsupported values
    /// fail; strings are emitted as data and never parsed as additional markup.
    pub fn evaluate(
        &self,
        mut numeric: impl FnMut(char, &str, &str) -> Option<f64>,
        mut string: impl FnMut(&str) -> Option<String>,
    ) -> Result<Vec<Span>, EvalError> {
        let mut output = Vec::new();
        let mut color = Color::Default;
        let mut size = 0;
        fn run(
            parts: &[Part],
            color: &mut Color,
            size: &mut usize,
            output: &mut Vec<Span>,
            numeric: &mut impl FnMut(char, &str, &str) -> Option<f64>,
            string: &mut impl FnMut(&str) -> Option<String>,
        ) -> Result<(), EvalError> {
            for part in parts {
                let text = match part {
                    Part::Literal(text) => text.clone(),
                    Part::StringVariable(name, format) => {
                        format.text(string(name).ok_or_else(|| EvalError::Unavailable {
                            variable: format!("A:{name}"),
                            units: "string".into(),
                        })?)?
                    }
                    Part::Numeric(program, format) => {
                        format.number(program.evaluate(&mut *numeric)?)?
                    }
                    Part::Color(value) => {
                        *color = *value;
                        continue;
                    }
                    Part::If(program, yes, no) => {
                        let condition = program.evaluate(&mut *numeric)?;
                        run(
                            if condition != 0. { yes } else { no },
                            color,
                            size,
                            output,
                            numeric,
                            string,
                        )?;
                        continue;
                    }
                };
                if text.len() > MAX_OUTPUT - *size || text.contains('\0') {
                    return Err(EvalError::Invalid("GaugeString output limit or NUL".into()));
                }
                *size += text.len();
                if !text.is_empty() {
                    if let Some(last) = output.last_mut().filter(|last| last.color == *color) {
                        last.text.push_str(&text);
                    } else {
                        output.push(Span {
                            text,
                            color: *color,
                        });
                    }
                }
            }
            Ok(())
        }
        run(
            &self.parts,
            &mut color,
            &mut size,
            &mut output,
            &mut numeric,
            &mut string,
        )?;
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Program;
    fn formatted(source: &str) -> String {
        GaugeString::compile(source)
            .unwrap()
            .evaluate(|_, _, _| None, |_| None)
            .unwrap()
            .into_iter()
            .map(|s| s.text)
            .collect()
    }
    #[test]
    fn integer_formats_round_pad_and_preserve_sign() {
        for (source, expected) in [
            ("%(27.6)%!d!", "28"),
            ("%(-27.6)%!d!", "-28"),
            ("%(27.5)%!d!", "28"),
            ("%(-27.5)%!d!", "-28"),
            ("%(27.4)%!+05d!", "+0027"),
            ("%(-27.4)%!05d!", "-0027"),
            ("%(27)%!5d!", "   27"),
            ("%(27)%!-5d!", "27   "),
            ("%(27)%! 5d!", "   27"),
            ("%(123456)%!2d!", "123456"),
            ("%(-0.4)%!d!", "0"),
            ("%(0)%!+d!", "0"),
        ] {
            assert_eq!(formatted(source), expected, "{source}");
        }
    }
    #[test]
    fn floating_formats_use_precision_and_ignore_field_width() {
        for (source, expected) in [
            ("%(7.8125)%!2.3f!", "7.812"),
            ("%(7.8125)%!02.4f!", "7.8125"),
            ("%(5432.625)%!1.2f!", "5432.62"),
            ("%(-0.125)%!.3f!", "-0.125"),
            ("%(1.25)%!f!", "1.250000"),
            ("%(8)%!.0f!", "8"),
        ] {
            assert_eq!(formatted(source), expected, "{source}");
        }
    }
    #[test]
    fn formatted_variables_keep_units_namespaces_and_inactive_branches_lazy() {
        let script = GaugeString::compile(r"%(1)%{if}COM %((A:COM ACTIVE FREQUENCY:1, MHz))%!6.3f! ALT %((A:INDICATED ALTITUDE, feet))%!+06d! %((P:clock, seconds) 2 /)%!04d!%{else}%((A:missing, number))%!d!%{end} ID %((A:ATC ID, string))%!6s!").unwrap();
        let result = script
            .evaluate(
                |ns, name, units| match (ns, name, units) {
                    ('A', "COM ACTIVE FREQUENCY:1", "MHz") => Some(123.45),
                    ('A', "INDICATED ALTITUDE", "feet") => Some(1524.3),
                    ('P', "clock", "seconds") => Some(14.),
                    _ => None,
                },
                |name| (name == "ATC ID").then(|| "OWN7".into()),
            )
            .unwrap();
        assert_eq!(result[0].text, "COM 123.450 ALT +01524 0007 ID   OWN7");
        assert_eq!(formatted("%(314)%!5s!"), "  314");
        let unicode = GaugeString::compile("%(( A:id, string ))%!5s!").unwrap();
        assert_eq!(
            unicode
                .evaluate(|_, _, _| None, |_| Some("Я7".into()))
                .unwrap()[0]
                .text,
            "   Я7"
        );
    }
    #[test]
    fn numeric_format_rejections_and_output_bounds() {
        for code in [
            "",
            "e",
            "x",
            "999999999999999999999d",
            "257d",
            ".17f",
            "3.2d",
            "3dextra",
            "0s",
            "++d",
            "1😈",
            "+f",
            "3..2f",
            "n",
            "*d",
        ] {
            assert!(
                GaugeString::compile(&format!("%(1)%!{code}! ")).is_err(),
                "{code}"
            );
        }
        for source in ["%(1e30)%!d!", "%(1.5)%!s!", "%((A:x, string))%!d!"] {
            assert!(GaugeString::compile(source)
                .and_then(|p| p
                    .evaluate(|_, _, _| None, |_| Some("x".into()))
                    .map_err(|e| format!("{e:?}")))
                .is_err());
        }
        let script = GaugeString::compile(&"%(1)%!256d!".repeat(33)).unwrap();
        assert!(script.evaluate(|_, _, _| None, |_| None).is_err());
        let script = GaugeString::compile("%((P:missing, number))%!d!").unwrap();
        assert_eq!(
            script.evaluate(|_, _, _| None, |_| None),
            Err(EvalError::Unavailable {
                variable: "P:missing".into(),
                units: "number".into()
            })
        );
    }
    #[test]
    fn string_color_branches_and_namespace_units() {
        let script = GaugeString::compile(r"ID %( (P:Clock phase, enum) 2 == (A:Panel switch, bool) and )%{if}\{clr2}%{else}day:%{end}%( (A:Registration, string) )%!s!\{clr} 100%%").unwrap();
        for (phase, switch, alternate) in [(2., 1., true), (2., 0., false), (1., 1., false)] {
            let result = script
                .evaluate(
                    |namespace, name, units| match (namespace, name, units) {
                        ('P', "Clock phase", "enum") => Some(phase),
                        ('A', "Panel switch", "bool") => Some(switch),
                        _ => None,
                    },
                    |name| (name == "Registration").then(|| "SYN-42".into()),
                )
                .unwrap();
            if alternate {
                assert_eq!(
                    result,
                    [
                        Span {
                            text: "ID ".into(),
                            color: Color::Default
                        },
                        Span {
                            text: "SYN-42".into(),
                            color: Color::Alternate
                        },
                        Span {
                            text: " 100%".into(),
                            color: Color::Default
                        }
                    ]
                );
            } else {
                assert_eq!(
                    result,
                    [Span {
                        text: "ID day:SYN-42 100%".into(),
                        color: Color::Default
                    }]
                );
            }
        }
        assert!(Program::compile("(P:Clock phase, enum)").is_err());
    }
    #[test]
    fn nested_branches_short_circuit_and_string_markup_is_data() {
        let script = GaugeString::compile("%(1)%{if}%(0)%{if}%( (A:missing, string) )%!s!%{else}%( (A:x, string) )%!s!%{end}%{else}bad%{end}").unwrap();
        assert_eq!(
            script
                .evaluate(|_, _, _| None, |_| Some(r"\{clr2}%{end}".into()))
                .unwrap(),
            [Span {
                text: r"\{clr2}%{end}".into(),
                color: Color::Default
            }]
        );
        let missing = GaugeString::compile("%((A:missing, bool))%{if}yes%{end}").unwrap();
        assert!(matches!(
            missing.evaluate(|_, _, _| None, |_| None),
            Err(EvalError::Unavailable { .. })
        ));
    }
    #[test]
    fn rejects_writes_unknown_formats_unbalanced_branches_and_limits() {
        for source in [
            "%{end}",
            "%{else}",
            "%(1)%{if}x",
            "%(1)%{if}x%{else}y",
            "%((>A:x, number))%{if}x%{end}",
            "%((K:x, number))%{if}x%{end}",
            "%((L:x, number))%{if}x%{end}",
            "%(1)%!x!",
            "%((P:x, string))%!s!",
            r"\{other}",
            "%(",
        ] {
            assert!(GaugeString::compile(source).is_err(), "{source}");
        }
        assert!(GaugeString::compile(&"x".repeat(MAX_SOURCE + 1)).is_err());
        assert!(GaugeString::compile(&format!(
            "{}{}",
            "%(1)%{if}".repeat(40),
            "%{end}".repeat(40)
        ))
        .is_err());
        let script = GaugeString::compile("%((A:x, string))%!s!%((A:x, string))%!s!").unwrap();
        assert!(script
            .evaluate(|_, _, _| None, |_| Some("x".repeat(5000)))
            .is_err());
    }

    #[test]
    fn panel_names_do_not_alias_indexed_aircraft_names_and_escapes_are_explicit() {
        let script = GaugeString::compile(r"%((A:P:x, number) (A:engine:1, number) + (P:x, number) == 0 ||)%{if}строка\n\t\\%{else}bad%{end}").unwrap();
        let result = script
            .evaluate(
                |namespace, name, units| {
                    assert_eq!(units, "number");
                    match (namespace, name) {
                        ('A', "P:x") => Some(2.),
                        ('A', "engine:1") => Some(3.),
                        ('P', "x") => Some(5.),
                        _ => None,
                    }
                },
                |_| None,
            )
            .unwrap();
        assert_eq!(
            result,
            [Span {
                text: "строка\n\t\\".into(),
                color: Color::Default
            }]
        );
        let script = GaugeString::compile("%((P:missing, bool))%{if}x%{end}").unwrap();
        assert_eq!(
            script.evaluate(|_, _, _| None, |_| None),
            Err(EvalError::Unavailable {
                variable: "P:missing".into(),
                units: "bool".into()
            })
        );
        assert!(script.evaluate(|_, _, _| Some(f64::NAN), |_| None).is_err());
        for source in [r"\q", r"\{clr3}", "\\"] {
            assert!(GaugeString::compile(source).is_err());
        }
    }
}
