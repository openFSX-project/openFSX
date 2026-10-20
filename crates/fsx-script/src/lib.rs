//! Bounded numeric RPN and GaugeString evaluation. Optional in-memory model locals; no host events.
use std::collections::BTreeMap;
mod gauge_format;
pub mod gauge_string;
const MAX_SOURCE: usize = 16 * 1024;
const MAX_OPS: usize = 4096;
const MAX_DEPTH: usize = 32;
const MAX_STACK: usize = 64;

#[derive(Debug)]
pub struct Program {
    ops: Vec<Op>,
    instructions: usize,
    needs_locals: bool,
}
#[derive(Debug)]
enum Op {
    Number(f64),
    Variable(String, String),
    LocalRead(String, String),
    LocalWrite(String, String),
    RegisterStore(usize, bool),
    RegisterLoad(usize),
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
#[derive(Debug, Clone, Default)]
pub struct LocalState {
    values: BTreeMap<String, (String, f64)>,
}
impl LocalState {
    fn key(name: &str, units: &str) -> Result<(String, String), EvalError> {
        let (name, units) = (name.trim(), units.trim());
        if name.is_empty()
            || name.len() > 256
            || units.is_empty()
            || units.len() > 64
            || !name.is_ascii()
            || !units.is_ascii()
        {
            return Err(EvalError::Invalid(
                "Invalid model-local name or units".into(),
            ));
        }
        Ok((name.to_ascii_lowercase(), units.to_ascii_lowercase()))
    }
    pub fn get(&self, name: &str, units: &str) -> Option<f64> {
        let (name, units) = Self::key(name, units).ok()?;
        self.values
            .get(&name)
            .filter(|(stored, _)| stored == &units)
            .map(|(_, value)| *value)
    }
    fn read(&mut self, name: &str, units: &str) -> Result<f64, EvalError> {
        let (key, unit) = Self::key(name, units)?;
        if let Some((stored, value)) = self.values.get(&key) {
            return if stored == &unit {
                Ok(*value)
            } else {
                Err(EvalError::Unavailable {
                    variable: format!("L:{name}"),
                    units: units.into(),
                })
            };
        }
        self.write(name, units, 0.)?;
        Ok(0.)
    }
    fn write(&mut self, name: &str, units: &str, value: f64) -> Result<(), EvalError> {
        let (key, unit) = Self::key(name, units)?;
        if !value.is_finite() {
            return Err(EvalError::Invalid("Non-finite model-local value".into()));
        }
        if let Some((stored, _)) = self.values.get(&key) {
            if stored != &unit {
                return Err(EvalError::Unavailable {
                    variable: format!("L:{name}"),
                    units: units.into(),
                });
            }
        } else if self.values.len() >= 512 {
            return Err(EvalError::Invalid(
                "Model-local variable limit exceeds 512".into(),
            ));
        }
        self.values.insert(key, (unit, value));
        Ok(())
    }
}
struct Parser<'a> {
    source: &'a str,
    offset: usize,
    count: usize,
    allow_locals: bool,
    allow_panel: bool,
    needs_locals: bool,
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
                let local_read = prefix.eq_ignore_ascii_case("L") && self.allow_locals;
                let local_write = prefix.eq_ignore_ascii_case(">L") && self.allow_locals;
                let panel = prefix.eq_ignore_ascii_case("P") && self.allow_panel;
                if !prefix.eq_ignore_ascii_case("A") && !local_read && !local_write && !panel {
                    return Err(if self.allow_panel {
                        "Only read-only A: and P: variables are supported in GaugeString expressions"
                    } else if self.allow_locals {
                        "Only A: reads and model-local L: reads/writes are supported"
                    } else {
                        "Only read-only A: variables are supported"
                    }
                    .into());
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
                if local_read || local_write {
                    LocalState::key(name, units)
                        .map_err(|_| "Invalid model-local name or units")?;
                    self.needs_locals = true;
                    if local_read {
                        Op::LocalRead(name.into(), units.into())
                    } else {
                        Op::LocalWrite(name.into(), units.into())
                    }
                } else if panel {
                    // NUL cannot occur in source, so this internal key cannot
                    // alias an A: variable (including indexed names with colons).
                    Op::Variable(format!("\0P:{name}"), units.into())
                } else {
                    Op::Variable(name.into(), units.into())
                }
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
                    "or" | "||" => Op::Binary(Binary::Or),
                    "abs" => Op::Unary(Unary::Abs),
                    "/-/" => Op::Unary(Unary::Neg),
                    "!" => Op::Unary(Unary::Not),
                    "d" => Op::Duplicate,
                    _ if self.allow_locals
                        && (token.starts_with('s') || token.starts_with('l')) =>
                    {
                        let (digits, store, pop) = if let Some(s) = token.strip_prefix("sp") {
                            (s, true, true)
                        } else if let Some(s) = token.strip_prefix('s') {
                            (s, true, false)
                        } else {
                            (&token[1..], false, false)
                        };
                        let index: usize = digits
                            .parse()
                            .map_err(|_| "Unsupported model register token")?;
                        if index >= 50 || digits != index.to_string() {
                            return Err("Model register index must be 0..49".into());
                        }
                        self.needs_locals = true;
                        if store {
                            Op::RegisterStore(index, pop)
                        } else {
                            Op::RegisterLoad(index)
                        }
                    }
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
        Self::compile_mode(source, false, false)
    }
    /// Allows only caller-owned L: data and per-evaluation numeric registers.
    /// Aircraft writes, events, native calls and all other namespaces still fail.
    pub fn compile_with_locals(source: &str) -> Result<Self, String> {
        Self::compile_mode(source, true, false)
    }
    fn compile_mode(source: &str, allow_locals: bool, allow_panel: bool) -> Result<Self, String> {
        if source.len() > MAX_SOURCE || !source.is_ascii() || source.contains('\0') {
            return Err("RPN source must be ASCII, without NUL, and at most 16 KiB".into());
        }
        let mut parser = Parser {
            source,
            offset: 0,
            count: 0,
            allow_locals,
            allow_panel,
            needs_locals: false,
        };
        let ops = parser.block(0, false)?;
        Ok(Self {
            ops,
            instructions: parser.count,
            needs_locals: parser.needs_locals,
        })
    }
    pub fn instruction_count(&self) -> usize {
        self.instructions
    }
    pub fn needs_local_state(&self) -> bool {
        self.needs_locals
    }
    pub fn evaluate(
        &self,
        lookup: impl FnMut(&str, &str) -> Option<f64>,
    ) -> Result<f64, EvalError> {
        if self.needs_locals {
            return Err(EvalError::Invalid(
                "Model-local state context is required".into(),
            ));
        }
        self.run(lookup, None)
    }
    /// Commit local changes only after successful execution and one finite result.
    pub fn evaluate_with_locals(
        &self,
        locals: &mut LocalState,
        lookup: impl FnMut(&str, &str) -> Option<f64>,
    ) -> Result<f64, EvalError> {
        let mut staged = locals.clone();
        let result = self.run(lookup, Some(&mut staged))?;
        *locals = staged;
        Ok(result)
    }
    fn run(
        &self,
        lookup: impl FnMut(&str, &str) -> Option<f64>,
        locals: Option<&mut LocalState>,
    ) -> Result<f64, EvalError> {
        let mut stack = Vec::new();
        let mut runtime = Runtime {
            lookup,
            locals,
            registers: [None; 50],
        };
        evaluate(&self.ops, &mut stack, &mut runtime)?;
        if stack.len() != 1 {
            return Err(EvalError::Invalid(
                "RPN must leave exactly one result".into(),
            ));
        }
        Ok(stack[0])
    }
}
struct Runtime<'a, F> {
    lookup: F,
    locals: Option<&'a mut LocalState>,
    registers: [Option<f64>; 50],
}
fn pop(stack: &mut Vec<f64>) -> Result<f64, EvalError> {
    stack
        .pop()
        .ok_or_else(|| EvalError::Invalid("RPN stack underflow".into()))
}
fn evaluate(
    ops: &[Op],
    stack: &mut Vec<f64>,
    runtime: &mut Runtime<'_, impl FnMut(&str, &str) -> Option<f64>>,
) -> Result<(), EvalError> {
    for op in ops {
        let value = match op {
            Op::Number(value) => *value,
            Op::Variable(name, units) => {
                (runtime.lookup)(name, units).ok_or_else(|| EvalError::Unavailable {
                    variable: name.clone(),
                    units: units.clone(),
                })?
            }
            Op::LocalRead(name, units) => runtime
                .locals
                .as_deref_mut()
                .ok_or_else(|| EvalError::Invalid("Missing model-local context".into()))?
                .read(name, units)?,
            Op::LocalWrite(name, units) => {
                let value = pop(stack)?;
                runtime
                    .locals
                    .as_deref_mut()
                    .ok_or_else(|| EvalError::Invalid("Missing model-local context".into()))?
                    .write(name, units, value)?;
                continue;
            }
            Op::RegisterStore(index, remove) => {
                let value = if *remove {
                    pop(stack)?
                } else {
                    *stack
                        .last()
                        .ok_or_else(|| EvalError::Invalid("RPN stack underflow".into()))?
                };
                runtime.registers[*index] = Some(value);
                continue;
            }
            Op::RegisterLoad(index) => runtime.registers[*index]
                .ok_or_else(|| EvalError::Invalid("Uninitialized model register".into()))?,
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
                evaluate(if condition != 0. { yes } else { no }, stack, runtime)?;
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
    fn model_locals_persist_and_registers_are_fresh_for_each_evaluation() {
        let mut state = LocalState::default();
        let counter = Program::compile_with_locals(
            "(L:TEST COUNTER, number) 1 + d (>L:TEST COUNTER, number)",
        )
        .unwrap();
        assert!(counter.evaluate(|_, _| None).is_err());
        for expected in [1., 2., 3.] {
            assert_eq!(
                counter
                    .evaluate_with_locals(&mut state, |_, _| None)
                    .unwrap(),
                expected
            );
        }
        assert_eq!(state.get("test counter", "NUMBER"), Some(3.));
        assert_eq!(state.get("TEST COUNTER", "percent"), None);
        assert_eq!(
            counter
                .evaluate_with_locals(&mut LocalState::default(), |_, _| None)
                .unwrap(),
            1.
        );
        for (source, expected) in [("7 s0 l0 -", 0.), ("9 sp49 l49", 9.)] {
            assert_eq!(
                Program::compile_with_locals(source)
                    .unwrap()
                    .evaluate_with_locals(&mut state, |_, _| None)
                    .unwrap(),
                expected
            );
        }
        assert!(Program::compile_with_locals("l0")
            .unwrap()
            .evaluate_with_locals(&mut state, |_, _| None)
            .is_err());
        assert_eq!(
            Program::compile_with_locals(
                "0 if{ (A:MISSING, bool) } els{ (L:TEST COUNTER, number) }"
            )
            .unwrap()
            .evaluate_with_locals(&mut state, |_, _| None)
            .unwrap(),
            3.
        );
    }
    #[test]
    fn failed_model_evaluation_rolls_back_local_writes_and_type_conflicts() {
        let mut state = LocalState::default();
        Program::compile_with_locals("3 d (>L:VALUE, number)")
            .unwrap()
            .evaluate_with_locals(&mut state, |_, _| None)
            .unwrap();
        for suffix in [
            "(A:MISSING, bool)",
            "1 0 /",
            "1 2",
            "l49",
            "(L:VALUE, percent)",
        ] {
            let p = Program::compile_with_locals(&format!(
                "9 (>L:VALUE, number) 7 (>L:NEW VALUE, number) {suffix}"
            ))
            .unwrap();
            assert!(p.evaluate_with_locals(&mut state, |_, _| None).is_err());
            assert_eq!(state.get("VALUE", "number"), Some(3.));
            assert_eq!(state.get("NEW VALUE", "number"), None);
        }
        assert!(Program::compile_with_locals("1 (>L:VALUE, percent) 0")
            .unwrap()
            .evaluate_with_locals(&mut state, |_, _| None)
            .is_err());
        assert_eq!(state.get("VALUE", "number"), Some(3.));
    }
    #[test]
    fn model_context_is_bounded_and_cannot_write_host_state_or_send_events() {
        for source in [
            "1 (>A:VALUE, number) 0",
            "(K:EVENT, number)",
            "(>K:EVENT)",
            "(>H:EVENT)",
            "(G:VALUE, number)",
            "s50",
            "sp50",
            "l50",
            "s01",
            "s-1",
        ] {
            assert!(Program::compile_with_locals(source).is_err(), "{source}");
        }
        assert!(Program::compile_with_locals(&format!("(L:{}, number)", "x".repeat(257))).is_err());
        let mut state = LocalState::default();
        for index in 0..512 {
            Program::compile_with_locals(&format!("(L:ORIGINAL TEST {index}, number)"))
                .unwrap()
                .evaluate_with_locals(&mut state, |_, _| None)
                .unwrap();
        }
        assert!(Program::compile_with_locals("(L:ONE TOO MANY, number)")
            .unwrap()
            .evaluate_with_locals(&mut state, |_, _| None)
            .is_err());
        assert_eq!(state.values.len(), 512);
        assert_eq!(state.get("ONE TOO MANY", "number"), None);
    }
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
