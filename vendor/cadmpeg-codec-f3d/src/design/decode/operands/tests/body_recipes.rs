// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::operands::body_recipe_operand_end;
use crate::design::decode::operands::body_recipe_prologue_end;
use crate::design::decode::operands::parse_body_recipe_operand;
use crate::design::decode::operands::unique_body_recipe;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::recipes::ConstructionRecipe;
use crate::records::recipes::ConstructionRecipeKind;
use crate::records::sketch_links::PersistentSubentityTag;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::construction::DesignConstructionOperandGroupFrame;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::test_support::indexed_header;
use crate::test_support::lp_utf16;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::ids::FaceId;

#[test]
fn body_recipe_candidate_index_refuses_collection_limit() {
    let recipe = ConstructionRecipe {
        id: "f3d:Design/BulkStream.dat:construction-recipe#1".into(),
        byte_offset: 1,
        kind: ConstructionRecipeKind::Body,
        design: None,
        recipe_index: 0,
        record_index: None,
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::decode::operands::bind_body_recipe_operand_candidates(
            &ctx, &mut [], std::slice::from_ref(&recipe), &[], &[],
        ),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && failure.operation == "f3d body recipe candidate index"
    ));
}

#[test]
fn body_recipe_decode_indices_refuse_collection_limits() {
    let archive = crate::test_support::zip_test::f3d_with_smbh_and_protein(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let header = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#100".into(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("365".to_owned())
                .unwrap(),
            record_index: 100,
        };
        let recipe = ConstructionRecipe {
            id: "f3d:Design/BulkStream.dat:construction-recipe#1".into(),
            byte_offset: 1,
            kind: ConstructionRecipeKind::Body,
            design: None,
            recipe_index: 0,
            record_index: None,
        };
        for (limit, operation) in [
            (0, "f3d body recipe header index"),
            (1, "f3d body recipe stream index"),
            (2, "f3d body recipe stream entries"),
        ] {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::default();
            policy.limits.max_collection_items = limit;

            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                crate::design::decode::operands::decode_body_recipe_operands(
                    &ctx, scan, &[], &[], std::slice::from_ref(&header), std::slice::from_ref(&recipe),
                ),
                Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                    if failure.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                        && failure.operation == operation
            ));
        }
    });
}

#[test]
fn body_recipe_operand_decodes_counted_and_empty_reference_tables() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
            scope_record_index: 80,
            scope_reference_ordinal: 0,
            record_index: 90,
            byte_offset: 900,
            class_tag: crate::records::references::DesignClassTag::try_from("269".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 100,
                offset: 926,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 921,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![crate::records::identity::Located {
                        value: 200,
                        offset: 943,
                    }],
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 971,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 975,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::ROLE_0X5,
                ),
            role_offset: 953,

            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "265".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 1024,
        },
    )
    .unwrap();
    let mut record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("365".to_owned()).unwrap(),
        record_index: 100,
    };
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"365", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&2265u64.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&2266u64.to_le_bytes());
    bytes.extend_from_slice(&32u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&103u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "53aa8ab4-194a-434b-bd52-8c6d761dc147");
    lp_utf16(&mut bytes, "8e685642-4d68-4909-96d0-0dd4437491b6");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[7, 0, 0, 0]);
    indexed_header(&mut bytes, *b"259", 100);
    indexed_header(&mut bytes, *b"283", 101);
    indexed_header(&mut bytes, *b"463", 102);
    indexed_header(&mut bytes, *b"452", 103);
    let recipe_at = bytes.len();
    bytes.extend_from_slice(b"body_recipe_data");
    let next_at = bytes.len();
    indexed_header(&mut bytes, *b"311", 104);
    let mut recipe = ConstructionRecipe {
        id: format!("f3d:Design/BulkStream.dat:construction-recipe#{recipe_at}"),
        byte_offset: u64_from_index(recipe_at),
        kind: ConstructionRecipeKind::Body,
        design: Some(crate::records::recipes::ConstructionRecipeDesign {
            id: crate::records::identity::RecordedValue {
                value: "2265".into(),
                offset: 0,
            },
            selector: Some(crate::records::recipes::ConstructionRecipeSelector {
                value: 9,
                byte_offset: 0,
            }),
        }),
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 0,
            offset: 0,
        }),
    };
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#80",
        crate::records::feature::scope::DesignFeatureKind::BoundaryFill,
        80,
    );

    let limited_arena = cadmpeg_core::decode::DecodeArena::new();
    let mut limited_policy = cadmpeg_core::decode::DecodePolicy::default();
    limited_policy.limits.max_collection_items = 1;
    let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "f3d body recipe references",
        |cap| {
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
            ((parse_body_recipe_operand(&limited_ctx, &bytes, &group, 0, &record, &recipe))
                .transpose())
            .map(|_| ())
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
        "f3d body recipe references",
        |cap| {
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
            ((parse_body_recipe_operand(&limited_ctx, &bytes, &group, 0, &record, &recipe))
                .transpose())
            .map(|_| ())
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
    let (limited_ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &limited_arena, &limited_policy)
            .unwrap();
    assert!(matches!(
        parse_body_recipe_operand(&limited_ctx, &bytes, &group, 0, &record, &recipe),
        Some(Err(cadmpeg_core::CodecError::ResourceLimit(failure)))
            if failure.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && failure.operation == "f3d body recipe references"
    ));
    for (limit, operation) in [
        (35, "f3d Design UTF-16 text"),
        (71, "f3d Design UTF-16 text"),
        (
            u64::try_from(72 + recipe.id.len() - 1).unwrap(),
            "f3d body recipe operand recipe ID",
        ),
    ] {
        let limited_arena = cadmpeg_core::decode::DecodeArena::new();
        let mut limited_policy = cadmpeg_core::decode::DecodePolicy::default();
        limited_policy.limits.max_retained_bytes = limit;
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            operation,
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
                ((parse_body_recipe_operand(&limited_ctx, &bytes, &group, 0, &record, &recipe))
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
            operation,
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
                ((parse_body_recipe_operand(&limited_ctx, &bytes, &group, 0, &record, &recipe))
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
            parse_body_recipe_operand(&limited_ctx, &bytes, &group, 0, &record, &recipe),
            Some(Err(cadmpeg_core::CodecError::ResourceLimit(failure)))
                if failure.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                    && failure.operation == operation
        ));
    }

    let mut operand = parse_body_recipe_operand(&ctx, &bytes, &group, 0, &record, &recipe)
        .expect("body recipe operand")
        .unwrap();
    assert_eq!(operand.references().len(), 2);
    assert_eq!(operand.references()[0].design_reference, 2265);
    assert_eq!(operand.references()[0].form, 3);
    assert_eq!(operand.references()[1].design_reference, 2266);
    assert_eq!(operand.references()[1].form, 32);
    assert_eq!(
        operand
            .clone()
            .into_draft()
            .selector_tail
            .map(|tail| tail.value),
        Some([7, 0, 0, 0])
    );
    assert_eq!(
        operand
            .clone()
            .into_draft()
            .selector_tail
            .map(|tail| tail.offset),
        Some(220)
    );
    assert_eq!(
        operand.owner,
        crate::records::topology::body_recipe::DesignOperandOwner::Group {
            group_record_index: 90,
            group_member_ordinal: 0,
        }
    );
    assert_eq!(operand.nested_record_index(), 103);
    assert_eq!(operand.recipe_id, recipe.id);
    assert_eq!(operand.next_byte_offset(), u64_from_index(next_at));
    operand.id = "f3d:Design/BulkStream.dat:body-recipe-operand#0".into();
    let limited_arena = cadmpeg_core::decode::DecodeArena::new();
    let mut limited_policy = cadmpeg_core::decode::DecodePolicy::default();
    limited_policy.limits.max_collection_items = 0;
    let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "f3d body recipe operand output",
        |cap| {
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
            crate::design::decode::operands::push_body_recipe_operand(
                &limited_ctx,
                &mut Vec::new(),
                operand.clone(),
                "Design/BulkStream.dat",
                0,
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
        "f3d body recipe operand output",
        |cap| {
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
            crate::design::decode::operands::push_body_recipe_operand(
                &limited_ctx,
                &mut Vec::new(),
                operand.clone(),
                "Design/BulkStream.dat",
                0,
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
    let (limited_ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &limited_arena, &limited_policy)
            .unwrap();
    assert!(matches!(
        crate::design::decode::operands::push_body_recipe_operand(
            &limited_ctx, &mut Vec::new(), operand.clone(), "Design/BulkStream.dat", 0,
        ),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && failure.operation == "f3d body recipe operand output"
    ));
    let limited_arena = cadmpeg_core::decode::DecodeArena::new();
    let mut limited_policy = cadmpeg_core::decode::DecodePolicy::default();
    let id_len = crate::ids::native_scope("Design/BulkStream.dat").len()
        + ":design-body-recipe-operand#".len()
        + 1;
    limited_policy.limits.max_retained_bytes = u64::try_from(id_len - 1).unwrap();
    let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "f3d body recipe operand ID",
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
            crate::design::decode::operands::push_body_recipe_operand(
                &limited_ctx,
                &mut Vec::new(),
                operand.clone(),
                "Design/BulkStream.dat",
                0,
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
        "f3d body recipe operand ID",
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
            crate::design::decode::operands::push_body_recipe_operand(
                &limited_ctx,
                &mut Vec::new(),
                operand.clone(),
                "Design/BulkStream.dat",
                0,
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
    let (limited_ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &limited_arena, &limited_policy)
            .unwrap();
    assert!(matches!(
        crate::design::decode::operands::push_body_recipe_operand(
            &limited_ctx, &mut Vec::new(), operand.clone(), "Design/BulkStream.dat", 0,
        ),
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                && failure.operation == "f3d body recipe operand ID"
    ));
    let candidate_tags = [
        PersistentSubentityTag {
            id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Face(
                FaceId::mint("test:model:face#same-stream").expect("identity grammar"),
            ),
            selector: 1,
            token: cadmpeg_core::text::NonBlankString::new("0").unwrap(),
            design_references: vec![2265],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#2".into(),
            target: AttributeTarget::Face(
                FaceId::mint("test:model:face#other-selector").expect("identity grammar"),
            ),
            selector: 2,
            token: cadmpeg_core::text::NonBlankString::new("0").unwrap(),
            design_references: vec![2265, 2266],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "f3d:xref/Other/occurrence-0/design:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Face(
                FaceId::mint("test:model:face#other-stream").expect("identity grammar"),
            ),
            selector: 0,
            token: cadmpeg_core::text::NonBlankString::new("0").unwrap(),
            design_references: vec![2265],
            ordinal: 0,
        },
    ];
    for (collection_limit, retained_limit, operation) in [
        (1, u64::MAX, "f3d operand face candidate"),
        (
            u64::MAX,
            u64::try_from(operand.id.len() - 1).unwrap(),
            "f3d body recipe candidate operand ID",
        ),
        (
            u64::MAX,
            u64::try_from(operand.id.len() + "test:model:face#same-stream".len() - 1).unwrap(),
            "f3d operand face candidate ID",
        ),
    ] {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = collection_limit;
        policy.limits.max_retained_bytes = retained_limit;
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
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
                let (limited_ctx, _) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                        .unwrap();
                let mut limited_operand = operand.clone();
                crate::design::decode::operands::bind_body_recipe_operand_candidates(
                    &limited_ctx,
                    std::slice::from_mut(&mut limited_operand),
                    std::slice::from_ref(&recipe),
                    &candidate_tags,
                    std::slice::from_ref(&scope),
                )
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
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
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
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
                let (limited_ctx, _) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                        .unwrap();
                let mut limited_operand = operand.clone();
                crate::design::decode::operands::bind_body_recipe_operand_candidates(
                    &limited_ctx,
                    std::slice::from_mut(&mut limited_operand),
                    std::slice::from_ref(&recipe),
                    &candidate_tags,
                    std::slice::from_ref(&scope),
                )
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
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
        let (limited_ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut limited_operand = operand.clone();
        assert!(matches!(
            crate::design::decode::operands::bind_body_recipe_operand_candidates(&limited_ctx, std::slice::from_mut(&mut limited_operand), std::slice::from_ref(&recipe), &candidate_tags, std::slice::from_ref(&scope)),
            Err(cadmpeg_core::CodecError::ResourceLimit(failure)) if failure.operation == operation
        ));
    }
    crate::design::decode::operands::bind_body_recipe_operand_candidates(
        &ctx,
        std::slice::from_mut(&mut operand),
        std::slice::from_ref(&recipe),
        &candidate_tags,
        std::slice::from_ref(&scope),
    )
    .unwrap();
    assert_eq!(
        operand.references()[0].candidate_faces,
        [
            FaceId::mint("test:model:face#other-selector").expect("identity grammar"),
            FaceId::mint("test:model:face#same-stream").expect("identity grammar")
        ]
    );

    // A legacy Combine tool keeps the same identity envelope with no
    // persistent Design-reference clauses. The marker therefore follows the
    // zero count at the ordinary reference-table cursor.
    let mut empty_bytes = bytes[..25].to_vec();
    empty_bytes[21..25].fill(0);
    empty_bytes.extend_from_slice(&bytes[49..]);
    let empty_recipe_at = recipe_at - 24;
    let empty_next_at = next_at - 24;
    let empty_recipe = ConstructionRecipe {
        id: format!("f3d:Design/BulkStream.dat:construction-recipe#{empty_recipe_at}"),
        byte_offset: u64_from_index(empty_recipe_at),
        ..recipe.clone()
    };
    let empty = parse_body_recipe_operand(&ctx, &empty_bytes, &group, 0, &record, &empty_recipe)
        .expect("empty body recipe operand")
        .unwrap();
    assert!(empty.references().is_empty());
    assert_eq!(empty.nested_record_index(), 103);
    assert_eq!(empty.next_byte_offset(), u64_from_index(empty_next_at));

    let mut combine_scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#80",
        crate::records::feature::scope::DesignFeatureKind::Combine,
        80,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Combine(slot) =
        combine_scope.payload_mut()
    {
        *slot = Some(crate::records::feature::combine::DesignCombineOperation {
            form: crate::records::feature::combine::DesignCombineForm::Standard,
            operation: cadmpeg_ir::features::BooleanKind::Join,
            operation_offset: 0,
            keep_tools: false,
            keep_tools_offset: 0,
            target_record_index: 0,
            tools: crate::records::feature::combine::DesignCombineTools {
                first: crate::records::feature::combine::DesignCombineBodySelection {
                    record_index: record.record_index,
                    external_identity: None,
                },
                additional: Vec::new(),
            },
        });
    }
    let mut combine_recipe = recipe.clone();
    combine_recipe.design.as_mut().unwrap().selector =
        Some(crate::records::recipes::ConstructionRecipeSelector {
            value: 1,
            byte_offset: 0,
        });
    let mut combine_operand = operand.clone();
    crate::design::decode::operands::bind_body_recipe_operand_candidates(
        &ctx,
        std::slice::from_mut(&mut combine_operand),
        std::slice::from_ref(&combine_recipe),
        &[
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("test:model:face#same-stream").expect("identity grammar"),
                ),
                selector: 1,
                token: cadmpeg_core::text::NonBlankString::new("0").unwrap(),
                design_references: vec![2265],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#2".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("test:model:face#other-selector").expect("identity grammar"),
                ),
                selector: 2,
                token: cadmpeg_core::text::NonBlankString::new("0").unwrap(),
                design_references: vec![2265, 2266],
                ordinal: 0,
            },
        ],
        std::slice::from_ref(&combine_scope),
    )
    .unwrap();
    assert_eq!(
        combine_operand.references()[0].candidate_faces,
        [FaceId::mint("test:model:face#same-stream").expect("identity grammar")]
    );

    let mut nested = Vec::new();
    indexed_header(&mut nested, *b"302", 1);
    indexed_header(&mut nested, *b"305", 11);
    bytes.splice(next_at..next_at, nested.iter().copied());
    let operand = parse_body_recipe_operand(&ctx, &bytes, &group, 0, &record, &recipe)
        .expect("body recipe operand with nested recipe records")
        .unwrap();
    assert_eq!(
        operand.next_byte_offset(),
        u64_from_index(next_at + nested.len())
    );

    let stream_name = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let archive = crate::test_support::zip_test::f3d_with_configuration(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
        stream_name,
        &bytes,
    );
    group.id = crate::ids::native_scoped_id(stream_name, "operand-group", 90);
    record.id = crate::ids::native_scoped_id(stream_name, "record", 100);
    recipe.id = crate::ids::native_scoped_id(stream_name, "construction-recipe", recipe_at);
    let output_id = crate::ids::native_scoped_id(stream_name, "design-body-recipe-operand", 0);
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let decoded = crate::design::decode::operands::decode_body_recipe_operands(
            &ctx,
            scan,
            &[],
            std::slice::from_ref(&group),
            std::slice::from_ref(&record),
            std::slice::from_ref(&recipe),
        )
        .unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].id, output_id);

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes =
            u64::try_from(72 + recipe.id.len() + output_id.len() * 2 - 1).unwrap();
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "f3d body recipe owner ID",
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
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
                let (ctx, _) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                        .unwrap();
                (crate::design::decode::operands::decode_body_recipe_operands(
                    &ctx,
                    scan,
                    &[],
                    std::slice::from_ref(&group),
                    std::slice::from_ref(&record),
                    std::slice::from_ref(&recipe),
                ))
                .map(|_| ())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
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
        let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            "f3d body recipe owner ID",
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
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
                let (ctx, _) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                        .unwrap();
                (crate::design::decode::operands::decode_body_recipe_operands(
                    &ctx,
                    scan,
                    &[],
                    std::slice::from_ref(&group),
                    std::slice::from_ref(&record),
                    std::slice::from_ref(&recipe),
                ))
                .map(|_| ())
            },
        ) {
            cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
            error => panic!("unexpected refusal: {error:?}"),
        };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match cadmpeg_core::decode::ResourceDimension::RetainedBytes {
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
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            crate::design::decode::operands::decode_body_recipe_operands(
                &ctx, scan, &[], std::slice::from_ref(&group),
                std::slice::from_ref(&record), std::slice::from_ref(&recipe),
            ),
            Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                if failure.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                    && failure.operation == "f3d body recipe owner ID"
        ));
    });
}

#[test]
fn class_367_body_recipe_operand_decodes_scale_member_frame() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"367", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&301u64.to_le_bytes());
    bytes.extend_from_slice(&33u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&103u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "53aa8ab4-194a-434b-bd52-8c6d761dc147");
    lp_utf16(&mut bytes, "8e685642-4d68-4909-96d0-0dd4437491b6");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    indexed_header(&mut bytes, *b"264", 100);
    indexed_header(&mut bytes, *b"404", 101);
    indexed_header(&mut bytes, *b"416", 102);
    indexed_header(&mut bytes, *b"424", 103);
    let recipe_at = bytes.len();
    bytes.extend_from_slice(b"body_recipe_data");
    let next_at = bytes.len();
    indexed_header(&mut bytes, *b"280", 104);

    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
            scope_record_index: 80,
            scope_reference_ordinal: 1,
            record_index: 90,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("287".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 100,
                offset: 21,
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
                "264".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap();
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("367".to_owned()).unwrap(),
        record_index: 100,
    };
    let recipe = ConstructionRecipe {
        id: format!("f3d:Design/BulkStream.dat:construction-recipe#{recipe_at}"),
        byte_offset: u64_from_index(recipe_at),
        kind: ConstructionRecipeKind::Body,
        design: Some(crate::records::recipes::ConstructionRecipeDesign {
            id: crate::records::identity::RecordedValue {
                value: "301".into(),
                offset: 0,
            },
            selector: Some(crate::records::recipes::ConstructionRecipeSelector {
                value: 6,
                byte_offset: 0,
            }),
        }),
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 0,
            offset: 0,
        }),
    };

    let operand = parse_body_recipe_operand(&ctx, &bytes, &group, 0, &record, &recipe)
        .expect("class-367 body recipe operand")
        .unwrap();
    assert_eq!(operand.references().len(), 1);
    assert_eq!(operand.references()[0].design_reference, 301);
    assert_eq!(operand.references()[0].form, 33);
    assert_eq!(
        operand
            .clone()
            .into_draft()
            .selector_tail
            .map(|tail| tail.value),
        Some([1, 0, 0, 0])
    );
    assert_eq!(
        operand
            .clone()
            .into_draft()
            .selector_tail
            .map(|tail| tail.offset),
        Some(208)
    );
    assert_eq!(operand.nested_record_index(), 103);
    assert_eq!(operand.next_byte_offset(), u64_from_index(next_at));
}

#[test]
fn body_recipe_envelope_uses_its_structural_record_boundary() {
    const RECORD_INDEX: u32 = 100;
    const NEXT_AT: usize = 70_000;
    const EARLY_RECIPE_AT: usize = 100;
    const LATE_RECIPE_AT: usize = NEXT_AT - 32;

    let mut bytes = Vec::new();
    for record_index in [
        RECORD_INDEX,
        RECORD_INDEX,
        RECORD_INDEX + 1,
        RECORD_INDEX + 2,
        RECORD_INDEX + 3,
    ] {
        indexed_header(&mut bytes, *b"365", record_index);
    }
    let prologue_end = bytes.len();
    bytes.resize(NEXT_AT, 0xaa);
    for recipe_at in [EARLY_RECIPE_AT, LATE_RECIPE_AT] {
        bytes[recipe_at..recipe_at + b"body_recipe_data".len()]
            .copy_from_slice(b"body_recipe_data");
    }
    indexed_header(&mut bytes, *b"311", RECORD_INDEX + 4);

    let header = DesignRecordHeader {
        id: "stream:record-100".into(),
        record_index: RECORD_INDEX,
        class_tag: crate::records::references::DesignClassTag::try_from("365".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let early = ConstructionRecipe {
        id: "stream:recipe-early".into(),
        byte_offset: u64_from_index(EARLY_RECIPE_AT),
        kind: ConstructionRecipeKind::Body,
        design: None,
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 0,
            offset: 0,
        }),
    };
    let late = ConstructionRecipe {
        id: "stream:recipe-late".into(),
        byte_offset: u64_from_index(LATE_RECIPE_AT),
        recipe_index: 1,
        ..early.clone()
    };

    assert_eq!(
        body_recipe_prologue_end(&bytes, 0, RECORD_INDEX),
        Some(prologue_end)
    );
    assert_eq!(
        body_recipe_operand_end(&bytes, prologue_end, RECORD_INDEX, EARLY_RECIPE_AT),
        Some(NEXT_AT)
    );
    assert_eq!(unique_body_recipe(&bytes, &header, &[&early]), Some(&early));
    assert_eq!(unique_body_recipe(&bytes, &header, &[&early, &late]), None);
}
