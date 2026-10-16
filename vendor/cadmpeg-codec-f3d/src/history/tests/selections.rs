// SPDX-License-Identifier: Apache-2.0
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

use cadmpeg_core::decode::u64_from_index;

use crate::history::active_brep_face_matches_source;
use crate::history::bind_historical_entity_versions;
use crate::history::bind_snapshot_revision_ids;
use crate::history::body_revision_without_topology_change;
use crate::history::combine_recipe_family_tool_slots;
use crate::history::grouped_reference_face_candidate;
use crate::history::historical_body_slot;
use crate::history::historical_record_archive;
use crate::history::historical_transition;
use crate::history::insert_only_active_record_count;
use crate::history::materialize_record_table;
use crate::history::pattern_combine_tool_slots;
use crate::history::profile_face_group_cardinality_candidates;
use crate::history::selection::bind_edge_identity_history;
use crate::history::selection::bind_hole_selection_history;
use crate::history::selection::complete_compact_edge_treatment_deletions;
use crate::history::selection::entity_selection_face_candidates;
use crate::history::singleton_body_revision_across_state_chain;
use crate::history::singleton_revised_input_body_across_state_chain;
use crate::history::TopologyStableBodyRevision;
use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind, AsmEntityVersion,
    AsmHistoricalCarrierBinding, AsmHistoricalEntityDelta, AsmHistoricalOptionalCarrierBinding,
    AsmHistoricalRelation, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
    AsmHistoricalTransition, AsmHistory, AsmHistoryRecord,
};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use crate::records::topology::edge_identity::DesignEdgeIdentityOperand;
use std::collections::{HashMap, HashSet};

fn with_history_decode_context<T>(
    f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    f(&ctx)
}

#[test]
fn entity_selection_face_proofs_preserve_history_namespaces() {
    let state = |parent: &str, state_id, topology| AsmDeltaState {
        id: format!("{parent}-state-{state_id}"),
        parent: parent.into(),
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
        topology_cache: crate::history_records::AsmTopologyCache::Complete(topology),
        transition: None,
    };
    let history = |id: &str, state| AsmHistory {
        id: id.into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![state],
    };
    let unrelated = history(
        "unrelated",
        state(
            "unrelated",
            1,
            AsmHistoricalTopology {
                points: vec![18044],
                ..AsmHistoricalTopology::default()
            },
        ),
    );
    let selected = history(
        "selected",
        state(
            "selected",
            2,
            AsmHistoricalTopology {
                faces: vec![30],
                loops: vec![20],
                coedges: vec![10],
                pcurves: vec![18044],
                face_loops: vec![AsmHistoricalRelation {
                    owner_ref: 30,
                    member_refs: vec![20],
                }],
                loop_coedges: vec![AsmHistoricalRelation {
                    owner_ref: 20,
                    member_refs: vec![10],
                }],
                coedge_pcurves: vec![AsmHistoricalOptionalCarrierBinding {
                    entity: 10,
                    carrier: Some(18044),
                }],
                ..AsmHistoricalTopology::default()
            },
        ),
    );

    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| entity_selection_face_candidates(
            decode_ctx,
            18044,
            &[unrelated, selected]
        ))
        .unwrap(),
        [
            crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate {
                history_id: "selected".into(),
                historical: crate::records::topology::fillet::HistoricalBinding {
                    kind: AsmHistoricalEntityKind::Pcurve,
                    entity_ref: 18044,
                    state_ids: vec![2],
                },
                face_slot: 30,
            }
        ]
    );
}

#[test]
fn hole_face_selection_history_binds_the_unique_persistent_face() {
    let state = AsmDeltaState {
        id: "selected-state-2".into(),
        parent: "selected".into(),
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
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Complete(AsmHistoricalTopology {
            faces: vec![30],
            loops: vec![20],
            coedges: vec![10],
            pcurves: vec![18044],
            face_loops: vec![AsmHistoricalRelation {
                owner_ref: 30,
                member_refs: vec![20],
            }],
            loop_coedges: vec![AsmHistoricalRelation {
                owner_ref: 20,
                member_refs: vec![10],
            }],
            coedge_pcurves: vec![AsmHistoricalOptionalCarrierBinding {
                entity: 10,
                carrier: Some(18044),
            }],
            ..AsmHistoricalTopology::default()
        }),
        transition: None,
    };
    let history = AsmHistory {
        id: "selected".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![state],
    };
    let face_selection = crate::records::feature::hole::DesignHoleFaceSelection {
        record_index: 100,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("333".to_owned()).unwrap(),
        asset_id: "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA"
            .to_owned()
            .try_into()
            .unwrap(),
        asset_id_offset: 0,
        context_id: "BBBBBBBB-BBBB-4BBB-8BBB-BBBBBBBBBBBB"
            .to_owned()
            .try_into()
            .unwrap(),
        context_id_offset: 0,
        identity_record_index: 103,
        identity_record_offset: 0,
        primary_identity: 18044,
        primary_identity_offset: 0,
        secondary: None,
        historical_face_candidates: Vec::new(),
        next_record_index: 104,
        next_byte_offset: 0,
    };
    let construction = crate::records::feature::hole::DesignHoleConstruction {
        point_record_index: 55,
        point_record_byte_offset: 0,
        position: crate::test_support::reals([0.0; 3]),
        position_offset: 0,
        direction: crate::test_support::reals([0.0, 0.0, 1.0]),
        direction_offset: 0,
        point_parameters: crate::test_support::reals([0.0; 2]),
        point_parameter_offsets: [0, 0],
        reference_type: 0,
        reference_type_offset: 0,
        tangent_point_data: None,
        input_records: vec![crate::records::identity::Located {
            value: 55,
            offset: 0,
        }],
        face_selection: Some(face_selection),
    };
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:scope#42",
        crate::records::feature::scope::DesignFeatureKind::Hole,
        42,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Hole(slot) = scope.payload_mut() {
        *slot = Some(construction);
    }

    crate::test_support::with_decode_context(|decode_ctx| {
        bind_hole_selection_history(decode_ctx, std::slice::from_mut(&mut scope), &[history])
    })
    .unwrap();

    assert_eq!(
        scope
            .hole_construction()
            .and_then(|construction| construction.face_selection.as_ref())
            .map(|selection| selection.historical_face_candidates.as_slice()),
        Some(
            &[
                crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate {
                    history_id: "selected".into(),
                    historical: crate::records::topology::fillet::HistoricalBinding {
                        kind: AsmHistoricalEntityKind::Pcurve,
                        entity_ref: 18044,
                        state_ids: vec![2],
                    },
                    face_slot: 30,
                }
            ][..]
        )
    );
}

#[test]
fn compact_edge_treatment_deletions_require_exact_cardinality() {
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            complete_compact_edge_treatment_deletions(decode_ctx, true, Some(2), &[17, 19])
        })
        .unwrap(),
        [17, 19]
    );
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        complete_compact_edge_treatment_deletions(decode_ctx, true, Some(2), &[17, 18, 19])
    })
    .unwrap()
    .is_empty());
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        complete_compact_edge_treatment_deletions(decode_ctx, false, Some(2), &[17, 19])
    })
    .unwrap()
    .is_empty());
    assert!(crate::test_support::with_decode_context(|decode_ctx| {
        complete_compact_edge_treatment_deletions(decode_ctx, true, None, &[17, 19])
    })
    .unwrap()
    .is_empty());
}

#[test]
fn compact_edge_treatment_deletions_refuse_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        complete_compact_edge_treatment_deletions(&ctx, true, Some(2), &[17, 19]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D compact treatment deletions")
    );
}

#[test]
fn compact_transition_fallback_is_scoped_to_each_operand_group() {
    let scope: crate::records::feature::scope::DesignParameterScope =
        serde_json::from_value(serde_json::json!({
            "id": "f3d:test:scope",
            "byte_offset": 0,
            "class_tag": "300",
            "record_index": 1,
            "frame_length": 200,
            "kind": "Fillet",
            "kind_offset": 32,
            "feature_ordinal": 1,
            "feature_ordinal_offset": 128,
            "history_state_id": 8,
            "history_state_id_offset": 24,
            "previous_history_state_id": 7,
            "previous_history_state_id_offset": 158,
            "reference_count_offset": 9,
            "reference_members": [2],
            "reference_member_offsets": [14],
            "paired_class_tag": "261",
            "paired_byte_offset": 200
        }))
        .expect("Fillet scope");
    let identity = |group_record_index, group_member_ordinal, record_index| {
        serde_json::from_value::<DesignEdgeIdentityOperand>(serde_json::json!({
            "id": format!("f3d:test:identity#{record_index}"),
            "scope_record_index": 1,
            "group_record_index": group_record_index,
            "group_member_ordinal": group_member_ordinal,
            "record_index": record_index,
            "byte_offset": 0,
            "class_tag": "277",
            "compact_layout": true,
            "local_id": record_index,
            "local_id_offset": 23,
            "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
            "asset_id_offset": 41,
            "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e",
            "context_id_offset": 117
        }))
        .expect("edge identity")
    };
    let mut operands = vec![identity(2, 0, 10), identity(2, 1, 11), identity(3, 0, 12)];
    let state = |state_id, topology, transition| AsmDeltaState {
        id: format!("history:state#{state_id}"),
        parent: "history".into(),
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
        topology_cache: crate::history_records::AsmTopologyCache::Complete(topology),
        transition,
    };
    let previous = state(
        7,
        AsmHistoricalTopology {
            edges: vec![17, 19],
            ..AsmHistoricalTopology::default()
        },
        None,
    );
    let current = state(
        8,
        AsmHistoricalTopology::default(),
        Some(AsmHistoricalTransition {
            previous_state_id: Some(7),
            records: AsmHistoricalEntityDelta::default(),
            topology: AsmHistoricalTopologyDelta::default(),
        }),
    );
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![current, previous],
    };

    crate::test_support::with_decode_context(|decode_ctx| {
        bind_edge_identity_history(
            decode_ctx,
            &mut operands,
            &[],
            std::slice::from_ref(&scope),
            std::slice::from_ref(&history),
            &HashMap::from([(scope.id.clone(), history.id.clone())]),
        )
    })
    .unwrap();

    assert_eq!(operands[0].transition_edge_candidates, [17, 19]);
    assert_eq!(operands[1].transition_edge_candidates, [17, 19]);
    assert!(operands[2].transition_edge_candidates.is_empty());
}

#[test]
fn body_selection_proofs_distinguish_stable_and_topology_changing_operations() {
    let state = |state_id, transition| AsmDeltaState {
        id: format!("state-{state_id}"),
        parent: "history".into(),
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
        topology_cache: crate::history_records::AsmTopologyCache::Complete(AsmHistoricalTopology {
            bodies: vec![7],
            ..AsmHistoricalTopology::default()
        }),
        transition,
    };
    let previous = state(10, None);
    let mut transition = AsmHistoricalTransition {
        previous_state_id: Some(10),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    };
    transition.topology.bodies.updated.push(7);
    let current = state(11, Some(transition.clone()));
    assert_eq!(
        body_revision_without_topology_change(&current),
        Some(TopologyStableBodyRevision::Revised(7))
    );

    transition.topology.points.updated.push(31);
    transition.topology.surfaces.inserted.push(32);
    transition.topology.curves.deleted.push(33);
    transition.topology.pcurves.updated.push(34);
    let carrier_revisions = state(11, Some(transition.clone()));
    assert_eq!(
        body_revision_without_topology_change(&carrier_revisions),
        Some(TopologyStableBodyRevision::Revised(7))
    );

    let mut intermediate_transition = AsmHistoricalTransition {
        previous_state_id: Some(10),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    };
    intermediate_transition.topology.bodies.updated.push(7);
    let intermediate = state(11, Some(intermediate_transition));
    transition.previous_state_id = Some(11);
    let result = state(12, Some(transition.clone()));
    let states = HashMap::from([
        (10, Some(&previous)),
        (11, Some(&intermediate)),
        (12, Some(&result)),
    ]);
    assert_eq!(
        singleton_body_revision_across_state_chain(
            &cadmpeg_test_support::service_decode_context(),
            &result,
            10,
            &states
        )
        .unwrap(),
        Some(7)
    );

    let topology = |bodies: &[i64]| AsmHistoricalTopology {
        bodies: bodies.to_vec(),
        ..AsmHistoricalTopology::default()
    };
    let mut split_previous = state(20, None);
    split_previous.topology_cache =
        crate::history_records::AsmTopologyCache::Complete(topology(&[7, 8]));
    let mut split_transition = AsmHistoricalTransition {
        previous_state_id: Some(20),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    };
    split_transition.topology.bodies.updated.push(7);
    split_transition.topology.bodies.inserted.push(9);
    let mut split_result = state(21, Some(split_transition.clone()));
    split_result.topology_cache =
        crate::history_records::AsmTopologyCache::Complete(topology(&[7, 8, 9]));
    let split_states = HashMap::from([(20, Some(&split_previous)), (21, Some(&split_result))]);
    assert_eq!(
        singleton_revised_input_body_across_state_chain(
            &cadmpeg_test_support::service_decode_context(),
            &split_result,
            20,
            &split_states
        )
        .unwrap(),
        Some(7)
    );

    split_transition.topology.bodies.updated.push(8);
    let mut ambiguous_split = state(21, Some(split_transition));
    ambiguous_split.topology_cache =
        crate::history_records::AsmTopologyCache::Complete(topology(&[7, 8, 9]));
    let ambiguous_states =
        HashMap::from([(20, Some(&split_previous)), (21, Some(&ambiguous_split))]);
    assert_eq!(
        singleton_revised_input_body_across_state_chain(
            &cadmpeg_test_support::service_decode_context(),
            &ambiguous_split,
            20,
            &ambiguous_states
        )
        .unwrap(),
        None
    );

    transition.previous_state_id = Some(10);
    transition.topology.faces.updated.push(19);
    let topology_changing = state(11, Some(transition));
    assert_eq!(
        body_revision_without_topology_change(&topology_changing),
        None
    );
}

#[test]
fn pattern_combine_tool_set_requires_target_membership_and_exact_cardinality() {
    let bodies = [2, 4, 5, 6, 7].into_iter().collect();

    with_history_decode_context(|ctx| {
        assert_eq!(
            pattern_combine_tool_slots(ctx, &bodies, 4, 4).unwrap(),
            Some(vec![2, 5, 6, 7])
        );
        assert_eq!(
            pattern_combine_tool_slots(ctx, &bodies, 3, 5).unwrap(),
            None
        );
        assert_eq!(
            pattern_combine_tool_slots(ctx, &bodies, 4, 3).unwrap(),
            None
        );
        assert_eq!(
            pattern_combine_tool_slots(ctx, &bodies, 4, 5).unwrap(),
            None
        );
    });
    assert_eq!(
        historical_body_slot("f3d:history-input:body#80:escaped-feature:35:2"),
        Some(2)
    );
    assert_eq!(historical_body_slot("f3d:brep:entity#2"), None);
}

fn with_combine_collection_limit<T>(
    max_items: u64,
    f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    f(&ctx)
}

#[test]
fn pattern_combine_tool_slots_refuse_collection_limit() {
    let bodies = [2, 4, 5].into_iter().collect();
    let result =
        with_combine_collection_limit(1, |ctx| pattern_combine_tool_slots(ctx, &bodies, 4, 2));
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit { .. })
    ));
}

#[test]
fn combine_recipe_tool_index_refuses_collection_limit() {
    let result = with_combine_collection_limit(0, |ctx| {
        combine_recipe_family_tool_slots(ctx, ("f3d:design", 1), &[1], 1, 0, &[], &[])
    });
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit { .. })
    ));
}

#[test]
fn combine_historical_rows_refuse_collection_limit() {
    let feature = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#combine").unwrap();
    let result = with_combine_collection_limit(0, |ctx| {
        super::super::combine_historical_rows(ctx, &feature, 1, vec![2], vec!["native".into()])
    });
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit { .. })
    ));
}

#[test]
fn combine_member_validation_refuses_collection_limit() {
    let result = with_combine_collection_limit(1, |ctx| {
        let body = cadmpeg_ir::ids::BodyId::mint("test:model:body#member").unwrap();
        let native = cadmpeg_core::text::NonBlankString::new("native").unwrap();
        cadmpeg_ir::features::BodyMembers::try_from_rows(
            vec![cadmpeg_ir::features::BodyMember::new(body, native)],
            ctx,
        )
        .map_err(cadmpeg_core::CodecError::from)
    });
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit { .. })
    ));
}

#[test]
fn combine_recipe_family_proves_unordered_generated_tools() {
    use crate::records::{
        recipes::{ConstructionRecipe, ConstructionRecipeKind, ConstructionRecipeSelector},
        topology::{
            body_recipe::DesignBodyRecipeOperand, body_recipe::DesignBodyRecipeReference,
            body_recipe::DesignOperandOwner,
        },
    };

    let stream = "f3d:Design/BulkStream.dat";
    let recipe = |record_index, design_id: &str, selector| ConstructionRecipe {
        id: format!("{stream}:construction-recipe#{record_index}"),
        byte_offset: 0,
        kind: ConstructionRecipeKind::Body,
        design: Some(crate::records::recipes::ConstructionRecipeDesign {
            id: crate::records::identity::RecordedValue {
                value: design_id.into(),
                offset: 0,
            },
            selector: Some(ConstructionRecipeSelector {
                value: selector,
                byte_offset: 0,
            }),
        }),
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 0,
            offset: 0,
        }),
    };
    let recipes = [
        recipe(101, "exact", 6),
        recipe(102, "family", 3),
        recipe(103, "family", 1),
        recipe(104, "family", 2),
    ];
    let operand = |record_index,
                   recipe: &ConstructionRecipe,
                   body: Option<i64>,
                   candidates: &[i64]| {
        DesignBodyRecipeOperand::try_new(
            crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
                id: format!("{stream}:design-body-recipe-operand#{record_index}"),
                scope_record_index: 10,
                owner: DesignOperandOwner::ScopeReference {
                    scope_reference_ordinal: record_index,
                },
                record_index,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("389".to_owned())
                    .unwrap(),
                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
                )
                .unwrap(),
                asset_id_offset: 56,
                context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
                )
                .unwrap(),
                context_id_offset: 132,
                selector_tail: None,

                references: vec![DesignBodyRecipeReference {
                    design_reference: if recipe
                        .design
                        .as_ref()
                        .map(|design| design.id.value.as_str())
                        == Some("family")
                    {
                        413
                    } else {
                        409
                    },
                    design_reference_offset: 25,
                    form: 3,
                    form_offset: 33,
                    candidate_faces: Vec::new(),
                    preceding_candidate_faces: Vec::new(),
                    preceding_body_slots: candidates.to_vec(),
                }],
                nested_record_index: u64::from(record_index + 3),
                nested_record_index_offset: 38,
                recipe_id: recipe.id.clone(),
                resolved_face_slot: None,
                resolved_body_state_id: body.map(|_| 317),
                resolved_body_slot: body,
                resolved_body_face_slots: Vec::new(),
                next_record_index: record_index + 4,
                next_byte_offset: 256,
            },
        )
        .unwrap()
    };
    let family = [6, 7, 8];
    let mut operands = vec![
        operand(1, &recipes[0], Some(5), &[]),
        operand(2, &recipes[1], None, &[]),
        operand(3, &recipes[2], None, &family),
        operand(4, &recipes[3], None, &[]),
    ];

    with_history_decode_context(|ctx| {
        assert_eq!(
            combine_recipe_family_tool_slots(
                ctx,
                (stream, 10),
                &[1, 2, 3, 4],
                317,
                1,
                &operands,
                &recipes
            )
            .unwrap(),
            Some(vec![5, 6, 7, 8])
        )
    });

    *operands[1]
        .reference_bindings_mut()
        .next()
        .unwrap()
        .preceding_body_slots = vec![6, 7, 9];
    assert!(
        with_history_decode_context(|ctx| combine_recipe_family_tool_slots(
            ctx,
            (stream, 10),
            &[1, 2, 3, 4],
            317,
            1,
            &operands,
            &recipes,
        )
        .unwrap())
        .is_none()
    );

    operands[1]
        .reference_bindings_mut()
        .next()
        .unwrap()
        .preceding_body_slots
        .clear();
    let mut duplicate_selector = recipes.clone();
    duplicate_selector[1].design.as_mut().unwrap().selector =
        duplicate_selector[2].design.as_ref().unwrap().selector;
    assert!(
        with_history_decode_context(|ctx| combine_recipe_family_tool_slots(
            ctx,
            (stream, 10),
            &[1, 2, 3, 4],
            317,
            1,
            &operands,
            &duplicate_selector,
        )
        .unwrap())
        .is_none()
    );
}

fn combine_external_identity(
    occurrence_reference: u64,
) -> crate::records::feature::combine::DesignCombineExternalBodyIdentity {
    crate::records::feature::combine::DesignCombineExternalBodyIdentityWire {
        selector_asset_id: "11111111-1111-4111-8111-111111111111"
            .to_owned()
            .try_into()
            .expect("GUID"),
        selector_asset_id_offset: 44,
        selector_context_id: "22222222-2222-4222-8222-222222222222"
            .to_owned()
            .try_into()
            .expect("GUID"),
        selector_context_id_offset: 120,
        occurrence_reference,
        occurrence_reference_offset: 205,
        external_body_reference: 700,
        external_body_reference_offset: 220,
        external_segment: 2,
        external_segment_offset: 229,
        external_asset_id: "11111111-1111-4111-8111-111111111111"
            .to_owned()
            .try_into()
            .expect("GUID"),
        external_asset_id_offset: 237,
        external_link_name: "component-body-link".into(),
        external_link_name_offset: 314,
        external_property_key: None,
        external_property_key_offset: None,
        external_version_urn: None,
        external_version_urn_offset: None,
        tail_values: [0, 0],
        tail_value_offsets: [359, 371],
    }
    .try_into()
    .unwrap()
}

#[test]
fn combine_external_identity_refuses_retained_limit() {
    let identity = combine_external_identity(500);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = crate::ids::neutral_combine_external_body_id_charged(&ctx, &identity);
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit { .. })
    ));
}

fn combine_external_scope() -> crate::records::feature::scope::DesignParameterScope {
    let identity = combine_external_identity;
    let tool = |record_index, occurrence_reference| {
        crate::records::feature::combine::DesignCombineBodySelection {
            record_index,
            external_identity: Some(identity(occurrence_reference)),
        }
    };
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::Combine,
        10,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Combine(slot) =
        scope.payload_mut()
    {
        *slot = Some(crate::records::feature::combine::DesignCombineOperation {
            form: crate::records::feature::combine::DesignCombineForm::ExtendedReference,
            operation: cadmpeg_ir::features::BooleanKind::Join,
            operation_offset: 0,
            keep_tools: false,
            keep_tools_offset: 0,
            target_record_index: 11,
            tools: crate::records::feature::combine::DesignCombineTools {
                first: tool(12, 500),
                additional: vec![tool(13, 501)],
            },
        });
    }
    scope
}

#[test]
fn combine_external_tools_refuse_collection_limit() {
    let scope = combine_external_scope();
    let result = with_combine_collection_limit(0, |ctx| {
        super::super::combine_external_local_tools(ctx, &scope)
    });
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit { .. })
    ));
}

#[test]
fn combine_external_tools_retain_complete_occurrence_local_identities() {
    use cadmpeg_ir::features::BodySelection;

    let tool = |record_index, occurrence_reference| {
        crate::records::feature::combine::DesignCombineBodySelection {
            record_index,
            external_identity: Some(combine_external_identity(occurrence_reference)),
        }
    };
    let mut scope = combine_external_scope();
    let BodySelection::Local { bodies, native } =
        with_history_decode_context(|ctx| super::super::combine_external_local_tools(ctx, &scope))
            .unwrap()
            .expect("complete local tool identity")
    else {
        panic!("local body selection");
    };
    assert_eq!(bodies.len(), 2);
    assert_ne!(bodies[0], bodies[1]);
    assert_eq!(native.as_str(), scope.id);

    scope
        .combine_operation_mut()
        .expect("Combine operation")
        .tools
        .additional[0] = tool(13, 500);
    assert!(
        with_history_decode_context(|ctx| super::super::combine_external_local_tools(ctx, &scope))
            .unwrap()
            .is_none()
    );
}

#[test]
fn active_brep_face_namespace_accepts_default_or_matching_named_source() {
    use cadmpeg_ir::ids::FaceId;

    assert!(active_brep_face_matches_source(
        &FaceId::mint("f3d:brep:entity#17").expect("identity grammar"),
        "history"
    ));
    assert!(active_brep_face_matches_source(
        &FaceId::mint("f3d:brep/history/brep:entity#17").expect("identity grammar"),
        "history"
    ));
    assert!(!active_brep_face_matches_source(
        &FaceId::mint("f3d:brep/other/brep:entity#17").expect("identity grammar"),
        "history"
    ));
}

#[test]
fn historical_transition_separates_membership_and_revision_changes() {
    let state = |state_id, versions: &[(i64, i64)], topology| AsmDeltaState {
        id: format!("state-{state_id}"),
        parent: "history".into(),
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
        entity_versions: versions
            .iter()
            .map(|&(entity_ref, record_ref)| AsmEntityVersion {
                entity_ref,
                record_ref,
            })
            .collect(),
        topology_cache: crate::history_records::AsmTopologyCache::Complete(topology),
        transition: None,
    };
    let previous = state(
        10,
        &[(1, 10), (4, 40), (8, 80)],
        AsmHistoricalTopology {
            bodies: vec![1],
            faces: vec![4],
            edges: vec![8],
            ..AsmHistoricalTopology::default()
        },
    );
    let current = state(
        11,
        &[(1, 11), (2, 2), (4, 40), (7, 70)],
        AsmHistoricalTopology {
            bodies: vec![1, 2],
            faces: vec![4],
            edges: vec![7],
            ..AsmHistoricalTopology::default()
        },
    );

    let transition = with_history_decode_context(|ctx| {
        historical_transition(ctx, &current, Some(&previous))
            .unwrap()
            .unwrap()
    });
    assert_eq!(transition.previous_state_id, Some(10));
    assert_eq!(transition.topology.bodies.inserted, [2]);
    assert_eq!(transition.topology.bodies.updated, [1]);
    assert!(transition.topology.faces.updated.is_empty());
    assert_eq!(transition.topology.edges.inserted, [7]);
    assert_eq!(transition.topology.edges.deleted, [8]);
    assert_eq!(transition.records.updated, [1]);
}

#[test]
fn snapshot_ordinals_bind_the_sorted_revision_interval() {
    let history_id = "history".to_string();
    let state_id = "state".to_string();
    let board_id = "board".to_string();
    let mut state = AsmDeltaState {
        id: state_id.clone(),
        parent: history_id,
        byte_offset: 0,
        state_id: 1,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 0,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: vec![AsmBulletinBoard {
            id: board_id.clone(),
            parent: state_id.clone(),
            byte_offset: 0,
            owner_ref: 0,
            number: 2,
            changes: [7, 5, 6]
                .into_iter()
                .enumerate()
                .map(|(index, old_ref)| AsmEntityChange {
                    id: format!("change-{index}"),
                    parent: board_id.clone(),
                    byte_offset: u64_from_index(index),
                    kind: AsmEntityChangeKind::Update {
                        old: old_ref,
                        new: i64::try_from(index).expect("fixture value fits i64"),
                    },
                })
                .collect(),
        }],
        records: (0..3)
            .map(|index| AsmHistoryRecord {
                id: format!("record-{index}"),
                parent: state_id.clone(),
                revision_id: None,
                byte_offset: index,
                framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                    index,
                    name: "edge".into(),
                    entity_references: Vec::new(),
                },
                raw_bytes: vec![0x11].into(),
            })
            .collect(),
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Absent,
        transition: None,
    };

    with_history_decode_context(|ctx| {
        bind_snapshot_revision_ids(ctx, std::slice::from_mut(&mut state)).unwrap()
    });

    assert_eq!(
        state
            .records
            .iter()
            .map(|record| record.revision_id)
            .collect::<Vec<_>>(),
        [Some(5), Some(6), Some(7)]
    );
}

#[test]
fn insert_only_history_uses_the_active_record_table_as_revisions() {
    let state = |node_index, next_ref, inserted: &[i64]| {
        let state_id = format!("state-{node_index}");
        let board_id = format!("board-{node_index}");
        AsmDeltaState {
            id: state_id.clone(),
            parent: "history".into(),
            byte_offset: u64::try_from(node_index).expect("fixture reference is nonnegative"),
            state_id: 10 - node_index,
            version_flag: 1,
            state_flag: 0,
            previous_ref: (node_index > 0).then_some(node_index - 1),
            next_ref,
            node_index,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: vec![AsmBulletinBoard {
                id: board_id.clone(),
                parent: state_id.clone(),
                byte_offset: u64::try_from(node_index).expect("fixture reference is nonnegative"),
                owner_ref: 0,
                number: 2,
                changes: inserted
                    .iter()
                    .enumerate()
                    .map(|(index, new_ref)| AsmEntityChange {
                        id: format!("change-{node_index}-{index}"),
                        parent: board_id.clone(),
                        byte_offset: u64_from_index(index),
                        kind: AsmEntityChangeKind::Insert { new: *new_ref },
                    })
                    .collect(),
            }],
            records: vec![AsmHistoryRecord {
                id: format!("record-{node_index}"),
                parent: state_id,
                revision_id: None,
                byte_offset: u64::try_from(node_index).expect("fixture reference is nonnegative"),
                framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                    index: 0,
                    name: "End-of-ASM-History-Section".into(),
                    entity_references: Vec::new(),
                },
                raw_bytes: vec![0x11].into(),
            }],
            entity_versions: Vec::new(),
            topology_cache: crate::history_records::AsmTopologyCache::Absent,
            transition: None,
        }
    };
    let mut states = vec![
        state(0, Some(1), &[1]),
        state(1, Some(2), &[2]),
        state(2, None, &[3]),
    ];

    with_history_decode_context(|ctx| {
        assert_eq!(
            insert_only_active_record_count(ctx, &states).unwrap(),
            Some(4)
        );
        bind_historical_entity_versions(ctx, &mut states).unwrap();
    });

    assert_eq!(
        states
            .iter()
            .map(|state| state.entity_versions.len())
            .collect::<Vec<_>>(),
        [4, 3, 2]
    );
    assert_eq!(
        states[1].entity_versions,
        [
            AsmEntityVersion {
                entity_ref: 0,
                record_ref: 0,
            },
            AsmEntityVersion {
                entity_ref: 2,
                record_ref: 2,
            },
            AsmEntityVersion {
                entity_ref: 3,
                record_ref: 3,
            },
        ]
    );
}

#[test]
fn insert_only_history_rejects_gaps_and_updates() {
    let mut state = AsmDeltaState {
        id: "state".into(),
        parent: "history".into(),
        byte_offset: 0,
        state_id: 1,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 0,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: vec![AsmHistoryRecord {
            id: "record".into(),
            parent: "state".into(),
            revision_id: None,
            byte_offset: 0,
            framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                index: 0,
                name: "End-of-ASM-History-Section".into(),
                entity_references: Vec::new(),
            },
            raw_bytes: vec![0x11].into(),
        }],
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Absent,
        transition: None,
    };
    let board = AsmBulletinBoard {
        id: "board".into(),
        parent: state.id.clone(),
        byte_offset: 0,
        owner_ref: 0,
        number: 2,
        changes: vec![
            AsmEntityChange {
                id: "gap-a".into(),
                parent: "board".into(),
                byte_offset: 0,
                kind: AsmEntityChangeKind::Insert { new: 1 },
            },
            AsmEntityChange {
                id: "gap-b".into(),
                parent: "board".into(),
                byte_offset: 0,
                kind: AsmEntityChangeKind::Insert { new: 3 },
            },
        ],
    };
    state.bulletin_boards.push(board);
    with_history_decode_context(|ctx| {
        assert_eq!(
            insert_only_active_record_count(ctx, &[state.clone()]).unwrap(),
            None
        )
    });
    state.bulletin_boards[0].changes[1].kind = AsmEntityChangeKind::Update { old: 2, new: 3 };
    with_history_decode_context(|ctx| {
        assert_eq!(
            insert_only_active_record_count(ctx, &[state]).unwrap(),
            None
        )
    });
}

#[test]
fn materialized_record_table_normalizes_revision_references() {
    let mut archived_bytes = vec![0x0d, 4];
    archived_bytes.extend_from_slice(b"edge");
    archived_bytes.push(0x0c);
    archived_bytes.extend_from_slice(&2i64.to_le_bytes());
    archived_bytes.push(0x11);
    let state_id = "state".to_string();
    let board_id = "board".to_string();
    let state = AsmDeltaState {
        id: state_id.clone(),
        parent: "history".into(),
        byte_offset: 0,
        state_id: 1,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 0,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: vec![AsmBulletinBoard {
            id: board_id.clone(),
            parent: state_id.clone(),
            byte_offset: 0,
            owner_ref: 0,
            number: 2,
            changes: vec![AsmEntityChange {
                id: "change".into(),
                parent: board_id,
                byte_offset: 0,
                kind: AsmEntityChangeKind::Update { old: 2, new: 1 },
            }],
        }],
        records: vec![AsmHistoryRecord {
            id: "record".into(),
            parent: state_id,
            revision_id: Some(2),
            byte_offset: 0,
            framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                index: 0,
                name: "edge".into(),
                entity_references: vec![2],
            },
            raw_bytes: archived_bytes.clone().into(),
        }],
        entity_versions: vec![
            AsmEntityVersion {
                entity_ref: 0,
                record_ref: 0,
            },
            AsmEntityVersion {
                entity_ref: 1,
                record_ref: 2,
            },
        ],
        topology_cache: crate::history_records::AsmTopologyCache::Absent,
        transition: None,
    };
    let active = ["asmheader", "edge"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| cadmpeg_asm::sab::Record {
            index,
            name: name.into(),

            tokens: Vec::new().into(),
            offset: 0,
            len: 0,
        })
        .collect::<Vec<_>>();

    let [framed]: [_; 1] = cadmpeg_asm::test_support::sab::frame(
        &archived_bytes,
        0,
        archived_bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("archived record frames")
    .try_into()
    .expect("one archived record");
    let table = with_history_decode_context(|ctx| {
        let archive = historical_record_archive(
            ctx,
            std::slice::from_ref(&state),
            &active,
            HashMap::from([(2, framed)]),
        )
        .expect("history archive budget")
        .expect("complete historical record archive");
        materialize_record_table(ctx, &state, &archive)
            .expect("historical table budget")
            .expect("complete historical RecordTable")
    });

    assert_eq!(table.len(), 2);
    assert_eq!(table[1].index, 1);
    assert_eq!(&*table[1].tokens, [cadmpeg_asm::sab::Token::Ref(1)]);
}

#[test]
fn qualified_history_marker_remains_an_archived_record() {
    let mut archived_bytes = Vec::new();
    for part in ["End", "of", "ASM", "History"] {
        archived_bytes.extend_from_slice(&[0x0e, u8::try_from(part.len()).unwrap()]);
        archived_bytes.extend_from_slice(part.as_bytes());
    }
    archived_bytes.extend_from_slice(&[0x0d, 7]);
    archived_bytes.extend_from_slice(b"Section");
    archived_bytes.extend_from_slice(&[0x0d, 4]);
    archived_bytes.extend_from_slice(b"body");
    archived_bytes.push(0x0c);
    archived_bytes.extend_from_slice(&2i64.to_le_bytes());
    archived_bytes.push(0x11);
    let state_id = "state".to_string();
    let board_id = "board".to_string();
    let state = AsmDeltaState {
        id: state_id.clone(),
        parent: "history".into(),
        byte_offset: 0,
        state_id: 1,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 0,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: vec![AsmBulletinBoard {
            id: board_id.clone(),
            parent: state_id.clone(),
            byte_offset: 0,
            owner_ref: 0,
            number: 2,
            changes: vec![AsmEntityChange {
                id: "change".into(),
                parent: board_id,
                byte_offset: 0,
                kind: AsmEntityChangeKind::Update { old: 2, new: 1 },
            }],
        }],
        records: vec![AsmHistoryRecord {
            id: "record".into(),
            parent: state_id,
            revision_id: Some(2),
            byte_offset: 0,
            framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                index: 0,
                name: "End-of-ASM-History-Section".into(),
                entity_references: vec![2],
            },
            raw_bytes: archived_bytes.clone().into(),
        }],
        entity_versions: vec![
            AsmEntityVersion {
                entity_ref: 0,
                record_ref: 0,
            },
            AsmEntityVersion {
                entity_ref: 1,
                record_ref: 2,
            },
        ],
        topology_cache: crate::history_records::AsmTopologyCache::Absent,
        transition: None,
    };
    let active = ["asmheader", "body"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| cadmpeg_asm::sab::Record {
            index,
            name: name.into(),

            tokens: Vec::new().into(),
            offset: 0,
            len: 0,
        })
        .collect::<Vec<_>>();

    let [framed]: [_; 1] = cadmpeg_asm::test_support::sab::frame(
        &archived_bytes,
        0,
        archived_bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .expect("archived record frames")
    .try_into()
    .expect("one archived record");
    let archive = with_history_decode_context(|ctx| {
        historical_record_archive(
            ctx,
            std::slice::from_ref(&state),
            &active,
            HashMap::from([(2, framed)]),
        )
        .expect("history archive budget")
        .expect("qualified history marker is an archived record")
    });
    let record = archive.get(&2).expect("marker revision is retained");
    assert_eq!(record.name, "End-of-ASM-History-Section");
    assert_eq!(record.index, 1);
    assert!(record.tokens.contains(&cadmpeg_asm::sab::Token::Ref(1)));
}

#[test]
fn reverse_history_builds_complete_entity_version_maps() {
    let state = |node_index, previous_ref, next_ref, old_ref, new_ref| {
        let board_id = format!("board-{node_index}");
        AsmDeltaState {
            id: format!("state-{node_index}"),
            parent: "history".into(),
            byte_offset: u64::try_from(node_index).expect("fixture reference is nonnegative"),
            state_id: 10 - node_index,
            version_flag: 1,
            state_flag: 0,
            previous_ref,
            next_ref,
            node_index,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: vec![AsmBulletinBoard {
                id: board_id.clone(),
                parent: format!("state-{node_index}"),
                byte_offset: u64::try_from(node_index).expect("fixture reference is nonnegative"),
                owner_ref: 0,
                number: 2,
                changes: vec![AsmEntityChange {
                    id: format!("change-{node_index}"),
                    parent: board_id,
                    byte_offset: u64::try_from(node_index)
                        .expect("fixture reference is nonnegative"),
                    kind: match (old_ref, new_ref) {
                        (Some(old), Some(new)) => AsmEntityChangeKind::Update { old, new },
                        (None, Some(new)) => AsmEntityChangeKind::Insert { new },
                        (Some(old), None) => AsmEntityChangeKind::Delete { old },
                        (None, None) => unreachable!(),
                    },
                }],
            }],
            records: Vec::new(),
            entity_versions: Vec::new(),
            topology_cache: crate::history_records::AsmTopologyCache::Absent,
            transition: None,
        }
    };
    let mut states = vec![
        state(0, None, Some(1), Some(3), Some(1)),
        state(1, Some(0), Some(2), Some(4), Some(1)),
        state(2, Some(1), Some(3), None, Some(2)),
        state(3, Some(2), None, None, Some(1)),
    ];
    states[0].records = [3, 4]
        .map(|revision_id| AsmHistoryRecord {
            id: format!("record-{revision_id}"),
            parent: states[0].id.clone(),
            revision_id: Some(revision_id),
            byte_offset: 0,
            framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                index: u64::try_from(revision_id).expect("fixture reference is nonnegative") - 3,
                name: "edge".into(),
                entity_references: Vec::new(),
            },
            raw_bytes: vec![0x11].into(),
        })
        .into();

    with_history_decode_context(|ctx| bind_historical_entity_versions(ctx, &mut states).unwrap());

    assert_eq!(
        states
            .iter()
            .map(|state| state.entity_versions.len())
            .collect::<Vec<_>>(),
        [3, 3, 3, 2]
    );
    assert_eq!(
        states[1].entity_versions,
        [
            AsmEntityVersion {
                entity_ref: 0,
                record_ref: 0,
            },
            AsmEntityVersion {
                entity_ref: 1,
                record_ref: 3,
            },
            AsmEntityVersion {
                entity_ref: 2,
                record_ref: 2,
            },
        ]
    );
    assert_eq!(states[2].entity_versions[1].record_ref, 4);
}

#[test]
fn profile_face_group_cardinality_requires_one_changed_surface_family() {
    let topology = AsmHistoricalTopology {
        faces: vec![10, 11, 12, 20],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 12,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 20,
                carrier: 200,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let changed = [20, 12, 10, 11].into_iter().collect();
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            profile_face_group_cardinality_candidates(decode_ctx, &topology, &changed, 3)
        })
        .unwrap(),
        Some(vec![10, 11, 12])
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            profile_face_group_cardinality_candidates(
                decode_ctx,
                &topology,
                &[20].into_iter().collect(),
                1,
            )
        })
        .unwrap(),
        Some(vec![20])
    );
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            profile_face_group_cardinality_candidates(
                decode_ctx,
                &topology,
                &[10, 20].into_iter().collect(),
                1,
            )
        })
        .unwrap(),
        None
    );

    let mut ambiguous = topology;
    ambiguous.faces.extend([30, 31, 32]);
    ambiguous
        .face_surfaces
        .extend([30, 31, 32].map(|entity| AsmHistoricalCarrierBinding {
            entity,
            carrier: 300,
        }));
    let changed = [10, 11, 12, 30, 31, 32].into_iter().collect();
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            profile_face_group_cardinality_candidates(decode_ctx, &ambiguous, &changed, 3)
        })
        .unwrap(),
        None
    );
}

fn profile_candidate_limit_case(
    max_items: u64,
) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError> {
    let topology = AsmHistoricalTopology {
        faces: vec![10],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 10,
            carrier: 100,
        }],
        ..AsmHistoricalTopology::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    profile_face_group_cardinality_candidates(&ctx, &topology, &[10].into_iter().collect(), 1)
}

#[test]
fn profile_preceding_faces_refuse_collection_limit() {
    let error = profile_candidate_limit_case(0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D profile candidate faces")
    );
}

#[test]
fn profile_face_carriers_refuse_collection_limit() {
    let error = profile_candidate_limit_case(1).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D profile face carriers")
    );
}

#[test]
fn profile_carrier_faces_refuse_collection_limit() {
    let error = profile_candidate_limit_case(2).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D profile carrier faces")
    );
}

#[test]
fn grouped_face_reference_selects_one_changed_topology_face() {
    use crate::records::topology::face::DesignFaceOperand;

    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&5u32.to_le_bytes());
    for (token, references) in [
        ("1", [10u32, 20].as_slice()),
        ("2", [30u32].as_slice()),
        ("3", [40u32].as_slice()),
        ("4", [50u32].as_slice()),
        ("5", [60u32].as_slice()),
    ] {
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
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
    prefix.extend_from_slice(&0u32.to_le_bytes());

    let mut operand = serde_json::from_value::<DesignFaceOperand>(serde_json::json!({
        "id": "f3d:Design/BulkStream.dat:design-face-operand#1",
        "scope_record_index": 1,
        "scope_reference_ordinal": 0,
        "record_index": 2,
        "byte_offset": 0,
        "class_tag": "277",
        "paired_byte_offset": 16,
        "paired_class_tag": "259",
        "recipe_record_index": 5,
        "recipe_record_byte_offset": 32,
        "recipe_id": "f3d:Design/BulkStream.dat:construction-recipe#5",
        "recipe_prefix_offset": 43,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_kind": "bounded_face",
        "recipe_program_offset": 0,
        "recipe_program": [0],
        "recipe_node_offsets": [],
        "recipe_nodes": [],
        "next_record_index": 6,
        "next_byte_offset": 160
    }))
    .expect("grouped face operand");
    *operand.recipe_prefix_bytes = prefix;
    operand.recipe_references = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::dimension_frames::decode_recipe_references_charged(
            ctx,
            &operand.recipe_prefix_bytes,
            0,
        )
        .expect("recipe references")
    });
    let topology = AsmHistoricalTopology {
        faces: vec![10, 20],
        ..AsmHistoricalTopology::default()
    };

    assert_eq!(
        grouped_reference_face_candidate(&operand, &topology, &HashSet::from([10])),
        Some(
            cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(10))
                .expect("identity grammar")
        )
    );
    assert_eq!(
        grouped_reference_face_candidate(&operand, &topology, &HashSet::from([10, 20])),
        None
    );
    let mut trailing = operand;
    trailing.recipe_prefix_bytes.extend_from_slice(&[0; 4]);
    assert_eq!(
        grouped_reference_face_candidate(&trailing, &topology, &HashSet::from([10])),
        None
    );
}

mod extrude_profile;
