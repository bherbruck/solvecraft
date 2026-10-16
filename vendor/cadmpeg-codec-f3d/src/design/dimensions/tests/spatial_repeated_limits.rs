// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::test_support::parameter_record;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::math::{Point3, Vector3};
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
    let mut entities = Vec::new();
    let mut profiles = Vec::new();
    for (ordinal, x) in [(0_u32, 0.0), (4, 10.0)] {
        let corners = [
            Point3::new(x, 0.0, 0.0),
            Point3::new(x + 4.0, 0.0, 0.0),
            Point3::new(x + 4.0, 2.0, 0.0),
            Point3::new(x, 2.0, 0.0),
        ];
        let mut boundary = Vec::new();
        for edge in 0..4 {
            let entity = SpatialSketchEntity::new(
                SpatialSketchEntityId::mint(format!(
                    "synthetic:test:spatial-edge#{}",
                    ordinal + u32::try_from(edge).unwrap()
                ))
                .unwrap(),
                sketch.clone(),
                SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                    start: corners[edge],
                    end: corners[(edge + 1) % 4],
                })
                .unwrap(),
            );
            boundary.push(cadmpeg_ir::sketches::SpatialSketchEntityUse {
                entity: entity.id().clone(),
                reversed: false,
            });
            entities.push(entity);
        }
        profiles.push(
            cadmpeg_ir::sketches::SpatialSketchProfile::try_new(
                corners[0],
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                boundary,
                &cadmpeg_test_support::service_decode_context(),
                "spatial profile uniqueness",
            )
            .expect("fixture collection admission")
            .unwrap(),
        );
    }
    let sketches = [cadmpeg_ir::sketches::SpatialSketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        profiles,
        native_ref: None,
    }];

    super::assert_dimension_refusal(operation, dimension, |ctx| {
        crate::design::dimensions::owner_scoped_spatial_repeated_profile_line_distance_definition(
            ctx,
            &entities,
            &sketches,
            &sketch,
            &parameter,
            &parameter_id,
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn spatial_repeated_entity_index_refuses_collection_limit() {
    fixture(
        "f3d spatial repeated entity index",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_repeated_pair_key_refuses_collection_limit() {
    fixture(
        "f3d spatial repeated pair key",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_repeated_first_member_refuses_collection_limit() {
    fixture(
        "f3d spatial repeated first member",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_repeated_second_member_refuses_collection_limit() {
    fixture(
        "f3d spatial repeated second member",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_repeated_first_id_refuses_retained_limit() {
    fixture(
        "f3d spatial repeated first id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn spatial_repeated_second_id_refuses_retained_limit() {
    fixture(
        "f3d spatial repeated second id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn spatial_repeated_pair_refuses_collection_limit() {
    fixture(
        "f3d spatial repeated pair",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn owner_scoped_spatial_repeated_profile_line_distance_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d owner scoped spatial repeated profile line distance parameter id",
        ResourceDimension::RetainedBytes,
    );
}
