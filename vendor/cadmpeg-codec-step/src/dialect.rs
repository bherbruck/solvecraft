// SPDX-License-Identifier: Apache-2.0
//! STEP dialect identity: which registry row a document is, and how it was
//! admitted.
//!
//! The `*LossCode` template: the enum is internal, registry-generated
//! [`DialectId`] constants are the boundary, [`StepDialect::classify`] is the
//! one construction path for a [`DialectMatch`], and the vocabulary is closed.
//!
//! # The identity axis is the `FILE_SCHEMA` identifier
//!
//! Identity rows and parser grammars are independent, and STEP is the extreme
//! case of the gap. The codec has exactly one Part 21 read grammar:
//! nothing in the reader branches on the declared schema. The schema identifier
//! is read after the parse, recorded, and used for DATA-section name matching.
//! It is nonetheless the identity axis. [`StepSchema::file_schema`] owns the
//! declarations for writable Part 21 rows; [`Part21Dialect::schema_identifier`]
//! delegates those rows to it and adds the edition-unspecified AP242 row.
//!
//! The `FILE_DESCRIPTION` implementation level is the axis the parser does
//! branch on, and it is deliberately **not** an identity axis here: it is
//! recorded verbatim under [`DECLARED_IMPLEMENTATION_LEVEL`] and nothing in
//! this module reads it. Crossing the two axes would make the rows disjoint at
//! the cost of about 45 rows, most of them unwitnessed.
//!
//! # The declaration is evidence; the id is identity
//!
//! [`DialectMatch::declared`] records what `FILE_SCHEMA` says.
//! [`DialectMatch::dialect`] records which registry row the document satisfies.
//! A row matches only when *every* one of its discriminants matches. Four rows
//! share the AP242 schema name and separate on the object identifier, which
//! Part 21 makes optional: absent is [`Part21Dialect::Ap242`], the region where
//! the schema is declared and the edition is not; each declared edition has its
//! own row; an edition claim naming no declared edition satisfies no row and
//! lands on [`StepDialect::Unknown`], the mandatory totality row. Parsing an
//! edition out of an id, or expecting an id to agree with the
//! identifier beside it, is wrong for exactly the files whose declarations are
//! incomplete.
//!
//! # Alternate encodings
//!
//! Three rows — Part 28 XML, the AP242 BO-Model XML sidecar, and the Part 26
//! HDF5 encoding — are structural refusals below decode.
//! [`refuse_alternate_encoding`] returns [`CodecError::UnsupportedDialect`]
//! before any exchange structure is read. The error carries the refused row's
//! [`DialectMatch`] even though no decode report exists.

use crate::loss::StepLossCode;
use crate::options::StepSchema;
use crate::parse::schema_identifier::split_schema_identifier;
use crate::parse::Exchange;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::{Admission, DialectId, DialectLayers, DialectMatch, Grammar};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::BTreeMap;

include!("dialect/registry_ids.rs");

/// Key of the `FILE_SCHEMA` identifier the row keys on, in
/// [`DialectMatch::declared`].
///
/// Verbatim as read, object-identifier braces included. Absent when the header
/// declares no readable `FILE_SCHEMA` identifier at all.
const DECLARED_FILE_SCHEMA_IDENTIFIER: &str = "file_schema_identifier";
/// Key of the whole `FILE_SCHEMA` list, in [`DialectMatch::declared`], present
/// only when the list declares more than one identifier.
///
/// The identifiers verbatim as read, joined with `,`. See
/// [`StepDialect::classify`] for why one entry of the list is the identity.
const DECLARED_FILE_SCHEMA_IDENTIFIERS: &str = "file_schema_identifiers";
/// Key of the object-identifier arcs of the classified identifier, in
/// [`DialectMatch::declared`]. Absent when the identifier carries none.
///
/// The text between the braces, verbatim as read and untrimmed, so
/// `'… { 1 0 10303 442 3 1 4 }'` records `" 1 0 10303 442 3 1 4 "`. This is
/// evidence, not a join key: `crates/cadmpeg-registry/docs/dialects.toml` writes the same arcs trimmed
/// under `long_form_arcs`, and the resolved id is what a consumer compares.
const DECLARED_LONG_FORM_ARCS: &str = "long_form_arcs";
/// Key of the `FILE_DESCRIPTION` implementation level, in
/// [`DialectMatch::declared`]. Absent when the header declares no readable one.
///
/// Verbatim as read: `"2;1"`, `"4;2"`. Evidence only. The parser branches on
/// this value (`crate::parse::ImplementationLevel`) but no row here does.
const DECLARED_IMPLEMENTATION_LEVEL: &str = "implementation_level";

/// One row of `crates/cadmpeg-registry/docs/dialects.toml` under the `step` namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum StepDialect {
    /// A Part 21 identity row: every row with a `FILE_SCHEMA` identifier.
    Part21(Part21Dialect),
    Unknown,
}

/// A structurally identified STEP encoding this codec refuses before decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum AlternateEncoding {
    Part28Xml,
    Ap242BoModelXml,
    Part26Hdf5,
}

impl AlternateEncoding {
    #[cfg(test)]
    const ALL: [Self; 3] = [Self::Part28Xml, Self::Ap242BoModelXml, Self::Part26Hdf5];

    const fn id(self) -> DialectId {
        match self {
            Self::Part28Xml => STEP_PART28_XML,
            Self::Ap242BoModelXml => STEP_AP242_BO_MODEL_XML,
            Self::Part26Hdf5 => STEP_PART26_HDF5,
        }
    }

    const fn refusal_message(self) -> &'static str {
        match self {
            Self::Part26Hdf5 => "STEP Part 26 binary/HDF5 encoding",
            Self::Part28Xml => "STEP Part 28 XML encoding",
            Self::Ap242BoModelXml => "AP242 BO-Model XML sidecar",
        }
    }

    fn refused_match(self) -> DialectMatch {
        DialectMatch::refused(self.id())
    }
}

/// The rows a Part 21 `FILE_SCHEMA` identifier can name. Every row here has a
/// canonical identifier, so [`Part21Dialect::schema_identifier`] is total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Part21Dialect {
    /// A writable schema row.
    Schema(StepSchema),
    /// The edition-unspecified AP242 row: the schema name with no object
    /// identifier.
    Ap242,
}

impl Part21Dialect {
    /// The registry-generated id for this row.
    const fn id(self) -> DialectId {
        match self {
            Self::Schema(schema) => schema.id(),
            Self::Ap242 => STEP_AP242,
        }
    }

    /// The canonical `FILE_SCHEMA` identifier for this row.
    const fn schema_identifier(self) -> &'static str {
        match self {
            Self::Schema(schema) => schema.file_schema(),
            Self::Ap242 => "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF",
        }
    }
}

/// The row whose entity vocabulary this codec actually reads a Part 21
/// exchange with, whatever the exchange declares.
///
/// There is one Part 21 grammar and one entity vocabulary here, and it is the
/// AP242 one: tessellation, semantic PMI, and draughting presentation are
/// decoded from any exchange, and no reader stage consults `FILE_SCHEMA`.
/// Edition 3 is the newest of the AP242 rows and the one the reader's other
/// defaults follow, so it names the strategy used.
const NEAREST_STRATEGY: Part21Dialect = Part21Dialect::Schema(StepSchema::Ap242Edition3);

impl StepDialect {
    /// Every dialect identity this enum can name.
    #[cfg(test)]
    const ALL: [Self; 8] = [
        Self::Part21(Part21Dialect::Schema(StepSchema::Ap203Edition1)),
        Self::Part21(Part21Dialect::Schema(StepSchema::Ap203Edition2)),
        Self::Part21(Part21Dialect::Schema(StepSchema::Ap214)),
        Self::Part21(Part21Dialect::Ap242),
        Self::Part21(Part21Dialect::Schema(StepSchema::Ap242Edition1)),
        Self::Part21(Part21Dialect::Schema(StepSchema::Ap242Edition2)),
        Self::Part21(Part21Dialect::Schema(StepSchema::Ap242Edition3)),
        Self::Unknown,
    ];

    /// The registry-generated id for this variant.
    const fn id(self) -> DialectId {
        match self {
            Self::Part21(row) => row.id(),
            Self::Unknown => STEP_UNKNOWN,
        }
    }

    /// The row for one `FILE_SCHEMA` identifier, or [`Self::Unknown`] where the
    /// registry declares no such row.
    ///
    /// The schema name is the first discriminant of every Part 21 row, and the
    /// three single-row names need nothing else: their arcs, present or absent,
    /// separate no two rows. This matches the reader's own bare-or-arc-bearing
    /// name comparison (`crate::parse::schema_identifier_matches`).
    ///
    /// The AP242 name is shared by four rows, so the object identifier is the
    /// second discriminant there and its three states are distinct facts:
    ///
    /// - absent — a complete declaration naming no edition, [`Part21Dialect::Ap242`].
    ///   The object identifier is optional in Part 21; leaving it out is legal
    ///   and says the edition is unspecified, not that the file is unrecognized.
    /// - present and naming an edition — that edition's row, decided against
    ///   this enum's own canonical identifiers.
    /// - present and naming no edition — [`Self::Unknown`]. An edition claim
    ///   that matches nothing this codec declares is an unrecognized
    ///   declaration, unlike making no claim at all. Arcs that do not read as a
    ///   numeric object identifier reach the same place, through the same call.
    fn from_schema_identifier(identifier: &str, object_identifier: Option<&[u64]>) -> Self {
        let Some((name, object_identifier_text)) = split_schema_identifier(identifier) else {
            return Self::Unknown;
        };
        let ap242_name = Part21Dialect::Ap242.schema_identifier();
        if name.eq_ignore_ascii_case(ap242_name) {
            if object_identifier_text.is_none() {
                return Self::Part21(Part21Dialect::Ap242);
            }
            return Self::ap242_edition(object_identifier).map_or(Self::Unknown, Self::Part21);
        }
        [
            Part21Dialect::Schema(StepSchema::Ap203Edition1),
            Part21Dialect::Schema(StepSchema::Ap203Edition2),
            Part21Dialect::Schema(StepSchema::Ap214),
        ]
        .into_iter()
        .find(|row| {
            split_schema_identifier(row.schema_identifier())
                .is_some_and(|(candidate, _)| name.eq_ignore_ascii_case(candidate))
        })
        .map_or(Self::Unknown, Self::Part21)
    }

    /// The AP242 edition row whose canonical object identifier the declaration
    /// names. A future or malformed object identifier names no verified row.
    fn ap242_edition(object_identifier: Option<&[u64]>) -> Option<Part21Dialect> {
        match object_identifier? {
            [1, 0, 10303, 442, 1, 1, 4] => Some(Part21Dialect::Schema(StepSchema::Ap242Edition1)),
            [1, 0, 10303, 442, 3, 1, 4] => Some(Part21Dialect::Schema(StepSchema::Ap242Edition2)),
            [1, 0, 10303, 442, 4, 1, 4] => Some(Part21Dialect::Schema(StepSchema::Ap242Edition3)),
            _ => None,
        }
    }

    /// Classifies one Part 21 exchange. The single construction path for a
    /// [`DialectMatch`] in this codec, so a classification bug and the report
    /// can never disagree.
    ///
    /// Identity is the row whose discriminants the declared identifier
    /// satisfies. Admission follows from that one predicate: a document whose
    /// identifier matched a row was read with the grammar that row declares —
    /// there is only one — and a document on [`Self::Unknown`] was read with a
    /// grammar no row declares for it, which is [`NEAREST_STRATEGY`].
    /// [`dialect_loss`] reads the admission back rather than recomputing it, so
    /// the biconditional the decode policy requires holds by construction
    /// rather than by two authors agreeing.
    ///
    /// A `FILE_SCHEMA` list declaring several identifiers is legal Part 21 and
    /// has no representation in [`DialectMatch`], which holds one dialect per
    /// format layer. The first identifier is the identity, matching the dialect
    /// note the codec reports, and the whole list is recorded under
    /// [`DECLARED_FILE_SCHEMA_IDENTIFIERS`].
    pub(crate) fn classify(
        exchange: &Exchange,
        ctx: &DecodeContext<'_>,
    ) -> Result<DialectMatch, CodecError> {
        let first = exchange.schema_identifiers().next();
        let object_identifier = exchange.primary_schema_object_identifier(ctx)?;
        let dialect = first.map_or(Self::Unknown, |identifier| {
            Self::from_schema_identifier(identifier, object_identifier.as_deref())
        });

        let mut declared = BTreeMap::new();
        if let Some(identifier) = first {
            ctx.insert_btree_map(
                &mut declared,
                cadmpeg_core::nonblank_const!(DECLARED_FILE_SCHEMA_IDENTIFIER),
                ctx.copy_retained_text(identifier, "step_dialect_declared_text")?,
                "step_dialect_declared_entries",
            )?;
            if let Some((_, Some(arcs))) = split_schema_identifier(identifier) {
                ctx.insert_btree_map(
                    &mut declared,
                    cadmpeg_core::nonblank_const!(DECLARED_LONG_FORM_ARCS),
                    ctx.copy_retained_text(arcs, "step_dialect_declared_text")?,
                    "step_dialect_declared_entries",
                )?;
            }
        }
        if exchange.schema_identifiers().nth(1).is_some() {
            ctx.insert_btree_map(
                &mut declared,
                cadmpeg_core::nonblank_const!(DECLARED_FILE_SCHEMA_IDENTIFIERS),
                exchange.joined_schema_identifiers(ctx)?,
                "step_dialect_declared_entries",
            )?;
        }
        ctx.insert_btree_map(
            &mut declared,
            cadmpeg_core::nonblank_const!(DECLARED_IMPLEMENTATION_LEVEL),
            ctx.copy_retained_text(
                exchange.implementation_level(),
                "step_dialect_declared_text",
            )?,
            "step_dialect_declared_entries",
        )?;

        Ok(if dialect == Self::Unknown {
            DialectMatch::unverified(dialect.id(), Grammar::of(&NEAREST_STRATEGY.id()))
        } else {
            DialectMatch::admitted(dialect.id())
        }
        .with_declared(declared))
    }
}

/// The dialect-unverified loss a match requires.
///
/// `None` exactly when `matched.admission` is [`Admission::Admitted`], because
/// this reads that field rather than reclassifying the document. Design §7
/// requires the charge on every [`Admission::Unverified`], and this is
/// how STEP satisfies it: the codec decodes an unrecognized schema by recording
/// the string and reading the exchange with the AP242 entity vocabulary
/// anyway, which is a recovery, not a verified read.
pub(crate) fn dialect_loss(
    matched: &DialectMatch,
    ctx: &DecodeContext<'_>,
) -> Result<Option<LossNote>, CodecError> {
    let Admission::Unverified { using } = matched.admission() else {
        return Ok(None);
    };
    let operation = "step_dialect_unverified_loss_text";
    let message = match matched.declared().get(DECLARED_FILE_SCHEMA_IDENTIFIER) {
        Some(identifier) => ctx.format_retained(
            format_args!(
                "FILE_SCHEMA identifier {identifier}; it satisfies no declared STEP dialect, so this decode read the exchange \
with the entity vocabulary verified for {FORMAT}:{}",
                using.as_str()
            ), operation,
        )?,
        None => ctx.format_retained(
            format_args!(
                "The exchange declares no FILE_SCHEMA identifier; it satisfies no declared STEP dialect, so this decode read the exchange \
with the entity vocabulary verified for {FORMAT}:{}",
                using.as_str()
            ), operation,
        )?,
    };
    Ok(Some(StepLossCode::SourceDialectUnverified.note(message)))
}

/// Refuses the three alternate encodings this codec identifies and does not
/// decode.
///
/// Structural refusal below decode: it runs before the ISO-10303-21 magic test
/// and returns before any exchange structure exists, so no report is produced.
/// The error carries the structurally identified row as a [`DialectMatch`].
///
/// CE-03/CE-04: Part 28 marker detection is not UOS conformance or schema
/// mapping. The caller owns the exact binding, governing EXPRESS schema,
/// derived XML Schema, identity/reference checks, and validation result; this
/// codec has no Part 28 adapter and builds no partial graph.
/// CE-05: HDF5 signature detection is not HDF5 validation or Part 26 mapping.
/// The caller owns the mapping edition, governing EXPRESS schema, HDF5 and Part
/// 26 validation, resource-local row/reference mapping, and malformed-input
/// result; this codec builds no partial graph.
/// CE-06: Part 26 and Part 21 resource graphs have no universal join. The
/// caller owns the exact resource identities, row-to-occurrence map,
/// schema/unit/context agreement, conflict policy, and retention of both source
/// graphs.
pub(crate) fn refuse_alternate_encoding(bytes: &[u8]) -> Result<(), CodecError> {
    let encoding = if crate::codec::is_part26_hdf5(bytes) {
        AlternateEncoding::Part26Hdf5
    } else if crate::codec::is_part28_xml(bytes) {
        AlternateEncoding::Part28Xml
    } else if crate::codec::is_ap242_bo_model_xml(bytes) {
        AlternateEncoding::Ap242BoModelXml
    } else {
        return Ok(());
    };
    Err(CodecError::UnsupportedDialect {
        dialects: Box::new(DialectLayers::of(encoding.refused_match())),
        message: encoding.refusal_message().into(),
    })
}

#[cfg(test)]
mod tests;
