// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::records::dimensions::{
    DesignDimensionAnnotationFrame, DesignDimensionAnnotationFrameDraft,
    DesignDimensionAnnotationOperand, DesignDimensionLocus, DesignDimensionLocusGroup,
    DesignDimensionLocusPair, DesignDimensionLocusPairDraft, DesignDimensionPresentationFrame,
    DesignDimensionRecipeRecord,
};
use crate::records::identity::Located;
use std::num::NonZeroU32;

#[derive(Clone, Copy)]
enum Case {
    Recipe,
    Pair,
    Annotation,
    Presentation,
    Group,
    NullPair,
}

fn pair(has_first: bool) -> DesignDimensionLocusPair {
    let prefix = if has_first { 40 } else { 25 };
    DesignDimensionLocusPair::try_new(DesignDimensionLocusPairDraft {
        id: "f3d:native:locus-pair#0".into(),
        companion_record_index: 2,
        governing_companion_record_index: 3,
        byte_offset: 100,
        class_tag: "256".to_owned().try_into().unwrap(),
        record_index: 4,
        frame_length: 100,
        opaque_index: has_first.then_some(Located {
            value: 7,
            offset: 135,
        }),
        loci: [
            DesignDimensionAnnotationOperand {
                geometry_record_index: has_first.then(|| NonZeroU32::new(7).unwrap()),
                geometry_reference_offset: 100 + prefix,
                role: 1,
                role_offset: 110 + prefix,
            },
            DesignDimensionAnnotationOperand {
                geometry_record_index: NonZeroU32::new(8),
                geometry_reference_offset: 115 + prefix,
                role: 2,
                role_offset: 125 + prefix,
            },
        ],
        paired_class_tag: "257".to_owned().try_into().unwrap(),
        paired_byte_offset: 200,
    })
    .unwrap()
}

fn annotation() -> DesignDimensionAnnotationFrame {
    crate::test_support::with_decode_context(|ctx| {
        DesignDimensionAnnotationFrame::try_new_charged(
            ctx,
            DesignDimensionAnnotationFrameDraft {
                id: "f3d:native:annotation#0".into(),
                companion_record_index: None,
                governing_companion_record_index: 2,
                byte_offset: 100,
                class_tag: "256".to_owned().try_into().unwrap(),
                record_index: 3,
                frame_length: 300,
                operands: [0, 10, 11, 10]
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, index)| DesignDimensionAnnotationOperand {
                        geometry_record_index: NonZeroU32::new(index),
                        geometry_reference_offset: 125 + u64_from_index(ordinal) * 15,
                        role: 1,
                        role_offset: 135 + u64_from_index(ordinal) * 15,
                    })
                    .collect(),
                entity_genesis: 0,
                annotation_bytes: vec![7, 8],
                annotation_byte_offset: 241,
                governing_owner_record_index: 4,
                governing_owner_reference_offset: 244,
                return_members: [10, 10, 11]
                    .into_iter()
                    .enumerate()
                    .map(|(ordinal, index)| Located {
                        value: NonZeroU32::new(index).unwrap(),
                        offset: 259 + u64_from_index(ordinal) * 11,
                    })
                    .collect(),
                paired_class_tag: "259".to_owned().try_into().unwrap(),
                paired_byte_offset: 400,
                owner_reference: 5,
                owner_reference_offset: 420,
            },
        )
    })
    .unwrap()
}

fn group() -> DesignDimensionLocusGroup {
    DesignDimensionLocusGroup {
        id: "f3d:native:locus-group#0".into(),
        companion_record_index: 2,
        byte_offset: 100,
        class_tag: "256".to_owned().try_into().unwrap(),
        record_index: 3,
        frame_length: 200,
        loci: vec![DesignDimensionLocus {
            returned: Located {
                value: 30,
                offset: 140,
            },
            geometry_record_index: 10,
            geometry_reference_offset: 120,
            role: 1,
            role_offset: 124,
        }],
        owner_reference: 4,
        owner_reference_offset: 180,
        owner_role: 1,
        owner_role_offset: 184,
        state: 1,
        state_offset: 188,
        next_class_tag: "259".to_owned().try_into().unwrap(),
        next_record_index: 5,
        next_byte_offset: 300,
    }
}

fn dimension_error(case: Case, max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let mut native = crate::native::F3dNative::default();
        match case {
            Case::Recipe => {
                native
                    .design_dimension_recipe_records
                    .push(DesignDimensionRecipeRecord {
                        id: "f3d:native:recipe#0".into(),
                        companion_record_index: 2,
                        recipe_ordinal: 0,
                        recipe_id: "f3d:native:construction-recipe#0".into(),
                        recipe_kind: crate::records::recipes::ConstructionRecipeKind::Face,
                        byte_offset: 100,
                        class_tag: "256".to_owned().try_into().unwrap(),
                        record_index: 3,
                        frame_length: 11,
                        prefix_offset: 111,
                        prefix_bytes: Vec::new().into(),
                        references: Vec::new(),
                        program_offset: 111,
                        program: Vec::new(),
                        matching_edge_operand_ids: Vec::new(),
                    })
            }
            Case::Pair => {
                native.design_dimension_locus_pairs = vec![pair(true)].try_into().unwrap()
            }
            Case::Annotation => native.design_dimension_annotation_frames.push(annotation()),
            Case::Presentation => {
                native
                    .design_dimension_presentation_frames
                    .push(DesignDimensionPresentationFrame {
                        id: "f3d:native:presentation#0".into(),
                        byte_offset: 100,
                        class_tag: "256".to_owned().try_into().unwrap(),
                        record_index: 3,
                        frame_length: 100,
                        operands: Vec::new(),
                        presentation_bytes: Vec::new().into(),
                        presentation_byte_offset: 124,
                        paired_class_tag: "257".to_owned().try_into().unwrap(),
                        paired_byte_offset: 200,
                        owner_reference: 5,
                        owner_reference_offset: 220,
                        governing_owner_record_index: 4,
                        governing_parameter_record_index: 6,
                        governing_companion_record_index: 2,
                    })
            }
            Case::Group => native.design_dimension_locus_groups.push(group()),
            Case::NullPair => {
                native.design_dimension_null_locus_pairs = vec![pair(false)].try_into().unwrap()
            }
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        let mut findings = Vec::new();
        match case {
            Case::Recipe => {
                super::super::validate_dimension_recipe_records(&ctx, &mut findings).map(|_| ())
            }
            Case::Pair => {
                super::super::validate_dimension_locus_pairs(&ctx, &mut findings).map(|_| ())
            }
            Case::Annotation => {
                super::super::validate_dimension_annotation_frames(&ctx, &mut findings)
            }
            Case::Presentation => {
                super::super::validate_dimension_presentation_frames(&ctx, &mut findings)
            }
            Case::Group => {
                super::super::validate_dimension_locus_groups(&ctx, &mut findings).map(|_| ())
            }
            Case::NullPair => super::super::validate_dimension_null_locus_pairs(
                &ctx,
                &mut findings,
                &Default::default(),
                &Default::default(),
            ),
        }
        .unwrap_err()
    })
}

macro_rules! limit_case {
    ($name:ident, $case:expr, $items:expr, $retained:expr, $operation:literal) => {
        #[test]
        fn $name() {
            let error = if $retained != u64::MAX {
                cadmpeg_test_support::refusal::resource_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                    $operation, |cap| Err::<(), cadmpeg_core::CodecError>(dimension_error($case, $items, cap)))
            } else { dimension_error($case, $items, $retained) };
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == $operation));
        }
    };
}

limit_case!(
    dimension_recipe_invalid_finding_refuses_collection_limit,
    Case::Recipe,
    0,
    u64::MAX,
    "collect F3D native validation findings"
);
limit_case!(
    dimension_recipe_invalid_entity_refuses_retained_limit,
    Case::Recipe,
    u64::MAX,
    0,
    "retain F3D validation entity"
);
limit_case!(
    dimension_locus_pair_index_refuses_collection_limit,
    Case::Pair,
    0,
    u64::MAX,
    "index F3D dimension locus pairs"
);
limit_case!(
    dimension_locus_pair_companion_refuses_collection_limit,
    Case::Pair,
    1,
    u64::MAX,
    "index F3D dimension locus pair companions"
);
limit_case!(
    dimension_annotation_index_refuses_collection_limit,
    Case::Annotation,
    0,
    u64::MAX,
    "index F3D dimension annotation frames"
);
limit_case!(
    dimension_presentation_index_refuses_collection_limit,
    Case::Presentation,
    0,
    u64::MAX,
    "index F3D dimension presentation frames"
);
limit_case!(
    dimension_locus_group_index_refuses_collection_limit,
    Case::Group,
    0,
    u64::MAX,
    "index F3D dimension locus groups"
);
limit_case!(
    dimension_locus_group_companion_refuses_collection_limit,
    Case::Group,
    1,
    u64::MAX,
    "index F3D dimension locus group companions"
);
limit_case!(
    dimension_locus_members_refuse_collection_limit,
    Case::Group,
    2,
    u64::MAX,
    "collect F3D dimension locus members"
);
limit_case!(
    dimension_return_members_refuse_collection_limit,
    Case::Group,
    3,
    u64::MAX,
    "collect F3D dimension return members"
);
limit_case!(
    dimension_null_locus_index_refuses_collection_limit,
    Case::NullPair,
    0,
    u64::MAX,
    "index F3D null-locus dimension pairs"
);
limit_case!(
    dimension_null_locus_companion_refuses_collection_limit,
    Case::NullPair,
    1,
    u64::MAX,
    "index F3D null-locus dimension companions"
);
limit_case!(
    dimension_locus_pair_finding_refuses_collection_limit,
    Case::Pair,
    2,
    u64::MAX,
    "collect F3D native validation findings"
);
limit_case!(
    dimension_locus_pair_entity_refuses_retained_limit,
    Case::Pair,
    u64::MAX,
    0,
    "retain F3D validation entity"
);
limit_case!(
    dimension_annotation_finding_refuses_collection_limit,
    Case::Annotation,
    1,
    u64::MAX,
    "collect F3D native validation findings"
);
limit_case!(
    dimension_annotation_entity_refuses_retained_limit,
    Case::Annotation,
    u64::MAX,
    0,
    "retain F3D validation entity"
);
limit_case!(
    dimension_presentation_finding_refuses_collection_limit,
    Case::Presentation,
    1,
    u64::MAX,
    "collect F3D native validation findings"
);
limit_case!(
    dimension_presentation_entity_refuses_retained_limit,
    Case::Presentation,
    u64::MAX,
    0,
    "retain F3D validation entity"
);
limit_case!(
    dimension_locus_group_finding_refuses_collection_limit,
    Case::Group,
    4,
    u64::MAX,
    "collect F3D native validation findings"
);
limit_case!(
    dimension_locus_group_entity_refuses_retained_limit,
    Case::Group,
    u64::MAX,
    0,
    "retain F3D validation entity"
);
limit_case!(
    dimension_null_locus_finding_refuses_collection_limit,
    Case::NullPair,
    2,
    u64::MAX,
    "collect F3D native validation findings"
);
limit_case!(
    dimension_null_locus_entity_refuses_retained_limit,
    Case::NullPair,
    u64::MAX,
    0,
    "retain F3D validation entity"
);
