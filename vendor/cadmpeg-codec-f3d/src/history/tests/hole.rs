// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::default_trait_access)]

use crate::history::selection::bind_hole_selection_history;
use crate::history_records::AsmHistoricalPlane;
use crate::history_records::{
    AsmDeltaState, AsmEntityVersion, AsmHistoricalCarrierBinding, AsmHistoricalCylinder,
    AsmHistoricalEntityDelta, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
    AsmHistoricalTransition, AsmHistory,
};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use cadmpeg_ir::math::{Point3, Vector3};

fn carrier(entity: i64, carrier: i64) -> AsmHistoricalCarrierBinding {
    AsmHistoricalCarrierBinding { entity, carrier }
}

fn plane(surface: i64, origin: Point3, normal: Vector3) -> AsmHistoricalPlane {
    AsmHistoricalPlane {
        surface,
        origin,
        normal,
    }
}

fn state(
    state_id: i64,
    topology: AsmHistoricalTopology,
    transition: Option<AsmHistoricalTransition>,
) -> AsmDeltaState {
    AsmDeltaState {
        id: format!("history-state-{state_id}"),
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
        entity_versions: vec![AsmEntityVersion {
            entity_ref: 7,
            record_ref: 7,
        }],
        topology_cache: crate::history_records::AsmTopologyCache::Complete(topology),
        transition,
    }
}

fn test_history() -> AsmHistory {
    let top_origin = Point3::new(0.0, 0.0, 10.0);
    let bottom_origin = Point3::new(0.0, 0.0, 0.0);
    let preceding = AsmHistoricalTopology {
        faces: vec![10, 20],
        edges: vec![7],
        surfaces: vec![100, 200],
        surface_planes: vec![
            plane(100, top_origin, Vector3::new(0.0, 0.0, 1.0)),
            plane(200, bottom_origin, Vector3::new(0.0, 0.0, 1.0)),
        ],
        face_surfaces: vec![carrier(10, 100), carrier(20, 200)],
        ..AsmHistoricalTopology::default()
    };
    let result = AsmHistoricalTopology {
        faces: vec![10, 20, 30],
        edges: vec![7, 70],
        surfaces: vec![100, 200, 300],
        surface_planes: vec![
            plane(100, top_origin, Vector3::new(0.0, 0.0, 1.0)),
            plane(200, bottom_origin, Vector3::new(0.0, 0.0, 1.0)),
        ],
        surface_cylinders: vec![AsmHistoricalCylinder {
            surface: 300,
            origin: Point3::new(0.0, 0.0, -1.0),
            axis: Vector3::new(0.0, 0.0, -1.0),
            radius: 5.0,
        }],
        face_surfaces: vec![carrier(10, 100), carrier(20, 200), carrier(30, 300)],
        ..AsmHistoricalTopology::default()
    };
    let mut topology_delta = AsmHistoricalTopologyDelta::default();
    topology_delta.faces.inserted = vec![30];
    topology_delta.faces.updated = vec![10, 20];
    topology_delta.edges.inserted = vec![70];
    topology_delta.surfaces.inserted = vec![300];
    AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![
            state(
                2,
                result,
                Some(AsmHistoricalTransition {
                    previous_state_id: Some(1),
                    records: AsmHistoricalEntityDelta::default(),
                    topology: topology_delta,
                }),
            ),
            state(1, preceding, None),
        ],
    }
}

fn hole_scope() -> crate::records::feature::scope::DesignParameterScope {
    let face_selection = crate::records::feature::hole::DesignHoleFaceSelection {
        record_index: 1,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("375".to_owned()).unwrap(),
        asset_id: "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA"
            .to_owned()
            .try_into()
            .unwrap(),
        asset_id_offset: 0,
        context_id: "BBBBBBBB-BBBB-4BBB-8BBB-BBBBBBBBBBBB"
            .to_owned()
            .try_into()
            .unwrap(),
        context_id_offset: 0,
        identity_record_index: 2,
        identity_record_offset: 0,
        primary_identity: 7,
        primary_identity_offset: 0,
        secondary: None,
        historical_face_candidates: Vec::new(),
        next_record_index: 3,
        next_byte_offset: 0,
    };
    let construction = crate::records::feature::hole::DesignHoleConstruction {
        point_record_index: 4,
        point_record_byte_offset: 0,
        position: crate::test_support::reals([0.0, 0.0, 0.0]),
        position_offset: 0,
        direction: crate::test_support::reals([0.0, 0.0, 1.0]),
        direction_offset: 0,
        point_parameters: crate::test_support::reals([0.0, 0.0]),
        point_parameter_offsets: [0, 0],
        reference_type: 13,
        reference_type_offset: 0,
        tangent_point_data: None,
        input_records: vec![crate::records::identity::Located {
            value: 1,
            offset: 0,
        }],
        face_selection: Some(face_selection),
    };
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:scope#5",
        crate::records::feature::scope::DesignFeatureKind::Hole,
        5,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
        *slot = Some(construction);
    }
    scope
}

#[test]
fn edge_backed_hole_selection_uses_the_oriented_updated_support_plane() {
    let history = test_history();
    let mut scope = hole_scope();

    crate::test_support::with_decode_context(|decode_ctx| {
        bind_hole_selection_history(decode_ctx, std::slice::from_mut(&mut scope), &[history])
    })
    .unwrap();

    assert_eq!(
        scope
            .hole_construction()
            .and_then(|construction| construction.face_selection.as_ref())
            .map(|selection| selection.historical_face_candidates.as_slice()),
        Some(
            &[
                crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate {
                    history_id: "history".into(),
                    historical: crate::records::topology::fillet::HistoricalBinding {
                        kind: AsmHistoricalEntityKind::Edge,
                        entity_ref: 7,
                        state_ids: vec![1],
                    },
                    face_slot: 20,
                }
            ][..]
        )
    );
}

#[test]
fn edge_backed_hole_selection_rejects_ambiguous_support_planes() {
    let mut history = test_history();
    history.states[1]
        .topology_mut()
        .expect("preceding topology")
        .surface_planes[0]
        .origin = Point3::new(0.0, 0.0, 0.0);
    history.states[1]
        .topology_mut()
        .expect("preceding topology")
        .surface_planes[1]
        .normal = Vector3::new(0.0, 0.0, 1.0);
    let mut scope = hole_scope();

    crate::test_support::with_decode_context(|decode_ctx| {
        bind_hole_selection_history(decode_ctx, std::slice::from_mut(&mut scope), &[history])
    })
    .unwrap();

    assert!(scope
        .hole_construction()
        .and_then(|construction| construction.face_selection.as_ref())
        .is_some_and(|selection| selection.historical_face_candidates.is_empty()));
}
