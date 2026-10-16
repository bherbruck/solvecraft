// SPDX-License-Identifier: Apache-2.0

use super::super::{bind_extrude_profile_selections, SketchCurveSelectionResolution};
use super::*;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, Feature,
    FeatureDefinition, FeatureId, FeatureOperation, LinearTermination, PlanarProfileRef,
    ProfileRef,
};
use cadmpeg_ir::sketches::SpatialSketchId;
use std::collections::BTreeMap;

fn binder_scope() -> DesignParameterScope {
    DesignParameterScope::try_new(crate::records::feature::scope::DesignParameterScopeDraft {
        id: "f3d:Design/BulkStream.dat:scope#7".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned()).unwrap(),
        record_index: 7,
        frame_length: 200,
        kind_offset: 43,
        payload: crate::records::feature::scope::DesignScopePayload::Face,
        feature_ordinal: std::num::NonZeroU32::MIN,
        feature_ordinal_offset: 128,
        history_state_id: None,
        previous_history_state_id: None,
        previous_history_state_id_offset: None,
        reference_count_offset: 20,
        reference_members: crate::records::identity::ReferenceRun::from_columns(
            vec![9],
            vec![25],
            "reference_members",
        )
        .unwrap(),
        unclosed_construction_operand_groups: Vec::new(),
        paired_class_tag: crate::records::references::DesignClassTag::try_from("261".to_owned())
            .unwrap(),
        paired_byte_offset: 200,
    })
    .unwrap()
}

fn binder_group() -> DesignExtrudeSelectionGroup {
    DesignExtrudeSelectionGroup::try_from(
        crate::records::topology::extrude_selection::DesignExtrudeSelectionGroupWire {
            id: "f3d:Design/BulkStream.dat:selection-group#9".into(),
            scope_record_index: 7,
            scope_reference_ordinal: 0,
            record_index: 9,
            byte_offset: 0,
            class_tag: "277".to_owned(),
            member_count_offset: 32,
            members: vec![10],
            member_offsets: vec![37],
            opaque_index: 1,
            opaque_index_offset: 47,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 51,
            variant: false,
            paired_class_tag: "277".to_owned(),
            paired_byte_offset: 100,
        },
    )
    .unwrap()
}

fn binder_feature(scope: &DesignParameterScope, profile: ProfileRef) -> Feature {
    Feature {
        id: FeatureId::mint("f3d:model:feature#9").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile,
                direction: ExtrudeDirection::default(),
                start: ExtrudeStart::default(),
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::ThroughAll {},
                        draft: None,
                    },
                },
                op: BooleanOp::NewBody,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            }),
        ),
        native_ref: Some(scope.id.clone()),
    }
}

fn binder_sketch(id: SketchId) -> Sketch {
    Sketch {
        id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }
}

fn binder_spatial_member() -> DesignExtrudeSelectionMember {
    DesignExtrudeSelectionMember::try_new(
        crate::records::topology::extrude_selection::DesignExtrudeSelectionMemberDraft {
            id: "f3d:Design/BulkStream.dat:selection-member#10".into(),
            group_record_index: 9,
            group_member_ordinal: 0,
            record_index: 10,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("278".to_owned())
                .unwrap(),
            local_id: 200,
            local_id_offset: 21,
            asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 33,
            context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 109,
            tail_slot_present: false,
            tail_slot_offset: 0,
            resolved_geometry: Some(SketchRelationOperand::Curve {
                record_index: 20,
                primary_id: 200,
                secondary_id: 0,
            }),
            operand_identity_ids: Vec::new(),
            historical: None,
            next_record_index: 11,
            next_byte_offset: 190,
        },
    )
    .unwrap()
}

fn assert_extrude_binder_refusal(operation: &'static str, mode: u8, retained: bool) {
    let scope = binder_scope();
    let group = binder_group();
    let sketch_id = SketchId::mint("f3d:model:sketch#1").unwrap();
    let profile = if mode == 0 {
        ProfileRef::Planar(PlanarProfileRef::Native(group.id.clone()))
    } else {
        ProfileRef::Planar(PlanarProfileRef::Sketch(sketch_id.clone()))
    };
    let sketch = binder_sketch(sketch_id);
    let sketches = if mode == 1 {
        std::slice::from_ref(&sketch)
    } else {
        &[]
    };
    let spatial_id = SpatialSketchId::mint("f3d:model:spatial-sketch#1").unwrap();
    let spatial_sketch = SpatialSketch {
        id: spatial_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        profiles: if mode == 4 {
            vec![spatial_profile(&spatial_id, &[200])]
        } else {
            Vec::new()
        },
        native_ref: None,
    };
    let spatial_sketches = if mode == 2 || mode == 4 {
        std::slice::from_ref(&spatial_sketch)
    } else {
        &[]
    };
    let member = binder_spatial_member();
    let members = if mode == 4 {
        std::slice::from_ref(&member)
    } else {
        &[]
    };
    let arrangement_budget = WorkBudget::new(crate::design::geometry::MAX_ARRANGEMENT_WALK_WORK);
    let scope_histories = HashMap::new();
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            if retained {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes
            } else {
                cadmpeg_core::decode::ResourceDimension::CollectionItems
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
                let mut feature = binder_feature(&scope, profile.clone());
                let curve_resolution = SketchCurveSelectionResolution {
                    scopes: &[],
                    groups: &[],
                    operands: &[],
                    placements: &[],
                    curve_identities: &[],
                    sketches,
                    sketch_entities: &[],
                    spatial_sketches,
                    spatial_sketch_entities: &[],
                };
                let resolution = ExtrudeProfileResolution {
                    entities: &[],
                    spatial_sketches,
                    spatial_entities: &[],
                    histories: &[],
                    scope_histories: &scope_histories,
                    linear_tolerance: 0.000_001,
                    angular_tolerance: 0.000_000_001,
                    arrangement_budget: &arrangement_budget,
                    ctx: &ctx,
                };
                bind_extrude_profile_selections(
                    std::slice::from_mut(&mut feature),
                    std::slice::from_ref(&scope),
                    std::slice::from_ref(&group),
                    members,
                    sketches,
                    &curve_resolution,
                    resolution,
                )
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { cadmpeg_core::decode::ResourceDimension::RetainedBytes } else { cadmpeg_core::decode::ResourceDimension::CollectionItems })));
    }
}

#[test]
fn extrude_matching_group_refuses_collection_limit() {
    assert_extrude_binder_refusal("f3d extrude matching selection group", 0, false);
}

#[test]
fn extrude_resolved_selection_refuses_collection_limit() {
    assert_extrude_binder_refusal("f3d extrude resolved selection", 1, false);
}

#[test]
fn extrude_fallback_group_id_refuses_retained_limit() {
    assert_extrude_binder_refusal("f3d extrude fallback group id", 1, true);
}

#[test]
fn extrude_fallback_group_refuses_collection_limit() {
    assert_extrude_binder_refusal("f3d extrude fallback group", 1, false);
}

#[test]
fn extrude_unresolved_selection_id_refuses_retained_limit() {
    assert_extrude_binder_refusal("f3d extrude unresolved selection id", 3, true);
}

#[test]
fn spatial_extrude_selection_refuses_collection_limit() {
    assert_extrude_binder_refusal("f3d spatial profile selection", 2, false);
}

#[test]
fn spatial_extrude_group_id_refuses_retained_limit() {
    assert_extrude_binder_refusal("f3d spatial extrude selection group id", 2, true);
}

#[test]
fn spatial_extrude_group_refuses_collection_limit() {
    assert_extrude_binder_refusal("f3d spatial extrude selection group", 2, false);
}

#[test]
fn spatial_extrude_sketch_id_refuses_retained_limit() {
    assert_extrude_binder_refusal("f3d profile spatial sketch id", 2, true);
}

#[test]
fn spatial_extrude_profile_index_refuses_collection_limit() {
    assert_extrude_binder_refusal("f3d spatial extrude profile index", 4, false);
}
