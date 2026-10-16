// SPDX-License-Identifier: Apache-2.0

fn simple_topology() -> crate::history_records::AsmHistoricalTopology {
    use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
    AsmHistoricalTopology {
        bodies: vec![1],
        body_regions: vec![AsmHistoricalRelation {
            owner_ref: 1,
            member_refs: Vec::new(),
        }],
        ..Default::default()
    }
}

fn intersection_error(max_items: u64) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::collections::BTreeSet;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::bodies_intersecting(&ctx, &simple_topology(), &BTreeSet::from([1])).unwrap_err()
}

#[test]
fn historical_relation_index_refuses_collection_limit() {
    let error = intersection_error(0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical relations")
    );
}

#[test]
fn historical_body_closure_refuses_collection_limit() {
    let error = intersection_error(1);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D historical body closure")
    );
}

#[test]
fn affected_topology_bodies_refuse_collection_limit() {
    let error = intersection_error(4);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D affected topology bodies")
    );
}

fn index_error(
    topology: &crate::history_records::AsmHistoricalTopology,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::bodies_intersecting(&ctx, topology, &std::collections::BTreeSet::new())
        .unwrap_err()
}

#[test]
fn historical_coedge_index_refuses_collection_limit() {
    use crate::history_records::{AsmHistoricalCoedge, AsmHistoricalTopology};
    let mut topology = AsmHistoricalTopology::default();
    topology.coedge_topology.push(AsmHistoricalCoedge {
        coedge: 1,
        owner_loop: 2,
        edge: 3,
        next: 1,
        previous: 1,
        radial_next: 1,
    });
    let error = index_error(&topology);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical coedges")
    );
}

#[test]
fn historical_edge_index_refuses_collection_limit() {
    use crate::history_records::{AsmHistoricalEdge, AsmHistoricalTopology};
    let mut topology = AsmHistoricalTopology::default();
    topology.edge_vertices.push(AsmHistoricalEdge {
        edge: 1,
        start_vertex: 2,
        end_vertex: 3,
    });
    let error = index_error(&topology);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical edge vertices")
    );
}

#[test]
fn historical_carrier_index_refuses_collection_limit() {
    use crate::history_records::{AsmHistoricalCarrierBinding, AsmHistoricalTopology};
    let mut topology = AsmHistoricalTopology::default();
    topology.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: 1,
        carrier: 2,
    });
    let error = index_error(&topology);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical carriers")
    );
}

#[test]
fn historical_optional_carrier_index_refuses_collection_limit() {
    use crate::history_records::{AsmHistoricalOptionalCarrierBinding, AsmHistoricalTopology};
    let mut topology = AsmHistoricalTopology::default();
    topology
        .edge_curves
        .push(AsmHistoricalOptionalCarrierBinding {
            entity: 1,
            carrier: None,
        });
    let error = index_error(&topology);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical optional carriers")
    );
}

fn wire_topology(free_vertex: bool) -> crate::history_records::AsmHistoricalTopology {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalEdge, AsmHistoricalRelation,
        AsmHistoricalTopology,
    };
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    AsmHistoricalTopology {
        bodies: vec![1],
        body_regions: vec![relation(1, vec![2])],
        region_shells: vec![relation(2, vec![3])],
        shell_faces: vec![relation(3, Vec::new())],
        shell_wire_edges: vec![relation(3, vec![7])],
        shell_free_vertices: vec![relation(3, if free_vertex { vec![8] } else { Vec::new() })],
        edge_vertices: vec![AsmHistoricalEdge {
            edge: 7,
            start_vertex: 8,
            end_vertex: 9,
        }],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 8,
                carrier: 28,
            },
            AsmHistoricalCarrierBinding {
                entity: 9,
                carrier: 29,
            },
        ],
        ..Default::default()
    }
}

fn wire_error(max_items: u64, free_vertex: bool) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::bodies_intersecting(
        &ctx,
        &wire_topology(free_vertex),
        &std::collections::BTreeSet::from([1]),
    )
    .unwrap_err()
}

#[test]
fn historical_shell_edges_refuse_collection_limit() {
    let error = wire_error(11, false);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D historical shell edges")
    );
}

#[test]
fn historical_shell_vertices_refuse_collection_limit() {
    let error = wire_error(12, true);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D historical shell vertices")
    );
}

#[test]
fn historical_edge_vertices_refuse_collection_limit() {
    let error = wire_error(13, false);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D historical shell vertices")
    );
}

#[test]
fn repeated_coedge_incidence_visits_each_edge_and_vertex_once() {
    use crate::history_records::{AsmHistoricalCoedge, AsmHistoricalRelation};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::collections::BTreeSet;

    let mut topology = wire_topology(false);
    topology.shell_wire_edges[0].member_refs.clear();
    topology.shell_faces[0].member_refs.push(4);
    topology.face_loops.push(AsmHistoricalRelation {
        owner_ref: 4,
        member_refs: vec![5],
    });
    topology
        .face_surfaces
        .push(crate::history_records::AsmHistoricalCarrierBinding {
            entity: 4,
            carrier: 6,
        });
    topology.loop_coedges.push(AsmHistoricalRelation {
        owner_ref: 5,
        member_refs: (100..200).collect(),
    });
    for coedge in 100..200 {
        topology.coedge_topology.push(AsmHistoricalCoedge {
            coedge,
            owner_loop: 5,
            edge: 7,
            next: if coedge == 199 { 100 } else { coedge + 1 },
            previous: if coedge == 100 { 199 } else { coedge - 1 },
            radial_next: coedge,
        });
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 240;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let bodies = super::super::bodies_intersecting(&ctx, &topology, &BTreeSet::from([29]))
        .unwrap()
        .unwrap();
    assert_eq!(bodies, BTreeSet::from([1]));
    ctx.finish_session().unwrap();
}

#[test]
fn repeated_body_queries_reuse_incidence_and_validate_each_snapshot() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::collections::BTreeSet;
    let topology = simple_topology();
    let incomplete = crate::history_records::AsmHistoricalTopology {
        bodies: vec![1],
        ..Default::default()
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One relation, one closure member, one cached closure, one cache entry,
    // twenty returned body members, and one attempted closure member plus
    // one cache entry for the incomplete snapshot.
    policy.limits.max_collection_items = 26;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cache = super::super::HistoricalBodyClosureCache::default();
    for _ in 0..20 {
        assert_eq!(
            cache
                .intersecting(&ctx, &topology, &BTreeSet::from([1]))
                .unwrap(),
            Some(BTreeSet::from([1]))
        );
        assert_eq!(
            cache
                .intersecting(&ctx, &topology, &BTreeSet::from([2]))
                .unwrap(),
            Some(BTreeSet::new())
        );
        assert_eq!(
            cache
                .intersecting(&ctx, &incomplete, &BTreeSet::from([1]))
                .unwrap(),
            None
        );
    }
    ctx.finish_session().unwrap();
}
