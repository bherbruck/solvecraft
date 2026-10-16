// SPDX-License-Identifier: Apache-2.0
use super::{
    edit, owner_scoped_radial_dimension_definition, parameter_record,
    parse_design_parameter_record, radial_dimension_definition, Length, Point2,
    SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};
use cadmpeg_core::decode::ResourceDimension;
const EPS_REFUSAL_LINEAR: f64 = 1.0e-6;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let entity = SketchEntity::new(
        SketchEntityId::mint("f3d:model:sketch-entity#circle").unwrap(),
        SketchId::mint("f3d:model:sketch#radial").unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(2.0, 3.0),
            radius: Length::new(5.0).unwrap(),
        })
        .unwrap(),
    );
    let radius_parameter =
        cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#radius")
            .expect("identity grammar");
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| radial_dimension_definition(decode_ctx, &entity, "Radius Dimension-2", 0.5, radius_parameter.clone())).transpose().unwrap(),
        Some(SketchConstraintDefinitionInput::Radius { entity: ref actual, parameter: ref p })
            if actual == entity.id() && p == &radius_parameter
    ));
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| radial_dimension_definition(decode_ctx, &entity, "Radial Dimension-3", 0.5, radius_parameter.clone())).transpose().unwrap(),
        Some(SketchConstraintDefinitionInput::Radius { entity: ref actual, .. })
            if actual == entity.id()
    ));
    let diameter_parameter =
        cadmpeg_ir::features::ParameterId::mint("synthetic:test:parameter#diameter")
            .expect("identity grammar");
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| radial_dimension_definition(decode_ctx, &entity, "Diameter Dimension-2", 1.0, diameter_parameter.clone())).transpose().unwrap(),
        Some(SketchConstraintDefinitionInput::Diameter { entity: ref actual, parameter: ref p })
            if actual == entity.id() && p == &diameter_parameter
    ));
    assert!(
        crate::test_support::with_decode_context(|decode_ctx| radial_dimension_definition(
            decode_ctx,
            &entity,
            "Diameter Dimension-2",
            0.5,
            diameter_parameter.clone()
        ))
        .transpose()
        .unwrap()
        .is_none()
    );
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(1),
        "10 mm",
        "Diameter Dimension-2",
        Some("mm"),
        "d1",
        1.0,
    ))
    .expect("diameter parameter");
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| owner_scoped_radial_dimension_definition(decode_ctx, std::slice::from_ref(&entity), &entity.sketch, &parameter, &diameter_parameter, EPS_REFUSAL_LINEAR)).transpose().unwrap(),
        Some(SketchConstraintDefinitionInput::Diameter {
            entity: ref actual,
            ..
        }) if actual == entity.id()
    ));
    let mut duplicate = SketchEntity::new(
        SketchEntityId::mint("f3d:model:sketch-entity#duplicate-circle").unwrap(),
        entity.sketch.clone(),
        entity.geometry.clone(),
    )
    .with_construction(entity.construction)
    .with_native_ref(entity.native_ref.clone())
    .with_geometry_ref(entity.geometry_ref.clone())
    .with_endpoint_refs(entity.endpoint_refs.clone());
    edit::replace(&mut duplicate.geometry, |previous| {
        let mut definition = previous.definition().to_raw();
        {
            const RADIUS_PERTURBATION: f64 = 5.0e-7;

            let definition: &mut cadmpeg_ir::sketches::SketchGeometryDefinition = &mut definition;

            let SketchGeometryDefinition::Circle { radius, .. } = definition else {
                unreachable!("test entity is circular")
            };
            *radius = cadmpeg_ir::scalar::Length::new(radius.get() + RADIUS_PERTURBATION).unwrap();
        };
        definition.try_into()
    })
    .unwrap();
    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        owner_scoped_radial_dimension_definition(
            ctx,
            &[entity.clone(), duplicate.clone()],
            &entity.sketch,
            &parameter,
            &diameter_parameter,
            EPS_REFUSAL_LINEAR,
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn owner_scoped_radial_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d owner scoped radial parameter id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn owner_radial_definition_refuses_collection_limit() {
    fixture(
        "f3d owner radial definition",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn owner_radial_member_refuses_collection_limit() {
    fixture(
        "f3d owner radial member",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn owner_scoped_radial_repeated_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d owner scoped radial repeated parameter id",
        ResourceDimension::RetainedBytes,
    );
}
