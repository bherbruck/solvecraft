// SPDX-License-Identifier: Apache-2.0
//! Decode and inspection reports assembled from F3D identity and transfer facts.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_ir::codec::DecodeBody;
use cadmpeg_ir::document::SourceMeta;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::ContainerSummary;

use crate::container::ContainerScan;

/// Identity owner of a decoded F3D member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReportScope {
    /// The member is the decoded document and owns its classified layers.
    Standalone,
    /// The containing F3Z archive owns identity and classifies every member.
    ArchiveMember(DialectLayers),
}

/// Build a decode body from route-owned losses. Identity is authored once, on
/// the document, by [`classify_document`].
pub(crate) fn build_decode_report(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    transfer: cadmpeg_ir::report::decode::DecodeTransfer,
    mut losses: Vec<LossNote>,
) -> Result<DecodeBody, cadmpeg_core::CodecError> {
    header_losses(ctx, scan, &mut losses)?;
    Ok(DecodeBody {
        transfer,
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses,
        notes: crate::container::summary_notes(
            ctx,
            scan,
            if transfer.container_only() {
                crate::container::SummaryScope::ContainerOnly
            } else {
                crate::container::SummaryScope::FullDecode
            },
        )?,
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    })
}

/// Constructs the document's source metadata once from route-owned identity.
///
/// A standalone document classifies its own layers and charges classification
/// and dialect losses ahead of route losses. An archive member uses the outer
/// archive layers already classified by the F3Z session.
pub(crate) fn classify_document(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    scope: ReportScope,
    attributes: BTreeMap<String, String>,
    body: &mut DecodeBody,
) -> Result<SourceMeta, cadmpeg_core::CodecError> {
    let dialects = match scope {
        ReportScope::Standalone => {
            let (dialects, mut losses) = crate::dialect::classify_layers(ctx, scan)?;
            ctx.append_vec(
                &mut losses,
                &mut { crate::dialect::dialect_losses(ctx, &dialects)? },
                "append F3D dialect losses",
            )?;
            ctx.append_vec(
                &mut losses,
                &mut { std::mem::take(&mut body.losses) },
                "prepend F3D dialect losses",
            )?;
            body.losses = losses;
            dialects
        }
        ReportScope::ArchiveMember(dialects) => dialects,
    };
    Ok(SourceMeta::classified(
        dialects,
        cadmpeg_core::text::named_entries_for_decode(ctx, "the f3d document", attributes)?,
    ))
}

/// Build a single-document inspection summary with the same dialect facts that
/// decode projects into losses.
pub(crate) fn build_inspection_summary(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
) -> Result<ContainerSummary, cadmpeg_core::CodecError> {
    let (layers, mut losses) = crate::dialect::classify_layers(ctx, scan)?;
    ctx.append_vec(
        &mut losses,
        &mut { crate::dialect::dialect_losses(ctx, &layers)? },
        "append F3D inspection dialect losses",
    )?;
    let mut summary = crate::container::summarize(ctx, scan, layers)?;
    header_losses(ctx, scan, &mut losses)?;
    summary.losses = losses;
    Ok(summary)
}

/// Preserve per-entry recovery diagnostics on all inspection and decode routes.
pub(crate) fn header_losses(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<(), cadmpeg_core::CodecError> {
    for diagnostic in &scan.entry_diagnostics {
        ctx.push_vec(
            losses,
            crate::loss::F3dLossCode::ArchiveEntryUnreadable
                .note(ctx.copy_retained_text(diagnostic, "F3D optional entry loss")?),
            "F3D optional entry losses",
        )?;
    }
    for diagnostic in &scan.manifest_diagnostics {
        let message = ctx.format_retained(
            format_args!(
                "{} is unreadable; manifest selectors retained",
                diagnostic.field
            ),
            "F3D manifest metadata loss text",
        )?;
        ctx.push_vec(
            losses,
            crate::loss::F3dLossCode::ManifestMetadataUnresolved
                .note(message)
                .with_provenance(cadmpeg_ir::SourceProvenance::in_stream(
                    "f3d",
                    cadmpeg_ir::stream_name!("Manifest.dat"),
                    cadmpeg_core::decode::u64_from_index(diagnostic.offset),
                )),
            "F3D manifest metadata losses",
        )?;
    }
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(scan.breps.len()),
        "F3D header tolerance losses",
    )?;
    for brep in &scan.breps {
        let Some(header) = brep
            .kernel
            .as_ref()
            .and_then(crate::container::KernelFraming::model_metadata)
        else {
            continue;
        };
        kernel_tolerance_losses(ctx, &brep.name, header.linear, header.angular, losses)?;
        for field in header.unreadable_product_fields() {
            let message = ctx.format_retained(
                format_args!(
                    "BREP {} header {field} is unreadable; independent records retained",
                    brep.name
                ),
                "F3D kernel metadata loss text",
            )?;
            ctx.push_vec(
                losses,
                crate::loss::F3dLossCode::KernelHeaderMetadataUnresolved.note(message),
                "F3D kernel metadata losses",
            )?;
        }
    }
    for name in crate::container::text_brep_names(scan) {
        let Some(crate::container::TextBrepFraming::Parsed(stream)) = scan.text_breps.get(name)
        else {
            continue;
        };
        kernel_tolerance_losses(
            ctx,
            name,
            stream
                .header
                .resabs_cm()
                .map(cadmpeg_ir::scalar::NonNegativeReal::get),
            stream
                .header
                .resnor()
                .map(cadmpeg_ir::scalar::NonNegativeReal::get),
            losses,
        )?;
        for diagnostic in stream.header.diagnostics.iter().chain(&stream.framing) {
            let message = ctx.format_retained(
                format_args!(
                    "text BREP {name} byte {}: {}; independent records retained",
                    diagnostic.error.offset, diagnostic.error.reason
                ),
                "F3D text header loss text",
            )?;
            let stream_name = cadmpeg_ir::StreamName::try_from(
                ctx.copy_retained_text(name, "F3D text header source stream")?,
            )
            .map_err(cadmpeg_core::CodecError::malformed)?;
            ctx.push_vec(
                losses,
                crate::loss::F3dLossCode::TextHeaderNoncanonical
                    .note(message)
                    .with_provenance(cadmpeg_ir::SourceProvenance::in_stream(
                        "f3d",
                        stream_name,
                        cadmpeg_core::decode::u64_from_index(diagnostic.error.offset),
                    )),
                "F3D text header losses",
            )?;
        }
    }
    Ok(())
}

/// Interpret only positive finite tolerances in the canonical document units.
fn kernel_tolerance_losses(
    ctx: &DecodeContext<'_>,
    name: &str,
    linear_cm: Option<f64>,
    angular: Option<f64>,
    losses: &mut Vec<LossNote>,
) -> Result<(), cadmpeg_core::CodecError> {
    for (field, value) in [
        ("resabs", linear_cm.map(|value| value * 10.0)),
        ("resnor", angular),
    ] {
        if value.is_some_and(|value| !value.is_finite() || value <= 0.0) {
            let message = ctx.format_retained(format_args!("BREP {name} header {field} cannot supply a positive finite document tolerance; value left uninterpreted"), "F3D kernel tolerance loss text")?;
            let stream = cadmpeg_ir::StreamName::try_from(
                ctx.copy_retained_text(name, "F3D kernel tolerance source stream")?,
            )
            .map_err(cadmpeg_core::CodecError::malformed)?;
            ctx.push_vec(
                losses,
                crate::loss::F3dLossCode::KernelHeaderToleranceUnresolved
                    .note(message)
                    .with_provenance(cadmpeg_ir::SourceProvenance::in_stream("f3d", stream, 0)),
                "F3D kernel tolerance losses",
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::u64_from_index;

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    use super::{build_decode_report, classify_document, ReportScope};
    use crate::test_support::zip_test::synthetic_f3d;
    use std::collections::BTreeMap;

    fn assert_loss_append_refuses(operation: &'static str) {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let incoming =
            vec![crate::loss::F3dLossCode::DialectLayerCollision.note("duplicate layer")];
        let error = ctx
            .append_vec(&mut Vec::new(), &mut { incoming }, operation)
            .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == operation)
        );
    }

    #[test]
    fn report_dialect_loss_append_refuses_collection_limit() {
        assert_loss_append_refuses("append F3D dialect losses");
    }

    #[test]
    fn report_route_loss_reorder_refuses_collection_limit() {
        assert_loss_append_refuses("prepend F3D dialect losses");
    }

    #[test]
    fn inspection_dialect_loss_append_refuses_collection_limit() {
        assert_loss_append_refuses("append F3D inspection dialect losses");
    }

    #[test]
    fn decode_report_includes_a_kernel_identity_collision_loss() {
        let bytes = synthetic_f3d(true);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let mut scan = crate::container::scan(&ctx, root).unwrap();
        scan.breps.push(scan.breps[0].clone());

        let mut report = build_decode_report(
            &ctx,
            &scan,
            cadmpeg_ir::report::decode::DecodeTransfer::full(true),
            Vec::new(),
        )
        .unwrap();
        let source = classify_document(
            &ctx,
            &scan,
            ReportScope::Standalone,
            BTreeMap::new(),
            &mut report,
        )
        .unwrap();
        assert!(source.dialects().is_some());
        assert!(report
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::DialectLayerCollision.kind()));
    }

    #[test]
    fn summary_notes_refuse_collection_limit() {
        let bytes = synthetic_f3d(true);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let scan = crate::container::scan(&ctx, root).unwrap();
        let mut limited_policy = DecodePolicy::service();
        limited_policy.limits.max_collection_items = 0;
        let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &limited_policy).unwrap();
        let error = crate::container::summary_notes(
            &limited,
            &scan,
            crate::container::SummaryScope::FullDecode,
        )
        .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "collect F3D summary notes")
        );
    }

    #[test]
    fn summary_entry_copy_refuses_collection_limit() {
        let bytes = synthetic_f3d(true);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let scan = crate::container::scan(&ctx, root).unwrap();
        let mut limited_policy = DecodePolicy::service();
        limited_policy.limits.max_collection_items = 0;
        let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &limited_policy).unwrap();
        let error = crate::container::summarize(
            &limited,
            &scan,
            cadmpeg_core::dialect::DialectLayers::of(scan.kind.dialect().clone()),
        )
        .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "copy F3D summary entries")
        );
    }

    #[test]
    fn summary_attributes_refuse_collection_limit() {
        let bytes = synthetic_f3d(true);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let scan = crate::container::scan(&ctx, root).unwrap();
        let mut limited_policy = DecodePolicy::service();
        limited_policy.limits.max_collection_items = u64_from_index(scan.entries.len());
        let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &limited_policy).unwrap();
        let error = crate::container::summarize(
            &limited,
            &scan,
            cadmpeg_core::dialect::DialectLayers::of(scan.kind.dialect().clone()),
        )
        .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "copy F3D summary attributes")
        );
    }

    #[test]
    fn summary_note_text_refuses_retained_limit() {
        let bytes = synthetic_f3d(true);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let scan = crate::container::scan(&ctx, root).unwrap();
        let mut limited_policy = DecodePolicy::service();
        limited_policy.limits.max_retained_bytes =
            match cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                "retain F3D summary note",
                |cap| {
                    let mut limited_policy = DecodePolicy::service();
                    limited_policy.limits.max_retained_bytes = cap;
                    let (limited, _) =
                        DecodeContext::from_root_bytes(&[], &arena, &limited_policy).unwrap();
                    crate::container::summary_notes(
                        &limited,
                        &scan,
                        crate::container::SummaryScope::FullDecode,
                    )
                    .map(|_| ())
                },
            ) {
                cadmpeg_core::CodecError::ResourceLimit(limit) => limit.limit,
                error => panic!("unexpected refusal: {error:?}"),
            };
        let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &limited_policy).unwrap();
        let error = crate::container::summary_notes(
            &limited,
            &scan,
            crate::container::SummaryScope::FullDecode,
        )
        .unwrap_err();
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain F3D summary note")
        );
    }
    #[test]
    fn source_attribute_conversion_refuses_destination_map_growth() {
        let bytes = synthetic_f3d(false);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (normal, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let scan = crate::container::scan(&normal, root).unwrap();
        let mut report = build_decode_report(
            &normal,
            &scan,
            cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {},
            Vec::new(),
        )
        .unwrap();
        let mut limited_policy = policy;
        limited_policy.limits.max_collection_items = 0;
        crate::test_support::with_decode_policy(&limited_policy, |ctx| {
            let error = classify_document(
                ctx,
                &scan,
                ReportScope::ArchiveMember(cadmpeg_core::dialect::DialectLayers::of(
                    scan.kind.dialect().clone(),
                )),
                BTreeMap::from([("name".to_owned(), "value".to_owned())]),
                &mut report,
            )
            .unwrap_err();
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                panic!("attribute conversion must refuse destination growth");
            };
            assert_eq!(limit.operation, "named entry map nodes");
            assert_eq!(Some(limit), ctx.resource_refusal());
        });
    }
}
