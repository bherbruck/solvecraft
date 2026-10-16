// SPDX-License-Identifier: Apache-2.0
//! Admission limits for historical state indexes and change chains.

use crate::history::resolve_pattern_face_by_surface_radius;
use crate::history::selection::{historical_identity_edge, HistoricalIdentityIndex};
use crate::history::{
    collect_reference_edge_sets, face_boundary_contexts_for_slots, face_boundary_edge_index,
    face_boundary_edges, historical_edge_axis, historical_face_support_contexts,
    historical_loop_boundary, preceding_support_face_slots, selection::boundary_edges_in_changes,
    selection::faces_in_topology, selection::historical_edge_context,
    selection::recipe_selector_candidates, terminal_edge_recipe_faces,
    terminal_edge_recipe_reference_faces, treatment_edge_candidates, treatment_face_supports,
};
use crate::history::{
    edge_changes_across_state_chain, face_changes_across_state_chain, history_state_index,
};
use crate::history_records::{
    AsmDeltaState, AsmHistoricalEntityDelta, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
    AsmHistoricalTransition, AsmHistory,
};
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalEdge, AsmHistoricalRelation,
    AsmHistoricalSurfaceRadius,
};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use std::collections::HashMap;
use std::collections::HashSet;

#[test]
fn identity_index_refuses_history_collection_limit() {
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![change_state(1)],
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = HistoricalIdentityIndex::build(&ctx, &[history], [1])
        .err()
        .expect("limit refusal");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D identity histories")
    );
}

#[test]
fn identity_edges_refuse_collection_limit() {
    let topology = AsmHistoricalTopology {
        edges: vec![7],
        ..Default::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        historical_identity_edge(&ctx, AsmHistoricalEntityKind::Edge, 7, &topology).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D identity edges")
    );
}

fn edge_context_topology() -> AsmHistoricalTopology {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalRelation,
        AsmHistoricalSurfaceAxis,
    };
    AsmHistoricalTopology {
        coedge_topology: vec![AsmHistoricalCoedge {
            coedge: 6,
            owner_loop: 5,
            edge: 7,
            next: 6,
            previous: 6,
            radial_next: 6,
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 5,
            member_refs: vec![6],
        }],
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 4,
            member_refs: vec![5],
        }],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 4,
            carrier: 8,
        }],
        surface_axes: vec![AsmHistoricalSurfaceAxis {
            surface: 8,
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        }],
        ..Default::default()
    }
}

#[test]
fn historical_edge_context_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = historical_edge_context(&ctx, 7, &edge_context_topology()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D historical incident loops")
    );
}

#[test]
fn historical_edge_axis_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = historical_edge_axis(&ctx, 7, &edge_context_topology()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D historical incident loops")
    );
}

pub(super) fn change_state(state_id: i64) -> AsmDeltaState {
    AsmDeltaState {
        id: format!("state-{state_id}"),
        parent: "history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Complete(
            AsmHistoricalTopology::default(),
        ),
        transition: None,
    }
}

#[test]
fn history_state_index_refuses_collection_limit() {
    let history = AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![change_state(1)],
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = history_state_index(&ctx, &history).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D history states")
    );
}

#[test]
fn face_change_chain_refuses_visited_collection_limit() {
    let preceding = change_state(1);
    let mut result = change_state(2);
    result.transition = Some(AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    });
    let states = HashMap::from([(1, Some(&preceding)), (2, Some(&result))]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = face_changes_across_state_chain(&ctx, &result, 1, &states).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "track F3D face change state chain")
    );
}

#[test]
fn face_change_chain_refuses_changed_collection_limit() {
    let preceding = change_state(1);
    let mut result = change_state(2);
    let mut transition = AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    };
    transition.topology.faces.updated.push(10);
    result.transition = Some(transition);
    let states = HashMap::from([(1, Some(&preceding)), (2, Some(&result))]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = face_changes_across_state_chain(&ctx, &result, 1, &states).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D changed faces")
    );
}

#[test]
fn edge_change_chain_refuses_visited_collection_limit() {
    let preceding = change_state(1);
    let mut result = change_state(2);
    result.transition = Some(AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    });
    let states = HashMap::from([(1, Some(&preceding)), (2, Some(&result))]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = edge_changes_across_state_chain(&ctx, &result, 1, &states).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "track F3D edge change state chain")
    );
}

#[test]
fn edge_change_chain_refuses_deleted_collection_limit() {
    let preceding = change_state(1);
    let mut result = change_state(2);
    let mut transition = AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    };
    transition.topology.edges.deleted.push(20);
    result.transition = Some(transition);
    let states = HashMap::from([(1, Some(&preceding)), (2, Some(&result))]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = edge_changes_across_state_chain(&ctx, &result, 1, &states).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D deleted edges")
    );
}

#[test]
fn edge_change_chain_refuses_updated_collection_limit() {
    let preceding = change_state(1);
    let mut result = change_state(2);
    let mut transition = AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    };
    transition.topology.edges.updated.push(20);
    result.transition = Some(transition);
    let states = HashMap::from([(1, Some(&preceding)), (2, Some(&result))]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = edge_changes_across_state_chain(&ctx, &result, 1, &states).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D updated edges")
    );
}

fn pattern_face_limit_case(max_items: u64) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let candidate = crate::ids::brep_face_id(21);
    let preceding = AsmHistoricalTopology::default();
    let result = AsmHistoricalTopology {
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 21,
            carrier: 201,
        }],
        surface_radii: vec![AsmHistoricalSurfaceRadius {
            surface: 201,
            radius: 2.5,
        }],
        ..AsmHistoricalTopology::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    resolve_pattern_face_by_surface_radius(&ctx, &[candidate], &preceding, &result, &HashSet::new())
}

#[test]
fn pattern_face_candidate_index_refuses_collection_limit() {
    let error = pattern_face_limit_case(0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D pattern face candidates")
    );
}

#[test]
fn pattern_face_bound_index_refuses_collection_limit() {
    let error = pattern_face_limit_case(1).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D pattern bound faces")
    );
}

fn one_face_reference() -> crate::records::dimensions::DesignRecipeReference {
    crate::records::dimensions::DesignRecipeReference {
        selector: 0,
        selector_offset: 0,
        token: "face".into(),
        token_offset: 0,
        design_reference: 1,
        design_reference_offset: 0,
        candidate_faces: vec![crate::ids::brep_face_id(1)],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    }
}

#[test]
fn topology_face_index_refuses_collection_limit() {
    let topology = AsmHistoricalTopology {
        faces: vec![1],
        ..AsmHistoricalTopology::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        faces_in_topology(&ctx, &one_face_reference().candidate_faces, &topology).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D topology faces")
    );
}

#[test]
fn topology_face_copy_refuses_collection_limit() {
    let topology = AsmHistoricalTopology {
        faces: vec![1],
        ..AsmHistoricalTopology::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        faces_in_topology(&ctx, &one_face_reference().candidate_faces, &topology).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D faces in topology")
    );
}

#[test]
fn terminal_edge_recipe_face_union_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        terminal_edge_recipe_faces(&ctx, &one_face_reference().candidate_faces, &[]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D terminal edge recipe faces")
    );
}

#[test]
fn terminal_reference_group_refuses_collection_limit() {
    let mut reference = one_face_reference();
    reference.candidate_faces.clear();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = terminal_edge_recipe_reference_faces(&ctx, &[reference], None).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D terminal reference groups")
    );
}

#[test]
fn terminal_reference_face_copy_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        terminal_edge_recipe_reference_faces(&ctx, &[one_face_reference()], None).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D historical face identity")
    );
}

#[test]
fn reference_edge_set_groups_refuse_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let topology = AsmHistoricalTopology::default();
    let mut cache = crate::history::topology_cache::TopologyQueryCache::default();
    cache
        .boundaries
        .get(
            &cadmpeg_test_support::service_decode_context(),
            &topology,
            crate::history::topology_cache::aggregate_boundaries,
        )
        .unwrap();
    let error =
        collect_reference_edge_sets(&ctx, &[Vec::new()], &topology, &mut cache).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D reference edge sets")
    );
}

#[test]
fn boundary_topology_index_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = face_boundary_edges(
        &ctx,
        &one_face_reference().candidate_faces,
        &AsmHistoricalTopology {
            face_loops: vec![AsmHistoricalRelation {
                owner_ref: 1,
                member_refs: Vec::new(),
            }],
            ..Default::default()
        },
        &mut crate::history::topology_cache::TopologyQueryCache::default(),
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D aggregate boundary faces")
    );
}

#[test]
fn boundary_change_slots_refuse_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = boundary_edges_in_changes(&ctx, &[3], &[3]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D boundary edges in changes")
    );
}

#[test]
fn edge_recipe_selector_index_refuses_collection_limit() {
    use crate::records::topology::edge_recipe::{
        DesignEdgeRecipeStructure, DesignTopologyRecipeEntry, DesignTopologyRecipeSide,
        DesignTopologyRecipeTriplet,
    };
    let triplet = DesignTopologyRecipeTriplet {
        outer: std::num::NonZeroU32::new(1).unwrap(),
        middle: 0,
        incident: None,
    };
    let structure = DesignEdgeRecipeStructure {
        root: 2,
        sides: vec![DesignTopologyRecipeSide {
            header_value: 0,
            scalars: Vec::new(),
            payload_prefix: Vec::new(),
            entries: vec![DesignTopologyRecipeEntry {
                selector: 1,
                boundary_edge_count: std::num::NonZeroU32::new(1).unwrap(),
                topology_triplets: [triplet, triplet],
            }],
        }],
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = recipe_selector_candidates(&ctx, Some(&structure), &[]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D edge recipe selectors")
    );
}

#[test]
fn treatment_boundary_relation_index_refuses_collection_limit() {
    let topology = AsmHistoricalTopology {
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 1,
            member_refs: vec![2],
        }],
        ..AsmHistoricalTopology::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = face_boundary_edge_index(&ctx, &topology).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D boundary relations")
    );
}

#[test]
fn treatment_preceding_face_index_refuses_collection_limit() {
    let preceding = AsmHistoricalTopology {
        faces: vec![1],
        ..AsmHistoricalTopology::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = treatment_face_supports(
        &ctx,
        &[],
        &AsmHistoricalTopology::default(),
        &preceding,
        &HashMap::new(),
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D treatment preceding faces")
    );
}

#[test]
fn treatment_deleted_edge_index_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = AsmHistoricalTopology::default();
    let preceding = AsmHistoricalTopology::default();
    let mut cache = crate::history::treatment_index::TreatmentCache::default();
    cache
        .get(
            &cadmpeg_test_support::service_decode_context(),
            &result,
            &preceding,
        )
        .unwrap();
    let error = treatment_edge_candidates(&ctx, None, &[], &result, &preceding, &[1], &mut cache)
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D treatment deleted edges")
    );
}

#[test]
fn historical_support_face_index_refuses_collection_limit() {
    let preceding = AsmHistoricalTopology {
        faces: vec![1],
        ..AsmHistoricalTopology::default()
    };
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: Vec::new(),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = historical_face_support_contexts(
        &ctx,
        &[],
        &history,
        &preceding,
        &HashSet::new(),
        &mut crate::history::topology_cache::TopologyQueryCache::default(),
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical support faces")
    );
}

#[test]
fn face_boundary_loop_contexts_refuse_collection_limit() {
    let topology = AsmHistoricalTopology {
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 1,
            member_refs: vec![2],
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 2,
            member_refs: Vec::new(),
        }],
        ..AsmHistoricalTopology::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cache = crate::history::topology_cache::TopologyQueryCache::default();
    cache
        .loops
        .get(
            &cadmpeg_test_support::service_decode_context(),
            &topology,
            crate::history::loop_index::LoopIndex::new,
        )
        .unwrap();
    cache
        .endpoints
        .get(
            &cadmpeg_test_support::service_decode_context(),
            &topology,
            crate::history::topology_cache::endpoint_index,
        )
        .unwrap();
    let error = face_boundary_contexts_for_slots(&ctx, &[1], &topology, &mut cache).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D face boundary loops")
    );
}

#[test]
fn historical_loop_vertices_refuse_collection_limit() {
    let topology = AsmHistoricalTopology {
        edge_vertices: vec![AsmHistoricalEdge {
            edge: 2,
            start_vertex: 1,
            end_vertex: 1,
        }],
        ..AsmHistoricalTopology::default()
    };
    let coedges = vec![
        crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
            coedge_slot: 3,
            edge_slot: 2,
        },
    ];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = historical_loop_boundary(
        &ctx,
        coedges,
        &crate::history::loop_index::LoopIndex::new(
            &cadmpeg_test_support::service_decode_context(),
            &topology,
        )
        .unwrap(),
        &crate::history::topology_cache::endpoint_index(
            &cadmpeg_test_support::service_decode_context(),
            &topology,
        )
        .unwrap(),
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D loop vertices")
    );
}

#[test]
fn preceding_support_face_index_refuses_collection_limit() {
    let preceding = AsmHistoricalTopology {
        faces: vec![1],
        ..AsmHistoricalTopology::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        preceding_support_face_slots(&ctx, &[], &AsmHistoricalTopology::default(), &preceding)
            .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D preceding support faces")
    );
}
