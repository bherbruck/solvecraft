// SPDX-License-Identifier: Apache-2.0
//! STEP presentation style and topology color decoding.

use crate::ids::{key_word, kind};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::reference::references;
use super::{named_parameter, RecordExt, ValueExt};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{
    AppearanceId, BodyId, CurveId, EdgeId, FaceId, IdentityKey, LayerId, OccurrenceId, PmiId,
    PointId, ProductDefinitionId, SurfaceId, VertexId,
};
use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::Fraction;
use cadmpeg_ir::topology::Color;

use crate::ids;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::decode_text_charged;
use super::groups::NonemptyGroup;
use super::topology::TopologyData;
use super::StageOutcome;

pub(super) fn decode(
    exchange: &Exchange,
    topology: &TopologyData,
    ir: &mut CadIr,
    product_definition_ids_by_source: &BTreeMap<u64, Vec<ProductDefinitionId>>,
    ctx: &DecodeContext<'_>,
) -> Result<StageOutcome<()>, CodecError> {
    let mut typed = HashSet::new();
    let mut losses = Vec::new();
    let graph_limit = super::record_graph_limit(ctx);
    let face_indices = collect_identity_indices(
        ir.model.faces.iter().map(|face| face.id.as_str()),
        ctx,
        "step_presentation_face_indices",
    )?;
    let body_indices = collect_identity_indices(
        ir.model.bodies.iter().map(|body| body.id.as_str()),
        ctx,
        "step_presentation_body_indices",
    )?;
    let indices = PresentationIndices {
        faces: &face_indices,
        bodies: &body_indices,
    };
    let entity_ids = EntityIds {
        edges: ctx.collect_btree_set(
            ir.model.edges.iter().map(|item| item.id.as_str()),
            "step_presentation_edge_ids",
        )?,
        vertices: ctx.collect_btree_set(
            ir.model.vertices.iter().map(|item| item.id.as_str()),
            "step_presentation_vertex_ids",
        )?,
        points: ctx.collect_btree_set(
            ir.model.points.iter().map(|item| item.id.as_str()),
            "step_presentation_point_ids",
        )?,
        curves: ctx.collect_btree_set(
            ir.model.curves.iter().map(|item| item.id.as_str()),
            "step_presentation_curve_ids",
        )?,
        surfaces: ctx.collect_btree_set(
            ir.model.surfaces.iter().map(|item| item.id.as_str()),
            "step_presentation_surface_ids",
        )?,
        products: product_definition_ids_by_source,
        occurrences: ctx.collect_btree_set(
            ir.model.occurrences.iter().map(|item| item.id.as_str()),
            "step_presentation_occurrence_ids",
        )?,
        pmi: ctx.collect_btree_set(
            ir.model.pmi.iter().map(|item| item.id.as_str()),
            "step_presentation_pmi_ids",
        )?,
        tessellations: ctx.collect_btree_set(
            ir.model.tessellations.iter().map(|item| item.id.as_str()),
            "step_presentation_tessellation_ids",
        )?,
    };
    let mut appearance_ids = BTreeMap::<(u64, u32), AppearanceId>::new();
    let mut hidden_style_ids = BTreeSet::new();
    let mut hidden_layer_ids = BTreeSet::new();
    let mut deferred_invisibility = BTreeMap::<u64, (bool, BTreeSet<u64>, BTreeSet<u64>)>::new();
    for (&id, record) in exchange.records() {
        if record.partial("INVISIBILITY").is_none() {
            continue;
        }
        let Some(items) = named_parameter(record, "INVISIBILITY", 0).and_then(ValueExt::list)
        else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!("INVISIBILITY #{id} has no item set")),
                "step_presentation_losses",
            )?;
            continue;
        };
        let mut supported = true;
        let mut style_targets = BTreeSet::new();
        let mut layer_targets = BTreeSet::new();
        for target in items.iter().filter_map(ValueExt::reference) {
            if exchange
                .records()
                .get(&target)
                .is_some_and(|record| record.partial("PRESENTATION_LAYER_ASSIGNMENT").is_some())
            {
                ctx.insert_btree_set(
                    &mut hidden_layer_ids,
                    target,
                    "step_presentation_hidden_layer_ids",
                )?;
                ctx.insert_btree_set(
                    &mut layer_targets,
                    target,
                    "step_presentation_invisibility_layer_targets",
                )?;
                continue;
            }
            if exchange
                .records()
                .get(&target)
                .is_some_and(|record| styled_item_parts(record).is_some())
            {
                ctx.insert_btree_set(
                    &mut hidden_style_ids,
                    target,
                    "step_presentation_hidden_style_ids",
                )?;
                ctx.insert_btree_set(
                    &mut style_targets,
                    target,
                    "step_presentation_invisibility_style_targets",
                )?;
                continue;
            }
            if exchange
                .records()
                .get(&target)
                .is_some_and(super::drawing::is_supported_invisibility_target)
            {
                continue;
            }
            if exchange
                .records()
                .get(&target)
                .is_some_and(super::pmi::is_supported_invisibility_target)
            {
                continue;
            }
            let (body_ids, target_supported) =
                invisible_body_ids(target, exchange, topology, &body_indices, ctx)?;
            let mut hidden = false;
            for body_id in body_ids {
                if let Some(index) = body_indices.get(body_id.as_str()) {
                    ir.model.bodies[*index].visible = Some(false);
                    hidden = true;
                }
            }
            if !target_supported || !hidden {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                        "INVISIBILITY #{id} targets unsupported item #{target}"
                    )),
                    "step_presentation_losses",
                )?;
                supported = false;
            }
        }
        if style_targets.is_empty() && layer_targets.is_empty() && supported {
            ctx.insert_hash_set(&mut typed, id, "step_presentation_typed_claims")?;
        } else if !style_targets.is_empty() || !layer_targets.is_empty() {
            ctx.insert_btree_map(
                &mut deferred_invisibility,
                id,
                (supported, style_targets, layer_targets),
                "step_presentation_deferred_invisibility",
            )?;
        }
    }
    for (&layer_id, layer) in exchange.records() {
        if layer.partial("PRESENTATION_LAYER_ASSIGNMENT").is_none() {
            continue;
        }
        let Some(assigned_items) =
            named_parameter(layer, "PRESENTATION_LAYER_ASSIGNMENT", 2).and_then(ValueExt::list)
        else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "PRESENTATION_LAYER_ASSIGNMENT #{layer_id} has no assigned item set"
                )),
                "step_presentation_losses",
            )?;
            continue;
        };
        if assigned_items.is_empty() {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "PRESENTATION_LAYER_ASSIGNMENT #{layer_id} has an empty assigned item set"
                )),
                "step_presentation_losses",
            )?;
            continue;
        }
        let Some(name) = named_parameter(layer, "PRESENTATION_LAYER_ASSIGNMENT", 0)
            .map(|value| {
                decode_text_charged(
                    exchange,
                    value,
                    &mut losses,
                    layer_id,
                    "presentation layer name",
                    StepLossCode::MetadataStringInvalid,
                    ctx,
                )
            })
            .transpose()?
            .flatten()
        else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning.note(format!(
                    "PRESENTATION_LAYER_ASSIGNMENT #{layer_id} has no name"
                )),
                "step_presentation_losses",
            )?;
            continue;
        };
        let description = named_parameter(layer, "PRESENTATION_LAYER_ASSIGNMENT", 1)
            .map(|value| {
                decode_text_charged(
                    exchange,
                    value,
                    &mut losses,
                    layer_id,
                    "presentation layer description",
                    StepLossCode::MetadataStringInvalid,
                    ctx,
                )
            })
            .transpose()?
            .flatten()
            .filter(|value| !value.is_empty());
        let mut items = Vec::new();
        for id in assigned_items.iter().filter_map(ValueExt::reference) {
            append_presentation_items(
                id,
                exchange,
                topology,
                &entity_ids,
                indices,
                &mut items,
                ctx,
            )?;
        }
        ctx.push_vec(
            &mut ir.model.presentation_layers,
            PresentationLayer {
                id: LayerId::from(ids::presentation(kind!("layer"), layer_id)),
                name,
                description,
                visible: hidden_layer_ids.contains(&layer_id).then_some(false),
                items,
            },
            "step_presentation_layer_records",
        )?;
        ctx.insert_hash_set(&mut typed, layer_id, "step_presentation_typed_claims")?;
    }
    let mut styles = Vec::new();
    for (&id, record) in exchange.records() {
        if styled_item_parts(record).is_some() {
            let order = style_application_order(id, exchange, graph_limit, ctx)?;
            ctx.push_vec(&mut styles, (id, order), "step_presentation_style_ids")?;
        }
    }
    let mut overridden_styles = BTreeSet::new();
    for (id, _) in &styles {
        if let Some(overridden) = overridden_style(&exchange.records()[id]) {
            ctx.insert_btree_set(
                &mut overridden_styles,
                overridden,
                "step_presentation_overridden_styles",
            )?;
        }
    }
    ctx.stable_sort_by(
        &mut styles,
        |(_, left), (_, right)| left.cmp(right),
        |_| 0,
        "step_presentation_style_ids_sort",
    )?;
    let mut scalar_color_candidates =
        HashMap::<AppearanceTarget, NonemptyGroup<(u64, Color)>>::new();
    for (style_id, _) in styles {
        if overridden_styles.contains(&style_id) {
            ctx.insert_hash_set(&mut typed, style_id, "step_presentation_typed_claims")?;
            continue;
        }
        let style = &exchange.records()[&style_id];
        let Some(parts) = styled_item_parts(style) else {
            continue;
        };
        let Some(target_step) = parts.target.reference() else {
            ctx.push_vec(
                &mut losses,
                StepLossCode::DecodeWarning
                    .note(format!("STYLED_ITEM #{style_id} has no resolved target")),
                "step_presentation_losses",
            )?;
            continue;
        };
        if parts.styles.list().is_some_and(<[Value]>::is_empty) {
            ctx.insert_hash_set(&mut typed, style_id, "step_presentation_typed_claims")?;
            continue;
        }
        let domain = style_domain(target_step, exchange, ctx)?;
        let color_storage =
            std::cell::RefCell::new(ctx.reserve_scoped(0, "step color search storage")?);
        let mut active = BTreeSet::new();
        let mut color_cache = ColorCache::default();
        let mut invalid_surface_sides = BTreeSet::new();
        let mut style_references = Vec::new();
        for reference in parts
            .styles
            .list()
            .into_iter()
            .flatten()
            .flat_map(|value| references(value, ctx))
        {
            let reference = reference?;
            ctx.push_vec(
                &mut style_references,
                reference,
                "step_presentation_style_references",
            )?;
        }
        let mut context_style_ids = BTreeSet::new();
        for reference in &style_references {
            if exchange
                .records()
                .get(reference)
                .is_some_and(is_presentation_style_by_context)
            {
                ctx.insert_btree_set(
                    &mut context_style_ids,
                    *reference,
                    "step_presentation_context_style_ids",
                )?;
            }
        }
        if !context_style_ids.is_empty() {
            let message = context_style_message(style_id, &context_style_ids, exchange, ctx)?;
            ctx.push_vec(
                &mut losses,
                StepLossCode::ContextDependentStyleUnresolved.note(message),
                "step_presentation_losses",
            )?;
            continue;
        }
        let color = combine_color_resolutions(style_references.iter().copied().map(|reference| {
            find_color(
                reference,
                exchange,
                domain,
                ColorSearchState {
                    storage: &color_storage,
                    active: &mut active,
                    cache: &mut color_cache,
                    losses: &mut losses,
                    invalid_surface_sides: &mut invalid_surface_sides,
                },
                0,
                ctx,
            )
        }))?;
        let color = if color.is_none() && matches!(domain, StyleDomain::Curve | StyleDomain::Point)
        {
            combine_color_resolutions(style_references.iter().copied().map(|reference| {
                find_color(
                    reference,
                    exchange,
                    StyleDomain::Surface,
                    ColorSearchState {
                        storage: &color_storage,
                        active: &mut active,
                        cache: &mut color_cache,
                        losses: &mut losses,
                        invalid_surface_sides: &mut invalid_surface_sides,
                    },
                    0,
                    ctx,
                )
            }))?
        } else {
            color
        };
        let color = match color {
            Some(ColorResolution::Candidate(candidate)) => candidate,
            Some(ColorResolution::Ambiguous { .. }) => {
                ctx.push_vec(&mut losses, StepLossCode::ConflictingScalarColors.note(format!(
                    "STYLED_ITEM #{style_id} has distinct equal-precedence colors; no scalar color is selected and the source style graph remains retained"
                )), "step_presentation_losses")?;
                continue;
            }
            None => {
                let mut visited = BTreeSet::new();
                if !contains_null_style(parts.styles, exchange, &mut visited, 0, ctx)? {
                    ctx.push_vec(
                        &mut losses,
                        StepLossCode::DecodeWarning.note(format!(
                            "STYLED_ITEM #{style_id} has no resolved surface color"
                        )),
                        "step_presentation_losses",
                    )?;
                }
                continue;
            }
        };
        let ColorCandidate {
            id: color_id,
            color,
            name,
            ..
        } = color;
        let appearance_key = (color_id, color.a().to_bits());
        let appearance_id = if let Some(appearance_id) = appearance_ids.get(&appearance_key) {
            appearance_id.try_clone_for_decode(ctx, "step_presentation_identity_copy")?
        } else {
            let key = if color.a() == 1.0 {
                IdentityKey::from(color_id)
            } else {
                IdentityKey::from(color_id)
                    .dash(key_word!("alpha"))
                    .dash(color.a().to_bits())
            };
            let id = AppearanceId::from(ids::presentation(kind!("appearance"), key));
            ctx.push_vec(
                &mut ir.model.appearances,
                Appearance {
                    id: id.try_clone_for_decode(ctx, "step_presentation_identity_copy")?,
                    name,
                    asset_guid: None,
                    library_id: None,
                    visual_guid: None,
                    physical_token: None,
                    schema: Some("step_surface_style".into()),
                    category: None,
                    base_color: Some(color),
                    textures: Vec::new(),
                    properties: BTreeMap::new(),
                },
                "step_presentation_appearance_records",
            )?;
            ctx.insert_btree_map(
                &mut appearance_ids,
                appearance_key,
                id.try_clone_for_decode(ctx, "step_presentation_identity_copy")?,
                "step_presentation_appearance_ids",
            )?;
            id
        };
        let target_steps = expand_style_targets(
            target_step,
            exchange,
            &mut typed,
            &mut BTreeSet::new(),
            0,
            graph_limit,
            ctx,
        )?;
        for (ordinal, target_step) in target_steps.into_iter().enumerate() {
            let targets =
                appearance_targets(target_step, exchange, topology, &entity_ids, indices, ctx)?;
            if targets.is_empty() {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                        "STYLED_ITEM #{style_id} targets unsupported item #{target_step}"
                    )),
                    "step_presentation_losses",
                )?;
                continue;
            }
            for (target_ordinal, target) in targets.into_iter().enumerate() {
                push_scalar_candidate(&mut scalar_color_candidates, &target, style_id, color, ctx)?;
                ctx.push_vec(
                    &mut ir.model.appearance_bindings,
                    AppearanceBinding {
                        id: ids::presentation(
                            kind!("binding"),
                            IdentityKey::from(style_id)
                                .colon(ordinal)
                                .dash(target_ordinal),
                        )
                        .into(),
                        target,
                        appearance: appearance_id
                            .try_clone_for_decode(ctx, "step_presentation_identity_copy")?,
                        source_entity_id: Some(format!("#{style_id}")),
                        object_type: None,
                        visible: style_is_hidden(
                            style_id,
                            &hidden_style_ids,
                            exchange,
                            &mut BTreeSet::new(),
                            ctx,
                        )?
                        .then_some(false),
                        channels: BTreeMap::new(),
                    },
                    "step_presentation_appearance_bindings",
                )?;
            }
        }
        ctx.insert_hash_set(&mut typed, style_id, "step_presentation_typed_claims")?;
        if let Some(overridden) = overridden_style(style) {
            ctx.insert_hash_set(&mut typed, overridden, "step_presentation_typed_claims")?;
        }
        for &(id, _) in color_cache.keys() {
            if !invalid_surface_sides.contains(&id) {
                ctx.insert_hash_set(&mut typed, id, "step_presentation_typed_claims")?;
            }
        }
        ctx.insert_hash_set(&mut typed, color_id, "step_presentation_typed_claims")?;
    }
    for (invisibility_id, (mut supported, style_targets, layer_targets)) in deferred_invisibility {
        for style_id in style_targets {
            let mut matched = false;
            for binding in &mut ir.model.appearance_bindings {
                let Some(binding_style_id) = binding
                    .source_entity_id
                    .as_deref()
                    .and_then(|source_id| source_id.strip_prefix('#'))
                    .and_then(|source_id| source_id.parse::<u64>().ok())
                else {
                    continue;
                };
                if style_inherits_from(
                    binding_style_id,
                    style_id,
                    exchange,
                    &mut BTreeSet::new(),
                    ctx,
                )? {
                    binding.visible = Some(false);
                    matched = true;
                }
            }
            if !matched {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                        "INVISIBILITY #{invisibility_id} targets unsupported item #{style_id}"
                    )),
                    "step_presentation_losses",
                )?;
                supported = false;
            }
        }
        for layer_id in layer_targets {
            let expected_id = ids::presentation(kind!("layer"), layer_id);
            let mut matched = false;
            for layer in &mut ir.model.presentation_layers {
                if layer.id.as_str() == expected_id.as_str() {
                    layer.visible = Some(false);
                    matched = true;
                    break;
                }
            }
            if !matched {
                ctx.push_vec(
                    &mut losses,
                    StepLossCode::DecodeWarning.note(format!(
                        "INVISIBILITY #{invisibility_id} targets unsupported item #{layer_id}"
                    )),
                    "step_presentation_losses",
                )?;
                supported = false;
            }
        }
        if supported {
            ctx.insert_hash_set(
                &mut typed,
                invisibility_id,
                "step_presentation_typed_claims",
            )?;
        }
    }
    for (target, candidates) in scalar_color_candidates {
        let mut color = candidates.first().1;
        let mut conflict = false;
        for (_, candidate) in candidates.iter().skip(1) {
            ctx.charge_work(1, "step_presentation_scalar_color_compare")?;
            if candidate.r() != color.r()
                || candidate.g() != color.g()
                || candidate.b() != color.b()
            {
                conflict = true;
            } else if candidate.a() < color.a() {
                color = *candidate;
            }
        }
        if conflict {
            let message = scalar_conflict_message(&candidates, &target, ctx)?;
            ctx.push_vec(
                &mut losses,
                StepLossCode::ConflictingScalarColors.note(message),
                "step_presentation_losses",
            )?;
        } else {
            let slot = match &target {
                AppearanceTarget::Face(face) => face_indices
                    .get(face.as_str())
                    .map(|&index| &mut ir.model.faces[index].color),
                AppearanceTarget::Body(body) => body_indices
                    .get(body.as_str())
                    .map(|&index| &mut ir.model.bodies[index].color),
                _ => None,
            };
            if let Some(slot) = slot {
                *slot = Some(color);
            }
        }
    }
    Ok(StageOutcome {
        value: (),
        claims: typed,
        losses,
        notes: Vec::new(),
    })
}

fn invisible_body_ids(
    id: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    body_indices: &BTreeMap<String, usize>,
    ctx: &DecodeContext<'_>,
) -> Result<(BTreeSet<BodyId>, bool), CodecError> {
    let mut body_ids = BTreeSet::new();
    let mut active = BTreeSet::new();
    let supported = collect_invisible_body_ids(
        id,
        exchange,
        topology,
        body_indices,
        &mut active,
        &mut body_ids,
        ctx,
    )?;
    Ok((body_ids, supported))
}

fn collect_invisible_body_ids(
    id: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    body_indices: &BTreeMap<String, usize>,
    active: &mut BTreeSet<u64>,
    body_ids: &mut BTreeSet<BodyId>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if active.contains(&id) {
        return Ok(false);
    }
    let _nested = ctx.enter_nested("step_presentation_invisible_body_walk")?;
    ctx.insert_btree_set(active, id, "step_presentation_invisible_body_active")?;
    if let Some(ids) = topology.body_by_root.get(&id) {
        for body in ids {
            if !body_ids.contains(body) {
                let body =
                    body.try_clone_for_decode(ctx, "step_presentation_invisible_body_identity")?;
                ctx.insert_btree_set(body_ids, body, "step_presentation_invisible_body_ids")?;
            }
        }
        active.remove(&id);
        return Ok(!ids.is_empty());
    }
    let fallback = BodyId::from(ids::data(kind!("body"), id));
    if body_indices.contains_key(fallback.as_str()) {
        ctx.insert_btree_set(body_ids, fallback, "step_presentation_invisible_body_ids")?;
        active.remove(&id);
        return Ok(true);
    }

    let Some(record) = exchange.records().get(&id) else {
        active.remove(&id);
        return Ok(false);
    };
    let mut found_reference = false;
    let mut supported = true;
    if record
        .partials
        .iter()
        .any(|partial| partial.name == "STYLED_ITEM")
        || record
            .partials
            .iter()
            .any(|partial| partial.name == "OVER_RIDING_STYLED_ITEM")
    {
        if let Some(reference) =
            styled_item_parts(record).and_then(|parts| parts.target.reference())
        {
            found_reference = true;
            supported &= collect_invisible_body_ids(
                reference,
                exchange,
                topology,
                body_indices,
                active,
                body_ids,
                ctx,
            )?;
        }
    } else if record
        .partials
        .iter()
        .any(|partial| super::representation::is_representation_name(&partial.name))
    {
        if let Some(references) = super::representation::items(record) {
            for reference in references {
                found_reference = true;
                supported &= collect_invisible_body_ids(
                    reference,
                    exchange,
                    topology,
                    body_indices,
                    active,
                    body_ids,
                    ctx,
                )?;
            }
        }
    }
    active.remove(&id);
    Ok(found_reference && supported)
}

fn expand_style_targets(
    id: u64,
    exchange: &Exchange,
    typed: &mut HashSet<u64>,
    active: &mut BTreeSet<u64>,
    depth: usize,
    graph_limit: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<u64>, CodecError> {
    if depth >= graph_limit || active.contains(&id) {
        return Ok(Vec::new());
    }
    let _nested = ctx.enter_nested("step_presentation_style_target_walk")?;
    ctx.insert_btree_set(active, id, "step_presentation_style_target_active")?;
    let Some(record) = exchange.records().get(&id) else {
        active.remove(&id);
        let mut targets = Vec::new();
        ctx.push_vec(&mut targets, id, "step_presentation_style_target_items")?;
        return Ok(targets);
    };
    let Some(set_name) = record.partials.iter().find_map(|partial| {
        matches!(
            partial.name.as_str(),
            "GEOMETRIC_SET" | "GEOMETRIC_CURVE_SET"
        )
        .then_some(partial.name.as_str())
    }) else {
        active.remove(&id);
        let mut targets = Vec::new();
        ctx.push_vec(&mut targets, id, "step_presentation_style_target_items")?;
        return Ok(targets);
    };
    ctx.insert_hash_set(typed, id, "step_presentation_typed_claims")?;
    let mut targets = Vec::new();
    for item in named_parameter(record, set_name, 1)
        .and_then(ValueExt::list)
        .into_iter()
        .flatten()
        .filter_map(ValueExt::reference)
    {
        for target in
            expand_style_targets(item, exchange, typed, active, depth + 1, graph_limit, ctx)?
        {
            ctx.push_vec(&mut targets, target, "step_presentation_style_target_items")?;
        }
    }
    active.remove(&id);
    Ok(targets)
}

fn appearance_targets(
    id: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    entity_ids: &EntityIds<'_>,
    indices: PresentationIndices<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<AppearanceTarget>, CodecError> {
    let mut targets = Vec::new();
    if let Some(bodies) = topology.body_by_root.get(&id) {
        for body in bodies {
            if indices.bodies.contains_key(body.as_str()) {
                let body =
                    body.try_clone_for_decode(ctx, "step_presentation_appearance_body_identity")?;
                ctx.push_vec(
                    &mut targets,
                    AppearanceTarget::Body(body),
                    "step_presentation_appearance_targets",
                )?;
            }
        }
        return Ok(targets);
    }
    if let Some(faces) = topology.faces_by_source.get(&id) {
        for face in faces {
            if indices.faces.contains_key(face.as_str()) {
                let face =
                    face.try_clone_for_decode(ctx, "step_presentation_appearance_face_identity")?;
                ctx.push_vec(
                    &mut targets,
                    AppearanceTarget::Face(face),
                    "step_presentation_appearance_targets",
                )?;
            }
        }
        return Ok(targets);
    }
    if let Some(edges) = topology.edges_by_source.get(&id) {
        for edge in edges {
            if entity_ids.edges.contains(edge.as_str()) {
                let edge =
                    edge.try_clone_for_decode(ctx, "step_presentation_appearance_edge_identity")?;
                ctx.push_vec(
                    &mut targets,
                    AppearanceTarget::Edge(edge),
                    "step_presentation_appearance_targets",
                )?;
            }
        }
        return Ok(targets);
    }
    if let Some(vertices) = topology.vertices_by_source.get(&id) {
        for vertex in vertices {
            if entity_ids.vertices.contains(vertex.as_str()) {
                let vertex = vertex
                    .try_clone_for_decode(ctx, "step_presentation_appearance_vertex_identity")?;
                ctx.push_vec(
                    &mut targets,
                    AppearanceTarget::Vertex(vertex),
                    "step_presentation_appearance_targets",
                )?;
            }
        }
        return Ok(targets);
    }
    let face_id = ids::data(kind!("face"), id);
    let body_id = ids::data(kind!("body"), id);
    let edge_id = ids::data(kind!("edge"), id);
    let surface_id = ids::data(kind!("surface"), id);
    let curve_id = ids::data(kind!("curve"), id);
    let point_id = ids::data(kind!("point"), id);
    let tessellation_id = ids::tessellation(kind!("mesh"), id);
    let target = if indices.faces.contains_key(face_id.as_str()) {
        Some(AppearanceTarget::Face(FaceId::from(face_id)))
    } else if indices.bodies.contains_key(body_id.as_str()) {
        Some(AppearanceTarget::Body(BodyId::from(body_id)))
    } else if entity_ids.edges.contains(edge_id.as_str()) {
        Some(AppearanceTarget::Edge(EdgeId::from(edge_id)))
    } else if entity_ids.surfaces.contains(surface_id.as_str()) {
        Some(AppearanceTarget::Surface(SurfaceId::from(surface_id)))
    } else if entity_ids.curves.contains(curve_id.as_str()) {
        Some(AppearanceTarget::Curve(CurveId::from(curve_id)))
    } else if entity_ids.points.contains(point_id.as_str()) {
        Some(AppearanceTarget::Point(PointId::from(point_id)))
    } else if entity_ids.tessellations.contains(tessellation_id.as_str()) {
        Some(AppearanceTarget::Tessellation(
            tessellation_id.into_string(),
        ))
    } else if exchange.records().contains_key(&id) {
        Some(AppearanceTarget::Source {
            source_id: format!("#{id}"),
        })
    } else {
        None
    };
    if let Some(target) = target {
        ctx.push_vec(&mut targets, target, "step_presentation_appearance_targets")?;
    }
    Ok(targets)
}

fn append_presentation_items(
    id: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    entity_ids: &EntityIds<'_>,
    indices: PresentationIndices<'_>,
    items: &mut Vec<PresentationItem>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if let Some(bodies) = topology.body_by_root.get(&id) {
        for body in bodies {
            if indices.bodies.contains_key(body.as_str()) {
                let body =
                    body.try_clone_for_decode(ctx, "step_presentation_layer_body_identity")?;
                ctx.push_vec(
                    items,
                    PresentationItem::Body { body },
                    "step_presentation_layer_items",
                )?;
            }
        }
        return Ok(());
    }
    if let Some(faces) = topology.faces_by_source.get(&id) {
        for face in faces {
            if indices.faces.contains_key(face.as_str()) {
                let face =
                    face.try_clone_for_decode(ctx, "step_presentation_layer_face_identity")?;
                ctx.push_vec(
                    items,
                    PresentationItem::Face { face },
                    "step_presentation_layer_items",
                )?;
            }
        }
        return Ok(());
    }
    if let Some(edges) = topology.edges_by_source.get(&id) {
        for edge in edges {
            if entity_ids.edges.contains(edge.as_str()) {
                let edge =
                    edge.try_clone_for_decode(ctx, "step_presentation_layer_edge_identity")?;
                ctx.push_vec(
                    items,
                    PresentationItem::Edge { edge },
                    "step_presentation_layer_items",
                )?;
            }
        }
        return Ok(());
    }
    if let Some(vertices) = topology.vertices_by_source.get(&id) {
        for vertex in vertices {
            if entity_ids.vertices.contains(vertex.as_str()) {
                let vertex =
                    vertex.try_clone_for_decode(ctx, "step_presentation_layer_vertex_identity")?;
                ctx.push_vec(
                    items,
                    PresentationItem::Vertex { vertex },
                    "step_presentation_layer_items",
                )?;
            }
        }
        return Ok(());
    }
    if let Some(products) = entity_ids.products.get(&id) {
        for product in products {
            let product =
                product.try_clone_for_decode(ctx, "step_presentation_layer_product_identity")?;
            ctx.push_vec(
                items,
                PresentationItem::Product { product },
                "step_presentation_layer_items",
            )?;
        }
        return Ok(());
    }
    ctx.push_vec(
        items,
        presentation_item_one(id, exchange, entity_ids, indices),
        "step_presentation_layer_items",
    )
}

fn presentation_item_one(
    id: u64,
    exchange: &Exchange,
    entity_ids: &EntityIds<'_>,
    indices: PresentationIndices<'_>,
) -> PresentationItem {
    let candidate = |kind: &crate::ids::IdentityKind| ids::data(kind, id);
    let body = candidate(kind!("body"));
    if indices.bodies.contains_key(body.as_str()) {
        return PresentationItem::Body {
            body: BodyId::from(body),
        };
    }
    let face = candidate(kind!("face"));
    if indices.faces.contains_key(face.as_str()) {
        return PresentationItem::Face {
            face: FaceId::from(face),
        };
    }
    let edge = candidate(kind!("edge"));
    if entity_ids.edges.contains(edge.as_str()) {
        return PresentationItem::Edge {
            edge: EdgeId::from(edge),
        };
    }
    let vertex = candidate(kind!("vertex"));
    if entity_ids.vertices.contains(vertex.as_str()) {
        return PresentationItem::Vertex {
            vertex: VertexId::from(vertex),
        };
    }
    let point = candidate(kind!("point"));
    if entity_ids.points.contains(point.as_str()) {
        return PresentationItem::Point {
            point: PointId::from(point),
        };
    }
    let curve = candidate(kind!("curve"));
    if entity_ids.curves.contains(curve.as_str()) {
        return PresentationItem::Curve {
            curve: CurveId::from(curve),
        };
    }
    let surface = candidate(kind!("surface"));
    if entity_ids.surfaces.contains(surface.as_str()) {
        return PresentationItem::Surface {
            surface: SurfaceId::from(surface),
        };
    }
    let Some(record) = exchange.records().get(&id) else {
        return PresentationItem::Source {
            source_id: super::step_source_id(id),
        };
    };
    let has = |name: &str| record.partial(name).is_some();
    if has("NEXT_ASSEMBLY_USAGE_OCCURRENCE")
        && entity_ids
            .occurrences
            .contains(ids::product(kind!("occurrence"), id).as_str())
    {
        PresentationItem::Occurrence {
            occurrence: OccurrenceId::from(ids::product(kind!("occurrence"), id)),
        }
    } else if record.partials.iter().any(|partial| {
        (partial.name == "DATUM"
            || partial.name == "DATUM_SYSTEM"
            || partial.name.starts_with("DIMENSIONAL_")
            || partial.name.ends_with("_TOLERANCE")
            || super::pmi::is_presentation_annotation(&partial.name))
            && entity_ids
                .pmi
                .contains(ids::presentation(kind!("pmi"), id).as_str())
    }) {
        PresentationItem::Pmi {
            annotation: PmiId::from(ids::presentation(kind!("pmi"), id)),
        }
    } else if (has("TRIANGULATED_FACE")
        || has("COMPLEX_TRIANGULATED_FACE")
        || has("TRIANGULATED_SURFACE_SET")
        || has("COMPLEX_TRIANGULATED_SURFACE_SET"))
        && entity_ids
            .tessellations
            .contains(ids::tessellation(kind!("mesh"), id).as_str())
    {
        PresentationItem::Tessellation {
            tessellation: ids::tessellation(kind!("mesh"), id).into_string(),
        }
    } else {
        PresentationItem::Source {
            source_id: super::step_source_id(id),
        }
    }
}

struct EntityIds<'a> {
    edges: BTreeSet<&'a str>,
    vertices: BTreeSet<&'a str>,
    points: BTreeSet<&'a str>,
    curves: BTreeSet<&'a str>,
    surfaces: BTreeSet<&'a str>,
    products: &'a BTreeMap<u64, Vec<ProductDefinitionId>>,
    occurrences: BTreeSet<&'a str>,
    pmi: BTreeSet<&'a str>,
    tessellations: BTreeSet<&'a str>,
}

#[derive(Clone, Copy)]
struct PresentationIndices<'a> {
    faces: &'a BTreeMap<String, usize>,
    bodies: &'a BTreeMap<String, usize>,
}

fn push_scalar_candidate(
    candidates: &mut HashMap<AppearanceTarget, NonemptyGroup<(u64, Color)>>,
    target: &AppearanceTarget,
    style_id: u64,
    color: Color,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if !matches!(
        target,
        AppearanceTarget::Face(_) | AppearanceTarget::Body(_)
    ) {
        return Ok(());
    }
    if !candidates.contains_key(target) {
        let key = match target {
            AppearanceTarget::Face(face) => AppearanceTarget::Face(
                face.try_clone_for_decode(ctx, "step_presentation_scalar_target_identity")?,
            ),
            AppearanceTarget::Body(body) => AppearanceTarget::Body(
                body.try_clone_for_decode(ctx, "step_presentation_scalar_target_identity")?,
            ),
            _ => return Ok(()),
        };
        ctx.reserve_map(candidates, 1, "step_presentation_scalar_color_groups")?;
        candidates.insert(key, NonemptyGroup::new((style_id, color)));
        return Ok(());
    }
    let values = candidates
        .get_mut(target)
        .ok_or_else(|| CodecError::malformed("presentation scalar target was not indexed"))?;
    values.push(
        (style_id, color),
        ctx,
        "step_presentation_scalar_color_members",
    )
}

fn collect_identity_indices<'a>(
    identities: impl IntoIterator<Item = &'a str>,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<BTreeMap<String, usize>, CodecError> {
    let mut result = BTreeMap::new();
    for (index, identity) in identities.into_iter().enumerate() {
        if let Some(existing) = result.get_mut(identity) {
            *existing = index;
            continue;
        }
        let copy = ctx.copy_retained_text(identity, operation)?;
        ctx.insert_btree_map(&mut result, copy, index, operation)?;
    }
    Ok(result)
}

fn overridden_style(style: &RawRecord) -> Option<u64> {
    styled_item_parts(style)
        .and_then(|parts| parts.overridden)
        .and_then(ValueExt::reference)
}

struct StyledItemParts<'a> {
    styles: &'a Value,
    target: &'a Value,
    overridden: Option<&'a Value>,
}

fn styled_item_parts(record: &RawRecord) -> Option<StyledItemParts<'_>> {
    if let Some(partial) = record
        .partials
        .iter()
        .find(|partial| partial.name == "OVER_RIDING_STYLED_ITEM")
    {
        let parameters = partial.parameters.as_slice();
        let target = parameters.get(parameters.len().checked_sub(2)?)?;
        let styles = parameters.get(parameters.len().checked_sub(3)?)?;
        let overridden = parameters.last()?;
        return Some(StyledItemParts {
            styles,
            target,
            overridden: Some(overridden),
        });
    }
    let partial = record
        .partials
        .iter()
        .find(|partial| partial.name == "STYLED_ITEM")?;
    let parameters = partial.parameters.as_slice();
    let target = parameters.last()?;
    let styles = parameters.get(parameters.len().checked_sub(2)?)?;
    Some(StyledItemParts {
        styles,
        target,
        overridden: None,
    })
}

fn is_presentation_style_by_context(record: &RawRecord) -> bool {
    record
        .partials
        .iter()
        .any(|partial| partial.name == "PRESENTATION_STYLE_BY_CONTEXT")
}

fn presentation_style_context(record: &RawRecord) -> Option<&Value> {
    record
        .partials
        .iter()
        .find(|partial| partial.name == "PRESENTATION_STYLE_BY_CONTEXT")
        .and_then(|partial| partial.parameters.last())
}

struct ContextStyleDetails<'a> {
    ids: &'a BTreeSet<u64>,
    exchange: &'a Exchange,
}

impl std::fmt::Display for ContextStyleDetails<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, context_style_id) in self.ids.iter().enumerate() {
            if index > 0 {
                formatter.write_str(", ")?;
            }
            let context = self
                .exchange
                .records()
                .get(context_style_id)
                .and_then(presentation_style_context)
                .and_then(ValueExt::reference);
            match context {
                Some(context) => write!(formatter, "#{context_style_id} in #{context}")?,
                None => write!(formatter, "#{context_style_id} in unresolved")?,
            }
        }
        Ok(())
    }
}

fn context_style_message(
    style_id: u64,
    ids: &BTreeSet<u64>,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<String, CodecError> {
    let details = ContextStyleDetails { ids, exchange };
    ctx.format_retained(format_args!("STYLED_ITEM #{style_id} has context-dependent style assignments {details}; no presentation context is selected by the neutral model; those source branches remain opaque"), "step_presentation_context_style_text")
}

struct ScalarStyleIds<'a>(&'a NonemptyGroup<(u64, Color)>);

impl std::fmt::Display for ScalarStyleIds<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, (style_id, _)) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str(", ")?;
            }
            write!(formatter, "#{style_id}")?;
        }
        Ok(())
    }
}

fn scalar_conflict_message(
    candidates: &NonemptyGroup<(u64, Color)>,
    target: &AppearanceTarget,
    ctx: &DecodeContext<'_>,
) -> Result<String, CodecError> {
    let style_ids = ScalarStyleIds(candidates);
    ctx.format_retained(format_args!("independent styled items {style_ids} assign conflicting scalar colors to {target:?}; scalar color omitted and appearance bindings retain every assignment"), "step_presentation_scalar_conflict_text")
}

pub(super) fn styled_item_target(record: &RawRecord) -> Option<u64> {
    styled_item_parts(record).and_then(|parts| parts.target.reference())
}

/// The position of one styled item in override-depth order.
///
/// A style whose override walk revisits a style or passes the graph limit
/// states no depth. Absence takes a position of its own, after every stated
/// depth; it is not read as the deepest style.
fn style_application_order(
    id: u64,
    exchange: &Exchange,
    graph_limit: usize,
    ctx: &DecodeContext<'_>,
) -> Result<(bool, Option<u32>), CodecError> {
    let depth = style_depth(id, exchange, &mut BTreeSet::new(), 0, graph_limit, ctx)?;
    Ok((depth.is_none(), depth))
}

fn style_depth(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    depth: usize,
    graph_limit: usize,
    ctx: &DecodeContext<'_>,
) -> Result<Option<u32>, CodecError> {
    if depth >= graph_limit || active.contains(&id) {
        return Ok(None);
    }
    let _nested = ctx.enter_nested("step_presentation_style_depth_walk")?;
    ctx.insert_btree_set(active, id, "step_presentation_style_depth_active")?;
    let result = if let Some(style) = exchange.records().get(&id) {
        if let Some(base) = overridden_style(style) {
            style_depth(base, exchange, active, depth + 1, graph_limit, ctx)?
                .and_then(|depth| depth.checked_add(1))
        } else {
            Some(0)
        }
    } else {
        None
    };
    active.remove(&id);
    Ok(result)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SurfaceSideRank {
    NoUsage,
    Negative,
    Positive,
    Both,
}

struct ColorCandidate {
    rank: SurfaceSideRank,
    id: u64,
    color: Color,
    name: Option<String>,
}

enum ColorResolution {
    Candidate(ColorCandidate),
    Ambiguous { rank: SurfaceSideRank },
}

type CachedColor = Option<ColorResolution>;

fn clone_color_resolution(
    resolution: &CachedColor,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<CachedColor, CodecError> {
    Ok(match resolution {
        Some(ColorResolution::Candidate(candidate)) => {
            Some(ColorResolution::Candidate(ColorCandidate {
                rank: candidate.rank,
                id: candidate.id,
                color: candidate.color,
                name: candidate
                    .name
                    .as_deref()
                    .map(|name| ctx.copy_retained_text(name, operation))
                    .transpose()?,
            }))
        }
        Some(ColorResolution::Ambiguous { rank }) => {
            Some(ColorResolution::Ambiguous { rank: *rank })
        }
        None => None,
    })
}

impl ColorResolution {
    fn priority(&self) -> SurfaceSideRank {
        match self {
            Self::Candidate(candidate) => candidate.rank,
            Self::Ambiguous { rank } => *rank,
        }
    }

    fn with_min_rank(self, rank: SurfaceSideRank) -> Self {
        match self {
            Self::Candidate(mut candidate) => {
                candidate.rank = candidate.rank.max(rank);
                Self::Candidate(candidate)
            }
            Self::Ambiguous {
                rank: candidate_rank,
            } => Self::Ambiguous {
                rank: candidate_rank.max(rank),
            },
        }
    }
}

fn combine_color_resolutions(
    resolutions: impl IntoIterator<Item = Result<CachedColor, CodecError>>,
) -> Result<CachedColor, CodecError> {
    let mut best_priority = None;
    let mut best = None;
    let mut ambiguous = false;
    for resolution in resolutions {
        let Some(resolution) = resolution? else {
            continue;
        };
        let priority = resolution.priority();
        let replace = best_priority.is_none_or(|current| priority > current);
        if replace {
            let is_ambiguous = matches!(&resolution, ColorResolution::Ambiguous { .. });
            best_priority = Some(priority);
            best = match resolution {
                ColorResolution::Candidate(candidate) => Some(candidate),
                ColorResolution::Ambiguous { .. } => None,
            };
            ambiguous = is_ambiguous;
            continue;
        }
        if best_priority == Some(priority) {
            match resolution {
                ColorResolution::Candidate(candidate) => {
                    let Some(current) = best.as_mut() else {
                        ambiguous = true;
                        continue;
                    };
                    let same_rgb = current.color.r() == candidate.color.r()
                        && current.color.g() == candidate.color.g()
                        && current.color.b() == candidate.color.b();
                    if !same_rgb {
                        ambiguous = true;
                    } else if candidate.color.a() < current.color.a()
                        || (candidate.color.a() == current.color.a() && candidate.id < current.id)
                    {
                        *current = candidate;
                    }
                }
                ColorResolution::Ambiguous { .. } => ambiguous = true,
            }
        }
    }
    let Some(rank) = best_priority else {
        return Ok(None);
    };
    if ambiguous {
        Ok(Some(ColorResolution::Ambiguous { rank }))
    } else {
        Ok(best.map(ColorResolution::Candidate))
    }
}

/// Small style searches keep their memo entries in the local frame. Wider
/// graphs allocate checked ordered-map entries after the inline slots fill.
#[derive(Default)]
struct ColorCache {
    inline: [Option<((u64, StyleDomain), CachedColor)>; 8],
    overflow: BTreeMap<(u64, StyleDomain), CachedColor>,
}

impl ColorCache {
    fn get(
        &self,
        key: &(u64, StyleDomain),
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<&CachedColor>, CodecError> {
        for slot in &self.inline {
            ctx.charge_work(1, "step_presentation_color_cache_lookup")?;
            let Some((stored, value)) = slot else {
                break;
            };
            if stored == key {
                return Ok(Some(value));
            }
        }
        ctx.charge_work(1, "step_presentation_color_cache_lookup")?;
        Ok(self.overflow.get(key))
    }

    fn insert(
        &mut self,
        key: (u64, StyleDomain),
        value: CachedColor,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        for slot in &mut self.inline {
            ctx.charge_work(1, "step_presentation_color_cache_entries")?;
            match slot {
                Some((stored, cached)) if *stored == key => {
                    *cached = value;
                    return Ok(());
                }
                None => {
                    *slot = Some((key, value));
                    return Ok(());
                }
                Some(_) => {}
            }
        }
        ctx.insert_btree_map(
            &mut self.overflow,
            key,
            value,
            "step_presentation_color_cache_entries",
        )?;
        Ok(())
    }

    fn keys(&self) -> impl Iterator<Item = &(u64, StyleDomain)> {
        self.inline
            .iter()
            .filter_map(|slot| slot.as_ref().map(|(key, _)| key))
            .chain(self.overflow.keys())
    }
}

struct ColorSearchState<'a, 'ctx> {
    storage: &'a std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'ctx>>,
    active: &'a mut BTreeSet<u64>,
    cache: &'a mut ColorCache,
    losses: &'a mut Vec<LossNote>,
    invalid_surface_sides: &'a mut BTreeSet<u64>,
}

fn find_color(
    id: u64,
    exchange: &Exchange,
    domain: StyleDomain,
    state: ColorSearchState<'_, '_>,
    depth: usize,
    ctx: &DecodeContext<'_>,
) -> Result<CachedColor, CodecError> {
    let ColorSearchState {
        storage,
        active,
        cache,
        losses,
        invalid_surface_sides,
    } = state;
    if depth >= 256 {
        return Ok(None);
    }
    if let Some(result) = cache.get(&(id, domain), ctx)? {
        return clone_color_resolution(result, ctx, "step_presentation_color_cache_copy");
    }
    let Some(record) = exchange.records().get(&id) else {
        return Ok(None);
    };
    if is_presentation_style_by_context(record) {
        return Ok(None);
    }
    if active.contains(&id) {
        return Ok(None);
    }
    let _nested = ctx.enter_nested("step_presentation_color_walk")?;
    storage
        .borrow_mut()
        .with_storage(|| ctx.insert_btree_set(active, id, "step_presentation_color_active"))?;
    let transparency = if domain == StyleDomain::Surface {
        surface_transparency(id, record, exchange, losses, ctx)?
    } else {
        None
    };
    let result = (|| -> Result<CachedColor, CodecError> {
        let side_rank = if domain == StyleDomain::Surface {
            let Some(rank) =
                surface_side_rank(id, record, losses, invalid_surface_sides, storage, ctx)?
            else {
                return Ok(None);
            };
            rank
        } else {
            SurfaceSideRank::NoUsage
        };
        let name = record.simple_name().or_else(|| {
            record.partials.iter().find_map(|partial| {
                matches!(
                    partial.name.as_str(),
                    "COLOUR_RGB" | "DRAUGHTING_PRE_DEFINED_COLOUR"
                )
                .then_some(partial.name.as_str())
            })
        });
        let record_domain = record.partials.iter().find_map(|partial| {
            if partial.name.starts_with("SURFACE_STYLE") {
                Some(StyleDomain::Surface)
            } else if partial.name == "CURVE_STYLE" {
                Some(StyleDomain::Curve)
            } else if partial.name == "POINT_STYLE" {
                Some(StyleDomain::Point)
            } else {
                None
            }
        });
        let incompatible = record_domain
            .is_some_and(|candidate| domain != StyleDomain::Any && candidate != domain);
        if incompatible {
            for reference in record
                .partials
                .iter()
                .flat_map(|partial| partial.parameters.iter())
                .flat_map(|value| references(value, ctx))
            {
                let reference = reference?;
                // The recursive search caches the colour and records its losses.
                find_color(
                    reference,
                    exchange,
                    domain,
                    ColorSearchState {
                        storage,
                        active: &mut *active,
                        cache: &mut *cache,
                        losses: &mut *losses,
                        invalid_surface_sides: &mut *invalid_surface_sides,
                    },
                    depth + 1,
                    ctx,
                )?;
            }
            return Ok(None);
        }
        match name {
            Some("COLOUR_RGB") => {
                let Some(rgb) = record
                    .partials
                    .iter()
                    .find(|partial| partial.name == "COLOUR_RGB")
                else {
                    return Ok(None);
                };
                let offset = usize::from(record.partials.len() == 1);
                let Some(r) = rgb.parameters.get(offset).and_then(ValueExt::number) else {
                    return Ok(None);
                };
                let Some(g) = rgb.parameters.get(offset + 1).and_then(ValueExt::number) else {
                    return Ok(None);
                };
                let Some(b) = rgb.parameters.get(offset + 2).and_then(ValueExt::number) else {
                    return Ok(None);
                };
                let name_value = if record.partials.len() == 1 {
                    rgb.parameters.first()
                } else {
                    record
                        .partials
                        .iter()
                        .find(|partial| partial.name == "COLOUR_SPECIFICATION")
                        .and_then(|partial| partial.parameters.first())
                };
                let Some((r, g, b)) = cadmpeg_core::convert::f32_from_f64(r)
                    .zip(cadmpeg_core::convert::f32_from_f64(g))
                    .zip(cadmpeg_core::convert::f32_from_f64(b))
                    .map(|((r, g), b)| (r, g, b))
                else {
                    return Ok(None);
                };
                let Some(color) = Color::new(r, g, b, 1.0) else {
                    return Ok(None);
                };
                let name = name_value
                    .map(|value| {
                        decode_text_charged(
                            exchange,
                            value,
                            losses,
                            id,
                            "colour name",
                            StepLossCode::AttributeStringInvalid,
                            ctx,
                        )
                    })
                    .transpose()?
                    .flatten();
                Ok(Some(ColorResolution::Candidate(ColorCandidate {
                    rank: side_rank,
                    id,
                    color,
                    name,
                })))
            }
            Some("DRAUGHTING_PRE_DEFINED_COLOUR") => {
                let name_value = if record.partials.len() == 1 {
                    record.parameter(0)
                } else {
                    record
                        .partials
                        .iter()
                        .find(|partial| partial.name == "PRE_DEFINED_ITEM")
                        .and_then(|partial| partial.parameters.first())
                };
                let Some(name_value) = name_value else {
                    return Ok(None);
                };
                let Some(name) = decode_text_charged(
                    exchange,
                    name_value,
                    losses,
                    id,
                    "predefined colour name",
                    StepLossCode::AttributeStringInvalid,
                    ctx,
                )?
                else {
                    return Ok(None);
                };
                Ok(predefined(&name).map(|color| {
                    ColorResolution::Candidate(ColorCandidate {
                        rank: side_rank,
                        id,
                        color,
                        name: Some(name),
                    })
                }))
            }
            _ => combine_color_resolutions(
                record
                    .partials
                    .iter()
                    .flat_map(|partial| partial.parameters.iter())
                    .flat_map(|value| references(value, ctx))
                    .map(|reference| {
                        let reference = reference?;
                        find_color(
                            reference,
                            exchange,
                            domain,
                            ColorSearchState {
                                storage,
                                active: &mut *active,
                                cache: &mut *cache,
                                losses: &mut *losses,
                                invalid_surface_sides: &mut *invalid_surface_sides,
                            },
                            depth + 1,
                            ctx,
                        )
                    })
                    .map(|result| {
                        result.map(|candidate| {
                            candidate.map(|candidate| candidate.with_min_rank(side_rank))
                        })
                    }),
            ),
        }
    })();
    active.remove(&id);
    let mut result = result?;
    if let Some(transparency) = transparency {
        match result.as_mut() {
            Some(ColorResolution::Candidate(candidate)) => {
                if let Some(alpha) = cadmpeg_core::convert::f32_from_f64(1.0 - transparency.get()) {
                    candidate.color = candidate.color.with_alpha(alpha).unwrap_or(candidate.color);
                }
            }
            Some(ColorResolution::Ambiguous { .. }) => {}
            None => {}
        }
    }
    let cached = storage.borrow_mut().with_storage(|| {
        clone_color_resolution(&result, ctx, "step_presentation_color_cache_value")
    })?;
    storage
        .borrow_mut()
        .with_storage(|| cache.insert((id, domain), cached, ctx))?;
    Ok(result)
}

struct TransparencyDetails<'a>(&'a [(u64, Fraction)]);

impl std::fmt::Display for TransparencyDetails<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, (property_id, transparency)) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str(", ")?;
            }
            write!(formatter, "#{property_id}={}", transparency.get())?;
        }
        Ok(())
    }
}

fn surface_transparency(
    id: u64,
    record: &RawRecord,
    exchange: &Exchange,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Fraction>, CodecError> {
    let mut candidates = Vec::new();
    for property_id in record
        .partials
        .iter()
        .filter(|partial| partial.name == "SURFACE_STYLE_RENDERING_WITH_PROPERTIES")
        .flat_map(|partial| {
            partial
                .parameters
                .iter()
                .flat_map(|value| references(value, ctx))
        })
    {
        let property_id = property_id?;
        let Some(property) = exchange.records().get(&property_id) else {
            continue;
        };
        let Some(transparency) = property
            .partials
            .iter()
            .find(|partial| partial.name == "SURFACE_STYLE_TRANSPARENT")
            .and_then(|partial| partial.parameters.first())
            .and_then(ValueExt::number)
            .and_then(Fraction::new)
        else {
            continue;
        };
        ctx.push_vec(
            &mut candidates,
            (property_id, transparency),
            "step_presentation_transparency_candidates",
        )?;
    }
    match candidates.as_slice() {
        [] => Ok(None),
        [(_, transparency)] => Ok(Some(*transparency)),
        _ => {
            let details = TransparencyDetails(&candidates);
            let message = ctx.format_retained(format_args!("surface style rendering #{id} has conflicting transparency properties ({details}); transparency omitted"), "step_presentation_transparency_conflict_text")?;
            ctx.push_vec(
                losses,
                StepLossCode::SurfaceTransparencyConflict.note(message),
                "step_presentation_losses",
            )?;
            Ok(None)
        }
    }
}

fn surface_side_rank(
    id: u64,
    record: &RawRecord,
    losses: &mut Vec<LossNote>,
    invalid_surface_sides: &mut BTreeSet<u64>,
    storage: &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<SurfaceSideRank>, CodecError> {
    let Some(partial) = record
        .partials
        .iter()
        .find(|partial| partial.name == "SURFACE_STYLE_USAGE")
    else {
        return Ok(Some(SurfaceSideRank::NoUsage));
    };
    let Some(side) = partial.parameters.first().and_then(ValueExt::enumeration) else {
        storage.borrow_mut().with_storage(|| {
            ctx.insert_btree_set(
                invalid_surface_sides,
                id,
                "step_presentation_invalid_surface_sides",
            )
        })?;
        ctx.push_vec(
            losses,
            StepLossCode::SurfaceSideInvalid.note(format!(
                "SURFACE_STYLE_USAGE #{id} has no valid surface_side; style omitted"
            )),
            "step_presentation_losses",
        )?;
        return Ok(None);
    };
    match side {
        "BOTH" => Ok(Some(SurfaceSideRank::Both)),
        "POSITIVE" => Ok(Some(SurfaceSideRank::Positive)),
        "NEGATIVE" => Ok(Some(SurfaceSideRank::Negative)),
        _ => {
            storage.borrow_mut().with_storage(|| {
                ctx.insert_btree_set(
                    invalid_surface_sides,
                    id,
                    "step_presentation_invalid_surface_sides",
                )
            })?;
            let message = ctx.format_retained(
                format_args!(
                    "SURFACE_STYLE_USAGE #{id} has invalid surface_side .{side}.; style omitted"
                ),
                "step_presentation_invalid_surface_side_text",
            )?;
            ctx.push_vec(
                losses,
                StepLossCode::SurfaceSideInvalid.note(message),
                "step_presentation_losses",
            )?;
            Ok(None)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum StyleDomain {
    Any,
    Surface,
    Curve,
    Point,
}

fn style_domain(
    id: u64,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<StyleDomain, CodecError> {
    style_domain_at(id, exchange, &mut BTreeSet::new(), ctx)
}

fn style_domain_at(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<StyleDomain, CodecError> {
    if active.contains(&id) {
        return Ok(StyleDomain::Any);
    }
    let _nested = ctx.enter_nested("step_presentation_style_domain_walk")?;
    let Some(record) = exchange.records().get(&id) else {
        active.remove(&id);
        return Ok(StyleDomain::Any);
    };
    let set_name = record.partials.iter().find_map(|partial| {
        matches!(
            partial.name.as_str(),
            "GEOMETRIC_SET" | "GEOMETRIC_CURVE_SET"
        )
        .then_some(partial.name.as_str())
    });
    if let Some(set_name) = set_name {
        ctx.insert_btree_set(active, id, "step_presentation_style_domain_active")?;
        let mut first = None;
        let mut same = true;
        for member in named_parameter(record, set_name, 1)
            .and_then(ValueExt::list)
            .into_iter()
            .flatten()
            .filter_map(ValueExt::reference)
        {
            let domain = style_domain_at(member, exchange, active, ctx)?;
            if first.is_some_and(|first| first != domain) {
                same = false;
            }
            if first.is_none() {
                first = Some(domain);
            }
        }
        if let Some(first) = first {
            active.remove(&id);
            return Ok(if same { first } else { StyleDomain::Any });
        }
    }
    let has_point = record.partials.iter().any(|partial| {
        let name = partial.name.as_str();
        name.contains("POINT") || name.contains("VERTEX")
    });
    if has_point {
        active.remove(&id);
        return Ok(StyleDomain::Point);
    }
    let has_curve = record.partials.iter().any(|partial| {
        let name = partial.name.as_str();
        name.contains("CURVE")
            || name.contains("EDGE")
            || name.contains("_LINE")
            || matches!(
                name,
                "LINE" | "POLYLINE" | "CIRCLE" | "ELLIPSE" | "HYPERBOLA" | "PARABOLA"
            )
    });
    if has_curve {
        active.remove(&id);
        return Ok(StyleDomain::Curve);
    }
    let result = if record.partials.iter().any(|partial| {
        let name = partial.name.as_str();
        name.contains("FACE")
            || name.contains("SURFACE")
            || name.contains("SOLID")
            || name.contains("SHELL")
            || matches!(
                name,
                "PLANE" | "CYLINDER" | "CONE" | "SPHERE" | "TORUS" | "DEGENERATE_TORUS"
            )
    }) {
        StyleDomain::Surface
    } else {
        StyleDomain::Any
    };
    active.remove(&id);
    Ok(result)
}

fn style_is_hidden(
    id: u64,
    hidden_style_ids: &BTreeSet<u64>,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if hidden_style_ids.contains(&id) || active.contains(&id) {
        return Ok(hidden_style_ids.contains(&id));
    }
    let _nested = ctx.enter_nested("step_presentation_hidden_style_walk")?;
    ctx.insert_btree_set(active, id, "step_presentation_hidden_style_active")?;
    let hidden = if let Some(base) = exchange.records().get(&id).and_then(overridden_style) {
        style_is_hidden(base, hidden_style_ids, exchange, active, ctx)?
    } else {
        false
    };
    active.remove(&id);
    Ok(hidden)
}

fn style_inherits_from(
    id: u64,
    ancestor: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if id == ancestor || active.contains(&id) {
        return Ok(id == ancestor);
    }
    let _nested = ctx.enter_nested("step_presentation_style_inheritance_walk")?;
    ctx.insert_btree_set(active, id, "step_presentation_style_inheritance_active")?;
    let inherits = if let Some(base) = exchange.records().get(&id).and_then(overridden_style) {
        style_inherits_from(base, ancestor, exchange, active, ctx)?
    } else {
        false
    };
    active.remove(&id);
    Ok(inherits)
}

fn contains_null_style(
    value: &Value,
    exchange: &Exchange,
    visited: &mut BTreeSet<u64>,
    depth: usize,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if depth >= 256 {
        return Ok(false);
    }
    let _nested = ctx.enter_nested("step_presentation_null_style_walk")?;
    match value {
        Value::Typed(name, _) if name == "NULL_STYLE" => Ok(true),
        Value::Typed(_, value) => contains_null_style(value, exchange, visited, depth + 1, ctx),
        Value::List(values) => {
            for value in values {
                if contains_null_style(value, exchange, visited, depth + 1, ctx)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Value::Reference(id) if !visited.contains(id) => {
            ctx.insert_btree_set(visited, *id, "step_presentation_null_style_visited")?;
            if let Some(record) = exchange.records().get(id) {
                for value in record
                    .partials
                    .iter()
                    .flat_map(|partial| partial.parameters.iter())
                {
                    if contains_null_style(value, exchange, visited, depth + 1, ctx)? {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn predefined(name: &str) -> Option<Color> {
    let (r, g, b) = if name.eq_ignore_ascii_case("black") {
        (0.0, 0.0, 0.0)
    } else if name.eq_ignore_ascii_case("white") {
        (1.0, 1.0, 1.0)
    } else if name.eq_ignore_ascii_case("red") {
        (1.0, 0.0, 0.0)
    } else if name.eq_ignore_ascii_case("green") {
        (0.0, 1.0, 0.0)
    } else if name.eq_ignore_ascii_case("blue") {
        (0.0, 0.0, 1.0)
    } else if name.eq_ignore_ascii_case("yellow") {
        (1.0, 1.0, 0.0)
    } else if name.eq_ignore_ascii_case("magenta") {
        (1.0, 0.0, 1.0)
    } else if name.eq_ignore_ascii_case("cyan") {
        (0.0, 1.0, 1.0)
    } else {
        return None;
    };
    Color::new(r, g, b, 1.0)
}
#[cfg(test)]
pub(crate) mod tests;
