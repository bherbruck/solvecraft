// SPDX-License-Identifier: Apache-2.0
use super::{
    radial_dimension_definition, Length, Point2, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};
use cadmpeg_core::decode::ResourceDimension;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let entity = SketchEntity::new(
        SketchEntityId::mint("f3d:model:sketch-entity#circle").unwrap(),
        SketchId::mint("f3d:model:sketch#radial").unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(2.0, 3.0),
            radius: Length::new(5.0).unwrap(),
        })
        .unwrap(),
    );
    let radius_parameter =
        cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#radius")
            .expect("identity grammar");
    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        radial_dimension_definition(
            ctx,
            &entity,
            "Radius Dimension-2",
            0.5,
            radius_parameter.clone(),
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn radial_at_tolerance_entity_id_refuses_retained_limit() {
    fixture(
        "f3d radial at tolerance entity id",
        ResourceDimension::RetainedBytes,
    );
}
