// SPDX-License-Identifier: Apache-2.0
//! Collection admissions in the STEP PMI reader.

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::num::NonZeroU32;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const HEADER: &str = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;";
const TAIL: &str = "ENDSEC;END-ISO-10303-21;";

fn pmi_refuses(records: &str, operation: &str) {
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid PMI exchange");
    let arena = DecodeArena::new();
    let (setup_ctx, _) =
        DecodeContext::from_root_bytes(source.as_bytes(), &arena, &DecodePolicy::default())
            .expect("source fits setup policy");
    let mut setup_ir = cadmpeg_ir::document::CadIr::empty();
    let (geometry, _) = crate::reader::geometry::decode(&exchange, &mut setup_ir, &setup_ctx)
        .expect("geometry setup");
    let index =
        crate::reader::index::CarrierIndex::from_ir(&setup_ir, &setup_ctx).expect("carrier setup");
    let topology = crate::reader::topology::decode(&exchange, &mut setup_ir, &index, &setup_ctx)
        .expect("topology setup");
    let refused = (0..=32).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
            .expect("root fits collection policy");
        matches!(
            super::super::decode(&exchange, &geometry.value, &topology.value, &mut setup_ir.clone(), &ctx),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation
        )
    });
    assert!(refused, "no collection limit refused {operation}");
}

fn pmi_retained_refuses(records: &str, operation: &str) {
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid PMI exchange");
    let arena = DecodeArena::new();
    let (setup_ctx, _) =
        DecodeContext::from_root_bytes(source.as_bytes(), &arena, &DecodePolicy::default())
            .expect("source fits setup policy");
    let mut setup_ir = cadmpeg_ir::document::CadIr::empty();
    let (geometry, _) = crate::reader::geometry::decode(&exchange, &mut setup_ir, &setup_ctx)
        .expect("geometry setup");
    let index =
        crate::reader::index::CarrierIndex::from_ir(&setup_ir, &setup_ctx).expect("carrier setup");
    let topology = crate::reader::topology::decode(&exchange, &mut setup_ir, &index, &setup_ctx)
        .expect("topology setup");
    let refused = {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            operation,
            |limit| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_retained_bytes = limit;
                let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
                    .expect("root fits retained policy");

                (super::super::decode(
                    &exchange,
                    &geometry.value,
                    &topology.value,
                    &mut setup_ir.clone(),
                    &ctx,
                ))
                .map(|_| ())
            },
        );
        matches!(Err::<(), CodecError>(error), Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == operation)
    };
    assert!(refused, "no retained limit refused {operation}");
}

#[test]
fn pmi_base_aspects_refuse_collection_limit() {
    pmi_refuses("#1=DATUM('D');", "step_pmi_base_aspects");
}

#[test]
fn pmi_shape_aspects_refuse_collection_limit() {
    pmi_refuses("#1=DATUM('D');", "step_pmi_shape_aspects");
}

#[test]
fn pmi_targeted_aspects_refuse_collection_limit() {
    pmi_refuses("#1=DATUM('D');", "step_pmi_targeted_aspects");
}

#[test]
fn pmi_typed_claims_refuse_collection_limit() {
    pmi_refuses("#1=DATUM('D');", "step_pmi_typed_claims");
}

#[test]
fn pmi_hidden_annotation_ids_refuse_collection_limit() {
    pmi_refuses(
        "#1=ANNOTATION_TEXT_OCCURRENCE('note',());#2=INVISIBILITY((#1));",
        "step_pmi_hidden_annotation_ids",
    );
}

#[test]
fn pmi_loss_slots_refuse_collection_limit() {
    pmi_refuses("#1=PLUS_MINUS_TOLERANCE($);", "step_pmi_losses");
}

#[test]
fn pmi_plus_minus_references_refuse_collection_limit() {
    pmi_refuses(
        "#1=DIMENSIONAL_SIZE('size',#2);#2=SHAPE_ASPECT('edge',$,.F.);#3=PLUS_MINUS_TOLERANCE(#1,#4);#4=TOLERANCE_VALUE(0.1,0.2);",
        "step_pmi_plus_minus_references",
    );
}

#[test]
fn pmi_geometric_tolerance_references_refuse_collection_limit() {
    pmi_refuses(
        "#1=FLATNESS_TOLERANCE('flat',$,#2,#3);#2=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#4);#3=SHAPE_ASPECT('face',$,.F.);#4=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));",
        "step_pmi_geometric_tolerance_references",
    );
}

fn target_refusal(limit: u64) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits collection policy");
    super::super::targets([Ok(1), Ok(2)], &ctx).expect_err("two target IDs exceed the limit")
}

#[test]
fn pmi_target_ids_refuse_collection_limit() {
    assert!(matches!(
        target_refusal(0),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_target_ids"
    ));
}

#[test]
fn pmi_target_items_refuse_collection_limit() {
    assert!(matches!(
        target_refusal(1),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_target_items"
    ));
}

#[test]
fn pmi_geometric_usage_identity_refuses_retained_limit() {
    let id = crate::ids::data(crate::ids::kind!("body"), 1);
    let target = cadmpeg_ir::pmi::PmiTarget::Body {
        body: cadmpeg_ir::ids::BodyId::from(id.clone()),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(id.as_str().len() - 1).expect("ID fits u64");
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits retained policy");
    assert!(matches!(
        super::super::copy_pmi_target(&target, &ctx, "step_pmi_geometric_usage_identity"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_pmi_geometric_usage_identity"
    ));
}

#[test]
fn pmi_presentation_semantics_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits collection policy");
    assert!(matches!(
        ctx.push_vec(&mut Vec::new(), 1u64, "step_pmi_presentation_semantics"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_presentation_semantics"
    ));
}

#[test]
fn pmi_other_datum_target_form_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 10;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits retained policy");
    assert!(matches!(
        super::super::datum_target_form("custom form", &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_pmi_datum_target_form_copy"
    ));
}

#[test]
fn pmi_other_dimension_name_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 10;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits retained policy");
    assert!(matches!(
        super::super::dimension_kind("CUSTOM_SIZE", &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_pmi_other_dimension_name"
    ));
}

#[test]
fn pmi_modifier_text_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits retained policy");
    let value = crate::parse::Value::Enumeration("ABC".into());
    assert!(matches!(
        super::super::modifier_values(&value, &mut Vec::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_pmi_modifier_text"
    ));
}

#[test]
fn pmi_modifier_items_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits collection policy");
    let value = crate::parse::Value::Enumeration("ABC".into());
    assert!(matches!(
        super::super::modifier_values(&value, &mut Vec::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_modifier_items"
    ));
}

#[test]
fn pmi_modifier_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits depth policy");
    let value = crate::parse::Value::List(vec![crate::parse::Value::Enumeration("ABC".into())]);
    assert!(matches!(
        super::super::modifier_values(&value, &mut Vec::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_pmi_modifier_walk"
    ));
}

#[test]
fn pmi_datum_id_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits depth policy");
    let value = crate::parse::Value::List(vec![crate::parse::Value::Reference(1)]);
    assert!(matches!(
        super::super::visit_datum_ids(&value, &ctx, &mut |_| Ok(())),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_pmi_datum_id_walk"
    ));
}

#[test]
fn pmi_datum_modifier_text_refuses_retained_limit() {
    let source = format!("{HEADER}#1=ITEM();{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits retained policy");
    let mut losses = Vec::new();
    let mut measurements = super::super::MeasureContext {
        length_scale: 1.0,
        angle_scale: 1.0,
        graph_limit: 64,
        losses: &mut losses,
    };
    let value = crate::parse::Value::Enumeration("ABC".into());
    assert!(matches!(
        super::super::modifier_text(&value, &exchange, &mut HashSet::new(), &mut measurements, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_pmi_datum_modifier_text"
    ));
}

fn datum_reference_refuses(records: &str, operation: &str) {
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid datum exchange");
    let mut annotations = super::super::Annotations::default();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    crate::test_support::with_service_context(source.as_bytes(), |_, ctx| {
        annotations
            .push(
                ctx,
                &mut ir,
                2,
                super::super::annotations::AnnotationDraft {
                    name: None,
                    targets: Vec::new(),
                    visible: None,
                    definition: cadmpeg_ir::pmi::PmiDefinition::Datum {
                        identification: "A".into(),
                    },
                },
            )
            .expect("datum annotation setup");
    });
    let refused = (0..=8).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
            .expect("empty root fits collection policy");
        let mut losses = Vec::new();
        let mut measurements = super::super::MeasureContext {
            length_scale: 1.0,
            angle_scale: 1.0,
            graph_limit: 64,
            losses: &mut losses,
        };
        matches!(
            super::super::datum_references_for_compartment(&crate::parse::Value::Reference(1), NonZeroU32::new(1).expect("positive precedence"), &exchange, &annotations, &mut HashSet::new(), &mut measurements, &ctx),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation
        )
    });
    assert!(refused, "datum reference never refused {operation}");
}

#[test]
fn pmi_datum_reference_items_refuse_collection_limit() {
    datum_reference_refuses(
        "#1=DATUM_REFERENCE_COMPARTMENT($,$,$,$,#2,$);#2=DATUM('A');",
        "step_pmi_datum_reference_items",
    );
}

#[test]
fn pmi_datum_modifier_items_refuse_collection_limit() {
    datum_reference_refuses(
        "#1=DATUM_REFERENCE_COMPARTMENT($,$,$,$,#2,(.ABC.));#2=DATUM('A');",
        "step_pmi_datum_modifier_items",
    );
}

#[test]
fn pmi_datum_system_references_refuse_collection_limit() {
    pmi_refuses(
        "#1=DATUM('A');#2=DATUM_REFERENCE_COMPARTMENT($,$,$,$,#1,$);#3=DATUM_SYSTEM('S','',#4,.F.,(#2));#4=ITEM();",
        "step_pmi_datum_system_references",
    );
}

#[test]
fn pmi_datum_compartments_refuse_collection_limit() {
    pmi_refuses(
        "#1=DATUM('A');#2=DATUM_REFERENCE_COMPARTMENT($,$,$,$,#1,$);#3=DATUM_SYSTEM('S','',#4,.F.,(#2));#4=ITEM();",
        "step_pmi_datum_compartments",
    );
}

#[test]
fn pmi_datum_common_groups_refuse_collection_limit() {
    pmi_refuses(
        "#1=DATUM('A');#2=DATUM('B');#3=DATUM_REFERENCE_ELEMENT('',$,#7,.F.,#1,());#4=DATUM_REFERENCE_ELEMENT('',$,#7,.F.,#2,());#5=DATUM_REFERENCE_COMPARTMENT('',$,#7,.F.,COMMON_DATUM_LIST((#3,#4)),());#6=DATUM_SYSTEM('S','',#7,.F.,(#5));#7=ITEM();",
        "step_pmi_datum_common_groups",
    );
}

fn placement_refuses(operation: &str) {
    let source = format!(
        "{HEADER}#1=CARTESIAN_POINT('',(0.,0.,0.));#2=DIRECTION('',(0.,0.,1.));#3=DIRECTION('',(1.,0.,0.));#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);#5=TEXT_LITERAL('note',#4,'left',.RIGHT.,$);{TAIL}"
    );
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid placement exchange");
    let arena = DecodeArena::new();
    let (setup_ctx, _) =
        DecodeContext::from_root_bytes(source.as_bytes(), &arena, &DecodePolicy::default())
            .expect("setup root fits policy");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let (geometry, _) =
        crate::reader::geometry::decode(&exchange, &mut ir, &setup_ctx).expect("geometry setup");
    let refused = (0..=32).any(|limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
            .expect("empty root fits collection policy");
        matches!(
            super::super::collect_placement_candidates(5, &exchange, &geometry.value, &mut BTreeMap::new(), &mut BTreeMap::new(), 0, &ctx),
            Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation
        )
    });
    assert!(refused, "placement walk did not refuse {operation}");
}

#[test]
fn pmi_placement_visited_refuses_collection_limit() {
    placement_refuses("step_pmi_placement_visited");
}

#[test]
fn pmi_placement_candidates_refuse_collection_limit() {
    placement_refuses("step_pmi_placement_candidates");
}

#[test]
fn pmi_placement_walk_refuses_depth_limit() {
    let source = format!("{HEADER}#1=ITEM();{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let (setup_ctx, _) =
        DecodeContext::from_root_bytes(source.as_bytes(), &arena, &DecodePolicy::default())
            .expect("setup root fits policy");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let (geometry, _) =
        crate::reader::geometry::decode(&exchange, &mut ir, &setup_ctx).expect("geometry setup");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits depth policy");
    assert!(matches!(
        super::super::collect_placement_candidates(1, &exchange, &geometry.value, &mut BTreeMap::new(), &mut BTreeMap::new(), 0, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_pmi_placement_walk"
    ));
}

fn target_slot_refusal(operation: &'static str) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits collection policy");
    super::super::push_target(
        &mut Vec::new(),
        cadmpeg_ir::pmi::PmiTarget::ShapeAspect {
            source_id: crate::reader::step_source_id(1),
        },
        &ctx,
        operation,
    )
    .expect_err("target slot exceeds collection limit")
}

#[test]
fn pmi_datum_basis_targets_refuse_collection_limit() {
    assert!(matches!(
        target_slot_refusal("step_pmi_datum_basis_targets"),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_datum_basis_targets"
    ));
}

#[test]
fn pmi_topology_targets_refuse_collection_limit() {
    assert!(matches!(
        target_slot_refusal("step_pmi_topology_targets"),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_topology_targets"
    ));
}

#[test]
fn pmi_geometric_usage_targets_refuse_collection_limit() {
    assert!(matches!(
        target_slot_refusal("step_pmi_geometric_usage_targets"),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_geometric_usage_targets"
    ));
}

#[test]
fn pmi_usage_annotation_indices_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits collection policy");
    assert!(matches!(
        ctx.insert_btree_set(&mut std::collections::BTreeSet::new(), 1u64, "step_pmi_usage_annotation_indices").map(|_| ()),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_usage_annotation_indices"
    ));
}

fn measure_id_refusal(limit: u64, depth_limit: Option<u64>) -> CodecError {
    let source = format!("{HEADER}#1=MEASURE_REPRESENTATION_ITEM();{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid measure exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    if let Some(depth_limit) = depth_limit {
        policy.limits.max_recursion_depth = depth_limit;
    }
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    super::super::collect_measure_ids(
        &crate::parse::Value::Reference(1),
        &exchange,
        &mut std::collections::BTreeSet::new(),
        0,
        64,
        &mut std::collections::BTreeSet::new(),
        &ctx,
    )
    .expect_err("measure ID walk exceeds the limit")
}

#[test]
fn pmi_measure_active_ids_refuse_collection_limit() {
    assert!(matches!(
        measure_id_refusal(0, None),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_measure_active_ids"
    ));
}

#[test]
fn pmi_measure_ids_refuse_collection_limit() {
    assert!(matches!(
        measure_id_refusal(1, None),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_measure_ids"
    ));
}

#[test]
fn pmi_measure_id_walk_refuses_depth_limit() {
    assert!(matches!(
        measure_id_refusal(8, Some(0)),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_pmi_measure_id_walk"
    ));
}

fn measure_eval_refusal(limit: u64, depth_limit: Option<u64>, record: &str) -> CodecError {
    let source = format!("{HEADER}{record}{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid measure exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    if let Some(depth_limit) = depth_limit {
        policy.limits.max_recursion_depth = depth_limit;
    }
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let mut losses = Vec::new();
    let mut measurements = super::super::MeasureContext {
        length_scale: 1.0,
        angle_scale: 1.0,
        graph_limit: 64,
        losses: &mut losses,
    };
    super::super::measure(
        &crate::parse::Value::Reference(1),
        &exchange,
        &mut measurements,
        &ctx,
    )
    .expect_err("measure evaluation exceeds the limit")
}

#[test]
fn pmi_measure_eval_active_refuses_collection_limit() {
    assert!(matches!(
        measure_eval_refusal(0, None, "#1=MEASURE_REPRESENTATION_ITEM();"),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_measure_eval_active"
    ));
}

#[test]
fn pmi_measure_eval_walk_refuses_depth_limit() {
    assert!(matches!(
        measure_eval_refusal(8, Some(0), "#1=MEASURE_REPRESENTATION_ITEM();"),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_pmi_measure_eval_walk"
    ));
}

#[test]
fn pmi_measure_quantity_walk_refuses_depth_limit() {
    assert!(matches!(
        measure_eval_refusal(8, Some(1), "#1=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),$);"),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_pmi_measure_quantity_walk"
    ));
}

#[test]
fn pmi_measure_loss_slot_refuses_collection_limit() {
    assert!(matches!(
        measure_eval_refusal(1, None, "#1=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),$);"),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_pmi_losses"
    ));
}

#[test]
fn pmi_defined_area_unit_text_refuses_retained_limit() {
    pmi_retained_refuses(
        "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#2=(LENGTH_MEASURE_WITH_UNIT() MEASURE_WITH_UNIT(LENGTH_MEASURE(0.05),#1));#3=ITEM();#4=(FLATNESS_TOLERANCE('tol','',#2,#3) GEOMETRIC_TOLERANCE_WITH_DEFINED_AREA_UNIT(.PROJECTED.,#2));",
        "step_pmi_defined_area_unit_text",
    );
}

#[test]
fn pmi_invalid_tolerance_text_refuses_retained_limit() {
    pmi_retained_refuses(
        "#1=(CUSTOM_TOLERANCE_NAME_WITH_LONG_SOURCE_TEXT() FLATNESS_TOLERANCE('tol','',$,$));",
        "step_pmi_invalid_tolerance_text",
    );
}

#[test]
fn pmi_target_reference_filter_propagates_depth_refusal() {
    let value = crate::parse::Value::List(vec![crate::parse::Value::Reference(1)]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let ids = crate::reader::reference::references(&value, ctx).filter(|id| match id {
            Ok(id) => *id == 1,
            Err(_) => true,
        });
        assert!(
            matches!(super::super::targets(ids, ctx), Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_reference_value_walk")
        );
    });
}
