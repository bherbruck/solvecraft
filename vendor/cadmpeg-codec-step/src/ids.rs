// SPDX-License-Identifier: Apache-2.0
//! Codec-owned STEP identity builders.
//!
//! Every mint path composes `<format>:<scope>:<kind>#<key>` from parts that
//! already hold the identity grammar, so a two-component id such as
//! `step:signature#0` cannot be produced and no mint path can fail.

use cadmpeg_ir::ids::{Identity, IdentityComponent, IdentityKey, IdentityNamespace, UnknownId};

/// The `<kind>` component of a STEP identity.
///
/// The component holds at least one character and no `:`, `#` or whitespace,
/// checked when the value is built. The `kind!` macro builds one from a source
/// literal when the crate compiles; `IdentityKind::try_new` admits
/// file-derived text and returns an error when the text cannot be a kind.
pub(crate) type IdentityKind = IdentityComponent;

/// The `<format>` component of every STEP identity.
static FORMAT: IdentityComponent = cadmpeg_ir::identity_component!("step");

/// The `<scope>` component of a file-level identity.
static SCOPE_FILE: IdentityComponent = cadmpeg_ir::identity_component!("file");

/// The `<scope>` component of a DATA-section identity.
static SCOPE_DATA: IdentityComponent = cadmpeg_ir::identity_component!("data");

/// The `<scope>` component of a product-structure identity.
static SCOPE_PRODUCT: IdentityComponent = cadmpeg_ir::identity_component!("product");

/// The `<scope>` component of a presentation or PMI identity.
static SCOPE_PRESENTATION: IdentityComponent = cadmpeg_ir::identity_component!("presentation");

/// The `<scope>` component of a construction identity.
static SCOPE_CONSTRUCTION: IdentityComponent = cadmpeg_ir::identity_component!("construction");

/// The `<scope>` component of a tessellation identity.
static SCOPE_TESSELLATION: IdentityComponent = cadmpeg_ir::identity_component!("tessellation");

/// The `<scope>` component of a drawing-graph identity.
static SCOPE_DRAWING: IdentityComponent = cadmpeg_ir::identity_component!("drawing");

/// The `<kind>` component a source literal spells.
///
/// The literal is checked in the initializer of a `static` item, so a literal
/// outside the kind grammar is an E0080 under `cargo check`.
macro_rules! kind {
    ($literal:literal) => {{
        static KIND: $crate::ids::IdentityKind = cadmpeg_ir::identity_component!($literal);
        &KIND
    }};
}

pub(crate) use kind;

/// The `<key>` component a source literal spells.
///
/// The literal is checked in the initializer of a `static` item, so a literal
/// outside the key grammar is an E0080 under `cargo check`. Compose a longer
/// key from these with [`IdentityKey::dash`], [`IdentityKey::colon`] and
/// [`IdentityKey::then`], each of which keeps the grammar.
macro_rules! key_word {
    ($literal:literal) => {{
        static KEY_WORD: cadmpeg_ir::ids::IdentityKey = cadmpeg_ir::identity_key!($literal);
        KEY_WORD.clone()
    }};
}

pub(crate) use key_word;

/// File-level signature opaque record: `step:file:signature#{index}`.
#[must_use]
pub(crate) fn signature(index: usize) -> UnknownId {
    let namespace = IdentityNamespace::from_components(&FORMAT, &SCOPE_FILE, kind!("signature"));
    UnknownId::from(Identity::compose(&namespace, IdentityKey::from(index)))
}

/// Retained nonconforming header record, keyed by source byte offset.
pub(crate) fn header(offset: usize) -> UnknownId {
    let namespace = IdentityNamespace::from_components(&FORMAT, &SCOPE_FILE, kind!("header"));
    UnknownId::from(Identity::compose(&namespace, IdentityKey::from(offset)))
}

/// Omitted exchange statement, keyed by its source byte offset.
pub(crate) fn omitted_record(offset: usize) -> UnknownId {
    let namespace = IdentityNamespace::from_components(&FORMAT, &SCOPE_FILE, kind!("omitted"));
    UnknownId::from(Identity::compose(&namespace, IdentityKey::from(offset)))
}

/// Retained ZIP member declaration, keyed by its physical central offset.
#[must_use]
pub(crate) fn zip_declaration(offset: u64) -> UnknownId {
    let namespace =
        IdentityNamespace::from_components(&FORMAT, &SCOPE_FILE, kind!("zip-declaration"));
    UnknownId::from(Identity::compose(&namespace, IdentityKey::from(offset)))
}

/// DATA-section geometry or opaque kind: `step:data:{kind}#{key}`.
#[must_use]
pub(crate) fn data(kind: &IdentityKind, key: impl Into<IdentityKey>) -> Identity {
    let namespace = IdentityNamespace::from_components(&FORMAT, &SCOPE_DATA, kind);
    Identity::compose(&namespace, key)
}

/// Product structure identity: `step:product:{kind}#{key}`.
#[must_use]
pub(crate) fn product(kind: &IdentityKind, key: impl Into<IdentityKey>) -> Identity {
    let namespace = IdentityNamespace::from_components(&FORMAT, &SCOPE_PRODUCT, kind);
    Identity::compose(&namespace, key)
}

/// Presentation / PMI identity: `step:presentation:{kind}#{key}`.
#[must_use]
pub(crate) fn presentation(kind: &IdentityKind, key: impl Into<IdentityKey>) -> Identity {
    let namespace = IdentityNamespace::from_components(&FORMAT, &SCOPE_PRESENTATION, kind);
    Identity::compose(&namespace, key)
}

/// Construction / procedural identity: `step:construction:{kind}#{key}`.
#[must_use]
pub(crate) fn construction(kind: &IdentityKind, key: impl Into<IdentityKey>) -> Identity {
    let namespace = IdentityNamespace::from_components(&FORMAT, &SCOPE_CONSTRUCTION, kind);
    Identity::compose(&namespace, key)
}

/// Tessellation identity: `step:tessellation:{kind}#{key}`.
#[must_use]
pub(crate) fn tessellation(kind: &IdentityKind, key: impl Into<IdentityKey>) -> Identity {
    let namespace = IdentityNamespace::from_components(&FORMAT, &SCOPE_TESSELLATION, kind);
    Identity::compose(&namespace, key)
}

/// Drawing graph identity: `step:drawing:{kind}#{key}`.
#[must_use]
pub(crate) fn drawing(kind: &IdentityKind, key: impl Into<IdentityKey>) -> Identity {
    let namespace = IdentityNamespace::from_components(&FORMAT, &SCOPE_DRAWING, kind);
    Identity::compose(&namespace, key)
}

#[cfg(test)]
mod tests;
