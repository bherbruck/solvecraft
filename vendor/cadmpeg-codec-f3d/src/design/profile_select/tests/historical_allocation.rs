use cadmpeg_core::decode::ResourceDimension;
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::history_records::{
    AsmDeltaState, AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCylinder,
    AsmHistoricalEdge, AsmHistoricalEntityDelta, AsmHistoricalPoint, AsmHistoricalRelation,
    AsmHistoricalTopology, AsmHistoricalTopologyDelta, AsmHistoricalTransition, AsmHistory,
    AsmTopologyCache,
};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use crate::records::topology::fillet::HistoricalBinding;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn historical_point_member() -> DesignExtrudeSelectionMember {
    DesignExtrudeSelectionMember::try_new(
        crate::records::topology::extrude_selection::DesignExtrudeSelectionMemberDraft {
            id: "synthetic:selection-member#10".into(),
            group_record_index: 9,
            group_member_ordinal: 0,
            record_index: 10,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("278".to_owned())
                .unwrap(),
            local_id: 40,
            local_id_offset: 21,
            asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 33,
            context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 109,
            tail_slot_present: false,
            tail_slot_offset: 0,
            resolved_geometry: None,
            operand_identity_ids: Vec::new(),
            historical: Some(HistoricalBinding {
                kind: AsmHistoricalEntityKind::Point,
                entity_ref: 40,
                state_ids: vec![2],
            }),
            next_record_index: 11,
            next_byte_offset: 190,
        },
    )
    .unwrap()
}

fn assert_historical_selection_refusal(operation: &'static str) {
    let sketch = empty_sketch();
    let member = historical_point_member();
    let topology = AsmHistoricalTopology {
        point_positions: vec![AsmHistoricalPoint {
            point: 40,
            position: Point3::new(0.5, 0.5, 0.0),
        }],
        ..AsmHistoricalTopology::default()
    };
    let histories = [AsmHistory {
        id: "synthetic:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![transition_state(2, topology, None)],
    }];
    let arrangement_budget = WorkBudget::new(MAX_ARRANGEMENT_WALK_WORK);
    for limit in 0..64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match super::super::historical_selection_regions(
            &[&member],
            &sketch,
            &[],
            &histories,
            0.000_001,
            &arrangement_budget,
            &ctx,
        ) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected historical selection refusal at {operation}: {other:?}"),
        }
    }
    panic!("no historical selection refusal at {operation}");
}

macro_rules! historical_selection_refusal {
    ($name:ident, $operation:literal) => {
        #[test]
        fn $name() {
            assert_historical_selection_refusal($operation);
        }
    };
}

historical_selection_refusal!(
    historical_state_index_refuses_limit,
    "f3d historical selection state index"
);
historical_selection_refusal!(
    historical_state_id_refuses_limit,
    "f3d historical selection state id"
);
historical_selection_refusal!(
    historical_member_points_refuse_limit,
    "f3d historical selection member points"
);
historical_selection_refusal!(
    historical_combined_member_point_refuses_limit,
    "f3d historical combined member point"
);
historical_selection_refusal!(
    historical_member_selection_refuses_limit,
    "f3d historical member selection"
);
historical_selection_refusal!(
    historical_fallback_selection_refuses_limit,
    "f3d historical fallback selection"
);
historical_selection_refusal!(
    historical_projected_selection_point_refuses_limit,
    "f3d historical projected selection point"
);

fn assert_historical_boundary_refusal(operation: &'static str) {
    let mut sketch = empty_sketch();
    let entity_id = SketchEntityId::mint("synthetic:test:id#historical-boundary-line").unwrap();
    sketch
        .profiles
        .try_push(vec![SketchEntityUse {
            entity: entity_id.clone(),
            reversed: false,
        }])
        .unwrap();
    let entity = SketchEntity::new(
        entity_id,
        sketch.id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        })
        .unwrap(),
    );
    let arrangement_budget = WorkBudget::new(MAX_ARRANGEMENT_WALK_WORK);
    for limit in 0..16 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match super::super::selection_containing_points(
            &sketch,
            std::slice::from_ref(&entity),
            &[Point3::new(0.5, 0.0, 0.0)],
            0.000_001,
            &arrangement_budget,
            &ctx,
        ) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected historical boundary refusal at {operation}: {other:?}"),
        }
    }
    panic!("no historical boundary refusal at {operation}");
}

#[test]
fn historical_boundary_profile_refuses_limit() {
    assert_historical_boundary_refusal("f3d historical boundary profile");
}

#[test]
fn historical_selected_profile_refuses_limit() {
    assert_historical_boundary_refusal("f3d historical selected profile");
}

#[test]
fn ordered_selected_profile_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::super::ordered_unique_profile_selections([
            Some(super::super::ResolvedProfileSelection::Loops(vec![0])),
        ], &ctx),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d ordered selected profile"
    ));
}

#[test]
fn ordered_selected_region_refuses_collection_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let region = cadmpeg_ir::features::SketchProfileRegion::loops(
        0,
        Vec::new(),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("fixture loop-region admission")
    .unwrap();
    assert!(matches!(
        super::super::ordered_unique_profile_selections([
            Some(super::super::ResolvedProfileSelection::Regions(vec![region])),
        ], &ctx),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d ordered selected region"
    ));
}

fn assert_merged_profile_refusal(operation: &'static str, region: bool, retained: bool) {
    use cadmpeg_ir::features::{PlanarProfileRef, ProfileRef, SketchProfileRegion};

    let sketch = empty_sketch().id;
    let selection = if region {
        ProfileRef::Planar(
            PlanarProfileRef::sketch_regions(
                sketch.clone(),
                vec![SketchProfileRegion::loops(
                    0,
                    vec![1],
                    &cadmpeg_test_support::service_decode_context(),
                )
                .expect("fixture loop-region admission")
                .unwrap()],
            )
            .unwrap(),
        )
    } else {
        ProfileRef::Planar(
            PlanarProfileRef::sketch_profiles(
                sketch.clone(),
                vec![0],
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("profile membership admission")
            .unwrap(),
        )
    };
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            if retained {
                ResourceDimension::RetainedBytes
            } else {
                ResourceDimension::CollectionItems
            },
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                if retained {
                    policy.limits.max_retained_bytes = cap;
                } else {
                    policy.limits.max_collection_items = cap;
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                super::super::merge_resolved_profile_selections(
                    &sketch,
                    std::slice::from_ref(&selection),
                    &ctx,
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems })));
    }
}

#[test]
fn merged_selected_profile_refuses_collection_limit() {
    assert_merged_profile_refusal("f3d merged selected profile", false, false);
}

#[test]
fn merged_region_hole_refuses_collection_limit() {
    assert_merged_profile_refusal("f3d merged region hole", true, false);
}

#[test]
fn merged_selected_region_refuses_collection_limit() {
    assert_merged_profile_refusal("f3d merged selected region", true, false);
}

#[test]
fn merged_profile_sketch_id_refuses_retained_limit() {
    assert_merged_profile_refusal("f3d profile sketch id", false, true);
}

fn assert_merged_trimmed_region_refusal(operation: &'static str, retained: bool) {
    use cadmpeg_ir::features::{
        PlanarProfileRef, ProfileRef, SketchProfileBoundaryUse, SketchProfileRegion,
    };
    use cadmpeg_ir::geometry::DirectedParameterRange;
    use cadmpeg_ir::scalar::FiniteReal;

    let sketch = empty_sketch().id;
    let boundary = SketchProfileBoundaryUse {
        entity: SketchEntityId::mint("synthetic:test:id#trimmed-boundary").unwrap(),
        parameter_range: DirectedParameterRange::from_finite_endpoints([
            FiniteReal::new(0.0).unwrap(),
            FiniteReal::new(1.0).unwrap(),
        ])
        .unwrap(),
        reversed: false,
    };
    let region =
        SketchProfileRegion::trimmed(vec![boundary.clone()], vec![vec![boundary]]).unwrap();
    let selection =
        ProfileRef::Planar(PlanarProfileRef::sketch_regions(sketch.clone(), vec![region]).unwrap());
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            if retained {
                ResourceDimension::RetainedBytes
            } else {
                ResourceDimension::CollectionItems
            },
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                if retained {
                    policy.limits.max_retained_bytes = cap;
                } else {
                    policy.limits.max_collection_items = cap;
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                super::super::merge_resolved_profile_selections(
                    &sketch,
                    std::slice::from_ref(&selection),
                    &ctx,
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems })));
    }
}

#[test]
fn merged_boundary_entity_id_refuses_retained_limit() {
    assert_merged_trimmed_region_refusal("f3d merged region boundary entity id", true);
}

#[test]
fn merged_outer_boundary_refuses_collection_limit() {
    assert_merged_trimmed_region_refusal("f3d merged region outer boundary", false);
}

#[test]
fn merged_hole_boundary_refuses_collection_limit() {
    assert_merged_trimmed_region_refusal("f3d merged region hole boundary", false);
}

#[test]
fn merged_hole_ring_refuses_collection_limit() {
    assert_merged_trimmed_region_refusal("f3d merged region hole ring", false);
}

#[test]
fn resolved_member_profile_refuses_collection_limit() {
    let mut member = historical_point_member();
    member.resolved_geometry = Some(SketchRelationOperand::Curve {
        record_index: 10,
        primary_id: 100,
        secondary_id: 0,
    });
    let mut sketch = empty_sketch();
    let entity = neutral_sketch_curve_id(&sketch.id, 100, 0);
    sketch
        .profiles
        .try_push(vec![SketchEntityUse {
            entity,
            reversed: false,
        }])
        .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::super::resolved_selection_member_profiles(&member, &sketch, &ctx),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d resolved member profile"
    ));
}

fn assert_boundary_region_refusal(operation: &'static str) {
    use super::super::ResolvedProfileSelection;

    let member = historical_point_member();
    let sketch = empty_sketch();
    let region = cadmpeg_ir::features::SketchProfileRegion::loops(
        0,
        vec![1],
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("fixture loop-region admission")
    .unwrap();
    let selections = [
        Some(ResolvedProfileSelection::Regions(vec![region])),
        Some(ResolvedProfileSelection::Loops(vec![0])),
    ];
    for limit in 0..8 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match super::super::region_with_boundary_selection_members(
            &[&member, &member],
            &sketch,
            &selections,
            &ctx,
        ) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected boundary region refusal at {operation}: {other:?}"),
        }
    }
    panic!("no boundary region refusal at {operation}");
}

#[test]
fn historical_boundary_region_hole_refuses_collection_limit() {
    assert_boundary_region_refusal("f3d historical boundary region hole");
}

#[test]
fn historical_boundary_region_refuses_collection_limit() {
    assert_boundary_region_refusal("f3d historical boundary region");
}

fn assert_fallback_point_refusal(operation: &'static str) {
    let mut member = historical_point_member();
    member.historical.as_mut().unwrap().entity_ref = 999;
    member.resolved_geometry = Some(SketchRelationOperand::Point {
        record_index: 10,
        persistent_id: Some(100),
    });
    let sketch = empty_sketch();
    let entity = SketchEntity::new(
        crate::ids::neutral_sketch_point_id(&sketch.id, 100),
        sketch.id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(0.5, 0.5),
        })
        .unwrap(),
    );
    let histories = [AsmHistory {
        id: "synthetic:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![transition_state(2, AsmHistoricalTopology::default(), None)],
    }];
    let arrangement_budget = WorkBudget::new(MAX_ARRANGEMENT_WALK_WORK);
    for limit in 0..64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match super::super::historical_selection_regions(
            &[&member],
            &sketch,
            std::slice::from_ref(&entity),
            &histories,
            0.000_001,
            &arrangement_budget,
            &ctx,
        ) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected fallback point refusal at {operation}: {other:?}"),
        }
    }
    panic!("no fallback point refusal at {operation}");
}

#[test]
fn historical_fallback_member_point_refuses_collection_limit() {
    assert_fallback_point_refusal("f3d historical fallback member point");
}

#[test]
fn resolved_fallback_member_point_refuses_collection_limit() {
    assert_fallback_point_refusal("f3d resolved fallback member point");
}

#[test]
fn resolved_fallback_member_points_refuse_collection_limit() {
    assert_fallback_point_refusal("f3d resolved fallback member points");
}

#[test]
fn historical_selected_arrangement_region_refuses_collection_limit() {
    let mut sketch = empty_sketch();
    let corners = [
        Point2::new(0.0, 0.0),
        Point2::new(2.0, 0.0),
        Point2::new(2.0, 2.0),
        Point2::new(0.0, 2.0),
    ];
    let mut entities = Vec::new();
    let mut boundary = Vec::new();
    for index in 0..4 {
        let id =
            SketchEntityId::mint(format!("synthetic:test:id#arrangement-edge-{index}")).unwrap();
        entities.push(SketchEntity::new(
            id.clone(),
            sketch.id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: corners[index],
                end: corners[(index + 1) % 4],
            })
            .unwrap(),
        ));
        boundary.push(SketchEntityUse {
            entity: id,
            reversed: false,
        });
    }
    sketch.profiles.try_push(boundary).unwrap();
    let arrangement_budget = WorkBudget::new(MAX_ARRANGEMENT_WALK_WORK);
    for limit in 0..10_000 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match super::super::selection_containing_points(
            &sketch,
            &entities,
            &[Point3::new(1.0, 1.0, 0.0)],
            0.000_001,
            &arrangement_budget,
            &ctx,
        ) {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d historical selected arrangement region" =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected arrangement region refusal: {other:?}"),
        }
    }
    panic!("no arrangement region refusal");
}

fn assert_extrude_selection_refusal(operation: &'static str, matched: bool, retained: bool) {
    let group = DesignExtrudeSelectionGroup::try_from(
        crate::records::topology::extrude_selection::DesignExtrudeSelectionGroupWire {
            id: "f3d:Design/BulkStream.dat:selection-group#9".into(),
            scope_record_index: 7,
            scope_reference_ordinal: 0,
            record_index: 9,
            byte_offset: 0,
            class_tag: "277".to_owned(),
            member_count_offset: 32,
            members: vec![10],
            member_offsets: vec![37],
            opaque_index: 1,
            opaque_index_offset: 47,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 51,
            variant: false,
            paired_class_tag: "277".to_owned(),
            paired_byte_offset: 100,
        },
    )
    .unwrap();
    let mut member = historical_point_member();
    member.id = "f3d:Design/BulkStream.dat:selection-member#10".into();
    member.historical = None;
    member.resolved_geometry = Some(SketchRelationOperand::Curve {
        record_index: 10,
        primary_id: 100,
        secondary_id: 0,
    });
    let mut sketch = empty_sketch();
    if matched {
        let entity = neutral_sketch_curve_id(&sketch.id, 100, 0);
        sketch
            .profiles
            .try_push(vec![SketchEntityUse {
                entity,
                reversed: false,
            }])
            .unwrap();
    }
    let arrangement_budget = WorkBudget::new(MAX_ARRANGEMENT_WALK_WORK);
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            if retained {
                ResourceDimension::RetainedBytes
            } else {
                ResourceDimension::CollectionItems
            },
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                if retained {
                    policy.limits.max_retained_bytes = cap;
                } else {
                    policy.limits.max_collection_items = cap;
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let scope_histories = HashMap::new();
                let resolution = ExtrudeProfileResolution {
                    entities: &[],
                    spatial_sketches: &[],
                    spatial_entities: &[],
                    histories: &[],
                    scope_histories: &scope_histories,
                    linear_tolerance: 0.000_001,
                    angular_tolerance: 0.000_000_001,
                    arrangement_budget: &arrangement_budget,
                    ctx: &ctx,
                };
                super::super::resolved_extrude_profile_selection(
                    &sketch.id,
                    &group,
                    std::slice::from_ref(&member),
                    &sketch,
                    resolution.scoped(&[]),
                    None,
                    None,
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems })));
    }
}

#[test]
fn extrude_selection_member_refuses_collection_limit() {
    assert_extrude_selection_refusal("f3d extrude selection member", true, false);
}

#[test]
fn extrude_selected_profile_refuses_collection_limit() {
    assert_extrude_selection_refusal("f3d extrude selected profile", true, false);
}

#[test]
fn extrude_selection_group_id_refuses_retained_limit() {
    assert_extrude_selection_refusal("f3d extrude selection group id", false, true);
}

#[test]
fn extrude_selection_group_refuses_collection_limit() {
    assert_extrude_selection_refusal("f3d extrude selection group", false, false);
}

fn transition_state(
    state_id: i64,
    topology: AsmHistoricalTopology,
    transition: Option<AsmHistoricalTransition>,
) -> AsmDeltaState {
    AsmDeltaState {
        id: format!("synthetic:transition-state-{state_id}"),
        parent: "synthetic:history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 0,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 0,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        topology_cache: AsmTopologyCache::Complete(topology),
        transition,
    }
}

fn empty_sketch() -> Sketch {
    Sketch {
        id: SketchId::mint("synthetic:test:id#transition-allocation-sketch").unwrap(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles:
            cadmpeg_ir::sketches::SketchProfiles::try_from(Vec::<Vec<SketchEntityUse>>::new())
                .unwrap(),
        native_ref: None,
    }
}

fn assert_transition_collection_refusal(operation: &'static str, deleted: bool) {
    let sketch = empty_sketch();
    let previous_topology = AsmHistoricalTopology {
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 50,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 50,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let transition = AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta {
            faces: AsmHistoricalEntityDelta {
                inserted: if deleted { Vec::new() } else { vec![10] },
                deleted: if deleted { vec![10, 11] } else { Vec::new() },
                ..AsmHistoricalEntityDelta::default()
            },
            ..AsmHistoricalTopologyDelta::default()
        },
    };
    let histories = [AsmHistory {
        id: "synthetic:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![
            transition_state(1, previous_topology, None),
            transition_state(2, AsmHistoricalTopology::default(), Some(transition)),
        ],
    }];
    let arrangement_budget = WorkBudget::new(MAX_ARRANGEMENT_WALK_WORK);
    for limit in 0..16 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scope_histories = HashMap::new();
        let resolution = ExtrudeProfileResolution {
            entities: &[],
            spatial_sketches: &[],
            spatial_entities: &[],
            histories: &histories,
            scope_histories: &scope_histories,
            linear_tolerance: 0.000_001,
            angular_tolerance: 0.000_000_001,
            arrangement_budget: &arrangement_budget,
            ctx: &ctx,
        };
        match super::super::transition_profile_selection(
            &sketch,
            resolution.scoped(&histories),
            2,
            1,
        ) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected transition refusal at {operation}: {other:?}"),
        }
    }
    panic!("no transition refusal at {operation}");
}

#[test]
fn transition_inserted_selection_refuses_collection_limit() {
    assert_transition_collection_refusal("f3d transition inserted selection", false);
}

#[test]
fn transition_cylindrical_selection_refuses_collection_limit() {
    assert_transition_collection_refusal("f3d transition cylindrical selection", false);
}

#[test]
fn transition_deleted_selection_refuses_collection_limit() {
    assert_transition_collection_refusal("f3d transition deleted selection", true);
}

fn assert_deleted_carrier_refusal(operation: &'static str) {
    let topology = AsmHistoricalTopology {
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 50,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 50,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    for limit in 0..16 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match super::super::unique_multi_face_deleted_carrier_family(&[10, 11], &topology, &ctx) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected deleted carrier refusal at {operation}: {other:?}"),
        }
    }
    panic!("no deleted carrier refusal at {operation}");
}

#[test]
fn deleted_face_uniqueness_refuses_collection_limit() {
    assert_deleted_carrier_refusal("f3d deleted face uniqueness index");
}

#[test]
fn deleted_carrier_family_refuses_collection_limit() {
    assert_deleted_carrier_refusal("f3d deleted carrier family index");
}

#[test]
fn deleted_carrier_face_refuses_collection_limit() {
    assert_deleted_carrier_refusal("f3d deleted carrier family face");
}

fn assert_inserted_selection_refusal(operation: &'static str, region: bool) {
    use super::super::ResolvedProfileSelection;

    let sketch = empty_sketch();
    for limit in 0..16 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let selections = if region {
            let region = cadmpeg_ir::features::SketchProfileRegion::loops(
                0,
                vec![1],
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("fixture loop-region admission")
            .unwrap();
            vec![
                Some(ResolvedProfileSelection::Regions(vec![region])),
                Some(ResolvedProfileSelection::Loops(vec![0])),
                Some(ResolvedProfileSelection::Loops(vec![1])),
            ]
        } else {
            vec![
                Some(ResolvedProfileSelection::Loops(vec![0])),
                Some(ResolvedProfileSelection::Loops(vec![1])),
            ]
        };
        match super::super::transition_inserted_profile_selection(
            &sketch,
            &[],
            0.000_001,
            selections,
            &ctx,
        ) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected inserted selection refusal at {operation}: {other:?}"),
        }
    }
    panic!("no inserted selection refusal at {operation}");
}

#[test]
fn inserted_transition_loop_refuses_collection_limit() {
    assert_inserted_selection_refusal("f3d inserted transition profile loop", false);
}

#[test]
fn inserted_transition_hole_refuses_collection_limit() {
    assert_inserted_selection_refusal("f3d inserted transition region hole", true);
}

#[test]
fn inserted_transition_region_refuses_collection_limit() {
    assert_inserted_selection_refusal("f3d inserted transition region", true);
}

#[test]
fn cylindrical_profile_projected_points_refuse_collection_limit() {
    let sketch_id = SketchId::mint("synthetic:test:id#cylinder-allocation-sketch").unwrap();
    let circle_id = SketchEntityId::mint("synthetic:test:id#cylinder-allocation-circle").unwrap();
    let circle = SketchEntity::new(
        circle_id.clone(),
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
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
            entity: circle_id,
            reversed: false,
        }]])
        .unwrap(),
        native_ref: None,
    };
    let topology = AsmHistoricalTopology {
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 10,
            member_refs: vec![11],
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 11,
            member_refs: vec![12, 13, 14],
        }],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 12,
                owner_loop: 11,
                edge: 20,
                next: 13,
                previous: 14,
                radial_next: 12,
            },
            AsmHistoricalCoedge {
                coedge: 13,
                owner_loop: 11,
                edge: 21,
                next: 14,
                previous: 12,
                radial_next: 13,
            },
            AsmHistoricalCoedge {
                coedge: 14,
                owner_loop: 11,
                edge: 22,
                next: 12,
                previous: 13,
                radial_next: 14,
            },
        ],
        edge_vertices: vec![
            AsmHistoricalEdge {
                edge: 20,
                start_vertex: 30,
                end_vertex: 31,
            },
            AsmHistoricalEdge {
                edge: 21,
                start_vertex: 31,
                end_vertex: 32,
            },
            AsmHistoricalEdge {
                edge: 22,
                start_vertex: 32,
                end_vertex: 30,
            },
        ],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 30,
                carrier: 40,
            },
            AsmHistoricalCarrierBinding {
                entity: 31,
                carrier: 41,
            },
            AsmHistoricalCarrierBinding {
                entity: 32,
                carrier: 42,
            },
        ],
        point_positions: vec![
            AsmHistoricalPoint {
                point: 40,
                position: Point3::new(2.0, 0.0, 0.0),
            },
            AsmHistoricalPoint {
                point: 41,
                position: Point3::new(0.0, 2.0, 1.0),
            },
            AsmHistoricalPoint {
                point: 42,
                position: Point3::new(-2.0, 0.0, 0.0),
            },
        ],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 10,
            carrier: 50,
        }],
        surface_cylinders: vec![AsmHistoricalCylinder {
            surface: 50,
            origin: Point3::new(0.0, 0.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            radius: 2.0,
        }],
        ..AsmHistoricalTopology::default()
    };
    for limit in 0..32 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match super::super::inserted_cylindrical_profile_selection(
            &sketch,
            std::slice::from_ref(&circle),
            &topology,
            10,
            0.000_001,
            0.000_000_001,
            &ctx,
        ) {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d cylindrical profile projected point" =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected cylindrical projected point refusal: {other:?}"),
        }
    }
    panic!("no cylindrical projected point refusal");
}

fn assert_historical_face_profile_refusal(operation: &'static str, retained: bool) {
    let group = DesignExtrudeSelectionGroup::try_from(
        crate::records::topology::extrude_selection::DesignExtrudeSelectionGroupWire {
            id: "f3d:Design/BulkStream.dat:selection-group#9".into(),
            scope_record_index: 7,
            scope_reference_ordinal: 0,
            record_index: 9,
            byte_offset: 0,
            class_tag: "277".to_owned(),
            member_count_offset: 32,
            members: vec![10],
            member_offsets: vec![37],
            opaque_index: 1,
            opaque_index_offset: 47,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 51,
            variant: false,
            paired_class_tag: "277".to_owned(),
            paired_byte_offset: 100,
        },
    )
    .unwrap();
    let mut member = historical_point_member();
    member.id = "f3d:Design/BulkStream.dat:selection-member#10".into();
    member.local_id = 10;
    member.historical = Some(HistoricalBinding {
        kind: AsmHistoricalEntityKind::Face,
        entity_ref: 10,
        state_ids: vec![2],
    });
    let histories = [AsmHistory {
        id: "f3d:Design/BulkStream.dat:history#1".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![transition_state(
            2,
            AsmHistoricalTopology {
                faces: vec![10],
                ..AsmHistoricalTopology::default()
            },
            None,
        )],
    }];
    let feature = cadmpeg_ir::features::FeatureId::mint("synthetic:test:feature#1").unwrap();
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            if retained {
                ResourceDimension::RetainedBytes
            } else {
                ResourceDimension::CollectionItems
            },
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                if retained {
                    policy.limits.max_retained_bytes = cap;
                } else {
                    policy.limits.max_collection_items = cap;
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                super::super::historical_face_profile_selection(
                    &[&group],
                    std::slice::from_ref(&member),
                    Some(2),
                    &feature,
                    &histories,
                    &ctx,
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems })));
    }
}

#[test]
fn historical_profile_group_member_refuses_collection_limit() {
    assert_historical_face_profile_refusal("f3d historical profile group member", false);
}

#[test]
fn historical_profile_selected_face_refuses_collection_limit() {
    assert_historical_face_profile_refusal("f3d historical profile selected face", false);
}

#[test]
fn historical_profile_face_id_refuses_collection_limit() {
    assert_historical_face_profile_refusal("f3d historical profile face id", false);
}

#[test]
fn historical_profile_group_id_entry_refuses_collection_limit() {
    assert_historical_face_profile_refusal("f3d historical profile group id entry", false);
}

#[test]
fn historical_profile_group_id_refuses_retained_limit() {
    assert_historical_face_profile_refusal("f3d historical profile group id", true);
}
