// SPDX-License-Identifier: Apache-2.0
//! Exact thread construction scopes and thread payloads.

use crate::bytes::lp_utf16_bounded_charged;
use crate::layout::thread_compact_construction_tail as thread_compact_tail;
use crate::layout::thread_compact_legacy_construction_tail as thread_compact_legacy_tail;
use crate::layout::thread_owner_marked_scope_prefix as thread_owner;
use crate::layout::thread_standard_construction_tail as thread_tail;
use crate::layout::thread_standard_legacy_construction_tail as thread_standard_legacy_tail;
use crate::layout::thread_standard_scope_prefix as thread_standard;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::thread;
use crate::records::feature::thread::DesignThreadConstruction;
use crate::records::feature::thread::DesignThreadForm;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

pub(super) fn exact_thread_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Result<Option<DesignThreadConstruction>, CodecError> {
    let Some(start) = usize::try_from(scope.byte_offset()).ok() else {
        return Ok(None);
    };
    if scope.kind() != scope::DesignFeatureKind::Thread
        || scope.reference_members().len() < 2
        || !scope.reference_members().len().is_multiple_of(2)
    {
        return Ok(None);
    }
    let Some((prefix_form, designation_delta)) = bytes.get(start..).and_then(exact_thread_prefix)
    else {
        return Ok(None);
    };
    let Some(designation_at) = start.checked_add(designation_delta) else {
        return Ok(None);
    };
    let count = match prefix_form {
        ThreadPrefix::Standard => 1,
        ThreadPrefix::Compact => scope.reference_members().len() / 2,
    };

    let mut face_group_record_indices = Vec::new();
    ctx.reserve_vec(
        &mut face_group_record_indices,
        count,
        "f3d Thread face groups",
    )?;
    match prefix_form {
        ThreadPrefix::Standard => {
            let Some(first) = scope.reference_members().values().next() else {
                return Ok(None);
            };
            face_group_record_indices.push(*first);
        }
        ThreadPrefix::Compact => {
            face_group_record_indices
                .extend(scope.reference_members().values().step_by(2).copied());
        }
    }
    let Some(construction) = parse_thread_payload(
        ctx,
        bytes,
        designation_at,
        prefix_form,
        face_group_record_indices,
    )?
    else {
        return Ok(None);
    };
    let class_pair_is_valid = match construction.form {
        DesignThreadForm::StandardLegacy => {
            scope.class_tag.as_str() == "334" && scope.paired_class_tag.as_str() == "262"
        }
        DesignThreadForm::CompactLegacy => {
            scope.class_tag.as_str() == "414" && scope.paired_class_tag.as_str() == "263"
        }
        DesignThreadForm::Standard | DesignThreadForm::Compact(_) => true,
    };
    if !class_pair_is_valid {
        return Ok(None);
    }
    Ok(Some(construction))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ThreadPrefix {
    Standard,
    Compact,
}

fn exact_thread_prefix(bytes: &[u8]) -> Option<(ThreadPrefix, usize)> {
    let direct = if bytes.get(thread_standard::ZERO_RUN_10..thread_standard::FIXED_SCALAR)?
        == [0; 10]
        && View::f64_le_at(bytes, thread_standard::FIXED_SCALAR)?.to_bits() == 60.0f64.to_bits()
    {
        thread_form(
            bytes,
            thread_standard::STANDARD_MARKER,
            thread_standard::STANDARD_PREFIX_TAIL,
        )
        .map(|form| (form, thread_standard::LEN))
    } else {
        None
    };
    let owner_marked = if bytes.get(thread_owner::ZERO_RUN_9..thread_owner::OWNER_MARKER)? == [0; 9]
        && View::u32_le_at(bytes, thread_owner::OWNER_MARKER)? == 1
        && bytes.get(thread_owner::SEPARATOR) == Some(&0)
        && View::f64_le_at(bytes, thread_owner::FIXED_SCALAR)?.to_bits() == 60.0f64.to_bits()
    {
        thread_form(bytes, thread_owner::FORM_MARKER, thread_owner::FORM_TOKEN)
            .map(|form| (form, thread_owner::LEN))
    } else {
        None
    };
    match (direct, owner_marked) {
        (Some(prefix), None) | (None, Some(prefix)) => Some(prefix),
        _ => None,
    }
}

fn thread_form(bytes: &[u8], marker_at: usize, token_at: usize) -> Option<ThreadPrefix> {
    match (
        bytes.get(marker_at..marker_at + 5)?,
        bytes.get(token_at..token_at + 4)?,
    ) {
        ([1, 2, 0, 0, 0], [0x36, 0, 0x67, 0]) => Some(ThreadPrefix::Standard),
        ([0, 2, 0, 0, 0], [0x36, 0, 0x48, 0]) => Some(ThreadPrefix::Compact),
        _ => None,
    }
}

pub(super) fn parse_thread_payload(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    designation_at: usize,
    expected_form: ThreadPrefix,
    face_group_record_indices: Vec<u32>,
) -> Result<Option<DesignThreadConstruction>, CodecError> {
    let Some((designation, after_designation)) = lp_utf16_bounded_charged(
        ctx,
        bytes,
        designation_at,
        1..=128,
        "f3d Design UTF-16 text",
    )?
    else {
        return Ok(None);
    };
    let Some((nominal_size_text, after_nominal)) = lp_utf16_bounded_charged(
        ctx,
        bytes,
        after_designation,
        1..=64,
        "f3d Design UTF-16 text",
    )?
    else {
        return Ok(None);
    };
    let Some((profile, after_profile)) =
        lp_utf16_bounded_charged(ctx, bytes, after_nominal, 1..=256, "f3d Design UTF-16 text")?
    else {
        return Ok(None);
    };
    Ok((|| {
        let (pitch_marker, trailer_kind) =
            match (expected_form, bytes.get(after_profile..after_profile + 5)?) {
                (ThreadPrefix::Standard, [0, 1, 0, 0, 0]) => (1, ThreadTrailerKind::Standard),
                (ThreadPrefix::Standard, [1, 1, 0, 0, 0]) => (0, ThreadTrailerKind::StandardLegacy),
                (ThreadPrefix::Compact, [1, 2, 0, 0, 0]) => (0, ThreadTrailerKind::Compact),
                (ThreadPrefix::Compact, [1, 1, 0, 0, 0]) => (0, ThreadTrailerKind::CompactLegacy),
                _ => return None,
            };
        let nominal_size = thread::DesignThreadNominalSize::try_from(nominal_size_text).ok()?;
        let major_diameter = View::f64_le_at(bytes, after_profile + thread_tail::MAJOR_DIAMETER)?;
        let minor_diameter = View::f64_le_at(bytes, after_profile + thread_tail::MINOR_DIAMETER)?;
        let pitch = (bytes.get(after_profile + thread_tail::PITCH_MARKER) == Some(&pitch_marker))
            .then(|| View::f64_le_at(bytes, after_profile + thread_tail::PITCH))??;
        let pitch_diameter = View::f64_le_at(bytes, after_profile + thread_tail::PITCH_DIAMETER)?;
        let trailer_at = match trailer_kind {
            ThreadTrailerKind::Standard => thread_tail::STANDARD_TRAILER,
            ThreadTrailerKind::Compact => thread_compact_tail::COMPACT_TRAILER,
            ThreadTrailerKind::StandardLegacy => thread_standard_legacy_tail::LEGACY_TRAILER,
            ThreadTrailerKind::CompactLegacy => thread_compact_legacy_tail::LEGACY_TRAILER,
        };
        let trailer_offset = after_profile.checked_add(trailer_at)?;
        let form = match trailer_kind {
            ThreadTrailerKind::Standard
                if bytes.get(trailer_offset..trailer_offset + 2)? == [0, 1] =>
            {
                DesignThreadForm::Standard
            }
            ThreadTrailerKind::StandardLegacy
                if bytes.get(trailer_offset..trailer_offset + 4)? == [0, 0, 0, 1] =>
            {
                DesignThreadForm::StandardLegacy
            }
            ThreadTrailerKind::CompactLegacy
                if bytes.get(trailer_offset..trailer_offset + 4)? == [0, 0, 0, 1] =>
            {
                DesignThreadForm::CompactLegacy
            }
            ThreadTrailerKind::Compact
                if bytes.get(trailer_offset..trailer_offset + 4)? == [0, 0, 0, 1] =>
            {
                DesignThreadForm::Compact(None)
            }
            ThreadTrailerKind::Compact
                if bytes.get(trailer_offset) == Some(&1)
                    && bytes.get(trailer_offset + 5..trailer_offset + 11)? == [0; 6] =>
            {
                let reference_offset = trailer_offset.checked_add(1)?;
                let record_index =
                    std::num::NonZeroU32::new(View::u32_le_at(bytes, reference_offset)?)?;
                DesignThreadForm::Compact(Some(crate::records::identity::Located {
                    value: record_index,
                    offset: u64::try_from(reference_offset).ok()?,
                }))
            }
            _ => return None,
        };
        Some(DesignThreadConstruction {
            form,
            designation_offset: u64::try_from(designation_at).ok()?,
            designation: cadmpeg_core::text::NonBlankString::new(designation)?,
            nominal_size,
            profile: cadmpeg_core::text::NonBlankString::new(profile)?,
            pitch: cadmpeg_ir::scalar::PositiveReal::new(pitch)?,
            face_group_record_indices,
            diameters: thread::DesignThreadDiameters::new(
                major_diameter,
                minor_diameter,
                pitch_diameter,
            )?,
        })
    })())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThreadTrailerKind {
    Standard,
    Compact,
    StandardLegacy,
    CompactLegacy,
}
