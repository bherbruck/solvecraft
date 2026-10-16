// SPDX-License-Identifier: Apache-2.0
//! Mirror-history unit tests.

use crate::history::{
    discard_projection_caches, selection::historical_mirror_coedge_plane,
    selection::historical_mirror_face_operand_plane, selection::historical_mirror_plane,
    selection::historical_selection_identity_kind,
};
use crate::history_records::{AsmDeltaState, AsmHistory};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

#[test]
fn discard_projection_caches_retains_compact_mirror_plane_topology() {
    use crate::history_records::{
        AsmEntityVersion, AsmHistoricalCarrierBinding, AsmHistoricalPlane, AsmHistoricalTopology,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let topology = AsmHistoricalTopology {
        bodies: vec![99],
        faces: vec![10],
        surfaces: vec![20],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 10,
            carrier: 20,
        }],
        surface_planes: vec![AsmHistoricalPlane {
            surface: 20,
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }],
        ..Default::default()
    };
    let mut histories = [AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![AsmDeltaState {
            id: "history:state#1".into(),
            parent: "history".into(),
            byte_offset: 0,
            state_id: 1,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 1,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            entity_versions: vec![AsmEntityVersion {
                entity_ref: 10,
                record_ref: 30,
            }],
            topology_cache: crate::history_records::AsmTopologyCache::Complete(topology),
            transition: None,
        }],
    }];

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("decode context");
    discard_projection_caches(&ctx, &mut histories).expect("projection cache budget");

    let retained = histories[0].states[0]
        .topology()
        .expect("plane topology remains available to late Mirror binding");
    assert_eq!(retained.bodies, [99]);
    assert_eq!(retained.faces, [10]);
    assert_eq!(retained.surfaces, [20]);
    assert_eq!(retained.face_surfaces.len(), 1);
    assert_eq!(retained.surface_planes.len(), 1);
    assert_eq!(
        histories[0].states[0].entity_versions,
        [AsmEntityVersion {
            entity_ref: 10,
            record_ref: 30,
        }]
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| historical_selection_identity_kind(
            decode_ctx, &histories, 30
        ))
        .unwrap(),
        Some((AsmHistoricalEntityKind::Face, 10, vec![1]))
    );
}

#[test]
fn mirror_face_recipe_accepts_coincident_preceding_plane_faces() {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalPlane, AsmHistoricalTopology,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let operand: crate::records::topology::face::DesignFaceOperand =
        serde_json::from_value(serde_json::json!({
            "id": "f3d:Design/BulkStream.dat:design-face-operand#40",
            "scope_record_index": 42,
            "scope_reference_ordinal": 5,
            "group_record_index": 30,
            "group_member_ordinal": 0,
            "record_index": 40,
            "byte_offset": 0,
            "class_tag": "276",
            "paired_byte_offset": 16,
            "paired_class_tag": "262",
            "recipe_record_index": 43,
            "recipe_record_byte_offset": 32,
            "recipe_id": "f3d:Design/BulkStream.dat:construction-recipe#43",
            "recipe_prefix_offset": 43,
            "recipe_prefix_bytes": "",
            "recipe_references": [],
            "recipe_kind": "face",
            "recipe_program_offset": 0,
            "recipe_program": [0, -1],
            "recipe_node_offsets": [],
            "recipe_nodes": [],
            "preceding_candidate_faces": [
                "f3d:brep:entity#10",
                "f3d:brep:entity#11"
            ],
            "next_record_index": 44,
            "next_byte_offset": 160
        }))
        .expect("face-recipe operand");
    let topology = AsmHistoricalTopology {
        faces: vec![10, 11],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 20,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 21,
            },
        ],
        surface_planes: vec![
            AsmHistoricalPlane {
                surface: 20,
                origin: Point3::new(1.0, 2.0, 3.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
            },
            AsmHistoricalPlane {
                surface: 21,
                origin: Point3::new(1.0, 2.0, 3.0),
                normal: Vector3::new(0.0, 0.0, -1.0),
            },
        ],
        ..Default::default()
    };
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![AsmDeltaState {
            id: "history:state#1".into(),
            parent: "history".into(),
            byte_offset: 0,
            state_id: 1,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 1,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            entity_versions: Vec::new(),
            topology_cache: crate::history_records::AsmTopologyCache::Complete(topology),
            transition: None,
        }],
    };

    let plane = historical_mirror_face_operand_plane(&operand, &history, 1)
        .expect("coincident preceding faces share one mirror plane");
    assert_eq!(plane.origin, Point3::new(1.0, 2.0, 3.0));
    assert_eq!(plane.normal, Vector3::new(0.0, 0.0, 1.0));

    let mut noncoincident = history;
    noncoincident.states[0]
        .topology_mut()
        .expect("topology")
        .surface_planes[1]
        .origin
        .z = 4.0;
    assert!(historical_mirror_face_operand_plane(&operand, &noncoincident, 1).is_none());
}

#[test]
fn mirror_coedge_plane_uses_unique_planar_face_in_radial_cycle() {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalPlane,
        AsmHistoricalRelation, AsmHistoricalTopology,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let topology = AsmHistoricalTopology {
        faces: vec![10, 20],
        loops: vec![11, 21],
        coedges: vec![30, 31],
        face_loops: vec![
            AsmHistoricalRelation {
                owner_ref: 10,
                member_refs: vec![11],
            },
            AsmHistoricalRelation {
                owner_ref: 20,
                member_refs: vec![21],
            },
        ],
        loop_coedges: vec![
            AsmHistoricalRelation {
                owner_ref: 11,
                member_refs: vec![30],
            },
            AsmHistoricalRelation {
                owner_ref: 21,
                member_refs: vec![31],
            },
        ],
        coedge_topology: vec![
            AsmHistoricalCoedge {
                coedge: 30,
                owner_loop: 11,
                edge: 40,
                next: 30,
                previous: 30,
                radial_next: 31,
            },
            AsmHistoricalCoedge {
                coedge: 31,
                owner_loop: 21,
                edge: 40,
                next: 31,
                previous: 31,
                radial_next: 30,
            },
        ],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 20,
                carrier: 200,
            },
        ],
        surface_planes: vec![AsmHistoricalPlane {
            surface: 200,
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }],
        ..Default::default()
    };

    let plane = crate::test_support::with_decode_context(|decode_ctx| {
        historical_mirror_coedge_plane(decode_ctx, 30, &topology)
    })
    .unwrap()
    .expect("radial coedge cycle has one exact planar face");
    assert_eq!(plane.origin, Point3::new(1.0, 2.0, 3.0));
    assert_eq!(plane.normal, Vector3::new(0.0, 0.0, 1.0));

    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![AsmDeltaState {
            id: "history:state#1".into(),
            parent: "history".into(),
            byte_offset: 0,
            state_id: 1,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 1,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            entity_versions: Vec::new(),
            topology_cache: crate::history_records::AsmTopologyCache::Complete(topology.clone()),
            transition: None,
        }],
    };
    let candidate =
        crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate {
            history_id: "history".into(),
            historical: crate::records::topology::fillet::HistoricalBinding {
                kind: AsmHistoricalEntityKind::Coedge,
                entity_ref: 30,
                state_ids: vec![1],
            },
            face_slot: 10,
        };
    let dispatched = crate::test_support::with_decode_context(|decode_ctx| {
        historical_mirror_plane(decode_ctx, &candidate, 1, std::slice::from_ref(&history))
    })
    .unwrap()
    .expect("coedge dispatch uses radial plane resolver");
    assert_eq!(dispatched.origin, Point3::new(1.0, 2.0, 3.0));
    assert_eq!(dispatched.normal, Vector3::new(0.0, 0.0, 1.0));

    let mut open_cycle = topology.clone();
    open_cycle.coedge_topology[1].radial_next = 31;
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| historical_mirror_coedge_plane(
            decode_ctx,
            30,
            &open_cycle
        ))
        .unwrap()
        .is_none()
    );

    let mut ambiguous = topology;
    ambiguous.surface_planes.push(AsmHistoricalPlane {
        surface: 100,
        origin: Point3::new(1.0, 2.0, 4.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
    });
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| historical_mirror_coedge_plane(
            decode_ctx, 30, &ambiguous
        ))
        .unwrap()
        .is_none()
    );
}
