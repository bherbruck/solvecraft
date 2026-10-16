// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use super::exact_base_feature_construction;
use crate::records::feature::base_feature::DesignBaseFeatureConstruction;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::ReferenceRun;
use crate::records::references::DesignClassTag;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn result_body_frame() -> (Vec<u8>, DesignParameterScope) {
    let body_count = 3usize;
    let frame_length = 262 + 52 * body_count;
    let mut bytes = vec![0u8; frame_length];
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(
        &(2 * u32::try_from(body_count).expect("fixture value fits u32")).to_le_bytes(),
    );
    let mut cursor = 24;
    for ordinal in 0..body_count {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 9]
            .copy_from_slice(&(101 + u64_from_index(ordinal)).to_le_bytes());
        cursor += 15;
    }
    for ordinal in 0..body_count {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 9]
            .copy_from_slice(&(201 + u64_from_index(ordinal)).to_le_bytes());
        cursor += 15;
    }
    bytes[cursor] = 1;
    bytes[cursor + 7..cursor + 11].copy_from_slice(
        &(u32::try_from(body_count).expect("fixture value fits u32")).to_le_bytes(),
    );
    cursor += 11;
    for ordinal in 0..body_count {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 5].copy_from_slice(
            &(201 + u32::try_from(ordinal).expect("fixture value fits u32")).to_le_bytes(),
        );
        cursor += 11;
    }
    bytes[cursor] = 0;
    cursor += 1;
    bytes[cursor] = 1;
    bytes[cursor + 1..cursor + 9].copy_from_slice(&301u64.to_le_bytes());
    cursor += 11;
    bytes[cursor..cursor + 4].copy_from_slice(
        &(u32::try_from(body_count).expect("fixture value fits u32")).to_le_bytes(),
    );
    cursor += 4;
    for ordinal in 0..body_count {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 5].copy_from_slice(
            &(401 + u32::try_from(ordinal).expect("fixture value fits u32")).to_le_bytes(),
        );
        cursor += 11;
    }
    let mut scope = DesignParameterScope::empty(
        "f3d:scope#base-feature-result",
        DesignFeatureKind::BaseFeature,
        70,
    );
    scope.class_tag = DesignClassTag::try_from("409".to_owned()).unwrap();
    scope.paired_class_tag = DesignClassTag::try_from("262".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = u64_from_index(frame_length);
            draft.kind_offset = u64_from_index(frame_length - 102);
            draft.paired_byte_offset = u64_from_index(frame_length);
            draft.reference_members = ReferenceRun::unlocated(vec![301]);
            draft.reference_count_offset =
                draft.kind_offset - 12 - 11 * u64_from_index(draft.reference_members.len());
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope)
}

#[test]
fn base_feature_result_runs_refuse_each_collection_limit() {
    let (bytes, scope) = result_body_frame();
    for (limit, operation) in [
        (2, "f3d BaseFeature entities"),
        (5, "f3d BaseFeature references"),
        (8, "f3d BaseFeature repeated reference fields"),
        (10, "f3d BaseFeature remaining result bodies"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = exact_base_feature_construction(&ctx, &bytes, &scope);
        assert!(
            matches!(
                result,
                Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ),
            "limit {limit}: {operation}"
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 11;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = exact_base_feature_construction(&ctx, &bytes, &scope)
        .unwrap()
        .expect("admitted BaseFeature result bodies");
    let DesignBaseFeatureConstruction::ResultBodies { bodies, .. } = result else {
        panic!("unexpected BaseFeature construction");
    };
    assert_eq!(bodies.iter().count(), 3);
}

fn snapshot_frame() -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = vec![0u8; 485];
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    let mut cursor = 24;
    for (value, field) in [(101u64, [1u8, 2, 3, 4, 5, 6]), (202, [6u8, 5, 4, 3, 2, 1])] {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
        bytes[cursor + 9..cursor + 15].copy_from_slice(&field);
        cursor += 15;
    }
    bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
    bytes[cursor + 4..cursor + 8].copy_from_slice(&1u32.to_le_bytes());
    cursor += 8;
    for guid in [
        "11111111-2222-3333-4444-555555555555",
        "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
    ] {
        let encoded =
            crate::bytes::lp_utf16_bytes(guid).expect("fixture UTF-16 code-unit count fits u32");
        bytes[cursor..cursor + encoded.len()].copy_from_slice(&encoded);
        cursor += encoded.len();
    }
    bytes[cursor..cursor + 7].copy_from_slice(&[0, 0, 1, 1, 0, 0, 0]);
    cursor += 7;
    bytes[cursor] = 1;
    cursor += 1;
    bytes[cursor..cursor + 8].copy_from_slice(&101u64.to_le_bytes());
    cursor += 11;
    bytes[cursor] = 1;
    cursor += 1;
    bytes[cursor..cursor + 8].copy_from_slice(&301u64.to_le_bytes());
    cursor += 14;
    bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
    cursor += 4;
    bytes[cursor] = 1;
    cursor += 1;
    bytes[cursor..cursor + 8].copy_from_slice(&401u64.to_le_bytes());
    cursor += 18;
    let encoded = crate::bytes::lp_utf16_bytes("00000000-0000-0000-0000-000000000000")
        .expect("fixture UTF-16 code-unit count fits u32");
    bytes[cursor..cursor + encoded.len()].copy_from_slice(&encoded);
    cursor += encoded.len();
    cursor += 3;
    bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
    cursor += 4;
    bytes[cursor] = 1;
    cursor += 1;
    bytes[cursor..cursor + 4].copy_from_slice(&301u32.to_le_bytes());
    cursor += 10;
    bytes[cursor..cursor + 4].copy_from_slice(&7u32.to_le_bytes());
    cursor += 4;
    let encoded = crate::bytes::lp_utf16_bytes("Base Feature")
        .expect("fixture UTF-16 code-unit count fits u32");
    bytes[cursor..cursor + encoded.len()].copy_from_slice(&encoded);
    cursor += encoded.len();
    bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
    cursor += 4;
    assert_eq!(cursor, 401);

    let mut scope = DesignParameterScope::empty(
        "f3d:scope#base-feature-snapshot",
        DesignFeatureKind::BaseFeature,
        70,
    );
    scope.class_tag = DesignClassTag::try_from("314".to_owned()).unwrap();
    scope.paired_class_tag = DesignClassTag::try_from("259".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 485;
            draft.paired_byte_offset = 485;
            draft.kind_offset = 373;
            draft.feature_ordinal_offset = 397;
            draft.history_state_id = Some(7);
            draft.previous_history_state_id = None;
            draft.previous_history_state_id_offset = None;
            draft.reference_count_offset = 350;
            draft.reference_members =
                ReferenceRun::from_columns(vec![301], vec![355], "reference_members").unwrap();
        })
        .unwrap();
    (bytes, scope)
}

#[test]
fn base_feature_snapshot_bodies_refuse_collection_limit() {
    let (bytes, scope) = snapshot_frame();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = exact_base_feature_construction(&ctx, &bytes, &scope);
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d BaseFeature snapshot bodies"
    ));
    let arena = DecodeArena::new();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = exact_base_feature_construction(&ctx, &bytes, &scope)
        .unwrap()
        .expect("admitted BaseFeature body snapshot");
    let DesignBaseFeatureConstruction::BodySnapshot { bodies, .. } = result else {
        panic!("unexpected BaseFeature construction");
    };
    assert_eq!(bodies.len(), 2);
}

#[test]
fn base_feature_snapshot_guid_refusal_is_not_an_absent_candidate() {
    let (bytes, scope) = snapshot_frame();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "retain F3D relaxed GUID",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            match ResourceDimension::RetainedBytes {
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
            (exact_base_feature_construction(&ctx, &bytes, &scope)).map(|_| ())
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
    match ResourceDimension::RetainedBytes {
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
        ResourceDimension::RetainedBytes,
        "retain F3D relaxed GUID",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            match ResourceDimension::RetainedBytes {
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
            (exact_base_feature_construction(&ctx, &bytes, &scope)).map(|_| ())
        },
    ) {
        cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
        error => panic!("unexpected refusal: {error:?}"),
    };
    policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
    match ResourceDimension::RetainedBytes {
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
    assert!(
        matches!(exact_base_feature_construction(&ctx, &bytes, &scope),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain F3D relaxed GUID" && limit.additional == 36)
    );
}
