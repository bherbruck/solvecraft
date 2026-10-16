// SPDX-License-Identifier: Apache-2.0
use super::{
    parameter_record, parse_design_parameter_record, unique_point_class_dimension_definition,
    Point2, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};
use cadmpeg_core::decode::ResourceDimension;
const EPS_REFUSAL_LINEAR: f64 = 1.0e-6;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let sketch = SketchId::mint("f3d:model:sketch#point-classes").unwrap();
    let point = |name: &str, u: f64, v: f64| {
        SketchEntity::new(
            SketchEntityId::mint(format!("f3d:model:sketch-entity#{name}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(u, v),
            })
            .unwrap(),
        )
    };
    let lower = point("lower", -53.0, -20.875);
    let lower_duplicate = point("lower-duplicate", -53.0, -20.875 + 5.0e-7);
    let upper = point("upper", -53.0, -7.875);
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(1),
        "13 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d19",
        1.3,
    ))
    .expect("linear parameter");
    let parameter_id =
        cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#point-classes")
            .expect("identity grammar");

    let separated = point("separated", 0.0, 1.5 * EPS_REFUSAL_LINEAR);
    let origin = point("origin", 0.0, 0.0);
    let bridge = point("bridge", 0.0, 0.75 * EPS_REFUSAL_LINEAR);
    let selected = if operation == "f3d point class merged member" {
        vec![origin, separated, bridge, upper.clone()]
    } else {
        vec![lower, lower_duplicate, upper]
    };
    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        unique_point_class_dimension_definition(
            ctx,
            &selected,
            &sketch,
            &parameter,
            &parameter_id,
            EPS_REFUSAL_LINEAR,
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn point_class_candidate_refuses_collection_limit() {
    fixture(
        "f3d point class candidate",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn point_class_match_refuses_collection_limit() {
    fixture("f3d point class match", ResourceDimension::CollectionItems);
}

#[test]
fn point_class_member_refuses_collection_limit() {
    fixture("f3d point class member", ResourceDimension::CollectionItems);
}

#[test]
fn point_class_refuses_collection_limit() {
    fixture("f3d point class", ResourceDimension::CollectionItems);
}

#[test]
fn point_class_merged_member_refuses_collection_limit() {
    fixture(
        "f3d point class merged member",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn unique_point_class_first_id_refuses_retained_limit() {
    fixture(
        "f3d unique point class first id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn unique_point_class_second_id_refuses_retained_limit() {
    fixture(
        "f3d unique point class second id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn unique_point_class_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d unique point class parameter id",
        ResourceDimension::RetainedBytes,
    );
}
