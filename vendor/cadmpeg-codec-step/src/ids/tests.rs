// SPDX-License-Identifier: Apache-2.0
use crate::ids;
use cadmpeg_ir::{is_valid_identity, IdentityError};

#[test]
fn signature_uses_three_component_grammar() {
    let id = ids::signature(0);
    assert_eq!(id.as_str(), "step:file:signature#0");
    assert!(is_valid_identity(id.as_str()));
}

#[test]
fn data_and_opaque_preserve_existing_forms() {
    assert_eq!(
        ids::data(kind!("surface"), 12u64).as_str(),
        "step:data:surface#12"
    );
    assert!(is_valid_identity(
        ids::data(kind!("edge"), key_word!("3-shell-4")).as_str()
    ));
}

#[test]
fn scoped_builders_preserve_existing_forms() {
    assert_eq!(
        ids::product(kind!("occurrence"), key_word!("definition-9")).as_str(),
        "step:product:occurrence#definition-9"
    );
    assert_eq!(
        ids::presentation(kind!("pmi"), 4u64).as_str(),
        "step:presentation:pmi#4"
    );
    assert_eq!(
        ids::construction(kind!("trimmed_curve"), 9u64).as_str(),
        "step:construction:trimmed_curve#9"
    );
    assert_eq!(
        ids::tessellation(kind!("mesh"), 1u64).as_str(),
        "step:tessellation:mesh#1"
    );
    assert_eq!(
        ids::drawing(kind!("drawing_definition"), 2u64).as_str(),
        "step:drawing:drawing_definition#2"
    );
}

#[test]
fn empty_scope_is_rejected_at_construction() {
    // The Phase 1 regression was `step:signature#0` (missing scope).
    let err =
        cadmpeg_ir::ids::IdentityNamespace::new("step", "", "signature").expect_err("empty scope");
    assert!(matches!(
        err,
        IdentityError::InvalidComponent { label: "scope", .. }
    ));
    assert!(!is_valid_identity("step:signature#0"));
}
