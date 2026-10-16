// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::test_support::parameter_record;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::sketches::{
    SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
    SpatialSketchGeometryDefinition, SpatialSketchId,
};

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let sketch = SpatialSketchId::mint("synthetic:test:spatial-sketch#refusal").unwrap();
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(1),
        "2 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d1",
        0.2,
    ))
    .unwrap();
    let parameter_id = ParameterId::mint("synthetic:test:parameter#spatial-refusal").unwrap();
    let line = |index: u32, x0: f64, x1: f64, y: f64| {
        SpatialSketchEntity::new(
            SpatialSketchEntityId::mint(format!("synthetic:test:spatial-line#{index}")).unwrap(),
            sketch.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                start: Point3::new(x0, y, 0.0),
                end: Point3::new(x1, y, 0.0),
            })
            .unwrap(),
        )
    };
    let entities = [line(1, 0.0, 4.0, 0.0), line(2, 0.0, 4.0, 2.0)];
    super::assert_dimension_refusal(operation, dimension, |ctx| {
        crate::design::dimensions::unique_spatial_parallel_line_dimension_definition(
            ctx,
            &entities,
            &sketch,
            &parameter,
            &parameter_id,
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn spatial_parallel_line_candidate_refuses_collection_limit() {
    fixture(
        "f3d spatial parallel line candidate",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn unique_spatial_parallel_line_first_id_refuses_retained_limit() {
    fixture(
        "f3d unique spatial parallel line first id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn unique_spatial_parallel_line_second_id_refuses_retained_limit() {
    fixture(
        "f3d unique spatial parallel line second id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn unique_spatial_parallel_line_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d unique spatial parallel line parameter id",
        ResourceDimension::RetainedBytes,
    );
}
