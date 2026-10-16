// SPDX-License-Identifier: Apache-2.0
use crate::design::feature_project::closed_spatial_sketch_profiles;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::{
    SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
    SpatialSketchGeometryDefinition, SpatialSketchId,
};

const EPS_PROFILE_CLOSE: f64 = 1.0e-6;

fn fixture() -> (SpatialSketchId, Vec<SpatialSketchEntity>) {
    let sketch = SpatialSketchId::mint("synthetic:test:spatial-profile#sketch").unwrap();
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ];
    let mut entities = Vec::new();
    for index in 0..3 {
        entities.push(SpatialSketchEntity::new(
            SpatialSketchEntityId::mint(format!("synthetic:test:spatial-profile#line-{index}"))
                .unwrap(),
            sketch.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
                start: points[index],
                end: points[(index + 1) % 3],
            })
            .unwrap(),
        ));
    }
    entities.push(SpatialSketchEntity::new(
        SpatialSketchEntityId::mint("synthetic:test:spatial-profile#circle").unwrap(),
        sketch.clone(),
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Circle {
            center: Point3::new(4.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            reference_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: Length::new(1.0).unwrap(),
        })
        .unwrap(),
    ));
    (sketch, entities)
}

#[test]
fn spatial_profile_closed_loop_and_circle_keep_order() {
    let (sketch, entities) = fixture();
    let profiles = crate::test_support::with_decode_context(|decode_ctx| {
        closed_spatial_sketch_profiles(decode_ctx, &sketch, &entities, EPS_PROFILE_CLOSE)
    })
    .unwrap();
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].boundary().len(), 1);
    assert_eq!(profiles[1].boundary().len(), 3);
}

fn assert_limit(operation: &'static str, dimension: ResourceDimension) {
    let (sketch, entities) = fixture();
    let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
        closed_spatial_sketch_profiles(ctx, &sketch, &entities, EPS_PROFILE_CLOSE)
    });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
        if failure.operation == operation && failure.dimension == dimension)
    );
}

#[test]
fn spatial_profile_circle_id_refuses_retained_limit() {
    assert_limit(
        "f3d spatial profile circle id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn spatial_profile_boundary_id_refuses_retained_limit() {
    assert_limit(
        "f3d spatial profile boundary id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn spatial_profile_edge_refuses_collection_limit() {
    assert_limit(
        "f3d spatial profile edge",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_profile_unused_edge_refuses_collection_limit() {
    assert_limit(
        "f3d spatial profile unused edge",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_profile_use_refuses_collection_limit() {
    assert_limit(
        "f3d spatial profile use",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_profile_candidate_scan_refuses_work_limit() {
    assert_limit(
        "f3d spatial profile candidate scan",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn spatial_profile_boundary_use_refuses_collection_limit() {
    assert_limit(
        "f3d spatial profile boundary use",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_profile_output_refuses_collection_limit() {
    assert_limit("f3d spatial profile", ResourceDimension::CollectionItems);
}

#[test]
fn spatial_profile_uniqueness_refuses_collection_limit() {
    assert_limit(
        "f3d spatial profile boundary uniqueness",
        ResourceDimension::CollectionItems,
    );
}
