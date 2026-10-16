// SPDX-License-Identifier: Apache-2.0
use crate::design::feature_project::design_body_selection;
use crate::records::bodies::{DesignBodyBinding, DesignBodyBindingWire};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn fixture() -> (DesignParameterScope, DesignBodyBinding) {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::BaseFeature,
        10,
    );
    let binding = DesignBodyBinding::try_from(DesignBodyBindingWire {
        id: "f3d:Design/BulkStream.dat:design-body-binding#10".to_owned(),
        stream: "Design/BulkStream.dat".to_owned(),
        pair_count: 1,
        pair_ordinal: 0,
        asm_body_key: 7,
        asm_body_key_offset: 10,
        entity_suffix: 20,
        entity_suffix_offset: 18,
        blob_name: "BREP.body".to_owned(),
        blob_name_offset: 30,
        body: Some(cadmpeg_ir::ids::BodyId::try_from("f3d:model:body#1").unwrap()),
    })
    .unwrap();
    (scope, binding)
}

fn assert_selection_refusal(operation: &'static str, retained: bool) {
    let (scope, binding) = fixture();

    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            if retained {
                ResourceDimension::RetainedBytes
            } else {
                ResourceDimension::CollectionItems
            },
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                if retained {
                    policy.limits.max_retained_bytes = cap;
                } else {
                    policy.limits.max_collection_items = cap;
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = design_body_selection(
                    &ctx,
                    &scope,
                    [20].into_iter(),
                    std::slice::from_ref(&binding),
                );
                result
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems })));
    }
}

#[test]
fn body_selection_body_refuses_collection_limit() {
    assert_selection_refusal("f3d body selection body", false);
}

#[test]
fn body_selection_body_id_refuses_retained_limit() {
    assert_selection_refusal("f3d body selection body id", true);
}

#[test]
fn body_selection_resolved_native_id_refuses_retained_limit() {
    assert_selection_refusal("f3d body selection resolved native id", true);
}

#[test]
fn body_selection_missing_binding_native_id_refuses_retained_limit() {
    let (scope, _) = fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = design_body_selection(&ctx, &scope, [20].into_iter(), &[]).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::RetainedBytes
            && failure.operation == "f3d body selection native id"));
}

fn project_copied_body(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    binding: &DesignBodyBinding,
) -> Result<Vec<cadmpeg_ir::features::Feature>, CodecError> {
    let timeline = crate::records::entity_header::DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream("f3d:Design/BulkStream.dat", 0),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            0,
            vec![crate::records::identity::Located {
                value: 10,
                offset: 0,
            }],
        ),
        crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(1).unwrap(),
    )
    .unwrap();
    crate::design::feature_project::project_parameter_design_with_edge_identities(
        ctx,
        &crate::design::feature_project::ProjectInputs {
            scopes: std::slice::from_ref(scope),
            timelines: std::slice::from_ref(&timeline),
            body_bindings: std::slice::from_ref(binding),
            ..Default::default()
        },
    )
    .map(|(features, _)| features)
}

fn copied_body_fixture() -> (DesignParameterScope, DesignBodyBinding) {
    use crate::records::feature::body_ops::{DesignCopiedBody, DesignCopyPasteBodiesOperation};
    let (_, binding) = fixture();
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::CopyPasteBodies,
        10,
    );
    scope.feature_ordinal = std::num::NonZeroU32::new(1).unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::CopyPasteBodies(slot) =
        scope.payload_mut()
    {
        *slot = Some(
            crate::test_support::with_decode_context(|ctx| {
                DesignCopyPasteBodiesOperation::try_new_charged(
                    ctx,
                    vec![DesignCopiedBody {
                        operand: crate::records::identity::Located {
                            value: 502,
                            offset: 26,
                        },
                        source: crate::records::identity::Located {
                            value: 10,
                            offset: 25,
                        },
                        copied: crate::records::identity::Located {
                            value: 20,
                            offset: 40,
                        },
                    }],
                    crate::records::feature::body_ops::CopyPasteRecordLocation {
                        record_index: 501,
                        class_tag: crate::records::references::DesignClassTag::try_from(
                            "264".to_owned(),
                        )
                        .unwrap(),
                        byte_offset: 0,
                    },
                    crate::records::feature::body_ops::CopyPasteRecordLocation {
                        record_index: 503,
                        class_tag: crate::records::references::DesignClassTag::try_from(
                            "264".to_owned(),
                        )
                        .unwrap(),
                        byte_offset: 0,
                    },
                )
            })
            .unwrap(),
        );
    }
    (scope, binding)
}

fn assert_copied_body_output_refusal(operation: &'static str, retained: bool) {
    let (scope, binding) = copied_body_fixture();

    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            if retained {
                ResourceDimension::RetainedBytes
            } else {
                ResourceDimension::CollectionItems
            },
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                if retained {
                    policy.limits.max_retained_bytes = cap;
                } else {
                    policy.limits.max_collection_items = cap;
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                project_copied_body(&ctx, &scope, &binding)
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (if retained { ResourceDimension::RetainedBytes } else { ResourceDimension::CollectionItems })));
    }
}

#[test]
fn copied_body_output_refuses_collection_limit() {
    assert_copied_body_output_refusal("f3d copied body output", false);
}

#[test]
fn copied_body_output_id_refuses_retained_limit() {
    assert_copied_body_output_refusal("f3d copied body output id", true);
}

#[test]
fn copied_body_output_preserves_resolved_body() {
    let (scope, binding) = copied_body_fixture();
    let features = crate::test_support::with_decode_context(|decode_ctx| {
        project_copied_body(decode_ctx, &scope, &binding)
    })
    .unwrap();
    assert_eq!(features.len(), 1);
    assert_eq!(
        features[0].evaluation.outputs().as_slice(),
        [binding.body.as_ref().unwrap().clone()]
    );
}
