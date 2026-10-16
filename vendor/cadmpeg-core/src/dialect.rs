// SPDX-License-Identifier: Apache-2.0
//! Dialect identification: what a document is, as read from its own bytes.
//!
//! A dialect is a named region of one format's document space, bounded only by
//! discriminants the format itself declares. It exists whether or not cadmpeg
//! reads it. [`DialectMatch`] is the record of one run: inspect or decode
//! handled the file, and [`Admission`] states how.
//!
//! This vocabulary is about the source document. cadmpeg's own version
//! stamp, `IR_VERSION`, is data about cadmpeg, never merged with this one and
//! never sharing a type with it.
//!
//! # `DialectId` is opaque here
//!
//! [`DialectId`] is a namespaced pinned string at this layer and nothing more:
//! no shared enum of every format's dialects, no version parsing, no ordering.
//! An IR-level enum of dialects would make the shared layer depend on every
//! codec, and orderings are codec-local where they are real at all. Outside its
//! owning codec the id is comparable and printable; only the owner parses it.
//!
//! Producers use [`dialect_id!`](crate::dialect_id) for checked static ids. Wire readers use
//! [`DialectId::parse`] for checked owned ids. A codec backs its dialects with
//! its own enum and returns registry-generated pinned constants from it — the
//! `*LossCode` template: enum inside, pinned string at the boundary, one
//! construction path, closed vocabulary. Deserialization is the only other way
//! an id comes into being, and it reconstructs an id that some producer pinned.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use crate::decode::DecodeContext;
use crate::CodecError;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::text::NonBlankString;

/// A registry dialect id, for example `"rhino:archive-80"`.
///
/// Canonical form is `<format>:<name>`. The format contains lowercase ASCII
/// letters and digits. The name also admits dots and hyphens, but a hyphen
/// cannot be first or last. Outside the owning codec the validated id is an
/// opaque label.
///
/// Serializes and deserializes as the plain string. Read it with
/// [`DialectId::as_str`] or print it; there is no other access to the raw
/// string.
#[derive(Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "schema", derive(JsonSchema), schemars(with = "String"))]
pub struct DialectId {
    value: Cow<'static, str>,
    /// The separator found by admission. Both parts are non-empty ASCII.
    namespace_len: usize,
}

/// A dialect id admitted from static storage.
///
/// This copyable proof lets a const initializer reject invalid text before
/// constructing the borrowed-or-owned [`DialectId`].
#[derive(Debug, Clone, Copy)]
pub struct StaticDialectId {
    value: &'static str,
    namespace_len: usize,
}

impl StaticDialectId {
    /// Admit a static id, returning `None` for noncanonical text.
    #[must_use]
    pub const fn new(id: &'static str) -> Option<Self> {
        match dialect_separator(id) {
            Some(namespace_len) => Some(Self {
                value: id,
                namespace_len,
            }),
            None => None,
        }
    }
}

impl DialectId {
    /// Construct an id from admitted static text.
    #[must_use]
    pub const fn from_static(id: StaticDialectId) -> Self {
        Self {
            value: Cow::Borrowed(id.value),
            namespace_len: id.namespace_len,
        }
    }

    /// Parses and validates an owned dialect id.
    pub fn parse(id: impl Into<String>) -> Result<Self, DialectIdError> {
        let id = id.into();
        match dialect_separator(&id) {
            Some(namespace_len) => Ok(Self {
                value: Cow::Owned(id),
                namespace_len,
            }),
            None => Err(DialectIdError(id)),
        }
    }

    /// Returns the id as a string slice.
    #[must_use]
    pub const fn as_str(&self) -> &str {
        match &self.value {
            Cow::Borrowed(id) => id,
            Cow::Owned(id) => id.as_str(),
        }
    }

    /// Returns the format namespace before the id's validated separator.
    #[must_use]
    pub fn namespace(&self) -> &str {
        self.parts().0
    }

    pub(crate) fn local(&self) -> &str {
        self.parts().1
    }

    fn parts(&self) -> (&str, &str) {
        let (namespace, qualified_name) = self.as_str().split_at(self.namespace_len);
        (namespace, &qualified_name[1..])
    }
}

/// Construct a static dialect id whose grammar is checked during compilation.
///
/// ```compile_fail
/// let _ = cadmpeg_core::dialect_id!("rhino:");
/// ```
#[macro_export]
macro_rules! dialect_id {
    ($literal:literal $(,)?) => {{
        const STATIC_DIALECT_ID: $crate::dialect::StaticDialectId =
            match $crate::dialect::StaticDialectId::new($literal) {
                Some(id) => id,
                None => panic!("invalid pinned dialect id"),
            };
        $crate::dialect::DialectId::from_static(STATIC_DIALECT_ID)
    }};
}

const fn dialect_separator(id: &str) -> Option<usize> {
    let bytes = id.as_bytes();
    let mut index = 0;
    let mut colon = None;
    while index < bytes.len() {
        if bytes[index] == b':' {
            if colon.is_some() || index == 0 || index + 1 == bytes.len() {
                return None;
            }
            colon = Some(index);
        }
        index += 1;
    }
    let Some(colon) = colon else {
        return None;
    };
    index = 0;
    while index < colon {
        let byte = bytes[index];
        if !byte.is_ascii_lowercase() && !byte.is_ascii_digit() {
            return None;
        }
        index += 1;
    }
    if valid_grammar_bytes(bytes, colon + 1) {
        Some(colon)
    } else {
        None
    }
}

/// States whether `bytes[start..]` is a format-local name.
///
/// The name is not empty, holds lowercase ASCII letters, digits, dots and
/// hyphens only, and neither starts nor ends with a hyphen. A colon is outside
/// the class, so a full `<format>:<name>` id is not a name.
const fn valid_grammar_bytes(bytes: &[u8], start: usize) -> bool {
    if start >= bytes.len() {
        return false;
    }
    let mut index = start;
    while index < bytes.len() {
        let byte = bytes[index];
        if !byte.is_ascii_lowercase() && !byte.is_ascii_digit() && byte != b'-' && byte != b'.' {
            return false;
        }
        index += 1;
    }
    bytes[start] != b'-' && bytes[bytes.len() - 1] != b'-'
}

/// A string is not a canonical dialect id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectIdError(String);

impl fmt::Display for DialectIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid dialect id {:?}: expected <format>:<name> in lowercase canonical form",
            self.0
        )
    }
}

impl Error for DialectIdError {}

impl fmt::Display for DialectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for DialectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DialectId({:?})", self.as_str())
    }
}

impl Serialize for DialectId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DialectId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// How one format layer admitted, or refused, one document.
///
/// Identity and admission are orthogonal: a document can carry a registry row
/// of its own while its bytes are read with another grammar, and a damaged
/// frame can retain its identity while applying that row's grammar
/// unverified. The grammar name is local to the owning [`DialectMatch`]'s
/// format namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Admission {
    /// Parsed with the strategy declared for the identified dialect.
    Admitted,
    /// Parsed with another declared grammar of the same format namespace.
    ///
    /// The codec must charge its dialect-unverified loss.
    Unverified {
        /// Format-local name of the grammar the parser applied.
        using: Grammar,
    },
    /// Parsed without any declared grammar: the residual path.
    ///
    /// The codec must charge its dialect-unverified loss.
    Residual,
    /// Structurally identified; semantic decode refused.
    Refused,
}

/// Format-local name of a dialect grammar, for example `sch-sw-33103`.
///
/// The namespace is the owning [`DialectMatch`]'s, so a grammar cannot name a
/// foreign format by construction. The name holds the character class a
/// [`DialectId`] admits after its separator, which excludes the separator
/// itself: a full id has no spelling as a grammar name.
///
/// Serializes and deserializes as the plain string. `try_from` is the mint, so
/// no read path reaches the field without the check. serde refuses
/// `transparent` beside `try_from`; a newtype struct writes its inner string
/// with no wrapper of its own, which is the same wire form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "String")]
pub struct Grammar(String);

impl Grammar {
    /// Names the format-local half of a registry dialect id.
    #[must_use]
    pub fn of(dialect: &DialectId) -> Self {
        Self(dialect.local().to_owned())
    }

    /// Parses and validates a format-local grammar name.
    ///
    /// The only read path into the type, through its `TryFrom<String>`.
    pub fn parse(name: impl Into<String>) -> Result<Self, GrammarError> {
        let name = name.into();
        if valid_grammar_bytes(name.as_bytes(), 0) {
            Ok(Self(name))
        } else {
            Err(GrammarError(name))
        }
    }

    /// Returns the format-local grammar name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Grammar {
    type Error = GrammarError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        Self::parse(name)
    }
}

/// A string is not a format-local grammar name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarError(String);

impl fmt::Display for GrammarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid grammar name {:?}: expected a format-local name in lowercase canonical form",
            self.0
        )
    }
}

impl Error for GrammarError {}

/// One format layer's identification of one document.
///
/// A document carries several format layers: `.sldprt` contains Parasolid;
/// `.f3d`, `.ipt`, and SAT contain ACIS; NX contains Parasolid and JT. One
/// match describes one layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DialectMatch {
    /// Registry dialect id, for example `"rhino:archive-80"`.
    dialect: DialectId,
    /// Version fields the source declared, verbatim, under keys pinned per
    /// codec in the registry.
    ///
    /// Declarations are evidence, never a control input: the dialect is what
    /// the bytes obey, not what they declare.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[serde(deserialize_with = "crate::distinct_keys::btree_map")]
    declared: BTreeMap<NonBlankString, String>,
    /// Instance of this format layer inside the containing document.
    ///
    /// `None` when the layer occurs once or has no report-local identity. This
    /// is not source-declared evidence and therefore does not belong in
    /// [`Self::declared`].
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_instance"
    )]
    instance: Option<String>,
    /// How this layer was admitted.
    admission: Admission,
}

/// Whether a format layer is the sole instance or needs its carrier identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerInstance {
    /// The host contains one layer of this format.
    Sole,
    /// The host contains several layers and the carrier disambiguates this one.
    Tagged,
}

/// A report's primary format layer and any nested or carried format layers.
///
/// Extra layers are unique by `(format, instance)`. An extra layer for the
/// primary format has an instance that identifies it inside the containing
/// document. The wire names the primary explicitly, so the collection carries
/// its complete identity without an enclosing report's format.
///
/// A read rejects a wire whose `extra` repeats a `(format, instance)` key, and
/// one whose `extra` carries the primary key. The key set on the wire is the
/// key set of the value, so no layer is silently discarded or overwritten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DialectLayers {
    primary: DialectMatch,
    extra: Vec<DialectMatch>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DialectLayersWire {
    primary: DialectMatch,
    extra: Vec<DialectMatch>,
}

impl<'de> Deserialize<'de> for DialectLayers {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = DialectLayersWire::deserialize(deserializer)?;
        let mut layers = Self::of(wire.primary);
        for layer in wire.extra {
            layers.insert(layer).map_err(|rejected| {
                <D::Error as serde::de::Error>::custom(format!(
                    "duplicate dialect layer key: {rejected:?}"
                ))
            })?;
        }
        Ok(layers)
    }
}

/// Failure to insert a unique dialect layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialectLayerError {
    /// The existing layer has the same format and instance.
    Duplicate(DialectMatch),
    /// The decode policy or allocator refused storage or work.
    ResourceLimit(crate::decode::ResourceLimit),
}

impl fmt::Display for DialectLayerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Duplicate(layer) => write!(formatter, "duplicate dialect layer key: {layer:?}"),
            Self::ResourceLimit(limit) => {
                write!(formatter, "resource refusal during {}", limit.operation)
            }
        }
    }
}

impl Error for DialectLayerError {}

impl From<DialectLayerError> for CodecError {
    fn from(error: DialectLayerError) -> Self {
        match error {
            DialectLayerError::Duplicate(layer) => {
                Self::malformed(format_args!("duplicate dialect layer key: {layer:?}"))
            }
            DialectLayerError::ResourceLimit(limit) => Self::ResourceLimit(limit),
        }
    }
}

impl DialectLayers {
    /// Copy dialect layers and their owned storage through the decode budget.
    pub fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let primary = self.primary.try_clone_for_decode(ctx, operation)?;
        let mut extra = ctx.collection_vec(self.extra.len(), operation)?;
        ctx.charge_work(crate::decode::u64_from_index(self.extra.len()), operation)?;
        for layer in &self.extra {
            extra.push(layer.try_clone_for_decode(ctx, operation)?);
        }
        Ok(Self { primary, extra })
    }

    /// Constructs dialect layers with one primary and no extra layers.
    #[must_use]
    pub fn of(primary: DialectMatch) -> Self {
        Self {
            primary,
            extra: Vec::new(),
        }
    }

    /// Inserts a layer keyed by `(format, instance)`.
    ///
    /// The first layer for a key remains authoritative. A duplicate is
    /// returned unchanged so its producer can report the collision.
    pub fn insert(&mut self, layer: DialectMatch) -> Result<(), DialectLayerError> {
        let arena = crate::decode::DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes_limit(
            &[],
            &arena,
            &crate::decode::DecodePolicy::default(),
        )
        .map_err(DialectLayerError::ResourceLimit)?;
        self.insert_for_decode(&ctx, layer, "dialect layer insertion")
    }

    /// Insert one unique layer after admitting lookup work and retained storage.
    pub fn insert_for_decode(
        &mut self,
        ctx: &DecodeContext<'_>,
        layer: DialectMatch,
        operation: &'static str,
    ) -> Result<(), DialectLayerError> {
        ctx.charge_work_limit(
            crate::decode::u64_from_index(self.extra.len()) + 1,
            operation,
        )
        .map_err(DialectLayerError::ResourceLimit)?;
        if Self::same_key(&self.primary, &layer)
            || self
                .extra
                .iter()
                .any(|existing| Self::same_key(existing, &layer))
        {
            return Err(DialectLayerError::Duplicate(layer));
        }
        ctx.reserve_vec_limit(&mut self.extra, 1, operation)
            .map_err(DialectLayerError::ResourceLimit)?;
        self.extra.push(layer);
        Ok(())
    }

    /// Adds a layer, returning it unchanged when its key is occupied.
    pub fn with(mut self, layer: DialectMatch) -> Result<Self, DialectLayerError> {
        self.insert(layer)?;
        Ok(self)
    }

    /// Adds a layer through the caller's decode budget.
    pub fn with_for_decode(
        mut self,
        ctx: &DecodeContext<'_>,
        layer: DialectMatch,
        operation: &'static str,
    ) -> Result<Self, DialectLayerError> {
        self.insert_for_decode(ctx, layer, operation)?;
        Ok(self)
    }

    fn same_key(existing: &DialectMatch, layer: &DialectMatch) -> bool {
        existing.format() == layer.format() && existing.instance == layer.instance
    }

    /// Returns the report's primary format layer.
    #[must_use]
    pub fn primary(&self) -> &DialectMatch {
        &self.primary
    }

    /// Iterates over the primary layer followed by every extra layer.
    pub fn iter(&self) -> impl Iterator<Item = &DialectMatch> {
        std::iter::once(&self.primary).chain(&self.extra)
    }
}

mod format_identity_sealed {
    pub trait Sealed {}
}

/// A classified payload whose format namespace is intrinsic to the payload.
///
/// This trait is sealed. The supported payloads are [`DialectId`],
/// [`DialectMatch`], and [`DialectLayers`].
pub trait FormatIdentityPayload: format_identity_sealed::Sealed {
    /// Returns the payload's authoritative format namespace.
    fn format(&self) -> &str;
}

impl format_identity_sealed::Sealed for DialectId {}

impl FormatIdentityPayload for DialectId {
    fn format(&self) -> &str {
        self.namespace()
    }
}

impl format_identity_sealed::Sealed for DialectMatch {}

impl FormatIdentityPayload for DialectMatch {
    fn format(&self) -> &str {
        self.format()
    }
}

impl format_identity_sealed::Sealed for DialectLayers {}

impl FormatIdentityPayload for DialectLayers {
    fn format(&self) -> &str {
        self.primary().format()
    }
}

/// A format identity that is either classified by a typed payload or retains
/// only its known format.
///
/// Classified state stores no second format string. The payload is the one
/// author of its namespace, so the format is on the wire exactly once in
/// either state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "classification", rename_all = "snake_case", deny_unknown_fields)]
pub enum FormatIdentity<T> {
    /// The payload carries the complete classified identity.
    Classified {
        /// Classified payload, whose namespace is the format.
        dialects: T,
    },
    /// The format is known but no classification payload exists.
    Unclassified {
        /// Registry format namespace.
        format: String,
    },
}

impl<T: FormatIdentityPayload> FormatIdentity<T> {
    /// Constructs an identity whose format comes from its classified payload.
    #[must_use]
    pub const fn classified(payload: T) -> Self {
        Self::Classified { dialects: payload }
    }

    /// Constructs an identity for a known format without classification.
    #[must_use]
    pub fn unclassified(format: impl Into<String>) -> Self {
        Self::Unclassified {
            format: format.into(),
        }
    }

    /// Returns the authoritative format id.
    #[must_use]
    pub fn format(&self) -> &str {
        match self {
            Self::Classified { dialects } => dialects.format(),
            Self::Unclassified { format } => format,
        }
    }

    /// Returns the classified payload, when present.
    #[must_use]
    pub const fn classified_payload(&self) -> Option<&T> {
        match self {
            Self::Classified { dialects } => Some(dialects),
            Self::Unclassified { .. } => None,
        }
    }
}

impl DialectMatch {
    /// Copy a dialect layer into a decoded report under the caller's resource limits.
    pub fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, crate::CodecError> {
        let dialect = DialectId {
            value: match &self.dialect.value {
                Cow::Borrowed(value) => Cow::Borrowed(value),
                Cow::Owned(value) => Cow::Owned(ctx.copy_retained_text(value, operation)?),
            },
            namespace_len: self.dialect.namespace_len,
        };
        let mut declared = BTreeMap::new();
        for (key, value) in &self.declared {
            ctx.admit_retained_btree_record::<NonBlankString, String>(0, operation)?;
            let key = NonBlankString::new(ctx.copy_retained_text(key.as_str(), operation)?)
                .ok_or_else(|| crate::CodecError::malformed("dialect declaration key is blank"))?;
            declared.insert(key, ctx.copy_retained_text(value, operation)?);
        }
        let instance = self
            .instance
            .as_ref()
            .map(|value| ctx.copy_retained_text(value, operation))
            .transpose()?;
        let admission = match &self.admission {
            Admission::Admitted => Admission::Admitted,
            Admission::Residual => Admission::Residual,
            Admission::Refused => Admission::Refused,
            Admission::Unverified { using } => Admission::Unverified {
                using: Grammar(ctx.copy_retained_text(using.as_str(), operation)?),
            },
        };
        Ok(Self {
            dialect,
            declared,
            instance,
            admission,
        })
    }

    fn with_admission(dialect: DialectId, admission: Admission) -> Self {
        Self {
            dialect,
            declared: BTreeMap::new(),
            instance: None,
            admission,
        }
    }

    /// Constructs a layer parsed with its identified dialect's grammar.
    #[must_use]
    pub fn admitted(dialect: DialectId) -> Self {
        Self::with_admission(dialect, Admission::Admitted)
    }

    /// Constructs a layer parsed unverified with another declared grammar.
    ///
    /// `using` supplies its format-local name; the grammar's namespace is the
    /// layer's own.
    #[must_use]
    pub fn unverified(dialect: DialectId, using: Grammar) -> Self {
        Self::with_admission(dialect, Admission::Unverified { using })
    }

    /// Constructs a layer parsed on the residual path without a declared grammar.
    #[must_use]
    pub fn residual(dialect: DialectId) -> Self {
        Self::with_admission(dialect, Admission::Residual)
    }

    /// Constructs a structurally identified layer whose decode was refused.
    #[must_use]
    pub fn refused(dialect: DialectId) -> Self {
        Self::with_admission(dialect, Admission::Refused)
    }

    /// Attaches source-declared version fields before the match enters a report.
    #[must_use]
    pub fn with_declared(mut self, declared: BTreeMap<NonBlankString, String>) -> Self {
        self.declared = declared;
        self
    }

    /// Adds one declaration after charging its map slot and owned value.
    pub fn with_declared_entry(
        mut self,
        ctx: &crate::decode::DecodeContext<'_>,
        key: NonBlankString,
        value: &str,
        operation: &'static str,
    ) -> Result<Self, crate::CodecError> {
        if !self.declared.contains_key(&key) {
            ctx.admit_retained_btree_record::<NonBlankString, String>(0, operation)?;
        }
        let value = ctx.copy_retained_text(value, operation)?;
        let previous = self.declared.insert(key, value);
        drop(previous);
        Ok(self)
    }

    /// Attaches a report-local layer instance before the match enters a report.
    #[must_use]
    pub fn with_instance(mut self, instance: impl Into<String>) -> Self {
        self.instance = Some(instance.into());
        self
    }

    /// Returns the classified format layer.
    #[must_use]
    pub fn format(&self) -> &str {
        self.dialect.namespace()
    }

    /// Returns the registry dialect identity.
    #[must_use]
    pub fn dialect(&self) -> &DialectId {
        &self.dialect
    }

    /// Returns the source-declared version fields.
    #[must_use]
    pub fn declared(&self) -> &BTreeMap<NonBlankString, String> {
        &self.declared
    }

    /// Returns the report-local layer instance.
    #[must_use]
    pub fn instance(&self) -> Option<&str> {
        self.instance.as_deref()
    }

    /// Returns how this layer was admitted.
    #[must_use]
    pub fn admission(&self) -> &Admission {
        &self.admission
    }

    /// Returns the full registry id of the grammar applied, when this layer
    /// was parsed unverified with a declared grammar.
    #[must_use]
    pub fn using(&self) -> Option<DialectId> {
        match &self.admission {
            Admission::Unverified { using } => Some(self.grammar_id(using)),
            Admission::Admitted | Admission::Residual | Admission::Refused => None,
        }
    }

    /// Return the full grammar identity in this match’s format namespace.
    #[must_use]
    pub fn grammar_id(&self, grammar: &Grammar) -> DialectId {
        let namespace = self.format();
        DialectId {
            value: Cow::Owned(format!("{namespace}:{}", grammar.as_str())),
            namespace_len: namespace.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::collections::BTreeMap;

    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use crate::CodecError;

    use super::{Admission, DialectId, DialectLayers, DialectMatch, Grammar, StaticDialectId};

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct DialectIdConformance {
        valid: Vec<String>,
        invalid: Vec<String>,
    }

    fn layer(format: &str) -> DialectMatch {
        DialectMatch::admitted(DialectId::parse(format!("{format}:known")).unwrap())
    }

    #[test]
    fn charged_layer_copy_preserves_declarations_and_refuses_retained_limit() {
        let primary = DialectMatch::admitted(crate::dialect_id!("sldprt:unknown")).with_declared(
            std::collections::BTreeMap::from([(
                crate::nonblank_literal!("sw_version"),
                "12000".to_string(),
            )]),
        );
        let extra =
            DialectMatch::residual(crate::dialect_id!("parasolid:unknown")).with_instance("body-1");
        let layers = DialectLayers::of(primary).with(extra).unwrap();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert_eq!(
            layers
                .try_clone_for_decode(&ctx, "copy dialect text")
                .unwrap(),
            layers
        );

        let limited_arena = DecodeArena::new();
        let mut limited = DecodePolicy::service();
        limited.limits.max_retained_bytes = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &limited_arena, &limited).unwrap();
        let error = layers
            .try_clone_for_decode(&ctx, "copy dialect text")
            .unwrap_err();
        assert!(matches!(
            error,
            crate::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "copy dialect text"
        ));
    }

    #[test]
    fn copied_dialect_layers_refuse_owned_identity_at_retained_limit() {
        let layers = DialectLayers::of(layer("nx"));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();

        assert!(matches!(
            layers.try_clone_for_decode(&ctx, "dialect identity copy"),
            Err(crate::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "dialect identity copy"
        ));
    }

    #[test]
    fn copied_dialect_layers_refuse_extra_layer_at_collection_limit() {
        let mut layers = DialectLayers::of(layer("nx"));
        layers.insert(layer("step")).unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();

        assert!(matches!(
            layers.try_clone_for_decode(&ctx, "dialect layer copies"),
            Err(crate::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "dialect layer copies"
        ));
    }

    #[test]
    fn dialect_extra_copy_admits_retained_vector_storage() {
        let layers = DialectLayers::of(DialectMatch::admitted(crate::dialect_id!("nx:unknown")))
            .with(DialectMatch::admitted(crate::dialect_id!("step:unknown")))
            .unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes =
            crate::decode::u64_from_index(std::mem::size_of::<DialectMatch>()) - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            matches!(layers.try_clone_for_decode(&ctx, "copy extra storage"),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "copy extra storage")
        );
    }

    #[test]
    fn dialect_insertion_refuses_storage_without_changing_layers() {
        let primary = DialectMatch::admitted(crate::dialect_id!("nx:unknown"));
        let mut layers = DialectLayers::of(primary);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes =
            crate::decode::u64_from_index(std::mem::size_of::<DialectMatch>()) - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(layers.insert_for_decode(&ctx,
            DialectMatch::admitted(crate::dialect_id!("step:unknown")), "insert layer"),
            Err(super::DialectLayerError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes));
        assert_eq!(layers.iter().count(), 1);
    }

    #[test]
    fn dialect_declaration_copy_admits_map_node_storage() {
        let layer = DialectMatch::admitted(crate::dialect_id!("nx:unknown")).with_declared(
            std::collections::BTreeMap::from([(
                crate::nonblank_literal!("version"),
                String::new(),
            )]),
        );
        let node_bytes = 11
            * (std::mem::size_of::<crate::text::NonBlankString>() + std::mem::size_of::<String>())
            + 16 * std::mem::size_of::<usize>()
            + 2 * std::mem::align_of::<String>().max(std::mem::align_of::<usize>());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // One declaration record admits one backing node before its text copies.
        let bytes = node_bytes;
        policy.limits.max_retained_bytes = crate::decode::u64_from_index(bytes) - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            matches!(layer.try_clone_for_decode(&ctx, "copy declaration"),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes
                && limit.additional == crate::decode::u64_from_index(bytes))
        );
    }

    #[test]
    fn a_pinned_id_prints_and_serializes_as_the_plain_string() {
        let id = crate::dialect_id!("rhino:archive-80");

        assert_eq!(id.to_string(), "rhino:archive-80");
        assert_eq!(id.as_str(), "rhino:archive-80");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"rhino:archive-80\"");
        assert_eq!(
            serde_json::from_str::<DialectId>("\"rhino:archive-80\"").unwrap(),
            id
        );
    }

    #[test]
    fn static_dialect_admission_returns_none_for_invalid_runtime_text() {
        for text in [
            "",
            "rhino",
            "rhino:",
            ":known",
            "Rhino:known",
            "a:b:c",
            "a:é",
        ] {
            assert!(StaticDialectId::new(text).is_none(), "{text:?}");
        }
        let admitted = DialectId::from_static(StaticDialectId::new("rhino:archive-80").unwrap());
        assert_eq!(admitted.namespace(), "rhino");
        assert_eq!(admitted.local(), "archive-80");
        assert_eq!(admitted, DialectId::parse("rhino:archive-80").unwrap());
    }

    #[test]
    fn dialect_id_deserialization_rejects_noncanonical_strings() {
        for id in [
            "rhino",
            ":archive-80",
            "rhino:",
            "Rhino:archive-80",
            "rhino:archive_80",
            "rhino:-archive-80",
        ] {
            serde_json::from_value::<DialectId>(serde_json::json!(id))
                .expect_err("a malformed dialect id must be rejected");
        }
    }

    #[test]
    fn dialect_id_matches_the_shared_conformance_corpus() {
        let cases: DialectIdConformance =
            toml::from_str(include_str!("../../../docs/dialect-id-conformance.toml")).unwrap();
        for id in cases.valid {
            let admitted =
                DialectId::parse(id.clone()).unwrap_or_else(|_| panic!("valid dialect id {id:?}"));
            assert_eq!(format!("{}:{}", admitted.namespace(), admitted.local()), id);
        }
        for id in cases.invalid {
            assert!(
                DialectId::parse(id.clone()).is_err(),
                "invalid dialect id {id:?}"
            );
        }
    }

    #[test]
    fn an_admitted_match_serializes_its_identity() {
        let admitted = DialectMatch {
            dialect: crate::dialect_id!("rhino:archive-80"),
            declared: BTreeMap::new(),
            instance: None,
            admission: Admission::Admitted,
        };

        assert_eq!(
            serde_json::to_string(&admitted).unwrap(),
            "{\"dialect\":\"rhino:archive-80\",\"admission\":\"admitted\"}"
        );
    }

    #[test]
    fn a_dialect_match_wire_states_its_format_once_through_its_dialect() {
        let restated = serde_json::json!({
            "format": "rhino",
            "dialect": "rhino:archive-80",
            "admission": "admitted",
        });

        let error = serde_json::from_value::<DialectMatch>(restated)
            .expect_err("the format is read from the dialect, so the wire carries no format key");
        assert!(
            error.to_string().contains("unknown field `format`"),
            "{error}"
        );
        assert_eq!(
            DialectMatch::admitted(crate::dialect_id!("rhino:archive-80")).format(),
            "rhino"
        );
    }

    #[test]
    fn residual_constructor_records_the_absence_of_a_declared_grammar() {
        let residual = DialectMatch::residual(crate::dialect_id!("rhino:unknown"));

        assert_eq!(residual.admission(), &Admission::Residual);
        assert_eq!(residual.using(), None);
        assert_eq!(
            serde_json::to_string(&residual).unwrap(),
            "{\"dialect\":\"rhino:unknown\",\"admission\":\"residual\"}"
        );
    }

    #[test]
    fn an_unverified_admission_names_the_grammar_in_use_by_full_id() {
        let unverified = DialectMatch::unverified(
            crate::dialect_id!("acis:save-format-217"),
            Grammar::of(&crate::dialect_id!("acis:save-format-218")),
        );

        assert_eq!(
            unverified.admission(),
            &Admission::Unverified {
                using: Grammar::of(&crate::dialect_id!("acis:save-format-218")),
            }
        );
        assert_eq!(
            unverified.using(),
            Some(crate::dialect_id!("acis:save-format-218"))
        );
        let serialized = serde_json::to_string(&unverified).unwrap();
        assert_eq!(
            serialized,
            "{\"dialect\":\"acis:save-format-217\",\"admission\":{\"unverified\":{\"using\":\"save-format-218\"}}}"
        );
        assert_eq!(
            serde_json::from_str::<DialectMatch>(&serialized).unwrap(),
            unverified
        );
    }

    #[test]
    fn a_self_named_unverified_grammar_remains_opaque() {
        let self_named = serde_json::json!({
            "dialect": "rhino:unknown",
            "admission": {
                "unverified": { "using": "unknown" }
            },
        });
        let matched = serde_json::from_value::<DialectMatch>(self_named)
            .expect("core does not infer grammar semantics from an opaque dialect name");
        assert_eq!(
            matched.admission(),
            &Admission::Unverified {
                using: Grammar::of(&crate::dialect_id!("rhino:unknown")),
            }
        );
    }

    #[test]
    fn a_grammar_name_has_no_spelling_for_a_foreign_namespace() {
        let malformed = serde_json::json!({
            "dialect": "rhino:unknown",
            "admission": {
                "unverified": { "using": "step:ap242-e3" }
            },
        });
        let error = serde_json::from_value::<DialectMatch>(malformed)
            .expect_err("a grammar substitute belongs to the classified format layer");
        assert!(
            error
                .to_string()
                .contains("invalid grammar name \"step:ap242-e3\""),
            "{error}"
        );
    }

    #[test]
    fn grammar_parsing_rejects_a_name_outside_the_format_local_class() {
        for name in [
            "",
            "rhino:archive-80",
            "Archive-80",
            "archive_80",
            "-archive",
        ] {
            assert!(
                Grammar::parse(name).is_err(),
                "invalid grammar name {name:?}"
            );
        }
        assert_eq!(
            Grammar::parse("save-format-218").unwrap(),
            Grammar::of(&crate::dialect_id!("acis:save-format-218"))
        );
    }

    #[test]
    fn identity_does_not_encode_whether_an_unverified_path_used_a_grammar() {
        let dialect = crate::dialect_id!("rhino:archive-80");
        let without_grammar = DialectMatch::residual(dialect.clone());
        assert_eq!(without_grammar.admission(), &Admission::Residual);

        let self_named = DialectMatch::unverified(dialect.clone(), Grammar::of(&dialect));
        assert_eq!(
            self_named.admission(),
            &Admission::Unverified {
                using: Grammar::of(&crate::dialect_id!("rhino:archive-80")),
            }
        );
        assert_eq!(self_named.using(), Some(dialect));
    }

    #[test]
    fn dialect_layers_accept_a_same_format_extra_with_an_instance() {
        let member = layer("rhino").with_instance("components/member.3dm");
        let layers = DialectLayers::of(layer("rhino"))
            .with(member.clone())
            .expect("distinct dialect layer keys");

        let serialized = serde_json::to_value(&layers).unwrap();
        assert_eq!(
            serde_json::from_value::<DialectLayers>(serialized).unwrap(),
            layers
        );
        assert_eq!(layers.extra, [member]);
    }

    #[test]
    fn dialect_layers_insert_keeps_the_first_extra_layer_for_a_key() {
        let first = layer("acis").with_instance("body");
        let replacement =
            DialectMatch::residual(crate::dialect_id!("acis:other")).with_instance("body");
        let mut layers = DialectLayers::of(layer("rhino"))
            .with(first.clone())
            .expect("distinct dialect layer keys");

        assert_eq!(
            layers.insert(replacement.clone()),
            Err(super::DialectLayerError::Duplicate(replacement))
        );
        assert_eq!(layers.extra, [first]);
    }

    #[test]
    fn dialect_layers_insert_keeps_the_primary_on_its_own_key() {
        let replacement = DialectMatch::residual(crate::dialect_id!("rhino:other"));
        let mut layers = DialectLayers::of(layer("rhino"))
            .with(layer("acis"))
            .expect("distinct dialect layer keys");

        assert_eq!(
            layers.insert(replacement.clone()),
            Err(super::DialectLayerError::Duplicate(replacement))
        );
        assert_eq!(layers.primary(), &layer("rhino"));
        assert_eq!(layers.extra, [layer("acis")]);
    }

    #[test]
    fn dialect_layers_builder_returns_the_colliding_layer() {
        let first = layer("acis").with_instance("body");
        let replacement =
            DialectMatch::residual(crate::dialect_id!("acis:other")).with_instance("body");
        let layers = DialectLayers::of(layer("rhino"))
            .with(first)
            .expect("distinct dialect layer keys");
        assert_eq!(
            layers.with(replacement.clone()),
            Err(super::DialectLayerError::Duplicate(replacement))
        );
    }

    #[test]
    fn dialect_layers_keep_same_format_extras_with_distinct_instances() {
        let anonymous = layer("acis");
        let named = layer("acis").with_instance("body");
        let mut layers = DialectLayers::of(layer("rhino"));

        assert_eq!(layers.insert(anonymous.clone()), Ok(()));
        assert_eq!(layers.insert(named.clone()), Ok(()));
        assert_eq!(layers.extra, [anonymous, named]);
    }

    #[test]
    fn dialect_layers_deserialization_rejects_a_repeated_key() {
        let serialized = serde_json::json!({
            "primary": layer("rhino"),
            "extra": [layer("acis"), DialectMatch::residual(crate::dialect_id!("acis:other"))],
        });

        let error = serde_json::from_value::<DialectLayers>(serialized)
            .expect_err("a repeated extra layer key is a read error");
        assert!(
            error.to_string().contains("duplicate dialect layer key"),
            "{error}"
        );
    }

    #[test]
    fn dialect_layers_deserialization_rejects_an_extra_on_the_primary_key() {
        let serialized = serde_json::json!({
            "primary": layer("rhino"),
            "extra": [DialectMatch::residual(crate::dialect_id!("rhino:other"))],
        });

        let error = serde_json::from_value::<DialectLayers>(serialized)
            .expect_err("an extra layer on the primary key is a read error");
        assert!(
            error.to_string().contains("duplicate dialect layer key"),
            "{error}"
        );
    }

    #[test]
    fn dialect_layers_serialize_with_an_explicit_primary() {
        let layers = DialectLayers::of(layer("rhino"))
            .with(layer("acis"))
            .expect("distinct dialect layer keys");
        let serialized = serde_json::to_value(&layers).unwrap();

        assert_eq!(
            serialized,
            serde_json::json!({
                "primary": layer("rhino"),
                "extra": [layer("acis")],
            })
        );
        assert_eq!(
            serde_json::from_value::<DialectLayers>(serialized).unwrap(),
            layers
        );
        assert_eq!(
            layers.iter().map(DialectMatch::format).collect::<Vec<_>>(),
            ["rhino", "acis"]
        );
    }
}

// Each optional key below names itself in whatever it refuses.
crate::named_optional_field!(deserialize_instance, String, "instance");
