// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(
    test,
    allow(clippy::cloned_ref_to_slice_refs, clippy::default_trait_access)
)]
//! Decode the ASM construction-history partition after the active model slice.
//!
//! [`decode`] reads `delta_state` headers, bulletin-board entity changes, and
//! history records while retaining source bytes for records without typed
//! semantics.

use cadmpeg_core::decode::u64_from_index;

mod body_candidates;
mod body_index;
mod cache;
pub(crate) mod face_admission;
mod loop_index;
mod recipe_index;
pub(crate) mod selection;
mod topology_cache;
mod treatment_index;

use body_candidates::BodyCandidates;
use body_index::CompleteBodyIndex;
use cache::SnapshotCache;
use loop_index::LoopIndex;
use recipe_index::RecipeTopologyCache;
use topology_cache::{TopologyQueryCache, VertexBoundaryIndex};
use treatment_index::TreatmentCache;

use crate::bytes::int_at;
use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind, AsmEntityVersion,
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCylinder, AsmHistoricalEdge,
    AsmHistoricalEntityDelta, AsmHistoricalOptionalCarrierBinding, AsmHistoricalPoint,
    AsmHistoricalRelation, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
    AsmHistoricalTransition, AsmHistory, AsmHistoryRecord, AsmPreamble,
};
use crate::records::topology::{
    body_recipe::AsmHistoricalEntityKind, extrude_selection::DesignOperandRole,
};
use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const EPS_HISTORY_HEM_GAP_LENGTH_FORM_E7: f64 = 1.0e-7;
const EPS_HISTORY_HEM_DIRECTION_FROM_TRANSITION_E7: f64 = 1.0e-7;
const EPS_HISTORY_PARALLEL_DIRECTIONS_E7: f64 = 1.0e-7;
const EPS_HISTORY_RESOLVE_THREAD_FACE_BY_TRANSITION_E9: f64 = 1.0e-9;
const EPS_HISTORY_CYCLIC_POINT_SUBSEQUENCE_E12: f64 = 1.0e-12;
const EPS_HISTORY_SAME_AXIS_LINE_E9: f64 = 1.0e-9;
const EPS_HISTORY_SAME_AXIS_LINE_E8: f64 = 1.0e-8;
const EPS_HISTORY_BIND_MIRROR_SELECTION_PLANES_E9: f64 = 1.0e-9;
const EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E9: f64 = 1.0e-9;
const EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E8: f64 = 1.0e-8;
const EPS_HISTORY_HISTORICAL_LOOP_PLANE_E9: f64 = 1.0e-9;

const DELTA: &[u8] = b"\x11\x0d\x0bdelta_state";
const PREAMBLE: &[u8] = b"\x0d\x0ehistory_stream";
/// Relative tolerance for matching independently decoded millimetre point carriers.
const WORK_POINT_POSITION_TOLERANCE: f64 = 1.0e-9;
/// Relative tolerance for admitting an oriented planar Hole support face.
const HOLE_SUPPORT_NORMAL_TOLERANCE: f64 = 1.0e-9;
/// Relative tolerance for the Hole point lying on its support plane.
const HOLE_SUPPORT_POINT_TOLERANCE: f64 = 1.0e-8;

pub(crate) fn graph_is_coherent(history: &AsmHistory) -> bool {
    let decode_arena = cadmpeg_core::decode::DecodeArena::new();
    let Ok((decode_ctx, _)) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &decode_arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    ) else {
        return false;
    };
    let decode_ctx = &decode_ctx;

    graph_is_coherent_inner(decode_ctx, history).unwrap_or(false)
}

pub(crate) fn graph_is_coherent_charged(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &AsmHistory,
) -> Result<bool, cadmpeg_core::CodecError> {
    graph_is_coherent_inner(decode, history)
}

fn graph_is_coherent_inner(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &AsmHistory,
) -> Result<bool, cadmpeg_core::CodecError> {
    if history.states.is_empty() {
        return Ok(false);
    }
    let mut by_index = HashMap::new();
    for state in &history.states {
        if !by_index.contains_key(&state.node_index) {
            {
                decode.reserve_map(&mut by_index, 1, "index F3D ASM history states")?;
            }
        }
        by_index.insert(state.node_index, state);
    }
    if by_index.len() != history.states.len()
        || history
            .states
            .iter()
            .any(|state| state.node_index < 0 || state.parent != history.id)
    {
        return Ok(false);
    }
    let mut heads = history
        .states
        .iter()
        .filter(|state| state.previous_ref.is_none());
    let head = heads.next();
    let tails = history
        .states
        .iter()
        .filter(|state| state.next_ref.is_none())
        .count();
    if head.is_none() || heads.next().is_some() || tails != 1 {
        return Ok(false);
    }
    let Some(head) = head else {
        return Ok(false);
    };
    if let Some(preamble) = history.preamble {
        if head.state_id != preamble.stream_size || preamble.history_entry_count < 0 {
            return Ok(false);
        }
    }
    let mut visited = HashSet::new();
    let mut previous = None;
    let mut current = Some(head.node_index);
    while let Some(index) = current {
        let Some(state) = by_index.get(&index) else {
            return Ok(false);
        };
        if visited.contains(&index) || state.previous_ref != previous {
            return Ok(false);
        }
        {
            decode.reserve_set(&mut visited, 1, "visit F3D ASM history state")?;
        }
        visited.insert(index);
        if state.version_flag != 1 || state.state_flag != 0 {
            return Ok(false);
        }
        for board in &state.bulletin_boards {
            if board.parent != state.id
                || board.changes.iter().any(|change| change.parent != board.id)
            {
                return Ok(false);
            }
        }
        if state
            .records
            .iter()
            .any(|record| record.parent != state.id || record.raw_bytes.is_empty())
        {
            return Ok(false);
        }
        previous = Some(index);
        current = state.next_ref;
    }
    Ok(visited.len() == history.states.len())
}

/// Decode the construction-history tail of an ASM stream: every `delta_state`
/// record ([spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#32-delta_state-records)) from `bytes`, each with its `BulletinBoard` chain of
/// per-entity insert/delete/update changes and the raw history-entity records
/// framed between it and the next `delta_state`. `stream` is the source ZIP
/// entry name, recorded in each decoded item's provenance. Returns `None` when
/// `bytes` carries no `delta_state` record (the stream is a construction
/// snapshot with no history tail) or a malformed history body. `width` is the
/// stream's integer/ref width (4 for `BinaryFile4`, 8 for `BinaryFile8`).
pub(crate) fn decode(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    stream: &str,
    width: RefWidth,
    limits: &cadmpeg_core::decode::ResourceLimits,
) -> Result<Option<AsmHistory>, cadmpeg_core::CodecError> {
    let preamble_offset = bytes
        .windows(PREAMBLE.len())
        .position(|window| window == PREAMBLE);
    let history_offset = preamble_offset.unwrap_or(0);
    let history_id = crate::ids::native_scoped_id_charged(
        ctx,
        stream,
        "asm-history",
        format_args!("{history_offset:010}"),
    )?;
    let mut delta_offsets = Vec::new();
    let mut search = 0usize;
    while let Some(relative) = bytes[search..]
        .windows(DELTA.len())
        .position(|window| window == DELTA)
    {
        let offset = search + relative;

        ctx.reserve_vec(&mut delta_offsets, 1, "f3d history delta offsets")?;
        delta_offsets.push(offset);
        search = offset + DELTA.len();
    }
    let mut states = Vec::new();
    for (ordinal, &offset) in delta_offsets.iter().enumerate() {
        let state_record_id = crate::ids::native_scoped_id_charged(
            ctx,
            stream,
            "asm-delta-state",
            format_args!("{offset:010}"),
        )?;
        let mut position = offset + DELTA.len();
        let Some((
            state_id,
            version_flag,
            state_flag,
            previous,
            next,
            node_index,
            partner,
            owner_ref,
        )) = (|| {
            Some((
                take_int(bytes, &mut position, 0x04, width)?,
                take_int(bytes, &mut position, 0x04, width)?,
                take_int(bytes, &mut position, 0x04, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
            ))
        })()
        else {
            return Ok(None);
        };
        if bytes.get(position) != Some(&0x0b) {
            continue;
        }
        let Some((bulletin_boards, body_end)) = decode_bulletin_boards(
            ctx,
            bytes,
            position + 1,
            stream,
            offset,
            &state_record_id,
            width,
        )?
        else {
            return Ok(None);
        };
        let records = decode_history_records(
            ctx,
            bytes,
            body_end,
            delta_offsets.get(ordinal + 1).copied(),
            stream,
            &state_record_id,
            width,
        )?;

        ctx.reserve_vec(&mut states, 1, "admit F3D ASM delta state")?;
        let parent = ctx.copy_retained_text(&history_id, "copy F3D ASM history parent")?;
        states.push(AsmDeltaState {
            id: state_record_id,
            parent,
            byte_offset: u64_from_index(offset),
            state_id,
            version_flag,
            state_flag,
            previous_ref: (previous >= 0).then_some(previous),
            next_ref: (next >= 0).then_some(next),
            node_index,
            partner_ref: (partner >= 0).then_some(partner),
            owner_ref,
            bulletin_boards,
            records,
            entity_versions: Vec::new(),
            topology_cache: crate::history_records::AsmTopologyCache::Absent,
            transition: None,
        });
    }
    bind_snapshot_revision_ids(ctx, &mut states)?;
    bind_historical_entity_versions(ctx, &mut states)?;
    let record_table_binding_budget_exceeded =
        bind_complete_record_tables(ctx, &mut states, bytes, width, limits)?;
    if states.is_empty() {
        return Ok(None);
    }

    let preamble = preamble_offset
        .and_then(|offset| decode_preamble(bytes, offset + PREAMBLE.len(), width))
        .map(|(stream_size, history_entry_count)| AsmPreamble {
            stream_size,
            history_entry_count,
        });
    let offset = history_offset;
    Ok(Some(AsmHistory {
        id: history_id,
        byte_offset: u64_from_index(offset),
        preamble,
        record_table_binding_budget_exceeded,
        states,
    }))
}

fn bind_snapshot_revision_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &mut [AsmDeltaState],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut old_references = Vec::new();
    for old_reference in states
        .iter()
        .flat_map(|state| &state.bulletin_boards)
        .flat_map(|board| &board.changes)
        .filter_map(super::history_records::AsmEntityChange::old_ref)
    {
        ctx.reserve_vec(&mut old_references, 1, "collect F3D ASM old references")?;
        old_references.push(old_reference);
    }
    ctx.sort_unstable_by(
        &mut old_references,
        Ord::cmp,
        |_| 0,
        "sort F3D ASM old references",
    )?;
    let Some(&first) = old_references.first() else {
        return Ok(());
    };
    let count = i64::try_from(old_references.len())
        .map_err(|_| ctx.refuse_codec_limit("collect F3D ASM old references", 0, u64::MAX))?;
    let Some(end) = first.checked_add(count) else {
        return Ok(());
    };
    if old_references.iter().copied().ne(first..end) {
        return Ok(());
    }
    let snapshot_record_count = states
        .iter()
        .flat_map(|state| &state.records)
        .filter(|record| record.name() != "End-of-ASM-data")
        .count();
    if snapshot_record_count != old_references.len() {
        return Ok(());
    }
    for (record, revision_id) in states
        .iter_mut()
        .flat_map(|state| &mut state.records)
        .filter(|record| record.name() != "End-of-ASM-data")
        .zip(old_references)
    {
        record.revision_id = Some(revision_id);
    }
    Ok(())
}

fn is_history_boundary_record(record: &AsmHistoryRecord) -> bool {
    matches!(
        record.name(),
        "End-of-ASM-History-Section" | "End-of-ASM-data"
    )
}

fn archived_active_record_count(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &[AsmDeltaState],
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let mut archived = Vec::new();
    for revision_id in states
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|record| record.revision_id)
    {
        ctx.reserve_vec(&mut archived, 1, "collect F3D archived revisions")?;
        archived.push(revision_id);
    }
    ctx.sort_unstable_by(
        &mut archived,
        Ord::cmp,
        |_| 0,
        "sort F3D archived revisions",
    )?;
    let Some(&active_count) = archived.first() else {
        return Ok(None);
    };
    let count = i64::try_from(archived.len())
        .map_err(|_| ctx.refuse_codec_limit("collect F3D archived revisions", 0, u64::MAX))?;
    let Some(end) = active_count.checked_add(count) else {
        return Ok(None);
    };
    if active_count <= 0 || archived.iter().copied().ne(active_count..end) {
        return Ok(None);
    }
    Ok(usize::try_from(active_count).ok())
}

/// Return the active `RecordTable` length for a history that has no archived
/// snapshot. Insert-only chains use the active records themselves as every
/// revision, so their bulletin-board references must cover every non-header
/// slot exactly once.
fn insert_only_active_record_count(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &[AsmDeltaState],
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let mut has_boundary_record = false;
    for record in states.iter().flat_map(|state| &state.records) {
        if record.revision_id.is_some() || !is_history_boundary_record(record) {
            return Ok(None);
        }
        has_boundary_record = true;
    }
    if !has_boundary_record {
        return Ok(None);
    }
    let mut inserted = BTreeSet::new();
    for change in states
        .iter()
        .flat_map(|state| &state.bulletin_boards)
        .flat_map(|board| &board.changes)
    {
        let Some(new_ref) = change.new_ref().filter(|_| change.old_ref().is_none()) else {
            return Ok(None);
        };
        if new_ref <= 0 || inserted.contains(&new_ref) {
            return Ok(None);
        }
        ctx.insert_btree_set(&mut inserted, new_ref, "index F3D insert-only revisions")?;
    }
    let Some(&last) = inserted.last() else {
        return Ok(None);
    };
    if inserted.iter().copied().ne(1..=last) {
        return Ok(None);
    }
    Ok(last
        .checked_add(1)
        .and_then(|count| usize::try_from(count).ok()))
}

fn bind_historical_entity_versions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &mut [AsmDeltaState],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut archived_ids = Vec::new();
    for revision_id in states
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|record| record.revision_id)
    {
        ctx.reserve_vec(&mut archived_ids, 1, "index F3D archived revision IDs")?;
        archived_ids.push(revision_id);
    }
    ctx.sort_unstable_by(
        &mut archived_ids,
        Ord::cmp,
        |_| 0,
        "sort F3D archived revision IDs",
    )?;
    let active_count = match archived_active_record_count(ctx, states)? {
        Some(count) => Some(count),
        None => insert_only_active_record_count(ctx, states)?,
    }
    .and_then(|count| i64::try_from(count).ok());
    let Some(active_count) = active_count else {
        return Ok(());
    };
    let mut by_node = HashMap::new();
    for (ordinal, state) in states.iter().enumerate() {
        if !by_node.contains_key(&state.node_index) {
            ctx.reserve_map(&mut by_node, 1, "index F3D history node ordinals")?;
        }
        by_node.insert(state.node_index, ordinal);
    }
    if by_node.len() != states.len() {
        return Ok(());
    }
    let mut heads = states
        .iter()
        .enumerate()
        .filter(|(_, state)| state.previous_ref.is_none())
        .map(|(ordinal, _)| ordinal);
    let Some(mut ordinal) = heads.next() else {
        return Ok(());
    };
    if heads.next().is_some() {
        return Ok(());
    }
    let active_count_u64 = u64::try_from(active_count)
        .map_err(|_| ctx.refuse_codec_limit("seed F3D history versions", 0, u64::MAX))?;
    ctx.charge_work(active_count_u64, "seed F3D history versions")?;
    let mut versions = BTreeMap::new();
    for id in 0..active_count {
        ctx.insert_btree_map(&mut versions, id, id, "seed F3D history versions")?;
    }
    let mut projected = HashMap::new();
    let mut visited = HashSet::new();
    loop {
        let state = &states[ordinal];
        ctx.charge_work(1, "bind F3D historical entity versions")?;
        if visited.contains(&state.node_index) {
            return Ok(());
        }

        ctx.reserve_set(&mut visited, 1, "visit F3D history version state")?;
        visited.insert(state.node_index);
        let version_count = versions.len();

        let mut state_versions = Vec::new();
        ctx.reserve_vec(
            &mut state_versions,
            version_count,
            "materialize F3D state versions",
        )?;
        for (&entity_ref, &record_ref) in &versions {
            state_versions.push(AsmEntityVersion {
                entity_ref,
                record_ref,
            });
        }

        ctx.reserve_map(&mut projected, 1, "index F3D state version projections")?;
        projected.insert(state.node_index, state_versions);
        for change in state
            .bulletin_boards
            .iter()
            .flat_map(|board| &board.changes)
        {
            match change.kind {
                AsmEntityChangeKind::Update { old, new } => {
                    if !versions.contains_key(&new) || archived_ids.binary_search(&old).is_err() {
                        return Ok(());
                    }
                    versions.insert(new, old);
                }
                AsmEntityChangeKind::Insert { new } => {
                    if versions.remove(&new).is_none() {
                        return Ok(());
                    }
                }
                AsmEntityChangeKind::Delete { old } => {
                    if versions.contains_key(&old) || archived_ids.binary_search(&old).is_err() {
                        return Ok(());
                    }
                    ctx.insert_btree_map(
                        &mut versions,
                        old,
                        old,
                        "restore F3D historical version",
                    )?;
                }
            }
        }
        let Some(next) = state.next_ref else {
            break;
        };
        let Some(&next_ordinal) = by_node.get(&next) else {
            return Ok(());
        };
        ordinal = next_ordinal;
    }
    if visited.len() != states.len() || versions.len() != 1 || versions.get(&0) != Some(&0) {
        return Ok(());
    }
    for state in states {
        state.entity_versions = projected.remove(&state.node_index).unwrap_or_default();
    }
    Ok(())
}

/// Historical topology caches retain normalized records, topology entities,
/// incidence links, and geometry measurements while Design projection runs.
/// This conservative per-entry bound checks that temporary cache against the
/// caller's materialization policy. A binding that exceeds either limit
/// refuses through the caller context before the topology cache is built.
// One live entity can retain its 16-byte version pair, one 8-byte family slot,
// one 48-byte coedge link (the largest topology link), one 56-byte curve-axis
// measurement (the largest geometry measurement), one 8-byte ownership member,
// and one 32-byte relation allocation. The remaining 24 bytes cover the parent
// vector allocation and alignment. Mutually exclusive entity families make
// this an upper bound, not an average observed size.
const HISTORY_TOPOLOGY_CACHE_BYTES_PER_ENTRY: u64 = 192;
/// Conservative work charge for decoding one record in one historical state.
/// A state snapshot traverses the record table repeatedly to build topology,
/// incidence, and carrier projections; this keeps the admission estimate
/// proportional to the state-by-record cross product instead of only the
/// final cache size.
const HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY: u64 = 4096;

#[cfg(test)]
fn complete_table_binding_budget_exceeded(
    table_lengths: impl IntoIterator<Item = usize>,
    limits: &cadmpeg_core::decode::ResourceLimits,
) -> bool {
    table_lengths
        .into_iter()
        .try_fold(0_u64, |total, length| {
            total.checked_add(u64::try_from(length).ok()?)
        })
        .and_then(|entries| entries.checked_mul(HISTORY_TOPOLOGY_CACHE_BYTES_PER_ENTRY))
        .is_none_or(|bytes| bytes > limits.max_materialized_bytes)
}

#[cfg(test)]
fn history_topology_work_budget_exceeded(
    table_lengths: impl IntoIterator<Item = usize>,
    limits: &cadmpeg_core::decode::ResourceLimits,
) -> bool {
    table_lengths
        .into_iter()
        .try_fold(0_u64, |total, length| {
            total.checked_add(u64::try_from(length).ok()?)
        })
        .and_then(|entries| entries.checked_mul(HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY))
        .is_none_or(|work| work > limits.max_work_units)
}

fn admit_complete_table_binding_budget(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    mut table_lengths: impl ExactSizeIterator<Item = usize>,
    limits: &cadmpeg_core::decode::ResourceLimits,
) -> Result<(), cadmpeg_core::CodecError> {
    let state_count = u64_from_index(table_lengths.len());
    ctx.charge_work(state_count, "check F3D complete history topology")?;
    let entries = table_lengths.try_fold(0_u64, |total, length| {
        total.checked_add(u64_from_index(length))
    });
    let bytes = entries
        .and_then(|entries| entries.checked_mul(HISTORY_TOPOLOGY_CACHE_BYTES_PER_ENTRY))
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "bind F3D complete history topology bytes",
                limits.max_materialized_bytes,
                u64::MAX,
            )
        })?;
    if bytes > limits.max_materialized_bytes {
        return Err(ctx.refuse_codec_limit(
            "bind F3D complete history topology bytes",
            limits.max_materialized_bytes,
            bytes,
        ));
    }
    let work = entries
        .and_then(|entries| entries.checked_mul(HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY))
        .ok_or_else(|| {
            ctx.refuse_codec_limit(
                "bind F3D complete history topology work",
                limits.max_work_units,
                u64::MAX,
            )
        })?;
    if work > limits.max_work_units {
        return Err(ctx.refuse_codec_limit(
            "bind F3D complete history topology work",
            limits.max_work_units,
            work,
        ));
    }
    Ok(())
}

fn bind_complete_record_tables(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &mut [AsmDeltaState],
    bytes: &[u8],
    width: RefWidth,
    limits: &cadmpeg_core::decode::ResourceLimits,
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(start) = cadmpeg_asm::asm_header::record_stream_start(bytes) else {
        return Ok(false);
    };
    let active_limit =
        cadmpeg_asm::asm_header::solved_record_limit(ctx, bytes)?.unwrap_or(bytes.len());
    let framed = match cadmpeg_asm::sab::frame(ctx, bytes, start, active_limit, width, None) {
        Ok(records) => records,
        Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => return Err(error.into()),
        Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
            return Err(error.into_codec_error())
        }
        Err(_) => return Ok(false),
    };
    admit_complete_table_binding_budget(
        ctx,
        states.iter().map(|state| state.entity_versions.len()),
        limits,
    )?;
    let insert_only = insert_only_active_record_count(ctx, states)?;
    let archived_count = archived_active_record_count(ctx, states)?;
    let Some(active_count) = archived_count.or(insert_only) else {
        return Ok(false);
    };
    if insert_only.is_some() && framed.len() != active_count {
        return Ok(false);
    }
    let Some(active_records) = framed.get(..active_count) else {
        return Ok(false);
    };
    let mut archived_frames = HashMap::new();
    for record in states
        .iter()
        .flat_map(|state| &state.records)
        .filter(|record| record.revision_id.is_some())
    {
        let Some(revision_id) = record.revision_id else {
            return Ok(false);
        };
        let Some(offset) = usize::try_from(record.byte_offset).ok() else {
            return Ok(false);
        };
        let Some(limit) = offset.checked_add(record.raw_bytes.len()) else {
            return Ok(false);
        };
        if bytes.get(offset..limit) != Some(record.raw_bytes.as_slice()) {
            return Ok(false);
        }
        let mut framed = match cadmpeg_asm::sab::frame(ctx, bytes, offset, limit, width, None) {
            Ok(records) => records,
            Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => {
                return Err(error.into())
            }
            Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
                return Err(error.into_codec_error())
            }
            Err(_) => return Ok(false),
        };
        if framed.len() != 1 {
            return Ok(false);
        }
        let Some(framed) = framed.pop() else {
            return Ok(false);
        };
        if framed.name != record.name() {
            return Ok(false);
        }

        ctx.reserve_map(&mut archived_frames, 1, "retain F3D archived record frame")?;
        if archived_frames.insert(revision_id, framed).is_some() {
            return Ok(false);
        }
    }
    let Some(archive) = historical_record_archive(ctx, states, active_records, archived_frames)?
    else {
        return Ok(false);
    };
    let mut complete = true;
    for state in states.iter_mut() {
        let Some(records) = materialize_record_table(ctx, state, &archive)? else {
            complete = false;
            break;
        };
        let decoded =
            match crate::brep::decode_history_topology(ctx, &records, bytes, crate::ids::ID_FORMAT)
            {
                Ok(decoded) => decoded,
                Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => return Err(error),
                Err(_) => {
                    complete = false;
                    break;
                }
            };
        let Some(topology) = historical_topology_with_tags(ctx, &decoded)? else {
            complete = false;
            break;
        };
        state.topology_cache = crate::history_records::AsmTopologyCache::Complete(topology);
    }
    if complete {
        bind_historical_transitions(ctx, states)?;
        // Keep only record revisions that can be named by a late persistent
        // selection. Non-topological ASM attributes do not participate in
        // feature selection and need not survive projection finalization.
        for state in states {
            if let Some(topology) = state.topology() {
                let slots = topology_entity_slots(ctx, topology)?;
                state
                    .entity_versions
                    .retain(|version| slots.contains(&version.entity_ref));
            } else {
                state.entity_versions.clear();
            }
        }
    } else {
        for state in states {
            state.topology_cache = crate::history_records::AsmTopologyCache::Absent;
        }
    }
    Ok(false)
}

fn topology_entity_slots(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
) -> Result<HashSet<i64>, cadmpeg_core::CodecError> {
    let families = [
        &topology.bodies,
        &topology.regions,
        &topology.shells,
        &topology.faces,
        &topology.loops,
        &topology.coedges,
        &topology.edges,
        &topology.vertices,
        &topology.points,
        &topology.surfaces,
        &topology.curves,
        &topology.pcurves,
    ];
    let count = families
        .iter()
        .try_fold(0_usize, |total, family| total.checked_add(family.len()))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("index F3D historical topology slots", 0, u64::MAX)
        })?;
    let count_u64 = u64::try_from(count)
        .map_err(|_| ctx.refuse_codec_limit("index F3D historical topology slots", 0, u64::MAX))?;

    ctx.charge_work(count_u64, "index F3D historical topology slots")?;
    let mut slots = HashSet::new();
    ctx.reserve_set(&mut slots, count, "index F3D historical topology slots")?;
    for slot in families.into_iter().flatten() {
        slots.insert(*slot);
    }
    Ok(slots)
}

type HistoricalRecordArchive = HashMap<i64, cadmpeg_asm::sab::Record>;

fn historical_record_archive(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &[AsmDeltaState],
    active_records: &[cadmpeg_asm::sab::Record],
    archived_frames: HashMap<i64, cadmpeg_asm::sab::Record>,
) -> Result<Option<HistoricalRecordArchive>, cadmpeg_core::CodecError> {
    if active_records
        .iter()
        .enumerate()
        .any(|(index, record)| record.index != index)
    {
        return Ok(None);
    }
    let Some(active_count) = i64::try_from(active_records.len()).ok() else {
        return Ok(None);
    };

    let mut revision_entities = HashMap::new();
    ctx.reserve_map(
        &mut revision_entities,
        active_records.len(),
        "index F3D active record revisions",
    )?;
    for entity_ref in 0..active_count {
        revision_entities.insert(entity_ref, entity_ref);
    }
    for change in states
        .iter()
        .flat_map(|state| &state.bulletin_boards)
        .flat_map(|board| &board.changes)
    {
        let Some(old_ref) = change.old_ref() else {
            continue;
        };
        let entity_ref = change.new_ref().unwrap_or(old_ref);

        ctx.reserve_map(
            &mut revision_entities,
            1,
            "index F3D archived record revisions",
        )?;
        if revision_entities.insert(old_ref, entity_ref).is_some() {
            return Ok(None);
        }
    }

    let mut records = HashMap::new();
    ctx.reserve_map(
        &mut records,
        active_records.len(),
        "retain F3D active record archive",
    )?;
    for (revision, record) in active_records.iter().enumerate() {
        let Some(revision) = i64::try_from(revision).ok() else {
            return Ok(None);
        };
        records.insert(revision, clone_historical_record(ctx, record)?);
    }
    for (revision_id, framed) in archived_frames {
        ctx.reserve_map(&mut records, 1, "retain F3D archived record archive")?;
        if records.insert(revision_id, framed).is_some() {
            return Ok(None);
        }
    }
    if records.len() != revision_entities.len() {
        return Ok(None);
    }
    for (&revision_ref, record) in &mut records {
        let Some(&entity_ref) = revision_entities.get(&revision_ref) else {
            return Ok(None);
        };
        let Some(index) = usize::try_from(entity_ref).ok() else {
            return Ok(None);
        };
        record.index = index;
        if std::sync::Arc::strong_count(&record.tokens) > 1 {
            let token_count = u64::try_from(record.tokens.len()).map_err(|_| {
                ctx.refuse_codec_limit("copy F3D archived record tokens", 0, u64::MAX)
            })?;

            let token_width = u64::try_from(std::mem::size_of::<cadmpeg_asm::sab::Token>())
                .map_err(|_| {
                    ctx.refuse_codec_limit("copy F3D archived record tokens", 0, u64::MAX)
                })?;
            let token_bytes = token_count.checked_mul(token_width).ok_or_else(|| {
                ctx.refuse_codec_limit("copy F3D archived record tokens", 0, u64::MAX)
            })?;
            let _temporary = ctx.reserve_scoped(token_bytes, "copy F3D archived record tokens")?;
            let mut tokens =
                ctx.collection_vec(record.tokens.len(), "copy F3D archived record tokens")?;
            for token in record.tokens.iter() {
                tokens.push(clone_historical_token(ctx, token)?);
            }
            record.tokens = tokens.into();
        }
        for token in std::sync::Arc::make_mut(&mut record.tokens) {
            let cadmpeg_asm::sab::Token::Ref(reference) = token else {
                continue;
            };
            if *reference >= 0 {
                let Some(&entity_ref) = revision_entities.get(reference) else {
                    return Ok(None);
                };
                *reference = entity_ref;
            }
        }
    }
    Ok(Some(records))
}

fn clone_historical_record(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &cadmpeg_asm::sab::Record,
) -> Result<cadmpeg_asm::sab::Record, cadmpeg_core::CodecError> {
    let name = ctx.copy_retained_text(&record.name, "copy F3D historical record text")?;
    Ok(cadmpeg_asm::sab::Record {
        index: record.index,
        name,
        tokens: record.tokens.clone(),
        offset: record.offset,
        len: record.len,
    })
}

fn clone_historical_token(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    token: &cadmpeg_asm::sab::Token,
) -> Result<cadmpeg_asm::sab::Token, cadmpeg_core::CodecError> {
    use cadmpeg_asm::sab::Token;
    Ok(match token {
        Token::Str(value) => {
            Token::Str(ctx.copy_retained_text(value, "copy F3D historical record text")?)
        }
        Token::Ident(value) => {
            Token::Ident(ctx.copy_retained_text(value, "copy F3D historical record text")?)
        }
        Token::SubIdent(value) => {
            Token::SubIdent(ctx.copy_retained_text(value, "copy F3D historical record text")?)
        }
        _ => token.clone(),
    })
}

fn bind_historical_transitions(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &mut [AsmDeltaState],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut by_node = HashMap::new();
    ctx.reserve_map(&mut by_node, states.len(), "index F3D transition nodes")?;
    for (ordinal, state) in states.iter().enumerate() {
        by_node.insert(state.node_index, ordinal);
    }
    if by_node.len() != states.len() {
        return Ok(());
    }

    let mut transitions = Vec::new();
    ctx.reserve_vec(
        &mut transitions,
        states.len(),
        "collect F3D historical transitions",
    )?;
    for state in states.iter() {
        let previous = match state.next_ref {
            Some(node) => {
                let Some(&ordinal) = by_node.get(&node) else {
                    return Ok(());
                };
                states.get(ordinal)
            }
            None => None,
        };
        let Some(transition) = historical_transition(ctx, state, previous)? else {
            return Ok(());
        };
        transitions.push(transition);
    }
    for (state, transition) in states.iter_mut().zip(transitions) {
        state.transition = Some(transition);
    }
    Ok(())
}

fn retain_mirror_plane_topology(
    mut topology: AsmHistoricalTopology,
) -> Option<AsmHistoricalTopology> {
    let retained = AsmHistoricalTopology {
        bodies: std::mem::take(&mut topology.bodies),
        regions: std::mem::take(&mut topology.regions),
        shells: std::mem::take(&mut topology.shells),
        faces: std::mem::take(&mut topology.faces),
        loops: std::mem::take(&mut topology.loops),
        coedges: std::mem::take(&mut topology.coedges),
        edges: std::mem::take(&mut topology.edges),
        vertices: std::mem::take(&mut topology.vertices),
        points: std::mem::take(&mut topology.points),
        surfaces: std::mem::take(&mut topology.surfaces),
        curves: std::mem::take(&mut topology.curves),
        pcurves: std::mem::take(&mut topology.pcurves),
        face_surfaces: std::mem::take(&mut topology.face_surfaces),
        surface_planes: std::mem::take(&mut topology.surface_planes),
        loop_coedges: std::mem::take(&mut topology.loop_coedges),
        coedge_topology: std::mem::take(&mut topology.coedge_topology),
        face_loops: std::mem::take(&mut topology.face_loops),
        edge_curves: std::mem::take(&mut topology.edge_curves),
        coedge_pcurves: std::mem::take(&mut topology.coedge_pcurves),
        curve_axes: std::mem::take(&mut topology.curve_axes),
        ..Default::default()
    };
    (!retained.face_surfaces.is_empty()
        || !retained.surface_planes.is_empty()
        || !retained.loop_coedges.is_empty()
        || !retained.coedge_topology.is_empty()
        || !retained.face_loops.is_empty()
        || !retained.edge_curves.is_empty()
        || !retained.coedge_pcurves.is_empty()
        || !retained.curve_axes.is_empty())
    .then_some(retained)
}

/// Release complete historical snapshots after every projection consumer has
/// finished. Raw history records, sparse transitions, and the compact
/// plane-selection topology remain retained.
pub(crate) fn discard_projection_caches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &mut [AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    for history in histories {
        for state in &mut history.states {
            let topology = match std::mem::take(&mut state.topology_cache) {
                crate::history_records::AsmTopologyCache::Absent
                | crate::history_records::AsmTopologyCache::Released => None,
                crate::history_records::AsmTopologyCache::Complete(topology)
                | crate::history_records::AsmTopologyCache::Retained(topology) => {
                    retain_mirror_plane_topology(topology)
                }
            };
            if let Some(topology) = topology {
                let slots = topology_entity_slots(ctx, &topology)?;
                state
                    .entity_versions
                    .retain(|version| slots.contains(&version.entity_ref));
                state.topology_cache = crate::history_records::AsmTopologyCache::Retained(topology);
            } else {
                state.entity_versions.clear();
                state.topology_cache = crate::history_records::AsmTopologyCache::Released;
            }
        }
    }
    Ok(())
}

pub(crate) fn projection_was_finalized(histories: &[AsmHistory]) -> bool {
    !histories.is_empty()
        && histories
            .iter()
            .all(crate::history_records::AsmHistory::projection_finalized)
}

fn historical_transition(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    current: &AsmDeltaState,
    previous: Option<&AsmDeltaState>,
) -> Result<Option<AsmHistoricalTransition>, cadmpeg_core::CodecError> {
    let Some(current_topology) = current.topology() else {
        return Ok(None);
    };
    let previous_topology = previous.and_then(|state| state.topology());
    let current_versions = historical_version_map(ctx, &current.entity_versions)?;
    let previous_versions = match previous {
        Some(state) => historical_version_map(ctx, &state.entity_versions)?,
        None => BTreeMap::new(),
    };
    let delta = |current: &[i64], previous: &[i64]| {
        entity_delta(
            ctx,
            current,
            previous,
            &current_versions,
            &previous_versions,
        )
    };
    let empty = AsmHistoricalTopology::default();
    let previous_topology = previous_topology.unwrap_or(&empty);
    let current_record_keys = ctx.collect_vec(
        current_versions.keys().copied(),
        "collect F3D transition version keys",
    )?;
    let previous_record_keys = ctx.collect_vec(
        previous_versions.keys().copied(),
        "collect F3D transition version keys",
    )?;
    Ok(Some(AsmHistoricalTransition {
        previous_state_id: previous.map(|state| state.state_id),
        records: entity_delta(
            ctx,
            &current_record_keys,
            &previous_record_keys,
            &current_versions,
            &previous_versions,
        )?,
        topology: AsmHistoricalTopologyDelta {
            bodies: delta(&current_topology.bodies, &previous_topology.bodies)?,
            regions: delta(&current_topology.regions, &previous_topology.regions)?,
            shells: delta(&current_topology.shells, &previous_topology.shells)?,
            faces: delta(&current_topology.faces, &previous_topology.faces)?,
            loops: delta(&current_topology.loops, &previous_topology.loops)?,
            coedges: delta(&current_topology.coedges, &previous_topology.coedges)?,
            edges: delta(&current_topology.edges, &previous_topology.edges)?,
            vertices: delta(&current_topology.vertices, &previous_topology.vertices)?,
            points: delta(&current_topology.points, &previous_topology.points)?,
            surfaces: delta(&current_topology.surfaces, &previous_topology.surfaces)?,
            curves: delta(&current_topology.curves, &previous_topology.curves)?,
            pcurves: delta(&current_topology.pcurves, &previous_topology.pcurves)?,
        },
    }))
}

fn historical_version_map(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    versions: &[AsmEntityVersion],
) -> Result<BTreeMap<i64, i64>, cadmpeg_core::CodecError> {
    let mut indexed = BTreeMap::new();
    for version in versions {
        ctx.insert_btree_map(
            &mut indexed,
            version.entity_ref,
            version.record_ref,
            "index F3D transition versions",
        )?;
    }
    Ok(indexed)
}

fn entity_delta(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    current: &[i64],
    previous: &[i64],
    current_versions: &BTreeMap<i64, i64>,
    previous_versions: &BTreeMap<i64, i64>,
) -> Result<AsmHistoricalEntityDelta, cadmpeg_core::CodecError> {
    let current_count = u64::try_from(current.len()).map_err(|_| {
        ctx.refuse_codec_limit("index F3D current transition entities", 0, u64::MAX)
    })?;
    let previous_count = u64::try_from(previous.len()).map_err(|_| {
        ctx.refuse_codec_limit("index F3D previous transition entities", 0, u64::MAX)
    })?;
    let current = ctx.collect_btree_set(
        current.iter().copied(),
        "index F3D current transition entities",
    )?;
    let previous = ctx.collect_btree_set(
        previous.iter().copied(),
        "index F3D previous transition entities",
    )?;
    let work = current_count
        .checked_add(previous_count)
        .ok_or_else(|| ctx.refuse_codec_limit("compare F3D transition entities", 0, u64::MAX))?;
    ctx.charge_work(work, "compare F3D transition entities")?;
    Ok(AsmHistoricalEntityDelta {
        inserted: ctx.collect_vec(
            current.difference(&previous).copied(),
            "collect F3D transition delta",
        )?,
        deleted: ctx.collect_vec(
            previous.difference(&current).copied(),
            "collect F3D transition delta",
        )?,
        updated: ctx.collect_vec(
            current
                .intersection(&previous)
                .copied()
                .filter(|entity| current_versions.get(entity) != previous_versions.get(entity)),
            "collect F3D transition delta",
        )?,
    })
}

pub(crate) fn bind_feature_outputs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    active_bodies: &[cadmpeg_ir::topology::Body],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut state_outputs = HashMap::<i64, Option<Vec<i64>>>::new();
    let mut body_closures = HistoricalBodyClosureCache::default();
    for history in histories {
        let mut by_node = HashMap::new();
        for state in &history.states {
            if !by_node.contains_key(&state.node_index) {
                ctx.reserve_map(&mut by_node, 1, "index F3D feature output history nodes")?;
            }
            by_node.insert(state.node_index, state);
        }
        if by_node.len() != history.states.len() {
            continue;
        }
        for state in &history.states {
            let previous = match state.next_ref {
                Some(node) => match by_node.get(&node) {
                    Some(previous) => Some(*previous),
                    None => continue,
                },
                None => None,
            };
            let Some(outputs) = affected_body_refs(ctx, state, previous, &mut body_closures)?
            else {
                continue;
            };
            if !state_outputs.contains_key(&state.state_id) {
                ctx.reserve_map(&mut state_outputs, 1, "index F3D feature output states")?;
            }
            state_outputs
                .entry(state.state_id)
                .and_modify(|outputs| *outputs = None)
                .or_insert_with(|| Some(outputs));
        }
    }
    let mut active = HashMap::new();
    for body in active_bodies {
        let Some(slot) = stable_ref(body.id.as_str()) else {
            continue;
        };
        let id = body
            .id
            .try_clone_for_decode(ctx, "copy F3D active body identity")?;
        if !active.contains_key(&slot) {
            ctx.reserve_map(&mut active, 1, "index F3D active feature output bodies")?;
        }
        active.insert(slot, id);
    }
    for feature in features {
        let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|id| scopes.iter().find(|scope| scope.id == id))
        else {
            continue;
        };
        let (Some(state_id), Some(previous_state_id)) =
            (scope.history_state_id(), scope.previous_history_state_id())
        else {
            continue;
        };
        let Some(Some(outputs)) = state_outputs.get(&state_id) else {
            continue;
        };
        let transition_matches = histories
            .iter()
            .flat_map(|history| &history.states)
            .filter(|state| state.state_id == state_id)
            .map(|state| {
                state
                    .transition
                    .as_ref()
                    .and_then(|transition| transition.previous_state_id)
                    == Some(previous_state_id)
            })
            .eq([true]);
        if transition_matches {
            let mut resolved = Vec::new();
            for slot in outputs {
                let Some(id) = active.get(slot) else {
                    continue;
                };
                let id = id.try_clone_for_decode(ctx, "copy F3D feature output body identity")?;

                ctx.reserve_vec(&mut resolved, 1, "collect F3D feature output bodies")?;
                resolved.push(id);
            }
            feature.evaluation.set_outputs(
                cadmpeg_ir::features::DistinctMembers::try_from(resolved, ctx)
                    .map_err(cadmpeg_core::CodecError::from)?,
            );
            bind_base_feature_output_selection(ctx, feature)?;
        }
    }
    Ok(())
}

fn bind_base_feature_output_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature: &mut cadmpeg_ir::features::Feature,
) -> Result<(), cadmpeg_core::CodecError> {
    if feature.evaluation.outputs().is_empty() {
        return Ok(());
    }
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::BaseFeature {
            bodies: cadmpeg_ir::features::BodySelection::Native(native),
        },
    ) = feature.evaluation.definition()
    else {
        return Ok(());
    };
    let mut selected = Vec::new();
    for body in feature.evaluation.outputs() {
        let id = body.try_clone_for_decode(ctx, "copy F3D BaseFeature body identity")?;

        ctx.reserve_vec(&mut selected, 1, "collect F3D BaseFeature output bodies")?;
        selected.push(id);
    }
    let bodies = cadmpeg_ir::features::BodySelection::Resolved {
        bodies: cadmpeg_ir::features::DistinctMembers::try_from(selected, ctx)
            .map_err(cadmpeg_core::CodecError::from)?,
        native: ctx.copy_retained_text(native, "copy F3D BaseFeature native selection")?,
    };
    feature
        .evaluation
        .set_definition(cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::BaseFeature { bodies },
        ));
    Ok(())
}

pub(crate) fn bind_sweep_result_modes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    bodies: &[cadmpeg_ir::topology::Body],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SweepMode, SweepShape};
    use cadmpeg_ir::topology::BodyKind;

    let mut body_kinds = HashMap::new();
    for body in bodies {
        if !body_kinds.contains_key(&body.id) {
            ctx.reserve_map(&mut body_kinds, 1, "index F3D sweep body kinds")?;
        }
        body_kinds.insert(&body.id, body.kind);
    }
    for feature in features {
        let FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: SweepShape::Unresolved { sections, .. },
            ..
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        if feature.evaluation.outputs().is_empty() {
            continue;
        }
        let section_count = sections.len();
        let mut all_sheet = true;
        let mut all_solid = true;
        for output in feature.evaluation.outputs() {
            ctx.charge_work(1, "resolve F3D sweep output kind")?;
            match body_kinds.get(output) {
                Some(BodyKind::Sheet) => all_solid = false,
                Some(BodyKind::Solid) => all_sheet = false,
                _ => {
                    all_sheet = false;
                    all_solid = false;
                }
            }
        }
        let mode = if all_sheet {
            SweepMode::Surface {}
        } else if all_solid {
            SweepMode::Solid {
                op: cadmpeg_ir::features::SolidSweepOperation::NewBody,
            }
        } else {
            SweepMode::Unresolved {}
        };
        let mut solid_sections = Vec::new();
        if matches!(mode, SweepMode::Solid { .. }) {
            ctx.reserve_vec(
                &mut solid_sections,
                section_count,
                "convert F3D solid sweep sections",
            )?;
        }
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) = definition
            else {
                return;
            };
            let SweepShape::Unresolved { section, sections } =
                std::mem::replace(shape, SweepShape::unresolved(None))
            else {
                return;
            };
            *shape = match mode {
                SweepMode::Solid { op } => {
                    solid_sections.extend(sections.into_iter().map(Into::into));
                    SweepShape::Solid {
                        op,
                        section: section.into(),
                        sections: solid_sections,
                    }
                }
                _ => SweepShape::sheet_sections(mode, section, sections),
            };
        });
    }
    Ok(())
}

/// Native history and neutral topology used to resolve feature body operands.
pub(crate) struct FeatureBodySelectionInputs<'a> {
    /// Decoded Design feature scopes.
    pub(crate) scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    /// Counted Design construction-operand groups.
    pub(crate) groups:
        &'a [crate::records::topology::construction::DesignConstructionOperandGroup],
    /// Whole-body recipe operands.
    pub(crate) body_recipe_operands:
        &'a [crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    /// Construction recipes backing whole-body operands.
    pub(crate) construction_recipes: &'a [crate::records::recipes::ConstructionRecipe],
    /// Persistent body identities in the active solved B-rep.
    pub(crate) persistent_design_links: &'a [crate::records::sketch_links::PersistentDesignLink],
    /// Independent ASM history graphs.
    pub(crate) histories: &'a [AsmHistory],
    /// Neutral top-level bodies.
    pub(crate) bodies: &'a [cadmpeg_ir::topology::Body],
    /// Neutral body regions.
    pub(crate) regions: &'a [cadmpeg_ir::topology::Region],
    /// Neutral region shells.
    pub(crate) shells: &'a [cadmpeg_ir::topology::Shell],
}

pub(crate) fn bind_feature_body_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    inputs: &FeatureBodySelectionInputs<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{BodyMember, BodySelection, FeatureDefinition, FeatureOperation};

    let scopes = inputs.scopes;
    let groups = inputs.groups;
    let body_recipe_operands = inputs.body_recipe_operands;
    let histories = inputs.histories;
    let bodies = inputs.bodies;
    let regions = inputs.regions;
    let shells = inputs.shells;

    let mut body_candidates = BodyCandidates::new(
        bodies,
        regions,
        shells,
        inputs.construction_recipes,
        inputs.persistent_design_links,
    );
    bind_pattern_body_selections(ctx, features, inputs, &mut body_candidates)?;
    let mut pattern_body_slots = HashMap::new();
    'pattern: for feature in features.iter() {
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let cadmpeg_ir::features::patterns::PatternTransform::Circular { count, .. } =
            pattern.definition()
        else {
            continue;
        };
        let [cadmpeg_ir::features::patterns::PatternSeed::Bodies(BodySelection::Historical {
            bodies: seed_bodies,
            ..
        })] = seeds.as_slice()
        else {
            continue;
        };
        let [seed_body] = seed_bodies.as_slice() else {
            continue;
        };
        let Ok(expected_count) = usize::try_from(*count) else {
            continue;
        };
        if feature.evaluation.outputs().len().checked_add(1) != Some(expected_count) {
            continue;
        }
        let mut slots = BTreeSet::new();
        for slot in std::iter::once(historical_body_slot(seed_body.as_str())).chain(
            feature
                .evaluation
                .outputs()
                .iter()
                .map(|body| stable_ref(body.as_str())),
        ) {
            let Some(slot) = slot else {
                continue 'pattern;
            };
            ctx.insert_btree_set(&mut slots, slot, "index F3D pattern body slots")
                .map(|_| ())?;
        }
        if slots.len() != expected_count {
            continue;
        }
        let feature_id = feature
            .id
            .try_clone_for_decode(ctx, "copy F3D pattern body feature ID")?;
        if !pattern_body_slots.contains_key(&feature_id) {
            ctx.reserve_map(
                &mut pattern_body_slots,
                1,
                "index F3D pattern body features",
            )?;
        }
        pattern_body_slots.insert(feature_id, slots);
    }

    for feature in features {
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
        let dependencies = &feature.dependencies;
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
        'feature_edit: {
            let Some(native_ref) = native_ref else {
                break 'feature_edit;
            };
            let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
            let Some(scope) = matching_scopes.next() else {
                break 'feature_edit;
            };
            if matching_scopes.next().is_some() {
                break 'feature_edit;
            }
            if matches!(
                definition,
                FeatureDefinition::Operation(FeatureOperation::Pattern { .. })
            ) {
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::BoundaryFill { tools, cells }) =
                definition
            {
                if let Some(previous_state_id) = scope.previous_history_state_id() {
                    edit_result = bind_body_recipe_body_selection(
                        ctx,
                        tools,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                    for cell in cells {
                        if edit_result.is_err() { break; }
                        edit_result = bind_body_recipe_body_selection(
                            ctx,
                            cell,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            body_recipe_operands,
                        );
                    }
                } else {
                    edit_result = bind_direct_body_recipe_body_selection(ctx, tools, scope, inputs, &mut body_candidates);
                    for cell in cells {
                        if edit_result.is_err() { break; }
                        edit_result = bind_direct_body_recipe_body_selection(ctx, cell, scope, inputs, &mut body_candidates);
                    }
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
                definition
            {
                let edit = operands
                    .try_edit(|target, tools| {
                        macro_rules! admitted {
                            ($value:expr) => {
                                match $value {
                                    Ok(value) => value,
                                    Err(error) => { edit_result = Err(error); return; }
                                }
                            };
                        }
                        let (Some(state_id), Some(previous_state_id)) =
                            (scope.history_state_id(), scope.previous_history_state_id())
                        else {
                            edit_result = bind_direct_body_recipe_body_selection(ctx, target, scope, inputs, &mut body_candidates);
                            if edit_result.is_err() { return; }
                            edit_result = bind_direct_body_recipe_body_selection(ctx, tools, scope, inputs, &mut body_candidates);
                            if edit_result.is_err() { return; }
                            if matches!(
                                tools,
                                BodySelection::Native(_) | BodySelection::NativeSet(_)
                            ) {
                                if let Some(local) = admitted!(combine_external_local_tools(ctx, scope)) {
                                    *tools = local;
                                }
                            }
                            return;
                        };
                        if let Some(local) = admitted!(combine_external_local_tools(ctx, scope)) {
                            *tools = local;
                        }
                        let BodySelection::Native(native) = target else {
                            return;
                        };
                        let Some((history, state, _)) =
                            unique_history_state_pair(histories, state_id, previous_state_id)
                        else {
                            return;
                        };
                        let history_states = match unique_feature_history_states(ctx, history) {
                            Ok(states) => states,
                            Err(error) => { edit_result = Err(error); return; }
                        };
                        let body = match singleton_revised_input_body_across_state_chain(
                            ctx,
                            state,
                            previous_state_id,
                            &history_states,
                        ) {
                            Ok(Some(body)) => body,
                            Ok(None) => return,
                            Err(error) => { edit_result = Err(error); return; }
                        };
                        let input_state = admitted!(crate::ids::history_input_state_id_charged(
                            ctx, feature_id, previous_state_id));
                        let body_id = admitted!(crate::ids::history_input_body_id_charged(
                            ctx, feature_id, previous_state_id, body));
                        let native_id = admitted!(ctx.copy_retained_text(native, "copy F3D Combine target identity"));
                        let mut target_bodies = admitted!(ctx.collection_vec(1, "validate F3D Combine target body"));
                        target_bodies.push(body_id);
                        if let Ok(historical) = admitted!(cadmpeg_ir::features::BodySelection::historical(admitted!(crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id)), target_bodies, native_id, ctx).map_err(cadmpeg_core::CodecError::from)) {
                            *target = historical;
                        }
                        let Some(stream) = crate::ids::native_stream(&scope.id) else {
                            return;
                        };
                        let Some(operation) = scope.combine_operation() else {
                            return;
                        };
                        let mut native_tools = Vec::new();
                        for tool in operation.tools.iter() {
                            let native = admitted!(ctx.format_retained(format_args!("{stream}:design-record#{}", tool.record_index), "retain F3D Combine tool identity"));
                            admitted!(ctx.reserve_vec(&mut native_tools, 1, "collect F3D Combine tool identities"));
                            native_tools.push(native);
                        }
                        let current_history_source = historical_brep_source(&state.id);
                        let mut historical_tool_rows = Vec::new();
                        let mut direct_tool_rows = Vec::new();
                        for (record_index, native) in operation.tools.iter()
                            .map(|tool| tool.record_index).zip(&native_tools) {
                            let mut matching = body_recipe_operands.iter().filter(|operand| {
                                crate::ids::native_stream(&operand.id) == Some(stream)
                                    && operand.scope_record_index == scope.record_index
                                    && matches!(
                            operand.owner,
                            crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
                        ) && operand.record_index() == record_index
                            });
                            let Some(operand) = matching.next() else {
                                historical_tool_rows.clear();
                                direct_tool_rows.clear();
                                break;
                            };
                            if matching.next().is_some() {
                                historical_tool_rows.clear();
                                direct_tool_rows.clear();
                                break;
                            }
                            if let Some(body) = operand.resolved_body_slot {
                                let body = admitted!(crate::ids::history_input_body_id_charged(
                                    ctx, feature_id, previous_state_id, body));
                                let repeated = historical_tool_rows
                                    .iter()
                                    .any(|row: &BodyMember<_>| row.body() == &body);
                                let row = body_member(body, admitted!(ctx.copy_retained_text(native, "copy F3D Combine tool member identity")));
                                let (false, Some(row)) = (repeated, row) else {
                                    historical_tool_rows.clear();
                                    direct_tool_rows.clear();
                                    break;
                                };
                                admitted!(ctx.reserve_vec(&mut historical_tool_rows, 1, "collect F3D Combine historical tool rows"));
                                historical_tool_rows.push(row);
                                continue;
                            }
                            let body = match body_candidates.external(ctx, operand, current_history_source) {
                                Ok(Some(body)) => body,
                                Ok(None) => {
                                    historical_tool_rows.clear();
                                    direct_tool_rows.clear();
                                    break;
                                }
                                Err(error) => {
                                    edit_result = Err(error);
                                    return;
                                }
                            };
                            let repeated = direct_tool_rows
                                .iter()
                                .any(|row: &BodyMember<_>| row.body() == &body);
                            let row = body_member(body, admitted!(ctx.copy_retained_text(native, "copy F3D Combine direct tool member identity")));
                            let (false, Some(row)) = (repeated, row) else {
                                historical_tool_rows.clear();
                                direct_tool_rows.clear();
                                break;
                            };
                            admitted!(ctx.reserve_vec(&mut direct_tool_rows, 1, "collect F3D Combine direct tool rows"));
                            direct_tool_rows.push(row);
                        }
                        if historical_tool_rows.len() == native_tools.len() {
                            let Ok(members) = admitted!(cadmpeg_ir::features::BodyMembers::try_from_rows(
                                historical_tool_rows, ctx).map_err(cadmpeg_core::CodecError::from)) else {
                                return;
                            };
                            *tools = BodySelection::HistoricalSet {
                                state: input_state,
                                members,
                            };
                        } else if direct_tool_rows.len() == native_tools.len() {
                            *tools = if direct_tool_rows.len() == 1 {
                                let Some(row) = direct_tool_rows.pop() else { return; };
                                let (body, native) = row.into_parts();
                                let mut selected = admitted!(ctx.collection_vec(1, "validate F3D Combine resolved body"));
                                selected.push(body);
                                let bodies = match cadmpeg_ir::features::DistinctMembers::try_from(selected, ctx) {
    Ok(value) => value,
    Err(cadmpeg_ir::features::FeatureCollectionError::Invalid(_)) => return,
    Err(cadmpeg_ir::features::FeatureCollectionError::Resource(limit)) => { edit_result = Err(limit.into()); return; },
};
                                BodySelection::Resolved {
                                    bodies,
                                    native,
                                }
                            } else {
                                let Ok(members) = admitted!(cadmpeg_ir::features::BodyMembers::try_from_rows(
                                    direct_tool_rows, ctx).map_err(cadmpeg_core::CodecError::from)) else {
                                    return;
                                };
                                BodySelection::ResolvedSet { members }
                            };
                        } else {
                            let tool_record_indices = admitted!(ctx.collect_vec(operation.tools.iter().map(|tool| tool.record_index), "collect F3D Combine tool record indices"));
                            if let Some(tool_slots) = admitted!(combine_recipe_family_tool_slots(
                                ctx,
                                (stream, scope.record_index),
                                &tool_record_indices,
                                previous_state_id,
                                body,
                                body_recipe_operands,
                                inputs.construction_recipes,
                            )) {
                                if tool_slots.len() != native_tools.len() {
                                    return;
                                }
                                let Some(rows) = admitted!(combine_historical_rows(
                                    ctx, feature_id, previous_state_id, tool_slots,
                                    native_tools)) else {
                                    return;
                                };
                                let Ok(members) =
                                    admitted!(cadmpeg_ir::features::BodyMembers::try_from_rows(rows, ctx).map_err(cadmpeg_core::CodecError::from))
                                else {
                                    return;
                                };
                                *tools = BodySelection::HistoricalSet {
                                    state: input_state,
                                    members,
                                };
                                return;
                            }
                            let mut dependency_sets = dependencies.iter()
                                .filter_map(|dependency| pattern_body_slots.get(dependency));
                            let pattern_bodies = dependency_sets.next();
                            if dependency_sets.next().is_none() {
                              if let Some(pattern_bodies) = pattern_bodies {
                                if let Some(tool_slots) = admitted!(pattern_combine_tool_slots(
                                    ctx,
                                    pattern_bodies,
                                    body,
                                    native_tools.len(),
                                )) {
                                    if tool_slots.len() != native_tools.len() {
                                        return;
                                    }
                                    let Some(rows) = admitted!(combine_historical_rows(
                                        ctx, feature_id, previous_state_id, tool_slots,
                                        native_tools)) else {
                                        return;
                                    };
                                    let Ok(members) =
                                        admitted!(cadmpeg_ir::features::BodyMembers::try_from_rows(rows, ctx).map_err(cadmpeg_core::CodecError::from))
                                    else {
                                        return;
                                    };
                                    *tools = BodySelection::HistoricalSet {
                                        state: input_state,
                                        members,
                                    };
                                }
                              }
                            }
                        }
                    });
                if let Err(error) = edit {
                    edit_result = Err(cadmpeg_core::CodecError::malformed(error));
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::Coil {
                result: cadmpeg_ir::features::CoilResult::Boolean { targets, .. },
                ..
            }) = definition
            {
                if let Some(previous_state_id) = scope.previous_history_state_id() {
                    edit_result = bind_body_recipe_body_selection(
                        ctx,
                        targets,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                } else {
                    edit_result = bind_direct_body_recipe_body_selection(ctx, targets, scope, inputs, &mut body_candidates);
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::DeleteBody { bodies, .. }) =
                definition
            {
                if let Some(previous_state_id) = scope.previous_history_state_id() {
                    edit_result = bind_body_recipe_body_selection(
                        ctx,
                        bodies,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                } else {
                    edit_result = bind_direct_body_recipe_body_selection(ctx, bodies, scope, inputs, &mut body_candidates);
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::Scale { bodies, .. }) =
                definition
            {
                if let Some(previous_state_id) = scope.previous_history_state_id() {
                    edit_result = bind_body_recipe_body_selection(
                        ctx,
                        bodies,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                    if edit_result.is_ok() && matches!(bodies, BodySelection::Native(_)) {
                        edit_result = bind_direct_body_recipe_body_selection(ctx, bodies, scope, inputs, &mut body_candidates);
                    }
                } else {
                    edit_result = bind_direct_body_recipe_body_selection(ctx, bodies, scope, inputs, &mut body_candidates);
                }
                break 'feature_edit;
            }
            let (bodies, proof) = match definition {
                FeatureDefinition::Operation(FeatureOperation::MoveBody { bodies, .. }) => {
                    (bodies, BodySelectionProof::TopologyStableRevision)
                }
                FeatureDefinition::Operation(FeatureOperation::Shell {
                    bodies: Some(bodies),
                    ..
                }) => (bodies, BodySelectionProof::RevisedInput),
                FeatureDefinition::Operation(FeatureOperation::SplitBody { targets, .. }) => {
                    (targets, BodySelectionProof::RevisedInput)
                }
                _ => break 'feature_edit,
            };
            let BodySelection::Native(group_id) = bodies else {
                break 'feature_edit;
            };
            let mut matching_groups = groups.iter().filter(|group| {
                group.id == *group_id
                    && group.scope_record_index == scope.record_index
                    && group.role() == DesignOperandRole::BODIES_A
                    && crate::ids::native_stream(&group.id) == crate::ids::native_stream(&scope.id)
            });
            let Some(group) = matching_groups.next() else {
                break 'feature_edit;
            };
            if matching_groups.next().is_some() || group.members().len() != 1 {
                break 'feature_edit;
            }
            let (Some(state_id), Some(previous_state_id)) =
                (scope.history_state_id(), scope.previous_history_state_id())
            else {
                edit_result = bind_direct_body_recipe_body_selection(ctx, bodies, scope, inputs, &mut body_candidates);
                break 'feature_edit;
            };
            let Some((history, state, _previous)) =
                unique_history_state_pair(histories, state_id, previous_state_id)
            else {
                edit_result = bind_direct_body_recipe_body_selection(ctx, bodies, scope, inputs, &mut body_candidates);
                break 'feature_edit;
            };
            let history_states = match unique_feature_history_states(ctx, history) {
                Ok(states) => states,
                Err(error) => { edit_result = Err(error); break 'feature_edit; }
            };
            let body = match proof {
                BodySelectionProof::TopologyStableRevision => {
                    singleton_body_revision_across_state_chain(
                        ctx,
                        state,
                        previous_state_id,
                        &history_states,
                    )
                }
                BodySelectionProof::RevisedInput => {
                    singleton_revised_input_body_across_state_chain(
                        ctx,
                        state,
                        previous_state_id,
                        &history_states,
                    )
                }
            };
            let body = match body {
                Ok(body) => body,
                Err(error) => { edit_result = Err(error); break 'feature_edit; }
            };
            let Some(body) = body else {
                break 'feature_edit;
            };
            let state_id = crate::ids::history_input_state_id_charged(
                ctx, feature_id, previous_state_id);
            let body_id = crate::ids::history_input_body_id_charged(
                ctx, feature_id, previous_state_id, body);
            let native = ctx.copy_retained_text(group_id, "copy F3D pattern body group identity");
            let (state_id, body_id, native) = match (state_id, body_id, native) {
                (Ok(state_id), Ok(body_id), Ok(native)) => (state_id, body_id, native),
                (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => {
                    edit_result = Err(error);
                    break 'feature_edit;
                }
            };
            let mut selected = match ctx.collection_vec(1, "validate F3D pattern body selection") {
                Ok(selected) => selected,
                Err(error) => {
                    edit_result = Err(error);
                    break 'feature_edit;
                }
            };
            selected.push(body_id);
            match cadmpeg_ir::features::BodySelection::historical(state_id, selected, native, ctx) {
                Ok(Ok(historical)) => *bodies = historical,
                Ok(Err(_)) => {},
                Err(limit) => { edit_result = Err(limit.into()); break 'feature_edit; },
            }
        }
        });
        edit_result?;
    }

    Ok(())
}

fn combine_historical_rows(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    slots: Vec<i64>,
    native_tools: Vec<String>,
) -> Result<
    Option<Vec<cadmpeg_ir::features::BodyMember<cadmpeg_ir::ids::HistoricalBodyId>>>,
    cadmpeg_core::CodecError,
> {
    let mut rows = Vec::new();
    for (slot, native) in slots.into_iter().zip(native_tools) {
        let body =
            crate::ids::history_input_body_id_charged(ctx, feature_id, previous_state_id, slot)?;
        let Some(row) = body_member(body, native) else {
            return Ok(None);
        };

        ctx.reserve_vec(&mut rows, 1, "collect F3D Combine fallback rows")?;
        rows.push(row);
    }
    Ok(Some(rows))
}

fn pattern_combine_tool_slots(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    pattern_bodies: &BTreeSet<i64>,
    target_body: i64,
    native_tool_count: usize,
) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError> {
    if !pattern_bodies.contains(&target_body)
        || pattern_bodies.len().checked_sub(1) != Some(native_tool_count)
    {
        return Ok(None);
    }
    Ok(Some(
        ctx.collect_vec(
            pattern_bodies
                .iter()
                .copied()
                .filter(|body| *body != target_body),
            "collect F3D pattern Combine tools",
        )?,
    ))
}

fn combine_recipe_family_tool_slots(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope_key: (&str, u32),
    tool_record_indices: &[u32],
    previous_state_id: i64,
    target_body: i64,
    operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    recipes: &[crate::records::recipes::ConstructionRecipe],
) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError> {
    macro_rules! required {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    type FamilyKey<'a> = (
        &'a crate::records::mesh::DesignRelaxedGuidText,
        &'a crate::records::mesh::DesignRelaxedGuidText,
        u64,
        u32,
        &'a str,
    );
    type FamilyMember<'a> = (u32, Option<i64>, &'a [i64]);
    let (stream, scope_record_index) = scope_key;

    if tool_record_indices.is_empty() {
        return Ok(None);
    }
    let mut seen = HashSet::new();
    for index in tool_record_indices {
        if !ctx.insert_hash_set(&mut seen, *index, "index F3D Combine tool record indices")? {
            return Ok(None);
        }
    }
    let mut recipes_by_id =
        HashMap::<&str, Option<&crate::records::recipes::ConstructionRecipe>>::new();
    for recipe in recipes.iter().filter(|recipe| {
        recipe.kind == crate::records::recipes::ConstructionRecipeKind::Body
            && crate::ids::native_stream(&recipe.id) == Some(stream)
    }) {
        if !recipes_by_id.contains_key(recipe.id.as_str()) {
            ctx.reserve_map(&mut recipes_by_id, 1, "index F3D Combine recipes")?;
        }
        recipes_by_id
            .entry(recipe.id.as_str())
            .and_modify(|entry| *entry = None)
            .or_insert(Some(recipe));
    }
    let mut families = BTreeMap::<FamilyKey<'_>, Vec<FamilyMember<'_>>>::new();
    for record_index in tool_record_indices {
        let mut matching = operands.iter().filter(|operand| {
            crate::ids::native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope_record_index
                && matches!(
                    operand.owner,
                    crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
                )
                && operand.record_index() == *record_index
        });
        let operand = required!(matching.next());
        if matching.next().is_some() || operand.references().len() != 1 {
            return Ok(None);
        }
        let recipe = required!(required!(recipes_by_id.get(operand.recipe_id.as_str())).as_ref());
        let design = required!(recipe.design.as_ref());
        let selector = required!(design.selector).value;
        if selector == 0 {
            return Ok(None);
        }
        let resolved = match (operand.resolved_body_state_id, operand.resolved_body_slot) {
            (Some(state), Some(body)) if state == previous_state_id => Some(body),
            (None, None) => None,
            _ => return Ok(None),
        };
        let reference = &operand.references()[0];
        let key = (
            &operand.asset_id,
            &operand.context_id,
            reference.design_reference,
            reference.form,
            design.id.value.as_str(),
        );
        ctx.push_btree_group(
            &mut families,
            key,
            (
                selector,
                resolved,
                reference.preceding_body_slots.as_slice(),
            ),
            "index F3D Combine tool families",
            "collect F3D Combine family members",
        )?;
    }

    let mut selected = BTreeSet::new();
    for family in families.into_values() {
        let mut exact = BTreeSet::new();
        for body in family.iter().filter_map(|(_, body, _)| *body) {
            ctx.insert_btree_set(&mut exact, body, "index F3D Combine exact tool bodies")
                .map(|_| ())?;
        }
        if family.iter().all(|(_, body, _)| body.is_some()) {
            if exact.len() != family.len() {
                return Ok(None);
            }
            for body in exact {
                ctx.insert_btree_set(&mut selected, body, "index F3D Combine selected tools")
                    .map(|_| ())?;
            }
            continue;
        }
        let mut selectors = BTreeSet::new();
        for (selector, _, _) in &family {
            ctx.insert_btree_set(&mut selectors, *selector, "index F3D Combine selectors")?;
        }
        let count = required!(u32::try_from(family.len()).ok());
        if selectors.len() != family.len()
            || !(1..=count).all(|selector| selectors.contains(&selector))
        {
            return Ok(None);
        }
        let mut candidate_sets = family
            .iter()
            .map(|(_, _, candidates)| candidates)
            .filter(|candidates| !candidates.is_empty());
        let candidate_slice = required!(candidate_sets.next());
        let mut candidates = BTreeSet::new();
        for candidate in *candidate_slice {
            ctx.insert_btree_set(
                &mut candidates,
                *candidate,
                "index F3D Combine candidate tools",
            )
            .map(|_| ())?;
        }
        if candidates.len() != family.len()
            || candidates.contains(&target_body)
            || !exact.is_subset(&candidates)
            || candidate_sets.any(|other| {
                other.iter().any(|body| !candidates.contains(body))
                    || candidates.iter().any(|body| !other.contains(body))
            })
        {
            return Ok(None);
        }
        for body in candidates {
            ctx.insert_btree_set(&mut selected, body, "index F3D Combine selected tools")
                .map(|_| ())?;
        }
    }
    if selected.len() != tool_record_indices.len() || selected.contains(&target_body) {
        return Ok(None);
    }
    Ok(Some(ctx.collect_vec(
        selected,
        "collect F3D Combine recipe tool slots",
    )?))
}

fn combine_external_local_tools(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Result<Option<cadmpeg_ir::features::BodySelection>, cadmpeg_core::CodecError> {
    let Some(operation) = scope.combine_operation() else {
        return Ok(None);
    };
    let mut bodies = Vec::new();
    for tool in operation.tools.iter() {
        let Some(identity) = tool.external_identity.as_ref() else {
            return Ok(None);
        };
        let id = crate::ids::neutral_combine_external_body_id_charged(ctx, identity)?;

        ctx.reserve_vec(&mut bodies, 1, "collect F3D Combine external tools")?;
        ctx.charge_work(
            u64::try_from(bodies.len()).map_err(|_| {
                ctx.refuse_codec_limit("compare F3D Combine external tools", 0, u64::MAX)
            })?,
            "compare F3D Combine external tools",
        )?;
        if bodies.iter().any(|body| body == &id) {
            return Ok(None);
        }
        bodies.push(id);
    }
    let native = ctx.copy_retained_text(&scope.id, "copy F3D Combine scope identity")?;
    Ok(cadmpeg_ir::features::BodySelection::local(bodies, native, ctx)?.ok())
}

fn historical_body_slot(id: &str) -> Option<i64> {
    id.strip_prefix("f3d:history-input:body#")?
        .rsplit_once(':')?
        .1
        .parse()
        .ok()
}

fn bind_pattern_body_selections<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    inputs: &FeatureBodySelectionInputs<'a>,
    body_candidates: &mut BodyCandidates<'a>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{
        patterns::PatternSeed, BodySelection, FeatureDefinition, FeatureOperation,
    };

    let scopes = inputs.scopes;
    let groups = inputs.groups;
    let body_recipe_operands = inputs.body_recipe_operands;

    for feature in features {
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let stream = crate::ids::native_stream(&scope.id);
        let mut matching_groups = groups.iter().filter(|group| {
            group.scope_record_index == scope.record_index
                && group.role() == DesignOperandRole::BODIES_B
                && !group.members().is_empty()
                && crate::ids::native_stream(&group.id) == stream
        });
        let Some(group) = matching_groups.next() else {
            continue;
        };
        if matching_groups.next().is_some() {
            continue;
        }
        let mut seed = if seeds.is_empty() {
            Some(PatternSeed::Bodies(BodySelection::Native(
                ctx.copy_retained_text(&group.id, "copy F3D pattern seed native identity")?,
            )))
        } else {
            None
        };
        let feature_id = &feature.id;
        let mut reserve_error = None;
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = definition
            else {
                return;
            };
            if let Some(seed) = seed.take() {
                if let Err(error) = ctx.push_vec(seeds, seed, "collect F3D pattern body seeds") {
                    reserve_error = Some(error);
                    return;
                }
            }
            let [PatternSeed::Bodies(selection)] = seeds.as_mut_slice() else {
                return;
            };
            if let Some(previous_state_id) = scope.previous_history_state_id() {
                reserve_error = bind_body_recipe_body_selection(
                    ctx,
                    selection,
                    feature_id,
                    previous_state_id,
                    scope,
                    groups,
                    body_recipe_operands,
                )
                .err();
            } else {
                reserve_error = bind_direct_body_recipe_body_selection(
                    ctx,
                    selection,
                    scope,
                    inputs,
                    body_candidates,
                )
                .err();
            }
        });
        if let Some(error) = reserve_error {
            return Err(error);
        }
    }
    Ok(())
}

fn bind_body_recipe_body_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::BodySelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::BodySelection;

    let BodySelection::Native(group_id) = selection else {
        return Ok(());
    };
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = groups.iter().filter(|group| {
        group.id == *group_id
            && group.scope_record_index == scope.record_index
            && matches!(
                group.role(),
                DesignOperandRole::BODIES_A
                    | DesignOperandRole::ROLE_0X5
                    | DesignOperandRole::BODIES_B
            )
            && crate::ids::native_stream(&group.id) == stream
    });
    let Some(group) = matching_groups.next() else {
        return Ok(());
    };
    if matching_groups.next().is_some() || group.members().is_empty() {
        return Ok(());
    }
    let mut body_slots = Vec::new();
    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| member.value)
        .enumerate()
    {
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return Ok(());
        };
        let mut matching_operands = operands.iter().filter(|operand| {
            operand.owner.group() == Some((group.record_index, ordinal))
                && operand.record_index() == record_index
                && crate::ids::native_stream(&operand.id) == stream
        });
        let Some(operand) = matching_operands.next() else {
            return Ok(());
        };
        if matching_operands.next().is_some() {
            return Ok(());
        }
        let Some(body_slot) = operand.resolved_body_slot else {
            return Ok(());
        };
        if !body_slots.contains(&body_slot) {
            ctx.reserve_vec(&mut body_slots, 1, "collect F3D body recipe slots")?;
            body_slots.push(body_slot);
        }
    }

    let mut body_ids = Vec::new();
    ctx.reserve_vec(
        &mut body_ids,
        body_slots.len(),
        "collect F3D body recipe identities",
    )?;
    for slot in body_slots {
        body_ids.push(crate::ids::history_input_body_id_charged(
            ctx,
            feature_id,
            previous_state_id,
            slot,
        )?);
    }
    let state = crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id)?;
    let native = ctx.copy_retained_text(&group.id, "copy F3D body recipe group identity")?;
    if let Ok(historical) =
        cadmpeg_ir::features::BodySelection::historical(state, body_ids, native, ctx)?
    {
        *selection = historical;
    }
    Ok(())
}

fn bind_direct_body_recipe_body_selection<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::BodySelection,
    scope: &crate::records::feature::scope::DesignParameterScope,
    inputs: &FeatureBodySelectionInputs<'a>,
    body_candidates: &mut BodyCandidates<'a>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{BodyMember, BodySelection};

    let groups = inputs.groups;
    let operands = inputs.body_recipe_operands;

    let stream = crate::ids::native_stream(&scope.id);
    let native_members = match selection {
        BodySelection::Native(group_id) => {
            let mut matching_groups = groups.iter().filter(|group| {
                group.id == *group_id
                    && group.scope_record_index == scope.record_index
                    && matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::ROLE_0X5
                            | DesignOperandRole::BODIES_B
                    )
                    && crate::ids::native_stream(&group.id) == stream
            });
            let Some(group) = matching_groups.next() else {
                return Ok(());
            };
            if matching_groups.next().is_some() || group.members().is_empty() {
                return Ok(());
            }
            let mut selected = Vec::new();
            for (ordinal, record_index) in group
                .members()
                .iter()
                .map(|member| member.value)
                .enumerate()
            {
                let Ok(ordinal) = u32::try_from(ordinal) else {
                    return Ok(());
                };
                let mut matching_operands = operands.iter().filter(|operand| {
                    operand.owner.group() == Some((group.record_index, ordinal))
                        && operand.record_index() == record_index
                        && crate::ids::native_stream(&operand.id) == stream
                });
                let Some(operand) = matching_operands.next() else {
                    return Ok(());
                };
                if matching_operands.next().is_some() {
                    return Ok(());
                }
                let Some(body) = body_candidates.direct(ctx, operand)? else {
                    return Ok(());
                };
                if selected.contains(&body) {
                    return Ok(());
                }

                ctx.reserve_vec(
                    &mut selected,
                    1,
                    "collect F3D direct body recipe selections",
                )?;
                selected.push(body);
            }
            let bodies = match cadmpeg_ir::features::DistinctMembers::try_from(selected, ctx) {
                Ok(value) => value,
                Err(cadmpeg_ir::features::FeatureCollectionError::Invalid(_)) => return Ok(()),
                Err(cadmpeg_ir::features::FeatureCollectionError::Resource(limit)) => {
                    return Err(limit.into())
                }
            };
            let native =
                ctx.copy_retained_text(&group.id, "copy F3D direct body recipe group identity")?;
            *selection = BodySelection::Resolved { bodies, native };
            return Ok(());
        }
        BodySelection::NativeSet(native) => ctx.collect_vec(
            native.iter().map(String::as_str),
            "collect F3D direct body recipe native members",
        )?,
        _ => return Ok(()),
    };
    if native_members.is_empty() {
        return Ok(());
    }
    for (index, native) in native_members.iter().enumerate() {
        for previous in &native_members[..index] {
            ctx.charge_work(1, "validate F3D direct body recipe native members")?;
            if native == previous {
                return Ok(());
            }
        }
    }
    let mut rows = Vec::new();
    for native in &native_members {
        let Some((native_stream_name, record_index)) = native.rsplit_once(":design-record#") else {
            return Ok(());
        };
        let Ok(record_index) = record_index.parse::<u32>() else {
            return Ok(());
        };
        if Some(native_stream_name) != stream {
            return Ok(());
        }
        let mut matching_operands = operands.iter().filter(|operand| {
            crate::ids::native_stream(&operand.id) == Some(native_stream_name)
                && operand.scope_record_index == scope.record_index
                && matches!(
                    operand.owner,
                    crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
                )
                && operand.record_index() == record_index
        });
        let Some(operand) = matching_operands.next() else {
            return Ok(());
        };
        if matching_operands.next().is_some() {
            return Ok(());
        }
        let Some(body) = body_candidates.direct(ctx, operand)? else {
            return Ok(());
        };
        if rows.iter().any(|row: &BodyMember<_>| row.body() == &body) {
            return Ok(());
        }
        let native =
            ctx.copy_retained_text(native, "copy F3D direct body recipe member identity")?;
        let Some(row) = body_member(body, native) else {
            return Ok(());
        };

        ctx.reserve_vec(&mut rows, 1, "collect F3D direct body recipe rows")?;
        rows.push(row);
    }
    if let Ok(members) = cadmpeg_ir::features::BodyMembers::try_from_rows(rows, ctx)? {
        *selection = BodySelection::ResolvedSet { members };
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodySelectionProof {
    TopologyStableRevision,
    RevisedInput,
}

fn unique_feature_history_states<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &'a AsmHistory,
) -> Result<HashMap<i64, Option<&'a AsmDeltaState>>, cadmpeg_core::CodecError> {
    let mut states = HashMap::new();
    for state in &history.states {
        ctx.charge_work(1, "index F3D feature history states")?;
        if !states.contains_key(&state.state_id) {
            ctx.reserve_map(&mut states, 1, "index F3D feature history states")?;
        }
        states
            .entry(state.state_id)
            .and_modify(|existing| *existing = None)
            .or_insert(Some(state));
    }
    Ok(states)
}

fn singleton_revised_input_body_across_state_chain<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let mut current = state;
    let mut visited = HashSet::new();
    let mut revised = BTreeSet::new();
    while current.state_id != previous_state_id {
        if !ctx.insert_hash_set(
            &mut visited,
            current.state_id,
            "visit F3D revised input body states",
        )? {
            return Ok(None);
        }
        let Some(transition) = current.transition.as_ref() else {
            return Ok(None);
        };
        for &body in transition
            .topology
            .bodies
            .updated
            .iter()
            .chain(&transition.topology.bodies.deleted)
        {
            ctx.insert_btree_set(&mut revised, body, "index F3D revised input bodies")
                .map(|_| ())?;
        }
        let Some(previous_id) = transition.previous_state_id else {
            return Ok(None);
        };
        let Some(Some(previous)) = states.get(&previous_id) else {
            return Ok(None);
        };
        current = previous;
    }
    let Some(input) = current.topology() else {
        return Ok(None);
    };
    let mut candidates = input.bodies.iter().filter(|body| revised.contains(body));
    let Some(body) = candidates.next().copied() else {
        return Ok(None);
    };
    Ok(candidates.next().is_none().then_some(body))
}

fn singleton_body_revision_across_state_chain<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(result_topology) = state.topology() else {
        return Ok(None);
    };
    let mut current = state;
    let mut visited = HashSet::new();
    let mut selected = None;
    while current.state_id != previous_state_id {
        if !ctx.insert_hash_set(
            &mut visited,
            current.state_id,
            "visit F3D stable body revision states",
        )? {
            return Ok(None);
        }
        let Some(revision) = body_revision_without_topology_change(current) else {
            return Ok(None);
        };
        if let TopologyStableBodyRevision::Revised(body) = revision {
            match selected {
                None => selected = Some(body),
                Some(selected) if selected == body => {}
                Some(_) => return Ok(None),
            }
        }
        let Some(previous) = current
            .transition
            .as_ref()
            .and_then(|transition| transition.previous_state_id)
        else {
            return Ok(None);
        };
        let Some(Some(previous)) = states.get(&previous) else {
            return Ok(None);
        };
        current = previous;
    }
    let Some(body) = selected else {
        return Ok(None);
    };
    let Some(input_topology) = current.topology() else {
        return Ok(None);
    };
    Ok(
        (result_topology.bodies.contains(&body) && input_topology.bodies.contains(&body))
            .then_some(body),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TopologyStableBodyRevision {
    Unchanged,
    Revised(i64),
}

fn body_revision_without_topology_change(
    current: &AsmDeltaState,
) -> Option<TopologyStableBodyRevision> {
    let transition = current.transition.as_ref()?;
    let delta = &transition.topology;
    let body = match delta.bodies.updated.as_slice() {
        [] => TopologyStableBodyRevision::Unchanged,
        [body] => TopologyStableBodyRevision::Revised(*body),
        _ => return None,
    };
    if !delta.bodies.inserted.is_empty()
        || !delta.bodies.deleted.is_empty()
        || [
            &delta.regions,
            &delta.shells,
            &delta.faces,
            &delta.loops,
            &delta.coedges,
            &delta.edges,
            &delta.vertices,
        ]
        .into_iter()
        .any(|family| {
            !family.inserted.is_empty() || !family.deleted.is_empty() || !family.updated.is_empty()
        })
    {
        return None;
    }
    Some(body)
}

#[derive(Clone, Copy)]
pub(crate) struct FeatureFaceSelectionInputs<'a> {
    pub(crate) scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    pub(crate) groups:
        &'a [crate::records::topology::construction::DesignConstructionOperandGroup],
    pub(crate) operands: &'a [crate::records::topology::face::DesignFaceOperand],
    pub(crate) entity_operands:
        &'a [crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    pub(crate) body_recipe_operands:
        &'a [crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    pub(crate) histories: &'a [AsmHistory],
}

pub(crate) fn bind_feature_face_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    input_topologies: &mut [cadmpeg_ir::features::FeatureInputTopology],
    input: FeatureFaceSelectionInputs<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let FeatureFaceSelectionInputs {
        scopes,
        groups,
        operands,
        entity_operands,
        body_recipe_operands,
        histories,
    } = input;
    for feature in features {
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| {
            macro_rules! admitted {
                ($value:expr) => {
                    if let Err(error) = $value {
                        edit_result = Err(error);
                        return;
                    }
                };
            }
            'feature_edit: {
                let Some(native_ref) = native_ref else {
                    break 'feature_edit;
                };
                let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
                let Some(scope) = matching_scopes.next() else {
                    break 'feature_edit;
                };
                if matching_scopes.next().is_some() {
                    break 'feature_edit;
                }
                let Some(state_id) = scope.history_state_id() else {
                    break 'feature_edit;
                };
                let Some(previous_state_id) =
                    effective_scope_previous_history_state_id(scope, histories)
                else {
                    break 'feature_edit;
                };
                let Some((history, state, previous)) =
                    unique_history_state_pair(histories, state_id, previous_state_id)
                else {
                    break 'feature_edit;
                };
                let Some(transition) = &state.transition else {
                    break 'feature_edit;
                };
                if transition.previous_state_id != Some(previous_state_id) {
                    break 'feature_edit;
                }
                let Some(_topology) = previous.topology() else {
                    break 'feature_edit;
                };
                match definition {
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Extrude { start, extent, .. },
                    ) => {
                        if let cadmpeg_ir::features::ExtrudeStart::FromFace { face, .. } = start {
                            admitted!(selection::bind_face_selection(
                                ctx,
                                face,
                                scope,
                                groups,
                                operands,
                                &transition.topology.faces.updated,
                            ));
                            admitted!(bind_entity_face_selection(
                                ctx,
                                face,
                                FaceSelectionBinding {
                                    feature_id,
                                    previous_state_id,
                                    operation_history_id: &history.id,
                                    scope,
                                    input_topologies: &mut *input_topologies
                                },
                                groups,
                                entity_operands,
                            ));
                        }
                        let sides = match extent {
                            cadmpeg_ir::features::ExtrudeExtent::OneSided { side }
                            | cadmpeg_ir::features::ExtrudeExtent::Symmetric { side } => vec![side],
                            cadmpeg_ir::features::ExtrudeExtent::TwoSided { first, second } => {
                                vec![first, second]
                            }
                        };
                        for side in sides {
                            if let cadmpeg_ir::features::LinearTermination::ToFace {
                                face, ..
                            } = &mut side.termination
                            {
                                admitted!(selection::bind_face_selection(
                                    ctx,
                                    face,
                                    scope,
                                    groups,
                                    operands,
                                    &transition.topology.faces.updated,
                                ));
                            }
                        }
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Pattern { seeds, .. },
                    ) => {
                        for seed in seeds {
                            let cadmpeg_ir::features::patterns::PatternSeed::Faces(faces) = seed
                            else {
                                continue;
                            };
                            admitted!(selection::bind_face_selection(
                                ctx,
                                faces,
                                scope,
                                groups,
                                operands,
                                &transition.topology.faces.updated,
                            ));
                            admitted!(bind_entity_face_selection(
                                ctx,
                                faces,
                                FaceSelectionBinding {
                                    feature_id,
                                    previous_state_id,
                                    operation_history_id: &history.id,
                                    scope,
                                    input_topologies: &mut *input_topologies
                                },
                                groups,
                                entity_operands,
                            ));
                        }
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::MoveFace { faces, .. },
                    ) => {
                        admitted!(selection::bind_face_selection(
                            ctx,
                            faces,
                            scope,
                            groups,
                            operands,
                            &transition.topology.faces.updated,
                        ));
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Thicken { faces, .. },
                    ) => {
                        admitted!(selection::bind_face_selection(
                            ctx,
                            faces,
                            scope,
                            groups,
                            operands,
                            &transition.topology.faces.updated,
                        ));
                        admitted!(selection::bind_body_recipe_face_selection(
                            ctx,
                            faces,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            body_recipe_operands,
                        ));
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::KnitSurface { faces, .. },
                    ) => {
                        admitted!(bind_surface_stitch_face_selection(
                            ctx,
                            faces,
                            FaceSelectionBinding {
                                feature_id,
                                previous_state_id,
                                operation_history_id: &history.id,
                                scope,
                                input_topologies: &mut *input_topologies
                            },
                            groups,
                            entity_operands,
                        ));
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::SplitFace { targets, .. },
                    ) => {
                        admitted!(selection::bind_face_selection(
                            ctx,
                            targets,
                            scope,
                            groups,
                            operands,
                            &transition.topology.faces.updated,
                        ));
                    }
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Hole {
                            face: Some(face), ..
                        },
                    ) => {
                        admitted!(bind_hole_face_selection(
                            ctx,
                            face,
                            FaceSelectionBinding {
                                feature_id,
                                previous_state_id,
                                operation_history_id: &history.id,
                                scope,
                                input_topologies: &mut *input_topologies
                            },
                        ));
                    }
                    _ => {}
                }
            }
        });
        edit_result?;
    }
    Ok(())
}

struct FaceSelectionBinding<'a> {
    feature_id: &'a cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    operation_history_id: &'a str,
    scope: &'a crate::records::feature::scope::DesignParameterScope,
    input_topologies: &'a mut [cadmpeg_ir::features::FeatureInputTopology],
}

fn bind_entity_face_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    input: FaceSelectionBinding<'_>,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::FaceSelection;
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;

    let group_id = match selection {
        FaceSelection::Native(group_id) => group_id.as_str(),
        _ => return Ok(()),
    };
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = groups.iter().filter(|group| {
        group.id == group_id
            && group.scope_record_index == scope.record_index
            && crate::ids::native_stream(&group.id) == stream
    });
    let Some(group) = matching_groups.next() else {
        return Ok(());
    };
    if matching_groups.next().is_some() || group.members().is_empty() {
        return Ok(());
    }
    bind_entity_face_groups(
        ctx,
        selection,
        &group.id,
        FaceSelectionBinding {
            feature_id,
            previous_state_id,
            operation_history_id,
            scope,
            input_topologies: &mut *input_topologies,
        },
        std::slice::from_ref(&group),
        operands,
    )
}

// Keep the serialized-selection context explicit at this boundary so every
// admission input remains visible to the strict all-members proof.
fn bind_surface_stitch_face_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    input: FaceSelectionBinding<'_>,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Result<(), cadmpeg_core::CodecError> {
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;
    let native_id = match selection {
        cadmpeg_ir::features::FaceSelection::Native(native_id) => native_id.as_str(),
        _ => return Ok(()),
    };
    if native_id != scope.id {
        return Ok(());
    }
    let Some(input_end) = scope.reference_members().len().checked_sub(2) else {
        return Ok(());
    };
    if input_end == 0 || !input_end.is_multiple_of(2) {
        return Ok(());
    }
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = ctx.collect_vec(
        groups.iter().filter(|group| {
            crate::ids::native_stream(&group.id) == stream
                && group.scope_record_index == scope.record_index
                && group.role() == DesignOperandRole::ROLE_0X5
                && group.extrude_role().is_none()
                && group.extrude_face_role().is_none()
        }),
        "collect F3D Stitch face groups",
    )?;
    ctx.stable_sort_by(
        &mut matching_groups,
        |a, b| a.scope_reference_ordinal.cmp(&b.scope_reference_ordinal),
        |_| 0,
        "sort F3D Stitch face groups",
    )?;
    if matching_groups.len().checked_mul(2) != Some(input_end)
        || matching_groups
            .iter()
            .enumerate()
            .zip(
                scope
                    .reference_members()
                    .values()
                    .step_by(2)
                    .zip(scope.reference_members().values().skip(1).step_by(2)),
            )
            .any(|((ordinal, group), (group_reference, member_reference))| {
                u32::try_from(ordinal * 2) != Ok(group.scope_reference_ordinal)
                    || group.record_index != *group_reference
                    || !group
                        .members()
                        .iter()
                        .map(|member| member.value)
                        .eq([*member_reference])
            })
    {
        return Ok(());
    }
    bind_entity_face_groups(
        ctx,
        selection,
        &scope.id,
        FaceSelectionBinding {
            feature_id,
            previous_state_id,
            operation_history_id,
            scope,
            input_topologies: &mut *input_topologies,
        },
        &matching_groups,
        operands,
    )
}

// Keep the shared selection-binding inputs explicit; this helper is the
// single admission point for both one-group and SurfaceStitch selections.
fn bind_entity_face_groups(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    native_id: &str,
    input: FaceSelectionBinding<'_>,
    groups: &[&crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Result<(), cadmpeg_core::CodecError> {
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;

    if groups.is_empty() {
        return Ok(());
    }
    let mut selected = Vec::<(&str, i64, bool)>::new();
    let stream = crate::ids::native_stream(&scope.id);
    for group in groups {
        if group.members().is_empty() {
            return Ok(());
        }
        for (ordinal, record_index) in group
            .members()
            .iter()
            .map(|member| member.value)
            .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return Ok(());
            };
            let mut matches = operands.iter().filter(|operand| {
                operand.scope_record_index == scope.record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == ordinal
                    && operand.record_index() == record_index
                    && crate::ids::native_stream(&operand.id) == stream
            });
            let Some(operand) = matches.next() else {
                return Ok(());
            };
            let [candidate] = operand.historical_face_candidates.as_slice() else {
                return Ok(());
            };
            if matches.next().is_some() {
                return Ok(());
            }
            let local = candidate.history_id == operation_history_id
                && candidate.historical.state_ids.contains(&previous_state_id);
            let Some(source) = historical_brep_source(&candidate.history_id) else {
                return Ok(());
            };
            if !selected.contains(&(source, candidate.face_slot, local)) {
                ctx.reserve_vec(&mut selected, 1, "collect F3D entity face candidates")?;
                selected.push((source, candidate.face_slot, local));
            }
        }
    }
    let state_id = crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id)?;
    let mut topologies = input_topologies
        .iter_mut()
        .filter(|topology| topology.id == state_id && topology.input_of == *feature_id);
    let Some(topology) = topologies.next() else {
        return Ok(());
    };
    if topologies.next().is_some() {
        return Ok(());
    }
    let mut faces = Vec::new();
    for (source, face, local) in selected {
        if !local && (source.contains('#') || source.chars().any(char::is_whitespace)) {
            return Ok(());
        }
        let id = if local {
            crate::ids::history_input_face_id_charged(ctx, feature_id, previous_state_id, face)?
        } else {
            crate::ids::history_input_face_id_charged(
                ctx,
                feature_id,
                previous_state_id,
                format_args!("{}:{source}:{face}", source.len()),
            )?
        };

        ctx.reserve_vec(&mut faces, 1, "collect F3D historical entity faces")?;
        faces.push(id);
    }
    for face in &faces {
        if !topology.faces.contains(face) {
            let retained =
                face.try_clone_for_decode(ctx, "copy F3D historical topology face identity")?;
            topology
                .faces
                .insert(ctx, retained, "index F3D historical entity faces")?;
        }
    }
    let native =
        ctx.copy_retained_text(native_id, "copy F3D historical face selection identity")?;
    if let Ok(historical) =
        cadmpeg_ir::features::FaceSelection::historical(state_id, faces, native, ctx)?
    {
        *selection = historical;
    }
    Ok(())
}

fn bind_hole_face_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    input: FaceSelectionBinding<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::FaceSelection;
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;

    let FaceSelection::Native(native_id) = selection else {
        return Ok(());
    };
    let Some(construction) = scope.hole_construction() else {
        return Ok(());
    };
    let Some(face_selection) = &construction.face_selection else {
        return Ok(());
    };
    let [candidate] = face_selection.historical_face_candidates.as_slice() else {
        return Ok(());
    };
    let local = candidate.history_id == operation_history_id
        && candidate.historical.state_ids.contains(&previous_state_id);
    let Some(source) = historical_brep_source(&candidate.history_id) else {
        return Ok(());
    };
    let state_id = crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id)?;
    let mut topologies = input_topologies
        .iter_mut()
        .filter(|topology| topology.id == state_id && topology.input_of == *feature_id);
    let Some(topology) = topologies.next() else {
        return Ok(());
    };
    if topologies.next().is_some() {
        return Ok(());
    }
    if !local && (source.contains('#') || source.chars().any(char::is_whitespace)) {
        return Ok(());
    }
    let face = if local {
        crate::ids::history_input_face_id_charged(
            ctx,
            feature_id,
            previous_state_id,
            candidate.face_slot,
        )?
    } else {
        crate::ids::history_input_face_id_charged(
            ctx,
            feature_id,
            previous_state_id,
            format_args!("{}:{source}:{}", source.len(), candidate.face_slot),
        )?
    };
    if !topology.faces.contains(&face) {
        let retained = face.try_clone_for_decode(ctx, "copy F3D historical hole topology face")?;
        topology
            .faces
            .insert(ctx, retained, "index F3D historical hole face")?;
    }
    let native = ctx.copy_retained_text(native_id, "copy F3D historical hole identity")?;
    let mut selected = ctx.collection_vec(1, "validate F3D historical hole face")?;
    selected.push(face);
    if let Ok(historical) =
        cadmpeg_ir::features::FaceSelection::historical(state_id, selected, native, ctx)?
    {
        *selection = historical;
    }
    Ok(())
}

pub(crate) fn bind_feature_path_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SurfaceBoundary};

    for feature in features {
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
        let mut edit_result = Ok(());
        feature.evaluation.edit(|definition, _| 'feature_edit: {
            let Some(native_ref) = native_ref else {
                break 'feature_edit;
            };
            let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
            let Some(scope) = matching_scopes.next() else {
                break 'feature_edit;
            };
            if matching_scopes.next().is_some() {
                break 'feature_edit;
            }
            let Some(previous_state_id) = scope.previous_history_state_id() else {
                break 'feature_edit;
            };
            match definition {
                FeatureDefinition::Operation(FeatureOperation::FilledSurface {
                    boundary: SurfaceBoundary::Path(path),
                    ..
                }) => {
                    edit_result = bind_entity_selection_path(
                        ctx,
                        path,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        operands,
                    );
                }
                FeatureDefinition::Operation(FeatureOperation::Loft { guidance, .. }) => {
                    let paths = match guidance {
                        cadmpeg_ir::features::LoftGuidance::Guides(paths) => paths,
                        cadmpeg_ir::features::LoftGuidance::Centerline(path) => {
                            std::slice::from_mut(path)
                        }
                    };
                    for path in paths {
                        edit_result = bind_entity_selection_path(
                            ctx,
                            path,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            operands,
                        );
                        if edit_result.is_err() {
                            break;
                        }
                    }
                }
                FeatureDefinition::Operation(FeatureOperation::Sweep {
                    path, guide_rail, ..
                }) => {
                    if let Some(path) = path {
                        edit_result = bind_entity_selection_path(
                            ctx,
                            path,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            operands,
                        );
                        if edit_result.is_err() {
                            break 'feature_edit;
                        }
                    }
                    if let Some(guide_rail) = guide_rail {
                        edit_result = bind_entity_selection_path(
                            ctx,
                            &mut guide_rail.path,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            operands,
                        );
                    }
                }
                _ => {}
            }
        });
        edit_result?;
    }
    Ok(())
}

fn bind_entity_selection_path(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    path: &mut cadmpeg_ir::features::PathRef,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::PathRef;

    let PathRef::Native(group_id) = path else {
        return Ok(());
    };
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = groups.iter().filter(|group| {
        group.id == *group_id
            && group.scope_record_index == scope.record_index
            && crate::ids::native_stream(&group.id) == stream
    });
    let Some(group) = matching_groups.next() else {
        return Ok(());
    };
    if matching_groups.next().is_some() || group.members().is_empty() {
        return Ok(());
    }

    let mut edge_slots = Vec::new();
    ctx.reserve_vec(
        &mut edge_slots,
        group.members().len(),
        "collect F3D path edge slots",
    )?;
    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| member.value)
        .enumerate()
    {
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return Ok(());
        };
        let mut matching_operands = operands.iter().filter(|operand| {
            operand.group_record_index == group.record_index
                && operand.group_member_ordinal == ordinal
                && operand.record_index() == record_index
                && crate::ids::native_stream(&operand.id) == stream
        });
        let Some(operand) = matching_operands.next() else {
            return Ok(());
        };
        if matching_operands.next().is_some() {
            return Ok(());
        }
        let Some(edge_slot) = operand.resolved_edge_slot else {
            return Ok(());
        };
        edge_slots.push(edge_slot);
    }

    let mut edge_ids = Vec::new();
    ctx.reserve_vec(
        &mut edge_ids,
        edge_slots.len(),
        "collect F3D path edge identities",
    )?;
    for slot in edge_slots {
        edge_ids.push(crate::ids::history_input_edge_id_charged(
            ctx,
            feature_id,
            previous_state_id,
            slot,
        )?);
    }
    let state = crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id)?;
    let native = ctx.copy_retained_text(&group.id, "copy F3D path group identity")?;
    if let Ok(historical) = PathRef::historical_edges(state, edge_ids, native, ctx)? {
        *path = historical;
    }
    Ok(())
}

pub(crate) fn project_feature_input_topologies(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    edge_operands: &[crate::records::topology::edge_identity::DesignEdgeOperand],
    scope_histories: &HashMap<String, String>,
) -> Result<Vec<cadmpeg_ir::features::FeatureInputTopology>, cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::FeatureInputTopology;

    let mut projected = Vec::new();
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let scoped_history = if scope_histories.contains_key(&scope.id) {
            let Some(history) = bound_scope_history(&scope.id, scope_histories, histories) else {
                continue;
            };
            Some(history)
        } else {
            None
        };
        let scoped_histories = scoped_history.map_or(histories, std::slice::from_ref);
        let Some(previous_state_id) = scope
            .previous_history_state_id()
            .or_else(|| {
                crate::design::feature_project::work_point_recipe_state_id(scope, edge_operands)
            })
            .or_else(|| crate::design::feature_project::work_plane_recipe_state_id(scope))
            .or_else(|| effective_scope_previous_history_state_id(scope, scoped_histories))
        else {
            continue;
        };
        let Some(state) = scope
            .history_state_id()
            .and_then(|state_id| {
                unique_history_state_pair(scoped_histories, state_id, previous_state_id)
                    .map(|(_, _, previous)| previous)
            })
            .or_else(|| {
                unique_history_state(scoped_histories, previous_state_id).map(|(_, state)| state)
            })
        else {
            continue;
        };
        let Some(topology) = state.topology() else {
            continue;
        };
        let Some(bodies) =
            project_input_members(ctx, &topology.bodies, "collect F3D input bodies", |slot| {
                crate::ids::history_input_body_id_charged(ctx, &feature.id, previous_state_id, slot)
            })?
        else {
            continue;
        };
        let Some(faces) =
            project_input_members(ctx, &topology.faces, "collect F3D input faces", |slot| {
                crate::ids::history_input_face_id_charged(ctx, &feature.id, previous_state_id, slot)
            })?
        else {
            continue;
        };
        let Some(edges) =
            project_input_members(ctx, &topology.edges, "collect F3D input edges", |slot| {
                crate::ids::history_input_edge_id_charged(ctx, &feature.id, previous_state_id, slot)
            })?
        else {
            continue;
        };
        let Some(vertices) = project_input_members(
            ctx,
            &topology.vertices,
            "collect F3D input vertices",
            |slot| {
                crate::ids::history_input_vertex_id_charged(
                    ctx,
                    &feature.id,
                    previous_state_id,
                    slot,
                )
            },
        )?
        else {
            continue;
        };
        let id = crate::ids::history_input_state_id_charged(ctx, &feature.id, previous_state_id)?;
        let input_of = feature
            .id
            .try_clone_for_decode(ctx, "copy F3D input feature identity")?;
        let native_ref = ctx.copy_retained_text(&state.id, "copy F3D input state reference")?;

        ctx.reserve_vec(&mut projected, 1, "collect F3D input topologies")?;
        projected.push(FeatureInputTopology {
            id,
            input_of,
            bodies,
            faces,
            edges,
            vertices,
            native_ref: Some(native_ref),
        });
    }
    Ok(projected)
}

fn project_input_members<T: Eq + std::hash::Hash>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    slots: &[i64],
    operation: &'static str,
    mut id: impl FnMut(i64) -> Result<T, cadmpeg_core::CodecError>,
) -> Result<Option<cadmpeg_ir::features::DistinctMembers<T>>, cadmpeg_core::CodecError> {
    let mut members = Vec::new();
    ctx.reserve_vec(&mut members, slots.len(), operation)?;
    for &slot in slots {
        members.push(id(slot)?);
    }
    Ok(Some(
        match cadmpeg_ir::features::DistinctMembers::try_from(members, ctx) {
            Ok(value) => value,
            Err(cadmpeg_ir::features::FeatureCollectionError::Invalid(_)) => return Ok(None),
            Err(cadmpeg_ir::features::FeatureCollectionError::Resource(limit)) => {
                return Err(limit.into())
            }
        },
    ))
}

/// Resolve persistent vertex recipes in the last history-bearing feature state
/// that precedes their owning construction in authored timeline order.
pub(crate) fn bind_vertex_recipe_history(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &mut [crate::records::feature::scope::DesignParameterScope],
    timelines: &[crate::records::entity_header::DesignFeatureTimeline],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut topology_queries = TopologyQueryCache::default();
    let source_ordinals =
        crate::design::feature_project::authored_scope_ordinals_per_stream(ctx, scopes, timelines)?;
    let mut input_states = HashMap::new();
    for scope in scopes.iter().filter(|scope| {
        matches!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::WorkPlane
                | crate::records::feature::scope::DesignFeatureKind::WorkPoint
        )
    }) {
        let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
        let Some(&ordinal) = source_ordinals.get(&(stream, scope.record_index)) else {
            continue;
        };
        let mut predecessors = scopes.iter().filter_map(|candidate| {
            let candidate_stream =
                crate::ids::native_stream(&candidate.id).unwrap_or(crate::ids::DEFAULT_STREAM);
            let candidate_ordinal =
                *source_ordinals.get(&(candidate_stream, candidate.record_index))?;
            (candidate_stream == stream && candidate_ordinal < ordinal)
                .then_some((candidate_ordinal, candidate.history_state_id()?))
        });
        let Some(predecessor) = predecessors.next() else {
            continue;
        };
        let predecessor = predecessors.fold(predecessor, |latest, candidate| {
            if candidate.0 > latest.0 {
                candidate
            } else {
                latest
            }
        });
        let id = ctx.copy_retained_text(&scope.id, "copy F3D vertex recipe scope identity")?;
        if !input_states.contains_key(&id) {
            ctx.reserve_map(&mut input_states, 1, "index F3D vertex recipe input states")?;
        }
        input_states.insert(id, predecessor.1);
    }

    for scope in scopes.iter_mut().filter(|scope| {
        scope.kind() == crate::records::feature::scope::DesignFeatureKind::WorkPoint
    }) {
        let state_id = input_states.get(&scope.id).copied();
        let Some(construction) = scope.work_point_construction_mut() else {
            continue;
        };
        let solved_position = cadmpeg_ir::math::Point3::new(
            construction.position[0].get() * 10.0,
            construction.position[1].get() * 10.0,
            construction.position[2].get() * 10.0,
        );
        for recipe in construction.rule.vertex_recipes_mut() {
            recipe.resolution = None;
            let Some(state_id) = state_id else {
                continue;
            };
            let Some((_, state)) = unique_history_state(histories, state_id) else {
                continue;
            };
            let Some(topology) = state.topology() else {
                continue;
            };
            let Some((vertex, position)) =
                vertex_recipe_candidate(ctx, recipe, topology, &mut topology_queries)?
            else {
                continue;
            };
            if !point_matches(position, solved_position) {
                continue;
            }
            recipe.resolution = crate::records::feature::work_geometry::DesignVertexResolution::new(
                state_id, vertex,
            );
        }
    }

    for scope in scopes.iter_mut().filter(|scope| {
        scope.kind() == crate::records::feature::scope::DesignFeatureKind::WorkPlane
    }) {
        let transform = scope.work_plane_transform();
        let state_id = input_states.get(&scope.id).copied();
        let Some(construction) = scope.work_plane_construction_mut() else {
            continue;
        };
        construction.clear_resolution();
        let Some(state_id) = state_id else {
            continue;
        };
        let Some((_, state)) = unique_history_state(histories, state_id) else {
            continue;
        };
        let Some(topology) = state.topology() else {
            continue;
        };
        let [first, second, third] = construction.inputs();
        let (Some(first), Some(second), Some(third)) = (
            vertex_recipe_candidate(ctx, first, topology, &mut topology_queries)?,
            vertex_recipe_candidate(ctx, second, topology, &mut topology_queries)?,
            vertex_recipe_candidate(ctx, third, topology, &mut topology_queries)?,
        ) else {
            continue;
        };
        let candidates = [first, second, third];
        if !three_point_plane_matches(
            transform.map(crate::records::sketch_placement::SketchPlacementMatrix::rows),
            [first.1, second.1, third.1],
        ) {
            continue;
        }
        let [Some(first), Some(second), Some(third)] = candidates.map(|(vertex, _)| {
            crate::records::feature::work_geometry::DesignVertexResolution::new(state_id, vertex)
        }) else {
            continue;
        };
        construction
            .try_set_resolution([first, second, third])
            .map_err(cadmpeg_core::CodecError::malformed)?;
    }
    Ok(())
}

/// Resolve edge-treatment corner recipes in their bound feature-input state.
pub(crate) fn bind_edge_treatment_vertex_history(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::feature::work_geometry::DesignEdgeTreatmentVertexOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut topology_queries = TopologyQueryCache::default();
    let mut recipe_topologies = RecipeTopologyCache::default();
    for operand in operands {
        operand.recipe.resolution = None;
        let stream = crate::ids::native_stream(&operand.id);
        let mut matching_scopes = scopes.iter().filter(|scope| {
            scope.record_index == operand.scope_record_index
                && crate::ids::native_stream(&scope.id) == stream
        });
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let (Some(state_id), Some(previous_state_id)) = (
            scope.history_state_id(),
            effective_scope_previous_history_state_id(scope, histories),
        ) else {
            continue;
        };
        let Some((_, _, previous)) = bound_history_state_pair(
            &scope.id,
            state_id,
            previous_state_id,
            scope_histories,
            histories,
        ) else {
            continue;
        };
        let Some(topology) = previous.topology() else {
            continue;
        };
        for reference in &mut operand.recipe.recipe_references {
            bind_historical_recipe_reference_candidates(
                decode,
                reference,
                topology,
                &mut recipe_topologies,
            )?;
        }
        let Some(vertex) = recipe_reference_common_vertex(
            decode,
            &operand.recipe,
            topology,
            &mut topology_queries,
        )?
        else {
            continue;
        };
        operand.recipe.resolution =
            crate::records::feature::work_geometry::DesignVertexResolution::new(
                previous_state_id,
                vertex,
            );
    }
    Ok(())
}

fn vertex_recipe_candidate<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    recipe: &crate::records::feature::work_geometry::DesignVertexRecipe,
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<Option<(i64, cadmpeg_ir::math::Point3)>, cadmpeg_core::CodecError> {
    let mut face_slots = Vec::new();
    for reference in &recipe.recipe_references {
        let mut slots = ctx.collect_vec(
            reference
                .candidate_faces
                .iter()
                .filter_map(|face| stable_ref(face.as_str()))
                .filter(|face| topology.faces.contains(face)),
            "collect F3D vertex recipe candidate faces",
        )?;
        ctx.sort_unstable_by(
            &mut slots,
            Ord::cmp,
            |_| 0,
            "sort F3D vertex recipe candidate faces",
        )?;
        slots.dedup();
        let [slot] = slots.as_slice() else {
            return Ok(None);
        };

        ctx.reserve_vec(&mut face_slots, 1, "collect F3D vertex recipe face slots")?;
        face_slots.push(*slot);
    }
    if face_slots.is_empty() {
        return Ok(None);
    }
    let Some(vertex) = common_face_vertex(ctx, &face_slots, topology, topology_queries)? else {
        return Ok(None);
    };
    Ok(unique_historical_vertex_position(vertex, topology).map(|position| (vertex, position)))
}

fn three_point_plane_matches(
    transform: Option<[[f64; 4]; 4]>,
    points: [cadmpeg_ir::math::Point3; 3],
) -> bool {
    use cadmpeg_ir::math::{Point3, Vector3};

    let Some(transform) = transform else {
        return false;
    };
    let origin = Point3::new(
        transform[0][3] * 10.0,
        transform[1][3] * 10.0,
        transform[2][3] * 10.0,
    );
    let Some(normal) = Vector3::new(transform[0][2], transform[1][2], transform[2][2]).unit()
    else {
        return false;
    };
    let Some(point_normal) = points[1]
        .vector_from(points[0])
        .cross(points[2].vector_from(points[0]))
        .unit()
    else {
        return false;
    };
    let scale = points
        .iter()
        .flat_map(|point| [point.x.abs(), point.y.abs(), point.z.abs()])
        .fold(1.0_f64, f64::max);
    (normal.dot(point_normal).abs() - 1.0).abs() <= WORK_POINT_POSITION_TOLERANCE
        && points.iter().all(|point| {
            point.vector_from(origin).dot(normal).abs() <= WORK_POINT_POSITION_TOLERANCE * scale
        })
}

fn boundary_vertices_for_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    faces: impl IntoIterator<Item = i64>,
    index: &VertexBoundaryIndex,
    edge_endpoints: &topology_cache::EdgeEndpoints,
) -> Result<Option<HashSet<i64>>, cadmpeg_core::CodecError> {
    let mut vertices = HashSet::new();
    for face in faces {
        let Some(edges) = index.boundaries.get(&face) else {
            return Ok(None);
        };
        for edge_slot in edges {
            decode.charge_work(1, "query F3D boundary vertex endpoints")?;
            let Some([start, end]) = edge_endpoints.get(edge_slot).copied().flatten() else {
                return Ok(None);
            };
            if !index.vertices.contains(&start) || !index.vertices.contains(&end) {
                return Ok(None);
            }
            decode.insert_hash_set(&mut vertices, start, "collect F3D boundary vertices")?;
            decode.insert_hash_set(&mut vertices, end, "collect F3D boundary vertices")?;
        }
    }
    Ok((!vertices.is_empty()).then_some(vertices))
}

fn common_face_vertex<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    face_slots: &[i64],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let index = topology_queries
        .vertices
        .get(decode, topology, VertexBoundaryIndex::new)?;
    let edge_endpoints =
        topology_queries
            .endpoints
            .get(decode, topology, topology_cache::endpoint_index)?;
    let mut common = None::<HashSet<i64>>;
    for face in face_slots {
        let Some(vertices) =
            boundary_vertices_for_faces(decode, std::iter::once(*face), index, edge_endpoints)?
        else {
            return Ok(None);
        };
        if let Some(common) = &mut common {
            common.retain(|vertex| vertices.contains(vertex));
        } else {
            common = Some(vertices);
        }
    }
    let Some(common) = common else {
        return Ok(None);
    };
    Ok(if common.len() == 1 {
        common.into_iter().next()
    } else {
        None
    })
}

fn recipe_reference_common_vertex<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    recipe: &crate::records::feature::work_geometry::DesignVertexRecipe,
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let index = topology_queries
        .vertices
        .get(decode, topology, VertexBoundaryIndex::new)?;
    let edge_endpoints =
        topology_queries
            .endpoints
            .get(decode, topology, topology_cache::endpoint_index)?;
    let mut common = None::<HashSet<i64>>;
    for reference in &recipe.recipe_references {
        let faces = reference
            .candidate_faces
            .iter()
            .filter_map(|face| stable_ref(face.as_str()))
            .filter(|face| index.faces.contains(face));
        let Some(vertices) = boundary_vertices_for_faces(decode, faces, index, edge_endpoints)?
        else {
            return Ok(None);
        };
        if let Some(common) = &mut common {
            common.retain(|vertex| vertices.contains(vertex));
        } else {
            common = Some(vertices);
        }
    }
    let Some(common) = common else {
        return Ok(None);
    };
    Ok(if common.len() == 1 {
        common.into_iter().next()
    } else {
        None
    })
}

fn unique_historical_vertex_position(
    vertex: i64,
    topology: &AsmHistoricalTopology,
) -> Option<cadmpeg_ir::math::Point3> {
    let mut bindings = topology
        .vertex_points
        .iter()
        .filter(|binding| binding.entity == vertex);
    let point = bindings.next()?.carrier;
    if bindings.next().is_some() {
        return None;
    }
    let mut positions = topology
        .point_positions
        .iter()
        .filter(|position| position.point == point);
    let position = positions.next()?.position;
    (positions.next().is_none() && position.is_finite()).then_some(position)
}

fn point_matches(left: cadmpeg_ir::math::Point3, right: cadmpeg_ir::math::Point3) -> bool {
    [(left.x, right.x), (left.y, right.y), (left.z, right.z)]
        .into_iter()
        .all(|(left, right)| {
            let scale = left.abs().max(right.abs()).max(1.0);
            (left - right).abs() <= WORK_POINT_POSITION_TOLERANCE * scale
        })
}

#[cfg(test)]
fn feature_input_prefix(
    feature: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
) -> cadmpeg_ir::ids::IdentityKey {
    let feature_key = feature.key();
    crate::ids::history_input_prefix(&feature_key, previous_state_id)
}

fn unique_history_state(
    histories: &[AsmHistory],
    state_id: i64,
) -> Option<(&AsmHistory, &AsmDeltaState)> {
    let mut matches = histories.iter().filter_map(|history| {
        let mut states = history
            .states
            .iter()
            .filter(|state| state.state_id == state_id);
        let state = states.next()?;
        states.next().is_none().then_some((history, state))
    });
    let state = matches.next()?;
    matches.next().is_none().then_some(state)
}

fn unique_history_state_in(history: &AsmHistory, state_id: i64) -> bool {
    let mut states = history
        .states
        .iter()
        .filter(|state| state.state_id == state_id);
    states.next().is_some() && states.next().is_none()
}

/// Return the effective input state for a scope. Some Design scope envelopes
/// omit the preceding state identity even though the current ASM delta state
/// carries the direct transition predecessor.
pub(crate) fn effective_scope_previous_history_state_id(
    scope: &crate::records::feature::scope::DesignParameterScope,
    histories: &[AsmHistory],
) -> Option<i64> {
    scope.previous_history_state_id().or_else(|| {
        let state_id = scope.history_state_id()?;
        let (history, state) = unique_history_state(histories, state_id)?;
        linked_previous_state_id(history, state)
    })
}

pub(crate) fn unique_history_state_pair(
    histories: &[AsmHistory],
    state_id: i64,
    previous_state_id: i64,
) -> Option<(&AsmHistory, &AsmDeltaState, &AsmDeltaState)> {
    let mut direct = histories
        .iter()
        .filter_map(|history| history_state_pair(history, state_id, previous_state_id, true));
    if let Some(pair) = direct.next() {
        return direct.next().is_none().then_some(pair);
    }
    let mut matches = histories
        .iter()
        .filter_map(|history| history_state_pair(history, state_id, previous_state_id, false));
    let pair = matches.next()?;
    matches.next().is_none().then_some(pair)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HemGapLengthForm {
    Flat,
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HemGeometrySemantics {
    pub(crate) direction: Option<cadmpeg_ir::features::SheetMetalHemDirection>,
    pub(crate) gap_length_form: Option<HemGapLengthForm>,
}

/// Resolve Hem semantics from the bend carriers in the owning history
/// transition. The source operation's fixed fields do not carry these
/// meanings; the selected edge and the inserted coaxial cylinders do.
pub(crate) fn hem_geometry_semantics(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
    edge_slot: i64,
    histories: &[AsmHistory],
) -> Result<HemGeometrySemantics, cadmpeg_core::CodecError> {
    let unresolved = HemGeometrySemantics {
        direction: None,
        gap_length_form: None,
    };
    let (Some(state_id), Some(previous_state_id)) = (
        scope.history_state_id(),
        effective_scope_previous_history_state_id(scope, histories),
    ) else {
        return Ok(unresolved);
    };
    let Some((_, state, previous)) =
        unique_history_state_pair(histories, state_id, previous_state_id)
    else {
        return Ok(unresolved);
    };
    let (Some(previous_topology), Some(transition)) =
        (previous.topology(), state.transition.as_ref())
    else {
        return Ok(unresolved);
    };
    let Some(current_topology) = state.topology() else {
        return Ok(unresolved);
    };
    let inserted_surfaces = &transition.topology.surfaces.inserted;
    if !current_topology
        .surface_cylinders
        .iter()
        .any(|cylinder| inserted_surfaces.contains(&cylinder.surface))
    {
        return Ok(unresolved);
    }
    let edge_direction =
        historical_edge_axis(decode, edge_slot, previous_topology)?.map(|(_, direction)| direction);
    let cylinders = || {
        current_topology
            .surface_cylinders
            .iter()
            .filter(|cylinder| {
                inserted_surfaces.contains(&cylinder.surface)
                    && edge_direction
                        .is_none_or(|direction| parallel_directions(direction, cylinder.axis))
            })
    };
    if cylinders().next().is_none() {
        return Ok(unresolved);
    }
    Ok(HemGeometrySemantics {
        direction: hem_direction_from_transition(
            decode,
            edge_slot,
            &cylinders(),
            previous_topology,
            transition,
        )?,
        gap_length_form: hem_gap_length_form(cylinders()),
    })
}

fn hem_gap_length_form<'a>(
    mut cylinders: impl Iterator<Item = &'a AsmHistoricalCylinder>,
) -> Option<HemGapLengthForm> {
    let first = cylinders.next()?;
    let second = cylinders.next()?;
    if cylinders.next().is_some() {
        return None;
    }
    if !same_axis_line((first.origin, first.axis), (second.origin, second.axis)) {
        return None;
    }
    let [inner, outer] = if first.radius <= second.radius {
        [first.radius, second.radius]
    } else {
        [second.radius, first.radius]
    };
    if !inner.is_finite() || !outer.is_finite() || inner < 0.0 || outer <= inner {
        return None;
    }
    let thickness = outer - inner;
    let tolerance = EPS_HISTORY_HEM_GAP_LENGTH_FORM_E7 * (1.0 + outer.abs() + inner.abs());
    if (2.0 * inner - thickness).abs() <= tolerance {
        Some(HemGapLengthForm::Open)
    } else if 2.0 * inner < thickness - tolerance {
        Some(HemGapLengthForm::Flat)
    } else {
        None
    }
}

fn hem_direction_from_transition<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    edge_slot: i64,
    cylinders: &(impl Iterator<Item = &'a AsmHistoricalCylinder> + Clone),
    previous: &AsmHistoricalTopology,
    transition: &AsmHistoricalTransition,
) -> Result<Option<cadmpeg_ir::features::SheetMetalHemDirection>, cadmpeg_core::CodecError> {
    if (*cylinders).clone().next().is_none() {
        return Ok(None);
    }
    let edge_context = selection::historical_edge_context(decode, edge_slot, previous)?;
    let mut candidate = None;
    for (ordinal, incident) in edge_context.incident_loops.iter().enumerate() {
        let face = incident.face_slot;
        if edge_context.incident_loops[..ordinal]
            .iter()
            .any(|earlier| earlier.face_slot == face)
        {
            continue;
        }
        if transition.topology.faces.deleted.contains(&face) {
            continue;
        }
        let mut bindings = previous
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == face);
        let Some(binding) = bindings.next() else {
            continue;
        };
        if bindings.next().is_some() {
            continue;
        }
        let mut planes = previous
            .surface_planes
            .iter()
            .filter(|plane| plane.surface == binding.carrier);
        let Some(plane) = planes.next() else {
            continue;
        };
        if planes.next().is_some() {
            continue;
        }
        let length = plane.normal.norm();
        if !(length.is_finite() && length > 0.0) {
            continue;
        }
        let normal = plane.normal.scale(1.0 / length);
        let Some(first_cylinder) = (*cylinders).clone().next() else {
            continue;
        };
        let first = normal.dot(first_cylinder.origin.vector_from(plane.origin));
        if !first.is_finite() {
            continue;
        }
        let sign_tolerance = EPS_HISTORY_HEM_DIRECTION_FROM_TRANSITION_E7
            * (1.0
                + plane.origin.x.abs()
                + plane.origin.y.abs()
                + plane.origin.z.abs()
                + (*cylinders).clone().fold(0.0_f64, |scale, cylinder| {
                    scale
                        .max(cylinder.origin.x.abs())
                        .max(cylinder.origin.y.abs())
                        .max(cylinder.origin.z.abs())
                }));
        if first.abs() <= sign_tolerance
            || (*cylinders).clone().any(|cylinder| {
                let offset = normal.dot(cylinder.origin.vector_from(plane.origin));
                !offset.is_finite()
                    || offset.abs() <= sign_tolerance
                    || offset.is_sign_positive() != first.is_sign_positive()
            })
        {
            continue;
        }
        if candidate.replace(first.is_sign_positive()).is_some() {
            return Ok(None);
        }
    }
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    Ok(Some(if candidate {
        cadmpeg_ir::features::SheetMetalHemDirection::Forward
    } else {
        cadmpeg_ir::features::SheetMetalHemDirection::Reverse
    }))
}

fn parallel_directions(left: cadmpeg_ir::math::Vector3, right: cadmpeg_ir::math::Vector3) -> bool {
    let left_length = left.norm();
    let right_length = right.norm();
    if !(left_length.is_finite()
        && left_length > 0.0
        && right_length.is_finite()
        && right_length > 0.0)
    {
        return false;
    }
    let dot = left
        .scale(1.0 / left_length)
        .dot(right.scale(1.0 / right_length));
    dot.is_finite() && (dot.abs() - 1.0).abs() <= EPS_HISTORY_PARALLEL_DIRECTIONS_E7
}

fn history_state_pair(
    history: &AsmHistory,
    state_id: i64,
    previous_state_id: i64,
    require_direct: bool,
) -> Option<(&AsmHistory, &AsmDeltaState, &AsmDeltaState)> {
    let mut states = history
        .states
        .iter()
        .filter(|state| state.state_id == state_id);
    let state = states.next()?;
    if states.next().is_some()
        || (require_direct && linked_previous_state_id(history, state) != Some(previous_state_id))
    {
        return None;
    }
    let mut previous_states = history
        .states
        .iter()
        .filter(|state| state.state_id == previous_state_id);
    let previous = previous_states.next()?;
    if previous_states.next().is_some()
        || (!require_direct && !history_state_reaches(history, state, previous_state_id))
    {
        return None;
    }
    Some((history, state, previous))
}

/// Return the one ASM history bound to a Design scope.
pub(crate) fn bound_scope_history<'a>(
    scope_id: &str,
    scope_histories: &HashMap<String, String>,
    histories: &'a [AsmHistory],
) -> Option<&'a AsmHistory> {
    let history_id = scope_histories.get(scope_id)?;
    let mut matches = histories.iter().filter(|history| &history.id == history_id);
    let history = matches.next()?;
    matches.next().is_none().then_some(history)
}

fn bound_history_state_pair<'a>(
    scope_id: &str,
    state_id: i64,
    previous_state_id: i64,
    scope_histories: &HashMap<String, String>,
    histories: &'a [AsmHistory],
) -> Option<(&'a AsmHistory, &'a AsmDeltaState, &'a AsmDeltaState)> {
    history_state_pair(
        bound_scope_history(scope_id, scope_histories, histories)?,
        state_id,
        previous_state_id,
        false,
    )
}

fn insert_scope_history_binding(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    resolved: &mut HashMap<String, String>,
    scope_id: &str,
    history_id: &str,
) -> Result<(), cadmpeg_core::CodecError> {
    if !resolved.contains_key(scope_id) {
        decode.reserve_map(resolved, 1, "index F3D scope history bindings")?;
    }
    let scope = {
        let ctx = decode;
        ctx.copy_retained_text(scope_id, "copy F3D bound scope identity")?
    };
    let history = {
        let ctx = decode;
        ctx.copy_retained_text(history_id, "copy F3D bound history identity")?
    };
    resolved.insert(scope, history);
    Ok(())
}

pub(crate) fn bind_scope_histories(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    body_bindings: &[crate::records::bodies::DesignBodyBinding],
    body_recipe_operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    histories: &[AsmHistory],
) -> Result<HashMap<String, String>, cadmpeg_core::CodecError> {
    let mut candidates = Vec::new();
    for scope in scopes {
        let Some(state_id) = scope.history_state_id() else {
            continue;
        };
        let scope_candidates = if let Some(previous_state_id) = scope.previous_history_state_id() {
            let direct = decode.collect_vec(
                histories.iter().filter(|history| {
                    history_state_pair(history, state_id, previous_state_id, true).is_some()
                }),
                "collect F3D direct scope histories",
            )?;
            if direct.is_empty() {
                decode.collect_vec(
                    histories.iter().filter(|history| {
                        history_state_pair(history, state_id, previous_state_id, false).is_some()
                    }),
                    "collect F3D reachable scope histories",
                )?
            } else {
                direct
            }
        } else {
            decode.collect_vec(
                histories
                    .iter()
                    .filter(|history| unique_history_state_in(history, state_id)),
                "collect F3D matching scope histories",
            )?
        };
        if !scope_candidates.is_empty() {
            decode.reserve_vec(&mut candidates, 1, "collect F3D scopes with histories")?;
            candidates.push((scope, scope_candidates));
        }
    }
    let mut resolved = HashMap::<String, String>::new();
    for (scope, candidates) in &candidates {
        if candidates.len() == 1 {
            insert_scope_history_binding(decode, &mut resolved, &scope.id, &candidates[0].id)?;
            continue;
        }
        let next_scope_record_index = scopes
            .iter()
            .filter(|candidate| {
                crate::ids::same_native_occurrence(&candidate.id, &scope.id)
                    && candidate.record_index > scope.record_index
            })
            .map(|candidate| candidate.record_index)
            .min();
        let mut output_bindings = body_bindings.iter().filter(|binding| {
            crate::ids::same_native_occurrence(binding.id(), &scope.id)
                && binding.entity_suffix > u64::from(scope.record_index)
                && next_scope_record_index
                    .is_none_or(|next| binding.entity_suffix < u64::from(next))
        });
        if let Some(binding) = output_bindings.next() {
            if output_bindings.next().is_none() {
                let mut matching = candidates.iter().filter(|history| {
                    historical_brep_source(&history.id).is_some_and(|source| {
                        binding.blob_name().strip_prefix("BREP.") == Some(source)
                    })
                });
                if let Some(history) = matching.next().filter(|_| matching.next().is_none()) {
                    insert_scope_history_binding(decode, &mut resolved, &scope.id, &history.id)?;
                    continue;
                }
            }
        }
        let candidate_faces = body_recipe_operands
            .iter()
            .filter(|operand| {
                crate::ids::same_native_occurrence(&operand.id, &scope.id)
                    && operand.scope_record_index == scope.record_index
            })
            .flat_map(super::records::topology::body_recipe::DesignBodyRecipeOperand::references)
            .flat_map(|reference| &reference.candidate_faces);
        if candidate_faces.clone().next().is_some() {
            let mut matching = candidates.iter().filter(|history| {
                historical_brep_source(&history.id).is_some_and(|source| {
                    candidate_faces
                        .clone()
                        .any(|face| active_brep_face_matches_source(face, source))
                })
            });
            if let Some(history) = matching.next().filter(|_| matching.next().is_none()) {
                insert_scope_history_binding(decode, &mut resolved, &scope.id, &history.id)?;
                continue;
            }
        }
        let Some(construction) = scope.base_feature_construction() else {
            continue;
        };
        let mut referenced_histories = construction.body_reference_records().filter_map(|suffix| {
            let mut bindings = body_bindings.iter().filter(|binding| {
                crate::ids::same_native_occurrence(binding.id(), &scope.id)
                    && binding.entity_suffix == u64::from(suffix)
            });
            let binding = bindings.next()?;
            if bindings.next().is_some() {
                return None;
            }
            let mut matching = candidates.iter().filter(|history| {
                historical_brep_source(&history.id)
                    .is_some_and(|source| binding.blob_name().strip_prefix("BREP.") == Some(source))
            });
            let history = matching.next()?;
            matching.next().is_none().then_some(history.id.as_str())
        });
        let Some(history_id) = referenced_histories.next() else {
            continue;
        };
        if referenced_histories.all(|candidate| candidate == history_id) {
            insert_scope_history_binding(decode, &mut resolved, &scope.id, history_id)?;
        }
    }
    let mut groups = HashMap::<(&str, i64, Option<i64>), Vec<usize>>::new();
    for (index, (scope, _)) in candidates.iter().enumerate() {
        let (Some(stream), Some(state_id)) = (
            crate::ids::native_stream(&scope.id),
            scope.history_state_id(),
        ) else {
            continue;
        };
        let key = (stream, state_id, scope.previous_history_state_id());
        decode.push_hash_group(
            &mut groups,
            key,
            index,
            "index F3D scope history groups",
            "collect F3D scope history group members",
        )?;
    }
    for members in groups.values() {
        let mut candidate_histories = HashSet::new();
        for index in members {
            for history in &candidates[*index].1 {
                decode.insert_hash_set(
                    &mut candidate_histories,
                    history.id.as_str(),
                    "index F3D scope candidate histories",
                )?;
            }
        }
        if candidate_histories.len() != members.len() {
            continue;
        }
        loop {
            let mut assigned = HashSet::new();
            for index in members {
                if let Some(history_id) = resolved.get(&candidates[*index].0.id) {
                    let copy = {
                        let ctx = decode;
                        ctx.copy_retained_text(history_id, "copy F3D assigned history identity")?
                    };
                    decode.insert_hash_set(
                        &mut assigned,
                        copy,
                        "index F3D assigned scope histories",
                    )?;
                }
            }
            if assigned.len()
                != members
                    .iter()
                    .filter(|index| resolved.contains_key(&candidates[**index].0.id))
                    .count()
            {
                break;
            }
            let mut progress = false;
            for index in members {
                let (scope, scope_candidates) = &candidates[*index];
                if resolved.contains_key(&scope.id) {
                    continue;
                }
                let mut remaining = scope_candidates
                    .iter()
                    .filter(|history| !assigned.contains(history.id.as_str()));
                if let Some(history) = remaining.next().filter(|_| remaining.next().is_none()) {
                    insert_scope_history_binding(decode, &mut resolved, &scope.id, &history.id)?;
                    progress = true;
                }
            }
            if !progress {
                break;
            }
        }
    }
    Ok(resolved)
}

fn history_state_reaches(
    history: &AsmHistory,
    state: &AsmDeltaState,
    previous_state_id: i64,
) -> bool {
    let mut current = state;
    for _ in 0..=history.states.len() {
        if current.state_id == previous_state_id {
            return true;
        }
        let Some(previous_state_id) = linked_previous_state_id(history, current) else {
            return false;
        };
        let mut states = history
            .states
            .iter()
            .filter(|state| state.state_id == previous_state_id);
        let Some(previous) = states.next() else {
            return false;
        };
        if states.next().is_some() {
            return false;
        }
        current = previous;
    }
    false
}

/// Return the state ID reached by a delta state's `next` link.
///
/// Reject disagreement between the derived transition predecessor and the raw
/// `next` chain.
fn linked_previous_state_id(history: &AsmHistory, state: &AsmDeltaState) -> Option<i64> {
    let linked = state.next_ref.and_then(|node_index| {
        let mut states = history
            .states
            .iter()
            .filter(|candidate| candidate.node_index == node_index);
        let previous = states.next()?;
        states.next().is_none().then_some(previous.state_id)
    });
    let derived = state
        .transition
        .as_ref()
        .and_then(|transition| transition.previous_state_id);
    match (derived, linked) {
        (Some(derived), Some(linked)) if derived != linked => None,
        (Some(derived), _) => Some(derived),
        (None, Some(linked)) => Some(linked),
        (None, None) => None,
    }
}

fn history_state_index<'h>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    history: &'h AsmHistory,
) -> Result<HashMap<i64, Option<&'h AsmDeltaState>>, cadmpeg_core::CodecError> {
    let mut states = HashMap::new();
    for state in &history.states {
        decode.charge_work(1, "index F3D history states")?;
        if !states.contains_key(&state.state_id) {
            decode.reserve_map(&mut states, 1, "index F3D history states")?;
        }
        states
            .entry(state.state_id)
            .and_modify(|state| *state = None)
            .or_insert(Some(state));
    }
    Ok(states)
}

fn exact_face_selection_group<'a>(
    operand: &crate::records::topology::face::DesignFaceOperand,
    scope: &crate::records::feature::scope::DesignParameterScope,
    operand_groups: &'a [crate::records::topology::construction::DesignConstructionOperandGroup],
) -> Option<&'a crate::records::topology::construction::DesignConstructionOperandGroup> {
    let stream = crate::ids::native_stream(&operand.id)?;
    if crate::ids::native_stream(&scope.id) != Some(stream) {
        return None;
    }
    let group_record_index = operand.group_record_index()?;
    let group_member_ordinal = usize::try_from(operand.group_member_ordinal()?).ok()?;
    let mut groups = operand_groups.iter().filter(|group| {
        crate::ids::native_stream(&group.id) == Some(stream)
            && group.scope_record_index == scope.record_index
            && group.record_index == group_record_index
            && group.role() == DesignOperandRole::ROLE_0X10
            && group
                .members()
                .get(group_member_ordinal)
                .map(|member| &member.value)
                == Some(&operand.record_index())
    });
    let group = groups.next()?;
    groups.next().is_none().then_some(group)
}

/// Bind one recipe reference to every live face or edge fragment carrying its
/// token and Design reference in the recipe-state topology.
fn bind_historical_recipe_reference_candidates<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    reference: &mut crate::records::dimensions::DesignRecipeReference,
    topology: &'a AsmHistoricalTopology,
    recipe_topologies: &mut RecipeTopologyCache<'a>,
) -> Result<(), cadmpeg_core::CodecError> {
    reference.candidate_faces.clear();
    reference.candidate_edges.clear();
    reference.alternate_selector_faces.clear();
    reference.alternate_selector_edges.clear();
    let index = recipe_topologies.get(decode, topology)?;
    for tag_index in index.matching_tags(decode, &reference.token, reference.design_reference)? {
        decode.charge_work(1, "walk F3D matching recipe tags")?;
        let tag = &index.topology.persistent_subentity_tags[*tag_index];
        match tag.entity_kind {
            AsmHistoricalEntityKind::Face => {
                decode.reserve_vec(
                    &mut reference.candidate_faces,
                    1,
                    "collect F3D recipe reference faces",
                )?;
                reference
                    .candidate_faces
                    .push(historical_face_id(decode, tag.entity_ref)?);
            }
            AsmHistoricalEntityKind::Edge => {
                decode.reserve_vec(
                    &mut reference.candidate_edges,
                    1,
                    "collect F3D recipe reference edges",
                )?;
                reference
                    .candidate_edges
                    .push(historical_edge_id(decode, tag.entity_ref)?);
            }
            _ => {}
        }
    }
    decode.stable_sort_by(
        &mut reference.candidate_faces,
        |left, right| left.as_str().cmp(right.as_str()),
        |face| face.as_str().len(),
        "sort F3D recipe reference faces",
    )?;
    reference.candidate_faces.dedup();
    decode.stable_sort_by(
        &mut reference.candidate_edges,
        |left, right| left.as_str().cmp(right.as_str()),
        |edge| edge.as_str().len(),
        "sort F3D recipe reference edges",
    )?;
    reference.candidate_edges.dedup();
    Ok(())
}

fn historical_recipe_faces<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    design_reference: i64,
    topology: &'a AsmHistoricalTopology,
    recipe_topologies: &mut RecipeTopologyCache<'a>,
) -> Result<Vec<cadmpeg_ir::ids::FaceId>, cadmpeg_core::CodecError> {
    let index = recipe_topologies.get(decode, topology)?;
    let topology = index.topology;
    let mut faces = Vec::new();
    for tag_index in index.face_tags(decode, design_reference)? {
        decode.charge_work(1, "walk F3D matching recipe faces")?;
        let tag = &topology.persistent_subentity_tags[*tag_index];
        decode.reserve_vec(&mut faces, 1, "collect F3D historical recipe faces")?;
        faces.push(historical_face_id(decode, tag.entity_ref)?);
    }
    decode.stable_sort_by(
        &mut faces,
        |left, right| left.as_str().cmp(right.as_str()),
        |face| face.as_str().len(),
        "sort F3D historical recipe faces",
    )?;
    faces.dedup();
    Ok(faces)
}

fn direct_face_recipe_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    recipe_kind: crate::records::recipes::ConstructionRecipeKind,
    references: &[crate::records::dimensions::DesignRecipeReference],
    recipe_record_index: i32,
) -> Result<Option<Vec<cadmpeg_ir::ids::FaceId>>, cadmpeg_core::CodecError> {
    if recipe_kind != crate::records::recipes::ConstructionRecipeKind::Face {
        return Ok(None);
    }
    let mut faces = Vec::new();
    for face in references
        .iter()
        .filter(|reference| reference.design_reference == i64::from(recipe_record_index))
        .flat_map(|reference| &reference.candidate_faces)
    {
        decode.reserve_vec(&mut faces, 1, "collect F3D direct face recipe candidates")?;
        faces.push((face).try_clone_for_decode(decode, "copy F3D historical face identity")?);
    }
    decode.stable_sort_by(
        &mut faces,
        |left, right| left.as_str().cmp(right.as_str()),
        |face| face.as_str().len(),
        "sort F3D direct face recipe candidates",
    )?;
    faces.dedup();
    Ok((!faces.is_empty()).then_some(faces))
}

fn historical_face_id(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    slot: i64,
) -> Result<cadmpeg_ir::ids::FaceId, cadmpeg_core::CodecError> {
    let ctx = decode;
    let text = ctx.format_retained(
        format_args!("f3d:brep:entity#{slot}"),
        "retain F3D historical face identity",
    )?;
    cadmpeg_ir::ids::FaceId::mint(text).map_err(cadmpeg_core::CodecError::malformed)
}

fn historical_edge_id(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    slot: i64,
) -> Result<cadmpeg_ir::ids::EdgeId, cadmpeg_core::CodecError> {
    let ctx = decode;
    let text = ctx.format_retained(
        format_args!("f3d:brep:entity#{slot}"),
        "retain F3D historical edge identity",
    )?;
    cadmpeg_ir::ids::EdgeId::mint(text).map_err(cadmpeg_core::CodecError::malformed)
}

pub(crate) fn bind_face_operand_history_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::face::DesignFaceOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    operand_groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    recipes: &[crate::records::recipes::ConstructionRecipe],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut topology_queries = TopologyQueryCache::default();
    let mut state_indexes = SnapshotCache::default();
    let mut recipe_topologies = RecipeTopologyCache::default();
    if projection_was_finalized(histories) {
        return Ok(());
    }
    let recipe_record_indices = decode.collect_hash_map(
        recipes
            .iter()
            .filter_map(|recipe| Some((recipe.id.as_str(), recipe.record_index?.value))),
        "index F3D face operand recipe records",
    )?;
    for operand in &mut *operands {
        operand.preceding_candidate_faces.clear();
        operand.changed_candidate_faces.clear();
        operand.historical_support_contexts.clear();
        operand.resolved_face_slots.clear();
        operand.resolved_active_face = None;
        let stream = crate::ids::native_stream(&operand.id);
        let mut matching_scopes = scopes.iter().filter(|scope| {
            scope.record_index == operand.scope_record_index
                && crate::ids::native_stream(&scope.id) == stream
        });
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let scoped_history = if scope_histories.contains_key(&scope.id) {
            let Some(history) = bound_scope_history(&scope.id, scope_histories, histories) else {
                continue;
            };
            Some(history)
        } else {
            None
        };
        let scoped_histories = scoped_history.map_or(histories, std::slice::from_ref);
        let Some(state_id) = scope.history_state_id() else {
            continue;
        };
        let Some(previous_state_id) =
            effective_scope_previous_history_state_id(scope, scoped_histories)
        else {
            continue;
        };
        let Some((history, state, previous)) = (if scoped_history.is_some() {
            bound_history_state_pair(
                &scope.id,
                state_id,
                previous_state_id,
                scope_histories,
                histories,
            )
        } else {
            unique_history_state_pair(histories, state_id, previous_state_id)
        }) else {
            continue;
        };
        let states = state_indexes.get(decode, history, history_state_index)?;
        let Some(topology) = previous.topology() else {
            continue;
        };
        for reference in &mut operand.recipe_references {
            bind_historical_recipe_reference_candidates(
                decode,
                reference,
                topology,
                &mut recipe_topologies,
            )?;
        }
        if let Some(recipe_record_index) = recipe_record_indices.get(operand.recipe_id.as_str()) {
            operand.candidate_faces = historical_recipe_faces(
                decode,
                i64::from(*recipe_record_index),
                topology,
                &mut recipe_topologies,
            )?;
            operand.unreferenced_candidate_faces = decode.try_collect_vec(
                (operand.candidate_faces.iter().filter(|face| {
                    !operand
                        .recipe_references
                        .iter()
                        .flat_map(|reference| &reference.candidate_faces)
                        .any(|candidate| candidate == *face)
                }))
                .map(|face| face.try_clone_for_decode(decode, "copy F3D historical face identity")),
                "collect F3D unreferenced candidate faces",
            )?;
            operand.alternate_selector_candidate_faces.clear();
        }
        let Some(changed_faces) =
            face_changes_across_state_chain(decode, state, previous_state_id, states)?
        else {
            continue;
        };
        let direct_face_candidates =
            if let Some(record_index) = recipe_record_indices.get(operand.recipe_id.as_str()) {
                direct_face_recipe_candidates(
                    decode,
                    operand.recipe_kind,
                    &operand.recipe_references,
                    *record_index,
                )?
            } else {
                None
            };
        let feature_family = crate::design::design_feature_family(&scope.kind());
        let thread_face_candidates = if feature_family
            == Some(crate::design::DesignFeatureFamily::Thread)
            && exact_face_selection_group(operand, scope, operand_groups).is_some()
        {
            operand
                .recipe_references
                .first()
                .and_then(|reference| {
                    let candidates = effective_faces(reference);
                    (!candidates.is_empty()).then_some(candidates)
                })
                .map(|candidates| {
                    decode.try_collect_vec(
                        (candidates).iter().map(|face| {
                            face.try_clone_for_decode(decode, "copy F3D historical face identity")
                        }),
                        "copy F3D thread face candidates",
                    )
                })
                .transpose()?
        } else {
            None
        };
        let nested_split_face_candidates = if scope.kind()
            == crate::records::feature::scope::DesignFeatureKind::SplitFace
            && exact_face_selection_group(operand, scope, operand_groups).is_some()
        {
            crate::design::face_resolve::nested_bounded_face_history_candidates(decode, operand)?
        } else {
            None
        };
        let grouped_reference_face_candidates = (!matches!(
            feature_family,
            Some(
                crate::design::DesignFeatureFamily::Thread
                    | crate::design::DesignFeatureFamily::Split
            )
        ))
        .then(|| grouped_reference_face_candidate(operand, topology, &changed_faces))
        .flatten()
        .map(|face| vec![face]);
        let legacy_face_candidates = (|| -> Result<Option<_>, cadmpeg_core::CodecError> {
            if feature_family != Some(crate::design::DesignFeatureFamily::Extrude) {
                return Ok(None);
            }
            let Some(group_record_index) = operand.group_record_index() else {
                return Ok(None);
            };
            let mut groups = operand_groups.iter().filter(|group| {
                crate::ids::native_stream(&group.id) == stream
                    && group.scope_record_index == scope.record_index
                    && group.record_index == group_record_index
                    && group.extrude_role().is_some_and(|role| {
                        matches!(
                            role,
                            crate::records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_)
                        )
                    })
                    && group.extrude_face_role().is_some()
            });
            if groups.next().is_none() {
                return Ok(None);
            }
            if groups.next().is_some() {
                return Ok(None);
            }
            let Some(recipe_record_index) = recipe_record_indices.get(operand.recipe_id.as_str())
            else {
                return Ok(None);
            };
            crate::design::face_resolve::legacy_face_recipe_reference_candidates(
                decode,
                operand,
                *recipe_record_index,
            )
        })()?;
        let fallback_candidates;
        let history_candidates = if let Some(candidates) = direct_face_candidates
            .as_deref()
            .or(thread_face_candidates.as_deref())
            .or(nested_split_face_candidates.as_deref())
            .or(grouped_reference_face_candidates.as_deref())
        {
            candidates
        } else {
            fallback_candidates =
                crate::design::face_resolve::historical_face_operand_candidates(decode, operand)?;
            &fallback_candidates
        };
        operand.preceding_candidate_faces =
            selection::faces_in_topology(decode, history_candidates, topology)?;
        operand.changed_candidate_faces = decode.try_collect_vec(
            (operand.preceding_candidate_faces.iter().filter(|face| {
                stable_ref(face.as_str()).is_some_and(|slot| changed_faces.contains(&slot))
            }))
            .map(|face| face.try_clone_for_decode(decode, "copy F3D historical face identity")),
            "collect F3D changed candidate faces",
        )?;
        operand.historical_support_contexts = historical_face_support_contexts(
            decode,
            history_candidates,
            history,
            topology,
            &changed_faces,
            &mut topology_queries,
        )?;
        if direct_face_candidates.is_some() {
            operand.resolved_face_slots = decode.collect_vec(
                operand
                    .preceding_candidate_faces
                    .iter()
                    .filter_map(|face| stable_ref(face.as_str())),
                "collect F3D direct resolved face slots",
            )?;
            continue;
        }
        let preserves_stable_face_set = feature_family
            == Some(crate::design::DesignFeatureFamily::Shell)
            || operand
                .group_record_index()
                .is_some_and(|group_record_index| {
                    let mut groups = operand_groups.iter().filter(|group| {
                        crate::ids::native_stream(&group.id) == stream
                            && group.scope_record_index == scope.record_index
                            && group.record_index == group_record_index
                    });
                    let Some(group) = groups.next() else {
                        return false;
                    };
                    groups.next().is_none()
                        && group.extrude_face_role()
                            == Some(crate::records::topology::extrude_selection::DesignExtrudeFaceRole::Termination)
                });
        operand.resolved_face_slots = match &scope.payload() {
            crate::records::feature::scope::DesignScopePayload::OffsetFaces(Some(_))
            | crate::records::feature::scope::DesignScopePayload::DecalerLesFaces(Some(_)) => {
                let direct = resolve_direct_face_recipe_clauses(
                    decode,
                    &operand.recipe_references,
                    topology,
                    &changed_faces,
                )?;
                if direct.is_empty() {
                    crate::design::face_resolve::resolve_face_operand_history_candidates(operand)
                        .into_iter()
                        .collect()
                } else {
                    direct
                }
            }
            _ => {
                let direct =
                    crate::design::face_resolve::resolve_face_operand_history_candidates(operand);
                if let Some(direct) = direct {
                    vec![direct]
                } else if scope.kind()
                    == crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
                {
                    crate::design::face_resolve::resolve_surface_delete_face_history_set(
                        decode, operand,
                    )?
                    .unwrap_or_default()
                } else if preserves_stable_face_set {
                    if let Some(stable) =
                        crate::design::face_resolve::resolve_stable_bounded_face_history_set(
                            decode, operand,
                        )?
                    {
                        stable
                    } else {
                        crate::design::face_resolve::resolve_bounded_face_history_candidates(
                            decode, operand,
                        )?
                        .unwrap_or_default()
                    }
                } else if let Some(bounded) =
                    crate::design::face_resolve::resolve_bounded_face_history_candidates(
                        decode, operand,
                    )?
                {
                    bounded
                } else {
                    let pattern = if feature_family
                        == Some(crate::design::DesignFeatureFamily::CircularPattern)
                    {
                        if let Some(result) = state.topology() {
                            resolve_pattern_face_by_surface_radius(
                                decode,
                                crate::design::face_resolve::face_operand_candidates(operand),
                                topology,
                                result,
                                &changed_faces,
                            )?
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    pattern.into_iter().collect()
                }
            }
        };
        if let Some(candidates) = &thread_face_candidates {
            // Thread's first prefix reference is its exclusive selection lane.
            // An unresolved lane must not fall back to construction context.
            operand.resolved_face_slots =
                crate::design::face_resolve::resolve_face_operand_history_candidate_from(
                    operand, candidates,
                )
                .or_else(|| {
                    resolve_thread_face_by_transition(
                        scope,
                        candidates,
                        history,
                        topology,
                        &changed_faces,
                    )
                })
                .into_iter()
                .collect();
        }
        if let Some(candidates) = &grouped_reference_face_candidates {
            // A grouped frame's unique changed topology-face reference is its
            // exact historical selection lane.
            operand.resolved_face_slots =
                crate::design::face_resolve::resolve_face_operand_history_candidate_from(
                    operand, candidates,
                )
                .into_iter()
                .collect();
        }
        if feature_family == Some(crate::design::DesignFeatureFamily::Split) {
            operand.resolved_face_slots = resolve_split_tool_face(decode, operand, topology)?
                .into_iter()
                .collect();
        }
        if feature_family == Some(crate::design::DesignFeatureFamily::Loft)
            && operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::BoundedFace
            && state
                .transition
                .as_ref()
                .is_some_and(|transition| transition.previous_state_id == Some(previous_state_id))
        {
            if let Some((result, transition)) = state.topology().zip(state.transition.as_ref()) {
                if let Some(face) = resolve_bounded_face_recipe_target(
                    decode,
                    operand,
                    topology,
                    result,
                    &transition.topology.bodies.inserted,
                    &mut topology_queries,
                )? {
                    operand.resolved_face_slots = vec![face];
                }
            }
        }
        if operand.resolved_face_slots.is_empty()
            && scope.kind() == crate::records::feature::scope::DesignFeatureKind::Draft
        {
            if let Some(result) = state.topology() {
                if let Some(face) =
                    resolve_draft_face_by_surface_transition(decode, operand, topology, result)?
                {
                    operand.resolved_face_slots = vec![face];
                }
            }
        }
        if operand.resolved_face_slots.is_empty() {
            if let Some(candidates) = legacy_face_candidates {
                match select_legacy_extrude_face_candidate(
                    candidates,
                    topology,
                    &changed_faces,
                    historical_brep_source(&history.id),
                ) {
                    Some(LegacyFaceResolution::Historical(slot)) => {
                        operand.resolved_face_slots = vec![slot];
                    }
                    Some(LegacyFaceResolution::Active(face)) => {
                        operand.resolved_active_face = Some(face);
                    }
                    None => {}
                }
            }
        }
    }
    bind_profile_face_group_cardinality(
        decode,
        operands,
        scopes,
        operand_groups,
        histories,
        scope_histories,
    )?;
    Ok(())
}

/// Resolve a Draft face whose persistent selector lane is ambiguous in the
/// active B-rep by following the surface carrier through the exact Draft
/// transition.
///
/// Draft changes the selected face's actual surface geometry. The same
/// transition may update tangent or context faces, so the transition is
/// admissible only when exactly one face from the operand's alternate lane (or,
/// when no alternate lane exists, its exact candidate lane) changes surface
/// geometry. A missing, duplicate, or unchanged surface is not a selection
/// proof. Carrier record identities are not compared: a history update may
/// assign a new carrier record without changing the surface.
#[derive(Clone, Copy, PartialEq)]
enum DraftSurfaceGeometry {
    Plane {
        origin: cadmpeg_ir::math::Point3,
        normal: cadmpeg_ir::math::Vector3,
    },
    Cylinder {
        origin: cadmpeg_ir::math::Point3,
        axis: cadmpeg_ir::math::Vector3,
        radius: f64,
    },
    Axis {
        origin: cadmpeg_ir::math::Point3,
        direction: cadmpeg_ir::math::Vector3,
        radius: Option<f64>,
    },
}

fn draft_surface_geometry(
    topology: &crate::history_records::AsmHistoricalTopology,
    face: i64,
) -> Option<DraftSurfaceGeometry> {
    let mut bindings = topology
        .face_surfaces
        .iter()
        .filter(|binding| binding.entity == face)
        .map(|binding| binding.carrier);
    let carrier = bindings.next()?;
    if bindings.next().is_some() {
        return None;
    }
    if let Some(plane) = topology
        .surface_planes
        .iter()
        .find(|surface| surface.surface == carrier)
    {
        return Some(DraftSurfaceGeometry::Plane {
            origin: plane.origin,
            normal: plane.normal,
        });
    }
    if let Some(cylinder) = topology
        .surface_cylinders
        .iter()
        .find(|surface| surface.surface == carrier)
    {
        return Some(DraftSurfaceGeometry::Cylinder {
            origin: cylinder.origin,
            axis: cylinder.axis,
            radius: cylinder.radius,
        });
    }
    topology
        .surface_axes
        .iter()
        .find(|surface| surface.surface == carrier)
        .map(|axis| DraftSurfaceGeometry::Axis {
            origin: axis.origin,
            direction: axis.direction,
            radius: topology
                .surface_radii
                .iter()
                .find(|radius| radius.surface == carrier)
                .map(|radius| radius.radius),
        })
}

fn resolve_draft_face_by_surface_transition(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::face::DesignFaceOperand,
    preceding: &crate::history_records::AsmHistoricalTopology,
    result: &crate::history_records::AsmHistoricalTopology,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
        || !matches!(
            crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program),
            Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { .. })
        )
        || operand.recipe_nodes.is_empty()
    {
        return Ok(None);
    }
    let mut candidate_slots = HashSet::new();
    for face in operand
        .candidate_faces
        .iter()
        .filter_map(|face| stable_ref(face.as_str()))
    {
        decode.insert_hash_set(
            &mut candidate_slots,
            face,
            "index F3D draft face candidates",
        )?;
    }
    if candidate_slots.is_empty() {
        return Ok(None);
    }
    let mut alternate_slots = BTreeSet::new();
    for face in operand
        .recipe_references
        .iter()
        .flat_map(|reference| &reference.alternate_selector_faces)
        .filter_map(|face| stable_ref(face.as_str()))
        .filter(|face| candidate_slots.contains(face))
    {
        decode
            .insert_btree_set(
                &mut alternate_slots,
                face,
                "collect F3D draft alternate faces",
            )
            .map(|_| ())?;
    }
    let mut exact_slots = BTreeSet::new();
    for face in operand
        .recipe_references
        .iter()
        .flat_map(|reference| &reference.candidate_faces)
        .filter_map(|face| stable_ref(face.as_str()))
        .filter(|face| candidate_slots.contains(face))
    {
        decode
            .insert_btree_set(&mut exact_slots, face, "collect F3D draft exact faces")
            .map(|_| ())?;
    }
    let has_alternates = !alternate_slots.is_empty();
    let candidates = if has_alternates {
        alternate_slots
    } else {
        exact_slots
    };
    if candidates.is_empty() {
        return Ok(None);
    }
    if !has_alternates {
        let mut faces = candidates.iter();
        let Some(face) = faces.next().copied() else {
            return Ok(None);
        };
        if faces.next().is_some() {
            return Ok(None);
        }
        return Ok(
            (preceding.faces.contains(&face) && result.faces.contains(&face)).then_some(face),
        );
    }
    let mut changed = None;
    for face in candidates
        .into_iter()
        .filter(|face| preceding.faces.contains(face) && result.faces.contains(face))
        .filter(|face| {
            draft_surface_geometry(preceding, *face)
                .zip(draft_surface_geometry(result, *face))
                .is_some_and(|(before, after)| before != after)
        })
    {
        if changed.replace(face).is_some() {
            return Ok(None);
        }
    }
    Ok(changed)
}

fn resolve_pattern_face_by_surface_radius(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &[cadmpeg_ir::ids::FaceId],
    preceding: &crate::history_records::AsmHistoricalTopology,
    result: &crate::history_records::AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let mut candidate_faces = HashSet::new();
    for face in candidates
        .iter()
        .filter_map(|face| stable_ref(face.as_str()))
    {
        decode.insert_hash_set(
            &mut candidate_faces,
            face,
            "index F3D pattern face candidates",
        )?;
    }
    if candidate_faces.is_empty() {
        return Ok(None);
    }
    let mut result_radius = None;
    let mut bound_candidates = HashSet::new();
    for binding in result
        .face_surfaces
        .iter()
        .filter(|binding| candidate_faces.contains(&binding.entity))
    {
        if !decode.insert_hash_set(
            &mut bound_candidates,
            binding.entity,
            "index F3D pattern bound faces",
        )? {
            return Ok(None);
        }
        let mut radii = result
            .surface_radii
            .iter()
            .filter(|radius| radius.surface == binding.carrier);
        let Some(radius) = radii.next().map(|radius| radius.radius) else {
            return Ok(None);
        };
        if radii.next().is_some() || !radius.is_finite() || radius <= 0.0 {
            return Ok(None);
        }
        match result_radius {
            None => result_radius = Some(radius.to_bits()),
            Some(expected) if expected == radius.to_bits() => {}
            Some(_) => return Ok(None),
        }
    }
    if bound_candidates != candidate_faces {
        return Ok(None);
    }
    let Some(result_radius) = result_radius else {
        return Ok(None);
    };
    let mut matches = preceding
        .face_surfaces
        .iter()
        .filter(|binding| changed_faces.contains(&binding.entity))
        .filter_map(|binding| {
            let mut radii = preceding
                .surface_radii
                .iter()
                .filter(|radius| radius.surface == binding.carrier);
            let radius = radii.next()?;
            (radii.next().is_none() && radius.radius.to_bits() == result_radius)
                .then_some(binding.entity)
        });
    let Some(face) = matches.next() else {
        return Ok(None);
    };
    Ok(matches.next().is_none().then_some(face))
}

fn resolve_split_tool_face(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::face::DesignFaceOperand,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    if operand.group_record_index().is_some()
        || operand.group_member_ordinal().is_some()
        || operand.scope_reference_ordinal != 1
        || operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::Face
        || operand.recipe_program != [0, -1]
    {
        return Ok(None);
    }
    let [reference] = operand.recipe_references.as_slice() else {
        return Ok(None);
    };
    let candidates = selection::faces_in_topology(decode, &reference.candidate_faces, topology)?;
    let [face] = candidates.as_slice() else {
        return Ok(None);
    };
    Ok(stable_ref(face.as_str()))
}

fn effective_faces(
    reference: &crate::records::dimensions::DesignRecipeReference,
) -> &[cadmpeg_ir::ids::FaceId] {
    if reference.candidate_faces.is_empty() {
        &reference.alternate_selector_faces
    } else {
        &reference.candidate_faces
    }
}

fn resolve_thread_face_by_transition(
    scope: &crate::records::feature::scope::DesignParameterScope,
    candidates: &[cadmpeg_ir::ids::FaceId],
    history: &AsmHistory,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Option<i64> {
    let construction = scope.thread_construction()?;
    let source = historical_brep_source(&history.id)?;
    let mut source_candidates = candidates
        .iter()
        .filter(|face| active_brep_face_matches_source(face, source));
    source_candidates.next()?;
    if source_candidates.next().is_some() {
        return None;
    }
    let minimum_radius = construction.diameters.minor() * 5.0;
    let maximum_radius = construction.diameters.major() * 5.0;
    if !minimum_radius.is_finite() || !maximum_radius.is_finite() {
        return None;
    }
    let tolerance = EPS_HISTORY_RESOLVE_THREAD_FACE_BY_TRANSITION_E9 * (1.0 + maximum_radius.abs());
    let mut matching_faces = topology
        .faces
        .iter()
        .copied()
        .filter(|face| changed_faces.contains(face))
        .filter_map(|face| {
            let mut bindings = topology
                .face_surfaces
                .iter()
                .filter(|binding| binding.entity == face);
            let binding = bindings.next()?;
            if bindings.next().is_some() {
                return None;
            }
            let mut cylinders = topology
                .surface_cylinders
                .iter()
                .filter(|cylinder| cylinder.surface == binding.carrier);
            let cylinder = cylinders.next()?;
            (cylinders.next().is_none()
                && cylinder.radius + tolerance >= minimum_radius
                && cylinder.radius <= maximum_radius + tolerance)
                .then_some(face)
        });
    let face = matching_faces.next()?;
    matching_faces
        .all(|candidate| candidate == face)
        .then_some(face)
}

fn grouped_reference_face_candidate(
    operand: &crate::records::topology::face::DesignFaceOperand,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Option<cadmpeg_ir::ids::FaceId> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
        || !crate::design::decode::dimension_frames::is_grouped_recipe_reference_frame(
            &operand.recipe_prefix_bytes,
        )
    {
        return None;
    }
    let mut candidates = operand
        .recipe_references
        .iter()
        .map(|reference| reference.design_reference)
        .filter(|reference| {
            topology.faces.contains(reference) && changed_faces.contains(reference)
        });
    let face = candidates.next()?;
    candidates
        .all(|candidate| candidate == face)
        .then(|| crate::ids::brep_face_id(face))
}

fn relation_members(
    relations: &[crate::history_records::AsmHistoricalRelation],
    owner: i64,
) -> Option<&[i64]> {
    let mut matches = relations
        .iter()
        .filter(|relation| relation.owner_ref == owner);
    let members = matches.next()?.member_refs.as_slice();
    matches.next().is_none().then_some(members)
}

fn resolve_bounded_face_recipe_target<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::face::DesignFaceOperand,
    preceding: &'a crate::history_records::AsmHistoricalTopology,
    result: &'a crate::history_records::AsmHistoricalTopology,
    inserted_bodies: &[i64],
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { header_value }) =
        crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program)
    else {
        return Ok(None);
    };
    if operand.recipe_nodes.len() != header_value
        || operand
            .recipe_nodes
            .iter()
            .any(|node| node.recipe_structure.is_none())
    {
        return Ok(None);
    }
    let Some(first) = operand.recipe_references.first() else {
        return Ok(None);
    };
    let first_clause_len = operand
        .recipe_references
        .iter()
        .take_while(|reference| {
            reference.selector_offset == first.selector_offset
                && reference.token_offset == first.token_offset
        })
        .count();
    let first_clause = &operand.recipe_references[..first_clause_len];
    let mut topology_faces = HashSet::new();
    for face in &preceding.faces {
        decode.insert_hash_set(
            &mut topology_faces,
            *face,
            "index F3D bounded topology faces",
        )?;
    }
    let mut target_candidates = BTreeSet::new();
    for face in first_clause
        .first()
        .into_iter()
        .flat_map(effective_faces)
        .filter_map(|face| stable_ref(face.as_str()))
        .filter(|face| topology_faces.contains(face))
    {
        decode
            .insert_btree_set(
                &mut target_candidates,
                face,
                "collect F3D bounded target faces",
            )
            .map(|_| ())?;
    }
    for reference in first_clause.iter().skip(1) {
        let mut candidates = HashSet::new();
        for face in effective_faces(reference)
            .iter()
            .filter_map(|face| stable_ref(face.as_str()))
            .filter(|face| topology_faces.contains(face))
        {
            decode.insert_hash_set(&mut candidates, face, "collect F3D bounded clause faces")?;
        }
        target_candidates.retain(|face| candidates.contains(face));
    }
    let mut construction_faces = Vec::new();
    'bodies: for body in inserted_bodies {
        let Some(regions) = relation_members(&result.body_regions, *body) else {
            continue;
        };
        let mut unique = None;
        let mut ambiguous = false;
        for region in regions {
            let Some(shells) = relation_members(&result.region_shells, *region) else {
                continue 'bodies;
            };
            for shell in shells {
                let Some(faces) = relation_members(&result.shell_faces, *shell) else {
                    continue 'bodies;
                };
                for face in faces {
                    if unique.is_some_and(|prior| prior != *face) {
                        ambiguous = true;
                    }
                    unique = Some(*face);
                }
            }
        }
        if !ambiguous {
            if let Some(face) = unique {
                decode.reserve_vec(&mut construction_faces, 1, "collect F3D construction faces")?;
                construction_faces.push(face);
            }
        }
    }
    if construction_faces.is_empty() {
        return Ok(None);
    }
    let mut face_loop_positions = |face,
                                   topology|
     -> Result<
        Option<(usize, Vec<cadmpeg_ir::math::Point3>)>,
        cadmpeg_core::CodecError,
    > {
        let contexts =
            face_boundary_contexts_for_slots(decode, &[face], topology, topology_queries)?;
        let [context] = contexts.as_slice() else {
            return Ok(None);
        };
        let [loop_] = context.loops.as_slice() else {
            return Ok(None);
        };
        let crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Positions(
            rows,
        ) = &loop_.boundary
        else {
            return Ok(None);
        };
        if rows.is_empty() {
            return Ok(None);
        }
        let positions = decode.collect_vec(
            rows.iter().map(|row| row.position),
            "collect F3D bounded loop positions",
        )?;
        Ok(Some((rows.len(), positions)))
    };
    let mut matches = Vec::new();
    for candidate in target_candidates {
        let Some((edge_count, candidate_points)) = face_loop_positions(candidate, preceding)?
        else {
            continue;
        };
        if edge_count != header_value {
            continue;
        }
        let mut matched = false;
        for face in &construction_faces {
            if let Some((construction_edge_count, construction_points)) =
                face_loop_positions(*face, result)?
            {
                if construction_edge_count >= edge_count
                    && cyclic_point_subsequence(&candidate_points, &construction_points)
                {
                    matched = true;
                    break;
                }
            }
        }
        if matched {
            decode.reserve_vec(&mut matches, 1, "collect F3D bounded face matches")?;
            matches.push(candidate);
        }
    }
    decode.sort_unstable_by(
        &mut matches,
        Ord::cmp,
        |_| 0,
        "sort F3D bounded face matches",
    )?;
    matches.dedup();
    let [face] = matches.as_slice() else {
        return Ok(None);
    };
    Ok(Some(*face))
}

fn cyclic_point_subsequence(
    candidate: &[cadmpeg_ir::math::Point3],
    construction: &[cadmpeg_ir::math::Point3],
) -> bool {
    let coincident = |left: &cadmpeg_ir::math::Point3, right: &cadmpeg_ir::math::Point3| {
        let dx = left.x - right.x;
        let dy = left.y - right.y;
        let dz = left.z - right.z;
        dx.mul_add(dx, dy.mul_add(dy, dz * dz)) <= EPS_HISTORY_CYCLIC_POINT_SUBSEQUENCE_E12
    };
    let matches_orientation = |candidate: &[cadmpeg_ir::math::Point3]| {
        construction.iter().enumerate().any(|(start, point)| {
            if !coincident(&candidate[0], point) {
                return false;
            }
            let mut cursor = start;
            candidate.iter().skip(1).all(|target| {
                let limit = start + construction.len();
                while cursor < limit {
                    cursor += 1;
                    if coincident(target, &construction[cursor % construction.len()]) {
                        return true;
                    }
                }
                false
            })
        })
    };
    if candidate.is_empty() || candidate.len() > construction.len() {
        return false;
    }
    if matches_orientation(candidate) {
        return true;
    }
    let reversed_matches = construction.iter().enumerate().any(|(start, point)| {
        if !coincident(&candidate[candidate.len() - 1], point) {
            return false;
        }
        let mut cursor = start;
        candidate.iter().rev().skip(1).all(|target| {
            let limit = start + construction.len();
            while cursor < limit {
                cursor += 1;
                if coincident(target, &construction[cursor % construction.len()]) {
                    return true;
                }
            }
            false
        })
    });
    reversed_matches
}

pub(crate) fn bind_body_recipe_operand_history_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    recipes: &[crate::records::recipes::ConstructionRecipe],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut complete_bodies = SnapshotCache::default();
    let mut state_indexes = SnapshotCache::default();
    if projection_was_finalized(histories) {
        return Ok(());
    }
    let mut body_closures = HistoricalBodyClosureCache::default();
    for operand in operands.iter_mut() {
        for reference in operand.reference_bindings_mut() {
            reference.preceding_candidate_faces.clear();
            reference.preceding_body_slots.clear();
        }
        operand.resolved_face_slot = None;
        operand.resolved_body_state_id = None;
        operand.resolved_body_slot = None;
        operand.resolved_body_face_slots.clear();
        let Some((history, state, previous)) =
            body_recipe_operand_history_pair(operand, scopes, histories)
        else {
            continue;
        };
        let states = state_indexes.get(decode, history, history_state_index)?;
        let Some(topology) = previous.topology() else {
            continue;
        };
        if face_changes_across_state_chain(decode, state, previous.state_id, states)?.is_none() {
            continue;
        }
        let Some(source) = historical_brep_source(&previous.id) else {
            continue;
        };
        for reference in operand.reference_bindings_mut() {
            *reference.preceding_candidate_faces = selection::faces_in_topology(
                decode,
                reference
                    .candidate_faces
                    .iter()
                    .filter(|face| active_brep_face_matches_source(face, source)),
                topology,
            )?;
            let mut face_slots = BTreeSet::new();
            for face in reference
                .preceding_candidate_faces
                .iter()
                .filter_map(|face| stable_ref(face.as_str()))
            {
                decode
                    .insert_btree_set(&mut face_slots, face, "index F3D body recipe face slots")
                    .map(|_| ())?;
            }
            let Some(body_slots) = body_closures.intersecting(decode, topology, &face_slots)?
            else {
                continue;
            };
            *reference.preceding_body_slots =
                decode.collect_vec(body_slots, "collect F3D body recipe preceding bodies")?;
        }
        if let [reference] = operand.references().as_slice() {
            if let [face] = reference.preceding_candidate_faces.as_slice() {
                operand.resolved_face_slot = stable_ref(face.as_str());
            }
        }
        let Some(first) = operand.references().first() else {
            continue;
        };
        if first.preceding_body_slots.is_empty()
            || operand
                .references()
                .iter()
                .any(|reference| reference.preceding_body_slots.is_empty())
        {
            continue;
        }
        let mut intersection = BTreeSet::new();
        for body in &first.preceding_body_slots {
            decode
                .insert_btree_set(
                    &mut intersection,
                    *body,
                    "index F3D body recipe intersection",
                )
                .map(|_| ())?;
        }
        for reference in &operand.references()[1..] {
            intersection.retain(|body| reference.preceding_body_slots.contains(body));
        }
        if intersection.len() == 1 {
            operand.resolved_body_slot = intersection.into_iter().next();
        }
    }
    let mut recipes_by_id =
        HashMap::<_, Option<&crate::records::recipes::ConstructionRecipe>>::new();
    for recipe in recipes {
        if !recipes_by_id.contains_key(recipe.id.as_str()) {
            decode.reserve_map(&mut recipes_by_id, 1, "index F3D body recipes by id")?;
        }
        recipes_by_id
            .entry(recipe.id.as_str())
            .and_modify(|recipe| *recipe = None)
            .or_insert(Some(recipe));
    }
    let identity = |operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand| -> Result<Option<_>, cadmpeg_core::CodecError> {
        let Some(stream) = crate::ids::native_stream(&operand.id) else { return Ok(None) };
        let Some(recipe) = recipes_by_id
            .get(operand.recipe_id.as_str())
            .and_then(|recipe| *recipe) else { return Ok(None) };
        let Some(design) = recipe.design.as_ref() else { return Ok(None) };
        let Some(selector) = design.selector else { return Ok(None) };
        Ok(Some((
            decode.copy_retained_text(stream, "copy F3D body recipe identity stream")?,
            operand.asset_id.clone(),
            operand.context_id.clone(),
            decode.collect_vec(operand
                .references()
                .iter()
                .map(|reference| (reference.design_reference, reference.form)), "collect F3D body recipe identity references")?,
            decode.copy_retained_text(&design.id.value, "copy F3D body recipe design id")?,
            selector.value,
        )))
    };
    let mut resolved_by_identity = HashMap::new();
    for operand in operands.iter() {
        let (Some(identity), Some(body)) = (identity(operand)?, operand.resolved_body_slot) else {
            continue;
        };
        if !resolved_by_identity.contains_key(&identity) {
            decode.reserve_map(
                &mut resolved_by_identity,
                1,
                "index F3D resolved body recipe identities",
            )?;
        }
        resolved_by_identity
            .entry(identity)
            .and_modify(|resolved| {
                if *resolved != Some(body) {
                    *resolved = None;
                }
            })
            .or_insert(Some(body));
    }
    for operand in operands {
        if operand.resolved_body_slot.is_none() {
            operand.resolved_body_slot = identity(operand)?
                .and_then(|identity| resolved_by_identity.get(&identity).copied())
                .flatten();
        }
        let Some(body_slot) = operand.resolved_body_slot else {
            continue;
        };
        let Some((_, _, previous)) = body_recipe_operand_history_pair(operand, scopes, histories)
        else {
            continue;
        };
        let Some(topology) = previous.topology() else {
            continue;
        };
        let Some(faces) = complete_bodies
            .get(decode, topology, CompleteBodyIndex::new)?
            .faces(decode, body_slot)?
        else {
            continue;
        };
        operand.resolved_body_state_id = Some(previous.state_id);
        operand.resolved_body_face_slots = faces;
    }
    Ok(())
}

fn body_recipe_operand_history_pair<'a>(
    operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &'a [AsmHistory],
) -> Option<(&'a AsmHistory, &'a AsmDeltaState, &'a AsmDeltaState)> {
    let stream = crate::ids::native_stream(&operand.id)?;
    let mut matching_scopes = scopes.iter().filter(|scope| {
        scope.record_index == operand.scope_record_index
            && crate::ids::native_stream(&scope.id) == Some(stream)
    });
    let scope = matching_scopes.next()?;
    if matching_scopes.next().is_some() {
        return None;
    }
    let state_id = scope.history_state_id()?;
    let previous_state_id = effective_scope_previous_history_state_id(scope, histories)?;
    let (history, state, previous) =
        unique_history_state_pair(histories, state_id, previous_state_id)?;
    Some((history, state, previous))
}

fn active_brep_face_matches_source(face: &cadmpeg_ir::ids::FaceId, source: &str) -> bool {
    face.as_str().starts_with("f3d:brep:entity#") || face_in_brep_source(face.as_str(), source)
}

fn face_in_brep_source(id: &str, source: &str) -> bool {
    id.strip_prefix("f3d:brep/")
        .and_then(|tail| tail.strip_prefix(source))
        .is_some_and(|tail| tail.starts_with('/'))
}

#[derive(Debug, PartialEq)]
enum LegacyFaceResolution {
    Historical(i64),
    Active(cadmpeg_ir::ids::FaceId),
}

fn select_legacy_extrude_face_candidate(
    mut candidates: Vec<cadmpeg_ir::ids::FaceId>,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
    history_source: Option<&str>,
) -> Option<LegacyFaceResolution> {
    for changed_only in [true, false] {
        let mut faces = candidates.iter().filter(|face| {
            stable_ref(face.as_str()).is_some_and(|slot| {
                topology.faces.contains(&slot) && (!changed_only || changed_faces.contains(&slot))
            })
        });
        let face = faces.next();
        if faces.next().is_none() {
            let Some(face) = face else {
                continue;
            };
            if let Some(slot) = stable_ref(face.as_str()) {
                return Some(LegacyFaceResolution::Historical(slot));
            }
        }
    }
    if let Some(source) = history_source {
        let unique_index = {
            let mut matches = candidates
                .iter()
                .enumerate()
                .filter(|(_, face)| face_in_brep_source(face.as_str(), source));
            let first = matches.next().map(|(index, _)| index);
            first.filter(|_| matches.next().is_none())
        };
        if let Some(index) = unique_index {
            return Some(LegacyFaceResolution::Active(candidates.swap_remove(index)));
        }
    }
    if candidates.len() == 1 {
        candidates.pop().map(LegacyFaceResolution::Active)
    } else {
        None
    }
}

/// One body/native selection row, minting the non-blank native member at the
/// boundary where the F3D record states it.
///
/// `None` when the record states a blank native member: a row the IR carrier
/// does not hold.
fn body_member<B>(body: B, native: String) -> Option<cadmpeg_ir::features::BodyMember<B>> {
    Some(cadmpeg_ir::features::BodyMember::new(
        body,
        cadmpeg_core::text::NonBlankString::new(native)?,
    ))
}

fn historical_brep_source(state_id: &str) -> Option<&str> {
    state_id
        .rsplit_once("/BREP.")
        .or_else(|| state_id.rsplit_once("BREP."))?
        .1
        .split_once(":asm-")
        .map(|(source, _)| source)
}

fn resolve_direct_face_recipe_clauses(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    references: &[crate::records::dimensions::DesignRecipeReference],
    topology: &crate::history_records::AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Result<Vec<i64>, cadmpeg_core::CodecError> {
    let mut clauses = Vec::<(
        u64,
        u64,
        Vec<&crate::records::dimensions::DesignRecipeReference>,
    )>::new();
    for reference in references {
        let key = (reference.selector_offset, reference.token_offset);
        if let Some((_, _, references)) = clauses
            .iter_mut()
            .find(|(selector, token, _)| (*selector, *token) == key)
        {
            decode.reserve_vec(references, 1, "group F3D direct face references")?;
            references.push(reference);
        } else {
            decode.reserve_vec(&mut clauses, 1, "collect F3D direct face clauses")?;

            let mut grouped = Vec::new();
            decode.reserve_vec(&mut grouped, 1, "group F3D direct face references")?;
            grouped.push(reference);
            clauses.push((key.0, key.1, grouped));
        }
    }
    let mut topology_faces = HashSet::new();
    for face in &topology.faces {
        decode.insert_hash_set(
            &mut topology_faces,
            *face,
            "index F3D direct topology faces",
        )?;
    }
    let mut resolved = Vec::new();
    for (_, _, references) in clauses {
        let mut intersection = None::<HashSet<i64>>;
        for reference in references {
            let candidates = if reference.candidate_faces.is_empty() {
                &reference.alternate_selector_faces
            } else {
                &reference.candidate_faces
            };
            let mut eligible = HashSet::new();
            for face in candidates
                .iter()
                .filter_map(|face| stable_ref(face.as_str()))
                .filter(|face| topology_faces.contains(face) && changed_faces.contains(face))
            {
                decode.insert_hash_set(
                    &mut eligible,
                    face,
                    "collect F3D direct face clause candidates",
                )?;
            }
            let candidates = eligible;
            if candidates.is_empty() {
                return Ok(Vec::new());
            }
            intersection = Some(match intersection {
                None => candidates,
                Some(mut intersection) => {
                    intersection.retain(|face| candidates.contains(face));
                    intersection
                }
            });
        }
        let Some(intersection) = intersection else {
            return Ok(Vec::new());
        };
        let mut candidates = intersection.into_iter();
        let Some(face) = candidates.next() else {
            return Ok(Vec::new());
        };
        if candidates.next().is_some() {
            return Ok(Vec::new());
        }
        if !resolved.contains(&face) {
            decode.reserve_vec(&mut resolved, 1, "collect F3D resolved direct faces")?;
            resolved.push(face);
        }
    }
    Ok(resolved)
}

fn bind_profile_face_group_cardinality(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::face::DesignFaceOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    operand_groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut state_indexes = SnapshotCache::default();
    for scope in scopes {
        let scoped_history = if scope_histories.contains_key(&scope.id) {
            let Some(history) = bound_scope_history(&scope.id, scope_histories, histories) else {
                continue;
            };
            Some(history)
        } else {
            None
        };
        let scoped_histories = scoped_history.map_or(histories, std::slice::from_ref);
        let Some(profile_groups) = crate::design::face_resolve::extrude_profile_group_roots(
            decode,
            scope,
            operand_groups,
        )?
        else {
            continue;
        };
        for group in profile_groups {
            let Some(indices) = crate::design::face_resolve::extrude_profile_group_operand_indices(
                decode,
                group,
                operand_groups,
                operands,
            )?
            else {
                continue;
            };
            if group.members().len() != indices.len()
                || indices.iter().any(|index| {
                    let operand = &operands[*index];
                    !operand.resolved_face_slots.is_empty()
                        || operand.resolved_active_face.is_some()
                })
            {
                continue;
            }
            let (Some(state_id), Some(previous_state_id)) = (
                scope.history_state_id(),
                effective_scope_previous_history_state_id(scope, scoped_histories),
            ) else {
                continue;
            };
            let history_pair = if scoped_history.is_some() {
                bound_history_state_pair(
                    &scope.id,
                    state_id,
                    previous_state_id,
                    scope_histories,
                    histories,
                )
            } else {
                unique_history_state_pair(histories, state_id, previous_state_id)
            };
            let Some((history, state, previous)) = history_pair else {
                continue;
            };
            let states = state_indexes.get(decode, history, history_state_index)?;
            let changed_faces =
                face_changes_across_state_chain(decode, state, previous_state_id, states)?;
            let (Some(topology), Some(changed_faces)) = (previous.topology(), changed_faces) else {
                continue;
            };
            let paired_aggregate = crate::design::face_resolve::is_paired_extrude_profile_aggregate(
                group,
                operand_groups,
                operands,
            );
            let faces =
                if paired_aggregate {
                    if let Some(transition) = state.transition.as_ref().filter(|transition| {
                        transition.previous_state_id == Some(previous_state_id)
                    }) {
                        let mut preceding_faces = HashSet::new();
                        for face in &topology.faces {
                            decode.insert_hash_set(
                                &mut preceding_faces,
                                *face,
                                "index F3D profile preceding faces",
                            )?;
                        }
                        let mut deleted = decode.collect_vec(
                            transition.topology.faces.deleted.iter().copied(),
                            "copy F3D profile deleted faces",
                        )?;
                        decode.sort_unstable_by(
                            &mut deleted,
                            Ord::cmp,
                            |_| 0,
                            "sort F3D profile deleted faces",
                        )?;
                        deleted.dedup();
                        (deleted.len() == group.members().len()
                            && deleted.len() == transition.topology.faces.deleted.len()
                            && deleted.iter().all(|face| {
                                preceding_faces.contains(face)
                                    && topology
                                        .face_surfaces
                                        .iter()
                                        .filter(|binding| binding.entity == *face)
                                        .count()
                                        == 1
                            }))
                        .then_some(deleted)
                    } else {
                        None
                    }
                } else {
                    profile_face_group_cardinality_candidates(
                        decode,
                        topology,
                        &changed_faces,
                        group.members().len(),
                    )?
                };
            let Some(faces) = faces else {
                continue;
            };
            for (index, face) in indices.into_iter().zip(faces) {
                let face_id = historical_face_id(decode, face)?;
                let preceding_id =
                    face_id.try_clone_for_decode(decode, "copy F3D historical face identity")?;
                operands[index].preceding_candidate_faces = decode.collect_vec(
                    std::iter::once(preceding_id),
                    "bind F3D profile preceding face",
                )?;
                operands[index].changed_candidate_faces = decode
                    .collect_vec(std::iter::once(face_id), "bind F3D profile changed face")?;
                operands[index].resolved_face_slots = vec![face];
            }
        }
    }
    Ok(())
}

fn profile_face_group_cardinality_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
    member_count: usize,
) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError> {
    let mut preceding_faces = HashSet::new();
    for face in &topology.faces {
        decode.insert_hash_set(
            &mut preceding_faces,
            *face,
            "index F3D profile candidate faces",
        )?;
    }
    let mut faces_by_carrier = HashMap::<i64, Vec<i64>>::new();
    for face in changed_faces
        .iter()
        .copied()
        .filter(|face| preceding_faces.contains(face))
    {
        let mut bindings = topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == face);
        let Some(carrier) = bindings.next().map(|binding| binding.carrier) else {
            continue;
        };
        if bindings.next().is_none() {
            decode.push_hash_group(
                &mut faces_by_carrier,
                carrier,
                face,
                "index F3D profile face carriers",
                "collect F3D profile carrier faces",
            )?;
        }
    }
    let mut candidates = faces_by_carrier
        .into_values()
        .filter(|faces| faces.len() == member_count);
    let Some(mut faces) = candidates.next() else {
        return Ok(None);
    };
    if candidates.next().is_some() {
        return Ok(None);
    }
    decode.sort_unstable_by(
        &mut faces,
        Ord::cmp,
        |_| 0,
        "sort F3D profile carrier faces",
    )?;
    faces.dedup();
    Ok((faces.len() == member_count).then_some(faces))
}

fn face_changes_across_state_chain<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Result<Option<HashSet<i64>>, cadmpeg_core::CodecError> {
    let mut current = state;
    let mut visited = HashSet::new();
    let mut changed = HashSet::new();
    while current.state_id != previous_state_id {
        if !decode.insert_hash_set(
            &mut visited,
            current.state_id,
            "track F3D face change state chain",
        )? {
            return Ok(None);
        }
        let Some(transition) = current.transition.as_ref() else {
            return Ok(None);
        };
        for face in transition
            .topology
            .faces
            .deleted
            .iter()
            .chain(&transition.topology.faces.updated)
        {
            decode.insert_hash_set(&mut changed, *face, "collect F3D changed faces")?;
        }
        let Some(previous) = transition
            .previous_state_id
            .and_then(|id| states.get(&id).copied().flatten())
        else {
            return Ok(None);
        };
        current = previous;
    }
    Ok(Some(changed))
}

#[derive(Debug, PartialEq)]
struct EdgeChanges {
    deleted: HashSet<i64>,
    updated: HashSet<i64>,
}

fn edge_changes_across_state_chain<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Result<Option<EdgeChanges>, cadmpeg_core::CodecError> {
    let mut current = state;
    let mut visited = HashSet::new();
    let mut deleted = HashSet::new();
    let mut updated = HashSet::new();
    while current.state_id != previous_state_id {
        if !decode.insert_hash_set(
            &mut visited,
            current.state_id,
            "track F3D edge change state chain",
        )? {
            return Ok(None);
        }
        let Some(transition) = current.transition.as_ref() else {
            return Ok(None);
        };
        for edge in &transition.topology.edges.deleted {
            decode.insert_hash_set(&mut deleted, *edge, "collect F3D deleted edges")?;
        }
        for edge in &transition.topology.edges.updated {
            decode.insert_hash_set(&mut updated, *edge, "collect F3D updated edges")?;
        }
        let Some(previous) = transition
            .previous_state_id
            .and_then(|id| states.get(&id).copied().flatten())
        else {
            return Ok(None);
        };
        current = previous;
    }
    Ok(Some(EdgeChanges { deleted, updated }))
}

fn historical_face_support_contexts<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &[cadmpeg_ir::ids::FaceId],
    history: &'a AsmHistory,
    preceding_topology: &'a AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<
    Vec<crate::records::topology::historical_context::DesignHistoricalFaceSupportContext>,
    cadmpeg_core::CodecError,
> {
    let mut preceding_faces = HashSet::new();
    for face in &preceding_topology.faces {
        decode.insert_hash_set(
            &mut preceding_faces,
            *face,
            "index F3D historical support faces",
        )?;
    }
    let mut contexts = Vec::new();
    'candidates: for candidate in candidates {
        let Some(active_face_slot) = stable_ref(candidate.as_str()) else {
            continue;
        };
        let mut preceding_bindings = preceding_topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == active_face_slot);
        let surface_slot = if let Some(binding) = preceding_bindings.next() {
            if preceding_bindings.next().is_some() {
                continue;
            }
            binding.carrier
        } else {
            let mut carriers = HashSet::new();
            for topology in history.states.iter().filter_map(|state| state.topology()) {
                let mut bindings = topology
                    .face_surfaces
                    .iter()
                    .filter(|binding| binding.entity == active_face_slot);
                if let Some(binding) = bindings.next() {
                    if bindings.next().is_some() {
                        continue 'candidates;
                    }
                    decode.insert_hash_set(
                        &mut carriers,
                        binding.carrier,
                        "index F3D historical support carriers",
                    )?;
                }
            }
            if carriers.len() != 1 {
                continue;
            }
            let Some(carrier) = carriers.into_iter().next() else {
                continue;
            };
            carrier
        };
        let mut preceding_face_slots = decode.collect_vec(
            preceding_topology
                .face_surfaces
                .iter()
                .filter(|binding| {
                    binding.carrier == surface_slot && preceding_faces.contains(&binding.entity)
                })
                .map(|binding| binding.entity),
            "collect F3D historical support face slots",
        )?;
        decode.sort_unstable_by(
            &mut preceding_face_slots,
            Ord::cmp,
            |_| 0,
            "sort F3D historical support face slots",
        )?;
        preceding_face_slots.dedup();
        if preceding_face_slots.is_empty() {
            continue;
        }
        let changed_preceding_face_slots = decode.collect_vec(
            preceding_face_slots
                .iter()
                .copied()
                .filter(|face| changed_faces.contains(face)),
            "collect F3D changed support face slots",
        )?;
        let preceding_face_boundaries = face_boundary_contexts_for_slots(
            decode,
            &preceding_face_slots,
            preceding_topology,
            topology_queries,
        )?;

        decode.reserve_vec(&mut contexts, 1, "collect F3D historical support contexts")?;
        contexts.push(
            crate::records::topology::historical_context::DesignHistoricalFaceSupportContext {
                active_face_slot,
                surface_slot,
                preceding_face_slots,
                preceding_face_boundaries,
                changed_preceding_face_slots,
            },
        );
    }
    Ok(contexts)
}

fn face_boundary_edges<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    faces: &[cadmpeg_ir::ids::FaceId],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<Vec<i64>, cadmpeg_core::CodecError> {
    let index =
        topology_queries
            .boundaries
            .get(decode, topology, topology_cache::aggregate_boundaries)?;
    let mut selected = HashSet::new();
    for face in faces.iter().filter_map(|face| stable_ref(face.as_str())) {
        decode.charge_work(1, "query F3D boundary faces")?;
        for edge in index.get(&face).into_iter().flatten() {
            decode.charge_work(1, "query F3D boundary edges")?;
            decode.insert_hash_set(&mut selected, *edge, "collect F3D boundary edges")?;
        }
    }
    let mut edges = decode.collect_vec(selected, "collect F3D boundary edges")?;
    decode.sort_unstable_by(&mut edges, Ord::cmp, |_| 0, "sort F3D boundary edges")?;
    edges.dedup();
    Ok(edges)
}

fn collect_reference_edge_sets<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    reference_faces: &[Vec<cadmpeg_ir::ids::FaceId>],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<Vec<Vec<i64>>, cadmpeg_core::CodecError> {
    let mut sets = Vec::new();
    for faces in reference_faces {
        let faces = selection::faces_in_topology(decode, faces, topology)?;
        let edges = face_boundary_edges(decode, &faces, topology, topology_queries)?;

        decode.reserve_vec(&mut sets, 1, "collect F3D reference edge sets")?;
        sets.push(edges);
    }
    Ok(sets)
}

fn face_boundary_contexts<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    faces: &[cadmpeg_ir::ids::FaceId],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<
    Vec<crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext>,
    cadmpeg_core::CodecError,
> {
    let face_slots = decode.collect_vec(
        faces.iter().filter_map(|face| stable_ref(face.as_str())),
        "collect F3D boundary face slots",
    )?;
    face_boundary_contexts_for_slots(decode, &face_slots, topology, topology_queries)
}

fn face_boundary_contexts_for_slots<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    face_slots: &[i64],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<
    Vec<crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext>,
    cadmpeg_core::CodecError,
> {
    let index = topology_queries
        .loops
        .get(decode, topology, LoopIndex::new)?;
    let edge_endpoints =
        topology_queries
            .endpoints
            .get(decode, topology, topology_cache::endpoint_index)?;
    let mut contexts = Vec::new();
    'faces: for face_slot in face_slots {
        let Some(face_loops) = index.face_loops.get(face_slot).copied().flatten() else {
            continue;
        };
        let mut loops = Vec::new();
        for loop_slot in face_loops {
            let Some(loop_coedges) = index.loop_coedges.get(loop_slot).copied().flatten() else {
                continue 'faces;
            };
            let mut coedges = Vec::new();
            for coedge_slot in loop_coedges {
                let Some(edge_slot) = index.coedge_edges.get(coedge_slot).copied().flatten() else {
                    continue 'faces;
                };
                decode.reserve_vec(&mut coedges, 1, "collect F3D face loop coedges")?;
                coedges.push(
                    crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
                        coedge_slot: *coedge_slot,
                        edge_slot,
                    },
                );
            }
            let boundary = historical_loop_boundary(decode, coedges, index, edge_endpoints)?;

            decode.reserve_vec(&mut loops, 1, "collect F3D face boundary loops")?;
            loops.push(
                crate::records::topology::historical_context::DesignHistoricalFaceLoopContext {
                    loop_slot: *loop_slot,
                    boundary,
                },
            );
        }

        decode.reserve_vec(&mut contexts, 1, "collect F3D face boundary contexts")?;
        contexts.push(
            crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext {
                face_slot: *face_slot,
                loops,
            },
        );
    }
    Ok(contexts)
}

fn historical_loop_boundary(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    coedges: Vec<crate::records::topology::historical_context::DesignHistoricalLoopCoedge>,
    index: &LoopIndex<'_>,
    edge_endpoints: &topology_cache::EdgeEndpoints,
) -> Result<
    crate::records::topology::historical_context::DesignHistoricalLoopBoundary,
    cadmpeg_core::CodecError,
> {
    use crate::records::topology::{
        historical_context::DesignHistoricalLoopBoundary,
        historical_context::DesignHistoricalLoopPoint,
        historical_context::DesignHistoricalLoopPosition,
        historical_context::DesignHistoricalLoopVertex,
    };
    let mut vertices = Vec::new();
    for (ordinal, coedge) in coedges.iter().enumerate() {
        let previous = coedges[(ordinal + coedges.len() - 1) % coedges.len()].edge_slot;
        decode.charge_work(2, "query F3D historical loop endpoints")?;
        let endpoints = |slot| edge_endpoints.get(&slot).copied().flatten();
        let (Some(previous), Some(current)) = (endpoints(previous), endpoints(coedge.edge_slot))
        else {
            return Ok(DesignHistoricalLoopBoundary::Coedges(coedges));
        };
        let mut shared = previous
            .into_iter()
            .filter(|vertex| current.contains(vertex));
        let Some(vertex_slot) = shared.next() else {
            return Ok(DesignHistoricalLoopBoundary::Coedges(coedges));
        };
        if shared.any(|vertex| vertex != vertex_slot) {
            return Ok(DesignHistoricalLoopBoundary::Coedges(coedges));
        }

        decode.reserve_vec(&mut vertices, 1, "collect F3D loop vertices")?;
        vertices.push(DesignHistoricalLoopVertex {
            coedge: coedge.clone(),
            vertex_slot,
        });
    }
    if vertices.is_empty() {
        return Ok(DesignHistoricalLoopBoundary::Coedges(coedges));
    }
    let mut points = Vec::new();
    for vertex in &vertices {
        decode.charge_work(1, "query F3D historical loop points")?;
        let Some(point_slot) = index
            .vertex_points
            .get(&vertex.vertex_slot)
            .copied()
            .flatten()
        else {
            return Ok(DesignHistoricalLoopBoundary::Vertices(vertices));
        };
        decode.reserve_vec(&mut points, 1, "collect F3D loop points")?;
        points.push(DesignHistoricalLoopPoint {
            vertex: vertex.clone(),
            point_slot,
        });
    }
    let mut positions = Vec::new();
    for point in &points {
        decode.charge_work(1, "query F3D historical loop positions")?;
        let Some(position) = index.positions.get(&point.point_slot).copied().flatten() else {
            return Ok(DesignHistoricalLoopBoundary::Points(points));
        };
        decode.reserve_vec(&mut positions, 1, "collect F3D loop positions")?;
        positions.push(DesignHistoricalLoopPosition {
            point: point.clone(),
            position,
        });
    }
    Ok(DesignHistoricalLoopBoundary::Positions(positions))
}

fn preceding_support_face_slots(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    result_faces: &[cadmpeg_ir::ids::FaceId],
    result_topology: &AsmHistoricalTopology,
    preceding_topology: &AsmHistoricalTopology,
) -> Result<Vec<i64>, cadmpeg_core::CodecError> {
    let mut preceding_faces = HashSet::new();
    for face in &preceding_topology.faces {
        decode.insert_hash_set(
            &mut preceding_faces,
            *face,
            "index F3D preceding support faces",
        )?;
    }
    let mut support_faces = Vec::new();
    for result_face in result_faces {
        let Some(result_face) = stable_ref(result_face.as_str()) else {
            continue;
        };
        let mut result_bindings = result_topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == result_face);
        let Some(carrier) = result_bindings.next().map(|binding| binding.carrier) else {
            continue;
        };
        if result_bindings.next().is_some() {
            continue;
        }
        let mut preceding_bindings = preceding_topology.face_surfaces.iter().filter(|binding| {
            binding.carrier == carrier && preceding_faces.contains(&binding.entity)
        });
        let Some(preceding_face) = preceding_bindings.next().map(|binding| binding.entity) else {
            continue;
        };
        if preceding_bindings.next().is_none() && !support_faces.contains(&preceding_face) {
            decode.reserve_vec(&mut support_faces, 1, "collect F3D preceding support faces")?;
            support_faces.push(preceding_face);
        }
    }
    Ok(support_faces)
}

#[derive(Clone, Copy)]
struct EdgeBoundaryContext<'a, 'b> {
    topology: &'a AsmHistoricalTopology,
    boundary_edges: &'b [i64],
}

fn edge_recipe_reference_context<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    reference_ordinal: u32,
    reference: &crate::records::dimensions::DesignRecipeReference,
    result: EdgeBoundaryContext<'a, '_>,
    preceding: EdgeBoundaryContext<'a, '_>,
    changed_edges: &HashSet<i64>,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<
    crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext,
    cadmpeg_core::CodecError,
> {
    let candidate_faces = if reference.candidate_faces.is_empty() {
        reference.alternate_selector_faces.as_slice()
    } else {
        reference.candidate_faces.as_slice()
    };
    let result_faces = selection::faces_in_topology(decode, candidate_faces, result.topology)?;
    let result_face_boundaries =
        face_boundary_contexts(decode, &result_faces, result.topology, topology_queries)?;
    let result_edges =
        face_boundary_edges(decode, &result_faces, result.topology, topology_queries)?;
    let result_shared_edge_slots = decode.collect_vec(
        result
            .boundary_edges
            .iter()
            .copied()
            .filter(|edge| result_edges.contains(edge)),
        "collect F3D result shared edges",
    )?;
    let preceding_faces =
        selection::faces_in_topology(decode, candidate_faces, preceding.topology)?;
    let preceding_face_boundaries = face_boundary_contexts(
        decode,
        &preceding_faces,
        preceding.topology,
        topology_queries,
    )?;
    let preceding_support_face_slots =
        preceding_support_face_slots(decode, &result_faces, result.topology, preceding.topology)?;
    let preceding_support_face_boundaries = face_boundary_contexts_for_slots(
        decode,
        &preceding_support_face_slots,
        preceding.topology,
        topology_queries,
    )?;
    let preceding_edges = face_boundary_edges(
        decode,
        &preceding_faces,
        preceding.topology,
        topology_queries,
    )?;
    let shared_edge_slots = decode.collect_vec(
        preceding
            .boundary_edges
            .iter()
            .copied()
            .filter(|edge| preceding_edges.contains(edge)),
        "collect F3D shared edge slots",
    )?;
    let changed_shared_edge_slots = decode.collect_vec(
        shared_edge_slots
            .iter()
            .copied()
            .filter(|edge| changed_edges.contains(edge)),
        "collect F3D changed shared edges",
    )?;
    let mut support_edges = HashSet::new();
    for edge in preceding_support_face_boundaries
        .iter()
        .flat_map(|face| &face.loops)
        .flat_map(|face_loop| face_loop.boundary.coedges().map(|row| row.edge_slot))
    {
        decode.insert_hash_set(&mut support_edges, edge, "index F3D support edges")?;
    }
    let mut changed_reference_edge_slots = decode.collect_vec(
        preceding_edges
            .iter()
            .copied()
            .chain(support_edges.iter().copied())
            .filter(|edge| changed_edges.contains(edge)),
        "collect F3D changed reference edges",
    )?;
    decode.sort_unstable_by(
        &mut changed_reference_edge_slots,
        Ord::cmp,
        |_| 0,
        "sort F3D changed reference edges",
    )?;
    changed_reference_edge_slots.dedup();
    Ok(
        crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
            reference_ordinal,
            result_faces,
            result_face_boundaries,
            result_shared_edge_slots,
            preceding_faces,
            preceding_face_boundaries,
            preceding_support_face_slots,
            preceding_support_face_boundaries,
            shared_edge_slots,
            changed_shared_edge_slots,
            changed_reference_edge_slots,
        },
    )
}

/// Resolve the unique candidate edge shared by the non-null face references
/// in the first side of a standard edge recipe.
fn side_one_recipe_edge(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    structure: Option<&crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>,
    reference_contexts: &[crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext],
    selectors: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    candidate_edges: &[i64],
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(side) = structure.and_then(|structure| structure.sides.first()) else {
        return Ok(None);
    };
    let mut ordinals = Vec::new();
    for value in std::iter::once(side.header_value)
        .chain(side.scalars.iter().copied())
        .filter(|value| *value != 0)
    {
        let Some(ordinal) = usize::try_from(value)
            .ok()
            .and_then(|value| value.checked_sub(1))
        else {
            return Ok(None);
        };

        decode.reserve_vec(&mut ordinals, 1, "collect F3D recipe side ordinals")?;
        ordinals.push(ordinal);
    }
    decode.sort_unstable_by(
        &mut ordinals,
        Ord::cmp,
        |_| 0,
        "sort F3D recipe side ordinals",
    )?;
    ordinals.dedup();
    let mut edge_sets = Vec::new();
    for ordinal in ordinals {
        let Some(context) = reference_contexts.get(ordinal) else {
            return Ok(None);
        };
        if Some(context.reference_ordinal) != u32::try_from(ordinal).ok() {
            return Ok(None);
        }

        decode.reserve_vec(&mut edge_sets, 1, "collect F3D recipe side edge sets")?;
        edge_sets.push(context.shared_edge_slots.as_slice());
    }
    if edge_sets.iter().any(|edges| edges.is_empty()) {
        return Ok(None);
    }
    let Some(first) = edge_sets.first() else {
        return Ok(None);
    };
    let mut candidates = decode.collect_vec(first.iter().copied(), "copy F3D recipe side edges")?;
    for edges in edge_sets.iter().skip(1) {
        candidates.retain(|candidate| edges.contains(candidate));
    }
    candidates.retain(|candidate| candidate_edges.contains(candidate));
    decode.sort_unstable_by(
        &mut candidates,
        Ord::cmp,
        |_| 0,
        "sort F3D recipe side edges",
    )?;
    candidates.dedup();
    match candidates.as_slice() {
        [edge] => Ok(Some(*edge)),
        _ => Ok(
            crate::design::edge_resolve::resolved_edge_candidate_intersection(selectors, edge_sets),
        ),
    }
}

pub(crate) fn bind_edge_operand_history_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::edge_identity::DesignEdgeOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    recipes: &[crate::records::recipes::ConstructionRecipe],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut treatment_cache = TreatmentCache::default();
    let mut topology_queries = TopologyQueryCache::default();
    let mut state_indexes = SnapshotCache::default();
    let mut recipe_topologies = RecipeTopologyCache::default();
    if projection_was_finalized(histories) {
        return Ok(());
    }
    let mut recipe_record_indices = HashMap::new();
    for recipe in recipes {
        if let Some(index) = recipe.record_index {
            if !recipe_record_indices.contains_key(recipe.id.as_str()) {
                decode.reserve_map(&mut recipe_record_indices, 1, "index F3D edge recipes")?;
            }
            recipe_record_indices.insert(recipe.id.as_str(), index.value);
        }
    }
    let mut terminal_topologies = Vec::new();
    for history in histories {
        let mut preceding = HashSet::new();
        for state in &history.states {
            if let Some(previous) = state
                .transition
                .as_ref()
                .and_then(|transition| transition.previous_state_id)
            {
                decode.insert_hash_set(
                    &mut preceding,
                    previous,
                    "index F3D terminal predecessors",
                )?;
            }
        }
        let mut terminals = history
            .states
            .iter()
            .filter(|state| !preceding.contains(&state.state_id));
        if let Some(state) = terminals.next().filter(|_| terminals.next().is_none()) {
            if let Some(topology) = state.topology() {
                decode.reserve_vec(
                    &mut terminal_topologies,
                    1,
                    "collect F3D terminal topologies",
                )?;
                terminal_topologies.push((state.state_id, topology));
            }
        }
    }
    for operand in operands {
        operand.result_candidate_faces.clear();
        operand.result_boundary_edge_slots.clear();
        operand.preceding_candidate_faces.clear();
        operand.terminal_candidate_faces.clear();
        operand.changed_candidate_faces.clear();
        operand.preceding_boundary_edge_slots.clear();
        operand.terminal_boundary_edge_slots.clear();
        operand.changed_boundary_edge_slots.clear();
        operand.deleted_boundary_edge_slots.clear();
        operand.updated_boundary_edge_slots.clear();
        operand.treatment_radius_candidates.clear();
        operand.changed_boundary_edge_contexts.clear();
        operand.terminal_boundary_edge_contexts.clear();
        operand.recipe_reference_contexts.clear();
        operand.recipe_selectors.clear();
        operand.recipe_state_id = None;
        operand.resolved_edge_slot = None;
        operand.resolved_axis = None;
        let stream = crate::ids::native_stream(&operand.id);
        let mut matching_scopes = scopes.iter().filter(|scope| {
            scope.record_index == operand.scope_record_index
                && crate::ids::native_stream(&scope.id) == stream
        });
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let Some(state_id) = scope.history_state_id() else {
            bind_active_edge_operand_for_scope(
                decode,
                operand,
                scope,
                &terminal_topologies,
                &mut topology_queries,
            )?;
            continue;
        };
        let Some(previous_state_id) = effective_scope_previous_history_state_id(scope, histories)
        else {
            bind_active_edge_operand_for_scope(
                decode,
                operand,
                scope,
                &terminal_topologies,
                &mut topology_queries,
            )?;
            continue;
        };
        let Some((history, state, previous)) = bound_history_state_pair(
            &scope.id,
            state_id,
            previous_state_id,
            scope_histories,
            histories,
        ) else {
            continue;
        };
        let (Some(result_topology), Some(topology)) = (state.topology(), previous.topology())
        else {
            continue;
        };
        for reference in &mut operand.recipe_references {
            bind_historical_recipe_reference_candidates(
                decode,
                reference,
                topology,
                &mut recipe_topologies,
            )?;
        }
        if let Some(recipe_record_index) = recipe_record_indices.get(operand.recipe_id.as_str()) {
            operand.candidate_faces = historical_recipe_faces(
                decode,
                i64::from(*recipe_record_index),
                topology,
                &mut recipe_topologies,
            )?;
        }
        let states = state_indexes.get(decode, history, history_state_index)?;
        let Some(changed_faces) =
            face_changes_across_state_chain(decode, state, previous_state_id, states)?
        else {
            continue;
        };
        let Some(EdgeChanges {
            deleted: chain_deleted_edges,
            updated: chain_updated_edges,
        }) = edge_changes_across_state_chain(decode, state, previous_state_id, states)?
        else {
            continue;
        };
        let mut preceding_faces = HashSet::new();
        for face in &topology.faces {
            decode.insert_hash_set(
                &mut preceding_faces,
                *face,
                "index F3D preceding edge faces",
            )?;
        }
        let inserted_faces = decode.collect_vec(
            result_topology
                .faces
                .iter()
                .copied()
                .filter(|face| !preceding_faces.contains(face)),
            "collect F3D inserted faces",
        )?;
        let mut result_edges = HashSet::new();
        for edge in &result_topology.edges {
            decode.insert_hash_set(&mut result_edges, *edge, "index F3D result edges")?;
        }
        let deleted_edges =
            decode.collect_vec(
                topology.edges.iter().copied().filter(|edge| {
                    !result_edges.contains(edge) && chain_deleted_edges.contains(edge)
                }),
                "collect F3D deleted edge candidates",
            )?;
        let updated_edges = decode.collect_vec(
            topology.edges.iter().copied().filter(|edge| {
                result_edges.contains(edge)
                    && (chain_deleted_edges.contains(edge) || chain_updated_edges.contains(edge))
            }),
            "collect F3D updated edge candidates",
        )?;
        operand.recipe_state_id = Some(previous_state_id);
        operand.result_candidate_faces =
            selection::faces_in_topology(decode, &operand.candidate_faces, result_topology)?;
        operand.result_boundary_edge_slots = face_boundary_edges(
            decode,
            &operand.result_candidate_faces,
            result_topology,
            &mut topology_queries,
        )?;
        operand.preceding_candidate_faces =
            selection::faces_in_topology(decode, &operand.candidate_faces, topology)?;
        operand.changed_candidate_faces = decode.try_collect_vec(
            (operand.preceding_candidate_faces.iter().filter(|face| {
                stable_ref(face.as_str()).is_some_and(|slot| changed_faces.contains(&slot))
            }))
            .map(|face| face.try_clone_for_decode(decode, "copy F3D historical face identity")),
            "collect F3D changed edge faces",
        )?;
        operand.preceding_boundary_edge_slots = face_boundary_edges(
            decode,
            &operand.preceding_candidate_faces,
            topology,
            &mut topology_queries,
        )?;
        let mut changed_edges = HashSet::new();
        for edge in deleted_edges.iter().chain(&updated_edges) {
            decode.insert_hash_set(
                &mut changed_edges,
                *edge,
                "index F3D changed edge candidates",
            )?;
        }
        operand.changed_boundary_edge_slots = decode.collect_vec(
            operand
                .preceding_boundary_edge_slots
                .iter()
                .copied()
                .filter(|edge| changed_edges.contains(edge)),
            "collect F3D changed boundary edges",
        )?;
        operand.deleted_boundary_edge_slots = selection::boundary_edges_in_changes(
            decode,
            &operand.preceding_boundary_edge_slots,
            &deleted_edges,
        )?;
        operand.updated_boundary_edge_slots = selection::boundary_edges_in_changes(
            decode,
            &operand.preceding_boundary_edge_slots,
            &updated_edges,
        )?;
        operand.treatment_radius_candidates = treatment_radius_candidates(
            decode,
            Some(&operand.result_candidate_faces),
            &inserted_faces,
            result_topology,
            topology,
            &deleted_edges,
            &mut treatment_cache,
        )?;
        operand.changed_boundary_edge_contexts = decode.try_collect_vec(
            operand
                .changed_boundary_edge_slots
                .iter()
                .copied()
                .map(|edge| selection::historical_edge_context(decode, edge, topology)),
            "collect F3D changed boundary contexts",
        )?;
        let mut reference_contexts = Vec::new();
        for (ordinal, reference) in operand.recipe_references.iter().enumerate() {
            let Ok(reference_ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let context = edge_recipe_reference_context(
                decode,
                reference_ordinal,
                reference,
                EdgeBoundaryContext {
                    topology: result_topology,
                    boundary_edges: &operand.result_boundary_edge_slots,
                },
                EdgeBoundaryContext {
                    topology,
                    boundary_edges: &operand.preceding_boundary_edge_slots,
                },
                &changed_edges,
                &mut topology_queries,
            )?;

            decode.reserve_vec(
                &mut reference_contexts,
                1,
                "collect F3D edge recipe contexts",
            )?;
            reference_contexts.push(context);
        }
        operand.recipe_reference_contexts = reference_contexts;
        if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfacePatch
            && operand.surface_patch_recipe_structure.is_some()
        {
            operand.resolved_edge_slot = surface_patch_edge_operand_slot(
                decode,
                operand.surface_patch_recipe_structure.as_ref(),
                &operand.recipe_references,
                topology,
                &mut topology_queries,
            )?;
            continue;
        }
        if crate::design::design_feature_family(&scope.kind())
            == Some(crate::design::DesignFeatureFamily::Sweep)
        {
            let reference_faces = terminal_edge_recipe_reference_faces(
                decode,
                &operand.recipe_references,
                operand.local_topology_references.as_deref(),
            )?;
            let reference_edge_sets = collect_reference_edge_sets(
                decode,
                &reference_faces,
                topology,
                &mut topology_queries,
            )?;
            let mut candidate_edges = BTreeSet::new();
            for edge in reference_edge_sets.iter().flatten() {
                decode
                    .insert_btree_set(
                        &mut candidate_edges,
                        *edge,
                        "index F3D sweep candidate edges",
                    )
                    .map(|_| ())?;
            }
            let contexts = decode.try_collect_vec(
                candidate_edges
                    .into_iter()
                    .map(|edge| selection::historical_edge_context(decode, edge, topology)),
                "collect F3D sweep edge contexts",
            )?;
            operand.recipe_selectors = selection::recipe_selector_candidates(
                decode,
                operand.recipe_structure.as_ref(),
                &contexts,
            )?;
            operand.resolved_edge_slot =
                crate::design::edge_resolve::unique_incidence_edge_shared_by_reference_faces(
                    &operand.recipe_selectors,
                    reference_edge_sets.iter().map(Vec::as_slice),
                );
            continue;
        }
        if crate::design::design_feature_family(&scope.kind())
            == Some(crate::design::DesignFeatureFamily::Revolve)
        {
            let reference_faces = terminal_edge_recipe_reference_faces(
                decode,
                &operand.recipe_references,
                operand.local_topology_references.as_deref(),
            )?;
            let reference_edge_sets = collect_reference_edge_sets(
                decode,
                &reference_faces,
                topology,
                &mut topology_queries,
            )?;
            let mut candidate_edges = BTreeSet::new();
            for edge in reference_edge_sets.iter().flatten() {
                decode
                    .insert_btree_set(
                        &mut candidate_edges,
                        *edge,
                        "index F3D revolve candidate edges",
                    )
                    .map(|_| ())?;
            }
            let contexts = decode.try_collect_vec(
                candidate_edges
                    .into_iter()
                    .map(|edge| selection::historical_edge_context(decode, edge, topology)),
                "collect F3D revolve edge contexts",
            )?;
            operand.recipe_selectors = selection::recipe_selector_candidates(
                decode,
                operand.recipe_structure.as_ref(),
                &contexts,
            )?;
            operand.resolved_edge_slot =
                crate::design::edge_resolve::resolved_edge_candidate_intersection(
                    &operand.recipe_selectors,
                    reference_edge_sets.iter().map(Vec::as_slice),
                );
            let resolved_axis = if let Some(edge) = operand.resolved_edge_slot {
                historical_edge_axis(decode, edge, topology)?
                    .and_then(|(origin, direction)| design_axis(origin, direction))
            } else {
                None
            };
            if let Some(axis) = resolved_axis {
                operand.resolved_axis = Some(axis);
            }
            continue;
        }
        let changed_edge_contexts = decode.try_collect_vec(
            topology
                .edges
                .iter()
                .copied()
                .filter(|edge| changed_edges.contains(edge))
                .map(|edge| selection::historical_edge_context(decode, edge, topology)),
            "collect F3D changed edge contexts",
        )?;
        operand.recipe_selectors = selection::recipe_selector_candidates(
            decode,
            operand.recipe_structure.as_ref(),
            &changed_edge_contexts,
        )?;
        operand.resolved_edge_slot = side_one_recipe_edge(
            decode,
            operand.recipe_structure.as_ref(),
            &operand.recipe_reference_contexts,
            &operand.recipe_selectors,
            &operand.preceding_boundary_edge_slots,
        )?;
    }
    Ok(())
}

/// A resolved axis with finite origin and unit direction.
fn design_axis(
    origin: cadmpeg_ir::math::Point3,
    direction: cadmpeg_ir::math::Vector3,
) -> Option<crate::records::feature::patterns::DesignAxis> {
    crate::records::feature::patterns::DesignAxis::from_parts(
        cadmpeg_ir::features::FinitePoint3::new(origin)?,
        cadmpeg_ir::features::FiniteVector3::new(direction)?,
    )
}

fn historical_edge_axis(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    edge: i64,
    topology: &AsmHistoricalTopology,
) -> Result<Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)>, cadmpeg_core::CodecError>
{
    if let Some(axis) = topology
        .edge_curves
        .iter()
        .find(|binding| binding.entity == edge)
        .and_then(|binding| binding.carrier)
        .and_then(|curve| topology.curve_axes.iter().find(|axis| axis.curve == curve))
    {
        return Ok(Some((axis.origin, axis.direction)));
    }
    let mut support_surfaces = HashSet::new();
    for carrier in selection::historical_edge_context(decode, edge, topology)?
        .incident_loops
        .into_iter()
        .filter_map(|context| {
            let mut bindings = topology
                .face_surfaces
                .iter()
                .filter(|binding| binding.entity == context.face_slot);
            let carrier = bindings.next()?.carrier;
            bindings.next().is_none().then_some(carrier)
        })
    {
        decode.insert_hash_set(
            &mut support_surfaces,
            carrier,
            "index F3D edge axis support surfaces",
        )?;
    }
    let mut axes = topology
        .surface_axes
        .iter()
        .filter(|axis| support_surfaces.contains(&axis.surface))
        .map(|axis| (axis.origin, axis.direction));
    let Some(first) = axes.next() else {
        return Ok(None);
    };
    Ok(axes
        .all(|axis| same_axis_line(first, axis))
        .then_some(first))
}

fn bind_active_edge_operand_for_scope<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &mut crate::records::topology::edge_identity::DesignEdgeOperand,
    scope: &crate::records::feature::scope::DesignParameterScope,
    terminal_topologies: &[(i64, &'a AsmHistoricalTopology)],
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<(), cadmpeg_core::CodecError> {
    bind_active_edge_operand_candidates(decode, operand, terminal_topologies, topology_queries)?;
    if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfacePatch
        && operand.surface_patch_recipe_structure.is_some()
    {
        operand.recipe_state_id = None;
        operand.resolved_edge_slot = None;
        let mut matched = None;
        let mut ambiguous = false;
        for (state_id, topology) in terminal_topologies {
            if let Some(edge) = surface_patch_edge_operand_slot(
                decode,
                operand.surface_patch_recipe_structure.as_ref(),
                &operand.recipe_references,
                topology,
                topology_queries,
            )? {
                if matched.is_some() {
                    ambiguous = true;
                    break;
                }
                matched = Some((*state_id, edge));
            }
        }
        if !ambiguous {
            if let Some((state_id, edge)) = matched {
                operand.recipe_state_id = Some(state_id);
                operand.resolved_edge_slot = Some(edge);
            }
        }
    }
    if crate::design::design_feature_family(&scope.kind())
        == Some(crate::design::DesignFeatureFamily::Revolve)
    {
        let topology = operand.recipe_state_id.and_then(|state_id| {
            terminal_topologies
                .iter()
                .find(|(candidate, _)| *candidate == state_id)
                .map(|(_, topology)| *topology)
        });
        let resolved_axis = if let Some((edge, topology)) = operand.resolved_edge_slot.zip(topology)
        {
            historical_edge_axis(decode, edge, topology)?
                .and_then(|(origin, direction)| design_axis(origin, direction))
        } else {
            None
        };
        if let Some(axis) = resolved_axis {
            operand.resolved_axis = Some(axis);
        }
    }
    Ok(())
}

fn surface_patch_edge_operand_slot<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    structure: Option<&crate::records::topology::edge_recipe::DesignSurfacePatchRecipeStructure>,
    recipe_references: &[crate::records::dimensions::DesignRecipeReference],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(structure) = structure else {
        return Ok(None);
    };
    let [first, second] = &structure.clauses;
    let Some(common_edge_reference) = common_surface_patch_reference(
        first.edge_reference_ordinals,
        second.edge_reference_ordinals,
    ) else {
        return Ok(None);
    };
    let Some(common_face_reference) = common_surface_patch_reference(
        first.face_reference_ordinals,
        second.face_reference_ordinals,
    ) else {
        return Ok(None);
    };
    let Some(edge_reference) = usize::try_from(common_edge_reference)
        .ok()
        .and_then(|ordinal| recipe_references.get(ordinal))
    else {
        return Ok(None);
    };
    let Some(face_reference) = usize::try_from(common_face_reference)
        .ok()
        .and_then(|ordinal| recipe_references.get(ordinal))
    else {
        return Ok(None);
    };
    if edge_reference.candidate_edges.is_empty() {
        return Ok(None);
    }
    let face_candidates = if face_reference.candidate_faces.is_empty() {
        &face_reference.alternate_selector_faces
    } else {
        &face_reference.candidate_faces
    };
    let faces = selection::faces_in_topology(decode, face_candidates, topology)?;
    let face_boundary_edges = face_boundary_edges(decode, &faces, topology, topology_queries)?;
    let mut candidates = decode.collect_vec(
        edge_reference
            .candidate_edges
            .iter()
            .filter_map(|edge| stable_ref(edge.as_str()))
            .filter(|edge| face_boundary_edges.contains(edge)),
        "collect F3D surface patch edge candidates",
    )?;
    decode.sort_unstable_by(
        &mut candidates,
        Ord::cmp,
        |_| 0,
        "sort F3D surface patch edge candidates",
    )?;
    candidates.dedup();
    match candidates.as_slice() {
        [edge] => Ok(Some(*edge)),
        _ => Ok(None),
    }
}

fn common_surface_patch_reference(left: [u32; 2], right: [u32; 2]) -> Option<u32> {
    let mut common = left
        .into_iter()
        .filter(|reference| right.contains(reference));
    let reference = common.next()?;
    common.next().is_none().then_some(reference)
}

fn bind_active_edge_operand_candidates<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &mut crate::records::topology::edge_identity::DesignEdgeOperand,
    topologies: &[(i64, &'a AsmHistoricalTopology)],
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut unique = None;
    for (state_id, topology) in topologies {
        let all_reference_faces =
            terminal_edge_recipe_reference_faces(decode, &operand.recipe_references, None)?;
        let reference_faces = terminal_edge_recipe_reference_faces(
            decode,
            &operand.recipe_references,
            operand.local_topology_references.as_deref(),
        )?;
        let terminal_faces =
            terminal_edge_recipe_faces(decode, &operand.candidate_faces, &reference_faces)?;
        let candidate_faces = selection::faces_in_topology(decode, &terminal_faces, topology)?;
        if topologies.len() != 1 && candidate_faces.is_empty() {
            continue;
        }
        let boundary_edges =
            face_boundary_edges(decode, &candidate_faces, topology, topology_queries)?;
        let contexts = decode.try_collect_vec(
            boundary_edges
                .iter()
                .copied()
                .map(|edge| selection::historical_edge_context(decode, edge, topology)),
            "collect F3D terminal edge contexts",
        )?;
        let selectors = selection::recipe_selector_candidates(
            decode,
            operand.recipe_structure.as_ref(),
            &contexts,
        )?;
        let reference_edge_sets =
            collect_reference_edge_sets(decode, &reference_faces, topology, topology_queries)?;
        let all_reference_edge_sets =
            collect_reference_edge_sets(decode, &all_reference_faces, topology, topology_queries)?;
        let edge = crate::design::edge_resolve::resolved_edge_candidate_intersection(
            &selectors,
            reference_edge_sets.iter().map(Vec::as_slice),
        );
        if unique.is_some() {
            return Ok(());
        }
        unique = Some((
            *state_id,
            edge,
            candidate_faces,
            boundary_edges,
            contexts,
            all_reference_edge_sets,
            selectors,
        ));
    }
    let Some((
        state_id,
        edge,
        candidate_faces,
        boundary_edges,
        contexts,
        all_reference_edge_sets,
        selectors,
    )) = unique
    else {
        return Ok(());
    };
    operand.terminal_candidate_faces = candidate_faces;
    operand.terminal_boundary_edge_slots = boundary_edges;
    operand.terminal_boundary_edge_contexts = contexts;
    operand.terminal_reference_edge_slots = all_reference_edge_sets;
    operand.recipe_selectors = selectors;
    operand.recipe_state_id = Some(state_id);
    operand.resolved_edge_slot = edge;
    Ok(())
}

fn terminal_edge_recipe_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    primary: &[cadmpeg_ir::ids::FaceId],
    reference_faces: &[Vec<cadmpeg_ir::ids::FaceId>],
) -> Result<Vec<cadmpeg_ir::ids::FaceId>, cadmpeg_core::CodecError> {
    let mut faces = decode.try_collect_vec(
        (primary.iter().chain(reference_faces.iter().flatten()))
            .map(|face| face.try_clone_for_decode(decode, "copy F3D historical face identity")),
        "collect F3D terminal edge recipe faces",
    )?;
    decode.stable_sort_by(
        &mut faces,
        |left, right| left.as_str().cmp(right.as_str()),
        |face| face.as_str().len(),
        "sort F3D terminal edge recipe faces",
    )?;
    faces.dedup();
    Ok(faces)
}

fn terminal_edge_recipe_reference_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    references: &[crate::records::dimensions::DesignRecipeReference],
    local_topology_references: Option<&[std::num::NonZeroU32]>,
) -> Result<Vec<Vec<cadmpeg_ir::ids::FaceId>>, cadmpeg_core::CodecError> {
    let mut selected = Vec::new();
    let mut append = |reference: &crate::records::dimensions::DesignRecipeReference| {
        let faces = if reference.candidate_faces.is_empty() {
            &reference.alternate_selector_faces
        } else {
            &reference.candidate_faces
        };
        let faces = decode.try_collect_vec(
            (faces)
                .iter()
                .map(|face| face.try_clone_for_decode(decode, "copy F3D historical face identity")),
            "copy F3D terminal reference faces",
        )?;

        decode.reserve_vec(&mut selected, 1, "collect F3D terminal reference groups")?;
        selected.push(faces);
        Ok::<(), cadmpeg_core::CodecError>(())
    };
    if let Some(ordinals) = local_topology_references {
        for ordinal in ordinals {
            if let Some(reference) = usize::try_from(ordinal.get())
                .ok()
                .and_then(|ordinal| ordinal.checked_sub(1))
                .and_then(|ordinal| references.get(ordinal))
            {
                append(reference)?;
            }
        }
    } else {
        for reference in references {
            append(reference)?;
        }
    }
    Ok(selected)
}

fn treatment_radius_candidates<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    result_candidate_faces: Option<&[cadmpeg_ir::ids::FaceId]>,
    inserted_faces: &[i64],
    result: &'a AsmHistoricalTopology,
    preceding: &'a AsmHistoricalTopology,
    deleted_edges: &[i64],
    treatment_cache: &mut TreatmentCache<'a>,
) -> Result<
    Vec<crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate>,
    cadmpeg_core::CodecError,
> {
    Ok(treatment_edge_candidates(
        decode,
        result_candidate_faces,
        inserted_faces,
        result,
        preceding,
        deleted_edges,
        treatment_cache,
    )?
    .0)
}

fn treatment_edge_candidates<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    result_candidate_faces: Option<&[cadmpeg_ir::ids::FaceId]>,
    inserted_faces: &[i64],
    result: &'a AsmHistoricalTopology,
    preceding: &'a AsmHistoricalTopology,
    deleted_edges: &[i64],
    treatment_cache: &mut TreatmentCache<'a>,
) -> Result<
    (
        Vec<crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate>,
        Vec<i64>,
    ),
    cadmpeg_core::CodecError,
> {
    let index = treatment_cache.get(decode, result, preceding)?;
    let result_boundaries = &index.result_boundaries;
    let preceding_boundaries = &index.preceding_boundaries;
    let mut deleted_edge_set = HashSet::new();
    for edge in deleted_edges {
        decode.insert_hash_set(
            &mut deleted_edge_set,
            *edge,
            "index F3D treatment deleted edges",
        )?;
    }
    let mut candidate_edges = HashSet::new();
    for edge in result_candidate_faces
        .into_iter()
        .flatten()
        .filter_map(|face| stable_ref(face.as_str()))
        .filter_map(|face| result_boundaries.get(&face))
        .flatten()
        .copied()
    {
        decode.insert_hash_set(
            &mut candidate_edges,
            edge,
            "index F3D treatment candidate edges",
        )?;
    }
    let mut radii_out = Vec::new();
    let mut transitions_out = Vec::new();
    for inserted in inserted_faces {
        decode.charge_work(1, "query F3D treatment supports")?;
        let Some((carrier, supports)) = index.supports.get(inserted) else {
            continue;
        };
        let Some(inserted_boundary) = result_boundaries.get(inserted) else {
            continue;
        };
        decode.charge_work(1, "query F3D treatment radii")?;
        let radius = index
            .radii
            .get(carrier)
            .copied()
            .flatten()
            .and_then(cadmpeg_ir::scalar::PositiveReal::new)
            .filter(|_| {
                candidate_edges.is_empty() || !inserted_boundary.is_disjoint(&candidate_edges)
            });
        for (ordinal, left) in supports.iter().enumerate() {
            let Some(left_edges) = preceding_boundaries.get(left) else {
                continue;
            };
            for right in supports.iter().skip(ordinal + 1) {
                let Some(right_edges) = preceding_boundaries.get(right) else {
                    continue;
                };
                decode.charge_work(
                    u64_from_index(left_edges.len().min(right_edges.len())),
                    "intersect F3D treatment support edges",
                )?;
                for edge in left_edges
                    .intersection(right_edges)
                    .filter(|edge| deleted_edge_set.contains(edge))
                {
                    decode.reserve_vec(
                        &mut transitions_out,
                        1,
                        "collect F3D treatment transition edges",
                    )?;
                    transitions_out.push(*edge);
                    if let Some(radius) = radius {
                        decode.reserve_vec(&mut radii_out, 1, "collect F3D treatment radii")?;
                        radii_out.push(
                            crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate {
                                edge_slot: *edge,
                                radius,
                            },
                        );
                    }
                }
            }
        }
    }
    decode.stable_sort_by(
        &mut radii_out,
        |left, right| {
            left.radius
                .get()
                .total_cmp(&right.radius.get())
                .then(left.edge_slot.cmp(&right.edge_slot))
        },
        |_| 0,
        "sort F3D treatment edge radii",
    )?;
    radii_out
        .dedup_by(|left, right| left.radius == right.radius && left.edge_slot == right.edge_slot);
    decode.sort_unstable_by(
        &mut transitions_out,
        Ord::cmp,
        |_| 0,
        "sort F3D treatment edge transitions",
    )?;
    transitions_out.dedup();
    Ok((radii_out, transitions_out))
}

fn treatment_face_supports(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    inserted_faces: &[i64],
    result: &AsmHistoricalTopology,
    preceding: &AsmHistoricalTopology,
    result_boundaries: &HashMap<i64, HashSet<i64>>,
) -> Result<Vec<(i64, i64, Vec<i64>)>, cadmpeg_core::CodecError> {
    let mut preceding_faces = HashSet::new();
    for face in &preceding.faces {
        decode.charge_work(1, "walk F3D treatment topology indexes")?;
        decode.insert_hash_set(
            &mut preceding_faces,
            *face,
            "index F3D treatment preceding faces",
        )?;
    }
    let mut preceding_surfaces = HashSet::new();
    for surface in &preceding.surfaces {
        decode.charge_work(1, "walk F3D treatment topology indexes")?;
        decode.insert_hash_set(
            &mut preceding_surfaces,
            *surface,
            "index F3D treatment preceding surfaces",
        )?;
    }
    let result_carriers = unique_face_carriers(decode, result)?;
    let preceding_carrier_faces = unique_carrier_faces(decode, preceding, &preceding_faces)?;
    let mut adjacent_faces = HashMap::<i64, Vec<i64>>::new();
    for (face, edges) in result_boundaries {
        decode.charge_work(1, "walk F3D treatment topology indexes")?;
        for edge in edges {
            decode.charge_work(1, "walk F3D treatment topology indexes")?;
            decode.push_hash_group(
                &mut adjacent_faces,
                *edge,
                *face,
                "index F3D adjacent treatment edges",
                "collect F3D adjacent treatment faces",
            )?;
        }
    }
    let mut selected = Vec::new();
    for inserted in inserted_faces.iter().copied() {
        decode.charge_work(1, "walk F3D treatment topology indexes")?;
        let Some(carrier) = result_carriers.get(&inserted).copied().flatten() else {
            continue;
        };
        if preceding_surfaces.contains(&carrier) {
            continue;
        }
        let Some(inserted_boundary) = result_boundaries.get(&inserted) else {
            continue;
        };
        for edge in inserted_boundary {
            decode.charge_work(1, "query F3D adjacent treatment faces")?;
            if let Some(adjacent) = adjacent_faces.get(edge) {
                decode.charge_work(
                    u64_from_index(adjacent.len()),
                    "query F3D adjacent treatment faces",
                )?;
            }
        }
        let mut supports = decode.collect_vec(
            inserted_boundary
                .iter()
                .filter_map(|edge| adjacent_faces.get(edge))
                .flatten()
                .copied()
                .filter(|face| *face != inserted)
                .filter_map(|face| result_carriers.get(&face).copied().flatten())
                .filter_map(|carrier| preceding_carrier_faces.get(&carrier).copied().flatten()),
            "collect F3D treatment support faces",
        )?;
        decode.sort_unstable_by(
            &mut supports,
            Ord::cmp,
            |_| 0,
            "sort F3D treatment support faces",
        )?;
        supports.dedup();

        decode.reserve_vec(&mut selected, 1, "collect F3D treatment face supports")?;
        selected.push((inserted, carrier, supports));
    }
    Ok(selected)
}

fn face_boundary_edge_index(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
) -> Result<HashMap<i64, HashSet<i64>>, cadmpeg_core::CodecError> {
    fn unique_relations<'a>(
        decode: &cadmpeg_core::decode::DecodeContext<'_>,
        relations: &'a [AsmHistoricalRelation],
    ) -> Result<HashMap<i64, Option<&'a [i64]>>, cadmpeg_core::CodecError> {
        let mut out = HashMap::<i64, Option<&[i64]>>::new();
        for relation in relations {
            decode.charge_work(1, "walk F3D treatment topology indexes")?;
            if !out.contains_key(&relation.owner_ref) {
                decode.reserve_map(&mut out, 1, "index F3D boundary relations")?;
            }
            out.entry(relation.owner_ref)
                .and_modify(|members| *members = None)
                .or_insert(Some(&relation.member_refs));
        }
        Ok(out)
    }
    let face_loops = unique_relations(decode, &topology.face_loops)?;
    let loop_coedges = unique_relations(decode, &topology.loop_coedges)?;
    let mut coedge_edges = HashMap::<i64, Option<i64>>::new();
    for coedge in &topology.coedge_topology {
        decode.charge_work(1, "walk F3D treatment topology indexes")?;
        if !coedge_edges.contains_key(&coedge.coedge) {
            decode.reserve_map(&mut coedge_edges, 1, "index F3D boundary coedge edges")?;
        }
        coedge_edges
            .entry(coedge.coedge)
            .and_modify(|edge| *edge = None)
            .or_insert(Some(coedge.edge));
    }
    let mut boundaries = HashMap::new();
    'faces: for face in &topology.faces {
        decode.charge_work(1, "walk F3D treatment topology indexes")?;
        let Some(loops) = face_loops.get(face).copied().flatten() else {
            continue;
        };
        let mut edges = HashSet::new();
        for loop_slot in loops {
            decode.charge_work(1, "walk F3D treatment topology indexes")?;
            let Some(coedges) = loop_coedges.get(loop_slot).copied().flatten() else {
                continue 'faces;
            };
            for coedge in coedges {
                decode.charge_work(1, "walk F3D treatment topology indexes")?;
                let Some(edge) = coedge_edges.get(coedge).copied().flatten() else {
                    continue 'faces;
                };
                decode.insert_hash_set(&mut edges, edge, "index F3D face boundary edges")?;
            }
        }
        if !boundaries.contains_key(face) {
            decode.reserve_map(&mut boundaries, 1, "index F3D face boundaries")?;
        }
        boundaries.insert(*face, edges);
    }
    Ok(boundaries)
}

fn unique_face_carriers(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
) -> Result<HashMap<i64, Option<i64>>, cadmpeg_core::CodecError> {
    let mut out = HashMap::new();
    for binding in &topology.face_surfaces {
        decode.charge_work(1, "walk F3D treatment topology indexes")?;
        if !out.contains_key(&binding.entity) {
            decode.reserve_map(&mut out, 1, "index F3D face carriers")?;
        }
        out.entry(binding.entity)
            .and_modify(|carrier| *carrier = None)
            .or_insert(Some(binding.carrier));
    }
    Ok(out)
}

fn unique_carrier_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
    included_faces: &HashSet<i64>,
) -> Result<HashMap<i64, Option<i64>>, cadmpeg_core::CodecError> {
    let mut out = HashMap::new();
    for binding in &topology.face_surfaces {
        decode.charge_work(1, "index F3D carrier faces")?;
        if !included_faces.contains(&binding.entity) {
            continue;
        }
        if !out.contains_key(&binding.carrier) {
            decode.reserve_map(&mut out, 1, "index F3D carrier faces")?;
        }
        out.entry(binding.carrier)
            .and_modify(|face| *face = None)
            .or_insert(Some(binding.entity));
    }
    Ok(out)
}

fn stable_ref(id: &str) -> Option<i64> {
    id.rsplit_once('#')?
        .1
        .split(':')
        .next()?
        .parse::<i64>()
        .ok()
}

pub(crate) fn same_axis_line(
    left: (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
    right: (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
) -> bool {
    let direction_dot = left.1.dot(right.1);
    if (direction_dot.abs() - 1.0).abs() > EPS_HISTORY_SAME_AXIS_LINE_E9 {
        return false;
    }
    let distance = right.0.vector_from(left.0).cross(left.1).norm();
    distance.is_finite() && distance <= EPS_HISTORY_SAME_AXIS_LINE_E8
}

fn affected_body_refs<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    current: &'a AsmDeltaState,
    previous: Option<&'a AsmDeltaState>,
    body_closures: &mut HistoricalBodyClosureCache<'a>,
) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError> {
    let Some(transition) = current.transition.as_ref() else {
        return Ok(None);
    };
    if transition.previous_state_id != previous.map(|state| state.state_id) {
        return Ok(None);
    }
    let Some(current_topology) = current.topology() else {
        return Ok(None);
    };
    let current_changes = changed_family_refs(ctx, &transition.topology, false)?;
    let Some(mut affected) = body_closures.intersecting(ctx, current_topology, &current_changes)?
    else {
        return Ok(None);
    };
    if let Some(previous) = previous {
        let Some(previous_topology) = previous.topology() else {
            return Ok(None);
        };
        let deleted = changed_family_refs(ctx, &transition.topology, true)?;
        let Some(previous_affected) =
            body_closures.intersecting(ctx, previous_topology, &deleted)?
        else {
            return Ok(None);
        };
        for body in previous_affected {
            ctx.insert_btree_set(&mut affected, body, "merge F3D affected history bodies")?;
        }
    }
    Ok(Some(ctx.collect_vec(
        affected,
        "collect F3D affected history bodies",
    )?))
}

fn changed_family_refs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    delta: &AsmHistoricalTopologyDelta,
    deleted: bool,
) -> Result<BTreeSet<i64>, cadmpeg_core::CodecError> {
    let families = [
        &delta.bodies,
        &delta.regions,
        &delta.shells,
        &delta.faces,
        &delta.loops,
        &delta.coedges,
        &delta.edges,
        &delta.vertices,
        &delta.points,
        &delta.surfaces,
        &delta.curves,
        &delta.pcurves,
    ];
    let mut changed = BTreeSet::new();
    for family in families {
        let members = if deleted {
            family.deleted.iter().chain([].iter())
        } else {
            family.inserted.iter().chain(family.updated.iter())
        };
        for &member in members {
            ctx.insert_btree_set(&mut changed, member, "index F3D changed topology members")?;
        }
    }
    Ok(changed)
}

type HistoricalBodyClosures = Vec<(i64, HashSet<i64>)>;

/// Reuse body incidence for queries over immutable historical snapshots.
#[derive(Default)]
struct HistoricalBodyClosureCache<'a> {
    entries: HashMap<*const AsmHistoricalTopology, Option<HistoricalBodyClosures>>,
    snapshots: std::marker::PhantomData<&'a AsmHistoricalTopology>,
}

impl<'a> HistoricalBodyClosureCache<'a> {
    fn intersecting(
        &mut self,
        decode: &cadmpeg_core::decode::DecodeContext<'_>,
        topology: &'a AsmHistoricalTopology,
        changed: &BTreeSet<i64>,
    ) -> Result<Option<BTreeSet<i64>>, cadmpeg_core::CodecError> {
        let key = std::ptr::from_ref(topology);
        if !self.entries.contains_key(&key) {
            let closures = historical_body_closures(decode, topology)?;
            decode.reserve_map(&mut self.entries, 1, "cache F3D historical body closures")?;
            self.entries.insert(key, closures);
        }
        let Some(closures) = self.entries.get(&key).and_then(Option::as_ref) else {
            return Ok(None);
        };
        let mut affected = BTreeSet::new();
        for (body, closure) in closures {
            decode.charge_work(
                u64_from_index(changed.len()),
                "query F3D historical body closure",
            )?;
            if changed.iter().any(|entity| closure.contains(entity)) {
                decode.insert_btree_set(
                    &mut affected,
                    *body,
                    "collect F3D affected topology bodies",
                )?;
            }
        }
        Ok(Some(affected))
    }
}

#[cfg(test)]
fn bodies_intersecting(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
    changed: &BTreeSet<i64>,
) -> Result<Option<BTreeSet<i64>>, cadmpeg_core::CodecError> {
    HistoricalBodyClosureCache::default().intersecting(decode, topology, changed)
}

fn historical_body_closures(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
) -> Result<Option<HistoricalBodyClosures>, cadmpeg_core::CodecError> {
    macro_rules! history_some {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    let body_regions = relation_map(decode, &topology.body_regions)?;
    let region_shells = relation_map(decode, &topology.region_shells)?;
    let shell_faces = relation_map(decode, &topology.shell_faces)?;
    let shell_wire_edges = relation_map(decode, &topology.shell_wire_edges)?;
    let shell_free_vertices = relation_map(decode, &topology.shell_free_vertices)?;
    let face_loops = relation_map(decode, &topology.face_loops)?;
    let loop_coedges = relation_map(decode, &topology.loop_coedges)?;
    let coedges = decode.collect_hash_map(
        topology
            .coedge_topology
            .iter()
            .map(|coedge| (coedge.coedge, coedge)),
        "index F3D historical coedges",
    )?;
    let edges = decode.collect_hash_map(
        topology.edge_vertices.iter().map(|edge| (edge.edge, edge)),
        "index F3D historical edge vertices",
    )?;
    let carrier = |items: &[AsmHistoricalCarrierBinding]| {
        decode.collect_hash_map(
            items
                .iter()
                .map(|binding| (binding.entity, binding.carrier)),
            "index F3D historical carriers",
        )
    };
    let optional_carrier = |items: &[AsmHistoricalOptionalCarrierBinding]| {
        decode.collect_hash_map(
            items
                .iter()
                .map(|binding| (binding.entity, binding.carrier)),
            "index F3D historical optional carriers",
        )
    };
    let face_surfaces = carrier(&topology.face_surfaces)?;
    let edge_curves = optional_carrier(&topology.edge_curves)?;
    let coedge_pcurves = optional_carrier(&topology.coedge_pcurves)?;
    let vertex_points = carrier(&topology.vertex_points)?;
    let mut closures = Vec::new();
    for &body in &topology.bodies {
        let mut closure = HashSet::new();
        decode
            .insert_hash_set(&mut closure, body, "collect F3D historical body closure")
            .map(|_| ())?;
        for &region in *history_some!(body_regions.get(&body)) {
            decode
                .insert_hash_set(&mut closure, region, "collect F3D historical body closure")
                .map(|_| ())?;
            for &shell in *history_some!(region_shells.get(&region)) {
                decode
                    .insert_hash_set(&mut closure, shell, "collect F3D historical body closure")
                    .map(|_| ())?;
                let mut shell_edges = decode.collect_hash_set(
                    history_some!(shell_wire_edges.get(&shell)).iter().copied(),
                    "copy F3D historical shell edges",
                )?;
                let mut shell_vertices = decode.collect_hash_set(
                    history_some!(shell_free_vertices.get(&shell))
                        .iter()
                        .copied(),
                    "copy F3D historical shell vertices",
                )?;
                for &face in *history_some!(shell_faces.get(&shell)) {
                    decode
                        .insert_hash_set(&mut closure, face, "collect F3D historical body closure")
                        .map(|_| ())?;
                    decode
                        .insert_hash_set(
                            &mut closure,
                            *history_some!(face_surfaces.get(&face)),
                            "collect F3D historical body closure",
                        )
                        .map(|_| ())?;
                    for &loop_ in *history_some!(face_loops.get(&face)) {
                        decode
                            .insert_hash_set(
                                &mut closure,
                                loop_,
                                "collect F3D historical body closure",
                            )
                            .map(|_| ())?;
                        for &coedge in *history_some!(loop_coedges.get(&loop_)) {
                            decode
                                .insert_hash_set(
                                    &mut closure,
                                    coedge,
                                    "collect F3D historical body closure",
                                )
                                .map(|_| ())?;
                            let coedge_topology = history_some!(coedges.get(&coedge));

                            decode.insert_hash_set(
                                &mut shell_edges,
                                coedge_topology.edge,
                                "collect F3D historical shell edges",
                            )?;
                            if let Some(pcurve) = coedge_pcurves.get(&coedge).copied().flatten() {
                                decode
                                    .insert_hash_set(
                                        &mut closure,
                                        pcurve,
                                        "collect F3D historical body closure",
                                    )
                                    .map(|_| ())?;
                            }
                        }
                    }
                }
                for edge in shell_edges {
                    decode
                        .insert_hash_set(&mut closure, edge, "collect F3D historical body closure")
                        .map(|_| ())?;
                    let edge_topology = history_some!(edges.get(&edge));
                    for vertex in [edge_topology.start_vertex, edge_topology.end_vertex] {
                        decode.insert_hash_set(
                            &mut shell_vertices,
                            vertex,
                            "collect F3D historical shell vertices",
                        )?;
                    }
                    if let Some(curve) = edge_curves.get(&edge).copied().flatten() {
                        decode
                            .insert_hash_set(
                                &mut closure,
                                curve,
                                "collect F3D historical body closure",
                            )
                            .map(|_| ())?;
                    }
                }
                for vertex in shell_vertices {
                    decode
                        .insert_hash_set(
                            &mut closure,
                            vertex,
                            "collect F3D historical body closure",
                        )
                        .map(|_| ())?;
                    decode
                        .insert_hash_set(
                            &mut closure,
                            *history_some!(vertex_points.get(&vertex)),
                            "collect F3D historical body closure",
                        )
                        .map(|_| ())?;
                }
            }
        }
        decode.push_vec(
            &mut closures,
            (body, closure),
            "cache F3D historical body closure",
        )?;
    }
    Ok(Some(closures))
}

fn relation_map<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    items: &'a [AsmHistoricalRelation],
) -> Result<HashMap<i64, &'a [i64]>, cadmpeg_core::CodecError> {
    decode.collect_hash_map(
        items
            .iter()
            .map(|relation| (relation.owner_ref, relation.member_refs.as_slice())),
        "index F3D historical relations",
    )
}

fn historical_topology(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    brep: &cadmpeg_asm::brep::AsmBrep,
) -> Result<Option<AsmHistoricalTopology>, cadmpeg_core::CodecError> {
    macro_rules! topology_some {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }

    fn refs<'a>(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        ids: impl Iterator<Item = &'a str>,
    ) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError> {
        ctx.collect_options(
            ids.map(stable_ref),
            "collect F3D historical topology references",
        )
    }

    fn relations<'a, M>(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        items: impl Iterator<Item = (&'a str, M)>,
    ) -> Result<Option<Vec<AsmHistoricalRelation>>, cadmpeg_core::CodecError>
    where
        M: Iterator<Item = &'a str>,
    {
        let mut collected = Vec::new();
        for (owner, members) in items {
            let Some(owner_ref) = stable_ref(owner) else {
                return Ok(None);
            };
            let Some(member_refs) = refs(ctx, members)? else {
                return Ok(None);
            };

            ctx.reserve_vec(
                &mut collected,
                1,
                "collect F3D historical topology relations",
            )?;
            collected.push(AsmHistoricalRelation {
                owner_ref,
                member_refs,
            });
        }
        Ok(Some(collected))
    }

    let mut surface_radii = ctx.collect_vec(
        brep.surfaces.iter().filter_map(|surface| {
            use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
            let radius = match &surface.geometry {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                    cylinder_surface.radius().get()
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
                    sphere_surface.radius().get()
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                    torus_surface.minor_radius().get()
                }
                _ => return None,
            };
            Some(crate::history_records::AsmHistoricalSurfaceRadius {
                surface: stable_ref(surface.id.as_str())?,
                radius: radius.abs(),
            })
        }),
        "collect F3D historical surface radii",
    )?;
    let mut blend_radii = HashMap::new();
    for (owner, procedural) in &brep.procedural_surfaces {
        ctx.charge_work(1, "scan F3D historical blend radii")?;
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Blend(definition_payload) =
            procedural.definition()
        else {
            continue;
        };
        let radius = definition_payload.radius();

        let cadmpeg_ir::geometry::BlendRadiusLaw::Constant { signed_radius } = radius else {
            continue;
        };
        let Some(surface) = stable_ref(owner.as_str()) else {
            continue;
        };
        if !blend_radii.contains_key(&surface) {
            ctx.reserve_map(&mut blend_radii, 1, "index F3D historical blend radii")?;
        }
        blend_radii.insert(surface, signed_radius.get().abs());
    }
    ctx.charge_work(
        u64_from_index(surface_radii.len()),
        "replace F3D historical blend radii",
    )?;
    surface_radii.retain(|candidate| !blend_radii.contains_key(&candidate.surface));
    for (surface, radius) in blend_radii {
        ctx.reserve_vec(
            &mut surface_radii,
            1,
            "collect F3D historical surface radii",
        )?;
        surface_radii.push(crate::history_records::AsmHistoricalSurfaceRadius { surface, radius });
    }
    ctx.stable_sort_by(
        &mut surface_radii,
        |a, b| a.surface.cmp(&b.surface),
        |_| 0,
        "sort F3D historical surface radii",
    )?;
    let mut surface_cylinders = ctx.collect_vec(
        brep.surfaces.iter().filter_map(|surface| {
            let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = surface.geometry.solved()
            else {
                return None;
            };
            let origin = cylinder_surface.origin().get();
            let axis = *cylinder_surface.frame().axis().as_raw();
            let radius = cylinder_surface.radius().get();
            Some(crate::history_records::AsmHistoricalCylinder {
                surface: stable_ref(surface.id.as_str())?,
                origin,
                axis,
                radius: radius.abs(),
            })
        }),
        "collect F3D historical surface cylinders",
    )?;
    ctx.stable_sort_by(
        &mut surface_cylinders,
        |a, b| a.surface.cmp(&b.surface),
        |_| 0,
        "sort F3D historical surface cylinders",
    )?;
    let mut surface_planes = ctx.collect_vec(
        brep.surfaces.iter().filter_map(|surface| {
            let Some(SolvedSurfaceGeometry::Plane(plane_surface)) = surface.geometry.solved()
            else {
                return None;
            };
            let origin = plane_surface.origin().get();
            let normal = *plane_surface.frame().axis().as_raw();
            Some(crate::history_records::AsmHistoricalPlane {
                surface: stable_ref(surface.id.as_str())?,
                origin,
                normal,
            })
        }),
        "collect F3D historical surface planes",
    )?;
    ctx.stable_sort_by(
        &mut surface_planes,
        |a, b| a.surface.cmp(&b.surface),
        |_| 0,
        "sort F3D historical surface planes",
    )?;
    let mut surface_axes = ctx.collect_vec(
        brep.surfaces.iter().filter_map(|surface| {
            use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
            let (origin, direction) = match surface.geometry {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                    let origin = cylinder_surface.origin().get();
                    let axis = *cylinder_surface.frame().axis().as_raw();
                    (origin, axis)
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
                    let origin = cone_surface.origin().get();
                    let axis = *cone_surface.frame().axis().as_raw();
                    (origin, axis)
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                    let center = torus_surface.center().get();
                    let axis = *torus_surface.frame().axis().as_raw();
                    (center, axis)
                }
                _ => return None,
            };
            Some(crate::history_records::AsmHistoricalSurfaceAxis {
                surface: stable_ref(surface.id.as_str())?,
                origin,
                direction,
            })
        }),
        "collect F3D historical surface axes",
    )?;
    ctx.stable_sort_by(
        &mut surface_axes,
        |a, b| a.surface.cmp(&b.surface),
        |_| 0,
        "sort F3D historical surface axes",
    )?;

    let mut ring_neighbors = HashMap::new();
    for loop_ in &brep.loops {
        let ring = loop_.coedges();
        for (ordinal, coedge) in ring.iter().enumerate() {
            let key = (&loop_.id, coedge);
            ctx.charge_work(
                u64_from_index(loop_.id.as_str().len()) + u64_from_index(coedge.as_str().len()),
                "index F3D historical ring neighbors",
            )?;
            if !ring_neighbors.contains_key(&key) {
                ctx.reserve_map(
                    &mut ring_neighbors,
                    1,
                    "index F3D historical ring neighbors",
                )?;
                ring_neighbors.insert(
                    key,
                    (
                        &ring[(ordinal + 1) % ring.len()],
                        &ring[(ordinal + ring.len() - 1) % ring.len()],
                    ),
                );
            }
        }
    }

    Ok(Some(AsmHistoricalTopology {
        bodies: topology_some!(refs(
            ctx,
            brep.bodies.iter().map(|entity| entity.id.as_str())
        )?),
        regions: topology_some!(refs(
            ctx,
            brep.regions.iter().map(|entity| entity.id.as_str())
        )?),
        shells: topology_some!(refs(
            ctx,
            brep.shells.iter().map(|entity| entity.id.as_str())
        )?),
        faces: topology_some!(refs(
            ctx,
            brep.faces.iter().map(|entity| entity.id.as_str())
        )?),
        loops: topology_some!(refs(
            ctx,
            brep.loops.iter().map(|entity| entity.id.as_str())
        )?),
        coedges: topology_some!(refs(
            ctx,
            brep.coedges.iter().map(|entity| entity.id.as_str())
        )?),
        edges: topology_some!(refs(
            ctx,
            brep.edges.iter().map(|entity| entity.id.as_str())
        )?),
        vertices: topology_some!(refs(
            ctx,
            brep.vertices.iter().map(|entity| entity.id.as_str())
        )?),
        points: topology_some!(refs(
            ctx,
            brep.points.iter().map(|entity| entity.id.as_str())
        )?),
        surfaces: topology_some!(refs(
            ctx,
            brep.surfaces.iter().map(|entity| entity.id.as_str())
        )?),
        surface_radii,
        surface_cylinders,
        surface_planes,
        surface_axes,
        curves: topology_some!(refs(
            ctx,
            brep.curves.iter().map(|entity| entity.id.as_str())
        )?),
        curve_axes: ctx.collect_vec(
            brep.curves.iter().filter_map(|curve| {
                use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
                let (origin, direction) = match curve.geometry {
                    CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
                        let origin = line_curve.origin().get();
                        let direction = *line_curve.direction().as_raw();
                        (origin, direction)
                    }
                    CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
                        let center = circle_curve.center().get();
                        let axis = *circle_curve.frame().axis().as_raw();
                        (center, axis)
                    }
                    CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
                        let center = ellipse_curve.center().get();
                        let axis = *ellipse_curve.frame().axis().as_raw();
                        (center, axis)
                    }
                    _ => return None,
                };
                Some(crate::history_records::AsmHistoricalCurveAxis {
                    curve: stable_ref(curve.id.as_str())?,
                    origin,
                    direction,
                })
            }),
            "collect F3D historical curve axes",
        )?,
        pcurves: topology_some!(refs(
            ctx,
            brep.pcurves.iter().map(|entity| entity.id.as_str())
        )?),
        persistent_subentity_tags: Vec::new(),
        body_regions: topology_some!(relations(
            ctx,
            brep.bodies.iter().map(|body| {
                (
                    body.id.as_str(),
                    body.regions.iter().map(cadmpeg_ir::ids::RegionId::as_str),
                )
            })
        )?),
        region_shells: topology_some!(relations(
            ctx,
            brep.regions.iter().map(|region| {
                (
                    region.id.as_str(),
                    region.shells.iter().map(cadmpeg_ir::ids::ShellId::as_str),
                )
            })
        )?),
        shell_faces: topology_some!(relations(
            ctx,
            brep.shells.iter().map(|shell| {
                (
                    shell.id.as_str(),
                    shell.faces().iter().map(cadmpeg_ir::ids::FaceId::as_str),
                )
            })
        )?),
        shell_wire_edges: topology_some!(relations(
            ctx,
            brep.shells.iter().map(|shell| {
                (
                    shell.id.as_str(),
                    shell
                        .wire_edges()
                        .iter()
                        .map(cadmpeg_ir::ids::EdgeId::as_str),
                )
            })
        )?),
        shell_free_vertices: topology_some!(relations(
            ctx,
            brep.shells.iter().map(|shell| {
                (
                    shell.id.as_str(),
                    shell
                        .free_vertices()
                        .iter()
                        .map(cadmpeg_ir::ids::VertexId::as_str),
                )
            })
        )?),
        face_loops: topology_some!(relations(
            ctx,
            brep.faces.iter().map(|face| {
                (
                    face.id.as_str(),
                    face.loops.iter().map(cadmpeg_ir::ids::LoopId::as_str),
                )
            })
        )?),
        loop_coedges: topology_some!(relations(
            ctx,
            brep.loops.iter().map(|loop_| {
                (
                    loop_.id.as_str(),
                    loop_
                        .coedges()
                        .iter()
                        .map(cadmpeg_ir::ids::CoedgeId::as_str),
                )
            })
        )?),
        coedge_topology: topology_some!(ctx.collect_options(
            brep.coedges.iter().map(|coedge| {
                let (next, previous) = ring_neighbors
                    .get(&(&coedge.owner_loop, &coedge.id))
                    .copied()?;
                Some(AsmHistoricalCoedge {
                    coedge: stable_ref(coedge.id.as_str())?,
                    owner_loop: stable_ref(coedge.owner_loop.as_str())?,
                    edge: stable_ref(coedge.edge.as_str())?,
                    next: stable_ref(next.as_str())?,
                    previous: stable_ref(previous.as_str())?,
                    radial_next: stable_ref(coedge.radial_next.as_str())?,
                })
            }),
            "collect F3D historical coedges"
        )?),
        edge_vertices: topology_some!(ctx.collect_options(
            brep.edges.iter().map(|edge| {
                Some(AsmHistoricalEdge {
                    edge: stable_ref(edge.id.as_str())?,
                    start_vertex: stable_ref(edge.start.as_str())?,
                    end_vertex: stable_ref(edge.end.as_str())?,
                })
            }),
            "collect F3D historical edges"
        )?),
        face_surfaces: topology_some!(ctx.collect_options(
            brep.faces.iter().map(|face| {
                Some(AsmHistoricalCarrierBinding {
                    entity: stable_ref(face.id.as_str())?,
                    carrier: stable_ref(face.surface.as_str())?,
                })
            }),
            "collect F3D historical face surfaces"
        )?),
        edge_curves: topology_some!(ctx.collect_options(
            brep.edges.iter().map(|edge| {
                Some(AsmHistoricalOptionalCarrierBinding {
                    entity: stable_ref(edge.id.as_str())?,
                    carrier: match edge.curve() {
                        Some(curve) => Some(stable_ref(curve.as_str())?),
                        None => None,
                    },
                })
            }),
            "collect F3D historical edge curves"
        )?),
        coedge_pcurves: topology_some!(ctx.collect_options(
            brep.coedges.iter().map(|coedge| {
                Some(AsmHistoricalOptionalCarrierBinding {
                    entity: stable_ref(coedge.id.as_str())?,
                    carrier: match coedge.pcurves.first() {
                        Some(use_) => Some(stable_ref(use_.pcurve.as_str())?),
                        None => None,
                    },
                })
            }),
            "collect F3D historical coedge pcurves"
        )?),
        vertex_points: topology_some!(ctx.collect_options(
            brep.vertices.iter().map(|vertex| {
                Some(AsmHistoricalCarrierBinding {
                    entity: stable_ref(vertex.id.as_str())?,
                    carrier: stable_ref(vertex.point.as_str())?,
                })
            }),
            "collect F3D historical vertex points"
        )?),
        point_positions: topology_some!(ctx.collect_options(
            brep.points.iter().map(|point| {
                Some(AsmHistoricalPoint {
                    point: stable_ref(point.id.as_str())?,
                    position: point.position().get(),
                })
            }),
            "collect F3D historical point positions"
        )?),
    }))
}

pub(crate) fn historical_topology_with_tags(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    brep: &crate::brep::Brep,
) -> Result<Option<AsmHistoricalTopology>, cadmpeg_core::CodecError> {
    let Some(mut topology) = historical_topology(ctx, &brep.asm)? else {
        return Ok(None);
    };
    for tag in &brep.persistent_subentity_tags {
        let (entity_kind, entity_ref) = match &tag.target {
            cadmpeg_ir::attributes::AttributeTarget::Face(face) => {
                (AsmHistoricalEntityKind::Face, stable_ref(face.as_str()))
            }
            cadmpeg_ir::attributes::AttributeTarget::Edge(edge) => {
                (AsmHistoricalEntityKind::Edge, stable_ref(edge.as_str()))
            }
            _ => continue,
        };
        let Some(entity_ref) = entity_ref else {
            continue;
        };
        let design_references = ctx.collect_vec(
            tag.design_references.iter().copied(),
            "collect F3D historical tag design references",
        )?;
        let token = ctx.copy_retained_text(tag.token.as_str(), "copy F3D historical tag token")?;

        ctx.reserve_vec(
            &mut topology.persistent_subentity_tags,
            1,
            "collect F3D historical persistent tags",
        )?;
        topology.persistent_subentity_tags.push(
            crate::history_records::AsmHistoricalPersistentSubentityTag {
                entity_kind,
                entity_ref,
                selector: tag.selector,
                token,
                design_references,
                ordinal: tag.ordinal,
            },
        );
    }
    Ok(Some(topology))
}

fn materialize_record_table(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    state: &AsmDeltaState,
    archive: &HistoricalRecordArchive,
) -> Result<Option<Vec<cadmpeg_asm::sab::Record>>, cadmpeg_core::CodecError> {
    if state.entity_versions.is_empty() {
        return Ok(None);
    }

    let mut present = HashSet::new();
    ctx.reserve_set(
        &mut present,
        state.entity_versions.len(),
        "index F3D historical record presence",
    )?;
    for version in &state.entity_versions {
        present.insert(version.entity_ref);
    }
    if present.len() != state.entity_versions.len() {
        return Ok(None);
    }

    let mut records = Vec::new();
    ctx.reserve_vec(
        &mut records,
        state.entity_versions.len(),
        "materialize F3D historical record table",
    )?;
    for version in &state.entity_versions {
        let Some(record) = archive.get(&version.record_ref) else {
            return Ok(None);
        };
        if i64::try_from(record.index).ok() != Some(version.entity_ref) {
            return Ok(None);
        }
        for token in record.tokens.iter() {
            let cadmpeg_asm::sab::Token::Ref(reference) = token else {
                continue;
            };
            if *reference >= 0 && !present.contains(reference) {
                return Ok(None);
            }
        }
        records.push(clone_historical_record(ctx, record)?);
    }
    ctx.sort_unstable_by(
        &mut records,
        |a, b| a.index.cmp(&b.index),
        |_| 0,
        "sort F3D historical records",
    )?;
    Ok(Some(records))
}

fn decode_bulletin_boards(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    mut position: usize,
    stream: &str,
    state_offset: usize,
    state_id: &str,
    width: RefWidth,
) -> Result<Option<(Vec<AsmBulletinBoard>, usize)>, cadmpeg_core::CodecError> {
    if bytes.get(position) == Some(&0x11) {
        return Ok(Some((Vec::new(), position)));
    }
    let mut boards = Vec::new();
    loop {
        let board_offset = position;
        let Some(present) = take_int(bytes, &mut position, 0x04, width) else {
            return Ok(None);
        };
        if present == 0 {
            break;
        }
        let Some(owner_ref) = take_int(bytes, &mut position, 0x0c, width) else {
            return Ok(None);
        };
        let Some(number) = take_int(bytes, &mut position, 0x04, width) else {
            return Ok(None);
        };
        let board_id = crate::ids::native_scoped_id_charged(
            ctx,
            stream,
            "asm-bulletin-board",
            format_args!("{state_offset:010}:{:06}", boards.len()),
        )?;
        let mut changes = Vec::new();
        loop {
            let change_offset = position;
            let Some(present) = take_int(bytes, &mut position, 0x04, width) else {
                return Ok(None);
            };
            if present == 0 {
                break;
            }
            let Some(old) = take_int(bytes, &mut position, 0x0c, width) else {
                return Ok(None);
            };
            let Some(new) = take_int(bytes, &mut position, 0x0c, width) else {
                return Ok(None);
            };
            let kind = match (old >= 0, new >= 0) {
                (false, true) => AsmEntityChangeKind::Insert { new },
                (true, false) => AsmEntityChangeKind::Delete { old },
                (true, true) => AsmEntityChangeKind::Update { old, new },
                (false, false) => return Ok(None),
            };

            ctx.reserve_vec(&mut changes, 1, "admit F3D ASM entity change")?;
            let change_id = crate::ids::native_scoped_id_charged(
                ctx,
                stream,
                "asm-entity-change",
                format_args!(
                    "{state_offset:010}:{:06}:{:06}",
                    boards.len(),
                    changes.len()
                ),
            )?;
            let parent = ctx.copy_retained_text(&board_id, "copy F3D ASM change parent")?;
            changes.push(AsmEntityChange {
                id: change_id,
                parent,
                byte_offset: u64_from_index(change_offset),
                kind,
            });
        }

        ctx.reserve_vec(&mut boards, 1, "admit F3D ASM bulletin board")?;
        let parent = ctx.copy_retained_text(state_id, "copy F3D ASM board parent")?;
        boards.push(AsmBulletinBoard {
            id: board_id,
            parent,
            byte_offset: u64_from_index(board_offset),
            owner_ref,
            number,
            changes,
        });
    }
    Ok(Some((boards, position)))
}

fn decode_history_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    state_end: usize,
    next_delta: Option<usize>,
    stream: &str,
    state_id: &str,
    width: RefWidth,
) -> Result<Vec<AsmHistoryRecord>, cadmpeg_core::CodecError> {
    let mut start = state_end + usize::from(bytes.get(state_end) == Some(&0x11));
    if bytes.get(start) == Some(&0x04)
        && int_at(bytes, start + 1, width) == Some(0)
        && bytes.get(start + 1 + width.bytes()) == Some(&0x11)
    {
        start += 2 + width.bytes();
    }
    let limit = next_delta.map_or(bytes.len(), |offset| offset + 1);
    if start >= limit {
        return Ok(Vec::new());
    }
    match cadmpeg_asm::sab::frame_history(ctx, bytes, start, limit, width, None) {
        Ok(records) => {
            let mut decoded = Vec::new();
            for record in records {
                let reference_count = record
                    .tokens
                    .iter()
                    .filter(|token| matches!(token, cadmpeg_asm::sab::Token::Ref(_)))
                    .count();

                let mut entity_references = Vec::new();
                ctx.reserve_vec(
                    &mut entity_references,
                    reference_count,
                    "frame F3D history references",
                )?;
                for token in record.tokens.iter() {
                    if let cadmpeg_asm::sab::Token::Ref(value) = token {
                        entity_references.push(*value);
                    }
                }
                let end = record.offset.checked_add(record.len).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("F3D history record byte range overflows")
                })?;
                let source = bytes.get(record.offset..end).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "F3D history record byte range exceeds stream",
                    )
                })?;
                let raw_bytes = ctx.copy_retained(source, "retain F3D history record")?;

                ctx.reserve_vec(&mut decoded, 1, "frame F3D history record")?;
                let id = crate::ids::native_scoped_id_charged(
                    ctx,
                    stream,
                    "asm-history-record",
                    format_args!("{:010}", record.offset),
                )?;
                let parent = ctx.copy_retained_text(state_id, "copy F3D history record parent")?;
                decoded.push(AsmHistoryRecord {
                    id,
                    parent,
                    revision_id: None,
                    byte_offset: u64_from_index(record.offset),
                    framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                        index: u64_from_index(record.index),
                        name: record.name,
                        entity_references,
                    },
                    raw_bytes: raw_bytes.into(),
                });
            }
            Ok(decoded)
        }
        Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => Err(error.into()),
        Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
            Err(error.into_codec_error())
        }
        Err(error) => {
            let raw_bytes =
                ctx.copy_retained(&bytes[start..limit], "retain opaque F3D history record")?;
            let mut decoded = ctx.collection_vec(1, "frame opaque F3D history record")?;
            let id = crate::ids::native_scoped_id_charged(
                ctx,
                stream,
                "asm-history-record",
                format_args!("{start:010}"),
            )?;
            let parent =
                ctx.copy_retained_text(state_id, "copy opaque F3D history record parent")?;
            let error =
                ctx.format_retained(format_args!("{error}"), "retain opaque F3D history error")?;
            decoded.push(AsmHistoryRecord {
                id,
                parent,
                revision_id: None,
                byte_offset: u64_from_index(start),
                framing: crate::history_records::AsmHistoryRecordFraming::Opaque { error },
                raw_bytes: raw_bytes.into(),
            });
            Ok(decoded)
        }
    }
}

fn decode_preamble(bytes: &[u8], mut position: usize, width: RefWidth) -> Option<(i64, i64)> {
    let size = take_int(bytes, &mut position, 0x04, width)?;
    let duplicate = take_int(bytes, &mut position, 0x04, width)?;
    let zero = take_int(bytes, &mut position, 0x04, width)?;
    let entry_count = take_int(bytes, &mut position, 0x04, width)?;
    (size == duplicate && zero == 0).then_some((size, entry_count))
}

/// Read a tagged little-endian signed integer of the stream's ref width (4 or
/// 8 bytes) and advance past it.
fn take_int(bytes: &[u8], position: &mut usize, tag: u8, width: RefWidth) -> Option<i64> {
    if bytes.get(*position) != Some(&tag) {
        return None;
    }
    let value = int_at(bytes, *position + 1, width)?;
    *position += 1 + width.bytes();
    Some(value)
}

#[cfg(test)]
mod tests;
