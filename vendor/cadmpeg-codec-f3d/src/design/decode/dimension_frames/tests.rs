// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

use cadmpeg_core::decode::u64_from_index;

use super::{
    contiguous_i32_program, indexed_record_containing, parse_dimension_annotation_frame,
    parse_dimension_presentation_frame, recipe_record_prefix,
};
use crate::design::decode::parameters::parse_design_parameter_record;

use crate::design::test_support::{parameter_record, push_genesis_block, push_reference};
use crate::records::{parameters::DesignParameterOwner, sketch_links::PersistentSubentityTag};
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::ids::{EdgeId, FaceId};

#[test]
fn recipe_reference_candidate_vectors_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let targets = [
        (
            AttributeTarget::Face(FaceId::mint("test:model:face#one").unwrap()),
            1,
        ),
        (
            AttributeTarget::Edge(EdgeId::mint("test:model:edge#one").unwrap()),
            1,
        ),
        (
            AttributeTarget::Face(FaceId::mint("test:model:face#two").unwrap()),
            2,
        ),
        (
            AttributeTarget::Edge(EdgeId::mint("test:model:edge#two").unwrap()),
            2,
        ),
    ];
    for (target, selector) in targets {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut reference = crate::records::dimensions::DesignRecipeReference {
            selector: 1,
            selector_offset: 0,
            token: "13".into(),
            token_offset: 0,
            design_reference: 331,
            design_reference_offset: 0,
            candidate_faces: Vec::new(),
            candidate_edges: Vec::new(),
            alternate_selector_faces: Vec::new(),
            alternate_selector_edges: Vec::new(),
        };
        let tag = PersistentSubentityTag {
            id: "matching".into(),
            target,
            selector,
            token: cadmpeg_core::text::NonBlankString::new("13").unwrap(),
            design_references: vec![331],
            ordinal: 0,
        };
        assert!(matches!(
            super::bind_recipe_reference_candidates_charged(
                &ctx, &mut reference, &[tag], None,
            ),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "f3d recipe reference candidate"
        ));
    }
}
use cadmpeg_ir::math::Point2;

use std::collections::{HashMap, HashSet};

const TEST_LINEAR_TOLERANCE: f64 = 1.0e-6;

#[test]
fn recipe_program_words_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        contiguous_i32_program(&ctx, &1i32.to_le_bytes(), 0, 4),
        Some(Err(CodecError::ResourceLimit(failure)))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d recipe program words"
    ));
}

#[test]
fn recipe_prefix_copies_refuse_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let mut bytes = vec![0; 11];
    bytes.extend_from_slice(&[7, 8, 9]);
    bytes.extend_from_slice(&16u32.to_le_bytes());
    let family_name_offset = bytes.len();
    bytes.extend_from_slice(b"edge_recipe_data");
    let (_, prefix) = recipe_record_prefix(&bytes, 0, family_name_offset, 16).unwrap();
    for operation in [
        "f3d dimension recipe prefix",
        "f3d recipe operand prefix",
        "f3d face operand prefix",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = 2;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            ctx.copy_retained(prefix, operation),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == operation
        ));
    }
}

#[test]
fn dimension_recipe_edge_id_refuses_collection_and_retained_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let id = "f3d:Design/BulkStream.dat:edge-operand#100";
    for (collection_limit, retained_limit, dimension, operation) in [
        (
            0,
            u64::MAX,
            ResourceDimension::CollectionItems,
            "f3d dimension recipe edge IDs",
        ),
        (
            u64::MAX,
            u64::try_from(id.len() - 1).unwrap(),
            ResourceDimension::RetainedBytes,
            "f3d dimension recipe edge ID text",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = collection_limit;
        policy.limits.max_retained_bytes = retained_limit;
        let refusal_cap =
            match cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match dimension {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut ids = Vec::new();
                ctx.push_formatted_retained(
                    &mut ids,
                    format_args!("{}", id),
                    "f3d dimension recipe edge IDs",
                    "f3d dimension recipe edge ID text",
                )
            }) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match dimension {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let refusal_cap =
            match cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match dimension {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut ids = Vec::new();
                ctx.push_formatted_retained(
                    &mut ids,
                    format_args!("{}", id),
                    "f3d dimension recipe edge IDs",
                    "f3d dimension recipe edge ID text",
                )
            }) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match dimension {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ids = Vec::new();
        assert!(matches!(
            ctx.push_formatted_retained(&mut ids, format_args!("{}", id), "f3d dimension recipe edge IDs", "f3d dimension recipe edge ID text"),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation
        ));
        assert!(ids.is_empty());
    }
}

#[test]
fn dimension_recipe_uses_its_immediate_indexed_record_boundary() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut bytes = vec![0xaa; 5];
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"415");
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 17]);
    let recipe_offset = bytes.len();
    bytes.extend_from_slice(b"edge_recipe_data");
    bytes.extend_from_slice(&[0; 13]);
    let next_offset = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"423");
    bytes.extend_from_slice(&41u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 9]);

    assert_eq!(
        indexed_record_containing(&bytes, 5, bytes.len(), recipe_offset),
        Some((5, "415", 40, next_offset))
    );
    assert_eq!(
        indexed_record_containing(&bytes, 5, bytes.len(), next_offset + 11),
        Some((next_offset, "423", 41, bytes.len()))
    );
    assert_eq!(indexed_record_containing(&bytes, 6, bytes.len(), 7), None);
    assert_eq!(
        contiguous_i32_program(&ctx, &[u8::MAX; 8], 0, 8)
            .transpose()
            .unwrap(),
        Some(vec![-1, -1])
    );
    assert_eq!(
        contiguous_i32_program(&ctx, &[0; 7], 0, 7)
            .transpose()
            .unwrap(),
        None
    );

    let mut framed = vec![0; 11];
    framed.extend_from_slice(&[7, 8, 9]);
    framed.extend_from_slice(&16u32.to_le_bytes());
    let family_name_offset = framed.len();
    framed.extend_from_slice(b"edge_recipe_data");
    assert_eq!(
        recipe_record_prefix(&framed, 0, family_name_offset, 16),
        Some((11, &[7, 8, 9][..]))
    );
    framed[14..18].copy_from_slice(&15u32.to_le_bytes());
    assert_eq!(
        recipe_record_prefix(&framed, 0, family_name_offset, 16),
        None
    );
}

fn recipe_reference_limit(
    prefix: &[u8],
    collection_limit: u64,
    retained_limit: u64,
    operation: &str,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::decode::dimension_frames::decode_recipe_references_charged(
            &ctx, prefix, 0,
        ),
        Err(CodecError::ResourceLimit(failure)) if failure.operation == operation
    ));
}

#[test]
fn standard_recipe_references_refuse_nested_items_and_token_text() {
    let mut prefix = vec![0; 10];
    for word in [1u32, 3, 4, 1] {
        prefix.extend_from_slice(&word.to_le_bytes());
    }
    prefix.extend_from_slice(b"7");
    prefix.extend_from_slice(&[0; 4]);
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&331u32.to_le_bytes());
    prefix.extend_from_slice(&[0; 8]);
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            crate::design::decode::dimension_frames::decode_recipe_references_charged(
                ctx, &prefix, 0,
            )
            .expect("recipe references")
        })
        .len(),
        1
    );
    recipe_reference_limit(&prefix, u64::MAX, 0, "f3d recipe reference token");
    recipe_reference_limit(&prefix, 0, u64::MAX, "f3d recipe operand references");
    recipe_reference_limit(&prefix, 1, u64::MAX, "f3d recipe standard references");
}

#[test]
fn paired_recipe_references_refuse_operand_and_flattened_runs() {
    let mut prefix = vec![0; 10];
    for word in [1u32, 2, 1, 1] {
        prefix.extend_from_slice(&word.to_le_bytes());
    }
    prefix.extend_from_slice(b"2");
    prefix.extend_from_slice(&[0; 4]);
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&305u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(b"2");
    prefix.extend_from_slice(&[0; 4]);
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&305u32.to_le_bytes());
    prefix.extend_from_slice(&[0; 4]);
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            crate::design::decode::dimension_frames::decode_recipe_references_charged(
                ctx, &prefix, 0,
            )
            .expect("recipe references")
        })
        .len(),
        2
    );
    recipe_reference_limit(&prefix, 2, u64::MAX, "f3d recipe paired operands");
    recipe_reference_limit(&prefix, 4, u64::MAX, "f3d recipe paired references");
}

#[test]
fn grouped_recipe_references_refuse_output_run() {
    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&4u32.to_le_bytes());
    for selector in 1u32..=4 {
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&selector.to_le_bytes());
        prefix.extend_from_slice(b"2");
        prefix.extend_from_slice(&[0; 4]);
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&(300 + selector).to_le_bytes());
    }
    prefix.extend_from_slice(&[0; 4]);
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            crate::design::decode::dimension_frames::decode_recipe_references_charged(
                ctx, &prefix, 0,
            )
            .expect("recipe references")
        })
        .len(),
        4
    );
    recipe_reference_limit(&prefix, 1, u64::MAX, "f3d recipe grouped references");
}

#[test]
fn dimension_recipe_decodes_ordered_persistent_reference_entries() {
    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&3u32.to_le_bytes());
    prefix.extend_from_slice(&4u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&2u32.to_le_bytes());
    let first_token_at = prefix.len();
    prefix.extend_from_slice(b"13");
    prefix.extend_from_slice(&0u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    let first_reference_at = prefix.len();
    prefix.extend_from_slice(&331u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());

    prefix.extend_from_slice(&2u32.to_le_bytes());
    let second_token_at = prefix.len();
    prefix.extend_from_slice(&[b'9', 0, 0, 0]);
    prefix.push(0);
    prefix.extend_from_slice(&2u32.to_le_bytes());
    let second_reference_at = prefix.len();
    prefix.extend_from_slice(&303u32.to_le_bytes());
    let third_reference_at = prefix.len();
    prefix.extend_from_slice(&304u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());

    let references = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::decode_recipe_references_charged(
            ctx, &prefix, 1_000,
        )
        .expect("recipe references")
    });
    assert_eq!(references.len(), 3);
    assert_eq!(references[0].selector, 1);
    assert_eq!(references[0].selector_offset, 1_022);
    assert_eq!(references[0].token, "13");
    assert_eq!(
        references[0].token_offset,
        1_000 + u64_from_index(first_token_at)
    );
    assert_eq!(references[0].design_reference, 331);
    assert_eq!(
        references[0].design_reference_offset,
        1_000 + u64_from_index(first_reference_at)
    );
    assert_eq!(references[1].selector, 2);
    assert_eq!(references[1].selector_offset, 1_048);
    assert_eq!(references[1].token, "9");
    assert_eq!(
        references[1].token_offset,
        1_000 + u64_from_index(second_token_at)
    );
    assert_eq!(references[1].design_reference, 303);
    assert_eq!(
        references[1].design_reference_offset,
        1_000 + u64_from_index(second_reference_at)
    );
    assert_eq!(references[2].selector, 2);
    assert_eq!(references[2].token, "9");
    assert_eq!(references[2].design_reference, 304);
    assert_eq!(
        references[2].design_reference_offset,
        1_000 + u64_from_index(third_reference_at)
    );
    let suffix_at = prefix.len() - 4;
    prefix.splice(
        suffix_at..,
        [1u32, 1, 0, 0, 2, 401, 402, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes),
    );
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            crate::design::decode::dimension_frames::decode_recipe_references_charged(
                ctx, &prefix, 1_000,
            )
            .expect("recipe references")
        }),
        references
    );
    prefix.extend_from_slice(&[0; 2]);
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            crate::design::decode::dimension_frames::decode_recipe_references_charged(
                ctx, &prefix, 1_000,
            )
            .expect("recipe references")
        }),
        references
    );
    let tags = [
        PersistentSubentityTag {
            id: "matching".into(),
            target: AttributeTarget::Face(
                FaceId::mint("test:model:face#face-b").expect("identity grammar"),
            ),
            selector: 1,
            token: cadmpeg_core::text::NonBlankString::new("13").unwrap(),
            design_references: vec![331],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "other".into(),
            target: AttributeTarget::Face(
                FaceId::mint("test:model:face#face-a").expect("identity grammar"),
            ),
            selector: 1,
            token: cadmpeg_core::text::NonBlankString::new("13").unwrap(),
            design_references: vec![999],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "alternate-face".into(),
            target: AttributeTarget::Face(
                FaceId::mint("test:model:face#face-c").expect("identity grammar"),
            ),
            selector: 2,
            token: cadmpeg_core::text::NonBlankString::new("13").unwrap(),
            design_references: vec![331],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "matching-edge".into(),
            target: AttributeTarget::Edge(
                EdgeId::mint("test:model:edge#edge-b").expect("identity grammar"),
            ),
            selector: 1,
            token: cadmpeg_core::text::NonBlankString::new("13").unwrap(),
            design_references: vec![331],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "alternate-edge".into(),
            target: AttributeTarget::Edge(
                EdgeId::mint("test:model:edge#edge-c").expect("identity grammar"),
            ),
            selector: 2,
            token: cadmpeg_core::text::NonBlankString::new("13").unwrap(),
            design_references: vec![331],
            ordinal: 0,
        },
    ];
    let mut bound = references[0].clone();
    crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::bind_recipe_reference_candidates_charged(
            ctx, &mut bound, &tags, None,
        )
        .expect("recipe references");
    });
    assert_eq!(
        bound.candidate_faces,
        [FaceId::mint("test:model:face#face-b").expect("identity grammar")]
    );
    assert_eq!(
        bound.candidate_edges,
        [EdgeId::mint("test:model:edge#edge-b").expect("identity grammar")]
    );
    assert_eq!(
        bound.alternate_selector_faces,
        [FaceId::mint("test:model:face#face-c").expect("identity grammar")]
    );
    assert_eq!(
        bound.alternate_selector_edges,
        [EdgeId::mint("test:model:edge#edge-c").expect("identity grammar")]
    );
    let stream_tags = [
        PersistentSubentityTag {
            id: "f3d:xref/A/occurrence-0/design:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Face(
                FaceId::mint("test:model:face#face-a").expect("identity grammar"),
            ),
            selector: 1,
            token: cadmpeg_core::text::NonBlankString::new("13").unwrap(),
            design_references: vec![331],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "f3d:xref/B/occurrence-0/design:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Face(
                FaceId::mint("test:model:face#face-b").expect("identity grammar"),
            ),
            selector: 1,
            token: cadmpeg_core::text::NonBlankString::new("13").unwrap(),
            design_references: vec![331],
            ordinal: 0,
        },
    ];
    crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::bind_recipe_reference_candidates_charged(
            ctx,
            &mut bound,
            &stream_tags,
            Some("f3d:xref/A/occurrence-0/Asset/Design1/BulkStream.dat:dimension-recipe#1"),
        )
        .expect("recipe references");
    });
    assert_eq!(
        bound.candidate_faces,
        [FaceId::mint("test:model:face#face-a").expect("identity grammar")]
    );
}

#[test]
fn dimension_recipe_decodes_signed_decimal_reference_tokens() {
    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&3u32.to_le_bytes());
    prefix.extend_from_slice(&4u32.to_le_bytes());

    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&2u32.to_le_bytes());
    prefix.extend_from_slice(b"-2");
    prefix.extend_from_slice(&0u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&301u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());

    prefix.extend_from_slice(&2u32.to_le_bytes());
    prefix.extend_from_slice(b"-1");
    prefix.extend_from_slice(&0u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&304u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());

    let references = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::decode_recipe_references_charged(
            ctx, &prefix, 1_000,
        )
        .expect("recipe references")
    });
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].selector, 1);
    assert_eq!(references[0].token, "-2");
    assert_eq!(references[0].design_reference, 301);
    assert_eq!(references[1].selector, 2);
    assert_eq!(references[1].token, "-1");
    assert_eq!(references[1].design_reference, 304);
}

#[test]
fn face_recipe_decodes_paired_packed_reference_runs() {
    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&2u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    let mut reference_offsets = Vec::new();
    for (ordinal, token) in b"23".iter().copied().enumerate() {
        prefix.extend_from_slice(&1u32.to_le_bytes());
        if ordinal == 0 {
            prefix.push(token);
        } else {
            prefix.extend_from_slice(&1u32.to_le_bytes());
            prefix.push(token);
        }
        prefix.extend_from_slice(&[0; 4]);
        prefix.extend_from_slice(&2u32.to_le_bytes());
        reference_offsets.push(prefix.len());
        prefix.extend_from_slice(&305u32.to_le_bytes());
        prefix.extend_from_slice(&312u32.to_le_bytes());
        if ordinal == 1 {
            prefix.extend_from_slice(&0u32.to_le_bytes());
        }
    }

    let references = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::decode_recipe_references_charged(
            ctx, &prefix, 1_000,
        )
        .expect("recipe references")
    });
    assert!(crate::design::decode::dimension_frames::is_paired_recipe_reference_frame(&prefix));
    assert_eq!(references.len(), 4);
    assert_eq!(
        references
            .iter()
            .map(|reference| (reference.selector, reference.token.as_str()))
            .collect::<Vec<_>>(),
        [(1, "2"), (1, "2"), (1, "3"), (1, "3")]
    );
    assert_eq!(
        references
            .iter()
            .map(|reference| reference.design_reference)
            .collect::<Vec<_>>(),
        [305, 312, 305, 312]
    );
    assert_eq!(
        references[0].design_reference_offset,
        1_000 + u64_from_index(reference_offsets[0])
    );
    assert_eq!(
        references[2].design_reference_offset,
        1_000 + u64_from_index(reference_offsets[1])
    );

    let second_operand_at = reference_offsets[0] + 8;
    let mut packed_second = prefix.clone();
    packed_second.drain(second_operand_at + 4..second_operand_at + 8);
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::decode_recipe_references_charged(
            ctx,
            &packed_second,
            1_000,
        )
        .expect("recipe references")
    })
    .is_empty());
    let mut invalid_header = prefix.clone();
    invalid_header[0] = 1;
    assert!(
        !crate::design::decode::dimension_frames::is_paired_recipe_reference_frame(&invalid_header)
    );

    let mut trailing = prefix.clone();
    trailing.extend_from_slice(&0u32.to_le_bytes());
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::decode_recipe_references_charged(
            ctx, &trailing, 1_000,
        )
        .expect("recipe references")
    })
    .is_empty());
    assert!(!crate::design::decode::dimension_frames::is_paired_recipe_reference_frame(&trailing));

    let mut mismatched_selector = prefix.clone();
    mismatched_selector[second_operand_at..second_operand_at + 4]
        .copy_from_slice(&2u32.to_le_bytes());
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::decode_recipe_references_charged(
            ctx,
            &mismatched_selector,
            1_000,
        )
        .expect("recipe references")
    })
    .is_empty());

    let second_run_at = reference_offsets[1];
    prefix[second_run_at..second_run_at + 4].copy_from_slice(&306u32.to_le_bytes());
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::decode_recipe_references_charged(
            ctx, &prefix, 1_000,
        )
        .expect("recipe references")
    })
    .is_empty());
    assert!(!crate::design::decode::dimension_frames::is_paired_recipe_reference_frame(&prefix));
}

#[test]
fn face_recipe_decodes_five_group_reference_sequence() {
    fn operand(prefix: &mut Vec<u8>, selector: u32, token: &str, references: &[u32]) {
        prefix.extend_from_slice(&selector.to_le_bytes());
        prefix.extend_from_slice(token.as_bytes());
        prefix.extend_from_slice(&[0; 4]);
        prefix.extend_from_slice(
            &u32::try_from(references.len())
                .expect("synthetic reference count")
                .to_le_bytes(),
        );
        for reference in references {
            prefix.extend_from_slice(&reference.to_le_bytes());
        }
    }

    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&5u32.to_le_bytes());
    prefix.extend_from_slice(&2u32.to_le_bytes());
    operand(&mut prefix, 2, "-4", &[401, 402]);
    let first_operand_end = prefix.len();
    operand(&mut prefix, 3, "8", &[501]);

    let second_group_count_at = prefix.len();
    prefix.extend_from_slice(&1u32.to_le_bytes());
    operand(&mut prefix, 4, "-9", &[601, 602]);

    for (selector, token, reference) in [(5, "7", 701), (6, "-1", 801), (7, "0", 901)] {
        prefix.extend_from_slice(&1u32.to_le_bytes());
        operand(&mut prefix, selector, token, &[reference]);
    }
    prefix.extend_from_slice(&0u32.to_le_bytes());

    let references = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::decode_recipe_references_charged(
            ctx, &prefix, 1_000,
        )
        .expect("recipe references")
    });
    assert!(crate::design::decode::dimension_frames::is_grouped_recipe_reference_frame(&prefix));
    assert_eq!(
        references
            .iter()
            .map(|reference| {
                (
                    reference.selector,
                    reference.token.as_str(),
                    reference.design_reference,
                )
            })
            .collect::<Vec<_>>(),
        [
            (2, "-4", 401),
            (2, "-4", 402),
            (3, "8", 501),
            (4, "-9", 601),
            (4, "-9", 602),
            (5, "7", 701),
            (6, "-1", 801),
            (7, "0", 901),
        ]
    );
    assert_eq!(references[0].selector_offset, 1_022);
    assert_eq!(references[0].token_offset, 1_026);
    assert_eq!(references[0].design_reference_offset, 1_036);
    assert_eq!(references[1].design_reference_offset, 1_040);

    let mut wrong_first_group_count = prefix.clone();
    wrong_first_group_count[18..22].copy_from_slice(&3u32.to_le_bytes());
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::decode_recipe_references_charged(
            ctx,
            &wrong_first_group_count,
            1_000,
        )
        .expect("recipe references")
    })
    .is_empty());
    let mut wrong_group_count = prefix.clone();
    wrong_group_count[14..18].copy_from_slice(&4u32.to_le_bytes());
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::decode_recipe_references_charged(
            ctx,
            &wrong_group_count,
            1_000,
        )
        .expect("recipe references")
    })
    .is_empty());
    let mut empty_group = prefix.clone();
    empty_group[second_group_count_at..second_group_count_at + 4]
        .copy_from_slice(&0u32.to_le_bytes());
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::decode_recipe_references_charged(
            ctx,
            &empty_group,
            1_000,
        )
        .expect("recipe references")
    })
    .is_empty());
    let mut length_prefixed_token = prefix.clone();
    length_prefixed_token.splice(26..26, 2u32.to_le_bytes());
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::decode_recipe_references_charged(
            ctx,
            &length_prefixed_token,
            1_000,
        )
        .expect("recipe references")
    })
    .is_empty());
    let mut locally_terminated_operand = prefix.clone();
    locally_terminated_operand.splice(first_operand_end..first_operand_end, 0u32.to_le_bytes());
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::decode_recipe_references_charged(
            ctx,
            &locally_terminated_operand,
            1_000,
        )
        .expect("recipe references")
    })
    .is_empty());
    let mut trailing = prefix.clone();
    trailing.extend_from_slice(&0u32.to_le_bytes());
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::decode_recipe_references_charged(
            ctx, &trailing, 1_000,
        )
        .expect("recipe references")
    })
    .is_empty());
    assert!(!crate::design::decode::dimension_frames::is_grouped_recipe_reference_frame(&trailing));
}

#[test]
fn face_recipe_decodes_dynamic_group_reference_sequence() {
    fn operand(prefix: &mut Vec<u8>, selector: u32, token: &str, reference: u32) {
        prefix.extend_from_slice(&selector.to_le_bytes());
        prefix.extend_from_slice(token.as_bytes());
        prefix.extend_from_slice(&[0; 4]);
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&reference.to_le_bytes());
    }

    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&4u32.to_le_bytes());
    for (selector, token, reference) in [
        (1, "97", 302),
        (2, "88", 302),
        (3, "10", 302),
        (1, "8", 302),
    ] {
        prefix.extend_from_slice(&1u32.to_le_bytes());
        operand(&mut prefix, selector, token, reference);
    }
    prefix.extend_from_slice(&0u32.to_le_bytes());

    let references = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::decode_recipe_references_charged(
            ctx, &prefix, 1_000,
        )
        .expect("recipe references")
    });
    assert!(crate::design::decode::dimension_frames::is_grouped_recipe_reference_frame(&prefix));
    assert_eq!(
        references
            .iter()
            .map(|reference| (
                reference.selector,
                reference.token.as_str(),
                reference.design_reference
            ))
            .collect::<Vec<_>>(),
        [
            (1, "97", 302),
            (2, "88", 302),
            (3, "10", 302),
            (1, "8", 302)
        ]
    );
}

#[test]
fn dimension_recipe_rejects_non_decimal_reference_tokens() {
    for token in [b"-".as_slice(), b"+1", b"1-"] {
        let mut prefix = vec![0; 10];
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&3u32.to_le_bytes());
        prefix.extend_from_slice(&4u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(
            &u32::try_from(token.len())
                .expect("synthetic token length")
                .to_le_bytes(),
        );
        prefix.extend_from_slice(token);
        prefix.extend_from_slice(&0u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&301u32.to_le_bytes());
        prefix.extend_from_slice(&0u32.to_le_bytes());
        prefix.extend_from_slice(&0u32.to_le_bytes());

        assert!(
            crate::test_support::with_decode_context(|ctx| {
                crate::design::decode::dimension_frames::decode_recipe_references_charged(
                    ctx, &prefix, 1_000,
                )
                .expect("recipe references")
            })
            .is_empty(),
            "accepted token {token:?}"
        );
    }
}

#[test]
fn dimension_annotation_frame_links_nullable_loci_to_governing_owner() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"298");
    bytes.extend_from_slice(&388u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    for (reference, role) in [(0u32, 6u32), (354, 2), (376, 3)] {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&role.to_le_bytes());
    }
    push_genesis_block(&mut bytes, 0x202);
    let annotation_byte_offset = bytes.len();
    bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    push_reference(&mut bytes, 390);
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for reference in [376u32, 354] {
        push_reference(&mut bytes, reference);
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&[0; 4]);
    let paired_byte_offset = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"287");
    bytes.extend_from_slice(&388u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    push_reference(&mut bytes, 201);
    bytes.extend_from_slice(&[0; 6]);
    bytes.resize(paired_byte_offset + 59, 0);

    let frame = parse_dimension_annotation_frame(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        0,
        Some(383),
        &HashMap::from([(390, 391)]),
        &HashSet::from([354, 376]),
        &HashSet::from([201]),
    )
    .expect("annotated dimension frame")
    .expect("admitted annotation frame");
    assert_eq!(frame.companion_record_index, Some(383));
    assert_eq!(frame.governing_companion_record_index, 391);
    assert_eq!(frame.entity_genesis, 0x202);
    assert_eq!(
        frame.annotation_byte_offset(),
        u64_from_index(annotation_byte_offset)
    );
    assert_eq!(
        frame.clone().into_draft().annotation_bytes,
        [0xaa, 0xbb, 0xcc]
    );
    assert_eq!(frame.operands()[0].geometry_record_index, None);
    assert_eq!(
        frame
            .clone()
            .into_draft()
            .return_members
            .iter()
            .map(|member| member.value.get())
            .collect::<Vec<_>>(),
        [376, 354]
    );
    assert_eq!(
        frame.paired_byte_offset(),
        u64_from_index(paired_byte_offset)
    );
    assert_eq!(frame.owner_reference, 201);

    let leading = parse_dimension_annotation_frame(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        0,
        None,
        &HashMap::from([(390, 391)]),
        &HashSet::from([354, 376]),
        &HashSet::from([201]),
    )
    .expect("scope-prefix dimension frame")
    .expect("admitted scope-prefix frame");
    assert_eq!(leading.companion_record_index, None);
    assert_eq!(leading.governing_owner_record_index, 390);

    for (items, retained, operation) in [
        (2, u64::MAX, "f3d dimension annotation operands"),
        (3, u64::MAX, "f3d dimension annotation return members"),
        (u64::MAX, 2, "f3d dimension annotation bytes"),
    ] {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = items;
        policy.limits.max_retained_bytes = retained;
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                        .unwrap();
                ((parse_dimension_annotation_frame(
                    &ctx,
                    &bytes,
                    0,
                    Some(383),
                    &HashMap::from([(390, 391)]),
                    &HashSet::from([354, 376]),
                    &HashSet::from([201]),
                ))
                .transpose())
                .map(|_| ())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                        .unwrap();
                ((parse_dimension_annotation_frame(
                    &ctx,
                    &bytes,
                    0,
                    Some(383),
                    &HashMap::from([(390, 391)]),
                    &HashSet::from([354, 376]),
                    &HashSet::from([201]),
                ))
                .transpose())
                .map(|_| ())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            parse_dimension_annotation_frame(
                &ctx, &bytes, 0, Some(383), &HashMap::from([(390, 391)]),
                &HashSet::from([354, 376]), &HashSet::from([201]),
            ),
            Some(Err(cadmpeg_core::CodecError::ResourceLimit(failure)))
                if failure.operation == operation
        ));
    }
}

#[test]
fn dimension_presentation_frame_requires_registered_geometry_and_paired_sketch_header() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"314");
    bytes.extend_from_slice(&332u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for (reference, role) in [(306u32, 1u32), (331, 0)] {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&role.to_le_bytes());
    }
    let presentation_offset = bytes.len();
    bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    let paired_offset = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"281");
    bytes.extend_from_slice(&332u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&270u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 35]);

    let frame = parse_dimension_presentation_frame(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        0,
        "6CCF41D5-40BE-48ED-A834-18F3EAED6C57",
        &HashSet::from([306, 331]),
        &HashSet::from([270]),
        &HashSet::from([281]),
    )
    .expect("direct dimension presentation frame")
    .expect("admitted presentation frame");
    assert_eq!(frame.class_tag.as_str(), "314");
    assert_eq!(frame.record_index, 332);
    assert_eq!(frame.frame_length, u64_from_index(paired_offset));
    assert_eq!(
        frame.presentation_byte_offset,
        u64_from_index(presentation_offset)
    );
    assert_eq!(frame.presentation_bytes, [0xaa, 0xbb, 0xcc]);
    assert_eq!(frame.operands[0].geometry_record_index.get(), 306);
    assert_eq!(frame.operands[1].geometry_record_index.get(), 331);
    assert_eq!(frame.owner_reference, 270);

    assert!(parse_dimension_presentation_frame(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        0,
        "6CCF41D5-40BE-48ED-A834-18F3EAED6C57",
        &HashSet::from([306]),
        &HashSet::from([270]),
        &HashSet::from([281]),
    )
    .is_none());

    for (items, retained, operation) in [
        (1, u64::MAX, "f3d dimension presentation operands"),
        (u64::MAX, 2, "f3d dimension presentation bytes"),
    ] {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = items;
        policy.limits.max_retained_bytes = retained;
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                        .unwrap();
                ((parse_dimension_presentation_frame(
                    &ctx,
                    &bytes,
                    0,
                    super::DIMENSION_PRESENTATION_TYPE_GUID,
                    &HashSet::from([306, 331]),
                    &HashSet::from([270]),
                    &HashSet::from([281]),
                ))
                .transpose())
                .map(|_| ())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                        .unwrap();
                ((parse_dimension_presentation_frame(
                    &ctx,
                    &bytes,
                    0,
                    super::DIMENSION_PRESENTATION_TYPE_GUID,
                    &HashSet::from([306, 331]),
                    &HashSet::from([270]),
                    &HashSet::from([281]),
                ))
                .transpose())
                .map(|_| ())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            parse_dimension_presentation_frame(
                &ctx, &bytes, 0, super::DIMENSION_PRESENTATION_TYPE_GUID,
                &HashSet::from([306, 331]), &HashSet::from([270]),
                &HashSet::from([281]),
            ),
            Some(Err(cadmpeg_core::CodecError::ResourceLimit(failure)))
                if failure.operation == operation
        ));
    }
}

#[test]
fn companion_interval_refuses_foreign_scope_member_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let companion = crate::records::parameters::DesignParameterCompanion::unbound(
        "f3d:native:parameter-companion#11".into(),
        0,
        crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        11,
        10,
        std::num::NonZeroU64::MIN,
        42,
    );
    let scope = crate::records::feature::scope::DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: "f3d:native:parameter-scope#12".into(),
            byte_offset: 80,
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            record_index: 12,
            frame_length: 200,
            kind_offset: 180,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 89,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![55],
                vec![100],
                "reference_members",
            )
            .unwrap(),
            payload: crate::records::feature::scope::DesignFeatureKind::Extrude
                .try_into()
                .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 280,
        }
        .with_fixture_layout(),
    )
    .unwrap();
    let header = crate::records::decal::DesignRecordHeader {
        id: "f3d:native:record-header#55".into(),
        record_index: 55,
        class_tag: crate::records::references::DesignClassTag::try_from("302".to_owned()).unwrap(),
        byte_offset: 70,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let refusal = super::companion_owned_interval(
        &limited,
        &companion,
        std::iter::empty(),
        &[],
        &[scope.clone()],
        std::slice::from_ref(&header),
        100,
    );
    assert!(matches!(
        refusal,
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d companion foreign scope members"
    ));
    let admitted = super::companion_owned_interval(
        &cadmpeg_test_support::service_decode_context(),
        &companion,
        std::iter::empty(),
        &[],
        &[scope],
        &[header],
        100,
    );
    assert_eq!(admitted.unwrap(), Some((58, 70)));
}

#[test]
fn dimension_annotation_interval_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    const STREAM: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let companion = crate::records::parameters::DesignParameterCompanion::unbound(
        format!("{}:parameter-companion#0", crate::ids::native_scope(STREAM)),
        0,
        crate::records::references::DesignClassTag::try_from("408".to_owned()).unwrap(),
        11,
        10,
        std::num::NonZeroU64::MIN,
        42,
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(STREAM, stored).unwrap();
    zip.write_all(&[0; 58]).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let companions = [companion];
        let inputs = super::DimensionDecodeInputs {
            scan,
            placements: &[],
            parameters: &[],
            owners: &[],
            companions: &companions,
            scopes: &[],
            headers: &[],
            points: &[],
            curves: &[],
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;
        let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let refusal = super::decode_dimension_annotation_frames(&limited, &inputs, &[]);
        assert!(matches!(
            refusal,
            Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == "f3d dimension annotation intervals"
        ));
        let admitted = super::decode_dimension_annotation_frames(
            &cadmpeg_test_support::service_decode_context(),
            &inputs,
            &[],
        )
        .unwrap();
        assert!(admitted.is_empty());
    });
}

#[test]
fn dimension_annotation_stream_scan_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    const STREAM: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let companion = crate::records::parameters::DesignParameterCompanion::unbound(
        format!("{}:parameter-companion#0", crate::ids::native_scope(STREAM)),
        0,
        crate::records::references::DesignClassTag::try_from("408".to_owned()).unwrap(),
        11,
        10,
        std::num::NonZeroU64::MIN,
        42,
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(STREAM, stored).unwrap();
    zip.write_all(&[0; 58]).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let companions = [companion.clone(), companion];
        let inputs = super::DimensionDecodeInputs {
            scan,
            placements: &[],
            parameters: &[],
            owners: &[],
            companions: &companions,
            scopes: &[],
            headers: &[],
            points: &[],
            curves: &[],
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_work_units = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            super::decode_dimension_annotation_frames(&ctx, &inputs, &[]),
            Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::WorkUnits
                    && failure.operation == "f3d dimension annotation stream scan"
        ));
    });
}

#[test]
fn dimension_recipe_indexes_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    const STREAM: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut stream_bytes = Vec::new();
    stream_bytes.extend_from_slice(&3u32.to_le_bytes());
    stream_bytes.extend_from_slice(b"415");
    stream_bytes.extend_from_slice(&40u32.to_le_bytes());
    stream_bytes.extend_from_slice(&[0; 10]);
    stream_bytes.extend_from_slice(&16u32.to_le_bytes());
    let recipe_byte_offset = u64::try_from(stream_bytes.len()).unwrap();
    stream_bytes.extend_from_slice(b"edge_recipe_data");
    stream_bytes.extend_from_slice(&0i32.to_le_bytes());
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(STREAM, stored).unwrap();
    zip.write_all(&stream_bytes).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let mut parameter = parse_design_parameter_record(&parameter_record(
            Some(300),
            "40 mm",
            "Linear Dimension-3",
            Some("mm"),
            "d3",
            4.0,
        ))
        .unwrap();
        parameter.id = format!("{}:design-parameter#301", crate::ids::native_scope(STREAM));
        parameter.record_index = 301;
        let owner =
            DesignParameterOwner::try_from(crate::records::parameters::DesignParameterOwnerWire {
                id: format!(
                    "{}:design-parameter-owner#300",
                    crate::ids::native_scope(STREAM)
                ),
                byte_offset: 0,
                frame_length: 104,
                class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned())
                    .unwrap(),
                record_index: 300,
                scope_record_index: 10,
                local_ordinal: 0,
                evaluated_value: 4.0,
                evaluated_value_offset: 40,
                parameter_record_index: 301,
                owned_ordinal: 3,
                variant: Some(0),
                companion_record_index: 302,
            })
            .unwrap();
        let recipe = crate::records::recipes::ConstructionRecipe {
            id: format!("{}:construction-recipe#1", crate::ids::native_scope(STREAM)),
            byte_offset: recipe_byte_offset,
            kind: crate::records::recipes::ConstructionRecipeKind::Edge,
            design: None,
            recipe_index: 0,
            record_index: None,
        };
        for (parameters, owners, recipes, limit, operation) in [
            (
                std::slice::from_ref(&parameter),
                &[][..],
                &[][..],
                0,
                "f3d dimension recipe parameter index",
            ),
            (
                std::slice::from_ref(&parameter),
                std::slice::from_ref(&owner),
                &[][..],
                1,
                "f3d dimension recipe owners",
            ),
            (
                &[][..],
                &[][..],
                std::slice::from_ref(&recipe),
                0,
                "f3d dimension recipe index",
            ),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let refusal = super::decode_dimension_recipe_records(
                &ctx,
                scan,
                parameters,
                owners,
                &[],
                recipes,
            );
            assert!(matches!(
                refusal,
                Err(CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ));
        }
        let companion = crate::records::parameters::DesignParameterCompanion::unbound(
            format!(
                "{}:parameter-companion#302",
                crate::ids::native_scope(STREAM)
            ),
            0,
            crate::records::references::DesignClassTag::try_from("408".to_owned()).unwrap(),
            302,
            300,
            std::num::NonZeroU64::MIN,
            42,
        )
        .bound(crate::records::parameters::DesignCompanionPayload::new(
            0,
            u64::try_from(stream_bytes.len()).unwrap(),
            vec![recipe.id.clone()],
        ));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 4;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let refusal = super::decode_dimension_recipe_records(
            &ctx,
            scan,
            std::slice::from_ref(&parameter),
            std::slice::from_ref(&owner),
            std::slice::from_ref(&companion),
            std::slice::from_ref(&recipe),
        );
        assert!(matches!(
            refusal,
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == "f3d dimension recipe records"
        ));
    });
}

#[test]
fn dimension_locus_lookup_collections_refuse_collection_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(300),
        "40 mm",
        "Linear Dimension-3",
        Some("mm"),
        "d3",
        4.0,
    ))
    .unwrap();
    parameter.id = "f3d:Design/BulkStream.dat:design-parameter#301".into();
    parameter.record_index = 301;
    let parameters = [parameter];
    let owner =
        DesignParameterOwner::try_from(crate::records::parameters::DesignParameterOwnerWire {
            id: "f3d:Design/BulkStream.dat:design-parameter-owner#300".into(),
            byte_offset: 0,
            frame_length: 104,
            class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned())
                .unwrap(),
            record_index: 300,
            scope_record_index: 10,
            local_ordinal: 0,
            evaluated_value: 4.0,
            evaluated_value_offset: 40,
            parameter_record_index: 301,
            owned_ordinal: 3,
            variant: Some(0),
            companion_record_index: 302,
        })
        .unwrap();
    let parameter_index = super::dimension_parameter_index(
        &cadmpeg_test_support::service_decode_context(),
        &parameters,
        "f3d dimension locus parameter index",
    )
    .unwrap();
    let point = crate::records::sketch_geometry::SketchPoint::try_from(
        crate::records::sketch_geometry::SketchPointDraft {
            id: "f3d:Design/BulkStream.dat:sketch-point#0".into(),
            record_index: 20,
            owner_reference: None,
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            byte_offset: 0,
            coordinate_offset: 89,
            companion: crate::records::sketch_geometry::SketchPointCompanion {
                incident_curves: Vec::new(),
            },
            record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
                20,
                crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
                None,
                0.0,
            ),
            paired_reference: 0,
            coordinates: Point2::new(1.0, 2.0),
        },
    )
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let refusal =
        super::dimension_parameter_index(&ctx, &parameters, "f3d dimension locus parameter index");
    assert!(matches!(refusal, Err(CodecError::ResourceLimit(failure))
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d dimension locus parameter index"));
    let companion_arena = DecodeArena::new();
    let (companion_ctx, _) =
        DecodeContext::from_root_bytes(&[], &companion_arena, &policy).unwrap();
    let refusal = super::dimension_companion_keys(
        &companion_ctx,
        std::slice::from_ref(&owner),
        &parameter_index,
        "f3d dimension locus companions",
    );
    assert!(matches!(refusal, Err(CodecError::ResourceLimit(failure))
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d dimension locus companions"));
    let geometry_arena = DecodeArena::new();
    let (geometry_ctx, _) = DecodeContext::from_root_bytes(&[], &geometry_arena, &policy).unwrap();
    let refusal = super::dimension_geometry_indices(
        &geometry_ctx,
        "f3d:Design/BulkStream.dat",
        std::slice::from_ref(&point),
        &[],
    );
    assert!(matches!(refusal, Err(CodecError::ResourceLimit(failure))
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d dimension geometry indices"));
}

mod companion_limits;

mod loci;
