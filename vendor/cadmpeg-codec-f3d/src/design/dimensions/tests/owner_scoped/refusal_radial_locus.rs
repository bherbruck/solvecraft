// SPDX-License-Identifier: Apache-2.0
use super::{
    radial_locus_dimension_definition, Length, Point2, SketchEntity, SketchEntityId,
    SketchGeometry, SketchGeometryDefinition, SketchId,
};
use cadmpeg_core::decode::ResourceDimension;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let sketch = SketchId::mint("f3d:model:sketch#radial-loci").unwrap();
    let point = |id: &str, u, v| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, v),
            })
            .unwrap(),
        )
    };
    let circle = |id: &str, u, v, radius| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(u, v),
                radius: Length::new(radius).unwrap(),
            })
            .unwrap(),
        )
    };
    let center = point("synthetic:test:id#center", 2.0, 3.0);
    let annotation = point("synthetic:test:id#annotation", 7.0, 3.0);
    let measured = circle("synthetic:test:id#measured", 2.0, 3.0, 5.0);
    let other_center = circle("synthetic:test:id#other-center", 20.0, 30.0, 5.0);
    let other_radius = circle("synthetic:test:id#other-radius", 2.0, 3.0, 7.0);
    let all = [
        center.clone(),
        annotation.clone(),
        measured.clone(),
        other_center,
        other_radius,
    ];
    let parameter = cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#radial-loci")
        .expect("identity grammar");

    let repeated = circle("synthetic:test:id#repeated", 20.0, 3.0, 5.0);
    let selected: Vec<_> = if operation == "f3d radial center parameter id" {
        vec![&center]
    } else if matches!(
        operation,
        "f3d radial locus member"
            | "f3d radial locus unique member"
            | "f3d radial locus repeated parameter id"
    ) {
        vec![&measured, &repeated]
    } else {
        vec![&measured, &annotation]
    };
    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        radial_locus_dimension_definition(
            ctx,
            &selected,
            &all,
            "Radial Dimension-2",
            0.5,
            &parameter,
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn radial_locus_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d radial locus parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn radial_locus_definition_refuses_collection_limit() {
    fixture(
        "f3d radial locus definition",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn radial_locus_member_refuses_collection_limit() {
    fixture(
        "f3d radial locus member",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn radial_locus_unique_member_refuses_collection_limit() {
    fixture(
        "f3d radial locus unique member",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn radial_locus_repeated_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d radial locus repeated parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn radial_center_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d radial center parameter id",
        ResourceDimension::RetainedBytes,
    );
}
