// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

use cadmpeg_core::decode::u64_from_index;

use std::io::{Cursor, Write};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use zip::CompressionMethod;

use super::{
    decode_parameters, parse_design_parameter_record, parse_legacy_parameter_owner_68,
    parse_legacy_parameter_owner_88, parse_parameter_companion, parse_parameter_owner,
};
use crate::design::test_support::{parameter_owner_frame, parameter_record};
use crate::records::{
    parameters::{DesignParameterCompanion, DesignParameterKind, DesignParameterOwner},
    recipes::{ConstructionRecipe, ConstructionRecipeKind},
};
use crate::test_support::lp_utf16;
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::zip_test::with_scan;

fn compact_owned_parameter_record(
    owner_record_index: u32,
    source_ordinal: u32,
    expression: &str,
    source_kind: &str,
    unit: Option<&str>,
    name: &str,
    evaluated_value: f64,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"328");
    out.extend_from_slice(&(owner_record_index + 1).to_le_bytes());
    out.extend_from_slice(&[0; 15]);
    out.extend_from_slice(&source_ordinal.to_le_bytes());
    out.push(1);
    out.extend_from_slice(&owner_record_index.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
    lp_utf16(&mut out, expression);
    out.extend_from_slice(&[0; 5]);
    lp_utf16(&mut out, source_kind);
    if let Some(unit) = unit {
        lp_utf16(&mut out, unit);
    } else {
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    lp_utf16(&mut out, name);
    out.extend_from_slice(&evaluated_value.to_le_bytes());
    out.extend_from_slice(&[0, 1, 19, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out
}

fn class_287_parameter_record(source_kind: &str, name: &str) -> Vec<u8> {
    class_287_parameter_record_with_expression_trailer(source_kind, name, [0; 5])
}

fn class_287_parameter_record_with_expression_trailer(
    source_kind: &str,
    name: &str,
    expression_trailer: [u8; 5],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"287");
    out.extend_from_slice(&887u32.to_le_bytes());
    out.extend_from_slice(&[0; 15]);
    out.extend_from_slice(&20u32.to_le_bytes());
    out.push(1);
    out.extend_from_slice(&886u32.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
    lp_utf16(&mut out, "0.4375 in");
    out.extend_from_slice(&expression_trailer);
    lp_utf16(&mut out, source_kind);
    lp_utf16(&mut out, "in");
    lp_utf16(&mut out, name);
    out.extend_from_slice(&1.11125f64.to_le_bytes());
    out.extend_from_slice(&[0, 1, 175, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out
}

#[test]
fn class_287_parameter_accepts_the_compact_prefix_with_af_tail() {
    let parameter = parse_design_parameter_record(&class_287_parameter_record("HoleDepth", "d20"))
        .expect("class-287 parameter");
    assert_eq!(parameter.class_tag.as_str(), "287");
    assert_eq!(parameter.record_index, 887);
    assert_eq!(parameter.owner_record_index(), Some(886));
    assert_eq!(parameter.source_ordinal, 20);
    assert_eq!(parameter.expression(), "0.4375 in");
    assert_eq!(parameter.expression_offset(), 45);
    assert_eq!(parameter.source_kind(), "HoleDepth");
    assert_eq!(
        parameter.unit().map(|field| field.value.as_str()),
        Some("in")
    );
    assert_eq!(parameter.unit().map(|field| field.offset), Some(94));
    assert_eq!(parameter.name(), "d20");
    assert_eq!(parameter.evaluated_value_offset(), 108);

    let dimension =
        parse_design_parameter_record(&class_287_parameter_record("Diameter Dimension-2", "d1"))
            .expect("class-287 dimension parameter");
    assert_eq!(dimension.source_kind(), "Diameter Dimension-2");
    assert_eq!(dimension.name(), "d1");
}

#[test]
fn class_287_parameter_accepts_the_marked_expression_trailer() {
    let parameter = parse_design_parameter_record(
        &class_287_parameter_record_with_expression_trailer("OffsetX", "d63", [0, 0, 0, 1, 0]),
    )
    .expect("class-287 parameter with marked expression trailer");
    assert_eq!(parameter.source_kind(), "OffsetX");
    assert_eq!(parameter.name(), "d63");

    let malformed =
        class_287_parameter_record_with_expression_trailer("OffsetX", "d63", [0, 0, 0, 2, 0]);
    assert!(parse_design_parameter_record(&malformed).is_none());
}

#[test]
fn class_287_parameter_requires_its_marker_and_tail() {
    let mut frame = class_287_parameter_record("HoleDepth", "d20");
    frame[30] = 0;
    assert!(parse_design_parameter_record(&frame).is_none());

    let mut frame = class_287_parameter_record("HoleDepth", "d20");
    let tail = frame.len() - 12;
    frame[tail + 2] = 174;
    assert!(parse_design_parameter_record(&frame).is_none());
}

#[test]
fn compact_owned_design_parameter_has_no_family_discriminator() {
    let bytes =
        compact_owned_parameter_record(6653, 99, "82.00 mm", "Diameter", Some("mm"), "d99", 8.2);
    let parameter = parse_design_parameter_record(&bytes).expect("compact owned parameter");
    assert_eq!(parameter.record_index, 6654);
    assert_eq!(parameter.owner_record_index(), Some(6653));
    assert_eq!(parameter.source_ordinal, 99);
    assert_eq!(
        parameter
            .family_discriminator()
            .map(|value| value.value.code()),
        None
    );
    assert_eq!(
        parameter.family_discriminator().map(|value| value.offset),
        None
    );
    assert_eq!(parameter.expression(), "82.00 mm");
    assert_eq!(parameter.source_kind(), "Diameter");
    assert_eq!(
        parameter.unit().map(|field| field.value.as_str()),
        Some("mm")
    );
    assert_eq!(parameter.name(), "d99");
    assert_eq!(parameter.evaluated_value().get(), 8.2);
}

#[test]
fn legacy_owned_design_parameter_uses_the_compact_identity_prefix() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"296");
    bytes.extend_from_slice(&439_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 14]);
    bytes.extend_from_slice(&5_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&437_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    lp_utf16(&mut bytes, "0.00 mm");
    bytes.extend_from_slice(&[0; 5]);
    lp_utf16(&mut bytes, "OffsetX");
    lp_utf16(&mut bytes, "mm");
    lp_utf16(&mut bytes, "d5");
    bytes.extend_from_slice(&0.0_f64.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 18, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

    let parameter = parse_design_parameter_record(&bytes).expect("legacy owned parameter");
    assert_eq!(parameter.record_index, 439);
    assert_eq!(parameter.owner_record_index(), Some(437));
    assert_eq!(parameter.source_ordinal, 5);
    assert_eq!(parameter.source_kind(), "OffsetX");
    assert_eq!(
        parameter.unit().map(|field| field.value.as_str()),
        Some("mm")
    );
    assert_eq!(parameter.name(), "d5");
    assert_eq!(parameter.evaluated_value().get(), 0.0);
}

#[test]
fn parameter_variants_have_exact_string_and_scalar_boundaries() {
    let user = parse_design_parameter_record(&parameter_record(
        None,
        "60 mm",
        "User Parameter",
        Some("mm"),
        "Width",
        6.0,
    ))
    .unwrap();
    assert_eq!(user.kind(), DesignParameterKind::User);
    assert_eq!(user.owner_record_index(), None);
    assert_eq!(user.unit().map(|field| field.value.as_str()), Some("mm"));
    assert_eq!(user.evaluated_value().get(), 6.0);

    let feature = parse_design_parameter_record(&parameter_record(
        Some(44),
        "Width / 2",
        "AlongDistance",
        Some("mm"),
        "d12",
        3.0,
    ))
    .unwrap();
    assert_eq!(feature.kind(), DesignParameterKind::Feature);
    assert_eq!(feature.owner_record_index(), Some(44));
    assert_eq!(feature.expression(), "Width / 2");

    let boolean = parse_design_parameter_record(&parameter_record(
        None,
        "1",
        "User Parameter",
        None,
        "OnOff",
        1.0,
    ))
    .unwrap();
    assert_eq!(boolean.unit(), None);
    assert_eq!(boolean.name(), "OnOff");

    let mut tangency = parameter_record(Some(24409), "1", "TangencyWeight", Some(""), "d81", 1.0);
    tangency[22..30].copy_from_slice(&6u64.to_le_bytes());
    let tangency = parse_design_parameter_record(&tangency).expect("prefixed unitless parameter");
    assert_eq!(
        tangency
            .family_discriminator()
            .map(|value| value.value.code()),
        Some(6)
    );
    assert_eq!(tangency.unit(), None);
    assert_eq!(tangency.name(), "d81");
    assert_eq!(tangency.evaluated_value().get(), 1.0);

    let mut earlier_tangency =
        parameter_record(Some(24409), "1", "TangencyWeight", Some(""), "d81", 1.0);
    earlier_tangency[22..30].copy_from_slice(&0u64.to_le_bytes());
    assert_eq!(
        parse_design_parameter_record(&earlier_tangency)
            .expect("earlier tangency parameter")
            .family_discriminator()
            .map(|value| value.value.code()),
        Some(0)
    );

    let mut scale_factor = parameter_record(Some(1331), "1", "ScaleFactor", None, "scale", 1.0);
    let scale_factor_tail = scale_factor.len() - 12;
    scale_factor[scale_factor_tail + 2] = 16;
    let scale_factor =
        parse_design_parameter_record(&scale_factor).expect("scale-factor parameter");
    assert_eq!(
        scale_factor
            .family_discriminator()
            .map(|value| value.value.code()),
        Some(5)
    );
    assert_eq!(scale_factor.owner_record_index(), Some(1331));
    assert_eq!(scale_factor.unit(), None);
    assert_eq!(scale_factor.evaluated_value().get(), 1.0);

    for discriminator in [3u64, 4] {
        let mut earlier_distance = parameter_record(
            Some(44),
            "Width / 2",
            "AlongDistance",
            Some("mm"),
            "d12",
            3.0,
        );
        earlier_distance[22..30].copy_from_slice(&discriminator.to_le_bytes());
        assert_eq!(
            parse_design_parameter_record(&earlier_distance)
                .expect("earlier feature parameter")
                .family_discriminator()
                .map(|value| value.value.code()),
            Some(discriminator)
        );
    }

    let mut invalid_tangency = earlier_tangency;
    invalid_tangency[22..30].copy_from_slice(&5u64.to_le_bytes());
    assert!(parse_design_parameter_record(&invalid_tangency).is_none());

    let mut revised_distance = parameter_record(
        Some(44),
        "Width / 2",
        "AlongDistance",
        Some("mm"),
        "d12",
        3.0,
    );
    revised_distance[22..30].copy_from_slice(&6u64.to_le_bytes());
    let tail = revised_distance.len() - 12;
    revised_distance[tail + 2] = 16;
    assert_eq!(
        parse_design_parameter_record(&revised_distance)
            .expect("revision-six feature parameter")
            .family_discriminator()
            .map(|value| value.value.code()),
        Some(6)
    );

    let mut invalid_distance = revised_distance.clone();
    invalid_distance[22..30].copy_from_slice(&7u64.to_le_bytes());
    assert!(parse_design_parameter_record(&invalid_distance).is_none());

    revised_distance[tail + 2] = 19;
    assert!(parse_design_parameter_record(&revised_distance).is_none());

    let mut sheet_metal =
        parameter_record(Some(301), "50.00 mm", "FlangeHeight", Some("mm"), "d2", 5.0);
    sheet_metal[22..30].copy_from_slice(&6u64.to_le_bytes());
    let (_, expression_end) = crate::test_support::with_decode_context(|ctx| {
        crate::bytes::lp_utf16_bounded_charged(
            ctx,
            &sheet_metal,
            46,
            1..=256,
            "retain F3D UTF-16 string",
        )
        .unwrap()
    })
    .expect("sheet-metal expression");
    sheet_metal.insert(expression_end + 9, 0);
    let tail = sheet_metal.len() - 12;
    sheet_metal[tail + 2] = 16;
    let sheet_metal = parse_design_parameter_record(&sheet_metal)
        .expect("sheet-metal parameter with ten-byte expression trailer");
    assert_eq!(sheet_metal.source_kind(), "FlangeHeight");
    assert_eq!(sheet_metal.owner_record_index(), Some(301));
    assert_eq!(sheet_metal.evaluated_value().get(), 5.0);
}

#[test]
fn parameter_record_rejects_noncanonical_tail() {
    let mut record = parameter_record(
        Some(44),
        "45 deg",
        "TaperAngle",
        Some("deg"),
        "d13",
        std::f64::consts::FRAC_PI_4,
    );
    *record.last_mut().unwrap() = 1;
    assert!(parse_design_parameter_record(&record).is_none());
}

#[test]
fn duplicate_parameter_index_keeps_the_first_serialized_frame() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut bulk = parameter_record(Some(44), "first", "AlongDistance", Some("mm"), "d71", 1.0);
    let second = parameter_record(Some(44), "second", "AlongDistance", Some("mm"), "d71", 2.0);
    bulk.extend_from_slice(&second);

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let parameters = with_scan(&archive, |scan| {
        decode_parameters(&cadmpeg_test_support::service_decode_context(), scan)
    })
    .unwrap();
    let [parameter] = parameters.as_slice() else {
        panic!("expected one canonical parameter");
    };
    assert_eq!(parameter.record_index, 71);
    assert_eq!(parameter.byte_offset(), 0);
    assert_eq!(parameter.expression(), "first");
    assert_eq!(parameter.evaluated_value().get(), 1.0);
}

fn compact_parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 103];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"406");
    frame[7..11].copy_from_slice(&6653u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&6644u32.to_le_bytes());
    frame[35..39].copy_from_slice(&0u32.to_le_bytes());
    frame[40..48].copy_from_slice(&8.2f64.to_le_bytes());
    frame[48] = 1;
    frame[49..53].copy_from_slice(&6654u32.to_le_bytes());
    frame[59..63].copy_from_slice(&4u32.to_le_bytes());
    frame[67] = 1;
    frame[68..72].copy_from_slice(&6644u32.to_le_bytes());
    frame[80] = 1;
    frame[81..85].copy_from_slice(&6655u32.to_le_bytes());
    frame[92] = 1;
    frame[93..97].copy_from_slice(&6644u32.to_le_bytes());
    frame
}

fn compact_variant_parameter_owner_frame() -> Vec<u8> {
    let mut frame = compact_parameter_owner_frame();
    frame[4..7].copy_from_slice(b"299");
    frame[78] = 1;
    frame[79] = 0;
    frame
}

fn legacy_parameter_owner_68_frame(class_tag: &str) -> Vec<u8> {
    let mut frame = vec![0; 68];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(class_tag.as_bytes());
    frame[7..11].copy_from_slice(&100u32.to_le_bytes());
    frame[19] = 1;
    frame[33] = 1;
    frame[34..38].copy_from_slice(&101u32.to_le_bytes());
    frame[44..48].copy_from_slice(&290u32.to_le_bytes());
    frame[55] = 1;
    frame[56..60].copy_from_slice(&102u32.to_le_bytes());
    frame
}

fn legacy_parameter_owner_88_frame(class_tag: &str) -> Vec<u8> {
    let mut frame = legacy_parameter_owner_68_frame(class_tag);
    frame.resize(88, 0);
    frame[48..52].fill(0);
    frame[52] = 1;
    frame[53..57].copy_from_slice(&77u32.to_le_bytes());
    frame[65] = 1;
    frame[66..70].copy_from_slice(&102u32.to_le_bytes());
    frame[77] = 1;
    frame[78..82].copy_from_slice(&77u32.to_le_bytes());
    frame
}

fn counted_parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 101];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"316");
    frame[7..11].copy_from_slice(&44u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&12u32.to_le_bytes());
    frame[35..39].copy_from_slice(&2u32.to_le_bytes());
    frame[40] = 1;
    frame[41..45].copy_from_slice(&6u32.to_le_bytes());
    frame[45] = 1;
    frame[46..50].copy_from_slice(&45u32.to_le_bytes());
    frame[56..60].copy_from_slice(&9u32.to_le_bytes());
    frame[64] = 1;
    frame[65..69].copy_from_slice(&12u32.to_le_bytes());
    frame[75] = 1;
    frame[76] = 1;
    frame[78] = 1;
    frame[79..83].copy_from_slice(&46u32.to_le_bytes());
    frame[90] = 1;
    frame[91..95].copy_from_slice(&12u32.to_le_bytes());
    frame
}

fn compact_typed_counted_parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 100];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"320");
    frame[7..11].copy_from_slice(&44u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&12u32.to_le_bytes());
    frame[40] = 1;
    frame[41..45].copy_from_slice(&19u32.to_le_bytes());
    frame[45] = 1;
    frame[46..50].copy_from_slice(&46u32.to_le_bytes());
    frame[56..60].copy_from_slice(&9u32.to_le_bytes());
    frame[64] = 1;
    frame[65..69].copy_from_slice(&12u32.to_le_bytes());
    frame[77] = 1;
    frame[78..82].copy_from_slice(&45u32.to_le_bytes());
    frame[89] = 1;
    frame[90..94].copy_from_slice(&12u32.to_le_bytes());
    frame
}

fn compact_counted_parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 99];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"457");
    frame[7..11].copy_from_slice(&44u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&12u32.to_le_bytes());
    frame[35..39].copy_from_slice(&2u32.to_le_bytes());
    frame[40..44].copy_from_slice(&6u32.to_le_bytes());
    frame[44] = 1;
    frame[45..49].copy_from_slice(&45u32.to_le_bytes());
    frame[55..59].copy_from_slice(&9u32.to_le_bytes());
    frame[63] = 1;
    frame[64..68].copy_from_slice(&12u32.to_le_bytes());
    frame[76] = 1;
    frame[77..81].copy_from_slice(&46u32.to_le_bytes());
    frame[88] = 1;
    frame[89..93].copy_from_slice(&12u32.to_le_bytes());
    frame
}

fn tagged_scalar_parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 107];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"406");
    frame[7..11].copy_from_slice(&44u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&12u32.to_le_bytes());
    frame[35..39].copy_from_slice(&2u32.to_le_bytes());
    frame[39] = 1;
    frame[44..52].copy_from_slice(&6.0f64.to_le_bytes());
    frame[52] = 1;
    frame[53..57].copy_from_slice(&45u32.to_le_bytes());
    frame[63..67].copy_from_slice(&9u32.to_le_bytes());
    frame[71] = 1;
    frame[72..76].copy_from_slice(&12u32.to_le_bytes());
    frame[84] = 1;
    frame[85..89].copy_from_slice(&46u32.to_le_bytes());
    frame[96] = 1;
    frame[97..101].copy_from_slice(&12u32.to_le_bytes());
    frame
}

fn tagged_scalar_variant_parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 108];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"299");
    frame[7..11].copy_from_slice(&44u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&12u32.to_le_bytes());
    frame[35..39].copy_from_slice(&1u32.to_le_bytes());
    frame[39] = 1;
    frame[44..52].copy_from_slice(&0.8f64.to_le_bytes());
    frame[52] = 1;
    frame[53..57].copy_from_slice(&45u32.to_le_bytes());
    frame[63..67].copy_from_slice(&73u32.to_le_bytes());
    frame[71] = 1;
    frame[72..76].copy_from_slice(&12u32.to_le_bytes());
    frame[82] = 1;
    frame[85] = 1;
    frame[86..90].copy_from_slice(&46u32.to_le_bytes());
    frame[97] = 1;
    frame[98..102].copy_from_slice(&12u32.to_le_bytes());
    frame
}

#[test]
fn parameter_owner_frame_has_repeated_scope_and_both_record_orders() {
    let parsed = parse_parameter_owner(&parameter_owner_frame()).unwrap();
    assert_eq!(parsed.frame_length, 104);
    assert_eq!(parsed.record_index, 44);
    assert_eq!(parsed.scope_record_index, 12);
    assert_eq!(parsed.local_ordinal, 2);
    assert_eq!(parsed.evaluated_value.get(), 6.0);
    assert_eq!(parsed.parameter_record_index, 45);
    assert_eq!(parsed.owned_ordinal, 9);
    assert_eq!(parsed.variant, Some(1));
    assert_eq!(parsed.companion_record_index, 46);

    let mut parameter_first = parameter_owner_frame();
    parameter_first[49..53].copy_from_slice(&43u32.to_le_bytes());
    parameter_first[82..86].copy_from_slice(&45u32.to_le_bytes());
    let parsed = parse_parameter_owner(&parameter_first).expect("parameter-first owner frame");
    assert_eq!(parsed.parameter_record_index, 43);
    assert_eq!(parsed.record_index, 44);
    assert_eq!(parsed.companion_record_index, 45);

    let mut malformed = parameter_owner_frame();
    malformed[94..98].copy_from_slice(&13u32.to_le_bytes());
    assert!(parse_parameter_owner(&malformed).is_none());
}

#[test]
fn parameter_owner_requires_its_complete_structural_suffix() {
    let builders: [fn() -> Vec<u8>; 7] = [
        parameter_owner_frame,
        compact_parameter_owner_frame,
        counted_parameter_owner_frame,
        compact_typed_counted_parameter_owner_frame,
        compact_counted_parameter_owner_frame,
        tagged_scalar_parameter_owner_frame,
        tagged_scalar_variant_parameter_owner_frame,
    ];
    for build in builders {
        let frame = build();
        assert!(parse_parameter_owner(&frame).is_some());
        let mut longer = frame.clone();
        longer.push(0);
        assert!(parse_parameter_owner(&longer).is_none());
        assert!(parse_parameter_owner(&frame[..frame.len() - 1]).is_none());
    }

    assert_eq!(
        parse_parameter_owner(&parameter_owner_frame())
            .expect("owner frame")
            .evaluated_value_offset,
        super::FrameRelative(40)
    );
    assert_eq!(
        parse_parameter_owner(&compact_parameter_owner_frame())
            .expect("compact owner frame")
            .evaluated_value_offset,
        super::FrameRelative(40)
    );
}

#[test]
fn compact_parameter_owner_omits_the_variant_slot() {
    let parsed =
        parse_parameter_owner(&compact_parameter_owner_frame()).expect("compact parameter owner");
    assert_eq!(parsed.frame_length, 103);
    assert_eq!(parsed.record_index, 6653);
    assert_eq!(parsed.scope_record_index, 6644);
    assert_eq!(parsed.parameter_record_index, 6654);
    assert_eq!(parsed.companion_record_index, 6655);
    assert_eq!(parsed.owned_ordinal, 4);
    assert_eq!(parsed.variant, None);
    assert_eq!(parsed.evaluated_value.get(), 8.2);
}

#[test]
fn counted_parameter_owner_uses_typed_u32_scalar() {
    let parsed =
        parse_parameter_owner(&counted_parameter_owner_frame()).expect("counted parameter owner");
    assert_eq!(parsed.frame_length, 101);
    assert_eq!(parsed.evaluated_value.get(), 6.0);
    assert_eq!(parsed.evaluated_value_offset, super::FrameRelative(41));
    assert_eq!(parsed.parameter_record_index, 45);
    assert_eq!(parsed.companion_record_index, 46);
}

#[test]
fn legacy_counted_parameter_owner_uses_zero_typed_u32_scalar() {
    let mut frame = counted_parameter_owner_frame();
    frame[40] = 0;
    let parsed = parse_parameter_owner(&frame)
        .expect("legacy counted parameter owner with zero scalar marker");
    assert_eq!(parsed.frame_length, 101);
    assert_eq!(parsed.evaluated_value.get(), 6.0);
    assert_eq!(parsed.evaluated_value_offset, super::FrameRelative(41));
    assert_eq!(parsed.parameter_record_index, 45);
    assert_eq!(parsed.companion_record_index, 46);
}

#[test]
fn legacy_parameter_owner_68_uses_parameter_scalar_and_zero_scope() {
    let parsed = parse_legacy_parameter_owner_68(
        &legacy_parameter_owner_68_frame("284"),
        crate::records::identity::Located {
            value: 0.0,
            offset: 700,
        },
        0,
    )
    .expect("legacy 68-byte parameter owner")
    .into_record("Design/BulkStream.dat", 0)
    .unwrap();
    assert_eq!(parsed.frame_length(), 68);
    assert_eq!(parsed.class_tag().as_str(), "284");
    assert_eq!(parsed.record_index(), 100);
    assert_eq!(parsed.parameter_record_index(), 101);
    assert_eq!(parsed.companion_record_index(), 102);
    assert_eq!(parsed.scope_record_index(), 0);
    assert_eq!(parsed.local_ordinal(), 0);
    assert_eq!(
        serde_json::from_value::<crate::records::parameters::DesignParameterOwnerWire>(
            serde_json::to_value(&parsed).unwrap()
        )
        .unwrap()
        .owned_ordinal,
        290
    );
    assert_eq!(parsed.evaluated_value().get(), 0.0);

    for class_tag in ["268", "282", "289", "297", "299", "325", "336"] {
        assert!(parse_legacy_parameter_owner_68(
            &legacy_parameter_owner_68_frame(class_tag),
            crate::records::identity::Located {
                value: 1.25,
                offset: 700
            },
            0
        )
        .is_some());
    }
}

#[test]
fn legacy_parameter_owner_68_requires_its_admitted_class_and_shape() {
    assert!(parse_legacy_parameter_owner_68(
        &legacy_parameter_owner_68_frame("291"),
        crate::records::identity::Located {
            value: 1.0,
            offset: 700
        },
        0
    )
    .is_none());

    let mut malformed = legacy_parameter_owner_68_frame("284");
    malformed[55] = 0;
    assert!(parse_legacy_parameter_owner_68(
        &malformed,
        crate::records::identity::Located {
            value: 1.0,
            offset: 700
        },
        0
    )
    .is_none());
}

#[test]
fn legacy_parameter_owner_88_repeats_a_nonzero_scope_without_a_scalar_lane() {
    let parsed = parse_legacy_parameter_owner_88(
        &legacy_parameter_owner_88_frame("284"),
        crate::records::identity::Located {
            value: 2.5,
            offset: 700,
        },
        0,
    )
    .expect("legacy 88-byte parameter owner")
    .into_record("Design/BulkStream.dat", 0)
    .unwrap();
    assert_eq!(parsed.frame_length(), 88);
    assert_eq!(parsed.scope_record_index(), 77);
    assert_eq!(parsed.local_ordinal(), 0);
    assert_eq!(
        serde_json::from_value::<crate::records::parameters::DesignParameterOwnerWire>(
            serde_json::to_value(&parsed).unwrap()
        )
        .unwrap()
        .owned_ordinal,
        290
    );
    assert_eq!(parsed.parameter_record_index(), 101);
    assert_eq!(parsed.companion_record_index(), 102);
    assert_eq!(parsed.evaluated_value().get(), 2.5);

    for class_tag in ["282", "336", "325", "297"] {
        assert!(
            parse_legacy_parameter_owner_88(
                &legacy_parameter_owner_88_frame(class_tag),
                crate::records::identity::Located {
                    value: 2.5,
                    offset: 700
                },
                0
            )
            .is_some(),
            "class {class_tag} must use the admitted 88-byte owner grammar"
        );
    }
    assert!(parse_legacy_parameter_owner_88(
        &legacy_parameter_owner_88_frame("268"),
        crate::records::identity::Located {
            value: 2.5,
            offset: 700
        },
        0
    )
    .is_none());

    let mut mismatched = legacy_parameter_owner_88_frame("284");
    mismatched[78..82].copy_from_slice(&78u32.to_le_bytes());
    assert!(parse_legacy_parameter_owner_88(
        &mismatched,
        crate::records::identity::Located {
            value: 2.5,
            offset: 700
        },
        0
    )
    .is_none());
}

#[test]
fn compact_typed_counted_parameter_owner_omits_variant_slot() {
    let parsed = parse_parameter_owner(&compact_typed_counted_parameter_owner_frame())
        .expect("compact typed counted parameter owner");
    assert_eq!(parsed.frame_length, 100);
    assert_eq!(parsed.record_index, 44);
    assert_eq!(parsed.scope_record_index, 12);
    assert_eq!(parsed.local_ordinal, 0);
    assert_eq!(parsed.evaluated_value.get(), 19.0);
    assert_eq!(parsed.evaluated_value_offset, super::FrameRelative(41));
    assert_eq!(parsed.parameter_record_index, 46);
    assert_eq!(parsed.owned_ordinal, 9);
    assert_eq!(parsed.variant, None);
    assert_eq!(parsed.companion_record_index, 45);
}

#[test]
fn compact_counted_parameter_owner_omits_type_and_variant_markers() {
    let mut frame = compact_counted_parameter_owner_frame();
    frame[45..49].copy_from_slice(&46u32.to_le_bytes());
    frame[77..81].copy_from_slice(&45u32.to_le_bytes());
    let parsed = parse_parameter_owner(&frame).expect("compact counted parameter owner");
    assert_eq!(parsed.frame_length, 99);
    assert_eq!(parsed.evaluated_value.get(), 6.0);
    assert_eq!(parsed.evaluated_value_offset, super::FrameRelative(40));
    assert_eq!(parsed.parameter_record_index, 46);
    assert_eq!(parsed.variant, None);
    assert_eq!(parsed.companion_record_index, 45);
}

#[test]
fn tagged_scalar_parameter_owner_carries_a_scalar_type_prefix() {
    let parsed = parse_parameter_owner(&tagged_scalar_parameter_owner_frame())
        .expect("tagged scalar parameter owner");
    assert_eq!(parsed.frame_length, 107);
    assert_eq!(parsed.evaluated_value.get(), 6.0);
    assert_eq!(parsed.evaluated_value_offset, super::FrameRelative(44));
    assert_eq!(parsed.parameter_record_index, 45);
    assert_eq!(parsed.variant, None);
    assert_eq!(parsed.companion_record_index, 46);
}

#[test]
fn tagged_scalar_parameter_owner_can_carry_a_variant_slot() {
    let parsed = parse_parameter_owner(&tagged_scalar_variant_parameter_owner_frame())
        .expect("tagged scalar variant parameter owner");
    assert_eq!(parsed.frame_length, 108);
    assert_eq!(parsed.evaluated_value.get(), 0.8);
    assert_eq!(parsed.evaluated_value_offset, super::FrameRelative(44));
    assert_eq!(parsed.parameter_record_index, 45);
    assert_eq!(parsed.owned_ordinal, 73);
    assert_eq!(parsed.variant, Some(0));
    assert_eq!(parsed.companion_record_index, 46);
}

#[test]
fn compact_scalar_parameter_owner_can_carry_a_two_byte_variant_slot() {
    let parsed = parse_parameter_owner(&compact_variant_parameter_owner_frame())
        .expect("compact scalar variant parameter owner");
    assert_eq!(parsed.frame_length, 103);
    assert_eq!(parsed.class_tag.as_str(), "299");
    assert_eq!(parsed.variant, Some(0));
    assert_eq!(parsed.parameter_record_index, 6654);
    assert_eq!(parsed.companion_record_index, 6655);
}

#[test]
fn parameter_companion_prefix_has_owner_backlink_and_timestamp() {
    let mut prefix = vec![0; 58];
    prefix[0..4].copy_from_slice(&3u32.to_le_bytes());
    prefix[4..7].copy_from_slice(b"408");
    prefix[7..11].copy_from_slice(&46u32.to_le_bytes());
    prefix[31] = 1;
    prefix[32..36].copy_from_slice(&44u32.to_le_bytes());
    prefix[42..50].copy_from_slice(&1_678_000_000_000_000u64.to_le_bytes());

    let parsed = parse_parameter_companion(&prefix).unwrap();
    assert_eq!(parsed.record_index, 46);
    assert_eq!(parsed.owner_record_index, 44);
    assert_eq!(parsed.timestamp_micros.get(), 1_678_000_000_000_000);
    assert_eq!(
        parsed.timestamp_micros_offset,
        crate::design::decode::parameters::FrameRelative(42)
    );

    prefix[32..36].copy_from_slice(&45u32.to_le_bytes());
    assert_eq!(
        parse_parameter_companion(&prefix)
            .unwrap()
            .owner_record_index,
        45
    );
    prefix[42..50].fill(0);
    assert!(parse_parameter_companion(&prefix).is_none());
}

#[test]
fn parameter_companion_decode_refuses_index_output_and_identifier_limits() {
    const STREAM: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut prefix = vec![0; 58];
    prefix[0..4].copy_from_slice(&3u32.to_le_bytes());
    prefix[4..7].copy_from_slice(b"408");
    prefix[7..11].copy_from_slice(&46u32.to_le_bytes());
    prefix[31] = 1;
    prefix[32..36].copy_from_slice(&44u32.to_le_bytes());
    prefix[42..50].copy_from_slice(&1u64.to_le_bytes());
    let owner =
        DesignParameterOwner::try_from(crate::records::parameters::DesignParameterOwnerWire {
            id: format!(
                "{}:design-parameter-owner#1",
                crate::ids::native_scope(STREAM)
            ),
            byte_offset: 1,
            frame_length: 104,
            class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned())
                .unwrap(),
            record_index: 44,
            scope_record_index: 10,
            local_ordinal: 0,
            evaluated_value: 2.0,
            evaluated_value_offset: 41,
            parameter_record_index: 45,
            owned_ordinal: 0,
            variant: Some(0),
            companion_record_index: 46,
        })
        .unwrap();
    let header = crate::records::decal::DesignRecordHeader {
        id: crate::ids::native_design_record_header_id(STREAM, 0),
        record_index: 46,
        class_tag: crate::records::references::DesignClassTag::try_from("408".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(STREAM, stored).unwrap();
    zip.write_all(&prefix).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    with_scan(&archive, |scan| {
        let scope_len = u64_from_index(crate::ids::native_scope(STREAM).len());
        for (items, retained, dimension, operation) in [
            (
                0,
                u64::MAX,
                ResourceDimension::CollectionItems,
                "f3d parameter companion headers",
            ),
            (
                1,
                u64::MAX,
                ResourceDimension::CollectionItems,
                "f3d parameter companions",
            ),
            (
                u64::MAX,
                0,
                ResourceDimension::RetainedBytes,
                "f3d native stream key",
            ),
            (
                u64::MAX,
                scope_len,
                ResourceDimension::RetainedBytes,
                "f3d parameter companion identifier",
            ),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = items;
            policy.limits.max_retained_bytes = retained;
            let refusal_cap = match cadmpeg_test_support::refusal::resource_limit_at(
                dimension,
                operation,
                |cap| {
                    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                    match dimension {
                        cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                            policy.limits.max_retained_bytes = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                            policy.limits.max_collection_items = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                            policy.limits.max_materialized_bytes = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                            policy.limits.max_work_units = cap;
                        }
                        cadmpeg_core::decode::ResourceDimension::RecursionDepth => {
                            policy.limits.max_recursion_depth = cap;
                        }
                        dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                    }
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                    (super::decode_parameter_companions(
                        &ctx,
                        scan,
                        &[owner.clone()],
                        &[header.clone()],
                    ))
                    .map(|_| ())
                },
            ) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
            policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
            match dimension {
                cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                    policy.limits.max_retained_bytes = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                    policy.limits.max_collection_items = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    policy.limits.max_materialized_bytes = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    policy.limits.max_work_units = refusal_cap;
                }
                cadmpeg_core::decode::ResourceDimension::RecursionDepth => {
                    policy.limits.max_recursion_depth = refusal_cap;
                }
                dimension => panic!("unsupported refusal dimension: {dimension:?}"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result =
                super::decode_parameter_companions(&ctx, scan, &[owner.clone()], &[header.clone()]);
            assert!(
                matches!(
                    &result,
                    Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                        if failure.dimension == dimension && failure.operation == operation
                ),
                "item limit {items}, retained limit {retained}: {result:?}"
            );
        }
        let decoded = super::decode_parameter_companions(
            &cadmpeg_test_support::service_decode_context(),
            scan,
            &[owner.clone()],
            &[header.clone()],
        )
        .unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(
            decoded[0].id(),
            format!(
                "{}:design-parameter-companion#0",
                crate::ids::native_scope(STREAM)
            )
        );
    });
}

#[test]
fn parameter_owner_uses_the_paired_same_index_header_as_its_boundary() {
    fn paired_header() -> [u8; 11] {
        let mut header = [0; 11];
        header[0..4].copy_from_slice(&3u32.to_le_bytes());
        header[4..7].copy_from_slice(b"293");
        header[7..11].copy_from_slice(&44u32.to_le_bytes());
        header
    }
    fn archive(stream: &str, bulk: &[u8]) -> Vec<u8> {
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file(stream, stored).unwrap();
        zip.write_all(bulk).unwrap();
        zip.finish().unwrap().into_inner()
    }

    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let parameter = crate::records::parameters::DesignParameter::try_from(
        crate::records::parameters::DesignParameterDraft {
            id: crate::ids::native_design_parameter_id(stream, 200),
            byte_offset: 200,
            class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned())
                .unwrap(),
            record_index: 45,
            source_ordinal: 0,
            source: crate::records::parameters::DesignParameterSource::new(
                "Distance".into(),
                Some(44),
                Some(crate::records::identity::Located {
                    value: crate::records::parameters::DesignParameterDiscriminator::Code0,
                    offset: 222,
                }),
            )
            .unwrap(),
            expression: "6 cm".into(),
            expression_offset: 240,
            source_kind_offset: 260,

            unit: Some(crate::records::identity::RecordedValue {
                value: "cm".into(),
                offset: 280,
            }),
            name: "distance".into(),
            name_offset: 300,
            evaluated_value: 6.0,
            evaluated_value_offset: 320,
        },
    )
    .unwrap();
    let header = crate::records::decal::DesignRecordHeader {
        id: crate::ids::native_design_record_header_id(stream, 0),
        record_index: 44,
        class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned()).unwrap(),
        byte_offset: 0,
    };

    let mut exact = parameter_owner_frame();
    exact.extend_from_slice(&paired_header());
    let owners = with_scan(&archive(stream, &exact), |scan| {
        crate::design::test_support::with_test_decode_context(|ctx| {
            crate::design::decode::parameters::decode_parameter_owners(
                ctx,
                scan,
                std::slice::from_ref(&parameter),
                std::slice::from_ref(&header),
            )
        })
    })
    .expect("exact owner frame");
    let [owner] = owners.as_slice() else {
        panic!("expected one parameter owner");
    };
    assert_eq!(owner.frame_length(), 104);
    assert_eq!(owner.evaluated_value_offset(), 40);

    let unresolved = with_scan(&archive(stream, &[]), |scan| {
        crate::design::test_support::with_test_decode_context(|ctx| {
            crate::design::decode::parameters::decode_parameter_owners(
                ctx,
                scan,
                std::slice::from_ref(&parameter),
                &[],
            )
        })
    })
    .expect("missing owner frame is retained as an unresolved binding");
    assert!(unresolved.is_empty());

    let mut extended = parameter_owner_frame();
    extended.push(0);
    extended.extend_from_slice(&paired_header());
    let error = with_scan(&archive(stream, &extended), |scan| {
        crate::design::test_support::with_test_decode_context(|ctx| {
            crate::design::decode::parameters::decode_parameter_owners(
                ctx,
                scan,
                std::slice::from_ref(&parameter),
                std::slice::from_ref(&header),
            )
        })
    })
    .expect_err("an owner-shaped prefix must not shorten the exact frame");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn parameter_owner_maps_and_output_refuse_collection_limit() {
    fn paired_header() -> [u8; 11] {
        let mut header = [0; 11];
        header[0..4].copy_from_slice(&3u32.to_le_bytes());
        header[4..7].copy_from_slice(b"293");
        header[7..11].copy_from_slice(&44u32.to_le_bytes());
        header
    }
    fn archive(stream: &str, bulk: &[u8]) -> Vec<u8> {
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file(stream, stored).unwrap();
        zip.write_all(bulk).unwrap();
        zip.finish().unwrap().into_inner()
    }

    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let parameter = crate::records::parameters::DesignParameter::try_from(
        crate::records::parameters::DesignParameterDraft {
            id: crate::ids::native_design_parameter_id(stream, 200),
            byte_offset: 200,
            class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned())
                .unwrap(),
            record_index: 45,
            source_ordinal: 0,
            source: crate::records::parameters::DesignParameterSource::new(
                "Distance".into(),
                Some(44),
                Some(crate::records::identity::Located {
                    value: crate::records::parameters::DesignParameterDiscriminator::Code0,
                    offset: 222,
                }),
            )
            .unwrap(),
            expression: "6 cm".into(),
            expression_offset: 240,
            source_kind_offset: 260,

            unit: Some(crate::records::identity::RecordedValue {
                value: "cm".into(),
                offset: 280,
            }),
            name: "distance".into(),
            name_offset: 300,
            evaluated_value: 6.0,
            evaluated_value_offset: 320,
        },
    )
    .unwrap();
    let header = crate::records::decal::DesignRecordHeader {
        id: crate::ids::native_design_record_header_id(stream, 0),
        record_index: 44,
        class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned()).unwrap(),
        byte_offset: 0,
    };

    let mut exact = parameter_owner_frame();
    exact.extend_from_slice(&paired_header());
    let bytes = archive(stream, &exact);
    for limit in [0, 1, 2, 6] {
        let result = with_scan(&bytes, |scan| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::default();
            policy.limits.max_collection_items = limit;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            crate::design::decode::parameters::decode_parameter_owners(
                &ctx,
                scan,
                std::slice::from_ref(&parameter),
                std::slice::from_ref(&header),
            )
        });
        assert!(
            matches!(
                result,
                Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                    if refusal.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            ),
            "collection limit {limit}"
        );
    }
}

#[test]
fn parameter_companion_orders_recipes_by_payload_byte_offset() {
    let stream = "f3d:Design/BulkStream.dat";
    let parameter = crate::records::parameters::DesignParameter::try_from(
        crate::records::parameters::DesignParameterDraft {
            id: format!("{stream}:design-parameter#20"),
            byte_offset: 1,
            class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned())
                .unwrap(),
            record_index: 20,
            source_ordinal: 0,
            source: crate::records::parameters::DesignParameterSource::new(
                "Linear Dimension-1".into(),
                Some(21),
                None,
            )
            .unwrap(),
            expression: "distance".into(),
            expression_offset: 40,
            source_kind_offset: 60,

            unit: Some(crate::records::identity::RecordedValue {
                value: "mm".into(),
                offset: 70,
            }),
            name: "d1".into(),
            name_offset: 80,
            evaluated_value: 2.0,
            evaluated_value_offset: 90,
        },
    )
    .unwrap();
    let owner =
        DesignParameterOwner::try_from(crate::records::parameters::DesignParameterOwnerWire {
            id: format!("{stream}:design-parameter-owner#21"),
            byte_offset: 0,
            frame_length: 104,
            class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned())
                .unwrap(),
            record_index: 21,
            scope_record_index: 10,
            local_ordinal: 0,
            evaluated_value: 2.0,
            evaluated_value_offset: 40,
            parameter_record_index: 20,
            owned_ordinal: 0,
            variant: Some(0),
            companion_record_index: 22,
        })
        .unwrap();
    let companion = DesignParameterCompanion::unbound(
        format!("{stream}:design-parameter-companion#22"),
        10,
        crate::records::references::DesignClassTag::try_from("408".to_owned()).unwrap(),
        22,
        21,
        std::num::NonZeroU64::new(1).unwrap(),
        50,
    );
    let recipe = |record_index, byte_offset| ConstructionRecipe {
        id: format!("{stream}:construction-recipe#{record_index}"),
        byte_offset,
        kind: ConstructionRecipeKind::Edge,
        design: None,
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: record_index,
            offset: 0,
        }),
    };
    let recipes = [recipe(31, 100), recipe(30, 80)];

    let bound = super::bind_parameter_companion_payloads(
        &cadmpeg_test_support::service_decode_context(),
        vec![companion],
        &super::ParameterCompanionInputs {
            parameters: std::slice::from_ref(&parameter),
            owners: std::slice::from_ref(&owner),
            scopes: &[],
            entities: &[],
            headers: &[],
            recipes: &recipes,
            stream_lengths: &std::collections::HashMap::from([(stream.to_owned(), 200)]),
        },
    )
    .unwrap();

    let payload = bound[0].payload().expect("bound payload");
    assert_eq!(payload.byte_offset(), 68);
    assert_eq!(payload.byte_length(), 132);
    assert_eq!(
        payload.owned_recipe_ids(),
        [
            format!("{stream}:construction-recipe#30"),
            format!("{stream}:construction-recipe#31"),
        ]
    );
}

#[test]
fn parameter_companion_binding_refuses_output_recipe_and_id_limits() {
    let companion = DesignParameterCompanion::unbound(
        "f3d:native:design-parameter-companion#0".into(),
        0,
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap(),
        1,
        2,
        std::num::NonZeroU64::new(1).unwrap(),
        42,
    );
    let recipe = ConstructionRecipe {
        id: "f3d:native:construction-recipe#60".into(),
        byte_offset: 60,
        kind: ConstructionRecipeKind::Edge,
        design: None,
        recipe_index: 0,
        record_index: None,
    };
    let lengths = std::collections::HashMap::from([("f3d:native".to_owned(), 100)]);
    let inputs = super::ParameterCompanionInputs {
        parameters: &[],
        owners: &[],
        scopes: &[],
        entities: &[],
        headers: &[],
        recipes: std::slice::from_ref(&recipe),
        stream_lengths: &lengths,
    };
    for (items, retained, dimension, operation) in [
        (
            0,
            u64::MAX,
            ResourceDimension::CollectionItems,
            "f3d bound parameter companions",
        ),
        (
            1,
            u64::MAX,
            ResourceDimension::CollectionItems,
            "f3d companion owned recipes",
        ),
        (
            2,
            u64::MAX,
            ResourceDimension::CollectionItems,
            "f3d companion owned recipe identifiers",
        ),
        (
            u64::MAX,
            0,
            ResourceDimension::RetainedBytes,
            "f3d companion owned recipe identifier",
        ),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = items;
        policy.limits.max_retained_bytes = retained;
        let refusal_cap =
            match cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                match dimension {
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                        policy.limits.max_retained_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                        policy.limits.max_collection_items = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                        policy.limits.max_work_units = cap;
                    }
                    cadmpeg_core::decode::ResourceDimension::RecursionDepth => {
                        policy.limits.max_recursion_depth = cap;
                    }
                    dimension => panic!("unsupported refusal dimension: {dimension:?}"),
                }
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                (super::bind_parameter_companion_payloads(&ctx, vec![companion.clone()], &inputs))
                    .map(|_| ())
            }) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
        policy.limits = cadmpeg_core::decode::DecodePolicy::service().limits;
        match dimension {
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::CollectionItems => {
                policy.limits.max_collection_items = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = refusal_cap;
            }
            cadmpeg_core::decode::ResourceDimension::RecursionDepth => {
                policy.limits.max_recursion_depth = refusal_cap;
            }
            dimension => panic!("unsupported refusal dimension: {dimension:?}"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result =
            super::bind_parameter_companion_payloads(&ctx, vec![companion.clone()], &inputs);
        assert!(
            matches!(
                &result,
                Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                    if failure.dimension == dimension && failure.operation == operation
            ),
            "item limit {items}, retained limit {retained}: {result:?}"
        );
    }
    let bound = super::bind_parameter_companion_payloads(
        &cadmpeg_test_support::service_decode_context(),
        vec![companion],
        &inputs,
    )
    .unwrap();
    assert_eq!(bound[0].payload().unwrap().owned_recipe_ids(), [recipe.id]);
}

#[test]
fn parameter_source_rejects_missing_or_unexpected_owner() {
    let unowned_feature = parameter_record(None, "1", "Distance", None, "d1", 1.0);
    assert!(parse_design_parameter_record(&unowned_feature).is_none());
    let owned_user = parameter_record(Some(2), "1", "User Parameter", None, "d1", 1.0);
    assert!(parse_design_parameter_record(&owned_user).is_none());
    let compact_user = compact_owned_parameter_record(2, 0, "1", "User Parameter", None, "d1", 1.0);
    assert!(parse_design_parameter_record(&compact_user).is_none());
    let legacy_user = class_287_parameter_record("User Parameter", "d1");
    assert!(parse_design_parameter_record(&legacy_user).is_none());
}

#[test]
fn parameter_owner_value_offset_is_localized_once() {
    let parsed = parse_parameter_owner(&parameter_owner_frame()).unwrap();
    assert_eq!(parsed.evaluated_value_offset, super::FrameRelative(40));
    let owner = parsed.into_record("Design/BulkStream.dat", 1000).unwrap();
    assert_eq!(owner.byte_offset(), 1000);
    assert_eq!(owner.evaluated_value_offset(), 1040);
    assert!(parse_parameter_owner(&parameter_owner_frame())
        .unwrap()
        .into_record("Design/BulkStream.dat", u64::MAX)
        .is_none());
    for owner in [
        parse_legacy_parameter_owner_68(
            &legacy_parameter_owner_68_frame("284"),
            crate::records::identity::Located {
                value: 2.5,
                offset: 700,
            },
            0,
        )
        .unwrap(),
        parse_legacy_parameter_owner_88(
            &legacy_parameter_owner_88_frame("284"),
            crate::records::identity::Located {
                value: 2.5,
                offset: 700,
            },
            0,
        )
        .unwrap(),
    ] {
        let owner = owner.into_record("Design/BulkStream.dat", 0).unwrap();
        assert_eq!(owner.evaluated_value_offset(), 700);
    }
}

#[test]
fn legacy_parameter_owner_preserves_external_scalar_offsets() {
    for frame_start in [400, 1000] {
        let evaluated = crate::records::identity::Located {
            value: 2.5,
            offset: 700,
        };
        for parsed in [
            parse_legacy_parameter_owner_68(
                &legacy_parameter_owner_68_frame("284"),
                evaluated,
                frame_start,
            )
            .unwrap(),
            parse_legacy_parameter_owner_88(
                &legacy_parameter_owner_88_frame("284"),
                evaluated,
                frame_start,
            )
            .unwrap(),
        ] {
            assert_eq!(
                parsed.evaluated_value_offset,
                super::FrameRelative(i128::from(700) - i128::from(frame_start))
            );
            let owner = parsed
                .into_record("Design/BulkStream.dat", frame_start)
                .unwrap();
            assert_eq!(owner.byte_offset(), frame_start);
            assert_eq!(owner.evaluated_value_offset(), 700);
            assert_eq!(owner.evaluated_value().get(), 2.5);
            assert_eq!(
                owner.id(),
                &crate::ids::native_design_parameter_owner_id("Design/BulkStream.dat", frame_start)
            );
        }
    }
}

#[test]
fn frame_relative_offsets_refuse_to_saturate_at_the_end_of_the_address_space() {
    use crate::design::decode::parameters::FrameRelative;

    assert_eq!(FrameRelative(1).absolute(u64::MAX), None);

    let payload = parameter_record(None, "1", "User Parameter", None, "p", 1.0);
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    assert!(super::parse_design_parameter(
        &cadmpeg_test_support::service_decode_context(),
        &payload
    )
    .unwrap()
    .expect("parsed parameter")
    .into_record(stream, u64::MAX)
    .is_none());

    let parsed =
        super::parse_design_parameter(&cadmpeg_test_support::service_decode_context(), &payload)
            .unwrap()
            .expect("parsed parameter");
    let error = super::locate_design_parameter(parsed, stream, usize::MAX)
        .expect_err("a frame at the end of the address space cannot be located");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
}

#[test]
fn design_parameter_text_fields_refuse_each_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let payload = parameter_record(Some(44), "1", "AlongDistance", Some("mm"), "d71", 1.0);
    let mut charged = 0usize;
    for field in ["1", "AlongDistance", "mm", "d71"] {
        charged += field.len();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from(charged - 1).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::parse_design_parameter(&ctx, &payload).err().unwrap();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "f3d Design UTF-16 text")
        );
    }
    let parsed =
        super::parse_design_parameter(&cadmpeg_test_support::service_decode_context(), &payload)
            .unwrap()
            .unwrap();
    assert_eq!(parsed.name, "d71");

    let legacy = class_287_parameter_record("HoleDepth", "d20");
    let mut charged = 0usize;
    for field in ["0.4375 in", "HoleDepth", "in", "d20"] {
        charged += field.len();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::try_from(charged - 1).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::parse_design_parameter(&ctx, &legacy).err().unwrap();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "f3d Design UTF-16 text")
        );
    }
}

#[test]
fn decoded_parameter_records_refuse_each_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&parameter_record(
        Some(44),
        "1",
        "AlongDistance",
        Some("mm"),
        "d71",
        1.0,
    ))
    .unwrap();
    let archive = zip.finish().unwrap().into_inner();
    with_scan(&archive, |scan| {
        for (limit, operation) in [
            (0, "f3d parameter record index"),
            (1, "f3d decoded parameter records"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let error = decode_parameters(&ctx, scan).err().unwrap();
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation)
            );
        }
    });
}
