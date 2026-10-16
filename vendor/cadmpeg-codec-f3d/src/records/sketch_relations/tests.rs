// SPDX-License-Identifier: Apache-2.0

mod constraint_state;

use crate::records::sketch_relations::SketchRelation;

#[test]
fn sketch_relation_member_constructor_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::SketchRelationMembers::from_indices(&ctx, [(7, 1, 0)]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "admit F3D sketch relation members")
    );
}

#[test]
fn sketch_relation_return_member_constructor_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::SketchRelationReturnMembers::from_indices(&ctx, [(7, 1)]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "admit F3D sketch relation return members")
    );
}

#[test]
fn sketch_relation_member_resolution_refuses_collection_limit() {
    let mut members = crate::test_support::with_decode_context(|ctx| {
        super::SketchRelationMembers::from_indices(ctx, [(7, 1, 0)]).unwrap()
    });
    let unchanged = members.clone();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = members
        .resolve(&ctx, |record_index| super::SketchRelationOperand::Record {
            record_index,
        })
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "resolve F3D sketch relation members")
    );
    assert_eq!(members, unchanged);
}

#[test]
fn sketch_relation_return_member_resolution_refuses_collection_limit() {
    let mut members = crate::test_support::with_decode_context(|ctx| {
        super::SketchRelationReturnMembers::from_indices(ctx, [(7, 1)]).unwrap()
    });
    let unchanged = members.clone();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = members
        .resolve(&ctx, |record_index| super::SketchRelationOperand::Record {
            record_index,
        })
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "resolve F3D sketch relation return members")
    );
    assert_eq!(members, unchanged);
}

fn charged_relation_error(
    wire: super::SketchRelationSerde,
    max_collection_items: u64,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    SketchRelation::from_wire_charged(&ctx, wire).unwrap_err()
}

#[test]
fn sketch_relation_auxiliary_run_refuses_collection_limit() {
    let mut wire: super::SketchRelationSerde = serde_json::from_str(RELATION_WIRE).unwrap();
    wire.auxiliary_references.push(9);
    wire.auxiliary_reference_offsets.push(12);
    let error = charged_relation_error(wire, 0);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "admit sketch relation auxiliary references"
    ));
}

#[test]
fn sketch_relation_nonempty_resolution_run_refuses_collection_limit() {
    let resolved = RELATION_WIRE.replace(
        r#""resolved_members":[]"#,
        r#""resolved_members":[{"kind":"point","record_index":1,"persistent_id":10},{"kind":"record","record_index":2}]"#,
    );
    let wire: super::SketchRelationSerde = serde_json::from_str(&resolved).unwrap();
    let error = charged_relation_error(wire, 1);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "admit sketch relation resolutions"
    ));
}

#[test]
fn sketch_relation_ordinal_run_refuses_collection_limit() {
    let wire: super::SketchRelationSerde = serde_json::from_str(RELATION_WIRE).unwrap();
    let error = charged_relation_error(wire, 2);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "admit sketch relation ordinals"
    ));
}

#[test]
fn sketch_relation_member_run_refuses_collection_limit() {
    let wire: super::SketchRelationSerde = serde_json::from_str(RELATION_WIRE).unwrap();
    let error = charged_relation_error(wire, 4);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "admit sketch relation members"
    ));
}

#[test]
fn sketch_relation_return_member_run_refuses_collection_limit() {
    let wire: super::SketchRelationSerde = serde_json::from_str(RELATION_WIRE).unwrap();
    let error = charged_relation_error(wire, 8);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "admit sketch relation return members"
    ));
}

#[test]
fn sketch_relation_offset_padding_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut wire: super::SketchRelationSerde = serde_json::from_str(RELATION_WIRE).unwrap();
    wire.member_offsets.clear();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = SketchRelation::from_wire_charged(&ctx, wire).unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "pad sketch relation offsets"
    ));
}

#[test]
fn sketch_relation_resolution_padding_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let wire: super::SketchRelationSerde = serde_json::from_str(RELATION_WIRE).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = SketchRelation::from_wire_charged(&ctx, wire).unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "pad sketch relation resolutions"
    ));
}

#[test]
fn native_rectangular_pattern_refuses_nonunit_direction() {
    let value = serde_json::json!({
        "evaluated_count": 2,
        "count_parameter": 3,
        "direction": [2.0, 0.0, 0.0],
        "evaluated_distance": 1.0,
        "distance_parameter": 4
    });
    assert!(
        serde_json::from_value::<crate::records::sketch_relations::SketchPatternDirection>(value)
            .is_err()
    );
}

const RELATION_WIRE: &str = r#"{"id":"relation","record_index":1,"class_tag":"000","byte_offset":0,"state_offset":0,"owner_reference":1,"owner_entity_id":"owner","auxiliary_references":[],"auxiliary_reference_offsets":[],"members":[1,2],"resolved_members":[],"member_offsets":[25,40],"owner_reference_offset":0,"state":0,"constraint_kinds":["coincident"],"unknown_constraint_bits":0,"member_relation_ordinals":[3,5],"entity_genesis":null,"pattern":null,"return_members":[2,1],"resolved_return_members":[],"return_member_offsets":[60,75],"raw_bytes":"0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"}"#;

#[test]
fn sketch_relation_borrowed_wire_matches_owned_wire_bytes() {
    let resolved = RELATION_WIRE.replace(
        r#""resolved_members":[]"#,
        r#""resolved_members":[{"kind":"point","record_index":1,"persistent_id":10},{"kind":"record","record_index":2}]"#,
    ).replace(
        r#""resolved_return_members":[]"#,
        r#""resolved_return_members":[{"kind":"record","record_index":2},{"kind":"point","record_index":1,"persistent_id":10}]"#,
    );
    for wire in [RELATION_WIRE, resolved.as_str()] {
        let relation: SketchRelation = serde_json::from_str(wire).unwrap();
        let owned = super::SketchRelationSerde::from(relation.clone());
        assert_eq!(
            serde_json::to_vec(&relation).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn sketch_relation_native_retained_limit_refuses_before_clone() {
    let mut relation: SketchRelation = serde_json::from_str(RELATION_WIRE).unwrap();
    relation.id = "f3d:native:sketch-relation#0".into();
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &relation,
        "sketch_relations",
        || super::SKETCH_RELATION_CLONE_COUNT.with(|count| count.set(0)),
        || super::SKETCH_RELATION_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn sketch_relation_runs_preserve_wire_and_reject_conflicting_resolved_indices() {
    let base = RELATION_WIRE;
    let resolved = base.replace(r#""resolved_members":[]"#, r#""resolved_members":[{"kind":"point","record_index":1,"persistent_id":10},{"kind":"record","record_index":2}]"#)
        .replace(r#""resolved_return_members":[]"#, r#""resolved_return_members":[{"kind":"record","record_index":2},{"kind":"point","record_index":1,"persistent_id":10}]"#);
    for wire in [base, resolved.as_str()] {
        let relation: SketchRelation = serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_string(&relation).unwrap(), wire);
        assert_eq!(relation.member_indices(), [1, 2]);
        assert_eq!(relation.return_member_indices(), [2, 1]);
    }
    for ordinals in ["[]", "[0,0]"] {
        let wire = base.replace("[3,5]", ordinals);
        let relation: SketchRelation = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&relation).unwrap(), wire);
    }
    let relation: SketchRelation = serde_json::from_str(base).unwrap();
    let mut partial_ordinals = relation.members().to_vec();
    partial_ordinals[0].relation_ordinal = None;
    assert!(
        crate::records::sketch_relations::SketchRelationMembers::try_from(partial_ordinals)
            .is_err()
    );
    for field in ["resolved_members", "resolved_return_members"] {
        let value: serde_json::Value = serde_json::from_str(&resolved).unwrap();
        let mut mismatch = value.clone();
        mismatch[field][0]["record_index"] = serde_json::json!(3);
        assert!(serde_json::from_value::<SketchRelation>(mismatch)
            .unwrap_err()
            .to_string()
            .contains(field));
        let mut partial = value;
        partial[field].as_array_mut().unwrap().pop();
        assert!(serde_json::from_value::<SketchRelation>(partial)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
}

#[test]
fn sketch_relation_owner_preserves_absence_and_nonempty_ids_on_wire_round_trip() {
    assert!(cadmpeg_core::text::NonBlankString::new("").is_none());
    for (owner, field) in [
        (None, r#""owner_entity_id":"""#),
        (Some("owner"), r#""owner_entity_id":"owner""#),
        (Some(" owner "), r#""owner_entity_id":" owner ""#),
    ] {
        let mut relation: SketchRelation = serde_json::from_str(RELATION_WIRE).unwrap();
        relation.owner_entity_id =
            owner.map(|id| cadmpeg_core::text::NonBlankString::new(id).unwrap());
        let expected = RELATION_WIRE.replace(r#""owner_entity_id":"owner""#, field);
        let wire = serde_json::to_string(&relation).unwrap();
        assert_eq!(wire, expected);
        assert_eq!(
            serde_json::from_str::<SketchRelation>(&wire).unwrap(),
            relation
        );
    }
}

#[test]
fn sketch_relation_admission_bounds_every_reference_and_preserves_failed_edits() {
    let relation: SketchRelation = serde_json::from_str(RELATION_WIRE).unwrap();
    for field in [
        "member_offsets",
        "auxiliary_reference_offsets",
        "owner_reference_offset",
        "return_member_offsets",
    ] {
        for offset in [77_u32, u32::MAX] {
            let mut wire = serde_json::to_value(&relation).unwrap();
            if field == "owner_reference_offset" {
                wire[field] = serde_json::json!(offset);
            } else if field == "auxiliary_reference_offsets" {
                wire["auxiliary_references"] = serde_json::json!([1]);
                wire[field] = serde_json::json!([offset]);
            } else {
                wire[field][0] = serde_json::json!(offset);
            }
            assert!(serde_json::from_value::<SketchRelation>(wire)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
    }
    let mut edited = relation.clone();
    assert!(edited
        .try_edit(|draft| draft.raw_bytes.truncate(23))
        .is_err());
    assert_eq!(edited, relation);
    assert!(edited
        .try_edit(|draft| draft.auxiliary_references =
            crate::records::identity::ReferenceRun::unlocated(vec![1]))
        .is_err());
    assert_eq!(edited, relation);
    edited
        .try_edit(|draft| draft.owner_reference_offset = 76)
        .unwrap();
    assert_eq!(edited.owner_reference_offset(), 76);
    let mut draft = relation.into_draft();
    draft.members.0[0].offset = 77;
    assert!(SketchRelation::try_new(draft).is_err());
}

#[test]
fn sketch_relation_wire_entry_uses_default_limits() {
    let normal: super::SketchRelationSerde = serde_json::from_str(RELATION_WIRE).unwrap();
    assert!(SketchRelation::try_from(normal).is_ok());
    let mut oversized: super::SketchRelationSerde = serde_json::from_str(RELATION_WIRE).unwrap();
    let count = usize::try_from(
        cadmpeg_core::decode::DecodePolicy::default()
            .limits
            .max_collection_items,
    )
    .unwrap()
        + 1;
    oversized.members = vec![1; count];
    oversized.member_offsets.clear();
    let error: super::SketchRelationPayloadError = {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::default(),
        )
        .unwrap();
        super::SketchRelationPayloadError(
            SketchRelation::from_wire_charged(&ctx, oversized)
                .unwrap_err()
                .to_string(),
        )
    };
    // The padded offsets are `u32` slots. Their backing bytes are admitted
    // before the slots. An empty input allows the 16 MiB retained base
    // allowance, which four bytes per default collection item exceed.
    let expected = cadmpeg_core::CodecError::ResourceLimit(cadmpeg_core::decode::ResourceLimit {
        dimension: cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        reason: cadmpeg_core::decode::ResourceFailure::BudgetExceeded,
        limit: 16 * 1024 * 1024,
        used: 0,
        additional: u64::try_from(count * std::mem::size_of::<u32>()).unwrap(),
        operation: "pad sketch relation offsets",
    });
    assert_eq!(error.to_string(), expected.to_string());
}
