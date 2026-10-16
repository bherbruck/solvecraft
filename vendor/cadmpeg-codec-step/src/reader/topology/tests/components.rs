// SPDX-License-Identifier: Apache-2.0
//! Connected shell component admission.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{CoedgeId, EdgeId, FaceId, LoopId, VertexId};
use cadmpeg_ir::topology::{Coedge, Loop, LoopBoundary, LoopRing, Sense};

fn faces() -> [FaceId; 2] {
    [
        FaceId::try_from("step:data:face#1").expect("test face id"),
        FaceId::try_from("step:data:face#2").expect("test face id"),
    ]
}

fn assert_limit(
    faces: &[FaceId],
    loops: &[Loop],
    max_collection_items: u64,
    operation: &'static str,
) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    let error = super::super::connected_face_components(faces, loops, &[], &BTreeMap::new(), &ctx)
        .expect_err("component scratch exceeds collection limit");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == operation
    ));
}

#[test]
fn connected_face_neighbors_refuse_collection_limit() {
    let faces = faces();
    let arena = DecodeArena::new();
    let (service_ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    let components =
        super::super::connected_face_components(&faces, &[], &[], &BTreeMap::new(), &service_ctx)
            .expect("service admits two face neighbors");
    assert_eq!(components, vec![vec![0], vec![1]]);

    assert_limit(&faces, &[], 1, "STEP connected-face neighbors");
}

#[test]
fn connected_face_indices_refuse_collection_limit() {
    assert_limit(&faces(), &[], 2, "STEP connected-face indices");
}

#[test]
fn connected_face_component_worklists_refuse_collection_limits() {
    let faces = faces();
    assert_limit(&faces, &[], 4, "STEP connected-face reached");
    assert_limit(&faces, &[], 6, "STEP connected-face pending");
    assert_limit(&faces, &[], 7, "STEP connected-face component");
    assert_limit(&faces, &[], 8, "STEP connected-face components");
}

#[test]
fn connected_face_shared_vertex_groups_and_links_refuse_collection_limits() {
    let faces = faces();
    let vertex = VertexId::try_from("step:data:vertex#1").expect("test vertex id");
    let loops = [
        Loop {
            id: LoopId::try_from("step:data:loop#1").expect("test loop id"),
            face: faces[0].clone(),
            boundary: LoopBoundary::Vertex {
                vertex: vertex.clone(),
                pcurves: Vec::new(),
            },
        },
        Loop {
            id: LoopId::try_from("step:data:loop#2").expect("test loop id"),
            face: faces[1].clone(),
            boundary: LoopBoundary::Vertex {
                vertex,
                pcurves: Vec::new(),
            },
        },
    ];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert_eq!(
        super::super::connected_face_components(&faces, &loops, &[], &BTreeMap::new(), &ctx)
            .expect("shared vertex joins faces"),
        vec![vec![0, 1]]
    );
    assert_limit(&faces, &loops, 4, "STEP connected-face groups");
    assert_limit(&faces, &loops, 5, "STEP connected-face group faces");
    assert_limit(&faces, &loops, 7, "STEP connected-face links");
}

#[test]
fn connected_face_coedge_map_refuses_collection_limit() {
    let faces = faces();
    let loop_id = LoopId::try_from("step:data:loop#1").expect("test loop id");
    let coedge_id = CoedgeId::try_from("step:data:coedge#1").expect("test coedge id");
    let edge_id = EdgeId::try_from("step:data:edge#1").expect("test edge id");
    let loops = [Loop {
        id: loop_id.clone(),
        face: faces[0].clone(),
        boundary: LoopBoundary::Ring(LoopRing::single(coedge_id.clone())),
    }];
    let coedges = [Coedge {
        id: coedge_id.clone(),
        owner_loop: loop_id,
        edge: edge_id,
        radial_next: coedge_id,
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
    }];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 4;
    let (limited_ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    let error = super::super::connected_face_components(
        &faces,
        &loops,
        &coedges,
        &BTreeMap::new(),
        &limited_ctx,
    )
    .expect_err("coedge lookup exceeds collection limit");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "STEP connected-face coedge edges"
    ));
}

#[test]
fn shared_vertex_components_use_spanning_links_and_preserve_partitions() {
    let count = 100;
    let faces = (0..count)
        .map(|id| FaceId::try_from(format!("step:data:face#{id}")).expect("face id"))
        .collect::<Vec<_>>();
    for groups in [1, 10, 100] {
        let loops = faces
            .iter()
            .enumerate()
            .map(|(index, face)| Loop {
                id: LoopId::try_from(format!("step:data:loop#{index}")).expect("loop id"),
                face: face.clone(),
                boundary: LoopBoundary::Vertex {
                    vertex: VertexId::try_from(format!("step:data:vertex#{}", index % groups))
                        .expect("vertex id"),
                    pcurves: Vec::new(),
                },
            })
            .collect::<Vec<_>>();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Face scratch, indexes, group memberships, spanning links, and DFS
        // outputs total eight entries per face for these disjoint groups.
        policy.limits.max_collection_items = cadmpeg_core::decode::u64_from_index(count) * 8;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
        let actual =
            super::super::connected_face_components(&faces, &loops, &[], &BTreeMap::new(), &ctx)
                .expect("component storage grows with membership, not group pairs");
        let expected = (0..groups)
            .map(|group| (group..count).step_by(groups).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
}
