// SPDX-License-Identifier: Apache-2.0
use super::{
    spatial_counted_offset_dimension_definition, Angle, HashMap, Length, ParameterId, Point3,
    SketchNativeOperand, SpatialSketch, SpatialSketchEntity, SpatialSketchEntityId,
    SpatialSketchEntityUse, SpatialSketchGeometry, SpatialSketchGeometryDefinition,
    SpatialSketchId, SpatialSketchProfile, Vector3,
};
use cadmpeg_core::decode::ResourceDimension;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let stream = "f3d:synthetic";
    let sketch_id = SpatialSketchId::mint("synthetic:test:spatial-sketch#offset").unwrap();
    let entity = |record_index, geometry| {
        SpatialSketchEntity::new(
            SpatialSketchEntityId::mint(format!("synthetic:test:spatial-curve#{record_index}"))
                .unwrap(),
            sketch_id.clone(),
            geometry,
        )
        .with_native_ref(Some(format!("{stream}:sketch-curve#{record_index}")))
    };
    let sources = [
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line {
            start: Point3::new(-20.0, 5.0, 8.0),
            end: Point3::new(-15.0, 5.0, 8.0),
        })
        .unwrap(),
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Arc {
            center: Point3::new(30.0, -12.0, 9.0),
            normal: Vector3::new(1.0, 0.0, 0.0),
            reference_direction: Vector3::new(0.0, 1.0, 0.0),
            radius: Length::new(2.0).unwrap(),
            start_angle: Angle::new(0.0).unwrap(),
            end_angle: Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
        })
        .unwrap(),
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Circle {
            center: Point3::new(50.0, 40.0, -7.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            reference_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: Length::new(4.0).unwrap(),
        })
        .unwrap(),
        SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Nurbs {
            curve: cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                &cadmpeg_test_support::service_decode_context(),
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(70.0, -5.0, 3.0), Point3::new(74.0, -2.0, 6.0)],
                None,
                false,
            )
            .expect("fixture constructor admission")
            .unwrap()
            .try_into()
            .unwrap(),
        })
        .unwrap(),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, geometry)| {
        entity(
            u32::try_from(index).expect("fixture value fits u32") + 1,
            geometry,
        )
    })
    .collect::<Vec<_>>();
    let results = [
        (Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)),
        (Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 10.0, 0.0)),
        (Point3::new(10.0, 10.0, 0.0), Point3::new(0.0, 10.0, 0.0)),
        (Point3::new(0.0, 10.0, 0.0), Point3::new(0.0, 0.0, 0.0)),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (start, end))| {
        entity(
            u32::try_from(index).expect("fixture value fits u32") + 11,
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line { start, end })
                .unwrap(),
        )
    })
    .collect::<Vec<_>>();
    let profile = SpatialSketchProfile::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        results
            .iter()
            .map(|entity| SpatialSketchEntityUse {
                entity: entity.id().clone(),
                reversed: false,
            })
            .collect(),
        &cadmpeg_test_support::service_decode_context(),
        "spatial profile uniqueness",
    )
    .expect("fixture collection admission")
    .unwrap();
    let sketch = SpatialSketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        profiles: vec![profile],
        native_ref: None,
    };
    let record_index = |entity: &SpatialSketchEntity| {
        entity
            .native_ref
            .as_deref()
            .and_then(|id| id.rsplit_once('#'))
            .and_then(|(_, index)| index.parse().ok())
            .expect("synthetic record index")
    };
    let mut operands = sources
        .iter()
        .map(|entity| SketchNativeOperand {
            native_kind: cadmpeg_core::text::NonBlankString::new("curve")
                .expect("source operand kind is nonempty"),
            field: Some(cadmpeg_ir::sketches::NativeOperandField {
                name: cadmpeg_core::text::NonBlankString::new("locus")
                    .expect("source field name is nonempty"),
                role: Some(1),
            }),
            object_index: Some(record_index(entity)),
            native_ref: entity.native_ref.clone(),
        })
        .chain(results.iter().map(|entity| {
            SketchNativeOperand {
                native_kind: cadmpeg_core::text::NonBlankString::new("curve")
                    .expect("source operand kind is nonempty"),
                field: Some(cadmpeg_ir::sketches::NativeOperandField {
                    name: cadmpeg_core::text::NonBlankString::new("locus")
                        .expect("source field name is nonempty"),
                    role: Some(0),
                }),
                object_index: Some(record_index(entity)),
                native_ref: entity.native_ref.clone(),
            }
        }))
        .collect::<Vec<_>>();
    operands.push(SketchNativeOperand {
        native_kind: cadmpeg_core::text::NonBlankString::new("record")
            .expect("source operand kind is nonempty"),
        field: Some(cadmpeg_ir::sketches::NativeOperandField {
            name: cadmpeg_core::text::NonBlankString::new("owner")
                .expect("source field name is nonempty"),
            role: Some(0),
        }),
        object_index: Some(100),
        native_ref: Some(format!("{stream}:design-entity#100")),
    });
    operands.extend(sources.iter().zip(&results).flat_map(|(source, result)| {
        [source, result].map(|entity| SketchNativeOperand {
            native_kind: cadmpeg_core::text::NonBlankString::new("curve")
                .expect("source operand kind is nonempty"),
            field: Some(cadmpeg_ir::sketches::NativeOperandField {
                name: cadmpeg_core::text::NonBlankString::new("return")
                    .expect("source field name is nonempty"),
                role: None,
            }),
            object_index: Some(record_index(entity)),
            native_ref: entity.native_ref.clone(),
        })
    }));
    let by_record = sources
        .iter()
        .chain(&results)
        .map(|entity| ((stream, record_index(entity)), entity))
        .collect::<HashMap<_, _>>();
    let parameter = ParameterId::mint("synthetic:test:parameter#offset").expect("identity grammar");

    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        spatial_counted_offset_dimension_definition(
            ctx,
            ("Linear Dimension-1", Some(0x20), &operands),
            (&parameter, 3.0, -3.0),
            &sketch_id,
            std::slice::from_ref(&sketch),
            &by_record,
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn spatial_offset_role_refuses_collection_limit() {
    fixture(
        "f3d spatial offset role",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_offset_result_record_refuses_collection_limit() {
    fixture(
        "f3d spatial offset result record",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_offset_result_identity_refuses_collection_limit() {
    fixture(
        "f3d spatial offset result identity",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_offset_used_source_refuses_collection_limit() {
    fixture(
        "f3d spatial offset used source",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_offset_used_result_refuses_collection_limit() {
    fixture(
        "f3d spatial offset used result",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_counted_offset_source_id_refuses_retained_limit() {
    fixture(
        "f3d spatial counted offset source id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn spatial_offset_source_member_refuses_collection_limit() {
    fixture(
        "f3d spatial offset source member",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_counted_offset_result_id_refuses_retained_limit() {
    fixture(
        "f3d spatial counted offset result id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn spatial_offset_result_member_refuses_collection_limit() {
    fixture(
        "f3d spatial offset result member",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_counted_offset_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d spatial counted offset parameter id",
        ResourceDimension::RetainedBytes,
    );
}
