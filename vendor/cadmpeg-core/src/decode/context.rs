// SPDX-License-Identifier: Apache-2.0
//! Decode state, decompression limits, and session lifecycle.

use std::cell::Cell;
use std::io::SeekFrom;

use crate::{CodecError, ReadSeek};

use super::arena::DecodeArena;
use super::budget::{DecodeBudget, DepthGuard, ScopedReservation, WorkBudget};
use super::error::{ResourceDimension, ResourceFailure, ResourceLimit};
use super::policy::{
    DecodePolicy, DECOMPRESSED_PER_EXPAND_BASE, DECOMPRESSED_PER_EXPAND_PER_INPUT_BYTE,
};
use super::space::{ByteRange, SpaceId};
use super::view::{u64_from_index, View};

#[derive(Clone, Copy)]
enum LimitScope {
    Global,
    PerExpand,
}

/// Cap on the initial per-expand reservation before any output is produced.
const RESERVE_CLAMP: usize = 8 * 1024 * 1024;

/// Shared monotonic decode state.
#[derive(Debug)]
pub struct DecodeContext<'a> {
    arena: &'a DecodeArena,
    container_only: bool,
    pub(super) budget: DecodeBudget,
    derived_spaces: Cell<usize>,
}

impl<'a> DecodeContext<'a> {
    /// Creates one session before input acquisition and detection.
    pub fn new(arena: &'a DecodeArena, policy: &DecodePolicy, container_only: bool) -> Self {
        Self {
            arena,
            container_only,
            budget: DecodeBudget::new(*policy, 0),
            derived_spaces: Cell::new(0),
        }
    }

    /// Starts post-decode processing with the measured byte length of an already
    /// loaded input. This admits the input-proportional allowances without
    /// keeping another copy of the source bytes. Format-declared counts must
    /// not be used as the input length.
    pub fn for_loaded_input(
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
        input_bytes: u64,
    ) -> Result<Self, CodecError> {
        let ctx = Self::new(arena, policy, false);
        ctx.budget
            .charge_input(input_bytes, "admit loaded input length")?;
        Ok(ctx)
    }

    /// Reads the root input under `max_input_bytes`, copies it into the arena,
    /// registers the root space, establishes input-proportional allowances,
    /// and returns the context and root view.
    pub fn read_root(
        reader: &mut dyn ReadSeek,
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
        container_only: bool,
    ) -> Result<(Self, View<'a>), CodecError> {
        if reader.seek(SeekFrom::End(0)).is_ok() {
            reader.rewind().map_err(CodecError::Io)?;
        }
        let ctx = Self::new(arena, policy, container_only);
        let mut buffer = Vec::new();
        ctx.complete_input(reader, &mut buffer)?;
        let bytes = arena.alloc(&ctx, buffer.into_boxed_slice())?;
        Ok((ctx, View::over_space(bytes, SpaceId::ROOT)))
    }

    /// Builds a context over caller-owned root bytes, for fuzz targets and
    /// tests. The arena still backs any expansions produced during decode.
    pub fn from_root_bytes(
        bytes: &'a [u8],
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
    ) -> Result<(Self, View<'a>), CodecError> {
        Self::from_bytes(bytes, arena, policy, false)
    }

    /// Builds a root context with a resource-only refusal channel.
    pub fn from_root_bytes_limit(
        bytes: &'a [u8],
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
    ) -> Result<(Self, View<'a>), ResourceLimit> {
        Self::from_bytes_limit(bytes, arena, policy, false)
    }

    fn from_bytes(
        bytes: &'a [u8],
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
        container_only: bool,
    ) -> Result<(Self, View<'a>), CodecError> {
        Self::from_bytes_limit(bytes, arena, policy, container_only).map_err(Into::into)
    }

    fn from_bytes_limit(
        bytes: &'a [u8],
        arena: &'a DecodeArena,
        policy: &DecodePolicy,
        container_only: bool,
    ) -> Result<(Self, View<'a>), ResourceLimit> {
        let length = u64_from_index(bytes.len());
        if length > policy.limits.max_input_bytes {
            return Err(root_limit(
                ResourceFailure::BudgetExceeded,
                policy.limits.max_input_bytes,
                length,
            ));
        }
        let ctx = DecodeContext {
            arena,
            container_only,
            budget: DecodeBudget::new(*policy, length),
            derived_spaces: Cell::new(0),
        };
        Ok((ctx, View::over_space(bytes, SpaceId::ROOT)))
    }

    /// Returns the decode policy in force.
    pub fn policy(&self) -> &DecodePolicy {
        self.budget.policy()
    }

    /// Returns whether the caller requested container-only decoding.
    pub fn container_only(&self) -> bool {
        self.container_only
    }

    fn decompression_allowance(&self) -> u64 {
        self.budget.decompression_allowance()
    }

    fn per_expand_allowance(&self) -> u64 {
        let policy_limit = self
            .budget
            .policy()
            .limits
            .max_decompressed_bytes_per_expand;
        let Some(proportional) = DECOMPRESSED_PER_EXPAND_PER_INPUT_BYTE
            .checked_mul(self.budget.input_bytes())
            .and_then(|bytes| DECOMPRESSED_PER_EXPAND_BASE.checked_add(bytes))
        else {
            return policy_limit;
        };
        policy_limit.min(proportional)
    }

    fn allocate_space(&self) -> Result<SpaceId, CodecError> {
        let used = self.derived_spaces.get();
        let index = used.checked_add(1).ok_or_else(|| {
            // Root owns zero; every other `usize` value identifies a derived space.
            self.budget.refuse(
                ResourceDimension::Codec("decode address spaces"),
                ResourceFailure::BudgetExceeded,
                u64_from_index(usize::MAX),
                u64_from_index(used),
                1,
                "decode address spaces",
            )
        })?;
        self.derived_spaces.set(index);
        Ok(SpaceId::from_index(index))
    }

    /// Records a permanent fuse and returns the resource error to propagate.
    fn fuse(
        &self,
        reason: ResourceFailure,
        scope: LimitScope,
        amount: u64,
        operation: &'static str,
    ) -> CodecError {
        let limit = match scope {
            LimitScope::Global => self.decompression_allowance(),
            LimitScope::PerExpand => self.per_expand_allowance(),
        };
        self.budget.refuse(
            ResourceDimension::DecompressedBytes,
            reason,
            limit,
            self.budget.decompressed_used(),
            amount,
            operation,
        )
    }

    /// Reserves bytes held by a temporary materialization.
    pub fn reserve_scoped(
        &self,
        bytes: u64,
        operation: &'static str,
    ) -> Result<ScopedReservation<'_>, CodecError> {
        self.budget.reserve_scoped(bytes, operation)
    }

    /// Admits a temporary byte allocation and holds its reservation until released.
    pub fn try_materialized<T>(
        &self,
        bytes: u64,
        operation: &'static str,
        allocate: impl FnOnce() -> Result<T, std::collections::TryReserveError>,
    ) -> Result<(ScopedReservation<'_>, T), CodecError> {
        let reservation = self.reserve_scoped(bytes, operation)?;
        let value =
            allocate().map_err(|_| self.budget.scoped_allocation_failed(bytes, operation))?;
        Ok((reservation, value))
    }

    /// Reserves temporary bytes and returns the typed resource refusal.
    pub fn reserve_scoped_limit(
        &self,
        bytes: u64,
        operation: &'static str,
    ) -> Result<ScopedReservation<'_>, ResourceLimit> {
        self.budget.reserve_scoped_limit(bytes, operation)
    }

    /// Charges bytes retained for the remainder of this session.
    pub fn charge_retained(&self, bytes: u64, operation: &'static str) -> Result<(), CodecError> {
        self.budget.charge_retained(bytes, operation)
    }

    pub(crate) fn retained_size_overflow_limit(&self, operation: &'static str) -> ResourceLimit {
        self.budget.retained_size_overflow_limit(operation)
    }

    /// Charge retained storage with a resource-only refusal channel.
    pub fn charge_retained_limit(
        &self,
        bytes: u64,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        self.budget.charge_retained_limit(bytes, operation)
    }

    /// Charges storage created by the closure as temporary until its reservation is dropped.
    pub fn with_scoped_storage<T, E: From<CodecError>>(
        &self,
        operation: &'static str,
        build: impl FnOnce() -> Result<T, E>,
    ) -> Result<(T, ScopedReservation<'_>), E> {
        let mut storage = self.reserve_scoped(0, operation)?;
        let value = storage.with_storage(build)?;
        Ok((value, storage))
    }

    /// Copies admitted text into retained storage with a typed resource refusal.
    pub fn copy_retained_text_limit(
        &self,
        text: &str,
        operation: &'static str,
    ) -> Result<String, ResourceLimit> {
        let bytes = super::u64_from_index(text.len());
        self.budget.charge_work_limit(bytes, operation)?;
        self.budget.charge_retained_limit(bytes, operation)?;
        let mut copy = String::new();
        copy.try_reserve_exact(text.len()).map_err(|_| {
            self.budget
                .retained_allocation_failed_limit(bytes, operation)
        })?;
        copy.push_str(text);
        Ok(copy)
    }

    /// Copies admitted text into session-retained storage.
    pub fn copy_retained_text(
        &self,
        text: &str,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        self.copy_retained_text_limit(text, operation)
            .map_err(Into::into)
    }

    /// Copies bytes into session-retained storage after charging and reserving safely.
    pub fn copy_retained(
        &self,
        bytes: &[u8],
        operation: &'static str,
    ) -> Result<Vec<u8>, CodecError> {
        self.charge_work(u64_from_index(bytes.len()), operation)?;
        self.charge_retained(u64_from_index(bytes.len()), operation)?;
        let mut copy = Vec::new();
        copy.try_reserve_exact(bytes.len()).map_err(|_| {
            self.budget
                .retained_allocation_failed(u64_from_index(bytes.len()), operation)
        })?;
        copy.extend_from_slice(bytes);
        Ok(copy)
    }

    /// Reserves growth of a session-retained string after charging its bytes.
    pub fn try_reserve_retained_text(
        &self,
        text: &mut String,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let required = text
            .len()
            .checked_add(additional)
            .ok_or_else(|| CodecError::from(self.retained_size_overflow_limit(operation)))?;
        let growth = if required > text.capacity() {
            required - text.capacity()
        } else {
            0
        };
        let bytes = u64_from_index(growth);
        self.charge_retained(bytes, operation)?;
        Self::reserve_admitted_string(text, additional, operation)
            .map_err(|_| self.budget.retained_allocation_failed(bytes, operation))
    }

    /// Copies source bytes as UTF-8 with replacement characters after charging
    /// the exact length of the retained text.
    pub fn copy_retained_lossy_utf8(
        &self,
        value: &[u8],
        operation: &'static str,
    ) -> Result<String, CodecError> {
        self.charge_work(u64_from_index(value.len()), operation)?;
        let mut remaining = value;
        let mut length = 0usize;
        loop {
            match std::str::from_utf8(remaining) {
                Ok(valid) => {
                    length = length
                        .checked_add(valid.len())
                        .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
                    break;
                }
                Err(error) => {
                    length = length
                        .checked_add(error.valid_up_to())
                        .and_then(|length| length.checked_add('\u{FFFD}'.len_utf8()))
                        .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
                    let Some(invalid_len) = error.error_len() else {
                        break;
                    };
                    remaining = &remaining[error.valid_up_to() + invalid_len..];
                }
            }
        }
        let mut text = String::new();
        self.charge_work(u64_from_index(value.len()), operation)?;
        self.charge_work(u64_from_index(length), operation)?;
        self.try_reserve_retained_text(&mut text, length, operation)?;
        let mut remaining = value;
        loop {
            match std::str::from_utf8(remaining) {
                Ok(valid) => {
                    text.push_str(valid);
                    break;
                }
                Err(error) => {
                    let valid_len = error.valid_up_to();
                    self.charge_work(u64_from_index(valid_len), operation)?;
                    text.push_str(
                        std::str::from_utf8(&remaining[..valid_len])
                            .map_err(|_| CodecError::malformed("valid UTF-8 prefix changed"))?,
                    );
                    text.push('\u{FFFD}');
                    let Some(invalid_len) = error.error_len() else {
                        break;
                    };
                    remaining = &remaining[valid_len + invalid_len..];
                }
            }
        }
        Ok(text)
    }

    /// Allocates `count` copies of `value` after charging slots, backing bytes
    /// and fill work, with fallible storage admission.
    ///
    /// Prefer this over `vec![value; parsed_count]` for attacker-influenced sizes.
    pub fn alloc_filled<T: Clone>(
        &self,
        count: usize,
        value: T,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        self.charge_work(u64_from_index(count), operation)?;
        let mut values = self.collection_vec(count, operation)?;
        values.resize(count, value);
        Ok(values)
    }

    /// Allocates `count` copies of `value` whose backing bytes the caller has
    /// already admitted, as retained or scoped storage, under its own operation.
    ///
    /// Charges collection items and fill work only, so the bytes are admitted
    /// exactly once. The caller must have admitted `count * size_of::<T>()`
    /// bytes before this call.
    pub fn alloc_filled_admitted<T: Clone>(
        &self,
        count: usize,
        value: T,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        self.charge_work(u64_from_index(count), operation)?;
        self.charge_collection_items(u64_from_index(count), operation)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| self.collection_allocation_failed(count, operation))?;
        values.resize(count, value);
        Ok(values)
    }

    /// Charges admitted entities.
    pub fn charge_entities(&self, count: u64, operation: &'static str) -> Result<(), CodecError> {
        self.budget.charge_entities(count, operation)
    }

    /// Charges entities newly present since `admitted`, then advances `admitted`.
    ///
    /// Codecs call this at admission boundaries so `max_entities` refuses further
    /// work instead of only reporting after a finished IR is built.
    pub fn admit_entities(
        &self,
        current: u64,
        admitted: &mut u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if current < *admitted {
            *admitted = current;
            return Ok(());
        }
        let additional = current - *admitted;
        self.charge_entities(additional, operation)?;
        *admitted = current;
        Ok(())
    }

    /// Charges admitted collection items.
    pub fn charge_collection_items(
        &self,
        count: u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.budget.charge_collection_items(count, operation)
    }

    /// Charges collection items before a fallible collection allocation.
    pub fn try_collection<T>(
        &self,
        count: usize,
        operation: &'static str,
        allocate: impl FnOnce() -> Result<T, std::collections::TryReserveError>,
    ) -> Result<T, CodecError> {
        let count = u64_from_index(count);
        self.charge_collection_items(count, operation)?;
        allocate().map_err(|_| self.budget.collection_allocation_failed(count, operation))
    }

    /// Reports allocator refusal after a collection-item charge was recorded.
    pub(super) fn collection_allocation_failed(
        &self,
        count: usize,
        operation: &'static str,
    ) -> CodecError {
        self.budget
            .collection_allocation_failed(u64_from_index(count), operation)
    }

    /// Charges collection items and returns the typed refusal for resource-only callers.
    pub fn charge_collection_items_limit(
        &self,
        count: u64,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        self.budget.charge_collection_items_limit(count, operation)
    }

    /// Enters one recursive nesting level until the returned guard is dropped.
    pub fn enter_nested(&self, operation: &'static str) -> Result<DepthGuard<'_>, CodecError> {
        self.enter_nested_limit(operation).map_err(Into::into)
    }

    /// Enters one nesting level with a resource-only refusal.
    pub fn enter_nested_limit(
        &self,
        operation: &'static str,
    ) -> Result<DepthGuard<'_>, ResourceLimit> {
        self.budget.enter_nested(operation)
    }

    /// Charges session-global algorithm work, fusing on refusal.
    pub fn charge_work(&self, units: u64, operation: &'static str) -> Result<(), CodecError> {
        self.budget.charge_work(units, operation)
    }

    /// Charges work with a resource-only refusal channel.
    pub fn charge_work_limit(
        &self,
        units: u64,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        self.budget.charge_work_limit(units, operation)
    }

    /// Sort admitted values stably using fallible index scratch.
    ///
    /// `key_bytes` states the key bytes a comparison can read from each value.
    /// Equal values retain input order. Values move in place without cloning children.
    pub fn stable_sort_by<T>(
        &self,
        values: &mut [T],
        mut compare: impl FnMut(&T, &T) -> std::cmp::Ordering,
        key_bytes: impl Fn(&T) -> usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let count = super::u64_from_index(values.len());
        self.charge_work(count, operation)?;
        let mut compared = |left: usize, right: usize| {
            let work = super::u64_from_index(key_bytes(&values[left]))
                .checked_add(super::u64_from_index(key_bytes(&values[right])))
                .and_then(|work| work.checked_add(1))
                .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
            self.charge_work(work, operation)?;
            Ok::<_, CodecError>(compare(&values[left], &values[right]))
        };
        let mut ordered = true;
        for index in 1..values.len() {
            if compared(index - 1, index)?.is_gt() {
                ordered = false;
                break;
            }
        }
        if ordered {
            return Ok(());
        }
        let index_bytes = super::u64_from_index(std::mem::size_of::<usize>());
        let scratch_bytes = count
            .checked_mul(index_bytes)
            .and_then(|bytes| bytes.checked_mul(2))
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        self.charge_work(scratch_bytes, operation)?;
        let mut order_vec = Vec::new();
        let mut destinations_vec = Vec::new();
        let _scratch = if values.len() > super::sort::INLINE_SORT_LIMIT {
            self.charge_collection_items(
                count
                    .checked_mul(2)
                    .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?,
                operation,
            )?;
            let reservation = self.reserve_scoped(scratch_bytes, operation)?;
            order_vec.try_reserve_exact(values.len()).map_err(|_| {
                self.budget
                    .scoped_allocation_failed(scratch_bytes, operation)
            })?;
            destinations_vec
                .try_reserve_exact(values.len())
                .map_err(|_| {
                    self.budget
                        .scoped_allocation_failed(scratch_bytes, operation)
                })?;
            order_vec.extend(0..values.len());
            destinations_vec.resize(values.len(), 0usize);
            Some(reservation)
        } else {
            None
        };
        let mut inline_order = [0usize; super::sort::INLINE_SORT_LIMIT];
        let mut inline_destinations = [0usize; super::sort::INLINE_SORT_LIMIT];
        let (mut order, mut destinations) = if values.len() > super::sort::INLINE_SORT_LIMIT {
            (order_vec.as_mut_slice(), destinations_vec.as_mut_slice())
        } else {
            for (index, slot) in inline_order[..values.len()].iter_mut().enumerate() {
                *slot = index;
            }
            (
                &mut inline_order[..values.len()],
                &mut inline_destinations[..values.len()],
            )
        };
        // Merge index runs fallibly. No record moves before all work is admitted.
        let mut width = 1usize;
        while width < values.len() {
            // Each output index is read and written; comparisons read two more.
            self.charge_work(
                scratch_bytes
                    .checked_mul(2)
                    .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
                operation,
            )?;
            let mut start = 0;
            while start < values.len() {
                let middle = start + width.min(values.len() - start);
                let end = middle + width.min(values.len() - middle);
                let (mut left, mut right) = (start, middle);
                for destination in &mut destinations[start..end] {
                    if left < middle
                        && (right == end || !compared(order[left], order[right])?.is_gt())
                    {
                        *destination = order[left];
                        left += 1;
                    } else {
                        *destination = order[right];
                        right += 1;
                    }
                }
                start = end;
            }
            std::mem::swap(&mut order, &mut destinations);
            width += width.min(values.len() - width);
        }
        // The inverse permutation and at most N-1 swaps have linear move work.
        let permutation = super::u64_from_index(std::mem::size_of::<T>())
            .checked_mul(4)
            .and_then(|bytes| bytes.checked_add(index_bytes.checked_mul(8)?))
            .and_then(|bytes| count.checked_mul(bytes))
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        self.charge_work(permutation, operation)?;
        for (destination, &source) in order.iter().enumerate() {
            destinations[source] = destination;
        }
        for index in 0..values.len() {
            while destinations[index] != index {
                let destination = destinations[index];
                values.swap(index, destination);
                destinations.swap(index, destination);
            }
        }
        Ok(())
    }

    /// Returns the resource refusal that has fused this decode session.
    pub fn resource_refusal(&self) -> Option<ResourceLimit> {
        self.budget.fused()
    }

    /// Permanently refuses a codec-local resource request.
    ///
    /// Codecs use this when a bounded recovery algorithm reaches a fixed
    /// local ceiling instead of a session-wide dimension. The refusal fuses
    /// the session so a caller cannot accidentally turn it into a semantic
    /// fallback or report success after the limit was reached.
    pub(crate) fn refuse_local_limit(
        &self,
        operation: &'static str,
        limit: u64,
        requested: u64,
    ) -> ResourceLimit {
        let (used, additional) = match requested.checked_sub(limit) {
            Some(excess) => (limit, excess),
            None => (requested, 0),
        };
        self.budget.refuse_limit(
            ResourceDimension::Codec(operation),
            ResourceFailure::BudgetExceeded,
            limit,
            used,
            additional,
            operation,
        )
    }

    /// Permanently refuses a codec-local resource request.
    pub fn refuse_codec_limit(
        &self,
        operation: &'static str,
        limit: u64,
        requested: u64,
    ) -> CodecError {
        self.refuse_local_limit(operation, limit, requested).into()
    }

    /// Creates a local work slice that also draws from the session allowance.
    pub fn work_budget(&self, local_limit: u64) -> WorkBudget<'_> {
        WorkBudget::for_session(local_limit, &self.budget)
    }

    // --- decompression ------------------------------------------------------

    /// Begins an expansion whose output is charged incrementally and becomes
    /// available only after successful finalization.
    pub fn begin_expand(&self, spec: ExpandSpec) -> Result<ExpandWriter<'_, 'a>, CodecError> {
        if let Some(limit) = self.budget.fused() {
            return Err(CodecError::ResourceLimit(limit));
        }
        if let ExpandSpec::Exact(size) = spec {
            let per_expand = self.per_expand_allowance();
            if size > per_expand {
                return Err(self.fuse(
                    ResourceFailure::BudgetExceeded,
                    LimitScope::PerExpand,
                    size,
                    "begin_expand",
                ));
            }
            if self
                .decompression_allowance()
                .checked_sub(self.budget.decompressed_used())
                .is_none_or(|remaining| size > remaining)
            {
                return Err(self.fuse(
                    ResourceFailure::BudgetExceeded,
                    LimitScope::Global,
                    size,
                    "begin_expand",
                ));
            }
        }
        let mut buffer: Vec<u8> = Vec::new();
        let reserve = match spec {
            // A declared size the address space cannot name is above the cap,
            // so the cap is the reservation either way.
            ExpandSpec::Exact(size) => match usize::try_from(size) {
                Ok(size) => size.min(RESERVE_CLAMP),
                Err(_) => RESERVE_CLAMP,
            },
            ExpandSpec::Unknown => 0,
        };
        if reserve > 0 {
            buffer.try_reserve(reserve).map_err(|_| {
                self.fuse(
                    ResourceFailure::AllocationFailed,
                    LimitScope::PerExpand,
                    u64_from_index(reserve),
                    "begin_expand",
                )
            })?;
        }
        Ok(ExpandWriter {
            ctx: self,
            spec,
            buffer,
        })
    }

    /// Copies several input extents into one derived view.
    pub fn concat_views(&self, inputs: &[View<'_>]) -> Result<View<'a>, CodecError> {
        if let Some(limit) = self.budget.fused() {
            return Err(CodecError::ResourceLimit(limit));
        }
        if inputs.is_empty() {
            return Err(CodecError::Malformed(
                "cannot concatenate an empty view list".into(),
            ));
        }
        self.charge_work(u64_from_index(inputs.len()), "concat_views")?;
        let total = inputs.iter().try_fold(0usize, |total, view| {
            total.checked_add(view.window().len()).ok_or_else(|| {
                self.budget.refuse(
                    ResourceDimension::RetainedBytes,
                    ResourceFailure::BudgetExceeded,
                    self.budget.policy().limits.max_retained_bytes,
                    u64_from_index(total),
                    u64_from_index(view.window().len()),
                    "concat_views",
                )
            })
        })?;
        self.charge_work(u64_from_index(total), "concat_views")?;
        let reservation = self.reserve_scoped(u64_from_index(total), "concat_views")?;
        let mut buffer = Vec::new();
        buffer.try_reserve_exact(total).map_err(|_| {
            self.budget.refuse(
                ResourceDimension::MaterializedBytes,
                ResourceFailure::AllocationFailed,
                self.budget.policy().limits.max_materialized_bytes,
                0,
                u64_from_index(total),
                "concat_views",
            )
        })?;
        for view in inputs {
            buffer.extend_from_slice(view.window());
        }
        let bytes = self.arena.alloc(self, buffer.into_boxed_slice())?;
        reservation.commit()?;
        let space = self.allocate_space()?;
        Ok(View::over_space(bytes, space))
    }

    /// Concatenates owned chunks into one retained buffer without an arena copy.
    pub fn concat_retained(
        &self,
        inputs: &[Vec<u8>],
        operation: &'static str,
    ) -> Result<Vec<u8>, CodecError> {
        if let Some(limit) = self.budget.fused() {
            return Err(CodecError::ResourceLimit(limit));
        }
        if inputs.is_empty() {
            return Err(CodecError::Malformed(
                "cannot concatenate an empty buffer list".into(),
            ));
        }
        let total = inputs.iter().try_fold(0_usize, |total, input| {
            total.checked_add(input.len()).ok_or_else(|| {
                CodecError::NotImplemented("retained concatenation exceeds usize".into())
            })
        })?;
        let total_bytes = crate::decode::u64_from_index(total);
        self.charge_work(u64_from_index(inputs.len()), operation)?;
        self.charge_work(total_bytes, operation)?;
        self.charge_retained(total_bytes, operation)?;
        let mut buffer = Vec::new();
        buffer.try_reserve_exact(total).map_err(|_| {
            self.budget
                .retained_allocation_failed(total_bytes, operation)
        })?;
        for input in inputs {
            buffer.extend_from_slice(input);
        }
        Ok(buffer)
    }

    /// Registers a stored (uncompressed) child range as a space that borrows
    /// the parent bytes without copying.
    ///
    /// `range` is expressed in the parent view's own space coordinates and must
    /// lie within the parent window; a range that escapes the parent is refused
    /// here, at the request site, exactly as [`View::child`] refuses. No bytes
    /// are copied. It is the archive-entry counterpart of [`DecodeContext::begin_expand`] —
    /// stored ZIP entries take this path, compressed ones take the expander.
    /// Registration still refuses on a fused context so a stored entry cannot be
    /// admitted after a refusal.
    pub fn register_slice<'v>(
        &self,
        parent: View<'v>,
        range: ByteRange,
    ) -> Result<View<'v>, CodecError> {
        if let Some(limit) = self.budget.fused() {
            return Err(CodecError::ResourceLimit(limit));
        }
        let start = usize::try_from(range.start).ok();
        let end = usize::try_from(range.end).ok();
        let child = start
            .zip(end)
            .and_then(|(start, end)| parent.child(start, end))
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "stored slice [{}, {}) escapes parent space {}",
                    range.start,
                    range.end,
                    parent.space().index()
                ))
            })?;
        self.charge_collection_items(1, "register borrowed space")?;
        let space = self.allocate_space()?;
        Ok(View::over_space(child.window(), space))
    }

    // --- lifecycle ----------------------------------------------------------

    /// Closes a decode or inspection session, returning a fused resource error
    /// even when codec code swallowed the charge that caused it.
    pub fn finish_session(self) -> Result<(), CodecError> {
        if let Some(limit) = self.budget.fused() {
            return Err(CodecError::ResourceLimit(limit));
        }
        Ok(())
    }
}

fn root_limit(reason: ResourceFailure, limit: u64, used: u64) -> ResourceLimit {
    if used <= limit {
        return ResourceLimit {
            dimension: ResourceDimension::InputBytes,
            reason,
            limit,
            used,
            additional: 0,
            operation: "read_root",
        };
    }
    let additional = used - limit;
    ResourceLimit {
        dimension: ResourceDimension::InputBytes,
        reason,
        limit,
        used,
        additional,
        operation: "read_root",
    }
}

/// How much output an expansion is expected to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpandSpec {
    /// A declared exact size, enforced per-write and at finalize.
    Exact(u64),
    /// No trustworthy declared size: the decompression limits apply.
    Unknown,
}

/// Writes decompressed output under incremental charging.
#[derive(Debug)]
pub struct ExpandWriter<'ctx, 'a> {
    ctx: &'ctx DecodeContext<'a>,
    spec: ExpandSpec,
    buffer: Vec<u8>,
}

impl<'a> ExpandWriter<'_, 'a> {
    /// Appends decompressed output, charging before it is retained.
    pub fn write(&mut self, data: &[u8]) -> Result<(), CodecError> {
        let len = u64_from_index(data.len());
        let new_written = self.written().checked_add(len).ok_or_else(|| {
            self.ctx.fuse(
                ResourceFailure::BudgetExceeded,
                LimitScope::PerExpand,
                len,
                "expand_write",
            )
        })?;
        match self.spec {
            ExpandSpec::Exact(size) if new_written > size => {
                return Err(CodecError::malformed(format_args!(
                    "expansion exceeded declared exact size {size}"
                )))
            }
            _ => {}
        }
        let per_expand = self.ctx.per_expand_allowance();
        if new_written > per_expand {
            return Err(self.ctx.fuse(
                ResourceFailure::BudgetExceeded,
                LimitScope::PerExpand,
                len,
                "expand_write",
            ));
        }
        self.ctx.budget.charge_decompressed(len, "expand_write")?;
        self.buffer.try_reserve(data.len()).map_err(|_| {
            self.ctx.fuse(
                ResourceFailure::AllocationFailed,
                LimitScope::PerExpand,
                len,
                "expand_write",
            )
        })?;
        self.buffer.extend_from_slice(data);
        Ok(())
    }

    /// Finalizes the expansion, stores it in the arena, and registers its space.
    pub fn finalize(self) -> Result<View<'a>, CodecError> {
        self.check_exact()?;
        let bytes = self
            .ctx
            .arena
            .alloc(self.ctx, self.buffer.into_boxed_slice())?;
        let space = self.ctx.allocate_space()?;
        Ok(View::over_space(bytes, space))
    }

    /// Finalizes an expansion directly into a caller-owned buffer.
    /// The expansion budget already admits its bytes, so no retained copy occurs.
    pub fn finalize_owned(self) -> Result<Vec<u8>, CodecError> {
        self.check_exact()?;
        Ok(self.buffer)
    }

    fn check_exact(&self) -> Result<(), CodecError> {
        if let ExpandSpec::Exact(size) = self.spec {
            if self.written() != size {
                return Err(CodecError::malformed(format_args!(
                    "expansion produced {} of declared exact {size} bytes",
                    self.written()
                )));
            }
        }
        Ok(())
    }

    /// Returns how many bytes have been written so far.
    pub fn written(&self) -> u64 {
        u64_from_index(self.buffer.len())
    }
}

#[cfg(test)]
mod tests {
    use super::{u64_from_index, ByteRange, DecodeArena, DecodeContext, DecodePolicy};
    use crate::decode::{ResourceDimension, ResourceFailure};
    use std::io::{self, Cursor, Read, Seek, SeekFrom};

    #[test]
    fn temporary_byte_storage_uses_its_byte_budget_and_releases_it() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 32;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let (reservation, bytes) = ctx
            .try_materialized(32, "test bytes", || {
                let mut bytes = Vec::<u8>::new();
                bytes.try_reserve_exact(32)?;
                Ok(bytes)
            })
            .expect("byte allowance");
        assert_eq!(bytes.capacity(), 32);
        drop(reservation);
        let (reservation, ()) = ctx
            .try_materialized(32, "test bytes", || Ok(()))
            .expect("released allowance");
        assert!(
            matches!(ctx.try_materialized(1, "test bytes", || Ok(())), Err(crate::CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes)
        );
        drop(reservation);
    }

    #[test]
    fn loaded_input_processing_preserves_proportional_and_absolute_limits() {
        let input_bytes = 1024;
        let allowance = crate::decode::policy::MATERIALIZED_BASE
            + input_bytes * crate::decode::policy::MATERIALIZED_PER_INPUT_BYTE;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        let ctx = DecodeContext::for_loaded_input(&arena, &policy, input_bytes)
            .expect("admitted input length");
        drop(
            ctx.reserve_scoped(allowance, "loaded input allowance")
                .expect("input-proportional allowance"),
        );
        assert!(
            matches!(ctx.reserve_scoped(allowance + 1, "loaded input allowance"),
            Err(crate::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes && limit.limit == allowance)
        );

        policy.limits.max_materialized_bytes = 32;
        let ctx = DecodeContext::for_loaded_input(&arena, &policy, input_bytes)
            .expect("admitted input length");
        assert!(matches!(ctx.reserve_scoped(33, "absolute limit"),
            Err(crate::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes && limit.limit == 32));

        policy.limits.max_input_bytes = input_bytes - 1;
        assert!(
            matches!(DecodeContext::for_loaded_input(&arena, &policy, input_bytes),
            Err(crate::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::InputBytes && limit.additional == input_bytes)
        );
    }

    #[test]
    fn zero_collection_limit_refuses_second_empty_finalization() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        for _ in 0..2 {
            let result = ctx
                .begin_expand(super::ExpandSpec::Exact(0))
                .and_then(super::ExpandWriter::finalize);
            assert!(
                matches!(result, Err(crate::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems)
            );
        }
    }

    #[test]
    fn one_collection_slot_admits_only_one_empty_finalization() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert!(ctx
            .begin_expand(super::ExpandSpec::Exact(0))
            .expect("empty expansion")
            .finalize()
            .expect("first registry slot")
            .window()
            .is_empty());
        assert!(
            matches!(ctx.begin_expand(super::ExpandSpec::Exact(0)).expect("empty expansion")
            .finalize(), Err(crate::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems && limit.used == 1)
        );
    }

    struct RewindFails(Cursor<Vec<u8>>);

    impl Read for RewindFails {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.0.read(buffer)
        }
    }

    impl Seek for RewindFails {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            if position == SeekFrom::Start(0) {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "rewind denied",
                ))
            } else {
                self.0.seek(position)
            }
        }
    }

    #[test]
    fn retained_collection_refuses_slot_before_copying_the_record() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
        let copied = std::cell::Cell::new(false);
        let result = ctx.try_collect_retained_with([1_u8], "collect record", |value| {
            copied.set(true);
            Ok::<_, crate::CodecError>(value)
        });
        assert!(
            matches!(result, Err(crate::CodecError::ResourceLimit(limit))
            if limit.dimension == super::ResourceDimension::RetainedBytes && limit.additional == 1)
        );
        assert!(!copied.get());
    }

    #[test]
    fn scoped_copy_storage_is_released_and_does_not_charge_retained_bytes() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 4;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
        let (text, reservation) = ctx
            .with_scoped_storage("temporary copy", || {
                ctx.copy_retained_text("abcd", "temporary copy")
            })
            .expect("admitted test operation");
        assert_eq!(text, "abcd");
        drop(text);
        drop(reservation);
        assert!(ctx.reserve_scoped(4, "reuse scope").is_ok());
    }

    #[test]
    fn scoped_copy_storage_refuses_materialized_bytes() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 3;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
        assert!(matches!(ctx.with_scoped_storage("temporary copy", || {
            ctx.copy_retained_text("abcd", "temporary copy")
        }), Err(crate::CodecError::ResourceLimit(limit))
            if limit.dimension == super::ResourceDimension::MaterializedBytes
                && limit.additional == 4));
    }

    #[test]
    fn scoped_copy_extends_existing_reservation_without_double_charging() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 6;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
        let mut storage = ctx
            .reserve_scoped(2, "temporary record")
            .expect("admitted test operation");
        let text = storage
            .with_storage(|| ctx.copy_retained_text("abcd", "temporary text"))
            .expect("admitted test operation");
        assert_eq!(text, "abcd");
        assert!(
            matches!(ctx.reserve_scoped(1, "full scope"), Err(crate::CodecError::ResourceLimit(limit)) if limit.used == 6)
        );
        drop(text);
        drop(storage);
    }

    #[test]
    fn nested_scoped_copies_hold_independent_reservations() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 6;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
        let ((first, second, inner), outer) = ctx
            .with_scoped_storage("outer", || {
                let first = ctx.copy_retained_text("ab", "outer")?;
                let (second, inner) =
                    ctx.with_scoped_storage("inner", || ctx.copy_retained_text("cd", "inner"))?;
                Ok::<_, crate::CodecError>((first, second, inner))
            })
            .expect("admitted test operation");
        assert_eq!((first.as_str(), second.as_str()), ("ab", "cd"));
        assert!(ctx.reserve_scoped(2, "remaining scope").is_ok());
        drop(first);
        drop(outer);
        assert!(ctx.reserve_scoped(4, "outer released").is_ok());
        drop(second);
        drop(inner);
        assert!(ctx.reserve_scoped(6, "all released").is_ok());
    }

    #[test]
    fn root_reader_preserves_bytes_across_fixed_chunk_boundaries() {
        let mut bytes = vec![0x5a_u8; 8193];
        bytes[8192] = 0x7f;
        let mut reader = Cursor::new(bytes.clone());
        let arena = DecodeArena::new();
        let (_, root) =
            DecodeContext::read_root(&mut reader, &arena, &DecodePolicy::default(), false)
                .expect("root is admitted");
        assert_eq!(root.window(), bytes.as_slice());
    }

    #[test]
    fn root_reader_propagates_failed_rewind_after_a_successful_size_probe() {
        let arena = DecodeArena::new();
        let mut reader = RewindFails(Cursor::new(b"not empty".to_vec()));
        assert!(matches!(
            DecodeContext::read_root(&mut reader, &arena, &DecodePolicy::default(), false),
            Err(crate::CodecError::Io(error)) if error.kind() == io::ErrorKind::PermissionDenied
        ));
    }

    #[test]
    fn exhausted_space_ids_refuse_registration_without_reusing_an_id() {
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(b"x", &arena, &DecodePolicy::default())
            .expect("test input fits the policy");
        ctx.derived_spaces.set(usize::MAX - 1);
        let range = ByteRange { start: 0, end: 1 };
        let view = ctx
            .register_slice(root, range)
            .expect("maximum space id is allocatable");
        assert_eq!(view.space().index(), usize::MAX);
        let error = ctx
            .register_slice(root, range)
            .expect_err("space IDs are exhausted");
        let crate::CodecError::ResourceLimit(limit) = error else {
            panic!("exhausted IDs return a resource refusal");
        };
        assert_eq!(limit.limit, u64_from_index(usize::MAX));
        assert_eq!(limit.used, u64_from_index(usize::MAX));
        assert_eq!(limit.additional, 1);
        assert!(ctx.register_slice(root, range).is_err());
        assert!(ctx.finish_session().is_err());
    }

    #[test]
    fn collection_reservation_reports_allocator_refusal_after_charge() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = u64::MAX;
        policy.limits.max_retained_bytes = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(b"x", &arena, &policy)
            .expect("test input fits the policy");
        let mut values = Vec::<u8>::new();
        let error = ctx
            .reserve_vec(&mut values, usize::MAX, "test collection reservation")
            .expect_err("a vector cannot reserve more than isize::MAX bytes");
        assert!(matches!(
            error,
            crate::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.reason == ResourceFailure::AllocationFailed
                    && limit.used == 0
                    && limit.additional == u64_from_index(usize::MAX)
                    && limit.operation == "test collection reservation"
        ));
        assert!(ctx.finish_session().is_err());
    }

    #[test]
    fn try_collection_refuses_before_allocation_and_succeeds_under_service_profile() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(b"x", &arena, &policy)
            .expect("test input fits the policy");
        let mut allocated = false;
        let error = ctx
            .try_collection(3, "test collection allocation", || {
                allocated = true;
                Ok(())
            })
            .expect_err("three items exceed a limit of two");
        assert!(!allocated);
        assert!(matches!(
            error,
            crate::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.reason == ResourceFailure::BudgetExceeded
                    && limit.operation == "test collection allocation"
        ));

        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"x", &arena, &DecodePolicy::service())
            .expect("test input fits the policy");
        let mut values = Vec::<u8>::new();
        ctx.try_collection(3, "test collection allocation", || values.try_reserve(3))
            .expect("the service profile admits three items");
        assert!(values.capacity() >= 3);
    }

    #[test]
    fn try_collection_reports_allocator_refusal_after_charge() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(b"x", &arena, &policy)
            .expect("test input fits the policy");
        let mut values = Vec::<u8>::new();
        let error = ctx
            .try_collection(usize::MAX, "test collection allocation", || {
                values.try_reserve(usize::MAX)
            })
            .expect_err("a vector cannot reserve more than isize::MAX bytes");
        assert!(matches!(
            error,
            crate::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.reason == ResourceFailure::AllocationFailed
                    && limit.operation == "test collection allocation"
        ));
    }

    #[test]
    fn scoped_text_copy_refuses_before_allocation_and_releases_on_drop() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 3;
        let (ctx, _) = DecodeContext::from_root_bytes(b"x", &arena, &policy)
            .expect("test input fits the policy");
        {
            let mut reservation = ctx
                .reserve_scoped(0, "test scoped text")
                .expect("empty reservation is admitted");
            let copy = ctx
                .copy_scoped_text("abc", &mut reservation, "test scoped text")
                .expect("three temporary bytes are admitted");
            assert_eq!(copy, "abc");
        }
        {
            let mut reservation = ctx
                .reserve_scoped(0, "test scoped text")
                .expect("empty reservation is admitted");
            assert_eq!(
                ctx.copy_scoped_text("def", &mut reservation, "test scoped text")
                    .expect("the prior reservation was released"),
                "def"
            );
        }
        let mut reservation = ctx
            .reserve_scoped(0, "test scoped text")
            .expect("empty reservation is admitted");
        let error = ctx
            .copy_scoped_text("abcd", &mut reservation, "test scoped text")
            .expect_err("four bytes exceed the temporary limit");
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "test scoped text"));
    }

    #[test]
    fn scoped_format_refuses_before_allocation_and_releases_on_drop() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 3;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        {
            let (text, _reservation) = ctx
                .format_scoped(format_args!("x{}", 12), "test scoped format")
                .expect("three rendered bytes fit");
            assert_eq!(text, "x12");
        }
        assert_eq!(
            ctx.format_scoped(format_args!("y{}", 34), "test scoped format")
                .expect("prior reservation was released")
                .0,
            "y34"
        );
        let error = ctx
            .format_scoped(format_args!("z{}", 345), "test scoped format")
            .expect_err("four bytes exceed the materialized allowance");
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "test scoped format"));
    }

    #[test]
    fn retained_format_refuses_before_string_growth() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 4;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        assert_eq!(
            ctx.format_retained(
                format_args!("{number}", number = 1234),
                "test retained format"
            )
            .expect("four rendered bytes fit"),
            "1234"
        );
        let error = ctx
            .format_retained(format_args!("{}", 5), "test retained format")
            .expect_err("one more byte exceeds the retained limit");
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "test retained format"));
    }
    #[test]
    fn string_growth_charges_only_new_capacity() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
        let mut text = String::with_capacity(4);
        ctx.try_reserve_retained_text(&mut text, 4, "existing text capacity")
            .expect("admitted test operation");
        assert_eq!(text.capacity(), 4);
        let error = ctx
            .try_reserve_retained_text(&mut text, 5, "new text capacity")
            .expect_err("test operation refuses");
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.used == 0 && limit.additional == 1));
        assert_eq!(text.capacity(), 4);
    }

    #[test]
    fn text_append_reserves_exact_growth() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 3;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
        let mut text = ctx
            .copy_retained_text("ab", "initial text")
            .expect("admitted test operation");
        ctx.append_retained(&mut text, "c", "append text")
            .expect("admitted test operation");
        assert_eq!(text, "abc");
        assert_eq!(text.capacity(), 3);
        let error = ctx
            .append_retained(&mut text, "d", "append text")
            .expect_err("test operation refuses");
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.used == 3 && limit.additional == 1));
        assert_eq!(text, "abc");
    }
}

#[cfg(test)]
mod sort_tests;
