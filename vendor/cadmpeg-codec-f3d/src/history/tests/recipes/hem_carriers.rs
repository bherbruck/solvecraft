// SPDX-License-Identifier: Apache-2.0

#[test]
fn hem_bend_carriers_prove_directional_gap_forms() {
    use crate::history_records::AsmHistoricalCylinder;
    use cadmpeg_ir::math::{Point3, Vector3};

    let cylinder = |radius| AsmHistoricalCylinder {
        surface: 1,
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 1.0, 0.0),
        radius,
    };
    let flat_inner = cylinder(0.01);
    let flat_outer = cylinder(2.51);
    assert_eq!(
        super::super::super::hem_gap_length_form([&flat_inner, &flat_outer].into_iter()),
        Some(super::super::super::HemGapLengthForm::Flat)
    );

    let open_inner = cylinder(1.25);
    let open_outer = cylinder(3.75);
    assert_eq!(
        super::super::super::hem_gap_length_form([&open_inner, &open_outer].into_iter()),
        Some(super::super::super::HemGapLengthForm::Open)
    );

    assert_eq!(
        super::super::super::hem_gap_length_form([&flat_inner].into_iter()),
        None
    );
}

#[test]
fn hem_carrier_offsets_prove_fold_direction() {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCylinder,
        AsmHistoricalEntityDelta, AsmHistoricalPlane, AsmHistoricalRelation, AsmHistoricalTopology,
        AsmHistoricalTopologyDelta, AsmHistoricalTransition,
    };
    use cadmpeg_ir::features::SheetMetalHemDirection;
    use cadmpeg_ir::math::{Point3, Vector3};

    let previous = AsmHistoricalTopology {
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
            carrier: 11,
        }],
        surface_planes: vec![AsmHistoricalPlane {
            surface: 11,
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(1.0, 0.0, 0.0),
        }],
        ..Default::default()
    };
    let transition = AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: Default::default(),
        topology: AsmHistoricalTopologyDelta {
            surfaces: AsmHistoricalEntityDelta {
                inserted: vec![12, 13],
                ..Default::default()
            },
            ..Default::default()
        },
    };
    let cylinder = |origin| AsmHistoricalCylinder {
        surface: 12,
        origin,
        axis: Vector3::new(0.0, 1.0, 0.0),
        radius: 1.0,
    };
    let forward_first = cylinder(Point3::new(1.0, 0.0, 0.0));
    let forward_second = cylinder(Point3::new(2.0, 0.0, 0.0));
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            super::super::super::hem_direction_from_transition(
                decode_ctx,
                7,
                &[&forward_first, &forward_second].into_iter(),
                &previous,
                &transition,
            )
        })
        .unwrap(),
        Some(SheetMetalHemDirection::Forward)
    );

    let reverse_first = cylinder(Point3::new(-1.0, 0.0, 0.0));
    let reverse_second = cylinder(Point3::new(-2.0, 0.0, 0.0));
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            super::super::super::hem_direction_from_transition(
                decode_ctx,
                7,
                &[&reverse_first, &reverse_second].into_iter(),
                &previous,
                &transition,
            )
        })
        .unwrap(),
        Some(SheetMetalHemDirection::Reverse)
    );

    let zero_offset = cylinder(Point3::new(0.0, 0.0, 0.0));
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            super::super::super::hem_direction_from_transition(
                decode_ctx,
                7,
                &[&zero_offset, &forward_second].into_iter(),
                &previous,
                &transition,
            )
        })
        .unwrap(),
        None
    );
}
