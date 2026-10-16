// SPDX-License-Identifier: Apache-2.0

fn design_type(
    id: &str,
    type_guid: &str,
    entities: &[u64],
) -> crate::records::entity_header::SegmentType {
    use crate::records::entity_header::{BaseTypeGuid, SegmentType, DESIGN_MODULE_FUSION};
    let is_timeline = type_guid == crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID;
    SegmentType::try_new(
        id.into(),
        crate::records::entity_header::SegmentTypeData {
            byte_offset: id.rsplit_once('#').unwrap().1.parse().unwrap(),
            type_guid: type_guid.to_owned().try_into().unwrap(),
            type_guid_offset: 4,
            base_type_guid: if is_timeline {
                BaseTypeGuid::Guid {
                    value: crate::design::decode::meta::FEATURE_TIMELINE_BASE_TYPE_GUID
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    offset: 8,
                }
            } else {
                BaseTypeGuid::Absent
            },
            version: if is_timeline {
                crate::design::decode::meta::FEATURE_TIMELINE_TYPE_VERSIONS[1]
            } else {
                1
            },
            version_offset: 44,
            module: DESIGN_MODULE_FUSION.into(),
            entities: crate::records::identity::ReferenceRun::located(
                entities
                    .iter()
                    .map(|value| crate::records::identity::Located {
                        value: *value,
                        offset: 100,
                    })
                    .collect(),
            ),
        },
    )
    .unwrap()
}

fn native() -> crate::native::F3dNative {
    use crate::records::{
        entity_header::{DesignFeatureTimeline, DesignTimelineFrame},
        identity::Located,
        references::DesignClassTag,
    };
    let meta = "f3d:FusionAssetName[Active]/Design1/MetaStream.dat";
    let bulk = "FusionAssetName[Active]/Design1/BulkStream.dat";
    crate::native::F3dNative {
        design_types: vec![
            design_type(
                &format!("{meta}:design-type#0"),
                crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID,
                &[35],
            ),
            design_type(
                &format!("{meta}:design-type#1"),
                "11111111-2222-3333-4444-555555555555",
                &[17, 101],
            ),
        ],
        design_feature_timelines: vec![DesignFeatureTimeline::try_new(
            crate::ids::native_design_feature_timeline_id(bulk, 200),
            DesignTimelineFrame::new(
                crate::records::admission::RecordAdmission::Admitted,
                200,
                60,
                220,
                240,
                vec![Located {
                    value: 101,
                    offset: 245,
                }],
            )
            .unwrap(),
            DesignClassTag::try_from("256".to_owned()).unwrap(),
            std::num::NonZeroU64::new(35).unwrap(),
            0,
            std::num::NonZeroU64::new(17).unwrap(),
        )
        .unwrap()],
        ..crate::native::F3dNative::default()
    }
}

fn timeline_error_with(
    native: crate::native::F3dNative,
    max_items: u64,
    max_retained: u64,
) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_feature_timelines(&ctx, &mut Vec::new()).unwrap_err()
    })
}

fn timeline_error(max_items: u64) -> cadmpeg_core::CodecError {
    timeline_error_with(native(), max_items, u64::MAX)
}

#[test]
fn timeline_type_order_refuses_collection_limit() {
    assert!(
        matches!(timeline_error(0), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "order F3D feature timeline types")
    );
}

#[test]
fn timeline_type_ordinal_refuses_collection_limit() {
    assert!(
        matches!(timeline_error(2), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D feature timeline type ordinals")
    );
}

#[test]
fn timeline_entity_type_refuses_collection_limit() {
    assert!(
        matches!(timeline_error(3), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D feature timeline entity types")
    );
}

#[test]
fn timeline_source_ordinal_refuses_collection_limit() {
    assert!(
        matches!(timeline_error(4), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D feature timeline source ordinals")
    );
}

#[test]
fn timeline_expected_record_refuses_collection_limit() {
    assert!(
        matches!(timeline_error(5), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D expected feature timelines")
    );
}

#[test]
fn timeline_record_order_refuses_collection_limit() {
    assert!(
        matches!(timeline_error(8), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "order F3D feature timeline records")
    );
}

#[test]
fn timeline_record_identity_refuses_collection_limit() {
    assert!(
        matches!(timeline_error(9), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D feature timeline record identities")
    );
}

#[test]
fn timeline_item_identity_refuses_collection_limit() {
    assert!(
        matches!(timeline_error(10), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D feature timeline item identities")
    );
}

#[test]
fn timeline_valid_records_keep_no_findings() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native();
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        let mut findings = Vec::new();
        super::super::validate_feature_timelines(&ctx, &mut findings).unwrap();
        assert!(findings.is_empty());
    })
}

#[test]
fn timeline_scope_position_refuses_collection_limit() {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let mut native = native();
    native
        .design_parameter_scopes
        .push(DesignParameterScope::empty(
            "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-parameter-scope#101",
            DesignFeatureKind::Hole,
            101,
        ));
    let error = timeline_error_with(native, 18, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D feature timeline scope positions")
    );
}

#[test]
fn timeline_duplicate_type_finding_refuses_collection_limit() {
    let mut native = native();
    native.design_types.push(design_type(
        "f3d:FusionAssetName[Active]/Design1/MetaStream.dat:design-type#2",
        crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID,
        &[35],
    ));
    let error = timeline_error_with(native, 9, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn timeline_duplicate_type_entity_refuses_retained_limit() {
    let mut native = native();
    native.design_types.push(design_type(
        "f3d:FusionAssetName[Active]/Design1/MetaStream.dat:design-type#2",
        crate::design::decode::meta::FEATURE_TIMELINE_TYPE_GUID,
        &[35],
    ));
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| {
            Err::<(), cadmpeg_core::CodecError>(timeline_error_with(native.clone(), u64::MAX, cap))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn timeline_invalid_record_finding_refuses_collection_limit() {
    let mut native = native();
    native.design_types[1].set_entities(crate::records::identity::ReferenceRun::located(vec![
        crate::records::identity::Located {
            value: 17,
            offset: 100,
        },
    ]));
    let error = timeline_error_with(native, 9, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn timeline_invalid_record_entity_refuses_retained_limit() {
    let mut native = native();
    native.design_types[1].set_entities(crate::records::identity::ReferenceRun::located(vec![
        crate::records::identity::Located {
            value: 17,
            offset: 100,
        },
    ]));
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| {
            Err::<(), cadmpeg_core::CodecError>(timeline_error_with(native.clone(), u64::MAX, cap))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn timeline_missing_record_finding_refuses_collection_limit() {
    let mut native = native();
    native.design_feature_timelines.clear();
    let error = timeline_error_with(native, 8, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn timeline_missing_record_entity_refuses_retained_limit() {
    let mut native = native();
    native.design_feature_timelines.clear();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| {
            Err::<(), cadmpeg_core::CodecError>(timeline_error_with(native.clone(), u64::MAX, cap))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn timeline_authored_order_finding_refuses_collection_limit() {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let mut native = native();
    for _ in 0..2 {
        native
            .design_parameter_scopes
            .push(DesignParameterScope::empty(
                "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-parameter-scope#101",
                DesignFeatureKind::Hole,
                101,
            ));
    }
    let error = timeline_error_with(native, 15, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn timeline_authored_order_entity_refuses_retained_limit() {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let mut native = native();
    for _ in 0..2 {
        native
            .design_parameter_scopes
            .push(DesignParameterScope::empty(
                "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-parameter-scope#101",
                DesignFeatureKind::Hole,
                101,
            ));
    }
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| {
            Err::<(), cadmpeg_core::CodecError>(timeline_error_with(native.clone(), u64::MAX, cap))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

fn history_scope(
    record_index: u32,
    state: i64,
    previous: Option<i64>,
) -> crate::records::feature::scope::DesignParameterScope {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let mut scope = DesignParameterScope::empty(
        &format!("f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-parameter-scope#{record_index}"),
        DesignFeatureKind::Hole,
        record_index,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(state);
            draft.previous_history_state_id = previous;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn forward_history_native() -> crate::native::F3dNative {
    use crate::records::{
        entity_header::{DesignFeatureTimeline, DesignTimelineFrame},
        identity::{Located, ReferenceRun},
    };
    let mut native = native();
    native.design_types[1].set_entities(ReferenceRun::located(vec![
        Located {
            value: 17,
            offset: 100,
        },
        Located {
            value: 101,
            offset: 108,
        },
        Located {
            value: 100,
            offset: 116,
        },
    ]));
    let timeline = &native.design_feature_timelines[0];
    native.design_feature_timelines[0] = DesignFeatureTimeline::try_new(
        timeline.id().clone(),
        DesignTimelineFrame::new(
            crate::records::admission::RecordAdmission::Admitted,
            200,
            80,
            220,
            240,
            vec![
                Located {
                    value: 101,
                    offset: 245,
                },
                Located {
                    value: 100,
                    offset: 256,
                },
            ],
        )
        .unwrap(),
        timeline.class_tag.clone(),
        timeline.record_index,
        timeline.source_ordinal,
        timeline.context_record_index,
    )
    .unwrap();
    native
        .design_parameter_scopes
        .push(history_scope(100, 7, None));
    native
        .design_parameter_scopes
        .push(history_scope(101, 8, Some(7)));
    native
}

fn cyclic_history_native() -> crate::native::F3dNative {
    let mut native = native();
    native
        .design_parameter_scopes
        .push(history_scope(101, 9, Some(7)));
    native
        .design_parameter_scopes
        .push(history_scope(102, 7, Some(8)));
    native
        .design_parameter_scopes
        .push(history_scope(103, 8, Some(7)));
    native
}

#[test]
fn timeline_forward_history_finding_refuses_collection_limit() {
    let error = timeline_error_with(forward_history_native(), 33, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn timeline_forward_history_entity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| {
            Err::<(), cadmpeg_core::CodecError>(timeline_error_with(
                forward_history_native(),
                u64::MAX,
                cap,
            ))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn timeline_cyclic_history_finding_refuses_collection_limit() {
    let error = timeline_error_with(cyclic_history_native(), 34, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn timeline_cyclic_history_entity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| {
            Err::<(), cadmpeg_core::CodecError>(timeline_error_with(
                cyclic_history_native(),
                u64::MAX,
                cap,
            ))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn timeline_history_findings_keep_specific_messages() {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        for (native, message) in [
            (
                forward_history_native(),
                "Fusion Design history edge runs forward in its feature timeline",
            ),
            (
                cyclic_history_native(),
                "Fusion Design scope history-state dependency is cyclic",
            ),
        ] {
            let ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
            let mut findings = Vec::new();
            super::super::validate_feature_timelines(&ctx, &mut findings).unwrap();
            assert!(findings.iter().any(|finding| finding.message == message));
        }
    })
}
