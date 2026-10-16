// SPDX-License-Identifier: Apache-2.0
use super::{
    exact_counted_dimension_relation, Point2, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};
use cadmpeg_core::decode::ResourceDimension;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let entity = |id: &str, geometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            geometry,
        )
    };
    let horizontal = entity(
        "generated:test:line#horizontal",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
        })
        .unwrap(),
    );
    let vertical = entity(
        "generated:test:line#vertical",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, -2.0),
            end: Point2::new(0.0, 2.0),
        })
        .unwrap(),
    );
    let left = entity(
        "generated:test:point#left",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(-2.0, 0.0),
        })
        .unwrap(),
    );
    let right = entity(
        "generated:test:point#right",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(2.0, 0.0),
        })
        .unwrap(),
    );
    let selected = if operation == "f3d exact counted axis id" {
        vec![&left, &right, &vertical]
    } else {
        vec![&horizontal, &vertical]
    };
    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        exact_counted_dimension_relation(ctx, &selected).map(|_| ())
    });
}

#[test]
fn exact_counted_first_id_refuses_retained_limit() {
    fixture(
        "f3d exact counted first id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn exact_counted_second_id_refuses_retained_limit() {
    fixture(
        "f3d exact counted second id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn exact_counted_axis_id_refuses_retained_limit() {
    fixture(
        "f3d exact counted axis id",
        ResourceDimension::RetainedBytes,
    );
}
