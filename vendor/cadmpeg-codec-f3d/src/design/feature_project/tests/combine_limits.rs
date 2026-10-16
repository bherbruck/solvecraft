// SPDX-License-Identifier: Apache-2.0
use crate::design::feature_project::project_combine;
use crate::records::feature::combine::{
    DesignCombineBodySelection, DesignCombineForm, DesignCombineOperation, DesignCombineTools,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const STREAM: &str = "f3d:Design/BulkStream.dat";

fn fixture(multiple: bool) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::Combine,
        10,
    );
    if let DesignScopePayloadMut::Combine(slot) = scope.payload_mut() {
        *slot = Some(DesignCombineOperation {
            form: DesignCombineForm::Standard,
            operation: cadmpeg_ir::features::BooleanKind::Join,
            operation_offset: 0,
            keep_tools: false,
            keep_tools_offset: 0,
            target_record_index: 11,
            tools: DesignCombineTools {
                first: DesignCombineBodySelection {
                    record_index: 12,
                    external_identity: None,
                },
                additional: if multiple {
                    vec![DesignCombineBodySelection {
                        record_index: 13,
                        external_identity: None,
                    }]
                } else {
                    Vec::new()
                },
            },
        });
    }
    scope
}

fn assert_refusal(operation: &'static str, dimension: ResourceDimension, multiple: bool) {
    let scope = fixture(multiple);
    for limit in 0..256 {
        let mut policy = DecodePolicy::default();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            _ => panic!("unsupported dimension"),
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match project_combine(&ctx, &scope, STREAM) {
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
fn combine_target_id_refuses_retained_limit() {
    assert_refusal(
        "f3d Combine target id",
        ResourceDimension::RetainedBytes,
        false,
    );
}

#[test]
fn combine_single_tool_id_refuses_retained_limit() {
    assert_refusal(
        "f3d Combine single tool id",
        ResourceDimension::RetainedBytes,
        false,
    );
}

#[test]
fn combine_tool_set_id_refuses_retained_limit() {
    assert_refusal(
        "f3d Combine tool set id",
        ResourceDimension::RetainedBytes,
        true,
    );
}

#[test]
fn combine_tool_selection_refuses_collection_limit() {
    assert_refusal(
        "f3d Combine tool selection",
        ResourceDimension::CollectionItems,
        true,
    );
}

#[test]
fn combine_tool_uniqueness_refuses_collection_limit() {
    assert_refusal(
        "f3d Combine tool uniqueness",
        ResourceDimension::CollectionItems,
        true,
    );
}
