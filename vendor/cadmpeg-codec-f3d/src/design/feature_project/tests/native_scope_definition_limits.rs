// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::feature_project::native_scope_definition;
use crate::design::test_support::parameter_record;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn fixture() -> (
    DesignParameterScope,
    crate::records::parameters::DesignParameter,
) {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::Combine,
        10,
    );
    let parameter = parse_design_parameter_record(&parameter_record(
        None,
        "2 mm",
        "User Parameter",
        Some("mm"),
        "Length",
        2.0,
    ))
    .unwrap();
    (scope, parameter)
}

fn assert_refusal(operation: &'static str, dimension: ResourceDimension) {
    let (scope, parameter) = fixture();
    for limit in 0..128 {
        let mut policy = DecodePolicy::default();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            _ => panic!("unsupported dimension"),
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match native_scope_definition(&ctx, &scope, &[(0, &parameter)]) {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == operation && failure.dimension == dimension =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            Ok(_) => panic!("expected {operation} refusal"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn native_parameter_name_refuses_retained_limit() {
    assert_refusal(
        "f3d native parameter name",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn native_parameter_expression_refuses_retained_limit() {
    assert_refusal(
        "f3d native parameter expression",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn native_parameter_property_refuses_collection_limit() {
    assert_refusal(
        "f3d native parameter property",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn native_parameter_restated_key_keeps_refusal() {
    let (scope, parameter) = fixture();
    let error = crate::test_support::with_decode_context(|decode_ctx| {
        native_scope_definition(decode_ctx, &scope, &[(0, &parameter), (1, &parameter)])
    })
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("states the property Length a second time"));
}

#[test]
fn native_scope_kind_refuses_retained_limit() {
    let kind = "source雪%";
    let scope = DesignParameterScope::empty(
        "f3d:test:design-parameter-scope#10",
        DesignFeatureKind::try_from(kind.to_owned()).unwrap(),
        10,
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = u64::try_from(kind.len() - 1).unwrap();

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(native_scope_definition(&ctx, &scope, &[]),
        Err(CodecError::ResourceLimit(failure)) if failure.operation == "f3d native feature kind"
            && failure.dimension == ResourceDimension::RetainedBytes));
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        native_scope_definition(decode_ctx, &scope, &[])
    })
    .unwrap();
    assert!(
        matches!(definition, cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Other(ref name), .. }) if name == kind)
    );
}
