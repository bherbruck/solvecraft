// SPDX-License-Identifier: Apache-2.0
//! History-module unit tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use crate::history::{discard_projection_caches, side_one_recipe_edge};
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology, AsmHistory};

#[test]
fn projection_caches_end_after_history_consumers() {
    let transition = crate::history_records::AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: Default::default(),
        topology: Default::default(),
    };
    let state = AsmDeltaState {
        id: "f3d:test:history-state#2".into(),
        parent: "f3d:test:history#0".into(),
        byte_offset: 0,
        state_id: 2,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 2,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: vec![crate::history_records::AsmEntityVersion {
            entity_ref: 3,
            record_ref: 4,
        }],
        topology_cache: crate::history_records::AsmTopologyCache::Complete(
            AsmHistoricalTopology::default(),
        ),
        transition: Some(transition.clone()),
    };
    let mut histories = [AsmHistory {
        id: "f3d:test:history#0".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![state],
    }];

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("decode context");
    discard_projection_caches(&ctx, &mut histories).expect("projection cache budget");

    let state = &histories[0].states[0];
    assert!(histories[0].projection_finalized());
    assert!(state.entity_versions.is_empty());
    assert!(!state.record_table_complete());
    assert!(state.topology().is_none());
    assert_eq!(state.transition, Some(transition));

    let mut native = crate::native::F3dNative {
        asm_histories: histories.to_vec(),
        ..Default::default()
    };
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .expect("store native history");
    native = crate::native::F3dNative::load(&namespace).expect("load native history");
    assert!(native.asm_histories[0].projection_finalized());
}

#[test]
fn side_one_edge_uses_nonzero_references_and_ignores_second_side() {
    let side = |header_value, scalars: Vec<i32>| {
        crate::records::topology::edge_recipe::DesignTopologyRecipeSide {
            header_value,
            scalars,
            payload_prefix: vec![0],

            entries: Vec::new(),
        }
    };
    let structure = crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
        root: 2,
        sides: vec![side(1, vec![0, 2]), side(3, vec![0, 0])],
    };
    let context = |reference_ordinal, shared_edge_slots| {
        crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
            reference_ordinal,
            result_faces: Vec::new(),
            result_face_boundaries: Vec::new(),
            result_shared_edge_slots: Vec::new(),
            preceding_faces: Vec::new(),
            preceding_face_boundaries: Vec::new(),
            preceding_support_face_slots: Vec::new(),
            preceding_support_face_boundaries: Vec::new(),
            shared_edge_slots,
            changed_shared_edge_slots: Vec::new(),
            changed_reference_edge_slots: Vec::new(),
        }
    };
    let contexts = vec![
        context(0, vec![40, 41]),
        context(1, vec![41, 42]),
        context(2, vec![99]),
    ];

    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| side_one_recipe_edge(
            decode_ctx,
            Some(&structure),
            &contexts,
            &[],
            &[40, 41, 42]
        ))
        .unwrap(),
        Some(41)
    );

    let ambiguous_contexts = vec![
        context(0, vec![40, 41]),
        context(1, vec![40, 41]),
        context(2, vec![99]),
    ];
    let selector = crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext {
        selector: 0,
        clauses: vec![None, None],
        incidence_matching_edge_slots: vec![41],

        boundary_count_matching_edge_slots: Vec::new(),
    };
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| side_one_recipe_edge(
            decode_ctx,
            Some(&structure),
            &ambiguous_contexts,
            &[selector],
            &[40, 41, 42]
        ))
        .unwrap(),
        Some(41)
    );
}

#[test]
fn recipe_side_ordinals_refuse_collection_limit() {
    let structure = crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
        root: 2,
        sides: vec![
            crate::records::topology::edge_recipe::DesignTopologyRecipeSide {
                header_value: 1,
                scalars: Vec::new(),
                payload_prefix: Vec::new(),
                entries: Vec::new(),
            },
        ],
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = side_one_recipe_edge(&ctx, Some(&structure), &[], &[], &[]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D recipe side ordinals")
    );
}

#[test]
fn a_history_with_no_states_has_released_nothing_and_is_not_finalized() {
    let document = serde_json::json!({
        "id": "history",
        "byte_offset": 0,
        "states": []
    });
    let history: AsmHistory = serde_json::from_value(document).unwrap();
    assert!(!history.projection_finalized());
    assert!(!crate::history::projection_was_finalized(
        std::slice::from_ref(&history)
    ));
}
