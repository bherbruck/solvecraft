// SPDX-License-Identifier: Apache-2.0
//! Treatment candidate behavior over historical topology.
#![allow(clippy::unwrap_used)]

use crate::history::{treatment_edge_candidates, treatment_radius_candidates};
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalRelation,
    AsmHistoricalSurfaceRadius, AsmHistoricalTopology,
};

#[test]
fn treatment_radius_candidates_require_a_new_radius_carrier_and_deleted_support_edge() {
    use cadmpeg_ir::ids::FaceId;

    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    let coedge = |coedge, owner_loop, edge| AsmHistoricalCoedge {
        coedge,
        owner_loop,
        edge,
        next: coedge,
        previous: coedge,
        radial_next: coedge,
    };
    let preceding = AsmHistoricalTopology {
        faces: vec![10, 11],
        surfaces: vec![100, 101],
        face_loops: vec![relation(10, vec![110]), relation(11, vec![111])],
        loop_coedges: vec![relation(110, vec![1100]), relation(111, vec![1110])],
        coedge_topology: vec![coedge(1100, 110, 17), coedge(1110, 111, 17)],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 101,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let result = AsmHistoricalTopology {
        faces: vec![10, 11, 20],
        surfaces: vec![100, 101, 200],
        surface_radii: vec![AsmHistoricalSurfaceRadius {
            surface: 200,
            radius: 3.0,
        }],
        face_loops: vec![
            relation(10, vec![210]),
            relation(11, vec![211]),
            relation(20, vec![220]),
        ],
        loop_coedges: vec![
            relation(210, vec![2100]),
            relation(211, vec![2110]),
            relation(220, vec![2200, 2201]),
        ],
        coedge_topology: vec![
            coedge(2100, 210, 30),
            coedge(2110, 211, 31),
            coedge(2200, 220, 30),
            coedge(2201, 220, 31),
        ],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 101,
            },
            AsmHistoricalCarrierBinding {
                entity: 20,
                carrier: 200,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    // The transition index is built once; each query admits one deleted edge and two output rows.
    policy.limits.max_collection_items = 4_000;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cache = crate::history::treatment_index::TreatmentCache::default();
    for _ in 0..1_000 {
        let (radii, edges) =
            treatment_edge_candidates(&ctx, None, &[20], &result, &preceding, &[17], &mut cache)
                .unwrap();
        assert_eq!(edges, [17]);
        assert_eq!(radii.len(), 1);
        assert_eq!(radii[0].radius.get(), 3.0);
    }
    ctx.finish_session().unwrap();
    let candidates = crate::test_support::with_decode_context(|decode_ctx| {
        treatment_radius_candidates(
            decode_ctx,
            Some(&[FaceId::mint("f3d:brep:entity#10").expect("identity grammar")]),
            &[20],
            &result,
            &preceding,
            &[17],
            &mut crate::history::treatment_index::TreatmentCache::default(),
        )
    })
    .unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].edge_slot, 17);
    assert_eq!(candidates[0].radius.get(), 3.0);
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| treatment_edge_candidates(
            ctx,
            None,
            &[20],
            &result,
            &preceding,
            &[17],
            &mut crate::history::treatment_index::TreatmentCache::default()
        ))
        .unwrap()
        .1,
        [17]
    );

    let mut existing_carrier = preceding.clone();
    existing_carrier.surfaces.push(200);
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| treatment_radius_candidates(
            decode_ctx,
            Some(&[FaceId::mint("f3d:brep:entity#10").expect("identity grammar")]),
            &[20],
            &result,
            &existing_carrier,
            &[17],
            &mut crate::history::treatment_index::TreatmentCache::default()
        ))
        .unwrap()
        .is_empty()
    );
    assert!(
        crate::test_support::with_decode_context(|ctx| treatment_edge_candidates(
            ctx,
            None,
            &[20],
            &result,
            &preceding,
            &[18],
            &mut crate::history::treatment_index::TreatmentCache::default()
        ))
        .unwrap()
        .1
        .is_empty()
    );
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| treatment_radius_candidates(
            decode_ctx,
            Some(&[FaceId::mint("f3d:brep:entity#10").expect("identity grammar")]),
            &[20],
            &result,
            &preceding,
            &[18],
            &mut crate::history::treatment_index::TreatmentCache::default()
        ))
        .unwrap()
        .is_empty()
    );
}
