// SPDX-License-Identifier: Apache-2.0
use super::{
    Point2, SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};
use cadmpeg_core::decode::ResourceDimension;

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
    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        crate::design::dimensions::unique_parallel_line_dimension_definition(
            ctx,
            &entities,
            &sketch,
            &parameter,
            &cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#parameter")
                .expect("identity grammar"),
            0.0,
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn unique_parallel_line_first_id_refuses_retained_limit() {
    fixture(
        "f3d unique parallel line first id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn unique_parallel_line_second_id_refuses_retained_limit() {
    fixture(
        "f3d unique parallel line second id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn unique_parallel_line_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d unique parallel line parameter id",
        ResourceDimension::RetainedBytes,
    );
}
