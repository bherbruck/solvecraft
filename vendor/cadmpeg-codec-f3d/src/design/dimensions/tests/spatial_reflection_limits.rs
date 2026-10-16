// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::sketches::{
    SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
    SpatialSketchGeometryDefinition, SpatialSketchId,
};

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let sketch = SpatialSketchId::mint("synthetic:test:spatial-sketch#reflection").unwrap();
    let entity = |index: u32, geometry| {
        SpatialSketchEntity::new(
            SpatialSketchEntityId::mint(format!("synthetic:test:spatial-entity#{index}")).unwrap(),
            sketch.clone(),
            SpatialSketchGeometry::try_from(geometry).unwrap(),
        )
    };
    let entities = [
        entity(
            1,
            SpatialSketchGeometryDefinition::Point {
                position: Point3::new(-2.0, 0.0, 0.0),
            },
        ),
        entity(
            2,
            SpatialSketchGeometryDefinition::Point {
                position: Point3::new(2.0, 0.0, 0.0),
            },
        ),
        entity(
            3,
            SpatialSketchGeometryDefinition::Line {
                start: Point3::new(0.0, -2.0, 0.0),
                end: Point3::new(0.0, 2.0, 0.0),
            },
        ),
    ];
    let by_record = entities
        .iter()
        .enumerate()
        .map(|(index, entity)| (("stream", u32::try_from(index + 1).unwrap()), entity))
        .collect();
    let operand = |index, name: &str, role| cadmpeg_ir::sketches::SketchNativeOperand {
        native_kind: cadmpeg_core::text::NonBlankString::new("record").unwrap(),
        field: Some(cadmpeg_ir::sketches::NativeOperandField {
            name: cadmpeg_core::text::NonBlankString::new(name).unwrap(),
            role,
        }),
        object_index: Some(index),
        native_ref: None,
    };
    let operands = [
        operand(1, "locus", Some(0)),
        operand(2, "locus", Some(0)),
        operand(3, "locus", Some(5)),
        operand(4, "owner", Some(0x400)),
    ];

    super::assert_dimension_refusal(operation, dimension, |ctx| {
        crate::design::dimensions::spatial_reflection_symmetry(
            ctx,
            "Linear Dimension-6",
            Some(0),
            &operands,
            Some("stream:dimension#1"),
            &sketch,
            &by_record,
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn spatial_reflection_symmetry_first_id_refuses_retained_limit() {
    fixture(
        "f3d spatial reflection symmetry first id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn spatial_reflection_symmetry_second_id_refuses_retained_limit() {
    fixture(
        "f3d spatial reflection symmetry second id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn spatial_reflection_symmetry_axis_id_refuses_retained_limit() {
    fixture(
        "f3d spatial reflection symmetry axis id",
        ResourceDimension::RetainedBytes,
    );
}
