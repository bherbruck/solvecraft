// SPDX-License-Identifier: Apache-2.0

#[test]
fn dimension_frame_relation_index_refuses_collection_limit() {
    use crate::records::dimensions::{
        DesignDimensionAnnotationOperand, DesignDimensionLocusPair, DesignDimensionLocusPairDraft,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let pair = DesignDimensionLocusPair::try_new(DesignDimensionLocusPairDraft {
        id: "f3d:test:dimension-pair#1".into(),
        companion_record_index: 12,
        governing_companion_record_index: 12,
        byte_offset: 0,
        class_tag: "277".to_owned().try_into().unwrap(),
        record_index: 13,
        frame_length: 100,
        opaque_index: Some(crate::records::identity::Located {
            value: 0,
            offset: 35,
        }),
        loci: [
            DesignDimensionAnnotationOperand {
                geometry_record_index: std::num::NonZeroU32::new(20),
                geometry_reference_offset: 40,
                role: 0,
                role_offset: 50,
            },
            DesignDimensionAnnotationOperand {
                geometry_record_index: std::num::NonZeroU32::new(21),
                geometry_reference_offset: 55,
                role: 0,
                role_offset: 65,
            },
        ],
        paired_class_tag: "273".to_owned().try_into().unwrap(),
        paired_byte_offset: 100,
    })
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::dimensions::remove_dimension_frame_relations(
            &ctx, &mut Vec::new(), &[pair], &[], &[],
        ),
        Err(CodecError::ResourceLimit(failure))
            if failure.operation == "f3d dimension frame relation index"
                && failure.dimension == ResourceDimension::CollectionItems
    ));
}
