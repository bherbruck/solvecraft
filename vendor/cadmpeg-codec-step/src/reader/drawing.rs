// SPDX-License-Identifier: Apache-2.0
//! STEP drawing definitions, revisions, sheets, views, and their relations.

use crate::ids::kind;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::drawings::{Drawing, DrawingId, DrawingKind};
use cadmpeg_ir::ids::{Identity, ProductDefinitionId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{NativeField, NativeRecord};
use cadmpeg_ir::{ReferenceSelection, ReferenceTarget};

use crate::ids;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, ReferenceName, Value};

use super::claims::SourceClaims;
use super::representation;
use super::ValueExt;
use super::{decode_text_charged, opaque_record_id, record_targets, StageOutcome};

const DRAWING_ASSOCIATION_TYPES: &[&str] = &[
    "DRAUGHTING_MODEL_ITEM_ASSOCIATION",
    "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER",
];

struct TargetContext<'a> {
    target_identities: &'a BTreeMap<u64, BTreeSet<String>>,
    known_typed: &'a dyn SourceClaims,
    exchange: &'a Exchange,
    external_documents: &'a BTreeMap<u64, &'a str>,
    ctx: &'a DecodeContext<'a>,
}

struct DrawingCandidate<'a> {
    id: u64,
    name: &'static str,
    identity: Identity,
    offset: usize,
    parameters: DrawingParameters<'a>,
}

#[derive(Clone, Copy)]
struct DrawingParameters<'a> {
    inherited_name: Option<&'a Value>,
    direct: &'a [Value],
}

impl<'a> DrawingParameters<'a> {
    fn from_slice(direct: &'a [Value]) -> Self {
        Self {
            inherited_name: None,
            direct,
        }
    }

    fn len(self) -> usize {
        self.direct.len() + usize::from(self.inherited_name.is_some())
    }

    fn get(self, index: usize) -> Option<&'a Value> {
        match self.inherited_name {
            Some(name) if index == 0 => Some(name),
            Some(_) => index
                .checked_sub(1)
                .and_then(|index| self.direct.get(index)),
            None => self.direct.get(index),
        }
    }

    fn first(self) -> Option<&'a Value> {
        self.get(0)
    }

    fn iter(self) -> impl Iterator<Item = &'a Value> {
        self.inherited_name.into_iter().chain(self.direct.iter())
    }
}

enum TargetResolution {
    Resolved(ReferenceSelection),
    Ambiguous(BTreeSet<String>),
    Unresolved,
}

fn ensure_drawing_relationship_group(
    relationships: &mut BTreeMap<NonBlankString, Vec<ReferenceSelection>>,
    role: NonBlankString,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if !relationships.contains_key(&role) {
        ctx.insert_btree_map(
            relationships,
            role,
            Vec::new(),
            "step_drawing_relationship_groups",
        )?;
    }
    Ok(())
}

fn clone_drawing_identities(
    source: &BTreeSet<String>,
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<String>, CodecError> {
    let mut copy = BTreeSet::new();
    for identity in source {
        let text = ctx.copy_retained_text(identity, "step_drawing_ambiguous_identity_text")?;
        ctx.insert_btree_set(&mut copy, text, "step_drawing_ambiguous_identity_copy")?;
    }
    Ok(copy)
}

fn visit_drawing_references(
    value: &Value,
    ctx: &DecodeContext<'_>,
    visitor: &mut impl FnMut(u64) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let _nested = ctx.enter_nested("step_drawing_reference_walk")?;
    match value {
        Value::Reference(id) => visitor(*id)?,
        Value::List(values) => {
            for value in values {
                visit_drawing_references(value, ctx, visitor)?;
            }
        }
        Value::Typed(_, value) => visit_drawing_references(value, ctx, visitor)?,
        _ => {}
    }
    Ok(())
}

impl TargetContext<'_> {
    fn resolve(&self, id: u64) -> Result<TargetResolution, CodecError> {
        target_resolution(
            id,
            self.target_identities,
            self.known_typed,
            self.exchange,
            self.external_documents,
            self.ctx,
        )
    }
}

/// Decode the drawing object graph without claiming unsupported graphics.
pub(super) fn decode(
    exchange: &Exchange,
    ir: &mut CadIr,
    known_typed: &dyn SourceClaims,
    product_definition_ids_by_shape: &BTreeMap<u64, ProductDefinitionId>,
    ctx: &DecodeContext<'_>,
) -> Result<StageOutcome<()>, CodecError> {
    let mut losses = Vec::new();
    let mut candidates = Vec::new();
    for (&id, record) in exchange.records() {
        let Some((name, kind)) = drawing_type(record) else {
            continue;
        };
        let parameters = source_parameters(record, name);
        if required_parameter_count(name).is_some_and(|count| parameters.len() < count) {
            ctx.reserve_vec(&mut losses, 1, "step_drawing_losses")?;
            losses.push(StepLossCode::DrawingRecordTooFewParameters.note(format!(
                "STEP drawing record #{id} has too few {name} parameters and was retained opaque"
            )));
            continue;
        }
        ctx.reserve_vec(&mut candidates, 1, "step_drawing_candidates")?;
        candidates.push(DrawingCandidate {
            id,
            name,
            identity: ids::drawing(kind, id),
            offset: record.span.start,
            parameters,
        });
    }
    ctx.stable_sort_by(
        &mut candidates,
        |left, right| left.offset.cmp(&right.offset),
        |_| 0,
        "step_drawing_candidates_sort",
    )?;

    if candidates.is_empty() {
        return Ok(StageOutcome {
            value: (),
            claims: HashSet::new(),
            losses,
            notes: Vec::new(),
        });
    }

    let mut drawing_ids = BTreeSet::new();
    for candidate in &candidates {
        ctx.insert_btree_set(&mut drawing_ids, candidate.id, "step_drawing_ids")?;
    }
    let mut hidden_drawing_ids = BTreeSet::new();
    for record in exchange.records().values() {
        let Some(items) = record
            .partials
            .iter()
            .find(|partial| partial.name == "INVISIBILITY")
            .and_then(|partial| partial.parameters.first())
        else {
            continue;
        };
        visit_drawing_references(items, ctx, &mut |id| {
            if drawing_ids.contains(&id) {
                ctx.insert_btree_set(&mut hidden_drawing_ids, id, "step_hidden_drawing_ids")?;
            }
            Ok(())
        })?;
    }

    let mut target_identities =
        record_targets(ir, |record_id| known_typed.contains(record_id), ctx)?;
    for candidate in &candidates {
        ctx.admit_btree_entry(
            &target_identities,
            &candidate.id,
            "step_drawing_target_groups",
        )?;
        let targets = target_identities.entry(candidate.id).or_default();
        ctx.insert_btree_set(
            targets,
            ctx.copy_retained_text(
                candidate.identity.as_str(),
                "step_drawing_target_member_text",
            )?,
            "step_drawing_target_members",
        )?;
    }
    // DR-01: a drawing association scoped by PRODUCT_DEFINITION_SHAPE targets
    // that shape's one owning product-definition view, not a product-wide
    // identity set.
    for (&shape_id, product_definition_id) in product_definition_ids_by_shape {
        ctx.admit_btree_entry(&target_identities, &shape_id, "step_drawing_target_groups")?;
        let targets = target_identities.entry(shape_id).or_default();
        ctx.insert_btree_set(
            targets,
            ctx.copy_retained_text(
                product_definition_id.as_str(),
                "step_drawing_target_member_text",
            )?,
            "step_drawing_target_members",
        )?;
    }
    let drawing_target_ids = referenced_target_ids(exchange, &candidates, ctx)?;
    add_source_typed_targets(
        ir,
        exchange,
        known_typed,
        &drawing_target_ids,
        &mut target_identities,
        ctx,
    )?;
    let mut external_documents = BTreeMap::new();
    for entry in exchange.references() {
        if let ReferenceName::Entity(id) = entry.name {
            ctx.admit_btree_entry(&external_documents, &id, "step_drawing_external_documents")?;
            external_documents.insert(id, entry.uri.as_str());
        }
    }
    let target_context = TargetContext {
        target_identities: &target_identities,
        known_typed,
        exchange,
        external_documents: &external_documents,
        ctx,
    };

    let mut drawings = BTreeMap::<u64, Drawing>::new();
    for (order, candidate) in candidates.into_iter().enumerate() {
        let DrawingCandidate {
            id,
            name,
            identity,
            parameters,
            ..
        } = candidate;
        let mut stored_parameters = BTreeMap::new();
        ctx.insert_btree_map(
            &mut stored_parameters,
            cadmpeg_core::nonblank_literal!("source_id"),
            format!("#{id}"),
            "step_drawing_stored_parameters",
        )?;
        ctx.insert_btree_map(
            &mut stored_parameters,
            cadmpeg_core::nonblank_literal!("source_type"),
            name.into(),
            "step_drawing_stored_parameters",
        )?;
        for (index, value) in parameters.iter().enumerate() {
            if let Some(value) = value_text(
                exchange,
                value,
                &mut losses,
                id,
                &format!("drawing parameter {index}"),
                ctx,
            )? {
                let key = parameter_key(name, index);
                ctx.admit_btree_entry(&stored_parameters, &key, "step_drawing_stored_parameters")?;
                stored_parameters.insert(key, value);
            }
        }

        let Some(order) = cadmpeg_core::decode::id_from_index(order) else {
            ctx.reserve_vec(&mut losses, 1, "step_drawing_losses")?;
            losses.push(StepLossCode::DrawingOrderUnstatable.note(format!(
                "drawing #{id} position in the stored order exceeds the stated order width"
            )));
            continue;
        };

        let mut relationships = BTreeMap::new();
        add_reference_fields(
            &mut relationships,
            name,
            parameters,
            id,
            &target_context,
            &mut losses,
        )?;
        ctx.admit_btree_entry(&drawings, &id, "step_drawing_entries")?;
        drawings.insert(
            id,
            Drawing {
                id: DrawingId::from(
                    identity.try_clone_for_decode(ctx, "step_drawing_identity_copy")?,
                ),
                object: ctx.copy_retained_text(identity.as_str(), "step_drawing_object_copy")?,
                kind: drawing_kind(name),
                runtime_type: name.into(),
                order,
                visible: hidden_drawing_ids.contains(&id).then_some(false),
                relationships,
                template: None,
                position: None,
                scale: None,
                direction: None,
                rotation_degrees: None,
                parameters: stored_parameters,
                assets: Vec::new(),
                native_ref: identity.into_string(),
            },
        );
    }

    add_sheet_revision_usages(exchange, &mut drawings, &target_context, &mut losses, ctx)?;
    let mut association_ids = HashSet::new();
    add_draughting_model_associations(
        exchange,
        &mut drawings,
        &target_context,
        &mut losses,
        &mut association_ids,
    )?;

    let mut typed_records = HashSet::new();
    for &id in drawings.keys() {
        ctx.insert_hash_set(&mut typed_records, id, "step_drawing_typed_claims")?;
    }
    for id in association_ids {
        ctx.insert_hash_set(&mut typed_records, id, "step_drawing_typed_claims")?;
    }
    ctx.reserve_vec(
        &mut ir.model.drawings,
        drawings.len(),
        "step_drawing_ir_items",
    )?;
    ir.model.drawings.extend(drawings.into_values());
    Ok(StageOutcome {
        value: (),
        claims: typed_records,
        losses,
        notes: Vec::new(),
    })
}

pub(super) fn is_supported_invisibility_target(record: &RawRecord) -> bool {
    let Some((name, _)) = drawing_type(record) else {
        return false;
    };
    required_parameter_count(name)
        .is_none_or(|count| source_parameters(record, name).len() >= count)
}

fn referenced_target_ids(
    exchange: &Exchange,
    candidates: &[DrawingCandidate<'_>],
    ctx: &DecodeContext<'_>,
) -> Result<BTreeSet<u64>, CodecError> {
    let mut ids = BTreeSet::new();
    for candidate in candidates {
        for &index in relationship_indices(candidate.name) {
            if let Some(value) = candidate.parameters.get(index) {
                collect_reference_ids(value, &mut ids, ctx)?;
            }
        }
    }
    for (_, record) in exchange.entities("DRAWING_SHEET_REVISION_USAGE") {
        let parameters = source_parameters(record, "DRAWING_SHEET_REVISION_USAGE");
        for value in parameters.iter().take(2) {
            collect_reference_ids(value, &mut ids, ctx)?;
        }
    }
    for association_id in
        exchange.matching_entity_ids(|name| DRAWING_ASSOCIATION_TYPES.contains(&name))
    {
        let Some(record) = exchange.records().get(&association_id) else {
            continue;
        };
        let Some(parameters) = association_parameters(record) else {
            continue;
        };
        for index in [2, 4] {
            if let Some(value) = parameters.get(index) {
                collect_reference_ids(value, &mut ids, ctx)?;
            }
        }
        if record
            .partials
            .iter()
            .any(|partial| partial.name == "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER")
        {
            if let Some(placeholder_id) = association_placeholder_reference(record, parameters) {
                ctx.insert_btree_set(&mut ids, placeholder_id, "step_drawing_referenced_targets")?;
            }
        }
    }
    Ok(ids)
}

fn collect_reference_ids(
    value: &Value,
    output: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    visit_drawing_references(value, ctx, &mut |id| {
        ctx.insert_btree_set(output, id, "step_drawing_referenced_targets")
            .map(|_| ())
    })
}

fn add_source_typed_targets(
    ir: &mut CadIr,
    exchange: &Exchange,
    known_typed: &dyn SourceClaims,
    referenced_ids: &BTreeSet<u64>,
    target_identities: &mut BTreeMap<u64, BTreeSet<String>>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut native_targets = Vec::new();
    for &id in referenced_ids {
        if !known_typed.contains(id) || target_identities.contains_key(&id) {
            continue;
        }
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        if wrapper_target_resolution(id, target_identities, exchange, ctx)?.is_some() {
            continue;
        }
        let identity = opaque_record_id(id, record, ctx)?;
        let copied_identity = ctx.copy_retained_text(
            identity.as_str(),
            "step_drawing_native_target_identity_copy",
        )?;
        let source_type = ctx.join_display_retained(
            record.partials.iter().map(|partial| partial.name.as_str()),
            "+",
            "step_drawing_source_type_text",
        )?;
        ctx.reserve_vec(&mut native_targets, 1, "step_drawing_native_target_items")?;
        native_targets.push(NativeRecord::from_identity(
            identity,
            [
                ("source_id".to_owned(), NativeField::Text(format!("#{id}"))),
                ("source_type".to_owned(), NativeField::Text(source_type)),
            ],
        ));
        ctx.admit_btree_entry(target_identities, &id, "step_drawing_native_target_groups")?;
        ctx.charge_collection_items(1, "step_drawing_native_target_members")?;
        target_identities.insert(id, BTreeSet::from([copied_identity]));
    }
    if native_targets.is_empty() {
        return Ok(());
    }
    let namespace = ir.native.namespace_mut("step");
    let arenas = namespace.arenas_mut();
    let arena_key = String::from("drawing_targets");
    ctx.admit_btree_entry(arenas, &arena_key, "step_drawing_native_arena")?;
    let target_arena = arenas.entry(arena_key).or_default();
    ctx.reserve_vec(
        target_arena,
        native_targets.len(),
        "step_drawing_native_arena_items",
    )?;
    target_arena.extend(native_targets);
    Ok(())
}

/// Each drawing entity name and the identity kind that name spells.
///
/// The pairing is the type that makes the drawing mint path total: a drawing
/// record reaches [`ids::drawing`] with a kind that came from this table, so
/// no identity kind is ever derived from record text at run time.
fn drawing_entities() -> [(&'static str, &'static crate::ids::IdentityKind); 7] {
    [
        ("DRAWING_DEFINITION", kind!("drawing_definition")),
        ("DRAWING_REVISION", kind!("drawing_revision")),
        ("DRAWING_SHEET_REVISION", kind!("drawing_sheet_revision")),
        ("PRESENTATION_VIEW", kind!("presentation_view")),
        ("PRESENTATION_SIZE", kind!("presentation_size")),
        ("DRAUGHTING_MODEL", kind!("draughting_model")),
        ("DRAUGHTING_CALLOUT", kind!("draughting_callout")),
    ]
}

fn drawing_type(record: &RawRecord) -> Option<(&'static str, &'static crate::ids::IdentityKind)> {
    drawing_entities()
        .into_iter()
        .find(|(name, _)| record.partials.iter().any(|partial| partial.name == *name))
}

fn drawing_kind(name: &str) -> DrawingKind {
    match name {
        "DRAWING_SHEET_REVISION" => DrawingKind::Page,
        "PRESENTATION_VIEW" => DrawingKind::View,
        "DRAUGHTING_CALLOUT" => DrawingKind::Annotation,
        _ => DrawingKind::Other,
    }
}

fn required_parameter_count(name: &str) -> Option<usize> {
    match name {
        "DRAWING_DEFINITION" => Some(2),
        "DRAWING_REVISION" => Some(3),
        "DRAWING_SHEET_REVISION" => Some(4),
        "PRESENTATION_VIEW" => Some(3),
        "PRESENTATION_SIZE" => Some(2),
        "DRAUGHTING_MODEL" => Some(3),
        "DRAUGHTING_CALLOUT" => Some(2),
        _ => None,
    }
}

fn source_parameters<'a>(record: &'a RawRecord, name: &str) -> DrawingParameters<'a> {
    let direct = record
        .partials
        .iter()
        .find(|partial| partial.name == name)
        .map(|partial| partial.parameters.as_slice());
    if name == "DRAUGHTING_CALLOUT" {
        if let Some(parameters) = direct.filter(|parameters| parameters.len() >= 2) {
            return DrawingParameters::from_slice(parameters);
        }
        let inherited_name = record
            .partials
            .iter()
            .find(|partial| partial.name == "REPRESENTATION_ITEM")
            .and_then(|partial| partial.parameters.first());
        return DrawingParameters {
            inherited_name,
            direct: direct.unwrap_or_default(),
        };
    }
    if let Some(parameters) = direct.filter(|parameters| !parameters.is_empty()) {
        return DrawingParameters::from_slice(parameters);
    }
    if matches!(
        name,
        "DRAUGHTING_MODEL" | "PRESENTATION_VIEW" | "DRAWING_SHEET_REVISION"
    ) {
        if let Some(parameters) = representation::parameters(record) {
            return DrawingParameters::from_slice(parameters);
        }
    }
    DrawingParameters::from_slice(direct.unwrap_or_default())
}

fn parameter_key(name: &str, index: usize) -> NonBlankString {
    match (name, index) {
        ("DRAWING_DEFINITION", 0) => cadmpeg_core::nonblank_literal!("name"),
        ("DRAWING_DEFINITION", 1) => cadmpeg_core::nonblank_literal!("description"),
        ("DRAWING_REVISION", 0) => cadmpeg_core::nonblank_literal!("name"),
        ("DRAWING_REVISION", 1) => cadmpeg_core::nonblank_literal!("drawing"),
        ("DRAWING_REVISION", 2) => cadmpeg_core::nonblank_literal!("description"),
        ("DRAWING_SHEET_REVISION", 0) => cadmpeg_core::nonblank_literal!("name"),
        ("DRAWING_SHEET_REVISION", 1) => cadmpeg_core::nonblank_literal!("items"),
        ("DRAWING_SHEET_REVISION", 2) => cadmpeg_core::nonblank_literal!("presentation_context"),
        ("DRAWING_SHEET_REVISION", 3) => cadmpeg_core::nonblank_literal!("revision"),
        ("PRESENTATION_VIEW", 0) => cadmpeg_core::nonblank_literal!("name"),
        ("PRESENTATION_VIEW", 1) => cadmpeg_core::nonblank_literal!("items"),
        ("PRESENTATION_VIEW", 2) => cadmpeg_core::nonblank_literal!("presentation_context"),
        ("PRESENTATION_SIZE", 0) => cadmpeg_core::nonblank_literal!("drawing_sheet_revision"),
        ("PRESENTATION_SIZE", 1) => cadmpeg_core::nonblank_literal!("size"),
        ("DRAUGHTING_MODEL", 0) => cadmpeg_core::nonblank_literal!("name"),
        ("DRAUGHTING_MODEL", 1) => cadmpeg_core::nonblank_literal!("items"),
        ("DRAUGHTING_MODEL", 2) => cadmpeg_core::nonblank_literal!("presentation_context"),
        ("DRAUGHTING_CALLOUT", 0) => cadmpeg_core::nonblank_literal!("name"),
        ("DRAUGHTING_CALLOUT", 1) => cadmpeg_core::nonblank_literal!("contents"),
        _ => cadmpeg_core::nonblank_literal!("parameter_{index}"),
    }
}

fn relationship_indices(name: &str) -> &'static [usize] {
    match name {
        "DRAWING_REVISION" | "DRAUGHTING_CALLOUT" => &[1],
        "DRAWING_SHEET_REVISION" => &[1, 2, 3],
        "PRESENTATION_VIEW" | "DRAUGHTING_MODEL" => &[1, 2],
        "PRESENTATION_SIZE" => &[0, 1],
        _ => &[],
    }
}

fn add_reference_fields(
    relationships: &mut BTreeMap<NonBlankString, Vec<ReferenceSelection>>,
    name: &str,
    parameters: DrawingParameters<'_>,
    source_id: u64,
    target_context: &TargetContext<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    for &index in relationship_indices(name) {
        let Some(value) = parameters.get(index) else {
            continue;
        };
        let role = parameter_key(name, index);
        visit_drawing_references(value, target_context.ctx, &mut |target_id| {
            match target_context.resolve(target_id)? {
                TargetResolution::Resolved(target) => {
                    (target_context.ctx).push_btree_group(
                        relationships,
                        role.clone(),
                        target,
                        "step_drawing_relationship_groups",
                        "step_drawing_relationship_members",
                    )?;
                }
                TargetResolution::Ambiguous(identities) => note_ambiguous_target(
                    losses,
                    &format!("drawing #{source_id} {name}"),
                    role.as_str(),
                    target_id,
                    &identities,
                    target_context.ctx,
                )?,
                TargetResolution::Unresolved => {
                    target_context
                        .ctx
                        .reserve_vec(losses, 1, "step_drawing_losses")?;
                    losses.push(StepLossCode::DrawingRelationshipUntypedTarget.note(format!(
                        "STEP drawing #{source_id} {name} relationship {role} references source-typed record #{target_id} without a neutral identity; the raw source parameter is retained"
                    )));
                }
            }
            Ok(())
        })?;
    }
    Ok(())
}

fn note_ambiguous_target(
    losses: &mut Vec<LossNote>,
    source: &str,
    role: &str,
    target_id: u64,
    identities: &BTreeSet<String>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let identities = ctx.join_display_retained(
        identities.iter().map(String::as_str),
        ", ",
        "step_drawing_ambiguous_identities_text",
    )?;
    ctx.reserve_vec(losses, 1, "step_drawing_losses")?;
    let message = ctx.format_retained(format_args!("STEP {source} relationship {role} references source record #{target_id} with multiple neutral identities ({identities}); no target was selected and the raw source parameter is retained"), "step_drawing_ambiguous_loss_text")?;
    losses.push(StepLossCode::DrawingRelationshipTargetAmbiguous.note(message));
    Ok(())
}

fn add_sheet_revision_usages(
    exchange: &Exchange,
    drawings: &mut BTreeMap<u64, Drawing>,
    target_context: &TargetContext<'_>,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    for (usage_id, record) in exchange.entities("DRAWING_SHEET_REVISION_USAGE") {
        let parameters = source_parameters(record, "DRAWING_SHEET_REVISION_USAGE");
        let Some(sheet_id) = parameters.first().and_then(ValueExt::reference) else {
            continue;
        };
        let Some(revision_id) = parameters.get(1).and_then(ValueExt::reference) else {
            continue;
        };
        let sheet_target = target_context.resolve(revision_id)?;
        let revision_target = target_context.resolve(sheet_id)?;
        if let Some(sheet) = drawings.get_mut(&sheet_id) {
            match sheet_target {
                TargetResolution::Resolved(target) => (target_context.ctx).push_btree_group(
                    &mut sheet.relationships,
                    cadmpeg_core::nonblank_literal!("drawing_revision"),
                    target,
                    "step_drawing_relationship_groups",
                    "step_drawing_relationship_members",
                )?,
                TargetResolution::Ambiguous(identities) => note_ambiguous_target(
                    losses,
                    &format!("drawing sheet #{sheet_id} usage #{usage_id}"),
                    "drawing_revision",
                    revision_id,
                    &identities,
                    target_context.ctx,
                )?,
                TargetResolution::Unresolved => {
                    target_context
                        .ctx
                        .reserve_vec(losses, 1, "step_drawing_losses")?;
                    losses.push(StepLossCode::DrawingSheetRevisionUnresolved.note(format!(
                        "STEP drawing sheet #{sheet_id} usage #{usage_id} has no resolvable drawing revision #{revision_id}"
                    )));
                }
            }
            if let Some(sequence) = parameters
                .get(2)
                .map(|value| {
                    value_text(
                        target_context.exchange,
                        value,
                        losses,
                        usage_id,
                        "drawing sheet revision usage sequence",
                        ctx,
                    )
                })
                .transpose()?
                .flatten()
            {
                let key = cadmpeg_core::nonblank_literal!("usage_{usage_id}_sequence");
                target_context.ctx.admit_btree_entry(
                    &sheet.parameters,
                    &key,
                    "step_drawing_usage_sequences",
                )?;
                sheet.parameters.insert(key, sequence);
            }
        }
        if let Some(revision) = drawings.get_mut(&revision_id) {
            match revision_target {
                TargetResolution::Resolved(target) => (target_context.ctx).push_btree_group(
                    &mut revision.relationships,
                    cadmpeg_core::nonblank_literal!("sheet_revision"),
                    target,
                    "step_drawing_relationship_groups",
                    "step_drawing_relationship_members",
                )?,
                TargetResolution::Ambiguous(identities) => note_ambiguous_target(
                    losses,
                    &format!("drawing revision #{revision_id} usage #{usage_id}"),
                    "sheet_revision",
                    sheet_id,
                    &identities,
                    target_context.ctx,
                )?,
                TargetResolution::Unresolved => {
                    target_context
                        .ctx
                        .reserve_vec(losses, 1, "step_drawing_losses")?;
                    losses.push(StepLossCode::DrawingRevisionSheetUnresolved.note(format!(
                        "STEP drawing revision #{revision_id} usage #{usage_id} has no resolvable sheet revision #{sheet_id}"
                    )));
                }
            }
        }
    }
    Ok(())
}

fn add_draughting_model_associations(
    exchange: &Exchange,
    drawings: &mut BTreeMap<u64, Drawing>,
    target_context: &TargetContext<'_>,
    losses: &mut Vec<LossNote>,
    typed: &mut HashSet<u64>,
) -> Result<(), CodecError> {
    for association_id in
        exchange.matching_entity_ids(|name| DRAWING_ASSOCIATION_TYPES.contains(&name))
    {
        let Some(record) = exchange.records().get(&association_id) else {
            continue;
        };
        let Some(parameters) = association_parameters(record) else {
            continue;
        };
        let Some(model_id) = parameters.get(3).and_then(ValueExt::reference) else {
            continue;
        };
        let Some(model) = drawings.get_mut(&model_id) else {
            continue;
        };

        let mut complete = true;
        let definition_id = parameters.get(2).and_then(ValueExt::reference);
        let definition_target = if let Some(definition_id) = definition_id {
            match target_context.resolve(definition_id)? {
                TargetResolution::Resolved(definition) => Some(definition),
                TargetResolution::Ambiguous(identities) => {
                    note_ambiguous_target(
                        losses,
                        &format!("draughting model #{model_id} association #{association_id}"),
                        "semantic_definition",
                        definition_id,
                        &identities,
                        target_context.ctx,
                    )?;
                    complete = false;
                    None
                }
                TargetResolution::Unresolved => {
                    target_context
                        .ctx
                        .reserve_vec(losses, 1, "step_drawing_losses")?;
                    losses.push(StepLossCode::DraughtingSemanticDefinitionUntyped.note(
                        format!(
                            "STEP draughting model #{model_id} association #{association_id} references a typed semantic definition without a neutral identity; the raw source parameter is retained"
                        ),
                    ));
                    complete = false;
                    None
                }
            }
        } else {
            None
        };
        if definition_id.is_none() {
            complete = false;
        }

        ensure_drawing_relationship_group(
            &mut model.relationships,
            cadmpeg_core::nonblank_literal!("associated_items"),
            target_context.ctx,
        )?;
        let mut has_items = false;
        if let Some(items) = parameters.get(4) {
            visit_drawing_references(items, target_context.ctx, &mut |item_id| {
                has_items = true;
                match target_context.resolve(item_id)? {
                    TargetResolution::Resolved(item) => (target_context.ctx).push_btree_group(
                        &mut model.relationships,
                        cadmpeg_core::nonblank_literal!("associated_items"),
                        item,
                        "step_drawing_relationship_groups",
                        "step_drawing_relationship_members",
                    )?,
                    TargetResolution::Ambiguous(identities) => {
                        note_ambiguous_target(
                            losses,
                            &format!("draughting model #{model_id} association #{association_id}"),
                            "associated_items",
                            item_id,
                            &identities,
                            target_context.ctx,
                        )?;
                        complete = false;
                    }
                    TargetResolution::Unresolved => {
                        target_context
                            .ctx
                            .reserve_vec(losses, 1, "step_drawing_losses")?;
                        losses.push(StepLossCode::DraughtingAssociatedItemUntyped.note(
                            format!(
                                "STEP draughting model #{model_id} association #{association_id} references source-typed item #{item_id} without a neutral identity; the raw source parameter is retained"
                            ),
                        ));
                        complete = false;
                    }
                }
                Ok(())
            })?;
        }
        if !has_items {
            complete = false;
        }

        let placeholder_target = if record
            .partials
            .iter()
            .any(|partial| partial.name == "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER")
        {
            match association_placeholder_reference(record, parameters) {
                Some(placeholder_id) => match target_context.resolve(placeholder_id)? {
                    TargetResolution::Resolved(placeholder) => Some(placeholder),
                    TargetResolution::Ambiguous(identities) => {
                        note_ambiguous_target(
                            losses,
                            &format!("draughting model #{model_id} association #{association_id}"),
                            "annotation_placeholder",
                            placeholder_id,
                            &identities,
                            target_context.ctx,
                        )?;
                        complete = false;
                        None
                    }
                    TargetResolution::Unresolved => {
                        target_context
                            .ctx
                            .reserve_vec(losses, 1, "step_drawing_losses")?;
                        losses.push(StepLossCode::DrawingRelationshipUntypedTarget.note(format!(
                            "STEP draughting model #{model_id} association #{association_id} relationship annotation_placeholder references source-typed record #{placeholder_id} without a neutral identity"
                        )));
                        complete = false;
                        None
                    }
                },
                None => {
                    complete = false;
                    None
                }
            }
        } else {
            None
        };

        if let Some(definition) = definition_target {
            (target_context.ctx).push_btree_group(
                &mut model.relationships,
                cadmpeg_core::nonblank_literal!("semantic_definition"),
                definition,
                "step_drawing_relationship_groups",
                "step_drawing_relationship_members",
            )?;
        }
        if let Some(placeholder) = placeholder_target {
            (target_context.ctx).push_btree_group(
                &mut model.relationships,
                cadmpeg_core::nonblank_literal!("annotation_placeholder"),
                placeholder,
                "step_drawing_relationship_groups",
                "step_drawing_relationship_members",
            )?;
        }
        if complete {
            target_context.ctx.insert_hash_set(
                typed,
                association_id,
                "step_drawing_typed_claims",
            )?;
        }
    }
    Ok(())
}

fn association_parameters(record: &RawRecord) -> Option<&[Value]> {
    [
        "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER",
        "DRAUGHTING_MODEL_ITEM_ASSOCIATION",
        "ITEM_IDENTIFIED_REPRESENTATION_USAGE",
    ]
    .into_iter()
    .find_map(|name| {
        record
            .partials
            .iter()
            .find(|partial| partial.name == name && partial.parameters.len() >= 5)
            .map(|partial| partial.parameters.as_slice())
    })
}

fn association_placeholder_reference(record: &RawRecord, parameters: &[Value]) -> Option<u64> {
    parameters.get(5).and_then(ValueExt::reference).or_else(|| {
        record
            .partials
            .iter()
            .find(|partial| partial.name == "ANNOTATION_PLACEHOLDER_OCCURRENCE")
            .and_then(|partial| partial.parameters.first())
            .and_then(ValueExt::reference)
    })
}

fn target_resolution(
    id: u64,
    target_identities: &BTreeMap<u64, BTreeSet<String>>,
    known_typed: &dyn SourceClaims,
    exchange: &Exchange,
    external_documents: &BTreeMap<u64, &str>,
    ctx: &DecodeContext<'_>,
) -> Result<TargetResolution, CodecError> {
    if let Some(identity) = target_identities
        .get(&id)
        .filter(|identities| identities.len() == 1)
        .and_then(|identities| identities.first())
    {
        return Ok(TargetResolution::Resolved(ReferenceSelection::new(
            ReferenceTarget::Local(
                ctx.copy_retained_text(identity, "step_drawing_local_target_text")?,
            ),
            Vec::new(),
        )));
    }
    if let Some(uri) = external_documents.get(&id) {
        return Ok(TargetResolution::Resolved(ReferenceSelection::new(
            ReferenceTarget::External {
                document: ctx.copy_retained_text(uri, "step_drawing_external_target_text")?,
                object: format!("#{id}"),
            },
            Vec::new(),
        )));
    }
    let wrapper_ambiguity = match wrapper_target_resolution(id, target_identities, exchange, ctx)? {
        Some(WrapperTargetResolution::Singleton(identity)) => {
            return Ok(TargetResolution::Resolved(ReferenceSelection::new(
                ReferenceTarget::Local(identity),
                Vec::new(),
            )));
        }
        Some(WrapperTargetResolution::Ambiguous(identities)) => Some(identities),
        None => None,
    };
    if !known_typed.contains(id) {
        if let Some(record) = exchange.records().get(&id) {
            return Ok(TargetResolution::Resolved(ReferenceSelection::new(
                ReferenceTarget::Local(opaque_record_id(id, record, ctx)?.into_string()),
                Vec::new(),
            )));
        }
    }
    let ambiguity = target_identities
        .get(&id)
        .filter(|identities| identities.len() > 1)
        .map(|identities| clone_drawing_identities(identities, ctx))
        .transpose()?
        .or(wrapper_ambiguity);
    Ok(ambiguity.map_or(TargetResolution::Unresolved, TargetResolution::Ambiguous))
}

enum WrapperTargetResolution {
    Singleton(String),
    Ambiguous(BTreeSet<String>),
}

fn wrapper_target_resolution(
    id: u64,
    target_identities: &BTreeMap<u64, BTreeSet<String>>,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<Option<WrapperTargetResolution>, CodecError> {
    if target_identities.contains_key(&id) {
        return Ok(None);
    }
    let mut identities = BTreeSet::new();
    let mut active = BTreeSet::new();
    let mut complete = BTreeSet::new();
    let mut pending = ctx.alloc_filled(1, (id, false), "step_drawing_wrapper_pending")?;
    while let Some((id, leaving)) = pending.pop() {
        if leaving {
            active.remove(&id);
            ctx.insert_btree_set(&mut complete, id, "step_drawing_wrapper_complete")?;
            continue;
        }
        if complete.contains(&id) {
            continue;
        }
        if active.contains(&id) {
            return Ok(None);
        }
        ctx.insert_btree_set(&mut active, id, "step_drawing_wrapper_active")?;
        ctx.reserve_vec(&mut pending, 1, "step_drawing_wrapper_pending")?;
        pending.push((id, true));
        if let Some(targets) = target_identities.get(&id) {
            for target in targets {
                if !identities.contains(target) {
                    let copy =
                        ctx.copy_retained_text(target, "step_drawing_wrapper_identity_text")?;
                    ctx.insert_btree_set(&mut identities, copy, "step_drawing_wrapper_identities")?;
                }
            }
            continue;
        }
        let Some(record) = exchange.records().get(&id) else {
            continue;
        };
        if let Some(plane) = record
            .partials
            .iter()
            .find(|partial| partial.name == "ANNOTATION_PLANE")
            .and_then(|partial| partial.parameters.get(2))
            .and_then(ValueExt::reference)
        {
            ctx.reserve_vec(&mut pending, 1, "step_drawing_wrapper_pending")?;
            pending.push((plane, false));
        } else if let Some(items) = mapped_representation(record, exchange)
            .and_then(|representation| exchange.records().get(&representation))
            .and_then(representation::items)
        {
            for item in items.rev() {
                ctx.reserve_vec(&mut pending, 1, "step_drawing_wrapper_pending")?;
                pending.push((item, false));
            }
        }
    }
    if identities.len() == 1 {
        let Some(identity) = identities.pop_first() else {
            return Ok(None);
        };
        return Ok(Some(WrapperTargetResolution::Singleton(identity)));
    }
    if identities.is_empty() {
        Ok(None)
    } else {
        Ok(Some(WrapperTargetResolution::Ambiguous(identities)))
    }
}

fn mapped_representation(record: &RawRecord, exchange: &Exchange) -> Option<u64> {
    let map_id = record
        .partials
        .iter()
        .find(|partial| partial.name == "MAPPED_ITEM")
        .and_then(|partial| partial.parameters.get(1))
        .and_then(ValueExt::reference)?;
    exchange
        .records()
        .get(&map_id)
        .and_then(|map| {
            map.partials
                .iter()
                .find(|partial| partial.name == "REPRESENTATION_MAP")
        })
        .and_then(|partial| partial.parameters.get(1))
        .and_then(ValueExt::reference)
}

fn value_text(
    exchange: &Exchange,
    value: &Value,
    losses: &mut Vec<LossNote>,
    record_id: u64,
    field: &str,
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    let _depth_guard = ctx.enter_nested("step_drawing_value_text_depth")?;
    let text = match value {
        Value::UninterpretedLiteral => return Ok(None),
        Value::Reference(id) => {
            ctx.format_retained(format_args!("#{id}"), "step_drawing_value_text")?
        }
        Value::ExternalReference(id) => {
            ctx.format_retained(format_args!("@{id}"), "step_drawing_value_text")?
        }
        Value::ConstantEntity(name) => {
            ctx.format_retained(format_args!("#{name}"), "step_drawing_value_text")?
        }
        Value::ExpressValueConstant(name) => {
            ctx.format_retained(format_args!("@{name}"), "step_drawing_value_text")?
        }
        Value::Integer(value) => {
            ctx.format_retained(format_args!("{value}"), "step_drawing_value_text")?
        }
        Value::Real(value) => {
            ctx.format_retained(format_args!("{}", value.get()), "step_drawing_value_text")?
        }
        Value::Enumeration(value) => {
            ctx.format_retained(format_args!(".{value}."), "step_drawing_value_text")?
        }
        Value::String(_) => {
            return decode_text_charged(
                exchange,
                value,
                losses,
                record_id,
                field,
                StepLossCode::MetadataStringInvalid,
                ctx,
            )
        }
        Value::Binary(value) => {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            let mut text = ctx.format_retained(
                format_args!("binary:{}:", value.bit_len()),
                "step_drawing_value_text",
            )?;
            for byte in value.data() {
                ctx.append_formatted_retained(
                    &mut text,
                    format_args!(
                        "{}{}",
                        char::from(HEX[usize::from(byte >> 4)]),
                        char::from(HEX[usize::from(byte & 0x0f)])
                    ),
                    "step_drawing_value_text",
                )?;
            }
            text
        }
        Value::Resource(value) => {
            ctx.format_retained(format_args!("<{value}>"), "step_drawing_value_text")?
        }
        Value::Omitted => ctx.format_retained(format_args!("$"), "step_drawing_value_text")?,
        Value::Derived => ctx.format_retained(format_args!("*"), "step_drawing_value_text")?,
        Value::List(values) => {
            let mut text = ctx.format_retained(format_args!("("), "step_drawing_value_text")?;
            for (index, value) in values.iter().enumerate() {
                let Some(part) = value_text(exchange, value, losses, record_id, field, ctx)? else {
                    return Ok(None);
                };
                if index != 0 {
                    ctx.append_retained(&mut text, ",", "step_drawing_value_text")?;
                }
                ctx.append_retained(&mut text, &part, "step_drawing_value_text")?;
            }
            ctx.append_retained(&mut text, ")", "step_drawing_value_text")?;
            text
        }
        Value::Typed(name, value) => {
            let Some(value) = value_text(exchange, value, losses, record_id, field, ctx)? else {
                return Ok(None);
            };
            ctx.format_retained(format_args!("{name}({value})"), "step_drawing_value_text")?
        }
    };
    Ok(Some(text))
}

#[cfg(test)]
mod tests;
