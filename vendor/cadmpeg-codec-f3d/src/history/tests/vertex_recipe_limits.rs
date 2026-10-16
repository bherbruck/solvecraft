// SPDX-License-Identifier: Apache-2.0
//! Decode limits for vertex recipe candidate selection.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn recipe_fixture() -> (
    crate::records::feature::work_geometry::DesignVertexRecipe,
    crate::history_records::AsmHistoricalTopology,
) {
    use crate::records::dimensions::DesignRecipeReference;
    use crate::records::feature::work_geometry::{DesignVertexRecipe, DesignVertexRecipeDraft};
    let reference = DesignRecipeReference {
        selector: 1,
        selector_offset: 0,
        token: "10".into(),
        token_offset: 0,
        design_reference: 200,
        design_reference_offset: 0,
        candidate_faces: vec![
            cadmpeg_ir::ids::FaceId::mint(crate::ids::brep_entity_id(10)).unwrap(),
        ],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    };
    let recipe = DesignVertexRecipe::try_new(DesignVertexRecipeDraft {
        record_index: 202,
        byte_offset: 0,
        class_tag: "369".to_owned().try_into().unwrap(),
        paired_byte_offset: 16,
        paired_class_tag: "261".to_owned().try_into().unwrap(),
        recipe_record_index: 205,
        recipe_record_byte_offset: 32,
        recipe_id: "f3d:design:construction-recipe#vertex".into(),
        recipe_prefix_offset: 43,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: vec![reference],
        recipe_program_offset: 4,
        recipe_program: vec![0],
        resolution: None,
        next_record_index: 207,
        next_byte_offset: 200,
    })
    .unwrap();
    let topology = crate::history_records::AsmHistoricalTopology {
        faces: vec![10],
        ..Default::default()
    };
    (recipe, topology)
}

fn select(
    max_items: u64,
) -> Result<Option<(i64, cadmpeg_ir::math::Point3)>, cadmpeg_core::CodecError> {
    let (recipe, topology) = recipe_fixture();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::vertex_recipe_candidate(
        &ctx,
        &recipe,
        &topology,
        &mut crate::history::topology_cache::TopologyQueryCache::default(),
    )
}

fn bind_input_states(
    max_items: u64,
    max_retained_bytes: u64,
) -> Result<(), cadmpeg_core::CodecError> {
    use crate::records::entity_header::{DesignFeatureTimeline, DesignTimelineFrame};
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let stream = "f3d:Design/BulkStream.dat";
    let mut source = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        DesignFeatureKind::Extrude,
        100,
    );
    source
        .try_edit(|draft| {
            draft.history_state_id = Some(4);
        })
        .unwrap();
    let target = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#200"),
        DesignFeatureKind::WorkPoint,
        200,
    );
    let timeline = DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
        DesignTimelineFrame::test_items(
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
        "256".to_owned().try_into().unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(1).unwrap(),
    )
    .unwrap();
    let mut scopes = vec![source, target];
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained_bytes;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::bind_vertex_recipe_history(&ctx, &mut scopes, &[timeline], &[])
}

#[test]
fn vertex_recipe_candidate_faces_refuse_collection_limit() {
    let error = select(0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D vertex recipe candidate faces")
    );
}

#[test]
fn vertex_recipe_face_slots_refuse_collection_limit() {
    let error = select(1).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D vertex recipe face slots")
    );
}

#[test]
fn vertex_recipe_scope_id_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D vertex recipe scope identity",
        |cap| bind_input_states(u64::MAX, cap),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D vertex recipe scope identity")
    );
}

#[test]
fn vertex_recipe_input_states_refuse_collection_limit() {
    let error = bind_input_states(12, u64::MAX).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D vertex recipe input states")
    );
}
