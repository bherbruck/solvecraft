use cadmpeg_core::decode::ResourceDimension;
// SPDX-License-Identifier: Apache-2.0
use crate::design::sketch_project::project_sketch_design;
use crate::design::sketch_project::project_spatial_sketch_constraints;
use crate::design::sketch_project::project_spatial_sketch_design;
use crate::records::sketch_geometry::SketchCurveGeometry;
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::sketch_geometry::SketchSurface;
use crate::records::sketch_geometry::SketchText;
use crate::records::sketch_placement::DesignSketchPlacement;
use crate::records::sketch_relations::SketchRelation;
use crate::records::sketch_relations::SketchRelationMember;
use crate::records::sketch_relations::SketchRelationReturnMember;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;

const EPS_PROJECTION_LIMITS_E6: f64 = 1.0e-6;

fn owner_limit_curve(spatial: bool) -> SketchCurveIdentity {
    SketchCurveIdentity {
        id: "f3d:BulkStream.dat:curve#10".into(),
        record_index: 10,
        owner_reference: Some(42),
        class_tag: crate::records::references::DesignClassTag::try_from("375".to_owned()).unwrap(),
        byte_offset: 10,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(10).unwrap(),
        secondary_id: 0,
        geometry: spatial.then(|| {
            SketchCurveGeometry::line(
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 0.0, 1.0).unit().unwrap(),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap()
        }),
    }
}

fn owner_limit_placement() -> DesignSketchPlacement {
    DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),
        id: "f3d:BulkStream.dat:placement#0".into(),
        scope_record_index: None,
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_42".to_owned())
            .unwrap(),
        visibility: None,
        class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        record_index: 1,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("257".to_owned())
            .unwrap(),
    }
}

#[test]
fn sketch_placement_indices_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let placement = owner_limit_placement();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        project_sketch_design(&ctx, std::slice::from_ref(&placement), &[], &[], &[], &[], EPS_PROJECTION_LIMITS_E6),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d planar sketch placement index"
    ));
    let arena = DecodeArena::new();

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        project_spatial_sketch_design(&ctx, &[placement], &[], &[], &[], &[], EPS_PROJECTION_LIMITS_E6),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d spatial sketch placement index"
    ));
}

#[test]
fn spatial_sketch_curve_index_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let curve = owner_limit_curve(false);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        project_spatial_sketch_design(&ctx, &[], &[], &[curve], &[], &[], EPS_PROJECTION_LIMITS_E6),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d spatial sketch curve index"
    ));
}

#[test]
fn spline_segment_index_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let points = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut segments = std::collections::HashMap::new();
    assert!(matches!(
        crate::design::sketch_project::record_spline_segment(&ctx, &mut segments, "Design", 10, points),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d spatial sketch spline segment index"
    ));
    assert!(segments.is_empty());
    let mut segments = std::collections::HashMap::new();
    crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::sketch_project::record_spline_segment(
            decode_ctx,
            &mut segments,
            "Design",
            10,
            points,
        )
    })
    .unwrap();
    crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::sketch_project::record_spline_segment(
            decode_ctx,
            &mut segments,
            "Design",
            10,
            points,
        )
    })
    .unwrap();
    assert_eq!(segments.get(&("Design", 10)), Some(&Some(points)));
    crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::sketch_project::record_spline_segment(
            decode_ctx,
            &mut segments,
            "Design",
            10,
            [Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
        )
    })
    .unwrap();
    assert_eq!(segments.get(&("Design", 10)), Some(&None));
}

#[test]
fn spatial_spline_member_index_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let members = [
        SketchRelationReturnMember::from_index(10),
        SketchRelationReturnMember::from_index(11),
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::sketch_project::distinct_return_member_indices(&ctx, &members),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d spatial spline member index"
    ));
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::sketch_project::distinct_return_member_indices(decode_ctx, &members)
    })
    .unwrap());
    let duplicates = [
        SketchRelationReturnMember::from_index(10),
        SketchRelationReturnMember::from_index(10),
    ];
    assert!(!crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::sketch_project::distinct_return_member_indices(decode_ctx, &duplicates)
    })
    .unwrap());
}

#[test]
fn spatial_spline_segment_candidate_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut segments = Vec::new();
    let candidate = (10, [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]);
    assert!(matches!(
        ctx.push_vec(&mut segments, candidate, "f3d spatial spline segment candidates"),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d spatial spline segment candidates"
    ));
    assert!(segments.is_empty());
}

#[test]
fn spatial_constraint_indices_refuse_collection_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let placement = owner_limit_placement();
    let curve = owner_limit_curve(false);
    let sketch = crate::ids::neutral_spatial_sketch_id(&placement);
    let entity = cadmpeg_ir::sketches::SpatialSketchEntity::new(
        crate::ids::neutral_spatial_sketch_record_id(&sketch, curve.record_index),
        sketch,
        cadmpeg_ir::sketches::SpatialSketchGeometry::try_from(
            cadmpeg_ir::sketches::SpatialSketchGeometryDefinition::Point {
                position: Point3::new(0.0, 0.0, 0.0),
            },
        )
        .unwrap(),
    )
    .with_native_ref(Some(curve.id.clone()));
    for (limit, placements, entities, operation) in [
        (
            0,
            std::slice::from_ref(&placement),
            std::slice::from_ref(&entity),
            "f3d spatial constraint sketch index",
        ),
        (
            0,
            &[][..],
            &[][..],
            "f3d spatial constraint native record index",
        ),
        (
            1,
            &[][..],
            std::slice::from_ref(&entity),
            "f3d spatial constraint entity index",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            matches!(
                project_spatial_sketch_constraints(
                    &ctx, placements, &[], &[], std::slice::from_ref(&curve), &[], entities,
                ),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ),
            "operation {operation}"
        );
    }
}

#[test]
fn spatial_geometry_owner_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let curve = owner_limit_curve(true);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::sketch_project::spatial_geometry_owners(&ctx, &[], &[curve]),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d spatial geometry owner"
    ));
}

fn owner_limit_text() -> SketchText {
    SketchText {
        id: "f3d:BulkStream.dat:text#20".into(),
        record_index: 20,
        owner_reference: 42,
        class_tag: crate::records::references::DesignClassTag::try_from("376".to_owned()).unwrap(),
        class_version: 0,
        byte_offset: 20,
        entity_genesis: None,
        persistent_id: None,
        base_id: None,
        text: "A".into(),
        font_family: "Arial".into(),
        font_weight: 400,
        height: cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap(),
        color: cadmpeg_ir::topology::Color::new(0.0, 0.0, 0.0, 1.0).unwrap(),
        layout: crate::records::sketch_geometry::SketchTextLayout::TextexTag {
            width_factor: cadmpeg_ir::scalar::NonNegativeReal::new(1.0).unwrap(),
            alignment: None,
            first_reference: None,
            second_reference: None,
            placement: None,
        },
        raw_bytes: Vec::new().into(),
    }
}

#[test]
fn text_frame_owner_indices_refuse_collection_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let curve = owner_limit_curve(false);
    let text = owner_limit_text();
    for (curves, texts, operation) in [
        (
            std::slice::from_ref(&curve),
            &[][..],
            "f3d text frame curve owner",
        ),
        (
            &[][..],
            std::slice::from_ref(&text),
            "f3d text frame text owner",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            matches!(
                crate::design::sketch_project::text_frame_curve_records(&ctx, &[], curves, texts),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ),
            "operation {operation}"
        );
    }
}

fn owner_limit_surface() -> SketchSurface {
    SketchSurface {
        id: "f3d:BulkStream.dat:surface#2".into(),
        record_index: 2,
        owner_reference: Some(42),
        class_tag: crate::records::references::DesignClassTag::try_from("306".to_owned()).unwrap(),
        byte_offset: 0,
        entity_genesis: None,
        persistent_id: std::num::NonZeroU64::new(2).unwrap(),
        geometry: crate::records::sketch_geometry::SketchSurfaceGeometry::from_parts(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
            ],
        )
        .unwrap(),
    }
}

#[test]
fn spatial_surface_owner_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let surface = owner_limit_surface();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        project_spatial_sketch_design(&ctx, &[], &[], &[], &[surface], &[], EPS_PROJECTION_LIMITS_E6),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d spatial surface owner"
    ));
}

#[test]
fn spatial_surface_lanes_refuse_each_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let placement = owner_limit_placement();
    let surface = owner_limit_surface();
    for (limit, operation) in [
        (2, "f3d spatial sketch surface u knots"),
        (6, "f3d spatial sketch surface v knots"),
        (10, "f3d spatial sketch surface control rows"),
        (11, "f3d spatial sketch surface control points"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            matches!(
                project_spatial_sketch_design(
                    &ctx, std::slice::from_ref(&placement), &[], &[],
                    std::slice::from_ref(&surface), &[], EPS_PROJECTION_LIMITS_E6,
                ),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ),
            "limit {limit}, operation {operation}"
        );
    }
}

#[test]
fn spatial_sketch_id_index_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    let placement = owner_limit_placement();
    let surface = owner_limit_surface();
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "f3d spatial sketch id index copy",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                project_spatial_sketch_design(
                    &ctx,
                    std::slice::from_ref(&placement),
                    &[],
                    &[],
                    std::slice::from_ref(&surface),
                    &[],
                    EPS_PROJECTION_LIMITS_E6,
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == "f3d spatial sketch id index copy" && failure.dimension == (ResourceDimension::RetainedBytes)));
    }
}

#[test]
fn sketch_nurbs_lanes_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    for operation in [
        "f3d planar sketch nurbs poles",
        "f3d planar sketch nurbs weights",
        "f3d spatial sketch nurbs poles",
        "f3d spatial sketch nurbs weights",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            matches!(
                ctx.collect_vec([1.0_f64], operation),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ),
            "operation {operation}"
        );
    }
}

#[test]
fn text_frame_curve_records_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let curve = owner_limit_curve(false);
    let text = owner_limit_text();
    let relation = SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
        id: "f3d:BulkStream.dat:relation#30".into(),
        record_index: 30,
        class_tag: crate::records::references::DesignClassTag::try_from("377".to_owned()).unwrap(),
        byte_offset: 30,
        state_offset: 0,
        owner_reference: 42,
        owner_entity_id: Some(cadmpeg_core::text::NonBlankString::new("Sketch_42").unwrap()),
        auxiliary_references: crate::records::identity::ReferenceRun::located(vec![
            crate::records::identity::Located {
                value: 20,
                offset: 0,
            },
        ]),
        rectangular_counted_reference_count: None,
        members: vec![
            SketchRelationMember::from_index(20),
            SketchRelationMember::from_index(10),
        ]
        .try_into()
        .unwrap(),
        owner_reference_offset: 0,
        definition: crate::records::sketch_relations::SketchRelationDefinition::new(
            0x100_0000_0000,
            Some(
                crate::records::sketch_relations::SketchPatternDefinition::TextFrame {
                    text_reference: 20,
                },
            ),
        )
        .unwrap(),
        entity_genesis: None,
        return_members: vec![SketchRelationReturnMember::from_index(10)]
            .try_into()
            .unwrap(),
        raw_bytes: vec![0; 160],
    })
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 2;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::sketch_project::text_frame_curve_records(&ctx, &[relation], &[curve], &[text]),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d text frame curve record"
    ));
}

#[test]
fn sketch_nurbs_refuses_nonpositive_weight_before_projection() {
    let error = crate::records::sketch_geometry::SketchNurbsPoles::from_wire(
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
        vec![1.0, 0.0],
    )
    .expect_err("nonpositive weight must be refused at admission");
    assert!(
        error.contains("weight is not positive and finite"),
        "{error}"
    );
}

#[test]
fn projected_sketch_text_copies_refuse_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    for operation in [
        "f3d planar sketch name",
        "f3d planar sketch native reference",
        "f3d planar sketch point native reference",
        "f3d planar sketch curve native reference",
        "f3d planar sketch text",
        "f3d planar sketch font family",
        "f3d planar sketch text native reference",
        "f3d spatial sketch curve native reference",
        "f3d spatial sketch point native reference",
        "f3d spatial sketch surface native reference",
        "f3d spatial sketch name",
        "f3d spatial sketch native reference",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 4;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            matches!(
                ctx.copy_retained_text("input", operation),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::RetainedBytes
                        && failure.operation == operation
            ),
            "operation {operation}"
        );
    }
}

#[test]
fn projected_sketch_entries_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    for operation in [
        "f3d planar sketch",
        "f3d planar sketch point entity",
        "f3d planar sketch curve entity",
        "f3d planar sketch text entity",
        "f3d spatial sketch curve entity",
        "f3d spatial sketch point entity",
        "f3d spatial sketch surface entity",
        "f3d spatial sketch",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut entries = Vec::new();
        assert!(
            matches!(
                ctx.push_vec(&mut entries, 1, operation),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ),
            "operation {operation}"
        );
        assert!(entries.is_empty());
    }
}

#[test]
fn spatial_constraint_sketch_membership_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let placement = owner_limit_placement();
    let sketch = crate::ids::neutral_spatial_sketch_id(&placement);
    let entity = cadmpeg_ir::sketches::SpatialSketchEntity::new(
        crate::ids::neutral_spatial_sketch_record_id(&sketch, 10),
        sketch,
        cadmpeg_ir::sketches::SpatialSketchGeometry::try_from(
            cadmpeg_ir::sketches::SpatialSketchGeometryDefinition::Point {
                position: Point3::new(0.0, 0.0, 0.0),
            },
        )
        .unwrap(),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units =
        2 * cadmpeg_core::decode::u64_from_index(entity.sketch.as_str().len());

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        project_spatial_sketch_constraints(&ctx, std::slice::from_ref(&placement), &[], &[], &[], &[], &[entity]),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::WorkUnits
                && failure.operation == "f3d spatial constraint sketch membership work"
    ));
}

#[test]
fn spatial_constraint_copies_and_output_refuse_matching_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    for operation in [
        "f3d spatial constraint member id",
        "f3d spatial constraint operand id",
        "f3d spatial constraint sketch id",
        "f3d spatial constraint native reference",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 4;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            ctx.copy_retained_text("input", operation),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == operation
        ));
    }
    for operation in [
        "f3d spatial constraint semantic entity",
        "f3d spatial constraint member",
        "f3d spatial constraint output",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut items = Vec::new();
        assert!(matches!(
            ctx.push_vec(&mut items, 1, operation),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == operation
        ));
    }
    {
        let operation = "f3d spatial constraint distinct member";
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut members = std::collections::HashSet::new();
        assert!(matches!(
            ctx.insert_hash_set(&mut members, 1, operation).map(|_| ()),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == operation
        ));
        assert!(members.is_empty());
    }
}
