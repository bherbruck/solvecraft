// SPDX-License-Identifier: Apache-2.0
//! Shared byte-order assembly and byte-slice search over decode data.
//!
//! Empty needles are never a match. That matches the codec helpers this
//! module replaces and avoids `memchr`'s empty-needle-at-zero behavior.

/// Assemble a 16-bit little-endian integer from an exact byte array.
pub const fn assemble_u16_le(bytes: [u8; 2]) -> u16 {
    // endian-exception: reconstructed-scalar
    u16::from_le_bytes(bytes)
}

/// Assemble a 16-bit big-endian integer from an exact byte array.
pub const fn assemble_u16_be(bytes: [u8; 2]) -> u16 {
    // endian-exception: reconstructed-scalar
    u16::from_be_bytes(bytes)
}

/// Assemble a 24-bit little-endian integer from an exact byte array.
pub const fn assemble_u24_le(bytes: [u8; 3]) -> u32 {
    // endian-exception: reconstructed-scalar
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0])
}

/// Assemble a 24-bit big-endian integer from an exact byte array.
pub const fn assemble_u24_be(bytes: [u8; 3]) -> u32 {
    // endian-exception: reconstructed-scalar
    u32::from_be_bytes([0, bytes[0], bytes[1], bytes[2]])
}

/// Assemble a 32-bit little-endian integer from an exact byte array.
pub const fn assemble_u32_le(bytes: [u8; 4]) -> u32 {
    // endian-exception: reconstructed-scalar
    u32::from_le_bytes(bytes)
}

/// Assemble a 32-bit big-endian integer from an exact byte array.
pub const fn assemble_u32_be(bytes: [u8; 4]) -> u32 {
    // endian-exception: reconstructed-scalar
    u32::from_be_bytes(bytes)
}

/// Assemble a 64-bit little-endian integer from an exact byte array.
pub const fn assemble_u64_le(bytes: [u8; 8]) -> u64 {
    // endian-exception: reconstructed-scalar
    u64::from_le_bytes(bytes)
}

/// Assemble a 64-bit big-endian integer from an exact byte array.
pub const fn assemble_u64_be(bytes: [u8; 8]) -> u64 {
    // endian-exception: reconstructed-scalar
    u64::from_be_bytes(bytes)
}

/// Assemble an IEEE-754 binary32 value from exact little-endian bytes.
pub const fn assemble_f32_le(bytes: [u8; 4]) -> f32 {
    f32::from_bits(assemble_u32_le(bytes))
}

/// Assemble an IEEE-754 binary32 value from exact big-endian bytes.
pub const fn assemble_f32_be(bytes: [u8; 4]) -> f32 {
    f32::from_bits(assemble_u32_be(bytes))
}

/// Assemble an IEEE-754 binary64 value from exact little-endian bytes.
pub const fn assemble_f64_le(bytes: [u8; 8]) -> f64 {
    f64::from_bits(assemble_u64_le(bytes))
}

/// Assemble an IEEE-754 binary64 value from exact big-endian bytes.
pub const fn assemble_f64_be(bytes: [u8; 8]) -> f64 {
    f64::from_bits(assemble_u64_be(bytes))
}

/// First offset of `needle` in `haystack`, or `None` when `needle` is empty.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    memchr::memmem::find(haystack, needle)
}

/// First offset of `needle` at or after `from`, or `None` when `needle` is empty.
pub fn find_from(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    let tail = haystack.get(from..)?;
    find(tail, needle).map(|relative| from + relative)
}

/// First offset of `needle` in `haystack[start..end]`, returned as an absolute offset.
pub fn find_in(haystack: &[u8], needle: &[u8], start: usize, end: usize) -> Option<usize> {
    let window = haystack.get(start..end)?;
    find(window, needle).map(|relative| start + relative)
}

/// Whether `needle` occurs in `haystack`. Empty needles are absent.
pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    find(haystack, needle).is_some()
}

/// Offsets of every non-overlapping occurrence of `needle`.
///
/// An empty needle yields no offsets.
pub fn find_iter<'a>(haystack: &'a [u8], needle: &'a [u8]) -> impl Iterator<Item = usize> + 'a {
    let search = if needle.is_empty() {
        None
    } else {
        Some(memchr::memmem::find_iter(haystack, needle))
    };
    search.into_iter().flatten()
}

#[cfg(test)]
mod tests {
    use super::{contains, find, find_from, find_in, find_iter};

    #[test]
    fn assembles_wire_byte_orders_without_host_endian_assumptions() {
        assert_eq!(super::assemble_u16_le([0x02, 0x01]), 0x0102);
        assert_eq!(super::assemble_u24_be([0x01, 0x02, 0x03]), 0x0001_0203);
        assert_eq!(
            super::assemble_u32_be([0x01, 0x02, 0x03, 0x04]),
            0x0102_0304
        );
        assert_eq!(super::assemble_u64_le([1, 0, 0, 0, 0, 0, 0, 0]), 1);
        assert_eq!(super::assemble_f32_be([0x3f, 0xc0, 0, 0]), 1.5);
        assert_eq!(super::assemble_f64_le([0, 0, 0, 0, 0, 0, 0xf0, 0x3f]), 1.0);
    }

    #[test]
    fn empty_needle_is_never_a_match() {
        assert_eq!(find(b"abc", b""), None);
        assert_eq!(find_from(b"abc", b"", 1), None);
        assert_eq!(find_in(b"abc", b"", 0, 3), None);
        assert!(!contains(b"abc", b""));
        assert_eq!(
            find_iter(b"abc", b"").collect::<Vec<_>>(),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn finds_absolute_and_ranged_offsets() {
        let haystack = b"xxabcxxabc";
        assert_eq!(find(haystack, b"abc"), Some(2));
        assert_eq!(find_from(haystack, b"abc", 3), Some(7));
        assert_eq!(find_in(haystack, b"abc", 3, 10), Some(7));
        assert_eq!(find_in(haystack, b"abc", 3, 6), None);
        assert!(contains(haystack, b"abc"));
        assert_eq!(find_iter(haystack, b"abc").collect::<Vec<_>>(), [2, 7]);
    }
}
