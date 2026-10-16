// SPDX-License-Identifier: Apache-2.0
use super::{
    owner_scoped_angular_dimension_definition, parameter_record, parse_design_parameter_record,
    Point2, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};
use cadmpeg_core::decode::ResourceDimension;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let sketch = SketchId::mint("f3d:model:sketch#angular").unwrap();
    let line = |name: &str, angle: f64| {
        SketchEntity::new(
            SketchEntityId::mint(format!("f3d:model:sketch-entity#{name}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(angle.cos(), angle.sin()),
            })
            .unwrap(),
        )
    };
    let horizontal = line("horizontal", 0.0);
    let sloped = line("sloped", std::f64::consts::FRAC_PI_6);
    let vertical = line("vertical", std::f64::consts::FRAC_PI_2);
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(1),
        "30 deg",
        "Angular Dimension-2",
        Some("deg"),
        "d1",
        std::f64::consts::FRAC_PI_6,
    ))
    .expect("angular parameter");
    let parameter_id = cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#angle")
        .expect("identity grammar");

    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        owner_scoped_angular_dimension_definition(
            ctx,
            &[horizontal.clone(), sloped.clone(), vertical.clone()],
            &sketch,
            &parameter,
            &parameter_id,
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn owner_scoped_angular_first_id_refuses_retained_limit() {
    fixture(
        "f3d owner scoped angular first id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn owner_scoped_angular_second_id_refuses_retained_limit() {
    fixture(
        "f3d owner scoped angular second id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn owner_scoped_angular_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d owner scoped angular parameter id",
        ResourceDimension::RetainedBytes,
    );
}
