//! Independent numeric, read-only RPN subset. No events, writes or native code.
const MAX_SOURCE: usize = 16 * 1024;
const MAX_OPS: usize = 4096;
const MAX_DEPTH: usize = 32;
const MAX_STACK: usize = 64;

#[derive(Debug)]
pub struct Program {
    ops: Vec<Op>,
    instructions: usize,
}
#[derive(Debug)]
enum Op {
    Number(f64),
    Variable(String, String),
    Binary(Binary),
    Unary(Unary),
    Duplicate,
    If(Vec<Op>, Vec<Op>),
}
#[derive(Debug, Clone, Copy)]
enum Binary {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}
#[derive(Debug, Clone, Copy)]
enum Unary {
    Abs,
    Neg,
    Not,
}
#[derive(Debug, PartialEq)]
pub enum EvalError {
    Unavailable { variable: String, units: String },
    Invalid(String),
}
struct Parser<'a> {
    source: &'a str,
    offset: usize,
    count: usize,
}
impl<'a> Parser<'a> {
    fn whitespace(&mut self) {
        while self
            .source
            .as_bytes()
            .get(self.offset)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.offset += 1;
        }
    }
    fn block(&mut self, depth: usize, nested: bool) -> Result<Vec<Op>, String> {
        if depth > MAX_DEPTH {
            return Err("RPN nesting exceeds 32".into());
        }
        let mut ops = Vec::new();
        loop {
            self.whitespace();
            let rest = &self.source[self.offset..];
            if rest.is_empty() {
                return if nested {
                    Err("Unclosed RPN branch".into())
                } else {
                    Ok(ops)
                };
            }
            if rest.starts_with('}') {
                if !nested {
                    return Err("Unexpected RPN closing brace".into());
                }
                self.offset += 1;
                return Ok(ops);
            }
            self.count += 1;
            if self.count > MAX_OPS {
                return Err("RPN instruction limit exceeded".into());
            }
            let op = if rest.starts_with("if{") {
                self.offset += 3;
                let yes = self.block(depth + 1, true)?;
                self.whitespace();
                let no = if self.source[self.offset..].starts_with("els{") {
                    self.offset += 4;
                    self.block(depth + 1, true)?
                } else {
                    Vec::new()
                };
                Op::If(yes, no)
            } else if rest.starts_with('(') {
                let end = rest.find(')').ok_or("Unclosed RPN variable")?;
                let inner = &rest[1..end];
                let (prefix, body) = inner
                    .split_once(':')
                    .ok_or("Missing RPN variable namespace")?;
                if !prefix.eq_ignore_ascii_case("A") {
                    return Err("Only read-only A: variables are supported".into());
                }
                let (name, units) = body
                    .split_once(',')
                    .ok_or("RPN variable requires explicit units")?;
                let (name, units) = (name.trim(), units.trim());
                if name.is_empty() || units.is_empty() || inner.contains('(') || units.contains(',')
                {
                    return Err("Invalid RPN variable".into());
                }
                self.offset += end + 1;
                Op::Variable(name.into(), units.into())
            } else {
                let end = rest
                    .find(|c: char| c.is_ascii_whitespace() || ['{', '}', '(', ')'].contains(&c))
                    .unwrap_or(rest.len());
                if end == 0 {
                    return Err("Unexpected RPN delimiter".into());
                }
                let token = &rest[..end];
                self.offset += end;
                match token {
                    "+" => Op::Binary(Binary::Add),
                    "-" => Op::Binary(Binary::Sub),
                    "*" => Op::Binary(Binary::Mul),
                    "/" => Op::Binary(Binary::Div),
                    "min" => Op::Binary(Binary::Min),
                    "max" => Op::Binary(Binary::Max),
                    "<" => Op::Binary(Binary::Lt),
                    "<=" => Op::Binary(Binary::Le),
                    ">" => Op::Binary(Binary::Gt),
                    ">=" => Op::Binary(Binary::Ge),
                    "==" => Op::Binary(Binary::Eq),
                    "!=" => Op::Binary(Binary::Ne),
                    "and" => Op::Binary(Binary::And),
                    "or" => Op::Binary(Binary::Or),
                    "abs" => Op::Unary(Unary::Abs),
                    "/-/" => Op::Unary(Unary::Neg),
                    "!" => Op::Unary(Unary::Not),
                    "d" => Op::Duplicate,
                    _ => {
                        let value: f64 = token
                            .parse()
                            .map_err(|_| format!("Unsupported RPN token: {token}"))?;
                        if !value.is_finite() {
                            return Err("Non-finite RPN literal".into());
                        }
                        Op::Number(value)
                    }
                }
            };
            ops.push(op);
        }
    }
}
impl Program {
    pub fn compile(source: &str) -> Result<Self, String> {
        if source.len() > MAX_SOURCE || !source.is_ascii() || source.contains('\0') {
            return Err("RPN source must be ASCII, without NUL, and at most 16 KiB".into());
        }
        let mut parser = Parser {
            source,
            offset: 0,
            count: 0,
        };
        let ops = parser.block(0, false)?;
        Ok(Self {
            ops,
            instructions: parser.count,
        })
    }
    pub fn instruction_count(&self) -> usize {
        self.instructions
    }
    pub fn evaluate(
        &self,
        mut lookup: impl FnMut(&str, &str) -> Option<f64>,
    ) -> Result<f64, EvalError> {
        let mut stack = Vec::new();
        evaluate(&self.ops, &mut stack, &mut lookup)?;
        if stack.len() != 1 {
            return Err(EvalError::Invalid(
                "RPN must leave exactly one result".into(),
            ));
        }
        Ok(stack[0])
    }
}
fn pop(stack: &mut Vec<f64>) -> Result<f64, EvalError> {
    stack
        .pop()
        .ok_or_else(|| EvalError::Invalid("RPN stack underflow".into()))
}
fn evaluate(
    ops: &[Op],
    stack: &mut Vec<f64>,
    lookup: &mut impl FnMut(&str, &str) -> Option<f64>,
) -> Result<(), EvalError> {
    for op in ops {
        let value = match op {
            Op::Number(value) => *value,
            Op::Variable(name, units) => {
                lookup(name, units).ok_or_else(|| EvalError::Unavailable {
                    variable: name.clone(),
                    units: units.clone(),
                })?
            }
            Op::Duplicate => *stack
                .last()
                .ok_or_else(|| EvalError::Invalid("RPN stack underflow".into()))?,
            Op::Unary(op) => {
                let a = pop(stack)?;
                match op {
                    Unary::Abs => a.abs(),
                    Unary::Neg => -a,
                    Unary::Not => f64::from(a == 0.),
                }
            }
            Op::Binary(op) => {
                let b = pop(stack)?;
                let a = pop(stack)?;
                match op {
                    Binary::Add => a + b,
                    Binary::Sub => a - b,
                    Binary::Mul => a * b,
                    Binary::Div => a / b,
                    Binary::Min => a.min(b),
                    Binary::Max => a.max(b),
                    Binary::Lt => f64::from(a < b),
                    Binary::Le => f64::from(a <= b),
                    Binary::Gt => f64::from(a > b),
                    Binary::Ge => f64::from(a >= b),
                    Binary::Eq => f64::from(a == b),
                    Binary::Ne => f64::from(a != b),
                    Binary::And => f64::from(a != 0. && b != 0.),
                    Binary::Or => f64::from(a != 0. || b != 0.),
                }
            }
            Op::If(yes, no) => {
                let condition = pop(stack)?;
                evaluate(if condition != 0. { yes } else { no }, stack, lookup)?;
                continue;
            }
        };
        if !value.is_finite() {
            return Err(EvalError::Invalid("Non-finite RPN result".into()));
        }
        if stack.len() >= MAX_STACK {
            return Err(EvalError::Invalid("RPN stack exceeds 64".into()));
        }
        stack.push(value);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn operand_order_boolean_and_lazy_nested_branches() {
        let p = Program::compile(
            "(A:TEST RATE, percent) 3 > if{ (A:TEST RATE, percent) 9 < if{ 1 } els{ 0 } } els{ 0 }",
        )
        .unwrap();
        for (input, expected) in [(0., 0.), (3., 0.), (6., 1.), (9., 0.), (20., 0.)] {
            assert_eq!(p.evaluate(|_, _| Some(input)).unwrap(), expected);
        }
        assert_eq!(
            Program::compile("9 2 - 3 / 2 > 1 and")
                .unwrap()
                .evaluate(|_, _| None)
                .unwrap(),
            1.
        );
        assert_eq!(
            Program::compile("0 if{ (A:UNAVAILABLE, bool) } els{ -4 abs d + }")
                .unwrap()
                .evaluate(|_, _| None)
                .unwrap(),
            8.
        );
        assert_eq!(
            Program::compile("1 /-/ !")
                .unwrap()
                .evaluate(|_, _| None)
                .unwrap(),
            0.
        );
    }
    #[test]
    fn unresolved_variables_and_invalid_execution_are_distinct() {
        let p = Program::compile("(A:TEST, bool)").unwrap();
        assert!(matches!(
            p.evaluate(|_, _| None),
            Err(EvalError::Unavailable { .. })
        ));
        assert!(matches!(
            p.evaluate(|_, _| Some(f64::NAN)),
            Err(EvalError::Invalid(_))
        ));
        for source in ["1 0 /", "+", "1 2", "0 if{ 5 }"] {
            assert!(matches!(
                Program::compile(source).unwrap().evaluate(|_, _| None),
                Err(EvalError::Invalid(_))
            ));
        }
    }
    #[test]
    fn malformed_effectful_and_unbounded_programs_are_rejected() {
        for source in [
            "}",
            "els{ 0 }",
            "1 if{ 0",
            "(A:TEST)",
            "(A:, bool)",
            "(L:LOCAL, bool)",
            "(>K:EVENT)",
            "1 s0",
            "g0",
            "NaN",
            "inf",
            "1\0",
        ] {
            assert!(Program::compile(source).is_err(), "{source}");
        }
        assert!(Program::compile(&"1 ".repeat(MAX_SOURCE)).is_err());
        assert!(Program::compile(&"1 if{".repeat(MAX_DEPTH + 2)).is_err());
        assert!(Program::compile(&"1 ".repeat(MAX_OPS + 1)).is_err());
        assert!(Program::compile(&"1 ".repeat(MAX_STACK + 1))
            .unwrap()
            .evaluate(|_, _| None)
            .is_err());
    }
}
