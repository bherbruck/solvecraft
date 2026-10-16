// SPDX-License-Identifier: Apache-2.0
use super::{
    DesignDimensionAnnotationFrame, DesignDimensionAnnotationOperand, HashMap, ParameterId, Point2,
    SketchCurveIdentity, SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition,
    SketchId,
};
use cadmpeg_core::decode::ResourceDimension;
const EPS_REFUSAL_LINEAR: f64 = 1.0e-6;

fn fixture(operation: &'static str, dimension: ResourceDimension) {
    let stream = "f3d:Design/BulkStream.dat";
    let sketch = SketchId::mint("generated:test:sketch#offset").unwrap();
    let source_curve_id = format!("{stream}:sketch-curve#10");
    let result_curve_id = format!("{stream}:sketch-curve#11");
    let curve = |id: String, record_index, primary_id, secondary_id| SketchCurveIdentity {
        id,
        record_index,
        owner_reference: Some(100),
        class_tag: crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(primary_id).unwrap(),
        secondary_id,
        geometry: None,
    };
    let source_curve = curve(source_curve_id.clone(), 10, 20, 0);
    let result_curve = curve(result_curve_id.clone(), 11, 21, 7);
    let entity = |id: &str, native_ref: String, start, end| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).unwrap(),
        )
        .with_native_ref(Some(native_ref))
    };
    let source = entity(
        "synthetic:test:id#source",
        source_curve_id,
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
    );
    let result = entity(
        "synthetic:test:id#result",
        result_curve_id,
        Point2::new(0.0, -2.0),
        Point2::new(10.0, -2.0),
    );
    let parameter = crate::records::parameters::DesignParameter::try_from(
        crate::records::parameters::DesignParameterDraft {
            id: format!("{stream}:design-parameter#12"),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned())
                .unwrap(),
            record_index: 12,
            source_ordinal: 0,
            source: crate::records::parameters::DesignParameterSource::new(
                "Linear Dimension-2".into(),
                Some(13),
                Some(crate::records::identity::Located {
                    value: crate::records::parameters::DesignParameterDiscriminator::Code6,
                    offset: 22,
                }),
            )
            .unwrap(),
            expression: "2 mm".into(),
            expression_offset: 40,
            source_kind_offset: 60,

            unit: Some(crate::records::identity::RecordedValue {
                value: "mm".into(),
                offset: 70,
            }),
            name: "d1".into(),
            name_offset: 80,
            evaluated_value: 0.2,
            evaluated_value_offset: 90,
        },
    )
    .unwrap();
    let frame = crate::test_support::with_decode_context(|ctx| {
        DesignDimensionAnnotationFrame::try_new_charged(
            ctx,
            crate::records::dimensions::DesignDimensionAnnotationFrameDraft {
                id: format!("{stream}:design-dimension-annotation-frame#14"),
                companion_record_index: Some(15),
                governing_companion_record_index: 15,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned())
                    .unwrap(),
                record_index: 14,
                frame_length: 100,
                operands: vec![
                    DesignDimensionAnnotationOperand {
                        geometry_record_index: std::num::NonZeroU32::new(0),
                        geometry_reference_offset: 25,
                        role: 3,
                        role_offset: 35,
                    },
                    DesignDimensionAnnotationOperand {
                        geometry_record_index: std::num::NonZeroU32::new(10),
                        geometry_reference_offset: 40,
                        role: 2,
                        role_offset: 50,
                    },
                ],
                entity_genesis: 0x80,
                annotation_bytes: Vec::new(),
                annotation_byte_offset: 111,
                governing_owner_record_index: 13,
                governing_owner_reference_offset: 112,
                return_members: vec![crate::records::identity::Located {
                    value: std::num::NonZeroU32::new(10).unwrap(),
                    offset: 127,
                }],
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "256".to_owned(),
                )
                .unwrap(),
                paired_byte_offset: 100,
                owner_reference: 100,
                owner_reference_offset: 120,
            },
        )
    })
    .unwrap();
    let parameter_id =
        ParameterId::mint("generated:test:parameter#offset").expect("identity grammar");
    let projected = HashMap::from([((stream, 10), &source), ((stream, 11), &result)]);

    super::super::assert_dimension_refusal(operation, dimension, |ctx| {
        crate::design::dimensions::annotation_offset_dimension_definition(
            ctx,
            &frame,
            (&parameter, &parameter_id),
            stream,
            &[source_curve.clone(), result_curve.clone()],
            &projected,
            EPS_REFUSAL_LINEAR,
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn annotation_offset_index_refuses_collection_limit() {
    fixture(
        "f3d annotation offset index",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn annotation_offset_source_id_refuses_retained_limit() {
    fixture(
        "f3d annotation offset source id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn annotation_offset_result_id_refuses_retained_limit() {
    fixture(
        "f3d annotation offset result id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn annotation_offset_parameter_id_refuses_retained_limit() {
    fixture(
        "f3d annotation offset parameter id",
        ResourceDimension::RetainedBytes,
    );
}
