// SPDX-License-Identifier: Apache-2.0
//! Nonempty groups with inline first members and checked additional storage.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

#[derive(Debug)]
pub(super) struct NonemptyGroup<T> {
    first: T,
    additional: Vec<T>,
}

impl<T> NonemptyGroup<T> {
    pub(super) fn new(first: T) -> Self {
        Self {
            first,
            additional: Vec::new(),
        }
    }

    pub(super) fn first(&self) -> &T {
        &self.first
    }

    pub(super) fn len(&self) -> usize {
        1 + self.additional.len()
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &T> {
        std::iter::once(&self.first).chain(self.additional.iter())
    }

    pub(super) fn push(
        &mut self,
        value: T,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        ctx.push_vec(&mut self.additional, value, operation)
    }
}

impl<'a, T> IntoIterator for &'a NonemptyGroup<T> {
    type Item = &'a T;
    type IntoIter = std::iter::Chain<std::iter::Once<&'a T>, std::slice::Iter<'a, T>>;

    fn into_iter(self) -> Self::IntoIter {
        std::iter::once(&self.first).chain(self.additional.iter())
    }
}
