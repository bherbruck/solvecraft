// SPDX-License-Identifier: Apache-2.0
//! Collection refusals in the STEP geometry reader.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn uncertainty_values_text_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        &ctx.join_display_retained(["0.1", "0.2"], ", ", "step_uncertainty_values_text"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_uncertainty_values_text"
    ));
}

#[test]
fn uncertainty_note_text_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        ctx.format_retained(format_args!("ambiguous uncertainty values ({})", "0.1, 0.2"), "step_uncertainty_note_text"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_uncertainty_note_text"
    ));
}

fn pcurve_geometry_refusal(collection_limit: u64, depth_limit: u64) -> CodecError {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=UNKNOWN_CURVE();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid curve record");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    super::super::decode_pcurve_geometry(
        1,
        &exchange,
        super::super::PcurveSources {
            points: &BTreeMap::new(),
            vectors: &BTreeMap::new(),
            placements: &BTreeMap::new(),
            transformations: &BTreeMap::new(),
            angle_scale: 1.0,
        },
        &mut Vec::new(),
        &mut BTreeSet::new(),
        0,
        &ctx,
    )
    .expect_err("pcurve geometry exceeds the limit")
}

#[test]
fn pcurve_geometry_active_refuses_collection_limit() {
    assert!(
        matches!(pcurve_geometry_refusal(0, 128), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_pcurve_geometry_active")
    );
}

#[test]
fn pcurve_source_records_refuse_collection_limit() {
    assert!(
        matches!(pcurve_geometry_refusal(1, 128), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_pcurve_source_records")
    );
}

#[test]
fn pcurve_geometry_walk_refuses_depth_limit() {
    assert!(
        matches!(pcurve_geometry_refusal(2, 0), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RecursionDepth
            && refusal.operation == "step_pcurve_geometry_walk")
    );
}

fn surface_scale_refusal(
    collection_limit: u64,
    retained_limit: u64,
    depth_limit: u64,
) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_materialized_bytes = retained_limit;
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let ir = cadmpeg_ir::document::CadIr::empty();
    let id = cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#1").expect("valid identity");
    super::super::surface_parameter_scales_for_step(
        &ir,
        &id,
        &cadmpeg_ir::geometry::SurfaceGeometry::Solved(
            cadmpeg_ir::geometry::SolvedSurfaceGeometry::Unknown { record: None },
        ),
        1.0,
        1.0,
        &BTreeMap::new(),
        &ctx,
    )
    .expect_err("surface scale exceeds limit")
}

#[test]
fn surface_scale_active_refuses_collection_limit() {
    assert!(
        matches!(surface_scale_refusal(0, u64::MAX, 128), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_surface_scale_active")
    );
}

#[test]
fn surface_scale_active_id_refuses_scoped_limit() {
    assert!(
        matches!(surface_scale_refusal(128, 0, 128), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::MaterializedBytes
            && refusal.operation == "step_surface_scale_active_id")
    );
}

#[test]
fn surface_parameter_scale_walk_refuses_depth_limit() {
    assert!(
        matches!(surface_scale_refusal(128, u64::MAX, 0), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RecursionDepth
            && refusal.operation == "step_surface_parameter_scale_walk")
    );
}

#[test]
fn surface_geometry_scale_walk_refuses_depth_limit() {
    assert!(
        matches!(surface_scale_refusal(128, u64::MAX, 1), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RecursionDepth
            && refusal.operation == "step_surface_geometry_scale_walk")
    );
}

fn directrix_scale_refusal(
    collection_limit: u64,
    retained_limit: u64,
    depth_limit: u64,
) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let ir = cadmpeg_ir::document::CadIr::empty();
    let id = cadmpeg_ir::ids::CurveId::mint("test:model:curve#1").expect("valid identity");
    super::super::directrix_parameter_scale_inner(&ir, &id, 1.0, 1.0, &mut BTreeSet::new(), &ctx)
        .expect_err("directrix scale exceeds limit")
}

#[test]
fn directrix_scale_active_refuses_collection_limit() {
    assert!(
        matches!(directrix_scale_refusal(0, u64::MAX, 128), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_directrix_scale_active")
    );
}

#[test]
fn directrix_scale_active_id_refuses_retained_limit() {
    assert!(
        matches!(directrix_scale_refusal(128, 0, 128), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RetainedBytes
            && refusal.operation == "step_directrix_scale_active_id")
    );
}

#[test]
fn directrix_scale_walk_refuses_depth_limit() {
    assert!(
        matches!(directrix_scale_refusal(128, u64::MAX, 0), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RecursionDepth
            && refusal.operation == "step_directrix_scale_walk")
    );
}

#[test]
fn directrix_geometry_scale_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        super::super::directrix_geometry_parameter_scale(
            &cadmpeg_ir::geometry::SolvedCurveGeometry::Unknown { record: None },
            1.0, 1.0, &ctx,
        ),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_directrix_geometry_scale_walk"
    ));
}

macro_rules! association_name_refusal_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 4;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
                .expect("empty root fits policy");
            assert!(matches!(
                super::super::attach_geometry_source(&mut None, 1, Some("named"), &ctx, $operation),
                Err(CodecError::ResourceLimit(refusal))
                    if refusal.dimension == ResourceDimension::RetainedBytes
                        && refusal.operation == $operation
            ));
        }
    };
}

association_name_refusal_test!(
    geometric_set_association_name_copy_refuses_retained_limit,
    "step_geometric_set_association_name_copy"
);
association_name_refusal_test!(
    representation_association_name_copy_refuses_retained_limit,
    "step_representation_association_name_copy"
);
association_name_refusal_test!(
    presentation_association_name_copy_refuses_retained_limit,
    "step_presentation_association_name_copy"
);

fn line_scale_refusal(source: &[u8], collection_limit: u64, depth_limit: u64) -> CodecError {
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid curve record");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    super::super::line_parameter_scale(
        &exchange,
        1,
        cadmpeg_ir::scalar::PositiveReal::new(1.0).expect("positive scale"),
        &mut Vec::new(),
        &ctx,
    )
    .expect_err("line scale exceeds limit")
}

#[test]
fn line_parameter_scale_active_refuses_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=UNKNOWN_CURVE();ENDSEC;END-ISO-10303-21;";
    assert!(
        matches!(line_scale_refusal(source, 0, 128), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_line_parameter_scale_active")
    );
}

#[test]
fn line_parameter_scale_walk_refuses_depth_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=UNKNOWN_CURVE();ENDSEC;END-ISO-10303-21;";
    assert!(
        matches!(line_scale_refusal(source, 1, 0), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::RecursionDepth
            && refusal.operation == "step_line_parameter_scale_walk")
    );
}

#[test]
fn line_parameter_scale_losses_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=LINE('',#2,#3);#2=CARTESIAN_POINT('',(0.,0.,0.));#3=VECTOR('',#4,-1.);#4=DIRECTION('',(1.,0.,0.));ENDSEC;END-ISO-10303-21;";
    assert!(
        matches!(line_scale_refusal(source, 1, 128), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_line_parameter_scale_losses")
    );
}

fn trim_fallback_refusal(master: super::super::TrimMasterRepresentation) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let points = BTreeMap::new();
    let geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(
        cadmpeg_ir::geometry::SolvedCurveGeometry::Unknown { record: None },
    );
    let mut losses = Vec::new();
    let mut context = super::super::TrimParameterContext {
        points: &points,
        geometry: &geometry,
        angle_scale: 1.0,
        linear_parameter_scale: 1.0,
        parameter_offset: 0.0,
        tolerance: 1.0,
        master_representation: master,
        record_id: 1,
        losses: &mut losses,
        ctx: &ctx,
    };
    let parameter = crate::parse::Value::Integer(1);
    let cartesian = crate::parse::Value::Reference(2);
    let (parameter, cartesian) = match master {
        super::super::TrimMasterRepresentation::Parameter => (None, Some(&cartesian)),
        super::super::TrimMasterRepresentation::Cartesian => (Some(&parameter), None),
        super::super::TrimMasterRepresentation::Unspecified => (None, None),
    };
    super::super::select_trim_parameter(parameter, cartesian, &mut context)
        .expect_err("fallback loss exceeds limit")
}

#[test]
fn parameter_trim_fallback_loss_refuses_collection_limit() {
    assert!(
        matches!(trim_fallback_refusal(super::super::TrimMasterRepresentation::Parameter),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_trim_parameter_fallback_losses")
    );
}

#[test]
fn cartesian_trim_fallback_loss_refuses_collection_limit() {
    assert!(
        matches!(trim_fallback_refusal(super::super::TrimMasterRepresentation::Cartesian),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_trim_parameter_fallback_losses")
    );
}

#[test]
fn trim_parameter_scale_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(super::super::parameter_scale(
        &cadmpeg_ir::geometry::SolvedCurveGeometry::Unknown { record: None }, 1.0, 1.0, &ctx,
    ), Err(CodecError::ResourceLimit(refusal))
        if refusal.dimension == ResourceDimension::RecursionDepth
            && refusal.operation == "step_trim_parameter_scale_walk"));
}

#[test]
fn trim_parameter_value_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let points = BTreeMap::new();
    let geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(
        cadmpeg_ir::geometry::SolvedCurveGeometry::Unknown { record: None },
    );
    let mut losses = Vec::new();
    let context = super::super::TrimParameterContext {
        points: &points,
        geometry: &geometry,
        angle_scale: 1.0,
        linear_parameter_scale: 1.0,
        parameter_offset: 0.0,
        tolerance: 1.0,
        master_representation: super::super::TrimMasterRepresentation::Parameter,
        record_id: 1,
        losses: &mut losses,
        ctx: &ctx,
    };
    assert!(matches!(
        super::super::trim_parameter_value(&crate::parse::Value::Integer(1), &context),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_trim_parameter_value_walk"
    ));
}

fn deferred_dependency_refusal(
    limit: u64,
    group: &'static str,
    member: &'static str,
) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    super::super::defer_geometry_dependency(&mut HashMap::new(), 1, 2, &ctx, group, member)
        .expect_err("dependency exceeds the limit")
}

#[test]
fn deferred_curve_groups_refuse_collection_limit() {
    assert!(
        matches!(deferred_dependency_refusal(0, "step_deferred_curve_groups", "step_deferred_curve_members"),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_deferred_curve_groups")
    );
}

#[test]
fn deferred_curve_members_refuse_collection_limit() {
    assert!(
        matches!(deferred_dependency_refusal(1, "step_deferred_curve_groups", "step_deferred_curve_members"),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_deferred_curve_members")
    );
}

#[test]
fn deferred_surface_groups_refuse_collection_limit() {
    assert!(
        matches!(deferred_dependency_refusal(0, "step_deferred_surface_groups", "step_deferred_surface_members"),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_deferred_surface_groups")
    );
}

#[test]
fn deferred_surface_members_refuse_collection_limit() {
    assert!(
        matches!(deferred_dependency_refusal(1, "step_deferred_surface_groups", "step_deferred_surface_members"),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_deferred_surface_members")
    );
}

fn deferred_wake_refusal(operation: &'static str) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let mut waiting = HashMap::from([(1, vec![2])]);
    super::super::wake_deferred_dependents(1, &mut waiting, &mut VecDeque::new(), &ctx, operation)
        .expect_err("wake queue exceeds the limit")
}

#[test]
fn deferred_curve_queue_refuses_collection_limit() {
    assert!(matches!(deferred_wake_refusal("step_deferred_curve_queue"),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_deferred_curve_queue"));
}

#[test]
fn deferred_surface_queue_refuses_collection_limit() {
    assert!(
        matches!(deferred_wake_refusal("step_deferred_surface_queue"),
        CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_deferred_surface_queue")
    );
}

#[test]
fn curve_coordinate_rows_refuse_collection_limit() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.)));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid coordinate list");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(source, &arena, &policy).expect("source fits policy");
    assert!(matches!(
        super::super::coordinate_rows(&exchange.records()[&1], 1.0, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "step_curve_coordinate_rows"
    ));
}

fn strip_refusal(collection_limit: u64) -> CodecError {
    use crate::parse::Value;

    let strips = Value::List(vec![Value::List(vec![
        Value::Integer(1),
        Value::Integer(2),
    ])]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    super::super::tessellated_line_strips(Some(&strips), 2, &ctx)
        .expect_err("strip exceeds collection limit")
}

#[test]
fn curve_strip_indices_refuse_collection_limit() {
    assert!(
        matches!(strip_refusal(0), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_curve_strip_indices")
    );
}

#[test]
fn curve_strips_refuse_collection_limit() {
    assert!(
        matches!(strip_refusal(2), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_curve_strips")
    );
}

#[test]
fn curve_strip_source_name_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        ctx.format_retained(format_args!("{}", "curve"), "step_curve_strip_source_name"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_curve_strip_source_name"
    ));
}
