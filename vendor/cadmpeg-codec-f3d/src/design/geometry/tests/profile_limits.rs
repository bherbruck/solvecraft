// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::DecodeArena;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::SketchEntity;
use cadmpeg_ir::sketches::SketchEntityId;
use cadmpeg_ir::sketches::SketchEntityUse;
use cadmpeg_ir::sketches::SketchGeometry;
use cadmpeg_ir::sketches::SketchGeometryDefinition;
use cadmpeg_ir::sketches::SketchId;

#[test]
fn sketch_indexed_vectors_refuse_outer_and_inner_limits() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    for (index_operation, value_operation) in [
        (
            "f3d sketch endpoint cell",
            "f3d sketch endpoint cell member",
        ),
        (
            "f3d sketch edge adjacency",
            "f3d sketch edge adjacency member",
        ),
    ] {
        for (limit, operation) in [(0, index_operation), (1, value_operation)] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = limit;

            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut index = std::collections::HashMap::<usize, Vec<usize>>::new();
            assert!(
                matches!(
                    ctx.push_hash_group(&mut index, 1, 2, index_operation, value_operation),
                    Err(CodecError::ResourceLimit(failure))
                        if failure.dimension == ResourceDimension::CollectionItems
                            && failure.operation == operation
                ),
                "operation {operation}"
            );
        }
    }
}

#[test]
fn closed_sketch_profile_collections_refuse_matching_limits() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    for operation in [
        "f3d closed sketch profile member",
        "f3d closed sketch circle profile",
        "f3d closed sketch edge",
        "f3d closed sketch endpoint",
        "f3d closed sketch union parent",
        "f3d closed sketch edge nodes",
        "f3d closed sketch edge order",
        "f3d closed sketch pending edge",
        "f3d closed sketch component edge",
        "f3d closed sketch tangent profile",
        "f3d closed sketch branched profile",
        "f3d closed sketch component profile member",
        "f3d closed sketch component profile",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut items = Vec::new();
        assert!(
            matches!(
                ctx.push_vec(&mut items, 1, operation),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ),
            "operation {operation}"
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut seen = std::collections::HashSet::new();
    assert!(matches!(
        ctx.insert_hash_set(&mut seen, 1, "f3d closed sketch component seen"),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d closed sketch component seen"
    ));
}

#[test]
fn closed_sketch_profile_id_copies_refuse_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    for operation in [
        "f3d closed sketch profile entity id",
        "f3d closed sketch component profile id",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            matches!(
                SketchEntityId::mint("synthetic:test:id#edge").expect("test identity").try_clone_for_decode(&ctx, operation),
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::RetainedBytes
                        && failure.operation == operation
            ),
            "operation {operation}"
        );
    }
}

macro_rules! geometry_collection_refusal_test {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            use cadmpeg_core::decode::ResourceDimension;
            use cadmpeg_core::CodecError;

            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = 0;
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

geometry_collection_refusal_test!(
    branched_profile_start_refuses_limit,
    "f3d branched profile start half-edge"
);
geometry_collection_refusal_test!(
    branched_profile_member_refuses_limit,
    "f3d branched profile member"
);
geometry_collection_refusal_test!(
    branched_profile_output_refuses_limit,
    "f3d branched profile output"
);

#[test]
fn branched_profile_component_refuses_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut items = std::collections::HashSet::new();
    assert!(matches!(
        ctx.insert_hash_set(&mut items, 1, "f3d branched profile component edge"),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.operation == "f3d branched profile component edge"
    ));
}

#[test]
fn branched_profile_visited_refuses_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut items = std::collections::HashSet::new();
    assert!(matches!(
        ctx.insert_hash_set(&mut items, 1, "f3d branched profile visited half-edge"),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.operation == "f3d branched profile visited half-edge"
    ));
}

#[test]
fn branched_profile_outgoing_node_refuses_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut index = std::collections::HashMap::<usize, Vec<usize>>::new();
    assert!(matches!(
        ctx.push_hash_group(&mut index, 1, 2, "f3d branched profile outgoing node", "f3d branched profile outgoing edge"),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.operation == "f3d branched profile outgoing node"
    ));
}

#[test]
fn branched_profile_outgoing_edge_refuses_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut index = std::collections::HashMap::<usize, Vec<usize>>::new();
    assert!(matches!(
        ctx.push_hash_group(&mut index, 1, 2, "f3d branched profile outgoing node", "f3d branched profile outgoing edge"),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.operation == "f3d branched profile outgoing edge"
    ));
}

#[test]
fn branched_profile_next_refuses_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut items = std::collections::HashMap::new();
    assert!(matches!(
        ctx.insert_hash_map(&mut items, 1, 2, "f3d branched profile next half-edge").map(|_| ()),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.operation == "f3d branched profile next half-edge"
    ));
}

#[test]
fn branched_profile_entity_id_refuses_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        SketchEntityId::mint("synthetic:test:id#edge").expect("test identity").try_clone_for_decode(&ctx, "f3d branched profile entity id"),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.operation == "f3d branched profile entity id"
    ));
}

geometry_collection_refusal_test!(
    tangent_profile_cycle_edge_refuses_limit,
    "f3d tangent profile cycle edge"
);
geometry_collection_refusal_test!(
    tangent_profile_cycle_refuses_limit,
    "f3d tangent profile cycle"
);
geometry_collection_refusal_test!(
    tangent_profile_cycle_point_refuses_limit,
    "f3d tangent profile cycle point"
);
geometry_collection_refusal_test!(
    tangent_profile_member_refuses_limit,
    "f3d tangent profile member"
);
geometry_collection_refusal_test!(
    tangent_profile_output_point_refuses_limit,
    "f3d tangent profile output point"
);

#[test]
fn tangent_profile_used_edge_refuses_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut items = std::collections::HashSet::new();
    assert!(matches!(
        ctx.insert_hash_set(&mut items, 1, "f3d tangent profile used edge"),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.operation == "f3d tangent profile used edge"
    ));
}

#[test]
fn tangent_profile_entity_id_refuses_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        SketchEntityId::mint("synthetic:test:id#edge").expect("test identity").try_clone_for_decode(&ctx, "f3d tangent profile entity id"),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.operation == "f3d tangent profile entity id"
    ));
}

fn single_line_profile() -> (Vec<SketchEntity>, Vec<SketchEntityUse>) {
    let sketch = SketchId::mint("synthetic:test:id#profile-limits").unwrap();
    let id = SketchEntityId::mint("synthetic:test:id#profile-edge").unwrap();
    let entity = SketchEntity::new(
        id.clone(),
        sketch,
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .unwrap(),
    );
    (
        vec![entity],
        vec![SketchEntityUse {
            entity: id,
            reversed: false,
        }],
    )
}

const PROFILE_LIMIT_TEST_TOLERANCE: f64 = 1.0e-6;

#[test]
fn line_profile_vertex_refuses_collection_limit() {
    let (entities, profile) = single_line_profile();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::geometry::line_profile_vertices(&profile, &entities, PROFILE_LIMIT_TEST_TOLERANCE, &ctx),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.operation == "f3d line profile vertex"
    ));
}

#[test]
fn circular_arc_profile_segment_refuses_collection_limit() {
    let (entities, profile) = single_line_profile();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::geometry::circular_arc_profile_segments(&profile, &entities, PROFILE_LIMIT_TEST_TOLERANCE, &ctx),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.operation == "f3d circular arc profile segment"
    ));
}

#[test]
fn certified_profile_tubes_refuse_collection_limit() {
    let (entities, profile) = single_line_profile();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::geometry::certified_profile_loop(&profile, &entities,
            PROFILE_LIMIT_TEST_TOLERANCE, &ctx),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.operation == "f3d certified profile tubes"
    ));
}

fn split_limit_line() -> SketchGeometry {
    SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(2.0, 0.0),
    })
    .unwrap()
}

#[test]
fn arrangement_split_endpoints_refuse_collection_limit() {
    let line = split_limit_line();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::geometry::arrangement_split_parameters(&line, [0.0, 1.0], &[],
            PROFILE_LIMIT_TEST_TOLERANCE, &ctx),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.operation == "f3d arrangement split endpoints"
    ));
}

#[test]
fn arrangement_split_parameter_refuses_collection_limit() {
    let line = split_limit_line();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 2;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::geometry::arrangement_split_parameters(&line, [0.0, 1.0],
            &[Point2::new(1.0, 0.0)], PROFILE_LIMIT_TEST_TOLERANCE, &ctx),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.operation == "f3d arrangement split parameter"
    ));
}

#[test]
fn profile_use_polyline_refuses_collection_limit() {
    let (entities, _) = single_line_profile();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 2;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::geometry::profile_use_polyline(&entities[0], [0.0, 1.0], false,
            PROFILE_LIMIT_TEST_TOLERANCE, &ctx),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.operation == "f3d profile use polyline"
    ));
}

#[test]
fn certified_loop_containment_uses_existing_tube_vertices() {
    let vertices = [
        Point2::new(0.0, 0.0),
        Point2::new(2.0, 0.0),
        Point2::new(0.0, 2.0),
    ];
    let loop_ = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::geometry::CertifiedProfileLoop::from_vertices(&vertices, decode_ctx)
    })
    .unwrap()
    .unwrap();
    assert!(loop_.contains_point(Point2::new(0.25, 0.25)));
    assert!(!loop_.contains_point(Point2::new(1.5, 1.5)));
    assert!(!loop_.contains_point(Point2::new(0.0, 0.0)));
}
