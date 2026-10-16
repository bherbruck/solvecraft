// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::dimension_frames::companion_owned_interval;
use crate::design::decode::parameters::bind_parameter_companion_payloads;
use crate::design::decode::scopes::path_feature::exact_path_feature_construction;
use crate::design::test_support::parameter_record;
use crate::layout::fixed_pipe_operation_prefix as fixed_pipe_layout;
use crate::layout::legacy_pipe_operation_prefix as legacy_pipe_layout;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::DesignPathFeatureConstruction;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameterCompanion;
use crate::records::recipes::{ConstructionRecipe, ConstructionRecipeKind};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::{
    entity_selection::DesignLoftLegacyBodyCarrier, extrude_selection::DesignOperandRole,
};
use std::collections::HashMap;

pub(super) fn fixed_kind_path_operations(
    mut bytes: Vec<u8>,
    mut scope: DesignParameterScope,
    thicken_group: &DesignConstructionOperandGroup,
) {
    let loft_start = bytes.len();
    let mut loft = vec![0; 376];
    loft[29..33].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&loft);
    let mut loft_scope = scope.clone();
    loft_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(loft_start);
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Loft
                .try_into()
                .unwrap();
            draft.frame_length = 376;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &loft_scope,
            &[],
        ),
        Some(DesignPathFeatureConstruction::Loft(
            crate::records::feature::path_features::DesignLoftConstruction {
                operation: DesignExtrudeOperation::Join,
                operation_offset: u64_from_index(loft_start + 29),
            }
        ))
    );
    loft_scope.id = "stream:loft-scope".into();
    {
        let value = Some(DesignPathFeatureConstruction::Loft(
            crate::records::feature::path_features::DesignLoftConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: u64_from_index(loft_start + 29),
            },
        ));
        loft_scope
            .try_edit(|draft| {
                draft.payload =
                    value.map_or_else(|| draft.payload.kind().try_into().unwrap(), Into::into);
            })
            .unwrap();
    }
    let loft_record_index = loft_scope.record_index;
    let loft_group = |ordinal: u32, role: DesignOperandRole| {
        let mut group = thicken_group.clone();
        group.id = format!("stream:loft-group-{ordinal}");
        group.scope_record_index = loft_record_index;
        group.scope_reference_ordinal = ordinal;
        group.operand_role =
            crate::records::topology::construction::DesignConstructionOperandRole::Other(role);
        group
    };
    let role_41 = [
        loft_group(0, DesignOperandRole::PROFILE),
        loft_group(1, DesignOperandRole::PROFILE),
    ];
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::feature_project::project_fixed_loft(&loft_scope, &role_41, &[], &[], &[], &[], decode_ctx)).unwrap(),
        Some(cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Loft {
            sections,
            guidance: cadmpeg_ir::features::LoftGuidance::Guides(guides),
            ..
        })) if sections.len() == 2 && guides.is_empty()
    ));
    let guided_role_41 = [
        loft_group(0, DesignOperandRole::PROFILE),
        loft_group(1, DesignOperandRole::PROFILE),
        loft_group(2, DesignOperandRole::PROFILE),
        loft_group(3, DesignOperandRole::ROLE_0X5),
    ];
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::feature_project::project_fixed_loft(&loft_scope, &guided_role_41, &[], &[], &[], &[], decode_ctx)).unwrap(),
        Some(cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Loft {
            sections,
            guidance: cadmpeg_ir::features::LoftGuidance::Guides(guides),
            ..
        })) if sections.len() == 3 && guides.len() == 1
    ));
    let role_shape = |groups: &[DesignConstructionOperandGroup]| {
        groups
            .iter()
            .map(|group| (group.role(), group.members().len()))
            .collect::<Vec<_>>()
    };
    assert!(crate::validate::loft_operand_roles_are_valid(
        DesignExtrudeOperation::NewBody,
        &role_shape(&guided_role_41),
    ));
    {
        let value = Some(DesignPathFeatureConstruction::Loft(
            crate::records::feature::path_features::DesignLoftConstruction {
                operation: DesignExtrudeOperation::Cut,
                operation_offset: u64_from_index(loft_start + 29),
            },
        ));
        loft_scope
            .try_edit(|draft| {
                draft.payload =
                    value.map_or_else(|| draft.payload.kind().try_into().unwrap(), Into::into);
            })
            .unwrap();
    }
    let cut = [
        loft_group(0, DesignOperandRole::BODIES_A),
        loft_group(1, DesignOperandRole::PROFILE),
        loft_group(2, DesignOperandRole::ROLE_0X43),
    ];
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::feature_project::project_fixed_loft(&loft_scope, &cut, &[], &[], &[], &[], decode_ctx)).unwrap(),
        Some(cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Loft {
            sections,
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        })) if sections.len() == 2
    ));
    assert!(crate::validate::loft_operand_roles_are_valid(
        DesignExtrudeOperation::Cut,
        &role_shape(&cut),
    ));
    let legacy_carrier = DesignLoftLegacyBodyCarrier {
        id: "stream:legacy-loft-carrier".into(),
        scope_record_index: loft_scope.record_index,
        record_index: 500,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("322".to_owned()).unwrap(),
        owner_scope_record_index_offset: 22,
        member: 900,
        member_offset: 36,
        member_count_offset: 32,
        opaque_index: std::num::NonZeroU8::new(89).expect("nonzero ordinal"),
        opaque_index_offset: 47,
        opaque_scalar: cadmpeg_ir::scalar::FiniteReal::new(1.25).expect("checked fixture value"),
        opaque_scalar_offset: 51,
        repeated_opaque_index_offset: 59,
        next_next_record_index: 502,
        next_next_reference_offset: 63,
        flags_offset: 74,
        next_record_index: 501,
        next_reference_offset: 76,
        trailing_scope_reference_offset: None,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("262".to_owned())
            .unwrap(),
        paired_byte_offset: 87,
    };
    let legacy_cut = [
        loft_group(1, DesignOperandRole::BODIES_B),
        loft_group(2, DesignOperandRole::PROFILE),
        loft_group(3, DesignOperandRole::ROLE_0X43),
    ];
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::feature_project::project_fixed_loft(&loft_scope, &legacy_cut, std::slice::from_ref(&legacy_carrier), &[], &[], &[], decode_ctx)).unwrap(),
        Some(cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Loft {
            sections,
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        })) if sections.len() == 2
    ));
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::feature_project::project_fixed_loft(
                &loft_scope,
                &legacy_cut,
                &[],
                &[],
                &[],
                &[],
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );
    {
        let value = Some(DesignPathFeatureConstruction::Loft(
            crate::records::feature::path_features::DesignLoftConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: u64_from_index(loft_start + 29),
            },
        ));
        loft_scope
            .try_edit(|draft| {
                draft.payload =
                    value.map_or_else(|| draft.payload.kind().try_into().unwrap(), Into::into);
            })
            .unwrap();
    }
    let role_5 = [
        loft_group(0, DesignOperandRole::ROLE_0X5),
        loft_group(1, DesignOperandRole::ROLE_0X5),
        loft_group(2, DesignOperandRole::ROLE_0X5),
    ];
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::feature_project::project_fixed_loft(&loft_scope, &role_5, &[], &[], &[], &[], decode_ctx)).unwrap(),
        Some(cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Loft {
            sections,
            guidance: cadmpeg_ir::features::LoftGuidance::Guides(guides),
            ..
        })) if sections.len() == 3 && guides.is_empty()
    ));
    let centered = [
        loft_group(0, DesignOperandRole::ROLE_0X43),
        loft_group(1, DesignOperandRole::ROLE_0X43),
        loft_group(2, DesignOperandRole::ROLE_0X7),
    ];
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::feature_project::project_fixed_loft(&loft_scope, &centered, &[], &[], &[], &[], decode_ctx)).unwrap(),
        Some(cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Loft {
            sections,
            guidance: cadmpeg_ir::features::LoftGuidance::Centerline(
                cadmpeg_ir::features::PathRef::Native(centerline),
            ),
            ..
        })) if sections.len() == 2 && centerline == "stream:loft-group-2"
    ));
    let mixed = [
        loft_group(0, DesignOperandRole::ROLE_0X43),
        loft_group(1, DesignOperandRole::ROLE_0X43),
        loft_group(2, DesignOperandRole::ROLE_0X5),
        loft_group(3, DesignOperandRole::ROLE_0X7),
    ];
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::feature_project::project_fixed_loft(
                &loft_scope,
                &mixed,
                &[],
                &[],
                &[],
                &[],
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );
    assert!(!crate::validate::loft_operand_roles_are_valid(
        DesignExtrudeOperation::NewBody,
        &role_shape(&mixed),
    ));
    let mut point = loft_group(0, DesignOperandRole::ROLE_0X5);
    point
        .try_set_members(
            vec![10]
                .into_iter()
                .enumerate()
                .map(|(index, value)| crate::records::identity::Located {
                    value,
                    offset: u64_from_index(index) * 11,
                })
                .collect(),
        )
        .unwrap();
    let profile = loft_group(1, DesignOperandRole::ROLE_0X43);
    let mut boundary = loft_group(2, DesignOperandRole::ROLE_0X5);
    boundary
        .try_set_members(
            vec![20, 21, 22]
                .into_iter()
                .enumerate()
                .map(|(index, value)| crate::records::identity::Located {
                    value,
                    offset: u64_from_index(index) * 11,
                })
                .collect(),
        )
        .unwrap();
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::feature_project::project_fixed_loft(&loft_scope, &[point.clone(), profile.clone(), boundary.clone()], &[], &[], &[], &[], decode_ctx)).unwrap(),
        Some(cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Loft {
            sections,
            guidance: cadmpeg_ir::features::LoftGuidance::Guides(guides),
            ..
        })) if matches!(sections.as_slice(), [
            cadmpeg_ir::features::LoftSection::Point(
                cadmpeg_ir::features::LoftPointSection::Native(_)
            ),
            cadmpeg_ir::features::LoftSection::Profile(_),
            cadmpeg_ir::features::LoftSection::Profile(_),
        ]) && guides.is_empty()
    ));
    assert!(crate::validate::loft_operand_roles_are_valid(
        DesignExtrudeOperation::NewBody,
        &role_shape(&[point, profile, boundary]),
    ));

    let sweep_start = bytes.len();
    let mut sweep = vec![0; 499];
    sweep[25..29].copy_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&sweep);
    let sweep_values: [f64; 6] = [0.8, 0.0, 1.0, 1.0, 6.632_251_157_578_453, 0.0];
    let sweep_scalar_start = bytes.len();
    for (ordinal, value) in sweep_values.into_iter().enumerate() {
        let record_index = 80 + u32::try_from(ordinal).expect("fixture value fits u32");
        let mut scalar = vec![0; 100];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = u8::try_from(ordinal).expect("fixture value fits u8");
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let mut sweep_scope = scope.clone();
    sweep_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(sweep_start);
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Sweep
                .try_into()
                .unwrap();
            draft.frame_length = 499;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated((80..86).collect());
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &sweep_scope,
            &[],
        ),
        Some(DesignPathFeatureConstruction::Sweep(
            crate::records::feature::path_features::DesignSweepConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: u64_from_index(sweep_start + 25),
                values: crate::test_support::reals(sweep_values),
                record_indexes: [80, 81, 82, 83, 84, 85],
                value_offsets: std::array::from_fn(|ordinal| {
                    u64_from_index(sweep_scalar_start + ordinal * 111 + 40)
                }),
            }
        ))
    );
    sweep_scope.id = "stream:sweep-scope".into();
    {
        let value = exact_path_feature_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &sweep_scope,
            &[],
        );
        sweep_scope
            .try_edit(|draft| {
                draft.payload =
                    value.map_or_else(|| draft.payload.kind().try_into().unwrap(), Into::into);
            })
            .unwrap();
    }
    let sweep_record_index = sweep_scope.record_index;
    let sweep_group = |ordinal: u32, role: DesignOperandRole| {
        let mut group = thicken_group.clone();
        group.id = format!("stream:sweep-group-{ordinal}");
        group.scope_record_index = sweep_record_index;
        group.scope_reference_ordinal = ordinal;
        group.operand_role =
            crate::records::topology::construction::DesignConstructionOperandRole::Other(role);
        group
    };
    let profile = sweep_group(0, DesignOperandRole::PROFILE);
    let path = sweep_group(1, DesignOperandRole::ROLE_0X5);
    let body = sweep_group(2, DesignOperandRole::BODIES_A);
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::feature_project::project_fixed_sweep(&sweep_scope, &[profile.clone(), path.clone()], &[], &[], &[], &[], decode_ctx)).unwrap(),
        Some(cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Sweep {
            path_extent: Some(cadmpeg_ir::features::SweepPathExtent {
                along_fraction: fraction_0,
                against_fraction: fraction_1,
            }),
            twist: Some(actual_twist),
            taper: None,
            ..
        })) if fraction_1.get() == 0.0 && fraction_0.get() == 0.8 && actual_twist.get() == 6.632_251_157_578_453
    ));
    let rail = sweep_group(2, DesignOperandRole::ROLE_0X5);
    {
        let value = Some(DesignPathFeatureConstruction::Sweep(
            crate::records::feature::path_features::DesignSweepConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: u64_from_index(sweep_start + 25),
                values: crate::test_support::reals([0.0, 1.0, 0.0, 1.0, 0.0, 0.0]),
                record_indexes: [80, 81, 82, 83, 84, 85],
                value_offsets: std::array::from_fn(|ordinal| {
                    u64_from_index(sweep_scalar_start + ordinal * 111 + 40)
                }),
            },
        ));
        sweep_scope
            .try_edit(|draft| {
                draft.payload =
                    value.map_or_else(|| draft.payload.kind().try_into().unwrap(), Into::into);
            })
            .unwrap();
    }
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::feature_project::project_fixed_sweep(&sweep_scope, &[profile.clone(), path.clone(), rail], &[], &[], &[], &[], decode_ctx)).unwrap(),
        Some(cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Sweep {
            path: Some(cadmpeg_ir::features::PathRef::Native(path)),
            path_extent: Some(cadmpeg_ir::features::SweepPathExtent {
                along_fraction: fraction_2,
                against_fraction: fraction_3,
            }),
            guide_rail: Some(cadmpeg_ir::features::SweepGuideRail {
                path: cadmpeg_ir::features::PathRef::Native(rail),
                extent: cadmpeg_ir::features::SweepPathExtent {
                    along_fraction: fraction_4,
                    against_fraction: fraction_5,
                },
            }),
            ..
        })) if fraction_5.get() == 1.0 && fraction_4.get() == 0.0 && fraction_3.get() == 1.0 && fraction_2.get() == 0.0 && path == "stream:sweep-group-1" && rail == "stream:sweep-group-2"
    ));
    let complete_sweep_values = [1.0, 1.0, 1.0, 1.0, sweep_values[4], 0.0];
    {
        let value = Some(DesignPathFeatureConstruction::Sweep(
            crate::records::feature::path_features::DesignSweepConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: u64_from_index(sweep_start + 25),
                values: crate::test_support::reals(complete_sweep_values),
                record_indexes: [80, 81, 82, 83, 84, 85],
                value_offsets: std::array::from_fn(|ordinal| {
                    u64_from_index(sweep_scalar_start + ordinal * 111 + 40)
                }),
            },
        ));
        sweep_scope
            .try_edit(|draft| {
                draft.payload =
                    value.map_or_else(|| draft.payload.kind().try_into().unwrap(), Into::into);
            })
            .unwrap();
    }
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::feature_project::project_fixed_sweep(&sweep_scope, &[profile.clone(), path.clone()], &[], &[], &[], &[], decode_ctx)).unwrap(), Some(cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Sweep {
            shape,
            ..
        })) if matches!((&shape.mode(),), (cadmpeg_ir::features::SweepMode::Solid { op: cadmpeg_ir::features::SolidSweepOperation::NewBody },))));
    assert_eq!(
        crate::test_support::with_decode_context(|decode_ctx| {
            crate::design::feature_project::project_fixed_sweep(
                &sweep_scope,
                &[profile.clone(), path.clone(), body.clone()],
                &[],
                &[],
                &[],
                &[],
                decode_ctx,
            )
        })
        .unwrap(),
        None
    );
    {
        let value = Some(
            crate::records::topology::sketch_profile::DesignSketchProfileOperand::try_new(
                crate::records::topology::sketch_profile::DesignSketchProfileOperandDraft {
                    scope_reference_ordinal: 3,
                    record_index: 2795,
                    byte_offset: 32_000,
                    class_tag: crate::records::references::DesignClassTag::try_from(
                        "312".to_owned(),
                    )
                    .unwrap(),
                    asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                        "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
                    )
                    .unwrap(),
                    asset_id_offset: 32_040,
                    entity_id: crate::records::identity::DesignEntityId::try_from(
                        "0_2718".to_owned(),
                    )
                    .expect("valid entity identity"),
                    entity_reference_offset: 32_080,
                    region_selection: None,
                    paired_class_tag: crate::records::references::DesignClassTag::try_from(
                        "258".to_owned(),
                    )
                    .unwrap(),
                    paired_byte_offset: 32_180,
                },
            )
            .unwrap(),
        );
        if let crate::records::feature::scope::DesignScopePayloadMut::Sweep(slot) =
            sweep_scope.payload_mut()
        {
            slot.get_or_insert_with(Default::default).sweep_profile = value;
        }
    }
    let mut selected_profile = profile.clone();
    selected_profile
        .try_set_members(vec![crate::records::identity::Located {
            value: 2788,
            offset: selected_profile.members()[0].offset,
        }])
        .unwrap();
    let mut profile_carrier = profile.clone();
    profile_carrier.id = "stream:sweep-profile-carrier".into();
    profile_carrier.scope_reference_ordinal = 3;
    profile_carrier
        .try_set_members(vec![crate::records::identity::Located {
            value: 2795,
            offset: profile_carrier.members()[0].offset,
        }])
        .unwrap();
    let mut guide_surface = sweep_group(4, DesignOperandRole::FACES);
    guide_surface.id = "stream:sweep-guide-surface".into();
    let entity_selection =
        crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
            crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
                id: "stream:sweep-profile-selection".into(),
                scope_record_index: sweep_scope.record_index,
                group_record_index: selected_profile.record_index,
                group_member_ordinal: 0,
                record_index: 2788,
                byte_offset: 31_000,
                class_tag: crate::records::references::DesignClassTag::try_from("310".to_owned())
                    .unwrap(),
                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
                )
                .unwrap(),
                asset_id_offset: 31_040,
                context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
                )
                .unwrap(),
                context_id_offset: 31_080,
                identity_record_index: 2791,
                identity_record_offset: 31_180,
                primary_identity: 2718,
                primary_identity_offset: 31209,
                secondary: Some(crate::records::identity::DesignSecondaryIdentity {
                    identity: crate::records::identity::Located {
                        value: 164,
                        offset: 31217,
                    },
                    curve_identity: None,
                }),
                historical_edge_candidates: Vec::new(),
                historical_face_candidates: Vec::new(),
                resolved_edge_slot: None,
                next_record_index: 2792,
                next_byte_offset: 31225,
            },
        )
        .unwrap();
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::feature_project::project_fixed_sweep(&sweep_scope, &[selected_profile, profile_carrier, path.clone(), guide_surface], &[], &[], &[entity_selection], &[], decode_ctx)).unwrap(), Some(cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Sweep {
            shape,
            orientation: Some(cadmpeg_ir::features::SweepOrientation::GuideSurface {
                faces: cadmpeg_ir::features::FaceSelection::Native(faces),
            }),
            guide_rail: None,
            ..
        })) if matches!((shape.referenced_profile(),), (Some(cadmpeg_ir::features::PlanarProfileRef::Native(profile)),) if profile == "stream:sweep-group-0" && faces == "stream:sweep-guide-surface")));
    if let crate::records::feature::scope::DesignScopePayloadMut::Sweep(slot) =
        sweep_scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).sweep_profile = None;
    }
    {
        let value = Some(DesignPathFeatureConstruction::Sweep(
            crate::records::feature::path_features::DesignSweepConstruction {
                operation: DesignExtrudeOperation::Cut,
                operation_offset: u64_from_index(sweep_start + 25),
                values: crate::test_support::reals(complete_sweep_values),
                record_indexes: [80, 81, 82, 83, 84, 85],
                value_offsets: std::array::from_fn(|ordinal| {
                    u64_from_index(sweep_scalar_start + ordinal * 111 + 40)
                }),
            },
        ));
        sweep_scope
            .try_edit(|draft| {
                draft.payload =
                    value.map_or_else(|| draft.payload.kind().try_into().unwrap(), Into::into);
            })
            .unwrap();
    }
    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| crate::design::feature_project::project_fixed_sweep(&sweep_scope, &[profile, path, body], &[], &[], &[], &[], decode_ctx)).unwrap(), Some(cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Sweep {
            shape,
            ..
        })) if matches!((&shape.mode(),), (cadmpeg_ir::features::SweepMode::Solid {
                op: cadmpeg_ir::features::SolidSweepOperation::Cut
            },))));

    let pipe_start = bytes.len();
    let mut pipe = vec![0; 464];
    pipe[25..29].copy_from_slice(&4u32.to_le_bytes());
    pipe[29] = 1;
    pipe[30] = 1;
    bytes.extend_from_slice(&pipe);
    let pipe_values: [f64; 4] = [1.0, 1.0, 0.6, 0.15];
    let pipe_scalar_start = bytes.len();
    for (ordinal, value) in pipe_values.into_iter().enumerate() {
        let record_index = 170 + u32::try_from(ordinal).expect("fixture value fits u32");
        let mut scalar = vec![0; 100];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = u8::try_from(ordinal).expect("fixture value fits u8");
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let mut pipe_scope = scope.clone();
    pipe_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(pipe_start);
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Pipe
                .try_into()
                .unwrap();
            draft.frame_length = 464;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated((170..174).collect());
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &pipe_scope,
            &[],
        ),
        Some(DesignPathFeatureConstruction::Pipe(
            crate::records::feature::path_features::DesignPipeConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: u64_from_index(pipe_start + 25),
                section_shape:
                    crate::records::feature::surface_ops::DesignPipeSectionShape::Circular,
                section_shape_offset: u64_from_index(pipe_start + 29),
                filled: true,
                filled_offset: u64_from_index(pipe_start + 30),
                values: crate::test_support::reals(pipe_values),
                record_indexes: [170, 171, 172, 173],
                value_offsets: std::array::from_fn(|ordinal| {
                    u64_from_index(pipe_scalar_start + ordinal * 111 + 40)
                }),
            }
        ))
    );

    let owner_pipe_start = bytes.len();
    let mut owner_pipe = vec![0; fixed_pipe_layout::FILLED + 1];
    owner_pipe[fixed_pipe_layout::OPERATION..fixed_pipe_layout::OPERATION + 4]
        .copy_from_slice(&4u32.to_le_bytes());
    owner_pipe[fixed_pipe_layout::SECTION_SHAPE] = 1;
    owner_pipe[fixed_pipe_layout::FILLED] = 1;
    bytes.extend_from_slice(&owner_pipe);
    let owner_pipe_values: [f64; 4] = [1.0, 0.0, 0.175, 0.0438];
    let owner_pipe_record_indexes = [210, 211, 212, 213];
    let owner_pipe_owners = owner_pipe_values
        .into_iter()
        .enumerate()
        .map(|(ordinal, value)| {
            crate::records::parameters::DesignParameterOwner::try_from(
                crate::records::parameters::DesignParameterOwnerWire {
                    id: format!(
                        "f3d:Design/BulkStream.dat:parameter-owner#{}",
                        owner_pipe_record_indexes[ordinal]
                    ),
                    byte_offset: (10_000 + u64_from_index(ordinal)) - 40,
                    frame_length: 103,
                    class_tag: crate::records::references::DesignClassTag::try_from(
                        "342".to_owned(),
                    )
                    .unwrap(),
                    record_index: owner_pipe_record_indexes[ordinal],
                    scope_record_index: scope.record_index,
                    local_ordinal: u32::try_from(ordinal).expect("fixture value fits u32"),
                    evaluated_value: value,
                    evaluated_value_offset: 10_000 + u64_from_index(ordinal),
                    parameter_record_index: owner_pipe_record_indexes[ordinal] + 1,
                    owned_ordinal: u32::try_from(ordinal).expect("fixture value fits u32"),
                    variant: None,
                    companion_record_index: owner_pipe_record_indexes[ordinal] + 2,
                },
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let mut owner_pipe_scope = scope.clone();
    owner_pipe_scope.id = "f3d:Design/BulkStream.dat:scope#12".into();
    owner_pipe_scope
        .try_edit(|draft| {
            draft.byte_offset = u64_from_index(owner_pipe_start);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    owner_pipe_scope.class_tag =
        crate::records::references::DesignClassTag::try_from("421".to_owned()).unwrap();
    owner_pipe_scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("257".to_owned()).unwrap();
    owner_pipe_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Pipe
                .try_into()
                .unwrap();
            draft.frame_length = 405;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(owner_pipe_record_indexes.into());
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &owner_pipe_scope,
            &owner_pipe_owners,
        ),
        Some(DesignPathFeatureConstruction::Pipe(
            crate::records::feature::path_features::DesignPipeConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: u64_from_index(owner_pipe_start + fixed_pipe_layout::OPERATION),
                section_shape:
                    crate::records::feature::surface_ops::DesignPipeSectionShape::Circular,
                section_shape_offset: u64_from_index(
                    owner_pipe_start + fixed_pipe_layout::SECTION_SHAPE
                ),
                filled: true,
                filled_offset: u64_from_index(owner_pipe_start + fixed_pipe_layout::FILLED),
                values: crate::test_support::reals(owner_pipe_values),
                record_indexes: owner_pipe_record_indexes,
                value_offsets: [10_000, 10_001, 10_002, 10_003],
            }
        ))
    );
    let mut wrong_owner_class = owner_pipe_owners.clone();
    let mut wire =
        crate::records::parameters::DesignParameterOwnerWire::from(wrong_owner_class[0].clone());
    wire.class_tag =
        crate::records::references::DesignClassTag::try_from("341".to_owned()).unwrap();
    wrong_owner_class[0] =
        crate::records::parameters::DesignParameterOwner::try_from(wire).unwrap();
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &owner_pipe_scope,
            &wrong_owner_class,
        ),
        None
    );

    for (pair_ordinal, (class_tag, paired_class_tag)) in
        [("405", "259"), ("475", "260")].into_iter().enumerate()
    {
        let legacy_pipe_start = bytes.len();
        let mut legacy_pipe = vec![0; 383];
        legacy_pipe[legacy_pipe_layout::ZERO_RUN_9..legacy_pipe_layout::PREFIX_MARKER]
            .copy_from_slice(&[0; 9]);
        legacy_pipe[legacy_pipe_layout::PREFIX_MARKER] = legacy_pipe_layout::PREFIX_MARKER_VALUE;
        legacy_pipe[legacy_pipe_layout::ZERO_RUN_5..legacy_pipe_layout::OPERATION]
            .copy_from_slice(&[0; 5]);
        legacy_pipe[legacy_pipe_layout::OPERATION..legacy_pipe_layout::SECTION_SHAPE]
            .copy_from_slice(&4u32.to_le_bytes());
        legacy_pipe[legacy_pipe_layout::SECTION_SHAPE] = 1;
        legacy_pipe[legacy_pipe_layout::FILLED] = 1;
        bytes.extend_from_slice(&legacy_pipe);
        let legacy_scalar_start = bytes.len();
        let legacy_values: [f64; 4] = [1.0, 1.0, 0.6, 0.15];
        let first_record_index =
            180 + u32::try_from(pair_ordinal).expect("fixture value fits u32") * 4;
        for (ordinal, value) in legacy_values.into_iter().enumerate() {
            let record_index =
                first_record_index + u32::try_from(ordinal).expect("fixture value fits u32");
            let mut scalar = vec![0; 100];
            scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
            scalar[4..7].copy_from_slice(b"277");
            scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
            scalar[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
            scalar[24] = 1;
            scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
            scalar[35] = u8::try_from(ordinal).expect("fixture value fits u8");
            scalar[40..48].copy_from_slice(&value.to_le_bytes());
            scalar.extend_from_slice(&3u32.to_le_bytes());
            scalar.extend_from_slice(b"261");
            scalar.extend_from_slice(&record_index.to_le_bytes());
            bytes.extend_from_slice(&scalar);
        }
        let mut legacy_scope = scope.clone();
        legacy_scope
            .try_edit(|draft| {
                draft.byte_offset = u64_from_index(legacy_pipe_start);
                draft.reference_count_offset = draft.byte_offset + 9;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        legacy_scope.class_tag =
            crate::records::references::DesignClassTag::try_from(class_tag.to_owned()).unwrap();
        legacy_scope.paired_class_tag =
            crate::records::references::DesignClassTag::try_from(paired_class_tag.to_owned())
                .unwrap();
        legacy_scope
            .try_edit(|draft| {
                draft.payload = crate::records::feature::scope::DesignFeatureKind::Pipe
                    .try_into()
                    .unwrap();
                draft.frame_length = 383;
                draft.reference_members = crate::records::identity::ReferenceRun::unlocated(
                    (first_record_index..first_record_index + 4).collect(),
                );
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        assert_eq!(
            exact_path_feature_construction(
                &bytes,
                &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
                &legacy_scope,
                &[],
            ),
            Some(DesignPathFeatureConstruction::Pipe(
                crate::records::feature::path_features::DesignPipeConstruction {
                    operation: DesignExtrudeOperation::NewBody,
                    operation_offset: u64_from_index(
                        legacy_pipe_start + legacy_pipe_layout::OPERATION
                    ),
                    section_shape:
                        crate::records::feature::surface_ops::DesignPipeSectionShape::Circular,
                    section_shape_offset: u64_from_index(
                        legacy_pipe_start + legacy_pipe_layout::SECTION_SHAPE
                    ),
                    filled: true,
                    filled_offset: u64_from_index(legacy_pipe_start + legacy_pipe_layout::FILLED),
                    values: crate::test_support::reals(legacy_values),
                    record_indexes: [
                        first_record_index,
                        first_record_index + 1,
                        first_record_index + 2,
                        first_record_index + 3,
                    ],
                    value_offsets: std::array::from_fn(|ordinal| {
                        u64_from_index(legacy_scalar_start + ordinal * 111 + 40)
                    }),
                }
            ))
        );
    }

    let companion = DesignParameterCompanion::unbound(
        "f3d:native:parameter-companion#11".into(),
        0,
        crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        11,
        10,
        std::num::NonZeroU64::new(1).unwrap(),
        42,
    )
    .bound(crate::records::parameters::DesignCompanionPayload::new(
        58,
        0,
        Vec::new(),
    ));
    scope.id = "f3d:native:parameter-scope#12".into();
    scope
        .try_edit(|draft| {
            draft.byte_offset = 58;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        companion_owned_interval(
            &cadmpeg_test_support::service_decode_context(),
            &companion,
            std::iter::empty(),
            &[],
            &[scope.clone()],
            &[],
            100,
        )
        .unwrap(),
        Some((58, 58))
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 80;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        companion_owned_interval(
            &cadmpeg_test_support::service_decode_context(),
            &companion,
            std::iter::empty(),
            &[],
            &[scope.clone()],
            &[],
            100,
        )
        .unwrap(),
        Some((58, 80))
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 90;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let foreign_header = DesignRecordHeader {
        id: "f3d:native:record-header#55".into(),
        record_index: 55,
        class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned()).unwrap(),
        byte_offset: 70,
    };
    assert_eq!(
        companion_owned_interval(
            &cadmpeg_test_support::service_decode_context(),
            &companion,
            std::iter::empty(),
            &[],
            &[scope.clone()],
            &[foreign_header],
            100,
        )
        .unwrap(),
        Some((58, 70))
    );

    let mut parameter = crate::design::decode::parameters::parse_design_parameter(
        &cadmpeg_test_support::service_decode_context(),
        &parameter_record(None, "1", "User Parameter", None, "p", 1.0),
    )
    .unwrap()
    .expect("generated parameter")
    .into_record("Design/BulkStream.dat", 65)
    .expect("located parameter");
    parameter.id = "f3d:native:design-parameter#65".into();
    assert_eq!(
        companion_owned_interval(
            &cadmpeg_test_support::service_decode_context(),
            &companion,
            std::iter::once(&parameter),
            &[],
            &[],
            &[],
            100,
        )
        .unwrap(),
        Some((58, 65))
    );
    let recipe = ConstructionRecipe {
        id: "f3d:native:construction-recipe#60".into(),
        byte_offset: 60,
        kind: ConstructionRecipeKind::Edge,
        design: None,
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 303,
            offset: 0,
        }),
    };
    let bound = bind_parameter_companion_payloads(
        &cadmpeg_test_support::service_decode_context(),
        vec![companion.clone()],
        &crate::design::decode::parameters::ParameterCompanionInputs {
            parameters: std::slice::from_ref(&parameter),
            owners: &[],
            scopes: &[],
            entities: &[],
            headers: &[],
            recipes: std::slice::from_ref(&recipe),
            stream_lengths: &HashMap::from([("f3d:native".into(), 100)]),
        },
    )
    .unwrap();
    let payload = bound[0].payload().expect("bound payload");
    assert_eq!(payload.byte_offset(), 58);
    assert_eq!(payload.byte_length(), 7);
    assert_eq!(payload.owned_recipe_ids(), [recipe.id]);

    if let crate::records::feature::scope::DesignScopePayloadMut::Sketch(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::Esquisse(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::Skizze(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::Esboco(slot) = scope.payload_mut()
    {
        *slot = Some(crate::records::feature::scope::DesignSketchEntityBinding {
            entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_99".to_owned())
                .expect("valid entity identity"),
            entity_reference_offset: 0,
        });
    }
    let entity = crate::records::entity_header::DesignEntityHeader {
        id: "f3d:native:design-entity-header#70".into(),
        byte_offset: 70,

        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_99".to_owned())
            .expect("valid entity ID"),
        class_tag: crate::records::references::DesignClassTag::try_from("366".to_owned()).unwrap(),
        optional_slot_present: false,
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some("MSketch".into()),
            None,
            crate::records::identity::ReferenceRun::unlocated(Vec::new()),
        )
        .expect("valid module registration"),
    };
    let bound = bind_parameter_companion_payloads(
        &cadmpeg_test_support::service_decode_context(),
        vec![companion],
        &crate::design::decode::parameters::ParameterCompanionInputs {
            parameters: &[],
            owners: &[],
            scopes: std::slice::from_ref(&scope),
            entities: std::slice::from_ref(&entity),
            headers: &[],
            recipes: &[],
            stream_lengths: &HashMap::from([("f3d:native".into(), 100)]),
        },
    )
    .unwrap();
    let payload = bound[0].payload().expect("bound payload");
    assert_eq!(payload.byte_offset(), 58);
    assert_eq!(payload.byte_length(), 12);
}
