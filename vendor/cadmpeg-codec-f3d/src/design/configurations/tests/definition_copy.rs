// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, NativeFeatureKind};

#[test]
fn suppressed_definition_kind_refuses_retained_limit() {
    let (mut configurations, mut feature) = super::suppression_limit_fixture();
    let kind = "source雪%";
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Native {
            kind: NativeFeatureKind::Other(kind.to_owned()),
            parameters: std::collections::BTreeMap::new(),
        }));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes =
        u64::try_from(feature.id.as_str().len() + kind.len() - 1).unwrap();
    let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "f3d configuration suppressed definition",
        |cap| {
            let mut configurations = configurations.clone();
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
            super::bind_configuration_suppressed_features(
                &ctx,
                &mut configurations,
                std::slice::from_ref(&feature),
            )
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
        "f3d configuration suppressed definition",
        |cap| {
            let mut configurations = configurations.clone();
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
            super::bind_configuration_suppressed_features(
                &ctx,
                &mut configurations,
                std::slice::from_ref(&feature),
            )
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
    assert!(matches!(super::bind_configuration_suppressed_features(
        &ctx, &mut configurations, std::slice::from_ref(&feature)),
        Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::RetainedBytes
            && failure.operation == "f3d configuration suppressed definition"));
    assert!(configurations[0].feature_states.is_empty());
}

#[test]
fn suppressed_definition_parameter_value_refuses_retained_limit() {
    let (mut configurations, mut feature) = super::suppression_limit_fixture();
    let key = "distance";
    let value = "source雪%";
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Native {
            kind: NativeFeatureKind::Fillet,
            parameters: std::collections::BTreeMap::from([(
                cadmpeg_core::text::NonBlankString::new(key.to_owned()).unwrap(),
                value.to_owned(),
            )]),
        }));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes =
        u64::try_from(feature.id.as_str().len() + key.len() + value.len() - 1).unwrap();
    let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "f3d configuration suppressed definition",
        |cap| {
            let mut configurations = configurations.clone();
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
            super::bind_configuration_suppressed_features(
                &ctx,
                &mut configurations,
                std::slice::from_ref(&feature),
            )
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
        "f3d configuration suppressed definition",
        |cap| {
            let mut configurations = configurations.clone();
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
            super::bind_configuration_suppressed_features(
                &ctx,
                &mut configurations,
                std::slice::from_ref(&feature),
            )
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
    assert!(matches!(super::bind_configuration_suppressed_features(
        &ctx, &mut configurations, std::slice::from_ref(&feature)),
        Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::RetainedBytes
            && failure.operation == "f3d configuration suppressed definition"));
    assert!(configurations[0].feature_states.is_empty());
}

#[test]
fn suppressed_definition_parameters_refuse_collection_limit() {
    let (mut configurations, mut feature) = super::suppression_limit_fixture();
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Native {
            kind: NativeFeatureKind::Fillet,
            parameters: std::collections::BTreeMap::from([(
                cadmpeg_core::text::NonBlankString::new("distance".to_owned()).unwrap(),
                "value".to_owned(),
            )]),
        }));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    // One configuration feature state and one definition parameter need two items.
    policy.limits.max_collection_items = 1;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(super::bind_configuration_suppressed_features(
        &ctx, &mut configurations, std::slice::from_ref(&feature)),
        Err(CodecError::ResourceLimit(failure)) if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d configuration suppressed definition"));
    assert!(configurations[0].feature_states.is_empty());
}
