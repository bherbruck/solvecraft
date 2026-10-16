// SPDX-License-Identifier: Apache-2.0
//! Body-selection history ownership tests.

#![allow(clippy::default_trait_access)]

use crate::history::{bind_feature_body_selections, FeatureBodySelectionInputs};
use crate::records::topology::{
    construction::DesignConstructionOperandGroup,
    construction::DesignConstructionOperandGroupFrame, extrude_selection::DesignOperandRole,
};

#[test]
fn move_body_selection_uses_unique_owning_history() {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalEntityDelta, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
        AsmHistoricalTransition, AsmHistory,
    };
    use cadmpeg_ir::features::{
        BodySelection, Feature, FeatureDefinition, FeatureId, FeatureOperation,
    };

    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::Move,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(42);
            draft.previous_history_state_id = Some(41);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group_id = "f3d:Design/BulkStream.dat:design-construction-operand-group#20";
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: group_id.into(),
            scope_record_index: 10,
            scope_reference_ordinal: 0,
            record_index: 20,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("280".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 21,
                offset: 0,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::BODIES_A,
                ),
            role_offset: 0,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap();
    let topology = || AsmHistoricalTopology {
        bodies: vec![1],
        ..AsmHistoricalTopology::default()
    };
    let state = |state_id: i64,
                 parent: &str,
                 transition: Option<AsmHistoricalTransition>|
     -> AsmDeltaState {
        AsmDeltaState {
            id: format!("f3d:{parent}:asm-delta-state#{state_id}"),
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
            topology_cache: crate::history_records::AsmTopologyCache::Complete(topology()),
            transition,
        }
    };
    let mut transition = AsmHistoricalTransition {
        previous_state_id: Some(41),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    };
    transition.topology.bodies.updated.push(1);
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![
            state(42, "history", Some(transition)),
            state(41, "history", None),
        ],
    };
    let unrelated_history = AsmHistory {
        id: "unrelated-history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![state(41, "unrelated-history", None)],
    };
    let histories = [history, unrelated_history];
    let mut feature = Feature {
        id: FeatureId::mint("f3d:test:feature#move").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::MoveBody {
                bodies: BodySelection::Native(group_id.into()),
                translation: cadmpeg_ir::features::FiniteVector3::new(
                    cadmpeg_ir::math::Vector3::new(1.0, 2.0, 3.0),
                )
                .unwrap(),
                rotation: None,
                copies: 0,
            }),
        ),
        native_ref: None,
    };
    feature.native_ref = Some(scope.id.clone());
    let inputs = FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: std::slice::from_ref(&group),
        body_recipe_operands: &[],
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &histories,
        bodies: &[],
        regions: &[],
        shells: &[],
    };
    bind_feature_body_selections(
        &cadmpeg_test_support::service_decode_context(),
        std::slice::from_mut(&mut feature),
        &inputs,
    )
    .unwrap();

    let expected_body = crate::ids::history_input_body_id(
        &crate::ids::history_input_prefix(&cadmpeg_ir::identity_key!("move"), 41),
        1,
    );
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::MoveBody {
            bodies: BodySelection::Historical { ref bodies, ref native, .. },
            ..
        }) if bodies.as_slice() == [expected_body] && native.as_str() == group_id
    ));
}

fn pattern_seed_error(
    max_items: u64,
    max_retained: u64,
) -> Result<cadmpeg_ir::features::Feature, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::features::{
        patterns::PatternKind, Feature, FeatureDefinition, FeatureId, FeatureOperation,
    };
    let scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::CircularPattern,
        10,
    );
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:design-construction-operand-group#20".into(),
            scope_record_index: 10,
            scope_reference_ordinal: 0,
            record_index: 20,
            byte_offset: 0,
            class_tag: "280".to_owned().try_into().unwrap(),
            members: vec![crate::records::identity::Located {
                value: 21,
                offset: 0,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::BODIES_B,
                ),
            role_offset: 0,
            paired_class_tag: "259".to_owned().try_into().unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap();
    let mut feature = Feature {
        id: FeatureId::mint("f3d:test:feature#pattern").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::UNRESOLVED_CIRCULAR,
            }),
        ),
        native_ref: Some(scope.id.clone()),
    };
    let inputs = FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: std::slice::from_ref(&group),
        body_recipe_operands: &[],
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &[],
        bodies: &[],
        regions: &[],
        shells: &[],
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::bind_pattern_body_selections(
        &ctx,
        std::slice::from_mut(&mut feature),
        &inputs,
        &mut crate::history::body_candidates::BodyCandidates::new(
            inputs.bodies,
            inputs.regions,
            inputs.shells,
            inputs.construction_recipes,
            inputs.persistent_design_links,
        ),
    )?;
    Ok(feature)
}

#[test]
fn pattern_body_seed_refuses_collection_limit() {
    let error = pattern_seed_error(0, u64::MAX).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D pattern body seeds")
    );
}

#[test]
fn pattern_body_seed_identity_refuses_retained_limit() {
    let error = pattern_seed_error(u64::MAX, 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D pattern seed native identity")
    );
}

#[test]
fn pattern_body_seed_preserves_native_selection() {
    use cadmpeg_ir::features::{
        patterns::PatternSeed, BodySelection, FeatureDefinition, FeatureOperation,
    };
    let feature = pattern_seed_error(u64::MAX, u64::MAX).unwrap();
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. })
            if matches!(seeds.as_slice(),
                [PatternSeed::Bodies(BodySelection::Native(id))]
                    if id == "f3d:Design/BulkStream.dat:design-construction-operand-group#20")));
}

fn pattern_slots_error(max_items: u64, max_retained: u64) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternSeed},
        BodySelection, Feature, FeatureDefinition, FeatureId, FeatureOperation,
    };
    let feature_id = FeatureId::mint("f3d:test:feature#pattern").unwrap();
    let prefix = crate::ids::history_input_prefix(&feature_id.key(), 7);
    let seed = BodySelection::historical(
        crate::ids::history_input_state_id(&prefix),
        vec![crate::ids::history_input_body_id(&prefix, 1)],
        "f3d:test:native-selection#1".into(),
        &cadmpeg_test_support::service_decode_context(),
    )
    .expect("selection storage is admitted")
    .unwrap();
    let pattern: PatternKind = serde_json::from_value(serde_json::json!({
        "kind": "circular",
        "axis_origin": {"x": 0.0, "y": 0.0, "z": 0.0},
        "axis_dir": {"x": 0.0, "y": 0.0, "z": 1.0},
        "angle": 1.0,
        "count": 2
    }))
    .unwrap();
    let mut feature = Feature {
        id: feature_id,
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: vec![PatternSeed::Bodies(seed)],
                pattern,
            }),
        ),
        native_ref: None,
    };
    feature.evaluation.set_outputs(
        cadmpeg_ir::features::DistinctMembers::try_from(
            vec![cadmpeg_ir::ids::BodyId::mint("f3d:brep:body#2").unwrap()],
            &cadmpeg_test_support::service_decode_context(),
        )
        .unwrap(),
    );
    let inputs = FeatureBodySelectionInputs {
        scopes: &[],
        groups: &[],
        body_recipe_operands: &[],
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &[],
        bodies: &[],
        regions: &[],
        shells: &[],
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::bind_feature_body_selections(&ctx, std::slice::from_mut(&mut feature), &inputs)
}

#[test]
fn pattern_body_first_slot_refuses_collection_limit() {
    let error = pattern_slots_error(0, u64::MAX).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D pattern body slots")
    );
}

#[test]
fn pattern_body_second_slot_refuses_collection_limit() {
    let error = pattern_slots_error(1, u64::MAX).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D pattern body slots")
    );
}

#[test]
fn pattern_body_feature_id_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D pattern body feature ID",
        |cap| pattern_slots_error(u64::MAX, cap),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D pattern body feature ID")
    );
}

#[test]
fn pattern_body_feature_index_refuses_collection_limit() {
    let error = pattern_slots_error(2, u64::MAX).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D pattern body features")
    );
}
