// SPDX-License-Identifier: Apache-2.0
//! Exact assembly operand frames, including as-built frames.

use cadmpeg_core::decode::u64_from_index;

use super::legacy_operand_paths::exact_legacy_class_388_scope;
use super::shared_frames::marked_record_reference;
use super::shared_frames::rigid_transform_at;
use crate::bytes::f64s_at;
use crate::layout::assembly_class_383_258_scope_1011 as class_383_scope;
use crate::layout::assembly_class_388_266_scope_968 as class_388_assemble;
use crate::layout::assembly_class_406_261_scope_671 as class_406_assemble;
use crate::layout::assembly_operand_path_locator as path_locator;
use crate::records::feature::assembly::DesignAssemblyOperandFrame;
use crate::records::feature::assembly::DesignAssemblyOperandPath;
use crate::records::feature::scope::DesignParameterScope;

pub(super) fn exact_assembly_operand_frames(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<[DesignAssemblyOperandFrame; 2]> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let frame_variant = crate::design::assembly::AssemblyScopeGeneration::new(
        scope.frame_length(),
        scope.class_tag.as_str(),
        scope.paired_class_tag.as_str(),
    )
    .operand_frame_variant()?;
    let frame_offsets = match frame_variant {
        crate::design::assembly::AssemblyOperandFrameVariant::LegacyClass388 => (
            class_388_assemble::FIRST_OPERAND_REFERENCE,
            class_388_assemble::FIRST_OPERAND_TRANSFORM,
            class_388_assemble::SECOND_OPERAND_REFERENCE,
            class_388_assemble::SECOND_OPERAND_TRANSFORM,
        ),
        crate::design::assembly::AssemblyOperandFrameVariant::Standard
            if scope.class_tag.as_str() == "383" && scope.paired_class_tag.as_str() == "258" =>
        {
            (
                class_383_scope::FIRST_OPERAND_REFERENCE,
                class_383_scope::FIRST_OPERAND_TRANSFORM,
                class_383_scope::SECOND_OPERAND_REFERENCE,
                class_383_scope::SECOND_OPERAND_TRANSFORM,
            )
        }
        crate::design::assembly::AssemblyOperandFrameVariant::Standard
            if scope.class_tag.as_str() == "406" && scope.paired_class_tag.as_str() == "261" =>
        {
            (
                class_406_assemble::FIRST_OPERAND_REFERENCE,
                class_406_assemble::FIRST_OPERAND_TRANSFORM,
                class_406_assemble::SECOND_OPERAND_REFERENCE,
                class_406_assemble::SECOND_OPERAND_TRANSFORM,
            )
        }
        crate::design::assembly::AssemblyOperandFrameVariant::Standard => (28, 40, 168, 180),
        crate::design::assembly::AssemblyOperandFrameVariant::Compact => (24, 36, 164, 176),
        crate::design::assembly::AssemblyOperandFrameVariant::Axial => (28, 39, 167, 178),
    };
    if usize::try_from(scope.paired_byte_offset()).ok()?
        != start.checked_add(usize::try_from(scope.frame_length()).ok()?)?
        || bytes.get(start + 11..start + 20)? != [0; 9]
    {
        return None;
    }
    if matches!(
        frame_variant,
        crate::design::assembly::AssemblyOperandFrameVariant::LegacyClass388
    ) {
        exact_legacy_class_388_scope(bytes, scope)?;
        if bytes.get(start + 11..start + class_388_assemble::SCOPE_FLAGS)?
            != [0; class_388_assemble::SCOPE_FLAGS - 11]
            || bytes.get(
                start + class_388_assemble::SCOPE_FLAGS
                    ..start + class_388_assemble::SCOPE_FLAGS + 6,
            )? != class_388_assemble::SCOPE_FLAGS_VALUE
            || bytes.get(start + 26..start + class_388_assemble::FIRST_OPERAND_REFERENCE)? != [0; 2]
            || bytes.get(start + 39) != Some(&0)
            || bytes.get(start + 168 + 11..start + class_388_assemble::SECOND_OPERAND_TRANSFORM)?
                != [0; 1]
        {
            return None;
        }
    } else if matches!(
        frame_variant,
        crate::design::assembly::AssemblyOperandFrameVariant::Standard
    ) {
        let standard_tail_marker_offset = if scope.frame_length()
            == u64_from_index(class_383_scope::LEN)
            && scope.class_tag.as_str() == "383"
            && scope.paired_class_tag.as_str() == "258"
        {
            class_383_scope::STANDARD_TAIL_MARKER
        } else {
            308
        };
        if bytes.get(start + 20..start + 25)? != [1, 0, 0, 0, 0]
            || !matches!(bytes.get(start + 25), Some(0 | 1))
            || bytes.get(start + 26..start + 28)? != [0; 2]
            || bytes.get(start + 33..start + 40)? != [0; 7]
            || bytes.get(start + 173..start + 180)? != [0; 7]
            || !crate::design::assembly::variable_reference_assembly_generation(
                scope.class_tag.as_str(),
                scope.paired_class_tag.as_str(),
            ) && bytes
                .get(start + standard_tail_marker_offset..start + standard_tail_marker_offset + 4)?
                != [0; 4]
        {
            return None;
        }
    } else if matches!(
        frame_variant,
        crate::design::assembly::AssemblyOperandFrameVariant::Compact
    ) {
        let compact_flags = bytes.get(start + 20..start + 24)?;
        if (compact_flags != [0; 4] && compact_flags != [0, 1, 0, 0])
            || bytes.get(start + 29..start + 36)? != [0; 7]
            || bytes.get(start + 169..start + 176)? != [0; 7]
            || bytes.get(start + 304..start + 308)? != [0; 4]
        {
            return None;
        }
    } else if bytes.get(start + 20..start + 25)? != [1, 0, 0, 0, 0]
        || !matches!(bytes.get(start + 25), Some(0 | 1))
        || bytes.get(start + 26..start + 28)? != [0; 2]
        || bytes.get(start + 33..start + 39)? != [0; 6]
        || bytes.get(start + 172..start + 178)? != [0; 6]
        || bytes.get(start + 306..start + 310)? != [0; 4]
    {
        return None;
    }
    let frame = |reference_at: usize, transform_at: usize| {
        let reference_record_index = marked_record_reference(bytes, reference_at)?;
        let values = f64s_at::<16>(bytes, transform_at)?;
        let mut transform = [[0.0; 4]; 4];
        for (ordinal, value) in values.into_iter().enumerate() {
            transform[ordinal / 4][ordinal % 4] = value;
        }
        let transform =
            crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform).ok()?;
        Some(DesignAssemblyOperandFrame {
            reference_record_index,
            reference_offset: u64_from_index(reference_at + 1),
            transform,
            transform_offset: u64_from_index(transform_at),
        })
    };
    let first = frame(start + frame_offsets.0, start + frame_offsets.1)?;
    let second = frame(start + frame_offsets.2, start + frame_offsets.3)?;
    (first.reference_record_index != second.reference_record_index).then_some([first, second])
}

pub(super) fn exact_as_built_operand_frames(
    bytes: &[u8],
    paths: &[DesignAssemblyOperandPath; 2],
) -> Option<[DesignAssemblyOperandFrame; 2]> {
    let frames = paths.each_ref().map(|path| {
        let locator_at = usize::try_from(path.link().locator_byte_offset).ok()?;
        let reference_at = locator_at.checked_add(path_locator::NONZERO_RECORD_REFERENCE)?;
        let transform_at = locator_at.checked_add(path_locator::TRANSFORM)?;
        Some(DesignAssemblyOperandFrame {
            reference_record_index: marked_record_reference(bytes, reference_at)?,
            reference_offset: u64::try_from(reference_at.checked_add(1)?).ok()?,
            transform: rigid_transform_at(bytes, transform_at)?,
            transform_offset: u64::try_from(transform_at).ok()?,
        })
    });
    let [Some(first), Some(second)] = frames else {
        return None;
    };
    (first.reference_record_index != second.reference_record_index).then_some([first, second])
}
