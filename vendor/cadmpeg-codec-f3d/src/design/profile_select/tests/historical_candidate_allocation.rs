// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalRelation,
    AsmHistoricalTopology,
};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn candidate_topology() -> AsmHistoricalTopology {
    AsmHistoricalTopology {
        faces: vec![10],
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 10,
            member_refs: vec![11],
        }],
        coedge_topology: vec![AsmHistoricalCoedge {
            coedge: 12,
            owner_loop: 11,
            edge: 30,
            previous: 12,
            next: 12,
            radial_next: 12,
        }],
        edge_vertices: vec![AsmHistoricalEdge {
            edge: 30,
            start_vertex: 60,
            end_vertex: 61,
        }],
        vertex_points: vec![AsmHistoricalCarrierBinding {
            entity: 60,
            carrier: 70,
        }],
        ..AsmHistoricalTopology::default()
    }
}

fn assert_candidate_refusal(
    kind: AsmHistoricalEntityKind,
    entity: i64,
    limit: u64,
    operation: &'static str,
) {
    let topology = candidate_topology();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = limit;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        historical_profile_face_candidates(Some(kind), entity, &topology, &ctx),
        Err(CodecError::ResourceLimit(failure)) if failure.operation == operation
    ));
}

#[test]
fn historical_loop_face_candidate_refuses_collection_limit() {
    assert_candidate_refusal(
        AsmHistoricalEntityKind::Loop,
        11,
        0,
        "f3d historical loop face candidate",
    );
}

#[test]
fn historical_coedge_face_candidate_refuses_collection_limit() {
    assert_candidate_refusal(
        AsmHistoricalEntityKind::Coedge,
        12,
        1,
        "f3d historical coedge face candidate",
    );
}

#[test]
fn historical_edge_face_candidate_refuses_collection_limit() {
    assert_candidate_refusal(
        AsmHistoricalEntityKind::Edge,
        30,
        1,
        "f3d historical edge face candidate",
    );
}

#[test]
fn historical_point_vertex_candidate_refuses_collection_limit() {
    assert_candidate_refusal(
        AsmHistoricalEntityKind::Point,
        70,
        0,
        "f3d historical point vertex candidate",
    );
}

#[test]
fn historical_profile_face_candidate_refuses_collection_limit() {
    assert_candidate_refusal(
        AsmHistoricalEntityKind::Face,
        10,
        0,
        "f3d historical profile face candidate",
    );
}
