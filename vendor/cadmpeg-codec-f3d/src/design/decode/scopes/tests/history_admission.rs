// SPDX-License-Identifier: Apache-2.0

use super::super::parameter_scope::admit_history_bound_scope_variants;
use crate::history_records::{
    AsmDeltaState, AsmHistoricalEntityDelta, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
    AsmHistoricalTransition, AsmHistory,
};
use crate::records::feature::scope::DesignParameterScope;

fn with_test_ctx<T>(run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("test decode context");
    run(&ctx)
}

fn history_state(state_id: i64, previous_state_id: Option<i64>) -> AsmDeltaState {
    AsmDeltaState {
        id: format!("history:state#{state_id}"),
        parent: "history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Complete(
            AsmHistoricalTopology::default(),
        ),
        transition: previous_state_id.map(|previous_state_id| AsmHistoricalTransition {
            previous_state_id: Some(previous_state_id),
            records: AsmHistoricalEntityDelta::default(),
            topology: AsmHistoricalTopologyDelta::default(),
        }),
    }
}

fn history(states: Vec<AsmDeltaState>) -> AsmHistory {
    AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states,
    }
}

fn scope(
    record_index: u32,
    byte_offset: u64,
    state_id: i64,
    previous_state_id: i64,
) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("f3d:stream:design-parameter-scope#{byte_offset}"),
        crate::records::feature::scope::DesignFeatureKind::Chamfer,
        record_index,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = byte_offset;
            draft.history_state_id = Some(state_id);
            draft.previous_history_state_id = Some(previous_state_id);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

#[test]
fn history_bound_scope_admission_reports_collection_limit() {
    let mut scopes = vec![scope(42, 100, 7, 6)];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("test decode context");
    let error = admit_history_bound_scope_variants(&ctx, &mut scopes, &[])
        .expect_err("one scope exceeds the collection limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && limit.operation == "f3d scope admission")
    );
}

#[test]
fn history_bound_scope_admission_refuses_group_and_output_limits() {
    use cadmpeg_core::decode::ResourceDimension;

    for (cap, operation) in [
        (1, "f3d scope admission groups"),
        (2, "f3d scope admission group indices"),
        (3, "f3d scope admission retained output"),
    ] {
        let mut scopes = vec![scope(42, 100, 7, 6)];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_collection_items = cap;

        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy).unwrap();
        let result = admit_history_bound_scope_variants(&ctx, &mut scopes, &[]);
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == operation
        ));
        assert_eq!(scopes.len(), 1);
    }
}

#[test]
fn equivalent_scope_variant_refuses_work_and_materialization_limits() {
    use cadmpeg_core::decode::ResourceDimension;

    for (work_cap, materialized_cap, dimension, operation) in [
        (
            Some(0),
            None,
            ResourceDimension::WorkUnits,
            "f3d scope variant comparison",
        ),
        (
            None,
            Some(0),
            ResourceDimension::MaterializedBytes,
            "f3d scope variant JSON",
        ),
    ] {
        let mut scopes = vec![scope(42, 100, 7, 6), scope(42, 200, 9, 8)];
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        if let Some(cap) = work_cap {
            policy.limits.max_work_units = cap;
        }
        if let Some(cap) = materialized_cap {
            policy.limits.max_materialized_bytes = cap;
        }
        let refusal_cap =
            match cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                let mut scopes = scopes.clone();
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
                    cadmpeg_core::decode::ResourceDimension::RecursionDepth => {
                        policy.limits.max_recursion_depth = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
                        .unwrap();
                admit_history_bound_scope_variants(&ctx, &mut scopes, &[])
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
            cadmpeg_core::decode::ResourceDimension::RecursionDepth => {
                policy.limits.max_recursion_depth = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy).unwrap();
        let result = admit_history_bound_scope_variants(&ctx, &mut scopes, &[]);
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                if failure.dimension == dimension && failure.operation == operation
        ));
        assert_eq!(scopes.len(), 2);
    }
}

#[test]
fn equivalent_scope_json_refuses_nested_collection_depth_and_text_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    for (dimension, operation) in [
        (
            ResourceDimension::CollectionItems,
            "f3d configuration JSON object entry",
        ),
        (
            ResourceDimension::RecursionDepth,
            "f3d configuration JSON depth",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "f3d configuration JSON key",
        ),
    ] {
        let arena = DecodeArena::new();
        let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            let mut scopes = vec![scope(42, 100, 7, 6), scope(42, 200, 9, 8)];
            let mut policy = DecodePolicy::default();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = cap,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy).unwrap();
            let result = admit_history_bound_scope_variants(&ctx, &mut scopes, &[]);
            if result.is_err() {
                assert_eq!(scopes.len(), 2);
            }
            result
        });
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.dimension == dimension && failure.operation == operation));
    }
}

#[test]
fn retains_the_unique_history_bound_scope_envelope() {
    let mut scopes = vec![scope(42, 200, 9, 8), scope(42, 100, 7, 6)];
    let histories = [history(vec![
        history_state(7, Some(6)),
        history_state(6, None),
    ])];

    with_test_ctx(|ctx| admit_history_bound_scope_variants(ctx, &mut scopes, &histories))
        .expect("unique envelope");

    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].byte_offset(), 100);
}

#[test]
fn refuses_duplicate_scope_envelopes_without_one_history_binding() {
    let mut scopes = vec![scope(42, 100, 7, 6), scope(42, 200, 9, 8)];
    scopes[1].feature_ordinal = std::num::NonZeroU32::new(2).expect("nonzero ordinal");
    let histories = [history(vec![
        history_state(7, Some(6)),
        history_state(9, Some(8)),
    ])];

    assert!(
        with_test_ctx(|ctx| admit_history_bound_scope_variants(ctx, &mut scopes, &histories))
            .is_err()
    );
}

#[test]
fn retains_later_equivalent_scope_envelope_without_history_binding() {
    let mut older = scope(42, 100, 7, 6);
    older.class_tag =
        crate::records::references::DesignClassTag::try_from("392".to_owned()).unwrap();
    older
        .try_edit(|draft| {
            draft.frame_length = 260;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    older.feature_ordinal = std::num::NonZeroU32::new(1).expect("nonzero ordinal");
    older
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                vec![101, 102, 103],
                vec![110, 120, 130],
                "reference_members",
            )
            .unwrap();
            draft.reference_count_offset = *draft.reference_members.offsets().next().unwrap() - 5;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    older.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap();
    let mut newer = older.clone();
    newer.id = "f3d:stream:design-parameter-scope#200".into();
    newer
        .try_edit(|draft| {
            draft.byte_offset = 200;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    newer.class_tag =
        crate::records::references::DesignClassTag::try_from("404".to_owned()).unwrap();
    newer
        .try_edit(|draft| {
            draft.frame_length = 340;
            draft.history_state_id = Some(9);
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                draft.reference_members.values().copied().collect(),
                vec![210, 220, 230],
                "reference_members",
            )
            .unwrap();
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset = *draft.reference_members.offsets().next().unwrap() - 5;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    newer.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();

    let mut scopes = vec![older, newer];
    with_test_ctx(|ctx| admit_history_bound_scope_variants(ctx, &mut scopes, &[]))
        .expect("equivalent envelope");

    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].byte_offset(), 200);
}

/// Two same-index Thicken envelopes without a history binding, the second at
/// a later byte offset, whose signed thicknesses are `first` and `second`.
fn thicken_variants(first: f64, second: f64) -> Vec<DesignParameterScope> {
    [(100, first), (200, second)]
        .into_iter()
        .map(|(byte_offset, thickness)| {
            let mut scope = DesignParameterScope::empty(
                &format!("f3d:stream:design-parameter-scope#{byte_offset}"),
                crate::records::feature::scope::DesignFeatureKind::Thicken,
                42,
            );
            scope
                .try_edit(|draft| {
                    draft.byte_offset = byte_offset;
                    draft.reference_count_offset = draft.byte_offset + 9;
                    draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                    draft.layout_fixture_references();
                    draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                    draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                    draft.layout_fixture_tail();
                })
                .unwrap();
            let crate::records::feature::scope::DesignScopePayloadMut::Thicken(slot) =
                scope.payload_mut()
            else {
                panic!("a Thicken scope holds a Thicken payload");
            };
            *slot = Some(
                crate::records::feature::direct_face::DesignThickenOperation {
                    signed_thickness: crate::test_support::real(thickness),
                    thickness_record_index: 74,
                    thickness_offset: byte_offset + 40,
                },
            );
            scope
        })
        .collect()
}

/// The payload comparison states every difference of a checked float: the
/// later of two envelopes equal but for provenance is retained, and two
/// envelopes whose thicknesses differ in the last place stay unresolved.
#[test]
fn scope_variants_that_differ_in_a_payload_float_are_not_equivalent() {
    let mut equivalent = thicken_variants(-1.0, -1.0);
    with_test_ctx(|ctx| admit_history_bound_scope_variants(ctx, &mut equivalent, &[]))
        .expect("equivalent envelopes");
    assert_eq!(equivalent.len(), 1);
    assert_eq!(equivalent[0].byte_offset(), 200);

    let mut different = thicken_variants(-1.0, -1.0 - f64::EPSILON);
    assert!(
        with_test_ctx(|ctx| admit_history_bound_scope_variants(ctx, &mut different, &[])).is_err()
    );
}
