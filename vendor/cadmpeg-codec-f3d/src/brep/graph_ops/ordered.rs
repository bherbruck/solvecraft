// SPDX-License-Identifier: Apache-2.0
//! Select graph rows in source order through admitted identity indexes.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::AttributeTarget;
use std::collections::HashSet;

pub(super) fn admit_hash_key(
    ctx: &DecodeContext<'_>,
    value: &str,
    operation: &'static str,
) -> Result<(), CodecError> {
    // Read the key for hashing and a candidate equality comparison.
    let work = u64_from_index(value.len())
        .checked_mul(2)
        .and_then(|work| work.checked_add(1))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, operation)
}

pub(super) fn contains_graph_id(
    ctx: &DecodeContext<'_>,
    values: &HashSet<String>,
    value: &str,
    operation: &'static str,
) -> Result<bool, CodecError> {
    admit_hash_key(ctx, value, operation)?;
    Ok(values.contains(value))
}

pub(super) fn insert_graph_id(
    ctx: &DecodeContext<'_>,
    values: &mut HashSet<String>,
    value: String,
    operation: &'static str,
) -> Result<bool, CodecError> {
    if contains_graph_id(ctx, values, &value, operation)? {
        return Ok(false);
    }
    if values.len() == values.capacity() {
        // Reserve rehashes the existing keys only when the table grows.
        for key in values.iter() {
            admit_hash_key(ctx, key, operation)?;
        }
    }
    ctx.reserve_set(values, 1, operation)?;
    admit_hash_key(ctx, &value, operation)?;
    Ok(values.insert(value))
}

pub(super) fn select_rows<T>(
    ctx: &DecodeContext<'_>,
    rows: Vec<T>,
    reachable: &HashSet<String>,
    owner: impl Fn(&T) -> &str,
) -> Result<Vec<T>, CodecError> {
    let mut retained = Vec::new();
    for row in rows {
        ctx.charge_work(1, "select F3D retained BREP rows")?;
        if contains_graph_id(ctx, reachable, owner(&row), "select F3D retained BREP rows")? {
            ctx.push_vec(&mut retained, row, "collect F3D retained BREP rows")?;
        }
    }
    Ok(retained)
}

fn target_selected(
    ctx: &DecodeContext<'_>,
    target: &AttributeTarget,
    reachable: &HashSet<String>,
) -> Result<bool, CodecError> {
    let id = match target {
        AttributeTarget::Document => return Ok(true),
        AttributeTarget::Body(id) => id.as_str(),
        AttributeTarget::Face(id) => id.as_str(),
        AttributeTarget::Shell(id) => id.as_str(),
        AttributeTarget::Loop(id) => id.as_str(),
        AttributeTarget::Coedge(id) => id.as_str(),
        AttributeTarget::Edge(id) => id.as_str(),
        AttributeTarget::Vertex(id) => id.as_str(),
    };
    contains_graph_id(ctx, reachable, id, "select F3D retained attribute target")
}

pub(super) fn select_links<T>(
    ctx: &DecodeContext<'_>,
    rows: Vec<T>,
    reachable: &HashSet<String>,
    target: impl Fn(&T) -> &AttributeTarget,
    operation: &'static str,
) -> Result<Vec<T>, CodecError> {
    let mut retained = Vec::new();
    for row in rows {
        ctx.charge_work(1, operation)?;
        if target_selected(ctx, target(&row), reachable)? {
            ctx.charge_work(u64_from_index(std::mem::size_of::<T>()), operation)?;
            ctx.push_vec(&mut retained, row, operation)?;
        }
    }
    Ok(retained)
}

#[cfg(test)]
mod tests;
