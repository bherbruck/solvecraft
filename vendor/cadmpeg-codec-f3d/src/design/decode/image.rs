// SPDX-License-Identifier: Apache-2.0
//! Transfer uniquely named Design image resources into neutral assets.

use cadmpeg_core::container::ContainerRole;

use crate::container::ContainerScan;
use crate::design::decode::sketch::native_scope_charged;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::{Asset, AssetContent};
use std::fmt::Write;

pub(super) fn neutral_asset_id_charged(
    ctx: &DecodeContext<'_>,
    entry_name: &str,
) -> Result<cadmpeg_ir::assets::AssetId, CodecError> {
    const PREFIX: &str = "f3d:model:asset#";
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded_len = 0usize;
    for character in entry_name.chars() {
        let bytes = character.len_utf8();
        let width = if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
            bytes.checked_mul(3)
        } else {
            Some(bytes)
        }
        .ok_or_else(|| ctx.refuse_codec_limit("f3d asset identifier length", 0, 1))?;
        encoded_len = encoded_len
            .checked_add(width)
            .ok_or_else(|| ctx.refuse_codec_limit("f3d asset identifier length", 0, 1))?;
    }
    let mut digits = 1usize;
    let mut remaining = encoded_len;
    while remaining >= 10 {
        remaining /= 10;
        digits += 1;
    }
    let capacity = PREFIX
        .len()
        .checked_add(digits)
        .and_then(|length| length.checked_add(1))
        .and_then(|length| length.checked_add(encoded_len))
        .ok_or_else(|| ctx.refuse_codec_limit("f3d asset identifier length", 0, 1))?;

    let mut id = ctx.retained_string(capacity, "f3d asset identifier")?;
    id.push_str(PREFIX);
    write!(&mut id, "{encoded_len}:")
        .map_err(|_| CodecError::malformed("F3D asset identifier formatting failed"))?;
    for character in entry_name.chars() {
        if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
            let mut bytes = [0u8; 4];
            for byte in character.encode_utf8(&mut bytes).as_bytes() {
                id.push('%');
                id.push(char::from(HEX[usize::from(byte >> 4)]));
                id.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        } else {
            id.push(character);
        }
    }
    cadmpeg_ir::assets::AssetId::mint(id)
        .map_err(|error| crate::design::text::malformed_design(ctx, format_args!("{error}")))
}

pub(super) fn embedded_image_asset(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    asset_name: &str,
) -> Result<Option<Asset>, CodecError> {
    let mut entries = scan.entries.iter().filter(|entry| {
        scan.is_design_asset_entry(entry, ContainerRole::Image)
            && entry.name.rsplit('/').next() == Some(asset_name)
    });
    let (Some(entry), None) = (entries.next(), entries.next()) else {
        return Ok(None);
    };
    let media_type = std::path::Path::new(asset_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(|extension| {
            if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
                Some("image/jpeg")
            } else if extension.eq_ignore_ascii_case("png") {
                Some("image/png")
            } else {
                None
            }
        })
        .map(str::to_owned);
    if entry.attributes.contains_key("payload_error") {
        return Ok(None);
    }
    let data = ctx.copy_retained(scan.entry_bytes(&entry.name)?, "f3d embedded image data")?;
    let name =
        String::from_utf8(ctx.copy_retained(asset_name.as_bytes(), "f3d embedded image name")?)
            .map_err(|_| CodecError::Malformed("asset name must be UTF-8".into()))?;
    let native_ref = native_scope_charged(ctx, &entry.name)?;
    Ok(Some(
        Asset::try_new(
            neutral_asset_id_charged(ctx, &entry.name)?,
            Some(name),
            media_type,
            AssetContent::Embedded {
                data: cadmpeg_ir::assets::AssetData::new(data)
                    .ok_or_else(|| CodecError::Malformed("asset data must not be empty".into()))?,
            },
            Some(native_ref),
        )
        .map_err(CodecError::Malformed)?,
    ))
}

/// Decode image scopes in their owning streams, ordered by native identity.
pub(super) fn decode_scoped_images<T>(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    kind: &crate::records::feature::scope::DesignFeatureKind,
    mut parse: impl FnMut(
        &DecodeContext<'_>,
        &[u8],
        &str,
        &crate::records::feature::scope::DesignParameterScope,
    ) -> Result<Option<T>, CodecError>,
    id: impl Fn(&T) -> &str,
) -> Result<Vec<T>, CodecError> {
    let mut images = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let stream = native_scope_charged(ctx, &entry.name)?;
        for scope in scopes.iter().filter(|scope| {
            scope.kind().as_str() == kind.as_str()
                && crate::ids::native_stream(&scope.id) == Some(stream.as_str())
        }) {
            if let Some(image) = parse(ctx, bytes, &entry.name, scope)? {
                ctx.reserve_vec(&mut images, 1, "f3d scoped image records")?;
                images.push(image);
            }
        }
    }
    ctx.stable_sort_by(
        &mut images[..],
        |a, b| id(a).cmp(id(b)),
        |_| 0,
        "sort f3d design image 1",
    )?;
    images.dedup_by(|a, b| id(a) == id(b));
    Ok(images)
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::u64_from_index;

    use std::io::{Cursor, Write};

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use zip::CompressionMethod;

    use crate::test_support::manifest_test::write_synthetic_manifests;
    use crate::test_support::zip_test::with_scan;

    #[test]
    fn embedded_image_asset_refuses_each_retained_copy_limit() {
        const ENTRY: &str = "FusionAssetName[Active]/Design1/Images.BlobParts/mark.png";
        const NAME: &str = "mark.png";
        const DATA: &[u8] = b"PNG";
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file(ENTRY, stored).unwrap();
        zip.write_all(DATA).unwrap();
        let archive = zip.finish().unwrap().into_inner();
        with_scan(&archive, |scan| {
            for (limit, operation) in [
                (0, "f3d embedded image data"),
                (u64_from_index(DATA.len()), "f3d embedded image name"),
                (
                    u64_from_index(DATA.len() + NAME.len()),
                    "f3d native stream key",
                ),
            ] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = limit;

                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                assert!(matches!(
                    super::embedded_image_asset(&ctx, scan, NAME),
                    Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                        if failure.dimension == ResourceDimension::RetainedBytes
                            && failure.operation == operation
                ));
            }
            crate::design::test_support::with_test_decode_context(|ctx| {
                assert!(super::embedded_image_asset(ctx, scan, NAME)
                    .unwrap()
                    .is_some());
            });
        });
    }

    #[test]
    fn scoped_image_collection_refuses_before_growth() {
        const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file(ENTRY, stored).unwrap();
        zip.write_all(b"scope").unwrap();
        let archive = zip.finish().unwrap().into_inner();
        let stream = crate::ids::native_scope(ENTRY);
        let scope: crate::records::feature::scope::DesignParameterScope =
            serde_json::from_value(serde_json::json!({
                "id": format!("{stream}:scope#1"),
                "byte_offset": 0,
                "class_tag": "300",
                "record_index": 1,
                "frame_length": 200,
                "kind": "Fillet",
                "kind_offset": 32,
                "feature_ordinal": 1,
                "feature_ordinal_offset": 128,
                "history_state_id": 8,
                "history_state_id_offset": 24,
                "previous_history_state_id": 7,
                "previous_history_state_id_offset": 158,
                "reference_count_offset": 9,
                "reference_members": [2],
                "reference_member_offsets": [14],
                "fixed_fillet_parameters": {
                    "groups": [{
                        "tangency_weight": {"value": 1.0, "record_index": 4, "value_offset": 0},
                        "radii": [0.3],
                        "radius_record_indexes": [5],
                        "radius_offsets": [0],
                        "intermediate_parameters": [],
                        "intermediate_parameter_record_indexes": [],
                        "intermediate_parameter_offsets": []
                    }]
                },
                "paired_class_tag": "261",
                "paired_byte_offset": 200
            }))
            .unwrap();
        let kind = crate::records::feature::scope::DesignFeatureKind::Fillet;
        with_scan(&archive, |scan| {
            for (dimension, operation) in [
                (ResourceDimension::RetainedBytes, "f3d native stream key"),
                (
                    ResourceDimension::CollectionItems,
                    "f3d scoped image records",
                ),
            ] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                if dimension == ResourceDimension::RetainedBytes {
                    policy.limits.max_retained_bytes = 0;
                } else {
                    policy.limits.max_collection_items = 0;
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                assert!(matches!(
                    super::decode_scoped_images(
                        &ctx, scan, std::slice::from_ref(&scope), &kind,
                        |_, _, _, _| Ok(Some(17_u32)), |_| "image",
                    ),
                    Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                        if failure.dimension == dimension && failure.operation == operation
                ));
            }
            crate::design::test_support::with_test_decode_context(|ctx| {
                let images = super::decode_scoped_images(
                    ctx,
                    scan,
                    std::slice::from_ref(&scope),
                    &kind,
                    |_, _, _, _| Ok(Some(17_u32)),
                    |_| "image",
                )
                .unwrap();
                assert_eq!(images, [17]);
            });
        });
    }

    #[test]
    fn embedded_asset_identifier_refuses_retained_limit() {
        for entry in ["plain.png", "a:b#c%d.png", "spaced\tname é.png"] {
            let expected = crate::ids::neutral_asset_id(entry);
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_retained_bytes = u64::try_from(expected.as_str().len() - 1).unwrap();
            let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let refusal = super::neutral_asset_id_charged(&limited, entry);
            assert!(matches!(
                refusal,
                Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::RetainedBytes
                        && failure.operation == "f3d asset identifier"
            ));
            let admitted = super::neutral_asset_id_charged(
                &cadmpeg_test_support::service_decode_context(),
                entry,
            )
            .unwrap();
            assert_eq!(admitted, expected);
        }
    }
}
