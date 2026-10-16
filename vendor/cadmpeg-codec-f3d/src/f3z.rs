// SPDX-License-Identifier: Apache-2.0
//! Decode a multi-document `.f3z` archive
//! ([spec §1.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#15-multi-document-archives-f3z)).
//!
//! A `.f3z` holds one manifest-selected root and one member per document.
//! [`decode`] classifies every member, decodes the model root, and delegates
//! occurrence-scoped graph composition to [`merge`].

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::ContainerSummary;

use crate::container::ContainerScan;
use crate::decode::AuthoredDecoded;
use crate::loss::F3dLossCode;
use cadmpeg_ir::report::loss::LossNote;

mod archive;
mod merge;

fn push_loss(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    code: F3dLossCode,
    args: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "collect F3Z report losses";

    ctx.reserve_vec(losses, 1, OPERATION)?;
    losses.push(code.note(ctx.format_retained(args, "retain F3Z report loss")?));
    Ok(())
}

/// Inspects every document member under the F3Z archive identity.
pub(crate) fn inspect<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
) -> Result<ContainerSummary, CodecError> {
    let (model_root, _) = archive::model_root(ctx, scan)?;
    scan.entry_view(&model_root).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "f3z root member {model_root} is not present in the archive"
        ))
    })?;
    let classified = archive::classify_members(ctx, scan)?;
    let member_count = scan
        .entries
        .iter()
        .filter(|entry| crate::container::is_f3d_name(&entry.name))
        .count();
    let mut notes = Vec::new();
    ctx.push_formatted_retained(
        &mut notes,
        format_args!("f3z archive: {member_count} document member(s); model root {model_root}"),
        "collect F3Z report notes",
        "retain F3Z report note",
    )?;
    Ok(ContainerSummary::classified(
        classified.layers,
        cadmpeg_ir::ContainerKind::Zip,
        crate::container::copy_summary_entries(ctx, &scan.entries)?,
        classified.losses,
        notes,
    ))
}

/// Decodes a scanned `.f3z` archive into one occurrence-scoped document.
pub(crate) fn decode<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
) -> Result<Decoded, CodecError> {
    let (model_root, omitted_drawing_root) = archive::model_root(ctx, scan)?;
    let outer = archive::classify_members(ctx, scan)?;
    let root_scan = outer.member_scan(&model_root)?;
    let AuthoredDecoded {
        mut ir,
        source,
        body: mut report,
        source_fidelity: mut fidelity,
    } = crate::decode::decode_archive_member(ctx, root_scan, &outer.layers)?;
    fidelity.remove_retained_record(crate::ids::FILE_SOURCE_IMAGE_ID);
    fidelity.retain_unknown_records("f3d", [crate::decode::preserve_source_image(ctx, scan)?])?;
    if let Some(drawing_root) = omitted_drawing_root {
        push_loss(ctx, &mut report.losses, F3dLossCode::DrawingDocumentOmitted, format_args!(
            "drawing root {drawing_root} is omitted; decoded its unambiguous derived model {model_root}"
        ))?;
    }
    let member_count = scan
        .entries
        .iter()
        .filter(|entry| crate::container::is_f3d_name(&entry.name))
        .count();
    ctx.push_formatted_retained(
        &mut report.notes,
        format_args!("f3z archive: {member_count} document member(s); root {model_root}"),
        "collect F3Z report notes",
        "retain F3Z report note",
    )?;
    if ctx.container_only() {
        ctx.append_vec(
            &mut report.losses,
            &mut { outer.losses },
            "append F3Z report losses",
        )?;
        return finalize_result(ctx, ir, source, report, fidelity);
    }

    let merged = merge::merge_archive(
        ctx,
        scan,
        &outer,
        model_root,
        &mut ir,
        &mut report,
        &mut fidelity,
    )?;
    if merged > 0 {
        fidelity.remove_retained_record(crate::ids::FILE_SOURCE_IMAGE_ID);
        ctx.push_formatted_retained(&mut report.notes, format_args!(
            "{merged} merged component(s) retain occurrence-scoped model entities, native records, and source bytes"
        ), "collect F3Z report notes", "retain F3Z report note")?;
    }
    ctx.push_formatted_retained(
        &mut report.notes,
        format_args!("merged {merged} external occurrence(s) from the f3z archive"),
        "collect F3Z report notes",
        "retain F3Z report note",
    )?;
    merge::make_sibling_ordinals_unique(ctx, &mut ir.model.occurrences)?;
    ctx.append_vec(
        &mut report.losses,
        &mut { outer.losses },
        "append F3Z report losses",
    )?;
    finalize_result(ctx, ir, source, report, fidelity)
}

fn finalize_result(
    ctx: &DecodeContext<'_>,
    mut ir: cadmpeg_ir::CadIr,
    mut source: cadmpeg_ir::SourceMeta,
    body: DecodeBody,
    source_fidelity: cadmpeg_ir::SourceFidelity,
) -> Result<Decoded, CodecError> {
    ir.finalize(ctx)?;
    let hash = cadmpeg_ir::hash::document_local_sha256(
        ctx,
        &ir,
        Some(&source),
        "f3d",
        crate::ids::FILE_SOURCE_IMAGE_ID,
        "record F3Z document digest",
    )?;
    ctx.insert_btree_map(
        &mut source.attributes,
        cadmpeg_core::nonblank_const!(cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE),
        hash,
        "record F3Z document digest",
    )?;
    ir.source = Some(source);
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}

#[cfg(test)]
mod tests;
