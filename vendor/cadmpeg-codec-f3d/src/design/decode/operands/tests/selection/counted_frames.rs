// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::operands::bind_extrude_selection_geometry;
use crate::design::decode::operands::bind_extrude_selection_identities;
use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::operands::parse_extrude_selection_group;
use crate::design::decode::operands::parse_extrude_selection_member;

use crate::design::geometry::MAX_ARRANGEMENT_WALK_WORK;
use crate::design::profile_select::resolved_extrude_profile_selection;
use crate::ids::neutral_sketch_curve_id;
use crate::ids::neutral_sketch_point_id;
use crate::records::decal::DesignRecordHeader;

use crate::records::feature::scope::DesignParameterScope;

use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::sketch_relations::SketchRelationOperand;

use crate::records::topology::construction::DesignConstructionOperandIdentity;
use crate::records::topology::construction::DesignConstructionPersistentIdentity;

use crate::records::topology::sketch_profile::DesignSketchProfileOperand;
use crate::test_support::indexed_header;
use crate::test_support::lp_utf16;
use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::sketches::Sketch;
use cadmpeg_ir::sketches::SketchEntity;
use cadmpeg_ir::sketches::SketchEntityId;
use cadmpeg_ir::sketches::SketchEntityUse;
use cadmpeg_ir::sketches::SketchGeometry;
use cadmpeg_ir::sketches::SketchGeometryDefinition;
use cadmpeg_ir::sketches::SketchId;

#[test]
fn extrude_selection_group_and_members_have_exact_counted_frames() {
    crate::test_support::with_decode_context(|decode_ctx| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scope = DesignParameterScope::try_new(
            crate::records::feature::scope::DesignParameterScopeDraft {
                id: "f3d:Design/BulkStream.dat:scope#12".into(),
                byte_offset: 1000,
                class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                    .unwrap(),
                record_index: 12,
                frame_length: 200,
                kind_offset: 1100,
                feature_ordinal: std::num::NonZeroU32::MIN,
                feature_ordinal_offset: 0,
                history_state_id: None,

                previous_history_state_id: None,
                previous_history_state_id_offset: None,
                reference_count_offset: 1080,
                reference_members: crate::records::identity::ReferenceRun::from_columns(
                    vec![100],
                    vec![1085],
                    "reference_members",
                )
                .unwrap(),
                payload: crate::records::feature::scope::DesignFeatureKind::Extrude
                    .try_into()
                    .unwrap(),
                unclosed_construction_operand_groups: Vec::new(),
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "261".to_owned(),
                )
                .unwrap(),
                paired_byte_offset: 1200,
            }
            .with_fixture_layout(),
        )
        .unwrap();
        let record = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#100".into(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("331".to_owned())
                .unwrap(),
            record_index: 100,
        };
        let mut group_bytes = Vec::new();
        indexed_header(&mut group_bytes, *b"331", 100);
        group_bytes.extend_from_slice(&[0; 10]);
        group_bytes.push(1);
        group_bytes.extend_from_slice(&12u32.to_le_bytes());
        group_bytes.extend_from_slice(&[0; 6]);
        group_bytes.extend_from_slice(&2u32.to_le_bytes());
        for member in [200u32, 201] {
            group_bytes.push(1);
            group_bytes.extend_from_slice(&member.to_le_bytes());
            group_bytes.extend_from_slice(&[0; 6]);
        }
        group_bytes.extend_from_slice(&180u32.to_le_bytes());
        group_bytes.extend_from_slice(&0.25f64.to_le_bytes());
        group_bytes.extend_from_slice(&180u32.to_le_bytes());
        group_bytes.push(1);
        group_bytes.extend_from_slice(&102u32.to_le_bytes());
        group_bytes.extend_from_slice(&[0; 6]);
        group_bytes.extend_from_slice(&[1, 1, 0, 1]);
        group_bytes.extend_from_slice(&101u32.to_le_bytes());
        group_bytes.extend_from_slice(&[0; 7]);
        group_bytes.push(1);
        group_bytes.extend_from_slice(&12u32.to_le_bytes());
        group_bytes.extend_from_slice(&[0; 6]);
        let paired_at = group_bytes.len();
        indexed_header(&mut group_bytes, *b"259", 100);

        let mut group = parse_extrude_selection_group(&ctx, &group_bytes, &scope, 0, &record)
            .expect("group decode resources")
            .expect("counted Extrude selection group");
        assert_eq!(
            group
                .members()
                .iter()
                .map(|member| member.value)
                .collect::<Vec<_>>(),
            [200, 201]
        );
        assert_eq!(group.opaque_index.get(), 180);
        assert_eq!(group.opaque_scalar().get(), 0.25);
        assert!(group.variant);
        assert_eq!(group.paired_byte_offset(), u64_from_index(paired_at));

        let member_record = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#200".into(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("290".to_owned())
                .unwrap(),
            record_index: 200,
        };
        let mut member_bytes = Vec::new();
        indexed_header(&mut member_bytes, *b"290", 200);
        member_bytes.extend_from_slice(&[0; 10]);
        member_bytes.extend_from_slice(&586u64.to_le_bytes());
        lp_utf16(&mut member_bytes, "df9087bd-02a6-4a3f-a132-7e69990f323c");
        lp_utf16(&mut member_bytes, "0b2382d1-caaf-4eb9-b40d-a6322a7ed829");
        member_bytes.extend_from_slice(&2u32.to_le_bytes());
        member_bytes.extend_from_slice(&[0; 5]);
        indexed_header(&mut member_bytes, *b"290", 201);

        for retained_limit in [35, 71] {
            let limited_arena = cadmpeg_core::decode::DecodeArena::new();
            let mut limited_policy = cadmpeg_core::decode::DecodePolicy::default();
            limited_policy.limits.max_retained_bytes = retained_limit;
            let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                "f3d Design UTF-16 text",
                |cap| {
                    let mut limited_policy = cadmpeg_core::decode::DecodePolicy::service();
                    match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
                        cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                            limited_policy.limits.max_retained_bytes = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                            limited_policy.limits.max_collection_items = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                            limited_policy.limits.max_materialized_bytes = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                            limited_policy.limits.max_work_units = cap;
                        }
                        dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                    }
                    let (limited_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                        &[],
                        &limited_arena,
                        &limited_policy,
                    )
                    .unwrap();
                    ((parse_extrude_selection_member(
                        &limited_ctx,
                        &member_bytes,
                        &group,
                        0,
                        &member_record,
                    ))
                    .transpose())
                    .map(|_| ())
                },
            ) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
            limited_policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
            match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                    limited_policy.limits.max_retained_bytes = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    limited_policy.limits.max_collection_items = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    limited_policy.limits.max_materialized_bytes = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    limited_policy.limits.max_work_units = refusal_cap;
                }
                dimension => panic!("unsupported refusal dimension: {dimension:?}"),
            }
            let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                "f3d Design UTF-16 text",
                |cap| {
                    let mut limited_policy = cadmpeg_core::decode::DecodePolicy::service();
                    match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
                        cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                            limited_policy.limits.max_retained_bytes = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                            limited_policy.limits.max_collection_items = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                            limited_policy.limits.max_materialized_bytes = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                            limited_policy.limits.max_work_units = cap;
                        }
                        dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                    }
                    let (limited_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                        &[],
                        &limited_arena,
                        &limited_policy,
                    )
                    .unwrap();
                    ((parse_extrude_selection_member(
                        &limited_ctx,
                        &member_bytes,
                        &group,
                        0,
                        &member_record,
                    ))
                    .transpose())
                    .map(|_| ())
                },
            ) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
            limited_policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
            match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                    limited_policy.limits.max_retained_bytes = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    limited_policy.limits.max_collection_items = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    limited_policy.limits.max_materialized_bytes = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    limited_policy.limits.max_work_units = refusal_cap;
                }
                dimension => panic!("unsupported refusal dimension: {dimension:?}"),
            }
            let (limited_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &limited_arena,
                &limited_policy,
            )
            .unwrap();
            assert!(matches!(
                parse_extrude_selection_member(&limited_ctx, &member_bytes, &group, 0, &member_record),
                Some(Err(cadmpeg_core::CodecError::ResourceLimit(failure)))
                    if failure.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                        && failure.operation == "f3d Design UTF-16 text"
            ));
        }

        let mut member =
            parse_extrude_selection_member(&ctx, &member_bytes, &group, 0, &member_record)
                .expect("fixed Extrude selection member")
                .unwrap();
        assert_eq!(member.local_id, 586);
        assert_eq!(member.next_byte_offset(), 190);
        assert_eq!(member.next_record_index, 201);
        assert!(!member.tail_slot_present);
        assert_eq!(member.tail_slot_offset, 185);

        member_bytes[185] = 1;
        let member_with_slot =
            parse_extrude_selection_member(&ctx, &member_bytes, &group, 0, &member_record)
                .expect("Extrude selection member with present tail slot")
                .unwrap();
        assert!(member_with_slot.tail_slot_present);
        assert_eq!(member_with_slot.tail_slot_offset, 185);

        let terminal_member =
            parse_extrude_selection_member(&ctx, &member_bytes[..190], &group, 0, &member_record)
                .expect("terminal fixed Extrude selection member")
                .unwrap();
        assert_eq!(terminal_member.next_byte_offset(), 190);
        assert_eq!(terminal_member.next_record_index, 0);

        let mut edge_identity_bytes = Vec::new();
        indexed_header(&mut edge_identity_bytes, *b"278", 5887);
        edge_identity_bytes.extend_from_slice(&[0; 12]);
        edge_identity_bytes.push(1);
        edge_identity_bytes.extend_from_slice(&5890u32.to_le_bytes());
        edge_identity_bytes.extend_from_slice(&[0; 6]);
        edge_identity_bytes.extend_from_slice(&1u32.to_le_bytes());
        lp_utf16(
            &mut edge_identity_bytes,
            "ad3001bb-a0fc-44c2-9b7a-c8b8fb70bfc0",
        );
        lp_utf16(
            &mut edge_identity_bytes,
            "1d8b67fc-c638-4af3-b13d-776dce4f472d",
        );
        let edge_identity = crate::design::decode::operands::parse_edge_identity_member(
            &ctx,
            &edge_identity_bytes,
            0,
        )
        .expect("fixed edge-treatment selection identity")
        .unwrap();
        assert_eq!(edge_identity.local_id, 5890);
        assert!(!edge_identity.layout.is_compact());
        assert_eq!(edge_identity.layout.local_id_offset(), 24);
        assert_eq!(edge_identity.asset_id_offset, 42);
        assert_eq!(edge_identity.context_id_offset, 118);

        edge_identity_bytes.remove(22);
        let compact_edge_identity = crate::design::decode::operands::parse_edge_identity_member(
            &ctx,
            &edge_identity_bytes,
            0,
        )
        .expect("compact fixed edge-treatment selection identity")
        .unwrap();
        assert!(compact_edge_identity.layout.is_compact());
        assert_eq!(compact_edge_identity.local_id, 5890);
        assert_eq!(compact_edge_identity.layout.local_id_offset(), 23);
        assert_eq!(compact_edge_identity.asset_id_offset, 41);
        assert_eq!(compact_edge_identity.context_id_offset, 117);

        edge_identity_bytes.remove(21);
        let shortest_edge_identity = crate::design::decode::operands::parse_edge_identity_member(
            &ctx,
            &edge_identity_bytes,
            0,
        )
        .expect("short compact edge-treatment selection identity")
        .unwrap();
        assert!(shortest_edge_identity.layout.is_compact());
        assert_eq!(shortest_edge_identity.local_id, 5890);
        assert_eq!(shortest_edge_identity.layout.local_id_offset(), 22);
        assert_eq!(shortest_edge_identity.asset_id_offset, 40);
        assert_eq!(shortest_edge_identity.context_id_offset, 116);

        group.id = "f3d:Design/BulkStream.dat:selection-group#100".into();
        member.id = "f3d:Design/BulkStream.dat:selection-member#200".into();
        let mut relocated = member.into_draft();
        relocated.byte_offset += 74;
        relocated.local_id_offset += 74;
        relocated.asset_id_offset += 74;
        relocated.context_id_offset += 74;
        relocated.tail_slot_offset += 74;
        relocated.next_byte_offset += 74;
        member =
            crate::records::topology::extrude_selection::DesignExtrudeSelectionMember::try_new(
                relocated,
            )
            .unwrap();
        let identity = DesignConstructionOperandIdentity::try_new(
        crate::records::topology::construction::DesignConstructionOperandIdentityDraft {
            id: "f3d:Design/BulkStream.dat:operand-identity#50".into(),
            group_record_index: 50,
            wrappers: vec![crate::records::topology::construction::DesignIdentityWrapper {
                record_index: 150,
                byte_offset: 50,
                class_tag: crate::records::references::DesignClassTag::try_from("289".to_owned())
                    .unwrap(),
            }],
            following_record_index: 200,
            following_byte_offset: 74,
            following_class_tag: crate::records::references::DesignClassTag::try_from(
                "290".to_owned(),
            )
            .unwrap(),
            tracking_path: None,
            persistent_identity: Some(
                DesignConstructionPersistentIdentity::try_new(
                    crate::records::topology::construction::DesignConstructionPersistentIdentityDraft {
                        local_id: 586,
                        local_id_offset: 95,
                        asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                            "df9087bd-02a6-4a3f-a132-7e69990f323c".to_owned(),
                        )
                        .unwrap(),
                        asset_id_offset: 107,
                        context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                            "0b2382d1-caaf-4eb9-b40d-a6322a7ed829".to_owned(),
                        )
                        .unwrap(),
                        context_id_offset: 187,
                        tail_slot_present: false,
                        tail_slot_offset: 259,
                        next_record_index: 201,
                        next_byte_offset: 264,
                    },
                )
                .unwrap(),
            ),
        },
    )
    .unwrap();
        for (limit, operation) in [
            (0, "f3d Extrude identity matches"),
            (1, "f3d Extrude identity IDs"),
        ] {
            let limited_arena = cadmpeg_core::decode::DecodeArena::new();
            let mut limited_policy = cadmpeg_core::decode::DecodePolicy::default();
            limited_policy.limits.max_collection_items = limit;
            let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::CollectionItems,
                operation,
                |cap| {
                    let mut member = member.clone();
                    let mut limited_policy = cadmpeg_core::decode::DecodePolicy::service();
                    match cadmpeg_core::decode::ResourceDimension::CollectionItems {
                        cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                            limited_policy.limits.max_retained_bytes = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                            limited_policy.limits.max_collection_items = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                            limited_policy.limits.max_materialized_bytes = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                            limited_policy.limits.max_work_units = cap;
                        }
                        dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                    }
                    let (limited_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                        &[],
                        &limited_arena,
                        &limited_policy,
                    )
                    .unwrap();
                    bind_extrude_selection_identities(
                        &limited_ctx,
                        std::slice::from_mut(&mut member),
                        std::slice::from_ref(&identity),
                    )
                },
            ) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
            limited_policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
            match cadmpeg_core::decode::ResourceDimension::CollectionItems {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                    limited_policy.limits.max_retained_bytes = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    limited_policy.limits.max_collection_items = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    limited_policy.limits.max_materialized_bytes = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    limited_policy.limits.max_work_units = refusal_cap;
                }
                dimension => panic!("unsupported refusal dimension: {dimension:?}"),
            }
            let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::CollectionItems,
                operation,
                |cap| {
                    let mut member = member.clone();
                    let mut limited_policy = cadmpeg_core::decode::DecodePolicy::service();
                    match cadmpeg_core::decode::ResourceDimension::CollectionItems {
                        cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                            limited_policy.limits.max_retained_bytes = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                            limited_policy.limits.max_collection_items = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                            limited_policy.limits.max_materialized_bytes = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                            limited_policy.limits.max_work_units = cap;
                        }
                        dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                    }
                    let (limited_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                        &[],
                        &limited_arena,
                        &limited_policy,
                    )
                    .unwrap();
                    bind_extrude_selection_identities(
                        &limited_ctx,
                        std::slice::from_mut(&mut member),
                        std::slice::from_ref(&identity),
                    )
                },
            ) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
            limited_policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
            match cadmpeg_core::decode::ResourceDimension::CollectionItems {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                    limited_policy.limits.max_retained_bytes = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    limited_policy.limits.max_collection_items = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    limited_policy.limits.max_materialized_bytes = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    limited_policy.limits.max_work_units = refusal_cap;
                }
                dimension => panic!("unsupported refusal dimension: {dimension:?}"),
            }
            let (limited_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &limited_arena,
                &limited_policy,
            )
            .unwrap();
            assert!(matches!(
                bind_extrude_selection_identities(&limited_ctx, std::slice::from_mut(&mut member), std::slice::from_ref(&identity)),
                Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                    if failure.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                        && failure.operation == operation
            ));
        }
        let limited_arena = cadmpeg_core::decode::DecodeArena::new();
        let mut limited_policy = cadmpeg_core::decode::DecodePolicy::default();
        limited_policy.limits.max_retained_bytes = u64::try_from(identity.id.len() - 1).unwrap();
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "f3d Extrude identity ID text",
            |cap| {
                let mut member = member.clone();
                let mut limited_policy = cadmpeg_core::decode::DecodePolicy::service();
                match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        limited_policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        limited_policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        limited_policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        limited_policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (limited_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &limited_arena,
                    &limited_policy,
                )
                .unwrap();
                bind_extrude_selection_identities(
                    &limited_ctx,
                    std::slice::from_mut(&mut member),
                    std::slice::from_ref(&identity),
                )
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        limited_policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                limited_policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                limited_policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                limited_policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                limited_policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "f3d Extrude identity ID text",
            |cap| {
                let mut member = member.clone();
                let mut limited_policy = cadmpeg_core::decode::DecodePolicy::service();
                match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        limited_policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        limited_policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        limited_policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        limited_policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (limited_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &limited_arena,
                    &limited_policy,
                )
                .unwrap();
                bind_extrude_selection_identities(
                    &limited_ctx,
                    std::slice::from_mut(&mut member),
                    std::slice::from_ref(&identity),
                )
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        limited_policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                limited_policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                limited_policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                limited_policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                limited_policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (limited_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &limited_arena,
            &limited_policy,
        )
        .unwrap();
        assert!(matches!(
            bind_extrude_selection_identities(&limited_ctx, std::slice::from_mut(&mut member), std::slice::from_ref(&identity)),
            Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                if failure.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                    && failure.operation == "f3d Extrude identity ID text"
        ));
        bind_extrude_selection_identities(
            &ctx,
            std::slice::from_mut(&mut member),
            std::slice::from_ref(&identity),
        )
        .unwrap();
        assert_eq!(member.operand_identity_ids, [identity.id]);
        let mut owning_scope = scope;
        if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::Extrusion(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::Extrusao(slot) =
            owning_scope.payload_mut()
        {
            slot.get_or_insert_with(Default::default).extrude_profile = Some(
                DesignSketchProfileOperand::try_new(
                    crate::records::topology::sketch_profile::DesignSketchProfileOperandDraft {
                        scope_reference_ordinal: 1,
                        record_index: 300,
                        byte_offset: 3000,
                        class_tag: crate::records::references::DesignClassTag::try_from(
                            "308".to_owned(),
                        )
                        .unwrap(),
                        asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                            "df9087bd-02a6-4a3f-a132-7e69990f323c".to_owned(),
                        )
                        .unwrap(),
                        asset_id_offset: 3040,
                        entity_id: crate::records::identity::DesignEntityId::try_from(
                            "0_172".to_owned(),
                        )
                        .expect("valid entity identity"),
                        entity_reference_offset: 3120,
                        region_selection: None,
                        paired_class_tag: crate::records::references::DesignClassTag::try_from(
                            "259".to_owned(),
                        )
                        .unwrap(),
                        paired_byte_offset: 3200,
                    },
                )
                .unwrap(),
            );
        }
        let limited_arena = cadmpeg_core::decode::DecodeArena::new();
        let mut limited_policy = cadmpeg_core::decode::DecodePolicy::default();
        limited_policy.limits.max_collection_items = 0;
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "f3d selected Extrude sketch index",
            |cap| {
                let mut member = member.clone();
                let mut limited_policy = cadmpeg_core::decode::DecodePolicy::service();
                match cadmpeg_core::decode::ResourceDimension::CollectionItems {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        limited_policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        limited_policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        limited_policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        limited_policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (limited_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &limited_arena,
                    &limited_policy,
                )
                .unwrap();
                bind_extrude_selection_geometry(
                    &limited_ctx,
                    std::slice::from_mut(&mut member),
                    std::slice::from_ref(&group),
                    std::slice::from_ref(&owning_scope),
                    &[],
                    &[],
                )
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        limited_policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match cadmpeg_core::decode::ResourceDimension::CollectionItems {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                limited_policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                limited_policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                limited_policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                limited_policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "f3d selected Extrude sketch index",
            |cap| {
                let mut member = member.clone();
                let mut limited_policy = cadmpeg_core::decode::DecodePolicy::service();
                match cadmpeg_core::decode::ResourceDimension::CollectionItems {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        limited_policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        limited_policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        limited_policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        limited_policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (limited_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    &[],
                    &limited_arena,
                    &limited_policy,
                )
                .unwrap();
                bind_extrude_selection_geometry(
                    &limited_ctx,
                    std::slice::from_mut(&mut member),
                    std::slice::from_ref(&group),
                    std::slice::from_ref(&owning_scope),
                    &[],
                    &[],
                )
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        limited_policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match cadmpeg_core::decode::ResourceDimension::CollectionItems {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                limited_policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                limited_policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                limited_policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                limited_policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (limited_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &limited_arena,
            &limited_policy,
        )
        .unwrap();
        assert!(matches!(
            bind_extrude_selection_geometry(
                &limited_ctx,
                std::slice::from_mut(&mut member),
                std::slice::from_ref(&group),
                std::slice::from_ref(&owning_scope),
                &[],
                &[],
            ),
            Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                if failure.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                    && failure.operation == "f3d selected Extrude sketch index"
        ));
        let curve = SketchCurveIdentity {
            id: "f3d:Design/BulkStream.dat:sketch-curve#400".into(),
            record_index: 400,
            owner_reference: Some(172),
            class_tag: crate::records::references::DesignClassTag::try_from("270".to_owned())
                .unwrap(),
            byte_offset: 4000,
            geometry_offset: 100,
            entity_genesis: None,
            primary_id: std::num::NonZeroU64::new(586).unwrap(),
            secondary_id: 0,
            geometry: None,
        };
        bind_extrude_selection_geometry(
            &ctx,
            std::slice::from_mut(&mut member),
            std::slice::from_ref(&group),
            std::slice::from_ref(&owning_scope),
            &[],
            &[curve],
        )
        .unwrap();
        assert!(matches!(
            member.resolved_geometry,
            Some(SketchRelationOperand::Curve {
                record_index: 400,
                primary_id: 586,
                secondary_id: 0,
            })
        ));

        let all_members = group
            .members()
            .iter()
            .map(|member| member.value)
            .collect::<Vec<_>>();
        group.try_set_members(all_members[..1].to_vec()).unwrap();
        let sketch_id = SketchId::mint("f3d:model:sketch#172").unwrap();
        let sketch = Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
            profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
                entity: neutral_sketch_curve_id(&sketch_id, 586, 0),
                reversed: false,
            }]])
            .unwrap(),
            native_ref: None,
        };
        let arrangement_budget = WorkBudget::new(MAX_ARRANGEMENT_WALK_WORK);
        assert!(matches!(
            resolved_extrude_profile_selection(
                &sketch_id,
                &group,
                std::slice::from_ref(&member),
                &sketch,
                crate::design::profile_select::ExtrudeProfileResolution {
                    entities: &[],
                    spatial_sketches: &[],
                    spatial_entities: &[],
                    histories: &[],
                    scope_histories: &std::collections::HashMap::new(),
                    linear_tolerance: 1.0e-6,
                    angular_tolerance: 1.0e-9,
                    arrangement_budget: &arrangement_budget,
                    ctx: decode_ctx,
                }
                .scoped(&[]),
                None,
                None,
            ).unwrap(),
            cadmpeg_ir::features::ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::SketchProfiles {
                sketch: ref actual_sketch,
                ref profiles,
            }) if actual_sketch == &sketch_id && profiles.as_slice() == [0]
        ));
        let mut point_member = member.clone();
        point_member.id = "f3d:Design/BulkStream.dat:selection-member#201".into();
        let mut draft = point_member.into_draft();
        draft.record_index = 201;

        point_member =
            crate::records::topology::extrude_selection::DesignExtrudeSelectionMember::try_new(
                draft,
            )
            .unwrap();
        point_member.group_member_ordinal = 1;
        point_member.local_id = 587;
        point_member.resolved_geometry = Some(SketchRelationOperand::Point {
            record_index: 401,
            persistent_id: Some(587),
        });
        group.try_set_members(all_members).unwrap();
        let mut sketch = sketch;
        let second_profile_id = SketchEntityId::mint("synthetic:test:id#second-profile").unwrap();
        sketch
            .profiles
            .push_single(
                decode_ctx,
                SketchEntityUse {
                    entity: second_profile_id.clone(),
                    reversed: false,
                },
            )
            .unwrap();
        let point_entity = SketchEntity::new(
            neutral_sketch_point_id(&sketch_id, 587),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(0.5, 1.0),
            })
            .unwrap(),
        );
        let line_entity = SketchEntity::new(
            neutral_sketch_curve_id(&sketch_id, 586, 0),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            })
            .unwrap(),
        );
        let second_profile_entity = SketchEntity::new(
            second_profile_id,
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 1.0),
                end: Point2::new(1.0, 1.0),
            })
            .unwrap(),
        );
        let profile_entities = [line_entity, second_profile_entity, point_entity];
        assert!(matches!(
            resolved_extrude_profile_selection(
                &sketch_id,
                &group,
                &[member.clone(), point_member],
                &sketch,
                crate::design::profile_select::ExtrudeProfileResolution {
                    entities: &profile_entities,
                    spatial_sketches: &[],
                    spatial_entities: &[],
                    histories: &[],
                    scope_histories: &std::collections::HashMap::new(),
                    linear_tolerance: 1.0e-6,
                    angular_tolerance: 1.0e-9,
                    arrangement_budget: &arrangement_budget,
                    ctx: decode_ctx,
                }
                .scoped(&[]),
                None,
                None,
            ).unwrap(),
            cadmpeg_ir::features::ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::SketchProfiles {
                sketch: ref actual_sketch,
                ref profiles,
            }) if actual_sketch == &sketch_id && profiles.as_slice() == [0, 1]
        ));
        member.resolved_geometry = None;
        assert!(matches!(
            resolved_extrude_profile_selection(
                &sketch_id,
                &group,
                std::slice::from_ref(&member),
                &sketch,
                crate::design::profile_select::ExtrudeProfileResolution {
                    entities: &[],
                    spatial_sketches: &[],
                    spatial_entities: &[],
                    histories: &[],
                    scope_histories: &std::collections::HashMap::new(),
                    linear_tolerance: 1.0e-6,
                    angular_tolerance: 1.0e-9,
                    arrangement_budget: &arrangement_budget,
                    ctx: decode_ctx,
                }
                .scoped(&[]),
                None,
                None,
            ).unwrap(),
            cadmpeg_ir::features::ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::SketchSelection {
                sketch: ref actual_sketch,
                selections: ref actual_selections,
            }) if actual_sketch == &sketch_id && actual_selections.as_slice() == [group.id.clone()]
        ));
        let mut single_profile_sketch = sketch.clone();
        single_profile_sketch
            .profiles
            .edit(|profiles| profiles.truncate(1))
            .unwrap();
        assert!(matches!(
            resolved_extrude_profile_selection(
                &sketch_id,
                &group,
                std::slice::from_ref(&member),
                &single_profile_sketch,
                crate::design::profile_select::ExtrudeProfileResolution {
                    entities: &[],
                    spatial_sketches: &[],
                    spatial_entities: &[],
                    histories: &[],
                    scope_histories: &std::collections::HashMap::new(),
                    linear_tolerance: 1.0e-6,
                    angular_tolerance: 1.0e-9,
                    arrangement_budget: &arrangement_budget,
                    ctx: decode_ctx,
                }
                .scoped(&[]),
                None,
                None,
            ).unwrap(),
            cadmpeg_ir::features::ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::SketchProfiles {
                sketch: ref actual_sketch,
                ref profiles,
            }) if actual_sketch == &sketch_id && profiles.as_slice() == [0]
        ));
    });
}
