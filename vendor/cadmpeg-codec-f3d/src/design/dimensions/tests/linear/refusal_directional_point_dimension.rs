// SPDX-License-Identifier: Apache-2.0
use super::{
    directional_point_dimension, Point2, SketchEntityId, SketchGeometry, SketchGeometryDefinition,
    SketchId,
};
use cadmpeg_core::decode::ResourceDimension;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let entity = |id: &str, position| {
        cadmpeg_ir::sketches::SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            SketchId::mint("generated:test:sketch#0").unwrap(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position }).unwrap(),
        )
    };
    let first = entity("generated:test:point#first", Point2::new(4.0, 16.0));
    let second = entity("generated:test:point#second", Point2::new(4.0, 14.0));
    let parameter = cadmpeg_ir::features::ParameterId::mint("generated:test:parameter#distance")
        .expect("identity grammar");

    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        directional_point_dimension(ctx, &[&first, &second], 2.0, parameter.clone(), 0.0)
            .transpose()
            .map(|_| ())
    });
}

#[test]
fn directional_point_dimension_first_id_refuses_retained_limit() {
    fixture(
        "f3d directional point dimension first id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn directional_point_dimension_second_id_refuses_retained_limit() {
    fixture(
        "f3d directional point dimension second id",
        ResourceDimension::RetainedBytes,
    );
}
