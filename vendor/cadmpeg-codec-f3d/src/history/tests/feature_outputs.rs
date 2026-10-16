// SPDX-License-Identifier: Apache-2.0

fn empty_transition_history() -> crate::history_records::AsmHistory {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalEntityDelta, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
        AsmHistoricalTransition, AsmHistory, AsmTopologyCache,
    };
    AsmHistory {
        id: "f3d:asm-history#1".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![AsmDeltaState {
            id: "f3d:asm-delta-state#1".into(),
            parent: "f3d:asm-history#1".into(),
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
            records: Vec::new(),
            entity_versions: Vec::new(),
            topology_cache: AsmTopologyCache::Complete(AsmHistoricalTopology::default()),
            transition: Some(AsmHistoricalTransition {
                previous_state_id: None,
                records: AsmHistoricalEntityDelta::default(),
                topology: AsmHistoricalTopologyDelta::default(),
            }),
        }],
    }
}

fn feature_output_error(
    histories: &[crate::history_records::AsmHistory],
    bodies: &[cadmpeg_ir::topology::Body],
    max_items: u64,
    max_retained: u64,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::bind_feature_outputs(&ctx, &mut [], &[], histories, bodies).unwrap_err()
}

#[test]
fn feature_output_history_nodes_refuse_collection_limit() {
    let history = empty_transition_history();
    let error = feature_output_error(&[history], &[], 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D feature output history nodes")
    );
}

#[test]
fn feature_output_states_refuse_collection_limit() {
    let history = empty_transition_history();
    let error = feature_output_error(&[history], &[], 2, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D feature output states")
    );
}

fn active_body() -> cadmpeg_ir::topology::Body {
    let mut body = cadmpeg_ir::examples::unit_cube()
        .unwrap()
        .model
        .bodies
        .remove(0);
    body.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep:entity#1").unwrap();
    body
}

#[test]
fn feature_output_active_body_refuses_collection_limit() {
    let body = active_body();
    let error = feature_output_error(&[], &[body], 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D active feature output bodies")
    );
}

#[test]
fn feature_output_active_id_refuses_retained_limit() {
    let body = active_body();
    let error = feature_output_error(&[], &[body], u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D active body identity")
    );
}

fn base_feature() -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::{
        BodySelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    };
    use cadmpeg_ir::ids::BodyId;
    Feature {
        id: FeatureId::mint("test:model:feature#feature").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Base Feature".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Native("native:scope".into()),
            }),
            cadmpeg_ir::features::DistinctMembers::try_from(
                vec![
                    BodyId::mint("test:model:body#2").unwrap(),
                    BodyId::mint("test:model:body#1").unwrap(),
                ],
                &cadmpeg_test_support::service_decode_context(),
            )
            .unwrap(),
        ),
        native_ref: Some("native:scope".into()),
    }
}

fn base_feature_error(max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::bind_base_feature_output_selection(&ctx, &mut base_feature()).unwrap_err()
}

#[test]
fn base_feature_output_bodies_refuse_collection_limit() {
    let error = base_feature_error(0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D BaseFeature output bodies")
    );
}

#[test]
fn base_feature_output_body_id_refuses_retained_limit() {
    let error = base_feature_error(u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D BaseFeature body identity")
    );
}

#[test]
fn base_feature_native_selection_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D BaseFeature native selection",
        |cap| Err::<(), cadmpeg_core::CodecError>(base_feature_error(u64::MAX, cap)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D BaseFeature native selection")
    );
}

fn output_binding_inputs() -> (
    cadmpeg_ir::features::Feature,
    crate::records::feature::scope::DesignParameterScope,
    crate::history_records::AsmHistory,
    cadmpeg_ir::topology::Body,
) {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalEntityDelta, AsmHistoricalRelation, AsmHistoricalTopology,
        AsmHistoricalTopologyDelta, AsmHistoricalTransition, AsmHistory, AsmTopologyCache,
    };
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let mut feature = base_feature();
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1",
        DesignFeatureKind::BaseFeature,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(1);
            draft.previous_history_state_id = Some(0);
            draft.layout_fixture_tail();
        })
        .unwrap();
    feature.native_ref = Some(scope.id.clone());
    let mut current_topology = AsmHistoricalTopology::default();
    current_topology.bodies.push(1);
    current_topology.body_regions.push(AsmHistoricalRelation {
        owner_ref: 1,
        member_refs: Vec::new(),
    });
    let mut delta = AsmHistoricalTopologyDelta::default();
    delta.bodies.inserted.push(1);
    let state = |state_id, node_index, next_ref, topology, transition| AsmDeltaState {
        id: format!("f3d:asm-delta-state#{state_id}"),
        parent: "f3d:asm-history#1".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref,
        node_index,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        topology_cache: AsmTopologyCache::Complete(topology),
        transition,
    };
    let history = AsmHistory {
        id: "f3d:asm-history#1".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![
            state(
                1,
                1,
                Some(0),
                current_topology,
                Some(AsmHistoricalTransition {
                    previous_state_id: Some(0),
                    records: AsmHistoricalEntityDelta::default(),
                    topology: delta,
                }),
            ),
            state(0, 0, None, AsmHistoricalTopology::default(), None),
        ],
    };
    (feature, scope, history, active_body())
}

fn bound_output_error(max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (mut feature, scope, history, body) = output_binding_inputs();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::bind_feature_outputs(
        &ctx,
        std::slice::from_mut(&mut feature),
        &[scope],
        &[history],
        &[body],
    )
    .unwrap_err()
}

#[test]
fn feature_output_bodies_refuse_collection_limit() {
    // The two snapshot cache entries and one body-closure vector add three slots.
    let error = bound_output_error(12, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D feature output bodies")
    );
}

#[test]
fn feature_output_body_id_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D feature output body identity",
        |cap| Err::<(), cadmpeg_core::CodecError>(bound_output_error(u64::MAX, cap)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D feature output body identity")
    );
}

#[test]
fn feature_output_binding_preserves_base_feature_selection() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    let (mut feature, scope, history, body) = output_binding_inputs();
    let ctx = cadmpeg_test_support::service_decode_context();
    super::super::bind_feature_outputs(
        &ctx,
        std::slice::from_mut(&mut feature),
        &[scope],
        &[history],
        std::slice::from_ref(&body),
    )
    .unwrap();
    assert_eq!(feature.evaluation.outputs().len(), 1);
    assert_eq!(feature.evaluation.outputs()[0], body.id);
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Resolved { bodies, native }
        }) if bodies.as_slice() == [body.id] && native == "native:scope"));
}

#[test]
fn changed_topology_members_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut delta = crate::history_records::AsmHistoricalTopologyDelta::default();
    delta.bodies.inserted.push(1);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::changed_family_refs(&ctx, &delta, false).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D changed topology members")
    );
}

#[test]
fn affected_history_bodies_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (_, _, history, _) = output_binding_inputs();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 7;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::affected_body_refs(
        &ctx,
        &history.states[0],
        Some(&history.states[1]),
        &mut super::super::HistoricalBodyClosureCache::default(),
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D affected history bodies")
    );
}
