// SPDX-License-Identifier: Apache-2.0
//! Encoder target catalogs and typed target-selection refusals.

use std::fmt;

use crate::dialect::DialectId;
use serde::ser::{SerializeSeq, Serializer};
use serde::Serialize;

/// One dialect that a caller can request from an encoder.
///
/// Synthesis is a static capability. The catalog states ids, aliases, and
/// the cross-format default; input-conditioned preservation remains a write
/// planner decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetDescriptor {
    /// Registry dialect id, e.g. `step:ap242-e3`.
    pub id: DialectId,
    /// Short spellings accepted for `id`, e.g. `["6"]` for `rhino:archive-60`.
    pub aliases: &'static [&'static str],
}

impl TargetDescriptor {
    const fn token_bytes(&self, index: usize) -> &[u8] {
        match index {
            0 => self.id.as_str().as_bytes(),
            1 => {
                let bytes = self.id.as_str().as_bytes();
                let mut start = 0;
                while bytes[start] != b':' {
                    start += 1;
                }
                bytes.split_at(start + 1).1
            }
            _ => self.aliases[index - 2].as_bytes(),
        }
    }

    /// Every token accepted for this target: full id, format-local id, and aliases.
    pub fn accepted_tokens(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.id.as_str())
            .chain(std::iter::once(self.id.local()))
            .chain(self.aliases.iter().copied())
    }
}

/// An encoder's synthesis targets and its optional cross-format default.
///
/// The default is a position in `targets`, validated when the catalog is
/// constructed. A catalog may have no default when the encoder cannot
/// synthesize a document from a source of another format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetCatalog(CatalogEntries);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogEntries {
    Empty(&'static str),
    Rows {
        targets: &'static [TargetDescriptor],
        default: Option<usize>,
    },
}

impl TargetCatalog {
    /// Builds an empty catalog for a dialect-free format.
    #[must_use]
    pub const fn empty(namespace: &'static str) -> Self {
        Self(CatalogEntries::Empty(namespace))
    }

    /// Builds a nonempty catalog with one namespace and an optional default row.
    ///
    /// Panics if rows have different namespaces, a spelling selects multiple
    /// rows, or the default is out of bounds.
    #[must_use]
    pub const fn new(targets: &'static [TargetDescriptor], default: Option<usize>) -> Self {
        let [first, rest @ ..] = targets else {
            panic!("use TargetCatalog::empty for a dialect-free format");
        };
        let first = first.id.as_str().as_bytes();
        let mut row = 0;
        while row < rest.len() {
            let other = rest[row].id.as_str().as_bytes();
            let mut byte = 0;
            while byte < first.len() && byte < other.len() {
                assert!(
                    first[byte] == other[byte],
                    "target catalog rows must share a namespace"
                );
                if first[byte] == b':' {
                    break;
                }
                byte += 1;
            }
            row += 1;
        }
        let mut left = 0;
        while left < targets.len() {
            let mut right = left + 1;
            while right < targets.len() {
                let mut left_token = 0;
                while left_token < targets[left].aliases.len() + 2 {
                    let mut right_token = 0;
                    while right_token < targets[right].aliases.len() + 2 {
                        assert!(
                            !token_bytes_equal(
                                targets[left].token_bytes(left_token),
                                targets[right].token_bytes(right_token),
                            ),
                            "target catalog spellings must select one row"
                        );
                        right_token += 1;
                    }
                    left_token += 1;
                }
                right += 1;
            }
            left += 1;
        }
        if let Some(index) = default {
            assert!(
                index < targets.len(),
                "target catalog default is out of bounds"
            );
        }
        Self(CatalogEntries::Rows { targets, default })
    }

    /// Returns the format namespace owned by this catalog.
    #[must_use]
    pub fn namespace(self) -> &'static str {
        match self.0 {
            CatalogEntries::Empty(namespace) => namespace,
            CatalogEntries::Rows { targets, .. } => targets[0].id.namespace(),
        }
    }

    /// Returns all synthesis targets in catalog order.
    #[must_use]
    pub const fn targets(self) -> &'static [TargetDescriptor] {
        match self.0 {
            CatalogEntries::Empty(_) => &[],
            CatalogEntries::Rows { targets, .. } => targets,
        }
    }

    /// Returns whether this catalog has no synthesis targets.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        matches!(self.0, CatalogEntries::Empty(_))
    }

    /// Returns the number of synthesis targets.
    #[must_use]
    pub const fn len(self) -> usize {
        self.targets().len()
    }

    /// Iterates over synthesis targets in catalog order.
    pub fn iter(self) -> std::slice::Iter<'static, TargetDescriptor> {
        self.targets().iter()
    }

    /// Returns the row named by a full id, format-local id, or alias, with its position.
    #[must_use]
    pub fn find(self, token: &str) -> Option<(usize, &'static TargetDescriptor)> {
        self.iter()
            .enumerate()
            .find(|(_, target)| target.accepted_tokens().any(|accepted| accepted == token))
    }

    /// Returns the cross-format default and its position, when declared.
    #[must_use]
    pub fn default(self) -> Option<(usize, &'static TargetDescriptor)> {
        match self.0 {
            CatalogEntries::Empty(_) => None,
            CatalogEntries::Rows { targets, default } => {
                default.map(|index| (index, &targets[index]))
            }
        }
    }
}

impl IntoIterator for TargetCatalog {
    type Item = &'static TargetDescriptor;
    type IntoIter = std::slice::Iter<'static, TargetDescriptor>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl Serialize for TargetCatalog {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct TargetDescriptorWire<'a> {
            id: &'a DialectId,
            aliases: &'static [&'static str],
            default: bool,
        }

        let mut sequence = serializer.serialize_seq(Some(self.len()))?;
        for (index, target) in self.iter().enumerate() {
            sequence.serialize_element(&TargetDescriptorWire {
                id: &target.id,
                aliases: target.aliases,
                default: self.default().is_some_and(|(default, _)| default == index),
            })?;
        }
        sequence.end()
    }
}

const fn token_bytes_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// A caller-supplied dialect token requested from an encoder.
///
/// Unlike [`DialectId`], this token need not name a registered dialect:
/// refusals preserve an unknown explicit request verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct TargetToken(String);

impl TargetToken {
    /// Retains one requested target token verbatim.
    #[must_use]
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    /// Returns the requested token.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TargetToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One target-refusal reason, independent of the catalog presented with it.
///
/// Each request state is distinct. In particular, absence of an explicit
/// token does not conflate an unclassified same-format source with a foreign
/// source and a missing cross-format default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum TargetRefusalKind {
    /// An explicit token names no entry in the encoder catalog.
    UnknownExplicit {
        /// Token supplied by the caller, retained verbatim.
        requested: TargetToken,
    },
    /// A catalog target was selected explicitly but this input cannot reach it.
    ExplicitUnavailable {
        /// Canonical catalog target selected by the token.
        target: DialectId,
        /// Token supplied by the caller, retained verbatim.
        requested: TargetToken,
        /// Input-conditioned reason the writer cannot deliver the target.
        reason: String,
    },
    /// Same-format inheritance selected a source dialect the writer cannot preserve.
    InheritedUnavailable {
        /// Recorded source dialect selected by inheritance.
        source: DialectId,
        /// Input-conditioned reason the writer cannot preserve it.
        reason: String,
    },
    /// Same-format inheritance found source metadata without a dialect.
    UnrecordedSource,
    /// Inheritance had no same-format source and the catalog declares no default.
    NoDefault {
        /// Why no same-format source identity was available to inherit.
        source: DefaultSource,
    },
    /// A cross-format default was selected but this input cannot reach it.
    DefaultUnavailable {
        /// Canonical catalog default.
        target: DialectId,
        /// Why the catalog default, rather than source inheritance, was selected.
        source: DefaultSource,
        /// Input-conditioned reason the writer cannot deliver the target.
        reason: String,
    },
}

/// Why an encoder could not resolve or deliver a write target.
///
/// The refusal carries the encoder catalog once, beside the request-state
/// reason, so every reason is rendered and reported against the same catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetRefusal {
    kind: TargetRefusalKind,
    available: TargetCatalog,
}

impl Serialize for TargetRefusal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            format: &'a str,
            refusal: &'a TargetRefusalKind,
            available: TargetCatalog,
        }
        Wire {
            format: self.format(),
            refusal: &self.kind,
            available: self.available,
        }
        .serialize(serializer)
    }
}

/// Why inheritance must select an encoder catalog default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "format")]
pub enum DefaultSource {
    /// The document records no source metadata.
    NoSource,
    /// The document's source belongs to another format.
    ForeignFormat(String),
}

impl TargetRefusal {
    /// Associates one request-state reason with the refusing encoder catalog.
    #[must_use]
    pub fn new(kind: TargetRefusalKind, available: TargetCatalog) -> Self {
        Self { kind, available }
    }

    /// Builds the refusal for an explicit token outside an encoder catalog.
    #[must_use]
    pub fn unknown_explicit(requested: impl Into<String>, available: TargetCatalog) -> Self {
        Self::new(
            TargetRefusalKind::UnknownExplicit {
                requested: TargetToken::new(requested),
            },
            available,
        )
    }

    /// Returns the refusing encoder format.
    #[must_use]
    pub fn format(&self) -> &str {
        self.available.namespace()
    }

    /// Returns the encoder's structured synthesis catalog.
    #[must_use]
    pub const fn available(&self) -> &'static [TargetDescriptor] {
        self.available.targets()
    }

    fn write_available(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("; available targets: ")?;
        let mut targets = self.available().iter();
        let Some(first) = targets.next() else {
            return f.write_str("none");
        };
        f.write_str(first.id.as_str())?;
        for target in targets {
            write!(f, ", {}", target.id)?;
        }
        Ok(())
    }
}

impl fmt::Display for TargetRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let format = self.format();
        match &self.kind {
            TargetRefusalKind::UnknownExplicit { requested } => write!(
                f,
                "{format} cannot write {requested}: not a target this encoder can synthesize"
            )?,
            TargetRefusalKind::ExplicitUnavailable {
                target,
                requested,
                reason,
                ..
            } => write!(
                f,
                "{format} cannot write explicit target {requested} ({target}): {reason}"
            )?,
            TargetRefusalKind::InheritedUnavailable { source, reason } => write!(
                f,
                "{format} cannot preserve source dialect {source}: {reason}"
            )?,
            TargetRefusalKind::UnrecordedSource => write!(
                f,
                "{format} cannot inherit a write target: the {format} source records no dialect; name an explicit target"
            )?,
            TargetRefusalKind::NoDefault {
                source: DefaultSource::ForeignFormat(source_format),
            } => write!(
                f,
                "{format} cannot inherit a write target from source format {source_format}: this encoder declares no cross-format default"
            )?,
            TargetRefusalKind::NoDefault {
                source: DefaultSource::NoSource,
            } => write!(
                f,
                "{format} cannot select an inherited write target: the document records no source format and this encoder declares no default"
            )?,
            TargetRefusalKind::DefaultUnavailable {
                target,
                source,
                reason,
            } => {
                write!(f, "{format} cannot write default target {target}")?;
                if let DefaultSource::ForeignFormat(source_format) = source {
                    write!(f, " selected for source format {source_format}")?;
                }
                write!(f, ": {reason}")?;
            }
        }
        self.write_available(f)
    }
}

impl std::error::Error for TargetRefusal {}

#[cfg(test)]
mod tests {
    use super::{
        DefaultSource, TargetCatalog, TargetDescriptor, TargetRefusal, TargetRefusalKind,
        TargetToken,
    };
    use crate::dialect::DialectId;

    const NO_ALIASES: &[&str] = &[];

    const TARGETS: &[TargetDescriptor] = &[TargetDescriptor {
        id: crate::dialect_id!("fcstd:schema-4"),
        aliases: &["4"],
    }];

    const fn target(id: &'static str, aliases: &'static [&'static str]) -> TargetDescriptor {
        TargetDescriptor {
            id: DialectId::from_static(
                crate::dialect::StaticDialectId::new(id).expect("test id has dialect grammar"),
            ),
            aliases,
        }
    }

    #[test]
    fn target_refusal_serializes_request_state_and_the_complete_catalog() {
        let refusal = TargetRefusal::new(
            TargetRefusalKind::ExplicitUnavailable {
                target: crate::dialect_id!("fcstd:schema-4"),
                requested: TargetToken::new("4"),
                reason: "the source image cannot be patched".into(),
            },
            TargetCatalog::new(TARGETS, None),
        );

        assert_eq!(
            serde_json::to_value(refusal).expect("target refusal serializes"),
            serde_json::json!({
                "format": "fcstd",
                "refusal": {
                    "kind": "explicit_unavailable",
                    "target": "fcstd:schema-4",
                    "requested": "4",
                    "reason": "the source image cannot be patched",
                },
                "available": [{
                    "id": "fcstd:schema-4",
                    "aliases": ["4"],
                    "default": false
                }]
            })
        );
    }

    #[test]
    #[should_panic(expected = "target catalog rows must share a namespace")]
    fn target_catalog_rejects_mixed_namespaces() {
        static MIXED: &[TargetDescriptor] =
            &[target("fcstd:schema-4", &[]), target("step:ap242-e3", &[])];
        let _catalog = TargetCatalog::new(MIXED, None);
    }

    #[test]
    fn empty_catalog_refusal_retains_its_format() {
        let refusal = TargetRefusal::unknown_explicit("future", TargetCatalog::empty("cadir"));
        assert_eq!(
            serde_json::to_value(&refusal).expect("serialize target refusal"),
            serde_json::json!({
                "format": "cadir",
                "refusal": {"kind": "unknown_explicit", "requested": "future"},
                "available": []
            })
        );
        assert_eq!(refusal.to_string(), "cadir cannot write future: not a target this encoder can synthesize; available targets: none");
    }

    #[test]
    fn target_lookup_accepts_each_owned_spelling_and_rejects_a_miss() {
        const TARGETS: &[TargetDescriptor] = &[target("test:first", &["one", "primary"])];
        let catalog = TargetCatalog::new(TARGETS, None);

        assert_eq!(
            TARGETS[0].accepted_tokens().collect::<Vec<_>>(),
            vec!["test:first", "first", "one", "primary"]
        );
        for token in ["test:first", "first", "one", "primary"] {
            assert_eq!(
                catalog.find(token).map(|(_, entry)| entry.id.as_str()),
                Some("test:first"),
                "lookup failed for {token:?}"
            );
        }
        assert_eq!(catalog.find("missing"), None);
    }

    #[test]
    #[should_panic(expected = "target catalog default is out of bounds")]
    fn a_target_catalog_rejects_an_invalid_default_index() {
        let _catalog = TargetCatalog::new(TARGETS, Some(TARGETS.len()));
    }

    #[test]
    fn a_target_catalog_allows_repeated_spellings_within_one_row() {
        const TARGETS: &[TargetDescriptor] = &[
            target("test:first", &["first", "test:first", "one", "one"]),
            target("test:second", &["second", "two"]),
        ];
        const CATALOG: TargetCatalog = TargetCatalog::new(TARGETS, Some(0));
        for token in ["first", "test:first", "one"] {
            assert_eq!(CATALOG.find(token).map(|(index, _)| index), Some(0));
        }
        for token in ["second", "test:second", "two"] {
            assert_eq!(CATALOG.find(token).map(|(index, _)| index), Some(1));
        }
    }

    #[test]
    #[should_panic(expected = "target catalog spellings must select one row")]
    fn a_target_catalog_rejects_duplicate_ids() {
        const DUPLICATES: &[TargetDescriptor] = &[
            target("test:same", NO_ALIASES),
            target("test:same", NO_ALIASES),
        ];
        let _catalog = TargetCatalog::new(DUPLICATES, None);
    }

    #[test]
    #[should_panic(expected = "target catalog spellings must select one row")]
    fn a_target_catalog_rejects_duplicate_aliases() {
        const DUPLICATES: &[TargetDescriptor] = &[
            target("test:first", &["same"]),
            target("test:second", &["same"]),
        ];
        let _catalog = TargetCatalog::new(DUPLICATES, None);
    }

    #[test]
    #[should_panic(expected = "target catalog spellings must select one row")]
    fn a_target_catalog_rejects_an_alias_that_is_an_id() {
        const DUPLICATES: &[TargetDescriptor] = &[
            target("test:first", &["test:second"]),
            target("test:second", NO_ALIASES),
        ];
        let _catalog = TargetCatalog::new(DUPLICATES, None);
    }

    #[test]
    #[should_panic(expected = "target catalog spellings must select one row")]
    fn a_target_catalog_rejects_an_alias_that_is_another_rows_local_id() {
        const DUPLICATES: &[TargetDescriptor] = &[
            target("test:first", &["second"]),
            target("test:second", NO_ALIASES),
        ];
        let _catalog = TargetCatalog::new(DUPLICATES, None);
    }

    #[test]
    fn missing_default_names_a_foreign_source_without_inventing_a_dialect() {
        let refusal = TargetRefusal::new(
            TargetRefusalKind::NoDefault {
                source: DefaultSource::ForeignFormat("step".into()),
            },
            TargetCatalog::new(TARGETS, None),
        );

        assert_eq!(
            ({
                let wire = serde_json::to_value(&refusal).expect("serialize refusal");
                wire["refusal"]
                    .get("requested")
                    .or_else(|| wire["refusal"].get("source"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .as_deref(),
            None
        );
        assert_eq!(refusal.available(), TARGETS);
        assert_eq!(
            refusal.to_string(),
            "fcstd cannot inherit a write target from source format step: this encoder declares no cross-format default; available targets: fcstd:schema-4"
        );
    }

    #[test]
    fn unrecorded_same_format_source_is_not_a_missing_default() {
        let refusal = TargetRefusal::new(
            TargetRefusalKind::UnrecordedSource,
            TargetCatalog::new(TARGETS, None),
        );

        assert_eq!(
            refusal.to_string(),
            "fcstd cannot inherit a write target: the fcstd source records no dialect; name an explicit target; available targets: fcstd:schema-4"
        );
    }
}
