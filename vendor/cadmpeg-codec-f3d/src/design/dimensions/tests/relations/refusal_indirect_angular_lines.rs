// SPDX-License-Identifier: Apache-2.0
use super::{
    indirect_angular_lines, HashMap, Point2, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};
use cadmpeg_core::decode::ResourceDimension;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let entity = |id: &str, geometry: SketchGeometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            geometry,
        )
    };
    let point = entity(
        "generated:test:point#vertex",
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(0.0, 0.0),
        })
        .unwrap(),
    );
    let explicit = entity(
        "generated:test:line#explicit",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(2.0, -2.0),
            end: Point2::new(2.0, 2.0),
        })
        .unwrap(),
    );
    let diagonal = entity(
        "generated:test:line#diagonal",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 2.0),
        })
        .unwrap(),
    );
    let horizontal = entity(
        "generated:test:line#horizontal",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(2.0, 0.0),
        })
        .unwrap(),
    );
    let projected = HashMap::from([
        (("native", 1), &point),
        (("native", 2), &explicit),
        (("native", 3), &diagonal),
        (("native", 4), &horizontal),
    ]);

    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        indirect_angular_lines(
            ctx,
            "native",
            &[&point, &explicit],
            std::f64::consts::FRAC_PI_4,
            &projected,
        )
        .map(|_| ())
    });
}

#[test]
fn indirect_angular_first_id_refuses_retained_limit() {
    fixture(
        "f3d indirect angular first id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn indirect_angular_second_id_refuses_retained_limit() {
    fixture(
        "f3d indirect angular second id",
        ResourceDimension::RetainedBytes,
    );
}
