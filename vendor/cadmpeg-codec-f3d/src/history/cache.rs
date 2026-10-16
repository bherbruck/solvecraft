// SPDX-License-Identifier: Apache-2.0
//! Reuse indexes over immutable inputs within one projection pass.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::HashMap;
use std::marker::PhantomData;

pub(super) struct SnapshotCache<'a, T, I> {
    entries: HashMap<*const T, I>,
    inputs: PhantomData<&'a T>,
}

impl<T, I> Default for SnapshotCache<'_, T, I> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            inputs: PhantomData,
        }
    }
}

impl<'a, T, I> SnapshotCache<'a, T, I> {
    pub(super) fn get(
        &mut self,
        ctx: &DecodeContext<'_>,
        input: &'a T,
        build: impl FnOnce(&DecodeContext<'_>, &'a T) -> Result<I, CodecError>,
    ) -> Result<&I, CodecError> {
        ctx.charge_work(1, "query F3D immutable projection index")?;
        let key = std::ptr::from_ref(input);
        if !self.entries.contains_key(&key) {
            let index = build(ctx, input)?;
            ctx.reserve_map(&mut self.entries, 1, "cache F3D immutable projection index")?;
            self.entries.insert(key, index);
        }
        Ok(&self.entries[&key])
    }
}
