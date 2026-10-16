// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::cloned_ref_to_slice_refs)]
use super::project_sketch_design;
use super::project_spatial_sketch_constraints;
use super::project_spatial_sketch_design;
use super::sketch_text_horizontal_alignment;
use super::sketch_text_vertical_alignment;
use crate::design::constraints::project_sketch_constraints;
use crate::design::dimensions::exact_atomic_constraint;
use crate::design::dimensions::point_lies_on_sketch_geometry;
use crate::design::geometry::point_on_sketch_entity;
use crate::design::geometry::sketch_entity_endpoints;
use crate::ids::neutral_sketch_curve_id;
use crate::records::sketch_geometry::SketchCurveGeometry;
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::sketch_geometry::SketchPoint;
use crate::records::sketch_geometry::SketchSurface;
use crate::records::sketch_geometry::SketchText;
use crate::records::sketch_placement::DesignSketchPlacement;
use crate::records::sketch_placement::DesignSketchVisibility;
use crate::records::sketch_relations::SketchConstraintKind;
use crate::records::sketch_relations::SketchRelation;
use crate::records::sketch_relations::SketchRelationMember;
use crate::records::sketch_relations::SketchRelationReturnMember;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::sketches::SketchConstraintDefinitionInput;
use cadmpeg_ir::sketches::SketchCoordinateAxis;
use cadmpeg_ir::sketches::SketchEntity;
use cadmpeg_ir::sketches::SketchEntityId;
use cadmpeg_ir::sketches::SketchGeometryDefinition;
use cadmpeg_ir::sketches::SketchTextHorizontalAlignment as Horizontal;
use cadmpeg_ir::sketches::SketchTextVerticalAlignment as Vertical;

const EPS_POINT_PROJECTION: f64 = 1.0e-6;

#[test]
fn sketch_text_alignment_ordinals_project_to_named_positions() {
    assert_eq!(
        sketch_text_horizontal_alignment(Some(1)),
        Some(Horizontal::Left)
    );
    assert_eq!(
        sketch_text_horizontal_alignment(Some(2)),
        Some(Horizontal::Right)
    );
    assert_eq!(
        sketch_text_horizontal_alignment(Some(3)),
        Some(Horizontal::Center)
    );
    assert_eq!(
        sketch_text_horizontal_alignment(Some(99)),
        Some(Horizontal::Native(99))
    );
    assert_eq!(sketch_text_vertical_alignment(Some(1)), Some(Vertical::Top));
    assert_eq!(
        sketch_text_vertical_alignment(Some(2)),
        Some(Vertical::Bottom)
    );
    assert_eq!(
        sketch_text_vertical_alignment(Some(3)),
        Some(Vertical::Middle)
    );
    assert_eq!(
        sketch_text_vertical_alignment(Some(99)),
        Some(Vertical::Native(99))
    );
    assert_eq!(sketch_text_horizontal_alignment(None), None);
    assert_eq!(sketch_text_vertical_alignment(None), None);
}

#[test]
fn sketch_container_visibility_projects_to_the_neutral_sketch() {
    let placement = DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::MemberCompact {
                paired_byte_offset: 34,
            },
        )
        .unwrap(),
        id: "f3d:design:design-sketch-placement#1".into(),
        scope_record_index: None,
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_201".to_owned())
            .expect("valid entity ID"),

        visibility: Some(
            DesignSketchVisibility::new(std::num::NonZeroU32::new(1).unwrap(), 30, false).unwrap(),
        ),

        class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        record_index: 1,

        paired_class_tag: crate::records::references::DesignClassTag::try_from("257".to_owned())
            .unwrap(),
    };

    let (sketches, entities) = crate::test_support::with_decode_context(|decode_ctx| {
        project_sketch_design(decode_ctx, &[placement], &[], &[], &[], &[], 1.0e-6)
    })
    .expect("sketch lanes pair");
    assert!(entities.is_empty());
    assert_eq!(sketches.len(), 1);
    assert_eq!(sketches[0].visible, Some(false));
}

#[test]
fn text_frame_curves_are_construction_geometry_not_profiles() {
    let placement = DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),
        id: "f3d:BulkStream.dat:placement#0".into(),
        scope_record_index: None,
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_42".to_owned())
            .expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        record_index: 1,

        paired_class_tag: crate::records::references::DesignClassTag::try_from("257".to_owned())
            .unwrap(),
    };
    let curve =
        |record_index, primary_id, start: (f64, f64), end: (f64, f64)| SketchCurveIdentity {
            id: format!("f3d:BulkStream.dat:curve#{record_index}"),
            record_index,
            owner_reference: Some(42),
            class_tag: crate::records::references::DesignClassTag::try_from("375".to_owned())
                .unwrap(),
            byte_offset: u64::from(record_index),
            geometry_offset: 0,
            entity_genesis: Some(0),
            primary_id: std::num::NonZeroU64::new(primary_id).unwrap(),
            secondary_id: 0,
            geometry: Some(
                SketchCurveGeometry::line(
                    Point3::new(start.0, start.1, 0.0),
                    Point3::new(end.0, end.1, 0.0),
                    Vector3::new(end.0 - start.0, end.1 - start.1, 0.0)
                        .unit()
                        .unwrap(),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
        };
    let curves = vec![
        curve(10, 10, (0.0, 0.0), (10.0, 0.0)),
        curve(11, 11, (10.0, 0.0), (10.0, 10.0)),
        curve(12, 12, (10.0, 10.0), (0.0, 10.0)),
        curve(13, 13, (0.0, 10.0), (0.0, 0.0)),
    ];
    let point = SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
        id: "f3d:BulkStream.dat:point#14".into(),
        record_index: 14,
        owner_reference: Some(42),
        class_tag: crate::records::references::DesignClassTag::try_from("413".to_owned()).unwrap(),
        byte_offset: 14,
        coordinate_offset: 0,
        companion: crate::records::sketch_geometry::SketchPointCompanion {
            incident_curves: Vec::new(),
        },
        record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
            14,
            crate::records::sketch_geometry::SketchPointClosure::Selector4State0,
            None,
            0.0,
        ),
        paired_reference: 15,
        coordinates: Point2::new(0.0, 0.0),
    })
    .unwrap();
    let text = SketchText {
        id: "f3d:BulkStream.dat:text#20".into(),
        record_index: 20,
        owner_reference: 42,
        class_tag: crate::records::references::DesignClassTag::try_from("376".to_owned()).unwrap(),
        class_version: 0,
        byte_offset: 20,
        entity_genesis: Some(0),
        persistent_id: Some(20),
        base_id: None,
        text: "A".into(),
        font_family: "Arial".into(),
        font_weight: 400,
        height: cadmpeg_ir::scalar::PositiveLength::new(10.0).unwrap(),
        color: cadmpeg_ir::topology::Color::new(0.0, 0.0, 0.0, 1.0).expect("valid color"),
        layout: crate::records::sketch_geometry::SketchTextLayout::TextexTag {
            width_factor: cadmpeg_ir::scalar::NonNegativeReal::new(1.0).unwrap(),
            alignment: Some(crate::records::sketch_geometry::SketchTextAlignment {
                horizontal: 1,
                vertical: 1,
            }),
            first_reference: None,
            second_reference: None,
            placement: Some(cadmpeg_ir::sketches::TextPlacement {
                anchor: cadmpeg_ir::units::FinitePoint2::new(Point2::new(0.0, 0.0)).unwrap(),
                rotation: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
            }),
        },
        raw_bytes: Vec::new().into(),
    };
    let relation = SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
        id: "f3d:BulkStream.dat:relation#30".into(),
        record_index: 30,
        class_tag: crate::records::references::DesignClassTag::try_from("377".to_owned()).unwrap(),
        byte_offset: 30,
        state_offset: 0,
        owner_reference: 42,
        owner_entity_id: Some(cadmpeg_core::text::NonBlankString::new("Sketch_42").unwrap()),
        auxiliary_references: crate::records::identity::ReferenceRun::located(
            vec![20]
                .into_iter()
                .map(|value| crate::records::identity::Located { value, offset: 0 })
                .collect(),
        ),
        rectangular_counted_reference_count: None,
        members: (vec![
            SketchRelationMember::from_index(20),
            SketchRelationMember::from_index(10),
            SketchRelationMember::from_index(11),
            SketchRelationMember::from_index(12),
            SketchRelationMember::from_index(13),
        ])
        .try_into()
        .expect("uniform member resolution"),
        owner_reference_offset: 0,
        definition: crate::records::sketch_relations::SketchRelationDefinition::new(
            0x100_0000_0000,
            Some(
                crate::records::sketch_relations::SketchPatternDefinition::TextFrame {
                    text_reference: 20,
                },
            ),
        )
        .expect("valid relation definition"),
        entity_genesis: Some(0),
        return_members: (vec![
            SketchRelationReturnMember::from_index(10),
            SketchRelationReturnMember::from_index(11),
            SketchRelationReturnMember::from_index(12),
            SketchRelationReturnMember::from_index(13),
        ])
        .try_into()
        .expect("uniform member resolution"),
        raw_bytes: vec![0; 160],
    })
    .unwrap();

    let (sketches, entities) = crate::test_support::with_decode_context(|decode_ctx| {
        project_sketch_design(
            decode_ctx,
            &[placement],
            &[point],
            &curves,
            &[relation],
            &[text],
            EPS_POINT_PROJECTION,
        )
    })
    .expect("sketch lanes pair");
    assert_eq!(sketches.len(), 1);
    assert!(sketches[0].profiles.is_empty());
    assert_eq!(entities.len(), 6);
    assert!(entities
        .iter()
        .filter(|entity| entity
            .native_ref
            .as_deref()
            .is_some_and(|id| id.contains(":curve#")))
        .all(|entity| entity.construction));
    assert!(entities.iter().any(|entity| matches!(
        *entity.geometry.definition(),
        SketchGeometryDefinition::Text { .. }
    ) && !entity.construction));
    assert!(entities.iter().any(|entity| {
        entity.native_ref.as_deref() == Some("f3d:BulkStream.dat:point#14") && !entity.construction
    }));
}

#[test]
fn point_closure_does_not_mark_construction_geometry() {
    let placement = DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),
        id: "f3d:BulkStream.dat:placement#0".into(),
        scope_record_index: None,
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_42".to_owned())
            .expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        record_index: 1,

        paired_class_tag: crate::records::references::DesignClassTag::try_from("257".to_owned())
            .unwrap(),
    };
    let point = SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
        id: "f3d:BulkStream.dat:point#10".into(),
        record_index: 10,
        owner_reference: Some(42),
        class_tag: crate::records::references::DesignClassTag::try_from("413".to_owned()).unwrap(),
        byte_offset: 10,
        coordinate_offset: 0,
        companion: crate::records::sketch_geometry::SketchPointCompanion {
            incident_curves: Vec::new(),
        },
        record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
            10,
            crate::records::sketch_geometry::SketchPointClosure::Selector4State0,
            None,
            0.0,
        ),
        paired_reference: 11,
        coordinates: Point2::new(0.0, 0.0),
    })
    .unwrap();
    let standalone_point =
        SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
            id: "f3d:BulkStream.dat:point#11".into(),
            record_index: 11,
            owner_reference: Some(42),
            class_tag: crate::records::references::DesignClassTag::try_from("413".to_owned())
                .unwrap(),
            byte_offset: 11,
            coordinate_offset: 0,
            companion: crate::records::sketch_geometry::SketchPointCompanion {
                incident_curves: Vec::new(),
            },
            record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
                11,
                crate::records::sketch_geometry::SketchPointClosure::Selector2State1,
                None,
                0.0,
            ),
            paired_reference: 12,
            coordinates: Point2::new(2.0, 0.0),
        })
        .unwrap();
    let curve = SketchCurveIdentity {
        id: "f3d:BulkStream.dat:curve#20".into(),
        record_index: 20,
        owner_reference: Some(42),
        class_tag: crate::records::references::DesignClassTag::try_from("375".to_owned()).unwrap(),
        byte_offset: 20,
        geometry_offset: 0,
        entity_genesis: Some(0),
        primary_id: std::num::NonZeroU64::new(20).unwrap(),
        secondary_id: 0,
        geometry: Some(
            SketchCurveGeometry::line(
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(10.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        ),
    };

    let (sketches, entities) = crate::test_support::with_decode_context(|decode_ctx| {
        project_sketch_design(
            decode_ctx,
            &[placement],
            &[point, standalone_point],
            &[curve],
            &[],
            &[],
            EPS_POINT_PROJECTION,
        )
    })
    .expect("sketch lanes pair");
    assert_eq!(sketches.len(), 1);
    assert!(sketches[0].profiles.is_empty());
    assert_eq!(entities.len(), 3);
    assert!(entities.iter().all(|entity| !entity.construction));
}

#[test]
fn placed_sketch_projects_signed_normal_and_nonclamped_curves() {
    let placement = DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            100,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeExplicit(
                crate::records::sketch_placement::SketchPlacementMatrix::try_from([
                    [0.0, 0.0, 1.0, 10.0],
                    [1.0, 0.0, 0.0, 20.0],
                    [0.0, 1.0, 0.0, 30.0],
                    [0.0, 0.0, 0.0, 1.0],
                ])
                .unwrap(),
            ),
        )
        .unwrap(),

        id: "f3d:native:placement#0".into(),
        scope_record_index: Some(177),
        entity_id: crate::records::identity::DesignEntityId::try_from("0_172".to_owned())
            .expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::references::DesignClassTag::try_from("356".to_owned()).unwrap(),
        record_index: 185,

        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .unwrap(),
    };
    let point = SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
        id: "f3d:native:point#175".into(),
        record_index: 175,
        owner_reference: Some(172),
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        byte_offset: 400,
        coordinate_offset: 89,
        companion: crate::records::sketch_geometry::SketchPointCompanion {
            incident_curves: Vec::new(),
        },
        record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
            10,
            crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
            None,
            0.0,
        ),
        paired_reference: 0,
        coordinates: Point2::new(2.5, 4.0),
    })
    .unwrap();
    let line = SketchCurveIdentity {
        id: "f3d:native:curve#217".into(),
        record_index: 217,
        owner_reference: Some(172),
        class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned()).unwrap(),
        byte_offset: 500,
        geometry_offset: 100,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(20).unwrap(),
        secondary_id: 0,
        geometry: Some(
            SketchCurveGeometry::line(
                Point3::new(1.0, 2.0, 0.0),
                Point3::new(4.0, 6.0, 0.0),
                Vector3::new(0.6, 0.8, 0.0),
                Vector3::new(0.0, 0.0, -1.0),
            )
            .unwrap(),
        ),
    };
    let clockwise_arc = SketchCurveIdentity {
        id: "f3d:native:curve#220".into(),
        record_index: 220,
        owner_reference: Some(172),
        class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned()).unwrap(),
        byte_offset: 800,
        geometry_offset: 100,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(22).unwrap(),
        secondary_id: 0,
        geometry: Some(
            SketchCurveGeometry::arc(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, -1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
                0.0,
                std::f64::consts::FRAC_PI_2,
            )
            .unwrap(),
        ),
    };
    let nonclamped_nurbs = SketchCurveIdentity {
        id: "f3d:native:curve#218".into(),
        record_index: 218,
        owner_reference: Some(172),
        class_tag: crate::records::references::DesignClassTag::try_from("303".to_owned()).unwrap(),
        byte_offset: 700,
        geometry_offset: 100,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(21).unwrap(),
        secondary_id: 0,
        geometry: Some(SketchCurveGeometry::nurbs_from_parts(
            None,
            crate::records::references::DesignClassTag::try_from("304".to_owned()).unwrap(),
            219,
            crate::records::sketch_geometry::SketchNurbsGeometry::from_parts(
                2,
                1.0e-6,
                8,
                vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
                crate::records::sketch_geometry::SketchNurbsPoles::from_wire(
                    vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(2.0, 0.0, 0.0),
                        Point3::new(2.0, 2.0, 0.0),
                        Point3::new(4.0, 2.0, 0.0),
                    ],
                    vec![],
                )
                .unwrap(),
            )
            .unwrap(),
        )),
    };

    let placements = vec![placement];
    let points = vec![point];
    let curves = vec![line, nonclamped_nurbs, clockwise_arc];
    let (sketches, entities) = crate::test_support::with_decode_context(|decode_ctx| {
        project_sketch_design(decode_ctx, &placements, &points, &curves, &[], &[], 1.0e-6)
    })
    .expect("sketch lanes pair");
    assert_eq!(sketches.len(), 1);
    assert_eq!(
        sketches[0]
            .resolved_placement()
            .map(|(origin, normal, u_axis)| (origin.get(), normal.get(), u_axis.get())),
        Some((
            Point3::new(10.0, 20.0, 30.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ))
    );
    assert_eq!(entities.len(), 4);
    assert!(entities
        .iter()
        .any(|entity| matches!(*entity.geometry.definition(),
            SketchGeometryDefinition::Point { position } if position == Point2::new(2.5, 4.0)
        )));
    assert!(entities
        .iter()
        .any(|entity| matches!(*entity.geometry.definition(),
            SketchGeometryDefinition::Line { start, end }
                if start == Point2::new(1.0, 2.0) && end == Point2::new(4.0, 6.0)
        )));
    assert!(entities
        .iter()
        .any(|entity| matches!(*entity.geometry.definition(),
            SketchGeometryDefinition::Arc { start_angle, end_angle, .. }
                if start_angle.get() == 0.0
                    && end_angle.get() == -std::f64::consts::FRAC_PI_2
        )));
    let nurbs = entities
        .iter()
        .find(|entity| entity.native_ref.as_deref() == Some("f3d:native:curve#218"))
        .expect("non-clamped NURBS projects");
    let endpoints = crate::test_support::with_decode_context(|decode_ctx| {
        sketch_entity_endpoints(nurbs, decode_ctx)
    })
    .unwrap()
    .expect("non-clamped NURBS endpoints");
    assert_eq!(endpoints, [Point2::new(1.0, 0.0), Point2::new(3.0, 2.0)]);
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| point_on_sketch_entity(
            decode_ctx,
            Point2::new(2.0, 1.0),
            nurbs,
            EPS_CONTAINMENT_DISTANCE
        ))
        .expect("resource allocation did not fail")
    );
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| point_lies_on_sketch_geometry(
            decode_ctx,
            Point2::new(2.0, 1.0),
            &nurbs.geometry
        ))
        .expect("resource allocation did not fail")
    );

    let relation = |record_index, member| {
        SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
            id: format!("f3d:native:relation#{record_index}"),
            record_index,
            class_tag: crate::records::references::DesignClassTag::try_from("302".to_owned())
                .unwrap(),
            byte_offset: 600,
            state_offset: 70,
            owner_reference: 172,
            owner_entity_id: Some(cadmpeg_core::text::NonBlankString::new("0_172").unwrap()),
            auxiliary_references: crate::records::identity::ReferenceRun::located(Vec::new()),
            rectangular_counted_reference_count: None,
            members: (vec![member]
                .into_iter()
                .map(SketchRelationMember::from_index)
                .collect::<Vec<_>>())
            .try_into()
            .expect("uniform member resolution"),
            owner_reference_offset: 55,
            definition: crate::records::sketch_relations::SketchRelationDefinition::new(0x40, None)
                .expect("valid relation definition"),
            entity_genesis: None,
            return_members: (vec![member]
                .into_iter()
                .map(SketchRelationReturnMember::from_index)
                .collect::<Vec<_>>())
            .try_into()
            .expect("uniform member resolution"),
            raw_bytes: vec![0; 160],
        })
        .unwrap()
    };
    let mut curve_point_coincidence = relation(702, 217);
    let mut members = curve_point_coincidence.members().to_vec();
    members.push(SketchRelationMember {
        reference: crate::records::sketch_relations::SketchRelationReference::Index(175),
        offset: 40,
        relation_ordinal: None,
    });
    curve_point_coincidence
        .try_edit(|draft| draft.members = members.try_into().expect("uniform member resolution"))
        .unwrap();
    let mut returned = curve_point_coincidence.return_members().to_vec();
    returned.push(SketchRelationReturnMember::from_index(175));
    curve_point_coincidence
        .try_edit(|draft| {
            draft.return_members = returned.try_into().expect("uniform member resolution");
        })
        .unwrap();
    curve_point_coincidence.definition =
        crate::records::sketch_relations::SketchRelationDefinition::new(
            1,
            curve_point_coincidence.definition.pattern().cloned(),
        )
        .expect("valid relation definition");
    let mut midpoint = curve_point_coincidence.clone();
    midpoint.record_index = 703;
    midpoint.id = "f3d:native:relation#703".into();
    midpoint.definition = crate::records::sketch_relations::SketchRelationDefinition::new(
        0x1000,
        midpoint.definition.pattern().cloned(),
    )
    .expect("valid relation definition");
    let mut curvature = curve_point_coincidence.clone();
    curvature.record_index = 704;
    curvature.id = "f3d:native:relation#704".into();
    curvature.definition = crate::records::sketch_relations::SketchRelationDefinition::new(
        0x200,
        curvature.definition.pattern().cloned(),
    )
    .expect("valid relation definition");
    let mut spline_group = relation(705, 218);
    // Reverse the first run so only the specified semantic run can satisfy the
    // assertion below.
    spline_group
        .try_edit(|draft| {
            draft.members = ([(218, 25), (217, 40)]
                .into_iter()
                .map(|(record_index, offset)| {
                    crate::records::sketch_relations::SketchRelationMember {
                        reference: crate::records::sketch_relations::SketchRelationReference::Index(
                            record_index,
                        ),
                        offset,
                        relation_ordinal: Some(0),
                    }
                })
                .collect::<Vec<_>>())
            .try_into()
            .expect("uniform member resolution");
        })
        .unwrap();
    spline_group
        .try_edit(|draft| {
            draft.return_members = ([(217, 80), (218, 95)]
                .into_iter()
                .map(|(record_index, offset)| {
                    crate::records::sketch_relations::SketchRelationReturnMember {
                        reference: crate::records::sketch_relations::SketchRelationReference::Index(
                            record_index,
                        ),
                        offset,
                    }
                })
                .collect::<Vec<_>>())
            .try_into()
            .expect("uniform member resolution");
        })
        .unwrap();
    spline_group.definition = crate::records::sketch_relations::SketchRelationDefinition::new(
        0x8000_0000,
        spline_group.definition.pattern().cloned(),
    )
    .expect("valid relation definition");
    let mut horizontal_point = relation(701, 175);
    horizontal_point
        .try_edit(|draft| {
            draft.auxiliary_references = crate::records::identity::ReferenceRun::located(vec![
                crate::records::identity::Located {
                    value: 999,
                    offset: 0,
                },
            ]);
        })
        .unwrap();
    horizontal_point
        .try_edit(|draft| {
            draft.return_members = (vec![
                SketchRelationReturnMember::from_index(175),
                SketchRelationReturnMember::from_index(175),
            ])
            .try_into()
            .expect("uniform member resolution");
        })
        .unwrap();
    horizontal_point.definition = crate::records::sketch_relations::SketchRelationDefinition::new(
        0x1_0000_0040,
        horizontal_point.definition.pattern().cloned(),
    )
    .expect("valid relation definition");
    let constraints = crate::test_support::with_decode_context(|decode_ctx| {
        project_sketch_constraints(
            decode_ctx,
            &placements,
            &[],
            (&points, &curves, &[]),
            &[
                relation(700, 217),
                horizontal_point,
                curve_point_coincidence,
                midpoint,
                curvature,
                spline_group,
            ],
            &entities,
        )
    })
    .unwrap();
    assert!(matches!(
        constraints[0].definition.kind(),
        SketchConstraintDefinitionInput::Horizontal { .. }
    ));
    assert!(matches!(
        constraints[1].definition.kind(),
        SketchConstraintDefinitionInput::Native {
            ref native_kind,
            native_state: Some(0x1_0000_0040),
            native_flags: None,
            ref entities,
            ref operands,
            ..
        } if native_kind == "horizontal+unknown_bits"
            && entities.len() == 3
            && entities.iter().all(|entity| entity == &entities[0])
                    && operands.iter().map(|operand| (operand.field.as_ref().map(|field| field.name.as_str()), operand.native_kind.as_str(), operand.object_index)).collect::<Vec<_>>()
                == [
                    (Some("member"), "point", Some(175)),
                    (Some("auxiliary"), "record", Some(999)),
                    (Some("return"), "point", Some(175)),
                    (Some("return"), "point", Some(175)),
                ]
    ));
    assert!(matches!(
        constraints[2].definition.kind(),
        SketchConstraintDefinitionInput::Coincident { ref entities } if entities.len() == 2
    ));
    assert!(matches!(
        constraints[3].definition.kind(),
        SketchConstraintDefinitionInput::Midpoint { .. }
    ));
    assert!(matches!(
        constraints[4].definition.kind(),
        SketchConstraintDefinitionInput::Native {
            ref native_kind,
            ref entities,
            ..
        } if native_kind == "curvature" && entities.len() == 4
    ));
    assert!(matches!(
        constraints[5].definition.kind(),
        SketchConstraintDefinitionInput::SplineGroup { ref entities }
            if entities == &[
                neutral_sketch_curve_id(&sketches[0].id, 20, 0),
                neutral_sketch_curve_id(&sketches[0].id, 21, 0),
            ]
    ));
    let line = entities
        .iter()
        .find(|entity| {
            matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            )
        })
        .unwrap();
    let point = entities
        .iter()
        .find(|entity| {
            matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Point { .. }
            )
        })
        .unwrap();
    let other_point = SketchEntity::new(
        SketchEntityId::mint("generated:test:point#other").unwrap(),
        point.sketch.clone(),
        point.geometry.clone(),
    )
    .with_construction(point.construction)
    .with_native_ref(point.native_ref.clone())
    .with_geometry_ref(point.geometry_ref.clone())
    .with_endpoint_refs(point.endpoint_refs.clone());
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| exact_atomic_constraint(SketchConstraintKind::Horizontal, &[point, &other_point], decode_ctx)).unwrap(),
        Some(SketchConstraintDefinitionInput::SameCoordinate { relation }) if relation.axis() == SketchCoordinateAxis::V
    ));
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| exact_atomic_constraint(SketchConstraintKind::Vertical, &[point, &other_point], decode_ctx)).unwrap(),
        Some(SketchConstraintDefinitionInput::SameCoordinate { relation }) if relation.axis() == SketchCoordinateAxis::U
    ));
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| exact_atomic_constraint(
            SketchConstraintKind::Horizontal,
            &[point, point],
            decode_ctx
        ))
        .unwrap()
        .is_none()
    );
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| exact_atomic_constraint(
            SketchConstraintKind::Midpoint,
            &[line, point],
            decode_ctx
        ))
        .unwrap(),
        Some(SketchConstraintDefinitionInput::Midpoint { .. })
    ));
    for kind in [
        SketchConstraintKind::Tangent,
        SketchConstraintKind::Curvature,
        SketchConstraintKind::Equal,
    ] {
        assert!(
            crate::test_support::with_decode_context(|decode_ctx| exact_atomic_constraint(
                kind,
                &[line, point],
                decode_ctx
            ))
            .unwrap()
            .is_none()
        );
    }
    let other_line = SketchEntity::new(
        SketchEntityId::mint("generated:test:line#other").unwrap(),
        line.sketch.clone(),
        line.geometry.clone(),
    )
    .with_construction(line.construction)
    .with_native_ref(line.native_ref.clone())
    .with_geometry_ref(line.geometry_ref.clone())
    .with_endpoint_refs(line.endpoint_refs.clone());
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| exact_atomic_constraint(
            SketchConstraintKind::Tangent,
            &[line, &other_line],
            decode_ctx
        ))
        .unwrap(),
        Some(SketchConstraintDefinitionInput::Tangent { .. })
    ));
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| exact_atomic_constraint(
            SketchConstraintKind::Curvature,
            &[line, &other_line],
            decode_ctx
        ))
        .unwrap(),
        Some(SketchConstraintDefinitionInput::Curvature { .. })
    ));
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| exact_atomic_constraint(
            SketchConstraintKind::Equal,
            &[line, &other_line],
            decode_ctx
        ))
        .unwrap(),
        Some(SketchConstraintDefinitionInput::Equal { .. })
    ));
    for kind in [
        SketchConstraintKind::Colinear,
        SketchConstraintKind::EqualLength,
        SketchConstraintKind::Parallel,
        SketchConstraintKind::Perpendicular,
        SketchConstraintKind::Tangent,
        SketchConstraintKind::Curvature,
        SketchConstraintKind::Equal,
    ] {
        assert!(
            crate::test_support::with_decode_context(|decode_ctx| exact_atomic_constraint(
                kind,
                &[line, line],
                decode_ctx
            ))
            .unwrap()
            .is_none()
        );
    }
}

#[test]
fn nonplanar_sketch_curves_project_in_model_space() {
    use cadmpeg_ir::sketches::SpatialSketchGeometryDefinition;

    let placement = DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            100,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeExplicit(
                crate::records::sketch_placement::SketchPlacementMatrix::try_from([
                    [0.0, 0.0, 1.0, 10.0],
                    [1.0, 0.0, 0.0, 20.0],
                    [0.0, 1.0, 0.0, 30.0],
                    [0.0, 0.0, 0.0, 1.0],
                ])
                .unwrap(),
            ),
        )
        .unwrap(),

        id: "f3d:Design/BulkStream.dat:placement#100".into(),
        scope_record_index: None,
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_42".to_owned())
            .expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        record_index: 100,

        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .unwrap(),
    };
    let curve = |record_index, primary_id, geometry| SketchCurveIdentity {
        id: format!("f3d:Design/BulkStream.dat:curve#{record_index}"),
        record_index,
        owner_reference: Some(42),
        class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned()).unwrap(),
        byte_offset: u64::from(record_index),
        geometry_offset: 100,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(primary_id).unwrap(),
        secondary_id: 0,
        geometry: Some(geometry),
    };
    let mut curves = vec![
        curve(
            101,
            1,
            SketchCurveGeometry::line(
                Point3::new(1.0, 2.0, 3.0),
                Point3::new(4.0, 5.0, 6.0),
                Vector3::new(1.0, 1.0, 1.0).unit().unwrap(),
                Vector3::new(1.0, -1.0, 0.0).unit().unwrap(),
            )
            .unwrap(),
        ),
        curve(
            102,
            2,
            SketchCurveGeometry::arc(
                Point3::new(1.0, 2.0, 3.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                2.0,
                0.0,
                std::f64::consts::TAU,
            )
            .unwrap(),
        ),
        curve(
            108,
            7,
            SketchCurveGeometry::line(
                Point3::new(1.0, 2.0, 0.0),
                Point3::new(4.0, 2.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        ),
    ];
    curves.push(SketchCurveIdentity {
        id: "f3d:Design/BulkStream.dat:curve#103".into(),
        record_index: 103,
        owner_reference: Some(42),
        class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned()).unwrap(),
        byte_offset: 103,
        geometry_offset: 100,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(3).unwrap(),
        secondary_id: 0,
        geometry: None,
    });
    curves.push(curve(
        104,
        4,
        SketchCurveGeometry::nurbs_from_parts(
            None,
            crate::records::references::DesignClassTag::try_from("302".to_owned()).unwrap(),
            104,
            crate::records::sketch_geometry::SketchNurbsGeometry::from_parts(
                1,
                1.0e-8,
                8,
                vec![0.0, 0.0, 1.0, 1.0],
                crate::records::sketch_geometry::SketchNurbsPoles::from_wire(
                    vec![Point3::new(2.0, 3.0, 4.0), Point3::new(5.0, 6.0, 7.0)],
                    vec![1.0, 1.0],
                )
                .unwrap(),
            )
            .unwrap(),
        ),
    ));
    let relation = SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
        id: "f3d:Design/BulkStream.dat:relation#105".into(),
        record_index: 105,
        class_tag: crate::records::references::DesignClassTag::try_from("303".to_owned()).unwrap(),
        byte_offset: 105,
        state_offset: 0,
        owner_reference: 42,
        owner_entity_id: Some(cadmpeg_core::text::NonBlankString::new("Sketch_42").unwrap()),
        auxiliary_references: crate::records::identity::ReferenceRun::located(Vec::new()),
        rectangular_counted_reference_count: None,
        // Member run order disagrees with semantic order below.
        members: (vec![
            SketchRelationMember::from_index(104),
            SketchRelationMember::from_index(103),
        ])
        .try_into()
        .expect("uniform member resolution"),
        owner_reference_offset: 0,
        definition: crate::records::sketch_relations::SketchRelationDefinition::new(
            0x8000_0000,
            None,
        )
        .expect("valid relation definition"),
        entity_genesis: None,
        return_members: (vec![
            SketchRelationReturnMember::from_index(103),
            SketchRelationReturnMember::from_index(104),
        ])
        .try_into()
        .expect("uniform member resolution"),
        raw_bytes: vec![0; 160],
    })
    .unwrap();
    let point = SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
        id: "f3d:Design/BulkStream.dat:point#106".into(),
        record_index: 106,
        owner_reference: Some(42),
        class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned()).unwrap(),
        byte_offset: 106,
        coordinate_offset: 0,
        companion: crate::records::sketch_geometry::SketchPointCompanion {
            incident_curves: Vec::new(),
        },
        record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
            5,
            crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
            None,
            4.5,
        ),
        paired_reference: 0,
        coordinates: Point2::new(2.5, 3.5),
    })
    .unwrap();
    let mut midpoint_relation = relation.clone();
    midpoint_relation.id = "f3d:Design/BulkStream.dat:relation#106".into();
    midpoint_relation.record_index = 106;
    midpoint_relation.definition = crate::records::sketch_relations::SketchRelationDefinition::new(
        0x1000,
        midpoint_relation.definition.pattern().cloned(),
    )
    .expect("valid relation definition");
    midpoint_relation
        .try_edit(|draft| {
            draft.members = (vec![106, 101]
                .into_iter()
                .map(SketchRelationMember::from_index)
                .collect::<Vec<_>>())
            .try_into()
            .expect("uniform member resolution");
        })
        .unwrap();
    midpoint_relation
        .try_edit(|draft| {
            draft.return_members = (vec![101, 106]
                .into_iter()
                .map(SketchRelationReturnMember::from_index)
                .collect::<Vec<_>>())
            .try_into()
            .expect("uniform member resolution");
        })
        .unwrap();
    let mut coincident_point = point.clone();
    coincident_point.id = "f3d:Design/BulkStream.dat:point#107".into();
    coincident_point.record_index = 107;
    coincident_point.byte_offset = 107;
    let mut form = coincident_point.record_form().clone().into_raw();
    let crate::records::sketch_geometry::SketchPointRecordForm::Version11 { persistent_id, .. } =
        &mut form
    else {
        panic!("point fixture has a version-11 record form");
    };
    *persistent_id = std::num::NonZeroU64::new(6).unwrap();
    coincident_point.try_set_record_form(form).unwrap();
    let mut coincident_relation = relation.clone();
    coincident_relation.id = "f3d:Design/BulkStream.dat:relation#107".into();
    coincident_relation.record_index = 107;
    coincident_relation.definition =
        crate::records::sketch_relations::SketchRelationDefinition::new(
            1,
            coincident_relation.definition.pattern().cloned(),
        )
        .expect("valid relation definition");
    coincident_relation
        .try_edit(|draft| {
            draft.members = (vec![106, 107]
                .into_iter()
                .map(SketchRelationMember::from_index)
                .collect::<Vec<_>>())
            .try_into()
            .expect("uniform member resolution");
        })
        .unwrap();
    coincident_relation
        .try_edit(|draft| {
            draft.return_members = (vec![106, 107]
                .into_iter()
                .map(SketchRelationReturnMember::from_index)
                .collect::<Vec<_>>())
            .try_into()
            .expect("uniform member resolution");
        })
        .unwrap();
    let mut horizontal_relation = relation.clone();
    horizontal_relation.id = "f3d:Design/BulkStream.dat:relation#108".into();
    horizontal_relation.record_index = 108;
    horizontal_relation.definition =
        crate::records::sketch_relations::SketchRelationDefinition::new(
            0x40,
            horizontal_relation.definition.pattern().cloned(),
        )
        .expect("valid relation definition");
    horizontal_relation
        .try_edit(|draft| {
            draft.members = (vec![SketchRelationMember::from_index(108)])
                .try_into()
                .expect("uniform member resolution");
        })
        .unwrap();
    horizontal_relation
        .try_edit(|draft| {
            draft.return_members = (vec![SketchRelationReturnMember::from_index(108)])
                .try_into()
                .expect("uniform member resolution");
        })
        .unwrap();
    let surface = SketchSurface {
        id: "f3d:Design/BulkStream.dat:surface#109".into(),
        record_index: 109,
        owner_reference: Some(42),
        class_tag: crate::records::references::DesignClassTag::try_from("306".to_owned()).unwrap(),
        byte_offset: 109,
        entity_genesis: None,
        persistent_id: std::num::NonZeroU64::new(8).unwrap(),
        geometry: crate::records::sketch_geometry::SketchSurfaceGeometry::from_parts(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                vec![Point3::new(1.0, 2.0, 0.0), Point3::new(1.0, 5.0, 0.0)],
                vec![Point3::new(4.0, 2.0, 0.0), Point3::new(4.0, 5.0, 0.0)],
            ],
        )
        .unwrap(),
    };
    let mut point_on_surface_relation = relation.clone();
    point_on_surface_relation.id = "f3d:Design/BulkStream.dat:relation#109".into();
    point_on_surface_relation.record_index = 109;
    point_on_surface_relation.definition =
        crate::records::sketch_relations::SketchRelationDefinition::new(
            1,
            point_on_surface_relation.definition.pattern().cloned(),
        )
        .expect("valid relation definition");
    point_on_surface_relation
        .try_edit(|draft| {
            draft.members = (vec![106, 109]
                .into_iter()
                .map(SketchRelationMember::from_index)
                .collect::<Vec<_>>())
            .try_into()
            .expect("uniform member resolution");
        })
        .unwrap();
    point_on_surface_relation
        .try_edit(|draft| {
            draft.return_members = (vec![106, 109]
                .into_iter()
                .map(SketchRelationReturnMember::from_index)
                .collect::<Vec<_>>())
            .try_into()
            .expect("uniform member resolution");
        })
        .unwrap();

    let points = [point, coincident_point];
    let relations = [
        relation,
        midpoint_relation,
        coincident_relation,
        horizontal_relation,
        point_on_surface_relation,
    ];
    let (planar_sketches, planar_entities) =
        crate::test_support::with_decode_context(|decode_ctx| {
            project_sketch_design(
                decode_ctx,
                &[placement.clone()],
                &points,
                &curves,
                &[],
                &[],
                1.0e-6,
            )
        })
        .expect("sketch lanes pair");
    assert!(planar_sketches.is_empty());
    assert!(planar_entities.is_empty());
    let surfaces = [surface];
    let (sketches, entities) = crate::test_support::with_decode_context(|decode_ctx| {
        project_spatial_sketch_design(
            decode_ctx,
            &[placement.clone()],
            &points,
            &curves,
            &surfaces,
            &relations,
            1.0e-6,
        )
    })
    .unwrap();
    assert_eq!(sketches.len(), 1);
    assert_eq!(entities.len(), 8);
    assert!(entities
        .iter()
        .any(|entity| matches!(*entity.geometry.definition(),
            SpatialSketchGeometryDefinition::Line { start, end }
                if start == Point3::new(13.0, 21.0, 32.0)
                    && end == Point3::new(16.0, 24.0, 35.0)
        )));
    assert!(entities
        .iter()
        .any(|entity| matches!(*entity.geometry.definition(),
            SpatialSketchGeometryDefinition::Line { start, end }
                if start == Point3::new(14.0, 22.0, 33.0)
                    && end == Point3::new(17.0, 25.0, 36.0)
        )));
    assert!(entities
        .iter()
        .any(|entity| matches!(*entity.geometry.definition(),
            SpatialSketchGeometryDefinition::Line { start, end }
                if start == Point3::new(10.0, 21.0, 32.0)
                    && end == Point3::new(10.0, 24.0, 32.0)
        )));
    let constraints = crate::test_support::with_decode_context(|decode_ctx| {
        project_spatial_sketch_constraints(
            decode_ctx,
            &[placement],
            &relations,
            &points,
            &curves,
            &surfaces,
            &entities,
        )
    })
    .unwrap();
    assert!(matches!(
        constraints.first().map(|constraint| constraint.definition.kind()), Some(cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::SplineGroup { entities }) if entities == &[
            crate::ids::neutral_spatial_sketch_curve_id(&sketches[0].id, 3, 0),
            crate::ids::neutral_spatial_sketch_curve_id(&sketches[0].id, 4, 0),
        ]
    ));
    assert!(matches!(
        constraints
            .get(1)
            .map(|constraint| constraint.definition.kind()),
        Some(cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::Midpoint { .. })
    ));
    assert!(matches!(
        constraints
            .get(2)
            .map(|constraint| constraint.definition.kind()),
        Some(cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::Coincident { .. })
    ));
    assert!(matches!(
        constraints.get(3).map(|constraint| constraint.definition.kind()), Some(cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::ParallelToDirection {
                entity,
                direction,
            }) if entity == &crate::ids::neutral_spatial_sketch_curve_id(
            &sketches[0].id,
            7,
            0,
        ) && *direction.as_raw() == Vector3::new(0.0, 1.0, 0.0)
    ));
    assert!(matches!(
        constraints
            .get(4)
            .map(|constraint| constraint.definition.kind()),
        Some(cadmpeg_ir::sketches::SpatialSketchConstraintDefinitionInput::PointOnSurface { .. })
    ));
    assert!(entities
        .iter()
        .any(|entity| matches!(*entity.geometry.definition(),
            SpatialSketchGeometryDefinition::Circle {
                center,
                normal,
                reference_direction,
                radius: actual_radius,
            } if actual_radius.get() == 2.0 && center == Point3::new(13.0, 21.0, 32.0)
                && *normal.as_raw() == Vector3::new(0.0, 1.0, 0.0)
                && *reference_direction.as_raw() == Vector3::new(0.0, 0.0, 1.0)
        )));
}

#[test]
fn surface_only_owner_preserves_planar_and_spatial_projection_policies() {
    let placement = DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::MemberCompact {
                paired_byte_offset: 34,
            },
        )
        .unwrap(),
        id: "f3d:design:design-sketch-placement#1".into(),
        scope_record_index: None,
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_201".to_owned())
            .unwrap(),
        visibility: None,
        class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        record_index: 1,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("257".to_owned())
            .unwrap(),
    };
    let surface = SketchSurface {
        id: "f3d:design:surface#2".into(),
        record_index: 2,
        owner_reference: Some(201),
        class_tag: crate::records::references::DesignClassTag::try_from("306".to_owned()).unwrap(),
        byte_offset: 0,
        entity_genesis: None,
        persistent_id: std::num::NonZeroU64::new(2).unwrap(),
        geometry: crate::records::sketch_geometry::SketchSurfaceGeometry::from_parts(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
            ],
        )
        .unwrap(),
    };
    let placements = [placement];
    let (planar, planar_entities) = crate::test_support::with_decode_context(|decode_ctx| {
        project_sketch_design(
            decode_ctx,
            &placements,
            &[],
            &[],
            &[],
            &[],
            EPS_POINT_PROJECTION,
        )
    })
    .expect("sketch lanes pair");
    let (spatial, spatial_entities) = crate::test_support::with_decode_context(|decode_ctx| {
        project_spatial_sketch_design(
            decode_ctx,
            &placements,
            &[],
            &[],
            &[surface],
            &[],
            EPS_POINT_PROJECTION,
        )
    })
    .expect("valid spatial surface fixture");
    assert_eq!(planar.len(), 1);
    assert!(planar_entities.is_empty());
    assert_eq!(spatial.len(), 1);
    assert_eq!(spatial_entities.len(), 1);
}

const EPS_CONTAINMENT_DISTANCE: f64 = 1.0e-9;

mod projection_limits;
