// SPDX-License-Identifier: Apache-2.0
//! Exact assembly operand paths and their locator envelopes.

use super::shared_frames::exact_indexed_header_at;
use super::shared_frames::exact_same_segment_record_reference;
use super::shared_frames::rigid_transform_at;
use crate::bytes::lp_ascii_filtered_view;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::fixed_relaxed_guid_text;
use crate::layout::assembly_operand_path_locator as path_locator;
use crate::layout::assembly_operand_path_locator_reference_run as path_locator_run;
use crate::layout::assembly_operand_path_wrapper as path_wrapper;
use crate::layout::assembly_variable_reference_operand_path_locator as variable_path_locator;
use crate::records::feature::assembly::DesignAssemblyOperandPath;
use crate::records::feature::assembly::DesignAssemblyOperandPathLink;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

pub(super) fn exact_assembly_operand_paths(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<[DesignAssemblyOperandPath; 2]>, CodecError> {
    (|| {
        let scope_at = usize::try_from(scope.byte_offset()).ok()?;
        let search_start = usize::try_from(scope.paired_byte_offset())
            .ok()?
            .checked_add(11)?;
        let locator_offsets = crate::design::assembly::AssemblyScopeGeneration::new(
            scope.frame_length(),
            scope.class_tag.as_str(),
            scope.paired_class_tag.as_str(),
        )
        .operand_path_locator_offsets()?;
        let count_at = scope_at.checked_add(
            locator_offsets[0].checked_sub(path_locator_run::FIRST_LOCATOR_REFERENCE)?,
        )?;
        if View::u32_le_at(bytes, count_at)? != 2 {
            return None;
        }
        let mut paths = [None, None];
        for (ordinal, relative_offset) in locator_offsets.into_iter().enumerate() {
            let locator_reference_at = scope_at.checked_add(relative_offset)?;
            let (locator_record_index, locator_reference_offset) =
                exact_same_segment_record_reference(bytes, locator_reference_at)?;
            let candidates = records
                .offsets(locator_record_index)
                .iter()
                .copied()
                .filter(|locator_at| *locator_at >= search_start);
            let mut candidate = None;
            for locator_at in candidates {
                let parsed = match exact_assembly_operand_path_envelope(
                    ctx,
                    bytes,
                    scope,
                    locator_record_index,
                    locator_reference_offset,
                    locator_at,
                ) {
                    Ok(parsed) => parsed,
                    Err(error) => return Some(Err(error)),
                };
                if let Some(parsed) = parsed {
                    if candidate.replace(parsed).is_some() {
                        return None;
                    }
                }
            }
            paths[ordinal] = Some(candidate?);
        }
        let [Some(first), Some(second)] = paths else {
            return None;
        };
        let first_start = usize::try_from(first.link().locator_byte_offset).ok()?;
        let first_end = next_indexed_record_offset(
            bytes,
            usize::try_from(first.link().wrapper_byte_offset)
                .ok()?
                .checked_add(1)?,
        )?;
        let second_start = usize::try_from(second.link().locator_byte_offset).ok()?;
        let second_end = next_indexed_record_offset(
            bytes,
            usize::try_from(second.link().wrapper_byte_offset)
                .ok()?
                .checked_add(1)?,
        )?;
        if first.link().locator_record_index == second.link().locator_record_index
            || (first_start < second_end && second_start < first_end)
        {
            return None;
        }
        Some(Ok([first, second]))
    })()
    .transpose()
}

fn exact_assembly_operand_path_envelope(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
    locator_record_index: u32,
    locator_reference_offset: u64,
    locator_at: usize,
) -> Result<Option<DesignAssemblyOperandPath>, CodecError> {
    (|| {
        let locator_class_tag = exact_indexed_header_at(bytes, locator_at, locator_record_index)?;
        let variable_reference = crate::design::assembly::variable_reference_assembly_generation(
            scope.class_tag.as_str(),
            scope.paired_class_tag.as_str(),
        );
        let (locator_length, scope_backlink, wrapper_reference, constant_two, zero_tail) =
            if variable_reference {
                if locator_class_tag != "390"
                    || bytes
                        .get(
                            locator_at.checked_add(variable_path_locator::TRANSFORM + 16 * 8)?
                                ..locator_at.checked_add(variable_path_locator::SCOPE_BACKLINK)?,
                        )?
                        .iter()
                        .any(|byte| *byte != 0)
                    || rigid_transform_at(
                        bytes,
                        locator_at.checked_add(variable_path_locator::TRANSFORM)?,
                    )
                    .is_none()
                {
                    return None;
                }
                (
                    variable_path_locator::LEN,
                    variable_path_locator::SCOPE_BACKLINK,
                    variable_path_locator::WRAPPER_REFERENCE,
                    variable_path_locator::CONSTANT_TWO,
                    variable_path_locator::ZERO_TAIL,
                )
            } else {
                if bytes.get(
                    locator_at.checked_add(path_locator::ZERO_RUN_10)?
                        ..locator_at.checked_add(path_locator::NONZERO_RECORD_REFERENCE)?,
                )? != [0; 10]
                    || exact_same_segment_record_reference(
                        bytes,
                        locator_at.checked_add(path_locator::NONZERO_RECORD_REFERENCE)?,
                    )?
                    .0 == 0
                    || bytes.get(locator_at.checked_add(path_locator::ZERO_32)?) != Some(&0)
                    || rigid_transform_at(bytes, locator_at.checked_add(path_locator::TRANSFORM)?)
                        .is_none()
                    || bytes.get(locator_at.checked_add(path_locator::ZERO_161)?) != Some(&0)
                {
                    return None;
                }
                (
                    path_locator::LEN,
                    path_locator::SCOPE_BACKLINK,
                    path_locator::WRAPPER_REFERENCE,
                    path_locator::CONSTANT_TWO,
                    path_locator::ZERO_TAIL_2,
                )
            };
        let (scope_record_index, locator_scope_reference_offset) =
            exact_same_segment_record_reference(bytes, locator_at.checked_add(scope_backlink)?)?;
        let (wrapper_record_index, wrapper_reference_offset) =
            exact_same_segment_record_reference(bytes, locator_at.checked_add(wrapper_reference)?)?;
        let path_record_index = locator_record_index.checked_add(1)?;
        if scope_record_index != scope.record_index
            || if variable_reference {
                !(locator_record_index.checked_add(2)?..=locator_record_index.checked_add(65)?)
                    .contains(&wrapper_record_index)
            } else {
                wrapper_record_index != locator_record_index.checked_add(2)?
            }
            || View::u32_le_at(bytes, locator_at.checked_add(constant_two)?)? != 2
            || bytes
                .get(locator_at.checked_add(zero_tail)?..locator_at.checked_add(locator_length)?)?
                != [0; 2]
        {
            return None;
        }
        let path_at = locator_at.checked_add(locator_length)?;
        if next_indexed_record_offset(bytes, locator_at.checked_add(1)?)? != path_at {
            return None;
        }
        let span_count =
            usize::try_from(wrapper_record_index.checked_sub(path_record_index)?).ok()?;

        let mut path_spans = Vec::new();
        if let Err(error) = ctx.reserve_vec(&mut path_spans, span_count, "f3d assembly path spans")
        {
            return Some(Err(error));
        }
        let mut record_index = path_record_index;
        let mut record_at = path_at;
        let wrapper_at = loop {
            if record_index == wrapper_record_index {
                break record_at;
            }
            let next = next_indexed_record_offset(bytes, record_at.checked_add(1)?)?;
            path_spans.push((record_index, record_at, next));
            record_index = record_index.checked_add(1)?;
            record_at = next;
        };
        let wrapper_class_tag = exact_indexed_header_at(bytes, wrapper_at, wrapper_record_index)?;
        let wrapper_end = next_indexed_record_offset(bytes, wrapper_at.checked_add(1)?)?;
        let expected_wrapper_length = if variable_reference {
            path_wrapper::LEN.checked_add(path_spans.len().checked_sub(1)?.checked_mul(11)?)?
        } else {
            path_wrapper::LEN
        };
        if variable_reference && wrapper_class_tag != "397"
            || bytes.get(
                wrapper_at.checked_add(path_wrapper::ZERO_RUN_10)?
                    ..wrapper_at.checked_add(path_wrapper::CONSTANT_ONE_BYTE)?,
            )? != [0; 10]
            || bytes.get(wrapper_at.checked_add(path_wrapper::CONSTANT_ONE_BYTE)?) != Some(&1)
            || View::u32_le_at(
                bytes,
                wrapper_at.checked_add(path_wrapper::CONSTANT_ONE_WORD)?,
            )? != if variable_reference {
                u32::try_from(path_spans.len()).ok()?
            } else {
                1
            }
            || wrapper_end != wrapper_at.checked_add(expected_wrapper_length)?
        {
            return None;
        }
        let (referenced_path_record_index, path_reference_offset) =
            exact_same_segment_record_reference(
                bytes,
                wrapper_at.checked_add(path_wrapper::PATH_REFERENCE)?,
            )?;
        if referenced_path_record_index != path_record_index {
            return None;
        }
        if variable_reference
            && path_spans
                .iter()
                .skip(1)
                .enumerate()
                .any(|(ordinal, (record_index, _, _))| {
                    exact_same_segment_record_reference(
                        bytes,
                        wrapper_at + path_wrapper::LEN + ordinal * 11,
                    )
                    .map(|reference| reference.0)
                        != Some(*record_index)
                })
        {
            return None;
        }
        let link = DesignAssemblyOperandPathLink {
            locator_reference_offset,
            locator_record_index,
            locator_class_tag: locator_class_tag.try_into().ok()?,
            locator_byte_offset: u64::try_from(locator_at).ok()?,
            locator_scope_reference_offset,
            wrapper_record_index,
            wrapper_reference_offset,
            wrapper_class_tag: wrapper_class_tag.try_into().ok()?,
            wrapper_byte_offset: u64::try_from(wrapper_at).ok()?,
            path_reference_offset,
        };
        let mut paths = path_spans.into_iter().map(|(record_index, start, limit)| {
            exact_assembly_operand_path(ctx, bytes, start, record_index, limit, link.clone())
        });
        let mut path = match paths.next()? {
            Ok(Some(path)) => path,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if variable_reference && path.class_tag().as_str() != "330" {
            return None;
        }
        for continuation in paths {
            let continuation = match continuation {
                Ok(Some(path)) => path,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
            if !variable_reference || continuation.class_tag().as_str() != "330" {
                return None;
            }
            path = match path.try_append(continuation, ctx) {
                Ok(Some(path)) => path,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
        }
        Some(Ok(path))
    })()
    .transpose()
}

fn exact_assembly_operand_path(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    record_index: u32,
    limit: usize,
    link: DesignAssemblyOperandPathLink,
) -> Result<Option<DesignAssemblyOperandPath>, CodecError> {
    (|| {
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, start, 1..=8, u8::is_ascii_digit)?;
        if View::u64_le_at(bytes, after_tag)? != u64::from(record_index) {
            return None;
        }
        let mut occurrence_guids = Vec::new();
        let mut identity_guids = Vec::new();
        match class_tag {
            "294" | "299" | "307" => {
                let end = next_indexed_record_offset(bytes, start + 1)?;
                if end != limit
                    || bytes.get(after_tag + 8..after_tag + 14)? != [0; 6]
                    || bytes.get(after_tag + 14) != Some(&1)
                    || bytes.get(after_tag + 15..after_tag + 18)? != [0; 3]
                {
                    return None;
                }
                let mut position = after_tag + 18;
                let (occurrence, after_occurrence) =
                    match fixed_relaxed_guid_text(ctx, bytes.get(..end)?, position) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                occurrence_guids.push(crate::records::identity::Located {
                    value: occurrence,
                    offset: u64::try_from(position + 4).ok()?,
                });
                position = after_occurrence;
                for _ in 0..2 {
                    let (guid, after_guid) =
                        match fixed_relaxed_guid_text(ctx, bytes.get(..end)?, position) {
                            Ok(Some(value)) => value,
                            Ok(None) => return None,
                            Err(error) => return Some(Err(error)),
                        };
                    identity_guids.push(crate::records::identity::Located {
                        value: guid,
                        offset: u64::try_from(position + 4).ok()?,
                    });
                    position = after_guid;
                }
                if View::u64_le_at(bytes, position)? != 2 {
                    return None;
                }
                position += 8;
                for _ in 0..2 {
                    let (guid, after_guid) =
                        match fixed_relaxed_guid_text(ctx, bytes.get(..end)?, position) {
                            Ok(Some(value)) => value,
                            Ok(None) => return None,
                            Err(error) => return Some(Err(error)),
                        };
                    identity_guids.push(crate::records::identity::Located {
                        value: guid,
                        offset: u64::try_from(position + 4).ok()?,
                    });
                    position = after_guid;
                }
                if View::u32_le_at(bytes, position)? != 2
                    || !bytes.get(position + 4..end)?.iter().all(|byte| *byte == 0)
                {
                    return None;
                }
            }
            "329" | "330" | "386" | "390" => {
                if bytes.get(after_tag + 8..after_tag + 14)? != [0; 6] {
                    return None;
                }
                let count = usize::try_from(View::u32_le_at(bytes, after_tag + 14)?).ok()?;
                if !(1..=64).contains(&count) {
                    return None;
                }

                if let Err(error) = ctx.reserve_vec(
                    &mut occurrence_guids,
                    count,
                    "f3d assembly path occurrences",
                ) {
                    return Some(Err(error));
                }
                let mut position = after_tag + 18;
                for _ in 0..count {
                    let (guid, after_guid) =
                        match fixed_relaxed_guid_text(ctx, bytes.get(..limit)?, position) {
                            Ok(Some(value)) => value,
                            Ok(None) => return None,
                            Err(error) => return Some(Err(error)),
                        };
                    occurrence_guids.push(crate::records::identity::Located {
                        value: guid,
                        offset: u64::try_from(position + 4).ok()?,
                    });
                    position = after_guid;
                }
                if position == limit {
                    if !matches!(class_tag, "329" | "330") {
                        return None;
                    }
                } else {
                    for _ in 0..2 {
                        let (guid, after_guid) =
                            match fixed_relaxed_guid_text(ctx, bytes.get(..limit)?, position) {
                                Ok(Some(value)) => value,
                                Ok(None) => return None,
                                Err(error) => return Some(Err(error)),
                            };
                        identity_guids.push(crate::records::identity::Located {
                            value: guid,
                            offset: u64::try_from(position + 4).ok()?,
                        });
                        position = after_guid;
                    }
                    if View::u64_le_at(bytes, position)? != 2 {
                        return None;
                    }
                    position += 8;
                    for _ in 0..2 {
                        let (guid, after_guid) =
                            match fixed_relaxed_guid_text(ctx, bytes.get(..limit)?, position) {
                                Ok(Some(value)) => value,
                                Ok(None) => return None,
                                Err(error) => return Some(Err(error)),
                            };
                        identity_guids.push(crate::records::identity::Located {
                            value: guid,
                            offset: u64::try_from(position + 4).ok()?,
                        });
                        position = after_guid;
                    }
                    if View::u32_le_at(bytes, position)? != 2
                        || !bytes
                            .get(position + 4..limit)?
                            .iter()
                            .all(|byte| *byte == 0)
                    {
                        return None;
                    }
                }
            }
            _ => return None,
        }
        DesignAssemblyOperandPath::try_new(
            link,
            record_index,
            crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
            u64::try_from(start).ok()?,
            occurrence_guids,
            identity_guids,
        )
        .ok()
        .map(Ok)
    })()
    .transpose()
}

#[cfg(test)]
mod tests;
