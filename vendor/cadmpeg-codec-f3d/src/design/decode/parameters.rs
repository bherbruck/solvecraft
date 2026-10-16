// SPDX-License-Identifier: Apache-2.0
//! Parse Design parameter, owner, and companion frames.

use crate::bytes::lp_utf16_bounded_charged;
use cadmpeg_core::container::ContainerRole;

use crate::bytes::lp_ascii_filtered_view;
use crate::container::ContainerScan;
use crate::design::decode::body::decode_stream;
use crate::design::decode::dimension_frames::companion_owned_interval;
use crate::design::decode::sketch::{
    native_scope_charged, next_indexed_record_offset, IndexedRecordOffsets,
};
use crate::design::decode::text::design_record_id_charged;
use crate::ids::{self, native_stream};
use crate::layout::design_parameter_legacy_287_prefix as legacy_287;
use crate::layout::design_parameter_legacy_287_tail as legacy_287_tail;
use crate::layout::design_parameter_owner_legacy_68 as legacy_owner_68;
use crate::layout::design_parameter_owner_legacy_88 as legacy_owner_88;
use crate::layout::design_parameter_owner_prefix as owner_prefix;
use crate::layout::indexed_companion_record_prefix as companion_prefix;
use crate::layout::indexed_design_record_header as indexed_header;
use crate::records::{
    decal::DesignRecordHeader,
    entity_header::DesignEntityHeader,
    feature::scope::DesignParameterScope,
    parameters::{DesignParameter, DesignParameterCompanion, DesignParameterOwner},
    recipes::ConstructionRecipe,
};
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use std::collections::{HashMap, HashSet};

macro_rules! parameter_text {
    ($ctx:expr, $payload:expr, $at:expr, $bounds:expr) => {
        match lp_utf16_bounded_charged($ctx, $payload, $at, $bounds, "f3d Design UTF-16 text") {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        }
    };
}

/// Decode every parametric construction-recipe record (`body_recipe_data`,
/// `face_recipe_data`, `bounded_face_recipe_data`, `edge_recipe_data`,
/// `vertex_recipe_data`) from each design `BulkStream` entry in `scan`.
/// `recipe_index` is assigned per `(kind, design_id)` group in stream order.
pub(crate) fn decode_recipes(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<ConstructionRecipe>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        decode_stream(ctx, bytes, &entry.name, &mut out)?;
    }
    Ok(out)
}

/// Decode every indexed parameter record in each Design `BulkStream`.
pub(crate) fn decode_parameters(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<DesignParameter>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut position = 0usize;
        let mut emitted_record_indices = HashSet::new();
        while let Some(at) = next_indexed_record_offset(bytes, position) {
            let end = next_indexed_record_offset(bytes, at + 11).unwrap_or(bytes.len());
            if let Some(parsed) = parse_design_parameter(ctx, &bytes[at..end])? {
                // The Design primary index exposes one live header for each
                // logical record index. Keep the first serialized parameter
                // frame so stale copies cannot create duplicate owner
                // bindings or duplicate neutral parameter identities.
                if emitted_record_indices.contains(&parsed.record_index) {
                    position = end;
                    continue;
                }

                ctx.reserve_set(&mut emitted_record_indices, 1, "f3d parameter record index")?;
                emitted_record_indices.insert(parsed.record_index);

                ctx.reserve_vec(&mut out, 1, "f3d decoded parameter records")?;
                out.push(locate_design_parameter(parsed, &entry.name, at)?);
                position = end;
            } else {
                position = at + 1;
            }
        }
    }
    ctx.stable_sort_by(
        &mut out[..],
        |a, b| a.id.cmp(&b.id),
        |value| value.id.as_str().len(),
        "sort f3d design parameters 1",
    )?;
    Ok(out)
}

/// Design parameter parsed in frame-relative coordinates.
pub(in crate::design) struct ParsedDesignParameter {
    class_tag: crate::records::references::DesignClassTag,
    record_index: u32,
    source_ordinal: u32,
    source_kind: String,
    owner_record_index: Option<u32>,
    family_discriminator: Option<crate::records::parameters::DesignParameterDiscriminator>,
    expression: String,
    expression_offset: FrameRelative,
    source_kind_offset: FrameRelative,
    unit: Option<ParsedParameterUnit>,
    name: String,
    name_offset: FrameRelative,
    evaluated_value: f64,
    evaluated_value_offset: FrameRelative,
}

/// Unit token parsed in frame-relative coordinates.
struct ParsedParameterUnit {
    value: String,
    offset: FrameRelative,
}

impl ParsedDesignParameter {
    /// Locate this parameter in its containing stream.
    pub(in crate::design) fn into_record(
        self,
        stream: &str,
        frame_start: u64,
    ) -> Option<DesignParameter> {
        let family_discriminator = match self.family_discriminator {
            Some(value) => Some(crate::records::identity::Located {
                value,
                offset: FrameRelative(i128::from(DESIGN_PARAMETER_DISCRIMINATOR_FRAME_OFFSET))
                    .absolute(frame_start)?,
            }),
            None => None,
        };
        let unit = match self.unit {
            Some(unit) => Some(crate::records::identity::RecordedValue {
                value: unit.value,
                offset: unit.offset.absolute(frame_start)?,
            }),
            None => None,
        };
        crate::records::parameters::DesignParameter::try_from(
            crate::records::parameters::DesignParameterDraft {
                id: ids::native_design_parameter_id(stream, frame_start),
                byte_offset: frame_start,
                class_tag: self.class_tag,
                record_index: self.record_index,
                source_ordinal: self.source_ordinal,
                source: crate::records::parameters::DesignParameterSource::new(
                    self.source_kind,
                    self.owner_record_index,
                    family_discriminator,
                )
                .ok()?,
                expression: self.expression,
                expression_offset: self.expression_offset.absolute(frame_start)?,
                source_kind_offset: self.source_kind_offset.absolute(frame_start)?,
                unit,
                name: self.name,
                name_offset: self.name_offset.absolute(frame_start)?,
                evaluated_value: self.evaluated_value,
                evaluated_value_offset: self.evaluated_value_offset.absolute(frame_start)?,
            },
        )
        .ok()
    }
}

/// Frame-relative offset of the parameter family discriminator.
const DESIGN_PARAMETER_DISCRIMINATOR_FRAME_OFFSET: u64 = 22;

/// Parse one indexed parameter frame in a Design `BulkStream` test fixture.
#[cfg(test)]
pub(in crate::design) fn parse_design_parameter_record(payload: &[u8]) -> Option<DesignParameter> {
    parse_design_parameter(&cadmpeg_test_support::service_decode_context(), payload)
        .unwrap()?
        .into_record(TEST_PARAMETER_STREAM, 0)
}

/// Design `BulkStream` name used when a test parses one isolated frame.
#[cfg(test)]
const TEST_PARAMETER_STREAM: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";

/// Locate one parsed parameter frame in its containing stream.
fn locate_design_parameter(
    parsed: ParsedDesignParameter,
    stream: &str,
    at: usize,
) -> Result<DesignParameter, CodecError> {
    let frame_start = u64::try_from(at).map_err(|_| {
        CodecError::malformed("Fusion Design parameter frame offset exceeds the addressable stream")
    })?;
    parsed.into_record(stream, frame_start).ok_or_else(|| {
        CodecError::malformed("Fusion Design parameter frame has invalid fields or offsets")
    })
}

pub(in crate::design) fn parse_design_parameter(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<ParsedDesignParameter>, CodecError> {
    let parsed = (|| {
        if View::u32_le_at(payload, 0) != Some(3) || payload.get(11..22) != Some(&[0; 11]) {
            return None;
        }
        let raw_tag = payload.get(4..7)?;
        if !raw_tag.iter().all(u8::is_ascii_graphic) {
            return None;
        }
        let class_tag = crate::records::references::DesignClassTag::try_from(
            std::str::from_utf8(raw_tag).ok()?.to_owned(),
        )
        .ok()?;
        let record_index = View::u32_le_at(payload, 7)?;
        if class_tag.as_str() == "287" {
            return match parse_legacy_287_design_parameter(ctx, payload, class_tag, record_index) {
                Ok(value) => value.map(Ok),
                Err(error) => Some(Err(error)),
            };
        }
        let compact_owned = payload.get(11..26) == Some(&[0; 15])
            && payload.get(30) == Some(&1)
            && payload.get(35..41) == Some(&[0; 6]);
        let discriminated = !compact_owned && payload.get(30) == Some(&0);
        if !compact_owned && !discriminated {
            return match parse_legacy_design_parameter(ctx, payload, class_tag, record_index) {
                Ok(value) => value.map(Ok),
                Err(error) => Some(Err(error)),
            };
        }
        let (family_discriminator, source_ordinal, owner_record_index, expression_at, trailer_len) =
            if discriminated {
                let discriminator =
                    crate::records::parameters::DesignParameterDiscriminator::try_from(
                        View::u64_le_at(payload, 22)?,
                    )
                    .ok()?;
                let owner = match payload.get(35)? {
                    0 => (None, 36, 9),
                    1 if payload.get(40..46) == Some(&[0; 6]) => {
                        (Some(View::u32_le_at(payload, 36)?), 46, 9)
                    }
                    _ => return None,
                };
                (
                    Some(discriminator),
                    View::u32_le_at(payload, 31)?,
                    owner.0,
                    owner.1,
                    owner.2,
                )
            } else if compact_owned {
                (
                    None,
                    View::u32_le_at(payload, 26)?,
                    Some(View::u32_le_at(payload, 31)?),
                    41,
                    5,
                )
            } else {
                return None;
            };
        let (expression, expression_end) = parameter_text!(ctx, payload, expression_at, 1..=256);
        let expression_trailer = payload.get(expression_end..expression_end + trailer_len)?;
        let valid_expression_trailer = if discriminated && owner_record_index.is_none() {
            expression_trailer == [0, 0, 0, 0, 0, 0, 0, 0, 1]
        } else {
            expression_trailer.iter().all(|byte| *byte == 0)
        };
        if !valid_expression_trailer {
            return None;
        }
        let prefixed_kind = if discriminated
            && owner_record_index.is_some()
            && payload.get(expression_end..expression_end + 10) == Some(&[0; 10])
        {
            match lp_utf16_bounded_charged(
                ctx,
                payload,
                expression_end + 10,
                1..=256,
                "f3d Design UTF-16 text",
            ) {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            }
        } else {
            None
        };
        let source_kind_at = if prefixed_kind.is_some() {
            expression_end + 10
        } else {
            expression_end + trailer_len
        };
        let (source_kind, source_kind_end) = match prefixed_kind {
            Some(value) => value,
            None => parameter_text!(ctx, payload, source_kind_at, 1..=256),
        };
        let first_at = source_kind_end + usize::from(discriminated) * 4;
        if discriminated && View::u32_le_at(payload, source_kind_end) != Some(0) {
            return None;
        }
        let (unit, name, name_at, name_end) = if View::u32_le_at(payload, first_at) == Some(0) {
            let name_at = first_at + 4;
            let (name, name_end) = parameter_text!(ctx, payload, name_at, 1..=256);
            (None, name, name_at, name_end)
        } else {
            let (first, first_end) = parameter_text!(ctx, payload, first_at, 1..=256);
            let second_field = match lp_utf16_bounded_charged(
                ctx,
                payload,
                first_end,
                1..=256,
                "f3d Design UTF-16 text",
            ) {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            };
            if let Some((second, second_end)) = second_field {
                (
                    Some(ParsedParameterUnit {
                        value: first,
                        offset: FrameRelative(i128::from(u64_from_index(first_at + 4))),
                    }),
                    second,
                    first_end,
                    second_end,
                )
            } else {
                (None, first, first_at, first_end)
            }
        };
        let evaluated_value = View::f64_le_at(payload, name_end)?;
        let tail = payload.get(name_end + 8..)?;
        if tail.len() != 12
            || tail[0..2] != [0, 1]
            || tail[3..].iter().any(|byte| *byte != 0)
            || !valid_design_parameter_family(family_discriminator, &source_kind, tail[2])
            || source_kind.is_empty()
        {
            return None;
        }
        Some(Ok(ParsedDesignParameter {
            class_tag,
            record_index,
            source_ordinal,
            source_kind,
            owner_record_index,
            family_discriminator,
            expression,
            expression_offset: FrameRelative(i128::from(u64_from_index(expression_at + 4))),
            source_kind_offset: FrameRelative(i128::from(u64_from_index(source_kind_at + 4))),
            unit,
            name,
            name_offset: FrameRelative(i128::from(u64_from_index(name_at + 4))),
            evaluated_value,
            evaluated_value_offset: FrameRelative(i128::from(u64_from_index(name_end))),
        }))
    })();
    parsed.transpose()
}

/// Parse the class-287 owned parameter family.
///
/// This family uses the compact-owned prefix and a class-specific `0xAF` tail.
/// Its expression is followed by one of the two fixed five-byte trailers.
fn parse_legacy_287_design_parameter(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    class_tag: crate::records::references::DesignClassTag,
    record_index: u32,
) -> Result<Option<ParsedDesignParameter>, CodecError> {
    let parsed = (|| {
        if payload.get(legacy_287::ZERO_RUN_15..legacy_287::SOURCE_ORDINAL) != Some(&[0; 15])
            || payload.get(legacy_287::OWNER_MARKER) != Some(&legacy_287::OWNER_MARKER_VALUE)
            || payload.get(legacy_287::ZERO_RUN_6..legacy_287::EXPRESSION_LENGTH) != Some(&[0; 6])
        {
            return None;
        }
        let source_ordinal = View::u32_le_at(payload, legacy_287::SOURCE_ORDINAL)?;
        let owner_record_index = View::u32_le_at(payload, legacy_287::OWNER_RECORD_INDEX)?;
        let (expression, expression_end) =
            parameter_text!(ctx, payload, legacy_287::EXPRESSION_LENGTH, 1..=256);
        let expression_trailer_end =
            expression_end.checked_add(CLASS_287_EXPRESSION_TRAILER_LEN)?;
        let expression_trailer = payload.get(expression_end..expression_trailer_end)?;
        if !matches!(expression_trailer, [0, 0, 0, 0 | 1, 0]) {
            return None;
        }
        let source_kind_at = expression_trailer_end;
        let (source_kind, source_kind_end) = parameter_text!(ctx, payload, source_kind_at, 1..=256);
        let (unit, name, name_at, name_end) =
            if View::u32_le_at(payload, source_kind_end) == Some(0) {
                let name_at = source_kind_end.checked_add(4)?;
                let (name, name_end) = parameter_text!(ctx, payload, name_at, 1..=256);
                (None, name, name_at, name_end)
            } else {
                let (unit, unit_end) = parameter_text!(ctx, payload, source_kind_end, 1..=64);
                let (name, name_end) = parameter_text!(ctx, payload, unit_end, 1..=256);
                let unit_offset = source_kind_end.checked_add(4)?;
                (
                    Some(ParsedParameterUnit {
                        value: unit,
                        offset: FrameRelative(i128::try_from(unit_offset).ok()?),
                    }),
                    name,
                    unit_end,
                    name_end,
                )
            };
        let evaluated_value = View::f64_le_at(payload, name_end)?;
        let tail_start = name_end.checked_add(8)?;
        let tail = payload.get(tail_start..)?;
        if tail.len() != legacy_287_tail::LEN
            || tail[..2] != legacy_287_tail::TAIL_PREFIX_VALUE
            || tail[legacy_287_tail::FAMILY_MARKER] != legacy_287_tail::FAMILY_MARKER_VALUE
            || tail[legacy_287_tail::ZERO_RUN_9..]
                .iter()
                .any(|byte| *byte != 0)
            || source_kind.is_empty()
        {
            return None;
        }
        Some(Ok(ParsedDesignParameter {
            class_tag,
            record_index,
            source_ordinal,
            source_kind,
            owner_record_index: Some(owner_record_index),
            family_discriminator: None,
            expression,
            expression_offset: FrameRelative(
                i128::try_from(legacy_287::EXPRESSION_LENGTH + 4).ok()?,
            ),
            source_kind_offset: FrameRelative(i128::try_from(source_kind_at.checked_add(4)?).ok()?),
            unit,
            name,
            name_offset: FrameRelative(i128::try_from(name_at.checked_add(4)?).ok()?),
            evaluated_value,
            evaluated_value_offset: FrameRelative(i128::try_from(name_end).ok()?),
        }))
    })();
    parsed.transpose()
}

const CLASS_287_EXPRESSION_TRAILER_LEN: usize = 5;

fn parse_legacy_design_parameter(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    class_tag: crate::records::references::DesignClassTag,
    record_index: u32,
) -> Result<Option<ParsedDesignParameter>, CodecError> {
    let parsed = (|| {
        if payload.get(11..25)? != [0; 14]
            || payload.get(29) != Some(&1)
            || payload.get(34..40)? != [0; 6]
        {
            return None;
        }
        let source_ordinal = View::u32_le_at(payload, 25)?;
        let owner_record_index = View::u32_le_at(payload, 30)?;
        let expression_at = 40;
        let (expression, expression_end) = parameter_text!(ctx, payload, expression_at, 1..=256);
        if payload.get(expression_end..expression_end + 5)? != [0; 5] {
            return None;
        }
        let source_kind_at = expression_end + 5;
        let (source_kind, source_kind_end) = parameter_text!(ctx, payload, source_kind_at, 1..=256);
        let unit_at = source_kind_end;
        let (unit, unit_end) = parameter_text!(ctx, payload, unit_at, 1..=64);
        let name_at = unit_end;
        let (name, name_end) = parameter_text!(ctx, payload, name_at, 1..=256);
        let evaluated_value = View::f64_le_at(payload, name_end)?;
        let tail = payload.get(name_end + 8..)?;
        if tail != [0, 1, 18, 0, 0, 0, 0, 0, 0, 0, 0, 0] || source_kind.is_empty() {
            return None;
        }
        Some(Ok(ParsedDesignParameter {
            class_tag,
            record_index,
            source_ordinal,
            source_kind,
            owner_record_index: Some(owner_record_index),
            family_discriminator: None,
            expression,
            expression_offset: FrameRelative(i128::from(u64_from_index(expression_at + 4))),
            source_kind_offset: FrameRelative(i128::from(u64_from_index(source_kind_at + 4))),
            unit: Some(ParsedParameterUnit {
                value: unit,
                offset: FrameRelative(i128::from(u64_from_index(unit_at + 4))),
            }),
            name,
            name_offset: FrameRelative(i128::from(u64_from_index(name_at + 4))),
            evaluated_value,
            evaluated_value_offset: FrameRelative(i128::from(u64_from_index(name_end))),
        }))
    })();
    parsed.transpose()
}

pub(crate) fn design_parameter_discriminator(source_kind: &str) -> u64 {
    match source_kind {
        "ScaleFactor" => 5,
        "TangencyWeight" => 6,
        _ => 0,
    }
}

/// Whether a class tag admits the legacy owner grammar without scope or scalar
/// lanes.
pub(crate) fn is_legacy_parameter_owner_68_class(class_tag: &str) -> bool {
    matches!(
        class_tag,
        "268" | "282" | "284" | "289" | "297" | "299" | "325" | "336"
    )
}

/// Whether a class tag admits the legacy owner grammar with repeated scope
/// references but without scalar or local-ordinal lanes.
pub(crate) fn is_legacy_parameter_owner_88_class(class_tag: &str) -> bool {
    matches!(class_tag, "284" | "282" | "336" | "325" | "297")
}

fn valid_design_parameter_family(
    discriminator: Option<crate::records::parameters::DesignParameterDiscriminator>,
    source_kind: &str,
    tail: u8,
) -> bool {
    use crate::records::parameters::DesignParameterDiscriminator::{
        Code0, Code3, Code4, Code5, Code6,
    };
    match tail {
        16 => {
            (discriminator == Some(Code5) && source_kind == "ScaleFactor")
                || discriminator == Some(Code6)
        }
        19 => discriminator.is_none_or(|value| {
            matches!(value, Code0 | Code3 | Code4)
                || (value == Code6 && source_kind == "TangencyWeight")
        }),
        _ => false,
    }
}

/// Decode the exact same-index-delimited owner frame for every owned Design
/// parameter.
pub(crate) fn decode_parameter_owners(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    parameters: &[DesignParameter],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignParameterOwner>, CodecError> {
    if parameters
        .iter()
        .all(|parameter| parameter.owner_record_index().is_none())
    {
        return Ok(Vec::new());
    }
    let mut headers_by_stream = HashMap::<&str, HashMap<u32, &DesignRecordHeader>>::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        if !headers_by_stream.contains_key(stream) {
            ctx.reserve_map(&mut headers_by_stream, 1, "f3d owner header stream")?;
        }
        let stream_headers = headers_by_stream.entry(stream).or_default();
        if !stream_headers.contains_key(&header.record_index) {
            ctx.reserve_map(stream_headers, 1, "f3d owner header index")?;
        }
        if stream_headers.insert(header.record_index, header).is_some() {
            return Err(crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "Fusion Design stream has duplicate primary headers for record {}",
                    header.record_index
                ),
            ));
        }
    }
    let mut streams = HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let stream = native_scope_charged(ctx, &entry.name)?;
        if streams.contains_key(&stream) {
            return Err(CodecError::Malformed(
                "F3D contains duplicate Design BulkStream identities".into(),
            ));
        }

        ctx.reserve_map(&mut streams, 1, "f3d owner stream index")?;
        streams
            .entry(stream)
            .or_insert((entry, IndexedRecordOffsets::build(ctx, bytes)?));
    }
    let mut out = Vec::new();
    for parameter in parameters {
        let Some(owner_index) = parameter.owner_record_index() else {
            continue;
        };
        let malformed = |invariant: &str| {
            crate::design::text::malformed_design(
                ctx,
                format_args!(
                    "Fusion Design parameter {} owner {} {invariant}",
                    parameter.record_index, owner_index
                ),
            )
        };
        let scope = native_stream(&parameter.id)
            .ok_or_else(|| malformed("has no Design stream identity"))?;
        let Some(header) = headers_by_stream
            .get(scope)
            .and_then(|headers| headers.get(&owner_index))
            .copied()
        else {
            // A parameter can retain a source owner reference after Fusion has
            // omitted that owner's primary frame. Keep the parameter native;
            // projection reports the unresolved binding as a loss.
            continue;
        };
        let (entry, records) = streams
            .get(scope)
            .ok_or_else(|| malformed("has no containing Design BulkStream"))?;
        let bytes = scan.entry_bytes(&entry.name)?;
        let at = usize::try_from(header.byte_offset)
            .map_err(|_| malformed("primary header offset exceeds the platform address space"))?;
        let end = records
            .frames(owner_index)
            .find_map(|(start, end)| (start == at).then_some(end))
            .ok_or_else(|| malformed("has no following same-index paired header"))?;
        let frame = bytes
            .get(at..end)
            .ok_or_else(|| malformed("frame lies outside its Design BulkStream"))?;
        let evaluated = crate::records::identity::Located {
            value: parameter.evaluated_value().get(),
            offset: parameter.evaluated_value_offset(),
        };
        let owner = parse_parameter_owner(frame)
            .or_else(|| parse_legacy_parameter_owner_68(frame, evaluated, header.byte_offset))
            .or_else(|| parse_legacy_parameter_owner_88(frame, evaluated, header.byte_offset))
            .ok_or_else(|| malformed("does not match the parameter-owner grammar"))?
            .into_record(&entry.name, header.byte_offset)
            .ok_or_else(|| malformed("has invalid owner fields or evaluated-value offset"))?;
        if owner.record_index() != owner_index
            || owner.parameter_record_index() != parameter.record_index
        {
            return Err(malformed("does not link back to its referencing parameter"));
        }

        ctx.reserve_vec(&mut out, 1, "f3d parameter owner")?;
        out.push(owner);
    }
    ctx.stable_sort_by(
        &mut out[..],
        |a, b| a.id().cmp(b.id()),
        |_| 0,
        "sort f3d design parameters 2",
    )?;
    Ok(out)
}

/// Byte offset measured from an indexed frame start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FrameRelative(pub(crate) i128);

impl FrameRelative {
    fn between(offset: u64, frame_start: u64) -> Self {
        Self(i128::from(offset) - i128::from(frame_start))
    }

    /// Convert this offset to a stream-absolute position.
    pub(super) fn absolute(self, frame_start: u64) -> Option<u64> {
        u64::try_from(i128::from(frame_start).checked_add(self.0)?).ok()
    }
}

/// Parameter owner parsed in frame-relative coordinates.
pub(in crate::design) struct ParsedParameterOwner {
    pub(super) frame_length: u64,
    pub(super) class_tag: crate::records::references::DesignClassTag,
    pub(super) record_index: u32,
    pub(super) scope_record_index: u32,
    pub(super) local_ordinal: u32,
    pub(super) evaluated_value: FiniteReal,
    pub(super) evaluated_value_offset: FrameRelative,
    pub(super) parameter_record_index: u32,
    pub(super) owned_ordinal: u32,
    variant: Option<u8>,
    pub(super) companion_record_index: u32,
}

impl ParsedParameterOwner {
    /// Locate this owner in its containing stream.
    pub(in crate::design) fn into_record(
        self,
        stream: &str,
        frame_start: u64,
    ) -> Option<DesignParameterOwner> {
        crate::records::parameters::DesignParameterOwner::from_parts(
            crate::records::parameters::DesignParameterOwnerWire {
                id: ids::native_design_parameter_owner_id(stream, frame_start),
                byte_offset: frame_start,
                frame_length: self.frame_length,
                class_tag: self.class_tag,
                record_index: self.record_index,
                scope_record_index: self.scope_record_index,
                local_ordinal: self.local_ordinal,
                evaluated_value: self.evaluated_value,
                evaluated_value_offset: self.evaluated_value_offset.absolute(frame_start)?,
                parameter_record_index: self.parameter_record_index,
                owned_ordinal: self.owned_ordinal,
                variant: self.variant,
                companion_record_index: self.companion_record_index,
            },
        )
        .ok()
    }
}

pub(in crate::design) fn parse_parameter_owner(frame: &[u8]) -> Option<ParsedParameterOwner> {
    let (class_tag, after_tag) = lp_ascii_filtered_view(frame, 0, 0..=2000, u8::is_ascii_graphic)?;
    let class_tag = crate::design::decode::text::class_tag_from_view(class_tag).ok()?;
    if after_tag != indexed_header::RECORD_INDEX
        || frame.get(owner_prefix::ZERO_RUN_8..owner_prefix::ONE_MARKER) != Some(&[0; 8])
        || frame.get(owner_prefix::ONE_MARKER..owner_prefix::SCOPE_MARKER) != Some(&[1, 1, 0, 0, 0])
        || frame.get(owner_prefix::SCOPE_MARKER) != Some(&1)
        || frame.get(owner_prefix::ZERO_RUN_6..owner_prefix::LOCAL_ORDINAL) != Some(&[0; 6])
    {
        return None;
    }
    let record_index = View::u32_le_at(frame, indexed_header::RECORD_INDEX)?;
    let scope_record_index = View::u32_le_at(frame, owner_prefix::SCOPE_RECORD_INDEX)?;

    // Parse the fixed suffix backward from the exact paired-header boundary.
    // This prevents a valid shorter prefix from being accepted as the record.
    let final_scope_marker = frame.len().checked_sub(11)?;
    let companion_marker = final_scope_marker.checked_sub(12)?;
    if frame.get(final_scope_marker) != Some(&1)
        || View::u32_le_at(frame, final_scope_marker + 1) != Some(scope_record_index)
        || frame.get(final_scope_marker + 5..frame.len()) != Some(&[0; 6])
        || frame.get(companion_marker) != Some(&1)
        || frame.get(companion_marker + 5..final_scope_marker) != Some(&[0; 7])
    {
        return None;
    }

    let without_variant = companion_marker.checked_sub(13).and_then(|at| {
        (frame.get(at) == Some(&1)
            && View::u32_le_at(frame, at + 1) == Some(scope_record_index)
            && frame.get(at + 5..companion_marker) == Some(&[0; 8]))
        .then_some((at, None))
    });
    let with_variant = companion_marker.checked_sub(14).and_then(|at| {
        let variant = *frame.get(at + 12)?;
        (frame.get(at) == Some(&1)
            && View::u32_le_at(frame, at + 1) == Some(scope_record_index)
            && frame.get(at + 5..at + 11) == Some(&[0; 6])
            && frame.get(at + 11) == Some(&1)
            && variant <= 1
            && frame.get(at + 13) == Some(&0))
        .then_some((at, Some(variant)))
    });
    let with_compact_variant = companion_marker.checked_sub(13).and_then(|at| {
        let variant = *frame.get(at + 12)?;
        (frame.get(at) == Some(&1)
            && View::u32_le_at(frame, at + 1) == Some(scope_record_index)
            && frame.get(at + 5..at + 11) == Some(&[0; 6])
            && frame.get(at + 11) == Some(&1)
            && variant <= 1)
            .then_some((at, Some(variant)))
    });
    if [without_variant, with_variant, with_compact_variant]
        .into_iter()
        .flatten()
        .count()
        != 1
    {
        return None;
    }
    let (repeated_scope_marker, variant) =
        without_variant.or(with_variant).or(with_compact_variant)?;

    let owned_ordinal_offset = repeated_scope_marker.checked_sub(8)?;
    let parameter_marker = owned_ordinal_offset.checked_sub(11)?;
    if frame.get(owned_ordinal_offset + 4..repeated_scope_marker) != Some(&[0; 4])
        || frame.get(parameter_marker) != Some(&1)
        || frame.get(parameter_marker + 5..owned_ordinal_offset) != Some(&[0; 6])
    {
        return None;
    }

    let scalar = frame.get(owner_prefix::LEN..parameter_marker)?;
    let (evaluated_value, evaluated_value_offset) = match scalar.len() {
        9 if scalar.first() == Some(&0) => (View::f64_le_at(frame, 40)?, 40),
        6 if matches!(scalar.get(..2), Some([0, 0 | 1])) => {
            (f64::from(View::u32_le_at(frame, 41)?), 41)
        }
        5 if scalar.first() == Some(&0) && variant.is_none() => {
            (f64::from(View::u32_le_at(frame, 40)?), 40)
        }
        13 if scalar.first() == Some(&1) && scalar.get(1..5) == Some(&[0; 4]) => {
            (View::f64_le_at(frame, 44)?, 44)
        }
        _ => return None,
    };
    let evaluated_value = FiniteReal::new(evaluated_value)?;
    let parameter_record_index = View::u32_le_at(frame, parameter_marker + 1)?;
    let companion_record_index = View::u32_le_at(frame, companion_marker + 1)?;
    let consecutive = |first: u32, second: u32, third: u32| {
        first.checked_add(1) == Some(second) && second.checked_add(1) == Some(third)
    };
    if !(consecutive(record_index, parameter_record_index, companion_record_index)
        || consecutive(parameter_record_index, record_index, companion_record_index)
        || consecutive(record_index, companion_record_index, parameter_record_index))
    {
        return None;
    }

    Some(ParsedParameterOwner {
        frame_length: u64::try_from(frame.len()).ok()?,
        class_tag,
        record_index,
        scope_record_index,
        local_ordinal: View::u32_le_at(frame, owner_prefix::LOCAL_ORDINAL)?,
        evaluated_value,
        evaluated_value_offset: FrameRelative(evaluated_value_offset),
        parameter_record_index,
        owned_ordinal: View::u32_le_at(frame, owned_ordinal_offset)?,
        variant,
        companion_record_index,
    })
}

/// Parse the legacy owner envelope whose scope and scalar lanes are absent.
///
/// The class admission is intentional. A short frame is not enough to select
/// this grammar because older class tags also occur on modern owner records.
fn parse_legacy_parameter_owner_68(
    frame: &[u8],
    evaluated: crate::records::identity::Located<f64>,
    frame_start: u64,
) -> Option<ParsedParameterOwner> {
    let (class_tag, after_tag) = lp_ascii_filtered_view(frame, 0, 0..=2000, u8::is_ascii_graphic)?;
    if !is_legacy_parameter_owner_68_class(class_tag)
        || frame.len() != legacy_owner_68::LEN
        || after_tag != indexed_header::RECORD_INDEX
        || frame.get(legacy_owner_68::ZERO_RUN_8..legacy_owner_68::FIRST_MARKER) != Some(&[0; 8])
        || frame.get(legacy_owner_68::FIRST_MARKER) != Some(&1)
        || frame.get(legacy_owner_68::ZERO_RUN_13..legacy_owner_68::PARAMETER_MARKER)
            != Some(&[0; 13])
        || frame.get(legacy_owner_68::PARAMETER_MARKER) != Some(&1)
        || frame.get(legacy_owner_68::ZERO_RUN_6..legacy_owner_68::OWNED_ORDINAL) != Some(&[0; 6])
        || frame.get(legacy_owner_68::ZERO_RUN_7..legacy_owner_68::COMPANION_MARKER)
            != Some(&[0; 7])
        || frame.get(legacy_owner_68::COMPANION_MARKER) != Some(&1)
        || frame.get(legacy_owner_68::ZERO_RUN_8_TAIL..legacy_owner_68::LEN) != Some(&[0; 8])
    {
        return None;
    }
    let record_index = View::u32_le_at(frame, indexed_header::RECORD_INDEX)?;
    let parameter_record_index = View::u32_le_at(frame, legacy_owner_68::PARAMETER_RECORD_INDEX)?;
    let companion_record_index = View::u32_le_at(frame, legacy_owner_68::COMPANION_RECORD_INDEX)?;
    let consecutive = |first: u32, second: u32, third: u32| {
        first.checked_add(1) == Some(second) && second.checked_add(1) == Some(third)
    };
    let evaluated_value = FiniteReal::new(evaluated.value)?;
    if !consecutive(record_index, parameter_record_index, companion_record_index) {
        return None;
    }
    Some(ParsedParameterOwner {
        frame_length: u64::try_from(legacy_owner_68::LEN).ok()?,
        class_tag: crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
        record_index,
        scope_record_index: 0,
        local_ordinal: 0,
        evaluated_value,
        evaluated_value_offset: FrameRelative::between(evaluated.offset, frame_start),
        parameter_record_index,
        owned_ordinal: View::u32_le_at(frame, legacy_owner_68::OWNED_ORDINAL)?,
        variant: None,
        companion_record_index,
    })
}

/// Parse the legacy owner envelope whose scope is repeated in the suffix but
/// whose scalar and local-ordinal lanes are absent.
fn parse_legacy_parameter_owner_88(
    frame: &[u8],
    evaluated: crate::records::identity::Located<f64>,
    frame_start: u64,
) -> Option<ParsedParameterOwner> {
    let (class_tag, after_tag) = lp_ascii_filtered_view(frame, 0, 0..=2000, u8::is_ascii_graphic)?;
    if !is_legacy_parameter_owner_88_class(class_tag)
        || frame.len() != legacy_owner_88::LEN
        || after_tag != indexed_header::RECORD_INDEX
        || frame.get(legacy_owner_88::ZERO_RUN_8..legacy_owner_88::FIRST_MARKER) != Some(&[0; 8])
        || frame.get(legacy_owner_88::FIRST_MARKER) != Some(&1)
        || frame.get(legacy_owner_88::ZERO_RUN_13..legacy_owner_88::PARAMETER_MARKER)
            != Some(&[0; 13])
        || frame.get(legacy_owner_88::PARAMETER_MARKER) != Some(&1)
        || frame.get(legacy_owner_88::ZERO_RUN_6..legacy_owner_88::OWNED_ORDINAL) != Some(&[0; 6])
        || frame.get(legacy_owner_88::ZERO_RUN_4..legacy_owner_88::SCOPE_MARKER) != Some(&[0; 4])
        || frame.get(legacy_owner_88::SCOPE_MARKER) != Some(&1)
        || frame.get(legacy_owner_88::ZERO_RUN_8_BETWEEN_SCOPES..legacy_owner_88::COMPANION_MARKER)
            != Some(&[0; 8])
        || frame.get(legacy_owner_88::COMPANION_MARKER) != Some(&1)
        || frame.get(legacy_owner_88::ZERO_RUN_7..legacy_owner_88::REPEATED_SCOPE_MARKER)
            != Some(&[0; 7])
        || frame.get(legacy_owner_88::REPEATED_SCOPE_MARKER) != Some(&1)
        || frame.get(legacy_owner_88::ZERO_RUN_6_TAIL..legacy_owner_88::LEN) != Some(&[0; 6])
    {
        return None;
    }
    let record_index = View::u32_le_at(frame, indexed_header::RECORD_INDEX)?;
    let parameter_record_index = View::u32_le_at(frame, legacy_owner_88::PARAMETER_RECORD_INDEX)?;
    let scope_record_index = View::u32_le_at(frame, legacy_owner_88::SCOPE_RECORD_INDEX)?;
    if scope_record_index == 0
        || View::u32_le_at(frame, legacy_owner_88::REPEATED_SCOPE_RECORD_INDEX)?
            != scope_record_index
    {
        return None;
    }
    let companion_record_index = View::u32_le_at(frame, legacy_owner_88::COMPANION_RECORD_INDEX)?;
    let consecutive = |first: u32, second: u32, third: u32| {
        first.checked_add(1) == Some(second) && second.checked_add(1) == Some(third)
    };
    let evaluated_value = FiniteReal::new(evaluated.value)?;
    if !consecutive(record_index, parameter_record_index, companion_record_index) {
        return None;
    }
    Some(ParsedParameterOwner {
        frame_length: u64::try_from(legacy_owner_88::LEN).ok()?,
        class_tag: crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
        record_index,
        scope_record_index,
        local_ordinal: 0,
        evaluated_value,
        evaluated_value_offset: FrameRelative::between(evaluated.offset, frame_start),
        parameter_record_index,
        owned_ordinal: View::u32_le_at(frame, legacy_owner_88::OWNED_ORDINAL)?,
        variant: None,
        companion_record_index,
    })
}

/// Decode the fixed prefix of every indexed record paired with a parameter
/// owner. Record-specific payload after the prefix is decoded independently.
pub(crate) fn decode_parameter_companions(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    owners: &[DesignParameterOwner],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignParameterCompanion>, CodecError> {
    let mut headers_by_record = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        if !headers_by_record.contains_key(&key) {
            ctx.reserve_map(&mut headers_by_record, 1, "f3d parameter companion headers")?;
        }
        headers_by_record.insert(key, header);
    }
    let mut out = Vec::new();
    for owner in owners {
        let Some(scope) = native_stream(owner.id()) else {
            continue;
        };
        let Some(header) = headers_by_record.get(&(scope, owner.companion_record_index())) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, scope)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let at = usize::try_from(header.byte_offset).ok();
        let prefix = at.and_then(|at| at.checked_add(58).and_then(|end| bytes.get(at..end)));
        let Some(parsed) = prefix.and_then(parse_parameter_companion) else {
            continue;
        };
        if parsed.record_index != owner.companion_record_index()
            || parsed.owner_record_index != owner.record_index()
        {
            continue;
        }
        let Some(companion) = parsed.into_record(ctx, &entry.name, header.byte_offset)? else {
            continue;
        };

        ctx.reserve_vec(&mut out, 1, "f3d parameter companions")?;
        out.push(companion);
    }
    ctx.stable_sort_by(
        &mut out[..],
        |a, b| a.id().cmp(b.id()),
        |_| 0,
        "sort f3d design parameters 3",
    )?;
    Ok(out)
}

/// Parameter companion prefix parsed in frame-relative coordinates.
struct ParsedParameterCompanion {
    class_tag: crate::records::references::DesignClassTag,
    record_index: u32,
    owner_record_index: u32,
    timestamp_micros: std::num::NonZeroU64,
    timestamp_micros_offset: FrameRelative,
}

impl ParsedParameterCompanion {
    /// Locate this companion prefix in its containing stream. The owned payload
    /// extent and recipes are bound afterward by
    /// `bind_parameter_companion_payloads`.
    fn into_record(
        self,
        ctx: &DecodeContext<'_>,
        stream: &str,
        frame_start: u64,
    ) -> Result<Option<DesignParameterCompanion>, CodecError> {
        let Some(timestamp_offset) = self.timestamp_micros_offset.absolute(frame_start) else {
            return Ok(None);
        };
        let id = design_record_id_charged(
            ctx,
            stream,
            ":design-parameter-companion#",
            frame_start,
            "f3d parameter companion identifier",
        )?;
        Ok(Some(DesignParameterCompanion::unbound(
            id,
            frame_start,
            self.class_tag,
            self.record_index,
            self.owner_record_index,
            self.timestamp_micros,
            timestamp_offset,
        )))
    }
}

fn parse_parameter_companion(prefix: &[u8]) -> Option<ParsedParameterCompanion> {
    let (class_tag, after_tag) = lp_ascii_filtered_view(prefix, 0, 0..=2000, u8::is_ascii_graphic)?;
    let class_tag = crate::design::decode::text::class_tag_from_view(class_tag).ok()?;
    if prefix.len() != companion_prefix::LEN
        || after_tag != indexed_header::RECORD_INDEX
        || prefix.get(companion_prefix::ZERO_RUN_20..companion_prefix::OWNER_MARKER)
            != Some(&[0; 20])
        || prefix.get(companion_prefix::OWNER_MARKER) != Some(&1)
        || prefix.get(companion_prefix::ZERO_RUN_6..companion_prefix::TIMESTAMP_MICROS)
            != Some(&[0; 6])
        || prefix.get(companion_prefix::ZERO_RUN_8..companion_prefix::LEN) != Some(&[0; 8])
    {
        return None;
    }
    let timestamp_micros =
        std::num::NonZeroU64::new(View::u64_le_at(prefix, companion_prefix::TIMESTAMP_MICROS)?)?;
    Some(ParsedParameterCompanion {
        class_tag,
        record_index: View::u32_le_at(prefix, indexed_header::RECORD_INDEX)?,
        owner_record_index: View::u32_le_at(prefix, companion_prefix::OWNER_RECORD_INDEX)?,
        timestamp_micros,
        timestamp_micros_offset: FrameRelative(i128::from(u64_from_index(
            companion_prefix::TIMESTAMP_MICROS,
        ))),
    })
}

/// Records a companion payload is resolved against.
pub(crate) struct ParameterCompanionInputs<'a, S: std::hash::BuildHasher> {
    /// Indexed parameter records.
    pub(crate) parameters: &'a [DesignParameter],
    /// Parameter owner frames.
    pub(crate) owners: &'a [DesignParameterOwner],
    /// Parameter scope records.
    pub(crate) scopes: &'a [DesignParameterScope],
    /// Design entity headers.
    pub(crate) entities: &'a [DesignEntityHeader],
    /// Indexed Design record headers.
    pub(crate) headers: &'a [DesignRecordHeader],
    /// Construction recipes.
    pub(crate) recipes: &'a [ConstructionRecipe],
    /// Byte length of each Design `BulkStream`, by scope.
    pub(crate) stream_lengths: &'a HashMap<String, usize, S>,
}

/// Bind each companion to its exact owned byte interval and the construction
/// recipes nested in that interval. A companion whose payload cannot be
/// resolved is returned unbound.
pub(crate) fn bind_parameter_companion_payloads<S: std::hash::BuildHasher>(
    ctx: &DecodeContext<'_>,
    companions: Vec<DesignParameterCompanion>,
    inputs: &ParameterCompanionInputs<'_, S>,
) -> Result<Vec<DesignParameterCompanion>, CodecError> {
    let mut bound = Vec::new();
    ctx.reserve_vec(
        &mut bound,
        companions.len(),
        "f3d bound parameter companions",
    )?;
    for companion in companions {
        bound.push(match companion_payload(ctx, &companion, inputs)? {
            Some(payload) => companion.bound(payload),
            None => companion,
        });
    }
    Ok(bound)
}

/// Resolve the byte interval and nested recipes one companion owns.
fn companion_payload<S: std::hash::BuildHasher>(
    ctx: &DecodeContext<'_>,
    companion: &DesignParameterCompanion,
    inputs: &ParameterCompanionInputs<'_, S>,
) -> Result<Option<crate::records::parameters::DesignCompanionPayload>, CodecError> {
    let ParameterCompanionInputs {
        parameters,
        owners,
        scopes,
        entities,
        headers,
        recipes,
        stream_lengths,
    } = inputs;
    let Some(stream) = native_stream(companion.id()) else {
        return Ok(None);
    };
    let Some(stream_length) = stream_lengths.get(stream).copied() else {
        return Ok(None);
    };
    let Some((start, mut end)) = companion_owned_interval(
        ctx,
        companion,
        parameters.iter(),
        owners,
        scopes,
        headers,
        stream_length,
    )?
    else {
        return Ok(None);
    };
    // Entity headers precede their owning scope record. A parameter companion
    // immediately before a new scope does not own that scope's preamble even
    // though no indexed sibling separates the two records. Bind the preamble
    // through the scope's entity identity, not by an assumed class-tag or byte
    // length.
    let Ok(preamble_limit) = u64::try_from(end) else {
        return Ok(None);
    };
    end = scopes
        .iter()
        .filter(|scope| {
            native_stream(&scope.id) == Some(stream)
                && scope.byte_offset() >= preamble_limit
                && scope.sketch_entity().is_some()
        })
        .filter_map(|scope| {
            entities
                .iter()
                .filter(|entity| {
                    native_stream(&entity.id) == Some(stream)
                        && scope.sketch_entity().is_some_and(|binding| {
                            binding.entity_id.suffix() == entity.entity_id.suffix()
                        })
                        && entity.byte_offset >= u64_from_index(start)
                        && entity.byte_offset < u64_from_index(end)
                })
                .filter_map(|entity| usize::try_from(entity.byte_offset).ok())
                .min()
        })
        .min()
        .unwrap_or(end);
    let Ok(byte_offset) = u64::try_from(start) else {
        return Ok(None);
    };
    let Ok(byte_length) = u64::try_from(end - start) else {
        return Ok(None);
    };
    let mut owned = Vec::new();
    for recipe in recipes.iter().filter(|recipe| {
        native_stream(&recipe.id) == Some(stream)
            && recipe.byte_offset >= u64_from_index(start)
            && recipe.byte_offset < u64_from_index(end)
    }) {
        ctx.reserve_vec(&mut owned, 1, "f3d companion owned recipes")?;
        owned.push(recipe);
    }
    ctx.stable_sort_by(
        &mut owned[..],
        |left, right| {
            let left_key = {
                let recipe = left;
                recipe.byte_offset
            };
            let right_key = {
                let recipe = right;
                recipe.byte_offset
            };
            left_key.cmp(&right_key)
        },
        |_| 0,
        "sort f3d design parameters 4",
    )?;
    let mut owned_ids = Vec::new();
    for recipe in owned {
        let id = String::from_utf8(ctx.copy_retained(
            recipe.id.as_bytes(),
            "f3d companion owned recipe identifier",
        )?)
        .map_err(|_| CodecError::malformed("F3D companion recipe ID must be UTF-8"))?;

        ctx.reserve_vec(&mut owned_ids, 1, "f3d companion owned recipe identifiers")?;
        owned_ids.push(id);
    }
    Ok(Some(
        crate::records::parameters::DesignCompanionPayload::new(
            byte_offset,
            byte_length,
            owned_ids,
        ),
    ))
}

#[cfg(test)]
mod tests;
