// SPDX-License-Identifier: Apache-2.0
//! Shared STEP Part 21 byte-fixture helpers for `#[cfg(test)]` suites.

#![allow(clippy::unwrap_used)]

pub(crate) mod exchange;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

pub(crate) fn with_service_context<T>(
    bytes: &[u8],
    use_context: impl FnOnce(&[u8], &DecodeContext<'_>) -> T,
) -> T {
    with_policy_context(bytes, &DecodePolicy::service(), use_context)
}

pub(crate) fn with_policy_context<T>(
    bytes: &[u8],
    policy: &DecodePolicy,
    use_context: impl FnOnce(&[u8], &DecodeContext<'_>) -> T,
) -> T {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, policy)
        .expect("fixture fits selected policy");
    use_context(bytes, &ctx)
}
