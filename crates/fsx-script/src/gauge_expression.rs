//! Read-only numeric expressions for XML gauges, with explicit A:/P: lookup.
use crate::{EvalError, Program};

#[derive(Debug)]
pub struct GaugeExpression {
    program: Program,
}
impl GaugeExpression {
    pub fn compile(source: &str) -> Result<Self, String> {
        Ok(Self {
            program: Program::compile_mode(source, false, true)?,
        })
    }
    pub fn evaluate(
        &self,
        mut lookup: impl FnMut(char, &str, &str) -> Option<f64>,
    ) -> Result<f64, EvalError> {
        self.program
            .evaluate(|name, units| match name.strip_prefix("\0P:") {
                Some(name) => lookup('P', name, units),
                None => lookup('A', name, units),
            })
            .map_err(|error| match error {
                EvalError::Unavailable { variable, units } => EvalError::Unavailable {
                    variable: variable.strip_prefix('\0').unwrap_or(&variable).to_owned(),
                    units,
                },
                other => other,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gauge_expressions_keep_namespaces_read_only_and_model_mode_separate() {
        let expression =
            GaugeExpression::compile("(A:sample:1, number) (P:clock, seconds) >").unwrap();
        assert_eq!(
            expression
                .evaluate(|ns, name, units| match (ns, name, units) {
                    ('A', "sample:1", "number") => Some(4.),
                    ('P', "clock", "seconds") => Some(3.),
                    _ => None,
                })
                .unwrap(),
            1.
        );
        for source in [
            "(>A:x, number)",
            "(L:x, number)",
            "(K:x, number)",
            "(>P:x, number)",
        ] {
            assert!(GaugeExpression::compile(source).is_err());
        }
        assert!(Program::compile("(P:clock, seconds)").is_err());
        assert_eq!(
            expression.evaluate(|_, _, _| None),
            Err(EvalError::Unavailable {
                variable: "sample:1".into(),
                units: "number".into()
            })
        );
    }
}
