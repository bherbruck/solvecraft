// SPDX-License-Identifier: Apache-2.0
use crate::design::dimensions::relation_kind_name;
use crate::records::sketch_relations::{
    SketchRelation, SketchRelationDefinition, SketchRelationDraft,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn relation(state: u64) -> SketchRelation {
    SketchRelation::try_new(SketchRelationDraft {
        id: "f3d:test:sketch-relation#1".to_owned(),
        record_index: 1,
        class_tag: "300".to_owned().try_into().unwrap(),
        byte_offset: 0,
        state_offset: 0,
        owner_reference: 1,
        owner_entity_id: None,
        auxiliary_references: crate::records::identity::ReferenceRun::located(Vec::new()),
        rectangular_counted_reference_count: None,
        members: Vec::new().try_into().unwrap(),
        owner_reference_offset: 0,
        definition: SketchRelationDefinition::new(state, None).unwrap(),
        entity_genesis: None,
        return_members: Vec::new().try_into().unwrap(),
        raw_bytes: vec![0; 24],
    })
    .unwrap()
}

#[test]
fn relation_kind_text_refuses_retained_limit_and_preserves_order() {
    let relation = relation(0x8000_0000_0000_0011);
    let expected = "coincident+parallel+unknown_bits";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = u64::try_from(expected.len() - 1).unwrap();

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(relation_kind_name(&relation, &ctx),
        Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::RetainedBytes
            && failure.operation == "f3d sketch constraint native kind"));
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| relation_kind_name(
            &relation, decode_ctx
        ))
        .unwrap(),
        expected
    );
}

#[test]
fn relation_kind_temporary_collection_is_removed() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        relation_kind_name(&relation(0), &ctx).unwrap(),
        "coincident"
    );
    assert_eq!(
        crate::design::relation_kinds::sole_constraint_kind(&relation(0)),
        Some(crate::records::sketch_relations::SketchConstraintKind::Coincident)
    );
    assert!(crate::design::relation_kinds::sole_constraint_kind(&relation(0x11)).is_none());
    assert!(crate::design::relation_kinds::sole_constraint_kind(&relation(1u64 << 63)).is_none());
}
