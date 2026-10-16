// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::operands::indexed_operand_headers;
use crate::records::decal::DesignRecordHeader;
use crate::records::references::DesignClassTag;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn operand_header_index_refuses_collection_limit() {
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#7".to_owned(),
        record_index: 7,
        class_tag: DesignClassTag::try_from("301".to_owned()).unwrap(),
        byte_offset: 42,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        indexed_operand_headers(&ctx, std::slice::from_ref(&header)),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d operand header index"
    ));
}
