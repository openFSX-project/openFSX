//! Native VISL conditions and explicit fallback diagnostics.
use fsx_script::{EvalError, Program};

#[derive(Debug)]
pub struct Rule {
    pub default_visible: bool,
    pub source: String,
    condition: Condition,
}
#[derive(Debug)]
enum Condition {
    Sim(String, String),
    Code(std::result::Result<Program, String>),
    Deferred(String),
}
#[derive(Debug, PartialEq)]
pub enum Status {
    Evaluated,
    Unavailable,
    Unsupported(String),
    InvalidEvaluation(String),
}
#[derive(Debug)]
pub struct Decision {
    pub visible: bool,
    pub status: Status,
}
#[derive(Debug)]
pub struct Plan {
    pub nodes: Vec<bool>,
    pub decisions: Vec<Decision>,
}

pub(super) fn parse(bytes: &[u8]) -> super::Result<Vec<Rule>> {
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("VISL exceeds 4 MiB".into());
    }
    let mut rules = Vec::new();
    let mut instructions = 0;
    for (tag, data) in super::chunks(bytes)? {
        if rules.len() >= i16::MAX as usize {
            return Err("Too many visibility conditions".into());
        }
        if data.len() > 16 * 1024 + 1 {
            return Err("Visibility condition exceeds 16 KiB".into());
        }
        let end = data
            .iter()
            .position(|&c| c == 0)
            .ok_or("Unterminated visibility condition")?;
        if data[end..].iter().any(|&c| c != 0) {
            return Err("Nonzero data after visibility terminator".into());
        }
        let source = String::from_utf8_lossy(&data[..end]).into_owned();
        let condition = match &tag {
            b"VISS" | b"VINS" => {
                let fields: Vec<_> = source.split('#').collect();
                if fields.len() == 2 && !fields[0].trim().is_empty() && !fields[1].trim().is_empty()
                {
                    Condition::Sim(fields[0].trim().into(), fields[1].trim().into())
                } else {
                    Condition::Deferred("Visibility Sim modifiers or fields are unsupported".into())
                }
            }
            b"VISC" | b"VINC" => {
                let mut code = source.as_str();
                let mut stateful = false;
                if let Some(rest) = code.strip_prefix('#') {
                    let (number, rest) = rest
                        .split_once('#')
                        .ok_or("Invalid visibility lag prefix")?;
                    let lag: f64 = number.parse().map_err(|_| "Invalid visibility lag")?;
                    if !lag.is_finite() || lag < 0. {
                        return Err("Invalid visibility lag".into());
                    }
                    stateful = lag > 0.;
                    code = rest;
                }
                if stateful {
                    Condition::Deferred("Temporal visibility lag is not implemented".into())
                } else {
                    let program = Program::compile(code);
                    if let Ok(program) = &program {
                        instructions += program.instruction_count();
                        if instructions > 262144 {
                            return Err("VISL exceeds the total 262144-instruction budget".into());
                        }
                    }
                    Condition::Code(program)
                }
            }
            _ => Condition::Deferred(format!(
                "Unknown VISL tag {}",
                String::from_utf8_lossy(&tag)
            )),
        };
        rules.push(Rule {
            default_visible: &tag != b"VINC" && &tag != b"VINS",
            source,
            condition,
        });
    }
    Ok(rules)
}
impl Rule {
    pub fn evaluate(&self, lookup: &mut impl FnMut(&str, &str) -> Option<f64>) -> Decision {
        let result = match &self.condition {
            Condition::Sim(name, units) => {
                lookup(name, units).ok_or_else(|| EvalError::Unavailable {
                    variable: name.clone(),
                    units: units.clone(),
                })
            }
            Condition::Code(Ok(program)) => program.evaluate(lookup),
            Condition::Code(Err(reason)) | Condition::Deferred(reason) => {
                return Decision {
                    visible: self.default_visible,
                    status: Status::Unsupported(reason.clone()),
                }
            }
        };
        match result {
            Ok(value) if value.is_finite() => Decision {
                visible: value != 0.,
                status: Status::Evaluated,
            },
            Ok(_) => Decision {
                visible: self.default_visible,
                status: Status::InvalidEvaluation("Non-finite visibility variable".into()),
            },
            Err(EvalError::Unavailable { .. }) => Decision {
                visible: self.default_visible,
                status: Status::Unavailable,
            },
            Err(EvalError::Invalid(reason)) => Decision {
                visible: self.default_visible,
                status: Status::InvalidEvaluation(reason),
            },
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn chunk(tag: &[u8; 4], text: &[u8]) -> Vec<u8> {
        [tag.as_slice(), &(text.len() as u32).to_le_bytes(), text].concat()
    }
    #[test]
    fn native_conditions_defaults_and_deferred_state() {
        let rules = parse(
            &[
                chunk(b"VISC", b"(A:TEST RATE, percent) 3 > if{ 1 } els{ 0 }\0"),
                chunk(b"VINC", b"(A:MISSING, bool)\0"),
                chunk(b"VISS", b"TEST SWITCH#bool\0"),
                chunk(b"VINS", b"TEST SWITCH#bool\0"),
                chunk(b"VISC", b"#5#1\0"),
                chunk(b"VISC", b"(>K:EVENT)\0"),
            ]
            .concat(),
        )
        .unwrap();
        assert!(!rules[0].evaluate(&mut |_, _| Some(2.)).visible);
        assert!(rules[0].evaluate(&mut |_, _| Some(4.)).visible);
        let missing = rules[1].evaluate(&mut |_, _| None);
        assert!(!missing.visible && missing.status == Status::Unavailable);
        assert!(!rules[2].evaluate(&mut |_, _| Some(0.)).visible);
        assert!(rules[3].evaluate(&mut |_, _| Some(1.)).visible);
        for rule in &rules[4..] {
            let d = rule.evaluate(&mut |_, _| None);
            assert!(d.visible && matches!(d.status, Status::Unsupported(_)));
        }
        assert!(matches!(
            rules[2].evaluate(&mut |_, _| Some(f64::NAN)).status,
            Status::InvalidEvaluation(_)
        ));
    }
    #[test]
    fn malformed_lengths_strings_and_lag_are_errors() {
        for data in [
            chunk(b"VISC", b"1"),
            chunk(b"VISC", b"1\0x"),
            chunk(b"VISC", b"#NaN#1\0"),
            chunk(b"VISC", b"#-1#1\0"),
            chunk(b"VISS", &vec![1; 16386]),
        ] {
            assert!(parse(&data).is_err());
        }
        let original = chunk(b"VISC", b"1\0");
        for n in 1..original.len() {
            assert!(parse(&original[..n]).is_err());
        }
        let budget_source = format!("{}0\0", "0 d ".repeat(2047));
        let many_programs = chunk(b"VISC", budget_source.as_bytes()).repeat(65);
        assert!(parse(&many_programs)
            .unwrap_err()
            .contains("instruction budget"));
        assert!(parse(&vec![0; 4 * 1024 * 1024 + 1])
            .unwrap_err()
            .contains("4 MiB"));
    }
}
