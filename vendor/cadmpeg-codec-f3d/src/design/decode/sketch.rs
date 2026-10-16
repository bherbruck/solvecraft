// SPDX-License-Identifier: Apache-2.0
//! Parse Design sketch placements, headers, relations, and geometry.

use crate::bytes::lp_utf16_bounded_charged;
use cadmpeg_core::decode::u64_from_index;

use crate::records::sketch_placement::{
    DesignSketchFrame, DesignSketchFrameForm, SketchPlacementMatrix,
};

use cadmpeg_core::container::ContainerRole;

use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::{f64s_at, take_reference, Reference};
use crate::container::ContainerScan;
use crate::design::decode::text::design_record_id_charged;

use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::{self, native_stream};
use crate::layout::sketch_container_visibility_member_prefix as visibility_member;
use crate::records::{
    admission::RecordAdmission,
    decal::DesignRecordHeader,
    entity_header::{DesignEntityHeader, DESIGN_MODULE_SKETCH},
    feature::scope::DesignParameterScope,
    references::{LostEdgeReference, PersistentReference, PersistentReferenceKind},
    sketch_geometry::{
        SketchCurveGeometry, SketchCurveIdentity, SketchGeometryError, SketchPoint,
        SketchPointClosure, SketchPointCompanion, SketchPointCompanionReferenceEncoding,
        SketchPointRecordForm, SketchSurface, SketchSurfaceGeometry, SketchText,
        SketchTextAlignment, SketchTextLayout,
    },
    sketch_placement::{DesignSketchPlacement, DesignSketchVisibility},
    sketch_relations::{SketchGlyphTransform, SketchRelation, SketchRelationOperand},
};
use cadmpeg_core::bytes::find_from;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, FiniteReal, NonNegativeLength, NonNegativeReal, PositiveLength};
use cadmpeg_ir::sketches::TextPlacement;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::units::{FinitePoint2, UnitVector3};
use std::collections::HashMap;

use super::meta::{
    decode_types, design_primary_frames, metadata_for_bulk_stream, stream_types_by_class_tag,
};

const EPS_SKETCH_DECODE_LINE_COMPONENTS_E12: f64 = 1.0e-12;

/// Byte offsets of every indexed-record header in one `BulkStream`, grouped by
/// the record index carried at header offset seven.
pub(in crate::design) struct IndexedRecordOffsets {
    by_record_index: HashMap<u32, Vec<usize>>,
}

impl IndexedRecordOffsets {
    /// Index every exact indexed-record header in `bytes` in one forward pass.
    pub(in crate::design) fn build(
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
    ) -> Result<Self, CodecError> {
        let mut by_record_index = HashMap::<u32, Vec<usize>>::new();
        for header in indexed_record_offsets(bytes) {
            ctx.push_hash_group(
                &mut by_record_index,
                header.record_index,
                header.offset,
                "f3d indexed record key",
                "f3d indexed record offset",
            )?;
        }
        Ok(Self { by_record_index })
    }

    /// Ascending header offsets carrying `record_index`.
    pub(in crate::design) fn offsets(&self, record_index: u32) -> &[usize] {
        self.by_record_index
            .get(&record_index)
            .map_or(&[], Vec::as_slice)
    }

    /// Record indexes and their ascending header offsets.
    pub(in crate::design) fn records(&self) -> impl Iterator<Item = (u32, &[usize])> {
        self.by_record_index
            .iter()
            .map(|(record_index, offsets)| (*record_index, offsets.as_slice()))
    }

    /// The first header at or after `position` that carries `record_index`.
    pub(super) fn first_at_or_after(&self, position: usize, record_index: u32) -> Option<usize> {
        let offsets = self.offsets(record_index);
        offsets
            .get(offsets.partition_point(|offset| *offset < position))
            .copied()
    }

    /// Consecutive header offsets carrying `record_index`, each pair delimiting
    /// one frame of that record.
    pub(in crate::design) fn frames(
        &self,
        record_index: u32,
    ) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.offsets(record_index)
            .windows(2)
            .map(|pair| (pair[0], pair[1]))
    }
}

/// Build the retained native stream key with the identity component's percent encoding.
pub(in crate::design) fn native_scope_charged(
    ctx: &DecodeContext<'_>,
    name: &str,
) -> Result<String, CodecError> {
    let encoded_len = native_scope_encoded_len(ctx, name)?;

    let mut out = ctx.retained_string(encoded_len, "f3d native stream key")?;
    append_native_scope(name, &mut out);
    Ok(out)
}

pub(in crate::design) fn native_scope_scoped<'a>(
    ctx: &'a DecodeContext<'_>,
    name: &str,
) -> Result<(cadmpeg_core::decode::ScopedReservation<'a>, String), CodecError> {
    let encoded_len = native_scope_encoded_len(ctx, name)?;

    let mut reservation = ctx.reserve_scoped(0, "f3d scoped native stream key")?;
    let mut out = String::new();
    ctx.reserve_scoped_string(
        &mut reservation,
        &mut out,
        encoded_len,
        "f3d scoped native stream key",
    )?;
    append_native_scope(name, &mut out);
    Ok((reservation, out))
}

fn native_scope_encoded_len(ctx: &DecodeContext<'_>, name: &str) -> Result<usize, CodecError> {
    let encoded_len = name
        .chars()
        .try_fold(ids::SCHEME_PREFIX.len(), |length, character| {
            let character_len = if matches!(character, ':' | '#' | '%') || character.is_whitespace()
            {
                character.len_utf8().checked_mul(3)?
            } else {
                character.len_utf8()
            };
            length.checked_add(character_len)
        })
        .ok_or_else(|| {
            ctx.refuse_codec_limit("f3d native stream key length", u64::MAX - 1, u64::MAX)
        })?;
    Ok(encoded_len)
}

fn append_native_scope(name: &str, out: &mut String) {
    out.push_str(ids::SCHEME_PREFIX);
    for character in name.chars() {
        if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
            let mut bytes = [0; 4];
            for byte in character.encode_utf8(&mut bytes).as_bytes() {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                out.push('%');
                out.push(char::from(HEX[usize::from(byte >> 4)]));
                out.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        } else {
            out.push(character);
        }
    }
}

fn entity_meta_scope<'a>(
    ctx: &'a DecodeContext<'_>,
    bulk_scope: &str,
) -> Result<Option<(cadmpeg_core::decode::ScopedReservation<'a>, String)>, CodecError> {
    let Some(prefix) = bulk_scope.strip_suffix("BulkStream.dat") else {
        return Ok(None);
    };
    let meta_len = prefix
        .len()
        .checked_add("MetaStream.dat".len())
        .ok_or_else(|| ctx.refuse_codec_limit("f3d entity meta scope length", 0, 1))?;
    let mut reservation = ctx.reserve_scoped(0, "f3d entity meta scope")?;
    let mut meta_scope = String::new();
    ctx.reserve_scoped_string(
        &mut reservation,
        &mut meta_scope,
        meta_len,
        "f3d entity meta scope",
    )?;
    meta_scope.push_str(prefix);
    meta_scope.push_str("MetaStream.dat");
    Ok(Some((reservation, meta_scope)))
}

fn clone_sketch_entity_id_charged(
    ctx: &DecodeContext<'_>,
    entity_id: &crate::records::identity::DesignEntityId,
) -> Result<crate::records::identity::DesignEntityId, CodecError> {
    let source = entity_id.as_str();

    let mut text = ctx.retained_string(source.len(), "f3d sketch placement entity ID")?;
    text.push_str(source);
    crate::records::identity::DesignEntityId::try_from(text).map_err(CodecError::Malformed)
}

/// Cache a stream index under its borrowed identity after charging the map slot.
pub(in crate::design) fn cached_borrowed_record_offsets<'a, 's>(
    ctx: &DecodeContext<'_>,
    cache: &'a mut HashMap<&'s str, IndexedRecordOffsets>,
    stream: &'s str,
    bytes: &[u8],
) -> Result<&'a IndexedRecordOffsets, CodecError> {
    if !cache.contains_key(stream) {
        ctx.reserve_map(cache, 1, "f3d indexed stream cache entry")?;
    }
    match cache.entry(stream) {
        std::collections::hash_map::Entry::Occupied(entry) => Ok(entry.into_mut()),
        std::collections::hash_map::Entry::Vacant(entry) => {
            Ok(entry.insert(IndexedRecordOffsets::build(ctx, bytes)?))
        }
    }
}

/// Cache a stream index under owned text after charging its key and map slot.
pub(in crate::design) fn cached_owned_record_offsets<'a>(
    ctx: &DecodeContext<'_>,
    cache: &'a mut HashMap<String, IndexedRecordOffsets>,
    stream: &str,
    bytes: &[u8],
) -> Result<&'a IndexedRecordOffsets, CodecError> {
    if !cache.contains_key(stream) {
        ctx.reserve_map(cache, 1, "f3d indexed stream cache entry")?;
        ctx.charge_retained(u64_from_index(stream.len()), "f3d indexed stream cache key")?;
    }
    let mut key_bytes = ctx.reserve_scoped(0, "f3d indexed stream lookup")?;
    let mut key = String::new();
    ctx.reserve_scoped_string(
        &mut key_bytes,
        &mut key,
        stream.len(),
        "f3d indexed stream lookup",
    )?;
    key.push_str(stream);
    match cache.entry(key) {
        std::collections::hash_map::Entry::Occupied(entry) => Ok(entry.into_mut()),
        std::collections::hash_map::Entry::Vacant(entry) => {
            Ok(entry.insert(IndexedRecordOffsets::build(ctx, bytes)?))
        }
    }
}

/// Decode the unique local-to-model placement frame referenced by every
/// parameter-owning sketch scope, and every member-run head placement. A
/// localized Sketch scope follows its entity container within the same
/// stream interval even though its generic reference table does not repeat
/// the entity suffix.
pub(crate) fn decode_sketch_placements(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignSketchPlacement>, CodecError> {
    let mut out = Vec::new();
    let mut record_offsets = HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let scope = native_scope_charged(ctx, &entry.name)?;
        if !record_offsets.contains_key(&scope) {
            ctx.reserve_map(&mut record_offsets, 1, "f3d sketch placement stream index")?;
        }
        record_offsets.insert(scope, IndexedRecordOffsets::build(ctx, bytes)?);
    }
    let mut visibilities = HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) = metadata_for_bulk_stream(ctx, scan, &entry.name)? else {
            continue;
        };
        let (_scope_reservation, scope) = native_scope_scoped(ctx, &entry.name)?;
        let Some((key, _)) = record_offsets.get_key_value(&scope) else {
            continue;
        };
        for (entity_suffix, visibility) in
            decode_sketch_visibilities_in_stream(ctx, bytes, &metadata)?
        {
            if visibilities.contains_key(&(key.as_str(), entity_suffix)) {
                return Err(crate::design::text::malformed_design(
                    ctx,
                    format_args!(
                        "F3D Design stream {} repeats sketch visibility for entity {entity_suffix}",
                        entry.name
                    ),
                ));
            }

            ctx.reserve_map(
                &mut visibilities,
                1,
                "f3d sketch placement visibility index",
            )?;
            visibilities.insert((key.as_str(), entity_suffix), visibility);
        }
    }
    for scope in scopes
        .iter()
        .filter(|scope| design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Sketch))
    {
        let Some(binding) = scope.sketch_entity() else {
            continue;
        };
        let entity_id = &binding.entity_id;
        let mut entry = None;
        for candidate in scan
            .entries
            .iter()
            .filter(|candidate| scan.is_design_stream(candidate, ContainerRole::Bulkstream))
        {
            let (_reservation, candidate_scope) = native_scope_scoped(ctx, &candidate.name)?;
            if scope
                .id
                .strip_prefix(candidate_scope.as_str())
                .is_some_and(|tail| tail.starts_with(':'))
            {
                entry = Some(candidate);
                break;
            }
        }
        let Some(entry) = entry else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let (_scope_reservation, stream_scope) = native_scope_scoped(ctx, &entry.name)?;
        let Some(records) = record_offsets.get(&stream_scope) else {
            continue;
        };
        let start = usize::try_from(scope.byte_offset()).ok();
        let end = usize::try_from(scope.paired_byte_offset()).ok();
        let Some(frame) = start
            .zip(end)
            .and_then(|(start, end)| bytes.get(start..end))
        else {
            continue;
        };
        let mut referenced_indices = Vec::new();
        for window in frame.windows(11) {
            if window[0] == 1 && window[5..11] == [0; 6] {
                let Some(record_index) = View::u32_le_at(window, 1) else {
                    continue;
                };
                if !referenced_indices.contains(&record_index) {
                    ctx.reserve_vec(
                        &mut referenced_indices,
                        1,
                        "f3d sketch placement reference index",
                    )?;
                    referenced_indices.push(record_index);
                }
            }
        }
        let mut candidates = Vec::new();
        for record_index in referenced_indices {
            let parsed = parse_sketch_placement_candidates(
                ctx,
                bytes,
                scope.record_index,
                entity_id,
                record_index,
                records,
            )?;

            ctx.reserve_vec(
                &mut candidates,
                parsed.len(),
                "f3d sketch placement candidate merge",
            )?;
            candidates.extend(parsed);
        }
        if candidates.len() == 1 {
            let Some(mut placement) = candidates.pop() else {
                continue;
            };
            placement.id = design_record_id_charged(
                ctx,
                &entry.name,
                ":design-sketch-placement#",
                placement.byte_offset(),
                "f3d sketch placement ID",
            )?;

            ctx.reserve_vec(&mut out, 1, "f3d sketch placement output")?;
            out.push(placement);
        }
    }
    // A sketch entity header pairs with a same-index member-run record whose
    // leading marked reference names a head record carrying the row-major
    // 4×4 placement. A localized Sketch scope belongs to the preceding sketch
    // entity interval: it follows that entity and precedes the next sketch
    // entity in the same stream. Some member-run sketches have no scope.
    let initial_placements = out.len();
    for entity in entities.iter().filter(|entity| entity.in_sketch_module()) {
        let Some(stream) = native_stream(&entity.id) else {
            continue;
        };
        ctx.charge_work(
            u64_from_index(initial_placements),
            "f3d sketch placement duplicate search",
        )?;
        if out.iter().take(initial_placements).any(|placement| {
            native_stream(&placement.id) == Some(stream)
                && placement.entity_id.suffix() == entity.entity_id.suffix()
        }) {
            continue;
        }
        let Some(entry_name) = stream.strip_prefix(ids::SCHEME_PREFIX) else {
            continue;
        };
        let bytes = scan.entry_bytes(entry_name)?;
        let Some(records) = record_offsets.get(stream) else {
            continue;
        };
        let Some(mut placement) = parse_member_run_head_placement(
            ctx,
            bytes,
            entity.byte_offset,
            &entity.entity_id,
            records,
        )?
        else {
            continue;
        };
        let next_entity_offset = entities
            .iter()
            .filter(|candidate| {
                candidate.in_sketch_module()
                    && native_stream(&candidate.id) == Some(stream)
                    && candidate.byte_offset > entity.byte_offset
            })
            .map(|candidate| candidate.byte_offset)
            .min();
        let mut matching_scopes = scopes.iter().filter(|scope| {
            design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Sketch)
                && native_stream(&scope.id) == Some(stream)
                && scope.byte_offset() > entity.byte_offset
                && next_entity_offset.is_none_or(|end| scope.byte_offset() < end)
        });
        if let (Some(scope), None) = (matching_scopes.next(), matching_scopes.next()) {
            placement.scope_record_index = Some(scope.record_index);
        }
        placement.id = design_record_id_charged(
            ctx,
            entry_name,
            ":design-sketch-placement#",
            placement.byte_offset(),
            "f3d sketch placement ID",
        )?;

        ctx.reserve_vec(&mut out, 1, "f3d sketch placement output")?;
        out.push(placement);
    }
    for placement in &mut out {
        let Some(stream) = native_stream(&placement.id) else {
            continue;
        };
        placement.visibility = visibilities
            .get(&(stream, placement.entity_id.suffix()))
            .copied();
    }
    ctx.stable_sort_by(
        &mut out[..],
        |a, b| a.id.cmp(&b.id),
        |value| value.id.as_str().len(),
        "sort f3d design sketch 1",
    )?;
    Ok(out)
}

const CURRENT_SKETCH_CONTAINER_VERSION: u32 = 18;
const SKETCH_CONTAINER_MEMBER_TYPE_GUID: &str = "37AD519C-AFB3-4CE2-9E6D-E3269FC6CDB9";
const SKETCH_CONTAINER_MEMBER_BASE_TYPE_GUID: &str = "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081";
const SKETCH_CONTAINER_MEMBER_VERSION: u32 = 4;

/// Decode the direct display flag in every current sketch container's typed
/// Geometry member. Other sketch-container versions do not expose this member
/// layout and therefore leave neutral visibility unknown.
fn decode_sketch_visibilities_in_stream(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    metadata: &crate::metastream::MetaStream,
) -> Result<Vec<(u64, DesignSketchVisibility)>, CodecError> {
    let mut out = Vec::new();
    for frame in super::meta::typed_primary_frames(
        ctx,
        bytes,
        metadata,
        SKETCH_CONTAINER_TYPE_GUID,
        "sketch-container",
    )? {
        if frame.design_type.version != CURRENT_SKETCH_CONTAINER_VERSION {
            continue;
        }
        if frame.design_type.module != DESIGN_MODULE_SKETCH
            || !frame
                .design_type
                .base_type_guid
                .value()
                .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
                .is_some_and(|base| base.eq_ignore_ascii_case(SKETCH_CONTAINER_MEMBER_TYPE_GUID))
        {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D sketch container {} has incompatible registration metadata",
                    frame.entity_id
                ),
            ));
        }
        let Some(NamedEntityHeader {
            entity_id,
            end: header_end,
            ..
        }) = (match parse_settled_entity_header(ctx, &bytes[..frame.end], frame.start)? {
            Some(header) => Some(header),
            None => parse_genesis_entity_header(ctx, &bytes[..frame.end], frame.start)?,
        })
        else {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D sketch container {} has an invalid entity header",
                    frame.entity_id
                ),
            ));
        };
        let entity_suffix = entity_id.suffix();
        if entity_suffix != frame.entity_id {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D sketch container {} disagrees with its entity header {entity_suffix}",
                    frame.entity_id
                ),
            ));
        }
        let Some(member_at) = next_indexed_record_offset(&bytes[..frame.end], header_end) else {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!("F3D sketch container {entity_suffix} has no typed Geometry member"),
            ));
        };
        let Some((class_tag, after_tag)) =
            lp_ascii_filtered_view(bytes, member_at, 3..=3, u8::is_ascii_digit)
        else {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D sketch container {entity_suffix} has an invalid Geometry-member class tag"
                ),
            ));
        };
        let member_type = class_tag
            .parse::<usize>()
            .ok()
            .and_then(|tag| tag.checked_sub(256))
            .and_then(|ordinal| metadata.types.get(ordinal));
        if after_tag != member_at + 7
            || !member_type.is_some_and(|member_type| {
                member_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(SKETCH_CONTAINER_MEMBER_TYPE_GUID)
                    && member_type.version == SKETCH_CONTAINER_MEMBER_VERSION
                    && member_type.module == "Geometry"
                    && member_type
                        .base_type_guid
                        .value()
                        .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
                        .is_some_and(|base| {
                            base.eq_ignore_ascii_case(SKETCH_CONTAINER_MEMBER_BASE_TYPE_GUID)
                        })
            })
        {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D sketch container {entity_suffix} has an incompatible Geometry member"
                ),
            ));
        }
        let Some(visibility) =
            decode_sketch_visibility_member(&bytes[..frame.end], member_at, entity_suffix)
        else {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "F3D sketch container {entity_suffix} has an invalid visibility member"
                ),
            ));
        };

        ctx.reserve_vec(&mut out, 1, "f3d sketch visibility records")?;
        out.push((entity_suffix, visibility));
    }
    Ok(out)
}

fn decode_sketch_visibility_member(
    bytes: &[u8],
    member_at: usize,
    entity_suffix: u64,
) -> Option<DesignSketchVisibility> {
    let record_index = View::u64_le_at(bytes, member_at + visibility_member::ENTITY_SUFFIX)?;
    if record_index != entity_suffix
        || bytes.get(
            member_at + visibility_member::ZERO_RUN..member_at + visibility_member::OWNER_REFERENCE,
        ) != Some(&[0; 4])
    {
        return None;
    }
    let mut cursor = member_at + visibility_member::OWNER_REFERENCE;
    let owner = take_reference(bytes, &mut cursor)?;
    if cursor != member_at + visibility_member::STREAM_ORDINAL
        || !matches!(owner.local(), Some((target, None)) if target != 0)
    {
        return None;
    }
    let stream_ordinal = std::num::NonZeroU32::new(View::u32_le_at(bytes, cursor)?)?;
    if bytes.get(member_at + visibility_member::RESERVED_ZERO) != Some(&0) {
        return None;
    }
    let stream_ordinal_offset = cursor;
    let visible_offset = member_at + visibility_member::VISIBLE;
    let visible = match bytes.get(visible_offset) {
        Some(0) => false,
        Some(1) => true,
        _ => return None,
    };
    if bytes.get(member_at + visibility_member::TAIL_MARKER) != Some(&1) {
        return None;
    }
    DesignSketchVisibility::new(
        stream_ordinal,
        u64_from_index(stream_ordinal_offset),
        visible,
    )
    .ok()
}

/// Byte length of a member-run head carrying an explicit 4×4 transform.
const MEMBER_RUN_HEAD_FRAME: usize = 162;

/// Parse a member-run head placement: the paired same-index record after the
/// sketch's entity header opens with a marked
/// reference naming a head record. A 34-byte head denotes the identity
/// placement. A 162-byte head stores eleven zero bytes and the row-major 4×4
/// local-to-model transform at offset 22.
fn parse_member_run_head_placement(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    entity_byte_offset: u64,
    entity_id: &crate::records::identity::DesignEntityId,
    records: &IndexedRecordOffsets,
) -> Result<Option<DesignSketchPlacement>, CodecError> {
    let parsed = (|| {
        let start = usize::try_from(entity_byte_offset).ok()?;
        // Locate the paired same-index record after the entity header.
        let entity_index = u32::try_from(entity_id.suffix()).ok()?;
        let paired_at = records.first_at_or_after(start.checked_add(1)?, entity_index)?;
        let (paired_class_tag, paired_after_tag) =
            lp_ascii_filtered_view(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
        let paired_class_tag =
            crate::design::decode::text::class_tag_from_view(paired_class_tag).ok()?;
        // The paired record's prologue: the u32 index, zero bytes to offset 19,
        // then a marked u64 reference naming the head record.
        if paired_after_tag != paired_at + 7
            || bytes.get(paired_at + 11..paired_at + 19) != Some(&[0u8; 8][..])
            || bytes.get(paired_at + 19) != Some(&1)
        {
            return None;
        }
        let head_index = View::u32_le_at(bytes, paired_at + 20)?;
        if bytes.get(paired_at + 24..paired_at + 28) != Some(&[0u8; 4][..]) {
            return None;
        }
        // Locate the head record and decode its transform.
        let head_at = records.offsets(head_index).first().copied()?;
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, head_at, 0..=2000, u8::is_ascii_graphic)?;
        let class_tag = crate::design::decode::text::class_tag_from_view(class_tag).ok()?;
        if after_tag != head_at + 7 {
            return None;
        }
        let head_end = next_indexed_record_offset(bytes, head_at + 11).unwrap_or(bytes.len());
        let frame_length = head_end.checked_sub(head_at)?;
        let form = match frame_length {
            34 if bytes.get(head_at + 11..head_at + 21) == Some(&[0u8; 10][..])
                && bytes.get(head_at + 21..head_at + 24) == Some(&[1, 0, 1][..])
                && bytes.get(head_at + 28..head_at + 34) == Some(&[0u8; 6][..]) =>
            {
                DesignSketchFrameForm::MemberCompact {
                    paired_byte_offset: u64_from_index(paired_at),
                }
            }
            MEMBER_RUN_HEAD_FRAME
                if bytes.get(head_at + 11..head_at + 22) == Some(&[0u8; 11][..]) =>
            {
                let values = f64s_at::<16>(bytes, head_at + 22)?;
                let mut transform = [[0.0; 4]; 4];
                for (ordinal, value) in values.iter().copied().enumerate() {
                    transform[ordinal / 4][ordinal % 4] = value;
                }
                if bytes.get(head_at + 150..head_at + 152) != Some(&[0, 1][..]) {
                    return None;
                }
                DesignSketchFrameForm::MemberExplicit {
                    paired_byte_offset: u64_from_index(paired_at),
                    transform: SketchPlacementMatrix::try_from(transform).ok()?,
                }
            }
            _ => return None,
        };
        Some((head_at, form, class_tag, paired_class_tag, head_index))
    })();
    let Some((head_at, form, class_tag, paired_class_tag, head_index)) = parsed else {
        return Ok(None);
    };
    let Ok(frame) = DesignSketchFrame::new(u64_from_index(head_at), form) else {
        return Ok(None);
    };
    Ok(Some(DesignSketchPlacement {
        id: String::new(),
        scope_record_index: None,
        entity_id: clone_sketch_entity_id_charged(ctx, entity_id)?,

        visibility: None,
        frame,
        class_tag,
        record_index: head_index,
        paired_class_tag,
    }))
}

fn parse_sketch_placement_candidates(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope_record_index: u32,
    entity_id: &crate::records::identity::DesignEntityId,
    record_index: u32,
    records: &IndexedRecordOffsets,
) -> Result<Vec<DesignSketchPlacement>, CodecError> {
    let mut out = Vec::new();
    for pair in records.offsets(record_index).windows(2) {
        let start = pair[0];
        let paired_at = pair[1];
        let Some(frame_length) = paired_at.checked_sub(start) else {
            continue;
        };
        if !matches!(frame_length, 201 | 213 | 305 | 325 | 329 | 341) {
            continue;
        }
        let Some((class_tag, after_tag)) =
            lp_ascii_filtered_view(bytes, start, 0..=2000, u8::is_ascii_graphic)
        else {
            continue;
        };
        let Some((paired_class_tag, paired_after_tag)) =
            lp_ascii_filtered_view(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)
        else {
            continue;
        };
        if after_tag != start + 7
            || paired_after_tag != paired_at + 7
            || View::u32_le_at(bytes, paired_after_tag) != Some(record_index)
        {
            continue;
        }
        let Ok(class_tag) = crate::design::decode::text::class_tag_from_view(class_tag) else {
            continue;
        };
        let Ok(paired_class_tag) =
            crate::design::decode::text::class_tag_from_view(paired_class_tag)
        else {
            continue;
        };
        let form = match frame_length {
            201 => DesignSketchFrameForm::ScopeCompact,
            305 | 325 => {
                let Some(values) = f64s_at::<16>(bytes, start + 48) else {
                    continue;
                };
                let mut transform = [[0.0; 4]; 4];
                for (ordinal, value) in values.iter().copied().enumerate() {
                    transform[ordinal / 4][ordinal % 4] = value;
                }
                let Ok(transform) = SketchPlacementMatrix::try_from(transform) else {
                    continue;
                };
                if frame_length == 305 {
                    DesignSketchFrameForm::ScopeLegacy305(transform)
                } else {
                    DesignSketchFrameForm::ScopeLegacy325(transform)
                }
            }
            329 => {
                let Some(values) = f64s_at::<16>(bytes, start + 55) else {
                    continue;
                };
                let mut transform = [[0.0; 4]; 4];
                for (ordinal, value) in values.iter().copied().enumerate() {
                    transform[ordinal / 4][ordinal % 4] = value;
                }
                let Ok(transform) = SketchPlacementMatrix::try_from(transform) else {
                    continue;
                };
                DesignSketchFrameForm::ScopeExplicit(transform)
            }
            // The `EntityGenesis`-flavor frame: `0x01` at offset 55, nine
            // zero bytes, and a form byte at offset 65. Form `0x01` is the
            // identity transform; form `0x00` is followed by the row-major
            // 4×4 f64 matrix at offset 66. The WorkPlane sibling of this
            // record class carries a marked record reference at offset 57
            // and fails the zero-run check.
            213 | 341 => {
                if bytes.get(start + 55) != Some(&1)
                    || bytes.get(start + 56..start + 65) != Some(&[0u8; 9][..])
                {
                    continue;
                }
                match (frame_length, bytes.get(start + 65)) {
                    (213, Some(&1)) => DesignSketchFrameForm::ScopeGenesisCompact,
                    (341, Some(&0)) => {
                        let Some(values) = f64s_at::<16>(bytes, start + 66) else {
                            continue;
                        };
                        let mut transform = [[0.0; 4]; 4];
                        for (ordinal, value) in values.iter().copied().enumerate() {
                            transform[ordinal / 4][ordinal % 4] = value;
                        }
                        let Ok(transform) = SketchPlacementMatrix::try_from(transform) else {
                            continue;
                        };
                        DesignSketchFrameForm::ScopeGenesisExplicit(transform)
                    }
                    _ => continue,
                }
            }
            _ => continue,
        };
        let Ok(frame) = DesignSketchFrame::new(u64_from_index(start), form) else {
            continue;
        };

        ctx.reserve_vec(&mut out, 1, "f3d sketch placement candidate")?;
        out.push(DesignSketchPlacement {
            id: String::new(),
            scope_record_index: Some(scope_record_index),
            entity_id: clone_sketch_entity_id_charged(ctx, entity_id)?,

            visibility: None,
            frame,
            class_tag,
            record_index,
            paired_class_tag,
        });
    }
    Ok(out)
}

/// Decode the persistent u64 point and curve identity references
/// (`pt_tag`, `crv_primary_id`, `crv_secondary_id`, each typed
/// `IntrinsicMetaTypeuint64`) from every design `BulkStream` entry in `scan`,
/// sorted by stream offset.
pub(crate) fn decode_persistent_references(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<PersistentReference>, CodecError> {
    let mut out = Vec::new();
    for (entry_ordinal, entry) in scan
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        decode_persistent_references_from_stream(ctx, entry_ordinal, &entry.name, bytes, &mut out)?;
    }
    finish_persistent_references(ctx, out)
}

fn finish_persistent_references(
    ctx: &DecodeContext<'_>,
    mut out: Vec<(usize, PersistentReference)>,
) -> Result<Vec<PersistentReference>, CodecError> {
    ctx.stable_sort_by(
        &mut out[..],
        |left, right| {
            let left_key = {
                let (entry_ordinal, reference) = left;
                {
                    (*entry_ordinal, reference.byte_offset)
                }
            };
            let right_key = {
                let (entry_ordinal, reference) = right;
                {
                    (*entry_ordinal, reference.byte_offset)
                }
            };
            left_key.cmp(&right_key)
        },
        |_| 0,
        "sort f3d design sketch 2",
    )?;

    let mut references = Vec::new();
    ctx.reserve_vec(
        &mut references,
        out.len(),
        "f3d persistent reference output",
    )?;
    references.extend(out.into_iter().map(|(_, reference)| reference));
    Ok(references)
}

fn decode_persistent_references_from_stream(
    ctx: &DecodeContext<'_>,
    entry_ordinal: usize,
    entry_name: &str,
    bytes: &[u8],
    out: &mut Vec<(usize, PersistentReference)>,
) -> Result<(), CodecError> {
    for &(name, kind) in &[
        (b"pt_tag".as_slice(), PersistentReferenceKind::Point),
        (
            b"crv_primary_id".as_slice(),
            PersistentReferenceKind::CurvePrimary,
        ),
        (
            b"crv_secondary_id".as_slice(),
            PersistentReferenceKind::CurveSecondary,
        ),
    ] {
        let mut cursor = 0;
        while let Some(offset) = find_from(bytes, name, cursor) {
            cursor = offset + name.len();
            let compact_type_offset = offset + name.len();
            let type_offset = if View::u32_le_at(bytes, compact_type_offset) == Some(23) {
                compact_type_offset
            } else if View::u32_le_at(bytes, compact_type_offset) == Some(2)
                && View::u32_le_at(bytes, compact_type_offset + 4) == Some(14)
                && bytes
                    .get(compact_type_offset + 8..compact_type_offset + 22)
                    .is_some()
                && View::u32_le_at(bytes, compact_type_offset + 22) == Some(23)
            {
                compact_type_offset + 22
            } else {
                continue;
            };
            if View::u32_le_at(bytes, type_offset) != Some(23) {
                continue;
            }
            let type_name = b"IntrinsicMetaTypeuint64";
            if bytes.get(type_offset + 4..type_offset + 4 + type_name.len()) != Some(type_name) {
                continue;
            }
            let value_offset = type_offset + 4 + type_name.len();
            let Some(value) = View::u64_le_at(bytes, value_offset) else {
                continue;
            };

            ctx.reserve_vec(out, 1, "f3d persistent reference index")?;
            out.push((
                entry_ordinal,
                PersistentReference {
                    id: design_record_id_charged(
                        ctx,
                        entry_name,
                        ":persistent-reference#",
                        u64_from_index(offset),
                        "f3d persistent reference ID",
                    )?,
                    byte_offset: u64_from_index(offset),
                    value_offset: u32::try_from(value_offset - offset).map_err(|_| {
                        ctx.refuse_codec_limit(
                            "f3d persistent reference relative offset",
                            u64::from(u32::MAX),
                            u64_from_index(value_offset - offset),
                        )
                    })?,
                    kind,
                    value,
                },
            ));
        }
    }
    Ok(())
}

/// Decode every indexed `EDGE_REFERENCE_LOST` record from each design
/// `BulkStream` entry in `scan`.
pub(crate) fn decode_lost_edge_references(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<LostEdgeReference>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        decode_lost_edge_references_from_stream(ctx, &entry.name, bytes, &mut out)?;
    }
    Ok(out)
}

fn decode_lost_edge_references_from_stream(
    ctx: &DecodeContext<'_>,
    entry_name: &str,
    bytes: &[u8],
    out: &mut Vec<LostEdgeReference>,
) -> Result<(), CodecError> {
    let marker = b"EDGE_REFERENCE_LOST";
    let mut cursor = 0;
    while let Some(relative) = bytes[cursor..]
        .windows(marker.len())
        .position(|window| window == marker)
    {
        let offset = cursor + relative;
        cursor = offset + marker.len();
        let Some(header_offset) = offset.checked_sub(29) else {
            continue;
        };
        let Some((class_tag, after_tag)) =
            lp_ascii_filtered_view(bytes, header_offset, 0..=2000, u8::is_ascii_graphic)
        else {
            continue;
        };
        if after_tag != header_offset + 7
            || bytes.get(header_offset + 11..header_offset + 25) != Some(&[0; 14])
            || View::u32_le_at(bytes, header_offset + 25)
                != Some(u32::try_from(marker.len()).map_err(|_| {
                    ctx.refuse_codec_limit(
                        "f3d lost edge marker length",
                        u64::from(u32::MAX),
                        u64_from_index(marker.len()),
                    )
                })?)
        {
            continue;
        }
        let Some(record_index) = View::u32_le_at(bytes, after_tag) else {
            continue;
        };
        let next_byte_offset = offset + marker.len();
        let Some((next_class_tag, after_next_tag)) =
            lp_ascii_filtered_view(bytes, next_byte_offset, 0..=2000, u8::is_ascii_graphic)
        else {
            continue;
        };
        if after_next_tag != next_byte_offset + 7 {
            continue;
        }
        let Some(next_record_index) = View::u32_le_at(bytes, after_next_tag) else {
            continue;
        };
        if !class_tag.bytes().all(|byte| byte.is_ascii_digit())
            || !next_class_tag.bytes().all(|byte| byte.is_ascii_digit())
        {
            continue;
        }

        ctx.reserve_vec(out, 1, "f3d lost edge reference output")?;
        let Ok(reference) = LostEdgeReference::new(
            design_record_id_charged(
                ctx,
                entry_name,
                ":lost-edge-reference#",
                u64_from_index(header_offset),
                "f3d lost edge reference ID",
            )?,
            u64_from_index(header_offset),
            class_tag.to_owned(),
            record_index,
            next_class_tag.to_owned(),
            next_record_index,
        ) else {
            continue;
        };
        out.push(reference);
    }
    Ok(())
}

/// Parse the fixed entity-header layout at `start`: a u64 entity suffix, five
/// zero bytes, an optional slot, and the UTF-16LE entity id whose numeric
/// suffix equals the header's entity suffix.
pub(super) fn parse_settled_entity_header(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
) -> Result<Option<NamedEntityHeader>, CodecError> {
    let Some(entity_suffix) = View::u64_le_at(bytes, start + 7) else {
        return Ok(None);
    };
    if entity_suffix == 0 || bytes.get(start + 15..start + 20) != Some(&[0u8; 5]) {
        return Ok(None);
    }
    let (optional_slot_present, string_offset) = match bytes.get(start + 20) {
        Some(0) => (false, start + 21),
        Some(1) if bytes.get(start + 21..start + 25) == Some(&[0u8; 4]) => (true, start + 25),
        _ => return Ok(None),
    };
    let Some((entity_id, end)) =
        lp_utf16_bounded_charged(ctx, bytes, string_offset, 1..=256, "f3d Design UTF-16 text")?
    else {
        return Ok(None);
    };
    let Ok(entity_id) = crate::records::identity::DesignEntityId::try_from(entity_id) else {
        return Ok(None);
    };
    Ok(
        (entity_id.suffix() == entity_suffix).then_some(NamedEntityHeader {
            entity_id,
            entity_id_offset: string_offset + 4,
            optional_slot_present,
            end,
        }),
    )
}

/// An admitted entity identity and its source header locations.
pub(super) struct NamedEntityHeader {
    pub(super) entity_id: crate::records::identity::DesignEntityId,
    pub(super) entity_id_offset: usize,
    optional_slot_present: bool,
    pub(super) end: usize,
}

/// Parse the `EntityGenesis` entity-header layout at `start`: the u32 record
/// index doubles as the entity suffix and is followed by a zero run, a
/// `0x01`-marked u32 1, the `EntityGenesis` and `IntrinsicMetaTypeuint64`
/// key strings, the u64 origin bitfield, and the UTF-16LE entity id whose
/// numeric suffix equals the record index.
pub(super) fn parse_genesis_entity_header(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
) -> Result<Option<NamedEntityHeader>, CodecError> {
    let Some(entity_suffix) = View::u32_le_at(bytes, start + 7).map(u64::from) else {
        return Ok(None);
    };
    if entity_suffix == 0 {
        return Ok(None);
    }
    let mut cursor = start + 11;
    while bytes.get(cursor) == Some(&0) && cursor < start + 35 {
        cursor += 1;
    }
    if cursor == start + 11
        || bytes.get(cursor) != Some(&1)
        || View::u32_le_at(bytes, cursor + 1) != Some(1)
    {
        return Ok(None);
    }
    let Some(after_key) = lp_ascii_matches(bytes, cursor + 5, b"EntityGenesis") else {
        return Ok(None);
    };
    let Some(after_type) = lp_ascii_matches(bytes, after_key, b"IntrinsicMetaTypeuint64") else {
        return Ok(None);
    };
    let Some((entity_id, end)) = lp_utf16_bounded_charged(
        ctx,
        bytes,
        after_type + 8,
        1..=256,
        "f3d Design UTF-16 text",
    )?
    else {
        return Ok(None);
    };
    let Ok(entity_id) = crate::records::identity::DesignEntityId::try_from(entity_id) else {
        return Ok(None);
    };
    Ok(
        (entity_id.suffix() == entity_suffix).then_some(NamedEntityHeader {
            entity_id,
            entity_id_offset: after_type + 12,
            optional_slot_present: false,
            end,
        }),
    )
}

fn lp_ascii_matches(bytes: &[u8], at: usize, expected: &[u8]) -> Option<usize> {
    let count = usize::try_from(View::u32_le_at(bytes, at)?).ok()?;
    if count != expected.len() {
        return None;
    }
    let start = at.checked_add(4)?;
    let end = start.checked_add(count)?;
    (bytes.get(start..end) == Some(expected)).then_some(end)
}

/// Parse the counted member-record run of the paired same-index container
/// record that follows an `EntityGenesis`-form sketch entity header: the u32
/// member count at paired-record offset 52, the marked reference to the
/// sketch's base-point record, and `count` entries of `0x01 + u32
/// record_index + six zero bytes` naming the sketch's owned records. The
/// base-point reference is returned as the first member.
fn parse_sketch_member_run(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    from: usize,
    entity_suffix: u64,
) -> Result<Vec<crate::records::identity::Located<u32>>, CodecError> {
    let empty = Vec::new();
    let Some(paired) = next_indexed_record_offset(bytes, from) else {
        return Ok(empty);
    };
    if View::u32_le_at(bytes, paired + 7).map(u64::from) != Some(entity_suffix) {
        return Ok(empty);
    }
    let Some(count) =
        View::u32_le_at(bytes, paired + 52).and_then(|count| usize::try_from(count).ok())
    else {
        return Ok(empty);
    };
    if count == 0
        || bytes.get(paired + 56) != Some(&1)
        || bytes.get(paired + 61..paired + 67) != Some(&[0u8; 6][..])
    {
        return Ok(empty);
    }
    let Some(base_point_index) = View::u32_le_at(bytes, paired + 57) else {
        return Ok(empty);
    };
    let Some(capacity) = count.checked_add(1) else {
        return Ok(empty);
    };
    let Some(run_end) = count
        .checked_mul(11)
        .and_then(|length| paired.checked_add(67)?.checked_add(length))
    else {
        return Ok(empty);
    };
    if run_end > bytes.len() {
        return Ok(empty);
    }

    let mut members = Vec::new();
    ctx.reserve_vec(&mut members, capacity, "f3d sketch member run")?;
    members.push(crate::records::identity::Located {
        value: base_point_index,
        offset: u64_from_index(paired + 57),
    });
    for ordinal in 0..count {
        let marker = paired + 67 + ordinal * 11;
        if bytes.get(marker) != Some(&1)
            || bytes.get(marker + 5..marker + 11) != Some(&[0u8; 6][..])
        {
            return Ok(empty);
        }
        let Some(record_index) = View::u32_le_at(bytes, marker + 1) else {
            return Ok(empty);
        };
        members.push(crate::records::identity::Located {
            value: record_index,
            offset: u64_from_index(marker + 1),
        });
    }
    Ok(members)
}

/// Parse the counted member-record run of a legacy sketch container's paired
/// same-index record. The paired record stores its head-placement reference
/// at offset 19, six zero bytes, a u32 sketch ordinal and seven bytes of
/// state, then the member count at offset 41. Each member is a padded marked
/// reference.
fn parse_legacy_sketch_member_run(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    primary_at: usize,
    entity_suffix: u32,
) -> Result<Option<Vec<crate::records::identity::Located<u32>>>, CodecError> {
    let parsed = (|| {
        let paired_at = next_indexed_record_offset(bytes, primary_at + 11)?;
        if View::u32_le_at(bytes, paired_at + 7) != Some(entity_suffix)
            || bytes.get(paired_at + 11..paired_at + 19) != Some(&[0u8; 8][..])
            || bytes.get(paired_at + 19) != Some(&1)
            || bytes.get(paired_at + 24..paired_at + 30) != Some(&[0u8; 6][..])
        {
            return None;
        }
        let (paired_class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
        if after_tag != paired_at + 7
            || paired_class_tag.len() != 3
            || !paired_class_tag.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let count = usize::try_from(View::u32_le_at(bytes, paired_at + 41)?).ok()?;
        if count == 0 {
            return None;
        }
        let run_end = (paired_at + 45).checked_add(count.checked_mul(11)?)?;
        if run_end > bytes.len() {
            return None;
        }
        Some((paired_at, count))
    })();
    let Some((paired_at, count)) = parsed else {
        return Ok(None);
    };

    let mut members = Vec::new();
    ctx.reserve_vec(&mut members, count, "f3d legacy sketch member run")?;
    let parsed = (|| {
        for ordinal in 0..count {
            let marker = paired_at + 45 + ordinal * 11;
            if bytes.get(marker) != Some(&1)
                || bytes.get(marker + 5..marker + 11) != Some(&[0u8; 6][..])
            {
                return None;
            }
            members.push(crate::records::identity::Located {
                value: View::u32_le_at(bytes, marker + 1)?,
                offset: u64_from_index(marker + 1),
            });
        }
        Some(members)
    })();
    Ok(parsed)
}

/// Recognize either legacy sketch-container tail. A counted container owns
/// its complete member run. A localized container omits that run and is
/// accepted only when its paired record names an exact placement-head frame.
fn parse_legacy_sketch_container_members(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    primary_at: usize,
    entity_suffix: u32,
    records: &IndexedRecordOffsets,
) -> Result<Option<Vec<crate::records::identity::Located<u32>>>, CodecError> {
    if let Some(members) = parse_legacy_sketch_member_run(ctx, bytes, primary_at, entity_suffix)? {
        return Ok(Some(members));
    }
    let entity_id =
        crate::records::identity::DesignEntityId::from_parts("Sketch", u64::from(entity_suffix));
    if parse_member_run_head_placement(ctx, bytes, u64_from_index(primary_at), &entity_id, records)?
        .is_some()
    {
        Ok(Some(Vec::new()))
    } else {
        Ok(None)
    }
}

/// Decode every self-validating per-entity design `BulkStream` header (spec
/// [§3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)): a three-digit class tag, an entity suffix, a UTF-16LE entity ID
/// whose numeric suffix must match the header's entity suffix, and, for
/// sketch-typed entities, the trailing reference-list header. Headers occur in
/// the fixed layout or in the `EntityGenesis` layout.
pub(crate) fn decode_entity_headers(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<DesignEntityHeader>, CodecError> {
    let mut out = Vec::new();
    // Entity ids are unique per Design stream, not archive-wide.
    let mut entity_modules = HashMap::<&str, HashMap<u64, &str>>::new();
    let types = decode_types(ctx, scan)?;
    let mut legacy_sketch_candidates = HashMap::<&str, std::collections::HashSet<u32>>::new();
    for design_type in &types {
        if let Some(stream) = native_stream(design_type.id()) {
            for &entity_id in design_type.entities.values() {
                insert_entity_module(
                    ctx,
                    &mut entity_modules,
                    stream,
                    entity_id,
                    &design_type.module,
                )?;
            }
        }
        if design_type.module == DESIGN_MODULE_SKETCH {
            let Some(stream) = native_stream(design_type.id()) else {
                continue;
            };
            let Some(prefix) = stream
                .strip_prefix(ids::SCHEME_PREFIX)
                .and_then(|name| name.strip_suffix("MetaStream.dat"))
            else {
                continue;
            };
            for identity in design_type
                .entities
                .values()
                .copied()
                .filter_map(|id| u32::try_from(id).ok())
            {
                insert_legacy_candidate(ctx, &mut legacy_sketch_candidates, prefix, identity)?;
            }
        }
    }
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        // Modules come from the type table of this stream's own `MetaStream`.
        let (_scope_reservation, scope) = native_scope_scoped(ctx, &entry.name)?;
        let meta_scope = entity_meta_scope(ctx, &scope)?;
        let stream_modules = meta_scope
            .as_ref()
            .and_then(|(_, name)| entity_modules.get(name.as_str()));
        for header in indexed_record_offsets(bytes) {
            let start = header.offset;
            let class_tag = header.class_tag.clone();
            let settled = parse_settled_entity_header(ctx, bytes, start)?;
            let genesis_form = settled.is_none();
            let Some(NamedEntityHeader {
                entity_id,
                optional_slot_present,
                end,
                ..
            }) = (match settled {
                Some(header) => Some(header),
                None => parse_genesis_entity_header(ctx, bytes, start)?,
            })
            else {
                continue;
            };
            let entity_suffix = entity_id.suffix();
            let module = stream_modules
                .and_then(|modules| modules.get(&entity_suffix))
                .map(|module| ctx.copy_retained_text(module, "f3d entity module text"))
                .transpose()?;
            let in_sketch_module = module.as_deref() == Some(DESIGN_MODULE_SKETCH);
            let list = if in_sketch_module {
                decode_reference_list(ctx, bytes, end)?
            } else {
                None
            };
            let record_end = list.as_ref().map_or(end, |list| list.end);
            let references =
                list.map(
                    |list| crate::records::entity_header::SketchHeaderReferences {
                        record_reference: list.record_reference.value,
                        record_reference_offset: list.record_reference.offset,
                        references: list.references,
                    },
                );
            let members = if genesis_form && in_sketch_module {
                parse_sketch_member_run(ctx, bytes, record_end, entity_suffix)?
            } else {
                Vec::new()
            };
            ctx.push_vec(
                &mut out,
                DesignEntityHeader {
                    id: design_record_id_charged(
                        ctx,
                        &entry.name,
                        ":design-entity-header#",
                        u64_from_index(start),
                        "f3d entity header ID",
                    )?,
                    byte_offset: u64_from_index(start),

                    entity_id,
                    class_tag,
                    optional_slot_present,
                    registration: crate::records::entity_header::DesignEntityRegistration::new(
                        module,
                        references,
                        crate::records::identity::ReferenceRun::located(members),
                    )
                    .map_err(CodecError::Malformed)?,
                },
                "f3d entity header output",
            )?;
        }

        // Legacy Design streams do not carry textual entity headers. Their
        // MSketch metadata names candidate record indices; only actual sketch
        // containers have a consecutive same-index pair with the legacy
        // counted member run. Materialize the same ownership abstraction used
        // by later entity-header forms so downstream binding remains uniform.
        let candidates = entry
            .name
            .strip_suffix("BulkStream.dat")
            .and_then(|prefix| legacy_sketch_candidates.get(prefix));
        let Some(candidates) = candidates.filter(|candidates| !candidates.is_empty()) else {
            continue;
        };
        let records = IndexedRecordOffsets::build(ctx, bytes)?;
        let mut existing = std::collections::HashSet::new();
        for entity in out
            .iter()
            .filter(|entity| native_stream(&entity.id) == Some(scope.as_str()))
        {
            let Ok(index) = u32::try_from(entity.entity_id.suffix()) else {
                continue;
            };
            ctx.insert_hash_set(&mut existing, index, "f3d existing entity index")
                .map(|_| ())?;
        }
        for header in indexed_record_offsets(bytes) {
            let start = header.offset;
            let entity_suffix = header.record_index;
            if !candidates.contains(&entity_suffix) || existing.contains(&entity_suffix) {
                continue;
            }
            let class_tag = header.class_tag.clone();
            let Some(members) =
                parse_legacy_sketch_container_members(ctx, bytes, start, entity_suffix, &records)?
            else {
                continue;
            };
            ctx.insert_hash_set(&mut existing, entity_suffix, "f3d existing entity index")
                .map(|_| ())?;
            ctx.push_vec(
                &mut out,
                DesignEntityHeader {
                    id: design_record_id_charged(
                        ctx,
                        &entry.name,
                        ":design-entity-header#",
                        u64_from_index(start),
                        "f3d entity header ID",
                    )?,
                    byte_offset: u64_from_index(start),

                    entity_id: crate::records::identity::DesignEntityId::from_parts(
                        "Sketch",
                        u64::from(entity_suffix),
                    ),
                    class_tag,
                    optional_slot_present: false,
                    registration: crate::records::entity_header::DesignEntityRegistration::new(
                        Some(DESIGN_MODULE_SKETCH.to_owned()),
                        None,
                        crate::records::identity::ReferenceRun::located(members),
                    )
                    .map_err(CodecError::Malformed)?,
                },
                "f3d entity header output",
            )?;
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |a, b| a.id.cmp(&b.id),
        |value| value.id.as_str().len(),
        "sort f3d design sketch 3",
    )?;
    Ok(out)
}

fn insert_entity_module<'a>(
    ctx: &DecodeContext<'_>,
    entity_modules: &mut HashMap<&'a str, HashMap<u64, &'a str>>,
    stream: &'a str,
    entity_id: u64,
    module: &'a str,
) -> Result<(), CodecError> {
    if !entity_modules.contains_key(stream) {
        ctx.reserve_map(entity_modules, 1, "f3d entity module stream")?;
    }
    let stream_modules = entity_modules.entry(stream).or_default();
    if !stream_modules.contains_key(&entity_id) {
        ctx.reserve_map(stream_modules, 1, "f3d entity module index")?;
        stream_modules.insert(entity_id, module);
    }
    Ok(())
}

fn insert_legacy_candidate<'a>(
    ctx: &DecodeContext<'_>,
    streams: &mut HashMap<&'a str, std::collections::HashSet<u32>>,
    prefix: &'a str,
    identity: u32,
) -> Result<(), CodecError> {
    if !streams.contains_key(prefix) {
        ctx.reserve_map(streams, 1, "f3d legacy sketch stream")?;
    }
    let candidates = streams.entry(prefix).or_default();
    ctx.insert_hash_set(candidates, identity, "f3d legacy sketch candidate")
        .map(|_| ())?;
    Ok(())
}

/// Decode the indexed dynamic-class record headers ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)) that `entities`'
/// reference-list entries point at: a `u32` record index and a three-digit
/// class tag, for each record index named by any [`DesignEntityHeader`] in
/// `entities`.
pub(crate) fn decode_record_headers(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    let references = entities.iter().flat_map(|entity| {
        native_stream(&entity.id)
            .into_iter()
            .flat_map(move |scope| {
                entity
                    .reference_values()
                    .map(move |record_index| (scope, *record_index))
            })
    });
    let wanted = wanted_record_indices(ctx, references)?;
    decode_headers_for_indices(ctx, scan, &wanted)
}

/// Decode the indexed dynamic-class record headers ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)) named by
/// `indices` directly, bypassing entity reference lists. Used to fetch record
/// headers referenced by records other than [`DesignEntityHeader`] (for
/// example, sketch relation records).
pub(crate) fn decode_related_record_headers(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    indices: &[(String, u32)],
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    let wanted = wanted_record_indices(
        ctx,
        indices
            .iter()
            .map(|(scope, index)| (scope.as_str(), *index)),
    )?;
    decode_headers_for_indices(ctx, scan, &wanted)
}

fn wanted_record_indices<'a>(
    ctx: &DecodeContext<'_>,
    indices: impl IntoIterator<Item = (&'a str, u32)>,
) -> Result<std::collections::HashSet<(&'a str, u32)>, CodecError> {
    let mut wanted = std::collections::HashSet::new();
    for index in indices {
        if !wanted.contains(&index) {
            ctx.reserve_set(&mut wanted, 1, "f3d wanted record index")?;
            wanted.insert(index);
        }
    }
    Ok(wanted)
}

fn decode_headers_for_indices(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    wanted: &std::collections::HashSet<(&str, u32)>,
) -> Result<Vec<DesignRecordHeader>, CodecError> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let (_scope_reservation, scope) = native_scope_scoped(ctx, &entry.name)?;
        decode_headers_for_indices_from_stream(ctx, &entry.name, &scope, bytes, wanted, &mut out)?;
    }
    ctx.stable_sort_by(
        &mut out[..],
        |a, b| a.id.cmp(&b.id),
        |value| value.id.as_str().len(),
        "sort f3d design sketch 4",
    )?;
    Ok(out)
}

fn decode_headers_for_indices_from_stream(
    ctx: &DecodeContext<'_>,
    stream_name: &str,
    scope: &str,
    bytes: &[u8],
    wanted: &std::collections::HashSet<(&str, u32)>,
    out: &mut Vec<DesignRecordHeader>,
) -> Result<(), CodecError> {
    let mut emitted = std::collections::HashSet::new();
    for header in indexed_record_offsets(bytes) {
        let position = header.offset;
        let record_index = header.record_index;
        if wanted.contains(&(scope, record_index)) && !emitted.contains(&record_index) {
            ctx.reserve_set(&mut emitted, 1, "f3d record header emitted index")?;
            emitted.insert(record_index);

            ctx.reserve_vec(out, 1, "f3d record header output")?;
            out.push(DesignRecordHeader {
                id: design_record_id_charged(
                    ctx,
                    stream_name,
                    ":design-record-header#",
                    u64_from_index(position),
                    "f3d record header ID",
                )?,
                record_index,
                class_tag: header.class_tag,
                byte_offset: u64_from_index(position),
            });
        }
    }
    Ok(())
}

/// Decode the sketch-relation body at each `records` entry's offset: the
/// owning sketch relation's member reference list, owner reference, state,
/// and return-member list. `records` supplies the byte offsets and class tags
/// (typically from [`decode_related_record_headers`]).
pub(crate) fn decode_sketch_relations(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    records: &[DesignRecordHeader],
) -> Result<Vec<SketchRelation>, CodecError> {
    let mut out = Vec::new();
    // A record carries no class identity of its own: its class tag selects an
    // entry in its segment's own type table, and only that entry's GUID names
    // the class across segments.
    let types = decode_types(ctx, scan)?;
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let stream_types = stream_types_by_class_tag(ctx, &types, &entry.name)?;
        let scope = ids::native_scope(&entry.name);
        let bytes = scan.entry_bytes(&entry.name)?;
        for record in records
            .iter()
            .filter(|record| native_stream(&record.id) == Some(scope.as_str()))
        {
            let Ok(at) = usize::try_from(record.byte_offset) else {
                continue;
            };
            let record_end = next_indexed_record_offset(bytes, at + 11).unwrap_or(bytes.len());
            let Some(payload) = bytes.get(at..record_end) else {
                continue;
            };
            let class = stream_types
                .get(&record.class_tag.code())
                .and_then(|design_type| {
                    SketchRelationClass::of(design_type.type_guid.as_str(), design_type.version)
                });
            let parsed = match class {
                Some(class) => parse_classed_sketch_relation(ctx, payload, class)?,
                None => None,
            };
            let Some(mut parsed) = parsed else {
                continue;
            };
            if payload
                .get(parsed.parsed_end..)
                .is_none_or(|padding| padding.iter().any(|byte| *byte != 0))
            {
                continue;
            }
            let pattern = decode_pattern_definition(payload, &mut parsed);
            let Ok(definition) = crate::records::sketch_relations::SketchRelationDefinition::new(
                parsed.state,
                pattern,
            ) else {
                continue;
            };
            admit_sketch_relation(
                ctx,
                &mut out,
                &entry.name,
                record,
                payload,
                parsed,
                definition,
            )?;
        }
    }
    Ok(out)
}

fn admit_sketch_relation(
    ctx: &DecodeContext<'_>,
    out: &mut Vec<SketchRelation>,
    stream: &str,
    record: &DesignRecordHeader,
    payload: &[u8],
    parsed: ParsedSketchRelation,
    definition: crate::records::sketch_relations::SketchRelationDefinition,
) -> Result<(), CodecError> {
    ctx.reserve_vec(out, 1, "f3d sketch relation output")?;
    let rectangular_counted_reference_count = match &parsed.class_members {
        RelationClassMembers::Rectangular {
            reference_count, ..
        } => Some(*reference_count),
        _ => None,
    };
    let members = crate::records::sketch_relations::SketchRelationMembers::from_indices(
        ctx,
        parsed.members.into_iter().map(|member| {
            (
                member.reference.value,
                member.reference.offset,
                member.relation_ordinal,
            )
        }),
    )?;
    let return_members =
        crate::records::sketch_relations::SketchRelationReturnMembers::from_indices(
            ctx,
            parsed
                .return_members
                .into_iter()
                .map(|member| (member.value, member.offset)),
        )?;
    let auxiliary_count = parsed.auxiliary_references.len();

    let mut auxiliary_references = Vec::new();
    ctx.reserve_vec(
        &mut auxiliary_references,
        auxiliary_count,
        "f3d sketch relation auxiliary output",
    )?;
    for row in parsed.auxiliary_references {
        auxiliary_references.push(crate::records::identity::Located {
            value: row.value,
            offset: u32::try_from(row.offset).map_err(|_| {
                ctx.refuse_codec_limit(
                    "f3d sketch relation auxiliary offset",
                    u64::from(u32::MAX),
                    u64_from_index(row.offset),
                )
            })?,
        });
    }
    let relation = SketchRelation::try_new(crate::records::sketch_relations::SketchRelationDraft {
        id: design_record_id_charged(
            ctx,
            stream,
            ":sketch-relation#",
            u64::from(record.record_index),
            "f3d sketch relation ID",
        )?,
        record_index: record.record_index,
        class_tag: record.class_tag.clone(),
        byte_offset: record.byte_offset,
        state_offset: u32::try_from(parsed.state_offset).map_err(|_| {
            ctx.refuse_codec_limit(
                "f3d sketch relation state offset",
                u64::from(u32::MAX),
                u64_from_index(parsed.state_offset),
            )
        })?,
        owner_reference: parsed.owner_reference,
        owner_entity_id: None,
        owner_reference_offset: u32::try_from(parsed.owner_reference_offset).map_err(|_| {
            ctx.refuse_codec_limit(
                "f3d sketch relation owner offset",
                u64::from(u32::MAX),
                u64_from_index(parsed.owner_reference_offset),
            )
        })?,
        auxiliary_references: crate::records::identity::ReferenceRun::located(auxiliary_references),
        rectangular_counted_reference_count,
        members,
        definition,
        entity_genesis: parsed.entity_genesis,
        return_members,
        raw_bytes: ctx.copy_retained(payload, "f3d sketch relation raw bytes")?,
    })
    .map_err(|error| CodecError::malformed(error.to_string()))?;
    out.push(relation);
    Ok(())
}

/// Decode the pattern definition a relation's class members carry, reading them
/// beside the auxiliary references the parse recorded. Circular patterns store
/// the angle- and count-parameter references, the evaluated f64 total angle six
/// zero bytes after the count-parameter reference, and the evaluated u32
/// instance count directly after it. Rectangular patterns store, per direction,
/// the evaluated u32 count, the count-parameter reference, a three-component
/// f64 unit direction six zero bytes after that reference, the evaluated f64
/// source distance, and the distance-parameter reference. A non-empty counted
/// reference run stores adjacent spacing; an empty run stores the total
/// seed-to-final span. Text-frame relations repeat the sketch-text member as an
/// auxiliary reference.
fn decode_pattern_definition(
    payload: &[u8],
    parsed: &mut ParsedSketchRelation,
) -> Option<crate::records::sketch_relations::SketchPatternDefinition> {
    use crate::records::sketch_relations::{SketchPatternDefinition, SketchPatternDirection};
    let reference_end = |ordinal: usize| Some(parsed.auxiliary_references.get(ordinal)?.offset + 4);
    match &mut parsed.class_members {
        RelationClassMembers::CircularPattern => {
            if parsed.state != 0x1000_0000 || parsed.auxiliary_references.len() != 2 {
                return None;
            }
            let angle_at = reference_end(1)? + 6;
            let evaluated_angle = FiniteReal::new(View::f64_le_at(payload, angle_at)?)?;
            let evaluated_count = crate::records::sketch_relations::SketchPatternCount::try_from(
                View::u32_le_at(payload, angle_at + 8)?,
            )
            .ok()?;
            return Some(SketchPatternDefinition::Circular {
                angle_parameter: parsed.auxiliary_references[0].value,
                count_parameter: parsed.auxiliary_references[1].value,
                evaluated_angle,
                evaluated_count,
            });
        }
        RelationClassMembers::Rectangular { clauses, .. } => {
            if parsed.state != 0x2000_0000 {
                return None;
            }
            let [first_count, first_distance, second_count, second_distance] = (*clauses)?;
            let directions = [
                (first_count, first_distance),
                (second_count, second_distance),
            ]
            .map(|(count, distance)| {
                let count_at = count.offset.checked_sub(5)?;
                let evaluated_count =
                    crate::records::sketch_relations::SketchPatternCount::try_from(
                        View::u32_le_at(payload, count_at)?,
                    )
                    .ok()?;
                let direction_at = count.offset + 4 + 6;
                let direction = [
                    View::f64_le_at(payload, direction_at)?,
                    View::f64_le_at(payload, direction_at + 8)?,
                    View::f64_le_at(payload, direction_at + 16)?,
                ];
                Some(SketchPatternDirection {
                    evaluated_count,
                    count_parameter: count.value,
                    direction: direction.try_into().ok()?,
                    evaluated_distance: FiniteReal::new(View::f64_le_at(
                        payload,
                        direction_at + 24,
                    )?)?,
                    distance_parameter: distance.value,
                })
            });
            let [first, second] = directions;
            return Some(SketchPatternDefinition::Rectangular {
                directions: [first?, second?],
            });
        }
        RelationClassMembers::TextFrame => {
            if parsed.state == 0x100_0000_0000
                && parsed.auxiliary_references.len() == 1
                && parsed
                    .members
                    .iter()
                    .any(|member| member.reference.value == parsed.auxiliary_references[0].value)
            {
                return Some(SketchPatternDefinition::TextFrame {
                    text_reference: parsed.auxiliary_references[0].value,
                });
            }
        }
        RelationClassMembers::TextPath { glyph_transforms } => {
            if parsed.state == 0x200_0000_0000
                && parsed.auxiliary_references.len() == 1
                && parsed
                    .members
                    .iter()
                    .any(|member| member.reference.value == parsed.auxiliary_references[0].value)
            {
                return Some(SketchPatternDefinition::TextPath {
                    text_reference: parsed.auxiliary_references[0].value,
                    glyph_transforms: std::mem::take(glyph_transforms),
                });
            }
        }
        RelationClassMembers::Plain | RelationClassMembers::Tangent => {}
    }
    None
}

fn trailing_sketch_owner_reference(record: &[u8]) -> Option<u32> {
    let tail = record.len().checked_sub(11)?;
    if record.get(tail) != Some(&1) || record.get(tail + 5..tail + 11) != Some(&[0u8; 6][..]) {
        return None;
    }
    View::u32_le_at(record, tail + 1)
}

fn decode_sketch_points_from_stream(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    stream: &str,
) -> Result<Vec<SketchPoint>, CodecError> {
    let frames = design_primary_frames(ctx, bytes, meta)?;
    let mut frames_by_entity = HashMap::new();
    let mut types_by_entity = HashMap::new();
    for frame in &frames {
        let Ok(entity_id) = u32::try_from(frame.entity_id) else {
            continue;
        };
        if !frames_by_entity.contains_key(&entity_id) {
            ctx.reserve_map(&mut frames_by_entity, 1, "f3d sketch point frame index")?;
        }
        frames_by_entity.insert(entity_id, frame);
        if !types_by_entity.contains_key(&entity_id) {
            ctx.reserve_map(&mut types_by_entity, 1, "f3d sketch point type index")?;
        }
        types_by_entity.insert(
            entity_id,
            (
                frame.design_type.type_guid.as_str(),
                frame.design_type.version,
                frame.design_type.module.as_str(),
            ),
        );
    }
    let mut out = Vec::new();
    for frame in &frames {
        if !frame
            .design_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case(SKETCH_POINT_TYPE_GUID)
            || frame.design_type.module != CURRENT_SKETCH_POINT_TYPE.2
            || ![0, 8, 10, CURRENT_SKETCH_POINT_TYPE.1].contains(&frame.design_type.version)
        {
            continue;
        }
        let payload = &bytes[frame.start..frame.end];
        let record_index = u32::try_from(frame.entity_id)
            .map_err(|_| CodecError::Malformed("F3D sketch-point entity ID exceeds u32".into()))?;
        let Some(decoded) = decode_sketch_point_record(payload, frame.design_type.version) else {
            // Some offline Fusion files carry sketch points under version 11 (or companion
            // layouts) that B-rep import does not need; skip them instead of failing decode.
            continue;
        };
        let (u, v) = (decoded.coordinates[0] * 10.0, decoded.coordinates[1] * 10.0);
        if !point_target_has_guid(
            &types_by_entity,
            decoded.trailing_reference(),
            SKETCH_CONTAINER_TYPE_GUID,
        ) {
            continue;
        }
        let Some((record_form, companion)) = frames_by_entity
            .get(&decoded.paired_reference)
            .filter(|companion_frame| {
                let design_type = companion_frame.design_type;
                design_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(SKETCH_POINT_COMPANION_TYPE.0)
                    && design_type.version == SKETCH_POINT_COMPANION_TYPE.1
                    && design_type.module == SKETCH_POINT_COMPANION_TYPE.2
            })
            .map(|companion_frame| {
                decode_sketch_point_companion(
                    ctx,
                    &bytes[companion_frame.start..companion_frame.end],
                    record_index,
                    decoded.record_form.clone(),
                    &types_by_entity,
                )
            })
            .transpose()?
            .flatten()
        else {
            continue;
        };

        ctx.reserve_vec(&mut out, 1, "f3d sketch point output")?;
        out.push(SketchPoint::try_from_charged(
            ctx,
            crate::records::sketch_geometry::SketchPointDraft {
                id: design_record_id_charged(
                    ctx,
                    stream,
                    ":sketch-point#",
                    u64_from_index(frame.start),
                    "f3d sketch point ID",
                )?,
                record_index,
                owner_reference: decoded.owner_reference,
                class_tag: frame.class_tag.clone(),
                byte_offset: u64_from_index(frame.start),
                coordinate_offset: decoded.coordinate_offset,
                record_form,
                companion,
                paired_reference: decoded.paired_reference,
                coordinates: Point2::new(u, v),
            },
        )?);
    }
    Ok(out)
}

/// Decode every sketch-point record ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata), `pt_tag`) from each design
/// `BulkStream` entry in `scan`: its versioned identity, flags, coordinates,
/// closure, trailing reference, and paired reverse curve-incidence record,
/// converted centimetre→millimetre. Class version 0 supplies `(u,v)` and no
/// persistent identity; later forms supply `(u,v,w)` and `pt_tag`. A known
/// point record with a malformed or non-finite member sequence makes the
/// stream malformed.
pub(crate) fn decode_sketch_points(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<SketchPoint>, CodecError> {
    decode_sketch_streams(ctx, scan, decode_sketch_points_from_stream)
}

struct SketchProperty<'a> {
    name: &'a str,
    value: u64,
}

/// Read a class property block: a presence byte, and when it is `01`, a u32
/// count and that many `(key, type name, value)` triples.
///
/// Which keys a record carries varies by record, so a caller addresses a
/// property by name. Reading the block by fixed offset misframes every record
/// whose key set differs from the one the offsets were taken from.
struct SketchProperties<'a> {
    payload: &'a [u8],
    first: usize,
    count: usize,
}

impl SketchProperties<'_> {
    fn find(&self, key: &str) -> Option<u64> {
        let mut cursor = self.first;
        for _ in 0..self.count {
            let (name, after_name) =
                lp_ascii_filtered_view(self.payload, cursor, 0..=256, u8::is_ascii_graphic)?;
            let (_, after_type) =
                lp_ascii_filtered_view(self.payload, after_name, 0..=256, u8::is_ascii_graphic)?;
            let value = View::u64_le_at(self.payload, after_type)?;
            if name == key {
                return Some(value);
            }
            cursor = after_type.checked_add(8)?;
        }
        None
    }

    fn first_two(&self) -> Option<(SketchProperty<'_>, Option<SketchProperty<'_>>)> {
        if !matches!(self.count, 1 | 2) {
            return None;
        }
        let (first_name, after_name) =
            lp_ascii_filtered_view(self.payload, self.first, 0..=256, u8::is_ascii_graphic)?;
        let (_, after_type) =
            lp_ascii_filtered_view(self.payload, after_name, 0..=256, u8::is_ascii_graphic)?;
        let first_value = View::u64_le_at(self.payload, after_type)?;
        let second = if self.count == 2 {
            let at = after_type.checked_add(8)?;
            let (name, after_name) =
                lp_ascii_filtered_view(self.payload, at, 0..=256, u8::is_ascii_graphic)?;
            let (_, after_type) =
                lp_ascii_filtered_view(self.payload, after_name, 0..=256, u8::is_ascii_graphic)?;
            Some(SketchProperty {
                name,
                value: View::u64_le_at(self.payload, after_type)?,
            })
        } else {
            None
        };
        Some((
            SketchProperty {
                name: first_name,
                value: first_value,
            },
            second,
        ))
    }
}

fn read_property_block<'a>(payload: &'a [u8], cursor: &mut usize) -> Option<SketchProperties<'a>> {
    let mut count = 0;
    let mut first = *cursor;
    match payload.get(*cursor)? {
        0 => *cursor += 1,
        1 => {
            *cursor += 1;
            count = usize::try_from(View::u32_le_at(payload, *cursor)?).ok()?;
            if count > MAX_RELATION_RUN {
                return None;
            }
            *cursor += 4;
            first = *cursor;
            for _ in 0..count {
                let (_, after_key) =
                    lp_ascii_filtered_view(payload, *cursor, 0..=256, u8::is_ascii_graphic)?;
                let (type_name, after_type) =
                    lp_ascii_filtered_view(payload, after_key, 0..=256, u8::is_ascii_graphic)?;
                if type_name != "IntrinsicMetaTypeuint64" {
                    return None;
                }
                View::u64_le_at(payload, after_type)?;
                *cursor = after_type.checked_add(8)?;
            }
        }
        _ => return None,
    }
    Some(SketchProperties {
        payload,
        first,
        count,
    })
}

const SKETCH_TEXT_TYPE_GUIDS: [&str; 2] = [
    "E0618268-3A06-450E-9E94-7CF4C2E66802",
    "F0B1AFA3-3BAF-42D0-B2F3-94B95662F2A9",
];

fn sketch_utf16_text(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    count_at: usize,
    count: usize,
) -> Result<Option<(String, usize)>, CodecError> {
    lp_utf16_bounded_charged(
        ctx,
        payload,
        count_at,
        count..=count,
        "f3d Design UTF-16 text",
    )
}

/// Decode sketch-text records carrying persistent identities, font metrics,
/// UTF-16 content, and an owning-sketch reference.
fn decode_sketch_texts_from_stream(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    stream: &str,
) -> Result<Vec<SketchText>, CodecError> {
    let mut out = Vec::new();
    for frame in design_primary_frames(ctx, bytes, meta)? {
        if !SKETCH_TEXT_TYPE_GUIDS.iter().any(|type_guid| {
            frame
                .design_type
                .type_guid
                .as_str()
                .eq_ignore_ascii_case(type_guid)
        }) {
            continue;
        }
        let record_index = u32::try_from(frame.entity_id)
            .map_err(|_| CodecError::Malformed("F3D sketch-text entity ID exceeds u32".into()))?;
        let class_tag = frame.class_tag;
        let payload = &bytes[frame.start..frame.end];
        if let Some(text) = decode_sketch_text_record(
            ctx,
            payload,
            stream,
            class_tag,
            frame.design_type.version,
            record_index,
            frame.start,
        )? {
            ctx.reserve_vec(&mut out, 1, "f3d sketch text records")?;
            out.push(text);
        }
    }
    Ok(out)
}

/// Decode sketch-text records carrying persistent identities, font metrics,
/// UTF-16 content, and an owning-sketch reference.
pub(crate) fn decode_sketch_texts(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<SketchText>, CodecError> {
    decode_sketch_streams(ctx, scan, decode_sketch_texts_from_stream)
}

/// Whether a sketch-text record carries one of the two parameter-reference
/// members. A record either writes the member, whose own presence byte then
/// says whether it targets a parameter, or omits the member entirely; nothing
/// ahead of the slot distinguishes the two.
#[derive(Clone, Copy)]
enum TextReferenceSlot {
    Omitted,
    Written,
}

const TEXT_REFERENCE_SLOTS: [TextReferenceSlot; 2] =
    [TextReferenceSlot::Omitted, TextReferenceSlot::Written];

/// Bytes between a sketch-text record's last class member and its
/// owning-sketch reference, in either identity form.
const SKETCH_TEXT_TRAILING_RUN: usize = 30;

/// How far a placement-transform element may sit from the constant a planar
/// rigid placement writes there. The transform is composed in floating point,
/// so its constants arrive rounded; the bound is far tighter than a run of
/// misframed bytes would meet.
const TEXT_PLACEMENT_TOLERANCE: f64 = 1.0e-9;

/// Bytes between the end of the stored `txt_tag` rotation and the four f32 RGBA
/// components. The first byte is zero and four further bytes are unclassified.
const TXT_TAG_POST_ROTATION_RUN: usize = 5;

/// Bytes between the text-anchor coordinates and the u32 text count in the
/// `txt_tag` form, from [`TXT_TAG_ANCHOR_MEMBER_VERSION`] onward.
const TXT_TAG_ANCHOR_RUN: usize = 11;

/// The `txt_tag` class version that adds the eleventh byte of the run between
/// the text anchor and the text count. Below it the run is ten bytes.
const TXT_TAG_ANCHOR_MEMBER_VERSION: u32 = 4;

/// The `txt_tag` class version from which a record writes its persistent
/// identity as a property-block key. A property block carrying neither identity
/// key belongs to a `txt_tag` record below this version, which stores no
/// persistent identity at all.
const TXT_TAG_IDENTITY_KEY_VERSION: u32 = 4;

/// Three unknown bytes, one i32 font weight, and eight further bytes between
/// the `txt_tag` form's counted reference run and the thirty-byte class tail.
const TXT_TAG_MEMBER_RUN: usize = 15;
const TXT_TAG_FONT_WEIGHT_AT: usize = 3;

/// Which identity key a sketch-text record carries. The key selects the layout
/// the record uses from the property block onward.
#[derive(Clone, Copy, PartialEq)]
enum SketchTextIdentity {
    /// `textex_tag`: a `1` byte, the width factor, a zero byte between the font
    /// family and the height, and the anchor inside a placement transform.
    TextexTag { width_factor: NonNegativeReal },
    /// `txt_tag`: a `0` byte, no width factor, the height directly after the
    /// font family, and the anchor stored on its own.
    TxtTag { rotation: Angle },
}

/// Read one parameter-reference slot in the given form, advancing `cursor` by
/// what that form occupies. An omitted member reads as a null reference.
fn read_text_reference<'a>(
    payload: &'a [u8],
    cursor: &mut usize,
    slot: TextReferenceSlot,
) -> Option<Reference<&'a str, crate::bytes::utf16::Utf16View<'a>>> {
    match slot {
        TextReferenceSlot::Omitted => Some(Reference::Null),
        TextReferenceSlot::Written => take_reference(payload, cursor),
    }
}

/// Read the four f32 RGBA colour components both sketch-text classes store,
/// advancing `cursor` past them. Every component is a fraction of full
/// intensity, so a run leaving `[0, 1]` says the record is misframed rather
/// than that the text carries an out-of-range colour.
fn read_sketch_text_color(payload: &[u8], cursor: &mut usize) -> Option<Color> {
    let mut view = View::over_retained(payload);
    view.seek(*cursor)?;
    let mut components = [0f32; 4];
    for component in &mut components {
        let value = view.f32_le()?;
        (0.0..=1.0).contains(&value).then_some(())?;
        *component = value;
    }
    *cursor = view.position();
    let [r, g, b, a] = components;
    Color::new(r, g, b, a)
}

/// Read the row-major 4×4 f64 placement transform a frame-text record stores,
/// advancing `cursor` past it, and reduce it to the anchor point in millimetres
/// and the rotation about that point in radians.
///
/// The transform is a planar rigid placement: its third row and column are the
/// identity's, its bottom row is `(0, 0, 0, 1)`, its translation lives in the
/// last column, and its 2×2 basis is a rotation of unit determinant carrying no
/// scale or shear. A run failing any of that is not a placement, so the record
/// is misframed.
fn read_text_placement(payload: &[u8], cursor: &mut usize) -> Option<TextPlacement<FinitePoint2>> {
    let elements = f64s_at::<16>(payload, *cursor)?;
    *cursor = cursor.checked_add(128)?;
    let at = |row: usize, column: usize| elements[row * 4 + column];
    let constant = |value: f64, expected: f64| (value - expected).abs() <= TEXT_PLACEMENT_TOLERANCE;
    let planar = [
        (0, 2, 0.0),
        (1, 2, 0.0),
        (2, 0, 0.0),
        (2, 1, 0.0),
        (2, 2, 1.0),
        (2, 3, 0.0),
        (3, 0, 0.0),
        (3, 1, 0.0),
        (3, 2, 0.0),
        (3, 3, 1.0),
    ]
    .into_iter()
    .all(|(row, column, expected)| constant(at(row, column), expected));
    let determinant = at(0, 0) * at(1, 1) - at(0, 1) * at(1, 0);
    (planar && constant(determinant, 1.0)).then_some(())?;
    let anchor = Point2::new(at(0, 3) * 10.0, at(1, 3) * 10.0);
    let anchor = FinitePoint2::new(anchor)?;
    Some(TextPlacement {
        anchor,
        rotation: Angle::new(at(1, 0).atan2(at(0, 0)))?,
    })
}

/// The record index a reference names, absent when the reference is null.
fn reference_index<G: AsRef<str>, L>(reference: &Reference<G, L>) -> Option<u32> {
    reference
        .target()
        .and_then(|target| u32::try_from(target).ok())
}

/// Class-level fields of a sketch-text record, ending at the text height.
struct SketchTextHead {
    entity_genesis: Option<u64>,
    persistent_id: Option<u64>,
    base_id: Option<u64>,
    font_family: String,
    height: PositiveLength,
    color: Color,
    cursor: usize,
}

/// Fields following the height, whose framing depends on the two slot forms.
struct SketchTextTail {
    layout: SketchTextLayout,
    text: String,
    font_weight: i32,
    owner_reference: u32,
}

/// Read the class-defined leading block: the presence byte, and when it is
/// `01`, a u32 count and that many `(reference, u32)` pairs. The `txt_tag` form
/// writes such a block; the `textex_tag` form writes the `00` byte alone.
fn read_sketch_text_leading_block(payload: &[u8], cursor: &mut usize) -> Option<()> {
    match payload.get(*cursor)? {
        0 => *cursor += 1,
        1 => {
            *cursor += 1;
            let count = usize::try_from(View::u32_le_at(payload, *cursor)?).ok()?;
            if count > MAX_RELATION_RUN {
                return None;
            }
            *cursor = cursor.checked_add(4)?;
            for _ in 0..count {
                take_reference(payload, cursor)?;
                View::u32_le_at(payload, *cursor)?;
                *cursor = cursor.checked_add(4)?;
            }
        }
        _ => return None,
    }
    Some(())
}

/// Read the record prefix, property block, and the metrics up to the height.
fn decode_sketch_text_head(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    class_version: u32,
) -> Result<Option<(SketchTextHead, SketchTextIdentity)>, CodecError> {
    (|| {
        // Record prefix: the LP-ASCII class tag, the u64 entity ID, and the
        // LP-ASCII record name.
        let (_, after_tag) = lp_ascii_filtered_view(payload, 0, 3..=3, u8::is_ascii_digit)?;
        let (_, mut cursor) = lp_ascii_filtered_view(
            payload,
            after_tag.checked_add(8)?,
            0..=256,
            u8::is_ascii_graphic,
        )?;
        read_sketch_text_leading_block(payload, &mut cursor)?;
        // The block carries the text identities under keys that vary by record, so
        // each is addressed by name. The identity key is `textex_tag` or `txt_tag`,
        // and which of the two the record carries selects the layout that follows.
        // A `txt_tag` record below TXT_TAG_IDENTITY_KEY_VERSION writes neither key
        // and carries no persistent identity, so its class version selects the
        // layout in place of a key.
        let properties = read_property_block(payload, &mut cursor)?;
        let property = |key: &str| properties.find(key);
        let is_txt_tag = match (property("textex_tag"), property("txt_tag")) {
            (Some(_), _) => false,
            (None, Some(_)) => true,
            (None, None) if class_version < TXT_TAG_IDENTITY_KEY_VERSION => true,
            (None, None) => return None,
        };
        let (identity, color, persistent_id) = if is_txt_tag {
            let rotation = Angle::new(View::f64_le_at(payload, cursor)?)?;
            cursor = cursor.checked_add(8)?;
            (payload.get(cursor)? == &0).then_some(())?;
            cursor = cursor.checked_add(TXT_TAG_POST_ROTATION_RUN)?;
            let color = read_sketch_text_color(payload, &mut cursor)?;
            (
                SketchTextIdentity::TxtTag { rotation },
                color,
                property("txt_tag"),
            )
        } else {
            (payload.get(cursor)? == &1).then_some(())?;
            cursor += 1;
            let width_factor = NonNegativeReal::new(View::f64_le_at(payload, cursor)?)?;
            cursor = cursor.checked_add(8)?;
            let color = read_sketch_text_color(payload, &mut cursor)?;
            (
                SketchTextIdentity::TextexTag { width_factor },
                color,
                property("textex_tag"),
            )
        };
        let font_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
        if font_count == 0 || font_count > 1_024 {
            return None;
        }
        let (font_family, after_font) = match sketch_utf16_text(ctx, payload, cursor, font_count) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        cursor = after_font;
        if matches!(identity, SketchTextIdentity::TextexTag { .. }) {
            (payload.get(cursor)? == &0).then_some(())?;
            cursor += 1;
        }
        let height = PositiveLength::new(View::f64_le_at(payload, cursor)? * 10.0)?;
        cursor = cursor.checked_add(8)?;
        Some(Ok((
            SketchTextHead {
                entity_genesis: property("EntityGenesis"),
                persistent_id,
                base_id: property("txt_tag_base"),
                font_family,
                height,
                color,
                cursor,
            },
            identity,
        )))
    })()
    .transpose()
}

/// Read the indexed Design form of a `textex_tag` record. It has no leading
/// relation block or record-name string: the indexed header is followed by a
/// nine-byte zero entity lane and the ordinary property block. Its one-byte
/// width prefix is zero, unlike the legacy class form's one-byte prefix of
/// one; the f64 width factor and the remaining metrics have the same roles.
fn decode_indexed_sketch_text_head(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<(SketchTextHead, NonNegativeReal)>, CodecError> {
    (|| {
        let (_, after_tag) = lp_ascii_filtered_view(payload, 0, 3..=3, u8::is_ascii_digit)?;
        if after_tag != 7
            || View::u32_le_at(payload, after_tag).is_none()
            || payload.get(11..20)?.iter().any(|byte| *byte != 0)
        {
            return None;
        }
        let mut cursor = 20;
        let properties = read_property_block(payload, &mut cursor)?;
        let property = |key: &str| properties.find(key);
        let persistent_id = property("textex_tag")?;
        (payload.get(cursor)? == &0).then_some(())?;
        cursor += 1;
        let width_factor = NonNegativeReal::new(View::f64_le_at(payload, cursor)?)?;
        cursor = cursor.checked_add(8)?;
        let color = read_sketch_text_color(payload, &mut cursor)?;
        let font_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
        if font_count == 0 || font_count > 1_024 {
            return None;
        }
        let (font_family, after_font) = match sketch_utf16_text(ctx, payload, cursor, font_count) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        cursor = after_font;
        (payload.get(cursor)? == &0).then_some(())?;
        cursor += 1;
        let height = PositiveLength::new(View::f64_le_at(payload, cursor)? * 10.0)?;
        cursor = cursor.checked_add(8)?;
        Some(Ok((
            SketchTextHead {
                entity_genesis: property("EntityGenesis"),
                persistent_id: Some(persistent_id),
                base_id: property("txt_tag_base"),
                font_family,
                height,
                color,
                cursor,
            },
            width_factor,
        )))
    })()
    .transpose()
}

/// Read the alignment fields, text content, and class tail under one pair of
/// slot forms, requiring the walk to end exactly on the owning-sketch
/// reference.
fn decode_sketch_text_tail(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    mut cursor: usize,
    first_slot: TextReferenceSlot,
    second_slot: TextReferenceSlot,
    width_factor: NonNegativeReal,
) -> Result<Option<SketchTextTail>, CodecError> {
    (|| {
        let first_reference = read_text_reference(payload, &mut cursor, first_slot)?;
        // Horizontal alignment enum and three flag bytes.
        let horizontal_alignment = View::u32_le_at(payload, cursor)?;
        cursor = cursor.checked_add(7)?;
        let text_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
        if text_count == 0 || text_count > 1_048_576 {
            return None;
        }
        let (text, after_text) = match sketch_utf16_text(ctx, payload, cursor, text_count) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        cursor = after_text;
        let second_reference = read_text_reference(payload, &mut cursor, second_slot)?;
        // Vertical alignment enum, one flag byte, and the font weight.
        let vertical_alignment = View::u32_le_at(payload, cursor)?;
        let font_weight = i32::try_from(View::u32_le_at(payload, cursor.checked_add(5)?)?).ok()?;
        matches!(font_weight, 400 | 500 | 750).then_some(())?;
        cursor = cursor.checked_add(9)?;
        // The class tail opens with the text-type enum, which gates the placement
        // transform: frame text stores a 4x4 transform, path text stores none. One
        // flag byte follows the enum and repeats it, so a slot form that has
        // desynchronized fails here instead of framing a transform out of whatever
        // bytes the walk landed on.
        let text_type = View::u32_le_at(payload, cursor)?;
        (u32::from(*payload.get(cursor.checked_add(4)?)?) == text_type).then_some(())?;
        cursor = cursor.checked_add(5)?;
        let placement = match text_type {
            0 => Some(read_text_placement(payload, &mut cursor)?),
            1 => None,
            _ => return None,
        };
        cursor = cursor.checked_add(SKETCH_TEXT_TRAILING_RUN)?;
        let owner = take_reference(payload, &mut cursor)?;
        (cursor == payload.len()).then_some(())?;
        Some(Ok(SketchTextTail {
            layout: SketchTextLayout::TextexTag {
                width_factor,
                alignment: Some(SketchTextAlignment {
                    horizontal: horizontal_alignment,
                    vertical: vertical_alignment,
                }),
                first_reference: reference_index(&first_reference),
                second_reference: reference_index(&second_reference),
                placement,
            },
            text,
            font_weight,
            owner_reference: reference_index(&owner)?,
        }))
    })()
    .transpose()
}

/// Read the `txt_tag` form's members from the height to the end of the record.
/// Two bytes separate the height from the anchor coordinates, which this form
/// stores directly rather than in a placement transform. The form writes no
/// parameter-reference slot: an eleven-byte unclassified member run, ten bytes
/// below [`TXT_TAG_ANCHOR_MEMBER_VERSION`], follows the anchor. The text string
/// is followed by a counted reference run, fifteen bytes, and the trailing run
/// and owning-sketch reference that close both forms.
fn decode_txt_tag_sketch_text_tail(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    mut cursor: usize,
    class_version: u32,
    rotation: Angle,
) -> Result<Option<SketchTextTail>, CodecError> {
    (|| {
        cursor = cursor.checked_add(2)?;
        let anchor = Point2::new(
            View::f64_le_at(payload, cursor)? * 10.0,
            View::f64_le_at(payload, cursor.checked_add(8)?)? * 10.0,
        );
        let anchor = FinitePoint2::new(anchor)?;
        let anchor_run = if class_version < TXT_TAG_ANCHOR_MEMBER_VERSION {
            TXT_TAG_ANCHOR_RUN - 1
        } else {
            TXT_TAG_ANCHOR_RUN
        };
        cursor = cursor.checked_add(16 + anchor_run)?;
        let text_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
        if text_count == 0 || text_count > 1_048_576 {
            return None;
        }
        let (text, after_text) = match sketch_utf16_text(ctx, payload, cursor, text_count) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        cursor = after_text;
        let references = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
        if references > MAX_RELATION_RUN {
            return None;
        }
        cursor = cursor.checked_add(4)?;
        for _ in 0..references {
            take_reference(payload, &mut cursor)?;
        }
        let font_weight = i32::try_from(View::u32_le_at(
            payload,
            cursor.checked_add(TXT_TAG_FONT_WEIGHT_AT)?,
        )?)
        .ok()?;
        matches!(font_weight, 400 | 500 | 750).then_some(())?;
        cursor = cursor.checked_add(TXT_TAG_MEMBER_RUN + SKETCH_TEXT_TRAILING_RUN)?;
        let owner = take_reference(payload, &mut cursor)?;
        (cursor == payload.len()).then_some(())?;
        Some(Ok(SketchTextTail {
            layout: SketchTextLayout::TxtTag {
                placement: TextPlacement { anchor, rotation },
            },
            text,
            font_weight,
            owner_reference: reference_index(&owner)?,
        }))
    })()
    .transpose()
}

/// Read the indexed `textex_tag` tail. The member and alignment fields match
/// the common text body. The indexed class then writes a fixed 35-byte suffix:
/// a text-type u32, three fixed u32 values, a zero u32, a two-byte zero run,
/// two positive f32 scales, and a five-byte zero run before the owning-sketch
/// reference. The text-type values are the same frame (`0`) and path (`1`)
/// discriminators used by the legacy class tail. The indexed suffix carries no
/// neutral anchor or rotation; the source record is retained in full.
fn decode_indexed_sketch_text_tail(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    mut cursor: usize,
    first_slot: TextReferenceSlot,
    second_slot: TextReferenceSlot,
    width_factor: NonNegativeReal,
) -> Result<Option<SketchTextTail>, CodecError> {
    (|| {
        let first_reference = read_text_reference(payload, &mut cursor, first_slot)?;
        let horizontal_alignment = View::u32_le_at(payload, cursor)?;
        cursor = cursor.checked_add(7)?;
        let text_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
        if text_count == 0 || text_count > 1_048_576 {
            return None;
        }
        let (text, after_text) = match sketch_utf16_text(ctx, payload, cursor, text_count) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        cursor = after_text;
        let second_reference = read_text_reference(payload, &mut cursor, second_slot)?;
        let vertical_alignment = View::u32_le_at(payload, cursor)?;
        let font_weight = i32::try_from(View::u32_le_at(payload, cursor.checked_add(5)?)?).ok()?;
        matches!(font_weight, 400 | 500 | 750).then_some(())?;
        cursor = cursor.checked_add(9)?;
        if !matches!(View::u32_le_at(payload, cursor)?, 0 | 1)
            || View::u32_le_at(payload, cursor.checked_add(4)?)? != 1
            || View::u32_le_at(payload, cursor.checked_add(8)?)? != 256
            || View::u32_le_at(payload, cursor.checked_add(12)?)? != 0
            || View::u32_le_at(payload, cursor.checked_add(16)?)? != 0
            || payload
                .get(cursor.checked_add(20)?..cursor.checked_add(22)?)?
                .iter()
                .any(|byte| *byte != 0)
        {
            return None;
        }
        let mut scale_view = View::over_retained(payload);
        scale_view.seek(cursor.checked_add(22)?)?;
        let scale_u = scale_view.f32_le()?;
        let scale_v = scale_view.f32_le()?;
        if !scale_u.is_finite() || !scale_v.is_finite() || scale_u <= 0.0 || scale_v <= 0.0 {
            return None;
        }
        (payload
            .get(scale_view.position()..cursor.checked_add(35)?)?
            .iter()
            .all(|byte| *byte == 0))
        .then_some(())?;
        cursor = cursor.checked_add(35)?;
        let owner = take_reference(payload, &mut cursor)?;
        (cursor == payload.len()).then_some(())?;
        Some(Ok(SketchTextTail {
            layout: SketchTextLayout::TextexTag {
                width_factor,
                alignment: Some(SketchTextAlignment {
                    horizontal: horizontal_alignment,
                    vertical: vertical_alignment,
                }),
                first_reference: reference_index(&first_reference),
                second_reference: reference_index(&second_reference),
                placement: None,
            },
            text,
            font_weight,
            owner_reference: reference_index(&owner)?,
        }))
    })()
    .transpose()
}

fn decode_indexed_sketch_text_record_tail(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    cursor: usize,
    width_factor: NonNegativeReal,
) -> Result<Option<SketchTextTail>, CodecError> {
    let mut closed = None;
    for first_slot in TEXT_REFERENCE_SLOTS {
        for second_slot in TEXT_REFERENCE_SLOTS {
            let Some(tail) = decode_indexed_sketch_text_tail(
                ctx,
                payload,
                cursor,
                first_slot,
                second_slot,
                width_factor,
            )?
            else {
                continue;
            };
            if closed.replace(tail).is_some() {
                return Ok(None);
            }
        }
    }
    Ok(closed)
}

struct SketchTextRecord<'a> {
    stream: &'a str,
    class_tag: crate::records::references::DesignClassTag,
    class_version: u32,
    record_index: u32,
    byte_offset: usize,
    head: SketchTextHead,
    tail: SketchTextTail,
}

fn assemble_sketch_text(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    input: SketchTextRecord<'_>,
) -> Result<SketchText, CodecError> {
    let SketchTextRecord {
        stream,
        class_tag,
        class_version,
        record_index,
        byte_offset,
        head,
        tail,
    } = input;
    let id = design_record_id_charged(
        ctx,
        stream,
        ":sketch-text#",
        u64::try_from(byte_offset)
            .map_err(|_| ctx.refuse_codec_limit("f3d sketch text offset", 0, 1))?,
        "f3d sketch text identifier",
    )?;
    let raw_bytes = ctx.copy_retained(payload, "f3d sketch text raw bytes")?;
    Ok(SketchText {
        id,
        record_index,
        owner_reference: tail.owner_reference,
        class_tag,
        class_version,
        byte_offset: u64_from_index(byte_offset),
        entity_genesis: head.entity_genesis,
        persistent_id: head.persistent_id,
        base_id: head.base_id,
        text: tail.text,
        font_family: head.font_family,
        font_weight: tail.font_weight,
        height: head.height,
        color: head.color,
        layout: tail.layout,
        raw_bytes: raw_bytes.into(),
    })
}

pub(crate) fn decode_sketch_text_record(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    stream: &str,
    class_tag: crate::records::references::DesignClassTag,
    class_version: u32,
    record_index: u32,
    byte_offset: usize,
) -> Result<Option<SketchText>, CodecError> {
    (|| {
        let legacy_head = match decode_sketch_text_head(ctx, payload, class_version) {
            Ok(head) => head,
            Err(error) => return Some(Err(error)),
        };
        if let Some((head, identity)) = legacy_head {
            let tail = match identity {
                SketchTextIdentity::TextexTag { width_factor } => {
                    let mut closed = None;
                    let mut ambiguous = false;
                    'forms: for first_slot in TEXT_REFERENCE_SLOTS {
                        for second_slot in TEXT_REFERENCE_SLOTS {
                            let tail = decode_sketch_text_tail(
                                ctx,
                                payload,
                                head.cursor,
                                first_slot,
                                second_slot,
                                width_factor,
                            );
                            let Some(tail) = (match tail {
                                Ok(tail) => tail,
                                Err(error) => return Some(Err(error)),
                            }) else {
                                continue;
                            };
                            // Two slot forms both ending on the owning-sketch reference
                            // leave the parameter references undetermined.
                            if closed.replace(tail).is_some() {
                                ambiguous = true;
                                break 'forms;
                            }
                        }
                    }
                    if ambiguous {
                        None
                    } else {
                        closed
                    }
                }
                SketchTextIdentity::TxtTag { rotation } => match decode_txt_tag_sketch_text_tail(
                    ctx,
                    payload,
                    head.cursor,
                    class_version,
                    rotation,
                ) {
                    Ok(tail) => tail,
                    Err(error) => return Some(Err(error)),
                },
            };
            if let Some(tail) = tail {
                return Some(assemble_sketch_text(
                    ctx,
                    payload,
                    crate::design::decode::sketch::SketchTextRecord {
                        stream,
                        class_tag,
                        class_version,
                        record_index,
                        byte_offset,
                        head,
                        tail,
                    },
                ));
            }
        }
        let (head, width_factor) = match decode_indexed_sketch_text_head(ctx, payload) {
            Ok(head) => head?,
            Err(error) => return Some(Err(error)),
        };
        let tail =
            match decode_indexed_sketch_text_record_tail(ctx, payload, head.cursor, width_factor) {
                Ok(tail) => tail?,
                Err(error) => return Some(Err(error)),
            };
        Some(assemble_sketch_text(
            ctx,
            payload,
            crate::design::decode::sketch::SketchTextRecord {
                stream,
                class_tag,
                class_version,
                record_index,
                byte_offset,
                head,
                tail,
            },
        ))
    })()
    .transpose()
}

#[derive(Debug)]
struct DecodedSketchPoint {
    owner_reference: Option<u32>,
    coordinate_offset: u32,
    record_form: SketchPointRecordForm,
    paired_reference: u32,
    coordinates: [f64; 2],
}

impl DecodedSketchPoint {
    fn trailing_reference(&self) -> Option<u32> {
        match self.record_form {
            SketchPointRecordForm::Version10InlineTyped {
                trailing_reference, ..
            }
            | SketchPointRecordForm::Version11InlineTyped {
                trailing_reference, ..
            } => Some(trailing_reference),
            _ => self.owner_reference,
        }
    }
}

fn take_local_sketch_reference<'a>(
    payload: &'a [u8],
    cursor: &mut usize,
) -> Option<(u32, Option<&'a str>)> {
    let reference = take_reference(payload, cursor)?;
    let (target, inline_type_guid) = reference.into_local()?;
    Some((u32::try_from(target).ok()?, inline_type_guid))
}

fn take_same_segment_sketch_reference(payload: &[u8], cursor: &mut usize) -> Option<u32> {
    let (target, inline_type_guid) = take_local_sketch_reference(payload, cursor)?;
    inline_type_guid.is_none().then_some(target)
}

fn decode_version_zero_sketch_point(
    payload: &[u8],
    header_end: usize,
) -> Option<DecodedSketchPoint> {
    let mut cursor = header_end;
    let prefix_end = cursor.checked_add(10)?;
    if payload.get(cursor..prefix_end) != Some(&[0; 10][..]) {
        return None;
    }
    cursor = prefix_end;
    let paired_reference = take_same_segment_sketch_reference(payload, &mut cursor)?;
    let flag = *payload.get(cursor)?;
    if flag > 1 {
        return None;
    }
    cursor = cursor.checked_add(1)?;
    let coordinate_offset = u32::try_from(cursor).ok()?;
    let x = View::f64_le_at(payload, cursor)?;
    let y = View::f64_le_at(payload, cursor.checked_add(8)?)?;
    cursor = cursor.checked_add(16)?;
    if payload.get(cursor..cursor.checked_add(20)?) != Some(&[0; 20][..])
        || View::f32_le_at(payload, cursor + 20) != Some(1.0)
        || payload.get(cursor + 24..cursor + 36) != Some(&[0; 12][..])
        || View::f32_le_at(payload, cursor + 36) != Some(1.0)
        || View::f32_le_at(payload, cursor + 40) != Some(1.0)
        || payload.get(cursor + 44..cursor + 54) != Some(&[1, 1, 0, 0, 0, 0, 1, 0, 0, 0][..])
    {
        return None;
    }
    cursor = cursor.checked_add(54)?;
    if take_same_segment_sketch_reference(payload, &mut cursor)? != paired_reference {
        return None;
    }
    let owner_reference = take_same_segment_sketch_reference(payload, &mut cursor)?;
    if cursor != payload.len() {
        return None;
    }
    Some(DecodedSketchPoint {
        owner_reference: Some(owner_reference),
        coordinate_offset,
        record_form: SketchPointRecordForm::Version0 { flag: flag == 1 },
        paired_reference,
        coordinates: [x, y],
    })
}

fn decode_sketch_point_record(payload: &[u8], class_version: u32) -> Option<DecodedSketchPoint> {
    let (_, after_class_tag) = lp_ascii_filtered_view(payload, 0, 3..=3, u8::is_ascii_digit)?;
    let header_end = after_class_tag.checked_add(4)?;
    if class_version == 0 {
        return decode_version_zero_sketch_point(payload, header_end);
    }
    if !matches!(class_version, 8 | 10 | 11)
        || payload.get(header_end..header_end.checked_add(9)?) != Some(&[0; 9][..])
    {
        return None;
    }
    let mut cursor = header_end.checked_add(9)?;
    let properties = read_property_block(payload, &mut cursor)?;
    let (entity_genesis, persistent_id) = match properties.first_two()? {
        (
            SketchProperty {
                name: "pt_tag",
                value: persistent_id,
            },
            None,
        ) => (None, persistent_id),
        (
            SketchProperty {
                name: "EntityGenesis",
                value: entity_genesis,
            },
            Some(SketchProperty {
                name: "pt_tag",
                value: persistent_id,
            }),
        ) if class_version == 11 => (Some(entity_genesis), persistent_id),
        _ => return None,
    };
    let persistent_id = std::num::NonZeroU64::new(persistent_id)?;
    let (paired_reference, paired_type_guid) = take_local_sketch_reference(payload, &mut cursor)?;
    let inline_typed = match (class_version, paired_type_guid) {
        (8 | 10 | 11, None) => false,
        (10 | 11, Some(type_guid))
            if type_guid.eq_ignore_ascii_case(SKETCH_POINT_COMPANION_TYPE.0) =>
        {
            true
        }
        _ => return None,
    };
    let flag_count = if class_version == 11 { 8 } else { 7 };
    let flags_end = cursor.checked_add(flag_count)?;
    let source_flags = payload.get(cursor..flags_end)?;
    if source_flags.iter().any(|flag| *flag > 1) {
        return None;
    }
    let mut flags = [0; 8];
    flags[..flag_count].copy_from_slice(source_flags);
    cursor = flags_end;
    let coordinate_offset = u32::try_from(cursor).ok()?;
    let coordinates = [
        View::f64_le_at(payload, cursor)?,
        View::f64_le_at(payload, cursor.checked_add(8)?)?,
    ];
    let depth = View::f64_le_at(payload, cursor.checked_add(16)?)? * 10.0;
    cursor = cursor.checked_add(24)?;
    let selector = View::u64_le_at(payload, cursor)?;
    let state = *payload.get(cursor.checked_add(8)?)?;
    let reserved_zero_count = if class_version == 8 { 8 } else { 12 };
    let floats_at = cursor.checked_add(9 + reserved_zero_count)?;
    if payload
        .get(cursor + 9..floats_at)?
        .iter()
        .any(|byte| *byte != 0)
        || View::f32_le_at(payload, floats_at) != Some(1.0)
        || View::f32_le_at(payload, floats_at + 4) != Some(1.0)
        || payload.get(floats_at + 8..floats_at + 13) != Some(&[0, 1, 0, 0, 0][..])
    {
        return None;
    }
    cursor = floats_at.checked_add(13)?;
    let (repeated_reference, repeated_type_guid) =
        take_local_sketch_reference(payload, &mut cursor)?;
    let repeated_encoding_matches = match repeated_type_guid {
        None => !inline_typed,
        Some(type_guid) => {
            inline_typed && type_guid.eq_ignore_ascii_case(SKETCH_POINT_COMPANION_TYPE.0)
        }
    };
    if repeated_reference != paired_reference || !repeated_encoding_matches {
        return None;
    }
    let closure = SketchPointClosure::from_pair(selector, state)?;
    let mut seven = [0; 7];
    seven.copy_from_slice(&flags[..7]);
    let (record_form, owner_reference) = match (class_version, inline_typed) {
        (8, false) => {
            if closure != SketchPointClosure::Selector0State0 {
                return None;
            }
            let owner = take_same_segment_sketch_reference(payload, &mut cursor)?;
            (
                SketchPointRecordForm::Version8 {
                    depth,
                    persistent_id,
                    flags: seven.map(|flag| flag == 1),
                },
                Some(owner),
            )
        }
        (10, false) => {
            let owner = take_same_segment_sketch_reference(payload, &mut cursor)?;
            (
                SketchPointRecordForm::Version10 {
                    depth,
                    persistent_id,
                    flags: seven.map(|flag| flag == 1),
                    closure: crate::records::sketch_geometry::SketchPointClosure10::from_closure(
                        closure,
                    )?,
                },
                Some(owner),
            )
        }
        (10, true) => {
            let (trailing_reference, type_guid) =
                take_local_sketch_reference(payload, &mut cursor)?;
            if type_guid
                .is_none_or(|type_guid| !type_guid.eq_ignore_ascii_case(SKETCH_CONTAINER_TYPE_GUID))
            {
                return None;
            }
            (
                SketchPointRecordForm::Version10InlineTyped {
                    depth,
                    trailing_reference,
                    persistent_id,
                    flags: seven.map(|flag| flag == 1),
                    closure:
                        crate::records::sketch_geometry::SketchPointClosure10Inline::from_closure(
                            closure,
                        )?,
                },
                None,
            )
        }
        (11, true) => {
            let (trailing_reference, type_guid) =
                take_local_sketch_reference(payload, &mut cursor)?;
            if type_guid
                .is_none_or(|type_guid| !type_guid.eq_ignore_ascii_case(SKETCH_CONTAINER_TYPE_GUID))
            {
                return None;
            }
            (
                SketchPointRecordForm::Version11InlineTyped {
                    depth,
                    entity_genesis,
                    trailing_reference,
                    companion_prefix_present_zero: false,
                    persistent_id,
                    flags: flags.map(|flag| flag == 1),
                    closure,
                },
                None,
            )
        }
        (11, false) => {
            let padded_paired_reference = match payload.len().checked_sub(cursor)? {
                11 => false,
                15 if payload.get(cursor..cursor + 4) == Some(&[0; 4][..]) => {
                    cursor += 4;
                    true
                }
                _ => return None,
            };
            let owner = take_same_segment_sketch_reference(payload, &mut cursor)?;
            (
                SketchPointRecordForm::Version11 {
                    depth,
                    entity_genesis,
                    padded_paired_reference,
                    companion_prefix_present_zero: false,
                    persistent_id,
                    flags: flags.map(|flag| flag == 1),
                    closure,
                },
                Some(owner),
            )
        }
        _ => return None,
    };
    if cursor != payload.len() {
        return None;
    }
    Some(DecodedSketchPoint {
        owner_reference,
        coordinate_offset,
        record_form,
        paired_reference,
        coordinates,
    })
}

fn point_target_has_guid(
    types_by_entity: &HashMap<u32, (&str, u32, &str)>,
    target: Option<u32>,
    expected_guid: &str,
) -> bool {
    let Some(target) = target else {
        return false;
    };
    types_by_entity
        .get(&target)
        .is_some_and(|(type_guid, _, _)| type_guid.eq_ignore_ascii_case(expected_guid))
}

fn decode_sketch_point_companion(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    point_record_index: u32,
    record_form: SketchPointRecordForm,
    types_by_entity: &HashMap<u32, (&str, u32, &str)>,
) -> Result<Option<(SketchPointRecordForm, SketchPointCompanion)>, CodecError> {
    let parsed = (|| {
        let reference_encoding = SketchPointCompanionReferenceEncoding::for_form(&record_form);
        let (prefix_present_zero, mut cursor) = if payload.get(11..21) == Some(&[0; 10][..]) {
            (false, 21)
        } else if payload.get(11..20) == Some(&[0; 9][..])
            && payload.get(20..25) == Some(&[1, 0, 0, 0, 0][..])
        {
            (true, 25)
        } else {
            return None;
        };
        let record_form = record_form.with_companion_prefix_present_zero(prefix_present_zero)?;
        let count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
        if count > MAX_RELATION_RUN {
            return None;
        }
        cursor = cursor.checked_add(4)?;
        Some((reference_encoding, record_form, cursor, count))
    })();
    let Some((reference_encoding, record_form, mut cursor, count)) = parsed else {
        return Ok(None);
    };

    let mut incident_curves = Vec::new();
    ctx.reserve_vec(
        &mut incident_curves,
        count,
        "f3d sketch point incident curves",
    )?;
    let parsed = (|| {
        for _ in 0..count {
            let (target, type_guid) = take_local_sketch_reference(payload, &mut cursor)?;
            let registered_type = types_by_entity.get(&target)?;
            match reference_encoding {
                SketchPointCompanionReferenceEncoding::SameSegment if type_guid.is_none() => {}
                SketchPointCompanionReferenceEncoding::InlineTyped
                    if type_guid.is_some_and(|type_guid| {
                        type_guid.eq_ignore_ascii_case(registered_type.0)
                    }) => {}
                _ => return None,
            }
            incident_curves.push(target);
        }
        if payload.get(cursor) != Some(&0) {
            return None;
        }
        cursor = cursor.checked_add(1)?;
        let (inverse, inverse_type_guid) = take_local_sketch_reference(payload, &mut cursor)?;
        let inverse_encoding_matches = match reference_encoding {
            SketchPointCompanionReferenceEncoding::SameSegment => inverse_type_guid.is_none(),
            SketchPointCompanionReferenceEncoding::InlineTyped => inverse_type_guid
                .is_some_and(|type_guid| type_guid.eq_ignore_ascii_case(SKETCH_POINT_TYPE_GUID)),
        };
        if inverse != point_record_index || !inverse_encoding_matches || cursor != payload.len() {
            return None;
        }
        Some((record_form, SketchPointCompanion { incident_curves }))
    })();
    Ok(parsed)
}

const SKETCH_POINT_TYPE_GUID: &str = "C2CEDAE7-1716-47C1-B7B1-07B70081D0FB";
pub(crate) const CURRENT_SKETCH_POINT_TYPE: (&str, u32, &str) =
    (SKETCH_POINT_TYPE_GUID, 11, "Geometry");
pub(crate) const SKETCH_POINT_COMPANION_TYPE: (&str, u32, &str) =
    ("362B7EC3-0F09-47C8-A3BE-DC066715CDAE", 0, "Geometry");
pub(crate) const SKETCH_CONTAINER_TYPE_GUID: &str = "44A64366-4BD3-4B24-881A-F94C206E8F2D";
pub(crate) const CURRENT_SKETCH_LINE_TYPE: (&str, u32, &str) =
    ("DCA267ED-D615-4934-B64F-AD805E8003E2", 2, "Geometry");
pub(crate) const CURRENT_SKETCH_CIRCULAR_TYPE: (&str, u32, &str) =
    ("F0130424-8B7E-4092-93C9-1CA807482534", 0, "Geometry");
pub(crate) const CURRENT_SKETCH_NURBS_TYPE: (&str, u32, &str) =
    ("D82E012F-6DDD-4AED-BDE1-C0F7F9100B9B", 3, "MSketch");

const SKETCH_LINE_TYPES: [(&str, u32, &str); 6] = [
    CURRENT_SKETCH_LINE_TYPE,
    ("DCA267ED-D615-4934-B64F-AD805E8003E2", 1, "Geometry"),
    ("EA3B930A-3383-4AD3-BE25-4B2814EA3985", 0, "Geometry"),
    ("AE42BAB6-643F-4169-A33C-529C8E0A4D84", 0, "Geometry"),
    ("F279874A-17AB-43DA-BF8E-80259802D06E", 0, "Geometry"),
    ("58751243-FEA8-41E6-BBC9-37960EB8164B", 0, "Geometry"),
];
const SKETCH_CIRCULAR_TYPES: [(&str, u32, &str); 2] = [
    CURRENT_SKETCH_CIRCULAR_TYPE,
    ("FF23079A-D99C-47AB-940E-2F4E18F022AB", 2, "Geometry"),
];
const SKETCH_TEXT_FRAME_LINE_TYPE_GUID: &str = "16DEFC4D-1816-4FB0-8E39-9BDA23954248";

/// Geometry grammar selected by a sketch curve's stable type identity and
/// record version. Dynamic class tags are segment-local table ordinals and do
/// not identify a family without this resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SketchCurveClass {
    Line,
    Circular,
    Nurbs,
    TextFrameLine,
}

impl SketchCurveClass {
    fn of(type_guid: &str, version: u32, module: &str) -> Option<Self> {
        let matches = |(known_guid, known_version, known_module): &(&str, u32, &str)| {
            version == *known_version
                && module == *known_module
                && type_guid.eq_ignore_ascii_case(known_guid)
        };
        if SKETCH_LINE_TYPES.iter().any(matches) {
            Some(Self::Line)
        } else if SKETCH_CIRCULAR_TYPES.iter().any(matches) {
            Some(Self::Circular)
        } else if type_guid.eq_ignore_ascii_case(CURRENT_SKETCH_NURBS_TYPE.0)
            && version == CURRENT_SKETCH_NURBS_TYPE.1
            && module == CURRENT_SKETCH_NURBS_TYPE.2
        {
            Some(Self::Nurbs)
        } else if type_guid.eq_ignore_ascii_case(SKETCH_TEXT_FRAME_LINE_TYPE_GUID)
            && version == 0
            && module == "MSketch"
        {
            Some(Self::TextFrameLine)
        } else {
            None
        }
    }
}

/// Decode every sketch-curve record ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata), `crv_primary_id`/
/// `crv_secondary_id`) from each design `BulkStream` entry in `scan`: the
/// curve's persistent primary and secondary identities plus its NURBS, circular
/// arc, line, or referenced analytic geometry.
fn decode_sketch_curve_identities_from_stream(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    stream: &str,
) -> Result<Vec<SketchCurveIdentity>, CodecError> {
    let mut out = Vec::new();
    for frame in design_primary_frames(ctx, bytes, meta)? {
        let payload = &bytes[frame.start..frame.end];
        let Some((primary_id, secondary_id, geometry_shift, entity_genesis)) =
            decode_sketch_curve_identity(payload)
        else {
            continue;
        };
        let Some(primary_id) = std::num::NonZeroU64::new(primary_id) else {
            continue;
        };
        let record_index = u32::try_from(frame.entity_id)
            .map_err(|_| CodecError::Malformed("F3D sketch-curve entity ID exceeds u32".into()))?;
        let curve_class = SketchCurveClass::of(
            frame.design_type.type_guid.as_str(),
            frame.design_type.version,
            &frame.design_type.module,
        );
        let parsed_geometry = if let Some(curve_class) = curve_class {
            decode_sketch_curve_geometry(
                ctx,
                payload,
                geometry_shift,
                record_index,
                curve_class,
                frame.start,
            )?
        } else {
            None
        };
        let (geometry, geometry_offset) = parsed_geometry
            .map_or((None, geometry_shift + 133), |parsed| {
                (Some(parsed.geometry), parsed.geometry_offset)
            });

        ctx.reserve_vec(&mut out, 1, "f3d sketch curve output")?;
        out.push(SketchCurveIdentity {
            id: design_record_id_charged(
                ctx,
                stream,
                ":sketch-curve-identity#",
                u64_from_index(frame.start),
                "f3d sketch curve ID",
            )?,
            record_index,
            owner_reference: trailing_sketch_owner_reference(payload),
            class_tag: frame.class_tag,
            byte_offset: u64_from_index(frame.start),
            geometry_offset: u32::try_from(geometry_offset).map_err(|_| {
                ctx.refuse_codec_limit(
                    "f3d sketch curve geometry offset",
                    u64::from(u32::MAX),
                    u64_from_index(geometry_offset),
                )
            })?,
            entity_genesis,
            primary_id,
            secondary_id,
            geometry,
        });
    }
    Ok(out)
}

/// Decode every sketch-curve record ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata), `crv_primary_id`/
/// `crv_secondary_id`) from each design `BulkStream` entry in `scan`: the
/// curve's persistent primary and secondary identities plus its NURBS, circular
/// arc, line, or referenced analytic geometry.
pub(crate) fn decode_sketch_curve_identities(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<SketchCurveIdentity>, CodecError> {
    decode_sketch_streams(ctx, scan, decode_sketch_curve_identities_from_stream)
}

#[derive(Debug)]
struct ParsedSketchSurface {
    entity_genesis: Option<u64>,
    persistent_id: std::num::NonZeroU64,
    geometry: SketchSurfaceGeometry,
}

struct SketchSurfaceFrame {
    entity_genesis: Option<u64>,
    persistent_id: std::num::NonZeroU64,
    u_degree: u32,
    v_degree: u32,
    u_knots_at: usize,
    u_knot_count: usize,
    v_knots_at: usize,
    v_knot_count: usize,
    coordinate_count: usize,
    u_count: usize,
    v_count: usize,
}

fn parse_sketch_surface_frame(payload: &[u8]) -> Option<SketchSurfaceFrame> {
    if payload.get(20) != Some(&1)
        || View::u32_le_at(payload, 21) != Some(2)
        || View::u32_le_at(payload, 25) != Some(13)
        || payload.get(29..42) != Some(b"EntityGenesis")
        || View::u32_le_at(payload, 42) != Some(23)
        || payload.get(46..69) != Some(b"IntrinsicMetaTypeuint64")
        || View::u32_le_at(payload, 77) != Some(11)
        || payload.get(81..92) != Some(b"surface_tag")
        || View::u32_le_at(payload, 92) != Some(23)
        || payload.get(96..119) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    let entity_genesis = View::u64_le_at(payload, 69);
    let persistent_id = std::num::NonZeroU64::new(View::u64_le_at(payload, 119)?)?;
    let point_count = usize::try_from(View::u32_le_at(payload, 127)?).ok()?;
    if point_count == 0 || point_count > 100_000 {
        return None;
    }
    let coordinate_count = point_count.checked_mul(3)?;
    let coordinate_bytes = point_count.checked_mul(24)?;
    payload.get(131..131usize.checked_add(coordinate_bytes)?)?;
    let degrees_at = 131usize.checked_add(coordinate_bytes)?;
    let u_degree = View::u32_le_at(payload, degrees_at)?;
    let v_degree = View::u32_le_at(payload, degrees_at.checked_add(4)?)?;
    let u_knot_count =
        usize::try_from(View::u32_le_at(payload, degrees_at.checked_add(8)?)?).ok()?;
    let u_knots_at = degrees_at.checked_add(12)?;
    payload.get(u_knots_at..u_knots_at.checked_add(u_knot_count.checked_mul(8)?)?)?;
    let v_count_at = u_knots_at.checked_add(u_knot_count.checked_mul(8)?)?;
    let v_knot_count = usize::try_from(View::u32_le_at(payload, v_count_at)?).ok()?;
    let v_knots_at = v_count_at.checked_add(4)?;
    payload.get(v_knots_at..v_knots_at.checked_add(v_knot_count.checked_mul(8)?)?)?;
    let grid_at = v_knots_at.checked_add(v_knot_count.checked_mul(8)?)?;
    let u_count = usize::try_from(View::u32_le_at(payload, grid_at)?).ok()?;
    let v_count = usize::try_from(View::u32_le_at(payload, grid_at.checked_add(4)?)?).ok()?;
    // The grid count determines safe row framing. Geometry admission follows
    // after source coordinates are scaled to millimetres.
    if u_count.checked_mul(v_count) != Some(point_count) {
        return None;
    }
    Some(SketchSurfaceFrame {
        entity_genesis,
        persistent_id,
        u_degree,
        v_degree,
        u_knots_at,
        u_knot_count,
        v_knots_at,
        v_knot_count,
        coordinate_count,
        u_count,
        v_count,
    })
}

fn charged_sketch_scalar_values(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    offset: usize,
    count: usize,
    operation: &'static str,
) -> Result<Option<Vec<f64>>, CodecError> {
    let Some(byte_len) = count.checked_mul(8) else {
        return Ok(None);
    };
    let Some(end) = offset.checked_add(byte_len) else {
        return Ok(None);
    };
    if payload.get(offset..end).is_none() {
        return Ok(None);
    }

    let mut values = Vec::new();
    ctx.reserve_vec(&mut values, count, operation)?;
    for index in 0..count {
        let at = offset + index * 8;
        let value = View::f64_le_at(payload, at)
            .ok_or_else(|| CodecError::Malformed("F3D sketch scalar range changed".into()))?;
        values.push(value);
    }
    Ok(Some(values))
}

fn parse_sketch_surface(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    record_at: usize,
) -> Result<Option<ParsedSketchSurface>, CodecError> {
    let Some(frame) = parse_sketch_surface_frame(payload) else {
        return Ok(None);
    };
    let Some(coordinates) = charged_sketch_scalar_values(
        ctx,
        payload,
        131,
        frame.coordinate_count,
        "f3d sketch surface scalar values",
    )?
    else {
        return Ok(None);
    };
    let Some(u_knots) = charged_sketch_scalar_values(
        ctx,
        payload,
        frame.u_knots_at,
        frame.u_knot_count,
        "f3d sketch surface scalar values",
    )?
    else {
        return Ok(None);
    };
    let Some(v_knots) = charged_sketch_scalar_values(
        ctx,
        payload,
        frame.v_knots_at,
        frame.v_knot_count,
        "f3d sketch surface scalar values",
    )?
    else {
        return Ok(None);
    };
    let point_count = frame.coordinate_count / 3;

    let mut points = Vec::new();
    ctx.reserve_vec(&mut points, point_count, "f3d sketch surface scaled points")?;
    for (ordinal, values) in coordinates.chunks_exact(3).enumerate() {
        let Some(source) = FinitePoint3::new(Point3::new(values[0], values[1], values[2])) else {
            return Ok(None);
        };
        let point = scaled_sketch_point(source).ok_or_else(|| {
            crate::design::text::malformed_design(ctx, format_args!(
                "F3D sketch surface at byte {record_at} control point {ordinal} overflows millimetres"
            ))
        })?;
        points.push(point);
    }

    let mut control_points = Vec::new();
    ctx.reserve_vec(
        &mut control_points,
        frame.u_count,
        "f3d sketch surface rows",
    )?;
    for row in points.chunks(frame.v_count) {
        let mut row_points = Vec::new();
        ctx.reserve_vec(
            &mut row_points,
            frame.v_count,
            "f3d sketch surface row points",
        )?;
        row_points.extend_from_slice(row);
        control_points.push(row_points);
    }
    let geometry = match SketchSurfaceGeometry::from_checked_parts(
        RecordAdmission::Charged(ctx),
        frame.u_degree,
        frame.v_degree,
        u_knots,
        v_knots,
        control_points,
    ) {
        Ok(geometry) => geometry,
        Err(SketchGeometryError::Invalid(_)) => return Ok(None),
        Err(SketchGeometryError::Resource(error)) => return Err(error),
    };
    Ok(Some(ParsedSketchSurface {
        entity_genesis: frame.entity_genesis,
        persistent_id: frame.persistent_id,
        geometry,
    }))
}

/// Decode tensor-product surface entities owned by spatial Design sketches.
pub(crate) fn decode_sketch_surfaces(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<SketchSurface>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut at = 0usize;
        while let Some(record_at) = next_indexed_record_offset(bytes, at) {
            at = record_at + 1;
            let Some((class_tag, after_tag)) =
                lp_ascii_filtered_view(bytes, record_at, 0..=2000, u8::is_ascii_graphic)
            else {
                continue;
            };
            let Some(record_index) = View::u32_le_at(bytes, after_tag) else {
                continue;
            };
            let payload = &bytes[record_at..];
            let Some(surface) = parse_sketch_surface(ctx, payload, record_at)? else {
                continue;
            };

            ctx.reserve_vec(&mut out, 1, "f3d sketch surface output")?;
            out.push(SketchSurface {
                id: ids::native_sketch_surface_id(&entry.name, record_at),
                record_index,
                owner_reference: None,
                class_tag: crate::design::decode::text::class_tag_from_view(class_tag)
                    .map_err(CodecError::Malformed)?,
                byte_offset: u64_from_index(record_at),
                entity_genesis: surface.entity_genesis,
                persistent_id: surface.persistent_id,
                geometry: surface.geometry,
            });
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |a, b| a.id.cmp(&b.id),
        |value| value.id.as_str().len(),
        "sort f3d design sketch 5",
    )?;
    Ok(out)
}

/// Bind relation-connected sketch geometry to its unique owning sketch.
pub(crate) fn bind_sketch_graph(
    ctx: &DecodeContext<'_>,
    entities: &[DesignEntityHeader],
    points: &mut [SketchPoint],
    curves: &mut [SketchCurveIdentity],
    surfaces: &mut [SketchSurface],
    relations: &mut [SketchRelation],
) -> Result<(), CodecError> {
    let mut sketch_owners = HashMap::new();
    for entity in entities.iter().filter(|entity| entity.in_sketch_module()) {
        let (Some(scope), Ok(suffix)) = (
            native_stream(&entity.id),
            u32::try_from(entity.entity_id.suffix()),
        ) else {
            continue;
        };
        if !sketch_owners.contains_key(&(scope, suffix)) {
            ctx.reserve_map(&mut sketch_owners, 1, "f3d sketch graph owner key")?;
        }
        sketch_owners.insert((scope, suffix), entity.entity_id.as_str());
    }

    let mut scoped_relations = Vec::new();
    ctx.reserve_vec(
        &mut scoped_relations,
        relations.len(),
        "f3d sketch graph scoped relations",
    )?;
    for relation in relations.iter_mut() {
        let scope = native_stream(&relation.id).ok_or_else(|| {
            crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "Fusion sketch relation {} has no Design stream identity",
                    relation.record_index
                ),
            )
        })?;
        let owner = sketch_owners
            .get(&(scope, relation.owner_reference))
            .ok_or_else(|| {
                crate::design::text::malformed_design(
                    ctx,
                    format_args!(
                        "Fusion sketch relation {} in {scope} has no owning Design entity {}",
                        relation.record_index, relation.owner_reference,
                    ),
                )
            })?;

        let mut owner_text = ctx.retained_string(owner.len(), "f3d sketch relation owner text")?;
        owner_text.push_str(owner);
        relation.owner_entity_id = Some(
            cadmpeg_core::text::NonBlankString::new(owner_text).ok_or_else(|| {
                crate::design::text::malformed_design(
                    ctx,
                    format_args!(
                        "Fusion sketch relation {} has an empty owner_entity_id",
                        relation.record_index,
                    ),
                )
            })?,
        );
        scoped_relations.push((
            scope,
            relation.owner_reference,
            relation.members(),
            relation.return_members(),
        ));
    }
    let typed_record_keys = points
        .iter()
        .filter_map(|point| Some((native_stream(&point.id)?, point.record_index)))
        .chain(
            curves
                .iter()
                .filter_map(|curve| Some((native_stream(&curve.id)?, curve.record_index))),
        )
        .chain(
            surfaces
                .iter()
                .filter_map(|surface| Some((native_stream(&surface.id)?, surface.record_index))),
        );
    let mut typed_records = std::collections::HashSet::new();
    for key in typed_record_keys {
        if !typed_records.contains(&key) {
            ctx.reserve_set(&mut typed_records, 1, "f3d sketch graph typed record")?;
            typed_records.insert(key);
        }
    }
    let mut owners = std::collections::HashMap::new();
    let direct_owners = points
        .iter()
        .map(|point| (&point.id, point.record_index, point.owner_reference))
        .chain(
            curves
                .iter()
                .map(|curve| (&curve.id, curve.record_index, curve.owner_reference)),
        )
        .chain(
            surfaces
                .iter()
                .map(|surface| (&surface.id, surface.record_index, surface.owner_reference)),
        )
        .filter_map(|(id, record_index, owner_reference)| {
            Some((native_stream(id)?, record_index, owner_reference?))
        });
    for (scope, record_index, owner_reference) in direct_owners {
        if let Some((owner_scope, _)) = sketch_owners
            .keys()
            .find(|(owner_scope, owner)| *owner_scope == scope && *owner == owner_reference)
        {
            ctx.insert_hash_map(
                &mut owners,
                (*owner_scope, record_index),
                owner_reference,
                "f3d sketch graph record owner",
            )?;
        }
    }
    for &(scope, owner_reference, members, returned) in &scoped_relations {
        for record_index in members
            .iter()
            .map(|member| member.reference.record_index())
            .chain(
                returned
                    .iter()
                    .map(|member| member.reference.record_index()),
            )
        {
            if !typed_records.contains(&(scope, record_index)) {
                continue;
            }
            if ctx
                .insert_hash_map(
                    &mut owners,
                    (scope, record_index),
                    owner_reference,
                    "f3d sketch graph record owner",
                )?
                .is_some_and(|owner| owner != owner_reference)
            {
                return Err(crate::design::text::malformed_design(
                    ctx,
                    format_args!(
                    "Fusion sketch record {record_index} in {scope} belongs to multiple sketches"
                ),
                ));
            }
        }
    }
    // A direct backlink, a typed relation, and the sketch container's counted
    // member run are the three independent ownership joins. Payload-internal
    // references in an otherwise unowned Geometry record do not name a Sketch.
    // Apply the container join after relations and require all joins to agree.
    for entity in entities.iter().filter(|entity| entity.in_sketch_module()) {
        let (Some(scope), Ok(suffix)) = (
            native_stream(&entity.id),
            u32::try_from(entity.entity_id.suffix()),
        ) else {
            continue;
        };
        for record_index in entity.member_values() {
            if !typed_records.contains(&(scope, *record_index)) {
                continue;
            }
            if ctx
                .insert_hash_map(
                    &mut owners,
                    (scope, *record_index),
                    suffix,
                    "f3d sketch graph record owner",
                )?
                .is_some_and(|owner| owner != suffix)
            {
                return Err(crate::design::text::malformed_design(
                    ctx,
                    format_args!(
                    "Fusion sketch record {record_index} in {scope} belongs to multiple sketches"
                ),
                ));
            }
        }
    }
    for point in points.iter_mut() {
        point.owner_reference = native_stream(&point.id)
            .and_then(|scope| owners.get(&(scope, point.record_index)))
            .copied();
    }
    for curve in curves.iter_mut() {
        curve.owner_reference = native_stream(&curve.id)
            .and_then(|scope| owners.get(&(scope, curve.record_index)))
            .copied();
    }
    for surface in surfaces.iter_mut() {
        surface.owner_reference = native_stream(&surface.id)
            .and_then(|scope| owners.get(&(scope, surface.record_index)))
            .copied();
    }
    let operand_entries = points
        .iter()
        .filter_map(|point| {
            Some((
                (native_stream(&point.id)?, point.record_index),
                SketchRelationOperand::Point {
                    record_index: point.record_index,
                    persistent_id: point.persistent_id(),
                },
            ))
        })
        .chain(curves.iter().filter_map(|curve| {
            Some((
                (native_stream(&curve.id)?, curve.record_index),
                SketchRelationOperand::Curve {
                    record_index: curve.record_index,
                    primary_id: curve.primary_id.get(),
                    secondary_id: curve.secondary_id,
                },
            ))
        }))
        .chain(surfaces.iter().filter_map(|surface| {
            Some((
                (native_stream(&surface.id)?, surface.record_index),
                SketchRelationOperand::Surface {
                    record_index: surface.record_index,
                    persistent_id: surface.persistent_id.get(),
                },
            ))
        }));
    let mut operands = HashMap::new();
    for (key, operand) in operand_entries {
        if !operands.contains_key(&key) {
            ctx.reserve_map(&mut operands, 1, "f3d sketch graph operand key")?;
        }
        operands.insert(key, operand);
    }
    for relation in relations {
        let scope = native_stream(&relation.id).ok_or_else(|| {
            crate::design::text::malformed_design(
                ctx,
                format_args!("invalid sketch relation id {}", relation.id),
            )
        })?;
        let mut scope_reservation = ctx.reserve_scoped(0, "f3d sketch relation scope text")?;
        let mut owned_scope = String::new();
        ctx.reserve_scoped_string(
            &mut scope_reservation,
            &mut owned_scope,
            scope.len(),
            "f3d sketch relation scope text",
        )?;
        owned_scope.push_str(scope);
        let resolve = |record_index| {
            operands
                .get(&(owned_scope.as_str(), record_index))
                .cloned()
                .unwrap_or(SketchRelationOperand::Record { record_index })
        };
        relation.resolve_members(ctx, resolve)?;
    }
    Ok(())
}

fn decode_sketch_curve_identity(payload: &[u8]) -> Option<(u64, u64, usize, Option<u64>)> {
    if let Some((primary, secondary)) = decode_sketch_curve_identity_variant(payload, 0, 2) {
        return Some((primary, secondary, 0, None));
    }
    if View::u32_le_at(payload, 25) != Some(13)
        || payload.get(29..42) != Some(b"EntityGenesis")
        || View::u32_le_at(payload, 42) != Some(23)
        || payload.get(46..69) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    let entity_genesis = View::u64_le_at(payload, 69)?;
    decode_sketch_curve_identity_variant(payload, 52, 3)
        .map(|(primary, secondary)| (primary, secondary, 52, Some(entity_genesis)))
}

fn decode_sketch_curve_identity_variant(
    payload: &[u8],
    shift: usize,
    property_count: u32,
) -> Option<(u64, u64)> {
    if payload.get(20) != Some(&1)
        || View::u32_le_at(payload, 21) != Some(property_count)
        || View::u32_le_at(payload, 25 + shift) != Some(14)
        || payload.get(29 + shift..43 + shift) != Some(b"crv_primary_id")
        || View::u32_le_at(payload, 43 + shift) != Some(23)
        || payload.get(47 + shift..70 + shift) != Some(b"IntrinsicMetaTypeuint64")
        || View::u32_le_at(payload, 78 + shift) != Some(16)
        || payload.get(82 + shift..98 + shift) != Some(b"crv_secondary_id")
        || View::u32_le_at(payload, 98 + shift) != Some(23)
        || payload.get(102 + shift..125 + shift) != Some(b"IntrinsicMetaTypeuint64")
    {
        return None;
    }
    Some((
        View::u64_le_at(payload, 70 + shift)?,
        View::u64_le_at(payload, 125 + shift)?,
    ))
}

struct DecodedSketchCurveGeometry {
    geometry: SketchCurveGeometry,
    geometry_offset: usize,
}

fn decode_sketch_curve_geometry(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    geometry_shift: usize,
    record_index: u32,
    class: SketchCurveClass,
    record_at: usize,
) -> Result<Option<DecodedSketchCurveGeometry>, CodecError> {
    let Some(geometry_payload) = payload.get(geometry_shift..) else {
        return Ok(None);
    };
    let decoded = match class {
        SketchCurveClass::Line => {
            if let Some((geometry, _)) = decode_line_family(ctx, geometry_payload, record_at)? {
                Some((geometry, 133))
            } else if let Some(referenced) = referenced_analytic_payload(geometry_payload) {
                decode_line_family(ctx, referenced, record_at)?
                    .map(|(geometry, _)| (geometry, 11 + 133))
            } else {
                None
            }
        }
        SketchCurveClass::Circular => {
            if let Some(geometry) = decode_circular_arc(ctx, geometry_payload, record_at)? {
                Some((geometry, 133))
            } else if let Some(referenced) = referenced_analytic_payload(geometry_payload) {
                decode_circular_arc(ctx, referenced, record_at)?
                    .map(|geometry| (geometry, 11 + 133))
            } else {
                None
            }
        }
        SketchCurveClass::Nurbs => {
            let legacy =
                decode_legacy_sketch_nurbs(ctx, geometry_payload, record_at).transpose()?;
            let geometry = match legacy {
                Some(geometry) => Some(geometry),
                None => decode_sketch_nurbs(ctx, geometry_payload, record_at).transpose()?,
            };
            geometry.map(|(geometry, _)| (geometry, 133))
        }
        SketchCurveClass::TextFrameLine => {
            decode_text_frame_line(ctx, payload, geometry_shift, record_index, record_at)?.and_then(
                |(geometry, end)| {
                    end.checked_sub(geometry_shift + 12 * 8)
                        .map(|offset| (geometry, offset))
                },
            )
        }
    };
    Ok(
        decoded.map(|(geometry, offset)| DecodedSketchCurveGeometry {
            geometry,
            geometry_offset: geometry_shift + offset,
        }),
    )
}

fn decode_circular_arc(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    record_at: usize,
) -> Result<Option<SketchCurveGeometry>, CodecError> {
    let mut values = [0.0; 12];
    for (ordinal, value) in values.iter_mut().enumerate() {
        let Some(parsed) = View::f64_le_at(payload, 133 + ordinal * 8) else {
            return Ok(None);
        };
        *value = parsed;
    }
    let (Some(center_cm), Some(normal), Some(reference_direction), Some(radius_cm)) = (
        FinitePoint3::new(Point3::new(values[0], values[1], values[2])),
        UnitVector3::new(Vector3::new(values[3], values[4], values[5])),
        UnitVector3::new(Vector3::new(values[6], values[7], values[8])),
        PositiveLength::new(values[9]),
    ) else {
        return Ok(None);
    };
    let (Some(start_angle), Some(end_angle)) = (Angle::new(values[10]), Angle::new(values[11]))
    else {
        return Ok(None);
    };
    let center = scale_sketch_point(ctx, center_cm, record_at, "arc")?;
    let radius = PositiveLength::new(radius_cm.get() * 10.0);
    let Some(radius) = radius else {
        return Err(crate::design::text::malformed_design(
            ctx,
            format_args!("F3D sketch arc at byte {record_at} overflows millimetres"),
        ));
    };
    Ok(SketchCurveGeometry::arc_from_parts(
        center,
        normal,
        reference_direction,
        radius,
        start_angle,
        end_angle,
    )
    .ok())
}

fn scale_sketch_point(
    ctx: &DecodeContext<'_>,
    point_centimetres: FinitePoint3,
    record_at: usize,
    kind: &str,
) -> Result<FinitePoint3, CodecError> {
    scaled_sketch_point(point_centimetres).ok_or_else(|| {
        crate::design::text::malformed_design(
            ctx,
            format_args!("F3D sketch {kind} at byte {record_at} overflows millimetres"),
        )
    })
}

fn scaled_sketch_point(point_centimetres: FinitePoint3) -> Option<FinitePoint3> {
    let point = point_centimetres.get();
    FinitePoint3::new(Point3::new(point.x * 10.0, point.y * 10.0, point.z * 10.0))
}

fn referenced_analytic_payload(payload: &[u8]) -> Option<&[u8]> {
    if payload.get(133) != Some(&1) || payload.get(138..144) != Some(&[0; 6]) {
        return None;
    }
    payload.get(11..)
}

/// Decode a text-frame boundary line after its two point references and
/// inline analytic-curve record. The first point reference has a trailing
/// null-role byte in addition to its six-byte reference padding. The inline
/// record repeats the enclosing record index and carries eight zero bytes
/// before the line values.
fn decode_text_frame_line(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    geometry_shift: usize,
    record_index: u32,
    record_at: usize,
) -> Result<Option<(SketchCurveGeometry, usize)>, CodecError> {
    let values_at = (|| {
        let mut cursor = geometry_shift.checked_add(133)?;
        for zero_count in [7, 6] {
            let (_, end) = marked_u32(payload, cursor)?;
            if !payload
                .get(end..end + zero_count)?
                .iter()
                .all(|byte| *byte == 0)
            {
                return None;
            }
            cursor = end + zero_count;
        }
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(payload, cursor, 0..=2000, u8::is_ascii_graphic)?;
        if class_tag.len() != 3
            || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
            || View::u32_le_at(payload, after_tag) != Some(record_index)
            || payload.get(after_tag + 4..after_tag + 12) != Some(&[0; 8])
        {
            return None;
        }
        after_tag.checked_add(12)
    })();
    let Some(values_at) = values_at else {
        return Ok(None);
    };
    let Some(end) = values_at.checked_add(12 * 8) else {
        return Ok(None);
    };
    Ok(decode_line_values(ctx, payload, values_at, record_at)?.map(|geometry| (geometry, end)))
}

fn decode_sketch_nurbs(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    record_at: usize,
) -> Option<Result<(SketchCurveGeometry, usize), CodecError>> {
    let base = 133usize;
    let carrier = View::u64_le_at(payload, base)?;
    let carrier_reference = (carrier != u64::MAX).then_some(carrier);
    if View::u32_le_at(payload, base + 8) != Some(3) || payload.get(base + 88) != Some(&1) {
        return None;
    }
    let subtype_class_tag = std::str::from_utf8(payload.get(base + 12..base + 15)?)
        .ok()?
        .to_string();
    let subtype_class_tag =
        crate::records::references::DesignClassTag::try_from(subtype_class_tag).ok()?;
    let degree = View::u32_le_at(payload, base + 90)?;
    let fit_tolerance = View::f64_le_at(payload, base + 94)?;
    let knot_count = usize::try_from(View::u32_le_at(payload, base + 102)?).ok()?;
    if usize::try_from(View::u32_le_at(payload, base + 106)?).ok()? != knot_count
        || View::u32_le_at(payload, base + 110)? != 8
        || knot_count > 100_000
    {
        return None;
    }
    let knots = match charged_sketch_scalar_values(
        ctx,
        payload,
        base + 114,
        knot_count,
        "f3d sketch NURBS scalar values",
    ) {
        Ok(Some(values)) => values,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let weights_at = base + 114 + knot_count * 8;
    let weight_count = usize::try_from(View::u32_le_at(payload, weights_at)?).ok()?;
    if usize::try_from(View::u32_le_at(payload, weights_at + 4)?).ok()? != weight_count
        || View::u32_le_at(payload, weights_at + 8)? != 8
        || weight_count > 100_000
    {
        return None;
    }
    let weights = match charged_sketch_scalar_values(
        ctx,
        payload,
        weights_at + 12,
        weight_count,
        "f3d sketch NURBS scalar values",
    ) {
        Ok(Some(values)) => values,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let points_at = weights_at + 12 + weight_count * 8;
    let point_count = usize::try_from(View::u32_le_at(payload, points_at)?).ok()?;
    if usize::try_from(View::u32_le_at(payload, points_at + 4)?).ok()? != point_count
        || View::u32_le_at(payload, points_at + 8)? != 8
        || knot_count
            != point_count
                .checked_add(usize::try_from(degree).ok()?)?
                .checked_add(1)?
    {
        return None;
    }
    let coordinates = match charged_sketch_scalar_values(
        ctx,
        payload,
        points_at + 12,
        point_count.checked_mul(3)?,
        "f3d sketch NURBS scalar values",
    ) {
        Ok(Some(values)) => values,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let geometry = match admit_source_sketch_nurbs(
        ctx,
        degree,
        fit_tolerance,
        knots,
        weights,
        &coordinates,
        record_at,
    ) {
        Ok(Some(geometry)) => geometry,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    Some(Ok((
        SketchCurveGeometry::nurbs_from_parts(
            carrier_reference,
            subtype_class_tag,
            View::u32_le_at(payload, base + 15)?,
            geometry,
        ),
        points_at + 12 + point_count * 24,
    )))
}

fn decode_legacy_sketch_nurbs(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    record_at: usize,
) -> Option<Result<(SketchCurveGeometry, usize), CodecError>> {
    let base = 133usize;
    let carrier = View::u64_le_at(payload, base)?;
    let carrier_reference = (carrier != u64::MAX).then_some(carrier);
    if View::u32_le_at(payload, base + 8) != Some(3)
        || payload.get(base + 19..base + 27) != Some(&[0; 8])
        || payload.get(base + 27) != Some(&1)
        || payload.get(base + 32..base + 42) != Some(&[0; 10])
        || payload.get(base + 50..base + 55) != Some(&[0; 5])
        || payload.get(base + 55) != Some(&1)
        || payload.get(base + 60..base + 66) != Some(&[0; 6])
        || payload.get(base + 66) != Some(&1)
        || payload.get(base + 71..base + 77) != Some(&[0; 6])
        || payload.get(base + 77) != Some(&1)
        || payload.get(base + 80..base + 88) != Some(&[0; 8])
        || payload.get(base + 88).is_none_or(|value| *value > 1)
        || payload.get(base + 89).is_none_or(|value| *value > 1)
        || payload.get(base + 94..base + 102)
            != Some(&[0x95, 0xd6, 0x26, 0xe8, 0x0b, 0x2e, 0x11, 0x3e])
    {
        return None;
    }
    let subtype_class_tag = std::str::from_utf8(payload.get(base + 12..base + 15)?)
        .ok()?
        .to_string();
    let subtype_class_tag =
        crate::records::references::DesignClassTag::try_from(subtype_class_tag).ok()?;
    let degree = View::u32_le_at(payload, base + 90)?;
    let fit_tolerance = View::f64_le_at(payload, base + 42)?;
    let knot_count = usize::try_from(View::u32_le_at(payload, base + 102)?).ok()?;
    let knot_capacity = usize::try_from(View::u32_le_at(payload, base + 106)?).ok()?;
    if degree == 0
        || knot_capacity < knot_count
        || View::u32_le_at(payload, base + 110)? != 8
        || knot_capacity > 100_000
    {
        return None;
    }
    let knots = match charged_sketch_scalar_values(
        ctx,
        payload,
        base + 114,
        knot_count,
        "f3d legacy sketch NURBS scalar values",
    ) {
        Ok(Some(values)) => values,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let weights_at = base + 114 + knot_count * 8;
    let weight_count = usize::try_from(View::u32_le_at(payload, weights_at)?).ok()?;
    let weight_capacity = usize::try_from(View::u32_le_at(payload, weights_at + 4)?).ok()?;
    if weight_capacity < weight_count
        || View::u32_le_at(payload, weights_at + 8)? != 8
        || weight_capacity > 100_000
    {
        return None;
    }
    let weights = match charged_sketch_scalar_values(
        ctx,
        payload,
        weights_at + 12,
        weight_count,
        "f3d legacy sketch NURBS scalar values",
    ) {
        Ok(Some(values)) => values,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let points_at = weights_at + 12 + weight_count * 8;
    let point_count = usize::try_from(View::u32_le_at(payload, points_at)?).ok()?;
    let point_capacity = usize::try_from(View::u32_le_at(payload, points_at + 4)?).ok()?;
    if point_capacity < point_count
        || point_capacity > 100_000
        || View::u32_le_at(payload, points_at + 8)? != 8
        || knot_count
            != point_count
                .checked_add(usize::try_from(degree).ok()?)?
                .checked_add(1)?
    {
        return None;
    }
    let coordinates = match charged_sketch_scalar_values(
        ctx,
        payload,
        points_at + 12,
        point_count.checked_mul(3)?,
        "f3d legacy sketch NURBS scalar values",
    ) {
        Ok(Some(values)) => values,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let geometry = match admit_source_sketch_nurbs(
        ctx,
        degree,
        fit_tolerance,
        knots,
        weights,
        &coordinates,
        record_at,
    ) {
        Ok(Some(geometry)) => geometry,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    Some(Ok((
        SketchCurveGeometry::nurbs_from_parts(
            carrier_reference,
            subtype_class_tag,
            View::u32_le_at(payload, base + 15)?,
            geometry,
        ),
        points_at + 12 + point_count * 24,
    )))
}

fn admit_source_sketch_nurbs(
    ctx: &DecodeContext<'_>,
    degree: u32,
    fit_tolerance_cm: f64,
    knots: Vec<f64>,
    weights: Vec<f64>,
    coordinates: &[f64],
    record_at: usize,
) -> Result<Option<crate::records::sketch_geometry::SketchNurbsGeometry>, CodecError> {
    let Some(fit_tolerance_cm) = NonNegativeLength::new(fit_tolerance_cm) else {
        return Ok(None);
    };
    let fit_tolerance_mm = fit_tolerance_cm.get() * 10.0;
    let Some(fit_tolerance_mm) = NonNegativeLength::new(fit_tolerance_mm) else {
        return Err(crate::design::text::malformed_design(
            ctx,
            format_args!(
                "F3D sketch NURBS at byte {record_at} fit tolerance overflows millimetres"
            ),
        ));
    };
    let point_count = coordinates.len() / 3;

    let mut control_points = Vec::new();
    ctx.reserve_vec(
        &mut control_points,
        point_count,
        "f3d sketch NURBS control points",
    )?;
    for point in coordinates.chunks_exact(3) {
        let Some(source) = FinitePoint3::new(Point3::new(point[0], point[1], point[2])) else {
            return Ok(None);
        };
        control_points.push(scale_sketch_point(ctx, source, record_at, "NURBS")?);
    }
    let Ok(poles) = crate::records::sketch_geometry::SketchNurbsPoles::from_checked_points(
        control_points,
        weights,
    ) else {
        return Ok(None);
    };
    match crate::records::sketch_geometry::SketchNurbsGeometry::from_checked_parts(
        RecordAdmission::Charged(ctx),
        degree,
        fit_tolerance_mm,
        knots,
        poles,
    ) {
        Ok(geometry) => Ok(Some(geometry)),
        Err(SketchGeometryError::Invalid(_)) => Ok(None),
        Err(SketchGeometryError::Resource(error)) => Err(error),
    }
}

fn decode_line(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    record_at: usize,
) -> Result<Option<SketchCurveGeometry>, CodecError> {
    decode_line_values(ctx, payload, 133, record_at)
}

fn decode_compact_planar_line(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    record_at: usize,
) -> Result<Option<SketchCurveGeometry>, CodecError> {
    let values_at = 133;
    let mut values = [0.0; 9];
    for (ordinal, value) in values.iter_mut().enumerate() {
        let Some(parsed) = View::f64_le_at(payload, values_at + ordinal * 8) else {
            return Ok(None);
        };
        *value = parsed;
    }
    if values[2] != 0.0 || values[5] != 0.0 || values[8] != 0.0 {
        return Ok(None);
    }
    let Some((_, reference_end)) = marked_u32(payload, values_at + 9 * 8) else {
        return Ok(None);
    };
    if payload.get(reference_end..reference_end + 6) != Some(&[0; 6]) {
        return Ok(None);
    }
    decode_line_components(ctx, &values, Vector3::new(0.0, 0.0, 1.0), record_at)
}

fn decode_line_family(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    record_at: usize,
) -> Result<Option<(SketchCurveGeometry, usize)>, CodecError> {
    if let Some(geometry) = decode_line(ctx, payload, record_at)? {
        return Ok(Some((geometry, 12)));
    }
    Ok(decode_compact_planar_line(ctx, payload, record_at)?.map(|geometry| (geometry, 9)))
}

fn decode_line_values(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    values_at: usize,
    record_at: usize,
) -> Result<Option<SketchCurveGeometry>, CodecError> {
    let mut values = [0.0; 12];
    for (ordinal, value) in values.iter_mut().enumerate() {
        let Some(parsed) = View::f64_le_at(payload, values_at + ordinal * 8) else {
            return Ok(None);
        };
        *value = parsed;
    }
    let stored_normal = Vector3::new(values[9], values[10], values[11]);
    decode_line_components(ctx, &values, stored_normal, record_at)
}

fn decode_line_components(
    ctx: &DecodeContext<'_>,
    values: &[f64],
    stored_normal: Vector3,
    record_at: usize,
) -> Result<Option<SketchCurveGeometry>, CodecError> {
    let displacement = Vector3::new(values[3], values[4], values[5]);
    let (Some(_source_direction), Some(_stored_direction), Some(stored_normal)) = (
        UnitVector3::normalized(displacement),
        UnitVector3::new(Vector3::new(values[6], values[7], values[8])),
        UnitVector3::new(stored_normal),
    ) else {
        return Ok(None);
    };
    // Start plus displacement carries the bounded line and is corroborated by
    // the persistent endpoint records. Imported sketches can retain a stale
    // auxiliary unit direction, so derive the neutral tangent from the
    // admitted endpoints just as the normal is orthogonalized below.
    let Some(start_cm) = FinitePoint3::new(Point3::new(values[0], values[1], values[2])) else {
        return Ok(None);
    };
    let start = scale_sketch_point(ctx, start_cm, record_at, "line")?;
    let end = FinitePoint3::new(start.get().translated(displacement, 10.0)).ok_or_else(|| {
        crate::design::text::malformed_design(
            ctx,
            format_args!("F3D sketch line at byte {record_at} overflows millimetres"),
        )
    })?;
    let start_raw = start.get();
    let end_raw = end.get();
    let displacement_mm = FiniteVector3::new(Vector3::new(
        end_raw.x - start_raw.x,
        end_raw.y - start_raw.y,
        end_raw.z - start_raw.z,
    ))
    .ok_or_else(|| {
        crate::design::text::malformed_design(
            ctx,
            format_args!("F3D sketch line at byte {record_at} has an overflowing displacement"),
        )
    })?;
    let Some(direction) = UnitVector3::normalized(displacement_mm.get()) else {
        return Ok(None);
    };
    // The stored line normal is an auxiliary orientation vector. Imported
    // legacy sketches can retain a small component along the line direction;
    // remove that component so the typed carrier maintains its orthonormal
    // invariant without changing the line's endpoints or orientation side.
    let dot = direction.as_raw().dot(*stored_normal.as_raw());
    let projected_normal = *stored_normal.as_raw() - direction.as_raw().scale(dot);
    let projected_length = projected_normal.norm();
    let normal = if projected_length.is_finite()
        && projected_length > EPS_SKETCH_DECODE_LINE_COMPONENTS_E12
    {
        let Some(normal) = UnitVector3::normalized(projected_normal) else {
            return Ok(None);
        };
        normal
    } else {
        // Spatial line carriers can store a unit auxiliary vector parallel to
        // the line. The neutral spatial-line geometry has no plane normal;
        // retain the bounded carrier and choose a stable perpendicular basis
        // vector so the native geometry record still satisfies its typed
        // invariant.
        let basis = [
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ]
        .into_iter()
        .min_by(|left, right| {
            direction
                .as_raw()
                .dot(*left)
                .abs()
                .total_cmp(&direction.as_raw().dot(*right).abs())
        });
        let Some(basis) = basis else {
            return Ok(None);
        };
        let Some(normal) = UnitVector3::normalized(direction.as_raw().cross(basis)) else {
            return Ok(None);
        };
        normal
    };
    Ok(SketchCurveGeometry::line_from_parts(start, end, direction, normal).ok())
}

struct ParsedSketchRelationMember {
    reference: crate::records::identity::Located<u32, usize>,
    relation_ordinal: u32,
}

struct ParsedSketchRelation {
    members: Vec<ParsedSketchRelationMember>,
    auxiliary_references: Vec<crate::records::identity::Located<u32, usize>>,
    owner_reference: u32,
    owner_reference_offset: usize,
    state: u64,
    state_offset: usize,
    entity_genesis: Option<u64>,
    class_members: RelationClassMembers,
    return_members: Vec<crate::records::identity::Located<u32, usize>>,
    parsed_end: usize,
}

/// Type GUID of the shared sketch-relation class, which adds no member of its
/// own between the property block and `ParentNode`.
const RELATION_TYPE_GUID: &str = "60403D47-0C49-49B0-BDE8-1679608164A2";
/// Type GUID of the offset class, whose relations carry mask `0x2000000000`.
const OFFSET_RELATION_TYPE_GUID: &str = "D3BD153B-EB8A-405E-9D29-69EE0C3D227C";
/// Type GUID of the spline-group class, whose relations carry mask `0x80000000`.
const SPLINE_RELATION_TYPE_GUID: &str = "73762C3B-82DC-4632-93B0-B8FE1CC5282F";
/// Type GUID of the tangency class, whose relations carry mask `0x100`.
const TANGENT_RELATION_TYPE_GUID: &str = "24DB790E-3DCD-4336-AFA3-6F119EF2239B";
/// Type GUID of the circular-pattern class, mask `0x10000000`.
const CIRCULAR_PATTERN_RELATION_TYPE_GUID: &str = "8269E861-0BB7-47E0-9911-5AE3EC475058";
/// Type GUID of the rectangular-pattern class, mask `0x20000000`.
const RECTANGULAR_PATTERN_RELATION_TYPE_GUID: &str = "40800FB9-C2BE-494E-A047-7D76E82B9F6C";
/// Type GUID of the text-frame class, mask `0x10000000000`.
const TEXT_FRAME_RELATION_TYPE_GUID: &str = "8B369926-123F-4F9D-878E-6D4C076128D3";
/// Type GUID of the text-path class, mask `0x20000000000`.
const TEXT_PATH_RELATION_TYPE_GUID: &str = "9D30FCDC-EA07-4141-93E2-918B1A59E962";

/// A sketch-relation record class, named by the type GUID its segment's type
/// table carries for the record's entity. The class fixes the members written
/// between the property block and the base class's `ParentNode`. A class tag
/// cannot name it: a tag is `256` plus an index into the segment's own type
/// table, so one tag names different relation classes in different segments.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SketchRelationClass {
    /// A class whose most-derived level adds no member: the shared relation
    /// class, the offset class, and the spline-group class.
    Plain,
    /// Three `u8` flags, each `0` or `1`.
    Tangent,
    /// The angle-parameter reference, the count-parameter reference, the
    /// evaluated f64 total angle in radians, the evaluated u32 instance count,
    /// the pattern tables, and one `u8`.
    CircularPattern,
    /// Three `u8` flags, a u32-counted run of references, the pattern tables,
    /// and two direction clauses.
    RectangularPattern,
    /// Two references.
    TextFrame,
    /// The text-entity reference, a u32 character count, and that many glyph
    /// blocks. `leading_flag` is the `u8` the class writes before the
    /// reference, which it does from class version 1.
    TextPath {
        /// Whether the class version writes the leading `u8`.
        leading_flag: bool,
    },
}

impl SketchRelationClass {
    /// The relation class `type_guid` names at class version `version`, or
    /// `None` where the GUID is not a sketch-relation class.
    fn of(type_guid: &str, version: u32) -> Option<Self> {
        let matches = |known: &str| type_guid.eq_ignore_ascii_case(known);
        if matches(RELATION_TYPE_GUID)
            || matches(OFFSET_RELATION_TYPE_GUID)
            || matches(SPLINE_RELATION_TYPE_GUID)
        {
            return Some(Self::Plain);
        }
        if matches(TANGENT_RELATION_TYPE_GUID) {
            return Some(Self::Tangent);
        }
        if matches(CIRCULAR_PATTERN_RELATION_TYPE_GUID) {
            return Some(Self::CircularPattern);
        }
        if matches(RECTANGULAR_PATTERN_RELATION_TYPE_GUID) {
            return Some(Self::RectangularPattern);
        }
        if matches(TEXT_FRAME_RELATION_TYPE_GUID) {
            return Some(Self::TextFrame);
        }
        if matches(TEXT_PATH_RELATION_TYPE_GUID) {
            return Some(Self::TextPath {
                leading_flag: version >= 1,
            });
        }
        None
    }
}

/// Largest plausible counted run inside a sketch relation; a larger count is a
/// misparse rather than a record that owns that many members.
const MAX_RELATION_RUN: usize = 4096;

/// Serialized width selected by a sketch relation's leading-block member.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SketchRelationMaskWidth {
    U32,
    U64,
}

impl SketchRelationMaskWidth {
    fn from_leading_block(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::U32),
            1 => Some(Self::U64),
            _ => None,
        }
    }

    fn has_paired_member_run(self) -> bool {
        self == Self::U64
    }
}

/// Read the relation's mask width from its leading-block presence member.
pub(crate) fn relation_mask_width(record: &[u8]) -> Option<SketchRelationMaskWidth> {
    let (_, start) = lp_ascii_filtered_view(record, 15, 0..=256, u8::is_ascii_graphic)?;
    SketchRelationMaskWidth::from_leading_block(*record.get(start)?)
}

/// Take one reference member at `cursor`, returning its 32-bit target and the
/// byte offset of the target within the reference. Relation members address
/// records in the relation's own segment, so a reference whose target does not
/// fit a `u32` is a misparse.
fn take_relation_reference(
    payload: &[u8],
    cursor: &mut usize,
) -> Option<crate::records::identity::Located<u32, usize>> {
    let at = *cursor;
    let reference = take_reference(payload, cursor)?;
    Some(crate::records::identity::Located {
        value: u32::try_from(reference.target()?).ok()?,
        offset: at + 1,
    })
}

#[derive(Clone, Copy)]
enum AuxiliaryRelationReference {
    Absent,
    Present(crate::records::identity::Located<u32, usize>),
}

/// Take one reference member that the class may leave absent, recording it in
/// the auxiliary run when it is present. An absent reference is one zero byte
/// and names nothing, so it contributes no entry.
fn take_auxiliary_relation_reference(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    cursor: &mut usize,
    auxiliary_references: &mut Vec<crate::records::identity::Located<u32, usize>>,
) -> Result<Option<AuxiliaryRelationReference>, CodecError> {
    let at = *cursor;
    let Some(reference) = take_reference(payload, cursor) else {
        return Ok(None);
    };
    let Some(target) = reference.target() else {
        return Ok(Some(AuxiliaryRelationReference::Absent));
    };
    let Some(value) = u32::try_from(target).ok() else {
        return Ok(None);
    };
    let located = crate::records::identity::Located {
        value,
        offset: at + 1,
    };

    ctx.reserve_vec(
        auxiliary_references,
        1,
        "f3d sketch auxiliary relation references",
    )?;
    auxiliary_references.push(located);
    Ok(Some(AuxiliaryRelationReference::Present(located)))
}

/// Skip the two tables both pattern classes write after their own leading
/// members: a u32-counted map whose entry is a u64 key, a u32 count, and that
/// many u64 values; then a u32-counted run of u32.
fn skip_pattern_tables(payload: &[u8], cursor: &mut usize) -> Option<()> {
    let entries = usize::try_from(View::u32_le_at(payload, *cursor)?).ok()?;
    if entries > MAX_RELATION_RUN {
        return None;
    }
    *cursor += 4;
    for _ in 0..entries {
        let values = usize::try_from(View::u32_le_at(payload, *cursor + 8)?).ok()?;
        if values > MAX_RELATION_RUN {
            return None;
        }
        *cursor += 12 + values * 8;
    }
    let ordinals = usize::try_from(View::u32_le_at(payload, *cursor)?).ok()?;
    if ordinals > MAX_RELATION_RUN {
        return None;
    }
    *cursor += 4 + ordinals * 4;
    (*cursor <= payload.len()).then_some(())
}

/// What a sketch-relation subclass leaves behind after its own members.
enum RelationClassMembers {
    Plain,
    Tangent,
    CircularPattern,
    Rectangular {
        reference_count: u32,
        clauses: Option<[crate::records::identity::Located<u32, usize>; 4]>,
    },
    TextFrame,
    TextPath {
        glyph_transforms: Vec<SketchGlyphTransform>,
    },
}

/// Consume the members `class` writes between the property block and the base
/// class's `ParentNode`, advancing `cursor` past them and recording every
/// present reference among them in the auxiliary run
/// ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).
fn parse_relation_class_members(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    cursor: &mut usize,
    class: SketchRelationClass,
    auxiliary_references: &mut Vec<crate::records::identity::Located<u32, usize>>,
) -> Result<Option<RelationClassMembers>, CodecError> {
    let parsed = (|| {
        macro_rules! take {
            () => {
                match take_auxiliary_relation_reference(ctx, payload, cursor, auxiliary_references)
                {
                    Ok(reference) => reference,
                    Err(error) => return Some(Err(error)),
                }
            };
        }
        Some(Ok(match class {
            SketchRelationClass::Plain => RelationClassMembers::Plain,
            SketchRelationClass::Tangent => {
                for _ in 0..3 {
                    // A flag outside `{0, 1}` is a misparse, not a third state.
                    if !matches!(payload.get(*cursor)?, 0 | 1) {
                        return None;
                    }
                    *cursor += 1;
                }
                RelationClassMembers::Tangent
            }
            SketchRelationClass::TextFrame => {
                take!()?;
                take!()?;
                RelationClassMembers::TextFrame
            }
            SketchRelationClass::CircularPattern => {
                take!()?;
                take!()?;
                // The evaluated total angle and the evaluated instance count.
                *cursor += 12;
                skip_pattern_tables(payload, cursor)?;
                if payload.get(*cursor)? != &0 {
                    return None;
                }
                *cursor += 1;
                RelationClassMembers::CircularPattern
            }
            SketchRelationClass::RectangularPattern => {
                use AuxiliaryRelationReference::Present;

                // The three flags are not checked the way the tangency class's are:
                // the counted runs and the unit directions that follow them already
                // reject a misframed record, and the flags do not.
                *cursor += 3;
                let reference_count = View::u32_le_at(payload, *cursor)?;
                let references = usize::try_from(reference_count).ok()?;
                if references > MAX_RELATION_RUN {
                    return None;
                }
                *cursor += 4;
                for _ in 0..references {
                    take!()?;
                }
                skip_pattern_tables(payload, cursor)?;
                let mut clauses = [AuxiliaryRelationReference::Absent; 4];
                for pair in clauses.chunks_exact_mut(2) {
                    *cursor += 4;
                    pair[0] = take!()?;
                    *cursor += 32;
                    pair[1] = take!()?;
                }
                let clauses = match clauses {
                    [Present(a), Present(b), Present(c), Present(d)] => Some([a, b, c, d]),
                    _ => None,
                };
                RelationClassMembers::Rectangular {
                    reference_count,
                    clauses,
                }
            }

            SketchRelationClass::TextPath { leading_flag } => {
                if leading_flag {
                    if payload.get(*cursor)? != &1 {
                        return None;
                    }
                    *cursor += 1;
                }
                let (text_reference, transforms, end) =
                    match parse_text_glyph_run(ctx, payload, *cursor) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };

                if let Err(error) = ctx.reserve_vec(
                    auxiliary_references,
                    1,
                    "f3d sketch auxiliary relation references",
                ) {
                    return Some(Err(error));
                }
                auxiliary_references.push(crate::records::identity::Located {
                    value: text_reference,
                    offset: *cursor + 1,
                });
                *cursor = end;
                RelationClassMembers::TextPath {
                    glyph_transforms: transforms,
                }
            }
        }))
    })();
    parsed.transpose()
}

/// Parse one sketch-relation record body whose class is known
/// ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).
///
/// The payload is `u8 1`, a u32 count and that many `(reference, u32 relation
/// ordinal)` pairs, the property-block presence byte and its block, the
/// class-defined members, the `ParentNode` reference naming the owning sketch,
/// a u64 constraint mask, a u32 count and that many bare references, and one
/// zero byte. At relation base-class version 0 the leading byte is zero, the
/// pair list is absent, and the mask is a u32. Both reference runs hold the
/// same members; only the second is in semantic order.
fn parse_classed_sketch_relation(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    class: SketchRelationClass,
) -> Result<Option<ParsedSketchRelation>, CodecError> {
    let parsed = (|| {
        // The record header is the LP-ASCII class tag, the u64 entity id, and the
        // LP-ASCII record name; the member payload follows it.
        let (_, start) = lp_ascii_filtered_view(payload, 15, 0..=256, u8::is_ascii_graphic)?;
        let mask_width = SketchRelationMaskWidth::from_leading_block(*payload.get(start)?)?;
        let paired_run = mask_width.has_paired_member_run();
        let mut cursor = start + 1;
        let mut members = Vec::new();
        if paired_run {
            let member_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
            if member_count > MAX_RELATION_RUN {
                return None;
            }
            cursor += 4;

            if let Err(error) = ctx.reserve_vec(
                &mut members,
                member_count,
                "f3d sketch relation paired members",
            ) {
                return Some(Err(error));
            }
            for _ in 0..member_count {
                let reference = take_relation_reference(payload, &mut cursor)?;
                members.push(ParsedSketchRelationMember {
                    reference,
                    relation_ordinal: View::u32_le_at(payload, cursor)?,
                });
                cursor += 4;
            }
        }
        // The base class level opens with its property-block presence byte. The
        // block is `u32 count` and that many `(key, type name, value)` triples;
        // `EntityGenesis` is one such key.
        let entity_genesis = read_property_block(payload, &mut cursor)?.find("EntityGenesis");
        let mut auxiliary_references = Vec::new();
        let class_members = match parse_relation_class_members(
            ctx,
            payload,
            &mut cursor,
            class,
            &mut auxiliary_references,
        ) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let owner = take_relation_reference(payload, &mut cursor)?;
        let state_offset = cursor;
        // The constraint mask follows `ParentNode` directly. It is a u64 in the
        // paired-run form and a u32 at relation base-class version 0.
        let (state, mut cursor) = match mask_width {
            SketchRelationMaskWidth::U64 => {
                (View::u64_le_at(payload, state_offset)?, state_offset + 8)
            }
            SketchRelationMaskWidth::U32 => (
                u64::from(View::u32_le_at(payload, state_offset)?),
                state_offset + 4,
            ),
        };
        let return_count = usize::try_from(View::u32_le_at(payload, cursor)?).ok()?;
        if return_count > MAX_RELATION_RUN {
            return None;
        }
        cursor += 4;

        let mut return_members = Vec::new();
        if let Err(error) = ctx.reserve_vec(
            &mut return_members,
            return_count,
            "f3d sketch relation return members",
        ) {
            return Some(Err(error));
        }
        for _ in 0..return_count {
            return_members.push(take_relation_reference(payload, &mut cursor)?);
        }
        if payload.get(cursor) != Some(&0) {
            return None;
        }
        let parsed_end = cursor + 1;
        Some(Ok(ParsedSketchRelation {
            members,
            auxiliary_references,
            owner_reference: owner.value,
            owner_reference_offset: owner.offset,
            state,
            state_offset,
            entity_genesis,
            class_members,
            return_members,
            parsed_end,
        }))
    })();
    parsed.transpose()
}

/// Parse a text-path glyph run at `at`: the marked text-entity reference,
/// six zero bytes, a u32 character count, and that many blocks of `u32 16`
/// followed by sixteen finite f64 values forming a row-major 4×4 character
/// placement transform. Returns the text reference, the transforms in
/// character order, and the offset directly after the last block.
type TextGlyphRun = (u32, Vec<SketchGlyphTransform>, usize);

fn parse_text_glyph_run(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    at: usize,
) -> Result<Option<TextGlyphRun>, CodecError> {
    let parsed = (|| {
        let (text_reference, end) = marked_u32(payload, at)?;
        let mut view = View::over_retained(payload);
        view.seek(end)?;
        if view.take(6)? != [0u8; 6] {
            return None;
        }
        let count = usize::try_from(view.u32_le()?).ok()?;
        if !(1..=4096).contains(&count) {
            return None;
        }

        let mut transforms = Vec::new();
        if let Err(error) =
            ctx.reserve_vec(&mut transforms, count, "f3d sketch text glyph transforms")
        {
            return Some(Err(error));
        }
        for _ in 0..count {
            if view.u32_le()? != 16 {
                return None;
            }
            let mut transform = [[0.0; 4]; 4];
            for row in &mut transform {
                for cell in row {
                    *cell = view.f64_le()?;
                }
            }
            transforms.push(SketchGlyphTransform::try_from(transform).ok()?);
        }
        Some(Ok((text_reference, transforms, view.position())))
    })();
    parsed.transpose()
}

/// Validated indexed-record identity and byte offset.
pub(in crate::design::decode) struct IndexedRecordHeader {
    pub(super) offset: usize,
    pub(super) record_index: u32,
    pub(super) class_tag: crate::records::references::DesignClassTag,
}

pub(super) fn indexed_record_header_at(bytes: &[u8], at: usize) -> Option<IndexedRecordHeader> {
    if View::u32_le_at(bytes, at)? != 3 {
        return None;
    }
    let class_tag = std::str::from_utf8(bytes.get(at + 4..at + 7)?)
        .ok()?
        .to_owned()
        .try_into()
        .ok()?;
    Some(IndexedRecordHeader {
        offset: at,
        record_index: View::u32_le_at(bytes, at + 7)?,
        class_tag,
    })
}

pub(in crate::design) fn next_indexed_record_offset(
    bytes: &[u8],
    position: usize,
) -> Option<usize> {
    indexed_record_offsets(bytes.get(position..)?)
        .next()
        .map(|header| position + header.offset)
}

/// Header offsets of every indexed record whose class tag is three characters.
///
/// A class tag is `256` plus an index into the segment's own type table, so a
/// tag reaches four characters only in a segment registering more than 744
/// types. No segment registers that many.
pub(super) fn indexed_record_offsets(
    bytes: &[u8],
) -> impl Iterator<Item = IndexedRecordHeader> + '_ {
    memchr::memmem::find_iter(bytes, &[3, 0, 0, 0])
        .filter_map(|at| indexed_record_header_at(bytes, at))
}

pub(super) fn next_indexed_record_offset_with_index(
    bytes: &[u8],
    mut position: usize,
    record_index: u32,
) -> Option<usize> {
    loop {
        let offset = next_indexed_record_offset(bytes, position)?;
        if indexed_record_header_at(bytes, offset)
            .is_some_and(|header| header.record_index == record_index)
        {
            return Some(offset);
        }
        position = offset.checked_add(1)?;
    }
}

fn marked_u32(bytes: &[u8], position: usize) -> Option<(u32, usize)> {
    (bytes.get(position) == Some(&1))
        .then_some((View::u32_le_at(bytes, position + 1)?, position + 5))
}

struct SketchReferenceList {
    record_reference: crate::records::identity::Located<Option<u32>>,
    references: Vec<crate::records::identity::Located<u32>>,
    end: usize,
}

fn decode_reference_list(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    position: usize,
) -> Result<Option<SketchReferenceList>, CodecError> {
    // The eight-byte base-record slot is either a u32 record reference with a
    // zero high half or the all-ones sentinel marking a sketch with no base
    // record; the list grammar is identical in both forms.
    let parsed = (|| {
        let mut view = View::over_retained(bytes);
        view.seek(position)?;
        let low = view.u32_le()?;
        let high = view.u32_le()?;
        let record_reference = match (low, high) {
            (u32::MAX, u32::MAX) => None,
            (reference, 0) => Some(reference),
            _ => return None,
        };
        if view.u8()? != 1 {
            return None;
        }
        let declared_count = usize::try_from(view.u32_le()?).ok()?;
        Some((view, record_reference, declared_count))
    })();
    let Some((mut view, record_reference, declared_count)) = parsed else {
        return Ok(None);
    };
    let Some(required_bytes) = declared_count.checked_mul(11) else {
        return Ok(None);
    };
    let Some(remaining_bytes) = bytes.len().checked_sub(view.position()) else {
        return Ok(None);
    };
    if remaining_bytes < required_bytes {
        return Ok(None);
    }

    let mut references = Vec::new();
    ctx.reserve_vec(
        &mut references,
        declared_count,
        "f3d sketch header references",
    )?;
    loop {
        let mut probe = view;
        if probe.u8() != Some(1) {
            break;
        }
        let offset = probe.position();
        let Some(reference) = probe.u32_le() else {
            break;
        };
        if probe.take(6) != Some(&[0; 6]) {
            break;
        }
        if references.len() == declared_count {
            return Ok(None);
        }
        references.push(crate::records::identity::Located {
            value: reference,
            offset: u64_from_index(offset),
        });
        view = probe;
    }
    Ok(
        (references.len() == declared_count).then_some(SketchReferenceList {
            record_reference: crate::records::identity::Located {
                value: record_reference,
                offset: u64_from_index(position),
            },
            references,
            end: view.position(),
        }),
    )
}

fn decode_sketch_streams<T>(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    decode: impl Fn(
        &DecodeContext<'_>,
        &[u8],
        &crate::metastream::MetaStream,
        &str,
    ) -> Result<Vec<T>, CodecError>,
) -> Result<Vec<T>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(meta) = metadata_for_bulk_stream(ctx, scan, &entry.name)? else {
            continue;
        };
        ctx.append_vec(
            &mut out,
            &mut { decode(ctx, bytes, &meta, &entry.name)? },
            "f3d sketch stream output",
        )?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
