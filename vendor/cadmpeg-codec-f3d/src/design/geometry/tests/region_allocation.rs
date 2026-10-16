// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::CodecError;

const REGION_LIMIT_TEST_TOLERANCE: f64 = 1.0e-7;

macro_rules! region_item_refusal {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut items = Vec::new();
            assert!(matches!(
                ctx.push_vec(&mut items, 1, $operation),
                Err(CodecError::ResourceLimit(failure))
                    if failure.operation == $operation
            ));
        }
    };
}

region_item_refusal!(boundary_refuses_limit, "f3d profile boundaries");
region_item_refusal!(
    containment_cell_refuses_limit,
    "f3d profile containment cell"
);
region_item_refusal!(containment_row_refuses_limit, "f3d profile containment row");
region_item_refusal!(projected_point_refuses_limit, "f3d profile projected point");
region_item_refusal!(incidence_row_refuses_limit, "f3d profile incidence row");
region_item_refusal!(closure_match_refuses_limit, "f3d profile closure match");
region_item_refusal!(hole_index_refuses_limit, "f3d profile hole index");
region_item_refusal!(
    containing_boundary_refuses_limit,
    "f3d profile containing boundary"
);
region_item_refusal!(
    independent_boundary_refuses_limit,
    "f3d independent profile boundaries"
);

#[test]
fn incident_boundary_refuses_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut incident = std::collections::HashSet::new();
    assert!(matches!(
        ctx.insert_hash_set(&mut incident, 0, "f3d profile incident boundary"),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d profile incident boundary"
    ));
}

#[test]
fn immediate_hole_refuses_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let containment = vec![vec![false, true], vec![false, false]];
    assert!(matches!(
        super::super::immediate_containment_children(0, &containment, &ctx),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d profile immediate hole"
    ));
}

#[test]
fn circular_region_keeps_admitted_selection() {
    let sketch_id = SketchId::mint("synthetic:test:id#region").unwrap();
    let entity_id = SketchEntityId::mint("synthetic:test:id#region-circle").unwrap();
    let entity = SketchEntity::new(
        entity_id.clone(),
        sketch_id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(2.0).unwrap(),
        })
        .unwrap(),
    );
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
            entity: entity_id,
            reversed: false,
        }]])
        .unwrap(),
        native_ref: None,
    };
    let entities = [entity];
    let points = [Point3::new(0.5, 0.0, 0.0)];
    let expected = crate::test_support::with_decode_context(|decode_ctx| {
        super::region_containing_points(
            &sketch,
            &entities,
            &points,
            REGION_LIMIT_TEST_TOLERANCE,
            decode_ctx,
        )
    })
    .unwrap();
    assert!(expected.is_some());
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let actual = super::region_containing_points(
        &sketch,
        &entities,
        &points,
        REGION_LIMIT_TEST_TOLERANCE,
        &ctx,
    )
    .unwrap();
    assert_eq!(actual, expected);
}
