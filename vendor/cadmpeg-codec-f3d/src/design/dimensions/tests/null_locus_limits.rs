// SPDX-License-Identifier: Apache-2.0
use crate::records::dimensions::{
    DesignDimensionAnnotationOperand, DesignDimensionLocusPair, DesignDimensionLocusPairDraft,
};
use crate::records::references::DesignClassTag;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};

fn fixture(operation: &'static str) {
    let pair = DesignDimensionLocusPair::try_new(DesignDimensionLocusPairDraft {
        id: "stream:null-pair#1".into(),
        companion_record_index: 1,
        governing_companion_record_index: 1,
        byte_offset: 0,
        class_tag: DesignClassTag::try_from("277".to_owned()).unwrap(),
        record_index: 2,
        frame_length: 74,
        opaque_index: None,
        loci: [
            DesignDimensionAnnotationOperand {
                geometry_record_index: None,
                geometry_reference_offset: 25,
                role: 14,
                role_offset: 35,
            },
            DesignDimensionAnnotationOperand {
                geometry_record_index: std::num::NonZeroU32::new(3),
                geometry_reference_offset: 40,
                role: 3,
                role_offset: 50,
            },
        ],
        paired_class_tag: DesignClassTag::try_from("273".to_owned()).unwrap(),
        paired_byte_offset: 74,
    })
    .unwrap();
    let entity = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:entity#null-line").unwrap(),
        SketchId::mint("synthetic:test:sketch#null-line").unwrap(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 1.0),
        })
        .unwrap(),
    );
    let parameter = ParameterId::mint("synthetic:test:parameter#null-angle").unwrap();
    super::assert_dimension_refusal(operation, ResourceDimension::RetainedBytes, |ctx| {
        crate::design::dimensions::null_locus_dimension_definition(
            ctx,
            &pair,
            &entity,
            "Angular Dimension-2",
            std::f64::consts::FRAC_PI_4,
            parameter.clone(),
            0.0,
        )
        .transpose()
        .map(|_| ())
    });
}

#[test]
fn null_locus_parameter_id_refuses_retained_limit() {
    fixture("f3d null locus parameter id");
}
#[test]
fn null_locus_entity_id_refuses_retained_limit() {
    fixture("f3d null locus entity id");
}
