// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::scopes::parameter_scope::parse_parameter_scope;
use crate::design::decode::scopes::thread::ThreadPrefix;
use crate::design::decode::scopes::thread::{exact_thread_construction, parse_thread_payload};
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::thread::{DesignThreadConstruction, DesignThreadForm};
use crate::test_support::lp_utf16;

#[test]
fn thread_payload_refuses_each_text_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let texts = ["M30x3.5", "30.0", "ISO Metric profile"];
    let mut bytes = Vec::new();
    for text in texts {
        lp_utf16(&mut bytes, text);
    }
    let mut charged = 0usize;
    for text in texts {
        charged += text.len();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = u64::try_from(charged - 1).unwrap();

        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = parse_thread_payload(&ctx, &bytes, 0, ThreadPrefix::Standard, Vec::new());
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                if failure.dimension == ResourceDimension::RetainedBytes
                    && failure.operation == "f3d Design UTF-16 text"
        ));
    }
}

#[test]
fn thread_face_group_limit_refuses_before_payload() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut bytes = vec![0; 38];
    bytes[21..29].copy_from_slice(&60.0f64.to_le_bytes());
    bytes[29..34].copy_from_slice(&[1, 2, 0, 0, 0]);
    bytes[34..38].copy_from_slice(&[0x36, 0, 0x67, 0]);
    let mut scope = DesignParameterScope::empty(
        "f3d:scope#thread-limit",
        crate::records::feature::scope::DesignFeatureKind::Thread,
        987,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 200;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![988, 989]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = exact_thread_construction(&ctx, &bytes, &scope);
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d Thread face groups"
    ));
}

#[test]
fn thread_compact_face_group_limit_refuses_second_item() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut bytes = vec![0; 38];
    bytes[21..29].copy_from_slice(&60.0f64.to_le_bytes());
    bytes[29..34].copy_from_slice(&[0, 2, 0, 0, 0]);
    bytes[34..38].copy_from_slice(&[0x36, 0, 0x48, 0]);
    let mut scope = DesignParameterScope::empty(
        "f3d:scope#compact-thread-limit",
        crate::records::feature::scope::DesignFeatureKind::Thread,
        987,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 200;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![988, 989, 992, 993]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 1;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = exact_thread_construction(&ctx, &bytes, &scope);
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::CollectionItems
                && failure.operation == "f3d Thread face groups"
    ));
}

#[test]
fn thread_scope_decodes_standard_size_and_face_group() {
    let mut bytes = vec![0; 148];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"335");
    bytes[7..11].copy_from_slice(&987u32.to_le_bytes());
    bytes[21..29].copy_from_slice(&60.0f64.to_le_bytes());
    bytes[29..34].copy_from_slice(&[1, 2, 0, 0, 0]);
    bytes[34..38].copy_from_slice(&[0x36, 0, 0x67, 0]);
    let mut payload = Vec::new();
    lp_utf16(&mut payload, "M30x3.5");
    lp_utf16(&mut payload, "30.0");
    lp_utf16(&mut payload, "ISO Metric profile");
    assert_eq!(payload.len(), 70);
    bytes[38..108].copy_from_slice(&payload);
    bytes[108..113].copy_from_slice(&[0, 1, 0, 0, 0]);
    bytes[113..121].copy_from_slice(&2.97345f64.to_le_bytes());
    bytes[121..129].copy_from_slice(&2.5732f64.to_le_bytes());
    bytes[129] = 1;
    bytes[130..138].copy_from_slice(&0.35f64.to_le_bytes());
    bytes[138..146].copy_from_slice(&2.7568f64.to_le_bytes());
    bytes[146..148].copy_from_slice(&[0, 1]);

    let expected = DesignThreadConstruction {
        form: DesignThreadForm::Standard,
        designation_offset: 38,
        designation: cadmpeg_core::text::NonBlankString::new("M30x3.5").unwrap(),
        nominal_size: crate::records::feature::thread::DesignThreadNominalSize::try_from(
            "30.0".to_owned(),
        )
        .expect("nominal size"),
        profile: cadmpeg_core::text::NonBlankString::new("ISO Metric profile").unwrap(),
        pitch: cadmpeg_ir::scalar::PositiveReal::new(0.35).unwrap(),
        face_group_record_indices: vec![988],
        diameters: crate::records::feature::thread::DesignThreadDiameters::new(
            2.97345, 2.5732, 2.7568,
        )
        .unwrap(),
    };
    assert_thread_construction(
        parse_thread_payload(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            38,
            ThreadPrefix::Standard,
            vec![988],
        )
        .unwrap(),
        &expected,
    );
    let mut invalid_standard_pitch_marker = bytes.clone();
    invalid_standard_pitch_marker[129] = 0;
    assert_eq!(
        parse_thread_payload(
            &cadmpeg_test_support::service_decode_context(),
            &invalid_standard_pitch_marker,
            38,
            ThreadPrefix::Standard,
            vec![988],
        )
        .unwrap(),
        None
    );

    let mut scope = DesignParameterScope::empty(
        "f3d:scope#standard-thread",
        crate::records::feature::scope::DesignFeatureKind::Thread,
        987,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("901".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("902".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 200;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![988, 989]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_thread_construction(
        exact_thread_construction(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &scope,
        )
        .unwrap(),
        &expected,
    );

    let mut owner_marked = bytes;
    owner_marked.splice(20..20, [1, 0, 0, 0]);
    let shifted_expected = parse_thread_payload(
        &cadmpeg_test_support::service_decode_context(),
        &owner_marked,
        42,
        ThreadPrefix::Standard,
        vec![988],
    )
    .unwrap()
    .expect("owner-marked standard Thread payload");
    assert_eq!(shifted_expected.designation_offset, 42);
    scope
        .try_edit(|draft| {
            draft.frame_length += 4;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_thread_construction(
        exact_thread_construction(
            &cadmpeg_test_support::service_decode_context(),
            &owner_marked,
            &scope,
        )
        .unwrap(),
        &shifted_expected,
    );
    let mut invalid_owner_marker = owner_marked.clone();
    invalid_owner_marker[20..24].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        exact_thread_construction(
            &cadmpeg_test_support::service_decode_context(),
            &invalid_owner_marker,
            &scope
        )
        .unwrap(),
        None
    );
    assert_eq!(
        parse_thread_payload(
            &cadmpeg_test_support::service_decode_context(),
            &owner_marked,
            42,
            ThreadPrefix::Compact,
            vec![988]
        )
        .unwrap(),
        None
    );
}

#[test]
fn thread_scope_decodes_class_334_legacy_standard_tail() {
    let mut bytes = vec![0; 148];
    bytes[21..29].copy_from_slice(&60.0f64.to_le_bytes());
    bytes[29..34].copy_from_slice(&[1, 2, 0, 0, 0]);
    bytes[34..38].copy_from_slice(&[0x36, 0, 0x67, 0]);
    let mut payload = Vec::new();
    lp_utf16(&mut payload, "M7x1");
    lp_utf16(&mut payload, "7.0");
    lp_utf16(&mut payload, "ISO Metric profile");
    assert_eq!(payload.len(), 62);
    bytes[38..100].copy_from_slice(&payload);
    bytes[100..105].copy_from_slice(&[1, 1, 0, 0, 0]);
    bytes[105..113].copy_from_slice(&0.71472f64.to_le_bytes());
    bytes[113..121].copy_from_slice(&0.60355f64.to_le_bytes());
    bytes[121] = 0;
    bytes[122..130].copy_from_slice(&0.1f64.to_le_bytes());
    bytes[130..138].copy_from_slice(&0.64255f64.to_le_bytes());
    bytes[138..142].copy_from_slice(&[0, 0, 0, 1]);

    let expected = DesignThreadConstruction {
        form: DesignThreadForm::StandardLegacy,
        designation_offset: 38,
        designation: cadmpeg_core::text::NonBlankString::new("M7x1").unwrap(),
        nominal_size: crate::records::feature::thread::DesignThreadNominalSize::try_from(
            "7.0".to_owned(),
        )
        .expect("nominal size"),
        profile: cadmpeg_core::text::NonBlankString::new("ISO Metric profile").unwrap(),
        pitch: cadmpeg_ir::scalar::PositiveReal::new(0.1).unwrap(),
        face_group_record_indices: vec![988],
        diameters: crate::records::feature::thread::DesignThreadDiameters::new(
            0.71472, 0.60355, 0.64255,
        )
        .unwrap(),
    };
    assert_thread_construction(
        parse_thread_payload(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            38,
            ThreadPrefix::Standard,
            vec![988],
        )
        .unwrap(),
        &expected,
    );

    let mut scope = DesignParameterScope::empty(
        "f3d:scope#legacy-thread",
        crate::records::feature::scope::DesignFeatureKind::Thread,
        987,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("334".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![988, 991]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_thread_construction(
        exact_thread_construction(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &scope,
        )
        .unwrap(),
        &expected,
    );

    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("335".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    assert_eq!(
        exact_thread_construction(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &scope
        )
        .unwrap(),
        None
    );
}

fn assert_thread_construction(
    actual: Option<DesignThreadConstruction>,
    expected: &DesignThreadConstruction,
) {
    let actual = actual.expect("typed Thread construction");
    assert_eq!(actual.form, expected.form);
    assert_eq!(actual.designation_offset, expected.designation_offset);
    assert_eq!(actual.designation, expected.designation);
    assert_eq!(actual.nominal_size.text(), expected.nominal_size.text());
    assert_eq!(actual.profile, expected.profile);
    assert_eq!(
        actual.face_group_record_indices,
        expected.face_group_record_indices
    );
    for (actual, expected) in [
        (
            actual.nominal_size.value().expect("actual nominal size"),
            expected
                .nominal_size
                .value()
                .expect("expected nominal size"),
        ),
        (actual.diameters.major(), expected.diameters.major()),
        (actual.diameters.minor(), expected.diameters.minor()),
        (actual.pitch.get(), expected.pitch.get()),
        (actual.diameters.pitch(), expected.diameters.pitch()),
    ] {
        assert!((actual - expected).abs() < 1.0e-12);
    }
}

#[test]
fn thread_scope_decodes_compact_preamble_and_localized_profile() {
    let mut bytes = vec![0; 160];
    let mut payload = Vec::new();
    lp_utf16(&mut payload, "M3.5x0.6");
    lp_utf16(&mut payload, "3.5");
    lp_utf16(&mut payload, "GB Metric profile");
    bytes[21..29].copy_from_slice(&60.0f64.to_le_bytes());
    bytes[29..34].copy_from_slice(&[0, 2, 0, 0, 0]);
    bytes[34..38].copy_from_slice(&[0x36, 0, 0x48, 0]);
    bytes[38..38 + payload.len()].copy_from_slice(&payload);
    let after_profile = 38 + payload.len();
    assert_eq!(after_profile, 106);
    bytes[after_profile..after_profile + 5].copy_from_slice(&[1, 2, 0, 0, 0]);
    bytes[after_profile + 5..after_profile + 13].copy_from_slice(&0.35995f64.to_le_bytes());
    bytes[after_profile + 13..after_profile + 21].copy_from_slice(&0.293f64.to_le_bytes());
    bytes[after_profile + 21] = 0;
    bytes[after_profile + 22..after_profile + 30].copy_from_slice(&0.06f64.to_le_bytes());
    bytes[after_profile + 30..after_profile + 38].copy_from_slice(&0.3166f64.to_le_bytes());
    bytes[after_profile + 38..after_profile + 42].copy_from_slice(&[0, 0, 0, 1]);

    let expected = DesignThreadConstruction {
        form: DesignThreadForm::Compact(None),
        designation_offset: 38,
        designation: cadmpeg_core::text::NonBlankString::new("M3.5x0.6").unwrap(),
        nominal_size: crate::records::feature::thread::DesignThreadNominalSize::try_from(
            "3.5".to_owned(),
        )
        .expect("nominal size"),
        profile: cadmpeg_core::text::NonBlankString::new("GB Metric profile").unwrap(),
        pitch: cadmpeg_ir::scalar::PositiveReal::new(0.06).unwrap(),
        face_group_record_indices: vec![988],
        diameters: crate::records::feature::thread::DesignThreadDiameters::new(
            0.35995, 0.293, 0.3166,
        )
        .unwrap(),
    };
    assert_thread_construction(
        parse_thread_payload(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            38,
            ThreadPrefix::Compact,
            vec![988],
        )
        .unwrap(),
        &expected,
    );
    let mut referenced = bytes.clone();
    referenced[after_profile + 38] = 1;
    referenced[after_profile + 39..after_profile + 43].copy_from_slice(&2075u32.to_le_bytes());
    referenced[after_profile + 43..after_profile + 49].fill(0);
    let mut referenced_expected = expected.clone();
    referenced_expected.form = DesignThreadForm::Compact(Some(crate::records::identity::Located {
        value: std::num::NonZeroU32::new(2075).expect("reference"),
        offset: u64_from_index(after_profile + 39),
    }));
    assert_thread_construction(
        parse_thread_payload(
            &cadmpeg_test_support::service_decode_context(),
            &referenced,
            38,
            ThreadPrefix::Compact,
            vec![988],
        )
        .unwrap(),
        &referenced_expected,
    );

    let mut scope = DesignParameterScope::empty(
        "f3d:scope#compact-thread",
        crate::records::feature::scope::DesignFeatureKind::Thread,
        987,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("903".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 200;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![988, 989, 992, 993]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("904".to_owned()).unwrap();
    let mut plural_expected = expected.clone();
    plural_expected.face_group_record_indices.push(992);
    assert_thread_construction(
        exact_thread_construction(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &scope,
        )
        .unwrap(),
        &plural_expected,
    );

    let mut owner_marked = bytes;
    owner_marked.splice(20..20, [1, 0, 0, 0]);
    plural_expected.designation_offset += 4;
    scope
        .try_edit(|draft| {
            draft.frame_length += 4;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_thread_construction(
        exact_thread_construction(
            &cadmpeg_test_support::service_decode_context(),
            &owner_marked,
            &scope,
        )
        .unwrap(),
        &plural_expected,
    );
    let mut invalid_owner_separator = owner_marked.clone();
    invalid_owner_separator[24] = 1;
    assert_eq!(
        exact_thread_construction(
            &cadmpeg_test_support::service_decode_context(),
            &invalid_owner_separator,
            &scope
        )
        .unwrap(),
        None
    );

    scope
        .try_edit(|draft| {
            draft.reference_members = {
                let mut values: Vec<u32> = draft.reference_members.values().copied().collect();
                values.push(994);
                crate::records::identity::ReferenceRun::unlocated(values)
            };
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_thread_construction(
            &cadmpeg_test_support::service_decode_context(),
            &owner_marked,
            &scope
        )
        .unwrap(),
        None
    );
}

#[test]
fn thread_scope_decodes_class_414_legacy_compact_tail() {
    let mut bytes = vec![0; 160];
    let mut payload = Vec::new();
    lp_utf16(&mut payload, "M190x8");
    lp_utf16(&mut payload, "190.0");
    lp_utf16(&mut payload, "ISO Metric profile");
    bytes[21..29].copy_from_slice(&60.0f64.to_le_bytes());
    bytes[29..34].copy_from_slice(&[0, 2, 0, 0, 0]);
    bytes[34..38].copy_from_slice(&[0x36, 0, 0x48, 0]);
    bytes[38..38 + payload.len()].copy_from_slice(&payload);
    let after_profile = 38 + payload.len();
    bytes[after_profile..after_profile + 5].copy_from_slice(&[1, 1, 0, 0, 0]);
    bytes[after_profile + 5..after_profile + 13].copy_from_slice(&19.08149f64.to_le_bytes());
    bytes[after_profile + 13..after_profile + 21].copy_from_slice(&18.18397f64.to_le_bytes());
    bytes[after_profile + 21] = 0;
    bytes[after_profile + 22..after_profile + 30].copy_from_slice(&0.8f64.to_le_bytes());
    bytes[after_profile + 30..after_profile + 38].copy_from_slice(&18.50413f64.to_le_bytes());
    bytes[after_profile + 38..after_profile + 42].copy_from_slice(&[0, 0, 0, 1]);

    let expected = DesignThreadConstruction {
        form: DesignThreadForm::CompactLegacy,
        designation_offset: 38,
        designation: cadmpeg_core::text::NonBlankString::new("M190x8").unwrap(),
        nominal_size: crate::records::feature::thread::DesignThreadNominalSize::try_from(
            "190.0".to_owned(),
        )
        .expect("nominal size"),
        profile: cadmpeg_core::text::NonBlankString::new("ISO Metric profile").unwrap(),
        pitch: cadmpeg_ir::scalar::PositiveReal::new(0.8).unwrap(),
        face_group_record_indices: vec![988],
        diameters: crate::records::feature::thread::DesignThreadDiameters::new(
            19.08149, 18.18397, 18.50413,
        )
        .unwrap(),
    };
    assert_thread_construction(
        parse_thread_payload(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            38,
            ThreadPrefix::Compact,
            vec![988],
        )
        .unwrap(),
        &expected,
    );

    let mut scope = DesignParameterScope::empty(
        "f3d:scope#legacy-compact-thread",
        crate::records::feature::scope::DesignFeatureKind::Thread,
        987,
    );
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("414".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("263".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 200;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![988, 989]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_thread_construction(
        exact_thread_construction(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &scope,
        )
        .unwrap(),
        &expected,
    );

    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("334".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("262".to_owned()).unwrap();
    assert_eq!(
        exact_thread_construction(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &scope
        )
        .unwrap(),
        None
    );
}

#[test]
fn localized_sketch_scope_retains_its_generic_reference_table() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"301");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for record_index in [55u32, 56] {
        bytes.push(1);
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Esquisse");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned()).unwrap(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(
        &cadmpeg_test_support::service_decode_context(),
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .unwrap()
    .expect("localized Sketch scope");
    assert_eq!(
        scope.kind(),
        crate::records::feature::scope::DesignFeatureKind::Esquisse
    );
    assert_eq!(
        scope
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>(),
        [55, 56]
    );
    assert!(scope.sketch_entity().is_none());
}
