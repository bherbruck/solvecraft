// SPDX-License-Identifier: Apache-2.0

use super::exact_assembly_alignment;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::parameters::{DesignParameterOwner, DesignParameterOwnerWire};
use crate::records::references::DesignClassTag;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

#[test]
fn assembly_alignment_lanes_refuse_collection_limit() {
    let scope_record_index = 10;
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Assemble,
        scope_record_index,
    );
    let owner = DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: "f3d:Design/BulkStream.dat:design-parameter-owner#50".into(),
        byte_offset: 461,
        frame_length: 104,
        class_tag: DesignClassTag::try_from("457".to_owned()).unwrap(),
        record_index: 50,
        scope_record_index,
        local_ordinal: 0,
        evaluated_value: 3.0,
        evaluated_value_offset: 501,
        parameter_record_index: 51,
        owned_ordinal: 0,
        variant: Some(0),
        companion_record_index: 52,
    })
    .unwrap();
    let records = crate::design::test_support::indexed_record_offsets_for_test(&[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = exact_assembly_alignment(&ctx, &[], &records, &scope, std::slice::from_ref(&owner))
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d assembly alignment lanes")
    );

    let arena = DecodeArena::new();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        exact_assembly_alignment(&ctx, &[], &records, &scope, &[owner])
            .unwrap()
            .is_none()
    );
}
