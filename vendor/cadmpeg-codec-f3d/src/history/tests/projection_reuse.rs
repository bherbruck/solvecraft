// SPDX-License-Identifier: Apache-2.0
//! Projection indexes retain ambiguity and reuse immutable snapshots.
use crate::history::{
    cache::SnapshotCache, common_face_vertex, face_boundary_edges, history_state_index,
    topology_cache::TopologyQueryCache,
};
use crate::history_records::{
    AsmDeltaState, AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalRelation,
    AsmHistoricalTopology, AsmHistory,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn state(state_id: i64) -> AsmDeltaState {
    AsmDeltaState {
        id: format!("f3d:history:state#{state_id}"),
        parent: "f3d:history".into(),
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
        topology_cache: crate::history_records::AsmTopologyCache::Absent,
        transition: None,
    }
}
#[test]
fn history_state_queries_reuse_indexes_and_reject_duplicate_states() {
    let history = |states| AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states,
    };
    let duplicate = history(vec![state(1), state(1)]);
    let unique = history(vec![state(1)]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One state key and one snapshot key per history; repetitions add no index slots.
    policy.limits.max_collection_items = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cache = SnapshotCache::default();
    for _ in 0..1_000 {
        assert!(cache.get(&ctx, &duplicate, history_state_index).unwrap()[&1].is_none());
        assert_eq!(
            cache.get(&ctx, &unique, history_state_index).unwrap()[&1]
                .unwrap()
                .state_id,
            1
        );
    }
    ctx.finish_session().unwrap();
}
fn boundary(vertex: i64, edge: i64) -> AsmHistoricalTopology {
    AsmHistoricalTopology {
        faces: vec![10],
        vertices: vec![vertex],
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 10,
            member_refs: vec![20],
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 20,
            member_refs: vec![30],
        }],
        coedge_topology: vec![AsmHistoricalCoedge {
            coedge: 30,
            owner_loop: 20,
            edge,
            next: 30,
            previous: 30,
            radial_next: 30,
        }],
        edge_vertices: vec![AsmHistoricalEdge {
            edge,
            start_vertex: vertex,
            end_vertex: vertex,
        }],
        ..Default::default()
    }
}
#[test]
fn vertex_boundary_queries_reuse_indexes_and_keep_snapshots_separate() {
    let first = boundary(40, 50);
    let second = boundary(41, 51);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Two indexes plus one output vertex per query fit; rebuilding two indexes per round does not.
    policy.limits.max_collection_items = 2_500;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cache = TopologyQueryCache::default();
    for _ in 0..1_000 {
        assert_eq!(
            common_face_vertex(&ctx, &[10], &first, &mut cache).unwrap(),
            Some(40)
        );
        assert_eq!(
            common_face_vertex(&ctx, &[10], &second, &mut cache).unwrap(),
            Some(41)
        );
    }
    ctx.finish_session().unwrap();
}
#[test]
fn edge_boundary_queries_reuse_indexes_and_keep_snapshots_separate() {
    let first = boundary(40, 50);
    let second = boundary(41, 51);
    let face = cadmpeg_ir::ids::FaceId::mint("f3d:brep:entity#10").unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 4_500;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cache = TopologyQueryCache::default();
    for _ in 0..1_000 {
        assert_eq!(
            face_boundary_edges(&ctx, std::slice::from_ref(&face), &first, &mut cache).unwrap(),
            [50]
        );
        assert_eq!(
            face_boundary_edges(&ctx, std::slice::from_ref(&face), &second, &mut cache).unwrap(),
            [51]
        );
    }
    ctx.finish_session().unwrap();
}
#[test]
fn duplicate_boundary_incidence_keeps_aggregate_and_unique_query_semantics() {
    let mut topology = boundary(40, 50);
    topology.face_loops.push(topology.face_loops[0].clone());
    let face = cadmpeg_ir::ids::FaceId::mint("f3d:brep:entity#10").unwrap();
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut cache = TopologyQueryCache::default();
    assert_eq!(
        face_boundary_edges(&ctx, &[face], &topology, &mut cache).unwrap(),
        [50]
    );
    assert_eq!(
        common_face_vertex(&ctx, &[10], &topology, &mut cache).unwrap(),
        None
    );
}

#[test]
fn loop_and_vertex_queries_share_the_endpoint_index() {
    let mut topology = boundary(40, 50);
    topology
        .edge_vertices
        .extend((1_000..1_511).map(|edge| AsmHistoricalEdge {
            edge,
            start_vertex: 40,
            end_vertex: 40,
        }));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // 512 endpoint entries plus the queried face and loop indexes fit. A second endpoint table does not.
    policy.limits.max_collection_items = 600;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cache = TopologyQueryCache::default();
    assert_eq!(
        common_face_vertex(&ctx, &[10], &topology, &mut cache).unwrap(),
        Some(40)
    );
    let contexts =
        crate::history::face_boundary_contexts_for_slots(&ctx, &[10], &topology, &mut cache)
            .unwrap();
    assert_eq!(contexts.len(), 1);
    assert!(
        matches!(&contexts[0].loops[0].boundary, crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Vertices(rows) if rows.len() == 1 && rows[0].vertex_slot == 40)
    );
    ctx.finish_session().unwrap();
}
