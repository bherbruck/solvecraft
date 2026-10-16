// SPDX-License-Identifier: Apache-2.0
use super::{
    parameter_record, parse_design_parameter_record, Point2, SketchConstraintDefinitionInput,
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};
use cadmpeg_core::decode::ResourceDimension;
const EPS_REFUSAL_LINEAR: f64 = 1.0e-6;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let sketch = SketchId::mint("generated:test:sketch#presentation").unwrap();
    let entity = |record_index: u32, geometry: SketchGeometry| {
        SketchEntity::new(
            SketchEntityId::mint(format!("generated:test:entity#{record_index}")).unwrap(),
            sketch.clone(),
            geometry,
        )
        .with_native_ref(Some(format!("stream:geometry#{record_index}")))
    };
    let line = entity(
        306,
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(90.4875, -17.78),
            end: Point2::new(90.4875, 17.78),
        })
        .unwrap(),
    );
    let circle = entity(
        331,
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: cadmpeg_ir::scalar::Length::new(11.1125).unwrap(),
        })
        .unwrap(),
    );
    let arc = entity(
        796,
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(60.344_057_626_1, -19.05),
            radius: cadmpeg_ir::scalar::Length::new(12.7).unwrap(),
            start_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
            end_angle: cadmpeg_ir::scalar::Angle::new(0.975_682_713_4).unwrap(),
        })
        .unwrap(),
    );
    let outer_arc = entity(
        782,
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(60.344_057_626_1, 19.05),
            radius: cadmpeg_ir::scalar::Length::new(12.7).unwrap(),
            start_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
            end_angle: cadmpeg_ir::scalar::Angle::new(0.975_682_713_4).unwrap(),
        })
        .unwrap(),
    );
    let first_point = entity(
        1061,
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(11.1125, -11.1125),
        })
        .unwrap(),
    );
    let second_point = entity(
        1075,
        SketchGeometry::try_from(SketchGeometryDefinition::Point {
            position: Point2::new(11.1125, -7.3025),
        })
        .unwrap(),
    );
    let entities = [line, circle, arc, outer_arc, first_point, second_point];
    let projected = entities
        .iter()
        .map(|entity| {
            let record_index = entity
                .native_ref
                .as_deref()
                .and_then(|native_ref| native_ref.rsplit_once('#'))
                .and_then(|(_, index)| index.parse::<u32>().ok())
                .expect("synthetic native geometry record");
            (("stream", record_index), entity)
        })
        .collect::<std::collections::HashMap<_, _>>();
    let frame = |operands| crate::records::dimensions::DesignDimensionPresentationFrame {
        id: "stream:presentation#0".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("314".to_owned()).unwrap(),
        record_index: 0,
        frame_length: 0,
        operands,
        presentation_bytes: Vec::new().into(),
        presentation_byte_offset: 0,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("281".to_owned())
            .unwrap(),
        paired_byte_offset: 0,
        owner_reference: 0,
        owner_reference_offset: 0,
        governing_owner_record_index: 0,
        governing_parameter_record_index: 0,
        governing_companion_record_index: 0,
    };
    let operand = |record_index| crate::records::dimensions::DesignDimensionPresentationOperand {
        geometry_record_index: std::num::NonZeroU32::new(record_index).unwrap(),
        geometry_reference_offset: 0,
        role: 0,
        role_offset: 0,
    };
    let tangent_span = parse_design_parameter_record(&parameter_record(
        Some(44),
        "10.16",
        "Tangent Dimension-2",
        Some("cm"),
        "d4",
        10.16,
    ))
    .expect("synthetic tangent dimension");
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::dimensions::presentation_dimension_definition(decode_ctx, "stream", &frame(vec![operand(306), operand(331)]), &projected, &tangent_span, &cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#parameter:d4").expect("identity grammar"), EPS_REFUSAL_LINEAR)).transpose().unwrap(),
        Some(SketchConstraintDefinitionInput::Distance { entities, .. })
            if entities.len() == 2
    ));

    let tangent_radius = parse_design_parameter_record(&parameter_record(
        Some(45),
        "1.27",
        "Tangent Dimension-2",
        Some("cm"),
        "d16",
        1.27,
    ))
    .expect("synthetic tangent radius dimension");
    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        crate::design::dimensions::presentation_dimension_definition(
            ctx,
            "stream",
            &frame(vec![operand(796)]),
            &projected,
            &tangent_radius,
            &cadmpeg_ir::features::ParameterId::mint("synthetic:test:id#parameter:d16")
                .expect("identity grammar"),
            EPS_REFUSAL_LINEAR,
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn tangent_radius_entity_id_refuses_retained_limit() {
    fixture(
        "f3d tangent radius entity id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn tangent_radius_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d tangent radius parameter id",
        ResourceDimension::RetainedBytes,
    );
}
