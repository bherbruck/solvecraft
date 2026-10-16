// SPDX-License-Identifier: Apache-2.0
//! Length-prefixed string readers and GUID predicates shared by the record
//! decoders.
//!
//! Each reader takes a u32 length prefix and returns the decoded string with
//! the offset past its payload. The length bounds and charset policy differ
//! between record streams and are load-bearing: they decide which byte windows
//! parse as strings during heuristic scans. Callers pass their exact policy
//! through the `bounds` and `allowed` parameters rather than sharing one
//! unified policy.

use cadmpeg_core::decode::u64_from_index;

use cadmpeg_asm::kernel_header::RefWidth;
use std::convert::Infallible;
use std::ops::RangeInclusive;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

pub(crate) mod utf16;
use utf16::Utf16View;

/// Read a signed little-endian integer with a four- or eight-byte width.
pub(crate) fn int_at(bytes: &[u8], offset: usize, width: RefWidth) -> Option<i64> {
    match width {
        RefWidth::Four => Some(i64::from(View::i32_le_at(bytes, offset)?)),
        RefWidth::Eight => View::i64_le_at(bytes, offset),
    }
}

/// Read a u32-length-prefixed byte slice and return it with the end offset.
fn lp_u32_bytes_at(bytes: &[u8], offset: usize) -> Option<(&[u8], usize)> {
    let length = usize::try_from(View::u32_le_at(bytes, offset)?).ok()?;
    let start = offset.checked_add(4)?;
    let end = start.checked_add(length)?;
    Some((bytes.get(start..end)?, end))
}

/// Take a u32-length-prefixed byte slice, advancing only on success.
fn take_lp_u32_bytes<'a>(bytes: &'a [u8], position: &mut usize) -> Option<&'a [u8]> {
    let mut view = View::over_retained(bytes);
    view.seek(*position)?;
    let length = usize::try_from(view.u32_le()?).ok()?;
    let value = view.take(length)?;
    *position = view.position();
    Some(value)
}

/// Read a fixed little-endian `f64` lane into stack storage.
pub(crate) fn f64s_at<const N: usize>(bytes: &[u8], offset: usize) -> Option<[f64; N]> {
    let mut view = View::over_retained(bytes);
    view.seek(offset)?;
    let mut values = [0.0; N];
    for value in &mut values {
        *value = view.f64_le()?;
    }
    Some(values)
}

/// Read `N` consecutive little-endian `f64` values at `offset`, or `None`
/// when one is not finite.
pub(crate) fn finite_reals_at<const N: usize>(
    bytes: &[u8],
    offset: usize,
) -> Option<[FiniteReal; N]> {
    let mut admitted = [FiniteReal::ZERO; N];
    for (slot, value) in admitted.iter_mut().zip(f64s_at::<N>(bytes, offset)?) {
        *slot = FiniteReal::new(value)?;
    }
    Some(admitted)
}

/// Read a u32-length-prefixed ASCII string whose length lies in `bounds`,
/// decoding it as strict UTF-8. Returns the string and the offset past it.
pub(crate) fn lp_ascii_strict(
    bytes: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
) -> Option<(&str, usize)> {
    let length = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    if !bounds.contains(&length) {
        return None;
    }
    let (raw, end) = lp_u32_bytes_at(bytes, at)?;
    Some((std::str::from_utf8(raw).ok()?, end))
}

/// Read a bounded strict-UTF-8 string after admitting its retained bytes.
pub(crate) fn lp_ascii_strict_charged(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
) -> Result<Option<(String, usize)>, CodecError> {
    let Some(length_u32) = View::u32_le_at(bytes, at) else {
        return Ok(None);
    };
    let Ok(length) = usize::try_from(length_u32) else {
        return Ok(None);
    };
    if !bounds.contains(&length) {
        return Ok(None);
    }
    let Some((raw, end)) = lp_u32_bytes_at(bytes, at) else {
        return Ok(None);
    };
    ctx.charge_work(u64::from(length_u32), "decode F3D ASCII string")?;
    let Ok(value) = std::str::from_utf8(raw) else {
        return Ok(None);
    };

    let mut owned = ctx.retained_string(length, "retain F3D ASCII string")?;
    owned.push_str(value);
    Ok(Some((owned, end)))
}

/// Read an ASCII-only length-prefixed field without copying its contents.
pub(crate) fn lp_ascii_filtered_view(
    bytes: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
    allowed: fn(&u8) -> bool,
) -> Option<(&str, usize)> {
    let length = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    if !bounds.contains(&length) {
        return None;
    }
    let start = at.checked_add(4)?;
    let end = start.checked_add(length)?;
    let raw = bytes.get(start..end)?;
    if !raw.iter().all(allowed) {
        return None;
    }
    Some((std::str::from_utf8(raw).ok()?, end))
}

/// Read the byte span of a bounded counted UTF-16 field.
fn lp_utf16_raw(bytes: &[u8], at: usize, bounds: RangeInclusive<usize>) -> Option<(&[u8], usize)> {
    let count = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    if !bounds.contains(&count) {
        return None;
    }
    let start = at.checked_add(4)?;
    let end = start.checked_add(count.checked_mul(2)?)?;
    Some((bytes.get(start..end)?, end))
}

/// Read validated UTF-16 text without allocating a decoded copy.
pub(crate) fn lp_utf16_bounded_view(
    bytes: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
) -> Option<(Utf16View<'_>, usize)> {
    let (raw, end) = lp_utf16_raw(bytes, at, bounds)?;
    Some((Utf16View::new(raw)?, end))
}

/// Validate a counted UTF-16 field after admitting its scan.
fn lp_utf16_bounded_view_charged<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    at: usize,
    bounds: RangeInclusive<usize>,
) -> Result<Option<(Utf16View<'a>, usize)>, CodecError> {
    let Some((raw, end)) = lp_utf16_raw(bytes, at, bounds) else {
        return Ok(None);
    };
    ctx.charge_work(u64_from_index(raw.len()), "decode F3D UTF-16 string")?;
    let Some(text) = Utf16View::new(raw) else {
        return Ok(None);
    };
    Ok(Some((text, end)))
}

/// Decode a counted UTF-16 field into admitted retained storage.
pub(crate) fn lp_utf16_bounded_charged(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
    operation: &'static str,
) -> Result<Option<(String, usize)>, CodecError> {
    let Some((text, end)) = lp_utf16_bounded_view_charged(ctx, bytes, at, bounds)? else {
        return Ok(None);
    };
    Ok(Some((text.to_retained(ctx, operation)?, end)))
}

/// Decode a counted UTF-16 field into storage held by a scoped reservation.
pub(crate) fn lp_utf16_bounded_scoped<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &[u8],
    at: usize,
    bounds: RangeInclusive<usize>,
    operation: &'static str,
) -> Result<Option<(String, usize, ScopedReservation<'ctx>)>, CodecError> {
    let Some((text, end)) = lp_utf16_bounded_view_charged(ctx, bytes, at, bounds)? else {
        return Ok(None);
    };
    let (text, reservation) = text.to_scoped(ctx, operation)?;
    Ok(Some((text, end, reservation)))
}

#[cfg(test)]
mod charged_string_tests {
    use super::{lp_ascii_strict_charged, lp_utf16_bounded_charged, lp_utf16_bounded_scoped};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    #[test]
    fn bounded_utf16_string_refuses_retained_limit() {
        let bytes = [2, 0, 0, 0, b'A', 0, b'B', 0];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let error = lp_utf16_bounded_charged(&ctx, &bytes, 0, 0..=1024, "retain F3D UTF-16 string")
            .unwrap_err();
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "retain F3D UTF-16 string"
        ));
    }

    #[test]
    fn bounded_ascii_string_refuses_retained_limit() {
        let bytes = [2, 0, 0, 0, b'A', b'B'];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let error = lp_ascii_strict_charged(&ctx, &bytes, 0, 0..=128).unwrap_err();
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "retain F3D ASCII string"
        ));
    }

    #[test]
    fn bounded_utf16_scoped_preserves_work_and_storage_refusals() {
        let bytes = [2, 0, 0, 0, b'A', 0, b'B', 0];
        for (work, materialized, operation) in [
            (0, 1024, "decode F3D UTF-16 string"),
            (4, 1024, "f3d Design temporary UTF-16 text"),
            (1024, 0, "f3d Design temporary UTF-16 text"),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = work;
            policy.limits.max_materialized_bytes = materialized;
            crate::test_support::with_decode_policy(&policy, |ctx| {
                let error = lp_utf16_bounded_scoped(
                    ctx,
                    &bytes,
                    0,
                    0..=1024,
                    "f3d Design temporary UTF-16 text",
                )
                .unwrap_err();
                let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                    panic!("UTF-16 admission must refuse");
                };
                assert_eq!(limit.operation, operation);
                assert_eq!(ctx.resource_refusal(), Some(limit));
            });
        }
        crate::test_support::with_decode_context(|ctx| {
            let (text, end, reservation) = lp_utf16_bounded_scoped(
                ctx,
                &bytes,
                0,
                0..=1024,
                "f3d Design temporary UTF-16 text",
            )
            .unwrap()
            .unwrap();
            assert_eq!(text, "AB");
            assert_eq!(end, bytes.len());
            drop(reservation);
        });
    }
}

/// Take a u32-length-prefixed strict-UTF-8 string, advancing `at` past it on
/// success.
pub(crate) fn take_lp_utf8(bytes: &[u8], at: &mut usize) -> Option<String> {
    String::from_utf8(take_lp_u32_bytes(bytes, at)?.to_vec()).ok()
}

pub(crate) fn take_lp_utf8_charged(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: &mut usize,
) -> Result<Option<String>, CodecError> {
    let Some(raw) = take_lp_u32_bytes(bytes, at) else {
        return Ok(None);
    };
    let Ok(value) = std::str::from_utf8(raw) else {
        return Ok(None);
    };

    let mut owned = ctx.retained_string(raw.len(), "retain F3D UTF-8 string")?;
    owned.push_str(value);
    Ok(Some(owned))
}

/// Advance `at` past a u32-length-prefixed byte string, reading none of it.
///
/// `None` is the refusal a declared length the record cannot carry states.
/// The caller that reads no content states its skip through this, so a
/// truncation moves `at` nowhere and is a refusal rather than a short read
/// every following offset inherits.
pub(crate) fn skip_lp_u32_bytes(bytes: &[u8], at: &mut usize) -> Option<()> {
    let mut view = View::over_retained(bytes);
    view.seek(*at)?;
    let length = usize::try_from(view.u32_le()?).ok()?;
    view.skip(length)?;
    *at = view.position();
    Some(())
}

/// One reference member of a Fusion segment record
/// ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)
/// "**References.**").
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reference<G, L> {
    /// The null reference.
    Null,
    /// A target in the current segment.
    Local {
        target: u64,
        inline_type_guid: Option<G>,
    },
    /// A target in another segment.
    CrossSegment {
        target: u64,
        inline_type_guid: Option<G>,
        segment: u32,
    },
    /// A named target in another document.
    CrossDocument {
        target: u64,
        inline_type_guid: Option<G>,
        segment: u32,
        link_name: L,
    },
}

impl<G: AsRef<str>, L> Reference<G, L> {
    /// The target and inline type of a local reference.
    pub(crate) fn local(&self) -> Option<(u64, Option<&str>)> {
        match self {
            Self::Local {
                target,
                inline_type_guid,
            } => Some((*target, inline_type_guid.as_ref().map(AsRef::as_ref))),
            _ => None,
        }
    }

    /// Consume a local reference and return its target and inline type.
    pub(crate) fn into_local(self) -> Option<(u64, Option<G>)> {
        match self {
            Self::Local {
                target,
                inline_type_guid,
            } => Some((target, inline_type_guid)),
            _ => None,
        }
    }

    /// The target of a non-null reference.
    pub(crate) fn target(&self) -> Option<u64> {
        match self {
            Self::Null => None,
            Self::Local { target, .. }
            | Self::CrossSegment { target, .. }
            | Self::CrossDocument { target, .. } => Some(*target),
        }
    }
}

impl Reference<String, String> {
    /// The link name of a cross-document reference.
    pub(crate) fn link_name(&self) -> Option<&str> {
        match self {
            Self::CrossDocument { link_name, .. } => Some(link_name),
            _ => None,
        }
    }
}

/// Take one reference, advancing `at` past every byte it owns.
///
/// The width is not fixed: a null reference is one byte, a same-segment
/// reference eleven, an inline-typed same-segment reference 51, a cross-segment
/// reference fifteen, and a cross-document reference carries an asset GUID, a
/// type GUID, a link name, and an optional version tail. Any arithmetic that
/// assumes one width desynchronizes on the first nonstandard reference.
pub(crate) fn take_reference<'a>(
    bytes: &'a [u8],
    at: &mut usize,
) -> Option<Reference<&'a str, Utf16View<'a>>> {
    match parse_reference(bytes, at, |_, _| Ok::<(), Infallible>(())) {
        Ok(reference) => reference,
        Err(error) => match error {},
    }
}

/// Parse one reference under caller work admission and retain only its kept text.
pub(crate) fn take_reference_charged(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    at: &mut usize,
) -> Result<Option<Reference<String, String>>, CodecError> {
    let Some(reference) = parse_reference(bytes, at, |length, operation| {
        ctx.charge_work(u64_from_index(length), operation)
    })?
    else {
        return Ok(None);
    };
    let copy_guid = |guid: Option<&str>| -> Result<Option<String>, CodecError> {
        guid.map(|guid| {
            ctx.charge_work(u64_from_index(guid.len()), "retain F3D reference type GUID")?;
            ctx.copy_retained_text(guid, "retain F3D ASCII string")
        })
        .transpose()
    };
    Ok(Some(match reference {
        Reference::Null => Reference::Null,
        Reference::Local {
            target,
            inline_type_guid,
        } => Reference::Local {
            target,
            inline_type_guid: copy_guid(inline_type_guid)?,
        },
        Reference::CrossSegment {
            target,
            inline_type_guid,
            segment,
        } => Reference::CrossSegment {
            target,
            inline_type_guid: copy_guid(inline_type_guid)?,
            segment,
        },
        Reference::CrossDocument {
            target,
            inline_type_guid,
            segment,
            link_name,
        } => Reference::CrossDocument {
            target,
            inline_type_guid: copy_guid(inline_type_guid)?,
            segment,
            link_name: link_name.to_retained(ctx, "retain F3D UTF-16 string")?,
        },
    }))
}

fn parse_reference<'a, E>(
    bytes: &'a [u8],
    at: &mut usize,
    admit: impl Fn(usize, &'static str) -> Result<(), E>,
) -> Result<Option<Reference<&'a str, Utf16View<'a>>>, E> {
    macro_rules! some {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    let utf16 = |at, bounds| -> Result<Option<(Utf16View<'a>, usize)>, E> {
        let Some((raw, end)) = lp_utf16_raw(bytes, at, bounds) else {
            return Ok(None);
        };
        admit(raw.len(), "decode F3D reference UTF-16")?;
        Ok(Utf16View::new(raw).map(|text| (text, end)))
    };
    let ascii = |at, bounds: RangeInclusive<usize>| -> Result<Option<(&'a str, usize)>, E> {
        let Some((raw, end)) = lp_u32_bytes_at(bytes, at) else {
            return Ok(None);
        };
        if !RangeInclusive::contains(&bounds, &raw.len()) {
            return Ok(None);
        }
        admit(raw.len(), "decode F3D reference ASCII")?;
        Ok(std::str::from_utf8(raw).ok().map(|text| (text, end)))
    };
    let mut cursor = *at;
    let present = some!(bytes.get(cursor)).to_owned();
    cursor += 1;
    if present == 0 {
        *at = cursor;
        return Ok(Some(Reference::Null));
    }
    if present != 1 {
        return Ok(None);
    }
    let target = some!(View::u64_le_at(bytes, cursor));
    cursor += 8;
    let inline_type_guid = if View::u32_le_at(bytes, cursor) == Some(36) {
        let (guid, end) = some!(ascii(cursor, 36..=36)?);
        if !is_guid_hyphenated(guid) {
            return Ok(None);
        }
        cursor = end;
        Some(guid)
    } else {
        None
    };
    let reference = match *some!(bytes.get(cursor)) {
        0 => {
            cursor += 1;
            match *some!(bytes.get(cursor)) {
                0 => {
                    cursor += 1;
                    Reference::Local {
                        target,
                        inline_type_guid,
                    }
                }
                1 => {
                    let segment = some!(View::u32_le_at(bytes, cursor + 1));
                    cursor += 5;
                    Reference::CrossSegment {
                        target,
                        inline_type_guid,
                        segment,
                    }
                }
                _ => return Ok(None),
            }
        }
        1 => {
            cursor += 1;
            let segment = some!(View::u32_le_at(bytes, cursor));
            cursor += 4;
            let (_, end) = some!(utf16(cursor, 0..=64)?);
            cursor = end;
            match *some!(bytes.get(cursor)) {
                1 => {
                    cursor += 1;
                    Reference::CrossSegment {
                        target,
                        inline_type_guid,
                        segment,
                    }
                }
                0 => {
                    cursor += 1;
                    let (guid, end) = some!(ascii(cursor, 36..=36)?);
                    if !is_guid_hyphenated(guid) {
                        return Ok(None);
                    }
                    let (link_name, end) = some!(utf16(end, 0..=256)?);
                    cursor = end;
                    match *some!(bytes.get(cursor)) {
                        0 => cursor += 1,
                        1 => {
                            let (_, end) = some!(utf16(cursor + 1, 36..=36)?);
                            let (_, end) = some!(utf16(end, 0..=256)?);
                            cursor = end;
                        }
                        _ => return Ok(None),
                    }
                    Reference::CrossDocument {
                        target,
                        inline_type_guid,
                        segment,
                        link_name,
                    }
                }
                _ => return Ok(None),
            }
        }
        _ => return Ok(None),
    };
    *at = cursor;
    Ok(Some(reference))
}

/// Whether `value` is a 36-character hyphenated hexadecimal GUID.
pub(crate) fn is_guid_hyphenated(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

/// Whether `value` is 36 to 38 characters of alphanumerics, `-`, and `_`.
pub(crate) fn is_guid_relaxed(value: &str) -> bool {
    matches!(value.len(), 36..=38)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// Whether the first 36 bytes of `value` form a hyphenated hexadecimal GUID.
pub(crate) fn is_guid_prefix(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 36
        && bytes[..36].iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

/// Encode `value` as a u32-code-unit-prefixed UTF-16LE byte sequence, the form
/// used both as a search needle and by test fixtures.
pub(crate) fn lp_utf16_bytes(value: &str) -> Result<Vec<u8>, CodecError> {
    let units: Vec<u8> = value.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let count = u32::try_from(units.len() / 2).map_err(|_| {
        cadmpeg_core::decode::refuse_local_limit(
            "f3d UTF-16 code-unit count",
            u64::from(u32::MAX),
            u64_from_index(units.len() / 2),
        )
    })?;
    let mut out = count.to_le_bytes().to_vec();
    out.extend(units);
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn fixed_real_lanes_need_no_decode_storage() {
        let values = [1.0_f64, -2.0, 3.0];
        let bytes: Vec<_> = values.into_iter().flat_map(f64::to_le_bytes).collect();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            assert_eq!(super::f64s_at::<3>(&bytes, 0), Some(values));
            assert_eq!(super::f64s_at::<3>(&bytes[..23], 0), None);
            assert_eq!(super::f64s_at::<3>(&bytes, usize::MAX), None);
            assert_eq!(
                super::finite_reals_at::<3>(&bytes, 0)
                    .unwrap()
                    .map(cadmpeg_ir::scalar::FiniteReal::get),
                values
            );
            assert_eq!(ctx.resource_refusal(), None);
        });
        let bytes = f64::NAN.to_le_bytes();
        assert_eq!(super::finite_reals_at::<1>(&bytes, 0), None);
    }
    #[test]
    fn lp_utf8_string_refuses_retained_limit() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(b"text");
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 3;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test decode context");
        let error = super::take_lp_utf8_charged(&ctx, &bytes, &mut 0)
            .expect_err("encoded string must exceed retained budget");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain F3D UTF-8 string")
        );
    }
    #[test]
    fn borrowed_text_readers_preserve_boundaries_and_unicode() {
        let ascii = [3, 0, 0, 0, b'1', b'2', b'3'];
        for (text, end) in [
            super::lp_ascii_strict(&ascii, 0, 3..=3).unwrap(),
            super::lp_ascii_filtered_view(&ascii, 0, 3..=3, u8::is_ascii_digit).unwrap(),
        ] {
            assert_eq!(text, "123");
            assert_eq!(end, ascii.len());
            assert_eq!(text.as_ptr(), ascii[4..].as_ptr());
        }
        assert!(super::lp_ascii_strict(&ascii[..6], 0, 3..=3).is_none());
        assert!(
            super::lp_ascii_filtered_view(&ascii, usize::MAX, 3..=3, u8::is_ascii_digit).is_none()
        );
        let mut bytes = Vec::new();
        crate::test_support::lp_utf16(&mut bytes, " A😀 ");
        let (text, end) = super::lp_utf16_bounded_view(&bytes, 0, 0..=20).unwrap();
        assert!(text.eq_str(" A😀 "));
        assert_eq!(text.len(), " A😀 ".len());
        assert_eq!(end, bytes.len());
        assert!(super::lp_utf16_bounded_view(&bytes[..bytes.len() - 1], 0, 0..=20).is_none());
        assert!(super::lp_utf16_bounded_view(&bytes, usize::MAX, 0..=20).is_none());
        assert!(super::lp_utf16_bounded_view(&[1, 0, 0, 0, 0, 216], 0, 0..=20).is_none());
    }

    #[test]
    fn charged_reference_retains_only_kept_text() {
        let mut bytes = crate::test_support::cross_document_reference(7, "link");
        *bytes.last_mut().unwrap() = 1;
        crate::test_support::lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
        crate::test_support::lp_utf16(&mut bytes, "urn:version:discarded");
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = "link".len().try_into().unwrap();
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let mut at = 0;
            let reference = super::take_reference_charged(ctx, &bytes, &mut at)
                .unwrap()
                .unwrap();
            assert_eq!(reference.link_name(), Some("link"));
            assert_eq!(reference.target(), Some(7));
            assert_eq!(at, bytes.len());
        });
        let mut cross_segment = vec![1];
        cross_segment.extend_from_slice(&7_u64.to_le_bytes());
        cross_segment.push(1);
        cross_segment.extend_from_slice(&0_u32.to_le_bytes());
        crate::test_support::lp_utf16(&mut cross_segment, "11111111-2222-3333-4444-555555555555");
        cross_segment.push(1);
        policy.limits.max_retained_bytes = 0;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let mut at = 0;
            let reference = super::take_reference_charged(ctx, &cross_segment, &mut at)
                .unwrap()
                .unwrap();
            assert!(matches!(
                reference,
                super::Reference::CrossSegment {
                    target: 7,
                    segment: 0,
                    inline_type_guid: None
                }
            ));
            assert_eq!(at, cross_segment.len());
        });
        policy.limits.max_work_units = 0;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let error = super::take_reference_charged(ctx, &bytes, &mut 0).unwrap_err();
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
                if failure.operation == "decode F3D reference UTF-16")
            );
        });
    }

    #[test]
    fn reference_inline_guid_probe_borrows_source_text() {
        let mut bytes = vec![1];
        bytes.extend_from_slice(&7_u64.to_le_bytes());
        bytes.extend_from_slice(&36_u32.to_le_bytes());
        bytes.extend_from_slice(b"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        bytes.extend_from_slice(&[0, 0]);
        let mut at = 0;
        let reference = super::take_reference(&bytes, &mut at).unwrap();
        let (target, guid) = reference.local().unwrap();
        assert_eq!(target, 7);
        assert_eq!(guid.unwrap().as_ptr(), bytes[13..].as_ptr());
        assert_eq!(at, bytes.len());
    }

    #[test]
    fn charged_reference_ascii_refusal_keeps_context_and_cursor() {
        let mut bytes = vec![1];
        bytes.extend_from_slice(&7_u64.to_le_bytes());
        bytes.extend_from_slice(&36_u32.to_le_bytes());
        bytes.extend_from_slice(b"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        bytes.extend_from_slice(&[0, 0]);
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_work_units = 0;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let mut at = 0;
            let cadmpeg_core::CodecError::ResourceLimit(limit) =
                super::take_reference_charged(ctx, &bytes, &mut at).unwrap_err()
            else {
                panic!("ASCII admission must preserve resource refusal");
            };
            assert_eq!(limit.operation, "decode F3D reference ASCII");
            assert_eq!(Some(limit), ctx.resource_refusal());
            assert_eq!(at, 0);
        });
    }
}
