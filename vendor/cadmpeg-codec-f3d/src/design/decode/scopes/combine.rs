// SPDX-License-Identifier: Apache-2.0
//! Exact combine operations and external body identities.

use super::draft::contains_consecutive_guid_pair;
use super::parameter_scope::parameter_scope_payload_length;
use crate::bytes::lp_utf16_bounded_charged;
use crate::bytes::take_reference;
use crate::design::decode::sketch::IndexedRecordOffsets;

use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::layout::combine_compact_operation_prefix as combine_compact;
use crate::layout::combine_extended_reference_operation_prefix as combine_extended;
use crate::layout::combine_external_selector_prefix as combine_external;
use crate::layout::combine_standard_operation_prefix as combine_standard;
use crate::records::feature::combine;
use crate::records::feature::combine::DesignCombineBodySelection;
use crate::records::feature::combine::DesignCombineExternalBodyIdentity;
use crate::records::feature::combine::DesignCombineExternalBodyIdentityWire;
use crate::records::feature::combine::DesignCombineForm;
use crate::records::feature::combine::DesignCombineOperation;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

pub(super) fn exact_combine_operation(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Result<Option<DesignCombineOperation>, CodecError> {
    (|| {
        if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Combine)
            || scope.reference_members().len() < 4
            || !scope.reference_members().len().is_multiple_of(2)
        {
            return None;
        }
        let start = usize::try_from(scope.byte_offset()).ok()?;
        let compact = scope.class_tag.as_str() == "387"
            && scope.paired_class_tag.as_str() == "258"
            && parameter_scope_payload_length(scope) == Some(314);
        let extended_reference = scope.class_tag.as_str() == "329"
            && scope.paired_class_tag.as_str() == "261"
            && scope.frame_length() == 363;
        let (form, operation_offset, keep_tools_offset) = if compact {
            if bytes
                .get(start + combine_compact::ZERO_RUN_10..start + combine_compact::OPERATION)?
                != [0; 10]
                || bytes.get(
                    start + combine_compact::ZERO_RUN_3..start + combine_compact::REFERENCE_FORM,
                )? != [0; 3]
                || bytes.get(
                    start + combine_compact::REFERENCE_FORM..start + combine_compact::CONSTANT_ONE,
                )? != [1, 0]
                || View::u32_le_at(bytes, start + combine_compact::CONSTANT_ONE) != Some(1)
                || bytes.get(start + combine_compact::REFERENCE_MARKER) != Some(&1)
                || View::u64_le_at(bytes, start + combine_compact::REFERENCE_VALUE) == Some(0)
                || bytes
                    .get(start + combine_compact::REFERENCE_TAIL..start + combine_compact::LEN)?
                    != [0; 2]
            {
                return None;
            }
            (
                DesignCombineForm::Compact,
                start + combine_compact::OPERATION,
                start + combine_compact::KEEP_TOOLS,
            )
        } else if extended_reference {
            let mut reference_at = start.checked_add(combine_extended::REFERENCE_MARKER)?;
            let reference = take_reference(bytes, &mut reference_at)?;
            if bytes
                .get(start + combine_extended::ZERO_RUN_18..start + combine_extended::FORM_MARKER)?
                != [0; 18]
                || bytes.get(start + combine_extended::FORM_MARKER) != Some(&1)
                || reference.local().is_none_or(|(target, _)| target == 0)
                || reference_at != start.checked_add(combine_extended::LEN)?
            {
                return None;
            }
            (
                DesignCombineForm::ExtendedReference,
                start + combine_extended::OPERATION,
                start + combine_extended::KEEP_TOOLS,
            )
        } else {
            if bytes
                .get(start + combine_standard::ZERO_RUN_9..start + combine_standard::OPERATION)?
                != [0; 9]
                || bytes.get(start + combine_standard::ZERO_FLAG) != Some(&0)
                || bytes.get(start + combine_standard::ZERO_RUN_7..start + combine_standard::LEN)?
                    != [0; 7]
            {
                return None;
            }
            (
                DesignCombineForm::Standard,
                start + combine_standard::OPERATION,
                start + combine_standard::KEEP_TOOLS,
            )
        };
        let operation = match View::u32_le_at(bytes, operation_offset)? {
            1 => cadmpeg_ir::features::BooleanKind::Join,
            2 => cadmpeg_ir::features::BooleanKind::Cut,
            3 => cadmpeg_ir::features::BooleanKind::Intersect,
            _ => return None,
        };
        let keep_tools = match bytes.get(keep_tools_offset)? {
            0 => false,
            1 => true,
            _ => return None,
        };
        let mut target = None;
        let mut first_tool = None;
        let mut additional_tools = Vec::new();
        for (operation_record_index, selection_record_index) in scope
            .reference_members()
            .values()
            .step_by(2)
            .zip(scope.reference_members().values().skip(1).step_by(2))
        {
            let [operation_at, operation_end] = records.offsets(*operation_record_index) else {
                return None;
            };
            let role = combine_operation_identity_role(
                bytes.get(*operation_at..*operation_end)?,
                *selection_record_index,
            )?;
            let [selection_at, selection_end] = records.offsets(*selection_record_index) else {
                return None;
            };
            if !contains_consecutive_guid_pair(bytes.get(*selection_at..*selection_end)?) {
                return None;
            }
            match role {
                CombineOperandRole::Target => {
                    if target.replace(*selection_record_index).is_some() {
                        return None;
                    }
                }
                CombineOperandRole::Tool => {
                    let additional = first_tool.is_some();
                    if additional {
                        if let Err(error) = ctx.reserve_vec(
                            &mut additional_tools,
                            1,
                            "f3d Combine additional tools",
                        ) {
                            return Some(Err(error));
                        }
                    }
                    let external_identity = match exact_combine_external_body_identity(
                        ctx,
                        bytes,
                        *selection_at,
                        *selection_end,
                        scope.record_index,
                        *selection_record_index,
                    ) {
                        Ok(identity) => identity,
                        Err(error) => return Some(Err(error)),
                    };
                    let selection = DesignCombineBodySelection {
                        record_index: *selection_record_index,
                        external_identity,
                    };
                    if additional {
                        additional_tools.push(selection);
                    } else {
                        first_tool = Some(selection);
                    }
                }
            }
        }
        let target = target?;
        let tools = combine::DesignCombineTools {
            first: first_tool?,
            additional: additional_tools,
        };
        Some(Ok(DesignCombineOperation {
            form,
            operation,
            operation_offset: u64::try_from(operation_offset).ok()?,
            keep_tools,
            keep_tools_offset: u64::try_from(keep_tools_offset).ok()?,
            target_record_index: target,
            tools,
        }))
    })()
    .transpose()
}

pub(super) struct ExternalReferenceIdentity {
    pub(super) target: u64,
    pub(super) target_offset: u64,
    pub(super) segment: u32,
    pub(super) segment_offset: u64,
    pub(super) asset_id: crate::records::mesh::DesignRelaxedGuidText,
    pub(super) asset_id_offset: u64,
    pub(super) link_name: String,
    pub(super) link_name_offset: u64,
    pub(super) version: Option<combine::DesignExternalVersion>,
}

pub(super) fn take_external_reference_identity(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<Option<ExternalReferenceIdentity>, CodecError> {
    (|| {
        if bytes.get(*cursor) != Some(&1) {
            return None;
        }
        let target_at = cursor.checked_add(1)?;
        let target = View::u64_le_at(bytes, target_at)?;
        if target == 0 || bytes.get(target_at.checked_add(8)?) != Some(&1) {
            return None;
        }
        let segment_at = target_at.checked_add(9)?;
        let segment = View::u32_le_at(bytes, segment_at)?;
        let asset_at = segment_at.checked_add(4)?;
        let (asset_id, after_asset_id) =
            match lp_utf16_bounded_charged(ctx, bytes, asset_at, 1..=256, "f3d Design UTF-16 text")
            {
                Ok(Some(value)) => value,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
        let asset_id = crate::records::mesh::DesignRelaxedGuidText::try_from(asset_id).ok()?;
        if bytes.get(after_asset_id) != Some(&0) {
            return None;
        }
        let link_name_at = after_asset_id.checked_add(1)?;
        let (link_name, after_link_name) = match lp_utf16_bounded_charged(
            ctx,
            bytes,
            link_name_at,
            1..=256,
            "f3d Design UTF-16 text",
        ) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let (version, end) = match bytes.get(after_link_name)? {
            0 => (None, after_link_name.checked_add(1)?),
            1 => {
                let property_key_at = after_link_name.checked_add(1)?;
                let (property_key, after_property_key) = match lp_utf16_bounded_charged(
                    ctx,
                    bytes,
                    property_key_at,
                    1..=256,
                    "f3d Design UTF-16 text",
                ) {
                    Ok(Some(value)) => value,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                let version_urn_at = after_property_key;
                let (version_urn, end) = match lp_utf16_bounded_charged(
                    ctx,
                    bytes,
                    version_urn_at,
                    1..=256,
                    "f3d Design UTF-16 text",
                ) {
                    Ok(Some(value)) => value,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                let property_key =
                    crate::records::mesh::DesignRelaxedGuidText::try_from(property_key).ok()?;
                (
                    Some(combine::DesignExternalVersion {
                        property_key: crate::records::identity::Located {
                            value: property_key,
                            offset: u64::try_from(property_key_at.checked_add(4)?).ok()?,
                        },
                        version_urn: crate::records::identity::Located {
                            value: version_urn,
                            offset: u64::try_from(version_urn_at.checked_add(4)?).ok()?,
                        },
                    }),
                    end,
                )
            }
            _ => return None,
        };
        *cursor = end;
        Some(Ok(ExternalReferenceIdentity {
            target,
            target_offset: u64::try_from(target_at).ok()?,
            segment,
            segment_offset: u64::try_from(segment_at).ok()?,
            asset_id,
            asset_id_offset: u64::try_from(asset_at.checked_add(4)?).ok()?,
            link_name,
            link_name_offset: u64::try_from(link_name_at.checked_add(4)?).ok()?,
            version,
        }))
    })()
    .transpose()
}

fn exact_combine_external_body_identity(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    scope_record_index: u32,
    record_index: u32,
) -> Result<Option<DesignCombineExternalBodyIdentity>, CodecError> {
    (|| {
        if bytes.get(
            start + combine_external::ZERO_RUN_14
                ..start + combine_external::NESTED_REFERENCE_MARKER,
        )? != [0; 14]
        {
            return None;
        }
        let mut cursor = start.checked_add(combine_external::NESTED_REFERENCE_MARKER)?;
        let nested = take_reference(bytes, &mut cursor)?;
        if nested.local()?.0 != u64::from(record_index.checked_add(3)?)
            || View::u32_le_at(bytes, cursor)? != 1
        {
            return None;
        }
        cursor = cursor.checked_add(4)?;
        let selector_asset_at = cursor;
        let (selector_asset_id, after_selector_asset_id) = match lp_utf16_bounded_charged(
            ctx,
            bytes,
            selector_asset_at,
            1..=256,
            "f3d Design UTF-16 text",
        ) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let selector_context_at = after_selector_asset_id;
        let (selector_context_id, after_selector_context_id) = match lp_utf16_bounded_charged(
            ctx,
            bytes,
            selector_context_at,
            1..=256,
            "f3d Design UTF-16 text",
        ) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let selector_asset_id =
            crate::records::mesh::DesignRelaxedGuidText::try_from(selector_asset_id).ok()?;
        let selector_context_id =
            crate::records::mesh::DesignRelaxedGuidText::try_from(selector_context_id).ok()?;
        if View::u32_le_at(bytes, after_selector_context_id)? != 2
            || View::u32_le_at(bytes, after_selector_context_id.checked_add(4)?)? != 0
            || View::u32_le_at(bytes, after_selector_context_id.checked_add(8)?)? != 1
        {
            return None;
        }
        cursor = after_selector_context_id.checked_add(12)?;
        let occurrence_reference_at = cursor.checked_add(1)?;
        let occurrence = take_reference(bytes, &mut cursor)?;
        let (occurrence_reference, _) = occurrence.local()?;
        if occurrence_reference == 0 || View::u32_le_at(bytes, cursor)? != 1 {
            return None;
        }
        cursor = cursor.checked_add(4)?;
        let external = match take_external_reference_identity(ctx, bytes, &mut cursor) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if View::u32_le_at(bytes, cursor)? != 9
            || View::u16_le_at(bytes, cursor.checked_add(4)?)? != 2
        {
            return None;
        }
        cursor = cursor.checked_add(6)?;
        let first_tail_value_at = cursor;
        let first_tail_value = View::u64_le_at(bytes, cursor)?;
        cursor = cursor.checked_add(8)?;
        if View::u32_le_at(bytes, cursor)? != 48 {
            return None;
        }
        cursor = cursor.checked_add(4)?;
        let second_tail_value_at = cursor;
        let second_tail_value = View::u64_le_at(bytes, cursor)?;
        cursor = cursor.checked_add(8)?;
        let take_local = |cursor: &mut usize, expected| {
            let reference = take_reference(bytes, cursor)?;
            (reference.local()?.0 == u64::from(expected)).then_some(())
        };
        take_local(&mut cursor, record_index.checked_add(2)?)?;
        if bytes.get(cursor..cursor.checked_add(2)?)? != [0; 2] {
            return None;
        }
        cursor = cursor.checked_add(2)?;
        take_local(&mut cursor, record_index.checked_add(1)?)?;
        if bytes.get(cursor) != Some(&0) {
            return None;
        }
        cursor = cursor.checked_add(1)?;
        take_local(&mut cursor, scope_record_index)?;
        if cursor != paired_at {
            return None;
        }
        let (
            external_property_key,
            external_property_key_offset,
            external_version_urn,
            external_version_urn_offset,
        ) = match external.version {
            Some(version) => (
                Some(version.property_key.value),
                Some(version.property_key.offset),
                Some(version.version_urn.value),
                Some(version.version_urn.offset),
            ),
            None => (None, None, None, None),
        };
        DesignCombineExternalBodyIdentity::try_from(DesignCombineExternalBodyIdentityWire {
            selector_asset_id,
            selector_asset_id_offset: u64::try_from(selector_asset_at.checked_add(4)?).ok()?,
            selector_context_id,
            selector_context_id_offset: u64::try_from(selector_context_at.checked_add(4)?).ok()?,
            occurrence_reference,
            occurrence_reference_offset: u64::try_from(occurrence_reference_at).ok()?,
            external_body_reference: external.target,
            external_body_reference_offset: external.target_offset,
            external_segment: external.segment,
            external_segment_offset: external.segment_offset,
            external_asset_id: external.asset_id,
            external_asset_id_offset: external.asset_id_offset,
            external_link_name: external.link_name,
            external_link_name_offset: external.link_name_offset,
            external_property_key,
            external_property_key_offset,
            external_version_urn,
            external_version_urn_offset,
            tail_values: [first_tail_value, second_tail_value],
            tail_value_offsets: [
                u64::try_from(first_tail_value_at).ok()?,
                u64::try_from(second_tail_value_at).ok()?,
            ],
        })
        .ok()
        .map(Ok)
    })()
    .transpose()
}

#[derive(Clone, Copy)]
enum CombineOperandRole {
    Target,
    Tool,
}

fn combine_operation_identity_role(
    frame: &[u8],
    selection_record_index: u32,
) -> Option<CombineOperandRole> {
    let selection_reference = selection_record_index.to_le_bytes();
    if frame.get(11..21)? == [0; 10]
        && View::u32_le_at(frame, 21)? == 1
        && frame.get(25) == Some(&1)
        && frame.get(26..30)? == selection_reference
        && frame.get(30..36)? == [0; 6]
    {
        return Some(CombineOperandRole::Target);
    }
    if frame.get(11..20)? != [0; 9] || frame.get(20) != Some(&1) || View::u32_le_at(frame, 21)? != 1
    {
        return None;
    }
    let after_property = lp_ascii_literal(frame, 25, b"DcFeatureOperationIdFlag")?;
    let after_property_type = lp_ascii_literal(frame, after_property, b"IntrinsicMetaTypeuint64")?;
    let count_at = after_property_type.checked_add(8)?;
    if View::u32_le_at(frame, count_at)? != 1
        || frame.get(count_at + 4) != Some(&1)
        || frame.get(count_at + 5..count_at + 9)? != selection_reference
        || frame.get(count_at + 9..count_at + 15)? != [0; 6]
    {
        return None;
    }
    Some(CombineOperandRole::Tool)
}

fn lp_ascii_literal(bytes: &[u8], at: usize, literal: &[u8]) -> Option<usize> {
    if View::u32_le_at(bytes, at)? != u32::try_from(literal.len()).ok()? {
        return None;
    }
    let start = at.checked_add(4)?;
    let end = start.checked_add(literal.len())?;
    (bytes.get(start..end)? == literal).then_some(end)
}
