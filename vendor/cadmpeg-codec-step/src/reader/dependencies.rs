// SPDX-License-Identifier: Apache-2.0
//! External document and source dependency decoding.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::decode_text_charged;
use super::StageOutcome;
use super::{RecordExt, ValueExt};

pub(super) fn decode(
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<StageOutcome<()>, CodecError> {
    let mut losses = Vec::new();
    let mut documents = BTreeMap::new();
    let mut sources = BTreeMap::new();
    for (&id, record) in exchange.records() {
        if let Some(parameters) = document_parameters(record) {
            let identifier = parameters
                .first()
                .map(|value| {
                    decode_text_charged(
                        exchange,
                        value,
                        &mut losses,
                        id,
                        "document identifier",
                        StepLossCode::MetadataStringInvalid,
                        ctx,
                    )
                })
                .transpose()?
                .flatten()
                .unwrap_or_default();
            let name = parameters
                .get(1)
                .map(|value| {
                    decode_text_charged(
                        exchange,
                        value,
                        &mut losses,
                        id,
                        "document name",
                        StepLossCode::MetadataStringInvalid,
                        ctx,
                    )
                })
                .transpose()?
                .flatten()
                .unwrap_or_default();
            ctx.insert_btree_map(
                &mut documents,
                id,
                (
                    identifier,
                    name,
                    parameters.get(3).and_then(ValueExt::reference),
                ),
                "step_dependency_documents",
            )?;
        }
        if let Some(partial) = record.partial("EXTERNAL_SOURCE") {
            let parameters = partial.parameters.as_slice();
            if let Some(source) = parameters
                .first()
                .map(|value| source_text(exchange, value, &mut losses, id, "external source", ctx))
                .transpose()?
                .flatten()
            {
                ctx.insert_btree_map(&mut sources, id, source, "step_dependency_sources")?;
            }
        }
    }
    let mut typed = HashSet::new();
    let mut notes = BTreeSet::new();

    for (&id, record) in exchange.records() {
        if let Some(parameters) = document_reference_parameters(record) {
            let Some(document_id) = parameters.first().and_then(ValueExt::reference) else {
                continue;
            };
            let Some((identifier, name, kind)) = documents.get(&document_id) else {
                continue;
            };
            let source = parameters
                .get(1)
                .map(|value| {
                    decode_text_charged(
                        exchange,
                        value,
                        &mut losses,
                        id,
                        "document reference source",
                        StepLossCode::MetadataStringInvalid,
                        ctx,
                    )
                })
                .transpose()?
                .flatten()
                .unwrap_or_default();
            ctx.insert_btree_set(
                &mut notes,
                document_note(identifier, name, &source, ctx)?,
                "step_dependency_note_set",
            )?;
            ctx.insert_hash_set(&mut typed, id, "step_dependency_claims")?;
            ctx.insert_hash_set(&mut typed, document_id, "step_dependency_claims")?;
            if let Some(kind) = kind {
                ctx.insert_hash_set(&mut typed, *kind, "step_dependency_claims")?;
            }
        }
        if let Some(partial) = record.partial("EXTERNALLY_DEFINED_ITEM") {
            let Some(source_id) = partial.parameters.get(1).and_then(ValueExt::reference) else {
                continue;
            };
            let Some(source) = sources.get(&source_id) else {
                continue;
            };
            let item = partial
                .parameters
                .first()
                .map(|value| source_text(exchange, value, &mut losses, id, "external item", ctx))
                .transpose()?
                .flatten()
                .unwrap_or_default();
            ctx.insert_btree_set(
                &mut notes,
                ctx.join_retained(
                    &["external source ", source, " item ", &item],
                    "",
                    "step_dependency_note_text",
                )?,
                "step_dependency_note_set",
            )?;
            ctx.insert_hash_set(&mut typed, id, "step_dependency_claims")?;
            ctx.insert_hash_set(&mut typed, source_id, "step_dependency_claims")?;
        }
    }

    let mut ordered_notes = ctx.collection_vec(notes.len(), "step_dependency_note_vector")?;
    ordered_notes.extend(notes);
    Ok(StageOutcome {
        value: (),
        claims: typed,
        notes: ordered_notes,
        losses,
    })
}

fn document_parameters(record: &RawRecord) -> Option<&[Value]> {
    record
        .partial("DOCUMENT")
        .or_else(|| record.partial("DOCUMENT_FILE"))
        .map(|partial| partial.parameters.as_slice())
}

fn document_reference_parameters(record: &RawRecord) -> Option<&[Value]> {
    record
        .partial("DOCUMENT_REFERENCE")
        .or_else(|| record.partial("APPLIED_DOCUMENT_REFERENCE"))
        .map(|partial| partial.parameters.as_slice())
}

fn source_text(
    exchange: &Exchange,
    value: &Value,
    losses: &mut Vec<LossNote>,
    record_id: u64,
    field: &str,
    ctx: &DecodeContext<'_>,
) -> Result<Option<String>, CodecError> {
    match value {
        Value::String(_) => decode_text_charged(
            exchange,
            value,
            losses,
            record_id,
            field,
            StepLossCode::MetadataStringInvalid,
            ctx,
        ),
        Value::Typed(_, value) => source_text(exchange, value, losses, record_id, field, ctx),
        _ => Ok(None),
    }
}

fn document_note(
    identifier: &str,
    name: &str,
    source: &str,
    ctx: &DecodeContext<'_>,
) -> Result<String, CodecError> {
    let identity: &[&str] = match (identifier.is_empty(), name.is_empty()) {
        (false, false) => &[identifier, " (", name, ")"],
        (false, true) => &[identifier],
        (true, false) => &[name],
        (true, true) => &["unnamed"],
    };
    let mut parts = Vec::new();
    parts.push("external document ");
    parts.extend_from_slice(identity);
    if !source.is_empty() {
        parts.extend([" from ", source]);
    }
    ctx.join_retained(&parts, "", "step_dependency_note_text")
}

#[cfg(test)]
pub(crate) mod tests;
