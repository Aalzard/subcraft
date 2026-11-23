//! The small formula language of procedural materials (ADR-0047).
//!
//! A binding's formula turns the material's parameters into one property value, for example
//! `clamp(roughness - variation * 0.5, 0, 1)` or `mix(paint_color, rgb(0, 0, 0), age * 0.4)`.
//! Every value is three components; a number is the same on all three, so colours and numbers
//! mix freely (`paint_color * 0.5`). Formulas are plain text in the file: readable, diffable and
//! written by hand without a tool.
//!
//! Grammar: numbers, parameter ids, `+ - * /`, unary `-`, parentheses, and the functions listed
//! in [`FUNCTIONS`]. Unknown ids or functions and malformed text are errors; a binding with an
//! error is skipped, it never changes a layer to nonsense.

/// A formula value: three components (a number has the same value in all three).
pub type Value = [f32; 3];

/// Functions a formula may call, with their argument counts.
pub const FUNCTIONS: &[(&str, usize)] = &[
    ("mix", 3),
    ("clamp", 3),
    ("min", 2),
    ("max", 2),
    ("pow", 2),
    ("abs", 1),
    ("saturate", 1),
    ("step", 2),
    ("smoothstep", 3),
    ("rgb", 3),
    ("luma", 1),
    ("select", 3),
    ("remap", 3),
    ("round", 1),
];

#[derive(Clone, Debug, PartialEq)]
pub enum ExprError {
    /// Text that is not a formula, with the byte position where it stopped making sense.
    Syntax { at: usize, what: &'static str },
    UnknownParam(String),
    UnknownFunction(String),
    Arity { function: String, expected: usize, got: usize },
    /// Nested deeper than [`MAX_DEPTH`].
    TooDeep,
}

impl std::fmt::Display for ExprError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExprError::Syntax { at, what } => write!(f, "{what} at character {}", at + 1),
            ExprError::UnknownParam(p) => write!(f, "no parameter named '{p}'"),
            ExprError::UnknownFunction(n) => write!(f, "no function named '{n}'"),
            ExprError::Arity { function, expected, got } => write!(f, "{function}() takes {expected} values, got {got}"),
            ExprError::TooDeep => write!(f, "the formula is nested too deeply"),
        }
    }
}

/// Deepest nesting of parentheses and calls; a pasted formula cannot overflow the stack.
pub const MAX_DEPTH: usize = 48;
/// Longest formula text accepted.
pub const MAX_LENGTH: usize = 512;

/// Evaluate `text` with `param` giving each parameter's value (`None` = unknown id).
pub fn eval(text: &str, param: &dyn Fn(&str) -> Option<Value>) -> Result<Value, ExprError> {
    if text.len() > MAX_LENGTH {
        return Err(ExprError::Syntax { at: MAX_LENGTH, what: "formula too long" });
    }
    let mut p = Parser { src: text.as_bytes(), pos: 0, param, depth: 0 };
    let v = p.sum()?;
    p.skip_space();
    if p.pos != p.src.len() {
        return Err(ExprError::Syntax { at: p.pos, what: "unexpected text" });
    }
    Ok(v)
}

/// Parameter ids `text` reads (in order of first use), or the error that makes it unusable.
/// Ids are not checked against a parameter list here.
pub fn params_used(text: &str) -> Result<Vec<String>, ExprError> {
    let used = std::cell::RefCell::new(Vec::<String>::new());
    eval(text, &|id| {
        let mut u = used.borrow_mut();
        if !u.iter().any(|x| x == id) {
            u.push(id.to_string());
        }
        Some([0.5; 3])
    })?;
    Ok(used.into_inner())
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    param: &'a dyn Fn(&str) -> Option<Value>,
    depth: usize,
}

fn zip(a: Value, b: Value, f: impl Fn(f32, f32) -> f32) -> Value {
    [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])]
}

fn map(a: Value, f: impl Fn(f32) -> f32) -> Value {
    [f(a[0]), f(a[1]), f(a[2])]
}

impl Parser<'_> {
    fn skip_space(&mut self) {
        while self.src.get(self.pos).is_some_and(|c| c.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.skip_space();
        if self.src.get(self.pos) == Some(&c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn sum(&mut self) -> Result<Value, ExprError> {
        let mut v = self.product()?;
        loop {
            if self.eat(b'+') {
                v = zip(v, self.product()?, |a, b| a + b);
            } else if self.eat(b'-') {
                v = zip(v, self.product()?, |a, b| a - b);
            } else {
                return Ok(v);
            }
        }
    }

    fn product(&mut self) -> Result<Value, ExprError> {
        let mut v = self.unary()?;
        loop {
            if self.eat(b'*') {
                v = zip(v, self.unary()?, |a, b| a * b);
            } else if self.eat(b'/') {
                // Division by zero gives 0, not infinity: a slider at 0 must not blank a layer.
                v = zip(v, self.unary()?, |a, b| if b == 0.0 { 0.0 } else { a / b });
            } else {
                return Ok(v);
            }
        }
    }

    fn unary(&mut self) -> Result<Value, ExprError> {
        if self.eat(b'-') {
            return Ok(map(self.unary()?, |a| -a));
        }
        self.atom()
    }

    fn atom(&mut self) -> Result<Value, ExprError> {
        self.skip_space();
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(ExprError::TooDeep);
        }
        let start = self.pos;
        let out = match self.src.get(self.pos).copied() {
            Some(b'(') => {
                self.pos += 1;
                let v = self.sum()?;
                if !self.eat(b')') {
                    return Err(ExprError::Syntax { at: self.pos, what: "missing )" });
                }
                v
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => {
                while self.src.get(self.pos).is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
                    self.pos += 1;
                }
                let text = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("");
                let n: f32 = text.parse().map_err(|_| ExprError::Syntax { at: start, what: "not a number" })?;
                [n; 3]
            }
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                while self.src.get(self.pos).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
                    self.pos += 1;
                }
                let name = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("").to_string();
                if self.eat(b'(') {
                    self.call(&name)?
                } else {
                    (self.param)(&name).ok_or(ExprError::UnknownParam(name))?
                }
            }
            Some(_) => return Err(ExprError::Syntax { at: start, what: "unexpected character" }),
            None => return Err(ExprError::Syntax { at: start, what: "the formula ends early" }),
        };
        self.depth -= 1;
        Ok(out)
    }

    fn call(&mut self, name: &str) -> Result<Value, ExprError> {
        let Some(&(_, arity)) = FUNCTIONS.iter().find(|(n, _)| *n == name) else {
            return Err(ExprError::UnknownFunction(name.to_string()));
        };
        let mut args: Vec<Value> = Vec::with_capacity(arity);
        if !self.eat(b')') {
            loop {
                args.push(self.sum()?);
                if self.eat(b',') {
                    continue;
                }
                if self.eat(b')') {
                    break;
                }
                return Err(ExprError::Syntax { at: self.pos, what: "missing , or )" });
            }
        }
        if args.len() != arity {
            return Err(ExprError::Arity { function: name.to_string(), expected: arity, got: args.len() });
        }
        Ok(match name {
            "mix" => std::array::from_fn(|i| args[0][i] + (args[1][i] - args[0][i]) * args[2][i]),
            "clamp" => std::array::from_fn(|i| args[0][i].max(args[1][i]).min(args[2][i].max(args[1][i]))),
            "min" => zip(args[0], args[1], f32::min),
            "max" => zip(args[0], args[1], f32::max),
            "pow" => zip(args[0], args[1], |a, b| a.max(0.0).powf(b)),
            "abs" => map(args[0], f32::abs),
            "saturate" => map(args[0], |a| a.clamp(0.0, 1.0)),
            "step" => zip(args[0], args[1], |edge, x| if x >= edge { 1.0 } else { 0.0 }),
            "smoothstep" => std::array::from_fn(|i| {
                let (a, b, x) = (args[0][i], args[1][i], args[2][i]);
                if a == b {
                    return if x >= a { 1.0 } else { 0.0 };
                }
                let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            }),
            "rgb" => [args[0][0], args[1][0], args[2][0]],
            "luma" => [0.2126 * args[0][0] + 0.7152 * args[0][1] + 0.0722 * args[0][2]; 3],
            // select(condition, when_on, when_off): a toggle parameter is 1 or 0.
            "select" => std::array::from_fn(|i| if args[0][i] >= 0.5 { args[1][i] } else { args[2][i] }),
            // remap(t, at_0, at_1): the value at `t` on the line from `at_0` to `at_1`.
            "remap" => std::array::from_fn(|i| args[1][i] + (args[2][i] - args[1][i]) * args[0][i]),
            "round" => map(args[0], f32::round),
            _ => unreachable!("listed in FUNCTIONS"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(id: &str) -> Option<Value> {
        match id {
            "wear" => Some([0.25; 3]),
            "roughness" => Some([0.6; 3]),
            "variation" => Some([0.2; 3]),
            "paint" => Some([0.8, 0.4, 0.2]),
            "on" => Some([1.0; 3]),
            _ => None,
        }
    }

    fn close(a: Value, b: Value) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-6)
    }

    #[test]
    fn arithmetic_follows_the_usual_precedence() {
        assert_eq!(eval("1 + 2 * 3", &params), Ok([7.0; 3]));
        assert_eq!(eval("(1 + 2) * 3", &params), Ok([9.0; 3]));
        assert_eq!(eval("-2 * -3 - 1", &params), Ok([5.0; 3]));
        assert_eq!(eval(" 8 / 4 / 2 ", &params), Ok([1.0; 3]));
        assert_eq!(eval(".5 + 0.25", &params), Ok([0.75; 3]));
        assert_eq!(eval("3 / 0", &params), Ok([0.0; 3]), "a slider at 0 must not give infinity");
    }

    #[test]
    fn parameters_and_colours_mix_with_numbers() {
        assert!(close(eval("roughness - variation * 0.5", &params).unwrap(), [0.5; 3]));
        assert!(close(eval("paint * 0.5", &params).unwrap(), [0.4, 0.2, 0.1]));
        assert!(close(eval("mix(paint, rgb(0, 0, 0), wear)", &params).unwrap(), [0.6, 0.3, 0.15]));
        assert!(close(eval("luma(paint)", &params).unwrap(), [0.2126 * 0.8 + 0.7152 * 0.4 + 0.0722 * 0.2; 3]));
    }

    #[test]
    fn functions_compute_per_component() {
        assert_eq!(eval("clamp(roughness + 1, 0, 1)", &params), Ok([1.0; 3]));
        assert_eq!(eval("clamp(5, 2, 1)", &params), Ok([2.0; 3]), "a reversed range does not panic");
        assert_eq!(eval("min(wear, 0.1)", &params), Ok([0.1; 3]));
        assert_eq!(eval("max(wear, 0.1)", &params), Ok([0.25; 3]));
        assert_eq!(eval("saturate(-3)", &params), Ok([0.0; 3]));
        assert_eq!(eval("abs(-3)", &params), Ok([3.0; 3]));
        assert_eq!(eval("pow(4, 0.5)", &params), Ok([2.0; 3]));
        assert_eq!(eval("pow(-4, 0.5)", &params), Ok([0.0; 3]), "no NaN from a negative base");
        assert_eq!(eval("step(0.2, wear)", &params), Ok([1.0; 3]));
        assert_eq!(eval("smoothstep(0, 0.5, wear)", &params), Ok([0.5; 3]));
        assert_eq!(eval("smoothstep(1, 1, 2)", &params), Ok([1.0; 3]));
        assert_eq!(eval("select(on, 3, 4)", &params), Ok([3.0; 3]));
        assert_eq!(eval("select(0, 3, 4)", &params), Ok([4.0; 3]));
        assert_eq!(eval("remap(wear, 2, 6)", &params), Ok([3.0; 3]));
        assert_eq!(eval("round(2.6)", &params), Ok([3.0; 3]));
    }

    #[test]
    fn broken_formulas_are_errors_not_values() {
        assert_eq!(eval("missing + 1", &params), Err(ExprError::UnknownParam("missing".into())));
        assert_eq!(eval("sqrt(4)", &params), Err(ExprError::UnknownFunction("sqrt".into())));
        assert_eq!(eval("mix(1, 2)", &params), Err(ExprError::Arity { function: "mix".into(), expected: 3, got: 2 }));
        assert!(matches!(eval("1 +", &params), Err(ExprError::Syntax { .. })));
        assert!(matches!(eval("(1 + 2", &params), Err(ExprError::Syntax { .. })));
        assert!(matches!(eval("1 2", &params), Err(ExprError::Syntax { .. })));
        assert!(matches!(eval("", &params), Err(ExprError::Syntax { .. })));
        assert!(matches!(eval("1..2", &params), Err(ExprError::Syntax { .. })));
        assert!(matches!(eval("wear $ 2", &params), Err(ExprError::Syntax { .. })));
        let deep = format!("{}1{}", "(".repeat(MAX_DEPTH + 1), ")".repeat(MAX_DEPTH + 1));
        assert_eq!(eval(&deep, &params), Err(ExprError::TooDeep));
        assert!(matches!(eval(&"1+".repeat(MAX_LENGTH), &params), Err(ExprError::Syntax { .. })));
        assert!(ExprError::UnknownParam("x".into()).to_string().contains("'x'"));
    }

    #[test]
    fn the_parameters_a_formula_reads_are_listed_once_each() {
        assert_eq!(params_used("mix(a, b, wear) + a * 2").unwrap(), ["a", "b", "wear"]);
        assert_eq!(params_used("1 + 2").unwrap(), Vec::<String>::new());
        assert!(params_used("mix(a,").is_err());
    }
}
