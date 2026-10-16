use cadmpeg_core::decode::ResourceDimension;
// SPDX-License-Identifier: Apache-2.0

use super::{
    exact_circular_pattern, exact_rectangular_pattern, exact_text_relation, scalar_close,
    translated_sketch_geometry_matches, RectangularPatternDistanceForm,
};
use crate::records::{
    parameters::DesignParameter,
    sketch_relations::{
        SketchPatternDefinition, SketchRelation, SketchRelationMember, SketchRelationReturnMember,
    },
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchEntityId, SketchGeometry, SketchGeometryDefinition,
    SketchId,
};
use cadmpeg_test_support::wire;

mod circular_allocation;
mod rectangular_allocation;
mod text_allocation;

#[test]
fn translated_nurbs_match_borrowed_rational_poles() {
    let source = SketchGeometry::try_from(SketchGeometryDefinition::Nurbs {
        curve: PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(1.0, 0.0), Point2::new(2.0, 0.0)],
            Some(vec![1.0, 2.0]),
            false,
        )
        .expect("fixture pcurve construction admission")
        .unwrap(),
    })
    .unwrap();
    let shifted = SketchGeometry::try_from(SketchGeometryDefinition::Nurbs {
        curve: PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(4.0, 5.0), Point2::new(5.0, 5.0)],
            Some(vec![1.0, 2.0]),
            false,
        )
        .expect("fixture pcurve construction admission")
        .unwrap(),
    })
    .unwrap();
    assert!(translated_sketch_geometry_matches(
        &source,
        &shifted,
        Point2::new(3.0, 5.0)
    ));
}

#[test]
fn rotated_nurbs_match_borrowed_rational_poles() {
    let source = SketchGeometry::try_from(SketchGeometryDefinition::Nurbs {
        curve: PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(1.0, 0.0), Point2::new(2.0, 0.0)],
            Some(vec![1.0, 2.0]),
            false,
        )
        .expect("fixture pcurve construction admission")
        .unwrap(),
    })
    .unwrap();
    let rotated = SketchGeometry::try_from(SketchGeometryDefinition::Nurbs {
        curve: PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 1.0), Point2::new(0.0, 2.0)],
            Some(vec![1.0, 2.0]),
            false,
        )
        .expect("fixture pcurve construction admission")
        .unwrap(),
    })
    .unwrap();
    assert!(super::rotated_sketch_geometry_matches(
        &source,
        &rotated,
        Point2::new(0.0, 0.0),
        std::f64::consts::FRAC_PI_2
    ));
}

#[test]
fn constraint_index_refuses_each_collection_growth() {
    for operation in [
        "f3d sketch constraint placement index",
        "f3d sketch constraint native record key",
        "f3d sketch constraint projected entity index",
        "f3d sketch constraint point reference index",
        "f3d sketch constraint curve reference index",
        "f3d sketch constraint text reference index",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut index = std::collections::HashMap::new();
        assert!(matches!(
            ctx.insert_hash_map(&mut index, "input-key", 1, operation).map(|_| ()),
            Err(CodecError::ResourceLimit(limit)) if limit.operation == operation
        ));
        assert!(index.is_empty());
    }
}

fn projected_constraint_fixture() -> (
    crate::records::sketch_placement::DesignSketchPlacement,
    crate::records::sketch_geometry::SketchPoint,
    crate::records::sketch_geometry::SketchPoint,
    SketchRelation,
    cadmpeg_ir::sketches::SketchEntity,
) {
    use crate::records::identity::DesignEntityId;
    use crate::records::references::DesignClassTag;
    use crate::records::sketch_geometry::{
        SketchPointClosure, SketchPointCompanion, SketchPointDraft, SketchPointRecordForm,
    };
    use crate::records::sketch_placement::{
        DesignSketchFrame, DesignSketchFrameForm, DesignSketchPlacement,
    };
    use crate::records::sketch_relations::{SketchRelationDefinition, SketchRelationDraft};

    let placement = DesignSketchPlacement {
        frame: DesignSketchFrame::new(0, DesignSketchFrameForm::ScopeCompact).unwrap(),
        id: "f3d:test:design-sketch-placement#0".to_owned(),
        scope_record_index: Some(10),
        entity_id: DesignEntityId::try_from("test_100".to_owned()).unwrap(),
        visibility: None,
        class_tag: DesignClassTag::try_from("356".to_owned()).unwrap(),
        record_index: 11,
        paired_class_tag: DesignClassTag::try_from("259".to_owned()).unwrap(),
    };
    let make_point = |id: &str, record_index| {
        crate::records::sketch_geometry::SketchPoint::try_from(SketchPointDraft {
            id: id.to_owned(),
            record_index,
            owner_reference: None,
            class_tag: DesignClassTag::try_from("301".to_owned()).unwrap(),
            byte_offset: 0,
            coordinate_offset: 89,
            companion: SketchPointCompanion {
                incident_curves: Vec::new(),
            },
            record_form: SketchPointRecordForm::version11(
                u64::from(record_index),
                SketchPointClosure::Selector0State0,
                None,
                0.0,
            ),
            paired_reference: 0,
            coordinates: Point2::new(1.0, 2.0),
        })
        .unwrap()
    };
    let point = make_point("f3d:test:sketch-point#0", 20);
    let unprojected_point = make_point("f3d:test:sketch-point#1", 21);
    let relation = SketchRelation::try_new(SketchRelationDraft {
        id: "f3d:test:sketch-relation#30".to_owned(),
        record_index: 30,
        class_tag: DesignClassTag::try_from("302".to_owned()).unwrap(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 100,
        owner_entity_id: None,
        auxiliary_references: crate::records::identity::ReferenceRun::located(vec![
            crate::records::identity::Located {
                value: 21,
                offset: 0,
            },
        ]),
        rectangular_counted_reference_count: None,
        members: vec![SketchRelationMember::from_index(20)]
            .try_into()
            .unwrap(),
        owner_reference_offset: 0,
        definition: SketchRelationDefinition::new(0x1_0000_0040, None).unwrap(),
        entity_genesis: None,
        return_members: vec![SketchRelationReturnMember::from_index(20)]
            .try_into()
            .unwrap(),
        raw_bytes: vec![0; 160],
    })
    .unwrap();
    let mut entity = cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#projected-point").unwrap(),
        crate::ids::neutral_sketch_id(&placement),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(1.0, 2.0),
        })
        .unwrap(),
    );
    entity.native_ref = Some(point.id.clone());
    (placement, point, unprojected_point, relation, entity)
}

fn assert_projected_constraint_refusal(operation: &'static str) {
    let (placement, point, unprojected_point, relation, entity) = projected_constraint_fixture();
    let placements = [placement];
    let points = [point, unprojected_point];
    let relations = [relation];
    let entities = [entity];
    for limit in 0..64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match super::project_sketch_constraints(
            &ctx,
            &placements,
            &[],
            (&points, &[], &[]),
            &relations,
            &entities,
        ) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected collection refusal at {operation}: {other:?}"),
        }
    }
    panic!("no refusal at {operation}");
}

fn assert_projected_constraint_retained_refusal(operation: &'static str) {
    let (placement, point, unprojected_point, relation, entity) = projected_constraint_fixture();
    let placements = [placement];
    let points = [point, unprojected_point];
    let relations = [relation];
    let entities = [entity];
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                super::project_sketch_constraints(
                    &ctx,
                    &placements,
                    &[],
                    (&points, &[], &[]),
                    &relations,
                    &entities,
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (ResourceDimension::RetainedBytes)));
    }
}

#[test]
fn projected_constraint_refuses_sketch_id_copy() {
    assert_projected_constraint_retained_refusal("f3d sketch constraint sketch id");
}

#[test]
fn projected_constraint_refuses_native_entity_id_copy() {
    assert_projected_constraint_retained_refusal("f3d sketch constraint native entity id");
}

#[test]
fn projected_constraint_refuses_operand_native_reference_copy() {
    assert_projected_constraint_retained_refusal("f3d sketch constraint operand native reference");
}

#[test]
fn projected_constraint_refuses_relation_native_reference_copy() {
    assert_projected_constraint_retained_refusal("f3d sketch constraint native reference");
}

#[test]
fn projected_constraint_refuses_input_entity_growth() {
    assert_projected_constraint_refusal("f3d sketch constraint input entity");
}

#[test]
fn projected_constraint_refuses_semantic_entity_growth() {
    assert_projected_constraint_refusal("f3d sketch constraint semantic entity");
}

#[test]
fn projected_constraint_refuses_native_entity_growth() {
    assert_projected_constraint_refusal("f3d sketch constraint native entity");
}

#[test]
fn projected_constraint_refuses_native_operand_growth() {
    assert_projected_constraint_refusal("f3d sketch constraint native operand");
}

#[test]
fn projected_constraint_refuses_output_growth() {
    assert_projected_constraint_refusal("f3d projected sketch constraint");
}

#[test]
fn rectangular_pattern_instances_require_exact_translated_geometry() {
    let source = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(1.0, 2.0),
        end: Point2::new(4.0, 6.0),
    })
    .unwrap();
    let translated = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(11.0, -1.0),
        end: Point2::new(14.0, 3.0),
    })
    .unwrap();
    assert!(translated_sketch_geometry_matches(
        &source,
        &translated,
        Point2::new(10.0, -3.0),
    ));
    let reversed = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(14.0, 3.0),
        end: Point2::new(11.0, -1.0),
    })
    .unwrap();
    assert!(!translated_sketch_geometry_matches(
        &source,
        &reversed,
        Point2::new(10.0, -3.0),
    ));
    let resized = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
        center: Point2::new(12.0, 0.0),
        radius: cadmpeg_ir::scalar::Length::new(3.1).unwrap(),
    })
    .unwrap();
    assert!(!translated_sketch_geometry_matches(
        &SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(2.0, 3.0),
            radius: cadmpeg_ir::scalar::Length::new(3.0).unwrap(),
        })
        .unwrap(),
        &resized,
        Point2::new(10.0, -3.0),
    ));
}

fn point_entity(id: &str, u: f64) -> cadmpeg_ir::sketches::SketchEntity {
    cadmpeg_ir::sketches::SketchEntity::new(
        SketchEntityId::mint(id).unwrap(),
        SketchId::mint("generated:test:sketch#0").unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(u, 4.0),
        })
        .unwrap(),
    )
}

fn rectangular_point_relation(
    evaluated_count: u32,
    evaluated_distance: f64,
    distance_form: RectangularPatternDistanceForm,
) -> SketchRelation {
    let (rectangular_counted_reference_count, mut auxiliary_references) = match distance_form {
        RectangularPatternDistanceForm::AdjacentSpacing => (2, vec![100, 101]),
        RectangularPatternDistanceForm::SeedToFinalSpan => (0, Vec::new()),
    };
    auxiliary_references.extend([20, 21, 22, 23]);
    let members = (1..=evaluated_count).collect::<Vec<_>>();
    SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
        id: "f3d:native:sketch-relation#rectangular".into(),
        record_index: 10,
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 1,
        owner_entity_id: Some(cadmpeg_core::text::NonBlankString::new("0_1").unwrap()),
        auxiliary_references: crate::records::identity::ReferenceRun::located(
            auxiliary_references
                .into_iter()
                .map(|value| crate::records::identity::Located { value, offset: 0 })
                .collect(),
        ),
        rectangular_counted_reference_count: Some(rectangular_counted_reference_count),
        members: (members
            .clone()
            .into_iter()
            .map(SketchRelationMember::from_index)
            .collect::<Vec<_>>())
        .try_into()
        .expect("uniform member resolution"),
        owner_reference_offset: 0,
        definition: crate::records::sketch_relations::SketchRelationDefinition::new(
            0x2000_0000,
            Some(
                crate::records::sketch_relations::SketchPatternDefinition::Rectangular {
                    directions: [
                        crate::records::sketch_relations::SketchPatternDirection {
                            count_parameter: 20,
                            distance_parameter: 21,
                            evaluated_count:
                                crate::records::sketch_relations::SketchPatternCount::try_from(
                                    evaluated_count,
                                )
                                .unwrap(),
                            direction: [1.0, 0.0, 0.0].try_into().unwrap(),
                            evaluated_distance: cadmpeg_ir::scalar::FiniteReal::new(
                                evaluated_distance,
                            )
                            .unwrap(),
                        },
                        crate::records::sketch_relations::SketchPatternDirection {
                            count_parameter: 22,
                            distance_parameter: 23,
                            evaluated_count:
                                crate::records::sketch_relations::SketchPatternCount::try_from(1)
                                    .unwrap(),
                            direction: [0.0, 1.0, 0.0].try_into().unwrap(),
                            evaluated_distance: cadmpeg_ir::scalar::FiniteReal::ZERO,
                        },
                    ],
                },
            ),
        )
        .expect("valid relation definition"),
        entity_genesis: None,
        return_members: (members
            .iter()
            .copied()
            .map(SketchRelationReturnMember::from_index)
            .collect::<Vec<_>>())
        .try_into()
        .expect("uniform member resolution"),
        raw_bytes: vec![0; 160],
    })
    .unwrap()
}

fn rectangular_parameter(record_index: u32, value: f64) -> DesignParameter {
    crate::records::parameters::DesignParameter::try_from(
        crate::records::parameters::DesignParameterDraft {
            id: format!("native:design-parameter#{record_index}"),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("373".to_owned())
                .unwrap(),
            record_index,
            source_ordinal: 0,
            source: crate::records::parameters::DesignParameterSource::new(
                "R-Pattern1-distance".into(),
                Some(record_index),
                Some(crate::records::identity::Located {
                    value: crate::records::parameters::DesignParameterDiscriminator::Code6,
                    offset: 22,
                }),
            )
            .unwrap(),
            expression: value.to_string(),
            expression_offset: 40,
            source_kind_offset: 60,

            unit: Some(crate::records::identity::RecordedValue {
                value: "mm".into(),
                offset: 70,
            }),
            name: format!("d{record_index}"),
            name_offset: 80,
            evaluated_value: value,
            evaluated_value_offset: 90,
        },
    )
    .unwrap()
}

fn rectangular_parameters(count: u32, distance: f64) -> [DesignParameter; 4] {
    [
        rectangular_parameter(20, f64::from(count)),
        rectangular_parameter(21, distance),
        rectangular_parameter(22, 1.0),
        rectangular_parameter(23, 0.0),
    ]
}

#[test]
fn rectangular_pattern_projects_adjacent_spacing_and_parameter() {
    let seed = point_entity("generated:test:point#seed", 2.0);
    let second = point_entity("generated:test:point#second", 17.0);
    let third = point_entity("generated:test:point#third", 32.0);
    let relation =
        rectangular_point_relation(3, 1.5, RectangularPatternDistanceForm::AdjacentSpacing);
    let parameters = rectangular_parameters(3, 1.5);
    let Some(SketchConstraintDefinitionInput::RectangularPattern { pattern }) =
        crate::test_support::with_decode_context(|decode_ctx| {
            exact_rectangular_pattern(
                &relation,
                "native",
                &parameters,
                &[&seed, &second, &third],
                decode_ctx,
            )
        })
        .unwrap()
    else {
        panic!("rectangular pattern did not resolve");
    };
    let directions = pattern.directions();
    assert_eq!(directions[0].spacing().get(), 15.0);
    assert_eq!(directions[1].spacing().get(), 0.0);
    assert!(matches!(
        directions[0].distance,
        Some(cadmpeg_ir::sketches::SketchPatternDistance::Spacing { .. })
    ));
    assert!(directions[0].count_parameter.is_some());
    assert_eq!([pattern.rows().len(), pattern.rows()[0].len()], [3, 1]);
}

#[test]
fn rectangular_pattern_projects_total_span_and_keeps_span_parameter() {
    let seed = point_entity("generated:test:point#seed", 2.0);
    let second = point_entity("generated:test:point#second", 17.0);
    let third = point_entity("generated:test:point#third", 32.0);
    let relation =
        rectangular_point_relation(3, 3.0, RectangularPatternDistanceForm::SeedToFinalSpan);
    let parameters = rectangular_parameters(3, 3.0);
    let Some(SketchConstraintDefinitionInput::RectangularPattern { pattern }) =
        crate::test_support::with_decode_context(|decode_ctx| {
            exact_rectangular_pattern(
                &relation,
                "native",
                &parameters,
                &[&seed, &second, &third],
                decode_ctx,
            )
        })
        .unwrap()
    else {
        panic!("total-span rectangular pattern did not resolve");
    };
    let directions = pattern.directions();
    assert_eq!(directions[0].spacing().get(), 15.0);
    assert!(matches!(
        directions[0].distance,
        Some(cadmpeg_ir::sketches::SketchPatternDistance::Span { .. })
    ));
    assert!(directions[0].count_parameter.is_some());
}

#[test]
fn rectangular_pattern_does_not_change_distance_form_to_match_geometry() {
    let seed = point_entity("generated:test:point#seed", 2.0);
    let second = point_entity("generated:test:point#second", 17.0);
    let third = point_entity("generated:test:point#third", 32.0);
    for (distance_form, distance) in [
        (RectangularPatternDistanceForm::AdjacentSpacing, 3.0),
        (RectangularPatternDistanceForm::SeedToFinalSpan, 1.5),
    ] {
        let relation = rectangular_point_relation(3, distance, distance_form);
        let parameters = rectangular_parameters(3, distance);
        assert_eq!(
            crate::test_support::with_decode_context(|decode_ctx| exact_rectangular_pattern(
                &relation,
                "native",
                &parameters,
                &[&seed, &second, &third],
                decode_ctx
            ))
            .unwrap(),
            None
        );
    }
}

#[test]
fn rectangular_pattern_requires_the_retained_counted_reference_count() {
    let seed = point_entity("generated:test:point#seed", 2.0);
    let second = point_entity("generated:test:point#second", 17.0);
    let mut relation =
        rectangular_point_relation(2, 1.5, RectangularPatternDistanceForm::AdjacentSpacing);
    relation.rectangular_counted_reference_count = None;
    let parameters = rectangular_parameters(2, 1.5);

    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| exact_rectangular_pattern(
            &relation,
            "native",
            &parameters,
            &[&seed, &second],
            decode_ctx
        ))
        .unwrap(),
        None
    );
}

#[test]
fn rectangular_pattern_transfers_two_instances_in_both_distance_forms() {
    let seed = point_entity("generated:test:point#seed", 2.0);
    let second = point_entity("generated:test:point#second", 17.0);
    for distance_form in [
        RectangularPatternDistanceForm::AdjacentSpacing,
        RectangularPatternDistanceForm::SeedToFinalSpan,
    ] {
        let relation = rectangular_point_relation(2, 1.5, distance_form);
        let parameters = rectangular_parameters(2, 1.5);
        let Some(SketchConstraintDefinitionInput::RectangularPattern { pattern }) =
            crate::test_support::with_decode_context(|decode_ctx| {
                exact_rectangular_pattern(
                    &relation,
                    "native",
                    &parameters,
                    &[&seed, &second],
                    decode_ctx,
                )
            })
            .unwrap()
        else {
            panic!("two-instance rectangular pattern did not resolve");
        };
        let directions = pattern.directions();
        assert_eq!(directions[0].spacing().get(), 15.0);
        match distance_form {
            RectangularPatternDistanceForm::AdjacentSpacing => {
                assert!(matches!(
                    directions[0].distance,
                    Some(cadmpeg_ir::sketches::SketchPatternDistance::Spacing { .. })
                ));
            }
            RectangularPatternDistanceForm::SeedToFinalSpan => {
                assert!(matches!(
                    directions[0].distance,
                    Some(cadmpeg_ir::sketches::SketchPatternDistance::Span { .. })
                ));
            }
        }
    }
}

#[test]
fn circular_pattern_resolves_full_and_partial_instance_distributions() {
    let entity = |id: &str, geometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            geometry,
        )
    };
    let center = entity(
        "generated:test:point#center",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(2.0, -3.0),
        })
        .unwrap(),
    );
    let circle = |id: &str, angle: f64| {
        entity(
            id,
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(2.0 + 5.0 * angle.cos(), -3.0 + 5.0 * angle.sin()),
                radius: cadmpeg_ir::scalar::Length::new(0.75).unwrap(),
            })
            .unwrap(),
        )
    };
    let seed = circle("generated:test:circle#seed", 0.0);
    let middle = circle("generated:test:circle#middle", std::f64::consts::FRAC_PI_2);
    let last = circle("generated:test:circle#last", std::f64::consts::PI);
    let relation = |angle| {
        SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
            id: "f3d:native:sketch-relation#circular".into(),
            record_index: 10,
            class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned())
                .unwrap(),
            byte_offset: 0,
            state_offset: 0,
            owner_reference: 1,
            owner_entity_id: Some(cadmpeg_core::text::NonBlankString::new("0_1").unwrap()),
            auxiliary_references: crate::records::identity::ReferenceRun::located(
                vec![20, 21]
                    .into_iter()
                    .map(|value| crate::records::identity::Located { value, offset: 0 })
                    .collect(),
            ),
            rectangular_counted_reference_count: None,
            members: (vec![
                SketchRelationMember::from_index(1),
                SketchRelationMember::from_index(2),
                SketchRelationMember::from_index(3),
                SketchRelationMember::from_index(4),
            ])
            .try_into()
            .expect("uniform member resolution"),
            owner_reference_offset: 0,
            definition: crate::records::sketch_relations::SketchRelationDefinition::new(
                0x1000_0000,
                Some(
                    crate::records::sketch_relations::SketchPatternDefinition::Circular {
                        angle_parameter: 20,
                        count_parameter: 21,
                        evaluated_angle: cadmpeg_ir::scalar::FiniteReal::new(angle).unwrap(),
                        evaluated_count:
                            crate::records::sketch_relations::SketchPatternCount::try_from(3)
                                .unwrap(),
                    },
                ),
            )
            .expect("valid relation definition"),
            entity_genesis: None,
            return_members: (vec![
                SketchRelationReturnMember::from_index(2),
                SketchRelationReturnMember::from_index(3),
                SketchRelationReturnMember::from_index(4),
                SketchRelationReturnMember::from_index(1),
            ])
            .try_into()
            .expect("uniform member resolution"),
            raw_bytes: vec![0; 160],
        })
        .unwrap()
    };
    let members = [&center, &seed, &middle, &last];
    let returned = [&seed, &middle, &last, &center];
    let Some(SketchConstraintDefinitionInput::CircularPattern { pattern }) =
        crate::test_support::with_decode_context(|decode_ctx| {
            exact_circular_pattern(
                &relation(std::f64::consts::PI),
                "native",
                &[],
                &members,
                &returned,
                decode_ctx,
            )
        })
        .unwrap()
    else {
        panic!("partial circular pattern did not resolve");
    };
    assert_eq!(pattern.center(), center.id());
    assert_eq!(
        wire::field::<cadmpeg_ir::scalar::Angle>(&pattern, "angle").get(),
        std::f64::consts::PI
    );
    assert_eq!((pattern.instances().len() + 1), 3);
    // The seed is not an instance, so the instance list carries only the
    // rotations after it.
    assert_eq!(
        wire::field::<Vec<cadmpeg_ir::sketches::SketchEntityId>>(&pattern, "seed"),
        std::slice::from_ref(seed.id())
    );
    assert_eq!(
        pattern
            .instances()
            .iter()
            .map(|instance| instance.angle.get())
            .collect::<Vec<_>>(),
        [std::f64::consts::FRAC_PI_2, std::f64::consts::PI]
    );

    let full_middle = circle(
        "generated:test:circle#full-middle",
        std::f64::consts::TAU / 3.0,
    );
    let full_last = circle(
        "generated:test:circle#full-last",
        2.0 * std::f64::consts::TAU / 3.0,
    );
    let full_members = [&center, &seed, &full_middle, &full_last];
    let full_returned = [&seed, &full_middle, &full_last, &center];
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| exact_circular_pattern(&relation(std::f64::consts::TAU), "native", &[], &full_members, &full_returned, decode_ctx)).unwrap(),
        Some(SketchConstraintDefinitionInput::CircularPattern { ref pattern })
            if scalar_close(pattern.instances()[0].angle.get(), std::f64::consts::TAU / 3.0)
    ));
}

#[test]
fn circular_pattern_resolves_independently_of_relation_ordinals() {
    let entity = |id: &str, geometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            geometry,
        )
    };
    let center = entity(
        "generated:test:point#center",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(2.0, -3.0),
        })
        .unwrap(),
    );
    let circle = |id: &str, angle: f64| {
        entity(
            id,
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(2.0 + 5.0 * angle.cos(), -3.0 + 5.0 * angle.sin()),
                radius: cadmpeg_ir::scalar::Length::new(0.75).unwrap(),
            })
            .unwrap(),
        )
    };
    let seed = circle("generated:test:circle#seed", 0.0);
    let middle = circle("generated:test:circle#middle", std::f64::consts::TAU / 3.0);
    let last = circle(
        "generated:test:circle#last",
        2.0 * std::f64::consts::TAU / 3.0,
    );
    // Ordinals are all zero; geometry must still partition the members.
    let relation = SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
        id: "f3d:native:sketch-relation#circular".into(),
        record_index: 10,
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 1,
        owner_entity_id: Some(cadmpeg_core::text::NonBlankString::new("0_1").unwrap()),
        auxiliary_references: crate::records::identity::ReferenceRun::located(
            vec![20, 21]
                .into_iter()
                .map(|value| crate::records::identity::Located { value, offset: 0 })
                .collect(),
        ),
        rectangular_counted_reference_count: None,
        members: (vec![
            SketchRelationMember::from_index(1),
            SketchRelationMember::from_index(2),
            SketchRelationMember::from_index(3),
            SketchRelationMember::from_index(4),
        ])
        .try_into()
        .expect("uniform member resolution"),
        owner_reference_offset: 0,
        definition: crate::records::sketch_relations::SketchRelationDefinition::new(
            0x1000_0000,
            Some(
                crate::records::sketch_relations::SketchPatternDefinition::Circular {
                    angle_parameter: 20,
                    count_parameter: 21,
                    evaluated_angle: cadmpeg_ir::scalar::FiniteReal::new(std::f64::consts::TAU)
                        .unwrap(),
                    evaluated_count:
                        crate::records::sketch_relations::SketchPatternCount::try_from(3).unwrap(),
                },
            ),
        )
        .expect("valid relation definition"),
        entity_genesis: None,
        return_members: (vec![
            SketchRelationReturnMember::from_index(2),
            SketchRelationReturnMember::from_index(3),
            SketchRelationReturnMember::from_index(4),
            SketchRelationReturnMember::from_index(1),
        ])
        .try_into()
        .expect("uniform member resolution"),
        raw_bytes: vec![0; 160],
    })
    .unwrap();
    let members = [&center, &seed, &middle, &last];
    let returned = [&seed, &middle, &last, &center];
    let Some(SketchConstraintDefinitionInput::CircularPattern { pattern }) =
        crate::test_support::with_decode_context(|decode_ctx| {
            exact_circular_pattern(&relation, "native", &[], &members, &returned, decode_ctx)
        })
        .unwrap()
    else {
        panic!("role-agnostic circular pattern did not resolve");
    };
    assert_eq!(pattern.center(), center.id());
    assert_eq!((pattern.instances().len() + 1), 3);
}

#[test]
fn text_path_relation_projects_typed_entities_and_scaled_glyph_placements() {
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::scalar::Length;
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
        SketchGeometryDefinition, SketchId,
    };

    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let path = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#path").unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        })
        .unwrap(),
    );
    let text = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#text").unwrap(),
        sketch,
        SketchGeometry::try_from(SketchGeometryDefinition::Text {
            text: cadmpeg_core::text::NonBlankString::new("A").unwrap(),
            font_family: cadmpeg_core::text::NonBlankString::new("Arial").unwrap(),
            font_weight: cadmpeg_ir::sketches::SketchFontWeight::Regular,
            height: Length::new(10.0).unwrap(),
            width_factor: Some(0.8),
            placement: None,
            horizontal_alignment: None,
            vertical_alignment: None,
        })
        .unwrap(),
    );
    let mut glyph = [[0.0; 4]; 4];
    for ordinal in 0..4 {
        glyph[ordinal][ordinal] = 1.0;
    }
    glyph[0][3] = 0.5;
    let relation = SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
        id: "f3d:Design/BulkStream.dat:sketch-relation#3".into(),
        record_index: 3,
        class_tag: crate::records::references::DesignClassTag::try_from("413".to_owned()).unwrap(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 1,
        owner_entity_id: None,
        auxiliary_references: crate::records::identity::ReferenceRun::located(
            vec![2]
                .into_iter()
                .map(|value| crate::records::identity::Located { value, offset: 0 })
                .collect(),
        ),
        rectangular_counted_reference_count: None,
        members: (vec![
            SketchRelationMember::from_index(1),
            SketchRelationMember::from_index(2),
        ])
        .try_into()
        .expect("uniform member resolution"),
        owner_reference_offset: 0,
        definition: crate::records::sketch_relations::SketchRelationDefinition::new(
            0x200_0000_0000,
            Some(
                crate::records::sketch_relations::SketchPatternDefinition::TextPath {
                    text_reference: 2,
                    glyph_transforms: vec![
                        crate::records::sketch_relations::SketchGlyphTransform::try_from(glyph)
                            .expect("finite native glyph"),
                    ],
                },
            ),
        )
        .expect("valid relation definition"),
        entity_genesis: Some(2),
        return_members: (vec![SketchRelationReturnMember::from_index(1)])
            .try_into()
            .expect("uniform member resolution"),
        raw_bytes: vec![0; 160],
    })
    .unwrap();
    let projected = std::collections::HashMap::from([(("scope", 1), &path), (("scope", 2), &text)]);
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        exact_text_relation(&relation, "scope", &projected, decode_ctx)
    })
    .unwrap()
    .expect("typed text path");
    assert!(matches!(
        definition,
        SketchConstraintDefinitionInput::TextPath {
            text: ref text_id,
            path: ref path_id,
            ref glyph_transforms,
        } if text_id == text.id()
            && path_id == path.id()
            && glyph_transforms[0].rows()[0][3] == 5.0
    ));
    let mut relation = relation;
    let mut overflow = glyph;
    overflow[0][3] = f64::MAX;
    let mut non_affine = glyph;
    non_affine[3][3] = 2.0;
    for rows in [overflow, non_affine] {
        relation.definition = crate::records::sketch_relations::SketchRelationDefinition::new(
            0x200_0000_0000,
            Some(SketchPatternDefinition::TextPath {
                text_reference: 2,
                glyph_transforms: vec![
                    crate::records::sketch_relations::SketchGlyphTransform::try_from(glyph)
                        .unwrap(),
                    crate::records::sketch_relations::SketchGlyphTransform::try_from(rows).unwrap(),
                ],
            }),
        )
        .unwrap();
        assert!(
            crate::test_support::with_decode_context(|decode_ctx| exact_text_relation(
                &relation, "scope", &projected, decode_ctx
            ))
            .unwrap()
            .is_none()
        );
    }
}
