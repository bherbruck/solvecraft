// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{u64_from_index, DecodePolicy, ResourceDimension};

#[test]
fn xref_design_text_refuses_retained_transition() {
    let bytes = br#"{"name":"RedirectionsStream","schema-version":0,"designs":[{"file-version":1,"targetFileName":"part.f3d","displayName":"part","lineageUrn":"urn:lineage","versionUrn":"urn:version"}],"references":{}}"#;
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::RetainedBytes,
        "retain F3D xref design text",
        0,
        |ctx| crate::xref::parse(ctx, bytes),
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("design text must be admitted");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "retain F3D xref design text");
}

#[test]
fn xref_reference_text_refuses_retained_transition() {
    let reference = serde_json::from_slice::<crate::xref::ReferenceJson>(br#"{"type":"XREF","from":"root.f3d","relativePath":"part.f3d","properties":[{"neutronRole":{"dataType":"STRING","value":"role"}},{"neutronData":{"dataType":"STRING","value":"data"}}]}"#).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64_from_index("f3d:xref:reference#0".len());
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let error = reference.into_record(ctx, 0).unwrap_err();
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("reference text must be admitted");
        };
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(limit.operation, "retain F3D xref reference text");
        assert_eq!(Some(limit), ctx.resource_refusal());
    });
}
