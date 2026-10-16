// SPDX-License-Identifier: Apache-2.0
//! Charged growth of collections owned by a decode session.

use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque};
use std::fmt::{self, Write};
use std::hash::Hash;

use crate::CodecError;

use super::{
    u64_from_index, BoundedCount, DecodeContext, ResourceDimension, ResourceLimit,
    ScopedReservation,
};

/// A vector that must contain exactly a count proven against input.
#[derive(Debug)]
pub struct ExactVec<T> {
    values: Vec<T>,
    capacity: usize,
}

impl<T> ExactVec<T> {
    /// Charges and allocates storage for a count bounded by the input window.
    pub fn new(
        ctx: &DecodeContext<'_>,
        count: BoundedCount,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        let capacity = count.get();
        let values = ctx.collection_vec(capacity, operation)?;
        Ok(Self { values, capacity })
    }

    /// Appends one value without exceeding the bounded count.
    pub fn push(&mut self, value: T) -> Result<(), CodecError> {
        if self.values.len() == self.capacity {
            return Err(CodecError::Malformed(
                "fixed-capacity vector overflow".to_owned(),
            ));
        }
        self.values.push(value);
        Ok(())
    }

    /// Returns the values if the bounded count was filled exactly.
    pub fn finish(self) -> Result<Vec<T>, CodecError> {
        if self.values.len() == self.capacity {
            Ok(self.values)
        } else {
            Err(CodecError::malformed(format_args!(
                "fixed-capacity vector contains {} of {} values",
                self.values.len(),
                self.capacity
            )))
        }
    }
}

#[derive(Clone, Copy)]
enum VecGrowth {
    Exact,
    Amortized,
}

impl DecodeContext<'_> {
    /// Reserve backing storage without admitting values not yet inserted.
    pub fn reserve_capacity_limit<T>(
        &self,
        values: &mut Vec<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        self.reserve_retained_vec_storage(values, count, VecGrowth::Amortized, None, operation)
    }

    fn reserve_retained_vec_storage<T>(
        &self,
        values: &mut Vec<T>,
        count: usize,
        growth: VecGrowth,
        items: Option<usize>,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        let required = values
            .len()
            .checked_add(count)
            .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        let capacity = if std::mem::size_of::<T>() == 0 || required <= values.capacity() {
            values.capacity()
        } else if matches!(growth, VecGrowth::Exact) {
            required
        } else if values.capacity() == 0 {
            let minimum = match std::mem::size_of::<T>() {
                1 => 8,
                2..=1024 => 4,
                _ => 1,
            };
            required.max(minimum)
        } else {
            match values.capacity().checked_mul(2) {
                Some(grown) if grown > required => grown,
                _ => required,
            }
        };
        let added = capacity - values.capacity();
        let bytes = added
            .checked_mul(std::mem::size_of::<T>())
            .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        let maximum = usize::try_from(isize::MAX)
            .map_err(|_| self.retained_size_overflow_limit(operation))?;
        if bytes > maximum {
            return Err(self
                .budget
                .retained_allocation_failed_limit(u64_from_index(bytes), operation));
        }
        self.charge_retained_limit(u64_from_index(bytes), operation)?;
        if let Some(items) = items {
            self.charge_collection_items_limit(u64_from_index(items), operation)?;
        }
        if added != 0 {
            values
                .try_reserve_exact(capacity - values.len())
                .map_err(|_| {
                    self.budget
                        .retained_allocation_failed_limit(u64_from_index(bytes), operation)
                })?;
        }
        Ok(())
    }

    /// Retains vector storage whose slots were admitted in aggregate.
    pub fn reserve_capacity<T>(
        &self,
        values: &mut Vec<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_retained_vec_storage(values, count, VecGrowth::Amortized, None, operation)
            .map_err(Into::into)
    }

    /// Creates retained storage whose collection slots are admitted separately.
    pub fn vector_storage<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        let mut values = Vec::new();
        self.reserve_retained_vec_storage(&mut values, count, VecGrowth::Exact, None, operation)?;
        Ok(values)
    }

    /// Admits scoped collection slots whose storage is allocated by a later operation.
    pub fn reserve_scoped_collection<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<ScopedReservation<'_>, CodecError> {
        self.charge_collection_items(u64_from_index(count), operation)?;
        let bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .ok_or_else(|| self.budget.scoped_size_overflow_limit(operation))?;
        self.reserve_scoped(u64_from_index(bytes), operation)
    }

    /// Creates scoped storage whose collection slots are admitted separately.
    pub fn scoped_vector_storage<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<(Vec<T>, ScopedReservation<'_>), CodecError> {
        let mut reservation = self.reserve_scoped(0, operation)?;
        let values = reservation.with_storage(|| self.vector_storage(count, operation))?;
        Ok((values, reservation))
    }

    /// Grows scoped storage and admits additional vector slots.
    pub fn reserve_scoped_vec<T>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        values: &mut Vec<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_scoped_vec_limit(reservation, values, count, operation)
            .map_err(Into::into)
    }

    /// Reserves storage with a resource-only refusal channel.
    pub fn reserve_scoped_vec_limit<T>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        values: &mut Vec<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        reservation.with_storage_limit(|| self.reserve_vec_limit(values, count, operation))
    }

    /// Appends one item with scoped storage and a charged collection slot.
    pub fn push_scoped_vec<T>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        values: &mut Vec<T>,
        value: T,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_scoped_vec(reservation, values, 1, operation)?;
        values.push(value);
        Ok(())
    }

    /// Reserves temporary vector storage and returns its live byte reservation.
    pub fn reserve_temporary_vec<T>(
        &self,
        values: &mut Vec<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<ScopedReservation<'_>, ResourceLimit> {
        let mut reservation = self.reserve_scoped_limit(0, operation)?;
        reservation.with_storage_limit(|| {
            self.reserve_retained_vec_storage(
                values,
                count,
                VecGrowth::Exact,
                Some(count),
                operation,
            )
        })?;
        Ok(reservation)
    }

    /// Copies a slice into scoped storage and returns its live byte reservation.
    pub fn copy_temporary_slice<T: Copy>(
        &self,
        values: &[T],
        operation: &'static str,
    ) -> Result<(Vec<T>, ScopedReservation<'_>), ResourceLimit> {
        let mut copy = Vec::new();
        let reservation = self.reserve_temporary_vec(&mut copy, values.len(), operation)?;
        self.charge_work_limit(u64_from_index(values.len()), operation)?;
        copy.extend_from_slice(values);
        Ok((copy, reservation))
    }

    /// Collects retained text and charges both vector storage and text bytes.
    pub fn collect_retained_texts<'text>(
        &self,
        values: impl IntoIterator<Item = &'text str>,
        operation: &'static str,
    ) -> Result<Vec<String>, CodecError> {
        self.try_collect_retained_with(values, operation, |value| {
            self.copy_retained_text(value, operation)
        })
    }

    /// Copies scoped text values and returns the reservation for their storage.
    pub fn collect_scoped_texts<'text>(
        &self,
        values: impl IntoIterator<Item = &'text str>,
        operation: &'static str,
    ) -> Result<(Vec<String>, ScopedReservation<'_>), CodecError> {
        let mut reservation = self.reserve_scoped(0, operation)?;
        let copies = reservation.with_storage(|| self.collect_retained_texts(values, operation))?;
        Ok((copies, reservation))
    }

    /// Formats text in an existing scope after charging its byte count as work.
    pub fn format_scoped_text(
        &self,
        reservation: &mut ScopedReservation<'_>,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        reservation.with_storage(|| self.format_retained(args, operation))
    }

    /// Admits records, retains their storage and additional text, and reserves slots.
    pub fn reserve_record_vec<T>(
        &self,
        records: &mut Vec<T>,
        count: usize,
        text_bytes: u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_entities(u64_from_index(count), operation)?;
        self.charge_retained(text_bytes, operation)?;
        self.reserve_vec(records, count, operation)
    }

    /// Collects retained slots before adding each value.
    pub fn collect_retained_vec<T>(
        &self,
        values: impl IntoIterator<Item = T>,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        self.try_collect_retained_with(values, operation, Ok::<T, CodecError>)
    }

    /// Reserves text bytes charged by aggregate admission.
    pub(crate) fn reserve_admitted_string(
        text: &mut String,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        text.try_reserve_exact(additional).map_err(|_| {
            CodecError::ResourceLimit(ResourceLimit::allocation_failed(
                ResourceDimension::RetainedBytes,
                u64::MAX,
                u64_from_index(additional),
                operation,
            ))
        })
    }

    /// Admits the comparison bound for one standard-library B-tree lookup.
    pub fn charge_btree_lookup(
        &self,
        len: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        // Nodes have at most eleven keys. A height-h tree has at least
        // 2 * 6^h - 1 keys: the root has two children and other internal
        // nodes have at least six. A lookup compares at most eleven keys
        // per level, and never more keys than the whole tree contains.
        let mut levels = 1_u64;
        let mut minimum_subtree = len.div_ceil(2);
        while minimum_subtree >= 6 {
            minimum_subtree /= 6;
            levels += 1;
        }
        self.charge_work(u64_from_index(len).min(11 * levels), operation)
    }

    /// Inserts a new scoped tree key after charging lookup work and node storage.
    pub fn insert_scoped_btree_set<T: Ord>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        values: &mut BTreeSet<T>,
        value: T,
        work_operation: &'static str,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        self.charge_btree_lookup(values.len(), work_operation)?;
        if values.contains(&value) {
            return Ok(false);
        }
        // Insertion searches the tree again after storage admission.
        self.charge_btree_lookup(values.len(), work_operation)?;
        reservation.with_storage(|| {
            self.charge_retained(
                self.tree_growth_bytes::<T, ()>(values.len(), operation)?,
                operation,
            )?;
            self.charge_collection_items(1, operation)?;
            Ok(values.insert(value))
        })
    }

    /// Inserts a vacant scoped tree entry after charging lookup work and node storage.
    pub fn insert_scoped_btree_map_if_vacant<K: Ord, V>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        values: &mut BTreeMap<K, V>,
        key: K,
        value: V,
        work_operation: &'static str,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        self.charge_work(u64_from_index(values.len()), work_operation)?;
        reservation.with_storage(|| {
            if values.contains_key(&key) {
                return Ok(false);
            }
            self.insert_btree_map(values, key, value, operation)?;
            Ok(true)
        })
    }

    /// Appends a lazily built value to a scoped group after aggregate admission.
    pub fn push_scoped_btree_group<K: Ord, V>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        groups: &mut BTreeMap<K, Vec<V>>,
        key: K,
        value: impl FnOnce() -> V,
        owned_bytes: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_work(u64_from_index(groups.len()) + 1, operation)?;
        reservation.with_storage(|| {
            if let Some(values) = groups.get_mut(&key) {
                self.reserve_vec(values, 1, operation)?;
                self.charge_retained(u64_from_index(owned_bytes), operation)?;
                values.push(value());
                return Ok(());
            }
            self.admit_btree_entry(groups, &key, operation)?;
            let mut values = self.collection_vec(1, operation)?;
            self.charge_retained(u64_from_index(owned_bytes), operation)?;
            values.push(value());
            groups.insert(key, values);
            Ok(())
        })
    }

    /// Admits tree-record storage, owned bytes, one slot and one work unit.
    ///
    /// Without the current tree length, each record admits one backing node.
    pub fn admit_retained_btree_record<K, V>(
        &self,
        owned_bytes: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let bytes = self
            .tree_growth_bytes::<K, V>(0, operation)?
            .checked_add(u64_from_index(owned_bytes))
            .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        self.charge_collection_items(1, operation)?;
        self.charge_retained(bytes, operation)?;
        self.charge_work(1, operation)
    }

    /// Collects scoped groups in input order within each key.
    pub fn collect_scoped_btree_groups<'ctx, K: Ord, V>(
        &'ctx self,
        values: impl IntoIterator<Item = (K, V)>,
        operation: &'static str,
    ) -> Result<(BTreeMap<K, Vec<V>>, ScopedReservation<'ctx>), CodecError> {
        let mut groups = BTreeMap::new();
        let mut reservation = self.reserve_scoped(0, operation)?;
        for (key, value) in values {
            self.push_scoped_btree_group(
                &mut reservation,
                &mut groups,
                key,
                || value,
                0,
                operation,
            )?;
        }
        Ok((groups, reservation))
    }

    /// Collects scoped entries, keeping the last value for each key.
    pub fn collect_scoped_btree_map<'ctx, K: Ord, V>(
        &'ctx self,
        values: impl IntoIterator<Item = (K, V)>,
        operation: &'static str,
    ) -> Result<(BTreeMap<K, V>, ScopedReservation<'ctx>), CodecError> {
        let mut entries = BTreeMap::new();
        let mut reservation = self.reserve_scoped(0, operation)?;
        reservation.with_storage(|| {
            for (key, value) in values {
                self.charge_work(u64_from_index(entries.len()) + 1, operation)?;
                self.insert_btree_map(&mut entries, key, value, operation)?;
            }
            Ok::<(), CodecError>(())
        })?;
        Ok((entries, reservation))
    }

    /// Grows a scoped string after admitting its additional byte storage.
    pub fn reserve_scoped_string(
        &self,
        reservation: &mut ScopedReservation<'_>,
        text: &mut String,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        reservation.with_storage(|| self.try_reserve_retained_text(text, additional, operation))
    }

    /// Creates empty scoped text with fallible storage admission.
    pub fn scoped_string(
        &self,
        length: usize,
        operation: &'static str,
    ) -> Result<(String, ScopedReservation<'_>), CodecError> {
        let mut reservation = self.reserve_scoped(0, operation)?;
        let mut text = String::new();
        self.reserve_scoped_string(&mut reservation, &mut text, length, operation)?;
        Ok((text, reservation))
    }

    fn allocation_failed(
        &self,
        dimension: ResourceDimension,
        count: usize,
        operation: &'static str,
    ) -> CodecError {
        match dimension {
            ResourceDimension::CollectionItems => {
                self.collection_allocation_failed(count, operation)
            }
            ResourceDimension::MaterializedBytes => self
                .budget
                .scoped_allocation_failed(u64_from_index(count), operation),
            _ => self
                .budget
                .retained_allocation_failed(u64_from_index(count), operation),
        }
    }

    /// Allocates a vector for `count` admitted items.
    pub fn collection_vec<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        let mut values = Vec::new();
        self.reserve_retained_vec_storage(
            &mut values,
            count,
            VecGrowth::Exact,
            Some(count),
            operation,
        )?;
        Ok(values)
    }

    /// Reserves additional vector items and charges their admission.
    pub fn reserve_vec<T>(
        &self,
        values: &mut Vec<T>,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_vec_limit(values, additional, operation)
            .map_err(Into::into)
    }

    /// Reserves storage with a resource-only refusal channel.
    pub fn reserve_vec_limit<T>(
        &self,
        values: &mut Vec<T>,
        additional: usize,
        operation: &'static str,
    ) -> Result<(), ResourceLimit> {
        self.reserve_retained_vec_storage(
            values,
            additional,
            VecGrowth::Amortized,
            Some(additional),
            operation,
        )
    }

    /// Builds a vector of indexed values after charging all slots.
    pub fn collect_indexed_vec<T>(
        &self,
        count: usize,
        operation: &'static str,
        mut value_at: impl FnMut(usize) -> Result<T, CodecError>,
    ) -> Result<Vec<T>, CodecError> {
        let mut values = self.collection_vec(count, operation)?;
        for index in 0..count {
            values.push(value_at(index)?);
        }
        Ok(values)
    }

    /// Adds one vector item after charging its slot.
    pub fn push_vec<T>(
        &self,
        values: &mut Vec<T>,
        value: T,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_vec(values, 1, operation)?;
        values.push(value);
        Ok(())
    }

    /// Adds a formatted retained note after charging its collection slot.
    pub fn push_formatted_retained(
        &self,
        values: &mut Vec<String>,
        args: fmt::Arguments<'_>,
        slots_operation: &'static str,
        text_operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_vec(values, 1, slots_operation)?;
        values.push(self.format_retained(args, text_operation)?);
        Ok(())
    }

    /// Moves all source items into a vector after charging their slots.
    pub fn append_vec<T>(
        &self,
        target: &mut Vec<T>,
        source: &mut Vec<T>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_vec(target, source.len(), operation)?;
        target.append(source);
        Ok(())
    }

    /// Moves owned items into a vector after charging their slots.
    pub fn extend_vec<T>(
        &self,
        target: &mut Vec<T>,
        mut source: Vec<T>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.append_vec(target, &mut source, operation)
    }

    /// Collects iterator values with a charged slot for each value.
    pub fn collect_vec<T>(
        &self,
        values: impl IntoIterator<Item = T>,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        let mut out = Vec::new();
        for value in values {
            self.push_vec(&mut out, value, operation)?;
        }
        Ok(out)
    }

    /// Collects fallible iterator values with a charged slot for each success.
    pub fn try_collect_vec<T, E: From<CodecError>>(
        &self,
        values: impl IntoIterator<Item = Result<T, E>>,
        operation: &'static str,
    ) -> Result<Vec<T>, E> {
        let mut out = Vec::new();
        for value in values {
            self.push_vec(&mut out, value?, operation)
                .map_err(E::from)?;
        }
        Ok(out)
    }

    /// Inserts a unique tree value after admitting its scoped value storage.
    pub fn insert_scoped_btree_value<T: Ord>(
        &self,
        reservation: &mut ScopedReservation<'_>,
        values: &mut BTreeSet<T>,
        value: T,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        reservation.with_storage(|| self.insert_btree_set(values, value, operation))
    }

    /// Inserts a new set item after charging its slot.
    pub fn insert_hash_set<T: Eq + Hash>(
        &self,
        values: &mut HashSet<T>,
        value: T,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        if values.contains(&value) {
            return Ok(false);
        }
        self.reserve_set(values, 1, operation)?;
        Ok(values.insert(value))
    }

    /// Inserts a copied string into a set if the value is new.
    pub fn insert_string_set(
        &self,
        values: &mut HashSet<String>,
        value: &str,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        if values.contains(value) {
            return Ok(false);
        }
        self.reserve_set(values, 1, operation)?;
        let owned = self.copy_retained_text(value, operation)?;
        Ok(values.insert(owned))
    }

    /// Extends a set with one charged slot for each distinct new value.
    pub fn extend_hash_set<T: Eq + Hash>(
        &self,
        values: &mut HashSet<T>,
        additions: impl IntoIterator<Item = T>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        for value in additions {
            self.insert_hash_set(values, value, operation)?;
        }
        Ok(())
    }

    /// Collects distinct values into a charged hash set.
    pub fn collect_hash_set<T: Eq + Hash>(
        &self,
        values: impl IntoIterator<Item = T>,
        operation: &'static str,
    ) -> Result<HashSet<T>, CodecError> {
        let mut out = HashSet::new();
        for value in values {
            self.insert_hash_set(&mut out, value, operation)?;
        }
        Ok(out)
    }

    /// Reserves a map slot only when the key is new.
    pub fn admit_hash_map_entry<K: Eq + Hash, V>(
        &self,
        values: &mut HashMap<K, V>,
        key: &K,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if !values.contains_key(key) {
            self.reserve_map(values, 1, operation)?;
        }
        Ok(())
    }

    /// Appends a grouped value after admitting both new index and value slots.
    pub fn push_hash_group<K: Eq + Hash, V>(
        &self,
        values: &mut HashMap<K, Vec<V>>,
        key: K,
        value: V,
        index_operation: &'static str,
        value_operation: &'static str,
    ) -> Result<(), CodecError> {
        if let Some(group) = values.get_mut(&key) {
            return self.push_vec(group, value, value_operation);
        }
        self.charge_collection_items(1, index_operation)?;
        self.charge_collection_items(1, value_operation)?;
        let mut group = self.vector_storage(1, value_operation)?;
        let bytes = self.charge_hash_growth::<(K, Vec<V>)>(
            values.len(),
            values.capacity(),
            1,
            index_operation,
        )?;
        values.try_reserve(1).map_err(|_| {
            self.budget
                .retained_allocation_failed(u64_from_index(bytes), index_operation)
        })?;
        group.push(value);
        values.insert(key, group);
        Ok(())
    }

    /// Inserts a map entry after admitting a new key, if needed.
    pub fn insert_hash_map<K: Eq + Hash, V>(
        &self,
        values: &mut HashMap<K, V>,
        key: K,
        value: V,
        operation: &'static str,
    ) -> Result<Option<V>, CodecError> {
        self.admit_hash_map_entry(values, &key, operation)?;
        Ok(values.insert(key, value))
    }

    /// Collects entries into a charged hash map.
    pub fn collect_hash_map<K: Eq + Hash, V>(
        &self,
        values: impl IntoIterator<Item = (K, V)>,
        operation: &'static str,
    ) -> Result<HashMap<K, V>, CodecError> {
        let mut out = HashMap::new();
        for (key, value) in values {
            self.insert_hash_map(&mut out, key, value, operation)?;
        }
        Ok(out)
    }

    /// Copies temporary text while its scoped reservation remains live.
    pub fn copy_scoped_text(
        &self,
        text: &str,
        reservation: &mut ScopedReservation<'_>,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        reservation.with_storage(|| self.copy_retained_text(text, operation))
    }

    fn linear_growth<T>(
        &self,
        len: usize,
        capacity: usize,
        count: usize,
        operation: &'static str,
    ) -> Result<(usize, usize), CodecError> {
        let required = len
            .checked_add(count)
            .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        if std::mem::size_of::<T>() == 0 || required <= capacity {
            return Ok((0, 0));
        }
        let minimum = match std::mem::size_of::<T>() {
            1 => 8,
            2..=1024 => 4,
            _ => 1,
        };
        let target = capacity
            .checked_mul(2)
            .ok_or_else(|| self.retained_size_overflow_limit(operation))?
            .max(required)
            .max(minimum);
        let bytes = (target - capacity)
            .checked_mul(std::mem::size_of::<T>())
            .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        self.charge_retained(u64_from_index(bytes), operation)?;
        Ok((target - len, bytes))
    }

    /// Appends a deque item after charging its slot.
    pub fn push_back<T>(
        &self,
        values: &mut VecDeque<T>,
        value: T,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let (additional, bytes) =
            self.linear_growth::<T>(values.len(), values.capacity(), 1, operation)?;
        self.charge_collection_items(1, operation)?;
        values.try_reserve_exact(additional).map_err(|_| {
            self.budget
                .retained_allocation_failed(u64_from_index(bytes), operation)
        })?;
        values.push_back(value);
        Ok(())
    }

    /// Prepends a deque item after charging its slot.
    pub fn push_front<T>(
        &self,
        values: &mut VecDeque<T>,
        value: T,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let (additional, bytes) =
            self.linear_growth::<T>(values.len(), values.capacity(), 1, operation)?;
        self.charge_collection_items(1, operation)?;
        values.try_reserve_exact(additional).map_err(|_| {
            self.budget
                .retained_allocation_failed(u64_from_index(bytes), operation)
        })?;
        values.push_front(value);
        Ok(())
    }

    /// Reserves slots in a binary heap after charging them.
    pub fn reserve_heap<T: Ord>(
        &self,
        values: &mut BinaryHeap<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let (additional, bytes) =
            self.linear_growth::<T>(values.len(), values.capacity(), count, operation)?;
        self.charge_collection_items(u64_from_index(count), operation)?;
        values.try_reserve_exact(additional).map_err(|_| {
            self.budget
                .retained_allocation_failed(u64_from_index(bytes), operation)
        })
    }

    /// Creates a charged vector only when the source is present.
    pub fn optional_collection_vec<T>(
        &self,
        present: bool,
        count: usize,
        operation: &'static str,
    ) -> Result<Option<Vec<T>>, CodecError> {
        if present {
            Ok(Some(self.collection_vec(count, operation)?))
        } else {
            Ok(None)
        }
    }

    /// Copies a flat lane into rows after admitting every row and element slot.
    pub fn copy_rows<T: Copy>(
        &self,
        values: &[T],
        row_len: usize,
        row_operation: &'static str,
        item_operation: &'static str,
    ) -> Result<Vec<Vec<T>>, CodecError> {
        if row_len == 0 {
            return Err(CodecError::malformed("row width must be nonzero"));
        }
        self.try_collect_retained_with(values.chunks(row_len), row_operation, |row| {
            self.copy_slice(row, item_operation)
        })
    }

    /// Collects fallible values and admits the storage of each output slot.
    pub fn try_collect_retained_with<I, T, E: From<CodecError>>(
        &self,
        values: impl IntoIterator<Item = I>,
        operation: &'static str,
        mut map: impl FnMut(I) -> Result<T, E>,
    ) -> Result<Vec<T>, E> {
        let values = values.into_iter();
        let (minimum, maximum) = values.size_hint();
        let mut collected = Vec::new();
        for (index, value) in values.enumerate() {
            self.charge_work(1, operation)?;
            if index == 0 && maximum == Some(minimum) {
                self.reserve_retained_vec_storage(
                    &mut collected,
                    minimum.max(1),
                    VecGrowth::Exact,
                    Some(1),
                    operation,
                )
                .map_err(CodecError::from)?;
            } else {
                self.reserve_retained_vec_storage(
                    &mut collected,
                    1,
                    VecGrowth::Amortized,
                    Some(1),
                    operation,
                )
                .map_err(CodecError::from)?;
            }
            collected.push(map(value)?);
        }
        Ok(collected)
    }

    /// Copies a slice after charging its collection slots.
    pub fn copy_slice<T: Copy>(
        &self,
        values: &[T],
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        self.charge_work(u64_from_index(values.len()), operation)?;
        let mut copy = self.collection_vec(values.len(), operation)?;
        copy.extend_from_slice(values);
        Ok(copy)
    }

    /// Collects optional values, stopping at the first absent value.
    pub fn collect_options<T>(
        &self,
        values: impl IntoIterator<Item = Option<T>>,
        operation: &'static str,
    ) -> Result<Option<Vec<T>>, CodecError> {
        let mut collected = Vec::new();
        for value in values {
            let Some(value) = value else { return Ok(None) };
            self.push_vec(&mut collected, value, operation)?;
        }
        Ok(Some(collected))
    }

    /// Collects fallible optional values, stopping at the first absent value.
    pub fn collect_fallible_options<T, E: Into<CodecError>>(
        &self,
        values: impl IntoIterator<Item = Result<Option<T>, E>>,
        operation: &'static str,
    ) -> Result<Option<Vec<T>>, CodecError> {
        let mut collected = Vec::new();
        for value in values {
            let Some(value) = value.map_err(Into::into)? else {
                return Ok(None);
            };
            self.push_vec(&mut collected, value, operation)?;
        }
        Ok(Some(collected))
    }

    /// Collects retained strings into a charged hash set.
    pub fn collect_string_set<'a>(
        &self,
        values: impl IntoIterator<Item = &'a str>,
        operation: &'static str,
    ) -> Result<HashSet<String>, CodecError> {
        let mut collected = HashSet::new();
        for value in values {
            self.insert_string_set(&mut collected, value, operation)?;
        }
        Ok(collected)
    }

    /// Copies retained rows and charges row and item slots.
    pub fn copy_retained_rows<T: Copy>(
        &self,
        rows: &[Vec<T>],
        row_operation: &'static str,
        item_operation: &'static str,
    ) -> Result<Vec<Vec<T>>, CodecError> {
        self.try_collect_retained_with(rows, row_operation, |row| {
            self.copy_slice(row, item_operation)
        })
    }

    /// Copies a retained set after charging storage and entries.
    pub fn copy_retained_set<T: Copy + Eq + Hash>(
        &self,
        values: &HashSet<T>,
        operation: &'static str,
    ) -> Result<HashSet<T>, CodecError> {
        self.charge_work(u64_from_index(values.len()), operation)?;
        let mut copy = HashSet::new();
        self.reserve_set(&mut copy, values.len(), operation)?;
        copy.extend(values.iter().copied());
        Ok(copy)
    }

    // SwissTable has a maximum 7/8 load, power-of-two bucket storage, and
    // at most 16 trailing control bytes. Small elements require 16 bytes
    // of bucket storage. Alignment padding is bounded by max(align, 16)-1.
    fn hash_storage_bytes<T>(
        &self,
        capacity: usize,
        operation: &'static str,
    ) -> Result<usize, ResourceLimit> {
        if capacity == 0 {
            return Ok(0);
        }
        let buckets = if capacity < 4 {
            Some(4)
        } else if capacity < 8 {
            Some(8)
        } else {
            capacity
                .checked_mul(8)
                .and_then(|slots| (slots / 7).checked_next_power_of_two())
        }
        .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        let alignment = std::mem::align_of::<T>().max(16);
        buckets
            .checked_mul(std::mem::size_of::<T>())
            .and_then(|bytes| bytes.checked_add(alignment - 1))
            .and_then(|bytes| bytes.checked_add(buckets))
            .and_then(|bytes| bytes.checked_add(16))
            .ok_or_else(|| self.retained_size_overflow_limit(operation))
    }

    fn charge_hash_growth<T>(
        &self,
        len: usize,
        capacity: usize,
        count: usize,
        operation: &'static str,
    ) -> Result<usize, ResourceLimit> {
        let required = len
            .checked_add(count)
            .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        if required <= capacity {
            return Ok(0);
        }
        let minimum = match std::mem::size_of::<T>() {
            0..=1 => 14,
            2..=3 => 7,
            _ => 3,
        };
        let bytes = self.hash_storage_bytes::<T>(required.max(minimum), operation)?
            - self.hash_storage_bytes::<T>(capacity, operation)?;
        self.charge_retained_limit(u64_from_index(bytes), operation)?;
        Ok(bytes)
    }

    /// Reserves hash set entries after charging their slots.
    pub fn reserve_set<T: Eq + Hash>(
        &self,
        values: &mut HashSet<T>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let bytes =
            self.charge_hash_growth::<T>(values.len(), values.capacity(), count, operation)?;
        self.charge_collection_items(u64_from_index(count), operation)?;
        values.try_reserve(count).map_err(|_| {
            self.budget
                .retained_allocation_failed(u64_from_index(bytes), operation)
        })
    }

    /// Reserves hash map entries after charging their slots.
    pub fn reserve_map<K: Eq + Hash, V>(
        &self,
        values: &mut HashMap<K, V>,
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let bytes =
            self.charge_hash_growth::<(K, V)>(values.len(), values.capacity(), count, operation)?;
        self.charge_collection_items(u64_from_index(count), operation)?;
        values.try_reserve(count).map_err(|_| {
            self.budget
                .retained_allocation_failed(u64_from_index(bytes), operation)
        })
    }

    // A B-tree node stores at most 11 key/value lanes, 12 child pointers,
    // and a parent pointer plus length/index metadata. The bound includes
    // four pointer-width metadata slots and padding for both lane arrays.
    // A tree built by insertion has at most (n - 1) / 5 + 1 nodes for n > 0,
    // and none when empty: every node except the root keeps at least five keys.
    // Admit the increase of that bound before each insertion. Since insertion
    // frees no nodes, cumulative admission bounds the live node storage.
    fn tree_growth_bytes<K, V>(
        &self,
        len: usize,
        operation: &'static str,
    ) -> Result<u64, ResourceLimit> {
        let alignment = std::mem::align_of::<K>()
            .max(std::mem::align_of::<V>())
            .max(std::mem::align_of::<usize>());
        let next = len
            .checked_add(1)
            .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
        let before = if len == 0 { 0 } else { (len - 1) / 5 + 1 };
        let nodes = (next - 1) / 5 + 1 - before;
        std::mem::size_of::<K>()
            .checked_add(std::mem::size_of::<V>())
            .and_then(|bytes| bytes.checked_mul(11))
            .and_then(|bytes| bytes.checked_add(16 * std::mem::size_of::<usize>()))
            .and_then(|bytes| bytes.checked_add(2 * alignment))
            .and_then(|bytes| bytes.checked_mul(nodes))
            .map(u64_from_index)
            .ok_or_else(|| self.retained_size_overflow_limit(operation))
    }

    /// Admits the backing nodes for one ordered entry whose slot is already charged.
    pub fn admit_btree_node_storage<K, V>(
        &self,
        len: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_retained(self.tree_growth_bytes::<K, V>(len, operation)?, operation)
    }

    /// Charges a new B-tree map key before insertion.
    pub fn admit_btree_entry<K: Ord, V>(
        &self,
        values: &BTreeMap<K, V>,
        key: &K,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if !values.contains_key(key) {
            self.admit_btree_node_storage::<K, V>(values.len(), operation)?;
            self.charge_collection_items(1, operation)?;
        }
        Ok(())
    }

    /// Inserts a B-tree map entry after charging a new key.
    pub fn insert_btree_map<K: Ord, V>(
        &self,
        values: &mut BTreeMap<K, V>,
        key: K,
        value: V,
        operation: &'static str,
    ) -> Result<Option<V>, CodecError> {
        self.admit_btree_entry(values, &key, operation)?;
        Ok(values.insert(key, value))
    }

    /// Inserts a B-tree set item after charging a new value.
    pub fn insert_btree_set<T: Ord>(
        &self,
        values: &mut BTreeSet<T>,
        value: T,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        if values.contains(&value) {
            return Ok(false);
        }
        self.charge_retained(
            self.tree_growth_bytes::<T, ()>(values.len(), operation)?,
            operation,
        )?;
        self.charge_collection_items(1, operation)?;
        Ok(values.insert(value))
    }

    /// Collects distinct ordered values after admitting each new entry.
    pub fn collect_btree_set<T: Ord>(
        &self,
        values: impl IntoIterator<Item = T>,
        operation: &'static str,
    ) -> Result<BTreeSet<T>, CodecError> {
        let mut out = BTreeSet::new();
        for value in values {
            self.insert_btree_set(&mut out, value, operation)?;
        }
        Ok(out)
    }

    /// Appends an ordered group member after admitting its group and slot.
    pub fn push_btree_group<K: Ord, V>(
        &self,
        groups: &mut BTreeMap<K, Vec<V>>,
        key: K,
        value: V,
        group_operation: &'static str,
        item_operation: &'static str,
    ) -> Result<(), CodecError> {
        if let Some(values) = groups.get_mut(&key) {
            return self.push_vec(values, value, item_operation);
        }
        self.admit_btree_entry(groups, &key, group_operation)?;
        let mut values = self.collection_vec(1, item_operation)?;
        values.push(value);
        groups.insert(key, values);
        Ok(())
    }

    /// Inserts an ordered group member after admitting its group and value.
    pub fn insert_btree_group_set<K: Ord, V: Ord>(
        &self,
        groups: &mut BTreeMap<K, BTreeSet<V>>,
        key: K,
        value: V,
        group_operation: &'static str,
        item_operation: &'static str,
    ) -> Result<(), CodecError> {
        if let Some(values) = groups.get_mut(&key) {
            self.insert_btree_set(values, value, item_operation)?;
            return Ok(());
        }
        self.admit_btree_entry(groups, &key, group_operation)?;
        let mut values = BTreeSet::new();
        self.insert_btree_set(&mut values, value, item_operation)?;
        groups.insert(key, values);
        Ok(())
    }

    /// Resizes a byte buffer after admitting additional retained storage.
    pub fn resize_retained_bytes(
        &self,
        values: &mut Vec<u8>,
        length: usize,
        fill: u8,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if let Some(additional) = length.checked_sub(values.len()) {
            self.reserve_capacity(values, additional, operation)?;
            for _ in 0..additional {
                values.push(fill);
            }
        } else {
            values.truncate(length);
        }
        Ok(())
    }

    /// Extends retained bytes after charging both their slots and storage.
    pub fn extend_retained_bytes(
        &self,
        target: &mut Vec<u8>,
        source: &[u8],
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.reserve_vec(target, source.len(), operation)?;
        target.extend_from_slice(source);
        Ok(())
    }

    /// Reserves a scoped vector and returns its live reservation.
    pub fn temporary_vec<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<(Vec<T>, ScopedReservation<'_>), CodecError> {
        let mut reservation = self.reserve_scoped(0, operation)?;
        let values = reservation.with_storage(|| self.collection_vec(count, operation))?;
        Ok((values, reservation))
    }

    /// Reserves a scoped hash set and returns its live reservation.
    pub fn temporary_set<T: Eq + Hash>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<(HashSet<T>, ScopedReservation<'_>), CodecError> {
        self.temporary_set_limit(count, operation)
            .map_err(Into::into)
    }

    /// Reserves a scoped hash set with a typed resource error.
    pub fn temporary_set_limit<T: Eq + Hash>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<(HashSet<T>, ScopedReservation<'_>), ResourceLimit> {
        let mut reservation = self.reserve_scoped_limit(0, operation)?;
        let mut values = HashSet::new();
        reservation.with_storage_limit(|| {
            let bytes = self.charge_hash_growth::<T>(0, 0, count, operation)?;
            self.charge_collection_items_limit(u64_from_index(count), operation)?;
            values.try_reserve(count).map_err(|_| {
                self.budget
                    .retained_allocation_failed_limit(u64_from_index(bytes), operation)
            })
        })?;
        Ok((values, reservation))
    }

    /// Collects a borrowed string index with scoped hash storage.
    pub fn collect_scoped_string_set<'text>(
        &self,
        count: usize,
        values: impl IntoIterator<Item = &'text str>,
        operation: &'static str,
    ) -> Result<(HashSet<&'text str>, ScopedReservation<'_>), CodecError> {
        let (mut output, mut reservation) = self.temporary_set(count, operation)?;
        let mut remaining = count;
        reservation.with_storage(|| {
            for value in values {
                self.charge_work(
                    u64_from_index(value.len())
                        .checked_mul(2)
                        .and_then(|work| work.checked_add(1))
                        .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?,
                    operation,
                )?;
                if !output.contains(value) {
                    if remaining == 0 {
                        self.reserve_set(&mut output, 1, operation)?;
                    } else {
                        remaining -= 1;
                    }
                    output.insert(value);
                }
            }
            Ok::<(), CodecError>(())
        })?;
        Ok((output, reservation))
    }

    /// Collects a borrowed string-keyed index with scoped hash storage.
    pub fn collect_scoped_string_map<'text, V>(
        &self,
        count: usize,
        values: impl IntoIterator<Item = (&'text str, V)>,
        operation: &'static str,
    ) -> Result<(HashMap<&'text str, V>, ScopedReservation<'_>), CodecError> {
        let mut reservation = self.reserve_scoped(0, operation)?;
        let mut output = HashMap::new();
        reservation.with_storage(|| {
            self.reserve_map(&mut output, count, operation)?;
            let mut remaining = count;
            for (key, value) in values {
                self.charge_work(
                    u64_from_index(key.len())
                        .checked_mul(2)
                        .and_then(|work| work.checked_add(1))
                        .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?,
                    operation,
                )?;
                if !output.contains_key(key) {
                    if remaining == 0 {
                        self.reserve_map(&mut output, 1, operation)?;
                    } else {
                        remaining -= 1;
                    }
                }
                output.insert(key, value);
            }
            Ok::<(), CodecError>(())
        })?;
        Ok((output, reservation))
    }

    /// Reserves a scoped deque and returns its live reservation.
    pub fn temporary_queue<T>(
        &self,
        count: usize,
        operation: &'static str,
    ) -> Result<(VecDeque<T>, ScopedReservation<'_>), CodecError> {
        let mut reservation = self.reserve_scoped(0, operation)?;
        let mut values = VecDeque::new();
        reservation.with_storage(|| {
            let bytes = count
                .checked_mul(std::mem::size_of::<T>())
                .ok_or_else(|| self.retained_size_overflow_limit(operation))?;
            self.charge_retained(u64_from_index(bytes), operation)?;
            self.charge_collection_items(u64_from_index(count), operation)?;
            values.try_reserve_exact(count).map_err(|_| {
                self.budget
                    .retained_allocation_failed(u64_from_index(bytes), operation)
            })
        })?;
        Ok((values, reservation))
    }

    /// Charges retained text produced by a later aggregate materialization.
    pub fn charge_formatted_retained(
        &self,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let length = self.formatted_length(args, operation)?;
        self.charge_retained(u64_from_index(length), operation)
    }

    /// Formats a retained string after charging its exact byte count.
    pub fn format_retained(
        &self,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        let mut text = String::new();
        self.append_formatted_retained(&mut text, args, operation)?;
        Ok(text)
    }

    /// Formats a temporary string and returns its live reservation.
    pub fn format_scoped(
        &self,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<(String, ScopedReservation<'_>), CodecError> {
        let mut reservation = self.reserve_scoped(0, operation)?;
        let text = self.format_scoped_text(&mut reservation, args, operation)?;
        Ok((text, reservation))
    }

    /// Appends retained text after charging its bytes.
    pub fn append_retained(
        &self,
        output: &mut String,
        suffix: &str,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_work(u64_from_index(suffix.len()), operation)?;
        self.try_reserve_retained_text(output, suffix.len(), operation)?;
        output.push_str(suffix);
        Ok(())
    }

    /// Appends formatted text after admitting its exact retained byte count.
    pub fn append_formatted_retained(
        &self,
        output: &mut String,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        struct Output<'ctx, 'arena, 'text> {
            ctx: &'ctx DecodeContext<'arena>,
            text: &'text mut String,
            operation: &'static str,
            refusal: Option<CodecError>,
        }
        impl Write for Output<'_, '_, '_> {
            fn write_str(&mut self, fragment: &str) -> fmt::Result {
                if let Err(error) = self
                    .ctx
                    .append_retained(self.text, fragment, self.operation)
                {
                    self.refusal = Some(error);
                    return Err(fmt::Error);
                }
                Ok(())
            }
        }
        let length = self.formatted_length(args, operation)?;
        self.try_reserve_retained_text(output, length, operation)?;
        let mut writer = Output {
            ctx: self,
            text: output,
            operation,
            refusal: None,
        };
        match fmt::write(&mut writer, args) {
            Ok(()) => Ok(()),
            Err(error) => Err(match writer.refusal {
                Some(refusal) => refusal,
                None => CodecError::malformed(error),
            }),
        }
    }

    /// Reserves an empty retained string after charging its declared length.
    pub fn retained_string(
        &self,
        length: usize,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        self.charge_retained(u64_from_index(length), operation)?;
        let mut value = String::new();
        value.try_reserve_exact(length).map_err(|_| {
            self.allocation_failed(ResourceDimension::RetainedBytes, length, operation)
        })?;
        Ok(value)
    }

    /// Copies retained text and appends a retained suffix.
    pub fn retained_suffix(
        &self,
        value: &str,
        suffix: &str,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        let mut output = self.copy_retained_text(value, operation)?;
        self.append_retained(&mut output, suffix, operation)?;
        Ok(output)
    }

    /// Copies a slice of retained strings after charging collection slots.
    pub fn copy_retained_strings(
        &self,
        values: &[String],
        operation: &'static str,
    ) -> Result<Vec<String>, CodecError> {
        let mut copies = self.collection_vec(values.len(), operation)?;
        for value in values {
            copies.push(self.copy_retained_text(value, operation)?);
        }
        Ok(copies)
    }

    /// Joins retained text after measuring and charging the exact byte count.
    pub fn join_retained<S: AsRef<str>>(
        &self,
        parts: &[S],
        separator: &str,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        let mut count = 0_usize;
        for part in parts {
            count = count
                .checked_add(part.as_ref().len())
                .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        }
        let gaps = if parts.is_empty() { 0 } else { parts.len() - 1 };
        count = separator
            .len()
            .checked_mul(gaps)
            .and_then(|separators| count.checked_add(separators))
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
        self.charge_retained(u64_from_index(count), operation)?;
        let mut output = String::new();
        output.try_reserve_exact(count).map_err(|_| {
            self.allocation_failed(ResourceDimension::RetainedBytes, count, operation)
        })?;
        for (index, part) in parts.iter().enumerate() {
            if index != 0 {
                output.push_str(separator);
            }
            output.push_str(part.as_ref());
        }
        Ok(output)
    }

    /// Joins displayed values while charging each retained text fragment.
    pub fn join_display_retained<T: fmt::Display>(
        &self,
        values: impl IntoIterator<Item = T>,
        separator: &str,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        struct ChargedText<'a, 'arena> {
            ctx: &'a DecodeContext<'arena>,
            operation: &'static str,
            text: String,
            refusal: Option<CodecError>,
        }
        impl Write for ChargedText<'_, '_> {
            fn write_str(&mut self, fragment: &str) -> fmt::Result {
                if let Err(error) =
                    self.ctx
                        .append_retained(&mut self.text, fragment, self.operation)
                {
                    self.refusal = Some(error);
                    return Err(fmt::Error);
                }
                Ok(())
            }
        }
        let mut output = ChargedText {
            ctx: self,
            operation,
            text: String::new(),
            refusal: None,
        };
        for (index, value) in values.into_iter().enumerate() {
            let written = if index == 0 {
                output.write_fmt(format_args!("{value}"))
            } else {
                output
                    .write_str(separator)
                    .and_then(|()| output.write_fmt(format_args!("{value}")))
            };
            if written.is_err() {
                return Err(match output.refusal {
                    Some(error) => error,
                    None => self.refuse_codec_limit(operation, 0, 1),
                });
            }
        }
        Ok(output.text)
    }

    fn formatted_length(
        &self,
        args: fmt::Arguments<'_>,
        operation: &'static str,
    ) -> Result<usize, CodecError> {
        struct Count<'ctx, 'arena> {
            ctx: &'ctx DecodeContext<'arena>,
            operation: &'static str,
            length: Option<usize>,
            refusal: Option<CodecError>,
        }
        impl Write for Count<'_, '_> {
            fn write_str(&mut self, value: &str) -> fmt::Result {
                if let Err(error) = self
                    .ctx
                    .charge_work(u64_from_index(value.len()), self.operation)
                {
                    self.refusal = Some(error);
                    return Err(fmt::Error);
                }
                self.length = self.length.and_then(|total| total.checked_add(value.len()));
                self.length.map(|_| ()).ok_or(fmt::Error)
            }
        }
        let mut count = Count {
            ctx: self,
            operation,
            length: Some(0),
            refusal: None,
        };
        if fmt::write(&mut count, args).is_err() {
            return Err(match count.refusal {
                Some(error) => error,
                None => self.refuse_codec_limit(operation, u64::MAX, u64::MAX),
            });
        }
        count
            .length
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod temporary_capacity_tests {
    #[test]
    fn temporary_reserve_charges_no_storage_with_available_capacity() {
        use super::DecodeContext;
        let arena = super::super::DecodeArena::new();
        let mut policy = super::super::DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
        let mut values = Vec::<u64>::with_capacity(8);
        let capacity = values.capacity();
        let storage = ctx
            .reserve_temporary_vec(&mut values, 3, "existing temporary capacity")
            .expect("admitted test operation");
        assert_eq!(values.capacity(), capacity);
        drop(storage);
    }
}

#[cfg(test)]
mod tree_work_tests {
    use std::cell::Cell;
    use std::cmp::Ordering;
    use std::collections::BTreeSet;

    use super::DecodeContext;
    use crate::decode::{DecodeArena, DecodePolicy};

    thread_local! {
        static COMPARISONS: Cell<u64> = const { Cell::new(0) };
    }

    #[derive(Debug)]
    struct CountedKey {
        value: usize,
    }

    impl PartialEq for CountedKey {
        fn eq(&self, other: &Self) -> bool {
            self.value == other.value
        }
    }

    impl Eq for CountedKey {}

    impl PartialOrd for CountedKey {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    impl Ord for CountedKey {
        fn cmp(&self, other: &Self) -> Ordering {
            COMPARISONS.with(|count| count.set(count.get() + 1));
            self.value.cmp(&other.value)
        }
    }

    #[test]
    fn scoped_tree_work_refuses_before_comparing_or_inserting() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let ctx = DecodeContext::new(&arena, &policy, false);
        COMPARISONS.with(|count| count.set(0));
        let mut values = BTreeSet::from([CountedKey { value: 0 }]);
        let mut storage = ctx.reserve_scoped(0, "tree test storage").expect("scope");
        let error = ctx
            .insert_scoped_btree_set(
                &mut storage,
                &mut values,
                CountedKey { value: 1 },
                "tree test comparisons",
                "tree test storage",
            )
            .expect_err("lookup exceeds work allowance");
        assert!(matches!(error, crate::CodecError::ResourceLimit(limit)
            if limit.operation == "tree test comparisons"));
        assert_eq!(COMPARISONS.with(Cell::get), 0);
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn scoped_tree_work_bounds_comparisons_without_billing_full_scans() {
        const COUNT: usize = 16_384;
        for order in 0..3 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            // The old insertion charges alone cost COUNT * (COUNT - 1) / 2.
            policy.limits.max_work_units = 3_000_000;
            let ctx = DecodeContext::new(&arena, &policy, false);
            COMPARISONS.with(|count| count.set(0));
            let mut values = BTreeSet::new();
            let mut storage = ctx.reserve_scoped(0, "tree test storage").expect("scope");
            for index in 0..COUNT {
                let value = match order {
                    0 => index,
                    1 => COUNT - index - 1,
                    _ => (index * 8191) % COUNT,
                };
                assert!(ctx
                    .insert_scoped_btree_set(
                        &mut storage,
                        &mut values,
                        CountedKey { value },
                        "tree test comparisons",
                        "tree test storage",
                    )
                    .expect("indexed insertion fits work allowance"));
            }
            for value in 0..=COUNT {
                ctx.charge_btree_lookup(values.len(), "tree test membership")
                    .expect("indexed membership fits work allowance");
                assert_eq!(values.contains(&CountedKey { value }), value < COUNT);
            }
            assert!(!ctx
                .insert_scoped_btree_set(
                    &mut storage,
                    &mut values,
                    CountedKey { value: COUNT / 2 },
                    "tree test comparisons",
                    "tree test storage",
                )
                .expect("duplicate fits work allowance"));
            assert_eq!(values.len(), COUNT);
            let refusal = ctx
                .charge_work_limit(u64::MAX, "read tree test work")
                .expect_err("measurement exceeds the work limit");
            assert!(refusal.used >= COMPARISONS.with(Cell::get));
            assert!(refusal.used < policy.limits.max_work_units);
        }
    }
}
