// SPDX-License-Identifier: Apache-2.0
use super::{
    parameter_record, parse_design_parameter_record, ParameterId, Point2, SketchEntity,
    SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};
use cadmpeg_core::decode::ResourceDimension;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let line = |id: &str, end: Point2| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end,
            })
            .unwrap(),
        )
    };
    let first = line("generated:test:line#first", Point2::new(1.0, 1.0));
    let second = line("generated:test:line#second", Point2::new(-2.0, -2.0));
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(44),
        "45 deg",
        "Angular Dimension-2",
        Some("deg"),
        "d1",
        std::f64::consts::FRAC_PI_4,
    ))
    .expect("generated angular dimension is canonical");
    let parameter_id =
        ParameterId::mint("generated:test:parameter#axis-angle").expect("identity grammar");

    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        crate::design::dimensions::parallel_group_axis_angle_definition(
            ctx,
            &[&first, &second],
            &parameter,
            &parameter_id,
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn parallel_group_axis_angle_first_id_refuses_retained_limit() {
    fixture(
        "f3d parallel group axis angle first id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn parallel_group_axis_angle_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d parallel group axis angle parameter id",
        ResourceDimension::RetainedBytes,
    );
}
