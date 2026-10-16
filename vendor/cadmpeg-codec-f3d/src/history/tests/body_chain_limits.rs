// SPDX-License-Identifier: Apache-2.0
//! Decode limits for body revision history chains.

use crate::history_records::{
    AsmDeltaState, AsmHistoricalEntityDelta, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
    AsmHistoricalTransition, AsmHistory, AsmTopologyCache,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn history_fixture() -> AsmHistory {
    let state = |state_id, transition| AsmDeltaState {
        id: format!("f3d:history:state#{state_id}"),
        parent: "f3d:history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        topology_cache: AsmTopologyCache::Complete(AsmHistoricalTopology {
            bodies: vec![7],
            ..Default::default()
        }),
        transition,
    };
    let mut delta = AsmHistoricalTopologyDelta::default();
    delta.bodies.updated.push(7);
    AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![
            state(
                11,
                Some(AsmHistoricalTransition {
                    previous_state_id: Some(10),
                    records: AsmHistoricalEntityDelta::default(),
                    topology: delta,
                }),
            ),
            state(10, None),
        ],
    }
}

fn with_limit<T>(max_items: u64, run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    run(&ctx)
}

#[test]
fn feature_history_state_index_refuses_collection_limit() {
    let history = history_fixture();
    let error = with_limit(0, |ctx| {
        super::super::unique_feature_history_states(ctx, &history)
    })
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D feature history states")
    );
}

#[test]
fn revised_body_chain_visit_refuses_collection_limit() {
    let history = history_fixture();
    let states = std::collections::HashMap::from([
        (10, Some(&history.states[1])),
        (11, Some(&history.states[0])),
    ]);
    let error = with_limit(0, |ctx| {
        super::super::singleton_revised_input_body_across_state_chain(
            ctx,
            &history.states[0],
            10,
            &states,
        )
    })
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "visit F3D revised input body states")
    );
}

#[test]
fn revised_body_index_refuses_collection_limit() {
    let history = history_fixture();
    let states = std::collections::HashMap::from([
        (10, Some(&history.states[1])),
        (11, Some(&history.states[0])),
    ]);
    let error = with_limit(1, |ctx| {
        super::super::singleton_revised_input_body_across_state_chain(
            ctx,
            &history.states[0],
            10,
            &states,
        )
    })
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D revised input bodies")
    );
}

#[test]
fn stable_body_chain_visit_refuses_collection_limit() {
    let history = history_fixture();
    let states = std::collections::HashMap::from([
        (10, Some(&history.states[1])),
        (11, Some(&history.states[0])),
    ]);
    let error = with_limit(0, |ctx| {
        super::super::singleton_body_revision_across_state_chain(
            ctx,
            &history.states[0],
            10,
            &states,
        )
    })
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "visit F3D stable body revision states")
    );
}
