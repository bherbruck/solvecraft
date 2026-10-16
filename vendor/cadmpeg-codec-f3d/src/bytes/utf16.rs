// SPDX-License-Identifier: Apache-2.0
//! Validated UTF-16 text borrowed from a counted source field.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use std::fmt::Write;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Utf16View<'a> {
    raw: &'a [u8],
    utf8_len: usize,
}

impl<'a> Utf16View<'a> {
    pub(crate) fn new(raw: &'a [u8]) -> Option<Self> {
        if !raw.len().is_multiple_of(2) {
            return None;
        }
        let mut view = View::over_retained(raw);
        let mut utf8_len = 0usize;
        for character in char::decode_utf16(std::iter::from_fn(|| view.u16_le())) {
            utf8_len = utf8_len.checked_add(character.ok()?.len_utf8())?;
        }
        Some(Self { raw, utf8_len })
    }

    pub(crate) fn chars(self) -> impl Iterator<Item = char> + 'a {
        let mut view = View::over_retained(self.raw);
        // Construction validates every code unit before this iterator is available.
        char::decode_utf16(std::iter::from_fn(move || view.u16_le())).flatten()
    }
    pub(crate) fn is_empty(self) -> bool {
        self.utf8_len == 0
    }
    pub(crate) fn is_guid_hyphenated(self) -> bool {
        self.len() == 36 && self.guid_prefix()
    }
    pub(crate) fn len(self) -> usize {
        self.utf8_len
    }
    pub(crate) fn eq_str(self, text: &str) -> bool {
        self.chars().eq(text.chars())
    }
    pub(crate) fn eq_ignore_ascii_case(self, other: Self) -> bool {
        self.chars()
            .map(|character| character.to_ascii_lowercase())
            .eq(other
                .chars()
                .map(|character| character.to_ascii_lowercase()))
    }
    pub(crate) fn is_guid_relaxed(self) -> bool {
        matches!(self.len(), 36..=38)
            && self.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
    }
    fn guid_prefix(self) -> bool {
        self.len() >= 36
            && self.chars().take(36).enumerate().all(|(index, character)| {
                if matches!(index, 8 | 13 | 18 | 23) {
                    character == '-'
                } else {
                    character.is_ascii_hexdigit()
                }
            })
    }
    pub(crate) fn is_guid_urn_role(self) -> bool {
        self.guid_prefix() && self.chars().skip(36).take(5).eq("_urn:".chars())
    }
    pub(crate) fn to_retained(
        self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<String, CodecError> {
        ctx.utf16le_text(self.raw, self.raw.len() / 2, false, operation)
    }

    pub(crate) fn to_scoped<'ctx>(
        self,
        ctx: &'ctx DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<(String, ScopedReservation<'ctx>), CodecError> {
        ctx.utf16le_scoped_text(self.raw, self.raw.len() / 2, false, operation)
    }
}

impl std::fmt::Display for Utf16View<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for character in self.chars() {
            formatter.write_char(character)?;
        }
        Ok(())
    }
}
impl std::fmt::Debug for Utf16View<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("\"")?;
        for character in self.chars() {
            if character == '\'' {
                formatter.write_char(character)?;
            } else {
                for escaped in character.escape_debug() {
                    formatter.write_char(escaped)?;
                }
            }
        }
        formatter.write_str("\"")
    }
}

#[cfg(test)]
mod tests {
    use super::Utf16View;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn borrowed_utf16_retained_copy_refuses_exact_utf8_budget() {
        let view = Utf16View::new(&[0, 8]).unwrap();
        for retained in [2, 3] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = retained;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let result = view.to_retained(&ctx, "F3D UTF-16 test");
            if retained == 2 {
                assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::RetainedBytes && limit.additional == 3));
            } else {
                assert_eq!(result.unwrap(), "ࠀ");
                assert!(ctx.charge_retained(1, "after F3D text").is_err());
            }
        }
    }
}
