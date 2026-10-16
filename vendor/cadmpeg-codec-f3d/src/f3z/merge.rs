// SPDX-License-Identifier: Apache-2.0
//! Occurrence-scoped merge of decoded F3Z member graphs.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::document::{EntityRewrite, Model};
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::SourceFidelity;
use cadmpeg_ir::{Native, NativeRecord};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::archive::{ArchiveSession, ClassifiedMember};
use crate::container::ContainerScan;
use crate::loss::F3dLossCode;
use crate::records::xref::XrefReference;
use crate::xref::{self, XrefTable};

/// Merges the root document's recursively reachable members in archive scope.
pub(super) fn merge_archive(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    archive: &ArchiveSession<'_>,
    model_root: String,
    ir: &mut cadmpeg_ir::CadIr,
    report: &mut cadmpeg_ir::codec::DecodeBody,
    fidelity: &mut cadmpeg_ir::SourceFidelity,
) -> Result<usize, CodecError> {
    let table = xref_table_from_ir(ctx, ir)?;

    let mut stack = Vec::new();
    ctx.reserve_vec(&mut stack, 1, "seed F3Z merge stack")?;
    stack.push(model_root);
    MergeSession {
        ctx,
        scan,
        archive,
        stack,
    }
    .merge(ir, report, fidelity, &table)
}

/// Reassigns only repeated sibling ordinals after independent document graphs
/// have been combined.
pub(super) fn make_sibling_ordinals_unique(
    ctx: &DecodeContext<'_>,
    occurrences: &mut [cadmpeg_ir::products::Occurrence],
) -> Result<(), CodecError> {
    use std::collections::{HashMap, HashSet};

    let mut used = HashMap::<Option<&cadmpeg_ir::ids::OccurrenceId>, HashSet<u32>>::new();
    for occurrence in occurrences {
        ctx.charge_work(1, "index F3Z sibling ordinals")?;
        let parent = match &occurrence.parent {
            cadmpeg_ir::products::OccurrenceParent::Root {} => None,
            cadmpeg_ir::products::OccurrenceParent::Occurrence { occurrence } => Some(occurrence),
        };
        if !used.contains_key(&parent) {
            ctx.reserve_map(&mut used, 1, "index F3Z sibling parents")?;
        }
        let siblings = used.entry(parent).or_default();
        let ordinal = if siblings.contains(&occurrence.ordinal) {
            let mut free = None;
            for candidate in 0..=u32::MAX {
                ctx.charge_work(1, "find F3Z sibling ordinal")?;
                if !siblings.contains(&candidate) {
                    free = Some(candidate);
                    break;
                }
            }
            free.ok_or_else(|| {
                CodecError::malformed(
                    "F3Z sibling occurrence population exhausts the u32 ordinal space",
                )
            })?
        } else {
            occurrence.ordinal
        };
        ctx.insert_hash_set(siblings, ordinal, "index F3Z sibling ordinals")?;
        occurrence.ordinal = ordinal;
    }
    Ok(())
}

fn xref_table_from_ir(
    ctx: &DecodeContext<'_>,
    ir: &cadmpeg_ir::CadIr,
) -> Result<XrefTable, CodecError> {
    fn load_arena<T: DeserializeOwned>(
        ctx: &DecodeContext<'_>,
        namespace: &cadmpeg_ir::NativeNamespace,
        name: &str,
    ) -> Result<Vec<T>, CodecError> {
        match namespace.arena_as_for_decode(ctx, name) {
            Ok(records) => Ok(records),
            Err(error) => match CodecError::from(error) {
                error @ CodecError::ResourceLimit(_) => Err(error),
                CodecError::Malformed(message) => Err(CodecError::Malformed(ctx.format_retained(
                    format_args!("invalid F3D native data: {message}"),
                    "report invalid F3D native data",
                )?)),
                error => Err(error),
            },
        }
    }
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(XrefTable::default());
    };
    Ok(XrefTable {
        designs: load_arena(ctx, namespace, "xref_designs")?,
        references: load_arena(ctx, namespace, "xref_references")?,
        placement_failures: Vec::new(),
        placement_overrides: Vec::new(),
    })
}

/// State shared by recursive F3Z reference traversal.
struct MergeSession<'r, 'a> {
    ctx: &'r DecodeContext<'a>,
    scan: &'r ContainerScan<'a>,
    archive: &'r ArchiveSession<'a>,
    stack: Vec<String>,
}

impl MergeSession<'_, '_> {
    /// Resolves outgoing references and merges each usable component graph.
    fn merge(
        &mut self,
        parent_ir: &mut cadmpeg_ir::CadIr,
        parent_report: &mut cadmpeg_ir::codec::DecodeBody,
        parent_fidelity: &mut cadmpeg_ir::SourceFidelity,
        table: &XrefTable,
    ) -> Result<usize, CodecError> {
        let mut merged = 0usize;
        for reference in &table.references {
            let occurrence = occurrence_key(self.ctx, reference)?;
            let label = xref::design_for(table, reference)
                .map_or(reference.relative_path.as_str(), |design| {
                    design.display_name.as_str()
                });
            let mut cycle = false;
            for path in &self.stack {
                let work = cadmpeg_core::decode::u64_from_index(path.len())
                    .checked_add(1)
                    .ok_or_else(|| {
                        self.ctx.refuse_codec_limit(
                            "match F3Z reference cycle",
                            u64::MAX - 1,
                            u64::MAX,
                        )
                    })?;
                self.ctx.charge_work(work, "match F3Z reference cycle")?;
                if path == reference.relative_path.as_str() {
                    cycle = true;
                    break;
                }
            }
            if cycle {
                super::push_loss(
                    self.ctx,
                    &mut parent_report.losses,
                    F3dLossCode::XrefCycle,
                    format_args!(
                        "xref {label}: reference cycle through {}; the occurrence was not resolved",
                        reference.relative_path
                    ),
                )?;
                continue;
            }
            let Some(member) = self.archive.members.get(reference.relative_path.as_str()) else {
                if self.scan.entry_view(&reference.relative_path).is_some() {
                    super::push_loss(
                        self.ctx,
                        &mut parent_report.losses,
                        F3dLossCode::XrefMemberUndecoded,
                        format_args!(
                            "xref {label}: member {} is not an F3D document member; the occurrence was not resolved",
                            reference.relative_path
                        ),
                    )?;
                } else {
                    super::push_loss(
                        self.ctx,
                        &mut parent_report.losses,
                        F3dLossCode::XrefMemberMissing,
                        format_args!(
                            "xref {label}: member {} is not present in the archive; the occurrence was not resolved",
                            reference.relative_path
                        ),
                    )?;
                }
                continue;
            };
            let member_scan = match member {
                ClassifiedMember::Scanned(member_scan) => member_scan,
                ClassifiedMember::Unreadable(_) => continue,
            };
            let ctx = self.ctx;
            let _depth = ctx.enter_nested("f3z member reference")?;
            let component = match crate::decode::decode_archive_member(
                self.ctx,
                member_scan,
                &self.archive.layers,
            ) {
                Ok(component) => component.into_decoded(),
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(error) => {
                    super::push_loss(
                        self.ctx,
                        &mut parent_report.losses,
                        F3dLossCode::XrefMemberUndecoded,
                        format_args!(
                            "xref {label}: member {} failed to decode ({error}); the occurrence was not resolved",
                            reference.relative_path
                        ),
                    )?;
                    continue;
                }
            };
            let child_table = xref_table_from_ir(self.ctx, &component.ir)?;
            let cadmpeg_ir::codec::Decoded {
                ir: mut component_ir,
                body: mut component_report,
                source_fidelity: mut component_fidelity,
            } = component;
            self.ctx.push_formatted_retained(
                &mut self.stack,
                format_args!("{}", reference.relative_path),
                "grow F3Z merge stack",
                "retain F3Z merge stack path",
            )?;
            let descendants = self.merge(
                &mut component_ir,
                &mut component_report,
                &mut component_fidelity,
                &child_table,
            )?;
            self.stack.pop();
            if let Some(transform) = reference.transform {
                apply_occurrence_transform(self.ctx, &mut component_ir.model, transform)?;
            }
            append_feature_history(self.ctx, &parent_ir.model, &mut component_ir.model)?;
            let occurrence_start = parent_ir.model.occurrences.len();
            let mut scope = OccurrenceScope {
                ctx: self.ctx,
                occurrence: &occurrence,
            };
            parent_ir.model.extend_rewritten(
                self.ctx,
                component_ir.model,
                &mut scope,
                "F3Z rewritten model arenas",
            )?;
            reparent_component_roots(
                self.ctx,
                &mut parent_ir.model.occurrences[occurrence_start..],
                &crate::ids::neutral_xref_occurrence_id(
                    reference.ordinal,
                    reference.occurrence_ordinal,
                ),
            )?;
            extend_native(
                self.ctx,
                &mut parent_ir.native,
                component_ir.native,
                &occurrence,
            )?;
            parent_fidelity.append(
                self.ctx,
                rescope_fidelity(self.ctx, component_fidelity, &occurrence)?,
            )?;
            merged += descendants + 1;
            if component_report.transfer.geometry_transferred() {
                parent_report.transfer = cadmpeg_ir::report::decode::DecodeTransfer::full(true);
            }
            for loss in &mut component_report.losses {
                loss.message = self.ctx.format_retained(
                    format_args!("xref {label}: {}", loss.message),
                    "prefix F3Z component loss",
                )?;
            }
            self.ctx.append_vec(
                &mut parent_report.losses,
                &mut { component_report.losses },
                "append F3Z report losses",
            )?;
            let placement = if reference.transform.is_some() {
                "Design occurrence transform"
            } else {
                "identity placement"
            };
            self.ctx.push_formatted_retained(&mut parent_report.notes, format_args!(
                    "xref {label}: merged {} as occurrence {occurrence} ({placement}; {descendants} nested occurrence(s))",
                    reference.relative_path
                ), "collect F3Z report notes", "retain F3Z report note")?;
        }
        Ok(merged)
    }
}

/// Places every root-level occurrence from a merged member inside the
/// occurrence that owns that member. Child occurrence parents already carry
/// the member-local hierarchy and are left unchanged.
fn reparent_component_roots(
    ctx: &DecodeContext<'_>,
    occurrences: &mut [cadmpeg_ir::products::Occurrence],
    parent: &cadmpeg_ir::ids::OccurrenceId,
) -> Result<(), CodecError> {
    for occurrence in occurrences {
        if matches!(
            occurrence.parent,
            cadmpeg_ir::products::OccurrenceParent::Root {}
        ) {
            let parent_id = String::from_utf8(ctx.copy_retained(
                parent.as_str().as_bytes(),
                "copy F3Z parent occurrence identity",
            )?)
            .map_err(CodecError::malformed)?;
            occurrence.parent = cadmpeg_ir::products::OccurrenceParent::Occurrence {
                occurrence: cadmpeg_ir::ids::OccurrenceId::mint(parent_id)
                    .map_err(CodecError::malformed)?,
            };
        }
    }
    Ok(())
}

/// Places one component's feature history after the histories already merged.
fn append_feature_history(
    ctx: &DecodeContext<'_>,
    parent: &Model,
    component: &mut Model,
) -> Result<(), CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(component.features.len()),
        "scan F3Z component feature ordinals",
    )?;
    let Some(component_minimum) = component
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .min()
    else {
        return Ok(());
    };
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(parent.features.len()),
        "scan F3Z parent feature ordinals",
    )?;
    let next = parent
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .max()
        .map_or(Ok(0), |ordinal| {
            ordinal.checked_add(1).ok_or_else(|| {
                CodecError::Malformed("merged F3Z feature ordinal exceeds u64::MAX".into())
            })
        })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(component.features.len()),
        "rewrite F3Z feature ordinals",
    )?;
    for feature in &mut component.features {
        feature.ordinal = feature
            .ordinal
            .checked_sub(component_minimum)
            .and_then(|ordinal| ordinal.checked_add(next))
            .ok_or_else(|| {
                CodecError::Malformed("merged F3Z feature ordinal exceeds u64::MAX".into())
            })?;
    }
    Ok(())
}

fn rescope_fidelity(
    ctx: &DecodeContext<'_>,
    source: SourceFidelity,
    occurrence: &str,
) -> Result<SourceFidelity, CodecError> {
    let (mut annotations, records) = source.into_parts();
    annotations
        .map_ids(
            ctx,
            |id| match rescope_charged(ctx, id, occurrence)? {
                Some(id) => Ok(id),
                None => ctx.format_retained(format_args!("{id}"), "copy F3Z annotation identity"),
            },
            "index remapped annotation identities",
        )?
        .map_err(CodecError::from)?;
    // The occurrence is one owner component. Escape its separators so two
    // different occurrences cannot share an owner by shifting a path boundary.
    let owner = cadmpeg_ir::StreamName::try_from(ctx.format_retained(
        format_args!("f3d:xref/{}/", EscapedOccurrenceComponent(occurrence)),
        "retain F3Z fidelity owner",
    )?)
    .map_err(CodecError::malformed)?;
    let provenance = std::mem::take(&mut annotations.provenance);
    let mut builder = AnnotationBuilder::resume(annotations);
    for (id, provenance) in provenance {
        let stream = cadmpeg_ir::StreamName::try_from(ctx.format_retained(
            format_args!("{}{}", owner.as_str(), provenance.stream()),
            "retain F3Z provenance stream",
        )?)
        .map_err(CodecError::malformed)?;
        let stream = StreamHandle::new(ctx, stream, "allocate annotation stream handle")?;
        builder.note(
            ctx,
            &id,
            &stream,
            provenance.offset,
            provenance.tag.as_deref(),
        )?;
    }
    let mut rescoped = SourceFidelity::with_annotations(builder.build());
    for (id, record) in records {
        let id_text = match rescope_charged(ctx, id.as_str(), occurrence)? {
            Some(id) => id,
            None => {
                ctx.format_retained(format_args!("{id}"), "copy F3Z retained record identity")?
            }
        };
        let id = UnknownId::mint(id_text).map_err(|error| {
            CodecError::malformed(format_args!("F3Z retained record {id}: {error}"))
        })?;
        let stream = cadmpeg_ir::StreamName::try_from(ctx.format_retained(
            format_args!("{}{}", owner.as_str(), record.stream()),
            "retain F3Z record stream",
        )?)
        .map_err(CodecError::malformed)?;
        ctx.charge_collection_items(1, "collect F3Z rescoped retained records")?;
        rescoped.insert_retained_record(id, record.with_owner(stream))?;
    }
    Ok(rescoped)
}

fn occurrence_key(
    ctx: &DecodeContext<'_>,
    reference: &XrefReference,
) -> Result<String, CodecError> {
    let role = EscapedOccurrenceComponent(&reference.neutron_role);
    // `occurrence_ordinal` restarts for each Redirections reference. Keep the
    // source reference ordinal in the scope so two admitted rows carrying the
    // same role cannot merge their model or fidelity identities.
    ctx.format_retained(
        format_args!(
            "role-{role}/reference-{}/occurrence-{}",
            reference.ordinal, reference.occurrence_ordinal
        ),
        "retain F3Z occurrence key",
    )
}

struct EscapedOccurrenceComponent<'a>(&'a str);

impl std::fmt::Display for EscapedOccurrenceComponent<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for character in self.0.chars() {
            if matches!(character, ':' | '#' | '%' | '/') || character.is_whitespace() {
                let mut bytes = [0; 4];
                for byte in character.encode_utf8(&mut bytes).as_bytes() {
                    write!(formatter, "%{byte:02X}")?;
                }
            } else {
                write!(formatter, "{character}")?;
            }
        }
        Ok(())
    }
}

fn apply_occurrence_transform(
    ctx: &DecodeContext<'_>,
    model: &mut Model,
    transform: crate::records::xref::XrefPlacementTransform,
) -> Result<(), CodecError> {
    let source_rows = transform.rows();
    let mut rows = [source_rows[0], source_rows[1], source_rows[2]];
    for row in &mut rows {
        row[3] *= 10.0;
    }
    let occurrence = cadmpeg_ir::transform::Transform::affine(rows).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "F3Z occurrence translation is not a finite affine transform"
        ))
    })?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(model.bodies.len())
            .checked_mul(48)
            .ok_or_else(|| ctx.refuse_codec_limit("compose F3Z body transforms", 0, u64::MAX))?,
        "compose F3Z body transforms",
    )?;
    for body in &mut model.bodies {
        body.transform = Some(match body.transform {
            Some(local) => compose_transforms(occurrence, local)?,
            None => occurrence,
        });
    }
    Ok(())
}

/// Composes a component-local transform after its archive occurrence transform.
fn compose_transforms(
    outer: cadmpeg_ir::transform::Transform,
    inner: cadmpeg_ir::transform::Transform,
) -> Result<cadmpeg_ir::transform::Transform, CodecError> {
    outer.compose(inner).map_err(|error| {
        CodecError::malformed(format_args!(
            "F3Z occurrence composition is not a finite affine transform: {error}"
        ))
    })
}

fn rescope_charged(
    ctx: &DecodeContext<'_>,
    text: &str,
    occurrence: &str,
) -> Result<Option<String>, CodecError> {
    text.strip_prefix("f3d:")
        .map(|rest| {
            ctx.format_retained(
                format_args!("f3d:xref/{occurrence}/{rest}"),
                "rescope F3Z identity",
            )
        })
        .transpose()
}

/// Rewrites every `f3d:` identity in one model entity into occurrence scope.
struct OccurrenceScope<'r, 'a> {
    ctx: &'r DecodeContext<'a>,
    occurrence: &'r str,
}

impl EntityRewrite for OccurrenceScope<'_, '_> {
    type Error = CodecError;

    fn rewrite<T: cadmpeg_ir::schema::rewrite::typed::RewriteIdentities>(
        &mut self,
        entity: T,
    ) -> Result<T, CodecError> {
        cadmpeg_ir::schema::rewrite::identities(
            self.ctx,
            "rewrite F3Z model identity",
            entity,
            |id: &str| match rescope_charged(self.ctx, id, self.occurrence)? {
                Some(rewritten) => Ok(rewritten),
                None => self
                    .ctx
                    .copy_retained_text(id, "copy F3Z unchanged identity"),
            },
        )
    }
}

/// Appends all known component-native arenas after occurrence-local rescoping.
fn extend_native(
    ctx: &DecodeContext<'_>,
    root: &mut Native,
    mut component: Native,
    occurrence: &str,
) -> Result<(), CodecError> {
    let Some(mut source) = component.0.remove("f3d") else {
        return Ok(());
    };
    let target = root.namespace_mut("f3d");
    for name in crate::native::F3D_ARENA_NAMES
        .iter()
        .copied()
        .chain(std::iter::once("unknowns"))
    {
        let Some(records) = source.arenas_mut().remove(name) else {
            continue;
        };
        if records.is_empty() {
            continue;
        }
        let arena = target.arenas_mut().entry(name.to_string()).or_default();
        ctx.reserve_vec(arena, records.len(), "append F3Z native records")?;
        for record in records {
            arena.push(rescope_record(ctx, &record, name, occurrence)?);
        }
    }
    Ok(())
}

/// Rescopes one native record's identity and every identity it references.
fn rescope_record(
    ctx: &DecodeContext<'_>,
    record: &NativeRecord,
    arena: &str,
    occurrence: &str,
) -> Result<NativeRecord, CodecError> {
    let mut fields = typed_fields(ctx, record, arena, occurrence)?;
    rescope_native_reference_fields(ctx, arena, &mut fields, occurrence)?;
    let id = rescope_charged(ctx, record.id(), occurrence)?.map_or_else(
        || ctx.format_retained(format_args!("{}", record.id()), "copy F3Z native identity"),
        Ok,
    )?;
    NativeRecord::new(
        cadmpeg_ir::ids::Identity::new(id).map_err(CodecError::malformed)?,
        fields,
    )
    .map_err(CodecError::from)
}

/// Rewrite typed identity markers before JSON erases their ownership.
///
/// Most F3D native records carry source text and numeric stream facts. The
/// records listed here also carry an IR identity marker inside a structured
/// field. Going through the typed owner keeps that marker distinct from an
/// ordinary `String` while the field-specific pass below handles native text
/// references that have not yet gained a newtype.
fn typed_fields(
    ctx: &DecodeContext<'_>,
    record: &NativeRecord,
    arena: &str,
    occurrence: &str,
) -> Result<Map<String, Value>, CodecError> {
    macro_rules! typed {
        ($type:path) => {
            record.rewrite_fields::<$type, _>(ctx, |id| {
                match rescope_charged(ctx, id, occurrence)? {
                    Some(rewritten) => Ok(rewritten),
                    None => ctx.copy_retained_text(id, "copy F3Z unchanged native identity"),
                }
            })?
        };
    }

    Ok(match arena {
        "body_visibilities" => typed!(crate::records::bodies::BodyVisibility),
        "creation_timestamps" => typed!(crate::records::recipes::CreationTimestamp),
        "design_body_bindings" => typed!(crate::records::bodies::DesignBodyBinding),
        "design_body_recipe_operands" => {
            typed!(crate::records::topology::body_recipe::DesignBodyRecipeOperand)
        }
        "design_dimension_recipe_records" => {
            typed!(crate::records::dimensions::DesignDimensionRecipeRecord)
        }
        "design_edge_operands" => {
            typed!(crate::records::topology::edge_identity::DesignEdgeOperand)
        }
        "design_edge_treatment_vertex_operands" => {
            typed!(crate::records::feature::work_geometry::DesignEdgeTreatmentVertexOperand)
        }
        "design_face_operands" => typed!(crate::records::topology::face::DesignFaceOperand),
        "design_mesh_features" => typed!(crate::records::mesh::DesignMeshFeature),
        "design_parameter_scopes" => typed!(crate::records::feature::scope::DesignParameterScope),
        "persistent_design_links" => typed!(crate::records::sketch_links::PersistentDesignLink),
        "persistent_subentity_tags" => typed!(crate::records::sketch_links::PersistentSubentityTag),
        "sketch_curve_links" => typed!(crate::records::sketch_links::SketchCurveLink),
        _ => record.copy_fields(ctx)?,
    })
}

/// Rewrite only fields whose owners document an identity relationship.
///
/// Native records intentionally retain arbitrary source strings, including
/// configuration extensions and names that can happen to begin with `f3d:`.
/// Field names are therefore the admission boundary: this list is assembled
/// from the native record definitions and their readers, and no map key or
/// unowned string is traversed as an identity.
fn rescope_native_reference_fields(
    ctx: &DecodeContext<'_>,
    arena: &str,
    fields: &mut Map<String, Value>,
    occurrence: &str,
) -> Result<(), CodecError> {
    let direct_fields: &[&str] = match arena {
        "asm_bulletin_boards"
        | "asm_delta_states"
        | "asm_entity_changes"
        | "asm_history_records" => &["parent"],
        "body_native_keys" => &["body"],
        "edge_continuities" => &["edge"],
        "edge_ownerships" => &["edge", "owner_coedge"],
        "face_native_keys" | "face_sidedness" => &["face"],
        "design_body_bounds" => &["body_binding_ids"],
        "design_body_recipe_operands" | "design_edge_operands" | "design_face_operands" => {
            &["recipe_id"]
        }
        "design_dimension_recipe_records" => &["recipe_id", "matching_edge_operand_ids"],
        "design_construction_operand_groups" => &["lost_edge_references"],
        "design_extrude_selection_members" => &["operand_identity_ids"],
        "design_edge_identity_operands" => &["resolution_identity_id"],
        "design_parameter_companions" => &["owned_recipe_ids"],
        "mesh_surface_sentinels" => &["surface"],
        "tolerant_coedge_parameters" => &["coedge"],
        "tolerant_edge_tails" => &["edge"],
        "tolerant_vertex_tails" => &["vertex"],
        "transform_hints" => &["body"],
        "unknowns" => &["links"],
        "vertex_ownerships" => &["owning_edge", "vertex"],
        "wire_topologies" => &["edges", "free_vertex", "shell"],
        _ => &[],
    };
    for field in direct_fields {
        if let Some(value) = fields.get_mut(*field) {
            scope_identity_value(ctx, value, occurrence)?;
        }
    }

    // These records carry history-qualified selection proofs as nested
    // structs. Their `history_id` is a native record identity; the adjacent
    // historical slots are numeric source facts and remain unchanged.
    if matches!(
        arena,
        "design_entity_selection_operands"
            | "design_edge_identity_operands"
            | "design_extrude_selection_members"
    ) {
        scope_named_fields(ctx, fields, &["history_id"], occurrence)?;
    }

    // WorkPoint and mesh records contain native identities as ordinary strings
    // inside their nested envelopes. Their surrounding payloads also carry
    // source text, so the walker is restricted to the field names owned by the
    // corresponding native relations.
    match arena {
        "design_edge_treatment_vertex_operands" => {
            scope_named_fields(ctx, fields, &["recipe_id"], occurrence)?;
        }
        "design_mesh_features" => {
            scope_named_fields(ctx, fields, &["tessellation_id"], occurrence)?;
        }
        "design_parameter_scopes" => {
            scope_named_fields(
                ctx,
                fields,
                &["history_id", "operand_id", "point_native_id", "recipe_id"],
                occurrence,
            )?;
        }
        _ => {}
    }
    Ok(())
}

fn scope_identity_value(
    ctx: &DecodeContext<'_>,
    value: &mut Value,
    occurrence: &str,
) -> Result<(), CodecError> {
    match value {
        Value::String(text) => {
            if let Some(rescoped) = rescope_charged(ctx, text, occurrence)? {
                *text = rescoped;
            }
        }
        Value::Array(items) => {
            for item in items {
                if let Value::String(text) = item {
                    if let Some(rescoped) = rescope_charged(ctx, text, occurrence)? {
                        *text = rescoped;
                    }
                }
            }
        }
        // `AttributeTarget` is the only selected object field. Its `id` is a
        // typed identity and all other members are its discriminator/value.
        Value::Object(fields) => {
            if let Some(Value::String(text)) = fields.get_mut("id") {
                if let Some(rescoped) = rescope_charged(ctx, text, occurrence)? {
                    *text = rescoped;
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

fn scope_named_fields(
    ctx: &DecodeContext<'_>,
    fields: &mut Map<String, Value>,
    names: &[&str],
    occurrence: &str,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("walk F3Z native identity fields")?;
    for (name, value) in fields {
        ctx.charge_work(1, "inspect F3Z native identity field")?;
        if names.contains(&name.as_str()) {
            scope_identity_value(ctx, value, occurrence)?;
        } else {
            scope_named_values(ctx, value, names, occurrence)?;
        }
    }
    Ok(())
}

fn scope_named_values(
    ctx: &DecodeContext<'_>,
    value: &mut Value,
    names: &[&str],
    occurrence: &str,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("walk F3Z native identity values")?;
    match value {
        Value::Object(fields) => scope_named_fields(ctx, fields, names, occurrence)?,
        Value::Array(items) => {
            for item in items {
                scope_named_values(ctx, item, names, occurrence)?;
            }
        }
        Value::String(_) | Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    mod fidelity;
    mod occurrence;

    use super::{
        apply_occurrence_transform, compose_transforms, make_sibling_ordinals_unique,
        merge_archive, xref_table_from_ir,
    };
    use cadmpeg_ir::document::Model;

    fn root_occurrence(id: &str, ordinal: u32) -> cadmpeg_ir::products::Occurrence {
        cadmpeg_ir::products::Occurrence {
            id: cadmpeg_ir::ids::OccurrenceId::mint(id).unwrap(),
            prototype: cadmpeg_ir::products::PrototypeReference::Unresolved {},
            parent: cadmpeg_ir::products::OccurrenceParent::Root {},
            ordinal,
            transform: cadmpeg_ir::transform::Transform::identity(),
            linked_prototype: None,
            scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: None,
        }
    }

    #[test]
    fn f3z_merge_stack_refuses_collection_limit() {
        let bytes = crate::test_support::zip_test::synthetic_f3d(false);
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let normal_policy = cadmpeg_core::decode::DecodePolicy::service();
        let (normal, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &normal_policy)
                .unwrap();
        let scan = crate::container::scan(&normal, root).unwrap();
        let archive = super::ArchiveSession {
            members: std::collections::BTreeMap::new(),
            layers: cadmpeg_core::dialect::DialectLayers::of(scan.kind.dialect().clone()),
            losses: Vec::new(),
        };
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (limited, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut report = cadmpeg_ir::codec::DecodeBody::new(
            cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
        );
        let mut fidelity = cadmpeg_ir::SourceFidelity::default();
        let error = merge_archive(
            &limited,
            &scan,
            &archive,
            "root.f3d".into(),
            &mut ir,
            &mut report,
            &mut fidelity,
        )
        .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "seed F3Z merge stack")
        );
    }

    #[test]
    fn f3z_sibling_parent_index_refuses_collection_limit() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut occurrences = [root_occurrence("f3d:model:occurrence#0", 0)];
        let error = make_sibling_ordinals_unique(&ctx, &mut occurrences).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3Z sibling parents")
        );
    }

    #[test]
    fn f3z_sibling_ordinal_index_refuses_collection_limit() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut occurrences = [root_occurrence("f3d:model:occurrence#0", 0)];
        let error = make_sibling_ordinals_unique(&ctx, &mut occurrences).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "index F3Z sibling ordinals")
        );
    }

    #[test]
    fn f3z_sibling_reassignment_refuses_work_limit() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 2;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut occurrences = [
            root_occurrence("f3d:model:occurrence#0", 0),
            root_occurrence("f3d:model:occurrence#1", 0),
        ];
        let error = make_sibling_ordinals_unique(&ctx, &mut occurrences).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "find F3Z sibling ordinal")
        );
    }

    #[test]
    fn f3z_xref_native_reload_refuses_collection_limit() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let normal_policy = cadmpeg_core::decode::DecodePolicy::default();
        let normal =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &normal_policy)
                .unwrap()
                .0;
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.native
            .namespace_mut("f3d")
            .set_arena(
                &normal,
                "xref_designs",
                &[crate::records::xref::XrefDesign {
                    id: "f3d:xref:design#0".into(),
                    ordinal: 0,
                    file_version: 1,
                    target_file_name: "part.f3d".to_owned().try_into().unwrap(),
                    display_name: "Part".into(),
                    lineage_urn: "lineage".into(),
                    version_urn: "version".into(),
                }],
            )
            .unwrap();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let limited = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .unwrap()
            .0;
        let error = xref_table_from_ir(&limited, &ir).unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "load typed native record")
        );
    }

    #[test]
    fn occurrence_translation_overflow_is_rejected() {
        let rows = [
            [1.0, 0.0, 0.0, f64::MAX],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let source = cadmpeg_ir::transform::Transform::affine(rows).unwrap();
        let source = crate::records::xref::XrefPlacementTransform::try_from(source.rows()).unwrap();
        let error = crate::test_support::with_decode_context(|ctx| {
            apply_occurrence_transform(ctx, &mut Model::default(), source)
        })
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("F3Z occurrence translation is not a finite affine transform"));
    }

    #[test]
    fn occurrence_composition_overflow_is_rejected() {
        let transform = cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, f64::MAX],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap();
        let error = compose_transforms(transform, transform).unwrap_err();
        assert!(error
            .to_string()
            .contains("F3Z occurrence composition is not a finite affine transform"));
    }
}
