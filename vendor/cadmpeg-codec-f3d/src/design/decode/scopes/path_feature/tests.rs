// SPDX-License-Identifier: Apache-2.0
use super::exact_path_feature_construction;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::DesignPathFeatureConstruction;
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::indexed_header;

#[test]
fn compact_loft_prefix_reads_operation_at_offset_25_for_any_dynamic_class_tag() {
    for class_tag in ["301", "449"] {
        let mut bytes = Vec::new();
        let class_tag_bytes = class_tag
            .as_bytes()
            .try_into()
            .expect("three-byte class tag");
        indexed_header(&mut bytes, class_tag_bytes, 20);
        bytes.resize(128, 0);
        bytes[21..25].fill(1);
        bytes[25..29].copy_from_slice(&1u32.to_le_bytes());
        bytes[30..34].fill(0xff);

        let mut scope = DesignParameterScope::empty(
            "generated:loft#20",
            crate::records::feature::scope::DesignFeatureKind::Loft,
            20,
        );
        scope.class_tag =
            crate::records::references::DesignClassTag::try_from(class_tag.to_owned()).unwrap();
        scope
            .try_edit(|draft| {
                draft.frame_length = 128;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_tail();
            })
            .unwrap();
        let construction = exact_path_feature_construction(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope,
            &[],
        )
        .expect("compact Loft operation");
        assert_eq!(
            construction,
            DesignPathFeatureConstruction::Loft(
                crate::records::feature::path_features::DesignLoftConstruction {
                    operation: DesignExtrudeOperation::Join,
                    operation_offset: 25,
                }
            )
        );

        bytes[24] = 0;
        assert_eq!(
            exact_path_feature_construction(
                &bytes,
                &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
                &scope,
                &[],
            ),
            None
        );
    }
}
