// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for the crate's `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.f3d` ZIP archives and ASM stream payloads.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use std::io::Write;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::{report::export::WritePath, CadIr, SourceFidelity};

use crate::F3dCodec;

pub(crate) fn with_decode_context<T>(
    f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    with_decode_policy(&cadmpeg_core::decode::DecodePolicy::service(), f)
}

pub(crate) fn with_decode_policy<T>(
    policy: &cadmpeg_core::decode::DecodePolicy,
    f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, policy)
        .expect("test decode context");
    f(&ctx)
}

/// Admit earlier requests until the selected operation refuses at its boundary.
/// `skip` selects a later request when an operation labels multiple records.
pub(crate) fn resource_refusal_at<T>(
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &str,
    skip: usize,
    decode: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, CodecError>,
) -> CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    cadmpeg_test_support::refusal::resource_limit_at_nth(dimension, operation, skip, |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            _ => panic!("unsupported refusal dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = decode(&ctx);
        if let Err(CodecError::ResourceLimit(refusal)) = &result {
            assert_eq!(refusal.dimension, dimension);
            assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
        }
        result
    })
}

/// Plans an inherited write through the sealed encoder and writes its bytes.
pub(crate) fn plan_inherited_write(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    writer: &mut dyn Write,
) -> Result<WritePath, CodecError> {
    let plan = F3dCodec.plan(EncodeInput::new(ir, Some(fidelity)), TargetRequest::Inherit)?;
    Ok(plan.write_to(writer)?.write_path().clone())
}

/// A finite test value as the checked scalar a record holds.
pub(crate) fn real(value: f64) -> cadmpeg_ir::scalar::FiniteReal {
    cadmpeg_ir::scalar::FiniteReal::new(value).expect("a finite test value")
}

/// Finite test values as the checked lane a record holds.
pub(crate) fn reals<const N: usize>(values: [f64; N]) -> [cadmpeg_ir::scalar::FiniteReal; N] {
    values.map(real)
}

/// Write a length-prefixed ASCII string: a little-endian `u32` byte count then
/// the bytes.
pub(crate) fn lp_ascii(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(
        &u32::try_from(value.len())
            .expect("test ASCII string length")
            .to_le_bytes(),
    );
    out.extend_from_slice(value.as_bytes());
}

/// Write a length-prefixed UTF-16 string: a little-endian `u32` code-unit count
/// then the little-endian units.
pub(crate) fn lp_utf16(out: &mut Vec<u8>, value: &str) {
    let units = value.encode_utf16().collect::<Vec<_>>();
    out.extend_from_slice(
        &u32::try_from(units.len())
            .expect("test UTF-16 unit count")
            .to_le_bytes(),
    );
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
}

/// Write a same-document reference to a `u64` target: the presence byte, the
/// target, then the two zero bytes that close the reference.
pub(crate) fn push_reference_u64(out: &mut Vec<u8>, target: u64) {
    out.push(1);
    out.extend_from_slice(&target.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
}

/// A same-document reference record to `target`.
pub(crate) fn local_reference(target: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_reference_u64(&mut bytes, target);
    bytes
}

/// A cross-document reference record: the presence byte, the target, the
/// cross-document marker, the source type GUID, the link type GUID, and the
/// link name.
pub(crate) fn cross_document_reference(target: u64, link_name: &str) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&target.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
    bytes.push(0);
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(b"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    lp_utf16(&mut bytes, link_name);
    bytes.push(0);
    bytes
}

/// Append an indexed record header: the three-byte class tag behind its
/// little-endian `u32` length, then the little-endian record index.
pub(crate) fn indexed_header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&class_tag);
    bytes.extend_from_slice(&record_index.to_le_bytes());
}

/// Write an indexed record header over the first eleven bytes of `bytes`.
fn write_indexed_header(bytes: &mut [u8], class_tag: [u8; 3], record_index: u32) {
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(&class_tag);
    bytes[7..11].copy_from_slice(&record_index.to_le_bytes());
}

/// Write a present marked reference to `record_index` at `at`: the presence
/// byte then the little-endian record index.
pub(crate) fn write_marked_reference(bytes: &mut [u8], at: usize, record_index: u32) {
    bytes[at] = 1;
    bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
}

/// Append a present marked reference to `record_index`: the presence byte, the
/// little-endian record index, then the six zero bytes that close the mark.
pub(crate) fn push_marked_reference(bytes: &mut Vec<u8>, record_index: u32) {
    bytes.push(1);
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
}

pub(crate) mod tokens_test;

pub(crate) mod smbh_header_test;

pub(crate) mod smbh_blocks_test;

pub(crate) mod smbh_geometry_test;

pub(crate) mod smbh_pcurves_test;

pub(crate) mod smbh_curves_test;

pub(crate) mod smbh_surfaces_test;

pub(crate) mod smbh_revision_test;

pub(crate) mod smbh_blends_test;

pub(crate) mod smbh_bf4_test;

pub(crate) mod native_test;

pub(crate) mod manifest_test;

pub(crate) mod protein_test;

pub(crate) mod streams_test;

pub(crate) mod zip_test;

pub(crate) mod assembly_test;

pub(crate) mod procedural_test;
