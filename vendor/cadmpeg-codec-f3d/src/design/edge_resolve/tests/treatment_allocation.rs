// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::history_records::{
    AsmDeltaState, AsmHistoricalEdge, AsmHistoricalTopology, AsmHistory, AsmTopologyCache,
};
use crate::records::feature::work_geometry::{
    DesignEdgeTreatmentVertexOperand, DesignVertexRecipe, DesignVertexRecipeDraft,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn assert_treatment_refusal(operation: &'static str, retained: bool, valid_corner: bool) {
    let mut selection_group = group(2, 10);
    selection_group
        .try_set_members(vec![
            crate::records::identity::Located {
                value: 10,
                offset: 0,
            },
            crate::records::identity::Located {
                value: 11,
                offset: 11,
            },
        ])
        .unwrap();
    let mut edge = recipe_edge_operand(11, &[], &[]);
    edge.recipe_state_id = Some(7);
    edge.resolved_edge_slot = Some(17);
    let corner = DesignEdgeTreatmentVertexOperand {
        id: "f3d:test:edge-treatment-vertex-operand#10".into(),
        scope_record_index: 1,
        scope_reference_ordinal: 0,
        group_record_index: 2,
        group_member_ordinal: 0,
        recipe: DesignVertexRecipe::try_new(DesignVertexRecipeDraft {
            record_index: 10,
            byte_offset: 10,
            class_tag: crate::records::references::DesignClassTag::try_from("306".to_owned())
                .unwrap(),
            paired_byte_offset: 26,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
            recipe_record_index: 13,
            recipe_record_byte_offset: 42,
            recipe_id: "f3d:test:construction-recipe#10".into(),
            recipe_prefix_offset: 53,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_program_offset: 4,
            recipe_program: vec![0],
            resolution: Some(
                crate::records::feature::work_geometry::DesignVertexResolution::new(
                    7,
                    if valid_corner { 3 } else { 5 },
                )
                .unwrap(),
            ),
            next_record_index: 15,
            next_byte_offset: 210,
        })
        .unwrap(),
    };
    let history = AsmHistory {
        id: "f3d:test:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![AsmDeltaState {
            id: "f3d:test:state#7".into(),
            parent: "f3d:test:history".into(),
            byte_offset: 0,
            state_id: 7,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 7,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            entity_versions: Vec::new(),
            topology_cache: AsmTopologyCache::Complete(AsmHistoricalTopology {
                edges: vec![17],
                vertices: vec![3, 4, 5],
                edge_vertices: vec![AsmHistoricalEdge {
                    edge: 17,
                    start_vertex: 3,
                    end_vertex: 4,
                }],
                ..AsmHistoricalTopology::default()
            }),
            transition: None,
        }],
    };
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#fillet").unwrap();
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
                resolved_edge_treatment_group_with_corners(
                    &selection_group,
                    crate::design::edge_resolve::EdgeTreatmentInputs {
                        groups: std::slice::from_ref(&selection_group),
                        operands: std::slice::from_ref(&edge),
                        identity_operands: &[],
                        vertex_operands: std::slice::from_ref(&corner),
                        histories: std::slice::from_ref(&history),
                        previous_state_id: Some(7),
                        feature_id: &feature_id,
                        treatment_radius: None,
                    },
                    &ctx,
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems })));
    }
}

#[test]
fn treatment_edge_member_refuses_collection_limit() {
    assert_treatment_refusal("f3d treatment edge member", false, true);
}

#[test]
fn treatment_corner_slot_refuses_collection_limit() {
    assert_treatment_refusal("f3d treatment corner slot", false, true);
}

#[test]
fn treatment_endpoint_refuses_collection_limit() {
    assert_treatment_refusal("f3d treatment endpoint vertex", false, true);
}

#[test]
fn treatment_native_group_id_refuses_retained_limit() {
    assert_treatment_refusal("f3d native edge group id", true, false);
}
