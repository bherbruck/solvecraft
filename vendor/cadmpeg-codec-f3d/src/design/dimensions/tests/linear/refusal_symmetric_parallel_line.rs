// SPDX-License-Identifier: Apache-2.0
use super::{
    parameter_record, parse_design_parameter_record, Point2, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};
use cadmpeg_core::decode::ResourceDimension;
const EPS_REFUSAL_LINEAR: f64 = 1.0e-6;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let entity = |id: &str, geometry| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#symmetric-distance").unwrap(),
            geometry,
        )
    };
    let first = entity(
        "generated:test:line#first",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(0.0, 10.0),
        })
        .unwrap(),
    );
    let second = entity(
        "generated:test:line#second",
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(5.0, 2.0),
            end: Point2::new(5.0, 8.0),
        })
        .unwrap(),
    );
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(44),
        "value",
        "Linear Dimension-3",
        Some("mm"),
        "d1",
        1.0,
    ))
    .expect("symmetric line-width parameter");
    let parameter_id =
        cadmpeg_ir::features::ParameterId::mint("generated:test:parameter#symmetric")
            .expect("identity grammar");

    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        crate::design::dimensions::symmetric_parallel_line_dimension_definition(
            ctx,
            &first,
            &second,
            (1, 1),
            &parameter,
            parameter_id.clone(),
            EPS_REFUSAL_LINEAR,
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn symmetric_parallel_line_first_id_refuses_retained_limit() {
    fixture(
        "f3d symmetric parallel line first id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn symmetric_parallel_line_second_id_refuses_retained_limit() {
    fixture(
        "f3d symmetric parallel line second id",
        ResourceDimension::RetainedBytes,
    );
}
