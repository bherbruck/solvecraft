// SPDX-License-Identifier: Apache-2.0
use super::{
    Point2, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};
use cadmpeg_core::decode::ResourceDimension;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let line = |name: &str, start, end| {
        SketchEntity::new(
            SketchEntityId::mint(name).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
    };
    let mut entities = vec![
        line(
            "synthetic:test:id#first",
            Point2::new(0.0, 0.0),
            Point2::new(4.0, 0.0),
        ),
        line(
            "synthetic:test:id#second",
            Point2::new(1.0, 2.0),
            Point2::new(5.0, 2.0),
        ),
        line(
            "synthetic:test:id#unrelated",
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 1.0),
        ),
    ];
    for (name, position) in [
        ("synthetic:test:id#point-a", Point2::new(20.0, 0.0)),
        ("synthetic:test:id#point-b", Point2::new(22.0, 0.0)),
    ] {
        entities.push(SketchEntity::new(
            SketchEntityId::mint(name).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position }).unwrap(),
        ));
    }
    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        crate::design::dimensions::recipe_linear_dimension_candidates(
            ctx,
            &entities,
            &sketch,
            2.0,
            &cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#parameter")
                .expect("identity grammar"),
            0.0,
        )
        .map(|_| ())
    });
}

#[test]
fn recipe_sketch_candidate_refuses_collection_limit() {
    fixture(
        "f3d recipe sketch candidate",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn recipe_point_candidate_refuses_collection_limit() {
    fixture(
        "f3d recipe point candidate",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn recipe_line_candidate_refuses_collection_limit() {
    fixture(
        "f3d recipe line candidate",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn recipe_line_pair_refuses_collection_limit() {
    fixture("f3d recipe line pair", ResourceDimension::CollectionItems);
}

#[test]
fn recipe_directional_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d recipe directional parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn recipe_point_definition_refuses_collection_limit() {
    fixture(
        "f3d recipe point definition",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn recipe_line_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d recipe line parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn recipe_line_definition_refuses_collection_limit() {
    fixture(
        "f3d recipe line definition",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn recipe_line_output_id_retained_refuses_limit() {
    fixture(
        "f3d atomic member entity id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn recipe_line_output_member_collection_refuses_limit() {
    fixture("f3d atomic member", ResourceDimension::CollectionItems);
}
