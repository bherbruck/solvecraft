//! Parameter expressions: `2 * width + 5 mm`, `angle / 2`, `sqrt(a^2 + b^2)`.
//!
//! Values carry a simple dimension (powers of length and angle). Lengths are millimetres and
//! angles radians internally. A unit-less number added to a dimensioned value takes that value's
//! unit (`width + 5` adds 5 mm); a unit-less final result takes the expected unit's default
//! (mm for lengths, degrees for angles). Trigonometric functions take an angle; a unit-less
//! argument is read as degrees.

use std::collections::BTreeMap;

use crate::DocError;

const MAX_LEN: usize = 4096;
const MAX_DEPTH: usize = 64;

/// What an expression must evaluate to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Length,
    Angle,
    Unitless,
}

impl Kind {
    pub fn from_unit(u: &str) -> Kind {
        match u.trim() {
            "deg" | "rad" | "°" => Kind::Angle,
            "" | "none" | "unitless" => Kind::Unitless,
            _ => Kind::Length,
        }
    }
}

/// A dimensioned value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Value {
    pub v: f64,
    pub len: i8,
    pub ang: i8,
}

impl Value {
    pub fn num(v: f64) -> Value {
        Value { v, len: 0, ang: 0 }
    }
    pub fn length(mm: f64) -> Value {
        Value { v: mm, len: 1, ang: 0 }
    }
    pub fn angle(rad: f64) -> Value {
        Value { v: rad, len: 0, ang: 1 }
    }
    fn unitless(&self) -> bool {
        self.len == 0 && self.ang == 0
    }
    /// Convert to the expected kind (mm or radians).
    pub fn to_kind(self, k: Kind) -> Result<f64, DocError> {
        let out = match k {
            Kind::Length if self.len == 1 && self.ang == 0 => self.v,
            Kind::Length if self.unitless() => self.v,
            Kind::Angle if self.ang == 1 && self.len == 0 => self.v,
            Kind::Angle if self.unitless() => self.v.to_radians(),
            Kind::Unitless if self.unitless() => self.v,
            _ => return Err(DocError::Expr(format!("wrong units for a {k:?} value"))),
        };
        if out.is_finite() { Ok(out) } else { Err(DocError::Expr("result is not a finite number".into())) }
    }
}

fn unit(name: &str) -> Option<Value> {
    Some(match name {
        "mm" => Value::length(1.0),
        "cm" => Value::length(10.0),
        "m" => Value::length(1000.0),
        "um" => Value::length(0.001),
        "in" => Value::length(25.4),
        "ft" => Value::length(304.8),
        "deg" => Value::angle(std::f64::consts::PI / 180.0),
        "rad" => Value::angle(1.0),
        _ => return None,
    })
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Id(String),
    Op(char),
}

fn lex(s: &str) -> Result<Vec<Tok>, DocError> {
    if s.len() > MAX_LEN {
        return Err(DocError::Expr("expression too long".into()));
    }
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while let Some(&c) = cs.get(i) {
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let st = i;
            while cs.get(i).is_some_and(|c| c.is_ascii_digit() || *c == '.') {
                i += 1;
            }
            // Exponent: 1e-3
            if cs.get(i).is_some_and(|c| *c == 'e' || *c == 'E') && cs.get(i + 1).is_some_and(|c| c.is_ascii_digit() || *c == '-' || *c == '+') {
                i += 2;
                while cs.get(i).is_some_and(char::is_ascii_digit) {
                    i += 1;
                }
            }
            let t: String = cs.get(st..i).map(|s| s.iter().collect()).unwrap_or_default();
            out.push(Tok::Num(t.parse().map_err(|_| DocError::Expr(format!("bad number `{t}`")))?));
        } else if c.is_alphabetic() || c == '_' {
            let st = i;
            while cs.get(i).is_some_and(|c| c.is_alphanumeric() || *c == '_') {
                i += 1;
            }
            out.push(Tok::Id(cs.get(st..i).map(|s| s.iter().collect()).unwrap_or_default()));
        } else if c == '°' {
            out.push(Tok::Id("deg".into()));
            i += 1;
        } else if "+-*/^(),".contains(c) {
            out.push(Tok::Op(c));
            i += 1;
        } else {
            return Err(DocError::Expr(format!("unexpected `{c}`")));
        }
    }
    Ok(out)
}

/// Identifiers an expression refers to (excluding units and functions).
pub fn references(s: &str) -> Vec<String> {
    let Ok(toks) = lex(s) else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if let Tok::Id(id) = t {
            let is_call = matches!(toks.get(i + 1), Some(Tok::Op('(')));
            if !is_call && unit(id).is_none() && id != "pi" && id != "PI" && !out.contains(id) {
                out.push(id.clone());
            }
        }
    }
    out
}

struct Parser<'a> {
    toks: Vec<Tok>,
    pos: usize,
    lookup: &'a dyn Fn(&str) -> Result<Value, DocError>,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }
    fn enter(&mut self) -> Result<(), DocError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { Err(DocError::Expr("expression nested too deeply".into())) } else { Ok(()) }
    }
    fn expr(&mut self) -> Result<Value, DocError> {
        self.enter()?;
        let mut a = self.term()?;
        while let Some(Tok::Op(c @ ('+' | '-'))) = self.peek().cloned() {
            self.pos += 1;
            let b = self.term()?;
            a = add(a, b, c == '-')?;
        }
        self.depth -= 1;
        Ok(a)
    }
    fn term(&mut self) -> Result<Value, DocError> {
        let mut a = self.unary()?;
        loop {
            match self.peek().cloned() {
                Some(Tok::Op('*')) => {
                    self.pos += 1;
                    let b = self.unary()?;
                    a = mul(a, b, false);
                }
                Some(Tok::Op('/')) => {
                    self.pos += 1;
                    let b = self.unary()?;
                    a = mul(a, b, true);
                }
                // Juxtaposition: `10 mm`, `2 width`.
                Some(Tok::Id(_) | Tok::Num(_)) | Some(Tok::Op('(')) => {
                    let b = self.unary()?;
                    a = mul(a, b, false);
                }
                _ => return Ok(a),
            }
        }
    }
    fn unary(&mut self) -> Result<Value, DocError> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.pos += 1;
                self.enter()?;
                let v = self.unary()?;
                self.depth -= 1;
                Ok(Value { v: -v.v, ..v })
            }
            Some(Tok::Op('+')) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }
    fn power(&mut self) -> Result<Value, DocError> {
        let base = self.atom()?;
        if let Some(Tok::Op('^')) = self.peek() {
            self.pos += 1;
            self.enter()?;
            let e = self.unary()?;
            self.depth -= 1;
            if !e.unitless() {
                return Err(DocError::Expr("exponent must be unit-less".into()));
            }
            let n = e.v;
            let k = if (n - n.round()).abs() < 1e-12 && n.abs() < 16.0 { n.round() as i8 } else { 0 };
            if !base.unitless() && k == 0 {
                return Err(DocError::Expr("dimensioned values need a small integer exponent".into()));
            }
            return Ok(Value { v: base.v.powf(n), len: base.len.saturating_mul(k), ang: base.ang.saturating_mul(k) });
        }
        Ok(base)
    }
    fn atom(&mut self) -> Result<Value, DocError> {
        match self.next() {
            Some(Tok::Num(v)) => Ok(Value::num(v)),
            Some(Tok::Op('(')) => {
                let v = self.expr()?;
                match self.next() {
                    Some(Tok::Op(')')) => Ok(v),
                    _ => Err(DocError::Expr("missing `)`".into())),
                }
            }
            Some(Tok::Id(id)) => {
                if let Some(Tok::Op('(')) = self.peek() {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Tok::Op(')'))) {
                        loop {
                            args.push(self.expr()?);
                            match self.next() {
                                Some(Tok::Op(',')) => continue,
                                Some(Tok::Op(')')) => break,
                                _ => return Err(DocError::Expr("bad function arguments".into())),
                            }
                        }
                    } else {
                        self.pos += 1;
                    }
                    return call(&id, &args);
                }
                if id == "pi" || id == "PI" {
                    return Ok(Value::num(std::f64::consts::PI));
                }
                if let Some(u) = unit(&id) {
                    return Ok(u);
                }
                (self.lookup)(&id)
            }
            Some(Tok::Op(c)) => Err(DocError::Expr(format!("unexpected `{c}`"))),
            None => Err(DocError::Expr("unexpected end of expression".into())),
        }
    }
}

fn add(a: Value, b: Value, sub: bool) -> Result<Value, DocError> {
    let (len, ang) = if a.unitless() {
        (b.len, b.ang)
    } else if b.unitless() || (a.len == b.len && a.ang == b.ang) {
        (a.len, a.ang)
    } else {
        return Err(DocError::Expr("cannot add values with different units".into()));
    };
    // A unit-less operand next to an angle is in degrees.
    let conv = |x: Value| if x.unitless() && ang == 1 && len == 0 { x.v.to_radians() } else { x.v };
    let (x, y) = (conv(a), conv(b));
    Ok(Value { v: if sub { x - y } else { x + y }, len, ang })
}

fn mul(a: Value, b: Value, div: bool) -> Value {
    if div {
        Value { v: a.v / b.v, len: a.len.saturating_sub(b.len), ang: a.ang.saturating_sub(b.ang) }
    } else {
        Value { v: a.v * b.v, len: a.len.saturating_add(b.len), ang: a.ang.saturating_add(b.ang) }
    }
}

fn call(f: &str, args: &[Value]) -> Result<Value, DocError> {
    let one = || match args {
        [a] => Ok(*a),
        _ => Err(DocError::Expr(format!("`{f}` takes one argument"))),
    };
    let rad = |a: Value| {
        if a.ang == 1 && a.len == 0 {
            Ok(a.v)
        } else if a.unitless() {
            Ok(a.v.to_radians())
        } else {
            Err(DocError::Expr(format!("`{f}` needs an angle")))
        }
    };
    let num = |a: Value| if a.unitless() { Ok(a.v) } else { Err(DocError::Expr(format!("`{f}` needs a unit-less value"))) };
    Ok(match f {
        "sin" => Value::num(rad(one()?)?.sin()),
        "cos" => Value::num(rad(one()?)?.cos()),
        "tan" => Value::num(rad(one()?)?.tan()),
        "asin" => Value::angle(num(one()?)?.asin()),
        "acos" => Value::angle(num(one()?)?.acos()),
        "atan" => Value::angle(num(one()?)?.atan()),
        "sqrt" => {
            let a = one()?;
            if a.len % 2 != 0 || a.ang % 2 != 0 {
                return Err(DocError::Expr("sqrt of an odd power of a unit".into()));
            }
            Value { v: a.v.sqrt(), len: a.len / 2, ang: a.ang / 2 }
        }
        "abs" => {
            let a = one()?;
            Value { v: a.v.abs(), ..a }
        }
        "floor" | "ceil" | "round" => {
            let a = one()?;
            let v = match f {
                "floor" => a.v.floor(),
                "ceil" => a.v.ceil(),
                _ => a.v.round(),
            };
            Value { v, ..a }
        }
        "min" | "max" => {
            let Some(first) = args.first().copied() else { return Err(DocError::Expr(format!("`{f}` needs arguments"))) };
            let mut acc = first;
            for a in args.iter().skip(1) {
                let d = add(*a, Value { v: 0.0, ..acc }, false)?;
                if (f == "min" && d.v < acc.v) || (f == "max" && d.v > acc.v) {
                    acc = d;
                }
            }
            acc
        }
        _ => return Err(DocError::Expr(format!("unknown function `{f}`"))),
    })
}

/// Evaluate an expression; `lookup` resolves parameter names.
pub fn eval_with(s: &str, lookup: &dyn Fn(&str) -> Result<Value, DocError>) -> Result<Value, DocError> {
    let toks = lex(s)?;
    if toks.is_empty() {
        return Err(DocError::Expr("empty expression".into()));
    }
    let mut p = Parser { toks, pos: 0, lookup, depth: 0 };
    let v = p.expr()?;
    if p.pos < p.toks.len() {
        return Err(DocError::Expr(format!("unexpected input in `{s}`")));
    }
    if !v.v.is_finite() {
        return Err(DocError::Expr(format!("`{s}` is not a finite number")));
    }
    Ok(v)
}

/// Evaluate all parameters (name → (expression, kind)) in dependency order.
pub fn eval_params(params: &[(String, String, Kind)]) -> (BTreeMap<String, Value>, BTreeMap<String, String>) {
    let mut done: BTreeMap<String, Value> = BTreeMap::new();
    let mut errors: BTreeMap<String, String> = BTreeMap::new();
    fn visit(
        name: &str,
        params: &[(String, String, Kind)],
        done: &mut BTreeMap<String, Value>,
        errors: &mut BTreeMap<String, String>,
        stack: &mut Vec<String>,
    ) -> Result<Value, DocError> {
        if let Some(v) = done.get(name) {
            return Ok(*v);
        }
        if let Some(e) = errors.get(name) {
            return Err(DocError::Expr(e.clone()));
        }
        let Some((_, expr, kind)) = params.iter().find(|(n, _, _)| n == name) else {
            return Err(DocError::Expr(format!("unknown parameter `{name}`")));
        };
        if stack.iter().any(|s| s == name) || stack.len() > 256 {
            return Err(DocError::Expr(format!("circular reference through `{name}`")));
        }
        stack.push(name.to_string());
        let r = {
            let deps = references(expr);
            let mut vals: BTreeMap<String, Value> = BTreeMap::new();
            let mut err = None;
            for d in deps {
                match visit(&d, params, done, errors, stack) {
                    Ok(v) => {
                        vals.insert(d, v);
                    }
                    Err(e) => {
                        err = Some(e);
                        break;
                    }
                }
            }
            match err {
                Some(e) => Err(e),
                None => eval_with(expr, &|n| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`")))).and_then(|v| {
                    let x = v.to_kind(*kind)?;
                    Ok(match kind {
                        Kind::Length => Value::length(x),
                        Kind::Angle => Value::angle(x),
                        Kind::Unitless => Value::num(x),
                    })
                }),
            }
        };
        stack.pop();
        match &r {
            Ok(v) => {
                done.insert(name.to_string(), *v);
            }
            Err(e) => {
                errors.insert(name.to_string(), e.to_string());
            }
        }
        r
    }
    for (n, _, _) in params {
        let mut stack = Vec::new();
        let _ = visit(n, params, &mut done, &mut errors, &mut stack);
    }
    (done, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str, k: Kind) -> Result<f64, DocError> {
        let look = |n: &str| match n {
            "width" => Ok(Value::length(40.0)),
            "ang" => Ok(Value::angle(std::f64::consts::FRAC_PI_2)),
            "count" => Ok(Value::num(3.0)),
            _ => Err(DocError::Expr(format!("unknown {n}"))),
        };
        eval_with(s, &look)?.to_kind(k)
    }

    #[test]
    fn arithmetic_and_units() {
        assert_eq!(ev("10", Kind::Length).unwrap(), 10.0);
        assert!((ev("90 deg", Kind::Angle).unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert_eq!(ev("10 mm", Kind::Length).unwrap(), 10.0);
        assert_eq!(ev("1 in", Kind::Length).unwrap(), 25.4);
        assert_eq!(ev("2 cm + 5", Kind::Length).unwrap(), 25.0);
        assert_eq!(ev("width / 2 + 1", Kind::Length).unwrap(), 21.0);
        assert_eq!(ev("width * count", Kind::Length).unwrap(), 120.0);
        assert_eq!(ev("-(2+3)*2^2", Kind::Unitless).unwrap(), -20.0);
        assert!((ev("45", Kind::Angle).unwrap() - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
        assert!((ev("ang / 2", Kind::Angle).unwrap() - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
        assert!((ev("ang + 90", Kind::Angle).unwrap() - std::f64::consts::PI).abs() < 1e-12);
        assert!((ev("sqrt((3 mm) ^ 2 + (4 mm) ^ 2)", Kind::Length).unwrap() - 5.0).abs() < 1e-12);
        assert!((ev("sin(30)", Kind::Unitless).unwrap() - 0.5).abs() < 1e-12);
        assert!((ev("2*pi", Kind::Unitless).unwrap() - std::f64::consts::TAU).abs() < 1e-12);
        assert_eq!(ev("max(1 mm, 3, 2)", Kind::Length).unwrap(), 3.0);
        assert_eq!(ev("1e-3 m", Kind::Length).unwrap(), 1.0);
    }

    #[test]
    fn errors() {
        assert!(ev("width + ang", Kind::Length).is_err());
        assert!(ev("width * width", Kind::Length).is_err());
        assert!(ev("nope", Kind::Length).is_err());
        assert!(ev("(1", Kind::Length).is_err());
        assert!(ev("1 / 0", Kind::Length).is_err());
        assert!(ev("", Kind::Length).is_err());
        assert!(ev("1 $ 2", Kind::Length).is_err());
        let deep = "(".repeat(500) + "1" + &")".repeat(500);
        assert!(ev(&deep, Kind::Length).is_err());
        assert!(ev(&"-".repeat(500), Kind::Length).is_err());
    }

    #[test]
    fn params_in_order_with_cycles() {
        let p = vec![
            ("b".to_string(), "a * 2".to_string(), Kind::Length),
            ("a".to_string(), "10 mm".to_string(), Kind::Length),
            ("x".to_string(), "y".to_string(), Kind::Length),
            ("y".to_string(), "x + 1".to_string(), Kind::Length),
        ];
        let (v, e) = eval_params(&p);
        assert_eq!(v.get("b").unwrap().v, 20.0);
        assert!(e.contains_key("x") && e.contains_key("y"));
        assert_eq!(references("2*width + sin(ang) + 3 mm"), vec!["width".to_string(), "ang".to_string()]);
    }
}
