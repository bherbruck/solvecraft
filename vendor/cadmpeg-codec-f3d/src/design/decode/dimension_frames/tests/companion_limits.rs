// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::dimension_frames::parse_dimension_null_locus_pair;
use std::collections::HashSet;

#[test]
fn typed_dimension_companions_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    let mut bytes = vec![0; 74];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"277");
    bytes[7..11].copy_from_slice(&1394u32.to_le_bytes());
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    bytes[24] = 1;
    bytes[35..39].copy_from_slice(&10u32.to_le_bytes());
    bytes[39] = 1;
    bytes[40..44].copy_from_slice(&1109u32.to_le_bytes());
    bytes[50..54].copy_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"273");
    bytes.extend_from_slice(&1394u32.to_le_bytes());
    let mut pair =
        parse_dimension_null_locus_pair(&bytes, 0, 1290, &HashSet::from([1109])).unwrap();
    pair.id = "f3d:Design/BulkStream.dat:design-dimension-null-locus-pair#0".into();

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&[]).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let inputs = crate::design::decode::dimension_frames::DimensionDecodeInputs {
            scan,
            placements: &[],
            parameters: &[],
            owners: &[],
            companions: &[],
            scopes: &[],
            headers: &[],
            points: &[],
            curves: &[],
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            crate::design::decode::dimension_frames::decode_dimension_null_locus_pairs(&ctx, &inputs, &[pair], &[]),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == "f3d typed dimension companions"
        ));
    });
}

#[test]
fn dimension_presentation_sketch_scopes_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::io::{Cursor, Write};
    use zip::CompressionMethod;

    let placement = crate::records::sketch_placement::DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::MemberCompact {
                paired_byte_offset: 34,
            },
        )
        .unwrap(),
        id: "f3d:design:design-sketch-placement#1".into(),
        scope_record_index: Some(1),
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_201".to_owned())
            .unwrap(),
        visibility: None,
        class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        record_index: 1,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("257".to_owned())
            .unwrap(),
    };
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    zip.write_all(&[]).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let inputs = crate::design::decode::dimension_frames::DimensionDecodeInputs {
            scan,
            placements: std::slice::from_ref(&placement),
            parameters: &[],
            owners: &[],
            companions: &[],
            scopes: &[],
            headers: &[],
            points: &[],
            curves: &[],
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = 0;

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            crate::design::decode::dimension_frames::decode_dimension_presentation_frames(&ctx, &inputs, &[]),
            Err(CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::CollectionItems
                    && failure.operation == "f3d dimension presentation sketch scopes"
        ));
    });
}
