// SPDX-License-Identifier: Apache-2.0
//! Caller-budget refusals for STEP unit resolution.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::PositiveReal;

const HEADER: &str = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;";
const TAIL: &str = "ENDSEC;END-ISO-10303-21;";
const LENGTH: &str = "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));";
const ANGLE: &str = "#1=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));";

fn unit_refusal(
    records: &str,
    angle: bool,
    collection_limit: u64,
    depth_limit: Option<u64>,
) -> CodecError {
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid unit exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    if let Some(limit) = depth_limit {
        policy.limits.max_recursion_depth = limit;
    }
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let mut active = BTreeSet::new();
    let result = if angle {
        super::super::unit_scale_radians(1, &exchange, &mut active, &ctx)
    } else {
        super::super::unit_scale_mm(1, &exchange, &mut active, &ctx)
    };
    result.expect_err("unit resolution exceeds the limit")
}

#[test]
fn length_unit_active_refuses_collection_limit() {
    assert!(matches!(
        unit_refusal(LENGTH, false, 0, None),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_length_unit_active"
    ));
}

#[test]
fn angle_unit_active_refuses_collection_limit() {
    assert!(matches!(
        unit_refusal(ANGLE, true, 0, None),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_angle_unit_active"
    ));
}

#[test]
fn length_unit_scale_walk_refuses_depth_limit() {
    assert!(matches!(
        unit_refusal(LENGTH, false, 1, Some(0)),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_length_unit_scale_walk"
    ));
}

#[test]
fn angle_unit_scale_walk_refuses_depth_limit() {
    assert!(matches!(
        unit_refusal(ANGLE, true, 1, Some(0)),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_angle_unit_scale_walk"
    ));
}

fn document_refusal(records: &str, collection_limit: u64, with_context: bool) -> CodecError {
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid document units");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let result =
        super::super::document_unit_scale(&exchange, super::super::UnitScaleKind::Length, &ctx);
    assert_eq!(
        with_context,
        records.contains("GLOBAL_UNIT_ASSIGNED_CONTEXT")
    );
    result.expect_err("document unit collection exceeds the limit")
}

#[test]
fn document_unit_ids_refuse_collection_limit() {
    let records = format!(
        "{LENGTH}#2=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));"
    );
    assert!(matches!(
        document_refusal(&records, 0, true),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_document_unit_ids"
    ));
}

#[test]
fn document_unit_scales_refuse_collection_limit() {
    let records = format!(
        "{LENGTH}#2=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));"
    );
    assert!(matches!(
        document_refusal(&records, 2, true),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_document_unit_scales"
    ));
}

#[test]
fn document_context_scales_refuse_collection_limit() {
    let records = format!(
        "{LENGTH}#2=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));"
    );
    assert!(matches!(
        document_refusal(&records, 3, true),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_document_context_scales"
    ));
}

#[test]
fn document_fallback_scales_refuse_collection_limit() {
    assert!(matches!(
        document_refusal(LENGTH, 1, false),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_document_fallback_scales"
    ));
}

#[test]
fn context_length_scales_refuse_collection_limit() {
    let records = format!(
        "{LENGTH}#2=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));"
    );
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid context units");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    assert!(matches!(
        super::super::context_unit_scales(2, &exchange, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_context_length_scales"
    ));
}

#[test]
fn context_angle_scales_refuse_collection_limit() {
    let records = format!(
        "{ANGLE}#2=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));"
    );
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid context units");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    assert!(matches!(
        super::super::context_unit_scales(2, &exchange, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_context_angle_scales"
    ));
}

fn candidate_refusal(angle: bool, limit: u64) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let group = if angle {
        "step_angle_candidate_groups"
    } else {
        "step_length_candidate_groups"
    };
    super::super::add_unit_candidate(&mut BTreeMap::new(), 1, PositiveReal::ONE, &ctx, group)
        .expect_err("candidate exceeds the limit")
}

#[test]
fn length_candidate_groups_refuse_collection_limit() {
    assert!(matches!(candidate_refusal(false, 0),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_length_candidate_groups"));
}

#[test]
fn angle_candidate_groups_refuse_collection_limit() {
    assert!(matches!(candidate_refusal(true, 0),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_angle_candidate_groups"));
}

fn scope_refusal(records: &str, collection_limit: u64, depth_limit: Option<u64>) -> CodecError {
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid scope exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    if let Some(limit) = depth_limit {
        policy.limits.max_recursion_depth = limit;
    }
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    super::super::collect_unit_scope_members(1, &exchange, &mut BTreeSet::new(), &ctx)
        .expect_err("scope traversal exceeds the limit")
}

#[test]
fn unit_scope_members_refuse_collection_limit() {
    assert!(matches!(scope_refusal("#1=ITEM();", 0, None),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_unit_scope_members"));
}

#[test]
fn unit_scope_pending_refuses_collection_limit() {
    assert!(matches!(scope_refusal("#1=ITEM(#2);#2=ITEM();", 1, None),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_unit_scope_pending"));
}

#[test]
fn unit_scope_reference_walk_refuses_depth_limit() {
    assert!(
        matches!(scope_refusal("#1=ITEM(#2);#2=ITEM();", 10, Some(1)),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_reference_value_walk")
    );
}

#[test]
fn unit_selected_scales_refuse_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let candidates = BTreeMap::from([(1, Some(PositiveReal::ONE))]);
    let default = PositiveReal::new(2.0).expect("positive default");
    assert!(matches!(
        super::super::finalize_unit_candidates(candidates, default, "length", &mut Vec::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_unit_selected_scales"
    ));
}

#[test]
fn conflicting_unit_loss_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let candidates = BTreeMap::from([(1, None)]);
    assert!(matches!(
        super::super::finalize_unit_candidates(candidates, PositiveReal::ONE, "length", &mut Vec::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_geometry_losses"
    ));
}

fn uncertainty_refusal(collection_limit: u64) -> CodecError {
    let records = "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#2=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#1,'first_accuracy','');#3=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#2)) REPRESENTATION_CONTEXT('model','3D'));";
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid uncertainty exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    match super::super::linear_uncertainty(&exchange, &ctx) {
        Err(error) => error,
        Ok(_) => panic!("uncertainty collection did not refuse"),
    }
}

#[test]
fn uncertainty_context_measures_refuse_collection_limit() {
    assert!(matches!(
        uncertainty_refusal(1),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_uncertainty_context_measures"
    ));
}

#[test]
fn uncertainty_distinct_candidates_refuse_collection_limit() {
    assert!(matches!(
        uncertainty_refusal(2),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_uncertainty_distinct_candidates"
    ));
}

#[test]
fn shared_geometry_is_visited_once_per_unit_scope() {
    use std::fmt::Write;
    let mut records = format!("{LENGTH}#2=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('model','3D'));#3=CARTESIAN_POINT('',(1.,2.,3.));");
    for id in 10..1010 {
        write!(records, "#{id}=GEOMETRIC_SET((#3));").expect("write fixture");
    }
    records.push_str("#4=SHAPE_REPRESENTATION('',(");
    for id in 10..1010 {
        if id != 10 {
            records.push(',');
        }
        write!(records, "#{id}").expect("write fixture");
    }
    records.push_str("),#2);");
    let source = format!("{HEADER}{records}{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 7000;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy).expect("context");
    let scales = super::super::resolve_unit_scales(
        &exchange,
        PositiveReal::ONE,
        PositiveReal::ONE,
        &mut Vec::new(),
        &ctx,
    )
    .expect("shared members fit linear budget");
    assert_eq!(scales.length([3]), PositiveReal::ONE);
}

#[test]
fn repeated_unit_candidates_keep_one_first_scale_and_a_final_conflict() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut candidates = BTreeMap::new();
    let operation = "step_length_candidate_groups";
    for _ in 0..1_000 {
        super::super::add_unit_candidate(&mut candidates, 1, PositiveReal::ONE, &ctx, operation)
            .expect("one candidate slot");
    }
    assert_eq!(candidates[&1], Some(PositiveReal::ONE));
    super::super::add_unit_candidate(
        &mut candidates,
        1,
        PositiveReal::new(2.0).expect("positive"),
        &ctx,
        operation,
    )
    .expect("conflict replaces the same scalar state");
    super::super::add_unit_candidate(&mut candidates, 1, PositiveReal::ONE, &ctx, operation)
        .expect("later agreement cannot clear conflict");
    assert_eq!(candidates[&1], None);
}

#[test]
fn unit_scope_reuses_members_for_shared_children_and_cycles() {
    let source = format!("{HEADER}#1=ITEM((#2,#2));#2=ITEM((#1,#3));#3=ITEM();{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("cyclic authored scope");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Three unique members plus four traversal edges fit. A second visited
    // collection would need three additional member slots.
    policy.limits.max_collection_items = 7;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let mut members = BTreeSet::new();
    super::super::collect_unit_scope_members(1, &exchange, &mut members, &ctx)
        .expect("each scope member is stored once");
    assert_eq!(members, BTreeSet::from([1, 2, 3]));
}

#[test]
fn uniform_default_units_skip_geometry_scope_bookkeeping() {
    use std::fmt::Write;

    let mut source = format!("{HEADER}{LENGTH}#2=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('',''));#3=CARTESIAN_POINT('',(1.,2.,3.));");
    for id in 100..300 {
        write!(source, "#{id}=SHAPE_REPRESENTATION('',(#3),#2);").expect("authored representation");
    }
    source.push_str(TAIL);
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("shared authored context");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 16;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let mut losses = Vec::new();
    let scales = super::super::resolve_unit_scales(
        &exchange,
        PositiveReal::ONE,
        PositiveReal::ONE,
        &mut losses,
        &ctx,
    )
    .expect("one context admission; no per-representation geometry collections");
    assert!(scales.length.is_empty());
    assert!(scales.angle.is_empty());
    assert!(losses.is_empty());
}
