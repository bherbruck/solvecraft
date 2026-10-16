// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};
fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let sketch = SketchId::mint("synthetic:test:sketch#two-locus").unwrap();
    let entity = |name: &str, position| {
        SketchEntity::new(
            SketchEntityId::mint(name).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position }).unwrap(),
        )
    };
    let first = entity("synthetic:test:entity#first", Point2::new(0.0, 0.0));
    let second = entity("synthetic:test:entity#second", Point2::new(2.0, 3.0));
    let parameter = ParameterId::mint("synthetic:test:parameter#two-locus").unwrap();
    super::assert_dimension_refusal(operation, dimension, |ctx| {
        crate::design::dimensions::two_locus_distance_dimension(
            ctx,
            &[&first, &second],
            parameter.clone(),
        )
        .transpose()
        .map(|_| ())
    });
}
#[test]
fn two_locus_output_id_refuses_retained_limit() {
    fixture(
        "f3d atomic member entity id",
        ResourceDimension::RetainedBytes,
    );
}
#[test]
fn two_locus_output_member_refuses_collection_limit() {
    fixture("f3d atomic member", ResourceDimension::CollectionItems);
}
