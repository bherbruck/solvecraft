// SPDX-License-Identifier: Apache-2.0

fn complete_body_topology() -> crate::history_records::AsmHistoricalTopology {
    use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    AsmHistoricalTopology {
        bodies: vec![1],
        regions: vec![2],
        shells: vec![3],
        faces: vec![10],
        body_regions: vec![relation(1, vec![2])],
        region_shells: vec![relation(2, vec![3])],
        shell_faces: vec![relation(3, vec![10])],
        ..Default::default()
    }
}

fn complete_body_error(max_items: u64) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    crate::history::body_index::CompleteBodyIndex::new(&ctx, &complete_body_topology())
        .and_then(|index| index.faces(&ctx, 1))
        .unwrap_err()
}

#[test]
fn complete_body_entity_counts_refuse_collection_limit() {
    let error = complete_body_error(0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D complete body entity counts")
    );
}

#[test]
fn complete_body_relation_owners_refuse_collection_limit() {
    let error = complete_body_error(4);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D complete body relation owners")
    );
}

#[test]
fn complete_body_relation_members_refuse_collection_limit() {
    let error = complete_body_error(5);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D complete body relation members")
    );
}

#[test]
fn complete_body_regions_refuse_collection_limit() {
    let error = complete_body_error(10);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D complete body regions")
    );
}

#[test]
fn complete_body_shells_refuse_collection_limit() {
    let error = complete_body_error(11);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D complete body shells")
    );
}

#[test]
fn complete_body_faces_refuse_collection_limit() {
    let error = complete_body_error(12);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D complete body faces")
    );
}

#[test]
fn complete_body_face_slots_refuse_collection_limit() {
    let error = complete_body_error(13);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D complete body face slots")
    );
}
