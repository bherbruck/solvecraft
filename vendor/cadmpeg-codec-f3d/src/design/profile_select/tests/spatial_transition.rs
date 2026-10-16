// SPDX-License-Identifier: Apache-2.0
//! Transition profile selection over spatial sketches.

use super::{spatial_line, spatial_profile};
use crate::design::profile_select::{
    spatial_polyline_profile_containing_points, transition_spatial_profile_selection,
};
use crate::history_records::{
    AsmDeltaState, AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge,
    AsmHistoricalPoint, AsmHistoricalRelation, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
    AsmHistoricalTransition, AsmHistory,
};
use crate::ids::neutral_spatial_sketch_curve_id;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{
    SpatialSketch, SpatialSketchEntity, SpatialSketchEntityUse, SpatialSketchGeometry,
    SpatialSketchGeometryDefinition, SpatialSketchProfile,
};

#[test]
fn spatial_profile_polygon_points_refuse_collection_limit() {
    let sketch_id =
        cadmpeg_ir::sketches::SpatialSketchId::mint("synthetic:test:id#spatial-polygon-allocation")
            .unwrap();
    let entities = [
        spatial_line(
            &sketch_id,
            100,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ),
        spatial_line(
            &sketch_id,
            101,
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ),
        spatial_line(
            &sketch_id,
            102,
            Point3::new(0.0, 2.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ),
    ];
    let sketch = SpatialSketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        profiles: vec![spatial_profile(&sketch_id, &[100, 101, 102])],
        native_ref: None,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        spatial_polyline_profile_containing_points(
            &sketch, &entities, &[Point3::new(0.5, 0.5, 0.0)], 0.01, &ctx),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d spatial profile polygon point"
    ));
}

#[test]
fn spatial_transition_does_not_select_a_translated_equal_length_profile() {
    let sketch_id =
        cadmpeg_ir::sketches::SpatialSketchId::mint("f3d:model:spatial-sketch#transition").unwrap();
    let entities = [
        spatial_line(
            &sketch_id,
            100,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ),
        spatial_line(
            &sketch_id,
            101,
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ),
        spatial_line(
            &sketch_id,
            102,
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ),
        spatial_line(
            &sketch_id,
            200,
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(12.0, 0.0, 0.0),
        ),
        spatial_line(
            &sketch_id,
            201,
            Point3::new(12.0, 0.0, 0.0),
            Point3::new(10.0, 2.0, 0.0),
        ),
        spatial_line(
            &sketch_id,
            202,
            Point3::new(10.0, 2.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
        ),
    ];
    let sketch = SpatialSketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        profiles: vec![
            spatial_profile(&sketch_id, &[100, 101, 102]),
            spatial_profile(&sketch_id, &[200, 201, 202]),
        ],
        native_ref: None,
    };
    let topology = AsmHistoricalTopology {
        faces: vec![1],
        loops: vec![10],
        coedges: vec![20, 21, 22],
        edges: vec![30, 31, 32],
        vertices: vec![40, 41, 42],
        points: vec![50, 51, 52],
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 1,
            member_refs: vec![10],
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 10,
            member_refs: vec![20, 21, 22],
        }],
        coedge_topology: [(20, 30, 21, 22), (21, 31, 22, 20), (22, 32, 20, 21)]
            .into_iter()
            .map(|(coedge, edge, next, previous)| AsmHistoricalCoedge {
                coedge,
                owner_loop: 10,
                edge,
                next,
                previous,
                radial_next: coedge,
            })
            .collect(),
        edge_vertices: [(30, 40, 41), (31, 41, 42), (32, 42, 40)]
            .into_iter()
            .map(|(edge, start_vertex, end_vertex)| AsmHistoricalEdge {
                edge,
                start_vertex,
                end_vertex,
            })
            .collect(),
        vertex_points: [(40, 50), (41, 51), (42, 52)]
            .into_iter()
            .map(|(entity, carrier)| AsmHistoricalCarrierBinding { entity, carrier })
            .collect(),
        point_positions: [
            (50, Point3::new(100.0, 0.0, 0.0)),
            (51, Point3::new(101.0, 0.0, 0.0)),
            (52, Point3::new(100.0, 1.0, 0.0)),
        ]
        .into_iter()
        .map(|(point, position)| AsmHistoricalPoint { point, position })
        .collect(),
        ..Default::default()
    };
    let state = |state_id, topology, transition| AsmDeltaState {
        id: format!("history:state-{state_id}"),
        parent: "history".into(),
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
        topology_cache: crate::history_records::AsmTopologyCache::Complete(topology),
        transition,
    };
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![
            state(1, AsmHistoricalTopology::default(), None),
            state(
                2,
                topology,
                Some(AsmHistoricalTransition {
                    previous_state_id: Some(1),
                    records: crate::history_records::AsmHistoricalEntityDelta::default(),
                    topology: AsmHistoricalTopologyDelta {
                        faces: crate::history_records::AsmHistoricalEntityDelta {
                            inserted: vec![1],
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                }),
            ),
        ],
    };

    assert_eq!(
        crate::test_support::with_decode_context(
            |decode_ctx| transition_spatial_profile_selection(
                &sketch,
                &entities,
                &[history],
                2,
                1,
                1.0e-6,
                decode_ctx
            )
        )
        .unwrap(),
        None
    );
}

#[test]
fn spatial_transition_withholds_when_any_profile_boundary_is_nonlinear() {
    let sketch_id =
        cadmpeg_ir::sketches::SpatialSketchId::mint("f3d:model:spatial-sketch#nonlinear").unwrap();
    let mut entities = vec![
        spatial_line(
            &sketch_id,
            100,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ),
        spatial_line(
            &sketch_id,
            101,
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ),
        spatial_line(
            &sketch_id,
            102,
            Point3::new(0.0, 2.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ),
    ];
    let arc_id = neutral_spatial_sketch_curve_id(&sketch_id, 200, 0);
    entities.push(SpatialSketchEntity::new(
        arc_id.clone(),
        sketch_id.clone(),
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Arc {
            center: Point3::new(10.0, 10.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            reference_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: Length::new(1.0).unwrap(),
            start_angle: Angle::new(0.0).unwrap(),
            end_angle: Angle::new(std::f64::consts::PI).unwrap(),
        })
        .unwrap(),
    ));
    let sketch = SpatialSketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        profiles: vec![
            spatial_profile(&sketch_id, &[100, 101, 102]),
            SpatialSketchProfile::try_new(
                Point3::new(10.0, 10.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                vec![SpatialSketchEntityUse {
                    entity: arc_id,
                    reversed: false,
                }],
                &cadmpeg_test_support::service_decode_context(),
                "spatial profile uniqueness",
            )
            .expect("fixture collection admission")
            .unwrap(),
        ],
        native_ref: None,
    };
    let points = [
        Point3::new(0.25, 0.25, 0.0),
        Point3::new(0.5, 0.25, 0.0),
        Point3::new(0.25, 0.5, 0.0),
    ];

    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            spatial_polyline_profile_containing_points(
                &sketch, &entities, &points, 1.0e-6, decode_ctx,
            )
        })
        .unwrap(),
        None
    );
    let polyline_only = SpatialSketch {
        profiles: vec![sketch.profiles[0].clone()],
        ..sketch
    };
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            spatial_polyline_profile_containing_points(
                &polyline_only,
                &entities,
                &points,
                1.0e-6,
                decode_ctx,
            )
        })
        .unwrap(),
        Some(0)
    );
}
