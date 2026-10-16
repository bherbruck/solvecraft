// SPDX-License-Identifier: Apache-2.0
use super::{
    Point2, SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};
use cadmpeg_core::decode::ResourceDimension;
const EPS_REFUSAL_LINEAR: f64 = 1.0e-6;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let line = |name: &str, start, end| {
        SketchEntity::new(
            SketchEntityId::mint(name).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
    };
    let entities = vec![
        line(
            "synthetic:test:id#first",
            Point2::new(0.0, 0.0),
            Point2::new(4.0, 0.0),
        ),
        line(
            "synthetic:test:id#second",
            Point2::new(1.0, 2.0),
            Point2::new(5.0, 2.0),
        ),
        line(
            "synthetic:test:id#unrelated",
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 1.0),
        ),
    ];
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::dimensions::recipe_linear_dimension_candidates(decode_ctx, &entities, &sketch, 2.0, &cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar"), 0.0)).unwrap().as_slice(),
        [SketchConstraintDefinitionInput::Distance { entities, .. }]
            if entities.as_slice() == [SketchEntityId::mint("synthetic:test:id#first").unwrap(), SketchEntityId::mint("synthetic:test:id#second").unwrap()]
    ));
    let point = |name: &str, position| {
        SketchEntity::new(
            SketchEntityId::mint(name).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position }).unwrap(),
        )
    };
    let mut entities_with_endpoints = entities.clone();
    entities_with_endpoints.extend([
        point("synthetic:test:id#first-start", Point2::new(0.0, 0.0)),
        point("synthetic:test:id#first-end", Point2::new(4.0, 0.0)),
        point("synthetic:test:id#second-start", Point2::new(1.0, 2.0)),
        point("synthetic:test:id#second-end", Point2::new(5.0, 2.0)),
    ]);
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::dimensions::recipe_linear_dimension_candidates(decode_ctx, &entities_with_endpoints, &sketch, 2.0, &cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar"), 0.0)).unwrap().as_slice(),
        [SketchConstraintDefinitionInput::Distance { entities, .. }]
            if entities.as_slice() == [SketchEntityId::mint("synthetic:test:id#first").unwrap(), SketchEntityId::mint("synthetic:test:id#second").unwrap()]
    ));

    let parameter = crate::records::parameters::DesignParameter::try_from(
        crate::records::parameters::DesignParameterDraft {
            id: "f3d:A:design-parameter#1".into(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned())
                .unwrap(),
            record_index: 1,
            source_ordinal: 1,
            source: crate::records::parameters::DesignParameterSource::new(
                "Linear Dimension-2".into(),
                Some(2),
                Some(crate::records::identity::Located {
                    value: crate::records::parameters::DesignParameterDiscriminator::Code0,
                    offset: 22,
                }),
            )
            .unwrap(),
            expression: "2 mm".into(),
            expression_offset: 40,
            source_kind_offset: 60,

            unit: Some(crate::records::identity::RecordedValue {
                value: "mm".into(),
                offset: 70,
            }),
            name: "d1".into(),
            name_offset: 80,
            evaluated_value: 0.2,
            evaluated_value_offset: 90,
        },
    )
    .unwrap();
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::dimensions::unique_parallel_line_dimension_definition(decode_ctx, &entities, &sketch, &parameter, &cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#parameter").expect("identity grammar"), 0.0)).transpose().unwrap(),
        Some(SketchConstraintDefinitionInput::Distance {
            entities,
            ..
        }) if entities.as_slice()
            == [SketchEntityId::mint("synthetic:test:id#first").unwrap(), SketchEntityId::mint("synthetic:test:id#second").unwrap()]
    ));

    let fragment = line(
        "synthetic:test:id#second-fragment",
        Point2::new(7.0, 2.0),
        Point2::new(9.0, 2.0),
    );
    let mut fragmented_entities = entities.clone();
    fragmented_entities.push(fragment);
    fragmented_entities.push(line(
        "synthetic:test:id#disjoint-first",
        Point2::new(20.0, 0.0),
        Point2::new(20.0, 1.0),
    ));
    fragmented_entities.push(line(
        "synthetic:test:id#disjoint-second",
        Point2::new(22.0, 3.0),
        Point2::new(22.0, 4.0),
    ));
    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        crate::design::dimensions::owner_scoped_parallel_line_set_dimension_definition(
            ctx,
            &fragmented_entities,
            &sketch,
            &parameter,
            &cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#parameter")
                .expect("identity grammar"),
            EPS_REFUSAL_LINEAR,
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn parallel_line_set_candidate_refuses_collection_limit() {
    fixture(
        "f3d parallel line set candidate",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn parallel_line_carrier_candidate_refuses_collection_limit() {
    fixture(
        "f3d parallel line carrier candidate",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn planar_carrier_member_refuses_collection_limit() {
    fixture(
        "f3d planar carrier member",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn planar_carrier_refuses_collection_limit() {
    fixture("f3d planar carrier", ResourceDimension::CollectionItems);
}

#[test]
fn owner_scoped_parallel_line_set_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d owner scoped parallel line set parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn planar_carrier_output_id_retained_refuses_limit() {
    fixture(
        "f3d atomic member entity id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn planar_carrier_output_member_collection_refuses_limit() {
    fixture("f3d atomic member", ResourceDimension::CollectionItems);
}
