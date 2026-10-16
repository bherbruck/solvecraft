// SPDX-License-Identifier: Apache-2.0
use super::{
    owner_scoped_line_length_dimension_definition, parameter_record, parse_design_parameter_record,
    Point2, SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchLocus,
};
use cadmpeg_core::decode::ResourceDimension;
const EPS_REFUSAL_LINEAR: f64 = 1.0e-6;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let sketch = SketchId::mint("f3d:model:sketch#line-length").unwrap();
    let line = |name: &str, v: f64, length: f64| {
        SketchEntity::new(
            SketchEntityId::mint(format!("f3d:model:sketch-entity#{name}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, v),
                end: Point2::new(length, v),
            })
            .unwrap(),
        )
    };
    let first = line("first", 0.0, 4.0);
    let second = line("second", 2.0, 4.0 + 5.0e-7);
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(1),
        "4 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d1",
        0.4,
    ))
    .expect("linear parameter");
    let parameter_id =
        cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#line-length")
            .expect("identity grammar");

    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| owner_scoped_line_length_dimension_definition(decode_ctx, std::slice::from_ref(&first), &sketch, &parameter, &parameter_id, EPS_REFUSAL_LINEAR)).transpose().unwrap(),
        Some(SketchConstraintDefinitionInput::DistanceLoci {
            first: SketchLocus::Start(ref entity),
            second: SketchLocus::End(ref other),
            parameter: ref actual_parameter,
        }) if entity == first.id() && other == first.id() && actual_parameter == &parameter_id
    ));
    let selected = if operation == "f3d owner scoped line length entity id" {
        vec![first.clone()]
    } else {
        vec![first.clone(), second.clone()]
    };
    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        owner_scoped_line_length_dimension_definition(
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
fn line_length_candidate_refuses_collection_limit() {
    fixture(
        "f3d line length candidate",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn owner_scoped_line_length_entity_id_refuses_retained_limit() {
    fixture(
        "f3d owner scoped line length entity id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn owner_scoped_line_length_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d owner scoped line length parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn repeated_length_output_id_retained_refuses_limit() {
    fixture(
        "f3d atomic member entity id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn repeated_length_output_member_collection_refuses_limit() {
    fixture("f3d atomic member", ResourceDimension::CollectionItems);
}
