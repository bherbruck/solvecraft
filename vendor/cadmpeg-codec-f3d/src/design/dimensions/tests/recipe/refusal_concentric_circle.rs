// SPDX-License-Identifier: Apache-2.0
use super::{
    Length, Point2, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition,
    SketchId,
};
use cadmpeg_core::decode::ResourceDimension;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
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
    let circle = |name: &str, center, radius| {
        SketchEntity::new(
            SketchEntityId::mint(name).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center,
                radius: Length::new(radius).unwrap(),
            })
            .unwrap(),
        )
    };
    let circles = vec![
        circle("synthetic:test:id#outer-a", Point2::new(0.0, 0.0), 5.0),
        circle("synthetic:test:id#inner-a", Point2::new(0.0, 0.0), 3.0),
        circle("synthetic:test:id#outer-b", Point2::new(20.0, 0.0), 8.0),
        circle("synthetic:test:id#inner-b", Point2::new(20.0, 0.0), 6.0),
    ];
    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        crate::design::dimensions::concentric_circle_dimension_definition(
            ctx,
            if operation.starts_with("f3d concentric circle ")
                && operation != "f3d concentric circle candidate"
            {
                &circles[..2]
            } else {
                &circles
            },
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
fn concentric_circle_candidate_refuses_collection_limit() {
    fixture(
        "f3d concentric circle candidate",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn concentric_used_first_refuses_collection_limit() {
    fixture(
        "f3d concentric used first",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn concentric_used_second_refuses_collection_limit() {
    fixture(
        "f3d concentric used second",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn concentric_pair_refuses_collection_limit() {
    fixture("f3d concentric pair", ResourceDimension::CollectionItems);
}

#[test]
fn concentric_circle_first_id_refuses_retained_limit() {
    fixture(
        "f3d concentric circle first id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn concentric_circle_second_id_refuses_retained_limit() {
    fixture(
        "f3d concentric circle second id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn concentric_circle_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d concentric circle parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn concentric_repeated_first_id_refuses_retained_limit() {
    fixture(
        "f3d concentric repeated first id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn concentric_repeated_second_id_refuses_retained_limit() {
    fixture(
        "f3d concentric repeated second id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn concentric_measurement_refuses_collection_limit() {
    fixture(
        "f3d concentric measurement",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn concentric_repeated_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d concentric repeated parameter id",
        ResourceDimension::RetainedBytes,
    );
}
