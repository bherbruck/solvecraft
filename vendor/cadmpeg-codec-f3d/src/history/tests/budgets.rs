// SPDX-License-Identifier: Apache-2.0
//! History resource-budget unit tests.
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::u64_from_index;

use crate::history::{
    history_topology_work_budget_exceeded, HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY,
};

#[test]
fn face_operand_recipe_index_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let recipe = crate::records::recipes::ConstructionRecipe {
        id: "recipe".into(),
        byte_offset: 0,
        kind: crate::records::recipes::ConstructionRecipeKind::Face,
        design: None,
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 1,
            offset: 0,
        }),
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = crate::history::bind_face_operand_history_candidates(
        &ctx,
        &mut [],
        &[],
        &[],
        &[recipe],
        &[],
        &std::collections::HashMap::new(),
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D face operand recipe records")
    );
}

fn one_delta_state() -> Vec<u8> {
    let mut bytes = super::super::DELTA.to_vec();
    for (tag, value) in [
        (0x04, 1_i32),
        (0x04, 1),
        (0x04, 0),
        (0x0c, -1),
        (0x0c, -1),
        (0x0c, 0),
        (0x0c, -1),
        (0x0c, 0),
    ] {
        bytes.push(tag);
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0x0b, 0x11]);
    bytes
}

fn one_board_state() -> Vec<u8> {
    let mut bytes = one_delta_state();
    bytes.pop();
    for (tag, value) in [
        (0x04, 1_i32),
        (0x0c, 0),
        (0x04, 1),
        (0x04, 1),
        (0x0c, -1),
        (0x0c, 2),
        (0x04, 0),
        (0x04, 0),
    ] {
        bytes.push(tag);
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0x11);
    bytes
}

fn history_id_lengths() -> (u64, u64, u64, u64) {
    let history = crate::ids::native_scoped_id("history", "asm-history", format_args!("{:010}", 0));
    let state =
        crate::ids::native_scoped_id("history", "asm-delta-state", format_args!("{:010}", 0));
    let board = crate::ids::native_scoped_id("history", "asm-bulletin-board", "0000000000:000000");
    let change =
        crate::ids::native_scoped_id("history", "asm-entity-change", "0000000000:000000:000000");
    (
        u64_from_index(history.len()),
        u64_from_index(state.len()),
        u64_from_index(board.len()),
        u64_from_index(change.len()),
    )
}

fn history_record_with_limits(
    bytes: &[u8],
    policy: &cadmpeg_core::decode::DecodePolicy,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, policy).unwrap();
    super::super::decode_history_records(
        &ctx,
        bytes,
        0,
        None,
        "history",
        "state",
        cadmpeg_asm::kernel_header::RefWidth::Four,
    )
    .unwrap_err()
}

fn one_framed_history_record() -> Vec<u8> {
    let mut bytes = b"\x0d\x01x\x0c".to_vec();
    bytes.extend_from_slice(&3_i32.to_le_bytes());
    bytes.push(0x11);
    bytes
}

fn one_state_history() -> crate::history_records::AsmHistory {
    use crate::history_records::{AsmDeltaState, AsmHistory, AsmTopologyCache};

    AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![AsmDeltaState {
            id: "state".into(),
            parent: "history".into(),
            byte_offset: 0,
            state_id: 0,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 0,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            entity_versions: Vec::new(),
            topology_cache: AsmTopologyCache::Absent,
            transition: None,
        }],
    }
}

fn one_archived_state() -> crate::history_records::AsmHistory {
    use crate::history_records::{AsmHistoryRecord, AsmHistoryRecordFraming};

    let mut history = one_state_history();
    history.states[0].records.push(AsmHistoryRecord {
        id: "record".into(),
        parent: "state".into(),
        revision_id: Some(1),
        byte_offset: 0,
        framing: AsmHistoryRecordFraming::Framed {
            index: 0,
            name: "edge".into(),
            entity_references: Vec::new(),
        },
        raw_bytes: vec![0x11].into(),
    });
    history
}

fn one_insert_only_state() -> crate::history_records::AsmHistory {
    use crate::history_records::{
        AsmBulletinBoard, AsmEntityChange, AsmEntityChangeKind, AsmHistoryRecord,
        AsmHistoryRecordFraming,
    };

    let mut history = one_state_history();
    history.states[0].records.push(AsmHistoryRecord {
        id: "boundary".into(),
        parent: "state".into(),
        revision_id: None,
        byte_offset: 0,
        framing: AsmHistoryRecordFraming::Framed {
            index: 0,
            name: "End-of-ASM-data".into(),
            entity_references: Vec::new(),
        },
        raw_bytes: vec![0x11].into(),
    });
    history.states[0].bulletin_boards.push(AsmBulletinBoard {
        id: "board".into(),
        parent: "state".into(),
        byte_offset: 0,
        owner_ref: 0,
        number: 1,
        changes: vec![AsmEntityChange {
            id: "change".into(),
            parent: "board".into(),
            byte_offset: 0,
            kind: AsmEntityChangeKind::Insert { new: 1 },
        }],
    });
    history
}

fn archive_record() -> cadmpeg_asm::sab::Record {
    cadmpeg_asm::sab::Record {
        index: 0,
        name: "edge".into(),
        tokens: vec![cadmpeg_asm::sab::Token::Ref(-1)].into(),
        offset: 0,
        len: 0,
    }
}

fn archive_error(max_items: u64) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::historical_record_archive(&ctx, &[], &[archive_record()], Default::default())
        .unwrap_err()
}

#[test]
fn history_active_revision_index_refuses_collection_limit() {
    let error = archive_error(0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D active record revisions")
    );
}

#[test]
fn history_active_record_archive_refuses_collection_limit() {
    let error = archive_error(1);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D active record archive")
    );
}

#[test]
fn history_archived_token_copy_refuses_collection_limit() {
    let error = archive_error(2);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D archived record tokens")
    );
}

#[test]
fn history_record_name_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D historical record text",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            super::super::historical_record_archive(
                &ctx,
                &[],
                &[archive_record()],
                Default::default(),
            )
            .map(|_| ())
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        super::super::historical_record_archive(&ctx, &[], &[archive_record()], Default::default())
            .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D historical record text")
    );
}

#[test]
fn history_token_text_copy_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let record = cadmpeg_asm::sab::Record {
        name: String::new(),
        tokens: vec![cadmpeg_asm::sab::Token::Str("x".into())].into(),
        ..archive_record()
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D historical record text",
        |cap| {
            let record = record.clone();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            super::super::historical_record_archive(&ctx, &[], &[record], Default::default())
                .map(|_| ())
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::historical_record_archive(&ctx, &[], &[record], Default::default())
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D historical record text")
    );
}

fn table_error(max_items: u64) -> cadmpeg_core::CodecError {
    use crate::history_records::AsmEntityVersion;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut history = one_state_history();
    history.states[0].entity_versions.push(AsmEntityVersion {
        entity_ref: 0,
        record_ref: 0,
    });
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let archive = std::collections::HashMap::from([(0, archive_record())]);
    super::super::materialize_record_table(&ctx, &history.states[0], &archive).unwrap_err()
}

#[test]
fn history_record_presence_refuses_collection_limit() {
    let error = table_error(0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical record presence")
    );
}

#[test]
fn history_record_table_refuses_collection_limit() {
    let error = table_error(1);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "materialize F3D historical record table")
    );
}

#[test]
fn history_topology_slot_index_refuses_collection_limit() {
    use crate::history_records::AsmHistoricalTopology;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let topology = AsmHistoricalTopology {
        bodies: vec![1],
        faces: vec![2],
        ..Default::default()
    };
    let error = super::super::topology_entity_slots(&ctx, &topology).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical topology slots")
    );
}

fn transition_error(max_items: u64) -> cadmpeg_core::CodecError {
    use crate::history_records::{AsmEntityVersion, AsmHistoricalTopology, AsmTopologyCache};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut history = one_state_history();
    history.states[0].entity_versions.push(AsmEntityVersion {
        entity_ref: 1,
        record_ref: 1,
    });
    history.states[0].topology_cache = AsmTopologyCache::Complete(AsmHistoricalTopology {
        bodies: vec![1],
        ..Default::default()
    });
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::bind_historical_transitions(&ctx, &mut history.states).unwrap_err()
}

macro_rules! historical_transition_limit_test {
    ($name:ident, $limit:expr, $operation:literal) => {
        #[test]
        fn $name() {
            let error = transition_error($limit);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == $operation));
        }
    };
}

historical_transition_limit_test!(
    history_transition_node_index_refuses_limit,
    0,
    "index F3D transition nodes"
);
historical_transition_limit_test!(
    history_transition_vector_refuses_limit,
    1,
    "collect F3D historical transitions"
);
historical_transition_limit_test!(
    history_transition_version_map_refuses_limit,
    2,
    "index F3D transition versions"
);
historical_transition_limit_test!(
    history_transition_version_keys_refuses_limit,
    3,
    "collect F3D transition version keys"
);
historical_transition_limit_test!(
    history_transition_entity_index_refuses_limit,
    4,
    "index F3D current transition entities"
);
historical_transition_limit_test!(
    history_transition_delta_refuses_limit,
    5,
    "collect F3D transition delta"
);

#[test]
fn history_archived_count_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let history = one_archived_state();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::archived_active_record_count(&ctx, &history.states).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D archived revisions")
    );
}

#[test]
fn history_insert_only_count_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let history = one_insert_only_state();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::insert_only_active_record_count(&ctx, &history.states).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D insert-only revisions")
    );
}

fn historical_versions_error(max_items: u64, deletion: bool) -> cadmpeg_core::CodecError {
    use crate::history_records::{AsmBulletinBoard, AsmEntityChange, AsmEntityChangeKind};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut history = one_archived_state();
    if deletion {
        history.states[0].bulletin_boards.push(AsmBulletinBoard {
            id: "board".into(),
            parent: "state".into(),
            byte_offset: 0,
            owner_ref: 0,
            number: 1,
            changes: vec![AsmEntityChange {
                id: "change".into(),
                parent: "board".into(),
                byte_offset: 0,
                kind: AsmEntityChangeKind::Delete { old: 1 },
            }],
        });
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::bind_historical_entity_versions(&ctx, &mut history.states).unwrap_err()
}

macro_rules! historical_versions_limit_test {
    ($name:ident, $limit:expr, $deletion:expr, $operation:literal) => {
        #[test]
        fn $name() {
            let error = historical_versions_error($limit, $deletion);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == $operation));
        }
    };
}

historical_versions_limit_test!(
    history_version_archive_index_refuses_limit,
    0,
    false,
    "index F3D archived revision IDs"
);
historical_versions_limit_test!(
    history_version_node_index_refuses_limit,
    2,
    false,
    "index F3D history node ordinals"
);
historical_versions_limit_test!(
    history_version_seed_refuses_limit,
    3,
    false,
    "seed F3D history versions"
);
historical_versions_limit_test!(
    history_version_visit_refuses_limit,
    4,
    false,
    "visit F3D history version state"
);
historical_versions_limit_test!(
    history_version_state_vector_refuses_limit,
    5,
    false,
    "materialize F3D state versions"
);
historical_versions_limit_test!(
    history_version_projection_index_refuses_limit,
    6,
    false,
    "index F3D state version projections"
);
historical_versions_limit_test!(
    history_version_restore_refuses_limit,
    7,
    true,
    "restore F3D historical version"
);

#[test]
fn history_snapshot_old_references_refuse_collection_limit() {
    use crate::history_records::{AsmBulletinBoard, AsmEntityChange, AsmEntityChangeKind};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut history = one_state_history();
    history.states[0].bulletin_boards.push(AsmBulletinBoard {
        id: "board".into(),
        parent: "state".into(),
        byte_offset: 0,
        owner_ref: 0,
        number: 1,
        changes: vec![AsmEntityChange {
            id: "change".into(),
            parent: "board".into(),
            byte_offset: 0,
            kind: AsmEntityChangeKind::Delete { old: 1 },
        }],
    });
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::bind_snapshot_revision_ids(&ctx, &mut history.states).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D ASM old references")
    );
}

#[test]
fn history_graph_index_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::graph_is_coherent_charged(&ctx, &one_state_history()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D ASM history states")
    );
}

#[test]
fn history_graph_visit_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::graph_is_coherent_charged(&ctx, &one_state_history()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "visit F3D ASM history state")
    );
}

#[test]
fn history_record_references_refuse_collection_limit() {
    let bytes = one_framed_history_record();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 3;
    let error = history_record_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "frame F3D history references")
    );
}

#[test]
fn history_record_vector_refuses_collection_limit() {
    let bytes = one_framed_history_record();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 4;
    let error = history_record_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "frame F3D history record")
    );
}

#[test]
fn history_record_id_refuses_retained_limit() {
    let bytes = one_framed_history_record();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D native record ID",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(history_record_with_limits(&bytes, &policy))
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let error = history_record_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID")
    );
}

#[test]
fn history_record_parent_refuses_retained_limit() {
    let bytes = one_framed_history_record();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();

    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D history record parent",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();

            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(history_record_with_limits(&bytes, &policy))
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let error = history_record_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D history record parent")
    );
}

#[test]
fn opaque_history_record_id_refuses_retained_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D native record ID",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(history_record_with_limits(&[0xff], &policy))
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let error = history_record_with_limits(&[0xff], &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID")
    );
}

#[test]
fn opaque_history_record_parent_refuses_retained_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();

    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy opaque F3D history record parent",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();

            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(history_record_with_limits(&[0xff], &policy))
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let error = history_record_with_limits(&[0xff], &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy opaque F3D history record parent")
    );
}

#[test]
fn opaque_history_error_refuses_retained_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();

    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain opaque F3D history error",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();

            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(history_record_with_limits(&[0xff], &policy))
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let error = history_record_with_limits(&[0xff], &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain opaque F3D history error")
    );
}

#[test]
fn history_board_id_refuses_retained_limit() {
    let bytes = one_board_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let (history, state, _, _) = history_id_lengths();
    policy.limits.max_retained_bytes = history + state;
    let error = decode_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID")
    );
}

#[test]
fn history_change_vector_refuses_collection_limit() {
    let bytes = one_board_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let error = decode_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "admit F3D ASM entity change")
    );
}

#[test]
fn history_change_id_refuses_retained_limit() {
    let bytes = one_board_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let (history, state, board, _) = history_id_lengths();
    policy.limits.max_retained_bytes = history + state + board;
    let error = decode_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID")
    );
}

#[test]
fn history_change_parent_refuses_retained_limit() {
    let bytes = one_board_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();

    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D ASM change parent",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();

            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(decode_with_limits(&bytes, &policy))
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let error = decode_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D ASM change parent")
    );
}

#[test]
fn history_board_vector_refuses_collection_limit() {
    let bytes = one_board_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let error = decode_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "admit F3D ASM bulletin board")
    );
}

#[test]
fn history_board_parent_refuses_retained_limit() {
    let bytes = one_board_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();

    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D ASM board parent",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();

            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(decode_with_limits(&bytes, &policy))
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let error = decode_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D ASM board parent")
    );
}

fn decode_with_limits(
    bytes: &[u8],
    policy: &cadmpeg_core::decode::DecodePolicy,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, policy).unwrap();
    super::super::decode(
        &ctx,
        bytes,
        "history",
        cadmpeg_asm::kernel_header::RefWidth::Four,
        &policy.limits,
    )
    .unwrap_err()
}

#[test]
fn history_id_refuses_retained_limit() {
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let error = decode_with_limits(&[], &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID")
    );
}

#[test]
fn history_state_id_refuses_retained_limit() {
    let bytes = one_delta_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();

    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D native record ID",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();

            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(decode_with_limits(&bytes, &policy))
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let error = decode_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID")
    );
}

#[test]
fn history_state_vector_refuses_collection_limit() {
    let bytes = one_delta_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let error = decode_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "admit F3D ASM delta state")
    );
}

#[test]
fn history_parent_copy_refuses_retained_limit() {
    let bytes = one_delta_state();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();

    policy.limits.max_retained_bytes = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D ASM history parent",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();

            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(decode_with_limits(&bytes, &policy))
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    let error = decode_with_limits(&bytes, &policy);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D ASM history parent")
    );
}

#[test]
fn history_delta_offsets_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let bytes = [super::super::DELTA, super::super::DELTA].concat();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = super::super::decode(
        &ctx,
        &bytes,
        "history",
        cadmpeg_asm::kernel_header::RefWidth::Four,
        &policy.limits,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "f3d history delta offsets"
    ));
}

#[test]
fn history_binding_work_budget_charges_state_record_cross_product() {
    let desktop = cadmpeg_core::decode::ResourceLimits::desktop();
    let desktop_entries = desktop.max_work_units / HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY;
    assert!(!history_topology_work_budget_exceeded(
        [usize::try_from(desktop_entries).expect("desktop entry budget fits usize")],
        &desktop
    ));
    assert!(history_topology_work_budget_exceeded(
        [usize::try_from(desktop_entries + 1).expect("desktop entry overflow fits usize")],
        &desktop
    ));
    assert!(history_topology_work_budget_exceeded(
        [usize::MAX, 1],
        &desktop
    ));

    let service = cadmpeg_core::decode::ResourceLimits::service();
    let service_entries = service.max_work_units / HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY;
    assert!(!history_topology_work_budget_exceeded(
        [usize::try_from(service_entries).expect("service entry budget fits usize")],
        &service
    ));
    assert!(history_topology_work_budget_exceeded(
        [usize::try_from(service_entries + 1).expect("service entry overflow fits usize")],
        &service
    ));
}

#[test]
fn history_complete_table_binding_refuses_materialized_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = super::super::HISTORY_TOPOLOGY_CACHE_BYTES_PER_ENTRY - 1;
    let ctx = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap()
        .0;
    let error = super::super::admit_complete_table_binding_budget(
        &ctx,
        [1_usize].into_iter(),
        &policy.limits,
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref refusal)
        if refusal.operation == "bind F3D complete history topology bytes")
    );
}

#[test]
fn history_complete_table_binding_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY - 1;
    let ctx = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap()
        .0;
    let error = super::super::admit_complete_table_binding_budget(
        &ctx,
        [1_usize].into_iter(),
        &policy.limits,
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref refusal)
        if refusal.operation == "bind F3D complete history topology work")
    );
}
