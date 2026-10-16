// SPDX-License-Identifier: Apache-2.0
//! Bounded navigation over one address space.
#![warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)]

use super::error::SourceLocation;
use super::probe::{ParseError, ParseErrorKind};
use super::space::SpaceId;
use crate::bytes::{
    assemble_f32_be, assemble_f32_le, assemble_f64_be, assemble_f64_le, assemble_u16_be,
    assemble_u16_le, assemble_u24_be, assemble_u24_le, assemble_u32_be, assemble_u32_le,
    assemble_u64_be, assemble_u64_le,
};

/// Converts a declared element count into a safe `Vec` capacity.
///
/// Returns `None` unless `count * element_size <= remaining`, i.e. unless
/// the declared elements could actually be present in the unread input.
/// `element_size` must be the minimum encoded size of one element and must
/// be nonzero.
pub fn bounded_len(count: u64, element_size: usize, remaining: usize) -> Option<usize> {
    if element_size == 0 {
        return None;
    }
    let count = usize::try_from(count).ok()?;
    let bytes = count.checked_mul(element_size)?;
    (bytes <= remaining).then_some(count)
}

// The proof `index_from_u32` rests on. It justifies that one widening and
// nothing else.
const _: () = assert!(
    usize::BITS >= 32,
    "a target whose usize is narrower than 32 bits cannot hold a u32 index"
);

/// Widens a 32-bit count, identifier or index to a `usize` index.
///
/// A `const` assertion beside this function admits only targets whose `usize`
/// holds every `u32`, so the widening is exact, it is total, and it states no
/// refusal. A count that must also fit the unread input goes through
/// [`bounded_len`] instead.
pub const fn index_from_u32(value: u32) -> usize {
    #[cfg(target_pointer_width = "32")]
    {
        usize::from_ne_bytes(value.to_ne_bytes())
    }
    #[cfg(target_pointer_width = "64")]
    {
        let bytes = value.to_ne_bytes();
        #[cfg(target_endian = "little")]
        {
            usize::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3], 0, 0, 0, 0])
        }
        #[cfg(target_endian = "big")]
        {
            usize::from_ne_bytes([0, 0, 0, 0, bytes[0], bytes[1], bytes[2], bytes[3]])
        }
    }
}

// The proof `u64_from_index` rests on. It justifies that one widening and
// nothing else.
const _: () = assert!(
    usize::BITS <= 64,
    "a target whose usize is wider than 64 bits cannot hold every index in a u64"
);

/// Widens an in-memory length, index or capacity to a `u64`.
///
/// A `const` assertion beside this function admits only targets whose `u64`
/// holds every `usize`, so the widening is exact, it is total, and it states
/// no refusal. It is the counterpart of [`index_from_u32`]: it carries a
/// figure that already exists in memory into the wider type a work budget, a
/// byte offset or a limit report is stated in. It proves nothing about a
/// declared count read from input; such a count goes through [`bounded_len`].
pub const fn u64_from_index(value: usize) -> u64 {
    #[cfg(target_pointer_width = "64")]
    {
        u64::from_ne_bytes(value.to_ne_bytes())
    }
    #[cfg(target_pointer_width = "32")]
    {
        let bytes = value.to_ne_bytes();
        #[cfg(target_endian = "little")]
        {
            u64::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3], 0, 0, 0, 0])
        }
        #[cfg(target_endian = "big")]
        {
            u64::from_ne_bytes([0, 0, 0, 0, bytes[0], bytes[1], bytes[2], bytes[3]])
        }
    }
}

/// Narrows an offset stated in a wire `u64` to an index into bytes held in
/// memory.
///
/// `None` states that no index of this target names the offset, so the offset
/// names no byte of any payload the decoder holds. That is a refusal about the
/// offset, not about the payload's length: a reader that also needs the offset
/// to be inside a window tests the window itself.
///
/// It is the counterpart of [`u64_from_index`], which carries an index that
/// already exists in memory into the wider type a stored offset is stated in.
#[must_use]
pub fn index_from_u64(offset: u64) -> Option<usize> {
    usize::try_from(offset).ok()
}

/// Narrows the position of one item already held in memory to the `u32` an IR
/// identifier, order or index field is stated in.
///
/// This is the one rule for a population that outgrows an IR id width, and
/// every lane that meets that shape applies it. The population cannot be
/// widened: the IR field's width is the constraint, not the decoder's. A
/// position the field cannot state is therefore a refusal, and a refusal names
/// the instance and the lane it happened in — a `LossNote` at that lane, or
/// the lane's own refusal type. It is never a whole-file `Err`, because one
/// lane that outgrew its id width says nothing about the rest of the file, and
/// never a silent `continue`, because a dropped instance no one names is a
/// loss no report carries. `None` is that refusal; the caller states it.
///
/// A lane stated in an `i32` rather than a `u32` applies the same rule at that
/// width.
///
/// This is not [`bounded_len`]: the position already exists in memory, so
/// `None` states nothing about the input's own declared counts.
#[must_use]
pub fn id_from_index(index: usize) -> Option<u32> {
    u32::try_from(index).ok()
}

/// A count proven to fit in the unread input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundedCount(usize);

impl BoundedCount {
    /// Returns the proven element count.
    pub fn get(self) -> usize {
        self.0
    }
}

/// A bounded window over one address space.
///
/// The readable bytes are held as the slice itself, so [`View::window`] is a
/// field read with no range to recompute, and the bytes the cursor has not
/// passed are held as a suffix of that slice, so [`View::unread`],
/// [`View::remaining`] and [`View::take`] are slice operations with no offset
/// to recompute. `start` and `end` place the window in the space:
/// `end - start == window.len()`, and `unread` is a suffix of `window` for
/// every view. The two constructors are the only places those bounds are
/// proven — `over_space` takes a whole buffer, and `child` carves the
/// sub-slice with the same `get` that bounds the child.
///
/// `end` restates `start + window.len()`. This file denies
/// `clippy::arithmetic_side_effects` and `usize` states no total addition, so
/// every spelling of [`View::end`] that derives the bound is either that
/// denied `+` or a `checked_add` whose `None` no view can reach. The bound is
/// therefore stored, and `over_space` and `child` are the only places it is
/// proven.
#[derive(Debug, Clone, Copy)]
pub struct View<'a> {
    window: &'a [u8],
    unread: &'a [u8],
    space: SpaceId,
    start: usize,
    end: usize,
}

impl<'a> View<'a> {
    /// Creates a full-window view over an entire space buffer.
    pub(super) fn over_space(bytes: &'a [u8], space: SpaceId) -> View<'a> {
        View {
            window: bytes,
            unread: bytes,
            space,
            start: 0,
            end: bytes.len(),
        }
    }

    /// Bounded window over retained bytes that left the live space chain.
    ///
    /// Prefer views returned by [`DecodeContext`](super::DecodeContext). This
    /// constructor exists for opportunistic scanners over owned payloads that
    /// were copied out of the arena; locations report [`SpaceId::ROOT`].
    pub fn over_retained(bytes: &'a [u8]) -> View<'a> {
        Self::over_space(bytes, SpaceId::ROOT)
    }

    /// Returns the space this view navigates.
    pub(super) fn space(self) -> SpaceId {
        self.space
    }

    /// Returns the absolute position within the space.
    pub fn position(self) -> usize {
        // The unread bytes are the window suffix at or after the position, so
        // their count is the distance from the position to `end` and never
        // passes it. `abs_diff` is total at that distance.
        self.end.abs_diff(self.remaining())
    }

    /// Returns the window offset of the current position.
    ///
    /// Every view holds the unread bytes as a suffix of the window, so the
    /// bytes already read are the rest of it. `abs_diff` is total at that
    /// count: `unread` is never longer than `window`.
    pub fn read_len(self) -> usize {
        self.window.len().abs_diff(self.unread.len())
    }

    /// Returns the window's inclusive lower bound.
    pub fn start(self) -> usize {
        self.start
    }

    /// Returns the window's exclusive upper bound.
    pub fn end(self) -> usize {
        self.end
    }

    /// Returns the window offset of an absolute position in this space.
    ///
    /// `None` is the lower-bound refusal [`View::child`] and [`View::seek`]
    /// state: a position under this window's own start names no byte of the
    /// window.
    fn offset_of(self, position: usize) -> Option<usize> {
        position.checked_sub(self.start)
    }

    /// Returns the unread bytes of the window.
    ///
    /// The cursor is the split of the window, so this is a field read.
    pub fn unread(self) -> &'a [u8] {
        self.unread
    }

    /// Returns the number of unread bytes before the window's end.
    pub fn remaining(self) -> usize {
        self.unread.len()
    }

    /// Returns whether all bounded bytes have been read.
    pub fn is_empty(self) -> bool {
        self.unread.is_empty()
    }

    /// Returns this view's current source location.
    pub fn location(self) -> SourceLocation {
        SourceLocation {
            space: self.space,
            offset: u64_from_index(self.position()),
        }
    }

    /// Returns a source location at an absolute offset in this view's space.
    pub fn location_at(self, offset: usize) -> SourceLocation {
        SourceLocation {
            space: self.space,
            offset: u64_from_index(offset),
        }
    }

    /// Returns the readable window `start..end` as a slice.
    pub fn window(self) -> &'a [u8] {
        self.window
    }

    /// Takes `count` bytes, advancing only on success.
    ///
    /// The unread slice is the upper bound: `split_at_checked` refuses a count
    /// the unread bytes cannot serve, so no separate end test stands beside
    /// it, and the cursor moves only when the split holds.
    pub fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let (bytes, unread) = self.unread.split_at_checked(count)?;
        self.unread = unread;
        Some(bytes)
    }

    /// Takes a fixed-size byte array, advancing only on success.
    pub fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    /// Skips exactly `count` bytes.
    pub fn skip(&mut self, count: usize) -> Option<()> {
        self.take(count).map(|_| ())
    }

    /// Moves to an absolute offset, honoring the stored lower bound.
    ///
    /// The window states both bounds: `offset_of` refuses a position under the
    /// window's own start, and `get` refuses one past its end.
    pub fn seek(&mut self, position: usize) -> Option<()> {
        self.unread = self.window.get(self.offset_of(position)?..)?;
        Some(())
    }

    /// Moves the cursor to the window's exclusive upper bound.
    ///
    /// This is the one seek the window proves at construction. `seek(end)`
    /// reads `offset_of(end)`, which is the window length, and the suffix of a
    /// slice at its own length is the empty slice, so neither bound of `seek`
    /// can refuse it. The suffix is taken from `window` itself: `split_at` is
    /// total for every `mid <= len` by its own contract, and the `mid` here is
    /// that length, so the invariant that `unread` is a suffix of `window`
    /// holds by derivation and the move states no refusal to thread.
    /// `remaining` is then 0, `read_len` the whole window, and `position` the
    /// stored `end`.
    pub fn seek_to_end(&mut self) {
        self.unread = self.window.split_at(self.window.len()).1;
    }

    /// Reads a single byte.
    pub fn u8(&mut self) -> Option<u8> {
        self.array::<1>().map(|[value]| value)
    }

    /// Returns an exactly contained child window.
    ///
    /// The containment test is the sub-slice itself: `offset_of` refuses a
    /// lower bound under this window's own, and `get` refuses an inverted
    /// range or an upper bound past this window's end.
    pub fn child(self, start: usize, end: usize) -> Option<View<'a>> {
        let window = self
            .window
            .get(self.offset_of(start)?..self.offset_of(end)?)?;
        Some(View {
            window,
            unread: window,
            space: self.space,
            start,
            end,
        })
    }

    /// Takes `count` bytes as an exactly contained child window, advancing only
    /// on success.
    ///
    /// This is [`View::take`] for a reader that needs the taken bytes as a
    /// bounded window rather than a slice: the same `split_at_checked` states
    /// the one refusal, and the cursor moves only when the split holds. The
    /// child window is those bytes in this view's own space, so
    /// [`View::start`] of the result is the position the take began at,
    /// [`View::end`] is the position it reached, and [`View::location`] of the
    /// result is that beginning. No bound is recomputed against the parent and
    /// no sum is formed, so the operation is total wherever the take is.
    fn take_child(&mut self, count: usize) -> Option<View<'a>> {
        let start = self.position();
        let window = self.take(count)?;
        Some(View {
            window,
            unread: window,
            space: self.space,
            start,
            end: self.position(),
        })
    }

    /// Proves a declared element count could fit in the unread bytes.
    pub fn counted(self, count: u64, min_element_size: usize) -> Option<BoundedCount> {
        bounded_len(count, min_element_size, self.remaining()).map(BoundedCount)
    }

    /// Builds an unexpected-eof error from the view's current state.
    fn eof(self, needed: u64) -> ParseError {
        ParseError {
            location: self.location(),
            kind: ParseErrorKind::UnexpectedEof { needed },
            operation: "required_read",
        }
    }

    /// Required-read mirror of [`View::take`].
    pub fn req_take(&mut self, count: usize) -> Result<&'a [u8], ParseError> {
        match self.take(count) {
            Some(bytes) => Ok(bytes),
            None => Err(self.eof(u64_from_index(count))),
        }
    }

    /// Takes `count` bytes as a bounded child window, or states the truncation.
    ///
    /// The child window carries the same bounds and space as the optional take
    /// this mirrors: [`View::start`] of the result is the position the take
    /// began at and [`View::end`] is the position it reached. The view states
    /// the same located truncation as [`View::req_take`] for the same count,
    /// and leaves the view unmoved when it does.
    pub fn req_take_child(&mut self, count: usize) -> Result<View<'a>, ParseError> {
        match self.take_child(count) {
            Some(view) => Ok(view),
            None => Err(self.eof(u64_from_index(count))),
        }
    }

    /// Required-read mirror of [`View::u8`].
    pub fn req_u8(&mut self) -> Result<u8, ParseError> {
        match self.u8() {
            Some(value) => Ok(value),
            None => Err(self.eof(1)),
        }
    }
}

const fn decode_i16_le(bytes: [u8; 2]) -> i16 {
    // endian-exception: reconstructed-scalar
    i16::from_le_bytes(bytes)
}

const fn decode_i32_le(bytes: [u8; 4]) -> i32 {
    // endian-exception: reconstructed-scalar
    i32::from_le_bytes(bytes)
}

const fn decode_i64_le(bytes: [u8; 8]) -> i64 {
    // endian-exception: reconstructed-scalar
    i64::from_le_bytes(bytes)
}

const fn decode_i16_be(bytes: [u8; 2]) -> i16 {
    // endian-exception: reconstructed-scalar
    i16::from_be_bytes(bytes)
}

macro_rules! view_readers {
    ($(($probe:ident, [$($req:ident)?], $at:ident, $ty:ty, $decode:ident, $size:literal)),* $(,)?) => {
        impl View<'_> {
            $(
                #[doc = concat!("Probe read of a `", stringify!($ty), "` in the serialized byte order.")]
                pub fn $probe(&mut self) -> Option<$ty> {
                    self.array::<$size>().map($decode)
                }

                $(
                #[doc = concat!("Required-read mirror of [`View::", stringify!($probe), "`].")]
                pub fn $req(&mut self) -> Result<$ty, ParseError> {
                    match self.array::<$size>() {
                        Some(bytes) => Ok($decode(bytes)),
                        None => Err(self.eof($size)),
                    }
                }
                )?

                #[doc = concat!(
                    "Offset probe of a `",
                    stringify!($ty),
                    "` over retained bytes. Sequential readers should keep a live `View` and call [`View::",
                    stringify!($probe),
                    "`] instead."
                )]
                pub fn $at(bytes: &[u8], offset: usize) -> Option<$ty> {
                    let mut view = View::over_retained(bytes);
                    view.seek(offset)?;
                    view.$probe()
                }
            )*
        }
    };
}

view_readers!(
    (u16_le, [req_u16_le], u16_le_at, u16, assemble_u16_le, 2),
    (i16_le, [req_i16_le], i16_le_at, i16, decode_i16_le, 2),
    (u32_le, [req_u32_le], u32_le_at, u32, assemble_u32_le, 4),
    (i32_le, [req_i32_le], i32_le_at, i32, decode_i32_le, 4),
    (u64_le, [req_u64_le], u64_le_at, u64, assemble_u64_le, 8),
    (i64_le, [req_i64_le], i64_le_at, i64, decode_i64_le, 8),
    (f32_le, [req_f32_le], f32_le_at, f32, assemble_f32_le, 4),
    (f64_le, [req_f64_le], f64_le_at, f64, assemble_f64_le, 8),
    (u16_be, [], u16_be_at, u16, assemble_u16_be, 2),
    (i16_be, [], i16_be_at, i16, decode_i16_be, 2),
    (u32_be, [req_u32_be], u32_be_at, u32, assemble_u32_be, 4),
    (u64_be, [], u64_be_at, u64, assemble_u64_be, 8),
    (f32_be, [req_f32_be], f32_be_at, f32, assemble_f32_be, 4),
    (f64_be, [], f64_be_at, f64, assemble_f64_be, 8),
);

impl View<'_> {
    /// Reads a three-byte little-endian integer from retained bytes.
    pub fn u24_le_at(bytes: &[u8], offset: usize) -> Option<u32> {
        let end = offset.checked_add(3)?;
        let bytes = bytes.get(offset..end)?.try_into().ok()?;
        Some(assemble_u24_le(bytes))
    }

    /// Reads a three-byte big-endian integer from retained bytes.
    pub fn u24_be_at(bytes: &[u8], offset: usize) -> Option<u32> {
        let end = offset.checked_add(3)?;
        let bytes = bytes.get(offset..end)?.try_into().ok()?;
        Some(assemble_u24_be(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bounded_len, id_from_index, index_from_u32, index_from_u64, u64_from_index, BoundedCount,
        ParseError, ParseErrorKind, SourceLocation, View,
    };
    use crate::decode::space::SpaceId;

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn an_in_memory_position_past_the_id_width_states_no_identifier() {
        assert_eq!(id_from_index(0), Some(0));
        assert_eq!(id_from_index(index_from_u32(u32::MAX)), Some(u32::MAX));
        assert_eq!(id_from_index(index_from_u32(u32::MAX) + 1), None);
    }

    /// The widening and the narrowing agree on every index the target holds,
    /// and an offset past the index width states no index.
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn an_offset_past_the_index_width_states_no_index() {
        assert_eq!(index_from_u64(0), Some(0));
        assert_eq!(index_from_u64(u64_from_index(usize::MAX)), Some(usize::MAX));
        assert_eq!(
            index_from_u64(u64::from(u32::MAX)),
            Some(index_from_u32(u32::MAX))
        );
    }

    #[test]
    fn a_u32_index_widens_to_the_same_value() {
        assert_eq!(index_from_u32(0), 0);
        assert_eq!(
            index_from_u32(u32::MAX),
            usize::try_from(u32::MAX).expect("u32 fits usize")
        );
    }

    /// The widening is exact at the widest index the target can hold, so the
    /// conversion states no refusal and needs no sentinel.
    #[test]
    fn an_index_widens_to_the_same_value_and_back() {
        assert_eq!(u64_from_index(0), 0);
        assert_eq!(u64_from_index(1), 1);
        let widest = u64_from_index(usize::MAX);
        assert_eq!(widest, u64::try_from(usize::MAX).expect("usize fits u64"));
        assert_eq!(usize::try_from(widest), Ok(usize::MAX));
    }

    #[test]
    fn counted_reads_only_plausible_lengths() {
        let payload = [1u8, 0, 0, 0, 2, 0, 0, 0];
        let mut view = View::over_space(&payload, SpaceId::ROOT);
        assert_eq!(view.counted(2, 4).map(BoundedCount::get), Some(2));
        let mut values = [0; 2];
        for value in &mut values {
            *value = view.u32_le().expect("two fixture values");
        }
        assert_eq!(values, [1, 2]);
        let view = View::over_space(&payload, SpaceId::ROOT);
        assert_eq!(view.counted(u64::from(u32::MAX), 4), None);
    }

    #[test]
    fn counted_rejects_impossible_counts() {
        let payload = [0u8; 40];
        let view = View::over_space(&payload, SpaceId::ROOT);
        assert_eq!(view.counted(10, 4).map(BoundedCount::get), Some(10));
        assert_eq!(view.counted(11, 4), None);
        assert_eq!(bounded_len(u64::MAX, 20, usize::MAX), None);
    }

    /// The window is the slice the constructor proved, not a range recomputed
    /// against a wider buffer: `window()` reads the field a constructor stored.
    #[test]
    fn the_window_is_the_slice_the_constructor_proved() {
        let payload = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let view = View::over_space(&payload, SpaceId::ROOT);
        assert_eq!(view.window(), payload.as_slice());
        assert_eq!(view.start(), 0);
        assert_eq!(view.end(), payload.len());
        let mut child = view.child(2, 6).expect("contained child");
        assert_eq!(child.window(), payload.get(2..6).expect("fixture range"));
        assert_eq!(child.start(), 2);
        assert_eq!(child.end(), 6);
        assert_eq!(child.position(), 2);
        assert_eq!(child.remaining(), 4);
        assert_eq!(child.take(2), payload.get(2..4));
        assert_eq!(child.position(), 4);
        assert_eq!(child.remaining(), 2);
        assert_eq!(child.window(), payload.get(2..6).expect("fixture range"));
        assert_eq!(child.take(3), None);
        assert_eq!(child.position(), 4);
        let inner = view.child(2, 6).expect("contained child");
        assert!(inner.child(1, 6).is_none(), "lower bound under the window");
        assert!(inner.child(2, 7).is_none(), "upper bound past the window");
        assert!(inner.child(5, 3).is_none(), "inverted range");
        assert_eq!(inner.child(3, 5).map(View::window), payload.get(3..5));
    }

    /// The cursor is the split of the window: the unread bytes are the suffix
    /// the reader has not passed, and the bytes already read are the rest.
    #[test]
    fn the_unread_bytes_are_the_window_suffix_the_cursor_has_not_passed() {
        let payload = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut view = View::over_space(&payload, SpaceId::ROOT)
            .child(2, 7)
            .expect("contained child");
        assert_eq!(view.unread(), payload.get(2..7).expect("fixture range"));
        assert_eq!(view.read_len(), 0);
        assert_eq!(view.remaining(), 5);
        assert_eq!(view.position(), 2);
        assert_eq!(view.take(3), payload.get(2..5));
        assert_eq!(view.unread(), payload.get(5..7).expect("fixture range"));
        assert_eq!(view.read_len(), 3);
        assert_eq!(view.remaining(), 2);
        assert_eq!(view.position(), 5);
        assert_eq!(view.take(3), None, "a refused take states no bytes");
        assert_eq!(view.unread(), payload.get(5..7).expect("fixture range"));
        assert_eq!(
            view.position(),
            5,
            "a refused take does not move the cursor"
        );
        assert_eq!(view.seek(7), Some(()));
        assert!(view.unread().is_empty());
        assert!(view.is_empty());
        assert_eq!(view.read_len(), 5);
        assert_eq!(view.position(), 7);
        assert_eq!(view.seek(2), Some(()));
        assert_eq!(view.read_len(), 0);
        assert_eq!(view.remaining(), 5);
        assert_eq!(view.window(), payload.get(2..7).expect("fixture range"));
    }

    /// The window's placement and its cursor, as one comparable value: start,
    /// end, position and remaining.
    fn bounds(view: View<'_>) -> (usize, usize, usize, usize) {
        (view.start(), view.end(), view.position(), view.remaining())
    }

    /// The taken bytes are the child window, placed at the positions the take
    /// moved between, and a zero-length take states the empty window there.
    #[test]
    fn take_child_states_the_taken_range_as_a_window() {
        let payload = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut view = View::over_space(&payload, SpaceId::ROOT);
        assert_eq!(view.seek(2), Some(()));
        assert_eq!(view.take_child(3).map(View::window), payload.get(2..5));
        assert_eq!(view.position(), 5, "the take advanced by its own count");
        assert_eq!(view.remaining(), 3);
        assert_eq!(view.seek(2), Some(()));
        assert_eq!(
            view.take_child(3).map(bounds),
            Some((2, 5, 2, 3)),
            "the window is the taken range and its cursor is at the start"
        );
        assert_eq!(
            view.take_child(3).map(|window| window.location().offset),
            Some(u64_from_index(5)),
            "the result's location is the position the take began at"
        );
        assert_eq!(view.position(), 8);
        assert_eq!(view.take_child(0).map(bounds), Some((8, 8, 8, 0)));
        assert_eq!(view.take_child(0).map(View::window), payload.get(8..8));
        assert_eq!(view.position(), 8, "a zero-length take does not advance");
        assert_eq!(
            view.req_take_child(0).map(bounds),
            Ok((8, 8, 8, 0)),
            "the required mirror takes the empty window at the end"
        );
    }

    /// A taken window is an ordinary view of the same space: it carves children
    /// and takes further windows at the space's own positions.
    #[test]
    fn a_taken_window_carves_its_own_children() {
        let payload = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut view = View::over_space(&payload, SpaceId::ROOT);
        assert_eq!(view.skip(1), Some(()));
        let outer = view.take_child(6);
        assert_eq!(outer.map(bounds), Some((1, 7, 1, 6)));
        assert_eq!(
            outer.map(|window| window.child(3, 6).map(bounds)),
            Some(Some((3, 6, 3, 3))),
            "a child of a taken window states the space's own bounds"
        );
        assert_eq!(
            outer.map(|mut window| {
                window.skip(2)?;
                window.take_child(3).map(View::window)
            }),
            Some(payload.get(3..6)),
            "a taken window of a taken window is the same bytes"
        );
        assert_eq!(
            outer.map(|window| window.child(0, 6).map(bounds)),
            Some(None),
            "a lower bound under the taken window is refused"
        );
    }

    /// The count is bounded by the unread bytes, exactly as in `take`, and the
    /// required mirror states the located truncation `req_take` states.
    #[test]
    fn a_take_child_past_the_window_leaves_the_view_unmoved() {
        let payload = [0u8; 8];
        let mut view = View::over_space(&payload, SpaceId::ROOT);
        assert_eq!(view.seek(6), Some(()));
        assert_eq!(view.take_child(3).map(bounds), None);
        assert_eq!(
            view.position(),
            6,
            "a refused take does not move the cursor"
        );
        assert_eq!(view.remaining(), 2);
        assert_eq!(
            view.req_take_child(3).map(bounds),
            Err(ParseError {
                location: SourceLocation {
                    space: SpaceId::ROOT,
                    offset: 6,
                },
                kind: ParseErrorKind::UnexpectedEof { needed: 3 },
                operation: "required_read",
            })
        );
        assert_eq!(view.position(), 6);
        assert_eq!(
            view.req_take_child(3).map(bounds).err(),
            view.req_take(3).err(),
            "the same located truncation the required take states"
        );
        assert_eq!(view.take_child(2).map(bounds), Some((6, 8, 6, 2)));
    }

    #[test]
    fn seek_honors_lower_bound() {
        let payload = [0u8; 8];
        let mut view = View::over_space(&payload, SpaceId::ROOT)
            .child(2, 6)
            .expect("valid child");
        assert_eq!(view.seek(0), None);
        assert_eq!(view.seek(2), Some(()));
        assert_eq!(view.seek(6), Some(()));
        assert_eq!(view.seek(7), None);
    }

    #[test]
    fn seek_to_end_lands_on_the_window_bound_with_nothing_unread() {
        let payload = [0u8; 8];
        let mut view = View::over_space(&payload, SpaceId::ROOT)
            .child(2, 6)
            .expect("valid child");
        assert_eq!(view.u8(), Some(0));
        view.seek_to_end();
        assert_eq!(view.position(), view.end());
        assert_eq!(view.remaining(), 0);
        assert!(view.is_empty());
        assert_eq!(view.read_len(), view.window().len());
        assert!(view.unread().is_empty());
    }

    #[test]
    fn offset_helpers_match_live_reads() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0x0102_u16.to_le_bytes());
        bytes.extend_from_slice(&0x0304_u16.to_be_bytes());
        bytes.extend_from_slice(&(-5_i16).to_be_bytes());
        bytes.extend_from_slice(&1.5_f32.to_le_bytes());
        assert_eq!(View::u16_le_at(&bytes, 0), Some(0x0102));
        assert_eq!(View::u16_be_at(&bytes, 2), Some(0x0304));
        assert_eq!(View::i16_be_at(&bytes, 4), Some(-5));
        assert_eq!(View::f32_le_at(&bytes, 6), Some(1.5));
        assert_eq!(View::u16_le_at(&bytes, bytes.len()), None);
        assert_eq!(View::u24_le_at(&[0x02, 0x01, 0x00], 0), Some(0x0102));
        assert_eq!(View::u24_be_at(&[0x01, 0x02, 0x03], 0), Some(0x0001_0203));
        assert_eq!(View::u24_le_at(&[0, 1], 0), None);
        let mut view = View::over_space(&bytes, SpaceId::ROOT);
        assert_eq!(view.u16_le(), Some(0x0102));
        assert_eq!(view.u16_be(), Some(0x0304));
        assert_eq!(view.i16_be(), Some(-5));
        assert_eq!(view.f32_le(), Some(1.5));
    }
}
