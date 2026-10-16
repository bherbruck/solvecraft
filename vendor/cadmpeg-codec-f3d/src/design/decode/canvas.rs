// SPDX-License-Identifier: Apache-2.0
//! Parse exact image-plane bindings owned by Design `Canvas` scopes.

use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::lp_utf16_bounded_charged;
use crate::container::ContainerScan;
use crate::design::decode::image::embedded_image_asset;
use crate::design::decode::scopes::shared_frames::marked_reference;
use crate::design::decode::sketch::next_indexed_record_offset_with_index;

use crate::ids;
use crate::records::{
    canvas::{
        DesignCanvasAsset, DesignCanvasBounds, DesignCanvasGeometry, DesignCanvasGeometryPayload,
        DesignCanvasImage, DesignCanvasPrologue,
    },
    feature::scope::DesignParameterScope,
};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::Asset;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::math::Point2;

const DESIGN_LENGTH_TO_MM: f64 = 10.0;

/// Decode every structurally complete Canvas geometry and image-asset record.
pub(crate) fn decode_canvas_images(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Vec<DesignCanvasImage>, CodecError> {
    super::image::decode_scoped_images(
        ctx,
        scan,
        scopes,
        &crate::records::feature::scope::DesignFeatureKind::Canvas,
        parse_canvas_image,
        |image| image.id.as_str(),
    )
}

/// Project uniquely bound Canvas images into neutral raster resources and
/// model-space reference-image features.
pub(crate) fn project_canvas_images(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    images: &[DesignCanvasImage],
    features: &mut [Feature],
) -> Result<Vec<Asset>, CodecError> {
    let mut assets = Vec::new();
    for image in images {
        let Some(scope) = scopes.iter().find(|scope| {
            scope.record_index == image.scope_record_index
                && crate::ids::native_stream(&scope.id) == crate::ids::native_stream(&image.id)
        }) else {
            continue;
        };
        let feature_id = crate::design::identity::neutral_feature_id(ctx, scope)?;
        let Some(feature) = features.iter_mut().find(|feature| feature.id == feature_id) else {
            continue;
        };
        let (mirror_u, mirror_v) = image.geometry().boundary.mirroring();
        let [minimum, maximum] = image.geometry().boundary.extents();
        let Some(asset) = embedded_image_asset(ctx, scan, image.asset_name())? else {
            continue;
        };
        let asset_id = asset
            .id
            .try_clone_for_decode(ctx, "f3d image feature asset identifier")?;
        if !assets
            .iter()
            .any(|candidate: &Asset| candidate.id == asset_id)
        {
            ctx.reserve_vec(&mut assets, 1, "f3d Canvas assets")?;
            assets.push(asset);
        }
        let (opacity, frame) = image.geometry().payload.decoded();
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(
                FeatureOperation::ReferenceImage {
                    asset: asset_id,
                    visible: image.geometry().prologue.visible(),
                    mirror_u,
                    mirror_v,
                    frame,
                    bounds: cadmpeg_ir::features::FeatureImageBounds::new([
                        Point2::new(
                            minimum.u * DESIGN_LENGTH_TO_MM,
                            minimum.v * DESIGN_LENGTH_TO_MM,
                        ),
                        Point2::new(
                            maximum.u * DESIGN_LENGTH_TO_MM,
                            maximum.v * DESIGN_LENGTH_TO_MM,
                        ),
                    ])
                    .ok_or_else(|| {
                        CodecError::malformed(
                            "Canvas bounds must have finite corners and nonzero extents",
                        )
                    })?,
                    opacity: Some(
                        cadmpeg_ir::scalar::Fraction::new(f64::from(opacity)).ok_or_else(|| {
                            CodecError::malformed(
                                "Canvas opacity must be finite and between zero and one",
                            )
                        })?,
                    ),
                },
            ));
    }
    ctx.stable_sort_by(
        &mut assets[..],
        |a, b| a.id.cmp(&b.id),
        |value| value.id.as_str().len(),
        "sort f3d design canvas 1",
    )?;
    Ok(assets)
}

fn parse_canvas_image(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    stream: &str,
    scope: &DesignParameterScope,
) -> Result<Option<DesignCanvasImage>, CodecError> {
    let parsed = (|| {
        let scope_at = usize::try_from(scope.byte_offset()).ok()?;
        let geometry_reference_at = if bytes.get(scope_at + 11..scope_at + 21)? == [0; 10] {
            scope_at + 21
        } else if bytes.get(scope_at + 11..scope_at + 20)? == [0; 9]
            && marked_reference(bytes, scope_at + 20)? == 0
        {
            scope_at + 25
        } else {
            return None;
        };
        let geometry_record_index = marked_reference(bytes, geometry_reference_at)?;
        let geometry_at = next_indexed_record_offset_with_index(bytes, 0, geometry_record_index)?;
        let (geometry_class_tag, after_geometry_tag) =
            lp_ascii_filtered_view(bytes, geometry_at, 0..=2000, u8::is_ascii_graphic)?;
        let geometry_prologue: [u8; 15] = bytes
            .get(geometry_at + 11..geometry_at + 26)?
            .try_into()
            .ok()?;
        let geometry_prologue = DesignCanvasPrologue::try_from(geometry_prologue).ok()?;
        if View::u32_le_at(bytes, after_geometry_tag)? != geometry_record_index {
            return None;
        }

        let paired_at = next_indexed_record_offset_with_index(
            bytes,
            geometry_at.checked_add(11)?,
            geometry_record_index,
        )?;
        let (paired_geometry_class_tag, after_paired_tag) =
            lp_ascii_filtered_view(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
        let paired_component_at = paired_at + 19;
        if View::u32_le_at(bytes, after_paired_tag)? != geometry_record_index
            || paired_at <= geometry_at
            || bytes.get(paired_at + 11..paired_component_at)? != [0; 8]
        {
            return None;
        }

        let boundary_offsets = [
            geometry_at + 26,
            geometry_at + 34,
            geometry_at + 42,
            geometry_at + 50,
            geometry_at + 181,
            geometry_at + 189,
            geometry_at + 197,
            geometry_at + 205,
        ];
        let mut coordinates = [0.0; 8];
        for (coordinate, offset) in coordinates.iter_mut().zip(boundary_offsets) {
            *coordinate = View::f64_le_at(bytes, offset)?;
        }
        let boundary_segments = [
            [
                Point2::new(coordinates[0], coordinates[1]),
                Point2::new(coordinates[2], coordinates[3]),
            ],
            [
                Point2::new(coordinates[4], coordinates[5]),
                Point2::new(coordinates[6], coordinates[7]),
            ],
        ];
        let boundary = DesignCanvasBounds::try_from(boundary_segments).ok()?;

        let plane_at = geometry_at + 58;
        let scope_reference_at = geometry_at + 146;
        let component_at = geometry_at + 157;
        let asset_at = geometry_at + 169;
        let plane_entity_suffix = marked_reference(bytes, plane_at)?;
        let scope_record_index = marked_reference(bytes, scope_reference_at)?;
        let component_entity_suffix = marked_reference(bytes, component_at)?;
        let asset_record_index = marked_reference(bytes, asset_at)?;
        if scope_record_index != scope.record_index
            || marked_reference(bytes, paired_component_at)? != component_entity_suffix
            || bytes.get(paired_component_at + 5..paired_component_at + 11)? != [0; 6]
            || bytes.get(plane_at + 5..plane_at + 11)? != [0; 6]
            || bytes.get(scope_reference_at + 5..scope_reference_at + 11)? != [0; 6]
            || bytes.get(component_at + 5..component_at + 12)? != [0; 7]
            || bytes.get(asset_at + 5..asset_at + 11)? != [0; 6]
            || bytes.get(asset_at + 11) != Some(&1)
        {
            return None;
        }
        let geometry_payload = bytes.get(geometry_at + 69..geometry_at + 146)?;
        let geometry_payload = DesignCanvasGeometryPayload::try_from(geometry_payload).ok()?;

        let (label, after_label) = match lp_utf16_bounded_charged(
            ctx,
            bytes,
            geometry_at + 213,
            1..=256,
            "f3d Design UTF-16 text",
        ) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if after_label != paired_at {
            return None;
        }
        let asset_record_at = paired_at.checked_add(30)?;
        let (asset_class_tag, after_asset_tag) =
            lp_ascii_filtered_view(bytes, asset_record_at, 0..=2000, u8::is_ascii_graphic)?;
        if View::u32_le_at(bytes, after_asset_tag)? != asset_record_index
            || bytes.get(asset_record_at + 11..asset_record_at + 21)? != [0; 10]
        {
            return None;
        }
        let (asset_name, after_asset_name) = match lp_utf16_bounded_charged(
            ctx,
            bytes,
            asset_record_at + 21,
            1..=1024,
            "f3d Design UTF-16 text",
        ) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if after_asset_name != scope_at {
            return None;
        }
        let geometry_class_tag =
            match ctx.copy_retained_text(geometry_class_tag, "f3d Canvas geometry class tag") {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            };
        let paired_geometry_class_tag = match ctx.copy_retained_text(
            paired_geometry_class_tag,
            "f3d Canvas paired geometry class tag",
        ) {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        };
        let asset_class_tag =
            match ctx.copy_retained_text(asset_class_tag, "f3d Canvas asset class tag") {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            };

        DesignCanvasImage::new(
            ids::native_design_canvas_image_id(stream, geometry_at),
            scope.record_index,
            u64::try_from(geometry_reference_at + 1).ok()?,
            DesignCanvasGeometry::new(
                [geometry_class_tag, paired_geometry_class_tag],
                geometry_record_index,
                u64::try_from(geometry_at).ok()?,
                label,
                geometry_prologue,
                boundary,
                geometry_payload,
            )
            .ok()?,
            DesignCanvasAsset::new(
                asset_class_tag.try_into().ok()?,
                asset_record_index,
                asset_name,
            )
            .ok()?,
            plane_entity_suffix,
            component_entity_suffix,
        )
        .ok()
        .map(Ok)
    })();
    parsed.transpose()
}

#[cfg(test)]
mod tests {
    use super::parse_canvas_image;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    fn header(bytes: &mut [u8], at: usize, tag: [u8; 3], index: u32) {
        bytes[at..at + 4].copy_from_slice(&3u32.to_le_bytes());
        bytes[at + 4..at + 7].copy_from_slice(&tag);
        bytes[at + 7..at + 11].copy_from_slice(&index.to_le_bytes());
    }

    fn reference(bytes: &mut [u8], at: usize, index: u32) {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&index.to_le_bytes());
    }

    fn fixture() -> (
        Vec<u8>,
        crate::records::feature::scope::DesignParameterScope,
    ) {
        let mut bytes = vec![0; 320];
        header(&mut bytes, 0, *b"300", 10);
        header(&mut bytes, 219, *b"301", 10);
        header(&mut bytes, 249, *b"302", 20);
        header(&mut bytes, 284, *b"303", 30);
        for (at, value) in [
            (26, 0.0f64),
            (34, 0.0),
            (42, 1.0),
            (50, 0.0),
            (181, 0.0),
            (189, 1.0),
            (197, 1.0),
            (205, 1.0),
            (98, 1.0),
            (130, 1.0),
        ] {
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes[69..73].copy_from_slice(&1.0f32.to_le_bytes());
        reference(&mut bytes, 58, 40);
        reference(&mut bytes, 146, 30);
        reference(&mut bytes, 157, 50);
        reference(&mut bytes, 169, 20);
        bytes[180] = 1;
        bytes[213..217].copy_from_slice(&1u32.to_le_bytes());
        bytes[217..219].copy_from_slice(&u16::from(b'L').to_le_bytes());
        reference(&mut bytes, 238, 50);
        bytes[270..274].copy_from_slice(&5u32.to_le_bytes());
        for (ordinal, unit) in "a.png".encode_utf16().enumerate() {
            let at = 274 + ordinal * 2;
            bytes[at..at + 2].copy_from_slice(&unit.to_le_bytes());
        }
        reference(&mut bytes, 305, 10);
        let scope = serde_json::from_value(serde_json::json!({
            "id": "f3d:Design/BulkStream.dat:scope#30",
            "byte_offset": 284,
            "class_tag": "303",
            "record_index": 30,
            "frame_length": 216,
            "kind": "Canvas",
            "kind_offset": 316,
            "feature_ordinal": 1,
            "feature_ordinal_offset": 428,
            "history_state_id": 8,
            "history_state_id_offset": 308,
            "previous_history_state_id": 7,
            "previous_history_state_id_offset": 458,
            "reference_count_offset": 293,
            "reference_members": [10],
            "reference_member_offsets": [298],
            "paired_class_tag": "304",
            "paired_byte_offset": 500
        }))
        .unwrap();
        (bytes, scope)
    }

    fn assert_class_tag_retained_refusal(limit: u64, operation: &'static str) {
        let (bytes, scope) = fixture();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = parse_canvas_image(&ctx, &bytes, "Design/BulkStream.dat", &scope)
            .err()
            .unwrap();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == operation)
        );
    }

    #[test]
    fn canvas_geometry_class_tag_refuses_retained_limit() {
        assert_class_tag_retained_refusal(8, "f3d Canvas geometry class tag");
    }

    #[test]
    fn canvas_paired_geometry_class_tag_refuses_retained_limit() {
        assert_class_tag_retained_refusal(11, "f3d Canvas paired geometry class tag");
    }

    #[test]
    fn canvas_asset_class_tag_refuses_retained_limit() {
        assert_class_tag_retained_refusal(14, "f3d Canvas asset class tag");
    }

    #[test]
    fn canvas_label_and_asset_name_refuse_retained_limits() {
        let (bytes, scope) = fixture();
        let image = parse_canvas_image(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            "Design/BulkStream.dat",
            &scope,
        )
        .unwrap()
        .unwrap();
        assert_eq!(image.asset_name(), "a.png");
        for limit in [0, 5] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let error = parse_canvas_image(&ctx, &bytes, "Design/BulkStream.dat", &scope)
                .err()
                .unwrap();
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "f3d Design UTF-16 text")
            );
        }
    }

    #[test]
    fn canvas_projection_refuses_asset_copy_and_output_limits() {
        use cadmpeg_ir::features::{
            Feature, FeatureDefinition, FeatureEvaluation, FeatureOperation, SketchFeatureBinding,
        };
        use std::collections::BTreeMap;
        use std::io::{Cursor, Write};
        use zip::CompressionMethod;

        const ENTRY: &str = "FusionAssetName[Active]/Design1/Images.BlobParts/a.png";
        let (bytes, scope) = fixture();
        let image = parse_canvas_image(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            "Design/BulkStream.dat",
            &scope,
        )
        .unwrap()
        .unwrap();
        let feature = || Feature {
            id: crate::ids::neutral_feature_id(&scope),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::Sketch {
                    sketch: SketchFeatureBinding::Unresolved,
                },
            )),
            native_ref: Some(scope.id.clone()),
        };
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
        zip.start_file(ENTRY, stored).unwrap();
        zip.write_all(b"PNG").unwrap();
        let archive = zip.finish().unwrap().into_inner();
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let asset_id_len = crate::ids::neutral_asset_id(ENTRY).as_str().len();
            let base = 3 + "a.png".len() + crate::ids::native_scope(ENTRY).len() + asset_id_len;
            for (retained, items, dimension, operation) in [
                (
                    u64::MAX,
                    0,
                    ResourceDimension::CollectionItems,
                    "f3d Canvas assets",
                ),
                (
                    u64::try_from(base + asset_id_len - 1).unwrap(),
                    u64::MAX,
                    ResourceDimension::RetainedBytes,
                    "f3d image feature asset identifier",
                ),
            ] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = retained;
                policy.limits.max_collection_items = items;

                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = super::project_canvas_images(
                    &ctx,
                    scan,
                    std::slice::from_ref(&scope),
                    std::slice::from_ref(&image),
                    &mut [feature()],
                );
                assert!(matches!(
                    result,
                    Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                        if failure.dimension == dimension && failure.operation == operation
                ));
            }
            let assets = super::project_canvas_images(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                std::slice::from_ref(&scope),
                std::slice::from_ref(&image),
                &mut [feature()],
            )
            .unwrap();
            assert_eq!(assets.len(), 1);
            assert_eq!(assets[0].id, crate::ids::neutral_asset_id(ENTRY));
        });
    }
}
