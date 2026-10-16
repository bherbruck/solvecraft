// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::feature_project::project_parameter_design_with_edge_identities;
use crate::design::feature_project::{ScopeHistoryBinding, ScopeHistoryGraph};
use crate::design::test_support::parameter_record;
use crate::records::entity_header::DesignFeatureTimeline;
use crate::records::feature::assembly::DesignAssemblyAlignment;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_ir::features::FeatureDefinition;
use cadmpeg_ir::features::FeatureOperation;
use std::collections::HashMap;

#[test]
fn work_point_history_state_keys_are_history_qualified() {
    let scope_a = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#a",
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        1,
    );
    let scope_b = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#b",
        crate::records::feature::scope::DesignFeatureKind::Fillet,
        2,
    );
    let graph = ScopeHistoryGraph {
        binding: ScopeHistoryBinding::Bound(HashMap::from([
            (scope_a.id.clone(), "f3d:history#a".to_owned()),
            (scope_b.id.clone(), "f3d:history#b".to_owned()),
        ])),
        component_namespaces: HashMap::from([
            (
                scope_a.id.clone(),
                crate::design::feature_project::ComponentHistoryNamespace::Aggregate,
            ),
            (
                scope_b.id.clone(),
                crate::design::feature_project::ComponentHistoryNamespace::Aggregate,
            ),
        ]),
        scopes_by_state: HashMap::new(),
    };

    assert_ne!(
        crate::test_support::with_decode_context(|ctx| graph.state_key(ctx, &scope_a, 7)).unwrap(),
        crate::test_support::with_decode_context(|ctx| graph.state_key(ctx, &scope_b, 7)).unwrap()
    );
}

#[test]
fn history_state_predecessors_are_component_qualified() {
    let bulk_stream = "Design/BulkStream.dat";
    let stream = format!("f3d:{bulk_stream}");
    let mut first = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#15"),
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        15,
    );
    first
        .try_edit(|draft| {
            draft.history_state_id = Some(7);
        })
        .unwrap();
    let mut local_predecessor = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#22"),
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        22,
    );
    local_predecessor
        .try_edit(|draft| {
            draft.history_state_id = Some(7);
        })
        .unwrap();
    let mut second = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#25"),
        crate::records::feature::scope::DesignFeatureKind::Fillet,
        25,
    );
    second
        .try_edit(|draft| {
            draft.history_state_id = Some(8);
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let scopes = vec![first, local_predecessor.clone(), second.clone()];
    let naming_space = |component_record_index, context_uuid: &str| {
        crate::records::recipes::DesignComponentNamingSpace {
            id: crate::ids::native_design_component_naming_space_id(
                bulk_stream,
                component_record_index,
            ),
            byte_offset: component_record_index,
            component_record_index,
            context_uuid: context_uuid.to_owned().try_into().expect("GUID"),
            context_uuid_offset: component_record_index + 12,
        }
    };
    let naming_spaces = [
        naming_space(10, "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"),
        naming_space(20, "ffffffff-eeee-4ddd-8ccc-bbbbbbbbbbbb"),
    ];
    let graph = crate::test_support::with_decode_context(|decode_ctx| {
        ScopeHistoryGraph::new(decode_ctx, &scopes, &[], &[], &naming_spaces, &[])
    })
    .unwrap();

    let predecessor = crate::test_support::with_decode_context(|decode_ctx| {
        graph.predecessor(decode_ctx, &second, |_| true)
    })
    .expect("component-qualified state chain");
    let crate::design::feature_project::ScopeHistoryPredecessor::Scope(predecessor) = predecessor
    else {
        panic!("component-local predecessor");
    };
    assert_eq!(predecessor.id, local_predecessor.id);
}

#[test]
fn feature_projection_uses_timeline_items_not_scope_byte_order() {
    let stream = "f3d:Design/BulkStream.dat";
    let mut earlier = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#900"),
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        100,
    );
    earlier
        .try_edit(|draft| {
            draft.byte_offset = 900;
            draft.history_state_id = Some(7);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut later = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        crate::records::feature::scope::DesignFeatureKind::Fillet,
        200,
    );
    later
        .try_edit(|draft| {
            draft.byte_offset = 100;
            draft.previous_history_state_id = Some(7);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let scopes = vec![later.clone(), earlier.clone()];
    let timeline = |items: Vec<u64>| {
        DesignFeatureTimeline::try_new(
            crate::ids::native_design_feature_timeline_id_in_stream(stream, 10),
            crate::records::entity_header::DesignTimelineFrame::test_items(
                10,
                items
                    .into_iter()
                    .map(|value| crate::records::identity::Located { value, offset: 0 })
                    .collect(),
            ),
            crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
            std::num::NonZeroU64::new(35).unwrap(),
            0,
            std::num::NonZeroU64::new(17).unwrap(),
        )
        .unwrap()
    };
    let authored = timeline(vec![100, 150, 200]);
    let project = |timeline: &DesignFeatureTimeline| {
        crate::test_support::with_decode_context(|decode_ctx| {
            project_parameter_design_with_edge_identities(
                decode_ctx,
                &crate::design::feature_project::ProjectInputs {
                    scopes: &scopes,
                    timelines: std::slice::from_ref(timeline),
                    ..Default::default()
                },
            )
        })
    };
    let (features, _) = project(&authored).expect("exact authored order");
    let earlier_feature = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(&earlier.id))
        .expect("earlier feature");
    let later_feature = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(&later.id))
        .expect("later feature");
    assert_eq!(earlier_feature.ordinal, 0);
    assert_eq!(later_feature.ordinal, 2);
    assert_eq!(
        later_feature.dependencies.as_slice(),
        [earlier_feature.id.clone()]
    );

    let unrelated = DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream("f3d:Other/BulkStream.dat", 10),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            authored.frame().byte_offset(),
            vec![crate::records::identity::Located {
                value: 9000,
                offset: 0,
            }],
        ),
        authored.class_tag.clone(),
        authored.record_index,
        authored.source_ordinal,
        authored.context_record_index,
    )
    .unwrap();
    let ordinals = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::feature_project::authored_scope_ordinals(
            decode_ctx,
            &scopes,
            &[unrelated, authored.clone()],
        )
    })
    .expect("an unrelated timeline does not shift this stream");
    assert_eq!(ordinals[&(stream, 100)], 0);
    assert_eq!(ordinals[&(stream, 200)], 2);

    let reversed = timeline(vec![200, 150, 100]);
    let error = project(&reversed).expect_err("forward history edge must be rejected");
    assert!(error
        .to_string()
        .contains("dependency does not precede its authored timeline position"));

    let second = DesignFeatureTimeline::try_new(
        authored.id().clone(),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            authored.frame().byte_offset(),
            vec![crate::records::identity::Located {
                value: 300,
                offset: 0,
            }],
        ),
        authored.class_tag.clone(),
        std::num::NonZeroU64::new(36).unwrap(),
        1,
        authored.context_record_index,
    )
    .unwrap();
    let error = crate::test_support::with_decode_context(|decode_ctx| {
        project_parameter_design_with_edge_identities(
            decode_ctx,
            &crate::design::feature_project::ProjectInputs {
                scopes: &scopes,
                timelines: &[authored, second],
                ..Default::default()
            },
        )
    })
    .expect_err("independent nonempty timelines have no total order");
    assert!(error
        .to_string()
        .contains("multiple nonempty Design timelines"));
}

#[test]
fn feature_projection_collapses_internal_scope_history_chains() {
    let stream = "f3d:Design/BulkStream.dat";
    let mut predecessor = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        100,
    );
    predecessor
        .try_edit(|draft| {
            draft.history_state_id = Some(7);
        })
        .unwrap();
    let mut internal = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#150"),
        crate::records::feature::scope::DesignFeatureKind::BaseFeature,
        150,
    );
    internal
        .try_edit(|draft| {
            draft.history_state_id = Some(8);
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut successor = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#200"),
        crate::records::feature::scope::DesignFeatureKind::Fillet,
        200,
    );
    successor
        .try_edit(|draft| {
            draft.history_state_id = Some(9);
            draft.previous_history_state_id = Some(8);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let scopes = vec![successor.clone(), internal.clone(), predecessor.clone()];
    let timeline = DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream(stream, 10),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            10,
            vec![
                crate::records::identity::Located {
                    value: 100,
                    offset: 0,
                },
                crate::records::identity::Located {
                    value: 200,
                    offset: 0,
                },
            ],
        ),
        crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(35).unwrap(),
        0,
        std::num::NonZeroU64::new(17).unwrap(),
    )
    .unwrap();
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(40),
        "1 mm",
        "FeatureInput",
        Some("mm"),
        "InternalValue",
        0.1,
    ))
    .expect("synthetic internal parameter");
    parameter.id = format!("{stream}:design-parameter#41");
    parameter.record_index = 41;
    parameter
        .try_set_source(
            crate::records::parameters::DesignParameterSource::new(
                parameter.source_kind().to_owned(),
                Some(40),
                parameter.family_discriminator(),
            )
            .unwrap(),
        )
        .unwrap();
    let owner = crate::records::parameters::DesignParameterOwner::try_from(
        crate::records::parameters::DesignParameterOwnerWire {
            id: format!("{stream}:design-parameter-owner#40"),
            byte_offset: 0,
            frame_length: 103,
            class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned())
                .unwrap(),
            record_index: 40,
            scope_record_index: internal.record_index,
            local_ordinal: 0,
            evaluated_value: 0.1,
            evaluated_value_offset: 40,
            parameter_record_index: parameter.record_index,
            owned_ordinal: 0,
            variant: None,
            companion_record_index: 42,
        },
    )
    .unwrap();
    let (features, parameters) = crate::test_support::with_decode_context(|decode_ctx| {
        project_parameter_design_with_edge_identities(
            decode_ctx,
            &crate::design::feature_project::ProjectInputs {
                native: std::slice::from_ref(&parameter),
                owners: std::slice::from_ref(&owner),
                scopes: &scopes,
                timelines: std::slice::from_ref(&timeline),
                ..Default::default()
            },
        )
    })
    .expect("timeline-listed feature projection through one internal scope");

    assert_eq!(features.len(), 2);
    assert!(features
        .iter()
        .all(|feature| feature.native_ref.as_deref() != Some(internal.id.as_str())));
    let predecessor_feature = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(predecessor.id.as_str()))
        .expect("projected predecessor");
    let successor_feature = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(successor.id.as_str()))
        .expect("projected successor");
    assert_eq!(
        successor_feature.dependencies.as_slice(),
        [predecessor_feature.id.clone()]
    );
    assert_eq!(parameters.len(), 1);
    assert!(parameters[0].owner.is_none());
    assert_eq!(
        parameters[0].properties.get("owner_record_index"),
        Some(&owner.record_index().to_string())
    );
}

#[test]
fn feature_projection_uses_the_timeline_position_of_an_assembly_datum_envelope() {
    let stream = "f3d:Design/BulkStream.dat";
    let mut assembly = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#10"),
        crate::records::feature::scope::DesignFeatureKind::Assemble,
        10,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Assemble(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::AsBuilt(slot) =
        assembly.payload_mut()
    {
        *slot = Some(
            DesignAssemblyAlignment::try_new(
                0.0,
                [0.0; 3],
                Vec::new(),
                Some(
                    crate::records::feature::assembly::DesignAssemblyAlignmentForm::DatumEnvelope {
                        joint_origin_scope_record_index: 20,
                    },
                ),
            )
            .unwrap(),
        );
    }
    let mut origin = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#20"),
        crate::records::feature::scope::DesignFeatureKind::JointOrigin,
        20,
    );
    origin.with_joint_origin_transform(
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
    );
    let mut internal_origin = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#30"),
        crate::records::feature::scope::DesignFeatureKind::JointOrigin,
        30,
    );
    internal_origin.with_joint_origin_transform(
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
    );
    let scopes = vec![assembly, origin.clone(), internal_origin.clone()];
    let timeline = DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            0,
            vec![crate::records::identity::Located {
                value: 10,
                offset: 0,
            }],
        ),
        crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(2).unwrap(),
    )
    .unwrap();
    let (features, _) = crate::test_support::with_decode_context(|decode_ctx| {
        project_parameter_design_with_edge_identities(
            decode_ctx,
            &crate::design::feature_project::ProjectInputs {
                scopes: &scopes,
                timelines: std::slice::from_ref(&timeline),
                ..Default::default()
            },
        )
    })
    .expect("one authored datum envelope");

    let [feature] = features.as_slice() else {
        panic!("expected one projected datum feature");
    };
    assert_eq!(feature.ordinal, 0);
    assert_eq!(feature.native_ref.as_deref(), Some(origin.id.as_str()));
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DatumCoordinateSystem { .. })
    ));
    assert_ne!(
        feature.native_ref.as_deref(),
        Some(internal_origin.id.as_str())
    );

    let mut directly_listed = timeline;
    let mut items = directly_listed.frame().items().to_vec();
    items.push(crate::records::identity::Located {
        value: origin.record_index.into(),
        offset: 0,
    });
    directly_listed = crate::records::entity_header::DesignFeatureTimeline::try_new(
        directly_listed.id().clone(),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            directly_listed.frame().byte_offset(),
            items,
        ),
        directly_listed.class_tag.clone(),
        directly_listed.record_index,
        directly_listed.source_ordinal,
        directly_listed.context_record_index,
    )
    .unwrap();
    let (features, _) = crate::test_support::with_decode_context(|decode_ctx| {
        project_parameter_design_with_edge_identities(
            decode_ctx,
            &crate::design::feature_project::ProjectInputs {
                scopes: &scopes,
                timelines: std::slice::from_ref(&directly_listed),
                ..Default::default()
            },
        )
    })
    .expect("directly listed datum target");
    let [feature] = features.as_slice() else {
        panic!("expected one directly listed datum feature");
    };
    assert_eq!(feature.ordinal, 1);
}

#[test]
fn feature_projection_rejects_multiple_datum_envelope_positions() {
    let stream = "f3d:Design/BulkStream.dat";
    let envelope = |record_index| {
        let mut scope = DesignParameterScope::empty(
            &format!("{stream}:design-parameter-scope#{record_index}"),
            crate::records::feature::scope::DesignFeatureKind::Assemble,
            record_index,
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::Assemble(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::AsBuilt(slot) =
            scope.payload_mut()
        {
            *slot = Some(
                DesignAssemblyAlignment::try_new(
                    0.0,
                    [0.0; 3],
                    Vec::new(),
                    Some(
                        crate::records::feature::assembly::DesignAssemblyAlignmentForm::DatumEnvelope {
                            joint_origin_scope_record_index: 20,
                        },
                    ),
                )
                .unwrap(),
            );
        }
        scope
    };
    let mut origin = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#20"),
        crate::records::feature::scope::DesignFeatureKind::JointOrigin,
        20,
    );
    origin.with_joint_origin_transform(
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
    );
    let scopes = vec![envelope(10), envelope(11), origin];
    let timeline = DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            0,
            vec![
                crate::records::identity::Located {
                    value: 10,
                    offset: 0,
                },
                crate::records::identity::Located {
                    value: 11,
                    offset: 0,
                },
            ],
        ),
        crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(2).unwrap(),
    )
    .unwrap();
    let result = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::feature_project::authored_scope_ordinals(
            decode_ctx,
            &scopes,
            std::slice::from_ref(&timeline),
        )
    });
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn feature_projection_rejects_a_cyclic_internal_scope_history() {
    let stream = "f3d:Design/BulkStream.dat";
    let mut first_internal = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#10"),
        crate::records::feature::scope::DesignFeatureKind::BaseFeature,
        10,
    );
    first_internal
        .try_edit(|draft| {
            draft.history_state_id = Some(1);
            draft.previous_history_state_id = Some(2);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut second_internal = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#20"),
        crate::records::feature::scope::DesignFeatureKind::BaseFeature,
        20,
    );
    second_internal
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut consumer = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#30"),
        crate::records::feature::scope::DesignFeatureKind::Move,
        30,
    );
    consumer
        .try_edit(|draft| {
            draft.history_state_id = Some(3);
            draft.previous_history_state_id = Some(1);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let scopes = vec![first_internal, second_internal, consumer];
    let timeline = DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            0,
            vec![crate::records::identity::Located {
                value: 30,
                offset: 0,
            }],
        ),
        crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(2).unwrap(),
    )
    .unwrap();
    let result = crate::test_support::with_decode_context(|decode_ctx| {
        project_parameter_design_with_edge_identities(
            decode_ctx,
            &crate::design::feature_project::ProjectInputs {
                scopes: &scopes,
                timelines: std::slice::from_ref(&timeline),
                ..Default::default()
            },
        )
    });
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::Malformed(_))
    ));
}

#[test]
fn feature_projection_does_not_invent_an_ambiguous_internal_dependency() {
    let stream = "f3d:Design/BulkStream.dat";
    let mut predecessor = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        100,
    );
    predecessor
        .try_edit(|draft| {
            draft.history_state_id = Some(7);
        })
        .unwrap();
    let internal = |record_index| {
        let mut scope = DesignParameterScope::empty(
            &format!("{stream}:design-parameter-scope#{record_index}"),
            crate::records::feature::scope::DesignFeatureKind::BaseFeature,
            record_index,
        );
        scope
            .try_edit(|draft| {
                draft.history_state_id = Some(8);
                draft.previous_history_state_id = Some(7);
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope
    };
    let mut successor = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#200"),
        crate::records::feature::scope::DesignFeatureKind::Fillet,
        200,
    );
    successor
        .try_edit(|draft| {
            draft.history_state_id = Some(9);
            draft.previous_history_state_id = Some(8);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let scopes = vec![predecessor, internal(150), internal(160), successor.clone()];
    let timeline = DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            0,
            vec![
                crate::records::identity::Located {
                    value: 100,
                    offset: 0,
                },
                crate::records::identity::Located {
                    value: 200,
                    offset: 0,
                },
            ],
        ),
        crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(2).unwrap(),
    )
    .unwrap();
    let (features, _) = crate::test_support::with_decode_context(|decode_ctx| {
        project_parameter_design_with_edge_identities(
            decode_ctx,
            &crate::design::feature_project::ProjectInputs {
                scopes: &scopes,
                timelines: std::slice::from_ref(&timeline),
                ..Default::default()
            },
        )
    })
    .expect("ambiguous internal state chain remains unresolved");

    let successor = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(successor.id.as_str()))
        .expect("projected successor");
    assert!(successor.dependencies.is_empty());
}

#[test]
fn timeline_less_feature_family_uses_complete_family_ordinals() {
    let stream = "f3d:Design/BulkStream.dat";
    let mut first = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        100,
    );
    first.feature_ordinal = std::num::NonZeroU32::new(1).expect("nonzero ordinal");
    let mut second = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#200"),
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        200,
    );
    second.feature_ordinal = std::num::NonZeroU32::new(2).expect("nonzero ordinal");
    let scopes = vec![second.clone(), first.clone()];
    let ordinals = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::feature_project::authored_scope_ordinals(decode_ctx, &scopes, &[])
    })
    .expect("complete family ordinals carry exact order");
    assert_eq!(ordinals[&(stream, first.record_index)], 0);
    assert_eq!(ordinals[&(stream, second.record_index)], 1);

    let mut mixed = second;
    mixed
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Fillet
                .try_into()
                .unwrap();
        })
        .unwrap();
    let mixed_scopes = vec![first, mixed];
    let error = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::feature_project::authored_scope_ordinals(decode_ctx, &mixed_scopes, &[])
    })
    .expect_err("mixed families have no timeline-independent total order");
    assert!(error
        .to_string()
        .contains("no complete authored timeline order"));
}

#[test]
fn authored_scope_validation_orders_independent_streams_separately() {
    let mut first = DesignParameterScope::empty(
        "f3d:DesignA/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        10,
    );
    first.feature_ordinal = std::num::NonZeroU32::new(1).expect("nonzero ordinal");
    let mut second = DesignParameterScope::empty(
        "f3d:DesignB/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::Fillet,
        10,
    );
    second.feature_ordinal = std::num::NonZeroU32::new(1).expect("nonzero ordinal");
    let scopes = vec![first, second];

    let ordinals = crate::test_support::with_decode_context(|decode_ctx| {
        crate::design::feature_project::authored_scope_ordinals_per_stream(decode_ctx, &scopes, &[])
    })
    .expect("independent stream-local orders");
    assert_eq!(ordinals.len(), 2);
    assert!(ordinals.values().all(|ordinal| *ordinal == 0));
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::feature_project::authored_scope_ordinals(decode_ctx, &scopes, &[])
        }),
        Err(cadmpeg_core::CodecError::NotImplemented(_))
    ));
}

#[test]
fn move_matrix_decomposes_to_translation_and_axis_angle() {
    let angle = std::f64::consts::PI / 3.0;
    let transform: [[f64; 4]; 4] = [
        [angle.cos(), 0.0, angle.sin(), -14.0],
        [0.0, 1.0, 0.0, 2.0],
        [-angle.sin(), 0.0, angle.cos(), 9.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let rotation = crate::design::feature_project::matrix_axis_angle(&transform)
        .expect("nonidentity rotation");
    assert!((rotation.angle.get() - angle).abs() <= 1.0e-12);
    assert!((rotation.direction.x - 0.0).abs() <= 1.0e-12);
    assert!((rotation.direction.y - 1.0).abs() <= 1.0e-12);
    assert!((rotation.direction.z - 0.0).abs() <= 1.0e-12);
    assert_eq!(
        crate::design::feature_project::matrix_axis_angle(
            &crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows()
        ),
        None
    );
}

#[test]
fn history_state_identity_orders_cross_family_feature_dependencies() {
    let scope = |record_index, byte_offset, kind: &str, current, previous| {
        DesignParameterScope::try_new(
            crate::records::feature::scope::DesignParameterScopeDraft {
                id: format!("f3d:native/BulkStream.dat:scope#{record_index}"),
                byte_offset,
                class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                    .unwrap(),
                record_index,
                frame_length: 200,
                kind_offset: byte_offset + 100,
                feature_ordinal: std::num::NonZeroU32::MIN,
                feature_ordinal_offset: 0,
                history_state_id: current,

                previous_history_state_id: previous,
                previous_history_state_id_offset: Some(byte_offset + 120),
                reference_count_offset: byte_offset + 80,
                reference_members: crate::records::identity::ReferenceRun::from_columns(
                    vec![1],
                    vec![0],
                    "reference_members",
                )
                .unwrap(),
                payload: crate::records::feature::scope::DesignFeatureKind::try_from(
                    kind.to_owned(),
                )
                .expect("nonempty family name")
                .try_into()
                .unwrap(),
                unclosed_construction_operand_groups: Vec::new(),
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "261".to_owned(),
                )
                .unwrap(),
                paired_byte_offset: byte_offset + 200,
            }
            .with_fixture_layout(),
        )
        .unwrap()
    };
    let predecessor = scope(12, 200, "Fillet", Some(10), Some(9));
    let successor = scope(22, 100, "Chamfer", Some(11), Some(10));
    let parameter = |owner_record_index, record_index, expression: &str, name: &str| {
        let mut parameter = parse_design_parameter_record(&parameter_record(
            Some(owner_record_index),
            expression,
            "FeatureInput",
            Some("mm"),
            name,
            1.0,
        ))
        .expect("generated history-ordered parameter");
        parameter.id = format!("f3d:native/BulkStream.dat:parameter#{record_index}");
        parameter.record_index = record_index;
        parameter.source_ordinal = record_index;
        parameter
    };
    let owner = |record_index, parameter_record_index, scope_record_index| {
        crate::records::parameters::DesignParameterOwner::try_from(
            crate::records::parameters::DesignParameterOwnerWire {
                id: format!("f3d:native/BulkStream.dat:owner#{record_index}"),
                byte_offset: 0,
                frame_length: 104,
                class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned())
                    .unwrap(),
                record_index,
                scope_record_index,
                local_ordinal: parameter_record_index,
                evaluated_value: 1.0,
                evaluated_value_offset: 40,
                parameter_record_index,
                owned_ordinal: parameter_record_index,
                variant: Some(0),
                companion_record_index: record_index + 2,
            },
        )
        .unwrap()
    };
    let parameters = [
        parameter(44, 45, "10 mm", "Width"),
        parameter(54, 55, "Width / 2", "Depth"),
    ];
    let owners = [owner(44, 45, 12), owner(54, 55, 22)];
    let scopes = vec![successor, predecessor];
    let timeline = DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream("f3d:native/BulkStream.dat", 0),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            0,
            vec![
                crate::records::identity::Located {
                    value: 12,
                    offset: 0,
                },
                crate::records::identity::Located {
                    value: 22,
                    offset: 0,
                },
            ],
        ),
        crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(1).unwrap(),
    )
    .unwrap();
    let (features, parameters) = crate::test_support::with_decode_context(|decode_ctx| {
        project_parameter_design_with_edge_identities(
            decode_ctx,
            &crate::design::feature_project::ProjectInputs {
                native: &parameters,
                owners: &owners,
                scopes: &scopes,
                timelines: std::slice::from_ref(&timeline),
                ..Default::default()
            },
        )
    })
    .expect("authored cross-family timeline");
    let predecessor = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("f3d:native/BulkStream.dat:scope#12"))
        .expect("predecessor feature");
    let successor = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some("f3d:native/BulkStream.dat:scope#22"))
        .expect("successor feature");
    assert_eq!(successor.dependencies.as_slice(), [predecessor.id.clone()]);
    assert!(predecessor.ordinal < successor.ordinal);
    let width = parameters
        .iter()
        .find(|parameter| parameter.name == "Width")
        .expect("predecessor Width parameter");
    let depth = parameters
        .iter()
        .find(|parameter| parameter.name == "Depth")
        .expect("successor Depth parameter");
    assert_eq!(depth.dependencies.as_slice(), [width.id.clone()]);
}

#[test]
fn numerical_audit_half_turn_recovers_axis_with_zero_x() {
    let transform = [
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, -1.0, 0.0],
        [0.0, -1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let rotation = crate::design::feature_project::matrix_axis_angle(&transform).unwrap();
    assert_eq!(rotation.angle.get(), std::f64::consts::PI);
    let axis = [
        rotation.direction.x,
        rotation.direction.y,
        rotation.direction.z,
    ];
    for row in 0..3 {
        for column in 0..3 {
            let reconstructed =
                2.0 * axis[row] * axis[column] - (if row == column { 1.0 } else { 0.0 });
            assert!((reconstructed - transform[row][column]).abs() <= 8.0 * f64::EPSILON);
        }
    }
}

const SMALL_MATRIX_ROTATION: f64 = 1.0e-8;
#[test]
fn numerical_seventh_matrix_angle_preserves_shallow_rotations() {
    for angle in [
        SMALL_MATRIX_ROTATION,
        -SMALL_MATRIX_ROTATION,
        std::f64::consts::PI,
    ] {
        let (sine, cosine) = angle.sin_cos();
        let matrix = [
            [cosine, -sine, 0.0, 0.0],
            [sine, cosine, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let rotation = crate::design::feature_project::matrix_axis_angle(&matrix).unwrap();
        assert!((rotation.angle.get() - angle.abs()).abs() <= 8.0 * f64::EPSILON * angle.abs());
        if angle.abs() < 1.0 {
            assert_eq!(rotation.direction.get().z.signum(), angle.signum());
        }
    }
}

fn feature_dependency_index_fixture() -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::{
        Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    };
    Feature {
        id: FeatureId::mint("f3d:model:feature#dependency-index").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Native {
                kind: "IndexTest".into(),
                parameters: Default::default(),
            },
        )),
        native_ref: None,
    }
}

fn assert_feature_dependency_index_refusal(operation: &'static str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    let feature = feature_dependency_index_fixture();
    for limit in 0..3 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match super::super::ensure_feature_dependencies_precede(
            &ctx,
            std::slice::from_ref(&feature),
        ) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {}
            Ok(()) => panic!("expected {operation} refusal, got success"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn feature_dependency_ordinal_index_refuses_collection_limit() {
    assert_feature_dependency_index_refusal("f3d feature dependency ordinal index");
}

#[test]
fn feature_unique_ordinal_index_refuses_collection_limit() {
    assert_feature_dependency_index_refusal("f3d feature unique ordinal index");
}

fn authored_ordinal_limit_fixture() -> (Vec<DesignParameterScope>, DesignFeatureTimeline) {
    let stream = "f3d:Design/BulkStream.dat";
    let mut first = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        10,
    );
    first.feature_ordinal = std::num::NonZeroU32::new(1).unwrap();
    let mut second = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#11",
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        11,
    );
    second.feature_ordinal = std::num::NonZeroU32::new(2).unwrap();
    let timeline = DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            0,
            vec![
                crate::records::identity::Located {
                    value: 10,
                    offset: 0,
                },
                crate::records::identity::Located {
                    value: 11,
                    offset: 0,
                },
            ],
        ),
        crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(1).unwrap(),
    )
    .unwrap();
    (vec![first, second], timeline)
}

fn assert_authored_ordinal_refusal(operation: &'static str, with_timeline: bool, per_stream: bool) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (scopes, timeline) = authored_ordinal_limit_fixture();
    let timelines = if with_timeline {
        std::slice::from_ref(&timeline)
    } else {
        &[]
    };
    for limit in 0..25 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = if per_stream {
            crate::design::feature_project::authored_scope_ordinals_per_stream(
                &ctx, &scopes, timelines,
            )
        } else {
            crate::design::feature_project::authored_scope_ordinals(&ctx, &scopes, timelines)
        };
        match result {
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == operation =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            other => panic!("expected {operation} refusal: {other:?}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn authored_stream_index_refuses_collection_limit() {
    assert_authored_ordinal_refusal("f3d authored stream index", false, true);
}

#[test]
fn authored_stream_scope_refuses_collection_limit() {
    assert_authored_ordinal_refusal("f3d authored stream scope", false, true);
}

#[test]
fn authored_scope_record_index_refuses_collection_limit() {
    assert_authored_ordinal_refusal("f3d authored scope record index", false, true);
}

#[test]
fn authored_scope_order_refuses_collection_limit() {
    assert_authored_ordinal_refusal("f3d authored scope order", false, true);
}

#[test]
fn authored_scope_ordinal_refuses_collection_limit() {
    assert_authored_ordinal_refusal("f3d authored scope ordinal", false, true);
}

#[test]
fn authored_stream_timeline_refuses_collection_limit() {
    assert_authored_ordinal_refusal("f3d authored stream timeline", true, false);
}

#[test]
fn authored_timeline_item_ordinal_refuses_collection_limit() {
    assert_authored_ordinal_refusal("f3d authored timeline item ordinal", true, false);
}

fn history_graph_limit_fixture() -> Vec<DesignParameterScope> {
    let mut first = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        10,
    );
    first
        .try_edit(|draft| draft.history_state_id = Some(7))
        .unwrap();
    let mut second = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#11",
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        11,
    );
    second
        .try_edit(|draft| {
            draft.history_state_id = Some(8);
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        })
        .unwrap();
    vec![first, second]
}

fn assert_history_graph_refusal(operation: &'static str, retained: bool) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let scopes = history_graph_limit_fixture();

    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            if retained {
                ResourceDimension::RetainedBytes
            } else {
                ResourceDimension::CollectionItems
            },
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                if retained {
                    policy.limits.max_retained_bytes = cap;
                } else {
                    policy.limits.max_collection_items = cap;
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                ScopeHistoryGraph::new(&ctx, &scopes, &[], &[], &[], &[])
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems })));
    }
}

#[test]
fn component_history_scope_id_refuses_retained_limit() {
    assert_history_graph_refusal("f3d component history scope id", true);
}

#[test]
fn component_history_namespace_refuses_collection_limit() {
    assert_history_graph_refusal("f3d component history namespace", false);
}

#[test]
fn history_state_stream_refuses_retained_limit() {
    assert_history_graph_refusal("f3d history state stream", true);
}

#[test]
fn history_state_index_refuses_collection_limit() {
    assert_history_graph_refusal("f3d history state index", false);
}

#[test]
fn history_state_scope_refuses_collection_limit() {
    assert_history_graph_refusal("f3d history state scope", false);
}

#[test]
fn history_lookup_stream_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let scopes = history_graph_limit_fixture();
    let graph = crate::test_support::with_decode_context(|decode_ctx| {
        ScopeHistoryGraph::new(decode_ctx, &scopes, &[], &[], &[], &[])
    })
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = graph.state_key(&ctx, &scopes[1], 8).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::RetainedBytes
            && failure.operation == "f3d history lookup stream"));
}

#[test]
fn predecessor_stream_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let scopes = history_graph_limit_fixture();
    let graph = crate::test_support::with_decode_context(|decode_ctx| {
        ScopeHistoryGraph::new(decode_ctx, &scopes, &[], &[], &[], &[])
    })
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(error) = graph.predecessor(&ctx, &scopes[1], |_| true) else {
        panic!("expected predecessor stream refusal");
    };
    assert!(matches!(error, CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::MaterializedBytes
            && failure.operation == "f3d predecessor stream"));
}

#[test]
fn predecessor_visited_scope_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let scopes = history_graph_limit_fixture();
    let graph = crate::test_support::with_decode_context(|decode_ctx| {
        ScopeHistoryGraph::new(decode_ctx, &scopes, &[], &[], &[], &[])
    })
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(error) = graph.predecessor(&ctx, &scopes[1], |_| false) else {
        panic!("expected predecessor visited-scope refusal");
    };
    assert!(matches!(error, CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d predecessor visited scope"));
}

fn assert_projected_feature_refusal(operation: &'static str, retained: bool) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (scopes, timeline) = authored_ordinal_limit_fixture();
    let unit = if operation == "f3d projected parameter unit" {
        "custom"
    } else {
        "mm"
    };
    let expression_lookup = operation.starts_with("f3d expression ");
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(40),
        if expression_lookup {
            "Width / 2"
        } else {
            "1 mm"
        },
        "FeatureInput",
        Some(unit),
        "InternalValue",
        0.1,
    ))
    .unwrap();
    parameter.id = "f3d:Design/BulkStream.dat:design-parameter#41".to_owned();
    parameter.record_index = 41;
    parameter
        .try_set_source(
            crate::records::parameters::DesignParameterSource::new(
                parameter.source_kind().to_owned(),
                Some(40),
                parameter.family_discriminator(),
            )
            .unwrap(),
        )
        .unwrap();
    let owner = crate::records::parameters::DesignParameterOwner::try_from(
        crate::records::parameters::DesignParameterOwnerWire {
            id: "f3d:Design/BulkStream.dat:design-parameter-owner#40".to_owned(),
            byte_offset: 0,
            frame_length: 103,
            class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned())
                .unwrap(),
            record_index: 40,
            scope_record_index: 10,
            local_ordinal: 0,
            evaluated_value: 0.1,
            evaluated_value_offset: 40,
            parameter_record_index: 41,
            owned_ordinal: 0,
            variant: None,
            companion_record_index: 42,
        },
    )
    .unwrap();
    let document_alias = operation.starts_with("f3d document alias");
    let owners = if document_alias {
        &[][..]
    } else {
        std::slice::from_ref(&owner)
    };
    let mut native = vec![parameter];
    if expression_lookup {
        let mut width = parse_design_parameter_record(&parameter_record(
            None,
            "2 mm",
            "User Parameter",
            Some("mm"),
            "Width",
            0.2,
        ))
        .unwrap();
        width.id = "f3d:Design/BulkStream.dat:design-parameter#42".to_owned();
        width.record_index = 42;
        native.push(width);
    }
    let materialized = expression_lookup
        || (retained
            && matches!(
                operation,
                "f3d feature history state id"
                    | "f3d parameter scope index id"
                    | "f3d feature alias owner id"
                    | "f3d feature alias name"
                    | "f3d feature alias parameter id"
                    | "f3d owned alias name"
                    | "f3d owned alias parameter id"
                    | "f3d document alias name"
                    | "f3d document alias parameter id"
                    | "f3d parameter owner index id"
                    | "f3d parameter owner index owner id"
                    | "f3d feature order index id"
            ));

    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            if materialized {
                ResourceDimension::MaterializedBytes
            } else if retained {
                ResourceDimension::RetainedBytes
            } else {
                ResourceDimension::CollectionItems
            },
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                if materialized {
                    policy.limits.max_materialized_bytes = cap;
                } else if retained {
                    policy.limits.max_retained_bytes = cap;
                } else {
                    policy.limits.max_collection_items = cap;
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = project_parameter_design_with_edge_identities(
                    &ctx,
                    &crate::design::feature_project::ProjectInputs {
                        native: &native,
                        owners,
                        scopes: &scopes,
                        timelines: std::slice::from_ref(&timeline),
                        ..Default::default()
                    },
                );
                result
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if materialized { ResourceDimension::MaterializedBytes } else if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems })));
    }
}

#[test]
fn projected_scope_id_index_refuses_collection_limit() {
    assert_projected_feature_refusal("f3d projected scope id index", false);
}

#[test]
fn projected_parameter_owner_index_refuses_collection_limit() {
    assert_projected_feature_refusal("f3d projected parameter owner index", false);
}

#[test]
fn projected_scope_parameter_refuses_collection_limit() {
    assert_projected_feature_refusal("f3d projected scope parameter", false);
}

#[test]
fn projected_feature_output_refuses_collection_limit() {
    assert_projected_feature_refusal("f3d projected feature output", false);
}

#[test]
fn projected_feature_id_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d projected feature id", true);
}

#[test]
fn projected_feature_native_ref_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d projected feature native reference", true);
}

#[test]
fn projected_parameter_property_refuses_collection_limit() {
    assert_projected_feature_refusal("f3d projected parameter property", false);
}

#[test]
fn projected_parameter_source_kind_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d projected parameter source kind", true);
}

#[test]
fn projected_parameter_unit_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d projected parameter unit", true);
}

#[test]
fn projected_parameter_owner_id_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d projected parameter owner id", true);
}

#[test]
fn projected_parameter_name_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d projected parameter name", true);
}

#[test]
fn projected_parameter_expression_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d projected parameter expression", true);
}

#[test]
fn projected_parameter_native_ref_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d projected parameter native reference", true);
}

#[test]
fn projected_parameter_output_refuses_collection_limit() {
    assert_projected_feature_refusal("f3d projected parameter output", false);
}

#[test]
fn parameter_scope_index_id_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d parameter scope index id", true);
}

#[test]
fn parameter_scope_index_refuses_collection_limit() {
    assert_projected_feature_refusal("f3d parameter scope index", false);
}

#[test]
fn feature_alias_owner_id_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d feature alias owner id", true);
}

#[test]
fn feature_alias_name_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d feature alias name", true);
}

#[test]
fn feature_alias_parameter_id_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d feature alias parameter id", true);
}

#[test]
fn feature_alias_index_refuses_collection_limit() {
    assert_projected_feature_refusal("f3d feature alias index", false);
}

#[test]
fn owned_alias_name_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d owned alias name", true);
}

#[test]
fn owned_alias_parameter_id_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d owned alias parameter id", true);
}

#[test]
fn owned_alias_index_refuses_collection_limit() {
    assert_projected_feature_refusal("f3d owned alias index", false);
}

#[test]
fn owned_alias_member_refuses_collection_limit() {
    assert_projected_feature_refusal("f3d owned alias member", false);
}

#[test]
fn document_alias_name_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d document alias name", true);
}

#[test]
fn document_alias_parameter_id_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d document alias parameter id", true);
}

#[test]
fn document_alias_index_refuses_collection_limit() {
    assert_projected_feature_refusal("f3d document alias index", false);
}

#[test]
fn parameter_owner_index_id_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d parameter owner index id", true);
}

#[test]
fn parameter_owner_index_owner_id_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d parameter owner index owner id", true);
}

#[test]
fn parameter_owner_index_refuses_collection_limit() {
    assert_projected_feature_refusal("f3d parameter owner index", false);
}

#[test]
fn feature_order_index_id_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d feature order index id", true);
}

#[test]
fn feature_order_index_refuses_collection_limit() {
    assert_projected_feature_refusal("f3d feature order index", false);
}

fn assert_expression_dependency_refusal(operation: &'static str, retained: bool) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let (scopes, timeline) = authored_ordinal_limit_fixture();
    let parameter = |record_index, expression: &str, name: &str| {
        let mut parameter = parse_design_parameter_record(&parameter_record(
            None,
            expression,
            "User Parameter",
            Some("mm"),
            name,
            1.0,
        ))
        .unwrap();
        parameter.id = format!("f3d:Design/BulkStream.dat:design-parameter#{record_index}");
        parameter.record_index = record_index;
        parameter
    };
    let native = [
        parameter(40, "1 mm", "Width"),
        parameter(41, "Width / 2", "Half"),
    ];
    let materialized = operation == "f3d expression identifier lookup";

    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            if materialized {
                ResourceDimension::MaterializedBytes
            } else if retained {
                ResourceDimension::RetainedBytes
            } else {
                ResourceDimension::CollectionItems
            },
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                if materialized {
                    policy.limits.max_materialized_bytes = cap;
                } else if retained {
                    policy.limits.max_retained_bytes = cap;
                } else {
                    policy.limits.max_collection_items = cap;
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = project_parameter_design_with_edge_identities(
                    &ctx,
                    &crate::design::feature_project::ProjectInputs {
                        native: &native,
                        scopes: &scopes,
                        timelines: std::slice::from_ref(&timeline),
                        ..Default::default()
                    },
                );
                result
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if materialized { ResourceDimension::MaterializedBytes } else if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems })));
    }
}

#[test]
fn expression_identifier_lookup_refuses_materialized_limit() {
    assert_expression_dependency_refusal("f3d expression identifier lookup", false);
}

#[test]
fn expression_owner_lookup_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d expression owner lookup", true);
}

#[test]
fn expression_feature_identifier_lookup_refuses_materialized_limit() {
    assert_projected_feature_refusal("f3d expression feature identifier lookup", false);
}

#[test]
fn parameter_dependency_refuses_collection_limit() {
    assert_expression_dependency_refusal("f3d parameter dependency", false);
}

#[test]
fn parameter_dependency_id_refuses_retained_limit() {
    assert_expression_dependency_refusal("f3d parameter dependency id", true);
}

fn assert_history_dependency_refusal(operation: &'static str, retained: bool) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let dimension = if operation == "f3d feature history state id" {
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes
    } else {
        cadmpeg_core::decode::ResourceDimension::RetainedBytes
    };
    let (mut scopes, timeline) = authored_ordinal_limit_fixture();
    scopes[0]
        .try_edit(|draft| draft.history_state_id = Some(7))
        .unwrap();
    scopes[1]
        .try_edit(|draft| {
            draft.history_state_id = Some(8);
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        })
        .unwrap();

    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            if retained {
                dimension
            } else {
                ResourceDimension::CollectionItems
            },
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                if retained {
                    if operation == "f3d feature history state id" {
                        policy.limits.max_materialized_bytes = cap;
                    } else {
                        policy.limits.max_retained_bytes = cap;
                    }
                } else {
                    policy.limits.max_collection_items = cap;
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = project_parameter_design_with_edge_identities(
                    &ctx,
                    &crate::design::feature_project::ProjectInputs {
                        scopes: &scopes,
                        timelines: std::slice::from_ref(&timeline),
                        ..Default::default()
                    },
                );
                result
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { dimension } else { ResourceDimension::CollectionItems })));
    }
}

#[test]
fn feature_history_state_index_refuses_collection_limit() {
    assert_history_dependency_refusal("f3d feature history state index", false);
}

#[test]
fn feature_history_state_id_refuses_retained_limit() {
    assert_history_dependency_refusal("f3d feature history state id", true);
}

#[test]
fn feature_dependency_refuses_collection_limit() {
    assert_history_dependency_refusal("f3d feature dependency", false);
}

#[test]
fn feature_dependency_id_refuses_retained_limit() {
    assert_history_dependency_refusal("f3d feature dependency id", true);
}

#[test]
fn projected_feature_name_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d projected feature name", true);
}

#[test]
fn projected_feature_source_tag_refuses_retained_limit() {
    assert_projected_feature_refusal("f3d projected feature source tag", true);
}
