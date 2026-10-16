// SPDX-License-Identifier: Apache-2.0
use super::{
    exact_counted_dimension_relation, Angle, Length, Point2, SketchEntity, SketchEntityId,
    SketchGeometry, SketchGeometryDefinition, SketchId,
};
use cadmpeg_core::decode::ResourceDimension;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let entity = |id: &str, geometry: SketchGeometry| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            geometry,
        )
    };
    let circle = entity(
        "generated:test:circle#first",
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(1.0, 2.0),
            radius: Length::new(3.0).unwrap(),
        })
        .unwrap(),
    );
    let arc = entity(
        "generated:test:arc#second",
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(1.0, 2.0),
            radius: Length::new(2.0).unwrap(),
            start_angle: Angle::new(0.0).unwrap(),
            end_angle: Angle::new(1.0).unwrap(),
        })
        .unwrap(),
    );
    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        exact_counted_dimension_relation(ctx, &[&circle, &arc]).map(|_| ())
    });
}

#[test]
fn exact_centered_entity_relation_first_id_refuses_retained_limit() {
    fixture(
        "f3d exact centered entity relation first id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn exact_centered_entity_relation_second_id_refuses_retained_limit() {
    fixture(
        "f3d exact centered entity relation second id",
        ResourceDimension::RetainedBytes,
    );
}
