// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::SketchProfileBoundaryUse;

const ARRANGEMENT_FACE_TEST_TOLERANCE: f64 = 1.0e-7;

fn context_with_collection_limit(limit: u64) -> (DecodeArena, DecodePolicy) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = limit;
    (arena, policy)
}

macro_rules! arrangement_item_refusal {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let (arena, policy) = context_with_collection_limit(0);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut items = Vec::new();
            assert!(matches!(
                ctx.push_vec(&mut items, 1, $operation),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == $operation
            ));
        }
    };
}

arrangement_item_refusal!(candidate_use_refuses_limit, "f3d arrangement candidate use");
arrangement_item_refusal!(circle_refuses_limit, "f3d arrangement circle");
arrangement_item_refusal!(
    pending_boundary_refuses_limit,
    "f3d arrangement pending boundary"
);
arrangement_item_refusal!(
    split_boundary_refuses_limit,
    "f3d arrangement split boundary"
);
arrangement_item_refusal!(edge_refuses_limit, "f3d arrangement edge");
arrangement_item_refusal!(edge_tube_set_refuses_limit, "f3d arrangement edge tube set");
arrangement_item_refusal!(edge_bounds_refuse_limit, "f3d arrangement edge bounds");
arrangement_item_refusal!(face_boundary_refuses_limit, "f3d arrangement face boundary");
arrangement_item_refusal!(face_point_refuses_limit, "f3d arrangement face point");
arrangement_item_refusal!(face_refuses_limit, "f3d arrangement face");
arrangement_item_refusal!(
    retained_edge_mark_refuses_limit,
    "f3d arrangement retained edge mark"
);
arrangement_item_refusal!(retained_edge_refuses_limit, "f3d arrangement retained edge");

macro_rules! arrangement_id_refusal {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                SketchEntityId::mint("synthetic:test:id#edge").expect("test identity").try_clone_for_decode(&ctx, $operation),
                Err(CodecError::ResourceLimit(failure))
                    if failure.operation == $operation
            ));
        }
    };
}

arrangement_id_refusal!(
    candidate_entity_id_refuses_limit,
    "f3d arrangement candidate entity id"
);
arrangement_id_refusal!(
    pending_entity_id_refuses_limit,
    "f3d arrangement pending entity id"
);
arrangement_id_refusal!(
    split_entity_id_refuses_limit,
    "f3d arrangement split entity id"
);

#[test]
fn node_refuses_limit() {
    let (arena, policy) = context_with_collection_limit(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut nodes = Vec::new();
    assert!(matches!(
        super::super::arrangement_node(&mut nodes, Point2::new(0.0, 0.0),
            0.0, &ctx),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d arrangement node"
    ));
}

#[test]
fn pending_node_refuses_limit() {
    let (arena, policy) = context_with_collection_limit(1);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::super::arrangement_has_alternate_path(&[], 0, 0, 0, 1, &ctx),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d arrangement pending nodes"
    ));
}

#[test]
fn circle_angle_refuses_limit() {
    let (arena, policy) = context_with_collection_limit(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::super::arrangement_circle_angles(
            &[Point2::new(1.0, 0.0)], Point2::new(0.0, 0.0),
            positive_radius(1.0), 0.0, &ctx),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d arrangement circle angle"
    ));
}

#[test]
fn outgoing_entry_refuses_limit() {
    let (arena, policy) = context_with_collection_limit(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut outgoing = Vec::new();
    assert!(matches!(
        ctx.push_vec(&mut outgoing, (0, false, 0.0), "f3d arrangement outgoing entry"),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d arrangement outgoing entry"
    ));
}

#[test]
fn boundary_entity_id_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let boundary = SketchProfileBoundaryUse {
        entity: SketchEntityId::mint("synthetic:test:id#edge").unwrap(),
        parameter_range: cadmpeg_ir::geometry::DirectedParameterRange::new([0.0, 1.0]).unwrap(),
        reversed: false,
    };
    assert!(matches!(
        super::super::copy_arrangement_boundary(&ctx, &boundary),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d arrangement boundary entity id"
    ));
}

#[test]
fn selected_boundary_refuses_limit() {
    let (arena, policy) = context_with_collection_limit(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let boundary = SketchProfileBoundaryUse {
        entity: SketchEntityId::mint("synthetic:test:id#edge").unwrap(),
        parameter_range: cadmpeg_ir::geometry::DirectedParameterRange::new([0.0, 1.0]).unwrap(),
        reversed: false,
    };
    assert!(matches!(
        super::super::copy_arrangement_boundary_run(&ctx, &[boundary]),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d arrangement selected boundary"
    ));
}

#[test]
fn arrangement_admitted_route_keeps_two_faces() {
    let (sketch, entities, _, _) = super::coincident_circle_arc_arrangement();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let budget = super::local_arrangement_budget();
    let faces = super::sketch_arrangement_faces(
        &sketch,
        &entities,
        ARRANGEMENT_FACE_TEST_TOLERANCE,
        &budget,
        &ctx,
    )
    .unwrap()
    .unwrap();
    assert_eq!(faces.len(), 2);
}
