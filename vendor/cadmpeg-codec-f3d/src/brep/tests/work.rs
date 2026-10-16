// SPDX-License-Identifier: Apache-2.0
#[test]
fn typed_graph_walks_preserve_work_refusals() {
    for qualification in [true, false] {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 1;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let mut graph = super::one_body_brep();
            let error = if qualification {
                graph.qualify_ids(ctx, crate::ids::ID_FORMAT, "source")
            } else {
                graph.retain_body_keys(ctx, &std::collections::HashSet::new())
            }
            .unwrap_err();
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                panic!("typed walk must refuse")
            };
            assert_eq!(
                limit.dimension,
                cadmpeg_core::decode::ResourceDimension::WorkUnits
            );
            assert_eq!(Some(limit), ctx.resource_refusal());
        });
    }
}

#[test]
fn body_selector_scans_preserve_work_refusals() {
    let key = cadmpeg_asm::brep::records::BodyNativeKey {
        source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
            crate::ids::ID_FORMAT,
        ),
        body: cadmpeg_ir::ids::BodyId::mint("f3d:brep:body#1").unwrap(),
        record_index: 1,
        body_ordinal: 0,
        source_brep: None,
        asm_body_key: Some(7),
    };
    for (work, selector, operation) in [
        (0, 7, "match F3D native body selector"),
        (1, 0, "match F3D ordinal body selector"),
    ] {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = work;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let error =
                super::super::resolve_body_selector(ctx, [&key].into_iter(), selector).unwrap_err();
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                panic!("selector must refuse");
            };
            assert_eq!(limit.operation, operation);
            assert_eq!(Some(limit), ctx.resource_refusal());
        });
    }
}
