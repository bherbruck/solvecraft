// SPDX-License-Identifier: Apache-2.0
//! Parse Design segment metadata and the ordered feature timeline.

use crate::bytes::lp_utf16_bounded_charged;
use cadmpeg_core::container::{ContainerEntry, ContainerRole};

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::{take_reference, Reference};
use crate::container::ContainerScan;
use crate::design::decode::text::design_record_id_charged;
use crate::ids::native_stream;
use crate::records::{
    entity_header::{DesignFeatureTimeline, SegmentType, SegmentTypeData, DESIGN_MODULE_FUSION},
    recipes::DesignComponentNamingSpace,
};

const COMPONENT_MODULE: &str = "Component";
const COMPONENT_NAMING_SPACE_BASE_TYPE_GUID: &str = "21F379C8-CAFD-4985-B461-767673A4C502";
const COMPONENT_UUID_RESERVED_LENGTHS: [usize; 2] = [2, 3];

/// Stable Design type identity of the record that owns the ordered feature
/// scope list.
pub(crate) const FEATURE_TIMELINE_TYPE_GUID: &str = "2F4C1849-1A5A-4F6C-A086-8DD445CBF94B";
pub(crate) const FEATURE_TIMELINE_BASE_TYPE_GUID: &str = "98542EB9-A4F2-4137-A808-DBB5B3CD6159";
pub(crate) const FEATURE_TIMELINE_TYPE_VERSIONS: [u32; 2] = [2, 3];

/// Whether a type-table row has the exact registration metadata of a supported
/// feature-timeline frame.
pub(crate) fn is_supported_feature_timeline_type(design_type: &SegmentTypeData) -> bool {
    FEATURE_TIMELINE_TYPE_VERSIONS.contains(&design_type.version)
        && design_type.module == DESIGN_MODULE_FUSION
        && design_type
            .base_type_guid
            .value()
            .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
            .is_some_and(|base| base.eq_ignore_ascii_case(FEATURE_TIMELINE_BASE_TYPE_GUID))
}

struct MetaStreamEntry<'a> {
    entry: &'a ContainerEntry,
    prefix: &'a str,
}

impl<'a> MetaStreamEntry<'a> {
    fn from_design_entry(scan: &ContainerScan, entry: &'a ContainerEntry) -> Option<Self> {
        if !scan.is_design_stream(entry, ContainerRole::Metastream) {
            return None;
        }
        Some(Self {
            entry,
            prefix: entry.name.strip_suffix("MetaStream.dat")?,
        })
    }
}

fn paired_bulk_entry_name<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
    prefix: &str,
) -> Result<&'a str, CodecError> {
    if let Some(entry) = scan
        .entries
        .iter()
        .find(|entry| entry.name.strip_prefix(prefix) == Some("BulkStream.dat"))
    {
        return Ok(&entry.name);
    }
    let length = "entry "
        .len()
        .checked_add(prefix.len())
        .and_then(|length| length.checked_add("BulkStream.dat not found".len()))
        .ok_or_else(|| ctx.refuse_codec_limit("f3d missing design bulk name length", 0, 1))?;

    let mut message = ctx.retained_string(length, "f3d missing Design BulkStream error")?;
    message.push_str("entry ");
    message.push_str(prefix);
    message.push_str("BulkStream.dat not found");
    Err(CodecError::Malformed(message))
}

/// Decode the type table of every Design `MetaStream` entry.
pub(crate) fn decode_types(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<SegmentType>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter_map(|entry| MetaStreamEntry::from_design_entry(scan, entry))
    {
        let meta = scan.parsed_metastream(ctx, &entry.entry.name)?;
        for design_type in &meta.types {
            ctx.reserve_vec(&mut out, 1, "f3d design type table")?;
            out.push(copy_design_type(ctx, design_type, &entry.entry.name)?);
        }
    }
    Ok(out)
}

fn copy_design_type(
    ctx: &DecodeContext<'_>,
    design_type: &SegmentTypeData,
    stream: &str,
) -> Result<SegmentType, CodecError> {
    use crate::records::identity::ReferenceRun;

    let count = design_type.entities.values().len();

    let entities = if let Some(rows) = design_type.entities.located_rows() {
        let mut copied = Vec::new();
        ctx.reserve_vec(&mut copied, count, "f3d design type registered entities")?;
        copied.extend_from_slice(rows);
        ReferenceRun::located(copied)
    } else {
        let mut copied = ctx.collection_vec(count, "f3d design type registered entities")?;
        copied.extend(design_type.entities.values().copied());
        ReferenceRun::unlocated(copied)
    };
    let module = String::from_utf8(
        ctx.copy_retained(design_type.module.as_bytes(), "f3d design type module")?,
    )
    .map_err(|_| CodecError::Malformed("F3D Design module text is invalid UTF-8".into()))?;
    let id = design_record_id_charged(
        ctx,
        stream,
        ":design-type#",
        design_type.byte_offset,
        "f3d design type id suffix",
    )?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(id.len())
            .checked_mul(8)
            .ok_or_else(|| ctx.refuse_codec_limit("admit F3D type identity", 0, u64::MAX))?,
        "admit F3D type identity",
    )?;
    SegmentType::try_new(
        id,
        crate::records::entity_header::SegmentTypeData {
            byte_offset: design_type.byte_offset,
            type_guid: design_type.type_guid.clone(),
            type_guid_offset: design_type.type_guid_offset,
            base_type_guid: design_type.base_type_guid.clone(),
            version: design_type.version,
            version_offset: design_type.version_offset,
            module,
            entities,
        },
    )
    .map_err(CodecError::Malformed)
}

fn insert_component_naming_space(
    ctx: &DecodeContext<'_>,
    by_component: &mut HashMap<u64, DesignComponentNamingSpace>,
    bulk_name: &str,
    marker: usize,
    component_record_index: u64,
    context_uuid: crate::records::mesh::DesignRelaxedGuidText,
    context_uuid_offset: usize,
) -> Result<(), CodecError> {
    if let Some(existing) = by_component.get(&component_record_index) {
        if existing.context_uuid != context_uuid {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                "Design component {component_record_index} has conflicting context UUID bindings"
            ),
            ));
        }
        return Ok(());
    }

    ctx.reserve_map(by_component, 1, "f3d component naming spaces by entity")?;
    let byte_offset = u64::try_from(marker)
        .map_err(|_| ctx.refuse_codec_limit("f3d component naming marker offset", 0, 1))?;
    let id = design_record_id_charged(
        ctx,
        bulk_name,
        ":design-component-naming-space#",
        byte_offset,
        "f3d component naming space id suffix",
    )?;
    let context_uuid_offset = u64::try_from(context_uuid_offset)
        .map_err(|_| ctx.refuse_codec_limit("f3d component naming UUID offset", 0, 1))?;
    by_component.insert(
        component_record_index,
        DesignComponentNamingSpace {
            id,
            byte_offset,
            component_record_index,
            context_uuid,
            context_uuid_offset,
        },
    );
    Ok(())
}

/// Decode each component entity's UUID-bound local naming space.
pub(crate) fn decode_component_naming_spaces(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<DesignComponentNamingSpace>, CodecError> {
    let mut out = Vec::new();
    for meta_entry in scan
        .entries
        .iter()
        .filter_map(|entry| MetaStreamEntry::from_design_entry(scan, entry))
    {
        let meta = scan.parsed_metastream(ctx, &meta_entry.entry.name)?;
        let mut component_entities = HashSet::new();
        for design_type in meta.types.iter().filter(|design_type| {
            design_type.module == COMPONENT_MODULE
                && design_type
                    .base_type_guid
                    .value()
                    .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
                    .is_some_and(|base| {
                        base.eq_ignore_ascii_case(COMPONENT_NAMING_SPACE_BASE_TYPE_GUID)
                    })
        }) {
            for &entity_id in design_type.entities.values() {
                ctx.insert_hash_set(
                    &mut component_entities,
                    entity_id,
                    "f3d component naming registered entities",
                )?;
            }
        }
        if component_entities.is_empty() {
            continue;
        }
        let bulk_name = paired_bulk_entry_name(ctx, scan, meta_entry.prefix)?;
        let bytes = scan.entry_bytes(bulk_name)?;
        let mut by_component = HashMap::<u64, DesignComponentNamingSpace>::new();
        for reserved_len in COMPONENT_UUID_RESERVED_LENGTHS {
            let prefix_len = 1 + 8 + reserved_len;
            for uuid_offset in bytes
                .len()
                .checked_sub(4)
                .into_iter()
                .flat_map(|last| prefix_len..last)
            {
                let marker = uuid_offset - prefix_len;
                if bytes[marker] != 1
                    || (marker > 0 && bytes[marker - 1] == 1)
                    || !bytes[marker + 9..uuid_offset].iter().all(|byte| *byte == 0)
                {
                    continue;
                }
                let Some(component_record_index) = View::u64_le_at(bytes, marker + 1) else {
                    continue;
                };
                if !component_entities.contains(&component_record_index) {
                    continue;
                }
                let Some((context_uuid, _)) = lp_utf16_bounded_charged(
                    ctx,
                    bytes,
                    uuid_offset,
                    36..=36,
                    "f3d Design UTF-16 text",
                )?
                else {
                    continue;
                };
                let Ok(context_uuid) =
                    crate::records::mesh::DesignRelaxedGuidText::try_from(context_uuid)
                else {
                    continue;
                };
                insert_component_naming_space(
                    ctx,
                    &mut by_component,
                    bulk_name,
                    marker,
                    component_record_index,
                    context_uuid,
                    uuid_offset,
                )?;
            }
        }
        for marker in 0..bytes.len() {
            let mut uuid_offset = marker;
            let Some(reference) = take_reference(bytes, &mut uuid_offset) else {
                continue;
            };
            let Some((component_record_index, Some(inline_type_guid))) = reference.local() else {
                continue;
            };
            if !meta.types.iter().any(|design_type| {
                design_type.module == COMPONENT_MODULE
                    && design_type
                        .base_type_guid
                        .value()
                        .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
                        .is_some_and(|base| {
                            base.eq_ignore_ascii_case(COMPONENT_NAMING_SPACE_BASE_TYPE_GUID)
                        })
                    && design_type
                        .type_guid
                        .as_str()
                        .eq_ignore_ascii_case(inline_type_guid)
                    && design_type
                        .entities
                        .values()
                        .any(|registered| *registered == component_record_index)
            }) {
                continue;
            }
            let Some((context_uuid, _)) = lp_utf16_bounded_charged(
                ctx,
                bytes,
                uuid_offset,
                36..=36,
                "f3d Design UTF-16 text",
            )?
            else {
                continue;
            };
            let Ok(context_uuid) =
                crate::records::mesh::DesignRelaxedGuidText::try_from(context_uuid)
            else {
                continue;
            };
            insert_component_naming_space(
                ctx,
                &mut by_component,
                bulk_name,
                marker,
                component_record_index,
                context_uuid,
                uuid_offset,
            )?;
        }
        if let Some(missing) = component_entities
            .iter()
            .filter(|entity| !by_component.contains_key(entity))
            .min()
        {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!("Design component {missing} has no context UUID binding"),
            ));
        }

        ctx.reserve_vec(
            &mut out,
            by_component.len(),
            "f3d component naming spaces output",
        )?;
        out.extend(by_component.into_values());
    }
    ctx.stable_sort_by(
        &mut out[..],
        |a, b| a.id.cmp(&b.id),
        |value| value.id.as_str().len(),
        "sort f3d design meta 1",
    )?;
    Ok(out)
}

/// Parse the `MetaStream` paired with one Design `BulkStream`.
pub(crate) fn metadata_for_bulk_stream(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    bulk_entry_name: &str,
) -> Result<Option<std::rc::Rc<crate::metastream::MetaStream>>, CodecError> {
    let prefix = bulk_entry_name
        .strip_suffix("BulkStream.dat")
        .ok_or_else(|| CodecError::Malformed("Design stream has no BulkStream suffix".into()))?;
    let Some(meta_entry) = scan
        .entries
        .iter()
        .find(|entry| entry.name.strip_prefix(prefix) == Some("MetaStream.dat"))
    else {
        return Ok(None);
    };
    scan.parsed_metastream(ctx, &meta_entry.name).map(Some)
}

/// One live Design record selected by the primary index and resolved through
/// its segment-local class tag.
#[derive(Clone)]
pub(in crate::design::decode) struct DesignPrimaryFrame<'a> {
    pub(super) entity_id: u64,
    pub(super) class_tag: crate::records::references::DesignClassTag,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) design_type: &'a SegmentTypeData,
}

fn dynamic_type<'a>(
    meta: &'a crate::metastream::MetaStream,
    class_tag: &crate::records::references::DesignClassTag,
) -> Option<(usize, &'a SegmentTypeData)> {
    let ordinal = class_tag.as_str().parse::<usize>().ok()?.checked_sub(256)?;
    Some((ordinal, meta.types.get(ordinal)?))
}

fn record_header_class_tag(
    bytes: &[u8],
    at: usize,
    end: usize,
    expected_entity_id: u64,
) -> Option<crate::records::references::DesignClassTag> {
    let (class_tag, after_tag) = lp_ascii_filtered_view(bytes, at, 3..=3, u8::is_ascii_digit)?;
    let indexed_matches = after_tag
        .checked_add(4)
        .filter(|entity_end| *entity_end <= end)
        .and_then(|_| View::u32_le_at(bytes, after_tag))
        .is_some_and(|entity_id| u64::from(entity_id) == expected_entity_id);
    let named_matches = after_tag
        .checked_add(8)
        .filter(|entity_end| *entity_end <= end)
        .and_then(|_| View::u64_le_at(bytes, after_tag))
        == Some(expected_entity_id);
    if !indexed_matches && !named_matches {
        return None;
    }
    crate::design::decode::text::class_tag_from_view(class_tag).ok()
}

/// Resolve every live sibling record from the primary index. The primary
/// header must repeat the indexed entity ID and select a type-table row that
/// registers that entity. A secondary entry supplies the exact end of the
/// primary class-member sequence and must point to a nested header for the same
/// entity.
pub(super) fn design_primary_frames<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &'a crate::metastream::MetaStream,
) -> Result<Vec<DesignPrimaryFrame<'a>>, CodecError> {
    let indexed = crate::metastream::primary_record_frames(ctx, meta, bytes.len())?;
    let mut registered_entities = HashSet::new();
    for (ordinal, design_type) in meta.types.iter().enumerate() {
        for &entity_id in design_type.entities.values() {
            ctx.insert_hash_set(
                &mut registered_entities,
                (ordinal, entity_id),
                "f3d registered primary entities",
            )?;
        }
    }
    let mut frames = Vec::new();
    ctx.reserve_vec(&mut frames, indexed.len(), "f3d design primary frames")?;
    for frame in indexed {
        let entity_id = frame.entity_id;
        let Some(class_tag) = record_header_class_tag(bytes, frame.start, frame.end, entity_id)
        else {
            return Err(CodecError::Malformed(
                "F3D primary record index points to an invalid record header".into(),
            ));
        };
        let Some((type_ordinal, design_type)) = dynamic_type(meta, &class_tag) else {
            return Err(CodecError::Malformed(
                "F3D primary record class tag is outside its type table".into(),
            ));
        };
        if !registered_entities.contains(&(type_ordinal, entity_id)) {
            return Err(CodecError::Malformed(
                "F3D primary record type does not register its indexed entity ID".into(),
            ));
        }
        if frame.member_end < frame.end {
            let Some(nested_class_tag) =
                record_header_class_tag(bytes, frame.member_end, frame.end, entity_id)
            else {
                return Err(CodecError::Malformed(
                    "F3D secondary record index points to an invalid nested header".into(),
                ));
            };
            if dynamic_type(meta, &nested_class_tag).is_none() {
                return Err(CodecError::Malformed(
                    "F3D secondary record header is incompatible with its primary record".into(),
                ));
            }
        }
        frames.push(DesignPrimaryFrame {
            entity_id,
            class_tag,
            start: frame.start,
            end: frame.end,
            design_type,
        });
    }
    Ok(frames)
}

/// One primary `BulkStream` frame selected through a registered Design type.
#[derive(Clone, Copy)]
pub(super) struct TypedPrimaryFrame<'a> {
    pub(super) entity_id: u64,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) design_type: &'a SegmentTypeData,
}

/// Resolve every entity registered to `type_guid` through the sibling
/// `MetaStream` primary index and verify its dynamic class tag.
pub(super) fn typed_primary_frames<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &'a crate::metastream::MetaStream,
    type_guid: &str,
    record_kind: &str,
) -> Result<Vec<TypedPrimaryFrame<'a>>, CodecError> {
    let mut typed_entities = HashSet::new();
    for design_type in &meta.types {
        if !design_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case(type_guid)
        {
            continue;
        }
        for &entity_id in design_type.entities.values() {
            if typed_entities.contains(&entity_id) {
                return Err(crate::design::text::malformed_design(
                    ctx,
                    format_args!(
                        "F3D Design {record_kind} entity {entity_id} is registered more than once"
                    ),
                ));
            }

            ctx.reserve_set(&mut typed_entities, 1, "f3d typed primary entities")?;
            typed_entities.insert(entity_id);
        }
    }

    let mut resolved_entities = HashSet::new();
    let mut frames = Vec::new();
    for primary_frame in design_primary_frames(ctx, bytes, meta)? {
        if !primary_frame
            .design_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case(type_guid)
        {
            continue;
        }
        if !resolved_entities.contains(&primary_frame.entity_id) {
            ctx.reserve_set(&mut resolved_entities, 1, "f3d resolved primary entities")?;
            resolved_entities.insert(primary_frame.entity_id);
        }

        ctx.reserve_vec(&mut frames, 1, "f3d typed primary frames")?;
        frames.push(TypedPrimaryFrame {
            entity_id: primary_frame.entity_id,
            start: primary_frame.start,
            end: primary_frame.end,
            design_type: primary_frame.design_type,
        });
    }
    if let Some(entity_id) = typed_entities.difference(&resolved_entities).min() {
        return Err(crate::design::text::malformed_design(ctx, format_args!(
            "F3D Design {record_kind} entity {entity_id} has no primary record of its registered class"
        )));
    }
    Ok(frames)
}

/// Type GUID and record version keyed by the Design entity ids that carry the
/// type in the sibling `BulkStream`.
pub(super) fn stream_types_by_entity<'a>(
    ctx: &DecodeContext<'_>,
    types: &'a [SegmentType],
    bulk_entry_name: &str,
) -> Result<HashMap<u64, (&'a str, u32)>, CodecError> {
    let mut by_entity = HashMap::new();
    for design_type in types.iter().filter(|design_type| {
        native_stream(design_type.id())
            .is_some_and(|scope| meta_scope_matches_bulk(scope, bulk_entry_name))
    }) {
        for &entity_id in design_type.entities.values() {
            if !by_entity.contains_key(&entity_id) {
                ctx.reserve_map(&mut by_entity, 1, "f3d stream types by entity")?;
            }
            by_entity.insert(
                entity_id,
                (design_type.type_guid.as_str(), design_type.version),
            );
        }
    }
    Ok(by_entity)
}

/// Complete type-table row keyed by the segment-local dynamic class tag.
pub(super) fn stream_types_by_class_tag<'a>(
    ctx: &DecodeContext<'_>,
    types: &'a [SegmentType],
    bulk_entry_name: &str,
) -> Result<HashMap<u32, &'a SegmentType>, CodecError> {
    let mut by_class_tag = HashMap::new();
    for (ordinal, design_type) in types
        .iter()
        .filter(|design_type| {
            native_stream(design_type.id())
                .is_some_and(|scope| meta_scope_matches_bulk(scope, bulk_entry_name))
        })
        .enumerate()
    {
        let Some(class_tag) = u32::try_from(ordinal)
            .ok()
            .and_then(|ordinal| ordinal.checked_add(256))
        else {
            continue;
        };

        ctx.reserve_map(&mut by_class_tag, 1, "f3d stream types by class tag")?;
        by_class_tag.insert(class_tag, design_type);
    }
    Ok(by_class_tag)
}

/// Compare an encoded native `MetaStream` scope with the `BulkStream`'s sibling
/// name without materializing either name.
fn meta_scope_matches_bulk(scope: &str, bulk_entry_name: &str) -> bool {
    let Some(prefix) = bulk_entry_name.strip_suffix("BulkStream.dat") else {
        return false;
    };
    let Some(encoded) = scope
        .strip_prefix("f3d:")
        .and_then(|scope| scope.strip_suffix("MetaStream.dat"))
    else {
        return false;
    };
    let mut observed = encoded.bytes();
    for character in prefix.chars() {
        let mut buffer = [0; 4];
        let bytes = character.encode_utf8(&mut buffer).as_bytes();
        if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            for byte in bytes {
                if observed.next() != Some(b'%')
                    || observed.next() != Some(HEX[usize::from(byte >> 4)])
                    || observed.next() != Some(HEX[usize::from(byte & 0x0f)])
                {
                    return false;
                }
            }
        } else if !bytes.iter().all(|byte| observed.next() == Some(*byte)) {
            return false;
        }
    }
    observed.next().is_none()
}

fn local_reference(
    reference: &Reference<&str, crate::bytes::utf16::Utf16View<'_>>,
    type_guids_by_entity: &HashMap<u64, Vec<&str>>,
) -> Option<u64> {
    let (target, inline_type_guid) = reference.local()?;
    if let Some(inline_type_guid) = inline_type_guid {
        let registered_type_guids = type_guids_by_entity.get(&target)?;
        if !registered_type_guids
            .iter()
            .any(|registered| registered.eq_ignore_ascii_case(inline_type_guid))
        {
            return None;
        }
    }
    Some(target)
}

fn parse_feature_timeline_record(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    stream: &str,
    frame: std::ops::Range<usize>,
    expected: (&str, u64),
    source_ordinal: u32,
    type_guids_by_entity: &HashMap<u64, Vec<&str>>,
) -> Result<Option<DesignFeatureTimeline>, CodecError> {
    let (expected_class_tag, expected_entity_id) = expected;
    let Some((
        class_tag,
        mut at,
        context_reference_offset,
        context_record_index,
        item_count_offset,
        count,
    )) = (|| {
        let (start, end) = (frame.start, frame.end);
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
        if class_tag != expected_class_tag
            || View::u64_le_at(bytes, after_tag)? != expected_entity_id
        {
            return None;
        }
        let (_, payload) = lp_ascii_filtered_view(
            bytes,
            after_tag.checked_add(8)?,
            0..=2000,
            u8::is_ascii_graphic,
        )?;
        if bytes.get(payload..payload.checked_add(2)?)? != [0, 0] {
            return None;
        }
        let mut at = payload.checked_add(2)?;
        let context_reference_offset = at.checked_add(1)?;
        let context_record_index = std::num::NonZeroU64::new(local_reference(
            &take_reference(bytes, &mut at)?,
            type_guids_by_entity,
        )?)?;
        let item_count_offset = at;
        let count = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
        at = at.checked_add(4)?;
        if count > end.checked_sub(at)? / 11 {
            return None;
        }
        Some((
            class_tag,
            at,
            context_reference_offset,
            context_record_index,
            item_count_offset,
            count,
        ))
    })()
    else {
        return Ok(None);
    };

    let work = count
        .checked_next_power_of_two()
        .and_then(|power| usize::try_from(power.ilog2()).ok())
        .and_then(|levels| count.checked_mul(levels.checked_add(1)?)?.checked_mul(2))
        .and_then(|units| u64::try_from(units).ok())
        .ok_or_else(|| ctx.refuse_codec_limit("F3D timeline sort work", u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(work, "validate F3D timeline item uniqueness")?;
    let mut items = Vec::new();
    ctx.reserve_vec(&mut items, count, "admit F3D timeline item slots")?;
    for _ in 0..count {
        let Some(target_offset) = at.checked_add(1) else {
            return Ok(None);
        };
        let Some(reference) = take_reference(bytes, &mut at) else {
            return Ok(None);
        };
        let Some(target) = local_reference(&reference, type_guids_by_entity) else {
            return Ok(None);
        };
        let Some(target_offset) = u64::try_from(target_offset).ok() else {
            return Ok(None);
        };
        items.push(crate::records::identity::Located {
            value: target,
            offset: target_offset,
        });
    }
    if at != frame.end {
        return Ok(None);
    }

    let source_start = frame.start;
    let Some(frame_start) = u64::try_from(source_start).ok() else {
        return Ok(None);
    };
    let Some(frame_length) = frame
        .end
        .checked_sub(frame.start)
        .and_then(|value| u64::try_from(value).ok())
    else {
        return Ok(None);
    };
    let Some(context_reference_offset) = u64::try_from(context_reference_offset).ok() else {
        return Ok(None);
    };
    let Some(item_count_offset) = u64::try_from(item_count_offset).ok() else {
        return Ok(None);
    };
    let frame = match crate::records::entity_header::DesignTimelineFrame::new(
        crate::records::admission::RecordAdmission::Charged(ctx),
        frame_start,
        frame_length,
        context_reference_offset,
        item_count_offset,
        items,
    ) {
        Ok(frame) => frame,
        Err(crate::records::entity_header::DesignTimelineFrameError::Resource(error)) => {
            return Err(error);
        }
        Err(crate::records::entity_header::DesignTimelineFrameError::Invalid(_)) => {
            return Ok(None);
        }
    };
    let Some(class_tag) = crate::design::decode::text::class_tag_from_view(class_tag).ok() else {
        return Ok(None);
    };
    let Some(record_index) = std::num::NonZeroU64::new(expected_entity_id) else {
        return Ok(None);
    };
    let id = design_record_id_charged(
        ctx,
        stream,
        ":design-feature-timeline#",
        frame_start,
        "retain F3D timeline identity",
    )?;
    Ok(DesignFeatureTimeline::try_new(
        id,
        frame,
        class_tag,
        record_index,
        source_ordinal,
        context_record_index,
    )
    .ok())
}

/// Decode the exact counted scope list that carries authored feature order.
pub(crate) fn decode_feature_timelines(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<DesignFeatureTimeline>, CodecError> {
    let mut out = Vec::new();
    for meta_entry in scan
        .entries
        .iter()
        .filter_map(|entry| MetaStreamEntry::from_design_entry(scan, entry))
    {
        let meta = crate::metastream::parse(
            ctx,
            scan.entry_bytes(&meta_entry.entry.name)?,
            &meta_entry.entry.name,
        )?;
        let is_timeline_type = |design_type: &SegmentTypeData| {
            design_type
                .type_guid
                .as_str()
                .eq_ignore_ascii_case(FEATURE_TIMELINE_TYPE_GUID)
        };
        if !meta.types.iter().any(is_timeline_type) {
            continue;
        }
        if meta
            .types
            .iter()
            .filter(|design_type| is_timeline_type(design_type))
            .any(|design_type| !FEATURE_TIMELINE_TYPE_VERSIONS.contains(&design_type.version))
        {
            return Err(CodecError::NotImplemented(
                "unsupported Design feature-timeline record version".into(),
            ));
        }
        if meta
            .types
            .iter()
            .filter(|design_type| is_timeline_type(design_type))
            .any(|design_type| !is_supported_feature_timeline_type(design_type))
        {
            return Err(CodecError::Malformed(
                "Design feature-timeline type has incompatible registration metadata".into(),
            ));
        }
        if meta
            .records
            .windows(2)
            .any(|pair| pair[0].bulk_offset >= pair[1].bulk_offset)
        {
            return Err(CodecError::Malformed(
                "Design MetaStream record offsets are not strictly increasing".into(),
            ));
        }
        let bulk_name = paired_bulk_entry_name(ctx, scan, meta_entry.prefix)?;
        let bytes = scan.entry_bytes(bulk_name)?;
        let mut type_guids_by_entity = HashMap::<u64, Vec<&str>>::new();
        for design_type in &meta.types {
            for entity_id in design_type.entities.values() {
                ctx.push_hash_group(
                    &mut type_guids_by_entity,
                    *entity_id,
                    design_type.type_guid.as_str(),
                    "index F3D timeline entity",
                    "index F3D timeline type GUID",
                )?;
            }
        }
        let mut source_ordinal = 0_u32;
        for (type_ordinal, design_type) in meta
            .types
            .iter()
            .enumerate()
            .filter(|(_, design_type)| is_timeline_type(design_type))
        {
            let _class_tag_reservation = ctx.reserve_scoped(3, "format F3D timeline class tag")?;
            let expected_class_tag = u32::try_from(type_ordinal)
                .ok()
                .and_then(|ordinal| ordinal.checked_add(256))
                .filter(|class_tag| *class_tag <= 999)
                .ok_or_else(|| {
                    CodecError::Malformed(
                        "Design feature-timeline class tag is not three digits".into(),
                    )
                })?
                .to_string();
            for entity_id in design_type.entities.values() {
                let entity_source_ordinal = source_ordinal;
                source_ordinal = source_ordinal.checked_add(1).ok_or_else(|| {
                    CodecError::Malformed("Design feature-timeline ordinal exceeds u32".into())
                })?;
                ctx.charge_work(
                    u64::try_from(meta.records.len()).map_err(|_| {
                        ctx.refuse_codec_limit("F3D timeline record search", u64::MAX - 1, u64::MAX)
                    })?,
                    "find F3D timeline primary record",
                )?;
                let mut matches = meta
                    .records
                    .iter()
                    .enumerate()
                    .filter(|(_, record)| record.entity_id == *entity_id);
                let Some((record_ordinal, record)) = matches.next() else {
                    return Err(CodecError::Malformed(
                        "Design feature timeline has no unique primary record-index entry".into(),
                    ));
                };
                if matches.next().is_some() {
                    return Err(CodecError::Malformed(
                        "Design feature timeline has no unique primary record-index entry".into(),
                    ));
                }
                let start = usize::try_from(record.bulk_offset).map_err(|_| {
                    CodecError::Malformed("Design feature-timeline offset exceeds usize".into())
                })?;
                let end = meta.records.get(record_ordinal + 1).map_or_else(
                    || Ok(bytes.len()),
                    |next| {
                        usize::try_from(next.bulk_offset).map_err(|_| {
                            CodecError::Malformed(
                                "Design feature-timeline end exceeds usize".into(),
                            )
                        })
                    },
                )?;
                if start >= end || end > bytes.len() {
                    return Err(CodecError::Malformed(
                        "Design feature-timeline record extent is outside its BulkStream".into(),
                    ));
                }
                let timeline = parse_feature_timeline_record(
                    ctx,
                    bytes,
                    bulk_name,
                    start..end,
                    (&expected_class_tag, *entity_id),
                    entity_source_ordinal,
                    &type_guids_by_entity,
                )?
                .ok_or_else(|| {
                    CodecError::Malformed(
                        "Design feature-timeline record does not match its exact frame".into(),
                    )
                })?;

                ctx.reserve_vec(&mut out, 1, "retain F3D feature timeline")?;
                out.push(timeline);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
