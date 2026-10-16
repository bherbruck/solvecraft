// SPDX-License-Identifier: Apache-2.0
//! Exact legacy thicken and shell scope frames.

use cadmpeg_core::decode::u64_from_index;

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::{fixed_guid_end, fixed_utf16_ascii_eq};
use crate::layout::shell_class_369_261_scope_frame as shell_369_261;
use crate::layout::thicken_class_347_scope_frame as thicken_347;
use crate::records::feature::direct_face;
use crate::records::feature::direct_face::DesignDirectFaceOperation;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::View;

/// Decode class-347/258 Thicken, whose group precedes its scalar. The class
/// pair and 291-byte frame are part of admission for this distinct grammar.
pub(super) fn exact_legacy_thicken_class_347(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignDirectFaceOperation> {
    if scope.class_tag.as_str() != "347"
        || scope.paired_class_tag.as_str() != "258"
        || scope.frame_length() != u64::try_from(thicken_347::LEN).ok()?
        || scope.reference_members().len() != 3
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if bytes.get(start + thicken_347::ZERO_RUN_10..start + thicken_347::FEATURE_FORM)
        != Some(&[0; 10])
        || View::u32_le_at(bytes, start + thicken_347::FEATURE_FORM)? != 4
        || View::u32_le_at(bytes, start + thicken_347::GROUP_FORM)? != 1
        || marked_record_reference(bytes, start + thicken_347::GROUP_REFERENCE)?
            != scope.reference_members().values().next().copied()?
        || bytes.get(start + thicken_347::SCALAR_PREFIX..start + thicken_347::SCALAR_REFERENCE)
            != Some(&[1, 1])
        || View::u32_le_at(bytes, start + thicken_347::AUXILIARY_COUNT)? != 1
        || marked_record_reference(bytes, start + thicken_347::AUXILIARY_REFERENCE).is_none()
        || bytes.get(start + thicken_347::ZERO_RUN_8..start + thicken_347::GUID_CODE_UNIT_COUNT)
            != Some(&[0; 8])
        || View::u32_le_at(bytes, start + thicken_347::GUID_CODE_UNIT_COUNT)? != 36
        || bytes.get(start + thicken_347::ZERO_RUN_3..start + thicken_347::REFERENCE_COUNT)
            != Some(&[0; 3])
        || View::u32_le_at(bytes, start + thicken_347::REFERENCE_COUNT)? != 3
        || View::u32_le_at(bytes, start + thicken_347::KIND_CODE_UNIT_COUNT)? != 7
    {
        return None;
    }
    let guid_end = fixed_guid_end(bytes, start + thicken_347::GUID_CODE_UNIT_COUNT)?;
    if guid_end != start + thicken_347::ZERO_RUN_3 {
        return None;
    }
    let kind_end =
        fixed_utf16_ascii_eq(bytes, start + thicken_347::KIND_CODE_UNIT_COUNT, "Thicken")?;
    if kind_end != start + thicken_347::FEATURE_ORDINAL {
        return None;
    }
    let reference_entries = [
        thicken_347::GROUP_REFERENCE_ENTRY,
        thicken_347::MEMBER_REFERENCE_ENTRY,
        thicken_347::SCALAR_REFERENCE_ENTRY,
    ];
    for (offset, expected) in reference_entries
        .into_iter()
        .zip(scope.reference_members().values().copied())
    {
        if marked_record_reference(bytes, start + offset) != Some(expected) {
            return None;
        }
    }
    let thickness_record_index =
        marked_record_reference(bytes, start + thicken_347::SCALAR_REFERENCE)?;
    if scope.reference_members().values().next_back().copied()? != thickness_record_index {
        return None;
    }
    let scalar = exact_fixed_scalar(bytes, records, thickness_record_index)?;
    (scalar.value.get() != 0.0).then_some(DesignDirectFaceOperation::Thicken(
        direct_face::DesignThickenOperation {
            signed_thickness: scalar.value,
            thickness_record_index,
            thickness_offset: scalar.value_offset,
        },
    ))
}

pub(super) fn exact_shell_class_369_261(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Option<DesignDirectFaceOperation> {
    if scope.class_tag.as_str() != "369"
        || scope.paired_class_tag.as_str() != "261"
        || scope.frame_length() != u64_from_index(shell_369_261::LEN)
        || scope.reference_members().len() != 3
    {
        return None;
    }
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if bytes.get(start + shell_369_261::ZERO_RUN_9..start + shell_369_261::FEATURE_FORM)
        != Some(&[0; 9])
        || bytes.get(start + shell_369_261::FEATURE_FORM)
            != Some(&shell_369_261::FEATURE_FORM_VALUE)
        || bytes.get(start + shell_369_261::ZERO_RUN_3..start + shell_369_261::SCALAR_MARKER)
            != Some(&[0; 3])
        || bytes.get(start + shell_369_261::SCALAR_MARKER)
            != Some(&shell_369_261::SCALAR_MARKER_VALUE)
        || bytes
            .get(start + shell_369_261::ZERO_RUN_9_AFTER_SCALAR..start + shell_369_261::GROUP_FORM)
            != Some(&[0; 9])
        || bytes.get(start + shell_369_261::GROUP_FORM) != Some(&shell_369_261::GROUP_FORM_VALUE)
        || bytes.get(start + 47..start + shell_369_261::GROUP_REFERENCE) != Some(&[0; 3])
        || bytes.get(
            start + shell_369_261::ZERO_RUN_3_BEFORE_REFERENCES
                ..start + shell_369_261::REFERENCE_COUNT,
        ) != Some(&[0; 3])
        || View::u32_le_at(bytes, start + shell_369_261::GUID_CODE_UNIT_COUNT)
            != Some(shell_369_261::GUID_CODE_UNIT_COUNT_VALUE)
        || View::u32_le_at(bytes, start + shell_369_261::REFERENCE_COUNT)
            != Some(shell_369_261::REFERENCE_COUNT_VALUE)
        || View::u32_le_at(bytes, start + shell_369_261::KIND_CODE_UNIT_COUNT)
            != Some(shell_369_261::KIND_CODE_UNIT_COUNT_VALUE)
    {
        return None;
    }
    let outward = match bytes.get(start + shell_369_261::OUTWARD) {
        Some(0) => false,
        Some(1) => true,
        _ => return None,
    };
    let guid_end = fixed_utf16_ascii_eq(
        bytes,
        start + shell_369_261::GUID_CODE_UNIT_COUNT,
        "00000000-0000-0000-0000-000000000000",
    )?;
    if guid_end != start + shell_369_261::ZERO_RUN_3_BEFORE_REFERENCES {
        return None;
    }
    let kind_end =
        fixed_utf16_ascii_eq(bytes, start + shell_369_261::KIND_CODE_UNIT_COUNT, "Shell")?;
    if kind_end != start + shell_369_261::FEATURE_ORDINAL {
        return None;
    }
    let reference_entries = [
        shell_369_261::SCALAR_REFERENCE,
        shell_369_261::GROUP_REFERENCE,
        shell_369_261::REFERENCE_ENTRY_2,
    ];
    for (offset, expected) in reference_entries
        .into_iter()
        .zip(scope.reference_members().values().copied())
    {
        if marked_record_reference(bytes, start + offset) != Some(expected) {
            return None;
        }
    }
    let thickness_record_index = scope.reference_members().values().next().copied()?;
    if marked_record_reference(bytes, start + shell_369_261::SCALAR_REFERENCE)
        != Some(thickness_record_index)
    {
        return None;
    }
    let scalar = exact_fixed_scalar(bytes, records, thickness_record_index)?;
    Some(DesignDirectFaceOperation::Shell(
        direct_face::DesignShellOperation {
            thickness: cadmpeg_ir::scalar::PositiveReal::new(scalar.value.get())?,
            thickness_record_index,
            thickness_offset: scalar.value_offset,
            outward,
            outward_offset: u64::try_from(start + shell_369_261::OUTWARD).ok()?,
        },
    ))
}
