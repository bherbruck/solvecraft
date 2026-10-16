// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge,
    AsmHistoricalOptionalCarrierBinding, AsmHistoricalPoint, AsmHistoricalRelation,
    AsmHistoricalTopology,
};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use cadmpeg_core::CodecError;

fn triangle_topology() -> AsmHistoricalTopology {
    AsmHistoricalTopology {
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
                position: Point3::new(0.0, 0.0, 0.0),
            },
            AsmHistoricalPoint {
                point: 41,
                position: Point3::new(2.0, 0.0, 0.0),
            },
            AsmHistoricalPoint {
                point: 42,
                position: Point3::new(0.0, 1.0, 0.0),
            },
        ],
        edge_curves: vec![AsmHistoricalOptionalCarrierBinding {
            entity: 20,
            carrier: Some(60),
        }],
        coedge_pcurves: vec![AsmHistoricalOptionalCarrierBinding {
            entity: 12,
            carrier: Some(70),
        }],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 10,
            carrier: 50,
        }],
        body_regions: vec![AsmHistoricalRelation {
            owner_ref: 1,
            member_refs: vec![2],
        }],
        region_shells: vec![AsmHistoricalRelation {
            owner_ref: 2,
            member_refs: vec![3],
        }],
        shell_faces: vec![AsmHistoricalRelation {
            owner_ref: 3,
            member_refs: vec![10],
        }],
        ..AsmHistoricalTopology::default()
    }
}

fn assert_historical_refusal(kind: AsmHistoricalEntityKind, id: i64, operation: &'static str) {
    let topology = triangle_topology();
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match super::super::historical_entity_positions(kind, id, &topology, &ctx) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected historical refusal at {operation}: {other:?}"),
        }
    }
    panic!("no refusal at {operation}");
}

macro_rules! historical_refusal {
    ($name:ident, $kind:expr, $id:expr, $operation:literal) => {
        #[test]
        fn $name() {
            assert_historical_refusal($kind, $id, $operation);
        }
    };
}

historical_refusal!(
    historical_edge_ref_refuses_limit,
    AsmHistoricalEntityKind::Edge,
    20,
    "f3d historical direct edge reference"
);
historical_refusal!(
    historical_coedge_ref_refuses_limit,
    AsmHistoricalEntityKind::Coedge,
    12,
    "f3d historical coedge edge reference"
);
historical_refusal!(
    historical_curve_ref_refuses_limit,
    AsmHistoricalEntityKind::Curve,
    60,
    "f3d historical curve edge reference"
);
historical_refusal!(
    historical_loop_ref_refuses_limit,
    AsmHistoricalEntityKind::Loop,
    11,
    "f3d historical loop edge reference"
);
historical_refusal!(
    historical_pcurve_ref_refuses_limit,
    AsmHistoricalEntityKind::Pcurve,
    70,
    "f3d historical pcurve edge reference"
);
historical_refusal!(
    historical_vertex_position_refuses_limit,
    AsmHistoricalEntityKind::Vertex,
    30,
    "f3d historical vertex position"
);
historical_refusal!(
    historical_point_position_refuses_limit,
    AsmHistoricalEntityKind::Point,
    40,
    "f3d historical point position"
);
historical_refusal!(
    historical_edge_position_refuses_limit,
    AsmHistoricalEntityKind::Edge,
    20,
    "f3d historical edge position"
);
historical_refusal!(
    historical_face_position_refuses_limit,
    AsmHistoricalEntityKind::Face,
    10,
    "f3d historical face position"
);
historical_refusal!(
    historical_surface_position_refuses_limit,
    AsmHistoricalEntityKind::Surface,
    50,
    "f3d historical surface position"
);
historical_refusal!(
    historical_owned_face_refuses_limit,
    AsmHistoricalEntityKind::Body,
    1,
    "f3d historical owned face"
);
historical_refusal!(
    historical_owned_face_position_refuses_limit,
    AsmHistoricalEntityKind::Body,
    1,
    "f3d historical owned face position"
);

#[test]
fn historical_face_point_refuses_collection_limit() {
    let topology = triangle_topology();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::profile_select::historical_face_points(10, &topology, &ctx),
        Err(CodecError::ResourceLimit(failure)) if failure.operation == "f3d historical face point"
    ));
}
