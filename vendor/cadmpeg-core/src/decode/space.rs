// SPDX-License-Identifier: Apache-2.0
//! Session-local address-space identifiers and byte ranges.
//!
//! Every byte a decode reads belongs to exactly one address space: the root
//! input, an inflated entry, a reconstructed stream. A [`SpaceId`] names one
//! space within a single decode session; a [`View`](crate::decode::View)
//! carries the id so error locations fall out of the type. Coordinates are
//! absolute within a space: offset zero is that space's first byte.

/// Names one address space within a single decode.
///
/// Ids are dense and assigned in registration order; the root is always
/// [`SpaceId::ROOT`]. Error locations use the id to qualify offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpaceId(usize);

impl SpaceId {
    /// The root input space, registered first by every decode.
    pub const ROOT: SpaceId = SpaceId(0);

    /// Creates a session-local address-space identifier.
    pub(super) const fn from_index(index: usize) -> Self {
        Self(index)
    }

    /// Returns the dense index of this space.
    pub fn index(self) -> usize {
        self.0
    }
}

/// A half-open byte range `[start, end)` within one space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    /// Inclusive start offset.
    pub start: u64,
    /// Exclusive end offset.
    pub end: u64,
}
