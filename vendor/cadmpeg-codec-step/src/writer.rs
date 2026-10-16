// SPDX-License-Identifier: Apache-2.0
//! Encodes Part 21 DATA instances.
//!
//! The emitter allocates instance names, formats scalar values, counts entity
//! types, and interns repeated points and directions.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::collections::HashMap;

use cadmpeg_core::CodecError;
use cadmpeg_ir::transform::Transform;

pub(crate) mod target;

/// A STEP instance name such as `#42`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Ref(pub u64);

impl std::fmt::Display for Ref {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// A value the emitter was asked to state and Part 21 cannot state.
#[derive(Clone, Copy)]
enum Refusal {
    /// A non-finite number [`Emitter::real`] was asked to format.
    NonFiniteReal(f64),
    /// A transform [`Emitter::refuse_operator`] was given, which is not a
    /// similarity.
    NonSimilarOperator(Transform),
}

/// Accumulates DATA instances in allocation order and counts their entity types.
pub(crate) struct Emitter {
    lines: Vec<String>,
    counts: BTreeMap<&'static str, usize>,
    /// Leaf instances keyed by encoded type and parameters.
    interned: HashMap<String, Ref>,
    /// The first value the emitter could not state. The emitted lines are
    /// not handed out once one is recorded.
    refusal: Cell<Option<Refusal>>,
}

impl Emitter {
    pub(crate) fn new() -> Self {
        Emitter {
            lines: Vec::new(),
            counts: BTreeMap::new(),
            interned: HashMap::new(),
            refusal: Cell::new(None),
        }
    }

    /// Record `refusal` unless an earlier one is recorded.
    fn refuse(&self, refusal: Refusal) {
        if self.refusal.get().is_none() {
            self.refusal.set(Some(refusal));
        }
    }

    /// Record that `transform` has no `CARTESIAN_TRANSFORMATION_OPERATOR_3D`
    /// statement. [`Self::into_lines`] then refuses the whole DATA section.
    pub(crate) fn refuse_operator(&self, transform: Transform) {
        self.refuse(Refusal::NonSimilarOperator(transform));
    }

    /// Format `v` as a Part 21 real literal.
    ///
    /// A Part 21 real states a finite number only. A non-finite `v` is
    /// recorded, and [`Self::into_lines`] then refuses the whole DATA section,
    /// so the placeholder this returns never reaches a file.
    pub(crate) fn real(&self, v: f64) -> String {
        part21_real(v).unwrap_or_else(|| {
            self.refuse(Refusal::NonFiniteReal(v));
            String::new()
        })
    }

    /// Append `#id = TYPE(params);` and return the allocated reference.
    ///
    /// `type_` is also the entity-count key. Complex instances use their leading
    /// keyword as the key.
    pub(crate) fn emit(&mut self, type_: &'static str, params: &str) -> Ref {
        let id = cadmpeg_core::decode::u64_from_index(self.lines.len()) + 1;
        self.lines.push(format!("#{id} = {type_}({params});"));
        *self.counts.entry(type_).or_insert(0) += 1;
        Ref(id)
    }

    /// Append a preformatted entity or complex-instance body.
    ///
    /// `tally` supplies its entity-count key.
    pub(crate) fn emit_raw(&mut self, tally: &'static str, body: &str) -> Ref {
        let id = cadmpeg_core::decode::u64_from_index(self.lines.len()) + 1;
        self.lines.push(format!("#{id} = {body};"));
        *self.counts.entry(tally).or_insert(0) += 1;
        Ref(id)
    }

    /// Emit a value-like leaf or reuse an identical encoded instance.
    pub(crate) fn emit_interned(&mut self, type_: &'static str, params: &str) -> Ref {
        let key = format!("{type_}|{params}");
        if let Some(r) = self.interned.get(&key) {
            return *r;
        }
        let r = self.emit(type_, params);
        self.interned.insert(key, r);
        r
    }

    pub(crate) fn counts(&self) -> BTreeMap<String, usize> {
        self.counts
            .iter()
            .map(|(type_, count)| ((*type_).to_string(), *count))
            .collect()
    }

    /// Consume the emitter and return one encoded DATA instance per element,
    /// or refuse the section when [`Self::real`] was handed a non-finite
    /// number or [`Self::refuse_operator`] a transform.
    pub(crate) fn into_lines(self) -> Result<Vec<String>, CodecError> {
        match self.refusal.get() {
            None => Ok(self.lines),
            Some(Refusal::NonFiniteReal(value)) => Err(CodecError::NotImplemented(format!(
                "STEP writer computed the non-finite real {value}, which Part 21 cannot state"
            ))),
            Some(Refusal::NonSimilarOperator(transform)) => {
                Err(CodecError::NotImplemented(format!(
                    "STEP writer cannot state the transform with rows {:?} as a \
                     CARTESIAN_TRANSFORMATION_OPERATOR_3D, which states a similarity",
                    transform.rows()
                )))
            }
        }
    }
}

/// Format a finite `f64` as a Part 21 real literal, or `None` for a
/// non-finite one.
///
/// The result always contains a decimal point, including scientific notation.
fn part21_real(v: f64) -> Option<String> {
    if !v.is_finite() {
        return None;
    }
    // Shortest round-tripping decimal, then normalize to Part 21 lexical rules.
    let mut s = format!("{v}");
    if let Some(e_pos) = s.find(['e', 'E']) {
        // Scientific: ensure the mantissa has a decimal point and use uppercase E.
        let (mantissa, exp) = s.split_at(e_pos);
        let exp = &exp[1..];
        let mantissa = if mantissa.contains('.') {
            mantissa.to_string()
        } else {
            format!("{mantissa}.")
        };
        let exp = if let Some(rest) = exp.strip_prefix('-') {
            format!("-{rest}")
        } else {
            exp.strip_prefix('+').unwrap_or(exp).to_string()
        };
        return Some(format!("{mantissa}E{exp}"));
    }
    if !s.contains('.') {
        s.push('.');
    }
    Some(s)
}

/// Encode a Rust string as a Part 21 single-quoted string literal.
///
/// Apostrophes are doubled. Non-ASCII and control characters use
/// `\X2\..\X0\` or `\X4\..\X0\` hexadecimal notation, keeping the encoded file 7-bit.
pub(crate) fn string(s: &str) -> String {
    format!("'{}'", crate::strings::encode(s))
}

/// Join instance references into a Part 21 aggregate such as `(#1,#2,#3)`.
pub(crate) fn refs(items: &[Ref]) -> String {
    let mut out = String::from("(");
    for (i, r) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('#');
        out.push_str(&r.0.to_string());
    }
    out.push(')');
    out
}

#[cfg(test)]
pub(crate) mod tests;
