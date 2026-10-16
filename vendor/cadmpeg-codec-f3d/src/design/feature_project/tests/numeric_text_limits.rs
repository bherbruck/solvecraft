// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::test_support::parameter_record;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn projected_parameter_owner_record_text_refuses_retained_limit() {
    assert_numeric_text_refusal("f3d projected parameter owner record text", "mm");
}

#[test]
fn projected_parameter_evaluated_scalar_text_refuses_retained_limit() {
    assert_numeric_text_refusal("f3d projected parameter evaluated scalar text", "custom");
}

fn assert_numeric_text_refusal(operation: &'static str, unit: &str) {
    let parameter = parse_design_parameter_record(&parameter_record(
        Some(41),
        "2 mm",
        "FeatureInput",
        Some(unit),
        "Value",
        0.2,
    ))
    .unwrap();
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            &ctx,
            &crate::design::feature_project::ProjectInputs {
                native: std::slice::from_ref(&parameter),
                ..Default::default()
            },
        )
    };
    for limit in 0..4096 {
        match run(limit) {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == operation
                    && failure.dimension == ResourceDimension::RetainedBytes =>
            {
                let below = failure.used.checked_add(failure.additional).unwrap() - 1;
                assert!(matches!(run(below), Err(CodecError::ResourceLimit(failure))
                    if failure.operation == operation && failure.dimension == ResourceDimension::RetainedBytes));
                return;
            }
            Err(CodecError::ResourceLimit(_)) => {}
            result => panic!("missing {operation} refusal: {result:?}"),
        }
    }
    panic!("no {operation} refusal");
}
