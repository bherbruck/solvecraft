// SPDX-License-Identifier: Apache-2.0

use crate::test_support::assembly_test::{
    f3d_with_text_brep_stream, f3d_without_brep, f3z_archive, XREF_ROLE,
};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::smbh_header_test::synthetic_smbh;
use crate::test_support::zip_test::{f3d_with_smbh, f3d_with_smbh_and_manifest_version};
use crate::F3dCodec;
use crate::F3dLossCode;
use cadmpeg_ir::codec::Codec;
use cadmpeg_ir::codec::DecodeOptions;
use std::io::Cursor;

/// The outer archive's row survives the inner member's decode.
///
/// [`crate::decode::decode`] runs on the root `.f3d` member and classifies that
/// member. The file the codec was handed is the `.f3z`, so both the report and
/// `SourceMeta` must name the F3Z row, at inspect and at decode.
#[test]
fn an_f3z_archive_reports_the_multi_document_row_at_inspect_and_decode() {
    let component = f3d_with_smbh(&synthetic_geometry_smbh());
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("comp.f3d", component.as_slice()),
        ],
    );

    let summary = F3dCodec
        .inspect(
            &mut Cursor::new(archive.clone()),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    let inspected = summary
        .dialects()
        .expect("inspect must report a primary F3D layer")
        .primary()
        .clone();
    let inspected_dialects = summary.dialects();
    assert_eq!(inspected.format(), "f3d");
    assert_eq!(inspected.dialect().as_str(), "f3d:f3z-multi-document");
    assert_eq!(
        inspected.declared()["root_document_members"],
        "comp.f3d,root.f3d",
        "each root-level member is recorded as the archive spells it, sorted by path"
    );
    assert_eq!(
        inspected.admission(),
        &cadmpeg_core::dialect::Admission::Admitted
    );

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        decoded
            .report()
            .dialects()
            .expect("decode reports F3D layers")
            .primary(),
        inspected_dialects
            .as_ref()
            .expect("inspection reports F3D layers")
            .primary()
    );
    let extra_keys = |layers: &cadmpeg_core::dialect::DialectLayers| {
        layers
            .iter()
            .skip(1)
            .map(|matched| {
                (
                    matched.format().to_owned(),
                    matched.instance().map(str::to_owned),
                )
            })
            .collect::<std::collections::BTreeSet<_>>()
    };
    let inspected_extra_keys = extra_keys(inspected_dialects.as_ref().unwrap());
    let decoded_extra_keys = extra_keys(
        decoded
            .report()
            .dialects()
            .expect("decode reports F3D layers"),
    );
    assert_eq!(decoded_extra_keys, inspected_extra_keys);
    assert!(decoded_extra_keys.contains(&("f3d".to_owned(), Some("root.f3d".to_owned()))));
    assert!(decoded_extra_keys.contains(&("f3d".to_owned(), Some("comp.f3d".to_owned()))));
    assert!(decoded
        .report()
        .dialects()
        .expect("the report is classified")
        .iter()
        .skip(1)
        .all(|matched| matched
            .declared()
            .contains_key(crate::dialect::DECLARED_ARCHIVE_MEMBER)));
    assert!(decoded
        .report()
        .dialects()
        .expect("the report is classified")
        .iter()
        .skip(1)
        .any(|matched| matched.format() == "f3d" && matched.instance() == Some("root.f3d")));
    let source = decoded.ir().source.as_ref().unwrap();
    assert_eq!(source.dialect(), Some(&inspected));
    let report_layers = decoded.report().dialects().unwrap();
    assert_eq!(source.dialects(), Some(report_layers));
    let primary = report_layers.primary();
    assert_eq!(source.dialect(), Some(primary));
}

fn unverified_acis_text_member() -> Vec<u8> {
    b"23200 0 1 0 \n\
      16 Autodesk Neutron 21 ASM 232.4.0.65535 OSX 9 Synthetic \n\
      1 9.999999999999999547e-07 1.000000000000000036e-10 \n\
      body $-1 -1 $-1 $-1 $-1 $-1 #\n\
      End-of-ACIS-data \n"
        .to_vec()
}

#[test]
fn f3z_decode_retains_the_root_kernel_row_and_loss() {
    let stream = unverified_acis_text_member();
    let root = f3d_with_text_brep_stream(
        &["FusionAssetName[Active]/Breps.BlobParts/Body1.sat"],
        &stream,
    );
    let archive = f3z_archive("root.f3d", &[("root.f3d", root.as_slice())]);

    let inspected = F3dCodec
        .inspect(
            &mut Cursor::new(archive.clone()),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    assert!(inspected
        .notes
        .iter()
        .any(|note| note.contains("f3z archive: 1 document member(s); model root root.f3d")));
    assert!(inspected
        .notes
        .iter()
        .all(|note| !note.contains("0 ASM BREP stream")));

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    let inspected_extras = inspected
        .dialects()
        .unwrap()
        .iter()
        .skip(1)
        .collect::<Vec<_>>();
    let decoded_extras = decoded
        .report()
        .dialects()
        .unwrap()
        .iter()
        .skip(1)
        .collect::<Vec<_>>();
    assert_eq!(decoded_extras, inspected_extras);
    assert!(decoded
        .report()
        .dialects()
        .into_iter()
        .flat_map(cadmpeg_core::dialect::DialectLayers::iter)
        .any(|matched| {
            matched.format() == crate::dialect::FORMAT
                && matched.dialect().as_str() == "f3d:f3z-multi-document"
        }));
    assert!(
        decoded
            .report()
            .dialects()
            .into_iter()
            .flat_map(cadmpeg_core::dialect::DialectLayers::iter)
            .any(|matched| {
                matched.format() == "acis" && matched.dialect().as_str() == "acis:text-acis"
            }),
        "{:?}",
        decoded.report().dialects()
    );
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::KernelDialectUnverified.kind()));
}

#[test]
fn f3z_decode_retains_member_identity_and_unverified_loss() {
    let root = f3d_with_smbh_and_manifest_version(&synthetic_smbh(), "9-9-9-9");
    let member = F3dCodec
        .decode(&mut Cursor::new(root.clone()), &DecodeOptions::default())
        .unwrap();
    assert!(member
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::SourceDialectUnverified.kind()));
    let archive = f3z_archive("root.f3d", &[("root.f3d", root.as_slice())]);

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::SourceDialectUnverified.kind()));
    let member = decoded
        .report()
        .dialects()
        .expect("the F3Z report is classified")
        .iter()
        .find(|matched| matched.format() == crate::dialect::FORMAT && matched.instance().is_some())
        .expect("the member F3D identity is an extra layer");
    assert_eq!(member.instance(), Some("root.f3d"));
    assert_eq!(member.dialect().as_str(), "f3d:unknown");
}

#[test]
fn f3z_kernel_loss_is_derived_from_its_archive_member_layer() {
    let stream = unverified_acis_text_member();
    let component = f3d_with_text_brep_stream(
        &["FusionAssetName[Active]/Breps.BlobParts/Body1.sat"],
        &stream,
    );
    let bare = F3dCodec
        .decode(
            &mut Cursor::new(component.clone()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let bare_loss = bare
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::KernelDialectUnverified.kind())
        .expect("the bare member charges its kernel loss")
        .clone();
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("comp.f3d", component.as_slice()),
        ],
    );

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    let kernel_layers = decoded
        .report()
        .dialects()
        .expect("the report is classified")
        .iter()
        .filter(|matched| matched.format() == cadmpeg_asm::dialect::FORMAT)
        .collect::<Vec<_>>();
    assert_eq!(kernel_layers.len(), 1);
    assert_eq!(kernel_layers[0].instance(), Some("comp.f3d"));
    assert_eq!(
        kernel_layers[0].declared()[crate::dialect::DECLARED_ARCHIVE_MEMBER],
        "comp.f3d"
    );
    assert_eq!(
        kernel_layers[0].declared()["carrier"],
        "FusionAssetName[Active]/Breps.BlobParts/Body1.sat"
    );
    let loss = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::KernelDialectUnverified.kind())
        .expect("component kernel loss travels with its row");
    assert_eq!(loss.code, bare_loss.code);
    assert_eq!(loss.severity, bare_loss.severity);
    assert_eq!(
        loss.message,
        format!("archive member comp.f3d: {}", bare_loss.message)
    );
}

#[test]
fn an_unreferenced_unverified_member_still_charges_its_dialect_loss_once() {
    let root = f3d_with_smbh(&synthetic_smbh());
    let unreferenced = f3d_with_smbh_and_manifest_version(&synthetic_smbh(), "9-9-9-9");
    let unparseable_carrier = f3d_with_text_brep_stream(
        &["FusionAssetName[Active]/Breps.BlobParts/Body1.sat"],
        b"not an SAT stream",
    );
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("unreferenced.f3d", unreferenced.as_slice()),
            ("unparseable.f3d", unparseable_carrier.as_slice()),
        ],
    );

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .unwrap();
    let member = decoded
        .report()
        .dialects()
        .expect("the F3Z report is classified")
        .iter()
        .find(|matched| {
            matched.format() == crate::dialect::FORMAT
                && matched.instance() == Some("unreferenced.f3d")
        })
        .expect("the unreferenced member remains in the final layer set");
    assert_eq!(member.dialect().as_str(), "f3d:unknown");
    assert!(matches!(
        member.admission(),
        cadmpeg_core::dialect::Admission::Unverified { .. }
    ));
    let refused_kernel = decoded
        .report()
        .dialects()
        .expect("the F3Z report is classified")
        .iter()
        .find(|matched| {
            matched.format() == cadmpeg_asm::dialect::FORMAT
                && matched.instance() == Some("unparseable.f3d")
        })
        .expect("the unparseable member remains a refused kernel layer");
    assert_eq!(
        refused_kernel.admission(),
        &cadmpeg_core::dialect::Admission::Refused
    );

    let losses = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == F3dLossCode::SourceDialectUnverified.kind())
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("archive member unreferenced.f3d"));

    let carrier_losses = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == F3dLossCode::KernelCarrierUnparseable.kind())
        .collect::<Vec<_>>();
    assert_eq!(carrier_losses.len(), 1);
    assert!(carrier_losses[0]
        .message
        .contains("archive member unparseable.f3d"));
}

#[test]
fn an_unreadable_unreferenced_member_is_retained_without_refusing_the_root() {
    let root = f3d_with_smbh(&synthetic_smbh());
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("broken.f3d", b"not an F3D archive"),
        ],
    );

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("an unreadable unreferenced member must not erase the decoded root");
    let loss = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::XrefMemberUndecoded.kind())
        .expect("the unreadable retained member is reported");
    assert!(loss.message.starts_with("xref broken.f3d:"));
    assert!(loss.message.contains("source bytes remain retained"));
}

#[test]
fn merged_member_losses_keep_their_xref_context() {
    let component = f3d_without_brep("component-design", "comp.f3d", &[]);
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", root.as_slice()),
            ("comp.f3d", component.as_slice()),
        ],
    );

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("the bodyless member still merges its retained metadata");
    let loss = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::MissingGeometryStream.kind())
        .expect("the merged bodyless member reports its missing geometry carrier");
    assert!(loss.message.starts_with("xref "), "{loss:?}");
}

#[test]
fn duplicate_member_layer_identity_is_a_recorded_loss() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test input");
    let mut target = cadmpeg_core::dialect::DialectLayers::of(
        crate::dialect::F3dDialect::classify_f3z(&ctx, &["part.f3d"]).unwrap(),
    );
    let first = cadmpeg_core::dialect::DialectLayers::of(
        crate::dialect::F3dDialect::classify_document(&ctx, Some("3-2-0-0")).unwrap(),
    );
    let later = cadmpeg_core::dialect::DialectLayers::of(
        crate::dialect::F3dDialect::classify_document(&ctx, Some("3-3-0-0")).unwrap(),
    );

    assert!(
        crate::f3z::archive::merge_member_layers(&ctx, &mut target, &first, "part.f3d")
            .unwrap()
            .is_empty()
    );
    let losses =
        crate::f3z::archive::merge_member_layers(&ctx, &mut target, &later, "part.f3d").unwrap();

    assert_eq!(target.iter().count(), 2);
    assert!(target.iter().any(|layer| {
        layer.instance() == Some("part.f3d") && layer.dialect().as_str() == "f3d:manifest-3-2-0-0"
    }));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code, F3dLossCode::DialectLayerCollision.kind());
    assert!(losses[0].message.contains("f3d"));
    assert!(losses[0].message.contains("part.f3d"));
}

fn member_layer_collection_refusal(max_items: u64, collision: bool) -> cadmpeg_core::CodecError {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let normal_policy = cadmpeg_core::decode::DecodePolicy::default();
    let (normal, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &normal_policy).unwrap();
    let mut target = cadmpeg_core::dialect::DialectLayers::of(
        crate::dialect::F3dDialect::classify_f3z(&normal, &["part.f3d"]).unwrap(),
    );
    let member = cadmpeg_core::dialect::DialectLayers::of(
        crate::dialect::F3dDialect::classify_document(&normal, Some("3-2-0-0")).unwrap(),
    );
    if collision {
        crate::f3z::archive::merge_member_layers(&normal, &mut target, &member, "part.f3d")
            .unwrap();
    }
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let (limited, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    crate::f3z::archive::merge_member_layers(&limited, &mut target, &member, "part.f3d")
        .unwrap_err()
}

#[test]
fn f3z_member_layer_clone_refuses_collection_limit() {
    let error = member_layer_collection_refusal(0, false);
    assert!(
        matches!(&error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy dialect layers"),
        "{error:?}"
    );
}

#[test]
fn f3z_archive_member_declaration_refuses_collection_limit() {
    let error = member_layer_collection_refusal(1, false);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "declare F3Z archive member")
    );
}

#[test]
fn f3z_member_layer_collection_refuses_limit() {
    let error = member_layer_collection_refusal(2, false);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3Z member dialect layers")
    );
}

#[test]
fn f3z_layer_collision_loss_refuses_collection_limit() {
    let error = member_layer_collection_refusal(2, true);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3Z report losses")
    );
}

#[test]
fn f3z_layer_instance_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (normal, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .unwrap();
    let mut target = cadmpeg_core::dialect::DialectLayers::of(
        crate::dialect::F3dDialect::classify_f3z(&normal, &["part.f3d"]).unwrap(),
    );
    let member =
        cadmpeg_core::dialect::DialectLayers::of(cadmpeg_core::dialect::DialectMatch::admitted(
            cadmpeg_core::dialect_id!("f3d:manifest-3-2-0-0"),
        ));
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (limited, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        crate::f3z::archive::merge_member_layers(&limited, &mut target, &member, "part.f3d")
            .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3Z dialect layer instance")
    );
}
