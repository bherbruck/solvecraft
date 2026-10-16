// SPDX-License-Identifier: Apache-2.0
//! Format retained Design text under the caller budget.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::fmt;

/// Preserve a malformed diagnostic or return its resource refusal.
pub(super) fn malformed_design(
    ctx: &DecodeContext<'_>,
    arguments: fmt::Arguments<'_>,
) -> CodecError {
    match ctx.format_retained(arguments, "f3d Design diagnostic") {
        Ok(text) => CodecError::Malformed(text),
        Err(error) => error,
    }
}

#[cfg(test)]
mod tests;
