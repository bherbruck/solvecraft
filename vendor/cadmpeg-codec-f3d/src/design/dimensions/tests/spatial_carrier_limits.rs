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
    let entities = [
        line(1, 0.0, 4.0, 0.0),
        line(2, 4.0, 8.0, 0.0),
        line(3, 0.0, 4.0, 2.0),
        line(4, 4.0, 8.0, 2.0),
    ];
    super::assert_dimension_refusal(operation, dimension, |ctx| {
        crate::design::dimensions::owner_scoped_spatial_parallel_line_set_dimension_definition(
            ctx,
            &entities,
            &sketch,
            &parameter,
            &parameter_id,
            0.0,
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn spatial_line_set_candidate_refuses_collection_limit() {
    fixture(
        "f3d spatial line set candidate",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_line_carrier_candidate_refuses_collection_limit() {
    fixture(
        "f3d spatial line carrier candidate",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_carrier_member_refuses_collection_limit() {
    fixture(
        "f3d spatial carrier member",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_carrier_refuses_collection_limit() {
    fixture("f3d spatial carrier", ResourceDimension::CollectionItems);
}

#[test]
fn owner_scoped_spatial_parallel_line_set_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d owner scoped spatial parallel line set parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn spatial_carrier_output_entity_id_refuses_retained_limit() {
    fixture(
        "f3d spatial carrier output entity id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn spatial_carrier_output_member_refuses_collection_limit() {
    fixture(
        "f3d spatial carrier output member",
        ResourceDimension::CollectionItems,
    );
}
