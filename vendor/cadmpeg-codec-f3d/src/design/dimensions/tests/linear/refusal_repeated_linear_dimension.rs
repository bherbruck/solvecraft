// SPDX-License-Identifier: Apache-2.0
use super::repeated_linear_dimension;
use cadmpeg_core::decode::ResourceDimension;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    use cadmpeg_ir::features::ParameterId;
    use cadmpeg_ir::sketches::{
        SketchConstraintDefinitionInput as Definition, SketchEntityId, SketchLocus,
    };

    let entity =
        |name: &str| SketchEntityId::mint(format!("generated:test:entity#{name}")).unwrap();
    let parameter =
        ParameterId::mint("synthetic:test:id#generated:distance").expect("identity grammar");
    let horizontal = |first: &str, second: &str| Definition::HorizontalDistance {
        first: SketchLocus::Entity(entity(first)),
        second: SketchLocus::Entity(entity(second)),
        parameter: parameter.clone(),
    };
    let candidates = if operation.starts_with("f3d repeated distance ") {
        vec![
            Definition::Distance {
                entities: vec![entity("a"), entity("b")],
                parameter: parameter.clone(),
            },
            Definition::Distance {
                entities: vec![entity("c"), entity("d")],
                parameter: parameter.clone(),
            },
        ]
    } else {
        vec![horizontal("a", "b"), horizontal("c", "d")]
    };
    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        repeated_linear_dimension(ctx, &candidates, parameter.clone())
            .transpose()
            .map(|_| ())
    });
}

#[test]
fn repeated_distance_first_id_refuses_retained_limit() {
    fixture(
        "f3d repeated distance first id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn repeated_distance_second_id_refuses_retained_limit() {
    fixture(
        "f3d repeated distance second id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn repeated_directional_first_locus_refuses_retained_limit() {
    fixture(
        "f3d repeated directional first locus",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn repeated_directional_second_locus_refuses_retained_limit() {
    fixture(
        "f3d repeated directional second locus",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn repeated_first_member_refuses_collection_limit() {
    fixture(
        "f3d repeated first member",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn repeated_second_member_refuses_collection_limit() {
    fixture(
        "f3d repeated second member",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn repeated_distance_measurement_refuses_collection_limit() {
    fixture(
        "f3d repeated distance measurement",
        ResourceDimension::CollectionItems,
    );
}
