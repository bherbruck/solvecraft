// SPDX-License-Identifier: Apache-2.0

use super::super::{
    legacy_face_recipe_reference_candidates, resolved_explicit_bounded_face_group,
    resolved_extrude_profile_face_group, resolved_face_group,
};
use super::{face, reference, stable_bounded_face_operand};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::recipes::ConstructionRecipeKind;
use crate::records::topology::{
    construction::DesignConstructionOperandGroup, face::DesignFaceOperand,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::FaceId;

#[test]
fn historical_referenced_face_candidates_refuse_limits() {
    let mut operand = stable_bounded_face_operand();
    operand.recipe_kind = ConstructionRecipeKind::Face;
    operand.recipe_references = vec![reference(10, "selected", 1)];
    for (retained, items, dimension, operation) in [
        (
            0,
            1,
            ResourceDimension::RetainedBytes,
            "f3d historical face candidate id",
        ),
        (
            100,
            0,
            ResourceDimension::CollectionItems,
            "f3d historical face candidate",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = retained;
        policy.limits.max_collection_items = items;
        let refusal_cap =
            match cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match dimension {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (crate::design::face_resolve::historical_face_operand_candidates(&ctx, &operand))
                    .map(|_| ())
            }) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match dimension {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let refusal_cap =
            match cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match dimension {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (crate::design::face_resolve::historical_face_operand_candidates(&ctx, &operand))
                    .map(|_| ())
            }) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match dimension {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            crate::design::face_resolve::historical_face_operand_candidates(&ctx, &operand),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation
        ));
    }
}

#[test]
fn historical_fallback_face_candidates_refuse_limits() {
    let operand = stable_bounded_face_operand();
    for (retained, items, dimension, operation) in [
        (
            0,
            1,
            ResourceDimension::RetainedBytes,
            "f3d historical fallback face id",
        ),
        (
            100,
            0,
            ResourceDimension::CollectionItems,
            "f3d historical fallback face",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = retained;
        policy.limits.max_collection_items = items;
        let refusal_cap =
            match cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match dimension {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (crate::design::face_resolve::historical_face_operand_candidates(&ctx, &operand))
                    .map(|_| ())
            }) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match dimension {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let refusal_cap =
            match cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match dimension {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (crate::design::face_resolve::historical_face_operand_candidates(&ctx, &operand))
                    .map(|_| ())
            }) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match dimension {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            crate::design::face_resolve::historical_face_operand_candidates(&ctx, &operand),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation
        ));
    }
}

#[test]
fn nested_bounded_face_candidates_refuse_limits() {
    let mut operand = stable_bounded_face_operand();
    operand.candidate_faces.clear();
    operand.unreferenced_candidate_faces.clear();
    operand.alternate_selector_candidate_faces.clear();
    operand.recipe_references = vec![reference(10, "selected", 1)];
    for (retained, items, dimension, operation) in [
        (
            0,
            1,
            ResourceDimension::RetainedBytes,
            "f3d nested bounded face candidate id",
        ),
        (
            100,
            0,
            ResourceDimension::CollectionItems,
            "f3d nested bounded face candidate",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = retained;
        policy.limits.max_collection_items = items;
        let refusal_cap =
            match cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match dimension {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (crate::design::face_resolve::nested_bounded_face_history_candidates(
                    &ctx, &operand,
                ))
                .map(|_| ())
            }) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match dimension {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let refusal_cap =
            match cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match dimension {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (crate::design::face_resolve::nested_bounded_face_history_candidates(
                    &ctx, &operand,
                ))
                .map(|_| ())
            }) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match dimension {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            crate::design::face_resolve::nested_bounded_face_history_candidates(&ctx, &operand),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation
        ));
    }
}

#[test]
fn explicit_bounded_face_group_uses_only_its_owned_candidate_lane() {
    crate::test_support::with_decode_context(|decode_ctx| {
        let mut operand: DesignFaceOperand = serde_json::from_value(serde_json::json!({
        "id": "f3d:test:face-operand#200",
        "scope_record_index": 100,
        "scope_reference_ordinal": 0,
        "group_record_index": 150,
        "group_member_ordinal": 0,
        "record_index": 200,
        "byte_offset": 0,
        "class_tag": "346",
        "paired_byte_offset": 325,
        "paired_class_tag": "262",
        "recipe_record_index": 203,
        "recipe_record_byte_offset": 341,
        "recipe_id": "f3d:test:recipe#201",
        "recipe_prefix_offset": 352,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_kind": "bounded_face",
        "recipe_program_offset": 0,
        "recipe_program": [0, -1, 1],
        "recipe_node_offsets": [0],
        "recipe_nodes": [{
            "byte_offset": 0,
            "end_byte_offset": 12,
            "program": [0, -1, 1],
            "recipe_structure": {
                "root": 0,
                "prelude": [0, 0],
                "sides": [
                    {"field_count": 1, "header_value": 0, "payload_entry_count": 0, "payload_prefix": [], "scalars": [], "entries": []},
                    {"field_count": 1, "header_value": 0, "payload_entry_count": 0, "payload_prefix": [], "scalars": [], "entries": []}
                ],
                "postlude": []
            }
        }],
        "candidate_faces": ["f3d:brep:entity#10", "f3d:brep:entity#20"],
        "unreferenced_candidate_faces": [],
        "alternate_selector_candidate_faces": [],
        "preceding_candidate_faces": [],
        "changed_candidate_faces": [],
        "historical_support_contexts": [],
        "resolved_face_slots": [],
        "next_record_index": 202,
        "next_byte_offset": 469
    }))
    .expect("legacy bounded-face operand");
        operand.recipe_references = vec![
            reference(10, "selected-a", 201),
            reference(20, "selected-b", 201),
        ];

        let mut group: DesignConstructionOperandGroup = serde_json::from_value(serde_json::json!({
            "id": "f3d:test:construction-group#150",
            "scope_record_index": 100,
            "scope_reference_ordinal": 0,
            "record_index": 150,
            "byte_offset": 0,
            "class_tag": "346",
            "role": 0x0000_0010_0000_0000_u64,
            "members": [200],
            "member_offsets": [0],
            "frame": {
                "member_count_offset": 0,
                "opaque_index": 1,
                "opaque_index_offset": 18,
                "opaque_scalar": 0.0,
                "opaque_scalar_offset": 22,
                "variant": false
            },
            "role_offset": 0,
            "paired_class_tag": "262",
            "paired_byte_offset": 325,
            "next_record_index": 151,
            "next_byte_offset": 0
        }))
        .expect("legacy Draft face group");

        assert_eq!(
            resolved_explicit_bounded_face_group(decode_ctx, &group, &[operand.clone()])
                .expect("projection resource budget"),
            Some(cadmpeg_ir::features::FaceSelection::Resolved {
                faces: vec![face(10), face(20)],
                native: group.id.clone(),
            })
        );
        group.operand_role =
            crate::records::topology::construction::DesignConstructionOperandRole::ExtrudeProfile;
        let scope: DesignParameterScope = serde_json::from_value(serde_json::json!({
            "id": "f3d:test:scope#100",
            "byte_offset": 0,
            "class_tag": "304",
            "record_index": 100,
            "frame_length": 300,
            "kind": "Extrude",
            "kind_offset": 32,
            "feature_ordinal": 1,
            "feature_ordinal_offset": 228,
            "history_state_id": 2,
            "history_state_id_offset": 24,
            "previous_history_state_id": 1,
            "previous_history_state_id_offset": 258,
            "reference_count_offset": 9,
            "reference_members": [150],
            "reference_member_offsets": [14],
            "paired_class_tag": "258",
            "paired_byte_offset": 300
        }))
        .expect("Extrude scope");
        assert_eq!(
            resolved_extrude_profile_face_group(
                decode_ctx,
                &scope,
                &group,
                std::slice::from_ref(&group),
                &[operand.clone()]
            )
            .unwrap(),
            Some(cadmpeg_ir::features::ProfileRef::Planar(
                cadmpeg_ir::features::PlanarProfileRef::Faces(vec![face(10), face(20),])
            ))
        );
        operand.resolved_active_face =
            Some(FaceId::mint("f3d:brep/legacy/brep:entity#30").expect("identity grammar"));
        assert_eq!(
            resolved_face_group(decode_ctx, &group, std::slice::from_ref(&operand))
                .expect("projection resource budget"),
            Some(cadmpeg_ir::features::FaceSelection::Resolved {
                faces: vec![
                    FaceId::mint("f3d:brep/legacy/brep:entity#30").expect("identity grammar")
                ],
                native: group.id.clone(),
            })
        );
        operand.resolved_active_face = None;
        operand.preceding_candidate_faces = vec![face(10), face(20)];
        assert_eq!(
            resolved_face_group(decode_ctx, &group, std::slice::from_ref(&operand))
                .expect("projection resource budget"),
            None
        );
        operand.preceding_candidate_faces.clear();

        operand.candidate_faces.clear();
        operand.recipe_references = vec![
            reference(30, "context", 202),
            reference(10, "selected-a", 201),
            reference(20, "selected-b", 201),
        ];
        operand.candidate_faces = vec![face(10), face(20)];
        assert_eq!(
            legacy_face_recipe_reference_candidates(decode_ctx, &operand, 201)
                .expect("projection resource budget"),
            Some(vec![face(10), face(20)])
        );
        operand.candidate_faces.clear();
        assert_eq!(
            resolved_explicit_bounded_face_group(decode_ctx, &group, &[operand.clone()])
                .expect("projection resource budget"),
            Some(cadmpeg_ir::features::FaceSelection::Resolved {
                faces: vec![face(10), face(20)],
                native: group.id.clone(),
            })
        );
        assert_eq!(
            legacy_face_recipe_reference_candidates(decode_ctx, &operand, 201)
                .expect("projection resource budget"),
            Some(vec![face(10), face(20)])
        );
        assert!(
            legacy_face_recipe_reference_candidates(decode_ctx, &operand, 999)
                .expect("projection resource budget")
                .is_none()
        );
        operand.recipe_references.remove(0);
        operand.recipe_references[0]
            .alternate_selector_faces
            .push(face(30));
        assert!(
            resolved_explicit_bounded_face_group(decode_ctx, &group, &[operand])
                .expect("projection resource budget")
                .is_none()
        );
    });
}
