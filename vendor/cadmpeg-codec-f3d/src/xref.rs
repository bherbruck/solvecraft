// SPDX-License-Identifier: Apache-2.0
//! External-reference (`XRef`) and document-type entries of a `.f3d` container
//! ([spec §1.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#12-stored-property-and-configuration-entries),
//! [§1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)).
//!
//! [`decode`] parses the top-level `RedirectionsStream.dat` table into
//! [`XrefDesign`] and [`XrefReference`] records. [`docstruct`] parses the JSON
//! form of `Properties.dat`. [`is_assembly`] classifies a BREP-less document
//! whose model is the placement of its XREF targets.

use cadmpeg_core::decode::u64_from_index;

use cadmpeg_core::container::ContainerRole;

use std::collections::HashSet;

use serde::Deserialize;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::products::{ExternalDocument, Occurrence, OccurrenceParent, PrototypeReference};

use crate::bytes::utf16::Utf16View;
use crate::bytes::{
    is_guid_prefix, is_guid_relaxed, lp_ascii_filtered_view, lp_ascii_strict,
    lp_ascii_strict_charged, lp_utf16_bounded_charged, lp_utf16_bounded_view, take_reference,
    take_reference_charged,
};
use crate::container::ContainerScan;
use crate::layout::component_insert_grouped_identity_carrier as grouped_identity_layout;
use crate::records::{
    feature::{assembly_features::DesignComponentInsertConstruction, scope::DesignParameterScope},
    xref::{XrefDesign, XrefReference},
};

/// Top-level container entry holding the external-reference table.
const REDIRECTIONS_ENTRY: &str = "RedirectionsStream.dat";
/// Top-level container entry holding the document-properties slot.
const PROPERTIES_ENTRY: &str = "Properties.dat";
/// Top-level JSON document carrying component-reference extension data.
const COMPONENT_REFERENCE_ENTRY: &str = "ComponentReferenceData.json";

/// Stable type-table identity of a Design occurrence-placement record.
const OCCURRENCE_PLACEMENT_TYPE_GUID: &str = "CE2913AA-CFE0-4F04-9102-24424ED3BCFA";

/// The parsed `RedirectionsStream.dat` table.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct XrefTable {
    /// Design entries in source order; entry 0 is the document itself.
    pub(crate) designs: Vec<XrefDesign>,
    /// Outgoing XREF placements in source order; empty for a leaf document.
    pub(crate) references: Vec<XrefReference>,
    /// Source reference ordinals whose role-named placement records were
    /// admitted by the type table but did not close under the generation's
    /// placement grammar and had no other valid placement carrier.
    pub(crate) placement_failures: Vec<u32>,
    /// `(XREF ordinal, placement-record count)` pairs whose structured
    /// placements were superseded by scope-bound Component Insert carriers.
    pub(crate) placement_overrides: Vec<PlacementOverride>,
}

/// One XREF ordinal whose structured placements were superseded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlacementOverride {
    pub(crate) ordinal: u32,
    pub(crate) count: usize,
}

/// The `docstruct` document-type declaration of a JSON `Properties.dat`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Docstruct {
    /// Document type: `assembly-design` or `part-design`.
    pub(crate) doc_type: String,
    /// Document subtype, e.g. `assembly-standard` or `part-sheetmetal`.
    pub(crate) subtype: Option<String>,
}

#[derive(Deserialize)]
struct RedirectionsJson {
    name: String,
    #[serde(rename = "schema-version")]
    schema_version: u32,
    designs: Vec<DesignJson>,
    /// `{}` in a leaf document, an array in a referencing document.
    references: ReferencesJson,
}

#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum ReferencesJson {
    List(Vec<ReferenceJson>),
    /// The only legal non-array form is the empty leaf object `{}`.
    Leaf {},
}

#[derive(Deserialize)]
struct DesignJson {
    #[serde(rename = "file-version")]
    file_version: i64,
    #[serde(rename = "targetFileName")]
    target_file_name: String,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "lineageUrn")]
    lineage_urn: String,
    #[serde(rename = "versionUrn")]
    version_urn: String,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ReferenceJson {
    #[serde(rename = "XREF")]
    Xref {
        from: String,
        #[serde(rename = "relativePath")]
        relative_path: String,
        properties: Vec<PropertyJson>,
    },
}

#[derive(Deserialize)]
enum PropertyJson {
    #[serde(rename = "neutronRole")]
    Role(StringProperty),
    #[serde(rename = "neutronData")]
    Data(StringProperty),
}

#[derive(Deserialize)]
#[serde(tag = "dataType", deny_unknown_fields)]
enum StringProperty {
    #[serde(rename = "STRING")]
    String { value: String },
}

impl ReferenceJson {
    fn into_record(
        self,
        ctx: &DecodeContext<'_>,
        ordinal: usize,
    ) -> Result<XrefReference, CodecError> {
        let Self::Xref {
            from,
            relative_path,
            properties,
        } = self;
        let from_capacity = from.capacity();
        let path_capacity = relative_path.capacity();
        let from = required_text(from, format_args!("references[{ordinal}].from"))?;
        let relative_path = required_text(
            relative_path,
            format_args!("references[{ordinal}].relativePath"),
        )?;
        let mut role = None;
        let mut data = None;
        for property in properties {
            let (name, value, slot) = match property {
                PropertyJson::Role(StringProperty::String { value }) => {
                    ("neutronRole", value, &mut role)
                }
                PropertyJson::Data(StringProperty::String { value }) => {
                    ("neutronData", value, &mut data)
                }
            };
            if slot.replace(value).is_some() {
                return Err(redirections_error(format_args!(
                    "references[{ordinal}].properties repeats {name}"
                )));
            }
        }
        let neutron_role = role.ok_or_else(|| {
            redirections_error(format_args!(
                "references[{ordinal}].properties is missing neutronRole"
            ))
        })?;
        let role_capacity = neutron_role.capacity();
        let neutron_role = required_text(
            neutron_role,
            format_args!("references[{ordinal}].properties.neutronRole.value"),
        )?;
        let neutron_data = data.ok_or_else(|| {
            redirections_error(format_args!(
                "references[{ordinal}].properties is missing neutronData"
            ))
        })?;
        let id = ctx.format_retained(
            format_args!("f3d:xref:reference#{ordinal}"),
            "retain F3D xref record ID",
        )?;
        for capacity in [
            from_capacity,
            path_capacity,
            role_capacity,
            neutron_data.capacity(),
        ] {
            ctx.charge_retained(u64_from_index(capacity), "retain F3D xref reference text")?;
        }
        Ok(XrefReference {
            id,
            ordinal: ordinal_at(ordinal)?,
            occurrence_ordinal: 0,
            from,
            relative_path,
            neutron_role,
            neutron_data,
            transform: None,
        })
    }
}

fn redirections_error(message: impl std::fmt::Display) -> CodecError {
    CodecError::malformed(format_args!("{REDIRECTIONS_ENTRY}: {message}"))
}

fn required_text(
    value: String,
    field: impl std::fmt::Display,
) -> Result<crate::records::xref::RequiredXrefText, CodecError> {
    value
        .try_into()
        .map_err(|_| redirections_error(format_args!("{field} must be non-empty")))
}

/// Validate `ComponentReferenceData.json`, if present.
///
/// The stable grammar is an open top-level JSON object. Member names and values
/// are application-defined extension data: the codec validates the envelope,
/// preserves the original ZIP entry byte-for-byte, and performs no semantic
/// projection without a separately identified field contract.
fn validate_component_reference_data(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<(), CodecError> {
    if !scan
        .entries
        .iter()
        .any(|entry| entry.name == COMPONENT_REFERENCE_ENTRY)
    {
        return Ok(());
    }
    parse_component_reference_data(ctx, scan.entry_bytes(COMPONENT_REFERENCE_ENTRY)?)?;
    Ok(())
}

fn parse_component_reference_data<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
) -> Result<
    (
        serde_json::Value,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    ctx.charge_work(u64_from_index(bytes.len()), "validate F3D JSON UTF-8")?;
    let text = std::str::from_utf8(bytes).map_err(|error| {
        CodecError::malformed(format_args!(
            "{COMPONENT_REFERENCE_ENTRY} is not valid JSON: {error}"
        ))
    })?;
    let (reservation, value) = {
        let (value, reservation) = ctx
            .parse_json_value(text, "parse F3D component reference JSON")
            .map_err(|error| {
                let CodecError::Malformed(error) = error else {
                    return error;
                };
                CodecError::malformed(format_args!(
                    "{COMPONENT_REFERENCE_ENTRY} is not valid JSON: {error}"
                ))
            })?;
        (reservation, value)
    };
    if !value.is_object() {
        return Err(CodecError::malformed(format_args!(
            "{COMPONENT_REFERENCE_ENTRY} must contain a top-level JSON object"
        )));
    }
    Ok((value, reservation))
}

/// Parse the top-level `RedirectionsStream.dat` table, if present.
pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Option<XrefTable>, CodecError> {
    decode_with_scopes(ctx, scan, &[])
}

/// Parse the external-reference table and bind its occurrences to exact
/// `Component Insert` constructions already decoded from the Design streams.
pub(crate) fn decode_with_scopes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Option<XrefTable>, CodecError> {
    // Validate the extension document independently of whether a redirections
    // table is present. Its members are application-defined and retained by
    // source fidelity, so no field-level semantics are guessed here.
    validate_component_reference_data(ctx, scan)?;
    let bytes = match scan.entry_bytes(REDIRECTIONS_ENTRY) {
        Ok(bytes) => bytes,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    let mut table = parse(ctx, bytes)?;
    bind_occurrences(ctx, scan, &mut table, scopes)?;
    Ok(Some(table))
}

/// The `u32` ordinal for an enumerated position, or a malformed-input error.
fn ordinal_at(position: usize) -> Result<u32, CodecError> {
    u32::try_from(position).map_err(|_| {
        CodecError::malformed(format_args!(
            "F3D external-reference ordinal {position} exceeds u32"
        ))
    })
}

/// Parse `RedirectionsStream.dat` bytes into an [`XrefTable`].
fn parse(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<XrefTable, CodecError> {
    ctx.charge_work(u64_from_index(bytes.len()), "validate F3D JSON UTF-8")?;
    let text = std::str::from_utf8(bytes).map_err(|error| {
        CodecError::malformed(format_args!(
            "{REDIRECTIONS_ENTRY} is not valid JSON: {error}"
        ))
    })?;
    let parsed: RedirectionsJson = ctx
        .parse_json(text, "parse F3D redirections JSON")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            CodecError::malformed(format_args!(
                "{REDIRECTIONS_ENTRY} is not valid JSON: {error}"
            ))
        })?;
    if parsed.name != "RedirectionsStream" {
        return Err(redirections_error(format_args!(
            "name must be RedirectionsStream"
        )));
    }
    if parsed.schema_version != 0 {
        let message = ctx.format_retained(
            format_args!(
                "F3D {REDIRECTIONS_ENTRY}: unsupported schema-version {}",
                parsed.schema_version
            ),
            "describe unsupported F3D redirections schema",
        )?;
        return Err(CodecError::NotImplemented(message));
    }
    if parsed.designs.is_empty() {
        return Err(redirections_error(
            "designs must contain the document entry",
        ));
    }
    let mut designs = Vec::new();
    for (ordinal, design) in parsed.designs.into_iter().enumerate() {
        ctx.reserve_vec(&mut designs, 1, "admit F3D xref designs")?;
        let name_capacity = design.target_file_name.capacity();
        let target_file_name = required_text(
            design.target_file_name,
            format_args!("designs[{ordinal}].targetFileName"),
        )?;
        let id = ctx.format_retained(
            format_args!("f3d:xref:design#{ordinal}"),
            "retain F3D xref record ID",
        )?;
        ctx.charge_retained(u64_from_index(name_capacity), "retain F3D xref design text")?;
        for text in [
            &design.display_name,
            &design.lineage_urn,
            &design.version_urn,
        ] {
            ctx.charge_retained(
                u64_from_index(text.capacity()),
                "retain F3D xref design text",
            )?;
        }
        designs.push(XrefDesign {
            id,
            ordinal: ordinal_at(ordinal)?,
            file_version: design.file_version,
            target_file_name,
            display_name: design.display_name,
            lineage_urn: design.lineage_urn,
            version_urn: design.version_urn,
        });
    }
    let references = match parsed.references {
        ReferencesJson::List(references) if !references.is_empty() => references,
        ReferencesJson::List(_) => {
            return Err(redirections_error(
                "references must be {} when the document has no outgoing XREF",
            ));
        }
        ReferencesJson::Leaf {} => Vec::new(),
    };
    let mut admitted_references = Vec::new();
    for (ordinal, reference) in references.into_iter().enumerate() {
        ctx.reserve_vec(&mut admitted_references, 1, "admit F3D xref references")?;
        admitted_references.push(reference.into_record(ctx, ordinal)?);
    }
    Ok(XrefTable {
        designs,
        references: admitted_references,
        placement_failures: Vec::new(),
        placement_overrides: Vec::new(),
    })
}

/// Parse the `docstruct` declaration of a non-empty `Properties.dat`, if
/// present. The entry is a `u32` payload byte count followed by that many
/// JSON bytes; count 0 is the empty slot and carries no declaration.
pub(crate) fn docstruct(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Option<Docstruct>, CodecError> {
    let bytes = match scan.entry_bytes(PROPERTIES_ENTRY) {
        Ok(bytes) => bytes,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    let mut view = View::over_retained(bytes);
    let Some(count) = view.u32_le().and_then(|count| usize::try_from(count).ok()) else {
        return Ok(None);
    };
    let Some(payload) = view.take(count) else {
        return Ok(None);
    };
    ctx.charge_work(u64_from_index(payload.len()), "validate F3D JSON UTF-8")?;
    let Ok(text) = std::str::from_utf8(payload) else {
        return Ok(None);
    };
    let (_reservation, value) = match ctx.parse_json_value(text, "parse F3D properties JSON") {
        Ok((value, reservation)) => (reservation, value),
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    let Some(docstruct) = value.get("docstruct") else {
        return Ok(None);
    };
    let Some(doc_type) = docstruct.get("type").and_then(serde_json::Value::as_str) else {
        return Ok(None);
    };
    let doc_type = ctx.copy_retained_text(doc_type, "copy F3D docstruct type")?;
    let subtype = docstruct
        .get("subtype")
        .and_then(serde_json::Value::as_str)
        .map(|subtype| ctx.copy_retained_text(subtype, "copy F3D docstruct subtype"))
        .transpose()?;
    Ok(Some(Docstruct { doc_type, subtype }))
}

/// A valid assembly document: declared `assembly-design`, at least one
/// outgoing XREF, and no B-rep streams. Its model is the placement of its
/// XREF targets.
pub(crate) fn is_assembly(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    table: Option<&XrefTable>,
) -> Result<bool, CodecError> {
    if crate::container::design_breps(scan).next().is_some()
        || table.is_none_or(|table| table.references.is_empty())
    {
        return Ok(false);
    }
    Ok(docstruct(ctx, scan)?.is_some_and(|docstruct| docstruct.doc_type == "assembly-design"))
}

/// The lineage/version design entry for one reference: the entry whose
/// `target_file_name` equals the reference's `relative_path`.
pub(crate) fn design_for<'a>(
    table: &'a XrefTable,
    reference: &XrefReference,
) -> Option<&'a XrefDesign> {
    table
        .designs
        .iter()
        .find(|design| design.target_file_name == reference.relative_path)
}

/// Project each external-reference placement as one root product occurrence.
pub(crate) fn project_occurrences(
    ctx: &DecodeContext<'_>,
    table: &XrefTable,
) -> Result<Vec<Occurrence>, cadmpeg_core::CodecError> {
    let mut occurrences = Vec::new();
    for (ordinal, reference) in table.references.iter().enumerate() {
        ctx.reserve_vec(&mut occurrences, 1, "project F3D xref occurrence")?;
        let path = ctx.copy_retained_text(&reference.relative_path, "copy F3D xref path")?;
        let transform = reference.transform.map_or(
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            crate::records::xref::XrefPlacementTransform::rows,
        );
        occurrences.push(Occurrence {
            id: crate::ids::neutral_xref_occurrence_id(
                reference.ordinal,
                reference.occurrence_ordinal,
            ),
            prototype: PrototypeReference::External {
                document: ExternalDocument::path(path),
                object: None,
            },
            parent: OccurrenceParent::Root {},
            ordinal: ordinal_at(ordinal)?,
            transform: crate::design::components::neutral_transform(transform)?,
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: Some(
                ctx.copy_retained_text(&reference.id, "copy F3D xref native reference")?,
            ),
        });
    }
    Ok(occurrences)
}

/// Resolve exact `Component Insert` history scopes to their placed occurrences.
pub(crate) fn bind_component_insert_features(
    features: &mut [Feature],
    scopes: &[DesignParameterScope],
    table: &XrefTable,
) {
    for scope in scopes {
        let Some(construction) = scope.component_insert_construction() else {
            continue;
        };
        let mut matches = table.references.iter().filter(|reference| {
            reference.neutron_role.as_str() == construction.neutron_role
                && reference
                    .transform
                    .map(crate::records::xref::XrefPlacementTransform::rows)
                    == Some((*construction.transform()).into())
        });
        let Some(reference) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        let Some(feature) = features
            .iter_mut()
            .find(|feature| feature.native_ref.as_deref() == Some(scope.id.as_str()))
        else {
            continue;
        };
        if matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { .. })
        ) {
            feature
                .evaluation
                .set_definition(FeatureDefinition::Operation(
                    FeatureOperation::InsertComponent {
                        occurrence: crate::ids::neutral_xref_occurrence_id(
                            reference.ordinal,
                            reference.occurrence_ordinal,
                        ),
                    },
                ));
        }
    }
}

/// Expand container references through their occurrence records in the active
/// Design `BulkStream` and retain each occurrence-local placement matrix.
fn bind_occurrences(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    table: &mut XrefTable,
    scopes: &[DesignParameterScope],
) -> Result<(), CodecError> {
    let mut streams = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let meta_entry = entry
            .name
            .strip_suffix("BulkStream.dat")
            .and_then(|prefix| {
                scan.entries
                    .iter()
                    .find(|candidate| candidate.name.strip_prefix(prefix) == Some("MetaStream.dat"))
            });
        let (serializer_magic, placement_offsets) = if let Some(meta_entry) = meta_entry {
            let meta = scan.parsed_metastream(ctx, &meta_entry.name)?;
            let meta_bytes = scan.entry_bytes(&meta_entry.name)?;
            (
                Some(crate::metastream::serializer_magic(
                    ctx,
                    meta_bytes,
                    &meta_entry.name,
                )?),
                Some(typed_occurrence_placement_offsets(ctx, &meta)?),
            )
        } else {
            (None, None)
        };
        let headers = indexed_records(ctx, bytes)?;
        let (placements, failures) = occurrence_placements_with_failures(
            ctx,
            bytes,
            &headers,
            serializer_magic,
            placement_offsets.as_ref(),
        )?;

        ctx.reserve_vec(&mut streams, 1, "collect F3D xref streams")?;
        streams.push((
            placements,
            failures,
            crate::ids::native_scope_charged(ctx, &entry.name)?,
        ));
    }
    let mut expanded = Vec::new();
    let mut placement_failures = Vec::new();
    let mut placement_overrides = Vec::new();
    for reference in &table.references {
        let mut occurrences = Vec::new();
        for (placements, _, stream) in &streams {
            let direct = select_component_insert_transforms(
                ctx,
                scopes.iter().filter_map(|scope| {
                    let stream = crate::ids::native_stream(&scope.id)?;
                    let construction = scope.component_insert_construction()?;
                    Some((stream, construction))
                }),
                stream,
                &reference.neutron_role,
            )?;
            let structured_count =
                superseded_placement_count(&direct, placements, &reference.neutron_role);
            if !direct.is_empty() && structured_count != 0 {
                ctx.reserve_vec(
                    &mut placement_overrides,
                    1,
                    "report F3D xref placement override",
                )?;
                placement_overrides.push(PlacementOverride {
                    ordinal: reference.ordinal,
                    count: structured_count,
                });
            }
            let selected = occurrence_transforms_with_precedence(
                ctx,
                direct,
                placements,
                &reference.neutron_role,
            )?;

            ctx.reserve_vec(
                &mut occurrences,
                selected.len(),
                "collect F3D xref occurrence transforms",
            )?;
            occurrences.extend(selected);
        }
        if occurrences.is_empty() {
            if streams.iter().any(|(_, failures, stream)| {
                !scopes
                    .iter()
                    .filter_map(|scope| {
                        let construction_stream = crate::ids::native_stream(&scope.id)?;
                        let construction = scope.component_insert_construction()?;
                        Some((construction_stream, construction))
                    })
                    .any(|(construction_stream, construction)| {
                        construction_stream == stream
                            && construction.neutron_role == reference.neutron_role.as_str()
                    })
                    && failures.iter().any(|failure| {
                        failure
                            .link_names
                            .iter()
                            .any(|name| name == reference.neutron_role.as_str())
                    })
            }) {
                ctx.reserve_vec(
                    &mut placement_failures,
                    1,
                    "report F3D xref placement failure",
                )?;
                placement_failures.push(reference.ordinal);
            }

            ctx.reserve_vec(&mut expanded, 1, "expand F3D xref references")?;
            expanded.push(copy_reference_charged(ctx, reference, None)?);
            continue;
        }
        for (occurrence_ordinal, transform) in occurrences.into_iter().enumerate() {
            ctx.reserve_vec(&mut expanded, 1, "expand F3D xref references")?;
            let occurrence_id = ctx.format_retained(
                format_args!(
                    "f3d:xref:reference#{}-occurrence-{occurrence_ordinal}",
                    reference.ordinal
                ),
                "retain F3D xref record ID",
            )?;
            let mut occurrence = copy_reference_charged(ctx, reference, Some(occurrence_id))?;
            occurrence.occurrence_ordinal = ordinal_at(occurrence_ordinal)?;
            occurrence.transform = transform
                .map(crate::records::xref::XrefPlacementTransform::try_from)
                .transpose()
                .map_err(CodecError::NotImplemented)?;
            expanded.push(occurrence);
        }
    }
    table.references = expanded;
    table.placement_failures = placement_failures;
    table.placement_overrides = placement_overrides;
    Ok(())
}

fn copy_reference_charged(
    ctx: &DecodeContext<'_>,
    source: &XrefReference,
    occurrence_id: Option<String>,
) -> Result<XrefReference, CodecError> {
    let operation = "copy F3D xref reference";
    Ok(XrefReference {
        id: match occurrence_id {
            Some(id) => id,
            None => ctx.copy_retained_text(&source.id, operation)?,
        },
        ordinal: source.ordinal,
        occurrence_ordinal: source.occurrence_ordinal,
        from: ctx
            .copy_retained_text(&source.from, operation)?
            .try_into()
            .map_err(CodecError::malformed)?,
        relative_path: ctx
            .copy_retained_text(&source.relative_path, operation)?
            .try_into()
            .map_err(CodecError::malformed)?,
        neutron_role: ctx
            .copy_retained_text(&source.neutron_role, operation)?
            .try_into()
            .map_err(CodecError::malformed)?,
        neutron_data: ctx.copy_retained_text(&source.neutron_data, operation)?,
        transform: source.transform,
    })
}

/// Select the exact `Component Insert` constructions for one Design stream
/// and external-reference role. The construction parser has already joined
/// each role to its scope-owned relation record and verified its carrier
/// transform, so the class tag is not an admission discriminator here.
fn select_component_insert_transforms<'a, I>(
    ctx: &DecodeContext<'_>,
    constructions: I,
    stream: &str,
    role: &str,
) -> Result<Vec<[[f64; 4]; 4]>, CodecError>
where
    I: IntoIterator<Item = (&'a str, &'a DesignComponentInsertConstruction)>,
{
    ctx.collect_vec(
        constructions
            .into_iter()
            .filter(|(construction_stream, construction)| {
                *construction_stream == stream && construction.neutron_role == role
            })
            .map(|(_, construction)| construction.transform().rows()),
        "select F3D component insert transforms",
    )
}

/// Use scope-bound carriers when present. Placement records are the fallback
/// for a stream with no exact carrier for this role.
fn occurrence_transforms_with_precedence(
    ctx: &DecodeContext<'_>,
    direct: Vec<[[f64; 4]; 4]>,
    placements: &[OccurrencePlacement],
    role: &str,
) -> Result<Vec<Option<[[f64; 4]; 4]>>, CodecError> {
    if direct.is_empty() {
        occurrence_transforms(ctx, placements, role)
    } else {
        ctx.collect_vec(
            direct.into_iter().map(Some),
            "select F3D direct xref transforms",
        )
    }
}

/// Count structured placements that are discarded when a scope-bound carrier
/// supplies at least one transform for the same role.
fn superseded_placement_count(
    direct: &[[[f64; 4]; 4]],
    placements: &[OccurrencePlacement],
    role: &str,
) -> usize {
    if direct.is_empty() {
        0
    } else {
        placements
            .iter()
            .filter(|placement| placement.link_names.iter().any(|name| name == role))
            .count()
    }
}

/// Return the `BulkStream` offsets whose type-table identity is the stable
/// occurrence-placement type. Dynamic class tags and record shape are not
/// sufficient because unrelated component records can share that shape.
fn typed_occurrence_placement_offsets(
    ctx: &DecodeContext<'_>,
    meta: &crate::metastream::MetaStream,
) -> Result<HashSet<usize>, CodecError> {
    let mut placement_entities = HashSet::new();
    for entity in meta
        .types
        .iter()
        .filter(|design_type| {
            design_type
                .type_guid
                .as_str()
                .eq_ignore_ascii_case(OCCURRENCE_PLACEMENT_TYPE_GUID)
        })
        .flat_map(|design_type| design_type.entities.values().copied())
    {
        if !placement_entities.contains(&entity) {
            ctx.reserve_set(
                &mut placement_entities,
                1,
                "index F3D xref placement entities",
            )?;
            placement_entities.insert(entity);
        }
    }
    let mut offsets = HashSet::new();
    for record in meta.records.iter().chain(meta.secondary_records.iter()) {
        if !placement_entities.contains(&record.entity_id) {
            continue;
        }
        let offset = usize::try_from(record.bulk_offset).map_err(|_| {
            CodecError::Malformed("F3D occurrence-placement BulkStream offset exceeds usize".into())
        })?;
        if !offsets.contains(&offset) {
            ctx.reserve_set(&mut offsets, 1, "index F3D xref placement offsets")?;
            offsets.insert(offset);
        }
    }
    Ok(offsets)
}

#[derive(Debug)]
struct IndexedRecord {
    offset: usize,
    end: usize,
}

/// The transforms of every placement whose target path carries `role`, in
/// record order. One occurrence-placement record is one occurrence.
fn occurrence_transforms(
    ctx: &DecodeContext<'_>,
    placements: &[OccurrencePlacement],
    role: &str,
) -> Result<Vec<Option<[[f64; 4]; 4]>>, CodecError> {
    ctx.collect_vec(
        placements
            .iter()
            .filter(|placement| placement.link_names.iter().any(|name| name == role))
            .map(|placement| placement.transform),
        "select F3D structured xref transforms",
    )
}

fn indexed_records(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<IndexedRecord>, CodecError> {
    ctx.charge_work(u64_from_index(bytes.len()), "scan F3D xref record headers")?;
    let mut records: Vec<IndexedRecord> = Vec::new();
    for at in bytes
        .len()
        .checked_sub(11)
        .into_iter()
        .flat_map(|last| 0..last)
    {
        if View::u32_le_at(bytes, at) != Some(3) {
            continue;
        }
        let Some(tag) = bytes.get(at + 4..at + 7) else {
            continue;
        };
        if !tag.iter().all(u8::is_ascii_digit) || bytes.get(at + 7..at + 15).is_none() {
            continue;
        }
        if let Some(previous) = records.last_mut() {
            previous.end = at;
        }

        ctx.reserve_vec(&mut records, 1, "index F3D xref record frame")?;
        records.push(IndexedRecord {
            offset: at,
            end: bytes.len(),
        });
    }
    Ok(records)
}

/// One occurrence-placement record: the target path it names and the transform
/// it places that path at
/// ([spec §1.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#14-external-references)
/// "**Placement.**").
#[derive(Debug, Clone, PartialEq)]
struct OccurrencePlacement {
    /// Cross-document link names carried by the path elements, in path order.
    /// The role-bearing element is not necessarily the first.
    link_names: Vec<String>,
    /// `None` is the stored identity form, which carries no matrix.
    transform: Option<[[f64; 4]; 4]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OccurrencePlacementFailure {
    /// Link names recovered from the valid target-path prefix.
    link_names: Vec<String>,
}

/// Parse every indexed record that closes exactly under the occurrence-placement
/// grammar, in record order.
#[cfg(test)]
fn occurrence_placements(
    bytes: &[u8],
    records: &[IndexedRecord],
    serializer_magic: Option<u32>,
) -> Vec<OccurrencePlacement> {
    occurrence_placements_filtered(bytes, records, serializer_magic, None)
}

/// Parse occurrence-placement records, optionally restricted by the
/// `MetaStream` type-table admission set.
#[cfg(test)]
fn occurrence_placements_filtered(
    bytes: &[u8],
    records: &[IndexedRecord],
    serializer_magic: Option<u32>,
    typed_offsets: Option<&HashSet<usize>>,
) -> Vec<OccurrencePlacement> {
    occurrence_placements_with_failures(
        &cadmpeg_test_support::service_decode_context(),
        bytes,
        records,
        serializer_magic,
        typed_offsets,
    )
    .expect("test placement parse")
    .0
}

/// Parse admitted placement records and retain role names from records whose
/// target path is valid but whose remaining generation-specific payload is not.
fn occurrence_placements_with_failures(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[IndexedRecord],
    serializer_magic: Option<u32>,
    typed_offsets: Option<&HashSet<usize>>,
) -> Result<(Vec<OccurrencePlacement>, Vec<OccurrencePlacementFailure>), CodecError> {
    let mut placements = Vec::new();
    let mut failures = Vec::new();
    for record in records
        .iter()
        .filter(|record| typed_offsets.is_none_or(|offsets| offsets.contains(&record.offset)))
    {
        let Some(body) = bytes.get(record.offset..record.end) else {
            continue;
        };
        if let Some(placement) = occurrence_placement(ctx, body, serializer_magic)? {
            ctx.reserve_vec(&mut placements, 1, "collect F3D xref placements")?;
            placements.push(placement);
        } else if let Some((link_names, _)) = occurrence_path(ctx, body)? {
            ctx.reserve_vec(&mut failures, 1, "collect F3D xref placement failures")?;
            failures.push(OccurrencePlacementFailure { link_names });
        } else if let Some(link_name) = legacy_occurrence_role(body) {
            let link_name = link_name.to_retained(ctx, "retain F3D UTF-16 string")?;
            let link_names = ctx.collect_vec([link_name], "collect F3D legacy xref role")?;

            ctx.reserve_vec(&mut failures, 1, "collect F3D xref placement failures")?;
            failures.push(OccurrencePlacementFailure { link_names });
        }
    }
    Ok((placements, failures))
}

/// Parse one record body, header included, requiring the member sequence to end
/// exactly at the record end.
fn occurrence_placement(
    decode: &DecodeContext<'_>,
    body: &[u8],
    serializer_magic: Option<u32>,
) -> Result<Option<OccurrencePlacement>, CodecError> {
    if let Some((role, transform)) = legacy_occurrence_placement(body) {
        let role = role.to_retained(decode, "retain F3D UTF-16 string")?;
        let link_names = decode.collect_vec([role], "collect F3D legacy xref role")?;
        return Ok(Some(OccurrencePlacement {
            link_names,
            transform,
        }));
    }
    if let Some(placement) = repeated_target_occurrence_placement(decode, body)? {
        return Ok(Some(placement));
    }
    if let Some(placement) = modern_occurrence_placement(decode, body, serializer_magic)? {
        return Ok(Some(placement));
    }
    let Some(record_index) = View::u32_le_at(body, 7) else {
        return Ok(None);
    };
    let Some((link_name, _)) = grouped_component_insert_identity(body, 0, body.len(), record_index)
    else {
        return Ok(None);
    };
    let mut link_names = decode.collection_vec(1, "collect F3D grouped placement link name")?;
    link_names.push(link_name.to_retained(decode, "retain F3D UTF-16 string")?);
    Ok(Some(OccurrencePlacement {
        link_names,
        transform: None,
    }))
}

/// Parse the placement generation that repeats the target identity after the
/// standard path and stores the identity flag beside that repeated target.
fn repeated_target_occurrence_placement(
    decode: &DecodeContext<'_>,
    body: &[u8],
) -> Result<Option<OccurrencePlacement>, CodecError> {
    Ok(
        repeated_target_occurrence_placement_details(decode, body)?.map(|details| {
            OccurrencePlacement {
                link_names: details.link_names,
                transform: details.transform.map(|(_, matrix)| matrix),
            }
        }),
    )
}

struct RepeatedTargetPlacementDetails {
    link_names: Vec<String>,
    role: String,
    role_offset: usize,
    transform: Option<(usize, [[f64; 4]; 4])>,
}

macro_rules! xref_some {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

fn repeated_target_occurrence_placement_details(
    decode: &DecodeContext<'_>,
    body: &[u8],
) -> Result<Option<RepeatedTargetPlacementDetails>, CodecError> {
    const METADATA_MARKER: &[u8] = &[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0];
    let (link_names, mut at) = xref_some!(occurrence_path(decode, body)?);
    if !matches!(xref_some!(View::u32_le_at(body, at)), 1..=6) {
        return Ok(None);
    }
    at += 4;
    for _ in 0..2 {
        let (guid, next) = xref_some!(lp_utf16_bounded_charged(
            decode,
            body,
            at,
            36..=36,
            "retain F3D UTF-16 string"
        )?);
        if !is_guid_relaxed(&guid) {
            return Ok(None);
        }
        at = next;
    }
    if xref_some!(body.get(at..at + METADATA_MARKER.len())) != METADATA_MARKER {
        return Ok(None);
    }
    at += METADATA_MARKER.len();

    let (component_guid, next) = xref_some!(lp_utf16_bounded_charged(
        decode,
        body,
        at,
        36..=36,
        "retain F3D UTF-16 string"
    )?);
    if !is_guid_relaxed(&component_guid) {
        return Ok(None);
    }
    at = next;
    if body.get(at) != Some(&0) {
        return Ok(None);
    }
    at += 1;
    let (type_guid, next) = xref_some!(lp_ascii_strict_charged(decode, body, at, 36..=36)?);
    if !is_guid_relaxed(&type_guid) {
        return Ok(None);
    }
    at = next;
    let role_offset = at;
    let (role, next) = xref_some!(lp_utf16_bounded_charged(
        decode,
        body,
        at,
        36..=256,
        "retain F3D UTF-16 string"
    )?);
    if !is_guid_prefix(&role) {
        return Ok(None);
    }
    at = next;
    if body.get(at) != Some(&0) {
        return Ok(None);
    }
    at += 1;
    let transform = match *xref_some!(body.get(at)) {
        1 => {
            at += 1;
            None
        }
        0 => {
            at += 1;
            let offset = at;
            let matrix = xref_some!(decode_rigid_matrix(body, at));
            at = xref_some!(at.checked_add(128));
            Some((offset, matrix))
        }
        _ => return Ok(None),
    };
    if xref_some!(View::u32_le_at(body, at)) != 0 {
        return Ok(None);
    }
    at += 4;
    let (final_role, next) = xref_some!(lp_utf16_bounded_charged(
        decode,
        body,
        at,
        36..=256,
        "retain F3D UTF-16 string"
    )?);
    if !final_role.eq_ignore_ascii_case(&role) {
        return Ok(None);
    }
    at = next;
    if body.get(at) != Some(&0) {
        return Ok(None);
    }
    at += 1;
    let reference = {
        let ctx = decode;
        take_reference_charged(ctx, body, &mut at)?
    };
    xref_some!(reference);
    Ok(
        (at == body.len()).then_some(RepeatedTargetPlacementDetails {
            link_names,
            transform,
            role,
            role_offset: role_offset + 4,
        }),
    )
}

/// Bind a repeated-target occurrence carrier to a Component Insert scope
/// through its relation record.
pub(crate) fn repeated_target_component_insert(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
    expected_transform: [[f64; 4]; 4],
) -> Result<Option<(String, usize, Option<usize>)>, CodecError> {
    let body = xref_some!(bytes.get(carrier_at..relation_at));
    if xref_some!(View::u64_le_at(body, 7)) != u64::from(carrier_record_index) {
        return Ok(None);
    }
    let details = xref_some!(repeated_target_occurrence_placement_details(ctx, body)?);
    let transform = details.transform.map_or(
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        |(_, matrix)| matrix,
    );
    if transform != expected_transform {
        return Ok(None);
    }
    Ok(Some((
        details.role,
        carrier_at + details.role_offset,
        details.transform.map(|(offset, _)| carrier_at + offset),
    )))
}

/// Parse the grouped identity carrier used by the compact `Component Insert`
/// generation. The carrier has no matrix; its placement is the stored
/// identity transform. The repeated GUID and role fields are part of the
/// carrier grammar, not an occurrence-count signal.
pub(crate) fn grouped_component_insert_identity(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(Utf16View<'_>, usize)> {
    grouped_component_insert_identity_with_layout(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
        "382",
    )
}

/// Parse the class-380 grouped identity carrier used by the class-410/class-261
/// `Component Insert` generation.
pub(crate) fn grouped_component_insert_identity_class380(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(Utf16View<'_>, usize)> {
    grouped_component_insert_identity_with_layout(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
        "380",
    )
}

/// Parse the class-369 grouped identity carrier used by the class-426/class-258
/// `Component Insert` generation.
pub(crate) fn grouped_component_insert_identity_class369(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(Utf16View<'_>, usize)> {
    grouped_component_insert_identity_with_layout(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
        "369",
    )
}

/// Parse the variable-role grouped identity carrier used by the class-434/
/// class-266 `Component Insert` generation.
pub(crate) fn grouped_component_insert_identity_class341(
    bytes: &[u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
) -> Option<(Utf16View<'_>, usize)> {
    grouped_component_insert_identity_with_layout(
        bytes,
        carrier_at,
        relation_at,
        carrier_record_index,
        "341",
    )
}

fn grouped_component_insert_identity_with_layout<'a>(
    bytes: &'a [u8],
    carrier_at: usize,
    relation_at: usize,
    carrier_record_index: u32,
    expected_class_tag: &str,
) -> Option<(Utf16View<'a>, usize)> {
    const MARKER_AFTER_ROLE: &[u8] = &[0, 1, 0, 0, 0, 0, 1, 0, 0, 0];
    const CLASS_369_GUID_ROLE_MARKER: &[u8] = &[0, 3, 0, 0, 0, 0, 1, 0, 0, 0];
    const CLASS_369_EXTERNAL_ROLE_MARKER: &[u8] = &[0, 4, 0, 0, 0, 0, 1, 0, 0, 0];
    const MARKER_AFTER_METADATA: &[u8] = &[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0];
    const MARKER_AFTER_PLACEMENT: &[u8] = &[0, 1, 0, 0, 0, 0];
    const CLASS_341_REPEAT_MARKER: &[u8] = &[1, 0, 0, 0, 0];
    const CLOSURE: &[u8] = &[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    let (class_tag, after_tag) =
        lp_ascii_filtered_view(bytes, carrier_at, 3..=3, u8::is_ascii_digit)?;
    let carrier_span = relation_at.checked_sub(carrier_at)?;
    if class_tag != expected_class_tag
        || after_tag != carrier_at + 7
        || View::u32_le_at(bytes, after_tag) != Some(carrier_record_index)
        || (expected_class_tag != "369"
            && expected_class_tag != "341"
            && carrier_span != grouped_identity_layout::LEN)
        || ((expected_class_tag == "369" || expected_class_tag == "341")
            && carrier_span < grouped_identity_layout::LEN)
        || bytes.get(carrier_at + 11..carrier_at + 19)? != [0; 8]
        || bytes.get(carrier_at + 19) != Some(&1)
        || bytes.get(carrier_at + 20..carrier_at + 24)? != [1, 0, 0, 0]
        || bytes.get(carrier_at + 24) != Some(&1)
        || bytes.get(carrier_at + 33) != Some(&1)
        || bytes.get(carrier_at + 34..carrier_at + 38)? != [0; 4]
    {
        return None;
    }
    let occurrence_identity = View::u64_le_at(
        bytes,
        carrier_at + grouped_identity_layout::OCCURRENCE_IDENTITY,
    )?;
    let (component_guid, mut at) = lp_utf16_bounded_view(
        bytes,
        carrier_at + grouped_identity_layout::FIRST_COMPONENT_GUID,
        36..=36,
    )?;
    if !component_guid.is_guid_relaxed() {
        return None;
    }
    if bytes.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    let (type_guid, next) = lp_ascii_strict(bytes, at, 36..=36)?;
    if !is_guid_relaxed(type_guid) {
        return None;
    }
    at = next;
    let first_role_at = at;
    let variable_role = matches!(expected_class_tag, "341" | "369");
    let role_bounds = if variable_role { 36..=256 } else { 36..=36 };
    let (role, next) = lp_utf16_bounded_view(bytes, at, role_bounds.clone())?;
    let valid_role = if variable_role {
        role.is_guid_relaxed() || role.is_guid_urn_role()
    } else {
        role.is_guid_relaxed()
    };
    if !valid_role {
        return None;
    }
    at = next;
    let marker_after_role = if expected_class_tag == "369" {
        if role.len() == 36 {
            CLASS_369_GUID_ROLE_MARKER
        } else {
            CLASS_369_EXTERNAL_ROLE_MARKER
        }
    } else {
        MARKER_AFTER_ROLE
    };
    if bytes.get(at..at + marker_after_role.len())? != marker_after_role {
        return None;
    }
    at += marker_after_role.len();

    let (metadata_guid_a, next) = lp_utf16_bounded_view(bytes, at, 36..=36)?;
    if !metadata_guid_a.is_guid_relaxed() {
        return None;
    }
    at = next;
    let (metadata_guid_b, next) = lp_utf16_bounded_view(bytes, at, 36..=36)?;
    if !metadata_guid_b.is_guid_relaxed() {
        return None;
    }
    at = next;
    if expected_class_tag == "341" {
        if bytes.get(at..at + 2)? != [0, 1]
            || View::u64_le_at(bytes, at + 2)? != occurrence_identity
        {
            return None;
        }
        at += 10;
        if bytes.get(at..at + CLASS_341_REPEAT_MARKER.len())? != CLASS_341_REPEAT_MARKER {
            return None;
        }
        at += CLASS_341_REPEAT_MARKER.len();
    } else {
        if bytes.get(at..at + MARKER_AFTER_METADATA.len())? != MARKER_AFTER_METADATA {
            return None;
        }
        at += MARKER_AFTER_METADATA.len();
    }

    let (repeated_component_guid, next) = lp_utf16_bounded_view(bytes, at, 36..=36)?;
    if !repeated_component_guid.is_guid_relaxed()
        || !repeated_component_guid.eq_ignore_ascii_case(component_guid)
    {
        return None;
    }
    at = next;
    if bytes.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    let (repeated_type_guid, next) = lp_ascii_strict(bytes, at, 36..=36)?;
    if !is_guid_relaxed(repeated_type_guid) || !repeated_type_guid.eq_ignore_ascii_case(type_guid) {
        return None;
    }
    at = next;
    let (repeated_role, next) = lp_utf16_bounded_view(bytes, at, role_bounds.clone())?;
    if !repeated_role.eq_ignore_ascii_case(role) {
        return None;
    }
    at = next;
    if bytes.get(at..at + MARKER_AFTER_PLACEMENT.len())? != MARKER_AFTER_PLACEMENT {
        return None;
    }
    at += MARKER_AFTER_PLACEMENT.len();

    let (final_role, next) = lp_utf16_bounded_view(bytes, at, role_bounds)?;
    if !final_role.eq_ignore_ascii_case(role) {
        return None;
    }
    at = next;
    if bytes.get(at..at + CLOSURE.len())? != CLOSURE || at + CLOSURE.len() != relation_at {
        return None;
    }
    Some((role, first_role_at + 4))
}

/// Parse the current placement envelope: a standard target path, an optional
/// rigid matrix, and the generation-selected reference runs.
fn modern_occurrence_placement(
    decode: &DecodeContext<'_>,
    body: &[u8],
    serializer_magic: Option<u32>,
) -> Result<Option<OccurrencePlacement>, CodecError> {
    let Some((link_names, at)) = occurrence_path(decode, body)? else {
        return Ok(None);
    };
    // The identity marker is absent in the oldest container generation, which
    // always stores the matrix. Both readings start with a zero byte when the
    // marker is present and the matrix follows, so the record end decides.
    for identity_marker in [true, false] {
        let mut cursor = at;
        let mut transform = None;
        if identity_marker {
            match body.get(cursor) {
                Some(1) => cursor += 1,
                Some(0) => {
                    let Some(matrix) = decode_rigid_matrix(body, cursor + 1) else {
                        continue;
                    };
                    transform = Some(matrix);
                    cursor += 129;
                }
                _ => continue,
            }
        } else {
            let Some(matrix) = decode_rigid_matrix(body, cursor) else {
                continue;
            };
            transform = Some(matrix);
            cursor += 128;
        }
        if placement_tail(body, cursor, serializer_magic).is_some() {
            return Ok(Some(OccurrencePlacement {
                link_names,

                transform,
            }));
        }
    }
    Ok(None)
}

/// Parse the legacy typed placement envelope.
///
/// This form keeps the same stable type-table identity as the current
/// placement, but its target-reference carrier is wider and its matrix is
/// after the repeated target envelope. The dynamic class tag is deliberately
/// not an admission key: the type-table identity and exact member framing are
/// the stable discriminators.
fn legacy_occurrence_placement(body: &[u8]) -> Option<(Utf16View<'_>, Option<[[f64; 4]; 4]>)> {
    let mut at = legacy_occurrence_prefix(body)?;
    let identity_marker = *body.get(at)?;
    at += 1;
    let transform = match identity_marker {
        1 => None,
        0 => {
            let matrix = decode_rigid_matrix(body, at)?;
            at = at.checked_add(128)?;
            Some(matrix)
        }
        _ => return None,
    };
    if View::u32_le_at(body, at)? != 0 {
        return None;
    }
    at += 4;
    let (link_name, after_role) = lp_utf16_bounded_view(body, at, 36..=36)?;
    if !link_name.is_guid_relaxed() {
        return None;
    }
    at = after_role;
    if body.get(at..at + 12)? != [0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
        return None;
    }
    at += 12;
    (at == body.len()).then_some((link_name, transform))
}

/// Return the role from a structurally valid legacy placement prefix.
///
/// The role is recovered even when the transform or closing tail is damaged,
/// so the caller can report an undecoded typed placement against the correct
/// external reference instead of treating it as an unrelated record.
fn legacy_occurrence_role(body: &[u8]) -> Option<Utf16View<'_>> {
    let mut at = legacy_occurrence_prefix(body)?;
    match *body.get(at)? {
        1 => at += 1,
        0 => at = at.checked_add(129)?,
        _ => return None,
    }
    if View::u32_le_at(body, at)? != 0 {
        return None;
    }
    at += 4;
    let (link_name, _) = lp_utf16_bounded_view(body, at, 36..=36)?;
    link_name.is_guid_relaxed().then_some(link_name)
}

/// Parse the shared prefix of the legacy identity and matrix forms.
fn legacy_occurrence_prefix(body: &[u8]) -> Option<usize> {
    let (_class_tag, after_tag) = lp_ascii_strict(body, 0, 3..=3)?;
    let mut at = after_tag.checked_add(8)?;
    let (_name, after_name) = lp_ascii_strict(body, at, 0..=256)?;
    at = after_name;
    if body.get(at) != Some(&1) {
        return None;
    }
    at += 1;
    if View::u32_le_at(body, at)? != 1 {
        return None;
    }
    at += 4;
    take_legacy_occurrence_reference(body, &mut at)?;
    View::u32_le_at(body, at)?;
    at += 4;
    if View::u32_le_at(body, at)? != 1 {
        return None;
    }
    at += 4;
    if body.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    if View::u32_le_at(body, at)? != 1 {
        return None;
    }
    at += 4;
    for _ in 0..2 {
        let (guid, next) = lp_utf16_bounded_view(body, at, 36..=36)?;
        if !guid.is_guid_relaxed() {
            return None;
        }
        at = next;
    }
    if body.get(at) != Some(&0) {
        return None;
    }
    at += 1;
    take_legacy_occurrence_reference(body, &mut at)?;
    Some(at)
}

/// Consume one legacy occurrence target reference.
fn take_legacy_occurrence_reference(body: &[u8], at: &mut usize) -> Option<()> {
    if body.get(*at) != Some(&1) {
        return None;
    }
    *at += 1;
    *at = at.checked_add(8)?;
    if body.get(*at) != Some(&1) {
        return None;
    }
    *at += 1;
    *at = at.checked_add(8)?;
    if body.get(*at) != Some(&0) {
        return None;
    }
    *at += 1;
    let (type_guid, next) = lp_ascii_strict(body, *at, 36..=36)?;
    if !is_guid_relaxed(type_guid) {
        return None;
    }
    *at = next;
    if body.get(*at) != Some(&0) {
        return None;
    }
    *at += 1;
    Some(())
}

/// Parse the target-path prefix shared by every occurrence-placement form.
fn occurrence_path(
    decode: &DecodeContext<'_>,
    body: &[u8],
) -> Result<Option<(Vec<String>, usize)>, CodecError> {
    // Header: the LP-ASCII decimal class tag, the u64 entity ID, and the
    // LP-ASCII record name.
    let class_tag = {
        let ctx = decode;
        lp_ascii_strict_charged(ctx, body, 0, 3..=3)?
    };
    let Some((_, after_tag)) = class_tag else {
        return Ok(None);
    };
    let Some(mut at) = after_tag.checked_add(8) else {
        return Ok(None);
    };
    let name = {
        let ctx = decode;
        lp_ascii_strict_charged(ctx, body, at, 0..=256)?
    };
    let Some((_, after_name)) = name else {
        return Ok(None);
    };
    at = after_name;
    if body.get(at) != Some(&1) {
        return Ok(None);
    }
    at += 1;
    let Some(count) = View::u32_le_at(body, at).and_then(|count| usize::try_from(count).ok())
    else {
        return Ok(None);
    };
    if count == 0 || count > 4096 {
        return Ok(None);
    }
    at += 4;
    let mut link_names = Vec::new();
    for _ in 0..count {
        let element = {
            let ctx = decode;
            take_reference_charged(ctx, body, &mut at)?
        };
        let Some(element) = element else {
            return Ok(None);
        };
        if let Some(link_name) = element.link_name() {
            let name = {
                let ctx = decode;
                {
                    ctx.copy_retained_text(link_name, "copy F3D xref placement link name")?
                }
            };
            {
                let ctx = decode;

                ctx.reserve_vec(&mut link_names, 1, "collect F3D xref placement link names")?;
            }
            link_names.push(name);
        }
        if View::u32_le_at(body, at).is_none() {
            return Ok(None);
        }
        at += 4;
    }
    if body.get(at) != Some(&0) {
        return Ok(None);
    }
    at += 1;
    Ok(Some((link_names, at)))
}

/// Consume the three reference runs that close a placement, returning `Some`
/// only when they end exactly at the record end.
fn placement_tail(body: &[u8], mut at: usize, serializer_magic: Option<u32>) -> Option<()> {
    let count = usize::try_from(View::u32_le_at(body, at)?).ok()?;
    if count > 256 {
        return None;
    }
    at += 4;
    for _ in 0..count {
        take_reference(body, &mut at)?;
    }
    // Only the modern MetaStream serializer magic admits the tagged run. Its
    // tag byte is neither reference presence value.
    if serializer_magic == Some(crate::metastream::MODERN_SERIALIZER_MAGIC) {
        if matches!(body.get(at), Some(0 | 1)) {
            return None;
        }
        at += 1;
        let tagged = usize::try_from(View::u32_le_at(body, at)?).ok()?;
        if tagged > 256 {
            return None;
        }
        at = at.checked_add(4)?.checked_add(tagged.checked_mul(4)?)?;
        take_reference(body, &mut at)?;
    }
    take_reference(body, &mut at)?;
    take_reference(body, &mut at)?;
    (at == body.len()).then_some(())
}

fn decode_rigid_matrix(bytes: &[u8], at: usize) -> Option<[[f64; 4]; 4]> {
    let mut view = View::over_retained(bytes);
    view.seek(at)?;
    let mut rows = [[0.0; 4]; 4];
    for row in &mut rows {
        for value in row {
            *value = view.f64_le()?;
        }
    }
    crate::records::xref::XrefPlacementTransform::try_from(rows)
        .ok()
        .map(crate::records::xref::XrefPlacementTransform::rows)
}

#[cfg(test)]
mod tests;
